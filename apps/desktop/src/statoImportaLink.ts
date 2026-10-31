/**
 * Due pezzi di stato di «Importa da un link» (`ImportaLink` in `Importa.tsx`),
 * fuori da React perché si possano provare.
 *
 * # Due errori e non uno
 *
 * La finestrella ha due fonti di guasto che vivono tempi diversi: i gesti
 * dell'utente (leggere il link, «Riprova», «Importa») e il **piano**, che si
 * rifà da sé a ogni cambio della casella «Crea la playlist». Con un errore solo
 * un piano riuscito non lo toglieva: il rifiuto «playlist automatica» restava
 * sotto la casella ormai spenta, col suo tasto che non faceva più niente,
 * mentre «Importa» si era riacceso. E azzerarlo sul piano riuscito avrebbe
 * cancellato anche il guasto di un «Riprova» appena fallito. Così ognuno segue
 * la sua fonte.
 */

/** Gli errori della finestrella: `null` vuol dire nessuno. */
export interface ErroriLink {
  /** Dell'ultimo gesto: la lettura del link o l'importazione. */
  azione: unknown;
  /** Dell'ultimo piano chiesto. */
  piano: unknown;
}

export const NESSUN_ERRORE: ErroriLink = { azione: null, piano: null };

export type EventoLink =
  /** Una lettura nuova: il piano di prima non vale più, e si riparte puliti. */
  | { tipo: "letturaPartita" }
  | { tipo: "letturaFallita"; errore: unknown }
  | { tipo: "pianoArrivato" }
  | { tipo: "pianoFallito"; errore: unknown }
  | { tipo: "esecuzionePartita" }
  | { tipo: "esecuzioneFallita"; errore: unknown };

export function erroriDopo(stato: ErroriLink, evento: EventoLink): ErroriLink {
  switch (evento.tipo) {
    case "letturaPartita":
      return NESSUN_ERRORE;
    case "letturaFallita":
    case "esecuzioneFallita":
      return { ...stato, azione: evento.errore };
    case "esecuzionePartita":
      return { ...stato, azione: null };
    case "pianoArrivato":
      return { ...stato, piano: null };
    case "pianoFallito":
      return { ...stato, piano: evento.errore };
  }
}

/**
 * Il fuoco va su «Importa» adesso?
 *
 * Quando arriva l'anteprima il campo del link sparisce, e il fuoco deve andare
 * sul tasto che importa. Ma in quel disegno il piano non c'è ancora e il tasto
 * è spento, e `focus()` su un tasto spento non fa niente: il fuoco cadeva sulla
 * radice. Quindi lo si dà quando il tasto si **accende**, una volta per
 * anteprima (`servita` è l'ultima a cui è stato dato): un piano rifatto dalla
 * casella non lo deve riprendere a chi si è spostato. Con un'anteprima
 * degradata il tasto resta spento, e il fuoco resta alla radice.
 */
export function fuocoSulPrimario<T>({
  anteprima,
  servita,
  acceso,
}: {
  anteprima: T | null;
  servita: T | null;
  acceso: boolean;
}): boolean {
  return anteprima !== null && anteprima !== servita && acceso;
}
