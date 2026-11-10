/**
 * Lo scafale del telefono: dove stanno le cose su uno schermo stretto e alto.
 *
 * Lo scafale di una skin è pensato per una finestra larga almeno 880 pixel —
 * la navigazione di lato, la coda e la terza colonna accanto al contenuto — e
 * il nucleo lo valida contro quella larghezza. Su un telefono in verticale le
 * cose stanno una sopra l'altra: la testata, il contenuto, il lettore, le
 * schede (`stile/telefono.css`).
 *
 * La coda e la terza colonna non ci sono: sul telefono la coda sta dentro «In
 * riproduzione», che è già la colonna a schermo intero, e due modi di aprire la
 * stessa cosa sono un modo di troppo. Su un tablet largo più di 1100 pixel la
 * terza colonna si aprirebbe da sola all'avvio (`terzaColonna.ts`), e a
 * schermo intero, sopra la Home.
 *
 * Della skin resta tutto il resto — colori, caratteri, forme, le parti — perché
 * questo albero usa gli stessi widget e le stesse parti. Gli indirizzi dei nodi
 * (`at`) sono suoi, `telefono-…`: il foglio compilato di una skin misura i
 * nodi del **suo** albero per indirizzo (`[data-nodo='1-0']`), e le sue misure
 * qui non devono valere.
 */
import type { NodoScafale } from "./ipc";

function widget(nome: string, at: string, opzioni: NodoScafale["options"] = {}): NodoScafale {
  return {
    kind: "widget",
    at: `telefono-${at}`,
    name: nome,
    size: nome === "content" ? "fill" : "hug",
    gap: null,
    align: null,
    spread: null,
    part: null,
    fromPrefab: null,
    slot: null,
    options: opzioni,
    children: [],
  };
}

/** L'albero del telefono. Uno solo, per tutte le skin. */
export const SCAFALE_TELEFONO: NodoScafale = {
  kind: "zone",
  at: "telefono",
  name: "column",
  size: "fill",
  gap: null,
  align: "stretch",
  spread: "start",
  part: "app-shell",
  fromPrefab: null,
  slot: null,
  options: {},
  children: [
    widget("ambient", "ambiente"),
    widget("page-header", "testata"),
    widget("content", "contenuto"),
    widget("selection-bar", "selezione"),
    widget("player", "lettore"),
    widget("phone-nav", "navigazione"),
  ],
};
