/**
 * Una finestra di righe disegnate sopra un elenco che ne ha diciottomila.
 *
 * # Perché una finestra e non l'elenco intero
 *
 * Perché l'elenco intero non entra in una finestra di Windows, e non per modo di
 * dire. `usePagine` portava le righe a duecento per volta e non le buttava mai:
 * scorrendo la vista Brani fino in fondo su una libreria vera — 18.534 brani —
 * il disegno arrivava a 18.534 `.riga`, e ognuna porta un `<img>`, cinque
 * bottoni-stella, il cuore e l'indice. Sono oltre **duecentomila nodi DOM** in
 * un processo di rendering solo, ed è il conto che si leggeva nel `WorkingSet`
 * dei tre processi WebView2: 114, 125 e 137 MB su 521 MB d'albero.
 *
 * La cura non è tenere meno righe in mano — quello è il mestiere di `rada.ts` —
 * ma disegnarne solo quelle che si vedono. Sopra e sotto si mette un
 * riempitivo alto quanto le righe che non ci sono (`.paglia`), così la barra di
 * scorrimento resta lunga quanto l'elenco vero e nessuno si accorge che sotto
 * il dito ci sono trenta elementi invece di diciottomila.
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
 * «Davvero disegnata» vuol dire anche **vera**: in una lista rada le righe che
 * non sono ancora arrivate sono scheletri, e chi le disegna dice con
 * `selettoreMisura` quali righe valgono come metro. Gli scheletri prendono poi
 * l'altezza misurata, ed è quel che toglie il salto all'arrivo dei dati.
 *
 * # Le griglie
 *
 * Una griglia di schede è una finestra di **righe di schede**. Le colonne non le
 * decide questo modulo più di quanto decida l'altezza: le decide il foglio con
 * `repeat(auto-fill, minmax(…))`, e qui si contano le tracce che il motore ne ha
 * ricavato (`colonneDa` in `finestra.ts`). Il passo è l'altezza di una scheda
 * più lo spazio fra due righe, che le griglie hanno e gli elenchi no: per questo
 * chi disegna una griglia toglie `scarto` dall'altezza delle sue due paglie, che
 * stanno in una riga della griglia loro e si portano dietro uno spazio ciascuna.
 *
 * # La memoria, e perché sta qui
 *
 * Le viste della libreria restano montate sotto `<Activity>` quando se ne esce,
 * e lo scorrevole — `.dentro` — è uno per tutte. Tornando a una vista il suo
 * scorrimento va rimesso, e deve esserlo **prima** che la finestra si calcoli: se
 * la rimettesse chi sta sopra, i suoi effetti girerebbero dopo questi, e per un
 * fotogramma la vista mostrerebbe le righe dello scorrimento di un'altra.
 * Gli effetti di un `<Activity>` nascosto si smontano e rinascono quando torna
 * visibile: è quella rinascita che rimette lo scorrimento ricordato.
 *
 * # Perché il modulo sta qui e non dentro `App.tsx`
 *
 * Perché lo consumano anche il pannello «Cartelle» e la coda, che hanno lo
 * stesso problema con un albero appiattito e con una colonna stretta. Una
 * seconda copia sarebbe due calcoli di `cima` da tenere d'accordo, e il difetto
 * di un `cima` sbagliato è muto: la finestra resta nel posto giusto finché
 * l'impaginazione non cambia.
 *
 * # Quel che questo modulo **non** fa
 *
 * Non sa niente di richieste: le pagine le chiede chi disegna, con la finestra
 * che riceve da qui.
 */
import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";

import { colonneDa, finestraDa, larghezzaColonnaDa, righeDiGriglia } from "./finestra";
import { comeScorrere } from "./transizione";

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
  /**
   * Quante **righe** stanno sopra la finestra: l'altezza della prima `.paglia`,
   * in passi. In un elenco è `primo`; in una griglia le righe di schede.
   */
  sopra: number;
  /** Quante righe stanno sotto: l'altezza della seconda `.paglia`, in passi. */
  sotto: number;
  /**
   * Il primo indice che si vede davvero, senza il margine. Facoltativo qui
   * perché una griglia disegnata per intera (lo Studio) non ha una finestra:
   * vedi [`Virtuale`].
   */
  visibileDa?: number | undefined;
  /**
   * Quante **righe** entrano nello scorrevole, senza margine: il passo di
   * PagSu e PagGiù in `fuoco.ts`. Facoltativo per la stessa ragione.
   */
  visibili?: number | undefined;
}

