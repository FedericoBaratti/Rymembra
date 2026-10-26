/**
 * La coda: le righe, e il pannello che le conteneva.
 *
 * # Perché gli indici vengono dalla coda e non dalle righe
 *
 * `StatoRiproduzione.coda` sono identificativi; le righe si chiedono a parte con
 * `braniPerId`. Le due liste **non hanno la stessa lunghezza**: un brano tolto
 * dalla libreria mentre stava in coda non torna indietro — «gli identificativi
 * che non esistono più semplicemente non compaiono», dice `summaries_by_id`.
 *
 * Disegnare le righe ricevute e usarne la posizione vorrebbe dire mandare a
 * `coda_vai` un indice spostato di uno per ogni brano sparito, cioè far partire
 * una canzone diversa da quella cliccata. Perciò si cammina sulla coda — che è
 * la verità — e le righe si cercano per identificativo.
 *
 * # Perché il file è diviso in tre
 *
 * La coda ora si vede in due posti: nella terza colonna, che è la sua casa, e
 * nel pannello flottante, che torna quando la colonna è chiusa. Sono la stessa
 * lista con due cornici, e duplicarla vorrebbe dire correggere due volte ogni
 * difetto del riordino. Quindi: un gancio che tiene le righe, un elenco che le
 * disegna, e un pannello che è solo la cornice.
 */
