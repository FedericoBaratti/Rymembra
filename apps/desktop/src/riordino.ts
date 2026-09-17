/**
 * Riordinare un elenco trascinando una riga, col puntatore.
 *
 * # Perché non il trascinamento HTML5
 *
 * Perché su Windows non arriva mai a destinazione. La finestra ha
 * `dragDropEnabled` acceso in `tauri.conf.json` — è quel che fa arrivare a
 * `App` i file lasciati cadere dal sistema: skin, cartelle, brani, playlist — e
 * con quello acceso WebView2 tiene per sé il rilascio. `dragstart` partiva,
 * `dragover` e `drop` no, e `dragend` arrivava subito: la coda, le playlist a
 * mano e lo Studio disegnavano la maniglia e la presa, e al rilascio non
 * succedeva niente. Spegnere l'opzione avrebbe ridato il trascinamento dentro
 * la pagina togliendo quello dal sistema, cioè una funzione in cambio di
 * un'altra.
 *
 * # Due gesti, un meccanismo
 *
 * [`usePresaPerRiordino`] sposta una riga dentro il suo elenco: il bersaglio è
 * un'altra riga, e il rilascio ha una lettura sola. [`usePresaPerFessure`]
 * porta un pezzo **dentro** un posto che non è una riga — le fessure dello
 * scafale dello Studio, dove i nodi sono annidati e «sopra questa zona»
 * vorrebbe dire due cose. Sotto c'è lo stesso gesto: soglia, scorrimento sul
 * bordo, Esc, e il clic finale inghiottito.
 *
 * Il puntatore non passa dal sistema: `pointerdown`, `pointermove` e
 * `pointerup` sono eventi della pagina e basta.
 *
 * # Come si comporta
 *
 * - Il trascinamento comincia dopo {@link SOGLIA_PX} pixel: un clic resta un
 *   clic, un doppio clic resta un doppio clic.
 * - La riga d'arrivo è quella sotto il puntatore, cercata per `data-riordino`
 *   dentro lo stesso elenco della riga presa: un elenco non riceve righe da un
 *   altro — la coda della colonna e quella dello schermo intero possono essere
 *   a schermo insieme. «Lo stesso elenco» è il `[data-elenco]` più vicino, e
 *   dove nessuno lo dichiara è il genitore della riga: dove le righe sono
 *   sorelle non serve dire altro, e dove ognuna ha un guscio suo — la pila
 *   dello Studio, che sotto ogni livello apre le sue manopole — sarebbe
 *   l'unico modo di sbagliare.
 * - Vicino al bordo dello scorrevole l'elenco scorre da sé, come faceva il
 *   trascinamento del sistema: con la finestra virtuale la riga d'arrivo spesso
 *   non è ancora nel DOM.
 * - Il clic che il browser manda dopo il rilascio si inghiotte: sulla coda
 *   quel clic era «suona questa riga».
 * - Esc annulla.
 *
 * Il tatto no: sul tatto trascinare vuol dire scorrere, e `Alt+↑↓` resta la
 * strada da tastiera in tutti e due gli elenchi.
 */
import { useCallback, useEffect, useRef } from "react";

/** Quanto deve muoversi il puntatore prima che una pressione diventi una presa. */
const SOGLIA_PX = 5;

/** A che distanza dal bordo lo scorrevole comincia a scorrere da sé. */
const BORDO_PX = 48;

/** Quanto scorre al massimo per fotogramma, col puntatore sul bordo. */
const PASSO_MAX_PX = 18;

/** Quel che un elenco fa di un trascinamento. */
export interface AzioniRiordino {
  /** Comincia (`indice`) o finisce senza rilascio (`null`). */
  onPresa: (indice: number | null) => void;
  /** Il rilascio cadrebbe qui (`indice`), o da nessuna parte (`null`). */
  onMira: (indice: number | null) => void;
  /** Il rilascio è avvenuto su questa riga. */
  onLascia: (indice: number) => void;
}

/** L'elenco a cui una riga appartiene: il `[data-elenco]` più vicino, o il genitore. */
function elencoDi(riga: HTMLElement): HTMLElement | null {
  return riga.closest<HTMLElement>("[data-elenco]") ?? riga.parentElement;
}

/** Il primo antenato che scorre davvero in verticale, o `null`. */
function scorrevoleDi(nodo: HTMLElement | null): HTMLElement | null {
  for (let qui = nodo; qui !== null; qui = qui.parentElement) {
    const { overflowY } = getComputedStyle(qui);
    if (
      (overflowY === "auto" || overflowY === "scroll") &&
      qui.scrollHeight > qui.clientHeight
    ) {
      return qui;
    }
  }
  return null;
}

