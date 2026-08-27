/**
 * Lo stato delle importazioni da una fonte esterna, e della coda che le procura.
 *
 * Spotify è una delle sorgenti, non l'unica: `desiderati.source_service` dice
 * quale, e niente qui la nomina.
 *
 * # Perché lo stato non sta dentro la finestrella, né nella pagina
 *
 * Perché la parte lunga dell'importazione comincia **dopo** che la finestrella
 * ha finito il suo lavoro. Confermato un link, il nucleo scrive i brani
 * mancanti in `desiderati` e avvia una coda che gira su un filo suo: cento
 * brani sono un'ora. Il pannello che la mostrava viveva dentro la finestrella e
 * moriva con lei — e siccome la coda **non** si ferma quando la finestrella si
 * chiude, chi chiudeva si ritrovava un'applicazione che scaricava in silenzio,
 * senza un posto al mondo in cui accorgersene. Chi riapriva l'applicazione con
 * una coda in sospeso, idem.
 *
 * Lo stato vive quanto l'applicazione — si chiede una volta e poi si
 * **ascolta**, con la stessa disciplina di `nuvola` e `arricchimento` in
 * `App.tsx`. È anche ciò che permette la cosa che serviva davvero: avviare una
 * seconda importazione mentre la prima scende, e vederle tutte e due.
 *
 * # Un'importazione è un gruppo di righe
 *
 * Non c'è nessuna tabella delle importazioni: `desiderati` porta `source_id` su
 * ogni riga e non cancella mai niente, quindi `desiderati::per_sorgente` le
 * raggruppa e l'elenco sopravvive ai riavvii senza che nessuno lo salvi.
 * L'unica cosa che vive solo qui è la scelta di **cosa mostrare**: al riavvio le
 * concluse non si ripresentano, ma una che si conclude sotto gli occhi resta
 * finché non la si chiude.
 *
 * # Perché la presentazione è di là
 *
 * `schermate/Importazioni.tsx` è la superficie; questo è lo stato. Se le due
 * cose stessero nello stesso file, montare la pagina vorrebbe dire sottoscrivere
 * gli eventi e uscirne vorrebbe dire smettere — cioè tornare alla coda che
 * scende in silenzio.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  ipc,
  testoErrore,
  type BranoScarico,
  type EsitoImport,
  type SorgenteScarico,
  type StatoScarico,
} from "../ipc";
import { t } from "../lingue";

/** Un'importazione confermata in questa sessione. */
interface Sessione {
  esito: EsitoImport;
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
  /**
   * Quale livello del lettore ha risposto: `archivio`, `file-playlist`,
   * `archive.org`, `jamendo`, `audius`. `null` per un'importazione ritrovata
   * al riavvio, di cui il rapporto non c'è più — e allora la riga **omette** le
   * tacche invece di inventare «completo».
   */
  sorgente: string | null;
  /** La frase di `nomeSorgente()` per quel livello, o `null`. */
  provenienza: string | null;
  /** Quanti brani sono stati letti dalla fonte, se lo sappiamo. */
  brani: number | null;
  /**
   * Il nome stabile della fonte: `desiderati.source_service`.
   *
   * Decide se la riga in volo mostra una scelta o «dall'elenco». Da un link di
   * un catalogo il file è quello che l'utente ha incollato e la coda non ne
   * cerca un altro: l'asimmetria fra i due percorsi **è** il disegno, e
   * appianarla inventando una confidenza sarebbe la prima bugia di
   * un'interfaccia che se n'è vietata una sola.
   */
  servizio: string;
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
  /**
   * Le importazioni concluse in sessioni passate, di cui resta il rapporto.
   *
   * Stanno **fuori** dall'elenco principale e si mostrano solo su richiesta,
   * per la stessa ragione per cui al riavvio le concluse non si ripresentano: un
   * elenco che cresce a ogni link smette di essere leggibile dopo la decima
   * playlist. Servono a una cosa sola — poter riaprire un rapporto che si
   * credeva perso.
   */
  storia: EsitoImport[];
  /** Prende in carico un'importazione appena confermata. */
  registra: (esito: EsitoImport) => void;
  /** Toglie dall'elenco un'importazione conclusa. */
  scarta: (sourceId: string) => void;
  /** Dimentica l'esito del rientro: una notizia si legge una volta. */
  dimenticaRientro: () => void;
  /** Chiede alla coda di fermarsi. */
  annulla: () => void;
  /** Fa ripartire la coda. */
  riprendi: () => void;
  /** Rimette in fila i non riusciti. */
  riprovaFalliti: () => void;
  /**
   * Accetta o rifiuta le registrazioni diverse da quella chiesta.
   *
   * Vale per quel che la coda farà da adesso: non tocca niente di già preso.
   */
  ammettiAlternative: (ammesse: boolean) => void;
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
  /** I rapporti salvati: le concluse delle sessioni passate. */
  const [storia, setStoria] = useState<EsitoImport[]>([]);

