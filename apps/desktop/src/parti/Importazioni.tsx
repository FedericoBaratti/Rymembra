/**
 * L'elenco delle importazioni da Spotify, e lo stato della coda che le scarica.
 *
 * # Perché non sta dentro la finestrella
 *
 * Perché la parte lunga dell'importazione comincia **dopo** che la finestrella
 * ha finito il suo lavoro. Confermato un link, il nucleo scrive i brani
 * mancanti in `spotify_wanted` e avvia una coda che gira su un filo suo: cento
 * brani sono un'ora. Il pannello che la mostrava viveva dentro la finestrella e
 * moriva con lei — e siccome la coda **non** si ferma quando la finestrella si
 * chiude, chi chiudeva si ritrovava un'applicazione che scaricava in silenzio,
 * senza un posto al mondo in cui accorgersene. Chi riapriva l'applicazione con
 * una coda in sospeso, idem.
 *
 * Qui lo stato vive quanto l'applicazione: si chiede una volta e poi si
 * **ascolta**, con la stessa disciplina di `nuvola` e `arricchimento` in
 * `App.tsx`. È anche ciò che permette la cosa che serviva davvero: avviare una
 * seconda importazione mentre la prima scende, e vederle tutte e due.
 *
 * # Un'importazione è un gruppo di righe
 *
 * Non c'è nessuna tabella delle importazioni: `spotify_wanted` porta
 * `source_id` su ogni riga e non cancella mai niente, quindi
 * `desiderati::per_sorgente` le raggruppa e l'elenco sopravvive ai riavvii
 * senza che nessuno lo salvi. L'unica cosa che vive solo qui è la scelta di
 * **cosa mostrare**: al riavvio le concluse non si ripresentano, ma una che si
 * conclude sotto gli occhi resta finché non la si chiude.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  ipc,
  testoErrore,
  type BranoScarico,
  type EsitoSpotify,
  type SorgenteScarico,
  type StatoScarico,
} from "../ipc";
import { Icona } from "./Icone";

/** Un'importazione confermata in questa sessione. */
interface Sessione {
  esito: EsitoSpotify;
  /** Quando è stata confermata, per tenerla in cima. */
  quando: number;
}

/** Il viaggio di ritorno: i brani scesi che tornano nelle loro playlist. */
export interface Rientro {
  /** Quante voci sono tornate nelle loro playlist. */
  vociRimesse: number;
  /** Quante righe si sono chiuse perché il brano ormai in libreria c'è. */
  righeChiuse: number;
}

/** Una riga dell'elenco, da qualunque delle due parti arrivi. */
export interface RigaImportazione {
  sourceId: string;
  titolo: string;
  /** `brano`, `album`, `playlist` o `artista`. */
  genere: string;
  /** Quanti brani sono già scesi. */
  fatti: number;
  /** Quanti ne restano da scaricare. */
  attesa: number;
  falliti: number;
  introvabili: number;
  /** Quanti brani questa importazione ha messo in coda in tutto. */
  totale: number;
  /** Quanti erano già in libreria. `null` se non l'abbiamo visto succedere. */
  giaInLibreria: number | null;
  /** La playlist creata o riempita, se ce n'è una. */
  playlist: string | null;
  /** Per l'ordinamento: la più recente in cima. */
  quando: number;
}

