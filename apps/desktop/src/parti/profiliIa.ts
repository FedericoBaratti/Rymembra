/**
 * «Modelli di linguaggio», le regole che non sono disegno: quale profilo si
 * prova da solo dopo il salvataggio, e che cosa è in volo.
 *
 * Un modulo a parte perché è puro e si prova con `node --test`: `ModelliIA.tsx`
 * porta con sé `ipc.ts` e le lingue, che fuori da Vite non si caricano. Da
 * `../ipc` arrivano solo i **tipi**.
 */
import type { ProfiloIa, StatoIa } from "../ipc";

/**
 * Il profilo appena salvato, nella risposta di `iaSalvaProfilo`: quello
 * attivo, perché il nucleo rende attivo il profilo che salva (`CHIAVE_ATTIVO`)
 * e `attivo` lo rilegge da lì.
 *
 * Non per nome, come si cercava: il nucleo toglie gli spazi ai bordi e, a nome
 * vuoto, usa il modello — «» e «Mio » non si trovavano, e la prova automatica
 * non partiva senza dirlo — e due profili «OpenRouter» hanno id diversi: si
 * provava il primo, il vecchio, e il suo elenco di modelli finiva nel modulo
 * nuovo. Nemmeno per l'id della bozza: un profilo eliminato altrove nel
 * frattempo il nucleo lo ricrea con un id nuovo.
 */
export function profiloSalvato(stato: Pick<StatoIa, "profili" | "attivo">): ProfiloIa | undefined {
  return stato.attivo === null ? undefined : stato.profili.find((p) => p.id === stato.attivo);
}

/** Quel che può essere in volo: un salvataggio, una prova, un altro comando. */
export type GestoIa = "salva" | "prova" | "comando";

/**
 * Che cosa è in volo, gesto per gesto.
 *
 * Era un booleano solo per tutti: il salvataggio fa partire la prova da sé,
 * e il suo `finally` lo spegneva mentre la prova era ancora in volo — tasti
 * riaccesi e nessun segno d'attesa per i secondi che l'elenco di OpenRouter
 * chiede — e le scritte mentivano: «Salvataggio…» durante una prova,
 * «Prova in corso» durante un salvataggio. Ogni scritta guarda il suo; i tasti
 * spenti guardano tutti (`qualcosaInVolo`).
 */
export type InVolo = Readonly<Record<GestoIa, boolean>>;

export const NIENTE_IN_VOLO: InVolo = { salva: false, prova: false, comando: false };

/** Un gesto che parte o finisce. */
export interface EventoVolo {
  gesto: GestoIa;
  fase: "inizio" | "fine";
}

/** Il riduttore: il gesto acceso o spento, gli altri come stavano. */
export function inVoloDopo(prima: InVolo, evento: EventoVolo): InVolo {
  const acceso = evento.fase === "inizio";
  return prima[evento.gesto] === acceso ? prima : { ...prima, [evento.gesto]: acceso };
}

/** Qualcosa è in volo: i tasti che lanciano un comando restano spenti. */
export function qualcosaInVolo(v: InVolo): boolean {
  return v.salva || v.prova || v.comando;
}
