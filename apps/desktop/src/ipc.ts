/**
 * Il confine con il nucleo.
 *
 * Ogni chiamata al processo nativo passa da qui, e da nessun'altra parte: i tipi
 * di questo file sono il contratto, e tenerli in un posto solo è ciò che rende
 * visibile in un `git diff` il momento in cui cambia.
 *
 * Gli errori arrivano come record — codice, dominio, gravità, ritentabilità,
 * chiave di traduzione — non come stringhe. Nel vecchio albero arrivavano come
 * testo, e il risultato è che l'interfaccia mostrava «2» per un guasto di
 * riproduzione: il codice del motore veniva scartato per strada.
 */
import { invoke } from "@tauri-apps/api/core";

/** Un errore, come lo manda il nucleo. */
export interface ErroreIpc {
  code: string;
  domain: string;
  severity: "info" | "warning" | "error" | "fatal";
  retryable: boolean;
  i18nKey: string;
  message: string | null;
  cause: string | null;
}

/** È un errore del nucleo, o qualcosa di inatteso? */
export function eErroreIpc(value: unknown): value is ErroreIpc {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "domain" in value &&
    "severity" in value
  );
}

/** Il testo da mostrare per un errore qualsiasi. */
export function testoErrore(value: unknown): string {
  if (eErroreIpc(value)) {
    return value.message ?? value.cause ?? value.code;
  }
  return value instanceof Error ? value.message : String(value);
}

/** Un brano, come lo mostra una lista. */
export interface Brano {
  id: number;
  path: string;
  title: string;
  artist: string;
  album: string;
  albumKey: string | null;
  trackNumber: number | null;
  discNumber: number | null;
  durationMs: number;
  year: number | null;
  coverArtHash: string | null;
  playCount: number;
  liked: boolean;
  rating: number;
}

/** Un album, come lo mostra una griglia. */
export interface Album {
  albumKey: string;
  title: string;
  artist: string;
  year: number | null;
  genre: string | null;
  totalTracks: number;
  coverArtHash: string | null;
}

/** Quanto c'è in libreria. */
export interface Numeri {
  tracks: number;
  albums: number;
  artists: number;
  liked: number;
  durationMs: number;
}

/** Lo stato all'avvio. */
export interface Avvio {
  dataDir: string;
  migrazioni: number;
  fts5: boolean;
  cartelle: string[];
  numeri: Numeri;
}

/** Cosa ha fatto una scansione. */
export interface EsitoScansione {
  inseriti: number;
  aggiornati: number;
  spostati: number;
  tolti: number;
  illeggibili: number;
  copertineNuove: number;
  durataMs: number;
  numeri: Numeri;
}

/** L'avanzamento di una scansione. */
export interface Avanzamento {
  fatti: number;
  totale: number;
}

/** Come ordinare un elenco di brani. */
export type Ordine = "scaffale" | "recenti" | "ascoltati" | "titolo";

/** Una skin compilata. */
export interface Skin {
  id: string;
  /** Il foglio, già CSS. Nessun valore scritto dall'autore vi è finito dentro. */
  css: string;
  cost: number;
  /** I token che seguiranno la copertina quando ci sarà la riproduzione. */
  dynamicTokens: string[];
}

export const ipc = {
  avvio: () => invoke<Avvio>("avvio"),
  impostaCartelle: (cartelle: string[]) =>
    invoke<void>("imposta_cartelle", { cartelle }),
  scansiona: () => invoke<EsitoScansione>("scansiona"),
  cerca: (query: string, limite = 60) =>
    invoke<Brano[]>("cerca", { query, limite }),
  brani: (ordine: Ordine, offset: number, limite: number) =>
    invoke<Brano[]>("brani", { ordine, offset, limite }),
  album: (offset: number, limite: number) =>
    invoke<Album[]>("album", { offset, limite }),
  braniAlbum: (chiave: string) =>
    invoke<Brano[]>("brani_album", { chiave }),
  preferito: (id: number, valore: boolean) =>
    invoke<void>("preferito", { id, valore }),
  skin: (id?: string) => invoke<Skin>("skin", { id: id ?? null }),
};

/**
 * Applica una skin alla finestra.
 *
 * Un foglio a parte e non le proprietà scritte una a una su `style`: sostituire
 * il testo di un `<style>` è **un'unica** invalidazione per il motore di
 * rendering, mentre cinquanta `setProperty` sono cinquanta ricalcoli sull'intero
 * albero. Conta quando la skin cambierà dal vivo mentre la si costruisce.
 *
 * `data-skin` va messo dopo: il selettore del foglio è
 * `:root[data-skin='<id>']`, e metterlo prima significherebbe un fotogramma in
 * cui l'attributo c'è e le regole no.
 */
export function applicaSkin(skin: Skin): void {
  const id = "skin-attiva";
  const foglio =
    document.getElementById(id) ?? document.createElement("style");
  foglio.id = id;
  foglio.textContent = skin.css;
  if (!foglio.isConnected) document.head.append(foglio);
  document.documentElement.dataset.skin = skin.id;
}

/**
 * L'indirizzo di una copertina.
 *
 * Non passa dall'IPC: le immagini le chiede il motore di rendering al
 * protocollo `aether-cover`, in parallelo e con la sua cache. Novecento
 * copertine in base64 dentro delle risposte JSON sarebbero novecento stringhe
 * da tenere vive in memoria per disegnare dei quadratini.
 *
 * Su Windows il protocollo si raggiunge come `http://<schema>.localhost/…`.
 */
export function urlCopertina(
  hash: string | null,
  miniatura = true,
): string | null {
  if (!hash) return null;
  const nome = miniatura ? `${hash}.t` : hash;
  return `http://aether-cover.localhost/${nome}`;
}
