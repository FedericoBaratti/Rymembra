/**
 * Le playlist omonime nel piano dell'account (D-11, R06-3, R13c-1).
 *
 * Il nucleo non sostituisce più una playlist fatta qui con lo stesso nome:
 * quella dell'archivio le entra accanto, «Chill (Spotify)». Il piano lo deve
 * dire per nome — su un account da duecento playlist non lo si scopre — e deve
 * dire anche quali playlist importate la volta prima verranno riscritte.
 *
 * Fuori da `ipc/importazioni.ts` perché si prova da sola: quello si porta
 * dietro le lingue.
 */
import type { EsitoAccount } from "./ipc/importazioni";

/**
 * Le playlist dell'account che entrano **accanto** a una con lo stesso nome che
 * c'è già (D-11): la prima resta com'è, e il piano lo deve dire.
 */
export function accantoAccount(esito: EsitoAccount): { qui: string; nuova: string }[] {
  return esito.playlists.flatMap((p) =>
    p.playlistBeside != null && p.playlistName !== null
      ? [{ qui: p.playlistBeside, nuova: p.playlistName }]
      : [],
  );
}

/**
 * Le playlist importate la volta prima da questo stesso archivio, che il
 * contenuto di adesso riscrive.
 */
export function riscritteAccount(esito: EsitoAccount): string[] {
  return esito.playlists.flatMap((p) =>
    p.playlistReplaced && p.playlistName !== null ? [p.playlistName] : [],
  );
}
