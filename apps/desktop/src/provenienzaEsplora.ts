/**
 * La provenienza che «Tieni» e «Nella lista» registrano per una riga di
 * Esplora (`schermate/Esplora.tsx`): è quel che poi si legge in «Da comprare».
 *
 * La frase della ricerca che ha prodotto i risultati a schermo, non il testo
 * che c'è nella casella adesso: la ricerca parte solo con Invio, e chi ha
 * cercato «debussy» e ha cominciato a scrivere «coltr» ha ancora davanti i
 * risultati di Debussy — la riga nasceva con provenienza «coltr», o vuota con
 * la casella svuotata. I tasti esistono solo con un esito a schermo, quindi il
 * ripiego sulla casella non scatta quasi mai.
 */
export function provenienzaDi(
  fase: { tipo: "esito"; frase: string } | { tipo: string },
  casella: string,
): string {
  return fase.tipo === "esito" && "frase" in fase ? fase.frase : casella.trim();
}
