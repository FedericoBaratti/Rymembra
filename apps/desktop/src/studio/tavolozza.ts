/**
 * Una tavolozza proposta dal generatore, e per quale tema.
 *
 * La proposta ricorda il tema che si guardava quando è stata chiesta, e se
 * era scuro. Fino alla 3.2 si teneva solo l'elenco dei colori: dopo «Guarda
 * la variante» — il tasto sta subito sopra — la proposta scura restava a
 * schermo sotto il titolo «tema chiaro», e «Applica» la scriveva in
 * `variant.tokens`. Adesso vale solo per il tema e lo `scheme` di quando è
 * nata (anche una risposta arrivata dopo un cambio di tema), e si scrive nel
 * tema per cui è stata fatta. Pure, per `node --test` (`test/tavolozza.test.ts`).
 */
import type { OperazioneIa, VoceTavolozza } from "../ipc";
import type { Tema } from "./negozio";
import { sentieroToken } from "./sentieri";

export interface PropostaTavolozza {
  voci: VoceTavolozza[];
  /** Il tema che si guardava quando è stata chiesta. */
  tema: Tema;
  /** Chiesta per un tema scuro. */
  scura: boolean;
  daCopertina: boolean;
}

/** La proposta, se vale per il tema che si guarda adesso. */
export function propostaPer(
  proposta: PropostaTavolozza | null,
  tema: Tema,
  scura: boolean,
): PropostaTavolozza | null {
  return proposta !== null && proposta.tema === tema && proposta.scura === scura ? proposta : null;
}

/** Le scritture che applicano la proposta. */
export function operazioniTavolozza(proposta: PropostaTavolozza): OperazioneIa[] {
  return proposta.voci.map((v) => ({ togli: false, percorso: sentieroToken(v.token, proposta.tema), valore: v.valore }));
}
