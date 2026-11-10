/**
 * Il dito che trascina: il foglio che scende, la copertina che si sfoglia, il
 * mini lettore che si scosta.
 *
 * Un gancio solo per tutti i trascinamenti del telefono, perché hanno tutti gli
 * stessi quattro problemi, e ognuno li risolverebbe a modo suo:
 *
 * 1. **L'asse.** Un dito che scende per scorrere un elenco non sta trascinando
 *    il foglio di lato. Il gesto non parte finché il dito non ha fatto
 *    `SOGLIA_ASSE` pixel, e se va nella direzione che non interessa non parte
 *    affatto: il resto lo fa il browser, che scorre come sempre.
 * 2. **I fotogrammi.** Un `pointermove` arriva anche centoventi volte al secondo
 *    su un telefono con lo schermo veloce, e ridisegnare React a ogni evento è
 *    il modo più sicuro di far scattare un'animazione. Qui il movimento arriva
 *    al più una volta per fotogramma, e chi lo riceve scrive una variabile CSS su
 *    un elemento invece di cambiare stato.
 * 3. **Il clic dopo.** Lasciato il dito, il browser manda anche un `click` sul
 *    punto in cui si è staccato: dopo aver trascinato la copertina, quel clic
 *    aprirebbe la cosa sotto. Si ingoia, con lo stesso trucco di `riordino.ts`.
 * 4. **La cattura.** Partito il gesto, il dito resta suo anche se esce
 *    dall'elemento: `setPointerCapture`, e gli eventi si ascoltano sulla
 *    finestra.
 *
 * Quel che il gesto **vuol dire** non sta qui: lo decidono le funzioni pure di
 * `gesti.ts`, chiamate da chi riceve `onFine`.
 *
 * # `touch-action`
 *
 * Chi usa questo gancio deve dichiarare nel foglio di stile `touch-action`
 * sull'elemento: `none` dove il dito è tutto del gesto, `pan-y` dove deve poter
 * scorrere in verticale mentre il gesto ascolta il lato. Senza, il browser
 * prende per sé lo scorrimento e manda `pointercancel` a metà gesto.
 */
import { useEffect, useRef } from "react";

import { asseDelGesto, velocitaDa, type Campione } from "./gesti";

export type Asse = "x" | "y";

/** Com'è finito un gesto partito. */
export interface FineDelGesto {
  asse: Asse;
  dx: number;
  dy: number;
  /** Pixel al millisecondo, misurati sul rilascio: vedi `velocitaDa`. */
  vx: number;
  vy: number;
}

export interface OpzioniDito {
  /** Gli assi su cui il gesto può partire. */
  assi: Asse | "xy";
  /**
   * Dove il gesto non parte: un selettore CSS provato sul bersaglio e sui suoi
   * antenati. Il cursore della posizione, le stelle, un campo: lì il dito ha
   * già un mestiere.
   */
  ignora?: string | undefined;
  /** Spento: il dito fa quel che farebbe senza il gancio. */
  spento?: boolean | undefined;
  /** L'asse è deciso: da qui il dito è del gesto. */
  onParte?: ((asse: Asse) => void) | undefined;
  /** Il dito si è mosso; al più una volta per fotogramma. */
  onMuovi?: ((dx: number, dy: number, asse: Asse) => void) | undefined;
  /** Il dito si è staccato dopo che il gesto era partito. */
  onFine?: ((fine: FineDelGesto) => void) | undefined;
  /** Il sistema si è ripreso il dito (una chiamata, un gesto di sistema). */
  onAnnulla?: (() => void) | undefined;
}

/**
 * Il gestore da appendere a `onPointerDown`.
 *
 * Con identità stabile: le opzioni si leggono al momento del gesto, quindi chi
 * le passa nuove a ogni disegno non rifà niente.
 */
