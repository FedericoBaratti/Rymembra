/**
 * L'equalizzatore, le regole che non sono disegno: come si riconosce una curva
 * nella tendina, come si chiama una curva di serie, come si scrivono i decibel.
 *
 * Un modulo a parte perché è puro e si prova con `node --test`: il componente
 * porta con sé `ipc.ts` e le lingue, che fuori da Vite non si caricano. Da
 * `./ipc` arriva solo il **tipo**.
 */
import type { VocePreset } from "./ipc";

/**
 * La chiave di una curva nella tendina: `s:Rock` per quella di serie,
 * `u:Rock` per quella dell'utente.
 *
 * Non il nome, che stava lì: il nucleo accetta di salvare una curva col nome
 * di una di serie («Rock» ritoccata e salvata come «Rock»), e due `<option>`
 * con lo stesso valore sono due voci di cui si sceglie sempre la prima — la
 * curva salvata non si applicava mai, si poteva solo cancellare.
 */
export function chiavePreset(p: Pick<VocePreset, "diSerie" | "nome">): string {
  return `${p.diSerie ? "s" : "u"}:${p.nome}`;
}

/** La curva che ha questa chiave, o `undefined`. Vedi `chiavePreset`. */
export function trovaPreset<P extends Pick<VocePreset, "diSerie" | "nome">>(
  elenco: readonly P[],
  chiave: string,
): P | undefined {
  return elenco.find((p) => chiavePreset(p) === chiave);
}

/**
 * La chiave di lingua del nome di una curva di serie: `eq.preset.Rock`.
 *
 * Il nome arriva dal nucleo così com'è scritto in
 * `rymembra_play::equalizzatore::PRESET_DI_SERIE` — «Piatto», «Voce»,
 * «Bassi» — e resta il valore stabile; si traduce solo quel che si mostra.
 */
export function chiaveNomePreset(nome: string): string {
  return `eq.preset.${nome}`;
}

/**
 * I decibel di una banda, come si leggono accanto al cursore: «+1,5»,
 * «−2,5», «0».
 *
 * Con i separatori della lingua — il passo è mezzo decibel, quindi metà dei
 * valori ha un decimale, e «+1.5» in un'interfaccia italiana sembra un errore
 * di calcolo — e col segno meno vero (U+2212) invece del trattino, come il
 * «+» che gli sta di fronte. Il formattatore arriva da fuori (`numero` di
 * `formato.ts`) perché questo modulo resti senza lingue.
 */
export function decibel(db: number, numero: (n: number, decimali: number) => string): string {
  if (db === 0) return "0";
  const scritto = numero(Math.abs(db), Number.isInteger(db) ? 0 : 1);
  return db > 0 ? `+${scritto}` : `−${scritto}`;
}
