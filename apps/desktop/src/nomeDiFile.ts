/**
 * Un nome di playlist come nome di file da proporre nel dialogo di
 * salvataggio di «Esporta».
 *
 * Il nome passava intero come `defaultPath`, e il dialogo (`tauri-plugin-dialog`)
 * lo divide in cartella e nome: «AC/DC» proponeva la cartella relativa «AC» e
 * il nome «DC.m3u8», «Live: 2024?» un nome che Windows rifiuta al salvataggio,
 * e «A: lato» veniva letto come un'unità disco. Qui i nove caratteri che
 * Windows vieta (`\ / : * ? " < > |`) diventano un trattino, i caratteri di
 * controllo uno spazio, e via i punti e gli spazi in coda, che Windows toglie
 * da sé lasciando un nome diverso da quello mostrato. Un nome che resta vuoto,
 * o che è uno dei nomi riservati di Windows (CON, NUL, COM1…, anche con
 * un'estensione), ripiega su «playlist».
 *
 * Puro e senza lingue: `test/nomeDiFile.test.ts` lo prova con `node --test`.
 */

/** I caratteri che Windows non accetta in un nome di file. */
const VIETATI = /[\/:*?"<>|]/g;

/** I nomi che Windows riserva ai dispositivi, con o senza estensione. */
const RISERVATI = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?$/i;

export function nomeDiFile(nome: string): string {
  const pulito = nome
    .replace(/\p{Cc}/gu, " ")
    .replace(VIETATI, "-")
    .trim()
    .replace(/[. ]+$/, "");
  return pulito === "" || RISERVATI.test(pulito) ? "playlist" : pulito;
}
