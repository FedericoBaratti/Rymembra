/**
 * Tagliare un testo senza spezzare un carattere.
 *
 * `String.slice` conta unità UTF-16, e un carattere fuori dal piano di base —
 * un'emoji, un ideogramma raro — ne occupa due: una coppia di surrogati.
 * Tagliare fra le due lascia un surrogato alto da solo, che `JSON.stringify`
 * scrive come sequenza di escape e che `serde_json` rifiuta. Fino alla 3.2
 * capitava in due posti, con lo stesso effetto persistente: la cronologia
 * riassunta della chat (`prompt.ts`) faceva fallire `ia_conversa` a ogni
 * domanda, e un valore tagliato nei cambi (`conversazione.ts`) rendeva
 * `chat.json` illeggibile al nucleo a ogni salvataggio.
 */

/** Quante unità tenere per non andare oltre `quante` senza spezzare una coppia. */
export function confineSicuro(testo: string, quante: number): number {
  const n = Math.max(0, Math.min(quante, testo.length));
  if (n === 0 || n === testo.length) return n;
  const ultima = testo.charCodeAt(n - 1);
  // L'ultima tenuta è la metà alta di una coppia: si lascia anche lei.
  return ultima >= 0xd800 && ultima <= 0xdbff ? n - 1 : n;
}

/** Il testo accorciato a `quante` unità, con «…» se è stato tagliato. */
export function accorciato(testo: string, quante: number): string {
  return testo.length <= quante ? testo : `${testo.slice(0, confineSicuro(testo, quante))}…`;
}
