/**
 * Aspettare che un foglio abbia finito di muoversi, anche quando non si muove.
 *
 * Un foglio che si chiude scivola giù e **poi** si smonta: smontarlo subito
 * vorrebbe dire vederlo sparire a metà schermo. Il segnale è `transitionend`, e
 * ha due modi di non arrivare mai:
 *
 * - **il movimento è spento** — dal sistema o dalle impostazioni, che arrivano
 *   qui come `--motion-scale: 0` — e una transizione di durata zero non manda
 *   nessun evento;
 * - **non è cambiato niente** — il foglio era già giù, o la transizione è stata
 *   interrotta da un'altra — e l'evento resta in attesa di una fine che non c'è.
 *
 * In tutti e due i casi il foglio resterebbe montato, invisibile, a prendersi i
 * tocchi. Qui la fine arriva comunque: subito se il movimento è spento, e al
 * più tardi poco dopo la durata dichiarata se l'evento non si fa vivo.
 */
import { fermoRestando } from "./transizione";

/**
 * La durata più lunga di una lista CSS come `0.3s, 120ms`, in millisecondi.
 *
 * Le transizioni di un elemento sono una lista per proprietà, e quella che
 * conta è l'ultima a finire. Un valore che non si legge vale zero: è la durata
 * che il browser gli darebbe.
 */
export function durataInMs(lista: string): number {
  let massima = 0;
  for (const pezzo of lista.split(",")) {
    const v = pezzo.trim();
    const n = Number.parseFloat(v);
    if (!Number.isFinite(n)) continue;
    const ms = v.endsWith("ms") ? n : v.endsWith("s") ? n * 1000 : 0;
    massima = Math.max(massima, ms);
  }
  return massima;
}

/** Quanto oltre la durata dichiarata si aspetta un `transitionend` che non viene. */
const MARGINE_MS = 60;

/**
 * Chiama `fatto` quando la transizione di `transform` su `elemento` è finita.
 *
 * Ritorna una funzione che rinuncia: chi la chiama prima della fine — un
 * foglio riaperto mentre scendeva — non riceve `fatto`.
 */
export function dopoLaTransizione(
  elemento: HTMLElement | null,
  fatto: () => void,
): () => void {
  if (elemento === null || fermoRestando()) {
    fatto();
    return () => {};
  }
  const durata = durataInMs(getComputedStyle(elemento).transitionDuration);
  if (durata === 0) {
    fatto();
    return () => {};
  }
  let finito = false;
  const fine = () => {
    if (finito) return;
    finito = true;
    elemento.removeEventListener("transitionend", suFine);
    window.clearTimeout(riserva);
    fatto();
  };
  const suFine = (e: TransitionEvent) => {
    if (e.target === elemento && e.propertyName === "transform") fine();
  };
  elemento.addEventListener("transitionend", suFine);
  const riserva = window.setTimeout(fine, durata + MARGINE_MS);
  return () => {
    if (finito) return;
    finito = true;
    elemento.removeEventListener("transitionend", suFine);
    window.clearTimeout(riserva);
  };
}