/** Quel che il pannello riceve e i comandi che può dare. */
export interface UsoImportazioni {
  /** Le importazioni da mostrare, la più recente per prima. */
  elenco: RigaImportazione[];
  /** Come sta la coda, o `null` finché non ha risposto. */
  stato: StatoScarico | null;
  /** Il brano che sta scendendo adesso, o `null`. */
  brano: BranoScarico | null;
  /** Perché la coda non è partita, quando non parte. */
  errore: string | null;
  /**
   * Cosa ha rimesso a posto l'ultimo viaggio di ritorno, se ce n'è stato uno.
   *
   * I brani scesi non finiscono da soli nelle playlist da cui mancavano: il
   * nucleo li rimette al loro posto dopo la scansione finale, ed è l'unica
   * parte dell'importazione che avviene **dopo** che la coda ha finito. Senza
   * dirlo, chi guarda vede una playlist riempirsi da sola qualche secondo dopo
   * che tutto sembrava concluso.
   */
  rientro: Rientro | null;
  /** Prende in carico un'importazione appena confermata. */
  registra: (esito: EsitoSpotify) => void;
  /** Toglie dall'elenco un'importazione conclusa. */
  scarta: (sourceId: string) => void;
  /** Chiede alla coda di fermarsi. */
  annulla: () => void;
  /** Fa ripartire la coda. */
  riprendi: () => void;
  /** Rimette in fila i non riusciti. */
  riprovaFalliti: () => void;
}

/** Somma i quattro conteggi: quanti brani ha messo in coda un'importazione. */
function quanti(sorgente: SorgenteScarico): number {
  const c = sorgente.conteggi;
  return c.attesa + c.fatto + c.fallito + c.introvabile;
}

/**
 * Lo stato delle importazioni, per tutta la vita dell'applicazione.
 *
 * Va montato una volta sola, in `App`: due copie vorrebbero dire due
 * sottoscrizioni agli stessi eventi e due verità sullo stesso numero.
 */
