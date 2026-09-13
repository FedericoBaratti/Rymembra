/**
 * Una finestra di righe disegnate sopra un elenco che ne ha diciottomila.
 *
 * # Perché una finestra e non l'elenco intero
 *
 * Perché l'elenco intero non entra in una finestra di Windows, e non per modo di
 * dire. `usePagine` porta le righe a duecento per volta e non le butta mai:
 * scorrendo la vista Brani fino in fondo su una libreria vera — 18.534 brani —
 * il disegno arrivava a 18.534 `.riga`, e ognuna porta un `<img>`, cinque
 * bottoni-stella, il cuore e l'indice. Sono oltre **duecentomila nodi DOM** in
 * un processo di rendering solo, ed è il conto che si leggeva nel `WorkingSet`
 * dei tre processi WebView2: 114, 125 e 137 MB su 521 MB d'albero.
 *
 * La cura non è tenere meno righe — quelle servono, ed è il mestiere di
 * `pagine.ts`, che **non si tocca** — ma disegnarne solo quelle che si vedono.
 * Sopra e sotto si mette un riempitivo alto quanto le righe che non ci sono
 * (`.paglia`), così la barra di scorrimento resta lunga com'era e nessuno si
 * accorge che sotto il dito ci sono trenta elementi invece di diciottomila.
 *
 * # Perché l'altezza si misura invece di saperla
 *
 * Perché non la decide questo modulo. `data-density='spacious'` allarga la riga,
 * il ritmo della skin la riscrive, e `.riga` nel foglio dichiara `min-height:
 * 40px` e non `height`: cioè «questa è la minima», non «questa è l'altezza».
 * Un numero cablato qui sarebbe giusto in una configurazione e sbagliato nelle
 * altre, e l'errore non si vedrebbe come un errore: si vedrebbe come una
 * finestra che scorrendo salta righe, o ne disegna il doppio del necessario.
 *
 * Quindi si misura la **prima riga davvero disegnata**, e si rimisura con un
 * `ResizeObserver` — sullo scorrevole, da cui viene `clientHeight`, e sulla riga
 * stessa, che è il metro. Il 40 è solo il ripiego del primo disegno, quando di
 * righe non ce n'è ancora nessuna da misurare.
 *
 * # Perché il modulo sta qui e non dentro `App.tsx`
 *
 * Perché lo consuma anche il pannello «Cartelle», che ha lo stesso problema con
 * un albero appiattito invece di un elenco piatto. Una seconda copia sarebbe due
 * calcoli di `cima` da tenere d'accordo, e il difetto di un `cima` sbagliato è
 * muto: la finestra resta nel posto giusto finché l'impaginazione non cambia.
 *
 * # Quel che questo modulo **non** fa
 *
 * Non tocca la paginazione e non sa niente di richieste: la sentinella
 * dell'`IntersectionObserver` di `pagine.ts` resta dov'è, **fuori**
 * dall'elenco. Può restarci perché la `.paglia` inferiore conserva l'altezza
 * vera delle righe non disegnate: il fondo del documento è dove era prima,
 * quindi il `rootMargin: 600px` di `pagine.ts:154` scatta allo stesso pixel di
 * prima. Senza la paglia — o con una paglia alta zero — la sentinella sarebbe in
 * vista dal primo disegno e l'elenco si scaricherebbe tutto d'un fiato.
 */
import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";

/**
 * L'altezza di riga da usare finché non c'è una riga da misurare.
 *
 * È il minimo che `.riga` dichiara nel foglio, e vale per un disegno solo: appena
 * la prima riga esiste, il numero vero lo dà lei — che può essere più alto.
 *
 * Non si legge dal CSS e non si può: `getComputedStyle` darebbe il minimo, non
 * l'altezza, ed è precisamente la differenza che questo modulo deve conoscere.
 */
const ALTEZZA_DI_RIPIEGO = 40;