/** Quel che un gesto deve saper fare, qualunque cosa sia il bersaglio. */
interface Gesto<B> {
  /** Il bersaglio sotto il puntatore, o `null` se lì non si può lasciare. */
  sotto: (x: number, y: number) => B | null;
  /** La presa è cominciata: si è superata la soglia. */
  onPresa: () => void;
  /** Il rilascio cadrebbe qui, o da nessuna parte. */
  onMira: (dove: B | null) => void;
  /** Il rilascio è avvenuto qui. */
  onLascia: (dove: B) => void;
  /** Il gesto è finito senza rilasciare niente: Esc, o fuori bersaglio. */
  onNiente: () => void;
  /** Da cui si prende lo scorrevole su cui scorrere vicino al bordo. */
  dentro: HTMLElement | null;
}

/**
 * Il gesto vero e proprio: soglia, scorrimento sul bordo, Esc, clic inghiottito.
 *
 * Generico sul bersaglio perché i due usi lo hanno diverso — un indice di riga,
 * la chiave di una fessura — e tutto il resto è identico. Restituisce il modo
 * di smontarlo, per chi deve poterlo interrompere da fuori.
 */
function avviaGesto<B>(e: React.PointerEvent<HTMLElement>, gesto: Gesto<B>): () => void {
  const scorrevole = scorrevoleDi(gesto.dentro);
  const puntatore = e.pointerId;
  const partenza = { x: e.clientX, y: e.clientY };
  let ultimoY = e.clientY;
  let attivo = false;
  let mirato: B | null = null;
  let fotogramma = 0;

  const mira = (x: number, y: number) => {
    const n = gesto.sotto(x, y);
    if (n === mirato) return;
    mirato = n;
    gesto.onMira(n);
  };

  // Lo scorrimento sul bordo gira a fotogrammi e non a `pointermove`: col
  // puntatore fermo sul bordo non arriva nessun movimento, e l'elenco deve
  // scorrere lo stesso.
  const scorri = () => {
    fotogramma = 0;
    if (!attivo || scorrevole === null) return;
    const { top, bottom } = scorrevole.getBoundingClientRect();
    const verso =
      ultimoY < top + BORDO_PX
        ? -(top + BORDO_PX - ultimoY)
        : ultimoY > bottom - BORDO_PX
          ? ultimoY - (bottom - BORDO_PX)
          : 0;
    if (verso !== 0) {
      const passo = Math.max(
        -PASSO_MAX_PX,
        Math.min(PASSO_MAX_PX, Math.round((verso / BORDO_PX) * PASSO_MAX_PX)),
      );
      scorrevole.scrollTop += passo;
      mira(partenza.x, ultimoY);
    }
    fotogramma = window.requestAnimationFrame(scorri);
  };

  const smonta = () => {
    window.removeEventListener("pointermove", muovi, true);
    window.removeEventListener("pointerup", alza, true);
    window.removeEventListener("pointercancel", annulla, true);
    window.removeEventListener("keydown", tasto, true);
    if (fotogramma !== 0) window.cancelAnimationFrame(fotogramma);
    fotogramma = 0;
    delete document.documentElement.dataset.riordinando;
  };

  function muovi(ev: PointerEvent) {
    if (ev.pointerId !== puntatore) return;
    ultimoY = ev.clientY;
    if (!attivo) {
      const dx = ev.clientX - partenza.x;
      const dy = ev.clientY - partenza.y;
      if (Math.hypot(dx, dy) < SOGLIA_PX) return;
      attivo = true;
      // Il cursore e la selezione del testo, per tutta la pagina: il
      // puntatore può uscire dalla riga, e selezionare titoli a metà
      // trascinamento sarebbe il gesto sbagliato che si vede.
      document.documentElement.dataset.riordinando = "";
      window.getSelection()?.removeAllRanges();
      gesto.onPresa();
      fotogramma = window.requestAnimationFrame(scorri);
    }
    ev.preventDefault();
    // La x della partenza e non quella del puntatore: chi trascina in
    // verticale scivola di lato, e fuori dalla colonna dell'elenco
    // `elementFromPoint` troverebbe la navigazione.
    mira(partenza.x, ev.clientY);
  }

  function alza(ev: PointerEvent) {
    if (ev.pointerId !== puntatore) return;
    const eraAttivo = attivo;
    smonta();
    if (!eraAttivo) return;
    if (mirato !== null) gesto.onLascia(mirato);
    else gesto.onNiente();
    // Il clic che segue il rilascio: sulla riga di partenza, o sull'antenato
    // comune a quella d'arrivo. Arriva nello stesso giro di eventi, quindi un
    // `setTimeout` a zero toglie l'ascoltatore se per caso non arriva.
    const inghiotti = (c: MouseEvent) => {
      c.preventDefault();
      c.stopPropagation();
    };
    window.addEventListener("click", inghiotti, { capture: true, once: true });
    window.setTimeout(
      () => window.removeEventListener("click", inghiotti, { capture: true }),
      0,
    );
  }

  function annulla(ev: PointerEvent) {
    if (ev.pointerId !== puntatore) return;
    const eraAttivo = attivo;
    smonta();
    if (eraAttivo) gesto.onNiente();
  }

  function tasto(ev: KeyboardEvent) {
    if (ev.key !== "Escape" || !attivo) return;
    ev.preventDefault();
    ev.stopPropagation();
    smonta();
    gesto.onNiente();
  }

  window.addEventListener("pointermove", muovi, true);
  window.addEventListener("pointerup", alza, true);
  window.addEventListener("pointercancel", annulla, true);
  window.addEventListener("keydown", tasto, true);
  return smonta;
}

