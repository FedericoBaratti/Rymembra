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
import { createContext, memo, useCallback, useContext, useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";

import { durata, nomeArtista, numero, titoloAlbum } from "./formato";
import { useFuoco } from "./fuoco";
import { gestoPermesso, idDiLibreria, proprietarioDellaRiga, righeDaChiedere } from "./insieme";
import { ipc, type Brano, type StatoRiproduzione } from "./ipc";
import { Icona } from "./parti/Icone";
import { useProprietari } from "./parti/Insieme";
import { usePresaPerRiordino } from "./riordino";
import {
  ATTESA_RIORDINO_MS,
  prossimoRiordino,
  sparizioniDaTogliere,
  statoRiga,
  type MappaRigheCoda,
  type RiordinoInVolo,
} from "./righeCoda";
import { t, tSe, useLingua } from "./lingue";
import { pezziDellaFinestra, useVirtuale } from "./virtuale";
import { conIndice, statoSuono } from "./segnali";
import { useDati } from "./dati";
import { Presenza } from "./presenza";
import { MOBILE } from "./piattaforma";

/**
 * I comandi della riga su cui un doppio clic non fa partire il brano: la × che
 * toglie e il ⋮ del telefono. Vedi `onDoubleClick` in `RigheCoda`.
 */
const TASTI_SENZA_PARTENZA = ".tasto, .tasto-altro";

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
 *
 * # Chiesta e non trovata
 *
 * Un brano chiesto che la risposta non porta è sparito dalla libreria mentre
 * stava in coda: la mappa lo segna con `null`, ed è l'unico caso in cui la
 * riga dice «non più in libreria» (`statoRiga`). Una riga che la mappa non ha
 * ancora è in viaggio, e si disegna come uno scheletro: prima le due cose
 * erano la stessa, e all'avvio con una coda grande ogni riga si dava per persa
 * finché la risposta non arrivava (R13b-5).
 *
 * Una lettura fallita (il database occupato da una scansione) si riprova dopo
 * {@link RIPROVA_RIGHE_MS}: senza, le righe restavano scheletri finché la coda
 * non cambiava. L'errore si dice una volta, non a ogni tentativo.
 */
export function useRigheCoda(
  coda: readonly number[],
  onErrore: (e: unknown) => void,
  voci: number | null,
): MappaRigheCoda {
  const [righe, setRighe] = useState<Map<number, Brano | null>>(new Map());
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
  /** Cresce per rifare una lettura fallita. */
  const [riprova, setRiprova] = useState(0);
  /** La coda per cui l'errore è già stato detto: i tentativi dopo tacciono. */
  const erroreDetto = useRef<string | null>(null);

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
    const restano = (mappa: Map<number, Brano | null>) =>
      new Map([...mappa].filter(([id]) => nominati.has(id)));
    if (mancanti.length === 0) {
      vociLette.current = voci;
      if (prima.size !== nominati.size) setRighe(restano(prima));
      return;
    }
    let annullato = false;
    let attesa: number | undefined;
    letture
      .braniPerId(mancanti)
      .then((trovate) => {
        if (annullato) return;
        vociLette.current = voci;
        erroreDetto.current = null;
        setRighe((adesso) => {
          const fusa = restano(adesso);
          // Chieste e non trovate: sparite. Solo i brani della libreria; una
          // voce di «Ascolta insieme» che non torna è una voce che la sessione
          // non descrive più, non un brano tolto.
          for (const id of mancanti) if (idDiLibreria(id)) fusa.set(id, null);
          for (const brano of trovate) fusa.set(brano.id, brano);
          return fusa;
        });
      })
      .catch((e: unknown) => {
        if (annullato) return;
        if (erroreDetto.current !== chiave) {
          erroreDetto.current = chiave;
          onErrore(e);
        }
        attesa = window.setTimeout(() => setRiprova((n) => n + 1), RIPROVA_RIGHE_MS);
      });
    return () => {
      annullato = true;
      if (attesa !== undefined) window.clearTimeout(attesa);
    };
  }, [chiave, onErrore, letture, voci, riprova]);

  return righe;
}

