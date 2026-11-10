/**
 * Quanto la pagina deve stare lontana dai bordi del telefono.
 *
 * Da Android 15 un'app disegna fin sotto la barra di stato e quella dei gesti,
 * e deve lasciare libero quel tanto. La CSS ha `env(safe-area-inset-*)` per
 * dirlo, ma la WebView di Android non lo riempie su tutte le versioni: la misura
 * la prende `MainActivity.kt` (le barre, il ritaglio della fotocamera, la
 * tastiera aperta), la tiene nel ponte, e manda `rymembra:riquadri` quando
 * cambia. Qui la si rilegge e la si scrive in quattro variabili sulla radice —
 * `--riquadro-alto`, `-destra`, `-basso`, `-sinistra` — che
 * `stile/telefono.css` usa al posto di `env()`.
 *
 * E in più dice se la tastiera è aperta: `data-tastiera` sulla radice, e la sua
 * altezza in `--riquadro-tastiera`. Con la tastiera su, la barra delle schede e
 * il mini lettore se ne vanno (`telefono.css`): sarebbero spinti in su fino a
 * metà schermo, sopra la tastiera, a rubare spazio ai risultati proprio mentre
 * si scrive cosa cercare — ed è quel che fanno tutte le app di Android.
 *
 * Fuori da Android non fa niente.
 */
import { ipc } from "./ipc";
import { ANDROID } from "./piattaforma";

async function aggiorna(): Promise<void> {
  const r = await ipc.telefonoRiquadri();
  const radice = document.documentElement.style;
  radice.setProperty("--riquadro-alto", `${r.alto}px`);
  radice.setProperty("--riquadro-destra", `${r.destra}px`);
  radice.setProperty("--riquadro-basso", `${r.basso}px`);
  radice.setProperty("--riquadro-sinistra", `${r.sinistra}px`);
  // `?? 0`: una pagina nuova sopra un'attività di prima, che il campo non lo
  // manda, è una pagina senza tastiera — non una lettura fallita.
  const tastiera = r.tastiera ?? 0;
  radice.setProperty("--riquadro-tastiera", `${tastiera}px`);
  if (tastiera > 0) document.documentElement.dataset["tastiera"] = "";
  else delete document.documentElement.dataset["tastiera"];
}

if (ANDROID && typeof window !== "undefined") {
  const subito = () => void aggiorna().catch(() => {});
  subito();
  // Il segnale di `MainActivity`, e la rotazione per sicurezza: ruotando, i
  // riquadri si spostano da un lato all'altro.
  window.addEventListener("rymembra:riquadri", subito);
  window.addEventListener("resize", subito);
}
