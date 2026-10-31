/** Parte del contratto con il nucleo: le skin compilate e il registro dello Skin Studio. La riesporta `ipc.ts`: si importa da lì. */
import type { ErroreIpc } from "./errori";

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
  /** Il tema di base della skin. */
  scheme: Schema;
  /**
   * Ha la variante del tema opposto.
   *
   * Senza, la skin ha un tema solo e la scelta dell'utente non la cambia:
   * `data-theme` scrive comunque quello che si vede.
   */
  variante: boolean;
  /**
   * La skin permette all'accento di seguire la copertina.
   *
   * È una dichiarazione dell'autore e vince sulla preferenza: `vinile` dice di
   * no perché è costruita attorno al suo accento.
   */
  dynamicAccent: boolean;
  layout: ImpaginazioneSkin;
  /**
   * Le parti che entrano, escono o volano con una transizione di vista.
   *
   * Un innesco assente vuol dire nessuna transizione per quella parte: senza
   * una regola che la raccolga il motore farebbe la dissolvenza di serie.
   */
  presenze: Presenza[];
  /**
   * La skin che si sarebbe dovuta indossare e non si è potuta leggere.
   *
   * `null` quasi sempre. Quando c'è, quel che è arrivato è la skin di serie:
   * il pacchetto scelto non si compila — cancellato, spostato, o su un disco
   * che adesso non c'è — e il nucleo ha ripiegato invece di lasciare la
   * finestra senza impaginazione, che vuol dire senza niente. La scelta nel
   * database resta quella di prima, quindi il ripiego dura solo finché dura
   * l'assenza.
   */
  ripiego: string | null;
}

/** Il tema di base di una skin, nella parola del formato. */
export type Schema = "dark" | "light";

/** Una parte con una transizione di vista, e quali inneschi dichiara. */
export interface Presenza {
  /** Il nome della parte: la classe è `vt-<parte>-<innesco>`. */
  parte: string;
  enter: boolean;
  exit: boolean;
  share: boolean;
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
  /** Il tema di base. */
  scheme: Schema;
  /** Ha la variante del tema opposto. */
  variante: boolean;
  /**
   * Quel che chi l'ha appena installata deve sapere prima di indossarla.
   * Solo nella risposta di `skinInstalla`/`skinInstallaSorgente`, mai
   * nell'elenco. Vedi `EsameSkin`.
   */
  esame?: EsameSkin;
  /**
   * Il codice del guasto di una skin installata che non passa più la
   * validazione: da aprire nello Studio, non da indossare. Assente per tutte
   * le altre.
   */
  guasto?: string;
}

/** Un costo, e il budget su cui il nucleo lo misura. */
export interface MisuraSkin {
  costo: number;
  budget: number;
}

/** Un ciclo che dura quanto lo stato che lo accende. */
export interface CicloSkin {
  /** La parte che gira. */
  parte: string;
  /** Lo stato che lo accende: `playing`, `hover`, … */
  stato: string;
  /** Il nome dell'animazione. */
  animazione: string;
}

/**
 * L'esame di una skin appena installata: `skin.rs::EsameIpc`.
 *
 * Che cosa meriti una domanda prima di indossarla lo decide il nucleo, in
 * `daConfermare`: la finestra non rifà il conto sui budget, lo mostra.
 */
