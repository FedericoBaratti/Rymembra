/**
 * Le icone delle barre di sistema, sul telefono: l'ora, la batteria, la barra
 * dei gesti.
 *
 * Da Android 15 l'app disegna anche sotto quelle barre, e il colore delle loro
 * icone lo sceglie Android sul tema **del telefono**. Il tema dell'app è un
 * altro: con Rymembra scura su un telefono chiaro l'ora era nera su nero, e
 * con Rymembra chiara su un telefono scuro bianca su bianco. Qui le si allinea
 * al tema che si vede davvero, cioè a `data-theme` sulla radice (`tema.ts`):
 * fondo chiaro, icone scure.
 *
 * Basta il tema anche per «In riproduzione», che copre tutto: il suo fondo è
 * la superficie del tema con la copertina sfocata sotto un velo, chiara nel
 * chiaro e scura nello scuro.
 *
 * Non aspetta e non fallisce: icone del colore sbagliato non sono un guasto da
 * raccontare, e alla prossima volta si riprova.
 */
import { ipc } from "./ipc";
import { ANDROID } from "./piattaforma";

/** L'ultimo fondo detto ad Android, per non ridirlo a ogni attributo. */
let detto: boolean | null = null;

function allinea(): void {
  const chiaro = document.documentElement.dataset["theme"] === "light";
  if (chiaro === detto) return;
  detto = chiaro;
  ipc.telefonoBarre(chiaro).catch(() => {
    detto = null;
  });
}

if (ANDROID && typeof document !== "undefined") {
  new MutationObserver(allinea).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });
  // Di solito il tema arriva dopo questo modulo (`main.tsx` lo scrive prima
  // del primo disegno), e lo prende l'osservatore.
  if (document.documentElement.dataset["theme"] !== undefined) allinea();
}
