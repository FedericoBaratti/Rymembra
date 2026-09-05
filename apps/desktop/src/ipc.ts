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

import { t, tSe } from "./lingue";

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
    typeof value.code === "string" &&
    "domain" in value &&
    typeof value.domain === "string" &&
    "severity" in value &&
    typeof value.severity === "string"
  );
}

/**
 * Riprovare ha senso?
 *
 * La regola sta qui e non accanto a ogni tasto «Riprova» perché a deciderlo è
 * il catalogo del nucleo — `is_retryable` in `errors/catalog.rs` — e chi la
 * ricopia a mano la ricopia sbagliata alla terza volta. Un `unknown` che non è
 * un errore del nucleo non è ritentabile: di lui non si sa niente, e offrire di
 * rifare un gesto di cui non si conosce l'esito è peggio che non offrirlo.
 */
export function eRitentabile(value: unknown): boolean {
  return eErroreIpc(value) && value.retryable;
}

/**
 * Il testo da mostrare per un errore qualsiasi.
 *
 * # La tabella che c'era qui, e perché non c'è più
 *
 * Stava scritta a mano — venticinque messaggi italiani chiavati per codice — e
 * il campo che li avrebbe resi inutili arrivava dal nucleo da sempre senza che
 * nessuno lo leggesse: `ErroreIpc.i18nKey`, generato in
 * `aether-domain/src/errors/catalog.rs` come `concat!("errors.", <codice>)`.
 * Erano le stesse due tabelle da tenere allineate che il catalogo del nucleo
 * esiste per non avere: venticinque voci qui contro centoquattro codici di là,
 * e ogni codice nuovo compariva a schermo come `spotify.notPublic` finché
 * qualcuno non si ricordava di questo file.
 *
 * Adesso la chiave la decide il nucleo e il testo lo decide il catalogo delle
 * lingue, dove sono tutti e centoquattro. Un codice aggiunto di là senza testo
 * di qua ripiega su `message`, e in sviluppo lo dice in console.
 *
 * # Perché la chiave viene prima di `message`
 *
 * Perché `message` è il dettaglio tecnico che il nucleo aggiunge per chi legge
 * i registri — lo dice `AppError::with_message`: «per chi sviluppa, non per chi
 * ascolta musica». Quando ci sono tutti e due, quello scritto per una persona è
 * il primo.
 */
