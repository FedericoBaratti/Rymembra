/** Parte del contratto con il nucleo: le importazioni, le regole delle playlist intelligenti e la coda dei desiderati. La riesporta `ipc.ts`: si importa da lì. */
import { t } from "../lingue";
import type { Brano } from "./libreria";

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

// ── importazione da un catalogo ────────────────────────────────────────────

/**
 * Un livello del lettore che non ha risposto.
 *
 * L'elenco di quelli scartati prima di quello che ha funzionato. Serve alla
 * diagnosi: «ha risposto il secondo» distingue un catalogo che ha cambiato
 * qualcosa da un computer offline.
 */
export interface LivelloFallito {
  livello: string;
  perche: string;
}

/**
 * Un elenco arrivato più corto di quanto il catalogo dichiari.
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
export interface AnteprimaImport {
  /** Il nome stabile: `internet-archive`, `jamendo`, `audius`. Vedi `nomeFonte`. */
  fonte: string;
  /** `brano`, `album`, `playlist`, `artista` o `collezione`. */
  genere: string;
  id: string;
  titolo: string;
  autore: string | null;
  /**
   * La copertina come `data:` URI, già scaricata dal nucleo.
   *
   * **Non** un indirizzo del catalogo: la politica dei contenuti della finestra
   * (`tauri.conf.json`) ammette fra le immagini solo `data:` e il protocollo
   * locale delle copertine, e allargarla per sempre a un dominio esterno per una
   * miniatura è quel che questa applicazione non fa. Va quindi in `src` così
   * com'è, senza passare da `urlCopertina`, in `aspetto.ts` — quella serve la
   * libreria.
   */
  copertina: string | null;
  brani: number;
  /** Quale livello ha risposto. Non è statistica: dice quanto ci si può fidare. */
  sorgente: string;
  troncato: Troncatura | null;
  /**
   * Quanti di quei brani si possono tenere sul disco.
   *
   * Il numero che rende onesta questa schermata. Un elenco di venti brani di
   * cui tre si prendono e diciassette si ascoltano e basta è una cosa da sapere
   * **prima** di confermare, non da scoprire dalla coda che si riempie di righe
   * introvabili.
   */
  scaricabili: number;
}

/**
 * Come va la lettura di un link, mentre va.
 *
 * `pagine` è `null` quando il livello che risponde non sa quante saranno: la
 * barra diventa indeterminata invece di stimare. Un massimo non è una previsione.
 */
export interface AvanzamentoImport {
  /** Il livello che sta rispondendo. */
  sorgente: string;
  /** Quante pagine sono state lette, da 1. */
  pagina: number;
  pagine: number | null;
  /** Quanti brani sono stati raccolti finora. */
  brani: number;
}

/** Un brano di una fonte esterna che in libreria non c'è. */
export interface BranoMancante {
  /** La posizione nell'elenco del servizio, da 1. */
  position: number;
  title: string;
  artist: string | null;
  album: string | null;
}

/**
 * Cosa l'importazione da una fonte esterna porterebbe, o ha portato.
 *
 * `missingTracks` è la voce che conta, per lo stesso motivo di
 * `EsitoImportazione.unmatched`: sono brani che l'utente ha altrove e non
 * su questo disco, e sono l'unica cosa che non può ricostruire dopo.
 */
export interface EsitoImport {
  kind: string;
  /**
   * Quale livello del lettore ha risposto; da un archivio, `archivio`.
   */
  source: string;
  /**
   * L'identificativo del contenuto presso la fonte.
   *
   * **Non** `source`, che nonostante il nome è il livello del lettore. È questo
   * che combacia con `SorgenteScarico.sourceId`, ed è così che la finestrella
   * dice all'elenco quale importazione ha appena creato.
   */
  sourceId: string;
  title: string;
  /** Quanti brani sono stati letti dalla fonte. */
  resolved: number;
  matched: number;
  missing: number;
  /** Ritrovati per ISRC: stessa registrazione, senza guardare i nomi. */
  matchedIsrc: number;
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
  /** Voci rimesse nelle playlist dal viaggio di ritorno. */
  playlistRestored: number;
  /** Righe di `desiderati` chiuse perché il brano è comparso in libreria. */
  wantedClosed: number;
  truncated: { read: number; expected: number } | null;
}

