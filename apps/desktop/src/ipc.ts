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

/**
 * Cosa dire, per i codici che arrivano davanti a chi usa l'applicazione.
 *
 * Non tutti i codici del catalogo: solo quelli che una persona può leggere sullo
 * schermo. Il nucleo manda `message` soltanto quando ha qualcosa di specifico da
 * aggiungere — un percorso, uno stato HTTP — e per la maggior parte dei guasti
 * previsti non ce l'ha, perché il *codice* è già l'informazione. Senza questa
 * tabella `testoErrore` ripiegava su `code`, e chi sbagliava una lettera nel
 * link di Spotify leggeva `spotify.notPublic`.
 *
 * Ogni voce dice **cosa fare**, non solo cosa è successo: un messaggio che
 * constata e basta lascia la persona esattamente dov'era.
 */
const TESTI: Record<string, string> = {
  // Sono indistinguibili di là — Spotify risponde allo stesso modo a un
  // contenuto privato e a uno cancellato — quindi vanno detti tutti e due.
  "spotify.notPublic":
    "Questo contenuto non è pubblico, oppure il link non esiste più. Controlla il link, o rendi pubblica la playlist su Spotify.",
  "spotify.resolveFailed":
    "Non si è riusciti a leggere questo link. Apri «Perché non funziona?» qui sotto per sapere dove si è fermato.",
  "spotify.tokenUnavailable":
    "Spotify non ha rilasciato il gettone anonimo. Di solito è passeggero: riprova fra qualche istante.",
  "download.unrecognizedUrl":
    "Questo non sembra un link di Spotify. Vanno bene gli indirizzi «open.spotify.com», quelli brevi «spotify.link» e gli URI «spotify:album:…».",
  "net.offline": "Nessuna connessione a Internet.",
  "net.timeout": "Il server non ha risposto in tempo. Riprova.",
  "net.rateLimited":
    "Troppe richieste di fila: il servizio ha chiesto di rallentare. Aspetta un minuto e riprova.",
  "net.http": "Il server ha risposto con un errore.",
  "library.playlistNameInvalid":
    "Questo nome non identifica nessuna playlist: serve almeno una lettera o una cifra.",
  "library.playlistIsSmart":
    "Questa playlist è automatica: il suo contenuto lo decidono le regole, e le righe aggiunte a mano sparirebbero al primo ricalcolo.",
  // I sei dell'account. Ognuno dice **cosa fare**, perché sono i sei momenti in
  // cui una persona resta ferma senza sapere da che parte girarsi.
  "spotify.accountNotConfigured":
    "Manca l'identificativo dell'applicazione Spotify. Creane una su developer.spotify.com/dashboard, con «http://127.0.0.1» come Redirect URI, e incolla qui il Client ID.",
  // Sono due cose diverse e Spotify non dice quale: vanno dette tutte e due.
  "spotify.accountForbidden":
    "Spotify ha rifiutato l'accesso. O questo account non è fra quelli abilitati nella dashboard dell'applicazione, oppure chi ha registrato l'applicazione non ha più Spotify Premium — da febbraio 2026 è un requisito, e quando scade l'app smette di funzionare senza avvisare.",
  "spotify.accountAuthExpired":
    "Il collegamento con Spotify non vale più. Ricollega l'account.",
  "spotify.quotaExceeded":
    "La quota giornaliera dell'applicazione Spotify è esaurita. Non passa riprovando: riprova domani, oppure importa dall'archivio, che non ha quote.",
  "spotify.archiveUnreadable":
    "Questo file non si apre come archivio: può essere stato scaricato a metà. Riscaricalo da Spotify.",
  "spotify.archiveEmpty":
    "L'archivio si apre ma non contiene niente che Aether sappia leggere. Spotify ne manda più d'uno: cerca quello con dentro «Playlist1.json» o «Streaming_History_Audio».",
};

/**
 * Il testo da mostrare per un errore qualsiasi.
 *
 * La tabella per prima, e non `message`, perché `message` è il dettaglio tecnico
 * che il nucleo aggiunge per chi legge i registri: quando c'è tutti e due, quello
 * scritto per una persona è questo.
 */