export interface EsameSkin {
  /** La superficie più cara, sul budget di una superficie. */
  superficie: MisuraSkin;
  /** La parte che si muove di più, sul budget di una parte. */
  movimentoParte: MisuraSkin;
  /** Il movimento della finestra intera, sul suo budget. */
  movimento: MisuraSkin;
  /** Lo scafale, sul suo budget. */
  scafale: MisuraSkin;
  /** Si muove. */
  anima: boolean;
  /** Quel che gira per sempre, finché dura lo stato che lo accende. */
  cicli: CicloSkin[];
  /** Quante coppie di testo e superficie non si leggono. */
  contrasti: number;
  /** Quanti anelli del fuoco non si vedono: sotto 3:1 contro le superfici. */
  anelli: number;
  /** C'è qualcosa da chiedere prima di indossarla. */
  daConfermare: boolean;
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

/**
 * Un preset del registro: un blocco di valori che si scrive in un colpo solo.
 *
 * Non è un oggetto che la skin ricorda di aver applicato — non c'è nessun campo
 * `preset` nel documento, nessun riferimento da risolvere, nessuna eredità. È
 * una scrittura: si applica, e da quel momento la skin è indistinguibile da una
 * in cui qualcuno avesse battuto a mano gli stessi numeri.
 *
 * Per questo `valori` porta dei frammenti JSON e non dei valori tipizzati: la
 * sorgente di verità dello Studio è il testo del documento, e applicare un
 * preset è innestare quei frammenti con lo stesso `scriviIn` di `patch.ts` che
 * usa un cursore. Il tipo giusto non è quello del valore, è `string`.
 *
 * La tabella vive in Rust perché lì è il validatore a possederla: un token
 * rinominato o un numero fuori dai `limiti` cadono in una prova del crate,
 * invece che addosso a un autore che si vede rifiutare il salvataggio per un
 * valore che non ha scelto lui.
 */
export interface PresetRegistro {
  id: string;
  /** Il nome sul bottone. Arriva già tradotto dal nucleo, come le descrizioni. */
  nome: string;
  /**
   * Il gruppo di token su cui agisce, nello stesso vocabolario di
   * `TokenRegistro.group`.
   *
   * È l'unico campo che l'interfaccia confronta: la striscia si costruisce dal
   * gruppo del token selezionato, così nessun componente dello Studio deve
   * nominare un preset o un token per nome.
   */
  group: string;
  /** Le scritture, in coppie `[id del token, frammento JSON]`. */
  valori: [string, string][];
}

/** Una parte del registro. */
export interface ParteRegistro {
  name: string;
  group: string;
  description: string;
  /** Ha `::before` libero per un livello, dietro al contenuto. */
  before: boolean;
  /** Ha `::after` libero per un livello, sopra al fondo. */
  after: boolean;
  /** Entra ed esce dal documento come un'unità: ammette `enter` ed `exit`. */
  presenza: boolean;
  /** Può volare fra due schermate: ammette `share`. */
  condivisa: boolean;
}

/** Il tipo di una manopola d'effetto. Decide quale controllo la modifica. */
export type TipoParametro =
  | "color"
  | "length"
  | "angle"
  | "number"
  | "stops"
  | "corners"
  | "word"
  | "point"
  | "asset"
  | "flag";

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
  presets: PresetRegistro[];
  parts: ParteRegistro[];
  effects: EffettoRegistro[];
  widgets: WidgetRegistro[];
  vocabolario: VocabolarioRegistro;
  budget: number;
  /** Il budget dello scafale, che è un budget diverso da quello delle superfici. */
  shellBudget: number;
  format: number;
  contrastoMinimo: number;

  // I tetti del movimento, da `core/rymembra-skin/src/movimento.rs`. Stavano
  // ricopiati a mano in cima a `studio/Movimento.tsx`, col commento che diceva
  // che la strada giusta era farli passare di qui: sono gli stessi numeri con
  // cui il validatore rifiuta, e due copie sono due copie che divergono.
  /** `MAX_ANIMAZIONI`: quante un documento può dichiarare. */
  maxAnimazioni: number;
  /** `MIN_FOTOGRAMMI`: con uno solo non c'è interpolazione, c'è uno stato. */
  minFotogrammi: number;
  /** `MAX_FOTOGRAMMI`. */
  maxFotogrammi: number;
  /** `MAX_DURATA_MS`. */
  maxDurataMs: number;
  /** `MAX_RITARDO_MS`. */
  maxRitardoMs: number;
  /** `MAX_ITERAZIONI`, oltre a «infinite» sugli stati. */
  maxIterazioni: number;
  /** `MOTION_COST_BUDGET`: per parte, e non si somma con gli altri due. */
  motionBudget: number;
  /** `WINDOW_MOTION_BUDGET`: la somma su tutte le parti. */
  windowMotionBudget: number;
  /** Il peso di `CostClass::Composited`: da qui parte `animation_cost`. */
  pesoComposito: number;
  /** I versi, da `AnimDirection::ALL`. */
  versi: string[];
  /** I trigger, da `AnimTrigger::nomi()`. `enter` è la regola base. */
  trigger: string[];
  /** Gli stati del DOM, nell'ordine in cui il compilatore li emette. */
  stati: StatoRegistro[];
  /** Gli inneschi, con quel che ammettono. */
  inneschi: InnescoRegistro[];
  /** I campi di un fotogramma, coi loro estremi. */
  fotogramma: CampoFotogramma[];
  /** Le animazioni pronte, da `preset.rs`. */
  animazioniPronte: AnimazionePronta[];
  /** `both`, `forwards`, `backwards`, `none`. */
  riempimenti: string[];
  /** Le sorgenti dinamiche di un colore: `albumArt.vibrant` e le altre. */
  sorgenti: string[];
  /**
   * Le chiavi che il formato accetta, per contesto — `documento`, `aspetto`,
   * `animazione`, `fotogramma`… — dalle liste con cui il validatore rifiuta.
   */
  chiavi: Record<string, string[]>;
  /** Le parole chiuse di un campo, per nome: `scheme`, `borderStyle`, `easing`… */
  parole: Record<string, string[]>;
}

