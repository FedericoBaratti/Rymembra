/**
 * Il fuoco a rotazione: una fermata di tabulazione per un elenco intero.
 *
 * # Il conto che questo modulo chiude
 *
 * Ogni riga dell'elenco espone **sette** fermate di tabulazione: l'indice, le
 * cinque stelle, il cuore — e dentro una playlist anche la × che la toglie.
 * Sono sette per riga, e le righe su una libreria vera sono 18.534: oltre
 * **centomila pressioni di Tab** per attraversare l'elenco, se si arriva in
 * fondo. Non è scomodo, è assente: chi naviga da tastiera non può raggiungere né
 * la riga dopo né quel che viene **dopo l'elenco**, perché la strada per uscirne
 * è più lunga di quanto una persona stia a premere un tasto.
 *
 * La forma prevista per questo caso ha un nome ed è vecchia quanto le griglie:
 * **una** fermata di tabulazione per il componente, e dentro si gira con le
 * frecce. La riga attiva porta `tabIndex={0}`, tutte le altre `-1`, e i comandi
 * dentro le righe passano tutti a `-1` — non sono irraggiungibili, si
 * raggiungono con ←→ invece che con Tab. Da oltre centomila fermate a una.
 *
 * # Due assi, e perché non uno
 *
 * ↑↓ cambiano riga, ←→ girano fra i comandi **della riga col fuoco**. Un asse
 * solo — ↑↓ su tutto, comandi compresi — metterebbe nella stessa fila la riga
 * successiva e la terza stella della riga corrente, cioè due cose che non si
 * scelgono mai nello stesso momento: scorrere un elenco e votare un brano.
 *
 * # Perché lo scorrimento viene prima del fuoco
 *
 * Con una finestra virtuale (`virtuale.ts`) la riga a cui si vuole andare può non
 * avere un nodo nel DOM: premendo Fine su diciottomila brani la riga 18.533 non
 * è disegnata. `focus()` su un nodo che non c'è non fa niente e non dà errore —
 * il fuoco resta dov'era, o cade sul `<body>`, che è il modo peggiore di
 * sbagliare perché non si vede. Quindi prima si porta lo scorrevole al posto
 * giusto, si lascia che il disegno arrivi, e **poi** si focalizza: è un effetto
 * che passa di qui due volte, e la seconda trova il nodo.
 *
 * # Generico perché lo consuma anche il pannello «Cartelle»
 *
 * Come `virtuale.ts`: le righe qui sono `.riga`, là `.nodo-cartella`, e i due
 * selettori sono parametri proprio perché non ci siano due copie di questa
 * logica da tenere d'accordo.
 */
import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent, RefObject } from "react";

import type { Finestra } from "./virtuale";

/**
 * Per quanto aspettare che una riga entri nella finestra, prima di lasciar
 * perdere.
 *
 * Serve un tetto: se lo scorrevole è già in fondo e la riga chiesta resta fuori
 * — un elenco più corto di quanto si credeva, un'altezza appena rimisurata — il
 * fuoco in sospeso resterebbe in sospeso per sempre e proverebbe a prendersi la
 * scena a ogni disegno successivo, anche mezz'ora dopo.
 *
 * È un tempo e **non un numero di disegni**, ed è il punto: mentre un brano
 * suona `App` si ridisegna una ventina di volte al secondo per la posizione, e
 * un tetto di «quattro disegni» si esaurirebbe in duecento millisecondi — cioè
 * prima che lo scorrimento e il ricalcolo abbiano finito. Trecento millisecondi
 * sono venti fotogrammi: abbastanza per uno scorrimento e la rimisura, troppo
 * pochi perché qualcuno se ne accorga.
 */
const ATTESA_MS = 300;

/** Il fuoco dentro un elenco, e i modi di spostarlo. */
export interface Fuoco {
  /**
   * La riga che porta il fuoco: la **sola** con `tabIndex={0}`.
   *
   * È un indice nell'elenco intero, non nella finestra disegnata: la riga attiva
   * può essere fuori dallo schermo, e resta quella a cui Tab riporta.
   */
  attivo: number;
  /**
   * Porta il fuoco su una riga, scorrendo prima se è fuori dalla finestra.
   *
   * Lo chiamano le frecce, Inizio/Fine e `Alt+↑↓`: quest'ultimo sposta una riga
   * dentro una playlist e il fuoco deve seguire la riga spostata, non restare
   * sulla posizione.
   */
  vaiA: (indice: number) => void;
  /**
   * Registra che il fuoco è **già** arrivato su una riga, senza spostarlo.
   *
   * Lo chiama `onFocus` della riga. Serve perché il fuoco può arrivare senza
   * passare da qui — un clic, un Tab da fuori — e se `attivo` non lo seguisse la
   * riga col fuoco e la riga con `tabIndex={0}` sarebbero due righe diverse:
   * uscire e rientrare con Tab riporterebbe altrove.
   */
  segna: (indice: number) => void;
  /** Da appendere al contenitore delle righe: è lui che legge i tasti. */
  daTastiera: (e: KeyboardEvent<HTMLElement>) => void;
}

