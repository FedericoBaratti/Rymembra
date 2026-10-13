/**
 * La Home: i suoi ripiani e le raccolte del lunedì, e quando si richiedono.
 *
 * `schermate/Home.tsx` disegna; qui c'è quando chiedere. Tre momenti — entrando
 * nella Home, cambiando brano mentre la si guarda, e dopo che la libreria è
 * cambiata (`rileggiCasa`, che chiama `App` dal suo `ricaricaContorno`) — e un
 * turno solo per tutti e tre, perché le risposte possono incrociarsi. Nel
 * corpo di `App` erano tre effetti e due riferimenti lontani dai ripiani che
 * servivano, e dal turno che li teneva in fila.
 *
 * L'hook si chiama dove stavano i tre effetti, dopo quello degli artisti: lo
 * stato, che era dichiarato in cima ad `App`, nasce adesso qui.
 *
 * # Il brano corrente lo segue da sé
 *
 * L'effetto «restando» dipendeva dal brano corrente, e per averlo `App` si
 * iscriveva al lettore: a ogni brano nuovo si ridisegnava la finestra intera —
 * gli elenchi, le griglie — perché la Home, forse, doveva rileggersi. Adesso
 * l'hook riceve l'archivio del lettore e lo segue lui, senza disegnare niente:
 * quando il brano cambia mentre si guarda la Home, chiede; altrimenti tace.
 * Le richieste sono le stesse di prima, nello stesso momento.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { ipc, type Casa, type Raccolta } from "./ipc";
import type { Vista } from "./parti/Navigazione";
import type { ArchivioLettore } from "./riproduzione";
import { vinceLUltima } from "./ultima";

/** Quel che `useCasa` legge da `App`. */
export interface IngressiCasa {
  vista: Vista;
  /** C'è qualcosa nella casella di ricerca: la Home non si vede. */
  cercando: boolean;
  /** Lo stato del lettore, per il brano corrente: vedi «Il brano corrente lo segue da sé». */
  lettore: ArchivioLettore;
  segnalaErrore: (e: unknown) => void;
}

/** La Home, e come la si fa rileggere. */
export interface CasaDellaFinestra {
  /** I ripiani, o `null` finché non sono stati chiesti. */
  casa: Casa | null;
  /** Le raccolte del lunedì. */
  settimana: Raccolta[];
  /** Riscrive le raccolte: una appena aperta non è più nuova. */
  setSettimana: (cambia: (prima: Raccolta[]) => Raccolta[]) => void;
  /** Richiede i ripiani: la libreria è cambiata sotto. */
  rileggiCasa: () => void;
}