// ── l'account Spotify intero ─────────────────────────────────
// Una via sola: l'archivio che Spotify consegna su richiesta. Il consenso OAuth
// c'era e non c'è più — non per un guasto, ma perché quel che si può fare dei
// dati della Web API lo decide il *Spotify Developer Policy*, mentre l'archivio
// è dell'utente per diritto di portabilità (GDPR art. 20) e portarselo dove
// vuole è precisamente ciò che quell'articolo gli riconosce.

/** Un file dell'archivio che non si è aperto. */
export interface FileIlleggibile {
  nome: string;
  perche: string;
}

/** Cosa si è letto di un account, prima di guardare la libreria. */
export interface AnteprimaAccount {
  /**
   * `archivio`.
   *
   * Resta un campo e non una costante perché un database scritto da una
   * versione precedente può contenere `api`, e leggerlo non deve rompersi.
   */
  provenienza: string;
  profilo: string | null;
  spotifyUserId: string | null;
  /**
   * Questa via porta una cronologia degna di quel nome?
   *
   * Vero per l'archivio, che porta tutto. Falso per le vecchie importazioni via
   * API, che ne davano cinquanta righe: serve a non far sembrare un guasto il
   * limite di un endpoint che non si usa più.
   */
  cronologiaCompleta: boolean;
  playlist: number;
  braniInPlaylist: number;
  preferiti: number;
  album: number;
  artisti: number;
  cronologia: number;
  podcast: number;
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

/**
 * Le regole di una playlist intelligente.
 *
 * I nomi di campo e operatore sono stringhe e non unioni chiuse a caso: sono
 * **le stesse** che il nucleo scrive in `playlists.rules`, e la traduzione da
 * una parte all'altra è `Campo::da_testo`. Scriverne uno sbagliato qui non
 * rompe niente — quella regola viene saltata — ma non fa neanche quel che
 * dovrebbe, quindi le costanti stanno accanto.
 */
export interface RegolaSmart {
  campo: CampoSmart;
  operatore: OperatoreSmart;
  /** Il valore, quando l'operatore ne vuole uno di testo. */
  testo?: string | null;
  /** Il valore, quando l'operatore ne vuole uno numerico. */
  numero?: number | null;
}

/** L'insieme completo, com'è salvato. */
export interface InsiemeSmart {
  combinazione: "tutte" | "qualsiasi";
  regole: RegolaSmart[];
  limite?: number | null;
  ordinamento: OrdinamentoSmart;
}

export type CampoSmart =
  | "titolo"
  | "artista"
  | "album"
  | "genere"
  | "anno"
  | "valutazione"
  | "preferito"
  | "riproduzioni"
  | "aggiunto"
  | "durata"
  | "ultimoAscolto";

export type OperatoreSmart =
  | "contiene"
  | "nonContiene"
  | "uguale"
  | "diverso"
  | "inizia"
  | "finisce"
  | "maggiore"
  | "minore"
  | "negliUltimi"
  | "nonNegliUltimi"
  | "vuoto"
  | "nonVuoto";

export type OrdinamentoSmart =
  | "scaffale"
  | "piuAscoltati"
  | "menoAscoltati"
  | "recenti"
  | "casuale";

/** Che genere di valore vuole un campo. */
export type GenereSmart = "testo" | "numero" | "booleano" | "data";

/**
 * Il genere di ogni campo, e le sue etichette.
 *
 * Ricopiato da `rymembra_domain::regole` — `Campo::genere` e `Operatore::per` —
 * che è l'originale. Qui serve solo a disegnare i menù: quali coppie stiano in
 * piedi lo decide comunque il nucleo, che scarta le regole storte invece di
 * fallire.
 */
export function campiSmart(): readonly {
  chiave: CampoSmart;
  etichetta: string;
  genere: GenereSmart;
}[] {
  return [
    { chiave: "titolo", etichetta: t("smart.field.titolo"), genere: "testo" },
    { chiave: "artista", etichetta: t("smart.field.artista"), genere: "testo" },
    { chiave: "album", etichetta: t("smart.field.album"), genere: "testo" },
    { chiave: "genere", etichetta: t("smart.field.genere"), genere: "testo" },
    { chiave: "anno", etichetta: t("smart.field.anno"), genere: "numero" },
    {
      chiave: "valutazione",
      etichetta: t("smart.field.valutazione"),
      genere: "numero",
    },
    {
      chiave: "preferito",
      etichetta: t("smart.field.preferito"),
      genere: "booleano",
    },
    {
      chiave: "riproduzioni",
      etichetta: t("smart.field.riproduzioni"),
      genere: "numero",
    },
    { chiave: "durata", etichetta: t("smart.field.durata"), genere: "numero" },
    {
      chiave: "aggiunto",
      etichetta: t("smart.field.aggiunto"),
      genere: "data",
    },
    {
      chiave: "ultimoAscolto",
      etichetta: t("smart.field.ultimoAscolto"),
      genere: "data",
    },
  ];
}

/** Gli operatori che hanno senso per ogni genere di campo. */
export function operatoriSmart(): Record<
  GenereSmart,
  readonly { chiave: OperatoreSmart; etichetta: string }[]
> {
  return {
    testo: [
      { chiave: "contiene", etichetta: t("smart.op.text.contiene") },
      { chiave: "nonContiene", etichetta: t("smart.op.text.nonContiene") },
      { chiave: "uguale", etichetta: t("smart.op.text.uguale") },
      { chiave: "diverso", etichetta: t("smart.op.text.diverso") },
      { chiave: "inizia", etichetta: t("smart.op.text.inizia") },
      { chiave: "finisce", etichetta: t("smart.op.text.finisce") },
      { chiave: "vuoto", etichetta: t("smart.op.text.vuoto") },
      { chiave: "nonVuoto", etichetta: t("smart.op.text.nonVuoto") },
    ],
    // I quattro segni non si traducono: «=» e «>» si leggono uguali ovunque, e
    // sostituirli con delle parole allungherebbe un menù che sta su una riga.
    numero: [
      { chiave: "uguale", etichetta: "=" },
      { chiave: "diverso", etichetta: "≠" },
      { chiave: "maggiore", etichetta: ">" },
      { chiave: "minore", etichetta: "<" },
      { chiave: "vuoto", etichetta: t("smart.op.num.vuoto") },
      { chiave: "nonVuoto", etichetta: t("smart.op.num.nonVuoto") },
    ],
    booleano: [{ chiave: "uguale", etichetta: t("smart.op.bool.uguale") }],
    data: [
      { chiave: "negliUltimi", etichetta: t("smart.op.date.negliUltimi") },
      {
        chiave: "nonNegliUltimi",
        etichetta: t("smart.op.date.nonNegliUltimi"),
      },
      { chiave: "vuoto", etichetta: t("smart.op.date.vuoto") },
      { chiave: "nonVuoto", etichetta: t("smart.op.date.nonVuoto") },
    ],
  };
}

export function ordinamentiSmart(): readonly {
  chiave: OrdinamentoSmart;
  etichetta: string;
}[] {
  return [
    { chiave: "scaffale", etichetta: t("smart.order.scaffale") },
    { chiave: "recenti", etichetta: t("smart.order.recenti") },
    { chiave: "piuAscoltati", etichetta: t("smart.order.piuAscoltati") },
    { chiave: "menoAscoltati", etichetta: t("smart.order.menoAscoltati") },
    { chiave: "casuale", etichetta: t("smart.order.casuale") },
  ];
}

/** Gli operatori che non vogliono un valore accanto. */
export function senzaValore(operatore: OperatoreSmart): boolean {
  return operatore === "vuoto" || operatore === "nonVuoto";
}

/** Cosa prenderebbero delle regole non ancora salvate. */
export interface AnteprimaRegole {
  quanti: number;
  /** Regole che non stanno in piedi, saltate. */
  scartate: number;
  /** Nessuna condizione: prende tutta la libreria. */
  prendeTutto: boolean;
  /** Un «oppure» senza condizioni: non prende niente, mai. */
  prendeNiente: boolean;
  primi: Brano[];
}

/** Una voce di un file di playlist che in libreria non c'è. */
export interface VoceMancante {
  title: string | null;
  artist: string | null;
  /** Il percorso scritto nel file: l'unica cosa utile per andarsela a prendere. */
  path: string;
}

/** Cosa porterebbe dentro — o ha portato dentro — un file di playlist. */
export interface EsitoFilePlaylist {
  name: string;
  entries: number;
  /** Ritrovate perché il percorso porta a un brano in libreria. */
  matchedByPath: number;
  /** Ritrovate per titolo e interprete, con la scala di Spotify. */
  matchedByTags: number;
  missing: VoceMancante[];
  /** Righe del file che non si sono capite. */
  unreadable: number;
  playlistId: number | null;
  /** Ne esisteva una con lo stesso nome ed è stata sostituita. */
  replaced: boolean;
}

/** Il piano di un file, più il nome che la finestra propone. */
export interface PianoFilePlaylist {
  nome: string;
  rapporto: EsitoFilePlaylist;
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
  playlists: EsitoImport[];
  rejectedPlaylists: PlaylistRifiutata[];
  liked: EsitoImport;
  /** Quanti brani sono stati segnati preferiti **adesso**: zero alla seconda passata. */
  likedMarked: number;
  albums: EsitoImport[];
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

/** Che aria tira sull'account. Istantaneo: legge solo il database. */
export interface StatoAccount {
  spotifyUserId: string | null;
  displayName: string | null;
  ultimoMs: number | null;
  /** Oggi sempre `archivio`; `api` solo nei database scritti prima. */
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

/** Che aria tira su un catalogo. */
export interface DiagnosticaCatalogo {
  /**
   * Il nome stabile: `internet-archive`, `jamendo`, `audius`.
   *
   * L'etichetta che si legge non arriva di qui: la mette `nomeFonte`, dal
   * catalogo delle lingue.
   */
  nome: string;
  /** Risponde, adesso. Questa voce **tocca la rete**. */
  risponde: boolean;
  /**
   * Da qui si può tenere una copia, o solo ascoltare.
   *
   * Non è una capacità tecnica ma una regola: Jamendo vieta esplicitamente la
   * cache e l'accesso offline nei suoi termini, e Rymembra la rispetta invece di
   * scoprire se il server glielo lascerebbe fare.
   */
  consegna: boolean;
}

/** Che aria tira, catalogo per catalogo. */
export interface DiagnosticaImport {
  cataloghi: DiagnosticaCatalogo[];
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
   * Nessuna fonte lecita ce l'ha.
   *
   * **Non** «non esiste»: esiste, e da qualche parte si compra. Distinti dai
   * falliti di proposito — ritentare all'infinito un brano che nessun catalogo
   * libero avrà mai nasconde quelli che un ritentativo lo meritavano — e sono
   * le righe che riempiono «Da comprare».
   */
  introvabile: number;
}

/**
 * Un'importazione, vista dalla coda.
 *
 * Non c'è nessuna tabella delle importazioni: è il gruppo delle righe di
 * `desiderati` con lo stesso `sourceId`. Siccome quelle righe non si cancellano
 * mai, l'elenco sopravvive alla chiusura dell'applicazione.
 */
export interface SorgenteScarico {
  /** L'identificativo del contenitore presso il servizio. */
  sourceId: string;
  /** `brano`, `album`, `playlist` o `artista`. */
  sourceKind: string;
  /** Come si chiama. */
  sourceTitle: string;
  /**
   * Da dove viene: la colonna `desiderati.source_service`.
   *
   * `archivio-spotify`, `file-playlist`, `internet-archive`, `jamendo`,
   * `audius`. Serve a una cosa sola, e non è statistica: da un catalogo il file
   * è quello che l'utente ha incollato, e la riga deve poter dire «dall'elenco»
   * invece di mostrare una scelta che nessuno ha fatto.
   *
   * Porta il nome della colonna come i tre qui sopra, e non `fonte`: sono
   * quattro campi della stessa riga di database, e tradurne uno solo farebbe
   * sembrare che venga da un'altra parte.
   */
  sourceService: string;
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
   * Da dove Rymembra prende la musica, adesso.
   *
   * Sostituisce il vecchio `ytdlp: boolean`, e la differenza non è cosmetica:
   * là c'era una cosa che poteva **mancare**, e mezza interfaccia esisteva per
   * dirlo. Qui non manca niente — i cataloghi sono compilati dentro — e
   * l'elenco serve a un'altra cosa, che prima non si poteva fare: dire da dove
   * arriva quel che si sta prendendo.
   */
  cataloghi: CatalogoAttivo[];
  /**
   * Si accettano registrazioni diverse da quella chiesta.
   *
   * I cataloghi liberi non hanno le versioni in studio del catalogo
   * commerciale: hanno concerti e riletture. Acceso, la coda le prende e
   * **dice** che l'ha fatto; spento, torna severa e trova molto meno.
   */
  alternative: boolean;
}

/**
 * Un brano che nessuna fonte lecita ha, e che quindi si compra.
 *
 * # Perché non è un errore
 *
 * Perché non lo è. La sostituzione onesta di uno scaricamento che non si può
 * fare non è una riga rossa: è dire **dove** prendere quel brano. Queste righe
 * sono lo stato `introvabile` della coda, che fino a poco fa era solo un numero
 * in un conteggio.
 */
export interface DaComprare {
  titolo: string;
  /** Vuoto quando la fonte non l'ha dato. */
  artista: string;
  /** Vuoto quando la fonte non l'ha dato. */
  album: string;
  /** Da quale importazione veniva, per dire dove manca. */
  provenienza: string;
  /**
   * Perché nessuno ce l'ha.
   *
   * `download.noResults` è «non l'ho trovato da nessuna parte»;
   * `download.notPermitted` è «l'ho trovato e la licenza non me lo lascia
   * prendere» — cioè da qualche parte si **ascolta**, e sono due cose diverse
   * per chi legge.
   */
  motivo: string;
}

/** Un catalogo compilato dentro questa build. */
export interface CatalogoAttivo {
  /** Il nome stabile: `internet-archive`, `jamendo`, `audius`. Vedi `nomeFonte`. */
  nome: string;
  /** Da qui si può tenere una copia, o solo ascoltare. */
  consegna: boolean;
}

/** Il file che la coda ha scelto per un brano di cui aveva solo i nomi. */
export interface FileScelto {
  /** Il titolo, come sta nel catalogo. */
  titolo: string;
  /** Chi lo pubblica. `null` quando il catalogo non lo dà. */
  autore: string | null;
  /** Da quale catalogo: il nome stabile della fonte. */
  fonte: string;
  /**
   * Sotto che licenza sta: `pubblicoDominio`, `cc-by`, `cc-by-nc-sa`,
   * `openMusicLicense`, `liberaNonCommerciale`, `tutteRiservate`,
   * `sconosciuta`.
   *
   * Va **mostrata**. È la differenza fra un'applicazione che prende musica dove
   * le pare e una che sa cosa sta prendendo, e chi ascolta ha il diritto di
   * saperlo quanto chi pubblica.
   */
  licenza: string;
  /**
   * Che registrazione è: `studio`, `dalVivo`, `alternativa`.
   *
   * Il campo che rende onesta l'intera funzione. Un catalogo di concerti
   * risponde con dei concerti, e prenderne uno per la versione in studio senza
   * dirlo sarebbe scrivere in libreria una cosa per un'altra.
   */
  natura: string;
  /** Quanto è affidabile chi pubblica: `nomeAutore`, `verificata`, `ignota`. */
  affidabilita: string;
  /**
   * Scarto fra la durata del file e quella dichiarata, in ms.
   *
   * **Firmato**: positivo se il file è più lungo. Un `+8 s` è un'introduzione o
   * una coda che sfuma; un `−8 s` è una versione tagliata. Il valore assoluto
   * direbbe la metà della cosa. `null` quando una delle due durate non si sa:
   * un candidato senza durata resta in gara, e disegnare `0 s` mostrerebbe un
   * combaciare che nessuno ha verificato.
   */
  scartoMs: number | null;
  /** La pagina d'origine, da aprire per vedere da dove viene. */
  pagina: string | null;
}

/** Cosa sta succedendo a un brano della coda. */
export interface BranoScarico {
  titolo: string;
  artista: string | null;
  /** Da 0 a 1 mentre scende; `null` mentre cerca. */
  frazione: number | null;
  /** `cerco`, `prendo`, `fatto`, `fallito`, `introvabile`. */
  esito: string;
  /** Il codice del catalogo, quando è andata male. */
  codice: string | null;
  /** Da quale importazione viene, per metterlo sotto la riga giusta. */
  sorgenteId: string;
  /** Il nome di quel contenitore. */
  provenienza: string;
  /**
   * Il file scelto, e perché quello.
   *
   * `null` in due casi che chi legge deve distinguere: da un link di un
   * catalogo la scelta non c'è stata — il file era già noto e la coda salta la
   * ricerca — e mentre `esito` è `cerco` non è ancora stata fatta. Il primo si
   * riconosce da `SorgenteScarico.sourceService`, e la riga lo dice invece di
   * tacere.
   */
  scelto: FileScelto | null;
}