/**
 * Il fuoco a rotazione su un elenco finestrato.
 *
 * Un gestore solo, sul contenitore, e non uno per riga: le righe sono `memo` e
 * una prop nuova per riga con un'identità nuova a ogni disegno annullerebbe il
 * confronto, che è il secondo conto che questa release paga. La riga da cui
 * viene il tasto si ricava dal bersaglio dell'evento — `closest` — e la sua
 * posizione nella finestra dice l'indice.
 */
export function useFuoco(opzioni: {
  /** Quante righe ha l'elenco in tutto. */
  totale: number;
  /**
   * L'elenco dentro lo scorrevole.
   *
   * Lo scorrevole non serve: a muoverlo ci pensa `scorriA`, che lo conosce già.
   * Chiederlo anche qui sarebbe un secondo riferimento da tenere d'accordo col
   * primo per niente.
   */
  ancora: RefObject<HTMLElement | null>;
  /** Cos'è disegnato adesso: da `useVirtuale`. */
  finestra: Finestra;
  /** Da `useVirtuale`: porta lo scorrevole all'inizio di una riga. */
  scorriA: (indice: number) => void;
  /** Come si riconosce una riga. Di serie `.riga`. */
  selettoreRiga?: string | undefined;
  /** Come si riconosce un comando dentro la riga. Di serie `button`. */
  selettoreComando?: string | undefined;
  /** Invio sulla riga: di solito «suona». */
  onInvio?: ((indice: number) => void) | undefined;
  /** Spazio sulla riga: di solito «aggiungi alla selezione». */
  onSpazio?: ((indice: number) => void) | undefined;
}): Fuoco {
  const {
    totale,
    ancora,
    finestra,
    scorriA,
    selettoreRiga = ".riga",
    selettoreComando = "button",
    onInvio,
    onSpazio,
  } = opzioni;

  const [attivoStato, setAttivo] = useState(0);
  // Stretto in lettura e non con un effetto: l'elenco si accorcia — si cambia
  // vista, si toglie una riga da una playlist — e un effetto che corregge
  // `attivo` costerebbe un disegno in più per dire una cosa che si sa già.
  const attivo = stretto(attivoStato, totale);

  /** La riga che aspetta il fuoco, e da quando. */
  const inSospeso = useRef<{ indice: number; da: number } | null>(null);

  /** Le righe disegnate adesso, nell'ordine del DOM. */
  const righe = useCallback((): HTMLElement[] => {
    const elenco = ancora.current;
    if (elenco === null) return [];
    return Array.from(elenco.querySelectorAll<HTMLElement>(selettoreRiga));
  }, [ancora, selettoreRiga]);

  /**
   * La finestra di **adesso**, leggibile da una chiusura memoizzata.
   *
   * `vaiA` non può avere `finestra` fra le dipendenze: passerebbe di identità a
   * ogni scorrimento, e con lui le prop di `RigaBrano` — cioè il `memo` che
   * questa release esiste anche per difendere.
   */
  const finestraRef = useRef(finestra);
  finestraRef.current = finestra;

  const vaiA = useCallback(
    (indice: number) => {
      const dove = stretto(indice, totale);
      setAttivo(dove);
      const f = finestraRef.current;
      if (dove >= f.primo && dove < f.ultimo) {
        // Già disegnata: il fuoco ci va adesso, e non c'è niente da mettere in
        // sospeso. Metterlo in sospeso comunque sarebbe un errore silenzioso:
        // `setAttivo` con lo stesso valore non ridisegna — ↑ sulla prima riga,
        // ↓ sull'ultima — quindi l'effetto non girerebbe, la riga resterebbe in
        // sospeso, e al primo disegno altrui (un clic su un'altra riga, mezzo
        // minuto dopo) il fuoco salterebbe indietro da solo.
        inSospeso.current = null;
        righe()[dove - f.primo]?.focus();
        return;
      }
      inSospeso.current = { indice: dove, da: performance.now() };
    },
    [totale, righe],
  );

  const segna = useCallback((indice: number) => setAttivo(indice), []);

  /**
   * Porta il fuoco dove `vaiA` ha chiesto, appena la riga esiste.
   *
   * Senza elenco di dipendenze **di proposito**: deve girare dopo ogni disegno,
   * perché il disegno che fa comparire la riga è proprio quello che segue lo
   * scorrimento. Non fa niente quando non c'è niente in sospeso, che è
   * praticamente sempre.
   */
  useLayoutEffect(() => {
    const sospeso = inSospeso.current;
    if (sospeso === null) return;
    const { indice } = sospeso;
    if (indice >= finestra.primo && indice < finestra.ultimo) {
      inSospeso.current = null;
      righe()[indice - finestra.primo]?.focus();
      return;
    }
    if (performance.now() - sospeso.da > ATTESA_MS) {
      inSospeso.current = null;
      return;
    }
    // Prima lo scorrimento: vedi il docblock del modulo. Lo scorrimento fa
    // ricalcolare la finestra, la finestra rifà il disegno, e il disegno ripassa
    // da qui con la riga dentro. Ripeterlo a ogni disegno nel frattempo non
    // costa niente: `scorriA` scrive lo stesso `scrollTop` di prima.
    scorriA(indice);
  });

  const daTastiera = useCallback(
    (e: KeyboardEvent<HTMLElement>) => {
      // `Alt+↑↓` non è di qui: sposta la riga dentro una playlist, e il gestore
      // sta sulla riga. Intercettarlo significherebbe spostare il fuoco invece
      // di spostare la canzone.
      if (e.altKey || e.ctrlKey || e.metaKey) return;

      const bersaglio = e.target instanceof HTMLElement ? e.target : null;
      const riga = bersaglio?.closest<HTMLElement>(selettoreRiga) ?? null;
      if (riga === null) return;
      const disegnate = righe();
      const dentro = disegnate.indexOf(riga);
      if (dentro < 0) return;
      const indice = finestra.primo + dentro;

      /*
       * Preso **e fermato**.
       *
       * `preventDefault` non basta: le scorciatoie globali vivono su un ascolto
       * appeso a `window` che non guarda `defaultPrevented` (`tastiera.ts:345-379`),
       * e tre dei tasti di qui sono associati a un comando di trasporto —
       * Spazio è «pausa», ←→ sono «avanti/indietro di cinque secondi». Senza
       * fermare la propagazione, lo Spazio su una riga selezionerebbe **e**
       * metterebbe in pausa.
       *
       * Si ferma sul nodo invece di insegnare a `tastiera.ts` a guardare
       * `defaultPrevented` per la stessa ragione per cui lo fa `useFinestrella`
       * con Escape: il tasto lo ha consumato chi ha il fuoco, e chi ha il fuoco
       * è qui dentro.
       */
      const preso = () => {
        e.preventDefault();
        e.stopPropagation();
      };

      switch (e.key) {
        case "ArrowDown":
          preso();
          vaiA(indice + 1);
          return;
        case "ArrowUp":
          preso();
          vaiA(indice - 1);
          return;
        case "Home":
          preso();
          vaiA(0);
          return;
        case "End":
          preso();
          vaiA(totale - 1);
          return;
        case "ArrowRight":
        case "ArrowLeft": {
          // Consumato anche ai due capi, e non lasciato cadere sul trasporto:
          // una freccia che fa scorrere il brano **solo** quando i comandi della
          // riga sono finiti è una freccia che parte per sbaglio. Finché il fuoco
          // è dentro la griglia, ←→ sono della griglia.
          preso();
          // I comandi **visibili e accesi**: sotto i 560 px di contenitore le
          // stelle sono `display: none` e senza dispositivo audio l'indice è
          // spento. Un giro che si fermasse su quelli sembrerebbe rotto.
          const comandi = Array.from(
            riga.querySelectorAll<HTMLElement>(selettoreComando),
          ).filter(
            (n) =>
              n.offsetParent !== null &&
              !(n instanceof HTMLButtonElement && n.disabled),
          );
          // `-1` è la riga stessa: da lì ← non ha dove andare, e → porta al
          // primo comando. Non è un anello: arrivato in fondo si ferma, perché
          // un anello su sette comandi fa perdere il conto di dove si è.
          const fra = comandi.findIndex((n) => n === document.activeElement);
          const dove = fra + (e.key === "ArrowRight" ? 1 : -1);
          if (dove === -1) riga.focus();
          else comandi[dove]?.focus();
          return;
        }
        case "Enter":
        case " ":
          // La propagazione si ferma sempre, anche quando il tasto non è per
          // noi: su un comando lo consuma il comando — Invio su «Riproduci»
          // suona già — ma senza questa riga lo Spazio che vota una stella
          // metterebbe **anche** in pausa, perché il trasporto ascolta su
          // `window` e non sa niente di chi ha il fuoco.
          e.stopPropagation();
          // `preventDefault` no, però: su un bottone toglierebbe l'attivazione
          // nativa, cioè il clic che lo Spazio **è**.
          if (e.target !== riga) return;
          e.preventDefault();
          if (e.key === "Enter") onInvio?.(indice);
          else onSpazio?.(indice);
          return;
        default:
          return;
      }
    },
    [finestra.primo, righe, selettoreRiga, selettoreComando, totale, vaiA, onInvio, onSpazio],
  );

  // Memoizzato per la stessa ragione di `useVirtuale`: l'oggetto entra nelle
  // dipendenze di chi disegna.
  return useMemo(
    () => ({ attivo, vaiA, segna, daTastiera }),
    [attivo, vaiA, segna, daTastiera],
  );
}

/** Dentro l'elenco: da 0 all'ultimo indice, e 0 se l'elenco è vuoto. */
function stretto(indice: number, totale: number): number {
  return Math.min(Math.max(indice, 0), Math.max(totale - 1, 0));
}