export function useImportazioni(): UsoImportazioni {
  const [stato, setStato] = useState<StatoScarico | null>(null);
  const [brano, setBrano] = useState<BranoScarico | null>(null);
  const [errore, setErrore] = useState<string | null>(null);
  const [rientro, setRientro] = useState<Rientro | null>(null);
  /** Le importazioni confermate in questa sessione. */
  const [registrate, setRegistrate] = useState<Map<string, Sessione>>(
    () => new Map(),
  );
  /** Quelle che abbiamo visto in coda: restano anche quando finiscono. */
  const [appuntate, setAppuntate] = useState<Set<string>>(() => new Set());
  /** Quelle che l'utente ha tolto dall'elenco. */
  const [scartate, setScartate] = useState<Set<string>>(() => new Set());

  // Lo stato iniziale. È l'interrogazione che fa ricomparire una coda rimasta
  // in sospeso dalla sessione precedente: senza, l'elenco resterebbe vuoto
  // finché il primo brano non si conclude, cioè fino a un minuto di silenzio
  // identico a «non sta succedendo niente».
  useEffect(() => {
    let annullato = false;
    ipc
      .scaricoStato()
      .then((s) => {
        if (!annullato) setStato(s);
      })
      .catch(() => {
        /* Lo stato della coda non è un errore da mostrare: se non si legge,
           restano gli eventi. */
      });
    return () => {
      annullato = true;
    };
  }, []);

  // E poi solo eventi: interrogare a ripetizione darebbe numeri in ritardo su
  // quel che gli eventi già raccontano.
  useEffect(() => {
    const promesse = [
      listen<{ fatti: number; totale: number; sorgenti: SorgenteScarico[] }>(
        "scarico:avanzamento",
        (evento) => {
          setStato((prima) =>
            prima
              ? {
                  ...prima,
                  attiva: true,
                  fatti: evento.payload.fatti,
                  rimasti: evento.payload.totale - evento.payload.fatti,
                  sorgenti: evento.payload.sorgenti,
                }
              : prima,
          );
        },
      ),
      listen<BranoScarico>("scarico:brano", (evento) =>
        setBrano(evento.payload),
      ),
      // La coda è finita: si rilegge lo stato una volta sola, per avere i
      // conteggi definitivi senza tenere un interrogatorio acceso.
      listen("scarico:finito", () => {
        setBrano(null);
        void ipc
          .scaricoStato()
          .then(setStato)
          .catch(() => {});
      }),
      // Il viaggio di ritorno. Non è solo una notizia da mostrare: `riconcilia`
      // **chiude** delle righe di `spotify_wanted` senza passare per la coda,
      // quindi i conteggi in mano qui sono già vecchi nell'istante in cui
      // questo evento arriva. Senza la rilettura, un'importazione conclusa
      // resterebbe a mostrare dei brani «in attesa» che non esistono più.
      listen<Rientro>("scarico:riconciliato", (evento) => {
        setRientro(evento.payload);
        void ipc
          .scaricoStato()
          .then(setStato)
          .catch(() => {});
      }),
      listen<{ codice?: string; messaggio?: string }>(
        "scarico:guasto",
        (evento) =>
          setErrore(
            evento.payload.messaggio ??
              evento.payload.codice ??
              "la coda non è partita",
          ),
      ),
    ];
    return () => {
      for (const p of promesse) void p.then((stop) => stop());
    };
  }, []);

  // Chi passa dalla coda resta nell'elenco anche dopo aver finito. Senza,
  // un'importazione sparirebbe nell'istante in cui si conclude — proprio
  // mentre la stai guardando, e senza dirti com'è andata.
  useEffect(() => {
    const inCoda = (stato?.sorgenti ?? [])
      .filter((s) => s.conteggi.attesa > 0)
      .map((s) => s.sourceId);
    if (inCoda.length === 0) return;
    setAppuntate((prima) => {
      const nuove = inCoda.filter((id) => !prima.has(id));
      if (nuove.length === 0) return prima;
      const dopo = new Set(prima);
      for (const id of nuove) dopo.add(id);
      return dopo;
    });
  }, [stato]);

  const registra = useCallback((esito: EsitoSpotify) => {
    setRegistrate((prima) => {
      const dopo = new Map(prima);
      dopo.set(esito.sourceId, { esito, quando: Date.now() });
      return dopo;
    });
    // Reimportare qualcosa che si era tolto dall'elenco lo rimette: è di
    // nuovo un'importazione appena fatta, e nasconderla sarebbe rispondere
    // «niente» a un gesto esplicito.
    setScartate((prima) => {
      if (!prima.has(esito.sourceId)) return prima;
      const dopo = new Set(prima);
      dopo.delete(esito.sourceId);
      return dopo;
    });
    setErrore(null);
    // Le righe di questa importazione sono appena state scritte: lo stato in
    // mano è di un istante fa e non le contiene.
    void ipc
      .scaricoStato()
      .then(setStato)
      .catch(() => {});
  }, []);

  const scarta = useCallback((sourceId: string) => {
    setScartate((prima) => {
      const dopo = new Set(prima);
      dopo.add(sourceId);
      return dopo;
    });
  }, []);

  const annulla = useCallback(() => {
    // Non si tocca `attiva`: la coda si ferma alla fine del brano in corso, e
    // dire «ferma» prima che lo sia sarebbe la stessa bugia che
    // `onAnnullaScansione` evita in `App.tsx`.
    ipc.annullaScarico().catch((e: unknown) => setErrore(testoErrore(e)));
  }, []);

  const riprendi = useCallback(() => {
    ipc
      .scaricaDesiderati()
      .then((s) => {
        setErrore(null);
        setStato(s);
      })
      .catch((e: unknown) => setErrore(testoErrore(e)));
  }, []);

  const riprovaFalliti = useCallback(() => {
    ipc
      .riprovaFalliti()
      .then(() => ipc.scaricoStato())
      .then(setStato)
      .catch((e: unknown) => setErrore(testoErrore(e)));
  }, []);

  const elenco = useMemo(() => {
    const righe: RigaImportazione[] = [];
    const visti = new Set<string>();

    for (const sorgente of stato?.sorgenti ?? []) {
      const id = sorgente.sourceId;
      if (scartate.has(id)) continue;
      const sessione = registrate.get(id);
      // Al riavvio si ripresentano solo quelle con brani ancora in coda: il
      // resto è storia, e un elenco che cresce a ogni link importato smette di
      // essere leggibile dopo la decima playlist.
      if (sorgente.conteggi.attesa === 0 && !sessione && !appuntate.has(id)) {
        continue;
      }
      visti.add(id);
      righe.push({
        sourceId: id,
        titolo: sorgente.sourceTitle,
        genere: sorgente.sourceKind,
        fatti: sorgente.conteggi.fatto,
        attesa: sorgente.conteggi.attesa,
        falliti: sorgente.conteggi.fallito,
        introvabili: sorgente.conteggi.introvabile,
        totale: quanti(sorgente),
        giaInLibreria: sessione?.esito.matched ?? null,
        playlist: sessione?.esito.playlistName ?? null,
        quando: sessione?.quando ?? sorgente.aggiuntaMs,
      });
    }

    // Le importazioni di questa sessione che non hanno lasciato righe in
    // tabella: erano già tutte in libreria, e non c'è niente da scaricare.
    // Esistono solo qui — senza questo pezzo, confermare un link e non vedere
    // comparire niente sarebbe indistinguibile da un'importazione fallita.
    for (const [id, sessione] of registrate) {
      if (visti.has(id) || scartate.has(id)) continue;
      righe.push({
        sourceId: id,
        titolo: sessione.esito.title,
        genere: sessione.esito.kind,
        fatti: 0,
        attesa: 0,
        falliti: 0,
        introvabili: 0,
        totale: 0,
        giaInLibreria: sessione.esito.matched,
        playlist: sessione.esito.playlistName,
        quando: sessione.quando,
      });
    }

    righe.sort((a, b) => b.quando - a.quando);
    return righe;
  }, [stato, registrate, appuntate, scartate]);

  return {
    elenco,
    stato,
    brano,
    errore,
    rientro,
    registra,
    scarta,
    annulla,
    riprendi,
    riprovaFalliti,
  };
}

