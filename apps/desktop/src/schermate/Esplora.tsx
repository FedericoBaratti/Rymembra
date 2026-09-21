/**
 * Esplora: la ricerca nei cataloghi liberi.
 *
 * È la metà della issue #1 che mancava davvero. «Your exe doesn't do live
 * searches and streaming», scriveva chi l'ha aperta: la seconda metà il lettore
 * la sapeva fare da tre versioni — `aether_play::Sorgente` prende dei byte, non
 * un percorso — ma non c'era nessun posto da cui cercarli.
 *
 * # Perché la ricerca parte con Invio
 *
 * Perché dall'altra parte non c'è un indice in casa. La casella della libreria
 * cerca a ogni carattere e fa bene: interroga SQLite, e costa microsecondi.
 * Questa costa da una a cinque richieste all'Internet Archive e una ad Audius —
 * misurate, fra i trecento millisecondi e i tre secondi e mezzo — contro archivi
 * pubblici che ci ospitano gratis. Una ricerca per tasto premuto è il modo di
 * farsi chiudere la porta, e la porta chiusa la pagherebbe chi arriva dopo.
 *
 * # Le due frasi che non si confondono
 *
 * «Nessun risultato» e «i cataloghi non rispondono» sono due cose diverse, e chi
 * legge la seconda al posto della prima smette di cercare credendo che quella
 * musica non esista. Il codice d'errore per distinguerle c'è
 * (`download.externalSearchFailed`), e qui sono due stati vuoti diversi.
 *
 * # Perché ogni riga porta la sua licenza e il suo link
 *
 * Non è pignoleria legale ed è la cosa che rende Aether diverso da quel che
 * l'autore della issue aveva in mente. Da qui non esce niente che non si possa
 * prendere: la pastiglia dice che cosa se ne può fare, e il rimando alla pagina
 * pubblica è, per certe Creative Commons e per i termini di Audius, una
 * condizione d'uso — non un ringraziamento.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { Icona } from "../parti/Icone";
import { Avviso } from "../parti/Avvisi";
import {
  ipc,
  type AggiuntiDalCatalogo,
  type EsitoRicerca,
  type RisultatoEsplora,
} from "../ipc";
import { t } from "../lingue";
import { durata, nomeLicenza } from "../formato";

/**
 * Che cosa dire dopo un «aggiungi tutti».
 *
 * Tre numeri e non uno: quanti sono entrati, quanti c'erano già, e se non ne è
 * entrato nessuno **perché**. La versione di prima diceva solo il primo, e
 * quando valeva zero rispondeva «c'erano già tutti» anche quando non ce n'era
 * nemmeno uno — perché nessuno era tenibile.
 */
function riassunto(conteggi: AggiuntiDalCatalogo): string {
  const pezzi: string[] = [];
  if (conteggi.aggiunti === 1) pezzi.push(t("explore.done.uno"));
  else if (conteggi.aggiunti > 1)
    pezzi.push(t("explore.done", { n: conteggi.aggiunti }));
  if (conteggi.giaPresenti > 0)
    pezzi.push(t("explore.done.gia", { n: conteggi.giaPresenti }));

  if (pezzi.length > 0) return pezzi.join(" ");
  // Niente di niente: la ragione è l'unica cosa utile da dire.
  return t("explore.done.nessuno");
}

/** Lo stato in cui si trova la pagina. */
type Fase =
  | { tipo: "ferma" }
  | { tipo: "cerca" }
  | { tipo: "esito"; esito: EsitoRicerca; frase: string }
  | { tipo: "guasto" };

/**
 * Quel che una visita a Esplora lascia dietro di sé.
 *
 * # Perché non sta dentro il componente
 *
 * Perché il componente viene smontato ogni volta che si cambia schermata, e
 * con lui sparivano i risultati **e la frase scritta**. Tornare su Esplora
 * dopo aver guardato un album voleva dire una casella vuota e una pagina
 * bianca, cioè rifare la ricerca da capo: da una a cinque richieste
 * all'Internet Archive e una ad Audius, fra i trecento millisecondi e i tre
 * secondi e mezzo, contro archivi pubblici che ci ospitano gratis. La stessa
 * ragione per cui la ricerca parte a Invio e non a ogni lettera vale qui: era
 * scritta in testa al file e contraddetta dal più banale dei gesti.
 *
 * Il nucleo, dal canto suo, i candidati non li aveva mai buttati —
 * `StatoEsplora::ultima` in `esplora.rs` — quindi «aggiungi» e «ascolta»
 * avrebbero continuato a funzionare su righe che la finestra non mostrava più.
 * Era la finestra a dimenticare, da sola.
 */
