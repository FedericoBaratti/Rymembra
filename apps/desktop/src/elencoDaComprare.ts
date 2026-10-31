/**
 * Quando la sezione «Da comprare» della pagina Importazioni chiede il suo
 * elenco (`DaComprareSezione` in `schermate/Importazioni.tsx`).
 *
 * All'apertura e non al montaggio, per la ragione scritta lì. Ma non **una
 * volta sola**: la sezione resta montata mentre la coda gira, e il suo conto
 * (`quanti`) arriva dal vivo dagli eventi. Letto una volta, l'elenco restava
 * quello della prima apertura — il titolo diceva 23 e sotto ce n'erano 3, e
 * chiudere e riaprire non bastava. Quindi si rilegge quando il conto non è più
 * quello con cui l'elenco è stato letto; le righe vecchie restano a schermo
 * finché non arrivano le nuove, così l'elenco non sparisce e non perde lo
 * scorrimento a ogni brano introvabile.
 */
export function daRichiedere({
  aperta,
  letta,
  quanti,
  quantiLetti,
}: {
  /** La sezione è aperta. */
  aperta: boolean;
  /** Un elenco è già arrivato. */
  letta: boolean;
  /** Il conto di adesso. */
  quanti: number;
  /** Il conto con cui è stato chiesto l'elenco a schermo, o `null`. */
  quantiLetti: number | null;
}): boolean {
  if (!aperta) return false;
  return !letta || quantiLetti !== quanti;
}