/** Uno stato del DOM e il selettore che lo riconosce. */
export interface StatoRegistro {
  name: string;
  /** Per una parte che prende il fuoco da sé. */
  selector: string;
  /** Per una parte il cui fuoco lo prende un figlio. */
  dentro: string;
}

/** Un innesco del movimento. */
export interface InnescoRegistro {
  name: string;
  /** Ammette `"infinite"`: solo gli stati. */
  ciclo: boolean;
  /** Si realizza come transizione di vista: `exit` e `share`. */
  vista: boolean;
  /** È uno stato del DOM. */
  stato: boolean;
}

/** Un campo di un fotogramma. */
export interface CampoFotogramma {
  name: string;
  kind: "number" | "offset" | "angle" | "length" | "color" | "shadow" | "clip";
  /** Il valore di un elemento che non si anima. */
  riposo: number | null;
  min: number | null;
  max: number | null;
}

/** Un'animazione pronta da mettere nel documento. */
export interface AnimazionePronta {
  id: string;
  nome: string;
  innesco: string;
  /** La dichiarazione, come frammento JSON. */
  valore: string;
}

/** Un problema che blocca. */
export interface Problema {
  code: string;
  path: string;
  message: string;
  /** Il nome che forse si voleva scrivere, da `vicini()`. */
  forse: string[];
  /**
   * La riga in cui è scritto, da uno.
   *
   * La calcola il nucleo (`rymembra_skin::posizioni`) su una passata sola del
   * testo. Prima non c'era, e la finestra la indovinava cercando l'ultimo pezzo
   * del percorso col primo `indexOf` che corrispondeva — su
   * `parts.x.background.0.stops.1.color` finiva a sottolineare la prima riga
   * che nominasse un colore qualunque.
   */
  riga: number | null;
  /** La colonna, da uno, nelle stesse unità che conta la `<textarea>`. */
  colonna: number | null;
}

/** Un avviso, che non blocca. */
export interface Avviso {
  kind:
    | "missingRequiredToken"
    | "unkeptCapability"
    | "unusedPattern"
    | "unusedPrefab"
    | "unusedAnimation"
    | "costBudget"
    | "restingLoop"
    | "contrast";
  path: string;
  message: string;
  /** La riga in cui è scritto, come per {@link Problema}. */
  riga: number | null;
  /** La colonna, da uno. */
  colonna: number | null;
}

/** Una coppia di colori misurata. */
export interface Contrasto {
  davanti: string;
  dietro: string;
  /** Il rapporto nel tema di base. */
  base: number;
  /** Il rapporto nella variante, se la skin ne ha una. */
  variante: number | null;
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
export type Causa = "derivata" | "salvata" | "esportata" | "manuale" | "migrata";

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
  /**
   * Quanto costa il movimento, sommato su tutte le parti animate.
   *
   * Terzo numero e terzo budget. Il verdetto resta del nucleo — l'avviso
   * `costBudget` è **per parte** — e questo è il totale, cioè il numero che si
   * legge accanto agli altri due.
   */
  costoMovimento: number;
  parti: number;
  /** Quanto costa ogni parte ridisegnata, sui suoi due budget. */
  costi: CostoParte[];
  /**
   * Quanto movimento può essere in corso nello stesso momento: il numero che
   * `windowMotionBudget` misura.
   */
  costoFinestra: number;
  /** Le risorse che il documento nomina. */
  risorse: string[];
  dinamici: string[];
  compilatoMs: number;
  /**
   * L'id sotto cui il foglio è compilato — quello della bozza, non quello
   * del documento — e quindi quel che la tela scrive in `data-skin`. Vuoto
   * quando ci sono errori.
   */
  radice: string;
}

/** Com'è andato un salvataggio della bozza: `studio.rs::SalvataggioIpc`. */
export interface Salvataggio {
  /** Dove sta la bozza adesso: l'id nuovo, se il documento l'ha cambiato. */
  id: string;
  /** Perché la bozza non ha potuto prendere l'id che il documento dichiara. */
  rinomina: ErroreIpc | null;
}

/** Il costo di una parte. */
export interface CostoParte {
  parte: string;
  /** Motivi, livelli e filtri. */
  superficie: number;
  /** Le animazioni assegnate. */
  movimento: number;
}

/** Un colore della tavolozza generata, per un token. */
export interface VoceTavolozza {
  token: string;
  /** `#rrggbb` o `#rrggbbaa`. */
  valore: string;
}

/** Un testo portato al formato di oggi. */
export interface Migrato {
  sorgente: string;
  /** Di quale formato era. */
  da: number;
}