/**
 * Quante righe in più disegnare oltre il bordo dello schermo, per lato.
 *
 * Otto e non zero perché lo scorrimento con la rotella salta decine di pixel per
 * volta e il calcolo arriva un fotogramma dopo: con margine zero si vedrebbe la
 * striscia di vuoto prima che le righe la riempiano. Otto e non ottanta perché
 * ogni riga in più è il costo che questo modulo esiste per non pagare.
 *
 * Il margine serve anche a un'altra cosa, ed è il motivo per cui non è due: la
 * `cima` si ricava dall'ancora, che può disegnare qualcosa **prima** della prima
 * riga — l'intestazione di colonna dell'elenco, alta una trentina di pixel.
 * Quello scarto vale meno di una riga e il margine lo assorbe senza che nessuno
 * debba dichiarare quanto è alta un'intestazione.
 */
const MARGINE = 8;

/** La fetta di elenco da disegnare, e quanto vuoto tenere ai due lati. */
export interface Finestra {
  /** Primo indice disegnato, compreso. */
  primo: number;
  /**
   * Indice **dopo** l'ultimo disegnato: `righe.slice(primo, ultimo)`.
   *
   * Esclusivo e non compreso, per una ragione pratica: così `sotto` è
   * `totale - ultimo` senza un `+1` che qualcuno prima o poi scrive nel verso
   * sbagliato, e la fetta si prende con `slice` invece che a mano.
   */
  ultimo: number;
  /** Quante righe stanno sopra la finestra: l'altezza della prima `.paglia`. */
  sopra: number;
  /** Quante righe stanno sotto: l'altezza della seconda `.paglia`. */
  sotto: number;
}

/** La finestra, il metro con cui è stata calcolata, e il modo di spostarla. */
export interface Virtuale extends Finestra {
  /**
   * L'altezza di riga misurata, in pixel.
   *
   * Sta nel contratto perché chi disegna **deve** averla: le due `.paglia` sono
   * alte `sopra * altezza` e `sotto * altezza`, e nessun altro qui sa quanto
   * misura una riga.
   */
  altezza: number;
  /**
   * Porta lo scorrevole all'inizio di una riga.
   *
   * Serve al fuoco a rotazione (`fuoco.ts`): una riga fuori dalla finestra non
   * ha un nodo da focalizzare, e va prima portata dentro.
   */
  scorriA: (indice: number) => void;
}

/** Fra due estremi, compresi. */
function stretto(valore: number, minimo: number, massimo: number): number {
  return Math.min(Math.max(valore, minimo), massimo);
}

/**
 * Segue lo scorrimento e dice quali righe disegnare.
 *
 * `contenitore` è lo **scorrevole**, cioè l'elemento con `overflow-y: auto` — in
 * `App.tsx` è `.dentro`. `ancora` è l'elenco dentro di esso: serve a sapere a
 * quale altezza comincia, perché fra la cima dello scorrevole e la prima riga c'è
 * quel che la pagina ha messo sopra (avvisi, note, intestazioni) e quel numero
 * non è una costante.
 *
 * `selettoreRiga` esiste perché il modulo è generico: qui le righe sono `.riga`,
 * nel pannello «Cartelle» sono `.nodo-cartella`. È il solo punto in cui questo
 * modulo sa qualcosa del markup di chi lo usa, ed è un parametro proprio per
 * restare il solo.
 */
