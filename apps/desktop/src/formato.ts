/**
 * I numeri, nella forma in cui si leggono.
 *
 * Stavano in `App.tsx` quando la finestra era una schermata sola. Il lettore
 * mostra gli stessi minuti e secondi dell'elenco, e due `durata()` che si
 * somigliano sono due formati che prima o poi divergono di un carattere — la
 * versione piccola di ciò che è successo fra desktop e Android nel vecchio
 * albero.
 */

/** Millisecondi in `m:ss`, o `h:mm:ss` quando serve. */
export function durata(ms: number): string {
  const totale = Math.round(ms / 1000);
  const s = totale % 60;
  const m = Math.floor(totale / 60) % 60;
  const h = Math.floor(totale / 3600);
  const dueCifre = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${dueCifre(m)}:${dueCifre(s)}` : `${m}:${dueCifre(s)}`;
}

/** Millisecondi in ore, per il pannello laterale. */
export function ore(ms: number): string {
  const h = ms / 3_600_000;
  return h >= 10 ? `${Math.round(h)} h` : `${h.toFixed(1)} h`;
}

/** «1 brano», «2 brani». Metà della libreria ha un brano solo. */
export function brani_(n: number): string {
  return n === 1 ? "1 brano" : `${n} brani`;
}