/** Dopo quanto riprovare le righe della coda, se la lettura è fallita. */
const RIPROVA_RIGHE_MS = 2_000;

/**
 * Il menù di una riga della coda, da chi lo sa aprire: `App`.
 *
 * Un contesto e non una prop, perché la coda si disegna in quattro posti — la
 * colonna, il pannello, lo schermo intero, il telefono — e il menù della riga
 * deve esserci in tutti: è lì che stanno «Sposta su / giù / in cima», l'unico
 * modo di riordinare la coda senza trascinare né usare la tastiera (R16-6,
 * D-72). Fuori dalla finestra (lo Studio) nessuno lo fornisce, e la riga resta
 * senza menù.
 */
export const MenuDellaCoda = createContext<
  ((e: React.MouseEvent, indice: number, brano: Brano) => void) | null
>(null);

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
  onMenu,
}: {
  stato: StatoRiproduzione;
  righe: MappaRigheCoda;
  onErrore: (e: unknown) => void;
  /** Nella terza colonna: niente durata, la larghezza non c'è. */
  compatta?: boolean | undefined;
  /**
   * Sul telefono: il menù della riga, dal ⋮ che prende il posto della × e dal
   * tocco lungo. «Togli dalla coda» sta lì dentro. Solo sulle righe della
   * libreria: una voce di «Ascolta insieme» o un brano sparito non hanno un
   * menù, e tengono la loro ×.
   */
  onMenu?: ((e: React.MouseEvent, indice: number, brano: Brano) => void) | undefined;
}) {
  /* Il menù della riga: quello passato, o quello della finestra (`MenuDellaCoda`).
     Sul computer si apre col tasto destro, sul telefono anche dal ⋮. */
  const menuDellaFinestra = useContext(MenuDellaCoda);
  const apriMenu = onMenu ?? menuDellaFinestra ?? undefined;
  const { anteprima } = useDati();
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

  /**
   * Il brano della riga presa, letto quando la si prende.
   *
   * Non al rilascio: lo stato arriva quattro volte al secondo, e mentre il dito
   * trascina la coda può cambiare sotto — il «Riproduci dopo» di chi ascolta
   * insieme. Letto alla fine sarebbe il brano che adesso sta nella posizione
   * di partenza, il nucleo lo troverebbe al suo posto e sposterebbe la riga
   * sbagliata. Vedi `codaRiordina` in `ipc.ts`.
   */
  const preso = useRef<number | null>(null);

  const posa = () => {
    setTrascinato(null);
    setMirato(null);
    preso.current = null;
  };

  const lascia = (a: number) => {
    const brano = preso.current;
    if (trascinato === null || trascinato === a || brano === null) {
      posa();
      return;
    }
    comanda(ipc.codaRiordina(trascinato, a, brano));
    posa();
  };

  const presa = usePresaPerRiordino({
    onPresa: (indice) => {
      if (indice === null) {
        posa();
        return;
      }
      preso.current = stato.coda[indice] ?? null;
      setTrascinato(indice);
    },
    onMira: setMirato,
    onLascia: lascia,
  });

  /** Invio e Spazio su una riga: parte quel brano, se c'è ancora. */
  const coda = stato.coda;
  const vai = useCallback(
    (indice: number) => {
      const id = coda[indice];
      if (!codaLibera || id === undefined || !righe.get(id)) return;
      ipc.codaVai(indice, id).catch(onErrore);
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
  const seguire = useRef<(RiordinoInVolo & { entro: number }) | null>(null);
  /** Il riordino in volo, se non è scaduto: vedi `ATTESA_RIORDINO_MS`. */
  const inVolo = (): RiordinoInVolo | null => {
    const atteso = seguire.current;
    if (atteso !== null && Date.now() > atteso.entro) seguire.current = null;
    return seguire.current;
  };
  const vaiA = fuoco.vaiA;
  useEffect(() => {
    const atteso = seguire.current;
    if (atteso === null) return;
    // Scaduto: il riordino non è arrivato, e un'attesa appesa porterebbe il
    // fuoco qui più tardi, da dovunque sia.
    if (Date.now() > atteso.entro) {
      seguire.current = null;
      return;
    }
    if (coda[atteso.dove] !== atteso.id) return;
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
      // Col colpo di prima ancora in viaggio, si continua a portare il suo
      // brano: vedi `prossimoRiordino`.
      const colpo = prossimoRiordino(
        coda,
        inVolo(),
        indice,
        e.key === "ArrowDown" ? "giu" : "su",
      );
      if (colpo === null) return;
      const { da, a, id } = colpo;
      e.preventDefault();
      e.stopPropagation();
      // Il fuoco segue la riga spostata invece di restare sulla posizione: chi
      // tiene premuto Alt e freccia sta spostando **una** canzone, e lasciare il
      // fuoco fermo farebbe scendere quella dopo al colpo successivo. Subito
      // sulla posizione d'arrivo — `vaiA` sa scorrere se è fuori dalla
      // finestra — e di nuovo quando la coda nuova arriva: vedi `seguire`.
      seguire.current = { dove: a, id, entro: Date.now() + ATTESA_RIORDINO_MS };
      fuoco.vaiA(a);
      ipc.codaRiordina(da, a, id).catch((errore: unknown) => {
        seguire.current = null;
        onErrore(errore);
      });
      return;
    }
    fuoco.daTastiera(e);
  };

  /*
   * Le righe sparite dalla libreria si tolgono dalla coda da sole (decisione G
   * su R13b-5): una riga che non si può far partire e dice solo «non più in
   * libreria» non serve a nessuno. Una volta per riga, dalla più in fondo e
   * ognuna col suo brano (`sparizioniDaTogliere`), quindi se la coda cambia nel
   * frattempo il nucleo non toglie un'altra riga. Non dallo Studio, che
   * disegna una coda finta, e non dove la coda non è di questo PC: lì la riga
   * resta, col suo nome.
   */
  const chiesteVia = useRef(new Set<string>());
  useEffect(() => {
    if (anteprima || !codaLibera) return;
    const via = sparizioniDaTogliere(coda, righe);
    const chiavi = new Set(via.map((v) => `${v.indice}-${v.id}`));
    // Si dimentica quel che non è più da togliere: una riga tolta lascia la
    // coda, e il suo posto può tornare a essere di un altro.
    for (const k of chiesteVia.current) if (!chiavi.has(k)) chiesteVia.current.delete(k);
    const nuove = via.filter((v) => !chiesteVia.current.has(`${v.indice}-${v.id}`));
    if (nuove.length === 0) return;
    for (const v of nuove) chiesteVia.current.add(`${v.indice}-${v.id}`);
    void nuove
      .reduce<Promise<void>>(
        (prima, v) => prima.then(() => ipc.codaTogli(v.indice, v.id)),
        Promise.resolve(),
      )
      .catch(onErrore);
  }, [coda, righe, codaLibera, anteprima, onErrore]);

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
      {/* La finestra, le paglie, e la riga col fuoco anche quando una rotellata
          l'ha portata fuori: smontata, il fuoco cadeva sul `<body>` (R13b-7).
          Vedi `pezziDellaFinestra`. */}
      {pezziDellaFinestra(finestra.primo, finestra.ultimo, coda.length, fuoco.attivo).map((pezzo) => {
        if ("paglia" in pezzo) {
          return (
            <li
              key={pezzo.paglia}
              className="paglia"
              style={{
                height: Math.max(finestra.inizioDi(pezzo.a) - finestra.inizioDi(pezzo.da), 0),
              }}
              role="presentation"
            />
          );
        }
        const indice = pezzo.riga;
        const id = coda[indice];
        if (id === undefined) return null;
        const brano = righe.get(id) ?? undefined;
        const comeSta = statoRiga(id, righe);
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
            /* La posizione nella coda intera: la finestra ne disegna una
               trentina, e senza questi il lettore di schermo diceva «7 di 32»
               su una coda di diciottomila (R13b-6). */
            aria-posinset={indice + 1}
            aria-setsize={stato.coda.length}
            aria-busy={comeSta === "attesa" || undefined}
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
            onDoubleClick={(e) => {
              // Sulla riga, non sulla × né sul ⋮: il doppio clic sulla ×
              // risaliva fin qui, e oltre a togliere la riga faceva partire
              // quella che le era scivolata sotto.
              if ((e.target as Element).closest(TASTI_SENZA_PARTENZA)) return;
              if (codaLibera) comanda(ipc.codaVai(indice, id));
            }}
            // Sul telefono un tocco sulla riga, come il doppio clic qui sopra:
            // il tasto «salta» c'è, ma è piccolo, e la riga intera è il
            // bersaglio che un dito cerca.
            onClick={
              MOBILE
                ? (e) => {
                    if (codaLibera && !(e.target as Element).closest("button")) {
                      comanda(ipc.codaVai(indice, id));
                    }
                  }
                : undefined
            }
            onContextMenu={
              apriMenu !== undefined && brano !== undefined && idDiLibreria(id)
                ? (e) => apriMenu(e, indice, brano)
                : undefined
            }
          >
            <span className="presa" aria-hidden="true">
              <Icona nome={inAscolto ? "i-play" : "i-grip"} dim={13} />
            </span>
            <button
              type="button"
              className="salta"
              onClick={() => comanda(ipc.codaVai(indice, id))}
              /* Si raggiunge con ←→ dalla riga, non con Tab: `fuoco.ts`. */
              tabIndex={-1}
              /* Un brano sparito dalla libreria non si può far partire, ma si
                 deve poter togliere: il tasto si spegne, la riga no. */
              disabled={!brano || !codaLibera}
            >
              {/* `dir="auto"`: un titolo arabo o ebraico si legge da destra.
                  Una riga in viaggio è uno scheletro, una sparita lo dice e
                  basta: un «id 40213» non è un nome da mostrare a una persona. */}
              {comeSta === "attesa" ? (
                <>
                  <span className="nome riga-finta skeleton" />
                  <span className="autore riga-finta corta skeleton" />
                </>
              ) : (
                <>
                  <span className="nome" dir="auto">
                    {brano ? brano.title : t("queue.goneTrack")}
                  </span>
                  {brano && (
                    <span className="autore" dir="auto">
                      {`${nomeArtista(brano.artist)} · ${titoloAlbum(brano.album)}`}
                    </span>
                  )}
                </>
              )}
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
            {MOBILE && apriMenu !== undefined && brano !== undefined && idDiLibreria(id) ? (
              <button
                type="button"
                className="tasto-altro"
                aria-label={t("row.more.aria", { titolo: brano.title })}
                tabIndex={-1}
                onClick={(e) => apriMenu(e, indice, brano)}
              >
                <Icona nome="i-dots-v" dim={20} />
              </button>
            ) : (
              <button
                type="button"
                className="tasto icon-btn"
                /* Col titolo dentro, come fa la × dell'elenco. «Togli dalla coda»
                   era la stessa frase su tutte le righe: chi guarda sa quale × ha
                   sotto il dito, chi ascolta sentiva cinquanta volte la stessa
                   cosa senza sapere quale brano stava per perdere. Il brano non più
                   in libreria si nomina come si nomina nella riga — non c'è altro
                   nome da dargli. */
                aria-label={
                  comeSta === "attesa"
                    ? t("queue.remove")
                    : t("queue.remove.aria", {
                        titolo: brano ? brano.title : t("queue.goneTrack"),
                      })
                }
                /* Il `title` resta la frase corta: al passaggio del mouse il titolo
                   del brano è già scritto accanto, a due centimetri dal puntatore. */
                title={codaLibera ? t("queue.remove") : t("together.gestures.off.title")}
                tabIndex={-1}
                disabled={!codaLibera}
                onClick={(e) => {
                  // Il secondo clic di un doppio clic non è un'altra richiesta:
                  // dove lo stesso brano sta due volte di fila, il nucleo
                  // vedrebbe a quella posizione il brano atteso e toglierebbe
                  // anche la copia. Da tastiera `detail` è zero, e passa.
                  if (e.detail > 1) return;
                  comanda(ipc.codaTogli(indice, id));
                }}
              >
                <Icona nome="i-x" dim={14} />
              </button>
            )}
          </li>
        );
      })}
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
