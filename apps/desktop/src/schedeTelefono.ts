/**
 * Le quattro schede del telefono, e dove porta Indietro.
 *
 * Sul telefono la navigazione è una barra in basso con quattro schede — Home,
 * Libreria, Cerca, Altro — e non la colonna di sette destinazioni del computer.
 * Quattro, e non le sette di prima in fila: sette bersagli in
 * trecentosessanta pixel sono sette bersagli troppo piccoli, e la Libreria le
 * sue cinque viste le tiene come pillole sotto la testata, dove si sceglie fra
 * cose dello stesso genere.
 *
 * # Una scheda non è uno stato
 *
 * La scheda accesa si **ricava** da quel che si guarda — la `Pagina` e la
 * `vista` sotto di lei — e non si tiene a parte. Una seconda fonte per la stessa
 * domanda sarebbe di nuovo il difetto che `pagina.ts` racconta: due risposte che
 * divergono, e una barra che dice Libreria sopra la pagina della Home. Così un
 * album aperto da un ripiano della Home lascia accesa la Home, perché `vista`
 * resta `"home"` sotto di lui; e lo stesso disco aperto dagli Album accende la
 * Libreria.
 *
 * # Indietro
 *
 * Android ha una regola che ogni app rispetta e che la mano conosce: Indietro
 * risale **dentro** la scheda fino alla sua prima pagina, poi torna alla Home, e
 * dalla Home esce. La scala delle chiusure di `App.tsx` copre già il dentro —
 * lo schermo intero, il menu, la selezione, la ricerca, l'album, l'artista —
 * e `passoIndietroTelefono` aggiunge quel che sul computer non serve: la
 * playlist aperta, la sezione delle Impostazioni, e il ritorno fra le schede.
 */
import type { Pagina, VistaLibreria } from "./pagina";
import type { NomeIcona } from "./parti/Icone";
import type { Vista } from "./parti/Navigazione";

export type SchedaTelefono = "home" | "libreria" | "cerca" | "altro";

/**
 * Le pillole della Libreria, nell'ordine in cui si leggono.
 *
 * Le playlist per prime: sul computer stanno nella barra laterale, sempre in
 * vista, e sul telefono questa è l'unica porta che hanno. Poi i tre modi di
 * guardare la libreria da dentro, come sul computer, le cartelle e i preferiti.
 */
export const PILLOLE_LIBRERIA: readonly VistaLibreria[] = [
  "playlist",
  "album",
  "artisti",
  "brani",
  "cartelle",
  "preferiti",
];

/** Una vista che è una pillola della Libreria. */
export function eDellaLibreria(vista: Vista): vista is VistaLibreria {
  return (PILLOLE_LIBRERIA as readonly Vista[]).includes(vista);
}

/** I due segmenti della scheda Cerca: in libreria, o fuori (Esplora). */
export type SegmentoCerca = "cerca" | "esplora";

/** A quale scheda appartiene una vista, senza niente aperto sopra. */
function schedaDellaVista(vista: Vista): SchedaTelefono {
  if (vista === "home") return "home";
  if (vista === "cerca" || vista === "esplora") return "cerca";
  if (
    vista === "altro" ||
    vista === "impostazioni" ||
    vista === "importazioni" ||
    vista === "dispositivo"
  ) {
    return "altro";
  }
  return "libreria";
}

/**
 * La scheda accesa per la pagina che si guarda.
 *
 * La ricerca è sempre di Cerca, qualunque sia la vista sotto: sul telefono il
 * campo sta solo lì, e una ricerca rimasta da una sessione di prima si
 * ritrova dove la si può svuotare. Album, artista e playlist stanno nella
 * scheda da cui li si è aperti.
 */
export function schedaDi(pagina: Pagina, vista: Vista): SchedaTelefono {
  switch (pagina.tipo) {
    case "ricerca":
    case "cerca":
    case "esplora":
      return "cerca";
    case "altro":
    case "impostazioni":
    case "importazioni":
    case "dispositivo":
      return "altro";
    case "home":
      return "home";
    case "libreria":
      return "libreria";
    case "playlist":
    case "album":
    case "artista":
      return schedaDellaVista(vista);
  }
}

/**
 * La vista da cui comincia una scheda.
 *
 * La Libreria riparte dall'ultima pillola guardata, e Cerca dall'ultimo
 * segmento: tornare a una scheda vuol dire ritrovarla com'era, che è quel che
 * fa ogni app con le schede in basso.
 */
export function radiceDellaScheda(
  scheda: SchedaTelefono,
  pillola: VistaLibreria,
  segmento: SegmentoCerca,
): Vista {
  switch (scheda) {
    case "home":
      return "home";
    case "libreria":
      return pillola;
    case "cerca":
      return segmento;
    case "altro":
      return "altro";
  }
}

/** Quel che Indietro fa sul telefono, dopo la scala comune di `App.tsx`. */
export type PassoIndietro =
  /** Chiude la playlist aperta. */
  | "playlist"
  /** Dalla sezione delle Impostazioni all'elenco delle sezioni. */
  | "sezioni"
  /** Va a un'altra vista. */
  | { vai: Vista }
  /** Niente da chiudere: l'app va in secondo piano. */
  | null;

/**
 * Il passo di Indietro quando la scala comune non ha chiuso niente.
 *
 * Nell'ordine: la playlist aperta, la sezione delle Impostazioni, le pagine
 * che si aprono da Altro — Impostazioni, Importazioni e «Sfoglia» di un
 * dispositivo, che tornano ad Altro —
 * e infine la radice di qualunque scheda, che torna alla Home. Dalla Home non
 * c'è un passo: `null`, e `MainActivity` manda l'app dietro senza chiuderla.
 *
 * Una sezione aperta **da fuori** (`sezioneDaFuori`: «Collega un
 * dispositivo» o «Cartelle della musica» in Altro, un avviso) non passa
 * dall'elenco delle sezioni: chi ci è entrato da Altro non l'ha mai visto, e
 * Indietro lo mostrava al posto di Altro.
 */
export function passoIndietroTelefono({
  vista,
  playlistAperta,
  dentroLaSezione,
  sezioneDaFuori = false,
}: {
  vista: Vista;
  playlistAperta: boolean;
  dentroLaSezione: boolean;
  sezioneDaFuori?: boolean;
}): PassoIndietro {
  if (playlistAperta) return "playlist";
  if (vista === "impostazioni" && dentroLaSezione && !sezioneDaFuori) return "sezioni";
  if (vista === "impostazioni" || vista === "importazioni" || vista === "dispositivo") {
    return { vai: "altro" };
  }
  if (vista !== "home") return { vai: "home" };
  return null;
}

/** Le schede in fila, con la loro icona. Il testo lo mette chi le disegna. */
export const SCHEDE: readonly (readonly [SchedaTelefono, NomeIcona])[] = [
  ["home", "i-home"],
  ["libreria", "i-libreria"],
  ["cerca", "i-search"],
  ["altro", "i-dots"],
];
