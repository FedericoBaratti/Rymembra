/**
 * Com'è la coda delle importazioni, nel riepilogo della pagina.
 *
 * # Perché tre stati e non due
 *
 * Il riepilogo diceva «in corso» o «in pausa · N rimasti», e una coda finita
 * è ferma con zero rimasti: «12 / 12», la barra grigia di una coda fermata e
 * «in pausa · 0 rimasti», mentre il sottotitolo della stessa pagina diceva
 * «Nessuna coda in corso». «In pausa» faceva cercare un «Riprendi» che non
 * c'è. `fatti` resta quello dell'ultima passata finché un'importazione nuova
 * non riparte, quindi il riepilogo resta a schermo: deve dire «finita».
 *
 * Senza React e senza lingue, perché `node --test` la carichi da sola.
 */
export type StatoCoda = "corso" | "pausa" | "finita";

export function statoDellaCoda(coda: {
  attiva: boolean;
  fatti: number;
  inAttesa: number;
}): StatoCoda {
  if (coda.attiva) return "corso";
  return coda.inAttesa > 0 ? "pausa" : "finita";
}
