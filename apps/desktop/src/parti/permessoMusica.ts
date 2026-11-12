/**
 * Che cosa chiedere del permesso di leggere la musica, sul telefono, e dove.
 *
 * Il primo avvio lo chiede (`AccessoTelefono.tsx`), ma «Lo faccio dopo» resta,
 * e anche un «Non consentire» nel dialogo di Android: chi ci passava trovava
 * una libreria che si scansionava vuota per sempre — Android non mostra a
 * un'app i file degli altri — senza un posto nell'app dove concederlo. Adesso
 * lo si chiede anche da due posti (D-35):
 *
 * - **gli stati vuoti della libreria**, solo se manca del tutto: è l'unico caso
 *   in cui è il permesso a svuotarla. Con i soli brani la libreria si riempie,
 *   e una frase sui `.lrc` lì sarebbe fuori posto;
 * - **Impostazioni › Cartelle e scansione**, anche per chi al primo avvio ha
 *   scelto «Continua solo con i brani» e adesso vuole i testi, i `.cue` e le
 *   copertine accanto: lì si passa ad «Accesso a tutti i file».
 *
 * Una funzione pura, staccata dal componente, perché la regola si provi.
 */

/** Dove sta l'accesso adesso. Fuori da Android è sempre `pieno`. */
export type StatoAccesso = "verifica" | "manca" | "solo-media" | "pieno";

/** Il posto da cui lo si chiede. */
export type DoveSiChiede = "vuoto" | "impostazioni";

/**
 * `musica`: manca il permesso di leggere i brani; `tuttiIFile`: i brani sì, il
 * resto no; `niente`: non c'è niente da chiedere — o non lo si sa ancora, e una
 * frase che compare e sparisce a ogni avvio è peggio di niente.
 */
export type DaChiedere = "niente" | "musica" | "tuttiIFile";

export function cosaChiedere(stato: StatoAccesso, dove: DoveSiChiede): DaChiedere {
  if (stato === "manca") return "musica";
  if (stato === "solo-media" && dove === "impostazioni") return "tuttiIFile";
  return "niente";
}
