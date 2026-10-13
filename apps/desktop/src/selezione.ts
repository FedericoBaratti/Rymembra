/**
 * Che cosa vuol dire un clic su una riga, per la selezione: la regola, senza
 * lo stato.
 *
 * Lo stato — la mappa identificativo → posizione, le pagine da chiedere per un
 * intervallo che passa da righe non in mano — sta in `useSelezione`
 * (`selezioneVista.ts`), che riceve gli elenchi da `App`. Qui c'è soltanto la
 * decisione, perché è quella che si sbagliava senza che nessuno lo vedesse
 * finché non provava una sequenza precisa di clic: `test/selezione.test.ts`
 * le prova tutte. E, in fondo, le altre due regole che si possono dire senza
 * lo stato: di quale elenco è una selezione (`chiaveDellaSelezione`) e in che
 * ordine la si manda (`nellOrdineDellaVista`).
 *
 * # L'àncora
 *
 * L'àncora è la riga da cui Maiusc+clic conta l'intervallo, ed è una
 * **posizione nella vista** — il numero d'ordine della riga nell'elenco intero,
 * anche quando la finestra ne ha in mano poche pagine — non l'indice di una
 * riga caricata. Si sposta con ogni clic che non sia un intervallo: con Ctrl
 * (che sceglie o toglie quella riga) e **anche col clic semplice**.
 *
 * Il clic semplice non la spostava. Chiude la selezione aperta, e basta; ma
 * l'àncora restava sull'ultima riga toccata con Ctrl, magari tre schermate
 * più su, e il Maiusc+clic dopo un clic semplice partiva da lì invece che dalla
 * riga appena toccata. È la convenzione di ogni gestore di file — Esplora
 * risorse, il Finder, foobar2000 — ed è quella che la mano si aspetta: «da qui
 * a lì» comincia dall'ultimo «qui» toccato.
 *
 * L'intervallo invece l'àncora **non** la sposta: Maiusc+clic su una riga e
 * poi su un'altra allarga o stringe l'intervallo dallo stesso punto, che è il
 * modo di correggere un intervallo preso una riga troppo lungo.
 */
import type { Pagina } from "./pagina";

/** Un clic su una riga: dove, e con quali modificatori. */
export interface ClicSullaRiga {
  /** La posizione della riga nella vista. */
  indice: number;
  /** Maiusc premuto. */
  maiuscolo: boolean;
  /** Ctrl premuto (o ⌘, che su Windows non c'è ma non costa niente). */
  controllo: boolean;
}

/** Cosa fa il clic alla selezione. */
export type GestoDiSelezione =
  /** Sceglie le righe da `da` ad `a`, comprese, al posto di quelle di prima. */
  | { tipo: "intervallo"; da: number; a: number }
  /** Sceglie o toglie la sola riga premuta, lasciando le altre. */
  | { tipo: "alterna" }
  /** Nessun modificatore: chiude la selezione aperta, se ce n'è una. */
  | { tipo: "semplice" };

/**
 * Il gesto di un clic, e l'àncora che resta dopo.
 *
 * Maiusc senza un'àncora — niente ancora toccato in questa vista — vale come
 * un clic semplice: non c'è un «da» da cui contare, e inventarne uno (la prima
 * riga, per esempio) vorrebbe dire scegliere di colpo righe che nessuno ha
 * guardato.
 */
export function gestoDelClic(
  clic: ClicSullaRiga,
  ancora: number | null,
): { gesto: GestoDiSelezione; ancora: number | null } {
  if (clic.maiuscolo && ancora !== null) {
    return {
      gesto: {
        tipo: "intervallo",
        da: Math.min(ancora, clic.indice),
        a: Math.max(ancora, clic.indice),
      },
      ancora,
    };
  }
  if (clic.controllo) return { gesto: { tipo: "alterna" }, ancora: clic.indice };
  return { gesto: { tipo: "semplice" }, ancora: clic.indice };
}

/**
 * Di quale elenco è una selezione: una chiave, che cambia quando l'elenco cambia.
 *
 * La selezione non sopravvive al cambio di elenco: cinquanta brani scelti fra
 * i Preferiti non significano niente dentro un album. Ma le viste tenute
 * montate se la ritrovano tornando, e la ritrovano sotto questa chiave. Quella
 * dei Brani porta l'ordinamento: le posizioni ricordate sono di quell'ordine,
 * e in un altro non vorrebbero dire niente. Quella di una playlist porta
 * `updatedAt`: una playlist cambiata è un altro elenco, e le posizioni di
 * prima non vi indicano più gli stessi brani.
 *
 * `tenuta` è la vista tenuta montata che si guarda, o `null` (vedi
 * `VISTE_TENUTE` in `App.tsx`).
 */
export function chiaveDellaSelezione(
  pagina: Pagina,
  tenuta: string | null,
  ordine: string,
  query: string,
): string {
  if (tenuta === "brani") return `brani:${ordine}`;
  if (tenuta !== null) return tenuta;
  switch (pagina.tipo) {
    case "ricerca":
      return `ricerca:${query}`;
    case "playlist":
      return `playlist:${pagina.playlist.id}:${pagina.playlist.updatedAt}`;
    case "album":
      return `album:${pagina.album.albumKey}`;
    case "artista":
      return `artista:${pagina.artista.name}`;
    case "libreria":
      return pagina.vista;
    default:
      return pagina.tipo;
  }
}

/**
 * I selezionati, nell'ordine della vista.
 *
 * Da un elenco pieno (`pieni`) si filtra come sempre, ed è quel che tiene i
 * doppioni di una playlist. Da una lista rada (`pieni` a `null`) si ordina per
 * la posizione ricordata al momento della scelta: le righe scelte con
 * «Seleziona tutto» non sono quasi mai in mano, e l'ordine è quel che i
 * comandi della barra — «Riproduci», «In coda» — devono rispettare.
 */
export function nellOrdineDellaVista(
  scelti: ReadonlyMap<number, number>,
  pieni: readonly { id: number }[] | null,
): number[] {
  return pieni !== null
    ? pieni.filter((b) => scelti.has(b.id)).map((b) => b.id)
    : [...scelti.entries()].sort((x, y) => x[1] - y[1]).map(([id]) => id);
}
