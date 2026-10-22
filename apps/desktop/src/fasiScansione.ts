/**
 * Che cosa si racconta di una scansione in corso, a chi guarda e a chi ascolta.
 *
 * # Perché un modulo a parte
 *
 * La stessa scansione la disegnano in quattro: la pila dei toast, le
 * Impostazioni, il primo avvio e l'annuncio per chi ascolta lo schermo. Dalla
 * 3.0.2 l'avanzamento porta una fase (`ipc.ts`, `FaseScansione`), e ognuno dei
 * quattro avrebbe dovuto rifare da sé la stessa domanda — «a che punto è?» — e
 * lo stesso ripiego per il nucleo di prima, che la fase non la manda. Qui la si
 * fa una volta, senza React e senza lingue, così `node --test` la carica da sola
 * (`test/fasiScansione.test.ts`).
 */
import type { Avanzamento } from "./ipc";
import type { Chiave, Valori } from "./lingue";

/**
 * Il momento di una scansione, come si scrive.
 *
 * - `attende`: «Scansiona» aspetta che finisca una scansione partita da sola;
 * - `cerca`: la camminata, con le cartelle e i file trovati finora;
 * - `confronta`: il totale non si sa ancora e la fase non si sa — è il
 *   «confronto col disco…» di sempre, e il primo istante dopo il clic;
 * - `legge`: `fatti` su `totale`.
 */
export type Momento = "attende" | "cerca" | "confronta" | "legge";

/**
 * Il momento di un avanzamento.
 *
 * Senza `fase` — il nucleo di prima, o il segnaposto che `App` mette al clic —
 * vale quel che valeva: i numeri se il totale c'è, il confronto se no. Anche
 * `legge` con il totale ancora a zero è un confronto: fra la camminata e la
 * lettura il nucleo prepara il piano, e una barra «0 / 0» non dice niente.
 */
export function momentoDi(scansione: Avanzamento): Momento {
  if (scansione.fase === "attende") return "attende";
  if (scansione.fase === "cerca") return "cerca";
  return scansione.totale > 0 ? "legge" : "confronta";
}

/**
 * «2.561 cartelle», «29.503 file»: i due conti della camminata, già scritti.
 *
 * Due chiavi col loro plurale e non una frase sola, perché `t` sceglie la forma
 * su un numero solo (`n`): «1 cartelle» sarebbe la prima cosa che si legge.
 * `t` arriva da fuori, come in `menuBrani.ts`, così le prove ne passano una
 * finta.
 */
export function contiDellaCamminata(
  scansione: Avanzamento,
  t: (chiave: Chiave, valori?: Valori) => string,
): { cartelle: string; file: string } {
  return {
    cartelle: t("scan.folders", { n: scansione.cartelle ?? 0 }),
    file: t("scan.files", { n: scansione.trovati ?? 0 }),
  };
}

/**
 * Quel che l'annuncio distingue: niente, un'attesa, una scansione al lavoro.
 *
 * Tre stati e non i quattro momenti: chi ascolta lo schermo deve sapere che è
 * cominciata, che sta aspettando e che è finita. Che la camminata sia diventata
 * lettura è un battito come gli altri, e un avanzamento che parla da solo è
 * rumore — vedi `AnnuncioScansione`.
 */
export type StatoAnnunciato = "ferma" | "attende" | "lavora";

/** Lo stato da annunciare per una scansione, o per nessuna. */
export function statoAnnunciato(scansione: Avanzamento | null): StatoAnnunciato {
  if (scansione === null) return "ferma";
  return momentoDi(scansione) === "attende" ? "attende" : "lavora";
}

/**
 * Che cosa dire passando da uno stato all'altro, o `null` per tacere.
 *
 * Dall'attesa al lavoro si tace: «cominciata» l'ha già detto il clic, e
 * ripeterlo per la stessa scansione la farebbe sembrare un'altra. La fine si
 * dice comunque, da qualunque stato ci si arrivi.
 */
export function annuncioFra(
  prima: StatoAnnunciato,
  adesso: StatoAnnunciato,
): "inizio" | "attesa" | "fine" | null {
  if (prima === adesso) return null;
  if (adesso === "ferma") return "fine";
  if (adesso === "attende") return "attesa";
  return prima === "ferma" ? "inizio" : null;
}