/** La finestra, il metro con cui è stata calcolata, e il modo di spostarla. */
export interface Virtuale extends Finestra {
  /**
   * Lo scorrevole si è mosso almeno una volta.
   *
   * Da lì in poi una riga che nasce non sta **entrando**: è la finestra che si
   * è spostata, e il nodo è stato riciclato per un'altra riga. Chi disegna lo
   * scrive come `data-scorso` sul contenitore, e il compilatore delle skin
   * toglie l'entrata e la cascata sotto quell'attributo — altrimenti ogni
   * colpo di rotella farebbe rientrare trenta righe una dopo l'altra.
   */
  scorso: boolean;
  /**
   * Il passo misurato, in pixel: l'altezza di una riga, più lo spazio fra due
   * righe in una griglia.
   *
   * Sta nel contratto perché chi disegna **deve** averlo: le due `.paglia` sono
   * alte `sopra * altezza` e `sotto * altezza`, e nessun altro qui sa quanto
   * misura una riga.
   */
  altezza: number;
  /**
   * Lo spazio fra due righe di una griglia, in pixel; zero in un elenco.
   *
   * Una paglia in una griglia occupa una riga sua, e la griglia ci mette dopo
   * uno spazio: la paglia è alta `sopra * altezza - scarto` perché la prima
   * scheda disegnata cada esattamente dove cadrebbe senza finestra.
   */
  scarto: number;
  /** Quante colonne: uno in un elenco, quelle contate in una griglia. */
  colonne: number;
  /**
   * `altezza` viene da una riga vera e non dal ripiego.
   *
   * Lo guarda chi disegna scheletri: in una griglia il ripiego da quaranta
   * pixel non somiglia a una scheda, e uno scheletro alto quaranta sarebbe il
   * salto che gli scheletri esistono per togliere. Finché è falso, lo scheletro
   * si tiene la sua altezza naturale.
   */
  misurata: boolean;
  /** Quanto è larga una colonna della griglia, o `null` se non si sa. */
  larghezzaColonna: number | null;
  /**
   * Il primo indice che si vede davvero, senza il margine.
   *
   * Dice **cosa** si sta guardando: l'indice alfabetico degli artisti accende
   * la lettera di questo.
   */
  visibileDa: number;
  /** Quante righe entrano nello scorrevole: vedi `visibili` in [`Finestra`]. */
  visibili: number;
  /**
   * Porta lo scorrevole all'inizio della riga che contiene l'indice.
   *
   * Serve al fuoco a rotazione (`fuoco.ts`): una riga fuori dalla finestra non
   * ha un nodo da focalizzare, e va prima portata dentro. `comportamento`
   * `"smooth"` per chi salta lontano con un gesto — l'indice alfabetico —, e
   * diventa un salto col movimento ridotto, del sistema o dell'app
   * (`comeScorrere` in `transizione.ts`).
   */
  scorriA: (indice: number, comportamento?: ScrollBehavior) => void;
}

/**
 * L'indice scritto su un nodo (`data-indice`), o il ripiego.
 *
 * Chi lo scrive può disegnare righe fuori ordine — l'elenco dei brani tiene
 * montata la riga col fuoco anche quando esce dalla finestra — e allora la
 * posizione nel DOM non dice più quale riga sia. Chi non lo scrive (la coda, le
 * cartelle) disegna solo la finestra, e la posizione basta.
 */
export function indiceDi(nodo: HTMLElement, ripiego: number): number {
  const scritto = nodo.dataset.indice;
  if (scritto === undefined) return ripiego;
  const n = Number(scritto);
  return Number.isFinite(n) ? n : ripiego;
}

/** Lo stato della finestra fra un calcolo e l'altro. */
interface StatoFinestra {
  primo: number;
  ultimo: number;
  visibileDa: number;
  visibili: number;
  altezza: number;
  scarto: number;
  colonne: number;
  larghezzaColonna: number | null;
  misurata: boolean;
  scorso: boolean;
}

/**
 * Segue lo scorrimento e dice quali righe disegnare.
 *
 * `contenitore` è lo **scorrevole**, cioè l'elemento con `overflow-y: auto` — in
 * `App.tsx` è `.dentro`. `ancora` è l'elenco dentro di esso: serve a sapere a
 * quale altezza comincia, perché fra la cima dello scorrevole e la prima riga c'è
 * quel che la pagina ha messo sopra (avvisi, note, intestazioni) e quel numero
 * non è una costante. In una griglia l'ancora è la griglia stessa, e le colonne
 * si contano su di lei.
 *
 * `selettoreRiga` esiste perché il modulo è generico: qui le righe sono `.riga`,
 * nel pannello «Cartelle» sono `.nodo-cartella`, nelle griglie `.scheda`. È il
 * solo punto in cui questo modulo sa qualcosa del markup di chi lo usa, ed è un
 * parametro proprio per restare il solo.
 */
