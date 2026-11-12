/**
 * Quali documenti pubblici si leggono dalla copia che viaggia con l'app.
 *
 * La licenza, gli avvisi delle terze parti, la privacy e le condizioni devono
 * potersi leggere anche senza rete (D-10d): accanto all'eseguibile sul
 * computer, in `assets/legale/` nell'APK, c'è la loro copia, e la finestra la
 * mostra in una finestrella (`parti/DocumentoLocale.tsx`). Gli altri — il
 * repository, le segnalazioni — sono pagine e non testi, e vanno al browser.
 * L'elenco vero lo tiene il nucleo (`Documento::copia_locale` in
 * `comandi.rs`); questo dice soltanto a chi chiedere prima.
 */
export type ConCopiaLocale = "licenza" | "terze" | "privacy" | "condizioni";

const CON_COPIA: ReadonlySet<string> = new Set<ConCopiaLocale>(["licenza", "terze", "privacy", "condizioni"]);

export function conCopiaLocale(quale: string): quale is ConCopiaLocale {
  return CON_COPIA.has(quale);
}