/** Cosa sta succedendo al brano in corso, in due parole. */
function nota(brano: BranoScarico): string {
  switch (brano.esito) {
    case "cerco":
      return "cerco su YouTube…";
    case "scarico":
      return brano.frazione === null
        ? "scarico…"
        : `${Math.round(brano.frazione * 100)}%`;
    case "fatto":
      return "fatto";
    case "introvabile":
      return "non c'è su YouTube";
    default:
      return brano.codice ?? "non riuscito";
  }
}

/** Una riga: una playlist, un album o un brano, e come sta scendendo. */
function Riga({
  riga,
  brano,
  onScarta,
}: {
  riga: RigaImportazione;
  /** Il brano in corso, se è di questa importazione. */
  brano: BranoScarico | null;
  onScarta: (sourceId: string) => void;
}) {
  const percento =
    riga.totale > 0 ? Math.round((riga.fatti / riga.totale) * 100) : 100;
  const conclusa = riga.attesa === 0;

  return (
    <li className="importazione">
      <div className="che-cosa">
        <span className="titolo" title={riga.titolo}>
          {riga.titolo}
        </span>
        <span className="conteggio">
          {riga.totale > 0 ? `${riga.fatti}/${riga.totale}` : "nulla da scaricare"}
        </span>
        {/* Solo a coda vuota: togliere dall'elenco qualcosa che sta ancora
            scendendo nasconderebbe una cosa in corso, che è il difetto che
            questo pannello esiste per togliere. */}
        {conclusa && (
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={`Togli «${riga.titolo}» dall'elenco`}
            onClick={() => onScarta(riga.sourceId)}
          >
            <Icona nome="i-x" dim={12} />
          </button>
        )}
      </div>

      {riga.totale > 0 && (
        <div
          className="barra"
          role="progressbar"
          aria-valuenow={percento}
          aria-valuemin={0}
          aria-valuemax={100}
        >
          <div className="riempimento" style={{ width: `${percento}%` }} />
        </div>
      )}

      <div className="note">
        <span>{riga.genere}</span>
        {riga.giaInLibreria !== null && riga.giaInLibreria > 0 && (
          <span>{riga.giaInLibreria} già in libreria</span>
        )}
        {riga.playlist && <span>playlist «{riga.playlist}»</span>}
        {riga.falliti > 0 && (
          <span>
            {riga.falliti} {riga.falliti === 1 ? "non riuscito" : "non riusciti"}
          </span>
        )}
        {riga.introvabili > 0 && <span>{riga.introvabili} non su YouTube</span>}
      </div>

      {brano && (
        <div className="importazione-brano">
          <span className="titolo">
            {brano.artista ? `${brano.artista} — ` : ""}
            {brano.titolo}
          </span>
          <span className="nota">{nota(brano)}</span>
        </div>
      )}
    </li>
  );
}

