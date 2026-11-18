/**
 * «Resta attiva in sottofondo», sul telefono: la regola delle note sotto
 * l'interruttore, senza interfaccia (`parti/SottofondoTelefono.tsx`). E la
 * riga di «Parti con l'auto», che sta sotto di lui.
 */

/**
 * Come si mostra la riga «Parti con l'auto»:
 *
 * - `nascosta` dove la build non la offre (`PARTENZA_CON_AUTO` falsa: la
 *   variante per Google Play): non compare, né spenta né impedita;
 * - `serveIlSottofondo` se si può offrire ma il sottofondo è spento: senza
 *   il servizio acceso nessuno guarda l'auto, e la riga lo dice invece di
 *   accendersi senza effetto;
 * - `libera` altrimenti.
 */
export type RigaDellAuto = "nascosta" | "serveIlSottofondo" | "libera";

export function rigaDellAuto(offerta: boolean, sottofondo: boolean): RigaDellAuto {
  if (!offerta) return "nascosta";
  return sottofondo ? "libera" : "serveIlSottofondo";
}

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
