/**
 * Lo stesso contenuto, lo stesso oggetto.
 *
 * Ogni validazione torna dall'IPC con oggetti nuovi, anche quando la risposta
 * è ricordata dal nucleo (è una `clone()`), e un `memo` che confronta per
 * identità salta lo stesso: la tela ridisegnava l'intera app finta — Home,
 * Esplora, Impostazioni — a ogni battuta nel codice, anche quando lo scafale
 * non era cambiato (R14-11). Qui si tiene il precedente se il contenuto è lo
 * stesso: una `JSON.stringify` di uno scafale costa meno di un disegno.
 */
export function stabile<T>(precedente: T | null, nuovo: T | null): T | null {
  if (precedente === null || nuovo === null) return nuovo;
  return JSON.stringify(precedente) === JSON.stringify(nuovo) ? precedente : nuovo;
}
