/**
 * La vibrazione dei gesti, sul telefono.
 *
 * Un dito non sente un clic: sul vetro non c'è la corsa del tasto, e un gesto
 * che è andato a segno — un tocco lungo che ha aperto la selezione, una
 * strisciata che ha cambiato brano, un foglio che chiuderà se lo si lascia —
 * senza una risposta nel dito si verifica solo guardando. La vibrazione è la
 * corsa del tasto che il vetro non ha.
 *
 * È quella del sistema (`View.performHapticFeedback`, in `Ponte.kt`) e non un
 * motore acceso a mano da qui: segue «Feedback al tocco» delle impostazioni del
 * telefono, non chiede permessi, e ogni gesto vibra come vibra nelle altre app.
 * Tre forme soltanto, quelle che servono:
 *
 * - `tenuta`: un tocco lungo che ha fatto qualcosa;
 * - `scatto`: una soglia superata mentre il dito è ancora giù;
 * - `conferma`: un gesto finito e andato a buon fine.
 *
 * Non aspetta niente e non dice niente se non riesce: una vibrazione mancata
 * non è un guasto da raccontare a nessuno, e il gesto ha già fatto il suo.
 *
 * Fuori da Android non fa niente.
 */
import { ipc } from "./ipc";
import { ANDROID } from "./piattaforma";

export type Vibrazione = "tenuta" | "scatto" | "conferma";

/** Vibra, se si è su Android. Non aspetta e non fallisce. */
export function vibra(tipo: Vibrazione): void {
  if (!ANDROID) return;
  ipc.telefonoVibra(tipo).catch(() => {});
}

/*
 * Il tocco lungo vibra da qui, una volta per tutti.
 *
 * La WebView di Android trasforma il tocco lungo in `contextmenu`, e ogni posto
 * dell'app che ne fa qualcosa — una riga che entra nella selezione, una scheda
 * che apre il suo foglio, una cartella — lo prende con `preventDefault()`.
 * Questo ascoltatore sta sulla finestra, nella fase di risalita: arriva **dopo**
 * i gestori di React, che stanno sulla radice, e vibra solo se qualcuno l'ha
 * preso. Un tocco lungo sul vuoto non vibra, e non si deve ricordare di
 * chiamare `vibra` nessuno dei posti che il tocco lungo lo usano.
 */
if (ANDROID && typeof window !== "undefined") {
  window.addEventListener("contextmenu", (e) => {
    if (e.defaultPrevented) vibra("tenuta");
  });
}
