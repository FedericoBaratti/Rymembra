/** Parte del contratto con il nucleo: brani, album, cartelle, avvio, profilo, scansione, playlist e riproduzione. La riesporta `ipc.ts`: si importa da lì. */
import type { InsiemeNelLettore, ServizioNuvola } from "./servizi";

/** Un brano, come lo mostra una lista. */
export interface Brano {
  id: number;
  /**
   * Il percorso sul disco. `null` per un brano di catalogo, che un file non ce
   * l'ha: è il campo da guardare prima di offrire «mostra nella cartella»,
   * «riscrivi i tag» o qualunque altro gesto che voglia un file.
   */
  path: string | null;
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
  /**
   * Da quale catalogo arriva: `internet-archive`, `audius`, `jamendo`.
   * `null` per un file sul disco.
   */
  fonte: string | null;
  /**
   * La pagina pubblica del brano.
   *
   * Si mostra accanto a chi ascolta, e non per gentilezza: per certe Creative
   * Commons e per i termini di Audius il rimando visibile è una condizione
   * d'uso.
   */
  fontePagina: string | null;
  /** Sotto che licenza sta, col nome stabile del dominio. */
  licenza: string | null;
}

/** Un risultato della ricerca nei cataloghi liberi. */
export interface RisultatoEsplora {
  url: string;
  titolo: string;
  autore: string | null;
  album: string | null;
  durataSec: number | null;
  fonte: string;
  fonteEtichetta: string;
  licenza: string;
  disponibilita: string;
  pagina: string | null;
  inLibreria: boolean;
}

/** Quel che una ricerca nei cataloghi ha trovato. */
export interface EsitoAggiuntaTutti {
  /** Quanti nuovi, quanti c'erano già, quanti rifiutati. */
  conteggi: AggiuntiDalCatalogo;
  /**
   * Gli indirizzi che **adesso** stanno in libreria.
   *
   * È quel che permette di aggiornare le righe dicendo il vero invece di
   * indovinare: il nucleo scarta anche righe che da qui sembrano buone.
   */
  inLibreria: string[];
}

export interface EsitoRicerca {
  risultati: RisultatoEsplora[];
  annullata: boolean;
  /**
   * I cataloghi che non hanno risposto, coi nomi da mostrare.
   *
   * Vuoto è il caso normale. Pieno con dei risultati accanto vuol dire che
   * l'elenco sembra completo e non lo è.
   */
  muti: string[];
}

