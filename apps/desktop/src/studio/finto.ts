/**
 * Il mondo finto dell'anteprima.
 *
 * # Perché non c'è più una scenetta
 *
 * Prima l'anteprima disegnava cinque scene scritte a mano: `<div>` con le classi
 * giuste, che somigliavano all'applicazione senza esserlo. Divergevano per
 * costruzione — nessuno le rifà quando l'app cambia — e la divergenza si scopre
 * scrivendo una skin, cioè dopo.
 *
 * Adesso l'anteprima usa **lo stesso renderer** della finestra vera, e questo
 * file è l'unica cosa che cambia: al posto della libreria dell'utente c'è un
 * brano inventato, al posto dei comandi che suonano ci sono funzioni che non
 * fanno niente. Le cinque scene restano, ma sono cinque *stati* dell'unico
 * renderer invece di cinque markup — e la classe di bug «il finto è andato alla
 * deriva» sparisce, perché non c'è più un finto da tenere allineato.
 */
import type { ContestoWidget } from "../Impaginazione";
import type { Brano, StatoRiproduzione } from "../ipc";

/** Le schermate che l'anteprima sa mostrare. */
export type Scena = "libreria" | "riproduzione" | "modale" | "vuoto" | "caricamento";

export const SCENE: readonly (readonly [Scena, string])[] = [
  ["libreria", "Libreria"],
  ["riproduzione", "In riproduzione"],
  ["modale", "Modale"],
  ["vuoto", "Vuoto"],
  ["caricamento", "Caricamento"],
];

const BRANO: Brano = {
  id: 1,
  path: "",
  title: "Corale in mi minore",
  artist: "Anna Vestri",
  album: "Le stanze basse",
  albumKey: "anna vestri|le stanze basse",
  trackNumber: 3,
  discNumber: 1,
  durationMs: 251_000,
  year: 2019,
  coverArtHash: null,
  playCount: 12,
  liked: true,
  rating: 4,
};

const STATO: StatoRiproduzione = {
  brano: BRANO,
  inPausa: false,
  posizioneMs: 96_000,
  durataMs: BRANO.durationMs,
  shuffle: false,
  ripeti: "all",
  volume: 0.72,
  muto: false,
  coda: [1, 2, 3],
  posizioneCoda: 0,
  // Piatto e spento: l'anteprima di una skin mostra come si disegna il lettore,
  // non come suona, e una curva finta darebbe a chi guarda l'impressione che
  // l'equalizzatore sia una decisione della skin.
  eqAttivo: false,
  eqGuadagni: [],
};

/** Un comando che non fa niente: nell'anteprima non c'è niente da comandare. */
const niente = () => {
  /* apposta */
};

/**
 * Il contesto di una scena.
 *
 * Le cinque scene si distinguono per quel che il **mondo** contiene, non per
 * quello che il renderer disegna: «vuoto» è una libreria senza brani, non un
 * markup diverso. È la differenza che rende impossibile alla scena di divergere
 * dall'applicazione.
 */
export function contestoFinto(scena: Scena): ContestoWidget {
  const suona = scena !== "vuoto" && scena !== "caricamento";
  return {
    stato: suona ? STATO : { ...STATO, brano: null, coda: [], durataMs: 0 },
    vista: "album",
    playlistAperta: null,
    inLibreria: true,
    conteggi: suona ? { album: 135, artisti: 110, brani: 1592, preferiti: 41 } : {},
    playlist: [],
    // «In riproduzione» è la terza colonna aperta: è la forma in cui la
    // schermata grande e i comandi stanno insieme.
    colonnaAperta: scena === "riproduzione",
    codaAperta: scena === "modale",
    // Mai: lo schermo intero è l'unica schermata che copre il contenuto, e
    // l'anteprima dello Studio mostra proprio quello. Accendendolo, i tre
    // widget della riproduzione si toglierebbero di mezzo e la scena
    // «riproduzione» non avrebbe più niente da far vedere.
    grande: false,
    selezionati: scena === "modale" ? [1, 2] : [],
    tuttiSelezionati: false,
    onVista: niente,
    onPlaylist: niente,
    onMenuPlaylist: niente,
    onNuovaPlaylist: niente,
    onColonna: niente,
    onCoda: niente,
    onGrande: niente,
    onPreferito: niente,
    onVoto: niente,
    onErrore: niente,
    onSelezioneRiproduci: niente,
    onSelezioneDopo: niente,
    onSelezioneAccoda: niente,
    onSelezionePlaylist: niente,
    onSelezioneTuttiOAnnulla: niente,
    onSelezioneChiudi: niente,
  };
}