import { memo, useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";

import { durata, nomeArtista, numero, titoloAlbum } from "./formato";
import { useFuoco } from "./fuoco";
import { gestoPermesso, proprietarioDellaRiga, righeDaChiedere } from "./insieme";
import { ipc, type Brano, type StatoRiproduzione } from "./ipc";
import { Icona } from "./parti/Icone";
import { useProprietari } from "./parti/Insieme";
import { usePresaPerRiordino } from "./riordino";
import { t, tSe, useLingua } from "./lingue";
import { useVirtuale } from "./virtuale";
import { conIndice, statoSuono } from "./segnali";
import { useDati } from "./dati";
import { Presenza } from "./presenza";

/**
 * Le righe della coda, per identificativo.
 *
 * La chiave è la coda unita con le virgole e non l'array: React confronta per
 * identità, e ogni evento `riproduzione:stato` porta un array nuovo con dentro
 * gli stessi numeri. Il nucleo ne manda quattro al secondo — senza questo, ogni
 * secondo partirebbero quattro richieste di righe identiche.
 *
 * # Si chiedono solo le righe che mancano
 *
 * Prima ogni cambiamento della coda — un brano accodato, uno tolto, un riordino
 * — richiedeva **tutte** le righe: su una coda di diciottomila brani, «Tutti i
 * brani» fatto partire, erano diciottomila righe attraverso l'IPC per spostarne
 * una. Adesso quelle già in mano restano, quelle che la coda non nomina più si
 * lasciano andare, e si chiedono le sole nuove: un riordino non ne chiede
 * nessuna.
 *
 * I voti e i preferiti delle righe già in mano non si rinfrescano da qui: la
 * colonna li legge dallo stato del brano in ascolto, e una riga della coda mostra
 * titolo, artista e album, che non cambiano mentre il brano sta in coda.
 *
 * # Tranne le voci di «Ascolta insieme»
 *
 * Una voce della sessione (id negativo) arriva prima con quel che si sa e poi
 * si completa — la copertina viaggia dopo — con lo stesso id: chiesta una
 * volta sola, la riga restava senza copertina per sempre. `voci` è il segno
 * che le descrizioni sono cambiate (`InsiemeNelLettore.revisione`), `null`
 * fuori sessione; quando cambia, le voci si richiedono (`righeDaChiedere`).
 * Anche entrando e uscendo da una sessione: le maniglie sono della sessione, e
 * la `-3` di quella di prima non è la `-3` di questa.
 */
export function useRigheCoda(
  coda: readonly number[],
  onErrore: (e: unknown) => void,
  voci: number | null,
): Map<number, Brano> {
  const [righe, setRighe] = useState<Map<number, Brano>>(new Map());
  const letture = useDati().ipc;
  const chiave = coda.join(",");
  // Le righe già in mano, lette dall'effetto senza farne una dipendenza: una
  // risposta che arriva cambia la mappa, e con la mappa fra le dipendenze
  // l'effetto ripartirebbe per chiedere quel che ha appena ricevuto.
  const inMano = useRef(righe);
  inMano.current = righe;
  // Il segno delle voci per cui le righe in mano sono state lette. Si scrive
  // solo a lettura riuscita: un cambio di coda che annulla la richiesta a metà
  // la lascia da rifare al giro dopo, invece di darla per fatta.
  const vociLette = useRef<number | null | undefined>(undefined);

  useEffect(() => {
    const ids = chiave.length > 0 ? chiave.split(",").map(Number) : [];
    if (ids.length === 0) {
      setRighe(new Map());
      vociLette.current = voci;
      return;
    }
    const nominati = new Set(ids);
    const prima = inMano.current;
    const stantie = vociLette.current !== undefined && vociLette.current !== voci;
    const mancanti = righeDaChiedere(ids, (id) => prima.has(id), stantie);
    const restano = (mappa: Map<number, Brano>) =>
      new Map([...mappa].filter(([id]) => nominati.has(id)));
    if (mancanti.length === 0) {
      vociLette.current = voci;
      if (prima.size !== nominati.size) setRighe(restano(prima));
      return;
    }
    let annullato = false;
    letture
      .braniPerId(mancanti)
      .then((trovate) => {
        if (annullato) return;
        vociLette.current = voci;
        setRighe((adesso) => {
          const fusa = restano(adesso);
          for (const brano of trovate) fusa.set(brano.id, brano);
          return fusa;
        });
      })
      .catch(onErrore);
    return () => {
      annullato = true;
    };
  }, [chiave, onErrore, letture, voci]);

  return righe;
}

/**
 * L'elenco della coda, riordinabile.
 *
 * # Perché il riordino ha anche una scorciatoia
 *
 * Il trascinamento non esiste per chi non usa il mouse: nasce da un
 * puntatore, e nessuna combinazione di tasti lo produce. Finché il riordino era
 * **solo** trascinabile, riordinare la coda era una funzione che una parte degli
 * utenti non aveva — non «scomoda», assente. Il trascinamento stesso è quello
 * di `riordino.ts`, e il perché non è più quello HTML5 sta là.
 *
 * `Alt`+`↑↓` sposta la riga a fuoco. Alt e non le frecce nude perché quelle
 * devono continuare a muovere il fuoco: sono due gesti diversi sullo stesso
 * tasto, e il modificatore è quel che li distingue.
 *
 * # Una fermata di tabulazione, come l'elenco dei brani
 *
 * Ogni riga aveva due fermate — il titolo e la × — e la coda di «Tutti i
 * brani» ne ha diciottomila: chi arrivava con Tab nella terza colonna ne usciva
 * dopo una sessantina di pressioni, solo perché la finestra disegna trenta
 * righe. E una rotellata che portava via la riga col fuoco lo faceva cadere sul
 * `<body>`. Adesso la coda è un fuoco a rotazione (`fuoco.ts`): una fermata per
 * tutta la lista — la prima riga in vista se quella attiva è uscita —, ↑↓ fra
 * le righe, PagSu/PagGiù, Inizio/Fine, ←→ fra i due comandi della riga, Invio e
 * Spazio fanno partire il brano come faceva il titolo premuto.
 */
export function RigheCoda({
  stato,
  righe,
  onErrore,
  compatta,
}: {
  stato: StatoRiproduzione;
  righe: Map<number, Brano>;
  onErrore: (e: unknown) => void;
  /** Nella terza colonna: niente durata, la larghezza non c'è. */
  compatta?: boolean | undefined;
}) {
  const [trascinato, setTrascinato] = useState<number | null>(null);
  /**
   * Su quale riga cadrebbe il rilascio adesso.
   *
   * Il trascinamento c'era e non lo diceva: si lasciava andare e si scopriva
   * dopo dov'era finita la riga. È lo stesso segno che ora porta l'elenco di
   * una playlist.
   */
  const [mirato, setMirato] = useState<number | null>(null);

  /*
   * In una sessione di «Ascolta insieme», chi segue tocca la coda solo quando
   * l'host inoltra i suoi gesti (`gesti`, fase 2): fino ad allora far partire,
   * togliere e spostare una riga sono spenti qui, invece di accendersi e
   * rispondere con un errore. Il perché lo dice la fascia della sessione: un
   * tasto spento non mostra il suo suggerimento.
   */
  const codaLibera = gestoPermesso(stato.insieme, "coda");
  /* Di chi sono le voci degli altri PC: «di Marco» su ogni riga che non è di
     questo PC. Fuori sessione la mappa è vuota e non costa niente. */
  const proprietari = useProprietari();

  /*
   * Solo le righe che si vedono, come l'elenco dei brani. La coda è una lista
   * come le altre, e «Tutti i brani» fatto partire la riempie di diciottomila
   * righe: disegnarle tutte erano decine di migliaia di nodi nella terza
   * colonna, che è sempre aperta. Lo scorrevole è l'elenco stesso, quindi
   * contenitore e ancora coincidono; le due `.paglia` sono `li` perché stanno
   * dentro un `ol`.
   *
   * Con `altezzeDiverse`: il titolo si legge intero e va a capo quanto serve,
   * quindi un brano dal nome lungo è una riga più alta delle altre.
   */
  const scorrevole = useRef<HTMLOListElement>(null);
  const finestra = useVirtuale({
    totale: stato.coda.length,
    contenitore: scorrevole,
    ancora: scorrevole,
    selettoreRiga: ".riga-coda",
    altezzeDiverse: true,
  });

  const comanda = (azione: Promise<void>) => {
    azione.catch(onErrore);
  };

  const posa = () => {
    setTrascinato(null);
    setMirato(null);
  };

  const lascia = (a: number) => {
    if (trascinato === null || trascinato === a) {
      posa();
      return;
    }
    comanda(ipc.codaRiordina(trascinato, a));
    posa();
  };

  const presa = usePresaPerRiordino({
    onPresa: (indice) => {
      if (indice === null) posa();
      else setTrascinato(indice);
    },
    onMira: setMirato,
    onLascia: lascia,
  });

  /** Invio e Spazio su una riga: parte quel brano, se c'è ancora. */
  const coda = stato.coda;
  const vai = useCallback(
    (indice: number) => {
      const id = coda[indice];
      if (!codaLibera || id === undefined || !righe.has(id)) return;
      ipc.codaVai(indice).catch(onErrore);
    },
    [coda, righe, onErrore, codaLibera],
  );

  const fuoco = useFuoco({
    totale: coda.length,
    ancora: scorrevole,
    finestra,
    scorriA: finestra.scorriA,
    selettoreRiga: ".riga-coda",
    onInvio: vai,
    onSpazio: vai,
  });

  /**
   * La riga che il fuoco deve seguire quando la coda nuova arriva.
   *
   * Il riordino passa dal nucleo, e le righe si rifanno quando torna lo stato:
   * la chiave di una riga porta l'identificativo, quindi il nodo della riga
   * spostata **non** è quello di prima, e il fuoco che ci stava sopra cadrebbe
   * sul `<body>`. Si ricorda dove deve andare e **quale brano** ci deve
   * arrivare, e ci si va quando lo stato lo dice: lo stato arriva quattro volte
   * al secondo con una coda nuova anche quando è la stessa, e solo il brano al
   * suo posto dice che il riordino è arrivato.
   */
  const seguire = useRef<{ dove: number; id: number } | null>(null);
  const vaiA = fuoco.vaiA;
  useEffect(() => {
    const atteso = seguire.current;
    if (atteso === null || coda[atteso.dove] !== atteso.id) return;
    seguire.current = null;
    vaiA(atteso.dove);
    /* E poi dentro la vista, col minimo di scorrimento. Le due righe scambiate
       sono nodi nuovi, e lo scroll anchoring del motore — che non trova più il
       nodo su cui si reggeva — sposta lo scorrimento dell'altezza di una riga.
       Con righe uguali era una riga d'ingombro e la spostata restava in vista;
       con un titolo su cinque righe finiva mezza sotto il bordo, e il fuoco
       già sul nodo non la riportava su: `focus()` su chi ce l'ha non scorre. */
    const elenco = scorrevole.current;
    const riga =
      elenco?.querySelector<HTMLElement>(`.riga-coda[data-indice="${atteso.dove}"]`) ?? null;
    if (elenco === null || riga === null) return;
    const vista = elenco.getBoundingClientRect();
    const r = riga.getBoundingClientRect();
    const sopra = vista.top - r.top;
    const sotto = r.bottom - vista.bottom;
    if (sopra > 0) elenco.scrollTop -= sopra;
    else if (sotto > 0) elenco.scrollTop += Math.min(sotto, -sopra);
  }, [coda, vaiA]);

  const daTastiera = (e: KeyboardEvent<HTMLElement>) => {
    if (codaLibera && e.altKey && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      const riga =
        e.target instanceof HTMLElement ? e.target.closest<HTMLElement>(".riga-coda") : null;
      const indice = Number(riga?.dataset.indice);
      if (riga === null || !Number.isInteger(indice)) return;
      const a = indice + (e.key === "ArrowDown" ? 1 : -1);
      const id = coda[indice];
      if (a < 0 || a >= coda.length || id === undefined) return;
      e.preventDefault();
      e.stopPropagation();
      // Il fuoco segue la riga spostata invece di restare sulla posizione: chi
      // tiene premuto Alt e freccia sta spostando **una** canzone, e lasciare il
      // fuoco fermo farebbe scendere quella dopo al colpo successivo. Subito
      // sulla posizione d'arrivo — `vaiA` sa scorrere se è fuori dalla
      // finestra — e di nuovo quando la coda nuova arriva: vedi `seguire`.
      seguire.current = { dove: a, id };
      fuoco.vaiA(a);
      ipc.codaRiordina(indice, a).catch((errore: unknown) => {
        seguire.current = null;
        onErrore(errore);
      });
      return;
    }
    fuoco.daTastiera(e);
  };

  if (stato.coda.length === 0) {
    return <p className="vuota-coda empty-state">{t("queue.empty")}</p>;
  }

  return (
    <ol
      ref={scorrevole}
      className="righe-coda queue-list"
      data-compatta={compatta || undefined}
      data-scorso={finestra.scorso || undefined}
      /* Un gestore solo per tutte le righe, come nell'elenco dei brani. */
      onKeyDown={daTastiera}
    >
      <li
        className="paglia"
        style={{ height: finestra.pagliaSopra }}
        role="presentation"
      />
      {stato.coda.slice(finestra.primo, finestra.ultimo).map((id, k) => {
        const indice = finestra.primo + k;
        const brano = righe.get(id);
        const inAscolto = indice === stato.posizioneCoda;
        /* Il perché sta solo sul brano **subito dopo** quello in ascolto, ed è
           l'unico che ce l'ha: il nucleo spiega la scelta che ha appena fatto,
           non tutta la coda. Le righe più in là le hai messe tu, o le sceglierà
           quando ci arriverà — e una frase su una scelta non ancora presa
           sarebbe inventata. */
        const codice =
          stato.posizioneCoda !== null &&
          indice === stato.posizioneCoda + 1
            ? stato.motivoProssimo
            : null;
        /* `tSe` e non `t`: il codice arriva dal nucleo, quindi TypeScript non
           può verificarlo. Il ripiego è la stringa vuota, e una stringa vuota
           non disegna niente — un motivo che l'interfaccia non conosce si tace
           invece di mostrare la propria chiave. */
        const perche = codice === null ? "" : tSe(`queue.why.${codice}`, "");
        /* «di Marco»: di chi è la voce, in una sessione dove i brani arrivano
           da più PC. Su ogni riga che non è di questo PC; vedi
           `proprietarioDellaRiga`. */
        const diChi = proprietarioDellaRiga(id, proprietari);
        return (
          <li
            /* L'indice fa parte della chiave: la stessa canzone può stare due
               volte nella stessa coda, e l'identificativo da solo non la
               distinguerebbe. */
            key={`${indice}-${id}`}
            className="riga-coda list-row queue-row"
            {...statoSuono(inAscolto, stato.inPausa)}
            style={conIndice(indice)}
            data-indice={indice}
            aria-current={inAscolto}
            data-active={inAscolto || undefined}
            data-sopra={(mirato === indice && trascinato !== indice) || undefined}
            data-riordino={indice}
            /* La sola fermata di tabulazione della coda: vedi la nota. */
            tabIndex={indice === fuoco.fermata ? 0 : -1}
            onFocus={() => fuoco.segna(indice)}
            onPointerDown={(e) => {
              if (codaLibera) presa(e, indice);
            }}
            onDoubleClick={() => {
              if (codaLibera) comanda(ipc.codaVai(indice));
            }}
          >
            <span className="presa" aria-hidden="true">
              <Icona nome={inAscolto ? "i-play" : "i-grip"} dim={13} />
            </span>
            <button
              type="button"
              className="salta"
              onClick={() => comanda(ipc.codaVai(indice))}
              /* Si raggiunge con ←→ dalla riga, non con Tab: `fuoco.ts`. */
              tabIndex={-1}
              /* Un brano sparito dalla libreria non si può far partire, ma si
                 deve poter togliere: il tasto si spegne, la riga no. */
              disabled={!brano || !codaLibera}
            >
              {/* `dir="auto"`: un titolo arabo o ebraico si legge da destra. */}
              <span className="nome" dir="auto">
                {brano ? brano.title : t("queue.goneTrack")}
              </span>
              <span className="autore" dir="auto">
                {brano
                  ? `${nomeArtista(brano.artist)} · ${titoloAlbum(brano.album)}`
                  : `id ${id}`}
              </span>
              {perche !== "" && !compatta && (
                <span className="perche">{perche}</span>
              )}
              {/* Una riga sola coi puntini: il `title` porta il nome intero,
                  che è proprio la parte che i puntini tagliano. */}
              {diChi !== null && (
                <span
                  className="di-chi"
                  dir="auto"
                  title={t("together.sign.owner", { nome: diChi })}
                >
                  {t("together.sign.owner", { nome: diChi })}
                </span>
              )}
            </button>
            {!compatta && (
              <span className="durata">
                {brano ? durata(brano.durationMs) : "—"}
              </span>
            )}
            <button
              type="button"
              className="tasto icon-btn"
              /* Col titolo dentro, come fa la × dell'elenco. «Togli dalla coda»
                 era la stessa frase su tutte le righe: chi guarda sa quale × ha
                 sotto il dito, chi ascolta sentiva cinquanta volte la stessa
                 cosa senza sapere quale brano stava per perdere. Il brano non più
                 in libreria si nomina come si nomina nella riga — non c'è altro
                 nome da dargli. */
              aria-label={t("queue.remove.aria", {
                titolo: brano ? brano.title : t("queue.goneTrack"),
              })}
              /* Il `title` resta la frase corta: al passaggio del mouse il titolo
                 del brano è già scritto accanto, a due centimetri dal puntatore. */
              title={codaLibera ? t("queue.remove") : t("together.gestures.off.title")}
              tabIndex={-1}
              disabled={!codaLibera}
              onClick={() => comanda(ipc.codaTogli(indice))}
            >
              <Icona nome="i-x" dim={14} />
            </button>
          </li>
        );
      })}
      <li
        className="paglia"
        style={{ height: finestra.pagliaSotto }}
        role="presentation"
      />
    </ol>
  );
}

/**
 * Il pannello flottante della coda.
 *
 * Torna a servire solo quando la terza colonna è chiusa: con la colonna aperta
 * la coda è già visibile, e aprirne una seconda copia sopra sarebbe la stessa
 * lista due volte nella stessa finestra.
 */
export const Coda = memo(function Coda({
  stato,
  onChiudi,
  onErrore,
}: {
  stato: StatoRiproduzione;
  onChiudi: () => void;
  onErrore: (e: unknown) => void;
}) {
  // Sotto `memo` (vedi «I widget sotto `memo`» in `Impaginazione.tsx`): si
  // ridisegna per le sue prop, e per la lingua, che non è una prop — senza
  // l'iscrizione i testi resterebbero quelli del primo disegno.
  useLingua();
  const righe = useRigheCoda(stato.coda, onErrore, stato.insieme?.revisione ?? null);

  return (
    <Presenza parte="queue-panel">
      <aside
        className="pannello-coda queue-panel"
        // L'ancora del giro guidato: il ripiego del passo della colonna, per chi
        // la colonna la tiene chiusa.
        data-giro="coda"
        aria-label={t("queue.panel.aria")}
      >
        <header>
          <h2>{t("queue.panel.title")}</h2>
          <span className="conteggio">{numero(stato.coda.length)}</span>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            disabled={stato.coda.length === 0 || !gestoPermesso(stato.insieme, "svuota")}
            title={gestoPermesso(stato.insieme, "svuota") ? undefined : t("together.clear.off")}
            onClick={() => {
              ipc.codaSvuota().catch(onErrore);
            }}
          >
            {t("queue.clear")}
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("queue.close")}
            onClick={onChiudi}
          >
            <Icona nome="i-x" dim={15} />
          </button>
        </header>
        <RigheCoda stato={stato} righe={righe} onErrore={onErrore} />
      </aside>
    </Presenza>
  );
});