/** Quel che l'aggiunta di brani di catalogo ha prodotto. */
export interface AggiuntiDalCatalogo {
  aggiunti: number;
  giaPresenti: number;
  senzaByte: number;
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

/**
 * Quel che «Togli dalla libreria» ha portato via.
 *
 * `torneranno` non è una diagnostica: la libreria è un indice del disco, e un
 * file che sta ancora sotto una cartella sorvegliata rientra alla prima
 * scansione. Dirlo subito è l'unico modo perche' chi ha appena premuto non lo
 * scopra fra una settimana.
 */
export interface Cancellazione {
  tolti: number;
  torneranno: number;
}

/** Una cartella tolta dall'elenco perché ci sta già dentro un'altra. */
export interface CartellaAssorbita {
  cartella: string;
  dentro: string;
}

/**
 * Una cartella nuova che è una già sorvegliata scritta in un altro modo:
 * `\\nas\musica` accanto a `Z:\`, quando `Z:` è mappata lì.
 *
 * Non entra nell'elenco. La finestra propone il trasloco da `esistente` ad
 * `a` (`cartelleTrasloca`), che porta con sé ascolti, voti e playlist.
 */
export interface CartellaGemella {
  nuova: string;
  esistente: string;
  /** Come si chiamerebbe `esistente` nella grafia di `nuova`. */
  a: string;
}

/** L'elenco delle cartelle sorvegliate com'è stato scritto, e cosa si è deciso. */
export interface CartelleOrdinate {
  cartelle: string[];
  assorbite: CartellaAssorbita[];
  gemelle: CartellaGemella[];
}

/** Cosa ha cambiato il trasloco di una cartella sorvegliata. */
export interface Traslocati {
  percorsi: number;
  irrintracciabili: number;
  giaPresenti: number;
  nonVerificati: number;
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

/**
 * Una cartella dell'albero del pannello «Cartelle».
 *
 * Viene dal **database**, non dal disco: `cartelle.rs` spiega perché, e la
 * conseguenza visibile qui è che una cartella senza brani indicizzati non
 * compare, e che aprirne una non tocca mai la rete.
 */
export interface NodoCartella {
  /** Il percorso, nella grafia con cui i brani stanno sul disco. È l'identità. */
  percorso: string;
  /**
   * Quel che si scrive nella riga.
   *
   * L'ultimo segmento, tranne che sui nodi di primo livello, dove è il percorso
   * intero: una radice che si annunciasse come «musica» non direbbe di quale
   * disco sta parlando.
   */
  nome: string;
  /** Quanti brani ci sono qui sotto, **sottocartelle comprese**. */
  brani: number;
  /** Quante sottocartelle dirette: è quel che dice se disegnare la freccia. */
  sottocartelle: number;
  /**
   * Quanti brani stanno **direttamente** qui: le foglie che compaiono aprendo
   * la cartella.
   *
   * Facoltativo per i dati finti dello Studio, che lo precedono: senza, la
   * cartella si apre solo se ha sottocartelle, com'era prima delle foglie.
   */
  file?: number;
  /**
   * È una delle cartelle sorvegliate.
   *
   * `false` sui nodi sintetici, cioè quelli nati per dare un posto a un brano
   * che sotto nessuna radice sorvegliata non sta. Solo su una radice vera ha
   * senso chiedersi se risponde ancora.
   */
  radice: boolean;
  /**
   * Il servizio, solo sulle radici del cloud: per quelle `nome` è già
   * «OneDrive · Musica › Rock». Una radice di un conto non collegato arriva
   * come le radici che non rispondono, con questo in più.
   *
   * Facoltativo perché c'è soltanto lì. Il pannello, per l'icona e per il
   * titolo, guarda comunque il percorso (`cartelleNuvola.ts`): così vale
   * anche per i nodi finti dello Studio, che il campo non lo portano.
   */
  servizio?: ServizioNuvola;
}

/** Un file dentro una cartella: una foglia dell'albero del pannello «Cartelle». */
export interface FileCartella {
  /** Il brano: è quel che va in coda. */
  brano: number;
  /** Il percorso del file, nella grafia del database: è l'identità della riga. */
  percorso: string;
  /** Il nome del file, senza la cartella: è quel che decide l'ordine. */
  nome: string;
  /** Il titolo, come lo dicono i tag: è quel che la riga scrive. */
  titolo: string;
  durataMs: number;
}

/** Una cartella aperta dal filtro, con quel che le resta dentro. */
export interface RamoFiltrato {
  /** La cartella; `null` per il primo livello. */
  percorso: string | null;
  /** Le sottocartelle che sono una corrispondenza o portano a una. */
  cartelle: NodoCartella[];
  /** I file di questa cartella che corrispondono. */
  file: FileCartella[];
}

/** L'albero potato da un filtro: vedi `cartelle.rs::filtra`. */
export interface CartelleFiltrate {
  rami: RamoFiltrato[];
  /** Le cartelle il cui nome corrisponde, per distinguerle dagli antenati. */
  trovate: string[];
  /** Quante corrispondenze c'erano in tutto, prima del tetto. */
  corrispondenze: number;
  /** Erano più di quelle arrivate. */
  troncato: boolean;
}

/** Com'era rimasto il pannello «Cartelle» all'ultima chiusura. */
export interface StatoUiCartelle {
  /** I nodi aperti, dal meno recente al più recente. */
  aperte: string[];
  /** Il nodo su cui stava il fuoco, se ce n'era uno. */
  scelta: string | null;
}

/** I percorsi trascinati sulla finestra, divisi per destinazione. */
export interface Trascinati {
  cartelle: string[];
  skin: string[];
  playlist: string[];
  /** I brani in libreria dei file audio, nell'ordine in cui sono arrivati. */
  brani: number[];
  /** File audio che la libreria non conosce. */
  fuoriLibreria: number;
  /** Né cartelle né file che Rymembra sappia usare. */
  ignorati: number;
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

/**
 * Cosa ha portato via un'esportazione del profilo.
 *
 * Dalla 2.3.1 il profilo non è più un JSON ma un archivio `.ryprofile`, e non
 * c'è più nessun documento che passi di qui: il nucleo lo scrive direttamente
 * sul file scelto, una voce alla volta, perché con le copertine dentro può
 * pesare centinaia di megabyte.
 */
export interface EsportazioneProfilo {
  /** Dove è finito. */
  percorso: string;
  /** Quante preferenze ha portato via. */
  voci: number;
  /**
   * Le chiavi presenti nel database e rimaste qui.
   *
   * Il profilo porta un elenco di **inclusioni** — `nuvola.dispositivo` e la
   * coda non devono viaggiare — e questa è la riga che impedisce all'elenco di
   * essere silenzioso.
   */
  lasciate: string[];
  /** Quante righe di cronologia d'ascolto. */
  cronologia: number;
  /** Quante copertine distinte. */
  copertine: number;
  /** Quanti pacchetti skin. */
  skin: number;
  /** Quante bozze dello Studio. */
  bozze: number;
  /** Quanto pesa il file. */
  byte: number;
  /** Pesa più di mezzo gigabyte, e vale la pena dirlo prima della chiavetta. */
  pesante: boolean;
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

/**
 * Una radice del profilo che qui non c'è, e dove metterla.
 *
 * Il piano ne propone una per ogni radice mancante, con `a` vuoto; chi importa
 * la compila e ripete il piano. Il confronto passa da `path_key`, quindi
 * `D:\Musica`, `d:/musica` e `D:\Musica\` sono lo stesso prefisso.
 */
export interface RimappaturaRadice {
  /** Il prefisso come sta nel profilo. */
  da: string;
  /** Il prefisso su questo computer. Vuoto = non ancora scelto. */
  a: string;
  /** Quanti brani di **questa** libreria stanno sotto `da`. */
  brani: number;
}

/**
 * Quante cose la parte di libreria porterebbe.
 *
 * Piatta e non annidata di proposito: sotto ci sono due strutture del nucleo
 * con due convenzioni di nome diverse, e appiattirle di là evita che di qua si
 * scriva un campo che non esiste.
 */
export interface PortatiProfilo {
  ascolti: number;
  voti: number;
  preferiti: number;
  posizioni: number;
  playlist: number;
  cartelle: number;
  cronologia: number;
  correzioni: number;
  testi: number;
  desiderati: number;
  copertine: number;
  /** File di copertina che qui non ci sono e si scriverebbero. */
  fileCopertine: number;
}

/** Cosa farebbe importare un profilo. */
export interface PianoProfilo {
  /** Quando il profilo è stato scritto. */
  creatoMs: number;
  /** 1 il vecchio `.json`, 2 l'archivio. Il primo si legge e non si scrive più. */
  versione: number;
  /**
   * Il profilo viene da un'altra libreria.
   *
   * Allora la parte di libreria **non** si applica: le chiavi di brano di là
   * nominano canzoni che qui non ci sono. Preferenze, copertine e pacchetti
   * skin passano lo stesso, perché non parlano di brani.
   */
  identitaDiversa: boolean;
  cambi: CambioProfilo[];
  /** Le chiavi del file che questa versione non porta. Non è un guasto. */
  sconosciute: string[];
  /** I percorsi che arrivano e che su questo computer non esistono. */
  percorsiMancanti: string[];
  /** Quante chiavi sono già uguali a quel che c'è. */
  invariate: number;
  /** Le radici da rimappare, con quanti brani di qui stanno sotto ciascuna. */
  rimappature: RimappaturaRadice[];
  /** Percorsi di brani riscritti dalla rimappatura. */
  percorsiRiscritti: number;
  /** Brani il cui file, sotto il prefisso nuovo, non si trova. */
  braniIrrintracciabili: number;
  /** Brani il cui percorso nuovo è già di un'altra riga. */
  braniGiaPresenti: number;
  /**
   * Brani il cui file, sotto il prefisso nuovo, non si è potuto verificare: la
   * cartella di rete non rispondeva. Non sono «non trovati». Assente nella
   * risposta dell'annullamento, che non rimappa niente.
   */
  braniNonVerificati?: number;
  /** I numeri della rimappatura vengono da un campione: è il piano. */
  stima?: boolean;
  portati: PortatiProfilo;
}

/**
 * A che punto è un'esportazione o un'importazione del profilo.
 *
 * Stessa forma di `AvanzamentoNuvola`, perché è la stessa domanda e due forme
 * diverse sarebbero due componenti da disegnare.
 */
export interface AvanzamentoProfilo {
  fatti: number;
  totale: number;
  /** `archivio` mentre si scrive, `copia` per la copia di sicurezza, `copertine` in lettura. */
  cosa: "archivio" | "copia" | "copertine";
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
   * Davanti a una strage non si distrugge: la scansione automatica aspetta la
   * passata dopo, quella a mano chiede — vedi `daConfermare`.
   */
  rimozioniRinviate: number;
  /**
   * Le rimozioni trattenute che aspettano un sì, o `null`.
   *
   * Solo un riassunto: l'elenco vero resta al nucleo, e la conferma rimanda
   * indietro il `gettone` perché tolga esattamente quelle righe.
   */
  daConfermare: RimozioniDaConfermare | null;
  /**
   * Cartelle illeggibili dentro cartelle sorvegliate che rispondevano: i brani
   * lì sotto non sono stati né tolti né riscritti.
   */
  cartelleIlleggibili: string[];
  /** Quante righe stanno sotto quelle cartelle, lasciate stare. */
  nonVerificati: number;
  numeri: Numeri;
}

/** Le rimozioni trattenute sotto una cartella sorvegliata, in breve. */
export interface RimozioniSottoUnaRadice {
  /** La cartella sorvegliata; vuota se la riga non sta sotto nessuna. */
  radice: string;
  quante: number;
  /** I primi percorsi, per riconoscerle. */
  esempi: string[];
}

/** Quel che una scansione a mano chiede prima di togliere troppo. */
export interface RimozioniDaConfermare {
  /** Il segno di questo elenco: la conferma lo rimanda indietro. */
  gettone: number;
  quante: number;
  radici: RimozioniSottoUnaRadice[];
}

/** Com'è andata una conferma di rimozioni. */
export interface EsitoRimozioni {
  /** Quante righe sono state tolte davvero: zero se il gettone era superato. */
  tolti: number;
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

/**
 * In che punto è una scansione chiesta a mano.
 *
 * - `cerca`: la camminata delle cartelle, che sul NAS dura una decina di
 *   secondi e prima non diceva niente — la barra restava a zero su zero;
 * - `legge`: la lettura dei file, `fatti` su `totale`, come sempre;
 * - `attende`: «Scansiona» aspetta che finisca una scansione partita da sola,
 *   invece di rispondere «occupato».
 */
export type FaseScansione = "cerca" | "legge" | "attende";

/**
 * L'avanzamento di una scansione.
 *
 * `fase`, `cartelle` e `trovati` facoltativi: un nucleo di prima manda soltanto
 * i due numeri, e la finestra allora si comporta come allora — vedi
 * `fasiScansione.ts`. Facoltativi anche perché lo Studio disegna il toast con
 * un avanzamento finto scritto a mano.
 */
export interface Avanzamento {
  fase?: FaseScansione;
  /** File letti finora, nella fase `legge`. */
  fatti: number;
  /** File da leggere in tutto; zero finché non si sa. */
  totale: number;
  /** Cartelle elencate finora, nella fase `cerca`. */
  cartelle?: number;
  /** File trovati finora, nella fase `cerca`. */
  trovati?: number;
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

/**
 * Com'è andata un'aggiunta a una playlist (`playlists::Aggiunta`).
 *
 * `saltati` sono i brani che la finestra aveva ancora in mano e che una
 * scansione, nel frattempo, ha tolto dalla libreria: non entrano, e si dice.
 */
export interface AggiuntaPlaylist {
  playlist: Playlist;
  aggiunti: number;
  saltati: number;
}

/**
 * Com'è andata un'esportazione (`playlist::PlaylistEsportata`): i brani
 * entrati nel file e quelli rimasti fuori, del cloud o di un catalogo, che un
 * altro lettore non saprebbe aprire. Brani e non voci: una fila dello stesso
 * `.cue` è una voce sola nel file, e conta tutti i suoi brani.
 */
export interface PlaylistEsportata {
  scritti: number;
  lasciatiFuori: number;
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
  /** `local` se l'ha suonato Rymembra, `spotify` se importato da un account. */
  sorgente: string;
}

export type Ordine = "scaffale" | "recenti" | "ascoltati" | "titolo";

/**
 * Quale elenco di brani si sta guardando, detto al nucleo.
 *
 * Le stesse domande delle pagine — `brani`, `preferiti`, `cerca` — senza la
 * pagina: serve a chiedere gli identificativi della vista **intera**, per
 * «Seleziona tutto» e per suonare da una lista di cui la finestra ha in mano
 * solo le pagine guardate. Vedi `src-tauri/src/viste.rs`.
 */
export type VistaBrani =
  | { tipo: "brani"; ordine: Ordine }
  | { tipo: "preferiti" }
  | { tipo: "ricerca"; query: string };

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
 * Di chi è il livello a cui si normalizza: del brano, del disco, o si sceglie
 * da sé — il disco quando lo si ascolta in ordine, il brano altrimenti.
 */
export type ModoLivello = "brano" | "album" | "automatico";

/** Quanti brani con un file la passata di sottofondo ha già misurato. */
export type AvanzamentoVolume = {
  /** Misurati, muti o illeggibili: quelli di cui la passata si è occupata. */
  misurati: number;
  totale: number;
};

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
 * Il massimo di latenza d'uscita che si può dichiarare, in valore assoluto.
 *
 * Mezzo secondo, nei due versi. Non è un limite tecnico — una catena Bluetooth
 * scadente ci arriva — è il punto oltre il quale la correzione smette di
 * correggere: a mezzo secondo il cursore sta già visibilmente dietro la musica, e
 * chi continuasse ad alzare starebbe cercando di risolvere un problema diverso.
 * Il nucleo taglia comunque: questo serve al cursore per sapere dove finire.
 */
export const LATENZA_MASSIMA_MS = 500;

/**
 * Com'è fatto quel che sta suonando.
 *
 * Le colonne del database corrette dal decodificatore appena il brano si sente:
 * il codec vero (ALAC dentro un M4A, l'MP3 che si ascolta di un FLAC
 * dell'Internet Archive), e per un flusso, che colonne non ne ha, tutto.
 *
 * La riga descrive l'**edizione**, non l'uscita: se la scheda audio ricampiona
 * non lo scrive — cambierebbe cambiando cuffia — e lo dice solo a chi ci si
 * ferma sopra, con `outputRate`.
 *
 * Ogni campo può mancare, e non è un caso limite: le quattro colonne sono
 * nullable, `lofty` non ricava le proprietà di tutti i contenitori allo stesso
 * modo, e una libreria scansionata da una versione più vecchia le ha vuote. Chi
 * disegna unisce i pezzi che ci sono e tace sugli altri.
 */
export interface FormatoFile {
  /** «FLAC», «MP3», «M4A»: già tradotto dal nucleo, qui non si mappa niente. */
  codec: string | null;
  /** In hertz — 44100 — perché i kilohertz con la virgola sono lingua. */
  sampleRate: number | null;
  /** Il numero, non il nome: «stereo» e «5.1» sono parole, e si traducono. */
  channels: number | null;
  /**
   * Il bitrate **medio**, in kbit/s.
   *
   * Medio e non dichiarato: su un VBR dice quel che il file pesa davvero, e su
   * un FLAC — dove pure si mostra — dice quanto è densa l'edizione.
   */
  bitrate: number | null;
  /** Bit per campione, solo dei formati senza perdita: lo dice il motore. */
  bitsPerSample: number | null;
  /** A quanti hertz esce dalla scheda audio. Diverso da `sampleRate`: ricampionato. */
  outputRate: number | null;
}

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
  /**
   * Di quanti millisecondi la catena d'uscita ritarda il suono, dichiarati.
   *
   * La correzione a mano, non quella che il motore misura da sé: mixer di
   * sistema, driver, DAC, e su un'uscita senza fili la radio. Positiva racconta
   * la posizione più indietro, negativa più avanti. `0` è «nessuna correzione»,
   * ed è il valore di serie.
   */
  latenzaMs: number;
  /**
   * Di quanto una riga di testo si accende prima del suo tempo, in millisecondi.
   *
   * Viene dal nucleo — è `rymembra_domain::testo::ANTICIPO_MS` — e non si ripete
   * qui: la stessa ricerca della riga accesa esiste in Rust e in
   * `parti/Testo.tsx`, e per un po' il numero è stato scritto a mano in tutti e
   * due i posti. Chi lo legge passa da `riproduzione.ts::anticipoAdesso`.
   */
  anticipoMs: number;
  /** Il motore audio non c'è: perché, e se vale la pena riaprire. */
  audio: GuastoAudio | null;
  /**
   * Da quale uscita esce il suono adesso.
   *
   * `null` quando il motore non si è aperto. È il nome del dispositivo
   * **davvero** aperto, che può non essere quello scelto: una preferenza che
   * punta a una scheda staccata ripiega sul predefinito.
   */
  uscita: string | null;
  /**
   * Perché il brano dopo è quello, quando l'ha scelto l'autoplay.
   *
   * Un **codice** — `album`, `suono`, `ascolti`, `datanto`… — e non una frase:
   * la frase sta in `lingue/`, sotto `queue.why.<codice>`. `null` quando il
   * brano dopo l'hai messo tu, e allora non c'è niente da spiegare.
   */
  motivoProssimo: string | null;
  /**
   * Com'è fatto il file che sta suonando, se si deve mostrare.
   *
   * `null` vuol dire «non si disegna», e nient'altro: non c'è nessun brano, il
   * brano non è più in libreria, oppure la preferenza
   * `player.fileFormat.visible` è spenta. **Qui non si legge nessuna
   * preferenza**: il filtro sta nel nucleo, in un posto solo, e la riga si
   * mostra se e solo se il dato è arrivato.
   *
   * È deliberato. Leggere la preferenza qui vorrebbe dire una copia per ogni
   * schermata che disegna la riga, e chi spegne l'interruttore nelle
   * Impostazioni non sta guardando «In riproduzione»: al ritorno troverebbe la
   * riga ancora lì, disegnata da una copia stantia. Il setter nel nucleo rimanda
   * lo stato, e le due viste cambiano insieme.
   */
  formato: FormatoFile | null;
  /**
   * «Ascolta insieme», quando questo PC è in una sessione. `null` fuori.
   * Gemello di `InsiemeNelLettore` in `src-tauri/src/insieme/ipc.rs`.
   */
  insieme: InsiemeNelLettore | null;
  /**
   * Suona, ma i dati non arrivano: il cursore è fermo e lo si dice.
   *
   * Non lo manda `riproduzione:stato`: lo porta il colpo d'orologio (vedi
   * `Tempo.inAttesa`), e `useRiproduzione` lo aggiunge qui. Assente vale no.
   */
  inAttesa?: boolean;
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
 * Quanto la scena dello spettro può costare a questa macchina.
 *
 * Un'unione di tre stringhe e non un `string`, ed è onesta per costruzione: in
 * Rust `Qualita` porta `#[serde(rename_all = "lowercase")]`, quindi queste tre
 * parole sono **esattamente** quel che arriva sul filo e quel che finisce nel
 * database. Un quarto livello aggiunto di là senza aggiungerlo di qua è un
 * errore di compilazione qui, che è il posto giusto per accorgersene.
 *
 * Al contrario di quante barre disegnare e di se la scena parte accesa, questa
 * **non** viaggia nel profilo: descrive quel che questo computer regge, non
 * quel che chi ascolta preferisce.
 */
export type QualitaSpettro = "auto" | "alta" | "bassa";

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
  /**
   * Cosa è successo, in una frase **italiana**.
   *
   * Nasce dentro il motore, e da lì non può che uscire in una lingua sola: è
   * il ripiego di `causaCodice`, non quel che si disegna quando c'è di meglio.
   */
  causa: string;
  /**
   * Perché il dispositivo è sparito, in un codice traducibile.
   *
   * `deviceNotAvailable`, `systemFailure`, `unknown`. Sta accanto a `causa` per
   * la stessa ragione per cui `motivoProssimo` è un codice: una frase italiana
   * che attraversa l'IPC resta italiana anche con l'interfaccia in inglese, e
   * qui si leggeva «There is no audio. dispositivo non più disponibile.»
   *
   * `null` quando il motore non si è mai aperto: là la causa è un errore del
   * nucleo, e il suo codice sta già in `codice`.
   */
  causaCodice: string | null;
  /** Riaprire ha senso provarlo. */
  riapribile: boolean;
  /**
   * L'uscita fissata che si sta aspettando, se è lei che se n'è andata.
   *
   * Le cuffie fissate che si staccano non mandano più la musica agli
   * altoparlanti: il nucleo aspetta che tornino, e riprende da dov'era. Con
   * questo nome l'avviso dice un'attesa invece di un guasto, e il tasto
   * «Riapri» — che qui suona su un'altra uscita — può dire perché.
   */
  attesa: string | null;
}

/**
 * Un'uscita audio.
 *
 * # Perché c'è `presente`, invece di elencare solo quel che esiste
 *
 * Perché una scelta che sparisce dall'elenco racconta una bugia. Staccato il
 * DAC, la schermata mostrerebbe «predefinito di sistema» selezionato — cioè
 * che la preferenza è stata dimenticata — mentre invece è ancora scritta e
 * tornerà buona appena si riattacca il cavo. Un rigo spento dice la verità.
 */
export interface DispositivoAudio {
  /** L'identità, che è il nome che dà il sistema. */
  id: string;
  /** Come chiamarlo a schermo. */
  nome: string;
  /** È quello che il sistema usa di suo. */
  predefinito: boolean;
  /** C'è adesso. Falso per la scelta rimasta scritta di un cavo staccato. */
  presente: boolean;
  /** È quello da cui sta uscendo il suono in questo momento. */
  attivo: boolean;
  /**
   * È quello chiesto per nome, e non «quello che il sistema usa».
   *
   * Lo dice il nucleo invece di ricavarlo da `attivo` e `predefinito`, perché
   * da quei due non si ricava: chi ha fissato la scheda che *è anche* la
   * predefinita ha fatto una scelta diversa da chi ha lasciato «segui il
   * sistema», e dall'esterno le due si vedono identiche.
   */
  scelto: boolean;
}

/** Solo il tempo che passa: arriva quattro volte al secondo mentre suona. */
export interface Tempo {
  posizioneMs: number;
  durataMs: number;
  inPausa: boolean;
  /**
   * Non è in pausa, ma da mezzo secondo la musica non avanza perché i dati
   * non arrivano: la rete di un flusso, una condivisione lenta. La finestra
   * smette di far correre il cursore e lo dice.
   *
   * Facoltativo perché lo stesso ancoraggio riceve anche lo stato intero, che
   * non lo porta: assente vale no.
   */
  inAttesa?: boolean;
}

/**
 * Quante bande ha l'equalizzatore, e dove stanno.
 *
 * Ricopiate da `rymembra_play::equalizzatore::CENTRI_HZ`, che è l'originale. Qui
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
