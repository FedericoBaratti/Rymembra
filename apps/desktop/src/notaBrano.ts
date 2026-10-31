/**
 * Cosa sta succedendo al brano in corso, in due parole: la nota della riga in
 * volo della pagina Importazioni (`schermate/Importazioni.tsx`).
 *
 * Fuori dalla pagina perché si possa provare: restituisce la chiave del
 * catalogo, e la pagina la traduce.
 */
import type { BranoScarico } from "./ipc";

type Stato = Pick<BranoScarico, "esito" | "codice" | "frazione">;

export type Nota =
  | {
      chiave:
        | "imports.track.searching"
        | "imports.track.fetching"
        | "imports.track.done"
        | "imports.track.notAllowed"
        | "imports.track.nowhere"
        | "imports.track.failed";
    }
  | { percento: number };

export function notaDi(brano: Stato): Nota {
  switch (brano.esito) {
    case "cerco":
      return { chiave: "imports.track.searching" };
    case "prendo":
      return brano.frazione === null
        ? { chiave: "imports.track.fetching" }
        : { percento: Math.round(brano.frazione * 100) };
    case "fatto":
      return { chiave: "imports.track.done" };
    case "introvabile":
      // Due ragioni, e vanno distinte: «non l'ho trovato» e «l'ho trovato e la
      // licenza non me lo lascia prendere» sono cose diverse per chi legge —
      // la seconda vuol dire che da qualche parte si ascolta.
      return brano.codice === "download.notPermitted"
        ? { chiave: "imports.track.notAllowed" }
        : { chiave: "imports.track.nowhere" };
    default:
      // Era il codice nudo — «net.timeout», «download.http» — scritto nella
      // riga così com'era. Qui ci stanno due parole: il perché, che è una frase
      // intera del catalogo (`errors.<codice>`), va nel suggerimento.
      return { chiave: "imports.track.failed" };
  }
}

/**
 * La chiave della frase lunga di un brano non riuscito, `errors.<codice>`, o
 * `null` se non è un guasto o il codice non c'è. La pagina la passa a `tSe`,
 * che ripiega su «non riuscito» se il catalogo non la conosce.
 */
export function chiaveDelGuasto(brano: Stato): string | null {
  const guasto = brano.esito !== "cerco" && brano.esito !== "prendo" && brano.esito !== "fatto" && brano.esito !== "introvabile";
  return guasto && brano.codice !== null ? `errors.${brano.codice}` : null;
}