  // Cinquanta e non tutti: `desiderati` non cancella mai niente e i rapporti
  // seguono la stessa regola. Un elenco che cresce per sempre va potato prima o
  // poi, e potarlo dalla finestra non si può — quindi si chiede solo quel che si
  // mostra.
  useEffect(() => {
    let annullato = false;
    ipc
      .importRapporti(50)
      .then((r) => {
        if (!annullato) setStoria(r);
      })
      .catch(() => {
        /* Senza storia la pagina funziona lo stesso: è un'aggiunta, non una
           dipendenza. */
      });
    return () => {
      annullato = true;
    };
  }, []);

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
              t("queue.didNotStart"),
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

  const registra = useCallback((esito: EsitoImport) => {
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

  // Nella pagina il rientro può restare — è un esito, e la pagina è il posto
  // degli esiti. Nel toast no: un toast che non si chiude mai è un toast che si
  // impara a coprire.
  const dimenticaRientro = useCallback(() => setRientro(null), []);

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

  // Lo stato che torna è quello che il nucleo ha davvero scritto, non quello
  // che l'interruttore sperava: se la scrittura non riesce, l'interruttore
  // torna dov'era invece di mostrare una scelta che non è stata registrata.
  const ammettiAlternative = useCallback((ammesse: boolean) => {
    ipc
      .alternativeAmmettile(ammesse)
      .then((s) => {
        setErrore(null);
        setStato(s);
      })
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
        sorgente: sessione?.esito.source ?? null,
        provenienza:
          sessione !== undefined ? nomeSorgente(sessione.esito.source) : null,
        brani: sessione?.esito.resolved ?? null,
        servizio: sorgente.sourceService,
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
        sorgente: sessione.esito.source,
        provenienza: nomeSorgente(sessione.esito.source),
        brani: sessione.esito.resolved,
        // Qui non c'è una `SorgenteScarico` da cui leggerlo: questa
        // importazione non ha lasciato righe in coda, e il livello del lettore è
        // l'unica cosa che porta dentro la fonte. La distinzione non si vede —
        // una riga senza coda non ha una riga in volo — e si scrive lo stesso,
        // per non lasciare un campo mentito.
        servizio: fonteDelLivello(sessione.esito.source),
        quando: sessione.quando,
      });
    }

    righe.sort((a, b) => b.quando - a.quando);
    return righe;
  }, [stato, registrate, appuntate, scartate]);

  /**
   * La storia, tolte le importazioni che stanno già nell'elenco.
   *
   * Un'importazione ritrovata al riavvio con delle righe ancora in coda sta in
   * tutte e due: nell'elenco perché la coda la conosce, nella tabella dei
   * rapporti perché il rapporto è stato salvato. Mostrarla due volte darebbe
   * **due** «Riapri il rapporto» per la stessa cosa, e — visto che il rapporto
   * aperto è uno solo — premere quello in fondo aprirebbe il riquadro anche in
   * cima. La sezione dice «concluse prima di questa sessione»: quel che è ancora
   * in coda non è concluso, e non ci appartiene.
   */
  const storiaFuoriDallElenco = useMemo(() => {
    const inElenco = new Set(elenco.map((r) => r.sourceId));
    return storia.filter((r) => !inElenco.has(r.sourceId));
  }, [storia, elenco]);

  return {
    elenco,
    stato,
    brano,
    errore,
    rientro,
    storia: storiaFuoriDallElenco,
    registra,
    scarta,
    dimenticaRientro,
    annulla,
    riprendi,
    riprovaFalliti,
    ammettiAlternative,
  };
}

/**
 * Il nome del livello che ha risposto, in italiano.
 *
 * Non è decorazione: dice **quanto fidarsi** di quel che si sta per importare.
 * `archivio` vuol dire che l'elenco è completo per costruzione — è tutto quel
 * che c'è, e non c'è una pagina dopo — mentre `oembed` vuol dire che di brani
 * non ne è arrivato nessuno.
 *
 * Sta qui e non in `Importa.tsx` perché adesso la leggono in quattro — la scheda
 * d'anteprima, la riga della coda, il rapporto e la storia. Due copie della
 * stessa frase sono il modo di farle divergere alla prossima sorgente.
 */
export function nomeSorgente(sorgente: string): string {
  if (sorgente === "archivio") return t("source.archive");
  if (sorgente === "file-playlist") return t("source.playlistFile");
  if (sorgente === "archive.org") return t("source.internetArchive");
  if (sorgente === "jamendo") return t("source.jamendo");
  if (sorgente === "audius") return t("source.audius");
  return t("source.bare");
}

/**
 * Da quale fonte viene un'importazione, dedotto dal livello che ha risposto.
 *
 * Un ripiego, e si vede: la risposta vera è `desiderati.source_service`, che
 * però esiste solo se quell'importazione ha lasciato righe in coda. Quando non
 * ne ha lasciate — perché era già tutto in libreria — questa è l'unica cosa che
 * resta, e scriverla è meglio che lasciare un campo mentito.
 */
function fonteDelLivello(livello: string): string {
  switch (livello) {
    case "archive.org":
      return "internet-archive";
    case "jamendo":
      return "jamendo";
    case "audius":
      return "audius";
    case "file-playlist":
      return "file-playlist";
    default:
      return "archivio-spotify";
  }
}