/** Il gesto si può cominciare? Un clic con modificatori o col tatto non è una presa. */
function eUnaPresa(e: React.PointerEvent<HTMLElement>): boolean {
  if (e.button !== 0 || e.pointerType === "touch") return false;
  // Ctrl e Maiusc sono la selezione multipla: quel clic non è una presa.
  return !(e.ctrlKey || e.shiftKey || e.metaKey || e.altKey);
}

/**
 * Il gestore da appendere a `onPointerDown` di ogni riga, con il suo indice.
 *
 * Identità stabile per tutta la vita del componente: le azioni si leggono al
 * momento del gesto, quindi una riga `memo` non si ridisegna per colpa sua.
 * Le azioni `undefined` spengono il gesto — l'elenco di un album non si
 * riordina.
 *
 * La riga deve portare `data-riordino={indice}`: è così che si riconosce la
 * riga sotto il puntatore.
 */
export function usePresaPerRiordino(azioni: {
  [K in keyof AzioniRiordino]?: AzioniRiordino[K] | undefined;
}): (e: React.PointerEvent<HTMLElement>, indice: number) => void {
  const ultime = useRef(azioni);
  ultime.current = azioni;
  // Il gesto in corso sa smontarsi da sé: se l'elenco se ne va a metà
  // trascinamento non restano ascoltatori appesi a `window`.
  const smontaInCorso = useRef<(() => void) | null>(null);
  useEffect(() => () => smontaInCorso.current?.(), []);

  return useCallback((e: React.PointerEvent<HTMLElement>, indice: number) => {
    const { onPresa, onMira, onLascia } = ultime.current;
    if (onPresa === undefined || onMira === undefined || onLascia === undefined) return;
    if (!eUnaPresa(e)) return;
    smontaInCorso.current?.();

    const lista = elencoDi(e.currentTarget);
    smontaInCorso.current = avviaGesto<number>(e, {
      dentro: lista,
      sotto: (x, y) => {
        const riga = document
          .elementFromPoint(x, y)
          ?.closest<HTMLElement>("[data-riordino]");
        if (!riga || elencoDi(riga) !== lista) return null;
        const n = Number(riga.dataset.riordino);
        return Number.isInteger(n) ? n : null;
      },
      onPresa: () => ultime.current.onPresa?.(indice),
      onMira: (dove) => ultime.current.onMira?.(dove),
      onLascia: (dove) => ultime.current.onLascia?.(dove),
      onNiente: () => ultime.current.onPresa?.(null),
    });
  }, []);
}

/** Quel che fa di un trascinamento chi ha delle fessure invece che delle righe. */
export interface AzioniFessure<C> {
  /** Comincia col suo carico, o finisce senza rilascio (`null`). */
  onPresa: (carico: C | null) => void;
  /** Il rilascio cadrebbe in questa fessura, o in nessuna. */
  onMira: (fessura: string | null) => void;
  /** Il rilascio è avvenuto in questa fessura. */
  onLascia: (fessura: string, carico: C) => void;
}

/**
 * Il gestore per chi porta un pezzo **dentro** una fessura.
 *
 * Il carico è quel che si sta portando — un nodo dell'albero, un widget della
 * tavolozza — e non deve stare in nessun `dataTransfer`: viene da qui, e
 * arriva a `onLascia` com'era.
 *
 * Il bersaglio è l'elemento con `data-fessura`, e la chiave lì dentro è quel
 * che torna: che voglia dire «quarto posto di questa zona» lo sa chi l'ha
 * scritta. Nessun vincolo di parentela, a differenza del riordino: una fessura
 * è già un posto preciso, e portare un pezzo da una zona a un'altra è
 * esattamente il gesto che serve.
 */
export function usePresaPerFessure<C>(
  azioni: AzioniFessure<C>,
): (e: React.PointerEvent<HTMLElement>, carico: C) => void {
  const ultime = useRef(azioni);
  ultime.current = azioni;
  const smontaInCorso = useRef<(() => void) | null>(null);
  useEffect(() => () => smontaInCorso.current?.(), []);

  return useCallback((e: React.PointerEvent<HTMLElement>, carico: C) => {
    if (!eUnaPresa(e)) return;
    smontaInCorso.current?.();
    smontaInCorso.current = avviaGesto<string>(e, {
      dentro: e.currentTarget,
      sotto: (x, y) =>
        document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-fessura]")
          ?.dataset.fessura ?? null,
      onPresa: () => ultime.current.onPresa(carico),
      onMira: (dove) => ultime.current.onMira(dove),
      onLascia: (dove) => ultime.current.onLascia(dove, carico),
      onNiente: () => ultime.current.onPresa(null),
    });
  }, []);
}
