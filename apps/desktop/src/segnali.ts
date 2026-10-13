/**
 * Gli attributi di stato che una skin legge.
 *
 * Il formato 2 anima le parti su dieci stati del DOM (`core/aether-skin/src/
 * stati.rs`), e metà di quegli stati sono già attributi che l'app scriveva per
 * sé — `aria-current`, `aria-selected`, `aria-pressed`, `:disabled`. Gli altri
 * nascono qui, e ognuno ha **un** posto che lo scrive, così una skin che anima
 * «in riproduzione» vede lo stesso segnale sulla colonna, sulla riga e sulla
 * scheda dell'album.
 *
 * - `data-playing` / `data-paused`: questa cosa è quella che suona, e il
 *   lettore va o è fermo. Il compilatore mette le animazioni di `playing` nella
 *   lista di base, in pausa, e questo attributo le fa andare: un vinile che si
 *   ferma resta dov'era invece di tornare a capo.
 * - `data-cambio`: `a` o `b`, e cambia quando cambia il contenuto. Il
 *   compilatore scrive due `@keyframes` gemelle e le alterna su questo
 *   attributo: un'animazione riparte solo se cambia nome. Al primo disegno
 *   manca, perché il primo contenuto non è un cambio — è un'entrata, e
 *   l'entrata ha il suo innesco.
 * - `--indice`: la posizione fra i fratelli, per la cascata. Il compilatore la
 *   legge con `min(var(--indice), 12)`.
 */
import { createContext, useContext, useState, type CSSProperties } from "react";

/** Gli attributi del suono, da spargere su un elemento. */
export interface AttributiSuono {
  "data-playing"?: "";
  "data-paused"?: "";
}

/**
 * Gli attributi del suono di una cosa.
 *
 * `corrente` dice se questa cosa è quella che suona — il lettore lo è sempre
 * quando c'è un brano, una riga solo se è il brano corrente, la scheda di un
 * album solo se il brano corrente è suo. Una cosa che non è corrente non porta
 * niente: né in riproduzione né in pausa, perché non è lei a essere ferma.
 */
export function statoSuono(corrente: boolean, inPausa: boolean): AttributiSuono {
  if (!corrente) return {};
  return inPausa ? { "data-paused": "" } : { "data-playing": "" };
}

/** Quel che suona, per chi non lo riceve per prop. */
export interface QuelCheSuona {
  /** L'identificativo del brano corrente, o `null`. */
  brano: number | null;
  /** La chiave dell'album del brano corrente, o `null`. */
  album: string | null;
  inPausa: boolean;
}

/**
 * Quel che suona, per tutto l'albero: le schede della Home, degli album, il
 * testo.
 *
 * Il valore cambia solo quando cambia il brano o il lettore si ferma — la
 * posizione vive in un negozio suo, `riproduzione.ts` — quindi una griglia di
 * schede si ridisegna a ogni brano nuovo e non a ogni colpo dell'orologio.
 * Lo fornisce `App`; l'anteprima dello Studio fornisce il suo.
 */
export const Suona = createContext<QuelCheSuona>({
  brano: null,
  album: null,
  inPausa: true,
});

export function useQuelCheSuona(): QuelCheSuona {
  return useContext(Suona);
}

/** Gli attributi del suono di un brano. */
export function suonoBrano(ora: QuelCheSuona, id: number | null): AttributiSuono {
  return statoSuono(id !== null && id === ora.brano, ora.inPausa);
}

/** Gli attributi del suono di un album: suona se suona uno dei suoi brani. */
export function suonoAlbum(ora: QuelCheSuona, chiave: string | null): AttributiSuono {
  return statoSuono(chiave !== null && chiave === ora.album, ora.inPausa);
}

/** Il lato del cambio: niente finché il contenuto è quello del primo disegno. */
export type Cambio = "a" | "b" | undefined;

/**
 * `a` o `b`, alternati a ogni cambio di `chiave`; `undefined` prima del primo.
 *
 * Si ricava durante il disegno, confrontando la chiave con quella di prima: è
 * la forma che React raccomanda per uno stato che dipende da una prop, e non
 * c'è il fotogramma in cui l'attributo è ancora quello vecchio col contenuto
 * nuovo — che è precisamente il fotogramma in cui l'animazione deve partire.
 */
export function useCambio(chiave: unknown): Cambio {
  const [stato, setStato] = useState<{ chiave: unknown; lato: Cambio }>({
    chiave,
    lato: undefined,
  });
  if (!Object.is(stato.chiave, chiave)) {
    const lato = stato.lato === "a" ? "b" : "a";
    setStato({ chiave, lato });
    return lato;
  }
  return stato.lato;
}

/**
 * Lo stile che porta la posizione di una voce, per la cascata.
 *
 * `altro` si fonde: chi ha già uno stile in linea non deve sceglierne uno.
 */
export function conIndice(indice: number, altro?: CSSProperties): CSSProperties {
  return { ...altro, "--indice": indice } as CSSProperties;
}