/** Vedi il commento in testa. */
export function useCasa({
  vista,
  cercando,
  lettore,
  segnalaErrore,
}: IngressiCasa): CasaDellaFinestra {
  // La Home arriva in una chiamata sola. `null` è «non ancora chiesta», che è
  // diverso da «vuota»: la schermata mostra i suoi segnaposto finché il nucleo
  // non ha risposto, invece dello stato vuoto per un fotogramma.
  const [casa, setCasa] = useState<Casa | null>(null);
  // Le raccolte del lunedì. Vuoto e non `null`: qui «non ancora chieste» e
  // «questa settimana non ce ne sono» si disegnano uguale — niente ripiano — e
  // distinguerle costerebbe uno stato in più per non mostrare niente in due modi.
  const [settimana, setSettimana] = useState<Raccolta[]>([]);
  /**
   * Di quale brano corrente parla la `casa` che si ha in mano.
   *
   * «Riprendi dov'eri» fa partire la coda del nucleo, quindi deve nominare il
   * brano che quella coda ha davvero in mano. Questo riferimento è il confronto
   * che dice se i due sono ancora d'accordo, e sta fuori dallo stato perché
   * serve a **decidere** una richiesta, non a disegnare qualcosa.
   */
  const casaPerBrano = useRef<number | null>(null);
  /**
   * Il turno delle richieste della Home: vince l'ultima (`ultima.ts`).
   *
   * La Home si chiede da tre posti — entrando, a ogni cambio di brano, e dopo
   * una scansione o dei brani scaricati (`rileggiCasa`) — e le tre
   * risposte possono incrociarsi: quella chiesta durante la scansione, arrivata
   * dopo quella chiesta alla fine, rimetteva i ripiani di prima.
   */
  const [turnoCasa] = useState(vinceLUltima);

  /*
   * La Home, entrando.
   *
   * Fino a qui `ipc.casa()` stava **solo** dentro `caricaVista`, che parte sugli
   * eventi che cambiano la libreria — una scansione, dei brani scaricati. Non
   * all'apertura, e non entrando nella vista: quindi `casa` restava `null` per
   * sempre e la Home disegnava il vuoto sotto il proprio titolo. Era tutta
   * costruita — comando, stili, stringhe, indice — e non la chiedeva nessuno.
   *
   * I ripiani sono fatti di «di recente», che è vero solo nel momento in cui lo
   * si chiede: si rifà a ogni ritorno, come fa la vista degli artisti
   * (`artisti.ts`).
   *
   * `casa` non si azzera prima della richiesta. Tornando alla Home i ripiani di
   * prima restano sullo schermo finché non arrivano i nuovi — che è anche il
   * motivo per cui il segnaposto di `CasaFinta` si vede una volta sola, la
   * prima.
   */
  useEffect(() => {
    if (vista !== "home" || cercando) return;
    // Subito, non nella risposta: questo effetto e quello qui sotto girano tutti
    // e due quando si entra nella vista, e senza questa riga il secondo vedrebbe
    // il segno di prima e chiederebbe la stessa cosa una seconda volta.
    casaPerBrano.current = lettore.leggi().brano?.id ?? null;
    let annullato = false;
    const mio = turnoCasa();
    ipc
      .casa()
      .then((arrivata) => {
        if (annullato || !mio()) return;
        // E adesso il segno vero: di quale brano corrente parla la Home che si
        // ha in mano. All'avvio lo stato di riproduzione arriva per conto suo, e
        // senza questa riga il suo primo colpo sembrerebbe un cambio di brano —
        // cioè una seconda richiesta nel momento in cui la finestra ha già tutto
        // il resto da fare.
        casaPerBrano.current = arrivata.riprendi?.id ?? null;
        setCasa(arrivata);
      })
      .catch(segnalaErrore);
    return () => {
      annullato = true;
    };
    // Solo entrando: il brano corrente lo segue l'effetto «restando».
  }, [vista, cercando, segnalaErrore, lettore, turnoCasa]);

  /*
   * Le raccolte del lunedì, entrando nella Home.
   *
   * In una chiamata sua e non dentro `ipc.casa()`, che è il comando aggregato:
   * `casa` si rifà a ogni cambio di brano corrente — è la riga «riprendi dov'eri»
   * che deve restare vera — e la generazione delle raccolte apre una
   * transazione. Metterla lì dentro vorrebbe dire aprirne una a ogni cambio di
   * brano per riscoprire ogni volta che il lunedì c'è già.
   *
   * Non dipende dal brano corrente proprio per questo: entra nella vista, chiede
   * una volta, e per il resto della settimana la risposta è la stessa.
   *
   * «Una volta» però va fatto succedere, e le dipendenze da sole non bastano:
   * `vista` e `cercando` cambiano decine di volte in una sera — ogni giro fra
   * Home, Album e Artisti, ogni apertura e chiusura della ricerca — e ognuno
   * rifaceva la domanda per intero. Il lunedì chiesto si ricorda qui: finché è
   * lo stesso, la risposta che si ha in mano è già quella giusta.
   */
  const settimanaChiesta = useRef<number | null>(null);
  useEffect(() => {
    if (vista !== "home" || cercando) return;
    // Lo stesso lunedì che il nucleo calcolerebbe, e con lo stesso fuso: qui
    // serve solo a riconoscere che è cambiato, e a mezzanotte di domenica
    // cambia da sé senza che nessuno debba svegliare niente.
    const adesso = new Date();
    const giorno = (adesso.getDay() + 6) % 7;
    const lunedi = new Date(
      adesso.getFullYear(),
      adesso.getMonth(),
      adesso.getDate() - giorno,
    ).getTime();
    if (settimanaChiesta.current === lunedi) return;

    let annullato = false;
    ipc
      .settimana()
      .then((arrivate) => {
        if (annullato) return;
        // Si segna **dopo** la risposta, non prima della domanda: una chiamata
        // fallita deve poter essere rifatta entrando di nuovo nella Home.
        settimanaChiesta.current = lunedi;
        setSettimana(arrivate);
      })
      .catch(segnalaErrore);
    return () => {
      annullato = true;
    };
  }, [vista, cercando, segnalaErrore]);

  /*
   * E la Home, restando.
   *
   * «Riprendi dov'eri» fa partire **la coda del nucleo**, non il brano che ha
   * scritto sopra: se i due divergono la riga mente. E divergono appena si
   * preme qualcosa senza uscire dalla Home — la coda cambia brano corrente,
   * `casa` no.
   *
   * `trascurati` si conserva attraverso il rinfresco. Quel ripiano arriva da un
   * ordinamento casuale: rimescolarsi entrando nella vista va bene ed è il
   * senso di un ripiano di riscoperta, rimescolarsi mentre lo si sta guardando
   * vuol dire che la copertina sotto il dito non è più quella che si stava per
   * premere.
   */
  useEffect(() => {
    if (vista !== "home" || cercando) return;
    // Le risposte arrivate dopo che si è usciti dalla Home si buttano; fra due
    // brani di fila, quella del primo la butta il turno.
    let annullato = false;
    const segui = () => {
      const branoCorrente = lettore.leggi().brano?.id ?? null;
      // Il confronto sta in un riferimento e non fra le dipendenze: mettendoci
      // `casa` questo effetto girerebbe di nuovo a ogni sua risposta, e basterebbe
      // che la coda salvata sul database e quella del motore fossero d'accordo un
      // istante dopo invece che subito per farne un giro senza fine. Lo stesso
      // confronto lascia passare in silenzio tutto quel che dell'archivio non è
      // il brano: il volume, la pausa, l'equalizzatore.
      if (casaPerBrano.current === branoCorrente) return;
      casaPerBrano.current = branoCorrente;
      const mio = turnoCasa();
      ipc
        .casa()
        .then((arrivata) => {
          if (annullato || !mio()) return;
          setCasa((prima) =>
            prima ? { ...arrivata, trascurati: prima.trascurati } : arrivata,
          );
        })
        .catch(segnalaErrore);
    };
    segui();
    const smetti = lettore.iscrivi(segui);
    return () => {
      annullato = true;
      smetti();
    };
  }, [lettore, vista, cercando, segnalaErrore, turnoCasa]);

  /*
   * La Home dopo una scansione o dei brani scaricati: i suoi ripiani sono
   * fatti di «di recente», e quel che è appena entrato in libreria li cambia
   * tutti e quattro. Vince l'ultima: vedi `turnoCasa`.
   */
  const rileggiCasa = useCallback(() => {
    const mio = turnoCasa();
    ipc
      .casa()
      .then((arrivata) => {
        if (!mio()) return;
        casaPerBrano.current = arrivata.riprendi?.id ?? null;
        setCasa(arrivata);
      })
      .catch((e: unknown) => {
        if (mio()) segnalaErrore(e);
      });
  }, [segnalaErrore, turnoCasa]);

  return { casa, settimana, setSettimana, rileggiCasa };
}