/**
 * Il pannello, sotto il tasto che apre la finestrella.
 *
 * Non rende niente quando non c'è niente da dire: una scheda che dichiara «zero
 * importazioni» occupa lo spazio di un'informazione senza esserlo.
 */
export function Importazioni({
  importazioni,
}: {
  importazioni: UsoImportazioni;
}) {
  const { elenco, stato, brano, errore, rientro, scarta } = importazioni;
  if (elenco.length === 0) return null;

  const inAttesa = stato?.conteggi.attesa ?? 0;
  const falliti = stato?.conteggi.fallito ?? 0;
  const attiva = stato?.attiva ?? false;

  return (
    <div className="importazioni">
      <div className="importazioni-testata">
        <strong>Importazioni</strong>
        <span className="conteggio">
          {attiva
            ? "scaricamento in corso"
            : inAttesa > 0
              ? "in pausa"
              : "concluse"}
        </span>
      </div>

      {/* yt-dlp non c'è: è l'unica cosa da dire, e va detta qui perché è il
          motivo per cui le barre non si muovono. */}
      {stato && !stato.ytdlp && (
        <div className="avviso-monco">
          Per scaricare serve <strong>yt-dlp</strong>, che non è al suo posto. I
          brani restano registrati e la coda riparte da sola appena c&apos;è.
        </div>
      )}

      {errore && <div className="errore">{errore}</div>}

      {rientro !== null && rientro.vociRimesse > 0 && (
        <p className="esito">
          {rientro.vociRimesse.toLocaleString("it")}{" "}
          {rientro.vociRimesse === 1 ? "brano è tornato" : "brani sono tornati"}{" "}
          nelle playlist da cui {rientro.vociRimesse === 1 ? "mancava" : "mancavano"}.
        </p>
      )}

      <ul className="elenco-importazioni">
        {elenco.map((riga) => (
          <Riga
            key={riga.sourceId}
            riga={riga}
            brano={
              brano && brano.sorgenteId === riga.sourceId ? brano : null
            }
            onScarta={scarta}
          />
        ))}
      </ul>

      <div className="azioni">
        {attiva ? (
          <button
            type="button"
            className="bottone btn-ghost"
            onClick={importazioni.annulla}
          >
            <Icona nome="i-pause" dim={15} />
            Ferma la coda
          </button>
        ) : (
          inAttesa > 0 && (
            <button
              type="button"
              className="bottone btn-ghost"
              onClick={importazioni.riprendi}
            >
              <Icona nome="i-play" dim={15} />
              Riprendi ({inAttesa})
            </button>
          )
        )}
        {/* Rimette in fila solo i falliti: gli introvabili resterebbero
            introvabili, e riprovarli trasformerebbe «riprova» in «rifai
            tutto». */}
        {!attiva && falliti > 0 && (
          <button
            type="button"
            className="bottone btn-ghost"
            onClick={importazioni.riprovaFalliti}
          >
            <Icona nome="i-repeat" dim={15} />
            Riprova i {falliti} non riusciti
          </button>
        )}
      </div>
    </div>
  );
}
