/** Parte del contratto con il nucleo: cloud, sincronia, «Ascolta insieme», aggiornamenti, scrobbling, IA, testi, sorveglianza, arricchimento e manutenzione. La riesporta `ipc.ts`: si importa da lì. */
import type { ErroreIpc } from "./errori";

/**
 * Lo stato del backup su Drive.
 *
 * Ogni campo è **non opzionale**, come tutti in questo file: è ciò che fa
 * scoprire a `tsc` un `#[serde(rename_all = "camelCase")]` dimenticato di là.
 * Con un campo facoltativo, la stessa svista produrrebbe un `undefined` in
 * silenzio e una schermata che dice «mai» a un backup riuscito.
 */
export interface StatoNuvola {
  /**
   * Il backup ha un'app Google dell'utente. Se a quell'app manca il segreto
   * lo dice `AppUtente.segretoSalvato`, e collegando `cloud.appSecretMissing`.
   */
  configurato: boolean;
  /** Quale app Google usa il backup (`AppUtente.id`), o `null`. */
  app: string | null;
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

// ── le cartelle dal cloud ────────────────────────────────────────────────────
//
// Non c'entrano col backup qui sopra, anche se il prefisso è lo stesso: quello
// salva una copia della libreria nella cartella privata dell'app su Drive,
// queste portano **dentro** la libreria della musica che sta su OneDrive,
// Google Drive, Dropbox, MEGA o un server WebDAV. Sono i tipi del contratto
// fra la finestra e `nuvola_musica.rs`.

/** Il nome stabile di un servizio (`rymembra_domain::nuvola::Servizio::chiave`). */
export type ServizioNuvola =
  | "onedrive"
  | "gdrive"
  | "dropbox"
  | "webdav"
  | "mega"
  | "megalink";

/** Un servizio come lo offre questa build. */
export interface ServizioOfferto {
  servizio: ServizioNuvola;
  /** «OneDrive», «Google Drive», «Dropbox», «WebDAV», «MEGA». */
  nome: string;
  /**
   * `false`: per OneDrive, Google Drive e Dropbox l'utente non ha ancora
   * registrato un'app della loro piattaforma (vedi `AppUtente`). La linguetta
   * si vede lo stesso, e fa configurare l'app. WebDAV lo è sempre.
   */
  disponibile: boolean;
  /**
   * `"browser"`: il consenso si dà nel browser di sistema; `"modulo"`: le
   * credenziali si scrivono nella finestra (MEGA, WebDAV).
   */
  modo: "browser" | "modulo";
  /**
   * Questa fase lo implementa già. Gli altri si mostrano spenti, «in arrivo»:
   * meglio una linguetta che promette con onestà che una che manca e fa
   * pensare che non arriverà mai.
   */
  pronto: boolean;
}

/** Una cartella del cloud scelta, come radice della libreria. */
export interface RadiceNuvola {
  /** Il percorso `nuvola://…`, lo stesso di `Avvio.cartelle`. È l'identità. */
  percorso: string;
  /** I nomi decodificati dal drive in giù, già pronti: «Musica › Rock». */
  nome: string;
  /**
   * Il conto è collegato, ma la cartella sul servizio non c'è più dove è stata
   * scelta: tolta, o rinominata dove il rinomina non si segue da sé (Dropbox,
   * WebDAV). I brani restano; la cartella si sceglie di nuovo.
   */
  persa: boolean;
}

/**
 * Come sta un conto: `"elenco"` mentre legge l'elenco dei file la prima volta,
 * `"daRicollegare"` quando il servizio non accetta più il collegamento.
 */
export type StatoConto = "collegato" | "elenco" | "daRicollegare" | "errore";

/** Un account collegato a un servizio. */
export interface ContoNuvola {
  /** `onedrive-k3x7q2ab`: il servizio e un'impronta stabile dell'account. */
  conto: string;
  servizio: ServizioNuvola;
  /**
   * L'app con cui è collegato (`AppUtente.id`): `null` per WebDAV e MEGA, o se
   * l'app è stata tolta. Ricollegando si passa questa, per riavere la stessa.
   */
  app: string | null;
  /** L'email, il nome dell'account o l'indirizzo del server. */
  nome: string;
  stato: StatoConto;
  /**
   * Con `"errore"` o `"daRicollegare"`: il guasto, da dire con `testoErrore`.
   * Il codice dice anche il rimedio: vedi `rimedioDi` in `appNuvola.ts`.
   */
  errore: ErroreIpc | null;
  radici: RadiceNuvola[];
  /** Mentre `stato` è `"elenco"`: quante voci sono arrivate finora. */
  vociElencate: number | null;
  /** L'ultima volta che il servizio ha risposto. */
  ultimoMs: number | null;
}

/** Una cartella remota, nel selettore. */
export interface CartellaRemota {
  id: string;
  nome: string;
  /**
   * Una cartella che il servizio non ha — «Shared with me» di Google Drive,
   * che raccoglie le cartelle condivise con l'account —: ci si entra, ma non
   * si sceglie.
   */
  virtuale: boolean;
}

/** La cache su disco dei brani del cloud. */
export interface StatoCacheNuvola {
  attiva: boolean;
  /** Il tetto, in MB. Valori offerti: 200, 500, 1000, 2000, 5000, 10000. Di serie 200. */
  tettoMb: number;
  /** Quanto è occupato adesso. */
  usatiByte: number;
}

/**
 * Quel che si scrive per MEGA e WebDAV. Per OneDrive, Google Drive e Dropbox
 * niente: c'è il browser.
 *
 * Tutti facoltativi, e non per pigrizia: sono tre moduli diversi — l'account
 * MEGA, il link MEGA, il server WebDAV — e un tipo per ciascuno vorrebbe dire
 * tre comandi per un gesto solo, che dal lato del nucleo è «collega».
 */
export interface ModuloCollega {
  /** WebDAV: l'indirizzo `https://…` della cartella WebDAV. */
  indirizzo?: string;
  /** MEGA: l'email; WebDAV: l'utente. */
  utente?: string;
  /** Mai salvata dalla finestra: passa e basta. */
  password?: string;
  /** MEGA, verifica in due passaggi: il codice a 6 cifre, chiesto dopo `cloud.twoFactorRequired`. */
  codice?: string;
  /** MEGA senza account: il link della cartella (`https://mega.nz/folder/…#…`). */
  link?: string;
}

// ── le app dell'utente ───────────────────────────────────────────────────────
//
// Dalla 3.2 nessuna app OAuth sta nel binario: ognuno registra la sua presso
// Microsoft, Google e Dropbox, e ne scrive qui l'identificativo. I tipi del
// contratto con `nuvola_app.rs`.

/** Chi rilascia l'app: `rymembra_domain::nuvola_app::Piattaforma::chiave`. */
export type ChiavePiattaforma = "microsoft" | "google" | "dropbox";

/** Una piattaforma, con quel che va scritto nella sua console. */
export interface PiattaformaApp {
  chiave: ChiavePiattaforma;
  nome: "Microsoft" | "Google" | "Dropbox";
  /** L'app ha anche un segreto: solo Google, il cui client «App desktop» lo vuole. */
  segreto: boolean;
  /**
   * Gli indirizzi di ritorno da registrare, **esatti**. Vuoto per Google: un
   * client «App desktop» non ne registra.
   */
  ritorni: string[];
  /** I permessi (o, per Google, gli scope) da concedere all'app. */
  permessi: string[];
}

/** Chi usa un'app: un conto, o il backup su Drive (`conto: null`, nome vuoto). */
export interface UsoApp {
  conto: string | null;
  servizio: ServizioNuvola | null;
  nome: string;
}

/** Un'app registrata dall'utente. */
export interface AppUtente {
  /** `google-k3x7q2ab`, o `google-ereditata` per quella che c'era fino alla 3.1. */
  id: string;
  piattaforma: ChiavePiattaforma;
  nome: string;
  clientId: string;
  /** Google: il segreto c'è nel portachiavi. Per le altre sempre `true`. */
  segretoSalvato: boolean;
  /** Quella che si usa quando nessuno sceglie: una per piattaforma. */
  predefinita: boolean;
  usataDa: UsoApp[];
}

/** Le tre piattaforme (sempre in quest'ordine: Microsoft, Google, Dropbox) e le app. */
export interface AppNuvolaIpc {
  piattaforme: PiattaformaApp[];
  /** Nell'ordine in cui sono state registrate. */
  app: AppUtente[];
}

/**
 * Quel che si manda per salvare un'app.
 *
 * `id: null` è un'app nuova. `segreto` `null` (o vuoto) tiene quello salvato;
 * un'app Google nuova lo vuole, e per Microsoft e Dropbox va sempre `null`.
 */
export interface AppDaSalvare {
  id: string | null;
  piattaforma: ChiavePiattaforma;
  nome: string;
  clientId: string;
  segreto: string | null;
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
  /**
   * In `snake_case`, e non è una svista.
   *
   * Il `#[serde(rename_all = "camelCase")]` sta su `Resoconto`, che è il tipo
   * **esterno**, e serde non lo propaga ai tipi annidati: `Cambiamenti` vive in
   * `core/rymembra-app/src/sincronia.rs` e non ne ha uno suo, quindi serializza i
   * propri campi com'è scritto in Rust. È l'unico campo di due parole qui
   * dentro — tutti gli altri sono uguali nelle due grafie — ed è per questo che
   * il difetto è passato inosservato: dichiarato `playlistTolte`, a runtime
   * arrivava `undefined`, e «3 playlist cancellate altrove» non si è mai letto.
   *
   * Corretto di qua e non di là perché di là il nome è giusto: mettere un
   * `rename_all` sul tipo del nucleo per compiacere una riga di TypeScript
   * vorrebbe dire cambiare il formato di una struttura che il nucleo
   * serializza e deserializza per conto suo, per un campo solo.
   */
  playlist_tolte: number;
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
 * Che cosa può fare chi segue una sessione: lo sceglie chi ospita quando lo
 * fa entrare, e lo può cambiare finché resta.
 *
 * - `ascolta`: nessun gesto sulla musica degli altri;
 * - `comanda`: pausa, avanti, indietro, salto, ripeti;
 * - `tutto`: anche la coda — scegliere, togliere, spostare, aggiungere.
 *
 * Volume, equalizzatore, uscita e latenza restano di ciascuno a ogni livello.
 * Gemello di `Diritti` (`core/rymembra-insieme/src/messaggi.rs`).
 */
export type DirittiInsieme = "ascolta" | "comanda" | "tutto";

/**
 * «Ascolta insieme» visto dal lettore: il campo `insieme` di
 * `StatoRiproduzione`.
 */
export interface InsiemeNelLettore {
  /** Questo PC ospita (`true`) o segue (`false`). */
  ospita: boolean;
  /** Il nome del PC che ospita. */
  host: string;
  /** Quante persone, l'host compreso. */
  persone: number;
  /** Di chi è la voce che suona: `null` se è di questo PC. */
  proprietario: string | null;
  /** La voce che suona si legge da un file di questo PC, non dalla rete. */
  locale: boolean;
  /** Chi segue: la sincronia è agganciata. Chi ospita: sempre `true`. */
  agganciato: boolean;
  /**
   * Che cosa fanno i gesti di trasporto e di coda: chi ospita `tutto`, chi
   * segue quel che chi ospita gli ha concesso.
   */
  diritti: DirittiInsieme;
  /**
   * Suona la voce di calibrazione: il titolo si scrive nella lingua di chi
   * guarda, non quello che la voce porta.
   */
  calibrazione: boolean;
  /**
   * Cresce ogni volta che le descrizioni delle voci della sessione cambiano:
   * una copertina arrivata, un titolo o una durata corretti. La coda tiene le
   * righe per id, e un id che non cambia non si richiede da solo: questo
   * numero è il segno che le righe con l'id negativo vanno rilette.
   */
  revisione: number;
}

/** In che fase è questo PC. */
export type FaseInsieme =
  | "spenta"
  | "libera"
  | "entrando"
  | "ospita"
  | "membro"
  | "uscendo";

/**
 * Il pannello «Ascolta insieme». Evento `insieme:stato`, comando
 * `insieme_stato`. Gemello di `StatoInsiemeIpc` in
 * `src-tauri/src/insieme/ipc.rs`: chi cambia uno dei due cambia l'altro.
 */
export interface StatoInsieme {
  attivo: boolean;
  nome: string;
  fase: FaseInsieme;
  porta: number | null;
  firewallSpiegato: boolean;
  sessione: SessioneInsieme | null;
  vicini: VicinoInsieme[];
  fidati: FidatoInsieme[];
  associazione: AssociazioneInsieme | null;
  inviti: InvitoInsieme[];
  richieste: RichiestaInsieme[];
  indirizzi: string[];
  sincronia: SincroniaInsieme | null;
  /**
   * Mentre questo PC ospita: gli `ip:porta` che un collega può scrivere a mano
   * nei suoi «Indirizzi scritti a mano», il migliore per primo — quello della
   * strada di serie — e poi gli altri. Vuoto quando non si ospita.
   */
  indirizziDiQui: string[];
}

/** La sessione in corso. */
export interface SessioneInsieme {
  host: string;
  /** Questo PC è l'host. */
  mia: boolean;
  persone: PersonaInsieme[];
  /** I clic di calibrazione stanno suonando. */
  calibrazione: boolean;
  /**
   * Le voci della coda che un **membro** ha aggiunto e che non sono di questo
   * PC, per maniglia (l'id in coda): la riga scrive «di Marco». Le voci di chi
   * ospita non ci sono: sono la coda stessa, e la fascia dice già di chi è.
   */
  proprietari: ProprietarioInsieme[];
}

/** Di chi è una voce della coda. */
export interface ProprietarioInsieme {
  /** La maniglia: l'id della voce nella coda di questo PC. */
  id: number;
  nome: string;
}

/** Una persona nella sessione; `membro` 0 è l'host. */
export interface PersonaInsieme {
  membro: number;
  nome: string;
  io: boolean;
  /** Che cosa può fare: chi ospita lo cambia da «In sessione». L'host è `tutto`. */
  diritti: DirittiInsieme;
}

/** Un PC che ospita, visto in rete. */
export interface VicinoInsieme {
  /** La chiave per `insiemeAssocia`. */
  chiave: string;
  /** L'impronta a gruppi, per distinguere due PC con lo stesso nome. */
  impronta: string;
  nome: string | null;
  /** L'id fra i PC associati, se lo è. */
  pari: number | null;
  bloccato: boolean;
}

/** Un PC associato. */
export interface FidatoInsieme {
  id: number;
  nome: string;
  impronta: string;
  bloccato: boolean;
  associatoMs: number;
  vistoMs: number | null;
  /** Collegato adesso. */
  presente: boolean;
  ospita: boolean;
  inSessione: boolean;
}

/** L'associazione in corso. */
export interface AssociazioneInsieme {
  id: number;
  nome: string;
  /** Quattro cifre; `null` mentre si calcolano. */
  codice: string | null;
  /** Questo PC ha già confermato e aspetta l'altro. */
  confermato: boolean;
}

/** Un invito ricevuto. */
export interface InvitoInsieme {
  id: number;
  da: string;
  pari: number;
  /** Quando scade, ms dall'epoca su questo PC. */
  scadeMs: number;
}

/** Una richiesta d'ingresso ricevuta da chi ospita. */
export interface RichiestaInsieme {
  id: number;
  da: string;
  pari: number;
  scadeMs: number;
}

/** Com'è agganciato chi segue. */
export interface SincroniaInsieme {
  agganciato: boolean;
  /** Positivo se questo PC è avanti. */
  erroreMs: number | null;
  incertezzaMs: number | null;
}

/**
 * Un avviso di passaggio. Evento `insieme:avviso`: la **chiave** della frase
 * (`together.notice.joined`, `errors.insieme.connessioneCaduta`) e il nome da
 * metterci dentro.
 */
export interface AvvisoInsieme {
  chiave: string;
  nome: string | null;
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
 * È l'unica richiesta di rete che Rymembra fa senza che nessuno gliel'abbia
 * chiesta, e per questo ha un interruttore suo in Impostazioni e un numero suo
 * in `PRIVACY.md`.
 */
export interface StatoAggiornamenti {
  /** Il controllo periodico è acceso. Di serie lo è. */
  attivo: boolean;
  /**
   * Questa copia di Rymembra sa verificare la firma di un aggiornamento.
   *
   * Falso in un albero compilato senza chiavi di firma. Il controllo allora non
   * parte affatto — annunciare qualcosa che poi non si può installare sarebbe
   * solo un modo più lungo di fallire — e la finestra nasconde l'interruttore
   * invece di mostrarne uno che non comanda niente.
   */
  configurato: boolean;
  /**
   * A tenere aggiornata questa copia è il Microsoft Store.
   *
   * Quando è vero `configurato` è falso, e la finestra dice il perché giusto:
   * non «mancano le chiavi», ma «ci pensa il Store».
   */
  gestitoDalStore: boolean;
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

/** Chi serve un modello di linguaggio. */
export type FornitoreIa = "openrouter" | "ollama" | "bionic" | "custom";

/**
 * Un modello configurato.
 *
 * La chiave non è qui e non ci sarà mai: sta nel portachiavi del sistema, e
 * quel che attraversa l'IPC è `conChiave`, cioè la risposta alla domanda «ce
 * n'è una?». Un profilo che viaggia verso la finestra non porta con sé niente
 * da nascondere.
 */
export interface ProfiloIa {
  /** Stabile per tutta la vita del profilo. Lo genera il nucleo. */
  id: string;
  nome: string;
  fornitore: FornitoreIa;
  /** L'indirizzo di base, senza `/chat/completions`. */
  urlBase: string;
  modello: string;
  /** Una chiave per questo profilo sta nel portachiavi. */
  conChiave: boolean;
  /**
   * L'origine per cui la chiave è stata scritta (`https://ospite:porta`), o
   * `null` senza chiave e nei profili salvati prima della 3.0.0.
   *
   * Non è un segreto: è un pezzo dell'indirizzo. La chiave parte solo verso
   * questa origine, e salvare il profilo con un indirizzo di un'altra origine
   * senza riscriverla la fa dimenticare — vedi `chiaveDimenticata`.
   */
  origineChiave: string | null;
}

/** Quanto costa un modello, per quel che il servizio ne dichiara. */
export type PrezzoIa = "sconosciuto" | "gratis" | "apagamento";

/**
 * Un modello offerto da un fornitore.
 *
 * Non è più solo lo slug. Con OpenRouter l'elenco è di centinaia di righe e uno
 * slug da solo non dice niente di quel che serve a sceglierne una — quanto
 * costa, e quanto contesto regge — così chi lo scriveva a mano lo scopriva da
 * un 404.
 */
export interface ModelloIa {
  /** Lo slug: è quel che va scritto nel campo Modello. */
  id: string;
  /** Il nome leggibile, quando il servizio ne dichiara uno diverso dall'id. */
  nome: string | null;
  /** Quanti gettoni di contesto, quando lo dichiara. */
  contesto: number | null;
  /** `"sconosciuto"` per i locali, che non mandano nessun prezzo. */
  prezzo: PrezzoIa;
  /**
   * Legge le immagini, quando il servizio lo dichiara.
   *
   * `null` non è «no»: Ollama e LM Studio non dicono niente delle modalità, e
   * un modello locale con la visione esiste.
   */
  vede: boolean | null;
}

/** Un profilo come lo manda la finestra: senza id se è nuovo. */
export interface ProfiloIaDaSalvare {
  /** `null` per crearne uno. */
  id: string | null;
  nome: string;
  fornitore: FornitoreIa;
  urlBase: string;
  modello: string;
}

/** I profili e quale è scelto. */
export interface StatoIa {
  profili: ProfiloIa[];
  /** L'id di quello scelto, `null` se non ce n'è nessuno. */
  attivo: string | null;
  /** C'è una conversazione in corso: una alla volta. */
  occupato: boolean;
  /**
   * L'id del profilo la cui chiave **questo** salvataggio ha dimenticato,
   * perché l'indirizzo è passato a un'altra origine. Solo nella risposta di
   * `iaSalvaProfilo`, e solo quella volta: è un avviso, non uno stato.
   */
  chiaveDimenticata: string | null;
}

/** Chi parla, in un messaggio. */
export type RuoloIa = "system" | "user" | "assistant";

/** Una battuta. */
export interface MessaggioIa {
  ruolo: RuoloIa;
  testo: string;
  /**
   * Le immagini che l'accompagnano, come indirizzi `data:` PNG, JPEG o WebP.
   *
   * Il nucleo le controlla al confine prima che vadano in rete; vedi
   * `rymembra_ia::Messaggio::immagine_ammessa`.
   */
  immagini?: string[];
}

/** Perché il modello ha smesso di parlare. */
export type MotivoFineIa = "finito" | "tagliato" | "fermato" | "troncato";

/** Un pezzo di risposta, sull'evento `ia:pezzo`. */
export interface PezzoIa {
  turno: number;
  testo: string;
  /**
   * È ragionamento e non risposta.
   *
   * I modelli che pensano prima di rispondere lo scrivono in un campo suo, e
   * può durare un minuto prima che arrivi la prima parola vera. Si mostra —
   * altrimenti il pannello sembra fermo — ma non entra nella conversazione che
   * torna al modello, e soprattutto non passa dall'estrattore delle modifiche:
   * dentro un ragionamento ci sono i blocchi che il modello ha scritto per poi
   * cambiare idea.
   */
  pensiero: boolean;
}

/**
 * La fine di un turno, sull'evento `ia:fine`.
 *
 * I conteggi sono `null` quando il servizio non li manda — Ollama e LM Studio
 * spesso non lo fanno — e non zero: uno zero farebbe credere che una richiesta
 * a pagamento sia stata gratis.
 */
export interface FineIa {
  turno: number;
  motivo: MotivoFineIa;
  gettoniIn: number | null;
  gettoniOut: number | null;
}

/**
 * Una modifica che un modello ha proposto.
 *
 * L'estrazione dal testo la fa il nucleo: è la funzione che riceve l'ingresso
 * meno prevedibile dell'applicazione, e di qua non avrebbe nessuna prova.
 */
export interface OperazioneIa {
  /** Toglie invece di scrivere. */
  togli: boolean;
  /** Dove, un passo per livello. */
  percorso: string[];
  /** Cosa scrivere. Assente per una che toglie. */
  valore: unknown;
}

/** Quel che si è capito di una risposta, e quel che no. */
export interface OperazioniIa {
  operazioni: OperazioneIa[];
  /**
   * Perché il resto è stato scartato, una riga per pezzo.
   *
   * Nove modifiche buone e una storta valgono nove: buttare via tutto
   * costringerebbe a rifare la stessa domanda sperando in un'altra fortuna.
   */
  ragioni: string[];
}

/** Un turno finito male, sull'evento `ia:errore`. */
export interface GuastoIa {
  turno: number;
  errore: ErroreIpc;
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
/** Una voce del catalogo dei testi, come la vede chi sceglie a mano. */
export interface CandidatoTesto {
  id: number;
  titolo: string;
  artista: string;
  album: string | null;
  durataMs: number | null;
  sincronizzato: boolean;
  strumentale: boolean;
  /** Le prime due righe del testo, per riconoscerlo senza sceglierlo. */
  anteprima: string | null;
}

/** Quel che torna da `testoCandidati`. */
export interface CandidatiTesto {
  /**
   * Si è potuto chiedere al catalogo.
   *
   * Spento, `voci` è vuoto perché nessuno ha chiesto — e la finestrella lo dice
   * invece di far credere che il catalogo non conosca il brano.
   */
  rete: boolean;
  /** Le voci, nell'ordine in cui il catalogo le ha date. */
  voci: CandidatoTesto[];
}

export interface RigaTesto {
  /** Quando comincia, in millisecondi. */
  ms: number;
  /** La riga. */
  testo: string;
  /** I tempi delle parole, quando il file li porta. Quasi sempre vuoto. */
  parole: ParolaTesto[];
  /**
   * Le righe che il file mette allo stesso tempo di questa — una traduzione, la
   * pronuncia — una per riga. Si accendono con lei, e si leggono sotto.
   */
  secondaria: string | null;
}

/**
 * Il testo di un brano, già interpretato dal nucleo.
 *
 * Qui non arriva mai un LRC: arrivano righe in ordine, con i loro millisecondi.
 * È deliberato — il formato lo legge `rymembra_domain::testo`, e un secondo
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
   * `rymembra_domain::testo::posizione_corretta`, che è dove la regola vive.
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
   * `rymembra_app::testi::TestoBrano::da_chiedere`.
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
  /**
   * Un testo sincronizzato a mano si scrive anche in un `.lrc` accanto al
   * brano. Spento di serie: la riga nel database è la fonte, il file
   * un'esportazione per gli altri lettori.
   */
  accanto: boolean;
  /** Una passata sta girando adesso. */
  inCorso: boolean;
  /** Quanti brani ha guardato la passata in corso. */
  fatti: number;
  /** Quanti gliene restano. */
  rimasti: number;
  /** I quattro numeri, contati sui brani e non sui file. */
  copertura: CoperturaTesti;
  /**
   * Quanti brani una passata di adesso chiederebbe al catalogo.
   *
   * È questo, e non `copertura.mancanti`, a dire se «Riempi la libreria» abbia
   * qualcosa da fare: un brano col testo piatto non è fra i mancanti ma i tempi
   * gli mancano e la passata lo chiede; uno scartato o chiesto negli ultimi
   * quattordici giorni è fra i mancanti e la passata lo salta.
   */
  inCoda: number;
}

/** L'avanzamento di una passata sui testi, in brani. */
export interface AvanzamentoTesti {
  fatti: number;
  rimasti: number;
}

/**
 * Cosa si sorveglia: niente, i soli dischi di questo computer, o anche le
 * cartelle di rete (di serie dalla 3.0.2). Quelle di rete hanno gli avvisi del
 * sistema dove la condivisione li manda, più un riallineamento all'avvio, al
 * risveglio, quando la rete torna e ogni trenta minuti.
 */
export type ModoSorveglianza = "spenta" | "locali" | "tutte";

/**
 * Come si tiene d'occhio una cartella sorvegliata.
 *
 * `sondaggio` è il nome di sempre, ma dalla 3.0.2 vuol dire «solo il
 * riallineamento periodico, ogni cinque minuti»: gli avvisi del sistema su
 * quella cartella non si sono potuti armare.
 */
export type ComeSorvegliata = "avvisi" | "sondaggio" | "ferma" | "nonRaggiungibile";

/** La sorveglianza delle cartelle, come la vede la finestra. */
export interface QuadroSorveglianza {
  modo: ModoSorveglianza;
  /**
   * Le cartelle e come le si guarda. Vuoto nei primi secondi dopo l'avvio,
   * prima che il filo abbia guardato.
   */
  cartelle: { percorso: string; come: ComeSorvegliata }[];
  /** L'ultima scansione partita da sola, se c'è stata. */
  ultima: {
    quandoMs: number;
    cartelle: number;
    inseriti: number;
    aggiornati: number;
    tolti: number;
    /** Rimozioni trattenute dalla guardia: niente è stato tolto. */
    rinviate: number;
  } | null;
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
   * Brani che portano metadati messi dall'arricchimento, e che si possono
   * **dimenticare**.
   *
   * Sono righe di una tabella del database: dimenticarle rimette il brano ai
   * tag del suo file, che è intatto. A zero il pulsante «annulla» non ha niente
   * da fare, e mostrarlo attivo prometterebbe qualcosa che non succede.
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

/**
 * Com'è andato il controllo del database in questa sessione.
 *
 * Il controllo (`PRAGMA quick_check`) gira una volta, un minuto dopo l'avvio,
 * in disparte: fino ad allora vale `daFare`. `danneggiato` porta `db.corrupt`,
 * la stessa frase di una query che inciampa su una pagina rotta; `saltato` è
 * la cartella dati con il giornale ripiegato, dove un lettore lungo
 * fermerebbe le scritture.
 */
export interface SaluteDatabase {
  esito:
    | "daFare"
    | "sano"
    | "interrotto"
    | "saltato"
    | "danneggiato"
    | "nonRiuscito";
  /** Quando è finito, in millisecondi dall'epoca. */
  quandoMs: number | null;
  /** Quanto è durato. */
  millisecondi: number | null;
  /** Il guasto, per `danneggiato` e `nonRiuscito`. */
  errore: ErroreIpc | null;
}

/** La cache delle copertine e la salute del database, per le Impostazioni. */
export interface StatoManutenzione {
  /** Quanti file conta la cache delle copertine, miniature comprese. */
  file: number;
  /** Quanti byte occupa. */
  byte: number;
  /** Quante copertine nessun brano, album, artista o playlist nomina più. */
  orfane: number;
  salute: SaluteDatabase;
}

/** Quanto ha liberato una pulizia a mano, e lo stato dopo. */
export interface EsitoPulizia {
  /** Righe di copertina tolte dal database. */
  righe: number;
  /** File tolti dal disco: copertine, miniature, avanzi. */
  file: number;
  /** Byte liberati. */
  byte: number;
  stato: StatoManutenzione;
}