export function useDito(opzioni: OpzioniDito): {
  onPointerDown: (e: React.PointerEvent<HTMLElement>) => void;
} {
  const ultime = useRef(opzioni);
  ultime.current = opzioni;
  const smonta = useRef<(() => void) | null>(null);
  // Un gesto in corso quando il componente se ne va non deve lasciare
  // ascoltatori appesi alla finestra.
  useEffect(() => () => smonta.current?.(), []);

  const gestore = useRef((e: React.PointerEvent<HTMLElement>) => {
    const o = ultime.current;
    if (o.spento) return;
    // Il tasto principale del mouse, la penna, il dito: il destro e il centrale
    // hanno già il loro mestiere.
    if (e.pointerType === "mouse" && e.button !== 0) return;
    if (!e.isPrimary) return;
    const bersaglio = e.target as Element;
    if (o.ignora !== undefined && bersaglio.closest(o.ignora) !== null) return;
    smonta.current?.();

    const elemento = e.currentTarget;
    const id = e.pointerId;
    const x0 = e.clientX;
    const y0 = e.clientY;
    const campioni: Campione[] = [{ t: e.timeStamp, x: x0, y: y0 }];
    let asse: Asse | null = null;
    let fotogramma = 0;
    let ultimo = { dx: 0, dy: 0 };

    const muovi = (ev: PointerEvent) => {
      if (ev.pointerId !== id) return;
      const dx = ev.clientX - x0;
      const dy = ev.clientY - y0;
      campioni.push({ t: ev.timeStamp, x: ev.clientX, y: ev.clientY });
      // Non serve la storia intera: la velocità guarda gli ultimi ottanta
      // millisecondi, e un gesto lungo non deve accumulare migliaia di punti.
      if (campioni.length > 32) campioni.splice(0, campioni.length - 32);
      if (asse === null) {
        const deciso = asseDelGesto(dx, dy);
        if (deciso === null) return;
        const voluto = ultime.current.assi;
        if (voluto !== "xy" && voluto !== deciso) {
          // Va dove il gesto non ascolta: è uno scorrimento, e il dito torna
          // al browser per tutto il resto del tocco.
          fine();
          return;
        }
        asse = deciso;
        try {
          elemento.setPointerCapture(id);
        } catch {
          // Il puntatore è già andato: lo dirà `pointercancel`.
        }
        ultime.current.onParte?.(asse);
      }
      ultimo = { dx, dy };
      if (fotogramma !== 0) return;
      fotogramma = requestAnimationFrame(() => {
        fotogramma = 0;
        if (asse !== null) ultime.current.onMuovi?.(ultimo.dx, ultimo.dy, asse);
      });
    };

    const alza = (ev: PointerEvent) => {
      if (ev.pointerId !== id) return;
      const eraPartito = asse;
      campioni.push({ t: ev.timeStamp, x: ev.clientX, y: ev.clientY });
      fine();
      if (eraPartito === null) return;
      const { vx, vy } = velocitaDa(campioni);
      ultime.current.onFine?.({
        asse: eraPartito,
        dx: ev.clientX - x0,
        dy: ev.clientY - y0,
        vx,
        vy,
      });
      // Il clic che il browser manda dopo il rilascio: dopo un trascinamento
      // non è un tocco. Arriva nello stesso giro di eventi, e il `setTimeout`
      // toglie l'ascoltatore se per caso non arriva.
      const inghiotti = (c: MouseEvent) => {
        c.preventDefault();
        c.stopPropagation();
      };
      window.addEventListener("click", inghiotti, { capture: true, once: true });
      window.setTimeout(
        () => window.removeEventListener("click", inghiotti, { capture: true }),
        0,
      );
    };

    const annulla = (ev: PointerEvent) => {
      if (ev.pointerId !== id) return;
      const eraPartito = asse !== null;
      fine();
      if (eraPartito) ultime.current.onAnnulla?.();
    };

    function fine() {
      if (fotogramma !== 0) cancelAnimationFrame(fotogramma);
      fotogramma = 0;
      window.removeEventListener("pointermove", muovi, true);
      window.removeEventListener("pointerup", alza, true);
      window.removeEventListener("pointercancel", annulla, true);
      smonta.current = null;
    }

    window.addEventListener("pointermove", muovi, true);
    window.addEventListener("pointerup", alza, true);
    window.addEventListener("pointercancel", annulla, true);
    smonta.current = fine;
  });

  return { onPointerDown: gestore.current };
}