export function useVirtuale(opzioni: {
  /** Quante righe ha l'elenco in tutto, disegnate o no. */
  totale: number;
  /** Lo scorrevole: `overflow-y: auto`. */
  contenitore: RefObject<HTMLElement | null>;
  /** L'elenco dentro lo scorrevole. */
  ancora: RefObject<HTMLElement | null>;
  /** Finché non c'è una riga da misurare. Di serie 40. */
  altezzaDiRipiego?: number | undefined;
  /** Righe in più per lato. Di serie 8. */
  margine?: number | undefined;
  /** Come si riconosce una riga disegnata. Di serie `.riga`. */
  selettoreRiga?: string | undefined;
}): Virtuale {
  const {
    totale,
    contenitore,
    ancora,
    altezzaDiRipiego = ALTEZZA_DI_RIPIEGO,
    margine = MARGINE,
    selettoreRiga = ".riga",
  } = opzioni;

  const [stato, setStato] = useState<{
    primo: number;
    ultimo: number;
    altezza: number;
  }>(() => ({ primo: 0, ultimo: 0, altezza: altezzaDiRipiego }));

  // In un ref oltre che nello stato: il calcolo gira dentro un
  // `requestAnimationFrame` e deve leggere i valori di **adesso**, non quelli
  // che la chiusura aveva quando l'ascolto è stato registrato.
  const statoRef = useRef(stato);
  statoRef.current = stato;
  /** Dove comincia la prima riga, dentro lo scorrevole. */
  const cima = useRef(0);

  const calcola = useCallback(() => {
    const scorrevole = contenitore.current;
    const elenco = ancora.current;
    if (scorrevole === null || elenco === null) return;

    const disegnatoDa = statoRef.current.primo;

    // ── 1. il metro ─────────────────────────────────────────────────────────
    // La prima riga **davvero disegnata**, non la prima dell'elenco: quella può
    // essere la diecimillesima e non esistere nel DOM.
    const prima = elenco.querySelector<HTMLElement>(selettoreRiga);
    const rettPrima = prima === null ? null : prima.getBoundingClientRect();
    // `getBoundingClientRect` e non `offsetHeight`: il secondo arrotonda
    // all'intero, e mezzo pixel per riga su trenta righe sposta la finestra di
    // una riga e mezza.
    const altezza =
      rettPrima !== null && rettPrima.height > 0
        ? rettPrima.height
        : statoRef.current.altezza;

    // ── 2. la cima ──────────────────────────────────────────────────────────
    // Dalla riga disegnata, quando ce n'è una: il suo bordo alto meno gli indici
    // che la precedono dà la cima **esatta**, intestazione di colonna compresa,
    // e si corregge da sé se quel che sta sopra cambia altezza. Senza righe si
    // ripiega sull'ancora, che è la cima a meno di quell'intestazione — e il
    // `margine` la assorbe.
    const rettScorrevole = scorrevole.getBoundingClientRect();
    cima.current =
      rettPrima !== null
        ? rettPrima.top -
          rettScorrevole.top +
          scorrevole.scrollTop -
          disegnatoDa * altezza
        : elenco.getBoundingClientRect().top -
          rettScorrevole.top +
          scorrevole.scrollTop;

    // ── 3. la finestra ──────────────────────────────────────────────────────
    const quante = Math.ceil(scorrevole.clientHeight / altezza) + 2 * margine;
    const primo = stretto(
      Math.floor((scorrevole.scrollTop - cima.current) / altezza) - margine,
      0,
      Math.max(totale - 1, 0),
    );
    const ultimo = Math.min(primo + quante, totale);

    // Solo quando qualcosa è cambiato di almeno una riga. Gli indici sono interi
    // ed è questo a rendere il confronto giusto: senza, un pixel di rotella
    // rifarebbe lo stato — e quindi il disegno — sessanta volte al secondo per
    // mostrare le stesse trenta righe.
    if (
      primo === statoRef.current.primo &&
      ultimo === statoRef.current.ultimo &&
      altezza === statoRef.current.altezza
    ) {
      return;
    }
    setStato({ primo, ultimo, altezza });
  }, [totale, margine, selettoreRiga, contenitore, ancora]);

  /**
   * Tutto quel che fa ricalcolare la finestra, in un effetto solo.
   *
   * Gira dopo ogni disegno in cui la finestra è cambiata, ed è lì che l'altezza
   * passa dal ripiego alla misura vera: al primo giro di righe non ce n'era
   * nessuna, al secondo sì. Il giro si ferma da sé, perché `calcola` non rifà lo
   * stato quando il conto torna uguale.
   *
   * **Lo scorrimento** è strozzato a un fotogramma: `scroll` arriva molto più
   * spesso di quanto lo schermo si ridisegni, e ogni arrivo vorrebbe tre
   * `getBoundingClientRect`, cioè tre impaginazioni forzate. Con un fotogramma in
   * sospeso si scarta tutto quel che arriva nel mezzo, e il calcolo non ci perde
   * niente: legge `scrollTop` quando tocca a lui, non quando l'evento è partito.
   *
   * **Il `ResizeObserver`** guarda due cose diverse per due ragioni diverse: lo
   * scorrevole perché `clientHeight` decide **quante** righe servono, e la prima
   * riga perché la sua altezza decide **quali**. La seconda è quella che rende
   * innocuo il `min-height` di `.riga` — una riga cresciuta oltre il minimo si
   * misura, non si indovina — e il ritmo largo di `data-density='spacious'`.
   *
   * Un effetto e non due perché l'ascolto e l'osservatore vogliono lo stesso
   * nodo e la stessa condizione d'esistenza: tenerli separati voleva dire due
   * guardie uguali, e due occasioni di agganciarne una sola.
   */
  useLayoutEffect(() => {
    calcola();

    const scorrevole = contenitore.current;
    if (scorrevole === null) {
      // Lo scorrevole è un **antenato**, e React attacca i riferimenti dei figli
      // prima di quelli dei genitori: se nascono nello stesso giro di disegno,
      // qui non c'è ancora. Un fotogramma dopo c'è, e `calcola` rifà lo stato,
      // che rifà passare da qui. Senza questo ripiego la finestra resterebbe a
      // zero righe per sempre, e in silenzio.
      const dopo = window.requestAnimationFrame(() => calcola());
      return () => window.cancelAnimationFrame(dopo);
    }

    let inAttesa = 0;
    const alFotogramma = () => {
      if (inAttesa !== 0) return;
      inAttesa = window.requestAnimationFrame(() => {
        inAttesa = 0;
        calcola();
      });
    };
    // Passivo: questo ascolto non chiamerà mai `preventDefault`, e dirlo al
    // motore è ciò che gli permette di far scorrere la pagina senza aspettare
    // che il gestore abbia finito.
    scorrevole.addEventListener("scroll", alFotogramma, { passive: true });

    const osservatore = new ResizeObserver(() => calcola());
    osservatore.observe(scorrevole);
    const prima = ancora.current?.querySelector<HTMLElement>(selettoreRiga);
    if (prima !== null && prima !== undefined) osservatore.observe(prima);

    return () => {
      scorrevole.removeEventListener("scroll", alFotogramma);
      osservatore.disconnect();
      if (inAttesa !== 0) window.cancelAnimationFrame(inAttesa);
    };
  }, [calcola, stato, contenitore, ancora, selettoreRiga]);

  const scorriA = useCallback(
    (indice: number) => {
      const scorrevole = contenitore.current;
      if (scorrevole === null) return;
      // `scrollTop` e non `scrollIntoView`: il secondo muove **tutti** gli
      // antenati scorrevoli e, su un nodo che non è disegnato, non ha niente da
      // muovere. Qui la posizione si sa per calcolo anche quando la riga non
      // esiste nel DOM, che è precisamente il caso per cui questa funzione c'è.
      scorrevole.scrollTop = Math.max(
        cima.current + indice * statoRef.current.altezza,
        0,
      );
    },
    [contenitore],
  );

  // Memoizzato: l'oggetto entra nelle dipendenze di `useFuoco` e degli effetti
  // di chi disegna, e un'identità nuova a ogni giro li rifarebbe tutti.
  return useMemo(
    () => ({
      primo: stato.primo,
      ultimo: stato.ultimo,
      sopra: stato.primo,
      sotto: Math.max(totale - stato.ultimo, 0),
      altezza: stato.altezza,
      scorriA,
    }),
    [stato, totale, scorriA],
  );
}
