/**
 * I numeri, nella forma in cui si leggono.
 *
 * Stavano in `App.tsx` quando la finestra era una schermata sola. Il lettore
 * mostra gli stessi minuti e secondi dell'elenco, e due `durata()` che si
 * somigliano sono due formati che prima o poi divergono di un carattere — la
 * versione piccola di ciò che è successo fra desktop e Android nel vecchio
 * albero.
 *
 * # Perché la lingua entra qui e non solo nelle etichette
 *
 * «1,5 h» e «1.5 h» sono lo stesso numero scritto da due parti diverse del
 * mondo, e un'interfaccia in inglese che scrive la virgola decimale è più
 * fastidiosa di una parola non tradotta: sembra un errore di calcolo. Le
 * funzioni di qui chiedono quindi la lingua attiva al modulo `lingue` invece di
 * riceverla per argomento — chi formatta un numero è quasi sempre dentro un
 * `map`, e farla scendere fin lì costerebbe una prop in venti componenti.
 */
import { locale, t } from "./lingue";

/**
 * Millisecondi in `m:ss`, o `h:mm:ss` quando serve.
 *
 * Non passa da `Intl`: due cifre separate da due punti si scrivono così in ogni
 * lingua che questa applicazione parlerà, e `Intl.DurationFormat` produrrebbe
 * «3 min 42 s», che in una colonna di duecento righe è rumore.
 */
export function durata(ms: number): string {
  const totale = Math.round(ms / 1000);
  const s = totale % 60;
  const m = Math.floor(totale / 60) % 60;
  const h = Math.floor(totale / 3600);
  const dueCifre = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${dueCifre(m)}:${dueCifre(s)}` : `${m}:${dueCifre(s)}`;
}

/** Millisecondi in ore, per il pannello laterale. */
export function ore(ms: number): string {
  const h = ms / 3_600_000;
  return t("format.hours", {
    n: h >= 10 ? Math.round(h) : Number(h.toFixed(1)),
  });
}

/**
 * Un numero, con i separatori della lingua attiva.
 *
 * L'unico posto in cui questa applicazione chiama `toLocaleString`. Stava
 * scritto `toLocaleString("it")` in quarantacinque punti su diciassette file, e
 * tre volte senza argomento: tre numeri che seguivano il sistema operativo
 * mentre tutti gli altri seguivano l'italiano, senza che niente lo dicesse.
 */
export function numero(n: number, decimali?: number): string {
  return n.toLocaleString(
    locale(),
    decimali === undefined
      ? undefined
      : { minimumFractionDigits: decimali, maximumFractionDigits: decimali },
  );
}

/** «1 brano», «2 brani». Metà della libreria ha un brano solo. */
export function brani_(n: number): string {
  return t("format.tracks", { n });
}

/**
 * Una data e un'ora, come si scrivono nella lingua attiva.
 *
 * Esiste per la stessa ragione di [`numero`]: `toLocaleString("it")` era scritto
 * a mano in diciassette file, e tre volte senza argomento. Con un solo posto,
 * cambiare idea su come si scrive una data è una modifica invece di una caccia.
 */
export function dataOra(ms: number): string {
  return new Date(ms).toLocaleString(locale());
}

/** Solo la data. */
export function data(ms: number): string {
  return new Date(ms).toLocaleDateString(locale());
}

/**
 * I due segnaposti che il nucleo scrive quando il tag non c'è.
 *
 * Sono costanti in `aether-domain/src/album.rs`, e restano italiane di
 * proposito: `library.rs` le scrive **nelle righe del database**, e di lì
 * entrano nella chiave con cui un album si raggruppa. Tradurle là dove nascono
 * vorrebbe dire chiavi d'album diverse fra due avvii con lingue diverse, cioè
 * la stessa raccolta spezzata in due dal solo fatto di aver cambiato lingua.
 *
 * Quindi non si traducono alla scrittura: si riconoscono al disegno. Il
 * confronto è con la stringa italiana perché quella *è* la sentinella — non un
 * testo dell'interfaccia, un valore.
 */
const SENZA_ALBUM = "Album sconosciuto";
const SENZA_ARTISTA = "Artista sconosciuto";

/** Il titolo di un album, con la sentinella tradotta. */
export function titoloAlbum(nome: string): string {
  return nome === SENZA_ALBUM ? t("format.unknownAlbum") : nome;
}

/** Il nome di un artista, con la sentinella tradotta. */
export function nomeArtista(nome: string): string {
  return nome === SENZA_ARTISTA ? t("format.unknownArtist") : nome;
}

/**
 * Se questo nome è un artista vero, e non il segnaposto di un tag mancante.
 *
 * Serve a chi il nome lo usa per **dire qualcosa** — il titolo di una raccolta
 * del lunedì, per esempio — e non per mostrarlo in una riga. Un filtro sul
 * nome già passato da [`nomeArtista`] non basta: la sentinella lì è già
 * diventata «Artista sconosciuto», una stringa non vuota come le altre, e la
 * raccolta finiva per chiamarsi «Artista sconosciuto, Led Zeppelin».
 */
export function artistaNoto(nome: string): boolean {
  return nome !== "" && nome !== SENZA_ARTISTA;
}
