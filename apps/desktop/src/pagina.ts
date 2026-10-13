/**
 * Quale pagina si sta guardando.
 *
 * Non è uno stato solo: è la precedenza fra `vista`, la ricerca, la playlist
 * aperta, l'album aperto e l'artista aperto. Per anni quella precedenza è
 * stata scritta due volte in `App.tsx`, una per l'intestazione e una per il
 * corpo, e le due potevano divergere — e divergevano: aprire un album dalla
 * pagina di un artista dava le tracce dell'album sotto il titolo
 * dell'artista, senza copertina, senza «Riproduci», e con un tasto
 * «‹ Artisti» che portava **avanti** nella pagina dell'album.
 *
 * Adesso la scrive questa funzione, una volta, e l'intestazione e il corpo
 * guardano lo stesso risultato. Lo guarda anche lo Studio, che sceglie la
 * pagina da mettere sulla tela con le stesse parole.
 *
 * # Perché la Home sta in fondo e non in cima
 *
 * Sopra la ricerca voleva dire che dalla schermata su cui l'applicazione apre
 * la ricerca non funzionava: quel che si sta cercando è ciò che si vuole
 * vedere. Sopra l'album voleva dire che un disco aperto da un ripiano della
 * Home avrebbe rimesso la Home: `apriAlbum` scrive l'album e lascia `vista`
 * dov'è. Le pagine di servizio — Esplora, importazioni, impostazioni — restano
 * in cima perché là non c'è niente da cercare in libreria e niente da aprire.
 */
import type { Album, Artista, Playlist } from "./ipc";
import type { Vista } from "./parti/Navigazione";

/** Le pagine di servizio: non cercano in libreria e non aprono niente. */
export type PaginaDiServizio = "esplora" | "importazioni" | "impostazioni";

/** Le viste della libreria, quando niente è aperto sopra di loro. */
export type VistaLibreria = Exclude<Vista, PaginaDiServizio | "home">;

// Le tre di servizio una per una e non `{ tipo: PaginaDiServizio }`: così
// ogni confronto su `tipo` ne toglie una, e dopo le tre resta la libreria.
export type Pagina =
  | { tipo: "esplora" }
  | { tipo: "importazioni" }
  | { tipo: "impostazioni" }
  | { tipo: "ricerca" }
  | { tipo: "playlist"; playlist: Playlist }
  | { tipo: "album"; album: Album }
  | { tipo: "artista"; artista: Artista }
  | { tipo: "home" }
  | { tipo: "libreria"; vista: VistaLibreria };

/** Quel che decide la pagina. */
export interface StatoPagina {
  vista: Vista;
  /** C'è qualcosa nella casella di ricerca. */
  cercando: boolean;
  playlist: Playlist | null;
  album: Album | null;
  artista: Artista | null;
}

function diServizio(vista: Vista): vista is PaginaDiServizio {
  return vista === "esplora" || vista === "importazioni" || vista === "impostazioni";
}

/** La pagina che si sta guardando. Aggiungendo un caso, va aggiunto qui. */
export function paginaCorrente(stato: StatoPagina): Pagina {
  if (diServizio(stato.vista)) return { tipo: stato.vista };
  if (stato.cercando) return { tipo: "ricerca" };
  if (stato.playlist !== null) return { tipo: "playlist", playlist: stato.playlist };
  if (stato.album !== null) return { tipo: "album", album: stato.album };
  if (stato.artista !== null) return { tipo: "artista", artista: stato.artista };
  if (stato.vista === "home") return { tipo: "home" };
  return { tipo: "libreria", vista: stato.vista };
}