export function testoErrore(value: unknown): string {
  if (eErroreIpc(value)) {
    return tSe(value.i18nKey, value.message ?? value.cause ?? value.code);
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
/**
 * Una cartella che il sistema dichiara musicale e che contiene davvero qualcosa.
 *
 * Serve alla schermata di primo avvio, che le propone già spuntate invece di
 * chiedere all'utente di andarle a cercare in un dialogo.
 */
export interface CartellaCandidata {
  percorso: string;
  /** Quanti brani ci si sono contati, fermandosi al tetto del nucleo. */
  brani: number;
  /** Il conteggio si è fermato: ce n'erano altri, e il numero va scritto «2000+». */
  troncato: boolean;
  /**
   * La camminata ha perso dei rami — permessi negati, un disco che si sveglia.
   *
   * Il numero è quindi una sottostima. La cartella si propone lo stesso: chi ha
   * un disco esterno che sta partendo non deve vedersela sparire.
   */
  parziale: boolean;
}

export interface Avvio {
  dataDir: string;
  migrazioni: number;
  fts5: boolean;
  cartelle: string[];
  /**
   * Dove finiscono i brani scaricati, se è stata scelta.
   *
   * `null` **non** vuol dire «nessuna»: vuol dire la prima cartella
   * sorvegliata, che è quel che il nucleo fa davvero. Il valore di serie non
   * arriva già risolto perché la differenza fra una scelta e un ripiego è
   * proprio quel che la scheda deve poter dire.
   */
  cartellaDownload: string | null;
  /**
   * Il tema scelto, o `null` se non è mai stato scelto.
   *
   * `null` **non** vuol dire «sistema»: vuol dire che la scelta non c'è nel
   * database, e che va cercata in `localStorage` dove viveva prima. Confondere
   * i due stati butterebbe via il tema chiaro di chi l'aveva fissato.
   */
  tema: string | null;
  /**
   * La lingua scelta, come codice ISO, o `null` se non è mai stata scelta.
   *
   * `null` **non** vuol dire «inglese»: vuol dire che nessuno ha scelto, ed è
   * ciò che fa rilevare la lingua dal sistema operativo. Confondere i due stati
   * farebbe partire in inglese ogni installazione tedesca, per sempre.
   */
  lingua: string | null;
  /** Le scorciatoie riassegnate, come JSON. `null` = quelle di serie. */
  scorciatoie: string | null;
  numeri: Numeri;
}

/** Cosa ha portato via un'esportazione del profilo. */
export interface EsportazioneProfilo {
  /** Il documento, già scritto sul file scelto. */
  json: string;
  /** Quante chiavi ha portato via. */
  voci: number;
  /**
   * Le chiavi presenti nel database e rimaste qui.
   *
   * Il profilo porta un elenco di **inclusioni** — `nuvola.dispositivo` e la
   * coda non devono viaggiare — e questa è la riga che impedisce all'elenco di
   * essere silenzioso.
   */
  lasciate: string[];
}

/** Una chiave che l'importazione di un profilo cambierebbe. */
export interface CambioProfilo {
  chiave: string;
  /** `preferenza` viaggia sempre; `percorso` può non esistere di qua. */
  genere: "preferenza" | "percorso";
  /** Cosa c'è adesso, o `null` se la chiave non c'è. */
  prima: string | null;
  /** Cosa ci sarebbe. */
  dopo: string;
}

/** Cosa farebbe importare un profilo. */
export interface PianoProfilo {
  /** Quando il profilo è stato scritto. */
  creatoMs: number;
  cambi: CambioProfilo[];
  /** Le chiavi del file che questa versione non porta. Non è un guasto. */
  sconosciute: string[];
  /** I percorsi che arrivano e che su questo computer non esistono. */
  percorsiMancanti: string[];
  /** Quante chiavi sono già uguali a quel che c'è. */
  invariate: number;
}

/**
 * Un file che non è entrato come ci si aspettava, e il perché.
 *
 * Il motivo è un codice — `metadata.tagReadFailed`, `nonAudio`, `tooSmall` — e
 * non una frase: la frase la compone `t()`, così resta una sola per lingua.
 */
export interface FileSaltato {
  percorso: string;
  motivo: string;
}

/** Cosa ha fatto una scansione. */
export interface EsitoScansione {
  inseriti: number;
  aggiornati: number;
  spostati: number;
  tolti: number;
  /**
   * Quanti file non si sono potuti leggere.
   *
   * Entrano comunque in libreria, marcati «degradato»: si vedono, si sa perché
   * sono messi male, e la scansione dopo non ci ritorna.
   */
  illeggibili: number;
  /** Quali, fino a cinquanta. Il numero intero resta in `illeggibili`. */
  illeggibiliQuali: FileSaltato[];
  /** Copertine che non si sono potute salvare: il brano c'è, l'immagine no. */
  copertineFallite: FileSaltato[];
  /** File che il piano ha lasciato fuori, e perché. */
  saltati: FileSaltato[];
  copertineNuove: number;
  durataMs: number;
  /** È stata fermata a metà: quel che ha letto è scritto, il resto no. */
  annullata: boolean;
  /**
   * Le cartelle che non hanno risposto, e che quindi non sono state guardate.
   *
   * Va mostrato. Una scansione che non ha visto la cartella sul NAS ha fatto un
   * lavoro parziale, e un «completata» che non lo dice fa credere che i brani
   * che mancano non ci siano più — mentre stanno esattamente dov'erano, dietro
   * un cavo staccato.
   */
  radiciSaltate: string[];
  /**
   * Righe che il piano toglierebbe e che la guardia ha lasciato stare.
   *
   * Diverso da zero solo nelle scansioni che nessuno sta guardando: davanti a
   * una strage, non si distrugge e si aspetta la passata dopo.
   */
  rimozioniRinviate: number;
  numeri: Numeri;
}

/**
 * Da dove viene il valore di un campo, e com'era prima se è stato riparato.
 *
 * `da` vale `tag`, `tag-riparato`, `percorso`, `ripiego` o `manuale`. Un campo
 * che non compare fra le `Origini` viene dai tag: è il caso normale, e non si
 * annota.
 */
export interface Origine {
  da: string;
  prima?: string;
}

/** La provenienza di ogni campo di un brano. */
export interface Origini {
  titolo?: Origine;
  artista?: Origine;
  album?: Origine;
  albumArtist?: Origine;
  genere?: Origine;
  anno?: Origine;
  traccia?: Origine;
  disco?: Origine;
}

/** Un brano con i metadati da guardare. */
export interface TracciaIncerta {
  id: number;
  /** Il percorso: l'unica cosa sempre vera di un brano messo male. */
  path: string;
  title: string;
  artist: string;
  album: string;
  albumArtist: string | null;
  genre: string | null;
  year: number | null;
  trackNumber: number | null;
  discNumber: number | null;
  /** Zero su un brano degradato: la durata non si è potuta leggere. */
  durationMs: number;
  coverArtHash: string | null;
  /** `dedotto` o `degradato`. */
  health: string;
  /** I nomi dei problemi: `senza-tag`, `mojibake`, `segnaposto-artista`… */
  problems: string[];
  origins: Origini;
}

/**
 * Quel che l'utente corregge a mano.
 *
 * Un campo assente vuol dire «non l'ho toccato», mai «svuotalo»: non esiste un
 * modo di dire «cancellalo», e non deve esistere.
 */
export interface CorrezioniMetadati {
  titolo?: string;
  artista?: string;
  album?: string;
  albumArtist?: string;
  genere?: string;
  anno?: number;
  traccia?: number;
  disco?: number;
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
/**
 * Un ascolto, con il brano che lo ha prodotto.
 *
 * Il brano intero e non il solo identificativo: la cronologia è fatta di
 * titoli, e duecento righe vorrebbero dire duecento richieste — o una
 * `braniPerId` che rimescola l'ordine proprio quando l'ordine *è* il contenuto.
 */
export interface VoceCronologia {
  /** La riga, per distinguere due ascolti dello stesso brano nello stesso istante. */
  id: number;
  brano: Brano;
  /** Quando è **finito**, in millisecondi dall'epoca. */
  quandoMs: number;
  /** Quanto se n'è sentito. */
  msAscoltati: number;
  /** `local` se l'ha suonato Aether, `spotify` se importato da un account. */
  sorgente: string;
}

export type Ordine = "scaffale" | "recenti" | "ascoltati" | "titolo";

/** Come si ripete: niente, questo brano, tutta la coda. */
export type Ripetizione = "off" | "one" | "all";

/**
 * A che livello si normalizza il volume.
 *
 * Quattro stati e non un interruttore più un numero: sotto c'è un bersaglio in
 * decibel — il nucleo sa portare tutto a un livello qualunque fra −30 e −6 — ma
 * chiedere quel numero a chi ascolta vorrebbe dire chiedergli cos'è un LUFS.
 * `normale` è il riferimento a cui i tag ReplayGain sono misurati, `alto` è
 * quello delle piattaforme di streaming, `basso` sta cinque decibel sotto.
 */
export type Normalizzazione = "spento" | "basso" | "normale" | "alto";

/**
 * Quel che la Home mostra all'apertura.
 *
 * Un tipo solo e una chiamata sola: sono cinque domande che si fanno insieme,
 * e cinque `invoke` separati vorrebbero dire cinque attraversamenti dell'IPC
 * per disegnare una schermata sola.
 */
export interface Casa {
  /** Il brano su cui ci si era fermati. */
  riprendi: Brano | null;
  /** A che punto era, in millisecondi. */
  riprendiMs: number;
  /** Gli ultimi ascoltati, senza ripetizioni. */
  recenti: Brano[];
  /**
   * I dischi entrati in libreria per ultimi.
   *
   * Dischi e non brani: la musica entra una cartella alla volta, e dodici brani
   * ordinati per data d'ingresso sono dodici tracce dello stesso album.
   */
  aggiunti: Album[];
  /** Quel che non si ascolta da mesi. */
  trascurati: Brano[];
}

/**
 * Una raccolta del lunedì.
 *
 * Il nome non arriva dal nucleo: arriva `etichetta` — un genere, un artista, o
 * niente — e la frase si compone qui, nella lingua di chi legge. Un titolo già
 * scritto in italiano dentro il database sarebbe un titolo italiano anche per
 * chi ha l'interfaccia in inglese.
 */
export interface Raccolta {
  /** La riga, per marcarla aperta. */
  id: number;
  /** `ripescati` o `ancora`. */
  genere: string;
  /** Quale delle «Ancora» è. Zero per i ripescati. */
  ordine: number;
  /** Il materiale del nome, o niente se il gruppo non ha una maggioranza. */
  etichetta: string | null;
  /** Come leggerlo: `genere` o `artista`. */
  etichettaTipo: string | null;
  /** Se è già stata aperta almeno una volta. */
  aperta: boolean;
  /** I brani, nell'ordine deciso dal calcolo. */
  brani: Brano[];
}

/**
 * Il valore di `spegnimento` che dice «quando finisce questo brano».
 *
 * Non è una durata, quindi non può essere un numero di minuti. Il nucleo lo
 * riconosce come modo e non prepara il brano successivo, così la musica finisce
 * dove sarebbe finita comunque invece di essere tagliata a metà.
 */
export const FINE_DEL_BRANO = -1;

/**
 * Il massimo di dissolvenza che si può chiedere, in secondi.
 *
 * Dodici, come Spotify. Oltre, la sovrapposizione dura più della coda di quasi
 * ogni brano e quel che si sente non è più un passaggio ma due canzoni suonate
 * insieme. Il nucleo taglia comunque: questo serve al cursore per sapere dove
 * finire.
 */
export const DISSOLVENZA_MASSIMA_S = 12;

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
  /** A che livello normalizza il volume. Di serie `normale`. */
  replaygain: Normalizzazione;
  /**
   * Fra quanto si spegne da solo, in millisecondi.
   *
   * `null` se nessun timer è acceso, `0` se è «alla fine di questo brano» —
   * che non è una durata e si scrive a parole.
   */
  spegnimentoMs: number | null;
  /** A coda finita si continua da soli. Di serie no. */
  autoplay: boolean;
  /**
   * Quanto si sovrappongono due brani, in secondi. `0` è spenta.
   *
   * In secondi e non in millisecondi perché è così che si sceglie: il cursore
   * ha una tacca per secondo, e millisecondi vorrebbero dire dividere per
   * mille per disegnare e moltiplicare per mille per chiedere.
   */
  dissolvenzaS: number;
  /** Il motore audio non c'è: perché, e se vale la pena riaprire. */
  audio: GuastoAudio | null;
  /**
   * Perché il brano dopo è quello, quando l'ha scelto l'autoplay.
   *
   * Un **codice** — `album`, `suono`, `ascolti`, `datanto`… — e non una frase:
   * la frase sta in `lingue/`, sotto `queue.why.<codice>`. `null` quando il
   * brano dopo l'hai messo tu, e allora non c'è niente da spiegare.
   */
  motivoProssimo: string | null;
}

/**
 * Le bande dello spettro, come arrivano dall'evento `riproduzione:spettro`.
 *
 * Quante ne ha chieste chi guarda, da 8 a 1024, dalla più bassa alla più alta.
 *
 * Arrivano in `0..=255` e non in `0..=1`: mille numeri in virgola mobile trenta
 * volte al secondo sono mezzo megabyte al secondo di JSON per disegnare barre
 * alte qualche centinaio di pixel. Chi disegna divide per 255.
 *
 * Il nucleo legge dalla stessa trasformata anche le dieci bande d'ottava — quelle
 * dell'equalizzatore — e non stanno qui: le disegnava la striscia sotto la
 * copertina, che non c'è più, e dieci numeri per trenta eventi al secondo che
 * nessuno legge sono dieci numeri di troppo.
 */
export interface BandeSpettro {
  fini: number[];
}

/**
 * Il dispositivo audio non c'è, o non si è mai aperto.
 *
 * Prima di esistere, un dispositivo perso era indistinguibile da un brano che
 * non parte: il nucleo alzava un bit che nessuno leggeva, e staccare le cuffie
 * voleva dire premere play senza sentire niente, per sempre, senza nessuna
 * schermata che dicesse perché.
 */
export interface GuastoAudio {
  /** `playback.deviceLost` o `playback.engineUnavailable`. */
  codice: string;
  /** Cosa è successo, in una frase. */
  causa: string;
  /** Riaprire ha senso provarlo. */
  riapribile: boolean;
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
   * com'è, senza passare da `urlCopertina` — quella serve la libreria.
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
 * Ricopiato da `aether_domain::regole` — `Campo::genere` e `Operatore::per` —
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
   * cache e l'accesso offline nei suoi termini, e Aether la rispetta invece di
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
   * Da dove Aether prende la musica, adesso.
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

/** Il tipo di valore che un token accetta. Decide quale controllo lo modifica. */
export type TipoToken =
  | "color"
  | "length"
  | "duration"
  | "easing"
  | "number"
  | "fontStack"
  | "shadow";

/** Un token del registro. */
export interface TokenRegistro {
  id: string;
  css: string;
  kind: TipoToken;
  group: string;
  required: boolean;
  /**
   * Gli estremi, per gli undici token la cui libertà è limitata di proposito.
   *
   * Sono i capi del cursore: vengono dal registro perché sono gli stessi due
   * numeri con cui il validatore rifiuta il documento, e un cursore che
   * arrivasse altrove offrirebbe un valore che il salvataggio poi respinge.
   */
  min: number | null;
  max: number | null;
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

/** Il tipo di una manopola d'effetto. Decide quale controllo la modifica. */
export type TipoParametro =
  "color" | "length" | "angle" | "number" | "stops" | "corners" | "word";

/** Una manopola di un effetto, col controllo che le corrisponde. */
export interface ParametroRegistro {
  name: string;
  kind: TipoParametro;
  description: string;
  /** Le parole ammesse, per `word`. Vuoto altrimenti. */
  allowed: string[];
  min: number | null;
  max: number | null;
  /** Si può togliere: il nucleo ha un valore di serie per questo campo. */
  optional: boolean;
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
  /**
   * Le manopole, per aprire un livello e modificarlo.
   *
   * Senza, «Aggiungi livello» scriveva l'esemplare e finiva lì: si otteneva un
   * rettangolo nero e per cambiarne il colore si scendeva nel JSON.
   */
  params: ParametroRegistro[];
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

/**
 * Un dispositivo che partecipa alla sincronia.
 *
 * `nome` è `null` finché non lo si accoppia, ed è voluto: un dispositivo che
 * compare da solo nella cartella condivisa non ha un nome da esibire, e
 * inventargliene uno lo farebbe sembrare già conosciuto.
 */
export interface DispositivoSincronia {
  id: string;
  nome: string | null;
  /** Ci si fida di quel che scrive. */
  fidato: boolean;
  /** Quando il suo documento è stato letto l'ultima volta. */
  vistoMs: number | null;
  /** È questo computer. */
  sonoIo: boolean;
}

/**
 * Cosa la sincronia ha cambiato nella libreria.
 *
 * Non è telemetria: è la sola cosa che permette di fidarsi di un automatismo che
 * scrive da solo. «142 ascolti, 3 playlist rifatte» è una frase che si legge;
 * «sincronizzato» non lo è.
 */
export interface CambiamentiSincronia {
  ascolti: number;
  voti: number;
  preferiti: number;
  posizioni: number;
  playlist: number;
  playlistTolte: number;
  cartelle: number;
  dispositivi: number;
}

/** Com'è andata una passata di sincronia. */
export interface Resoconto {
  quandoMs: number;
  /** Quanti documenti altrui sono stati letti davvero. */
  letti: number;
  /** Quanti erano già in mano, immutati: è la passata a vuoto che costa niente. */
  saltati: number;
  /** Il proprio documento è stato riscritto. */
  scritto: boolean;
  /** Quanti documenti non si sono potuti leggere. */
  guasti: number;
  cambiamenti: CambiamentiSincronia;
}

/**
 * Lo stato della sincronia fra dispositivi.
 *
 * Come `StatoNuvola`, ogni campo è non opzionale: è ciò che fa scoprire a `tsc`
 * un `rename_all` dimenticato di là invece di lasciare un `undefined` in
 * silenzio.
 */
export interface StatoSincronia {
  /** La sincronia automatica è accesa. */
  attiva: boolean;
  /** Dove si depositano i documenti. */
  dove: "cartella" | "drive";
  cartella: string | null;
  /**
   * Il deposito è utilizzabile davvero.
   *
   * Diverso da `attiva`: una cartella non ancora scelta e un Drive non collegato
   * sono due modi di non essere pronti, e vanno distinti da «spento».
   */
  pronta: boolean;
  /** L'identificativo di questo computer. */
  io: string;
  dispositivi: DispositivoSincronia[];
  ultimaMs: number | null;
  inCorso: boolean;
  resoconto: Resoconto | null;
  errore: ErroreIpc | null;
}

/** Una versione più nuova che aspetta di essere installata. */
export interface AggiornamentoDisponibile {
  /** Il numero di versione annunciato dal manifesto. */
  versione: string;
  /** Le note di rilascio, quando il manifesto ne porta. */
  note: string | null;
  /** Quando è stata pubblicata. */
  dataMs: number | null;
  /**
   * L'utente ha già detto «non ora» per **questa** versione.
   *
   * Non spegne niente: al prossimo controllo la richiesta parte lo stesso, e se
   * nel frattempo ne esce un'altra l'avviso torna. Serve solo a non ripetere
   * ogni mezz'ora una domanda a cui è già stato risposto.
   */
  saltata: boolean;
}

/**
 * Lo stato del controllo aggiornamenti.
 *
 * È l'unica richiesta di rete che Aether fa senza che nessuno gliel'abbia
 * chiesta, e per questo ha un interruttore suo in Impostazioni e un numero suo
 * in `PRIVACY.md`.
 */
export interface StatoAggiornamenti {
  /** Il controllo periodico è acceso. Di serie lo è. */
  attivo: boolean;
  /**
   * Questa copia di Aether sa verificare la firma di un aggiornamento.
   *
   * Falso in un albero compilato senza chiavi di firma. Il controllo allora non
   * parte affatto — annunciare qualcosa che poi non si può installare sarebbe
   * solo un modo più lungo di fallire — e la finestra nasconde l'interruttore
   * invece di mostrarne uno che non comanda niente.
   */
  configurato: boolean;
  /** La versione installata adesso. */
  versioneCorrente: string;
  /** Quando è finito l'ultimo controllo riuscito. */
  ultimoMs: number | null;
  disponibile: AggiornamentoDisponibile | null;
  /** Un controllo è in corso adesso. Dura un secondo e non si mostra. */
  inCorso: boolean;
  /** Uno scaricamento è in corso adesso. Dura minuti e ha una barra. */
  installazione: boolean;
  errore: ErroreIpc | null;
}

/** Quanto è sceso di un aggiornamento in corso di scaricamento. */
export interface AvanzamentoAggiornamento {
  scaricati: number;
  /** Quanti se ne aspettano in tutto, quando il server lo dice. */
  totale: number | null;
}

/** Com'è messo un servizio di scrobbling. */
export interface CollegamentoScrobble {
  /**
   * Ci sono le credenziali per parlare col servizio.
   *
   * Diverso da `collegato`: per Last.fm servono una chiave e un segreto — che
   * si prendono dalla propria pagina di sviluppatore — *prima* di poter anche
   * solo chiedere il consenso. ListenBrainz non ha questo passo, e per lui è
   * sempre vero.
   */
  configurato: boolean;
  /** C'è un token o una sessione: si può mandare. */
  collegato: boolean;
  /** Come ci si chiama lassù. */
  utente: string | null;
  /** Quanti ascolti aspettano di partire verso questo servizio. */
  inAttesa: number;
  /** Quanti hanno finito i tentativi e stanno fermi. */
  abbandonati: number;
}

/** Lo stato dello scrobbling. */
export interface StatoScrobble {
  /** Mandare quel che si ascolta è acceso. */
  attivo: boolean;
  listenbrainz: CollegamentoScrobble;
  lastfm: CollegamentoScrobble;
  /** C'è un consenso Last.fm cominciato e non finito. */
  attesaLastfm: boolean;
  /** C'è una passata in corso adesso. */
  inCorso: boolean;
}

/** Com'è andata una passata di invio. */
export interface EsitoInvio {
  /** Quanti ascolti sono usciti e sono stati accettati. */
  mandati: number;
  /**
   * Quanti il servizio ha ricevuto e scartato.
   *
   * Ricevuto: escono comunque dalla coda. Un ascolto che Last.fm scarta perché
   * la data è troppo vecchia non tornerà mai accettato.
   */
  ignorati: number;
  /** Perché li ha scartati, un motivo per riga senza ripetizioni. */
  motivi: string[];
  /** Quanti restano in coda. */
  inAttesa: number;
  /** Quanti hanno finito i tentativi. */
  abbandonati: number;
  /** Il codice del guasto che ha fermato la passata, quando ce n'è stato uno. */
  guasto: string | null;
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

/** Una parola con il suo tempo, nell'LRC esteso. */
export interface ParolaTesto {
  /** Quando comincia, in millisecondi. */
  ms: number;
  /** Il pezzo di riga che le appartiene, spazi compresi. */
  testo: string;
}

/** Una riga di testo con il suo tempo. */
export interface RigaTesto {
  /** Quando comincia, in millisecondi. */
  ms: number;
  /** La riga. */
  testo: string;
  /** I tempi delle parole, quando il file li porta. Quasi sempre vuoto. */
  parole: ParolaTesto[];
}

/**
 * Il testo di un brano, già interpretato dal nucleo.
 *
 * Qui non arriva mai un LRC: arrivano righe in ordine, con i loro millisecondi.
 * È deliberato — il formato lo legge `aether_domain::testo`, e un secondo
 * lettore scritto in TypeScript divergerebbe dal primo su tutto quel che il
 * formato non dice, che è quasi tutto.
 */
export interface TestoBrano {
  /** Le righe con i tempi. Vuoto se il testo non è sincronizzato. */
  righe: RigaTesto[];
  /** Il testo senza tempi, da mostrare quando le righe non ci sono. */
  piatto: string | null;
  /** Il brano non ha parole. È una risposta, non un'assenza. */
  strumentale: boolean;
  /** Da dove viene. */
  fonte: "nessuna" | "sidecar" | "tag" | "lrclib" | "mano";
  /**
   * Lo scarto dichiarato dal file, nel verso dello standard: positivo anticipa.
   *
   * Arriva separato da `scartoMs` perché i due li decidono due persone diverse:
   * questo chi ha scritto il `.lrc`, l'altro chi sta ascoltando adesso. Si
   * sommano alla posizione, mai ai tempi delle righe — vedi
   * `aether_domain::testo::posizione_corretta`, che è dove la regola vive.
   */
  offsetMs: number;
  /** La correzione di chi ascolta. */
  scartoMs: number;
  /**
   * Quanto i tempi stanno dentro *questo* file.
   *
   * `sospetta` e `fuori` non nascondono il testo: lo mostrano dicendo che c'è
   * qualcosa da verificare. Un testo giusto della versione sbagliata scorre
   * benissimo, ed è il modo in cui un lettore mente senza accorgersene.
   */
  aderenza: "buona" | "sospetta" | "fuori";
  /** Si è già chiesto al catalogo per questo brano. */
  cercato: boolean;
  /**
   * Vale la pena chiedere al catalogo: quel che si ha non scorre.
   *
   * Non si ricostruisce qui da `fonte` e `cercato`. Il testo piatto conta come
   * «non si ha»: il catalogo tiene più voci per lo stesso brano e la prima che
   * risponde non è sempre quella con i tempi — vedi
   * `aether_app::testi::TestoBrano::da_chiedere`.
   */
  daChiedere: boolean;
}

/** Quanti brani stanno in ciascuno dei quattro stati. */
export interface CoperturaTesti {
  /** Brani con i tempi: scorrono. */
  sincronizzati: number;
  /** Brani col solo testo: si leggono. */
  piatti: number;
  /** Brani che non hanno parole. */
  strumentali: number;
  /** Brani per cui non si è ancora trovato niente. */
  mancanti: number;
}

/** Come stanno i testi sulla libreria intera. */
export interface StatoTesti {
  /** Si può chiedere al catalogo. */
  rete: boolean;
  /** Una passata sta girando adesso. */
  inCorso: boolean;
  /** Quanti brani ha guardato la passata in corso. */
  fatti: number;
  /** Quanti gliene restano. */
  rimasti: number;
  /** I quattro numeri, contati sui brani e non sui file. */
  copertura: CoperturaTesti;
}

/** L'avanzamento di una passata sui testi, in brani. */
export interface AvanzamentoTesti {
  fatti: number;
  rimasti: number;
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

  // ── la barra del titolo ───────────────────────────────────────────────────
  // La finestra non ha decorazioni: i quattro gesti che dava il sistema
  // operativo li disegna la pagina, e passano da comandi nostri invece che dai
  // permessi `core:window:*` — vedi `main.rs`, dove sta scritto perché. Il
  // quinto — lo schermo intero — quella fascia non lo dava nemmeno: arriva da
  // `F11`, cioè dalla tabella delle scorciatoie.
  finestraTrascina: () => invoke<void>("finestra_trascina"),
  finestraRiduci: () => invoke<void>("finestra_riduci"),
  // Torna com'è rimasta: un giro solo invece di «cambia» e poi «com'è?».
  finestraIngrandisci: () => invoke<boolean>("finestra_ingrandisci"),
  finestraIngrandita: () => invoke<boolean>("finestra_ingrandita"),
  // Lo schermo intero della **finestra**, che non è quello di «In riproduzione»:
  // quello riempie la finestra di un brano, questo toglie di mezzo il resto del
  // desktop. Torna com'è rimasta, come `finestraIngrandisci`.
  finestraSchermoIntero: () => invoke<boolean>("finestra_schermo_intero"),
  finestraChiudi: () => invoke<void>("finestra_chiudi"),

  avvio: () => invoke<Avvio>("avvio"),
  impostaCartelle: (cartelle: string[]) =>
    invoke<void>("imposta_cartelle", { cartelle }),
  // Le cartelle musicali del sistema che contengono davvero qualcosa. Elenco
  // vuoto anche quando la ricerca è scaduta: per la finestra i due casi si
  // disegnano uguali — non c'è niente da proporre — e distinguerli vorrebbe
  // dire un messaggio in più che non porta a nessuna azione diversa.
  cartelleCandidate: () =>
    invoke<CartellaCandidata[]>("cartelle_candidate"),
  // Stringa vuota = rimetti il valore di serie, cioè la prima cartella
  // sorvegliata. Il nucleo **toglie** la riga invece di scriverci dentro il
  // vuoto: «mai scelta» e «scelta vuota» devono restare la stessa cosa.
  impostaCartellaDownload: (percorso: string) =>
    invoke<void>("imposta_cartella_download", { percorso }),

  // ── le preferenze della finestra, e il profilo ────────────────────────────
  // Il tema stava in `localStorage` e adesso sta in `settings`: era l'unica
  // preferenza che non finiva né nel backup su Drive né nel profilo.
  impostaTema: (tema: string) => invoke<void>("imposta_tema", { tema }),
  // Stringa vuota = rimetti il rilevamento dal sistema. Il nucleo non sa quali
  // lingue esistono — l'elenco è la cartella `src/lingue/` — e controlla
  // soltanto che il testo sia un codice di lingua.
  impostaLingua: (lingua: string) => invoke<void>("imposta_lingua", { lingua }),
  // Stringa vuota = rimetti quelle di serie. Il nucleo controlla soltanto che
  // sia JSON: i nomi dei comandi sono roba della finestra, e un nucleo che li
  // validasse andrebbe ricompilato per aggiungere una scorciatoia.
  impostaScorciatoie: (scorciatoie: string) =>
    invoke<void>("imposta_scorciatoie", { scorciatoie }),
  profiloEsporta: (percorso: string) =>
    invoke<EsportazioneProfilo>("profilo_esporta", { percorso }),
  // Il piano prima di applicare, come per ogni altra cosa irreversibile qui.
  profiloPiano: (percorso: string) =>
    invoke<PianoProfilo>("profilo_piano", { percorso }),
  profiloImporta: (percorso: string) =>
    invoke<PianoProfilo>("profilo_importa", { percorso }),
  // La cronologia d'ascolto, dal più recente. Si scriveva dal primo giorno e
  // non la leggeva nessuno; dopo l'importazione di un account contiene anni.
  cronologia: (offset: number, limite: number) =>
    invoke<VoceCronologia[]>("cronologia", { offset, limite }),
  cronologiaConteggio: () => invoke<number>("cronologia_conteggio"),
  scansiona: () => invoke<EsitoScansione>("scansiona"),
  // ── i brani con i metadati messi male ──
  // Il conteggio sta a parte dall'elenco per la stessa ragione di
  // `cercaConteggio`: il numero si chiede a ogni scansione per la pastiglia,
  // l'elenco solo quando qualcuno apre la sezione.
  metadatiConteggio: () => invoke<number>("metadati_conteggio"),
  metadatiIncerti: (offset: number, limite: number) =>
    invoke<TracciaIncerta[]>("metadati_incerti", { offset, limite }),
  // Restituisce la traccia com'è rimasta: se è tornata a posto arriva `null`,
  // e la riga sparisce dall'elenco senza doverlo ricaricare tutto.
  metadatiCorreggi: (id: number, campi: CorrezioniMetadati) =>
    invoke<TracciaIncerta | null>("metadati_correggi", { id, campi }),
  metadatiConferma: (id: number) => invoke<void>("metadati_conferma", { id }),
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
  casa: () => invoke<Casa>("casa"),
  // Il fuso lo sa solo la finestra: il nucleo tiene il tempo in millisecondi
  // universali, e il lunedì comincia sette ore prima a Roma che a Los Angeles.
  // Il segno è quello che si legge — minuti da AGGIUNGERE all'universale — cioè
  // l'opposto di `getTimezoneOffset()`.
  settimana: () =>
    invoke<Raccolta[]>("settimana", {
      scostamentoMinuti: -new Date().getTimezoneOffset(),
    }),
  settimanaApri: (raccolta: number) =>
    invoke<void>("settimana_apri", { raccolta }),
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
  // Restituisce la skin che **resta attiva**: togliere quella indossata torna a
  // quella di serie, e il CSS da applicare arriva di qui invece che da una
  // seconda chiamata.
  skinDisinstalla: (id: string) => invoke<Skin>("skin_disinstalla", { id }),
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
  // Butta la bozza e restituisce quel che dice il pacchetto: la sorgente torna
  // di qui e non da una seconda chiamata, così non esiste un istante in cui la
  // finestra mostra un documento che non sta più da nessuna parte.
  studioScarta: (id: string) => invoke<string>("studio_scarta", { id }),
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
  // ── le playlist intelligenti ──────────────────────────────────────────
  // Non si materializzano: una playlist intelligente **è** la sua
  // interrogazione, e `playlistBrani` la esegue. Scrivere le righe vorrebbe
  // dire tenerle aggiornate a ogni ascolto, a ogni scansione e a ogni
  // mezzanotte che passa — «ascoltati negli ultimi 30 giorni» cambia da sé.
  playlistCreaSmart: (nome: string, regole: InsiemeSmart) =>
    invoke<Playlist>("playlist_crea_smart", { nome, regole }),
  playlistRegole: (id: number) =>
    invoke<InsiemeSmart | null>("playlist_regole", { id }),
  playlistRegoleScrivi: (id: number, regole: InsiemeSmart) =>
    invoke<Playlist>("playlist_regole_scrivi", { id, regole }),
  // L'anteprima mentre si scrive: è l'unico modo di accorgersi di aver scritto
  // «anno maggiore di 2050» prima di salvare una playlist vuota.
  playlistRegoleProva: (regole: InsiemeSmart) =>
    invoke<AnteprimaRegole>("playlist_regole_prova", { regole }),

  // ── i file di playlist ────────────────────────────────────────────────
  // Il piano prima, come ogni altra cosa che scrive: reimportare un file
  // **sostituisce** la playlist con lo stesso nome, e cancellare va detto
  // prima di farlo.
  playlistFilePiano: (percorso: string) =>
    invoke<PianoFilePlaylist>("playlist_file_piano", { percorso }),
  playlistFileImporta: (percorso: string, nome: string) =>
    invoke<EsitoFilePlaylist>("playlist_file_importa", { percorso, nome }),
  // Il formato lo decide l'estensione del file scelto: chiederlo una seconda
  // volta in una tendina accanto sarebbe la stessa domanda con due risposte.
  playlistEsporta: (id: number, percorso: string) =>
    invoke<number>("playlist_esporta", { id, percorso }),

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

  // ── importazione da un link ──────────────────────────────────────────────
  // Una serie sola per tutti i cataloghi: i lettori di là producono lo stesso
  // tipo, quindi da qui in poi il percorso è uno. Chi incolla non deve
  // scegliere un catalogo prima di sapere che cosa ha negli appunti.
  //
  // I link che valgono sono quelli di **archive.org**, **jamendo.com** e
  // **audius.co**. Un link che non si riconosce non viene tentato: il nucleo
  // risponde `download.unrecognizedUrl` senza fare nessuna richiesta, perché un
  // link a cui non si sa bussare è un link a cui non si bussa.
  //
  // Tutti e tre i comandi ricevono il **link**, mai il contenuto: quel che è
  // stato letto resta di là, in una cella indicizzata da catalogo e
  // identificativo. Farlo viaggiare fin qui e indietro vorrebbe dire
  // serializzare trecento brani due volte e poi fidarsi che quel che torna sia
  // quel che era partito.
  //
  // Chiamarli in fila con lo stesso link fa **una** lettura di rete: il secondo
  // e il terzo trovano la cella già piena.
  //
  // `forza` salta il riuso della cella, ed è quel che rende «Riprova» una cosa
  // che riprova: senza, una lettura caduta all'ultimo livello — perché in quel
  // momento la rete singhiozzava — resterebbe la risposta di quel link per tutto
  // il tempo in cui l'applicazione è aperta. Una rilettura **fallita** lascia in
  // cella quella di prima: si perde il tentativo, non quel che si aveva.
  //
  // Mentre `importAnteprima` legge arriva `import:avanzamento` — pagine e brani,
  // vedi `AvanzamentoImport`. Fino a poco fa una famiglia `import:*` non
  // esisteva affatto, ed era la ragione per cui una playlist da trecento brani
  // era una chiamata bloccante muta dietro il tasto «Guarda». Non c'è un evento
  // di annullamento: l'abort del lettore è cablato a «no», e un tasto che non
  // annulla è peggio della sua assenza.
  importAnteprima: (url: string, forza = false) =>
    invoke<AnteprimaImport>("import_anteprima", { url, forza }),
  importPiano: (url: string, creaPlaylist: boolean) =>
    invoke<EsitoImport>("import_piano", { url, creaPlaylist }),
  importEsegui: (url: string, creaPlaylist: boolean) =>
    invoke<EsitoImport>("import_esegui", { url, creaPlaylist }),
  // Il comando che rende leggibile un guasto invece di lasciare «non funziona»:
  // dice quali cataloghi rispondono adesso e da quali si può tenere una copia.
  // **Tocca la rete**, al contrario di `scaricoStato`: è l'unico posto in cui
  // farlo è giusto, perché è l'unico che l'utente apre per saperlo.
  importDiagnostica: () => invoke<DiagnosticaImport>("import_diagnostica"),
  /**
   * Il rapporto di un'importazione passata, o `null` se non c'è più.
   *
   * `null` **non è un errore**: è un'importazione più vecchia della persistenza,
   * o una di cui il rapporto è stato potato. Chi chiama lo distingue, perché
   * dirlo come guasto manderebbe qualcuno a cercare una riparazione che non
   * esiste.
   */
  importRapporto: (sourceId: string) =>
    invoke<EsitoImport | null>("import_rapporto", { sourceId }),
  /**
   * Gli ultimi rapporti, per la pagina: le concluse in sessioni passate.
   *
   * Si chiede un limite perché `desiderati` non cancella mai niente e i rapporti
   * seguono la stessa regola: un elenco che cresce per sempre va potato prima o
   * poi, e potarlo dalla finestra non si può — quindi si chiede solo quel che si
   * mostra.
   */
  importRapporti: (limite: number) =>
    invoke<EsitoImport[]>("import_rapporti", { limite }),

  // ── la procura dei desiderati ────────────────────────────────────────────
  // I brani che l'archivio nomina e la libreria non ha si cercano nei
  // **cataloghi liberi**: Internet Archive, e — quando ci saranno — Jamendo e
  // Audius. Non da Spotify, che non consegna file a nessuno, e non da YouTube,
  // le cui *API Developer Policies* (§ III.E.1.a) vietano di scaricare,
  // memorizzare o mettere in cache i loro contenuti. Nessuna parte di Aether ci
  // prova più.
  //
  // Il nome IPC è rimasto `scarica_*`, e vale la pena dire perché: cambiarlo
  // avrebbe rotto ogni chiamante per guadagnare una parola più giusta, e la
  // parola giusta sta nel modulo di là — `procura.rs` — dove chi legge il
  // codice la trova.
  //
  // Quel che il nucleo trova, lo prende **solo se la licenza lo consente**, e
  // la verifica avviene prima di qualunque richiesta. Quel che non trova
  // diventa una riga `introvabile`, che alimenta «Da comprare» invece di
  // ritentare per sempre.
  //
  // `scaricaDesiderati` torna **subito**: la coda gira su un filo suo di là, e
  // quel che succede arriva sui due eventi `scarico:*`. Chiamarlo mentre una
  // coda gira non ne avvia una seconda — quella in corso rilegge la tabella a
  // ogni lotto, quindi i brani appena importati li prende comunque.
  //
  // Di norma non serve chiamarlo: dopo un'importazione la coda parte da sé.
  // Resta per il caso in cui era stata annullata.
  scaricaDesiderati: () => invoke<StatoScarico>("scarica_desiderati"),
  // Torna subito anche questo: fermarsi vuol dire «appena il brano in corso si
  // interrompe», non «adesso». Il prelievo controlla il bit fra un blocco e
  // l'altro e scrive in una cartella temporanea, quindi il brano a metà non
  // lascia niente nella cartella sorvegliata.
  annullaScarico: () => invoke<void>("annulla_scarico"),
  scaricoStato: () => invoke<StatoScarico>("scarico_stato"),
  // Rimette in fila **solo** i falliti, mai gli introvabili: vedi
  // `ConteggiScarico`. Restituisce quanti ne sono tornati in coda, e se sono
  // più di zero riavvia la coda da sé.
  riprovaFalliti: () => invoke<number>("riprova_falliti"),
  // Quel che resta da comprare: le righe `introvabile`, distinte per titolo e
  // artista. Un comando a parte e non un campo di `scaricoStato`, che viene
  // chiamato decine di volte in una coda: questa lista può essere lunga come la
  // libreria di qualcuno, e vive in una sezione che di norma è chiusa.
  daComprare: (limite: number) =>
    invoke<DaComprare[]>("da_comprare", { limite }),
  // Il negozio si **nomina**, non si indirizza: l'indirizzo lo costruisce il
  // nucleo da un elenco chiuso di tre domini. Aprire un indirizzo nel browser di
  // sistema è l'unica capacità pericolosa che la finestra ha, e farle scegliere
  // *quale* vorrebbe dire darla intera a una pagina compromessa.
  cercaDoveComprare: (
    negozio: "bandcamp" | "qobuz" | "discogs",
    cosa: string,
  ) => invoke<void>("cerca_dove_comprare", { negozio, cosa }),
  // Accetta o rifiuta le registrazioni diverse da quella chiesta — concerti,
  // riletture. **Non** tocca quel che è già stato preso: decide cosa fa la coda
  // da adesso in poi. Restituisce lo stato aggiornato, così l'interruttore
  // riflette quel che il nucleo ha davvero scritto.
  alternativeAmmettile: (ammesse: boolean) =>
    invoke<StatoScarico>("alternative_ammettile", { ammesse }),

  // ── riproduzione ─────────────────────────────────────────────────────────
  // Nessuno di questi comandi restituisce lo stato: lo mandano tutti
  // sull'evento `riproduzione:stato`, e averne una sola sorgente è ciò che
  // impedisce alla finestra di credersi in pausa mentre il motore suona.
  suona: (brani: number[], indice: number) =>
    invoke<void>("suona", { brani, indice }),
  // La coda la costruisce il nucleo: sono trenta identificativi che la finestra
  // non guarda mai, e farseli mandare per rimandarli indietro sarebbe un
  // viaggio dell'IPC per niente.
  radio: (brano: number) => invoke<void>("radio", { brano }),
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
  // La normalizzazione invece manda `riproduzione:stato`: arriva quando un dito
  // preme un interruttore, non dodici volte al secondo. Sui brani senza tag
  // ReplayGain non sposta niente in nessuna delle due posizioni — la
  // correzione esiste solo dove c'è un guadagno dichiarato da rispettare.
  normalizzazione: (livello: Normalizzazione) =>
    invoke<void>("normalizzazione", { livello }),
  // Il timer di spegnimento. `minuti` a zero lo spegne, `FINE_DEL_BRANO` chiede
  // di fermarsi dove finisce quel che suona. I minuti e non un istante: «fra
  // mezz'ora» è quel che si intende, e un istante calcolato qui sarebbe
  // calcolato con l'orologio della finestra.
  spegnimento: (minuti: number) => invoke<void>("spegnimento", { minuti }),
  // La coda che non finisce. Come sopra: lo stato torna sull'evento, non da
  // qui, così l'interruttore si muove quando il nucleo ha davvero cambiato
  // idea.
  autoplay: (attivo: boolean) => invoke<void>("autoplay", { attivo }),
  // La dissolvenza incrociata, in secondi. Zero la spegne e riporta il
  // passaggio a com'era: esatto al campione. Il nucleo taglia a
  // `DISSOLVENZA_MASSIMA_S` e rilegge quel che ha scritto, quindi il valore
  // che torna sull'evento è quello vero, non quello chiesto.
  dissolvenza: (secondi: number) => invoke<void>("dissolvenza", { secondi }),
  // Riapre il dispositivo audio. È un comando e non un tentativo automatico
  // perché `cpal` apre il predefinito di **sistema**: staccate le cuffie, il
  // predefinito torna agli altoparlanti, e riaprire da soli vorrebbe dire far
  // uscire la musica dagli altoparlanti — in ufficio, di notte, in riunione.
  // Sopravvivono coda, volume, curva e normalizzazione; la posizione no.
  riapriAudio: () => invoke<void>("riapri_audio"),
  // Riprende il brano che una cartella di rete aveva interrotto, dal punto in
  // cui la musica si era fermata. È il tasto «Riprova» dell'avviso di rete: se
  // il NAS è ancora spento fallisce con lo stesso errore ritentabile e si può
  // premere di nuovo, se nel frattempo si è ascoltato altro non fa niente.
  riprovaCorrente: () => invoke<void>("riprova_corrente"),
  // Lo spettro si accende e si spegne: acceso, la callback audio scrive i
  // campioni in un terzo anello e un filo manda `riproduzione:spettro` trenta
  // volte al secondo. Spento non costa niente da nessuna delle due parti, ed è
  // la ragione per cui è un comando invece di essere sempre acceso.
  spettro: (attivo: boolean) => invoke<void>("spettro", { attivo }),
  // Quante barre disegna la scena dello spettro. La scelta sta in `settings`,
  // non in `localStorage`: è una preferenza, e le preferenze viaggiano con il
  // backup e con la sincronia.
  spettroBande: () => invoke<number>("spettro_bande"),
  // Riporta quante ne sono rimaste: fra le potenze di due non c'è niente, e un
  // numero che non è una di quelle si porta alla più vicina invece di essere
  // rifiutato. Chi ha premuto deve vedere accesa la linguetta vera.
  spettroBandeScegli: (quante: number) =>
    invoke<number>("spettro_bande_scegli", { quante }),
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

  // ── la sincronia fra dispositivi ─────────────────────────────────────────
  // Stessa forma del backup, e per la stessa ragione: chi tocca un interruttore
  // deve vedere subito com'è finita. `sincroniaAdesso` fa eccezione e restituisce
  // il **resoconto** — è l'unica cosa che rende leggibile un automatismo che
  // scrive nella libreria da solo, e chi ha appena premuto «Sincronizza adesso»
  // sta guardando proprio per sapere cosa è cambiato.
  sincroniaStato: () => invoke<StatoSincronia>("sincronia_stato"),
  sincroniaAttiva: (accesa: boolean) =>
    invoke<StatoSincronia>("sincronia_attiva", { accesa }),
  // `cartella` conta solo quando `dove` è `"cartella"`. Il percorso viene creato
  // subito se non esiste: un errore deve arrivare mentre si è ancora davanti al
  // campo che lo ha causato, non fra cinque minuti da un filo di sottofondo.
  sincroniaMagazzino: (dove: "cartella" | "drive", cartella: string | null) =>
    invoke<StatoSincronia>("sincronia_magazzino", { dove, cartella }),
  // Questa **aspetta**: una passata a vuoto è un'elencazione, e su una cartella
  // condivisa non tocca nemmeno la rete.
  sincroniaAdesso: () => invoke<Resoconto>("sincronia_adesso"),
  sincroniaDispositivi: () =>
    invoke<DispositivoSincronia[]>("sincronia_dispositivi"),
  sincroniaAccoppia: (id: string, nome: string | null) =>
    invoke<DispositivoSincronia[]>("sincronia_accoppia", { id, nome }),
  sincroniaDimentica: (id: string) =>
    invoke<DispositivoSincronia[]>("sincronia_dimentica", { id }),

  // ── gli aggiornamenti ────────────────────────────────────────────────────
  // Il controllo vero non passa mai di qui: lo fa un filo di sottofondo ogni
  // mezz'ora, e quel che la finestra vede arriva dall'evento
  // `aggiornamenti:stato`. Anche `aggiornamentiAdesso` si limita a svegliare
  // quel filo e a restituire lo stato di **prima**, apposta: una via sola per
  // raccontare com'è andata, invece di due che possono dire cose diverse.
  aggiornamentiStato: () => invoke<StatoAggiornamenti>("aggiornamenti_stato"),
  // Spegnendolo si butta via anche l'aggiornamento già trovato: lasciare
  // l'avviso in piedi dopo che qualcuno ha chiesto di non essere avvisato
  // sarebbe rispondere «va bene» e continuare come prima.
  aggiornamentiAttivo: (attivo: boolean) =>
    invoke<StatoAggiornamenti>("aggiornamenti_attivo", { attivo }),
  aggiornamentiAdesso: () => invoke<StatoAggiornamenti>("aggiornamenti_adesso"),
  // «Non ora», e vale per quella versione sola.
  aggiornamentiSalta: (versione: string) =>
    invoke<StatoAggiornamenti>("aggiornamenti_salta", { versione }),
  // Torna **subito**: scaricare settanta megabyte e lanciare un installer non
  // sta dentro una chiamata IPC. Quel che succede arriva da
  // `aggiornamenti:avanzamento`, e se va bene l'ultima cosa che questa finestra
  // fa è chiudersi — su Windows è l'installer NSIS a terminare Aether e a
  // riaprirlo.
  aggiornamentiInstalla: () => invoke<void>("aggiornamenti_installa"),

  // ── il diario ────────────────────────────────────────────────────────────
  // L'unica cosa che la finestra può fare col diario: farlo vedere. Non lo
  // legge e non lo manda a nessuno — un log che l'applicazione sa spedire da
  // sé è telemetria, e `PRIVACY.md` dice che non ce n'è. Qui si apre la
  // cartella nel gestore file, e cosa farne lo decide chi guarda.
  diarioApri: () => invoke<void>("diario_apri"),
  // L'altra metà: scriverci dentro un guasto del davanti. Il diario del nucleo
  // raccoglieva tutto quel che succede sotto e niente di quel che succede
  // sopra, e in rilascio non c'è una console: un errore JavaScript non
  // catturato lasciava la finestra bianca e nessuna traccia. `dove` finisce
  // nella parentesi quadra del diario («finestra», «promessa», «recinto»),
  // `cosa` è il messaggio — che il nucleo riduce a una riga sola prima di
  // scriverlo, perché uno stack trace si porta dietro i percorsi del disco.
  //
  // Non restituisce errori e non ne lancia: è chiamato da un gestore di errori,
  // e un gestore di errori che fallisce non ha nessuno a cui dirlo.
  diarioAnnota: (dove: string, cosa: string) =>
    invoke<void>("diario_annota", { dove, cosa }).catch(() => {}),

  // I documenti pubblici, nel browser di sistema. Il nome e non l'indirizzo: di
  // là c'è un elenco chiuso, e un comando che aprisse l'indirizzo che gli si
  // passa sarebbe un comando che apre qualunque indirizzo — dall'interno di un
  // programma di cui ci si fida.
  apriDocumento: (
    quale:
      | "repository"
      | "segnalazioni"
      | "licenza"
      | "terze"
      | "privacy"
      | "condizioni"
      // Non un documento, ma la stessa serratura: l'elenco di là non tiene «i
      // testi legali», tiene gli indirizzi che questa finestra ha il permesso
      // di far aprire.
      | "donazioni",
  ) => invoke<void>("apri_documento", { quale }),

  // ── l'account Spotify intero ───────────────────────────────
  // Il flusso è a tre tempi come per un link — anteprima, piano, conferma — ma
  // quel che si legge resta di là, in una cella. Qui viaggiano solo i conteggi:
  // un account sono decine di migliaia di brani più anni di cronologia, e
  // serializzarli tre volte per mostrarne il totale non ha senso.
  //
  // Una via sola: lo zip che Spotify consegna su richiesta. Il consenso OAuth
  // c'era e non c'è più — l'archivio è dell'utente per diritto di portabilità
  // (GDPR art. 20), mentre cosa si possa fare dei dati della Web API lo decide
  // il *Spotify Developer Policy*, e una libreria che li tiene per anni non ci
  // sta dentro.
  accountStato: () => invoke<StatoAccount>("account_stato"),
  // Lo zip arriva in due pezzi separati da settimane — i dati dell'account e la
  // cronologia estesa — e se ne può aprire uno solo: quel che manca resta
  // vuoto, e l'anteprima lo dice.
  archivioApri: (percorso: string) =>
    invoke<AnteprimaAccount>("archivio_apri", { percorso }),
  // Butta via quel che è in cella. Non succede da solo dopo l'importazione,
  // perché importare due volte con scelte diverse è una cosa che si fa; ma un
  // archivio di dieci anni sono centinaia di megabyte in memoria, e questo è il
  // tasto che li restituisce.
  archivioDimentica: () => invoke<StatoAccount>("archivio_dimentica"),
  // Tutti e due lavorano su quel che è in cella.
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

  // ── i testi ──────────────────────────────────────────────────────────────
  // `testoBrano` non tocca la rete: dice quel che si sa già dal disco — il
  // sidecar accanto al file, la riga in tabella, il tag — e lo dice subito.
  // Quando risponde `daChiedere: true`, c'è ancora qualcuno a cui chiedere.
  testoBrano: (id: number) => invoke<TestoBrano>("testo_brano", { id }),
  // Lo scarto sta sul brano, non sul file: chi ha lo stesso brano in FLAC e in
  // mp3 lo corregge una volta sola.
  testoScarto: (id: number, scartoMs: number) =>
    invoke<void>("testo_scarto", { id, scartoMs }),
  // Questo invece la rete la tocca, ed è per questo che lo chiama solo il
  // pannello del testo quando è aperto: chiedere un testo dice al catalogo
  // cosa si sta ascoltando. Con l'interruttore spento risponde quel che si sa
  // dal disco, senza fallire — spegnere non è un errore.
  testoCerca: (id: number) => invoke<TestoBrano>("testo_cerca", { id }),
  testiStato: () => invoke<StatoTesti>("testi_stato"),
  testiRete: (attivo: boolean) => invoke<StatoTesti>("testi_rete", { attivo }),
  // Torna subito: il lavoro va su un filo suo e l'avanzamento arriva con
  // l'evento `testi:avanzamento`.
  testiRiempi: () => invoke<StatoTesti>("testi_riempi"),
  testiFerma: () => invoke<void>("testi_ferma"),
  // Le battute date a orecchio tornano raddrizzate. Gli attacchi del brano non
  // attraversano l'IPC — sono centinaia per canzone, e qui non servirebbero a
  // niente: quel che serve è il risultato.
  testoAggancia: (id: number, battute: number[]) =>
    invoke<number[]>("testo_aggancia", { id, battute }),
  // Scrive il `.lrc` accanto al brano e la riga in tabella. L'LRC lo compone il
  // nucleo, con la stessa funzione che lo rilegge.
  testoSalva: (id: number, righe: { ms: number; testo: string }[]) =>
    invoke<TestoBrano>("testo_salva", { id, righe }),
  // Il verso opposto, e mai automatico: manda a LRCLIB il testo che si è
  // sincronizzato a mano, uno per volta e solo dopo averlo visto. Ci mette
  // secondi, e non per la rete — il catalogo chiede una prova di lavoro. Il
  // nucleo rifiuta da sé quel che non ha `fonte: "mano"`, così la regola non
  // dipende da questo file.
  testoPubblica: (id: number) => invoke<void>("testo_pubblica", { id }),

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

  // ── lo scrobbling ────────────────────────────────────────────────────────
  // Nessuno di questi comandi manda niente da sé, tranne `scrobbleInvia`: la
  // coda si svuota da sola su un filo di sottofondo, e quel che si accoda è
  // deciso dalla riproduzione. Qui ci sono le credenziali e i due gesti che
  // riguardano ciò che è rimasto indietro.
  scrobbleStato: () => invoke<StatoScrobble>("scrobble_stato"),
  // Spento, la coda **smette di riempirsi**: mettere in pausa e poi ritrovarsi
  // tre giorni di ascolti spediti insieme sarebbe una sorpresa.
  scrobbleAttivo: (attivo: boolean) =>
    invoke<StatoScrobble>("scrobble_attivo", { attivo }),
  // Il token si verifica **prima** di essere salvato: uno sbagliato non darebbe
  // nessun sintomo finché il primo ascolto non fallisce, ore dopo.
  scrobbleListenbrainzCollega: (token: string) =>
    invoke<StatoScrobble>("scrobble_listenbrainz_collega", { token }),
  scrobbleListenbrainzScollega: () =>
    invoke<StatoScrobble>("scrobble_listenbrainz_scollega"),
  // Vuoti cancellano e scollegano: una sessione ottenuta con un'altra chiave
  // continuerebbe a funzionare, e nasconderebbe che le nuove non vanno.
  scrobbleLastfmCredenziali: (apiKey: string, segreto: string) =>
    invoke<StatoScrobble>("scrobble_lastfm_credenziali", { apiKey, segreto }),
  // Il consenso di Last.fm è a due tempi e **in mezzo c'è una persona**: apre il
  // browser, e non succede più niente finché non si chiama `completa`. Last.fm
  // non richiama nessuno, non c'è nessun socket in ascolto.
  scrobbleLastfmCollega: () =>
    invoke<StatoScrobble>("scrobble_lastfm_collega"),
  scrobbleLastfmCompleta: () =>
    invoke<StatoScrobble>("scrobble_lastfm_completa"),
  scrobbleLastfmScollega: () =>
    invoke<StatoScrobble>("scrobble_lastfm_scollega"),
  // Svuota adesso invece di aspettare il filo. Può metterci minuti.
  scrobbleInvia: () => invoke<EsitoInvio>("scrobble_invia"),
  // Azzera i tentativi di chi li ha finiti. Serve dopo aver rimediato a quel
  // che li aveva fermati — ricollegare un account, aspettare che il servizio
  // torni su.
  scrobbleRiprova: () => invoke<StatoScrobble>("scrobble_riprova"),
  scrobbleDimentica: () => invoke<StatoScrobble>("scrobble_dimentica"),
  // Solo verso ListenBrainz, e non è una preferenza: Last.fm rifiuta gli
  // ascolti con una data vecchia e ha un tetto giornaliero, quindi quarantamila
  // righe di cronologia là comparirebbero come «ignorate».
  scrobbleImportaCronologia: (soloImportati: boolean) =>
    invoke<number>("scrobble_importa_cronologia", { soloImportati }),
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