export function useVirtuale(opzioni: {
  /** Quante righe ha l'elenco in tutto, disegnate o no (in una griglia: schede). */
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
  /**
   * Quali righe valgono come metro. Di serie le stesse di `selettoreRiga`; una
   * lista rada passa le righe vere, non gli scheletri.
   */
  selettoreMisura?: string | undefined;
  /** L'ancora è una griglia a colonne: vedi «Le griglie» nella nota del modulo. */
  griglia?: boolean | undefined;
  /**
   * Ricorda lo scorrimento e lo rimette quando la vista torna visibile: vedi
   * «La memoria» nella nota del modulo. Solo per chi sta sotto un `<Activity>`.
   */
  memoria?: boolean | undefined;
  /**
   * L'identità di quel che si mostra: cambiandola si torna in cima. Un altro
   * ordinamento è un altro elenco, e lo scorrimento di prima non ne è un posto.
   */
  chiave?: string | number | undefined;
}): Virtuale {
  const {
    totale,
    contenitore,
    ancora,
    altezzaDiRipiego = ALTEZZA_DI_RIPIEGO,
    margine = MARGINE,
    selettoreRiga = ".riga",
    selettoreMisura,
    griglia = false,
    memoria = false,
    chiave,
  } = opzioni;

  const [stato, setStato] = useState<StatoFinestra>(() => ({
    primo: 0,
    ultimo: 0,
    visibileDa: 0,
    visibili: 1,
    altezza: altezzaDiRipiego,
    scarto: 0,
    colonne: 1,
    larghezzaColonna: null,
    misurata: false,
    scorso: false,
  }));

  // In un ref oltre che nello stato: il calcolo gira dentro un
  // `requestAnimationFrame` e deve leggere i valori di **adesso**, non quelli
  // che la chiusura aveva quando l'ascolto è stato registrato.
  const statoRef = useRef(stato);
  statoRef.current = stato;
  /** Dove comincia la prima riga, dentro lo scorrevole. */
  const cima = useRef(0);
  /** Lo scorrimento di quando la vista era a schermo l'ultima volta. */
  const ricordato = useRef<number | null>(null);
  /** La chiave con cui lo scorrimento ricordato è stato preso. */
  const chiaveVista = useRef(chiave);

  const calcola = useCallback(() => {
    const scorrevole = contenitore.current;
    const elenco = ancora.current;
    if (scorrevole === null || elenco === null) return;
    const finora = statoRef.current;

    // ── 0. le colonne ───────────────────────────────────────────────────────
    // Solo in una griglia, e dal valore **calcolato**: vedi `colonneDa`. Un
    // valore non risolto tiene il numero di prima invece di crollare a uno.
    let colonne = 1;
    let scarto = 0;
    let larghezzaColonna = finora.larghezzaColonna;
    if (griglia) {
      const stile = getComputedStyle(elenco);
      colonne = colonneDa(stile.gridTemplateColumns) ?? finora.colonne;
      larghezzaColonna =
        larghezzaColonnaDa(stile.gridTemplateColumns) ?? finora.larghezzaColonna;
      const spazio = Number.parseFloat(stile.rowGap);
      scarto = Number.isFinite(spazio) ? spazio : 0;
    }

    // ── 1. il metro ─────────────────────────────────────────────────────────
    // La prima riga **davvero disegnata**, non la prima dell'elenco: quella può
    // essere la diecimillesima e non esistere nel DOM. E fra quelle disegnate,
    // la prima vera: uno scheletro misura quel che gli si dice di misurare.
    const prima = elenco.querySelector<HTMLElement>(selettoreRiga);
    const metro =
      selettoreMisura === undefined
        ? prima
        : elenco.querySelector<HTMLElement>(selettoreMisura);
    // `getBoundingClientRect` e non `offsetHeight`: il secondo arrotonda
    // all'intero, e mezzo pixel per riga su trenta righe sposta la finestra di
    // una riga e mezza.
    const rettMetro = metro === null ? null : metro.getBoundingClientRect();
    const misurata = rettMetro !== null && rettMetro.height > 0;
    const altezza = misurata ? rettMetro.height + scarto : finora.altezza;

    // ── 2. la cima ──────────────────────────────────────────────────────────
    // Dalla riga disegnata, quando ce n'è una: il suo bordo alto meno le righe
    // che la precedono dà la cima **esatta**, intestazione di colonna compresa,
    // e si corregge da sé se quel che sta sopra cambia altezza. Quale riga sia
    // lo dice `data-indice` quando c'è (vedi `indiceDi`); senza righe si ripiega
    // sull'ancora, che è la cima a meno di quell'intestazione — e il `margine`
    // la assorbe.
    const rettScorrevole = scorrevole.getBoundingClientRect();
    if (prima !== null) {
      const rettPrima =
        prima === metro && rettMetro !== null ? rettMetro : prima.getBoundingClientRect();
      const riga = Math.floor(indiceDi(prima, finora.primo) / colonne);
      cima.current =
        rettPrima.top - rettScorrevole.top + scorrevole.scrollTop - riga * altezza;
    } else {
      cima.current =
        elenco.getBoundingClientRect().top - rettScorrevole.top + scorrevole.scrollTop;
    }

    // ── 3. la finestra ──────────────────────────────────────────────────────
    // In righe, e poi in indici: in un elenco le due cose coincidono.
    const f = finestraDa({
      scorrimento: scorrevole.scrollTop,
      cima: cima.current,
      altezzaVista: scorrevole.clientHeight,
      passo: altezza,
      totale: righeDiGriglia(totale, colonne),
      margine,
    });
    const primo = Math.min(f.primo * colonne, totale);
    const ultimo = Math.min(f.ultimo * colonne, totale);
    const visibileDa = Math.min(f.visibileDa * colonne, Math.max(totale - 1, 0));
    // Le righe intere che entrano nello schermo, almeno una: PagGiù deve
    // muoversi anche in uno scorrevole più basso di una riga.
    const visibili = Math.max(Math.floor(scorrevole.clientHeight / altezza), 1);
    // Per sempre, una volta acceso: anche tornando in cima le righe che
    // rinascono sono riciclate, non nuove.
    const scorso = finora.scorso || scorrevole.scrollTop > 0;

    // Solo quando qualcosa è cambiato di almeno una riga. Gli indici sono interi
    // ed è questo a rendere il confronto giusto: senza, un pixel di rotella
    // rifarebbe lo stato — e quindi il disegno — sessanta volte al secondo per
    // mostrare le stesse trenta righe.
    if (
      primo === finora.primo &&
      ultimo === finora.ultimo &&
      visibileDa === finora.visibileDa &&
      visibili === finora.visibili &&
      altezza === finora.altezza &&
      scarto === finora.scarto &&
      colonne === finora.colonne &&
      larghezzaColonna === finora.larghezzaColonna &&
      (misurata || finora.misurata) === finora.misurata &&
      scorso === finora.scorso
    ) {
      return;
    }
    setStato({
      primo,
      ultimo,
      visibileDa,
      visibili,
      altezza,
      scarto,
      colonne,
      larghezzaColonna,
      // Una volta vera, resta: un disegno di soli scheletri non disfa la misura.
      misurata: misurata || finora.misurata,
      scorso,
    });
  }, [totale, margine, selettoreRiga, selettoreMisura, griglia, contenitore, ancora]);

  /*
   * La memoria: rimette lo scorrimento ricordato quando gli effetti nascono.
   *
   * Prima dell'effetto della finestra, ed è l'ordine che conta: gli effetti di
   * uno stesso componente girano nell'ordine in cui sono scritti, quindi il
   * calcolo qui sotto legge lo scorrimento già rimesso. Nasce al montaggio — e
   * lì porta in cima, perché lo scorrevole è condiviso e potrebbe essere ancora
   * dove l'aveva lasciato un'altra vista — e rinasce ogni volta che l'`<Activity>`
   * attorno torna visibile.
   */
  useLayoutEffect(() => {
    if (!memoria) return;
    const scorrevole = contenitore.current;
    if (scorrevole === null) return;
    scorrevole.scrollTop = ricordato.current ?? 0;
  }, [memoria, contenitore]);

  /*
   * Una chiave nuova riporta in cima, e dimentica lo scorrimento di prima.
   *
   * Anche se è cambiata mentre la vista era nascosta: alla rinascita il
   * riferimento ha ancora la chiave vecchia, e il confronto lo vede.
   */
  useLayoutEffect(() => {
    if (chiaveVista.current === chiave) return;
    chiaveVista.current = chiave;
    ricordato.current = 0;
    const scorrevole = contenitore.current;
    if (scorrevole !== null) scorrevole.scrollTop = 0;
  }, [chiave, contenitore]);

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
   * La memoria invece si scrive a ogni evento: è una lettura sola, e quella che
   * conta è l'ultima prima che la vista se ne vada.
   *
   * **Il `ResizeObserver`** guarda due cose diverse per due ragioni diverse: lo
   * scorrevole perché `clientHeight` decide **quante** righe servono — e in una
   * griglia la sua larghezza decide quante colonne — e il metro perché la sua
   * altezza decide **quali**. La seconda è quella che rende innocuo il
   * `min-height` di `.riga` — una riga cresciuta oltre il minimo si misura, non
   * si indovina — e il ritmo largo di `data-density='spacious'`.
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
      if (memoria) ricordato.current = scorrevole.scrollTop;
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
    const metro = ancora.current?.querySelector<HTMLElement>(
      selettoreMisura ?? selettoreRiga,
    );
    if (metro !== null && metro !== undefined) osservatore.observe(metro);

    return () => {
      scorrevole.removeEventListener("scroll", alFotogramma);
      osservatore.disconnect();
      if (inAttesa !== 0) window.cancelAnimationFrame(inAttesa);
    };
  }, [calcola, stato, contenitore, ancora, selettoreRiga, selettoreMisura, memoria]);

  /*
   * Finché non c'è una misura vera, si riprova a ogni disegno.
   *
   * In una lista rada il primo disegno ha solo scheletri: l'effetto qui sopra
   * gira, non trova un metro e tiene il ripiego. Le righe vere arrivano dopo,
   * con un disegno di chi possiede l'elenco che non cambia né `stato` né
   * `calcola` — e quindi non rifà l'effetto. Senza questo giro la misura
   * aspettava il primo scorrimento: fino ad allora le paglie erano alte
   * `40px` a riga su righe da 44, la barra un decimo più corta della lista, e
   * trascinarla in fondo a cinquantamila brani portava alla riga 48.200 — poi la
   * barra si allungava sotto il pollice (collaudo della Fase 3). Una volta
   * misurata, `misurata` resta vera e questo effetto non fa più niente; da lì
   * in poi la segue il `ResizeObserver` sul metro.
   */
  useLayoutEffect(() => {
    if (!statoRef.current.misurata) calcola();
  });

  const scorriA = useCallback(
    (indice: number, comportamento?: ScrollBehavior) => {
      const scorrevole = contenitore.current;
      if (scorrevole === null) return;
      // `scrollTop` e non `scrollIntoView`: il secondo muove **tutti** gli
      // antenati scorrevoli e, su un nodo che non è disegnato, non ha niente da
      // muovere. Qui la posizione si sa per calcolo anche quando la riga non
      // esiste nel DOM, che è precisamente il caso per cui questa funzione c'è.
      const { colonne, altezza } = statoRef.current;
      const alto = Math.max(cima.current + Math.floor(indice / colonne) * altezza, 0);
      if (comportamento === undefined) scorrevole.scrollTop = alto;
      else scorrevole.scrollTo({ top: alto, behavior: comeScorrere(comportamento) });
    },
    [contenitore],
  );

  // Memoizzato: l'oggetto entra nelle dipendenze di `useFuoco` e degli effetti
  // di chi disegna, e un'identità nuova a ogni giro li rifarebbe tutti.
  return useMemo(() => {
    // Stretti dentro il totale di **adesso**: se l'elenco si è accorciato e la
    // finestra non si è ancora ricalcolata, un disegno con la finestra di prima
    // disegnerebbe scheletri oltre la fine.
    const ultimo = Math.min(stato.ultimo, totale);
    const primo = Math.min(stato.primo, ultimo);
    const colonne = stato.colonne;
    return {
      primo,
      ultimo,
      sopra: Math.floor(primo / colonne),
      sotto: Math.max(righeDiGriglia(totale, colonne) - Math.ceil(ultimo / colonne), 0),
      altezza: stato.altezza,
      scarto: stato.scarto,
      colonne,
      larghezzaColonna: stato.larghezzaColonna,
      misurata: stato.misurata,
      visibileDa: Math.min(stato.visibileDa, Math.max(totale - 1, 0)),
      visibili: stato.visibili,
      scorso: stato.scorso,
      scorriA,
    };
  }, [stato, totale, scorriA]);
}
