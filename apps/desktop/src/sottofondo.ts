/**
 * «Resta attiva in sottofondo», sul telefono: la regola delle note sotto
 * l'interruttore, senza interfaccia (`parti/SottofondoTelefono.tsx`).
 */

/** Lo stato del risparmio batteria; `null` finché non si sa. */
export type SenzaRisparmio = boolean | null;

/**
 * Quali note vanno mostrate sotto l'interruttore, in ordine d'importanza:
 * `internet` — il sottofondo da solo non apre niente verso internet, e senza
 * «Raggiungibile da internet» i PC non trovano il telefono lo stesso —, poi lo
 * stato della batteria: `dentro` se Rymembra è ancora nel risparmio (la
 * spiegazione e il pulsante), `fuori` se ne è uscita, `null` se non si sa.
 *
 * Una funzione e non tre condizioni nel JSX, perché la regola si prova senza
 * un DOM (`test/sottofondoTelefono.test.ts`).
 */
export function noteDelSottofondo(
  acceso: boolean,
  raggiungibile: boolean,
  senzaRisparmio: SenzaRisparmio,
): { internet: boolean; batteria: "dentro" | "fuori" | null } {
  if (!acceso) return { internet: false, batteria: null };
  return {
    internet: !raggiungibile,
    batteria: senzaRisparmio === null ? null : senzaRisparmio ? "fuori" : "dentro",
  };
}
