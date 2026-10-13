/**
 * Cosa farne, dei file lasciati cadere sulla finestra.
 *
 * Quale percorso è cosa lo dice il nucleo (`trascinati.rs`): una cartella, un
 * `.aeskin`, una playlist, un brano che la libreria conosce, uno che non
 * conosce. Qui c'è la decisione che viene dopo, ed è una regola sola per
 * tutta la finestra: `pianoDeiTrascinati` la scrive senza toccare niente, e
 * `test/trascinati.test.ts` la prova; a farla succedere è `App`
 * (`useTrascinati` in `trascinamento.ts` porta l'evento fin lì).
 *
 * # La regola
 *
 * Una skin prima di tutto: si installa, e si atterra nella sezione
 * «Aspetto» delle Impostazioni, perché un'azione che avviene fuori dallo
 * schermo sembra non essere avvenuta. Poi una playlist, che apre la sua
 * finestrella: è un gesto con delle scelte dentro, e non si fa a metà insieme
 * ad altro. Del resto si fa tutto quel che si può: i brani in coda, le
 * cartelle fra le sorvegliate — o nell'elenco del primo avvio, finché quello
 * le sta chiedendo — e se non c'era niente da fare lo si dice.
 *
 * Prima si provava come cartella tutto quel che non era una skin, e un `.m3u8`
 * o un MP3 lasciati cadere finivano fra le cartelle sorvegliate.
 */
import type { Trascinati } from "./ipc";

/** Quel che della finestra decide dove vanno le cartelle. */
export interface SituazioneTrascinati {
  /**
   * Il primo avvio chiede ancora dove sta la musica: è aperto, non c'è una
   * scansione in corso, e la libreria è vuota. Le cartelle vanno nel suo
   * elenco.
   */
  primoInAttesa: boolean;
  /** `avvio` è arrivato: si sa quali cartelle sono già sorvegliate. */
  avviato: boolean;
}

/** Cosa fare dei file lasciati cadere. */
export type PianoTrascinati =
  /** Installare questa skin, e mostrarla. */
  | { tipo: "skin"; percorso: string }
  /** Aprire l'importazione di questa playlist. */
  | { tipo: "playlist"; percorso: string }
  | {
      tipo: "resto";
      /** I brani da mettere in coda, nell'ordine in cui sono arrivati. */
      brani: number[];
      /** Le cartelle, e dove vanno; `null` se non ce ne sono da sistemare. */
      cartelle: { dove: "primo" | "sorvegliate"; percorsi: string[] } | null;
      /** Cosa dire se non si è fatto niente: `null` se si è fatto qualcosa. */
      notizia: "fuoriLibreria" | "niente" | null;
    };

/** La regola: vedi il commento in testa. */
export function pianoDeiTrascinati(
  smistati: Trascinati,
  situazione: SituazioneTrascinati,
): PianoTrascinati {
  const skin = smistati.skin[0];
  if (skin !== undefined) return { tipo: "skin", percorso: skin };
  const playlist = smistati.playlist[0];
  if (playlist !== undefined) return { tipo: "playlist", percorso: playlist };
  const brani = smistati.brani;
  // Col primo avvio che chiede ancora dove sta la musica, le cartelle vanno
  // nel suo elenco: scriverle da qui voleva dire perderle al suo «Continua»,
  // che scrive le spuntate e basta. A lettura partita o finita con dei brani
  // «Continua» non c'è più, e la strada di sempre — che unisce — non perde
  // niente.
  const cartelle =
    smistati.cartelle.length === 0
      ? null
      : situazione.primoInAttesa
        ? { dove: "primo" as const, percorsi: smistati.cartelle }
        : situazione.avviato
          ? { dove: "sorvegliate" as const, percorsi: smistati.cartelle }
          : null;
  // Dopo i brani accodati, che hanno già la loro notizia: questa la
  // sostituirebbe, e i due casi insieme sono rari abbastanza da non meritare
  // una frase che li tenga tutti e due.
  const notizia =
    cartelle !== null || brani.length > 0
      ? null
      : smistati.fuoriLibreria > 0
        ? "fuoriLibreria"
        : smistati.ignorati > 0
          ? "niente"
          : null;
  return { tipo: "resto", brani, cartelle, notizia };
}