export function testoErrore(value: unknown): string {
  if (eErroreIpc(value)) {
    return TESTI[value.code] ?? value.message ?? value.cause ?? value.code;
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

/** Un artista, come lo mostra la griglia. */
export interface Artista {
  /** Il nome come sta nei tag. */
  name: string;
  /** Il nome sotto cui ordinarlo, senza articolo: «The Cure» sta sotto C. */
  sortName: string;
  albums: number;
  tracks: number;
  /** Fino a quattro copertine, per il mosaico due per due. */
  covers: string[];
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
  /** È stata fermata a metà: quel che ha letto è scritto, il resto no. */
  annullata: boolean;
  numeri: Numeri;
}

/** L'avanzamento di una scansione. */
export interface Avanzamento {
  fatti: number;
  totale: number;
}

/**
 * Una playlist, coi suoi numeri.
 *
 * `key` è il nome normalizzato ed è l'identità che attraversa la
 * sincronizzazione: due playlist con lo stesso nome sono la stessa playlist, e
 * rinominarne una ne fa una nuova. `id` è solo la riga, e serve ai comandi.
 */
export interface Playlist {
  id: number;
  key: string;
  name: string;
  description: string | null;
  /** Automatica: l'appartenenza la decidono le regole, non le righe. */
  isSmart: boolean;
  tracks: number;
  durationMs: number;
  updatedAt: number;
}

/** Come ordinare un elenco di brani. */
export type Ordine = "scaffale" | "recenti" | "ascoltati" | "titolo";

/** Come si ripete: niente, questo brano, tutta la coda. */
export type Ripetizione = "off" | "one" | "all";

/**
 * Lo stato della riproduzione.
 *
 * `coda` sono solo identificativi, ed è voluto: mandare millequattrocento righe
 * intere a ogni cambio di brano vorrebbe dire spedire qualche megabyte per
 * aggiornare un titolo. Le righe si chiedono con `braniPerId` quando si apre il
 * pannello della coda.
 */
export interface StatoRiproduzione {
  brano: Brano | null;
  inPausa: boolean;
  posizioneMs: number;
  durataMs: number;
  shuffle: boolean;
  ripeti: Ripetizione;
  volume: number;
  muto: boolean;
  coda: number[];
  posizioneCoda: number | null;
  eqAttivo: boolean;
  eqGuadagni: number[];
}

/** Solo il tempo che passa: arriva quattro volte al secondo mentre suona. */
export interface Tempo {
  posizioneMs: number;
  durataMs: number;
  inPausa: boolean;
}

/**
 * Quante bande ha l'equalizzatore, e dove stanno.
 *
 * Ricopiate da `aether_play::equalizzatore::CENTRI_HZ`, che è l'originale. Qui
 * servono solo per scrivere le etichette sotto i cursori: quante ne arrivano
 * davvero lo dice `eqGuadagni`, e il disegno si adatta a quello.
 */
export const CENTRI_EQ = [
  31.25, 62.5, 125, 250, 500, 1000, 2000, 4000, 8000, 16000,
] as const;

/** Di quanto si può alzare o abbassare una banda, in decibel. */
export const LIMITE_EQ_DB = 12;

/** Solo la curva dell'equalizzatore: arriva a ogni cambio di un cursore. */
export interface StatoEq {
  attivo: boolean;
  guadagni: number[];
}

/** Una curva fra cui scegliere. */
export interface VocePreset {
  nome: string;
  guadagni: number[];
  /** Viene con l'applicazione: non si può cancellare. */
  diSerie: boolean;
}

/**
 * Cosa l'importazione dal vecchio database porterebbe, o ha portato.
 *
 * `nonRitrovati` è la voce che conta: sono brani che l'utente ascoltava e che
 * dal disco sono spariti. Vanno mostrati, non contati e basta — altrimenti
 * l'importazione dichiara successo mentre lascia indietro cinquanta ascolti.
 */
export interface EsitoImportazione {
  legacyTracks: number;
  legacyWithStats: number;
  matched: number;
  unmatched: string[];
  playCountCarried: number;
  ratingsCarried: number;
  likedCarried: number;
  historyRows: number;
  historyOrphans: number;
  playlists: number;
  playlistEntries: number;
  playlistOrphans: number;
  tombstones: number;
}

// ── importazione da Spotify ────────────────────────────────────────────────

/**
 * Un livello del lettore keyless che non ha risposto.
 *
 * Ce ne sono tre in cascata — Pathfinder, pagina embed, oEmbed — e questo è
 * l'elenco di quelli scartati prima di quello che ha funzionato. Serve alla
 * diagnosi: gli endpoint sono interni e volatili, e «ha risposto il terzo»
 * distingue una rotazione di Spotify da un computer offline.
 */
export interface LivelloFallito {
  livello: string;
  perche: string;
}

/**
 * Un elenco arrivato più corto di quanto Spotify dichiari.
 *
 * `null` quando è completo. Non è un dettaglio da nascondere: una playlist
 * importata a metà **in silenzio** è il guasto peggiore possibile qui, perché
 * assomiglia in tutto a un successo.
 */
export interface Troncatura {
  letti: number;
  attesi: number;
}

/** Cosa c'è dietro un link, prima di guardare la libreria. */
export interface AnteprimaSpotify {
  /** `brano`, `album`, `playlist` o `artista`. */
  genere: string;
  id: string;
  titolo: string;
  autore: string | null;
  /**
   * La copertina come `data:` URI, già scaricata dal nucleo.
   *
   * **Non** un indirizzo di Spotify: la politica dei contenuti della finestra
   * (`tauri.conf.json`) ammette fra le immagini solo `data:` e il protocollo
   * locale delle copertine, e allargarla per sempre a un dominio esterno per una
   * miniatura è quel che questa applicazione non fa. Va quindi in `src` così
   * com'è, senza passare da `urlCopertina` — quella serve la libreria.
   */
  copertina: string | null;
  brani: number;
  /** Quale livello ha risposto: `pathfinder`, `embed` o `oembed`. */
  sorgente: string;
  troncato: Troncatura | null;
  falliti: LivelloFallito[];
}

/** Un brano di Spotify che in libreria non c'è. */
export interface BranoMancante {
  /** La posizione nell'elenco di Spotify, da 1. */
  position: number;
  title: string;
  artist: string | null;
  album: string | null;
}

/**
 * Cosa l'importazione da Spotify porterebbe, o ha portato.
 *
 * `missingTracks` è la voce che conta, per lo stesso motivo di
 * `EsitoImportazione.unmatched`: sono brani che l'utente ha su Spotify e non su
 * questo disco, e sono l'unica cosa che non può ricostruire dopo.
 */
export interface EsitoSpotify {
  kind: string;
  /** Quale livello del lettore ha risposto: `pathfinder`, `embed`, `oembed`. */
  source: string;
  /**
   * L'identificativo del contenuto su Spotify.
   *
   * **Non** `source`, che nonostante il nome è il livello del lettore. È questo
   * che combacia con `SorgenteScarico.sourceId`, ed è così che la finestrella
   * dice all'elenco quale importazione ha appena creato.
   */
  sourceId: string;
  title: string;
  /** Quanti brani sono stati letti da Spotify. */
  resolved: number;
  matched: number;
  missing: number;
  /** Artista, titolo e album coincidenti. */
  matchedExact: number;
  /** Artista e titolo, album diverso. */
  matchedByTitle: number;
  /** Ritrovati dopo aver tolto le decorazioni dai titoli. */
  matchedStripped: number;
  missingTracks: BranoMancante[];
  playlistId: number | null;
  playlistName: string | null;
  playlistCreated: boolean;
  /** Esisteva già e il contenuto è stato sostituito: è ciò che rende idempotente una seconda importazione. */
  playlistReplaced: boolean;
  playlistEntries: number;
  spotifyAlbumIdsWritten: number;
  /** Oggi sempre 0: Spotify non espone più l'ISRC. Il campo resta perché può tornare. */
  isrcWritten: number;
  /** Quante righe di «lista desideri» sono state scritte. */
  wantedRows: number;
  truncated: { read: number; expected: number } | null;
}


// ── l'account Spotify intero ─────────────────────────────────
// Due vie che finiscono nello stesso posto: il consenso OAuth e l'archivio che
// Spotify manda per posta producono lo stesso valore di là, quindi da qui in giù
// i comandi sono gli stessi. È il motivo per cui questa è una schermata sola e
// non due.

/** Un file dell'archivio che non si è aperto. */
export interface FileIlleggibile {
  nome: string;
  perche: string;
}

/**
 * Cosa si è letto di un account, prima di guardare la libreria.
 *
 * Una forma sola per tutte e due le vie: i campi che riguardano solo l'archivio
 * (`letti`, `ignorati`, …) arrivano vuoti quando si viene dall'API, e viceversa.
 * `provenienza` dice quale delle due è stata.
 */
export interface AnteprimaAccount {
  /** `api` o `archivio`. */
  provenienza: string;
  profilo: string | null;
  spotifyUserId: string | null;
  /**
   * Questa via porta una cronologia degna di quel nome?
   *
   * Falso per l'API, che ne dà cinquanta righe e basta. Serve a non far sembrare
   * un guasto il limite di un endpoint: «50 ascolti» da lassù sono tutto quel che
   * c'è, non un'importazione andata male.
   */
  cronologiaCompleta: boolean;
  playlist: number;
  braniInPlaylist: number;
  preferiti: number;
  album: number;
  artisti: number;
  cronologia: number;
  podcast: number;

  // solo dalla Web API
  /**
   * L'account ha Premium?
   *
   * `null` quando Spotify non l'ha detto — che è diverso da «no», e dirlo
   * sbagliato sarebbe un allarme falso. Dal febbraio 2026 un'applicazione in
   * Development Mode smette di funzionare quando il suo proprietario perde
   * l'abbonamento, e Spotify non manda nessun avviso: questo è l'unico modo di
   * dirlo prima.
   */
  premium: boolean | null;
  /**
   * Le playlist di cui Spotify non dà più il contenuto.
   *
   * Dal marzo 2026 i brani si leggono solo di quelle che l'utente possiede o in
   * cui collabora. Vanno mostrate: una playlist vuota senza spiegazione sembra
   * un guasto dell'abbinamento, e non lo è.
   */
  senzaContenuto: string[];
  /** Gli elenchi arrivati a metà. Vuoto su qualunque account vero. */
  troncati: string[];

  // solo dall'archivio
  letti: string[];
  ignorati: string[];
  illeggibili: FileIlleggibile[];
  righeIlleggibili: number;
  nonMusica: number;
}

/** Cosa portarsi dietro. Tutto acceso di serie. */
export interface ScelteAccount {
  playlist: boolean;
  preferiti: boolean;
  album: boolean;
  artisti: boolean;
  cronologia: boolean;
}

/** Una playlist che non si è potuta importare, e perché. */
export interface PlaylistRifiutata {
  name: string;
  /** Il codice del catalogo: `spotify.tracklistTruncated`, … */
  code: string;
}

/** Quel che della cronologia non è diventato un ascolto. */
export interface ScartiCronologia {
  duplicates: number;
  tooShort: number;
  /**
   * Brani che in libreria non ci sono.
   *
   * **Non** finiscono fra i desiderati: un brano sentito una volta nel 2017 non
   * è una cosa che l'utente ha chiesto di avere. I desiderati nascono dalle
   * playlist e dai preferiti, dove l'intenzione c'è.
   */
  notInLibrary: number;
}

/** Cosa l'importazione di un account porterebbe, o ha portato. */
export interface EsitoAccount {
  source: string;
  profile: string | null;
  spotifyUserId: string | null;
  fullHistory: boolean;
  /** Un rapporto per playlist, nella stessa forma dell'importazione da un link. */
  playlists: EsitoSpotify[];
  rejectedPlaylists: PlaylistRifiutata[];
  liked: EsitoSpotify;
  /** Quanti brani sono stati segnati preferiti **adesso**: zero alla seconda passata. */
  likedMarked: number;
  albums: EsitoSpotify[];
  albumsSeen: number;
  albumIdsWritten: number;
  artistsSeen: number;
  artistsLinked: number;
  historyRows: number;
  historySkipped: ScartiCronologia;
  statsUpdated: number;
  playlistRestored: number;
  wantedClosed: number;
}

/** Che aria tira sull'account, senza toccare la rete. */
export interface StatoAccount {
  configurato: boolean;
  clientId: string | null;
  collegato: boolean;
  spotifyUserId: string | null;
  displayName: string | null;
  ultimoMs: number | null;
  /** `api` o `archivio`. */
  ultimaVia: string | null;
  /**
   * Quanti ascolti importati ci sono adesso.
   *
   * È il numero che rende «dimentica gli ascolti importati» un tasto che dice
   * quel che sta per cancellare, invece di uno che chiede di fidarsi.
   */
  ascoltiImportati: number;
  inCorso: boolean;
  caricato: AnteprimaAccount | null;
}

/** A che punto è la lettura di un account. Arriva su `account:avanzamento`. */
export interface AvanzamentoAccount {
  /** `profilo`, `preferiti`, `album`, `artisti`, `playlist`, `cronologia`. */
  fase: string;
  nome: string | null;
  fatti: number;
  totali: number | null;
}

/** I totali di un esito, che la finestra somma in tre posti diversi. */
export function totaliAccount(esito: EsitoAccount): {
  ritrovati: number;
  mancanti: number;
  inCoda: number;
} {
  const elenchi = [...esito.playlists, ...esito.albums, esito.liked];
  return {
    ritrovati: elenchi.reduce((somma, r) => somma + r.matched, 0),
    mancanti: elenchi.reduce((somma, r) => somma + r.missing, 0),
    inCoda: elenchi.reduce((somma, r) => somma + r.wantedRows, 0),
  };
}

/** Quali brani mancano, senza ripetere lo stesso fra elenchi diversi. */
export function mancantiAccount(esito: EsitoAccount): BranoMancante[] {
  const visti = new Set<string>();
  const fuori: BranoMancante[] = [];
  for (const elenco of [...esito.playlists, ...esito.albums, esito.liked]) {
    for (const brano of elenco.missingTracks) {
      // Lo stesso brano può mancare da tre playlist: elencarlo tre volte
      // farebbe sembrare il problema tre volte più grande di quel che è.
      const chiave = `${brano.artist ?? ""}|${brano.title}`;
      if (visti.has(chiave)) continue;
      visti.add(chiave);
      fuori.push(brano);
    }
  }
  return fuori;
}

/** Non c'è proprio niente da importare? */
export function vuotoAccount(anteprima: AnteprimaAccount): boolean {
  return (
    anteprima.playlist === 0 &&
    anteprima.preferiti === 0 &&
    anteprima.album === 0 &&
    anteprima.artisti === 0 &&
    anteprima.cronologia === 0
  );
}

/** Che aria tira sul lettore keyless. */
export interface DiagnosticaSpotify {
  /** `null` se la stretta di mano riesce, altrimenti perché no. */
  strettaDiMano: string | null;
  cifrari: number;
  versioniCifrari: number[];
  /** Dove va messo il file che corregge le costanti scadute. */
  percorsoConfig: string;
  /** `assente`, `letto` o `illeggibile`. */
  fileConfig: string;
  fileConfigErrore: string | null;
}

/** Quanti desiderati ci sono, per stato. */
export interface ConteggiScarico {
  /** Da prendere. */
  attesa: number;
  /** Presi. */
  fatto: number;
  /** Non presi, e non si riprova più. Sono quelli che «Riprova» rimette in fila. */
  fallito: number;
  /**
   * Su YouTube non ci sono.
   *
   * Distinti dai falliti di proposito: ritentare all'infinito un brano che non
   * esiste nasconde quelli che un ritentativo lo meritavano.
   */
  introvabile: number;
}

/**
 * Un'importazione, vista dalla coda.
 *
 * Non c'è nessuna tabella delle importazioni: è il gruppo delle righe di
 * `spotify_wanted` con lo stesso `sourceId`. Siccome quelle righe non si
 * cancellano mai, l'elenco sopravvive alla chiusura dell'applicazione.
 */
export interface SorgenteScarico {
  /** L'identificativo del contenitore su Spotify. */
  sourceId: string;
  /** `brano`, `album`, `playlist` o `artista`. */
  sourceKind: string;
  /** Come si chiama. */
  sourceTitle: string;
  /** Come stanno i suoi brani. */
  conteggi: ConteggiScarico;
  /** Quando è stata importata. */
  aggiuntaMs: number;
  /** L'ultimo movimento su una delle sue righe. */
  aggiornataMs: number;
}

/** Come sta la coda di scaricamento. */
export interface StatoScarico {
  attiva: boolean;
  /** Quanti ne sono stati presi in questa passata. */
  fatti: number;
  /** Quanti ne restano. */
  rimasti: number;
  conteggi: ConteggiScarico;
  /**
   * Gli stessi conteggi, ma una riga per importazione.
   *
   * Il totale della coda dice «31 su 74» e nasconde quale delle due playlist
   * sta scendendo: è la sola cosa che chi ne ha avviate due vuole sapere.
   */
  sorgenti: SorgenteScarico[];
  /**
   * yt-dlp è al suo posto.
   *
   * `false` vuol dire che la coda non può nemmeno partire, ed è un'informazione
   * da mostrare **prima** che l'utente prema qualcosa.
   */
  ytdlp: boolean;
}

/** Cosa sta succedendo a un brano della coda. */
export interface BranoScarico {
  titolo: string;
  artista: string | null;
  /** Da 0 a 1 mentre scende; `null` mentre cerca. */
  frazione: number | null;
  /** `cerco`, `scarico`, `fatto`, `fallito`, `introvabile`. */
  esito: string;
  /** Il codice del catalogo, quando è andata male. */
  codice: string | null;
  /** Da quale importazione viene, per metterlo sotto la riga giusta. */
  sorgenteId: string;
  /** Il nome di quel contenitore. */
  provenienza: string;
}

/** Uno spostamento proposto dal riordino. */
export interface Spostamento {
  da: string;
  a: string;
}

/** Cosa il riordino proporrebbe di fare. */
export interface PianoRiordino {
  radice: string;
  letti: number;
  spostamenti: Spostamento[];
  /** I fermi per motivo: `alreadyInPlace`, `outsideRoot`, `needsReview`, `destinationTaken`. */
  fermi: { motivo: string; quanti: number }[];
  daRivedere: { album: string; brani: number; artisti: string[] }[];
  annullabile: boolean;
}

/** Cosa il riordino ha fatto. */
export interface EsitoRiordino {
  spostati: number;
  falliti: { da: string; a: string; errore: ErroreIpc }[];
  cartelleRimosse: number;
  annullabile: boolean;
}

/** Una skin compilata. */
/** Quel che una skin dice sull'impaginazione e sul movimento. */
/** Il valore di una manopola di widget. Il tipo segue quello dichiarato in Rust. */
export type ValoreOpzione = boolean | string | number;

/**
 * Un nodo dello scafale.
 *
 * `at` è l'indirizzo calcolato dal compilatore — `0-1-2`, il percorso degli
 * indici dei figli — ed è lo stesso che sta nei selettori del foglio. I due lati
 * non si accordano su niente: l'indirizzo è una funzione della posizione, e
 * quindi non può divergere.
 */
export interface NodoScafale {
  kind: "zone" | "widget";
  at: string;
  /** Per una zona: `row`, `column` o `scroll`. Per un widget: il suo nome. */
  name: string;
  /**
   * `hug`, `fill`, o una lunghezza — la stessa scrittura del documento.
   *
   * Sempre popolata, anche quando il documento taceva. È ciò che permette
   * all'editor di riscrivere l'albero intero da quel che ha ricevuto: quel che
   * esce dal nucleo, rimesso dentro, si rilegge uguale.
   */
  size: string;
  /** L'aria fra i figli. `null` per un widget. */
  gap: string | null;
  align: string | null;
  spread: string | null;
  /** La classe del registro delle parti che questo nodo porta. */
  part: string | null;
  /**
   * Da quale prefab viene questo sottoalbero.
   *
   * L'espansione è già avvenuta: il renderer non ne ha bisogno. Serve
   * all'editor, per dire «questo viene da un prefab, e modificarlo qui
   * modificherebbe anche gli altri usi».
   */
  fromPrefab: string | null;
  /** Il buco in cui la finestra infila il suo contenuto. */
  slot: string | null;
  /** Tutte le manopole, coi difetti già applicati dal nucleo. */
  options: Record<string, ValoreOpzione>;
  children: NodoScafale[];
}

export interface ImpaginazioneSkin {
  player: "bottom-bar" | "floating" | "compact";
  sidebar: "rail" | "expanded" | "hidden";
  density: "compact" | "comfortable" | "spacious";
  /** Si compone con `prefers-reduced-motion`, che vince sempre. */
  motion: "none" | "essential" | "full" | "maximum";
  /**
   * L'albero: dove stanno le cose.
   *
   * Sempre popolato, anche per una skin che non dichiara niente. L'albero di
   * serie sta in Rust e **non è duplicato qui**: quel che arriva è già la
   * risposta, e questo lato non ha un caso «manca».
   */
  shell: NodoScafale;
}

export interface Skin {
  id: string;
  /** Il foglio, già CSS. Nessun valore scritto dall'autore vi è finito dentro. */
  css: string;
  cost: number;
  /**
   * I token che seguono la copertina.
   *
   * Non c'è niente da fare con questa lista se non mostrarla: il compilatore li
   * scrive già come `var(--accent)`, quindi cambiano da soli quando cambia
   * l'accento. Serve a dire **quanti** sono in Impostazioni.
   */
  dynamicTokens: string[];
  /** Ha una variante chiara: senza, l'interruttore del tema non si mostra. */
  light: boolean;
  /**
   * La skin permette all'accento di seguire la copertina.
   *
   * È una dichiarazione dell'autore e vince sulla preferenza: `sala` dice di
   * no perché è costruita attorno al suo accento.
   */
  dynamicAccent: boolean;
  layout: ImpaginazioneSkin;
}

/** Una proprietà personalizzata da scrivere sulla radice della finestra. */
export interface Variabile {
  /** Il nome, `--accent` e simili. */
  nome: string;
  /** Il valore, già in CSS: qui non si compone nessun colore. */
  valore: string;
}

/** Una skin disponibile, per il selettore. */
export interface VoceSkin {
  id: string;
  nome: string;
  autore: string;
  descrizione: string | null;
  /** Compilata dentro l'applicazione: non si disinstalla. */
  diSerie: boolean;
  attiva: boolean;
  /** I tre colori della scheda, scritti dall'autore. Vuoto se non li dichiara. */
  anteprima: string[];
  chiara: boolean;
}

// ── lo Skin Studio ──────────────────────────────────────────────────────────

/** Un token del registro. */
export interface TokenRegistro {
  id: string;
  css: string;
  kind: "color" | "length" | "duration" | "easing" | "number" | "fontStack" | "shadow";
  group: string;
  required: boolean;
  description: string;
}

/** Una parte del registro. */
export interface ParteRegistro {
  name: string;
  group: string;
  description: string;
  /** Ha uno pseudo-elemento libero per un livello aggiuntivo. */
  layers: boolean;
}

/** Un effetto, col costo che dichiara. */
export interface EffettoRegistro {
  name: string;
  cost: number;
  target: "background" | "clipPath" | "filter";
  /**
   * L'esemplare minimo che il parser accetta, in JSON.
   *
   * È quel che «Aggiungi livello» scrive nel documento. Viene dal nucleo e non
   * da una tabella qui perché è la stessa stringa da cui si ricava il costo: un
   * vocabolario chiuso copiato in due lingue è un vocabolario che diverge.
   */
  esempio: string;
}

/** Una manopola di widget, col controllo che le corrisponde. */
export interface OpzioneRegistro {
  name: string;
  kind: "flag" | "word" | "count";
  description: string;
  default: ValoreOpzione;
  /** Le parole ammesse. Vuoto per gli altri due tipi. */
  allowed: string[];
  min: number | null;
  max: number | null;
}

/** Un widget dello scafale, come lo mostra la tavolozza. */
export interface WidgetRegistro {
  name: string;
  group: string;
  description: string;
  part: string | null;
  /** `no`, `yes`, o il nome del gruppo di cui deve esserci almeno un membro. */
  essential: string;
  singleton: boolean;
  fits: string[];
  cost: number;
  options: OpzioneRegistro[];
}

/** Le parole che una zona può usare. Vengono dagli enum, non da una lista qui. */
export interface VocabolarioRegistro {
  zones: string[];
  gaps: string[];
  aligns: string[];
  spreads: string[];
}

/** Il vocabolario che una skin può usare. Statico: si chiede una volta. */
export interface Registro {
  tokens: TokenRegistro[];
  parts: ParteRegistro[];
  effects: EffettoRegistro[];
  widgets: WidgetRegistro[];
  vocabolario: VocabolarioRegistro;
  budget: number;
  /** Il budget dello scafale, che è un budget diverso da quello delle superfici. */
  shellBudget: number;
  format: number;
  contrastoMinimo: number;
}

/** Un problema che blocca. */
export interface Problema {
  code: string;
  path: string;
  message: string;
  /** Il nome che forse si voleva scrivere, da `nearest_parts()`. */
  forse: string[];
}

/** Un avviso, che non blocca. */
export interface Avviso {
  kind:
    | "missingRequiredToken"
    | "unkeptCapability"
    | "unusedPattern"
    | "unusedPrefab"
    | "costBudget"
    | "contrast";
  path: string;
  message: string;
}

/** Una coppia di colori misurata. */
export interface Contrasto {
  davanti: string;
  dietro: string;
  scuro: number;
  chiaro: number | null;
  passa: boolean;
}

/** Una voce del pacchetto, come la mostra la colonna sinistra dello Studio. */
export interface VoceFile {
  /** `skin.json`, `preview.png`, `assets/x.woff2`. */
  nome: string;
  byte: number;
  genere: "manifest" | "miniatura" | "risorsa";
}

/**
 * Perché un'istantanea è stata presa.
 *
 * Un'istantanea non si battezza, si prende: il nome dice **perché**, ed è un
 * insieme chiuso — il nucleo lo respinge al confine se arriva altro.
 */
export type Causa = "derivata" | "salvata" | "esportata" | "manuale";

/** Un'istantanea di una bozza. */
export interface Istantanea {
  /** Millisecondi dall'epoca. È anche la sua identità. */
  quando: number;
  causa: Causa;
  /** Quante parti ridisegnava. Zero se il documento era a metà. */
  parti: number;
  /** Quanti token dichiarava. */
  token: number;
}

/** L'esito di una validazione. */
export interface Validazione {
  errori: Problema[];
  avvisi: Avviso[];
  contrasti: Contrasto[];
  /** Quante volte ogni colore della tavolozza è riferito. */
  tavolozza: [string, number][];
  css: string;
  /**
   * L'impaginazione, con lo scafale completo.
   *
   * `null` quando ci sono errori: la vista Impagina resta all'ultimo albero
   * valido, come l'anteprima resta all'ultimo foglio valido.
   */
  layout: ImpaginazioneSkin | null;
  costo: number;
  /** Quanti pezzi dell'app lo scafale monta, sul suo budget separato. */
  costoScafale: number;
  parti: number;
  dinamici: string[];
  compilatoMs: number;
}

/**
 * Lo stato del backup su Drive.
 *
 * Ogni campo è **non opzionale**, come tutti in questo file: è ciò che fa
 * scoprire a `tsc` un `#[serde(rename_all = "camelCase")]` dimenticato di là.
 * Con un campo facoltativo, la stessa svista produrrebbe un `undefined` in
 * silenzio e una schermata che dice «mai» a un backup riuscito.
 */
export interface StatoNuvola {
  /** Ci sono credenziali del client: compilate dentro o scritte a mano. */
  configurato: boolean;
  /** C'è un account collegato. */
  collegato: boolean;
  /** Il backup automatico è acceso. */
  attivo: boolean;
  email: string | null;
  /** Quando è riuscita l'ultima passata. */
  ultimoMs: number | null;
  /** C'è un'operazione in corso adesso. */
  inCorso: boolean;
  /** Com'è andata l'ultima passata automatica. */
  errore: ErroreIpc | null;
}

/** Un brano che il ripristino cambierebbe, o che nel backup non ha un file qui. */
export interface CambioBrano {
  /** I tre pezzi della chiave: sono la forma **normalizzata** dei tag. */
  artista: string;
  titolo: string;
  album: string;
  ascoltiPrima: number;
  ascoltiDopo: number;
  votoPrima: number;
  votoDopo: number;
  preferitoDopo: boolean;
}

/** Una playlist che il ripristino scriverebbe. */
export interface CambioPlaylist {
  nome: string;
  daCreare: boolean;
  /** È automatica: riceve le regole, mai l'appartenenza. */
  automatica: boolean;
  braniQui: number;
  braniNelBackup: number;
}

/** Una cartella sorvegliata che il ripristino aggiungerebbe. */
export interface CartellaDalBackup {
  percorso: string;
  /** Esiste ancora su questo disco. Si aggiunge lo stesso, ma si dice. */
  esiste: boolean;
}

/** Quel che un ripristino farebbe. */
export interface PianoRipristino {
  cEUnBackup: boolean;
  vuoto: boolean;
  generatoMs: number;
  braniDaAggiornare: number;
  braniInvariati: number;
  /** I primi cambiamenti per esteso; il numero totale è `braniDaAggiornare`. */
  cambi: CambioBrano[];
  /** I brani del backup di cui qui non c'è nessun file: elenco, non lavoro. */
  assenti: CambioBrano[];
  assentiTotale: number;
  playlist: CambioPlaylist[];
  playlistInvariate: number;
  cartelle: CartellaDalBackup[];
  skinDaInstallare: string[];
  skinPresenti: number;
  bozzeDaScrivere: string[];
  bozzePresenti: number;
  skinAttiva: string | null;
}

/** Com'è andato un ripristino. */
export interface EsitoRipristino {
  brani: number;
  playlist: number;
  cartelle: number;
  skin: number;
  bozze: number;
  skinAttiva: boolean;
  /** I file che il piano chiedeva e che su Drive non c'erano. */
  mancanti: string[];
}

/** L'avanzamento di una passata di backup. */
export interface AvanzamentoNuvola {
  fatti: number;
  totale: number;
  cosa: "metadati" | "skin" | "bozze";
}

/**
 * Lo stato dell'arricchimento automatico dei metadati.
 *
 * I tre conteggi arrivano contati dal database a ogni richiesta, non tenuti in
 * un contatore: un contatore divergerebbe al primo brano cancellato, e il
 * sintomo sarebbe un pannello che dice «142 completati» su una libreria che ne
 * ha novanta.
 */
export interface StatoArricchimento {
  /** L'arricchimento automatico è acceso. */
  attivo: boolean;
  /** C'è una passata in corso adesso. */
  inCorso: boolean;
  /** Brani che hanno ricevuto una corrispondenza applicata. */
  completati: number;
  /** Brani che nessun catalogo ha riconosciuto. */
  senzaCorrispondenza: number;
  /** Brani che aspettano ancora il loro turno. */
  daFare: number;
  /**
   * Scritture che si possono ancora riportare indietro.
   *
   * A zero il pulsante «annulla» non ha niente da fare: mostrarlo attivo
   * prometterebbe qualcosa che non succede.
   */
  annullabili: number;
  /** Quando è finita l'ultima passata. */
  ultimoMs: number | null;
  /** Com'è andata l'ultima passata automatica. */
  errore: ErroreIpc | null;
}

/** Com'è andato un annullamento dell'arricchimento. */
export interface EsitoAnnullamento {
  /** Brani tornati ai tag di prima. */
  riportati: number;
  /** File che non si sono potuti riscrivere. */
  falliti: number;
  /** Lo stato dopo, per non doverlo richiedere. */
  stato: StatoArricchimento;
}

/** L'avanzamento di una passata di arricchimento, in gruppi d'album. */
export interface AvanzamentoArricchimento {
  fatti: number;
  totale: number;
}

/** Quanto ha prodotto una passata di arricchimento. */
export interface EsitoArricchimento {
  applicati: number;
  astenuti: number;
  senzaCorrispondenza: number;
  copertine: number;
}

export const ipc = {
  // La finestra nasce nascosta: il fondo che il sistema operativo dipinge
  // prima che esista una pagina è quello di `tauri.conf.json`, e con una skin
  // chiara era un fotogramma scuro a ogni avvio. La si mostra quando skin e
  // tema sono già sul documento. Se questa chiamata non arriva mai, la mostra
  // il nucleo dopo due secondi.
  pronto: () => invoke<void>("pronto"),
  avvio: () => invoke<Avvio>("avvio"),
  impostaCartelle: (cartelle: string[]) =>
    invoke<void>("imposta_cartelle", { cartelle }),
  scansiona: () => invoke<EsitoScansione>("scansiona"),
  // Torna subito: fermarsi vuol dire «alla fine del lotto in corso», non
  // «adesso». Che sia successo lo dice `EsitoScansione.annullata`.
  annullaScansione: () => invoke<void>("annulla_scansione"),
  cerca: (query: string, offset: number, limite: number) =>
    invoke<Brano[]>("cerca", { query, offset, limite }),
  // Separato dalla pagina: la pagina si chiede a ogni scorrimento, il conteggio
  // una volta per query. Serve perché «N risultati» dica un numero vero invece
  // della lunghezza della prima pagina.
  cercaConteggio: (query: string) =>
    invoke<number>("cerca_conteggio", { query }),
  brani: (ordine: Ordine, offset: number, limite: number) =>
    invoke<Brano[]>("brani", { ordine, offset, limite }),
  album: (offset: number, limite: number) =>
    invoke<Album[]>("album", { offset, limite }),
  // Una query vera e non un filtro sull'elenco dei brani: prima se ne
  // chiedevano duemila e si tenevano quelli col cuore, cioè tutta la libreria
  // letta a ogni visita e i preferiti oltre il duemillesimo invisibili.
  preferiti: (offset: number, limite: number) =>
    invoke<Brano[]>("preferiti", { offset, limite }),
  // Anche questi al database: filtrare qui la pagina di album già scaricata
  // dava una pagina vuota agli artisti che stavano oltre.
  albumArtista: (nome: string, offset: number, limite: number) =>
    invoke<Album[]>("album_artista", { nome, offset, limite }),
  // Senza offset né limite: gli artisti sono pochi e la vista li mostra tutti
  // con un indice alfabetico invece che a pagine.
  artisti: () => invoke<Artista[]>("artisti"),
  braniAlbum: (chiave: string) =>
    invoke<Brano[]>("brani_album", { chiave }),
  preferito: (id: number, valore: boolean) =>
    invoke<void>("preferito", { id, valore }),
  valutazione: (id: number, stelle: number) =>
    invoke<void>("valutazione", { id, stelle }),
  // Senza `id` il nucleo compila la skin **scelta**, non quella di serie: la
  // scelta vive in `settings`, e la finestra non deve ricordarsela.
  skin: (id?: string) => invoke<Skin>("skin", { id: id ?? null }),
  skinElenco: () => invoke<VoceSkin[]>("skin_elenco"),
  skinInstalla: (percorso: string) =>
    invoke<VoceSkin>("skin_installa", { percorso }),
  // Installa un manifest senza passare da un file sul disco: è quel che serve
  // a un tema fatto qui dentro, che altrimenti dovrebbe uscire come `.aeskin`
  // e rientrare dalla stessa finestra di dialogo per essere usato.
  skinInstallaSorgente: (sorgente: string) =>
    invoke<VoceSkin>("skin_installa_sorgente", { sorgente }),
  skinScegli: (id: string) => invoke<Skin>("skin_scegli", { id }),

  // ── l'accento che segue la copertina ─────────────────────────────────────
  // Il colore non si decide qui. `accentoCopertina` risponde con le proprietà
  // già scritte, o con `null` quando non si deve toccare niente — preferenza
  // spenta, skin che non lo permette, copertina assente o in bianco e nero, o
  // nessuna chiarezza di quella tonalità che regga 4,5:1. Sono cinque motivi e
  // una sola risposta, perché l'azione da fare è la stessa: tenere l'accento
  // della skin.
  accentoDinamico: () => invoke<boolean>("accento_dinamico"),
  accentoDinamicoAttiva: (attivo: boolean) =>
    invoke<boolean>("accento_dinamico_attiva", { attivo }),
  accentoCopertina: (copertina: string | null, chiaro: boolean) =>
    invoke<Variabile[] | null>("accento_copertina", { copertina, chiaro }),

  // ── lo Studio ────────────────────────────────────────────────────────────
  studioRegistro: () => invoke<Registro>("studio_registro"),
  // Non fallisce mai: a metà di una parentesi il JSON non è JSON, ed è il caso
  // normale mentre si scrive. Gli errori sono un campo del risultato.
  studioValida: (sorgente: string) =>
    invoke<Validazione>("studio_valida", { sorgente }),
  studioDocumento: (id: string) => invoke<string>("studio_documento", { id }),
  // Non l'elenco di una cartella: le voci che il **formato** ammette, cioè
  // esattamente quel che `studioEsporta` rimetterà nell'archivio.
  studioPacchetto: (id: string) =>
    invoke<VoceFile[]>("studio_pacchetto", { id }),
  studioSalva: (id: string, sorgente: string) =>
    invoke<void>("studio_salva", { id, sorgente }),
  // Vuole l'`id` perché il manifest è quello dell'editor ma le risorse sono
  // quelle del pacchetto: senza, una skin col suo carattere dentro lo perdeva
  // ogni volta che usciva da qui.
  studioEsporta: (id: string, sorgente: string, percorso: string) =>
    invoke<void>("studio_esporta", { id, sorgente, percorso }),
  studioIstantanee: (id: string) =>
    invoke<Istantanea[]>("studio_istantanee", { id }),
  studioIstantanea: (id: string, sorgente: string, causa: Causa) =>
    invoke<void>("studio_istantanea", { id, sorgente, causa }),
  studioRipristina: (id: string, quando: number) =>
    invoke<string>("studio_ripristina", { id, quando }),

  // ── playlist ─────────────────────────────────────────────────────────────
  // Ogni comando che modifica restituisce la playlist aggiornata: i conteggi e
  // la durata li ricalcola il nucleo con una query sola, e rifarli qui
  // sommando le righe vorrebbe dire avere due definizioni di «quanto dura una
  // playlist» destinate a discostarsi.
  playlistElenco: () => invoke<Playlist[]>("playlist_elenco"),
  playlistBrani: (id: number) => invoke<Brano[]>("playlist_brani", { id }),
  playlistCrea: (nome: string) => invoke<Playlist>("playlist_crea", { nome }),
  playlistRinomina: (id: number, nome: string) =>
    invoke<Playlist>("playlist_rinomina", { id, nome }),
  playlistCancella: (id: number) => invoke<void>("playlist_cancella", { id }),
  playlistAggiungi: (id: number, brani: number[]) =>
    invoke<Playlist>("playlist_aggiungi", { id, brani }),
  playlistTogli: (id: number, posizione: number) =>
    invoke<Playlist>("playlist_togli", { id, posizione }),
  playlistRiordina: (id: number, da: number, a: number) =>
    invoke<Playlist>("playlist_riordina", { id, da, a }),

  // ── riordino della libreria ──────────────────────────────────────────────
  // L'unica famiglia di comandi che modifica i file dell'utente. `piano` non
  // tocca niente; `esegui` ricalcola il piano invece di ricevere quello
  // mostrato, perché fra l'anteprima e la conferma il disco può essere cambiato.
  pianoRiordino: (radice: string) =>
    invoke<PianoRiordino>("piano_riordino", { radice }),
  eseguiRiordino: (radice: string) =>
    invoke<EsitoRiordino>("esegui_riordino", { radice }),
  annullaRiordino: () => invoke<EsitoRiordino>("annulla_riordino"),

  // ── importazione dal vecchio database ────────────────────────────────────
  // Due comandi e non uno con un booleano: il piano non tocca niente e
  // l'importazione sì, e due nomi diversi rendono impossibile confonderli in un
  // punto di chiamata.
  pianoImportazione: (percorso: string) =>
    invoke<EsitoImportazione>("piano_importazione", { percorso }),
  importa: (percorso: string) =>
    invoke<EsitoImportazione>("importa", { percorso }),

  // ── importazione da Spotify ──────────────────────────────────────────────
  // Tutti e tre i comandi ricevono il **link**, mai il contenuto: quel che è
  // stato letto resta di là, in una cella indicizzata dall'URI. Farlo viaggiare
  // fin qui e indietro vorrebbe dire serializzare trecento brani due volte e
  // poi fidarsi che quel che torna sia quel che era partito.
  //
  // Chiamarli in fila con lo stesso link fa **una** stretta di mano e **una**
  // lettura di rete: il secondo e il terzo trovano la cella già piena.
  //
  // `forza` salta il riuso della cella, ed è quel che rende «Riprova» una cosa
  // che riprova: senza, una lettura caduta al terzo livello — perché in quel
  // momento la rete singhiozzava — resterebbe la risposta di quel link per tutto
  // il tempo in cui l'applicazione è aperta. Una rilettura **fallita** lascia in
  // cella quella di prima: si perde il tentativo, non quel che si aveva.
  spotifyAnteprima: (url: string, forza = false) =>
    invoke<AnteprimaSpotify>("spotify_anteprima", { url, forza }),
  spotifyPiano: (url: string, creaPlaylist: boolean) =>
    invoke<EsitoSpotify>("spotify_piano", { url, creaPlaylist }),
  spotifyImporta: (url: string, creaPlaylist: boolean) =>
    invoke<EsitoSpotify>("spotify_importa", { url, creaPlaylist }),
  // Il comando che rende leggibile un guasto invece di lasciare «non funziona»:
  // dice se il gettone anonimo si ottiene ancora e dove va messo il file che
  // corregge le costanti quando Spotify le ruota.
  spotifyDiagnostica: () =>
    invoke<DiagnosticaSpotify>("spotify_diagnostica"),

  // ── lo scaricamento dei desiderati ───────────────────────────────────────
  // I brani che Spotify nomina e la libreria non ha si prendono da **YouTube**,
  // non da Spotify: da Spotify non si scarica niente, e nessuna parte di Aether
  // ci prova. Il video lo sceglie il nucleo, preferendo i canali ufficiali.
  //
  // `scaricaDesiderati` torna **subito**: la coda gira su un filo suo di là, e
  // quel che succede arriva sui due eventi `scarico:*`. Chiamarlo mentre una
  // coda gira non ne avvia una seconda — quella in corso rilegge la tabella a
  // ogni lotto, quindi i brani appena importati li prende comunque.
  //
  // Di norma non serve chiamarlo: dopo `spotifyImporta` la coda parte da sé.
  // Resta per il caso in cui era stata annullata, o yt-dlp mancava.
  scaricaDesiderati: () => invoke<StatoScarico>("scarica_desiderati"),
  // Torna subito anche questo: fermarsi vuol dire «appena il brano in corso si
  // interrompe», non «adesso». Il processo di yt-dlp viene ucciso, quindi il
  // brano a metà non lascia niente nella cartella sorvegliata.
  annullaScarico: () => invoke<void>("annulla_scarico"),
  scaricoStato: () => invoke<StatoScarico>("scarico_stato"),
  // Rimette in fila **solo** i falliti, mai gli introvabili: vedi
  // `ConteggiScarico`. Restituisce quanti ne sono tornati in coda, e se sono
  // più di zero riavvia la coda da sé.
  riprovaFalliti: () => invoke<number>("riprova_falliti"),

  // ── riproduzione ─────────────────────────────────────────────────────────
  // Nessuno di questi comandi restituisce lo stato: lo mandano tutti
  // sull'evento `riproduzione:stato`, e averne una sola sorgente è ciò che
  // impedisce alla finestra di credersi in pausa mentre il motore suona.
  suona: (brani: number[], indice: number) =>
    invoke<void>("suona", { brani, indice }),
  pausa: () => invoke<void>("pausa"),
  riprendi: () => invoke<void>("riprendi"),
  alterna: () => invoke<void>("alterna"),
  prossimo: () => invoke<void>("prossimo"),
  precedente: () => invoke<void>("precedente"),
  vaiA: (ms: number) => invoke<void>("vai_a", { ms }),
  volume: (volume: number, muto: boolean) =>
    invoke<void>("volume", { volume, muto }),
  riproduzioneStato: () => invoke<StatoRiproduzione>("riproduzione_stato"),
  ripeti: () => invoke<void>("ripeti"),
  mescola: () => invoke<void>("mescola"),

  // ── l'equalizzatore ──────────────────────────────────────────────────────
  // `equalizzatore` è l'unico comando di riproduzione che **non** manda
  // `riproduzione:stato`: comporre quello stato richiede una lettura del brano
  // corrente dal database, e questo parte una dozzina di volte al secondo
  // finché un cursore è sotto il dito. Manda `riproduzione:eq`, che sono due
  // campi e nessuna query.
  equalizzatore: (guadagni: number[], attivo: boolean) =>
    invoke<void>("equalizzatore", { guadagni, attivo }),
  // Lo spettro si accende e si spegne: acceso, la callback audio scrive i
  // campioni in un terzo anello e un filo manda `riproduzione:spettro` trenta
  // volte al secondo. Spento non costa niente da nessuna delle due parti, ed è
  // la ragione per cui è un comando invece di essere sempre acceso.
  spettro: (attivo: boolean) => invoke<void>("spettro", { attivo }),
  eqPresetElenco: () => invoke<VocePreset[]>("eq_preset_elenco"),
  // Salva la curva **corrente**, quella che si sta ascoltando: il nome è
  // l'unica cosa che serve passare. `false` se il nome era vuoto; un nome che
  // c'è già sostituisce quella curva invece di affiancarla.
  eqPresetSalva: (nome: string) => invoke<boolean>("eq_preset_salva", { nome }),
  // `false` se non ce n'era una con quel nome. Quelle di serie non si cancellano.
  eqPresetCancella: (nome: string) =>
    invoke<boolean>("eq_preset_cancella", { nome }),

  // ── la coda ──────────────────────────────────────────────────────────────
  // Gli indici sono quelli dell'ORDINE DI RIPRODUZIONE, cioè dell'array `coda`
  // così come arriva: con lo shuffle acceso non è l'ordine interno del nucleo,
  // e ricalcolarlo qui vorrebbe dire riscrivere il mescolamento in TypeScript.
  codaAccoda: (brani: number[]) => invoke<void>("coda_accoda", { brani }),
  codaDopo: (brani: number[]) => invoke<void>("coda_dopo", { brani }),
  codaVai: (indice: number) => invoke<void>("coda_vai", { indice }),
  codaTogli: (indice: number) => invoke<void>("coda_togli", { indice }),
  codaRiordina: (da: number, a: number) =>
    invoke<void>("coda_riordina", { da, a }),
  codaSvuota: () => invoke<void>("coda_svuota"),
  braniPerId: (brani: number[]) => invoke<Brano[]>("brani_per_id", { brani }),

  // ── il backup su Drive ───────────────────────────────────────────────────
  // Cinque comandi restituiscono lo **stato intero** invece di un `void`: chi
  // collega, scollega o accende l'interruttore vuole vedere subito com'è finita,
  // e una seconda chiamata a `nuvolaStato` per scoprirlo lascerebbe un istante
  // in cui la schermata mostra la situazione di prima.
  nuvolaStato: () => invoke<StatoNuvola>("nuvola_stato"),
  // Apre il browser di **sistema** e aspetta il consenso, per non più di tre
  // minuti. Può quindi metterci a lungo: chi la chiama deve mostrare che sta
  // succedendo qualcosa.
  nuvolaCollega: () => invoke<StatoNuvola>("nuvola_collega"),
  nuvolaScollega: () => invoke<StatoNuvola>("nuvola_scollega"),
  // Un `clientId` vuoto rimette le credenziali compilate dentro l'applicazione.
  nuvolaCredenziali: (clientId: string, clientSecret: string) =>
    invoke<StatoNuvola>("nuvola_credenziali", { clientId, clientSecret }),
  nuvolaAttiva: (attivo: boolean) =>
    invoke<StatoNuvola>("nuvola_attiva", { attivo }),
  // Torna **subito**: sveglia il filo di sottofondo e basta. L'esito arriva
  // sull'evento `nuvola:stato`, come per la riproduzione — un comando che
  // aspettasse la fine del caricamento terrebbe fermo il canale per minuti.
  nuvolaSalva: () => invoke<void>("nuvola_salva"),
  // `piano` scarica e non applica; `ripristina` **riscarica e ricalcola**
  // invece di ricevere il piano mostrato, perché fra l'anteprima e la conferma
  // può essere finita una scansione.
  nuvolaPianoRipristino: () =>
    invoke<PianoRipristino>("nuvola_piano_ripristino"),
  nuvolaRipristina: () => invoke<EsitoRipristino>("nuvola_ripristina"),

  // ── l'account Spotify intero ───────────────────────────────
  // Il flusso è a tre tempi come per un link — anteprima, piano, conferma — ma
  // quel che si legge resta di là, in una cella. Qui viaggiano solo i conteggi:
  // un account sono decine di migliaia di brani più anni di cronologia, e
  // serializzarli tre volte per mostrarne il totale non ha senso.
  accountStato: () => invoke<StatoAccount>("account_stato"),
  // Un `clientId` vuoto lo cancella. Non ce n'è uno compilato dentro
  // l'applicazione, al contrario di Google: un'app Spotify in Development Mode
  // accetta **cinque** utenti, e una chiave distribuita nel binario li
  // esaurirebbe con i primi cinque che la usano.
  accountCredenziali: (clientId: string) =>
    invoke<StatoAccount>("account_credenziali", { clientId }),
  // Apre il browser di **sistema** e aspetta il consenso, per non più di tre
  // minuti. Può quindi metterci a lungo: chi la chiama deve mostrare che sta
  // succedendo qualcosa.
  accountCollega: () => invoke<StatoAccount>("account_collega"),
  // Pulisce il portachiavi e dimentica chi era. **Non** disfa niente di quel che
  // è stato importato: per quello c'è `cronologiaDimenticaImportati`, che dice
  // quante righe cancella.
  accountScollega: () => invoke<StatoAccount>("account_scollega"),
  // Minuti di rete: un account da duecento playlist sono duecento richieste.
  // L'avanzamento arriva sull'evento `account:avanzamento`.
  accountLeggi: () => invoke<AnteprimaAccount>("account_leggi"),
  // L'altra via, e non chiede niente a nessuno: lo zip che Spotify manda su
  // richiesta. Arriva in due pezzi separati da settimane — i dati dell'account e
  // la cronologia estesa — e se ne può aprire uno solo: quel che manca resta
  // vuoto, e l'anteprima lo dice.
  archivioApri: (percorso: string) =>
    invoke<AnteprimaAccount>("archivio_apri", { percorso }),
  // Tutti e due lavorano su quel che è in cella, da qualunque via sia arrivato.
  // Il piano è l'importazione vera dentro una transazione abbandonata: i numeri
  // che mostra sono quelli che si otterranno, non una previsione.
  accountPiano: (scelte: ScelteAccount) =>
    invoke<EsitoAccount>("account_piano", { scelte }),
  accountImporta: (scelte: ScelteAccount) =>
    invoke<EsitoAccount>("account_importa", { scelte }),
  // L'operazione che `play_history.source` esiste per rendere possibile.
  // Restituisce quante righe se ne sono andate. I conteggi d'ascolto **non**
  // scendono: `merge_stats` non sa scendere, e non deve — vedi la nota di là.
  cronologiaDimenticaImportati: () =>
    invoke<number>("cronologia_dimentica_importati"),

  // ── l'arricchimento dei metadati ─────────────────────────────────────────
  // Non c'è un «arricchisci adesso»: la passata è automatica per scelta, e un
  // pulsante che la lancia a mano sarebbe la schermata di revisione travestita.
  // Quel che serve davvero sono l'interruttore e il modo di disfare.
  arricchimentoStato: () => invoke<StatoArricchimento>("arricchimento_stato"),
  arricchimentoAttiva: (attivo: boolean) =>
    invoke<StatoArricchimento>("arricchimento_attiva", { attivo }),
  // **Spegne anche l'interruttore**, e non è un effetto collaterale: annullare
  // rimette i brani fra i candidati, quindi con l'automatico acceso la passata
  // successiva riscriverebbe entro mezz'ora quel che si è appena disfatto.
  // Può metterci decine di secondi: riapre e riscrive un file per brano.
  arricchimentoAnnulla: () =>
    invoke<EsitoAnnullamento>("arricchimento_annulla"),
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
/**
 * I nomi scritti a mano sulla radice dall'ultimo accento dinamico.
 *
 * Fuori da React perché non è stato dell'interfaccia: è quel che c'è
 * sull'elemento, e serve solo a poterlo togliere. Tenerlo in uno `useState`
 * vorrebbe dire un disegno in più per una lista che nessuno guarda.
 */
let accentoScritto: readonly string[] = [];

/**
 * Scrive — o toglie — l'accento che segue la copertina.
 *
 * Sulla radice e non nel foglio della skin: così sopravvive a un cambio di
 * tema, e soprattutto **vince** sul foglio senza doverne toccare il testo.
 *
 * Con `null` rimette le cose com'erano. È il caso normale, non l'eccezione:
 * un disco senza copertina, una skin che non lo vuole, la preferenza spenta.
 */
export function applicaAccento(variabili: readonly Variabile[] | null): void {
  const radice = document.documentElement;
  // Prima si toglie quel che c'era: le variabili di un disco non sono
  // necessariamente le stesse del successivo — una skin può dichiarare
  // `--accent-soft` e un'altra no — e lasciarne indietro una vorrebbe dire un
  // accento mezzo vecchio e mezzo nuovo.
  for (const nome of accentoScritto) radice.style.removeProperty(nome);
  accentoScritto = [];
  if (!variabili) return;
  for (const { nome, valore } of variabili) radice.style.setProperty(nome, valore);
  accentoScritto = variabili.map((v) => v.nome);
}

export function applicaSkin(skin: Skin): void {
  // Un accento tagliato sul contrasto della skin di prima non vale niente su
  // quella di adesso: si toglie subito, e chi guarda il brano in riproduzione
  // lo rimette con le superfici giuste. Vale anche per l'anteprima di una skin
  // che non è stata scelta, dove mostrare l'accento suo è la cosa onesta.
  applicaAccento(null);
  const id = "skin-attiva";
  const foglio =
    document.getElementById(id) ?? document.createElement("style");
  foglio.id = id;
  foglio.textContent = skin.css;
  if (!foglio.isConnected) document.head.append(foglio);
  const radice = document.documentElement;
  radice.dataset.skin = skin.id;
  // Le quattro scelte di impaginazione erano dichiarabili e non lette: il
  // formato le accettava, il compilatore ne scriveva una in un foglio che
  // nessuna regola interrogava, e le altre tre non uscivano nemmeno dal crate.
  // Da qui in giù sono attributi, quindi sono selettori, quindi contano.
  radice.dataset.player = skin.layout.player;
  radice.dataset.sidebar = skin.layout.sidebar;
  radice.dataset.density = skin.layout.density;
  radice.dataset.motion = skin.layout.motion;
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
