/**
 * Il perché di un problema del validatore, nella lingua dell'interfaccia.
 *
 * Fino alla 3.2 il nucleo mandava solo la frase italiana, e lo Studio in
 * inglese mostrava l'elenco dei problemi in italiano (D-46). Adesso ogni
 * problema e ogni avviso porta un `Motivo` (`rymembra_skin::motivo`): la
 * chiave del catalogo, i valori dei segnaposto, i nomi vicini. La frase si
 * compone qui; quella del nucleo resta il ripiego, e il modello della chat
 * continua a leggere quella (`prompt.ts`).
 *
 * Il traduttore arriva da fuori, come le descrizioni delle `Fonti` in
 * `codice.ts`: questo modulo si prova con `node --test`, che i JSON delle
 * lingue non li importa. Lo Studio gli passa `traduciMotivo` (`Dock.tsx`).
 */
import type { Motivo } from "../ipc";

/** Una chiave del catalogo coi suoi valori, o `null` se il catalogo non ce l'ha. */
export type Traduci = (chiave: string, valori: Record<string, string>) => string | null;

/** La frase di un motivo: tradotta se si può, altrimenti quella del nucleo. */
export function testoMotivo(motivo: Motivo, traduci: Traduci): string {
  if (motivo.chiave === "") return motivo.testo;
  const valori: Record<string, string> = {};
  for (const [nome, valore] of Object.entries(motivo.valori)) {
    valori[nome] = typeof valore === "string" ? valore : testoMotivo(valore, traduci);
  }
  const frase = traduci(motivo.chiave, valori);
  // Una chiave nata nel nucleo dopo questa finestra: la frase italiana ha già
  // il suo «Forse intendevi», e non se ne aggiunge un secondo.
  if (frase === null) return motivo.testo;
  if (motivo.forse.length === 0) return frase;
  const nomi = motivo.forse.map((nome) => traduci("skin.issue.forse.nome", { nome }) ?? nome).join(", ");
  const forse = traduci("skin.issue.forse", { nomi });
  return forse === null ? frase : `${frase} ${forse}`;
}