export interface StatoEsplora {
  /** Quel che c'è scritto nella casella. */
  frase: string;
  /** A che punto è la pagina. */
  fase: Fase;
}

/** Esplora com'è la prima volta che la si apre. */
export const ESPLORA_INIZIALE: StatoEsplora = {
  frase: "",
  fase: { tipo: "ferma" },
};

export function Esplora({
  stato,
  onStato,
  onErrore,
  onNotizia,
  onSuona,
  onAccoda,
  onLibreriaCambiata,
}: {
  /** Quel che la visita precedente ha lasciato. */
  stato: StatoEsplora;
  /** Lo aggiorna, e sopravvive al cambio di schermata. */
  onStato: (aggiorna: (prima: StatoEsplora) => StatoEsplora) => void;
  onErrore: (e: unknown) => void;
  onNotizia: (testo: string) => void;
  /** Mette in coda e fa partire un brano che è appena entrato in libreria. */
  onSuona: (id: number) => void;
  /** Accoda in fondo, senza interrompere quel che suona. */
  onAccoda: (id: number) => void;
  /** La libreria è cambiata: gli elenchi aperti vanno riletti. */
  onLibreriaCambiata: () => void;
}) {
  const { frase, fase } = stato;
  const setFrase = useCallback(
    (testo: string) => onStato((prima) => ({ ...prima, frase: testo })),
    [onStato],
  );
  // Prende anche una funzione, come `useState`: il codice sotto aggiorna la
  // fase a partire da quella di prima — una riga che cambia dentro un elenco
  // che non si rifà — e con il solo valore secco ognuno di quei punti avrebbe
  // dovuto chiudere sopra `fase`, cioè leggere una copia vecchia.
  const setFase = useCallback(
    (aggiorna: Fase | ((prima: Fase) => Fase)) =>
      onStato((prima) => ({
        ...prima,
        fase: typeof aggiorna === "function" ? aggiorna(prima.fase) : aggiorna,
      })),
    [onStato],
  );
  // Quali righe stanno cambiando: il tasto di quella riga si spegne, gli altri
  // no. Un unico booleano «sto lavorando» spegnerebbe tutta la lista per
  // un'aggiunta che dura un decimo di secondo.
  const [inCorso, setInCorso] = useState<ReadonlySet<string>>(new Set());
  const campo = useRef<HTMLInputElement>(null);

  /**
   * Il fuoco al campo, appena la schermata si apre.
   *
   * La `ref` c'era e non la leggeva nessuno: si arrivava su Esplora — che è
   * una schermata che esiste **per** scriverci dentro — e bisognava cliccare
   * nella casella prima di poter digitare. È l'unica schermata
   * dell'applicazione in cui la prima cosa da fare è scrivere, ed è l'unica in
   * cui l'autofocus non ruba il fuoco a nient'altro.
   */
  useEffect(() => {
    campo.current?.focus();
  }, []);

  const segna = useCallback((url: string, attivo: boolean) => {
    setInCorso((prima) => {
      const dopo = new Set(prima);
      if (attivo) dopo.add(url);
      else dopo.delete(url);
      return dopo;
    });
  }, []);

  /** Aggiorna una riga sul posto, senza rifare la ricerca. */
  const aggiorna = useCallback(
    (url: string, inLibreria: boolean) => {
      setFase((prima) =>
        prima.tipo === "esito"
          ? {
              ...prima,
              esito: {
                ...prima.esito,
                risultati: prima.esito.risultati.map((r) =>
                  r.url === url ? { ...r, inLibreria } : r,
                ),
              },
            }
          : prima,
      );
      onLibreriaCambiata();
    },
    [onLibreriaCambiata],
  );

  const cerca = useCallback(async () => {
    const testo = frase.trim();
    if (testo === "") return;
    setFase({ tipo: "cerca" });
    try {
      const esito = await ipc.esploraCerca(testo);
      setFase({ tipo: "esito", esito, frase: testo });
    } catch (e) {
      // Il guasto si mostra come stato della pagina e non come toast: un toast
      // sparisce, e quel che resta sarebbe una pagina vuota che dice «nessun
      // risultato» — cioè la frase sbagliata.
      setFase({ tipo: "guasto" });
      onErrore(e);
    }
  }, [frase, onErrore]);

  const aggiungi = useCallback(
    async (riga: RisultatoEsplora) => {
      segna(riga.url, true);
      try {
        const esito = await ipc.esploraAggiungi(riga.url);
        aggiorna(riga.url, true);
        // Un brano solo: «c'era già», non «c'erano già tutti».
        if (esito.giaPresenti > 0) onNotizia(t("explore.done.gia", { n: 1 }));
      } catch (e) {
        onErrore(e);
      } finally {
        segna(riga.url, false);
      }
    },
    [segna, aggiorna, onNotizia, onErrore],
  );

  const togli = useCallback(
    async (riga: RisultatoEsplora) => {
      segna(riga.url, true);
      try {
        await ipc.esploraTogli(riga.url);
        aggiorna(riga.url, false);
      } catch (e) {
        onErrore(e);
      } finally {
        segna(riga.url, false);
      }
    },
    [segna, aggiorna, onErrore],
  );

  /**
   * Ascolta: mette in libreria se serve, e poi suona.
   *
   * Un passo solo per chi guarda, due per il programma. Il motore suona quel
   * che sta in `tracks` — la coda viaggia per identificativo, e deve, perché è
   * quel che le permette di sopravvivere a un riavvio — quindi «ascolta» su
   * qualcosa che in libreria non c'è ancora vuol dire prima metterlo. Non è un
   * effetto collaterale nascosto: il brano compare in libreria, ed è quel che
   * chi ha premuto «ascolta» si aspetta di ritrovare domani.
   */
  const ascolta = useCallback(
    async (riga: RisultatoEsplora) => {
      segna(riga.url, true);
      try {
        if (!riga.inLibreria) {
          await ipc.esploraAggiungi(riga.url);
          aggiorna(riga.url, true);
        }
        const id = await ipc.esploraIdentificativo(riga.url);
        if (id !== null) onSuona(id);
      } catch (e) {
        onErrore(e);
      } finally {
        segna(riga.url, false);
      }
    },
    [segna, aggiorna, onSuona, onErrore],
  );

  /**
   * Accoda: come «ascolta», ma in fondo e senza interrompere niente.
   *
   * La differenza che mancava. «Ascolta» chiama `suona`, che **sostituisce**
   * la coda: premerlo su un risultato mentre si sta ascoltando altro buttava
   * via quattordici brani messi in fila a mano, senza chiederlo e senza dirlo.
   * Per un risultato di una ricerca «aggiungilo a quel che sto sentendo» è
   * almeno altrettanto probabile di «mettilo adesso», e fino a qui esisteva
   * solo il secondo.
   */
  const accoda = useCallback(
    async (riga: RisultatoEsplora) => {
      segna(riga.url, true);
      try {
        if (!riga.inLibreria) {
          await ipc.esploraAggiungi(riga.url);
          aggiorna(riga.url, true);
        }
        const id = await ipc.esploraIdentificativo(riga.url);
        if (id !== null) {
          onAccoda(id);
          onNotizia(t("explore.enqueued"));
        }
      } catch (e) {
        onErrore(e);
      } finally {
        segna(riga.url, false);
      }
    },
    [segna, aggiorna, onAccoda, onNotizia, onErrore],
  );

  /**
   * Tieni una copia: la metà che mancava alla pastiglia «Si può tenere».
   *
   * La pastiglia dichiarava un permesso e nessun tasto lo esercitava.
   * «Aggiungi» mette in libreria un riferimento — il brano suona arrivando
   * dalla rete — e su un brano che la licenza permette di copiare quella è la
   * risposta piccola: il titolo della schermata dice «ascoltala, e tienila
   * quando si può», e si poteva solo ascoltarla.
   *
   * Quel che questo tasto fa è mettere la riga in coda di prelievo. Non
   * scarica subito, e non è una scorciatoia mancata: la passata di prelievo
   * prende **tutte** le righe in attesa, e accenderla da qui vorrebbe dire far
   * partire scaricamenti rimasti in sospeso da un'importazione di un mese fa.
   * Per questo la notizia dice dove è finito il brano.
   */
  const tieni = useCallback(
    async (riga: RisultatoEsplora) => {
      segna(riga.url, true);
      try {
        const scritta = await ipc.esploraTieni(riga.url, frase.trim());
        onNotizia(t(scritta ? "explore.keep.done" : "explore.keep.gia"));
      } catch (e) {
        onErrore(e);
      } finally {
        segna(riga.url, false);
      }
    },
    [segna, frase, onNotizia, onErrore],
  );

  /**
   * Quel che nessun catalogo libero consegna finisce nella lista della spesa.
   *
   * È l'altra metà della promessa: da qui esce o musica da ascoltare, o
   * l'indicazione di dove comprarla. Fino a qui la riga «solo in negozio»
   * portava la sua pastiglia e nient'altro, cioè diceva il problema senza
   * offrire il gesto.
   */
  const nellaLista = useCallback(
    async (riga: RisultatoEsplora) => {
      segna(riga.url, true);
      try {
        const scritta = await ipc.esploraNellaLista(riga.url, frase.trim());
        onNotizia(t(scritta ? "explore.wanted.done" : "explore.wanted.gia"));
      } catch (e) {
        onErrore(e);
      } finally {
        segna(riga.url, false);
      }
    },
    [segna, frase, onNotizia, onErrore],
  );

  const aggiungiTutti = useCallback(async () => {
    try {
      const esito = await ipc.esploraAggiungiTutti();
      // Si segna quel che il nucleo dice essere in libreria, non quel che da
      // qui sembrava aggiungibile: il nucleo scarta anche righe che di qua
      // paiono buone, e marcarle comunque voleva dire un tasto «in libreria»
      // su un brano che a premerlo rispondeva con un errore.
      const dentro = new Set(esito.inLibreria);
      setFase((prima) =>
        prima.tipo === "esito"
          ? {
              ...prima,
              esito: {
                ...prima.esito,
                risultati: prima.esito.risultati.map((r) => ({
                  ...r,
                  inLibreria: dentro.has(r.url),
                })),
              },
            }
          : prima,
      );
      onLibreriaCambiata();
      onNotizia(riassunto(esito.conteggi));
    } catch (e) {
      onErrore(e);
    }
  }, [onLibreriaCambiata, onNotizia, onErrore]);

  const risultati = fase.tipo === "esito" ? fase.esito.risultati : [];
  const daAggiungere = risultati.filter(
    (r) => !r.inLibreria && r.disponibilita !== "soloAcquisto",
  ).length;

  return (
    <div className="esplora">
      <div className="esplora-barra">
        <div className="cerca">
          <Icona nome="i-search" dim={16} />
          <input
            ref={campo}
            className="campo"
            type="search"
            value={frase}
            placeholder={t("explore.placeholder")}
            aria-label={t("explore.placeholder")}
            onChange={(e) => setFrase(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void cerca();
            }}
          />
        </div>
        {fase.tipo === "cerca" ? (
          <button
            type="button"
            className="bottone btn-ghost"
            onClick={() => void ipc.esploraAnnulla()}
          >
            {t("explore.cancel")}
          </button>
        ) : (
          <button
            type="button"
            className="bottone primario btn-accent"
            disabled={frase.trim() === ""}
            onClick={() => void cerca()}
          >
            {t("explore.search")}
          </button>
        )}
        {daAggiungere > 1 && (
          <button
            type="button"
            className="bottone btn-ghost"
            onClick={() => void aggiungiTutti()}
          >
            {t("explore.addAll")}
          </button>
        )}
      </div>

      {fase.tipo === "ferma" && (
        <div className="vuoto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona nome="i-search" dim={30} />
          </span>
          <h2>{t("explore.start.title")}</h2>
          <p>{t("explore.start.sub")}</p>
          <p className="nota">{t("explore.start.note")}</p>
        </div>
      )}

      {fase.tipo === "cerca" && (
        <p className="nota" role="status">
          {t("explore.searching")}
        </p>
      )}

      {fase.tipo === "guasto" && (
        <div className="vuoto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona nome="i-alert" dim={30} />
          </span>
          <h2>{t("explore.down.title")}</h2>
          <p>{t("explore.down.sub")}</p>
        </div>
      )}

      {/* Senza risultati, i due casi si escludono a vicenda, e per un pezzo
          di tempo non lo facevano: annullando una ricerca prima che trovasse
          qualcosa comparivano insieme «Nessun risultato» e «Ricerca
          interrotta», che è esattamente la confusione contro cui mette in
          guardia la carta in testa a questo file. Interrotta vince: «non ho
          trovato niente» su una ricerca che non è finita non è vero. */}
      {fase.tipo === "esito" && risultati.length === 0 && (
        <div className="vuoto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona
              nome={fase.esito.annullata ? "i-x" : "i-search"}
              dim={30}
            />
          </span>
          <h2>
            {fase.esito.annullata
              ? t("explore.cancelled")
              : t("explore.empty.title")}
          </h2>
          <p>
            {fase.esito.annullata
              ? t("explore.cancelled.sub")
              : t("explore.empty.sub")}
          </p>
        </div>
      )}

      {/* Con dei risultati invece convivono, e devono: l'elenco c'è ed è
          parziale, e la nota dice perché. */}
      {fase.tipo === "esito" &&
        fase.esito.annullata &&
        risultati.length > 0 && (
          <Avviso livello="nota">{t("explore.cancelled")}</Avviso>
        )}

      {/* Il catalogo che non ha risposto mentre l'altro rispondeva. Senza
          questa riga l'elenco sembrava completo: chi cerca un brano che sta
          solo su Audius, con Audius giù, vedeva i risultati dell'Archive e
          concludeva che quel brano non esiste. */}
      {fase.tipo === "esito" && fase.esito.muti.length > 0 && (
        <Avviso livello="avviso">
          {t("explore.partial", { fonti: fase.esito.muti.join(", ") })}
        </Avviso>
      )}

      {risultati.length > 0 && (
        <>
          <p className="nota">
            {risultati.length === 1
              ? t("explore.results.uno")
              : t("explore.results", { n: risultati.length })}
          </p>
          <ul className="elenco-esplora">
            {risultati.map((riga) => {
              const occupata = inCorso.has(riga.url);
              const siPuoAscoltare = riga.disponibilita !== "soloAcquisto";
              return (
                <li key={riga.url} className="list-row">
                  <span className="titolo">
                    {riga.autore !== null && riga.autore !== ""
                      ? `${riga.autore} — `
                      : ""}
                    {riga.titolo}
                  </span>
                  {/* Album, durata, fonte e pastiglie stanno insieme in un
                      involucro che di norma **non esiste**: `display: contents`
                      li lascia figli diretti della riga, allineati in colonna
                      come prima. Serve quando la riga si stringe — la terza
                      colonna aperta lascia al contenuto seicento pixel — e
                      allora l'involucro riappare e li porta tutti su una
                      seconda riga sotto il titolo.

                      Senza, era il titolo a cedere: è l'unico elemento
                      elastico fra sei che non lo sono, quindi si restringeva
                      lui fino a zero e di un risultato si leggeva il nome del
                      concerto e non quello del brano. Lasciarlo elastico e
                      basta non bastava: nessuna larghezza minima salva un
                      titolo quando le colonne fisse da sole sfondano la
                      riga. */}
                  <span className="dati-esplora">
                    {riga.album !== null && riga.album !== "" && (
                      <span className="genere">{riga.album}</span>
                    )}
                    {riga.durataSec !== null && (
                      <span className="durata">
                        {durata(riga.durataSec * 1000)}
                      </span>
                    )}
                    <span className="provenienza">{riga.fonteEtichetta}</span>
                    <span
                      className="pastiglia licenza"
                      data-livello="nota"
                      title={nomeLicenza(riga.licenza)}
                    >
                      {nomeLicenza(riga.licenza)}
                    </span>
                    {riga.disponibilita === "scaricabile" ? (
                      <span
                        className="pastiglia tenibile"
                        data-livello="esito"
                        title={t("explore.keep.title")}
                      >
                        {t("explore.keep")}
                      </span>
                    ) : siPuoAscoltare ? (
                      <span
                        className="pastiglia flusso"
                        data-livello="nota"
                        title={t("explore.stream.title")}
                      >
                        {t("explore.stream")}
                      </span>
                    ) : (
                      <span
                        className="pastiglia solo-negozio"
                        data-livello="avviso"
                      >
                        {t("explore.notPermitted")}
                      </span>
                    )}
                  </span>
                  <span className="azioni">
                    {/* Il rimando alla pagina pubblica sta su ogni riga e non
                        dietro un menu: per certe licenze è una condizione
                        d'uso, e una condizione d'uso non si nasconde in un
                        sottomenu. */}
                    {riga.pagina !== null && (
                      <button
                        type="button"
                        className="bottone minuto btn-ghost"
                        title={t("explore.page")}
                        aria-label={t("explore.page")}
                        onClick={() => void ipc.esploraApriPagina(riga.url)}
                      >
                        <Icona nome="i-external" dim={14} />
                      </button>
                    )}
                    {/* Quel che non si può ascoltare si può almeno
                        annotare: è la seconda metà di quel che Aether
                        promette — o la musica, o dove comprarla.

                        Oggi questo ramo non si accende, e conviene saperlo
                        prima di andarlo a cercare: `Disponibilita::decidi`
                        risponde `soloAcquisto` solo per le fonti che non
                        consegnano — l'archivio Spotify, un file di playlist —
                        e da una ricerca nei cataloghi non ne esce nessuna.
                        Resta perché la disponibilità ha tre valori e la
                        finestra deve saperli disegnare tutti e tre: il giorno
                        in cui un catalogo che nomina e basta entra
                        nell'elenco, la riga sa già cosa offrire. */}
                    {!siPuoAscoltare && (
                      <button
                        type="button"
                        className="bottone minuto btn-ghost"
                        disabled={occupata}
                        onClick={() => void nellaLista(riga)}
                      >
                        {t("explore.wanted")}
                      </button>
                    )}
                    {siPuoAscoltare && (
                      <button
                        type="button"
                        className="bottone minuto btn-ghost"
                        disabled={occupata}
                        title={t("explore.play")}
                        aria-label={t("explore.play")}
                        onClick={() => void ascolta(riga)}
                      >
                        <Icona nome="i-play" dim={14} />
                      </button>
                    )}
                    {/* Accanto ad «ascolta», che sostituisce la coda: questo
                        la allunga. Due gesti diversi meritano due tasti, e il
                        secondo mancava — premere «ascolta» su un risultato
                        buttava via la coda che si stava ascoltando. */}
                    {siPuoAscoltare && (
                      <button
                        type="button"
                        className="bottone minuto btn-ghost"
                        disabled={occupata}
                        title={t("action.enqueue")}
                        aria-label={t("action.enqueue")}
                        onClick={() => void accoda(riga)}
                      >
                        <Icona nome="i-queue" dim={14} />
                      </button>
                    )}
                    {/* «Si può tenere» adesso si può tenere. Solo dove la
                        licenza lo dice: sugli altri il tasto non c'è, invece
                        di esserci e rispondere di no. */}
                    {riga.disponibilita === "scaricabile" && (
                      <button
                        type="button"
                        className="bottone minuto btn-ghost"
                        disabled={occupata}
                        title={t("explore.keep.hint")}
                        onClick={() => void tieni(riga)}
                      >
                        {t("explore.keep.action")}
                      </button>
                    )}
                    {siPuoAscoltare &&
                      (riga.inLibreria ? (
                        <button
                          type="button"
                          className="bottone minuto btn-ghost"
                          disabled={occupata}
                          onClick={() => void togli(riga)}
                        >
                          {t("explore.remove")}
                        </button>
                      ) : (
                        <button
                          type="button"
                          className="bottone minuto btn-ghost"
                          disabled={occupata}
                          onClick={() => void aggiungi(riga)}
                        >
                          {t("explore.add")}
                        </button>
                      ))}
                    {riga.inLibreria && (
                      <span className="pastiglia certo" data-livello="certo">
                        {t("explore.added")}
                      </span>
                    )}
                  </span>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}
