/**
 * Le scorciatoie, tutte qui.
 *
 * # Perché in un file solo
 *
 * Una scorciatoia sparsa nel componente che la usa è una scorciatoia che
 * nessuno sa che esiste, e soprattutto è una scorciatoia che può collidere con
 * un'altra senza che niente lo dica: due `keydown` su `window` in due componenti
 * diversi si eseguono tutti e due. Qui il conflitto si vede leggendo, e la
 * documentazione della mappa è la mappa.
 *
 * # La regola che vale per tutte: non mentre si scrive
 *
 * `Spazio` mette in pausa **tranne** quando il fuoco è in un campo di testo, e
 * `/` apre la ricerca tranne quando si sta già scrivendo. Senza questo controllo
 * chi cerca «space oddity» metterebbe in pausa a metà parola. Il controllo è sul
 * bersaglio dell'evento e non su uno stato dell'applicazione: l'unica fonte
 * attendibile di «dove sta il fuoco adesso» è il documento.
 *
 * `Escape` è l'eccezione voluta: funziona **anche** mentre si scrive, perché in
 * un campo pieno il primo significato di Escape è «lascia stare», e chi lo
 * preme dentro la ricerca vuole uscirne.
 */
import { useEffect, useRef } from "react";

import { posizioneAdesso } from "./riproduzione";

/** Di quanto si sposta il cursore con una freccia. */
const PASSO_MS = 5_000;

/** Cosa succede a ogni tasto. */
export type Azioni = {
  /** Spazio. */
  alterna: () => void;
  /** `/` e `Ctrl+F`. */
  cerca: () => void;
  /** `←` e `→`, in millisecondi assoluti. */
  vaiA: (ms: number) => void;
  /** `Escape`. Restituisce `true` se ha chiuso qualcosa. */
  chiudi: () => boolean;
  /** `F`. */
  inRiproduzione: () => void;
  /** La durata del brano, per non uscirne. */
  durataMs: number;
};

/** Il fuoco è dentro qualcosa in cui si scrive. */
function siStaScrivendo(bersaglio: EventTarget | null): boolean {
  if (!(bersaglio instanceof HTMLElement)) return false;
  if (bersaglio.isContentEditable) return true;
  const nome = bersaglio.tagName;
  if (nome === "TEXTAREA" || nome === "SELECT") return true;
  if (nome !== "INPUT") return false;
  // Un cursore a scorrimento è un `input` ma non ci si scrive dentro, e le
  // frecce ce le vuole lui: senza questa distinzione, spostare il volume con la
  // tastiera sposterebbe anche la posizione nel brano.
  const tipo = (bersaglio as HTMLInputElement).type;
  return tipo !== "range" && tipo !== "checkbox" && tipo !== "radio";
}

/** Il campo di ricerca dell'intestazione, se la pagina ne ha uno. */
export function campoRicerca(): HTMLInputElement | null {
  return document.querySelector<HTMLInputElement>("[data-cerca]");
}

export function useScorciatoie(azioni: Azioni): void {
  // In un ref, e l'effetto senza dipendenze: `azioni` arriva come oggetto
  // scritto sul posto, quindi la sua identità cambia a ogni disegno di `App`.
  // Tenerlo fra le dipendenze voleva dire togliere e rimettere l'ascoltatore
  // della tastiera a ogni disegno — cioè venti volte al secondo mentre suona,
  // per una mappa di tasti che non cambia mai.
  const ultime = useRef(azioni);
  ultime.current = azioni;

  useEffect(() => {
    const ascolta = (e: KeyboardEvent) => {
      const azioni = ultime.current;
      // Una combinazione con un modificatore di sistema non è nostra: `Ctrl+F`
      // è l'unica che si prende, ed è dichiarata sotto.
      if (e.altKey || e.metaKey) return;
      const scrivendo = siStaScrivendo(e.target);

      if (e.key === "Escape") {
        // Prima si esce dal campo, poi si chiude quel che c'è aperto: due
        // pressioni per due significati, invece di uno che ne annulla un altro.
        if (scrivendo && e.target instanceof HTMLElement) {
          e.target.blur();
          e.preventDefault();
          return;
        }
        if (azioni.chiudi()) e.preventDefault();
        return;
      }

      if (e.ctrlKey) {
        if (e.key === "f" || e.key === "F") {
          e.preventDefault();
          azioni.cerca();
        }
        return;
      }

      if (scrivendo) return;

      switch (e.key) {
        case " ":
          // `preventDefault` obbligatorio: senza, lo Spazio fa anche scorrere la
          // pagina di uno schermo, e mettere in pausa sposterebbe l'elenco.
          e.preventDefault();
          azioni.alterna();
          break;
        case "/":
          e.preventDefault();
          azioni.cerca();
          break;
        // La posizione si legge **adesso**, dall'archivio: è l'unico posto che
        // la vuole in risposta a un gesto invece che per disegnarla, e farsela
        // passare come prop la rimetterebbe fra le cose che ridisegnano `App`
        // venti volte al secondo.
        case "ArrowRight":
          e.preventDefault();
          azioni.vaiA(Math.min(posizioneAdesso() + PASSO_MS, azioni.durataMs));
          break;
        case "ArrowLeft":
          e.preventDefault();
          azioni.vaiA(Math.max(posizioneAdesso() - PASSO_MS, 0));
          break;
        case "f":
        case "F":
          e.preventDefault();
          azioni.inRiproduzione();
          break;
        default:
          break;
      }
    };

    window.addEventListener("keydown", ascolta);
    return () => window.removeEventListener("keydown", ascolta);
  }, []);
}
