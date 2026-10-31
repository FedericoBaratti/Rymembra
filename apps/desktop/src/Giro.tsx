/**
 * Il giro guidato: un benvenuto, e tredici riflettori sui comandi veri.
 *
 * # Perché non un carosello
 *
 * La forma facile sarebbe stata dieci schermate con dentro dieci immagini, o
 * dieci disegni. Costa poco e insegna poco: quel che si impara è dove stavano
 * le cose **nella figura**, e la figura invecchia al primo ritocco — o al primo
 * skin installato, che qui è un caso normale e non un'ipotesi. Chi lo ha
 * guardato sa di aver guardato qualcosa e poi cerca il cuore dove il carosello
 * lo aveva disegnato.
 *
 * Questo giro invece punta un riflettore **sull'elemento vero, dov'è adesso**:
 * il velo si scurisce, resta un buco sulla cosa di cui si sta parlando, e il
 * fumetto la spiega. Se una skin sposta la navigazione in fondo alla finestra,
 * il riflettore la segue là, perché non sa dove sia — lo chiede al DOM.
 *
 * # Le ancore sono attributi, non classi
 *
 * `data-giro="navigazione"` e non `.nav-pill`. Le classi sono **il contratto
 * delle skin**: `parts.rs` le dichiara perché una skin possa ridipingerle, e una
 * skin che le muove o le annida diversamente sta facendo esattamente quel che
 * il formato le promette. Un giro agganciato a `.nav-pill` si romperebbe quindi
 * per un uso legittimo del programma, e si romperebbe **in silenzio** — un velo
 * senza buco, che è la cosa peggiore che questo file possa produrre.
 *
 * `data-giro` non lo tocca nessuno: non lo legge il compilatore delle skin, non
 * lo scrive `stile.css`, non compare in nessun selettore che non sia qui
 * dentro. È l'unico modo di avere un aggancio che non è anche un'altra cosa.
 *
 * L'unica ancora che non porta quell'attributo è la ricerca, che si trova con
 * `[data-cerca]`: quell'attributo esiste già in `parti/Intestazione.tsx` per la
 * scorciatoia `/`, ha esattamente lo stesso mestiere — «raggiungi quel campo da
 * qualunque schermata» — e aggiungergliene accanto un secondo che dice la
 * stessa cosa sarebbe due nomi per un aggancio solo.
 *
 * Le altre eccezioni sono tre, e stanno scritte qui perché si vedano:
 *
 * - **il menù di un brano** si trova con `[role="menu"]`. Non è una classe: è
 *   la promessa che `Menu.tsx` fa a chi legge lo schermo, e un menù che la
 *   perdesse avrebbe un guasto più grave di un passo saltato;
 * - **l'albero delle Cartelle** (`.albero-cartelle`) e **la scheda delle
 *   scorciatoie** (`section.scheda:has(.scorciatoia)`) si trovano per classe.
 *   Nessuna delle due sta nell'impaginazione di una skin — una skin le
 *   ridipinge, non le sposta — quindi il guasto descritto sopra qui non
 *   succede. Ma sono un debito: un nome cambiato in quei due componenti fa
 *   saltare il passo senza dirlo, e il rimedio è un `data-giro` loro, come
 *   tutti gli altri.
 *
 * # Il buco: un'ombra, non quattro rettangoli e non un ritaglio
 *
 * Il riflettore è **un** elemento vuoto messo sul rettangolo del bersaglio, con
 * `box-shadow: 0 0 0 9999px <velo>`: l'ombra dipinge tutto **fuori** da quel
 * rettangolo, e l'angolo arrotondato lo prende dal `border-radius` copiato dal
 * bersaglio. Un elemento, nessuna giuntura, e gli angoli tondi gratis.
 *
 * Le due strade che qualcuno vorrà «correggere», e perché sono peggio:
 *
 * - **Quattro rettangoli** attorno al buco (sopra, sotto, sinistra, destra):
 *   sono quattro elementi da tenere d'accordo a ogni misura, tre giunture che
 *   sull'antialiasing si vedono come righe più chiare, e nessun modo di
 *   arrotondare l'angolo interno.
 * - **`clip-path` con `evenodd`**: il ritaglio funziona finché il buco è un
 *   rettangolo secco. Un rettangolo con gli angoli tondi in `polygon()` non si
 *   scrive, e `path()` vorrebbe dire comporre a mano una stringa SVG con otto
 *   archi ricalcolata a ogni `resize` — per ottenere quel che una `box-shadow`
 *   fa da sé.
 *
 * Il buco è `pointer-events: none` e non prende i clic: a prenderli è il velo
 * trasparente che gli sta sotto, che copre la finestra intera. Il velo li
 * **assorbe** e basta, non chiude: durante un giro ogni clic è quasi sempre un
 * tentativo di toccare la cosa illuminata, e sparire al primo tocco vorrebbe
 * dire perdere il giro proprio a chi lo sta seguendo. Per uscire ci sono
 * «Salta il giro» ed Escape, che sono due gesti dichiarati.
 *
 * # Un passo che non trova la sua ancora si salta
 *
 * Perché non tutto c'è sempre: la colonna si chiude, il testo esiste se il
 * brano ce l'ha, la coda è vuota finché non si mette qualcosa dentro. Il
 * riflettore chiede all'applicazione di mettersi dove il passo vive
 * (`onPrepara`), aspetta che il disegno arrivi — al più un secondo e mezzo,
 * `ATTESA_ANCORA_MS` in `guida.ts`, che dice anche perché a tempo e non a
 * fotogrammi — e se l'ancora non c'è **passa oltre** nella direzione in cui si
 * stava andando. In sviluppo scrive una riga in console; in rilascio no,
 * perché saltare un passo è un esito previsto e non un guasto.
 *
 * Mai un velo senza buco: finché una misura non c'è, il velo non si scurisce.
 *
 * # Ogni passo si mette e si toglie
 *
 * `onPrepara` può restituire chi lo disfa, e il giro lo chiama lasciando il
 * passo — avanti, indietro, o chiudendo. Serve al menù di un brano, che è un
 * menù vero aperto col tasto destro vero (`apriIlMenuDiUnaRiga`): lasciato
 * aperto, starebbe a schermo sotto il passo dopo a parlare d'altro. Il resto —
 * la vista, la colonna, la coda che il giro ha riempito — lo rimette `App` alla
 * chiusura, com'era prima di partire: un giro che lascia la finestra nelle
 * Impostazioni con un disco in coda che nessuno ha scelto non è finito, è
 * scappato.
 *
 * # Il fuoco resta nel fumetto
 *
 * Il fumetto è un dialogo modale e il velo prende i clic: l'unico posto in cui
 * il fuoco ha senso è lì dentro. Ma due passi mettono a schermo cose che il
 * fuoco se lo prendono da sole quando si aprono — lo schermo intero, che lo
 * porta su di sé, e il menù, che lo porta sulla prima voce — e da lì Invio non
 * mandava più avanti il giro: nello schermo intero non faceva niente, nel menù
 * faceva partire la prima voce. Il fuoco che esce dal fumetto mentre il giro è
 * aperto ci torna, sul pulsante che lo teneva.
 *
 * # La prosa, e le due copie lunghe
 *
 * I testi sono **corti apposta** e stanno in `lingue/{it,en}.json` sotto
 * `tour.step.*`. Le versioni lunghe delle stesse cose sono due e vivono
 * altrove: **`README.md` § Interface/Customization** e il
 * viaggio del sito, **`site/testi/<lingua>.json`** (le tappe da `playlist` a
 * `tuo`, in quattro lingue). L'allineamento si tiene **a mano**, rileggendo
 * questi testi quando una
 * funzione cambia: un generatore che le producesse tutte da una sorgente sola
 * costerebbe più di quel che risparmia, perché i tre testi hanno tre lunghezze
 * e tre mestieri — uno spiega a chi non ha ancora installato, uno a chi ha
 * appena installato, uno a chi guarda un sito.
 *
 * # Niente portale
 *
 * `createPortal` non compare in tutto l'albero, e non serve nemmeno qui: il
 * velo è `position: fixed`, quindi il suo rettangolo di riferimento è la
 * finestra e non il genitore, e `App` lo monta come ultimo figlio della propria
 * radice. L'unica cosa che un portale aggiungerebbe è l'immunità a un
 * `overflow: hidden` o a un `transform` di un antenato — e sopra questo nodo di
 * antenati ce n'è uno solo, che è il frammento di `App`.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { useDati } from "./dati";
import { useFinestrella } from "./finestrella";
import { ATTESA_ANCORA_MS, DISTACCO, posaFumetto } from "./guida";
import { t } from "./lingue";
import { Presenza } from "./presenza";

/**
 * I nomi delle ancore, cioè i valori ammessi di `data-giro`.
 *
 * Un tipo e non delle stringhe libere: chi mette l'attributo in un componente e
 * chi scrive il copione qui sono due punti lontani, e un nome sbagliato in uno
 * dei due darebbe un passo che si salta sempre — cioè, di nuovo, un'assenza
 * silenziosa.
 */
export type Ancora =
  | "navigazione"
  | "ricerca"
  | "riga-brano"
  | "menu"
  | "cartelle"
  | "trasporto"
  | "giudizio"
  | "colonna"
  | "coda"
  | "in-riproduzione"
  | "testo"
  | "playlist"
  | "studio"
  | "impostazioni"
  | "scorciatoie";

/**
 * I nomi dei passi, che sono tredici delle quindici ancore.
 *
 * `coda` e `testo` non ci sono perché non intitolano nessun passo: sono i
 * ripieghi di «colonna» e di «In riproduzione», e un passo che si illumina
 * sull'uno o sull'altro racconta comunque la stessa cosa con lo stesso testo.
 * Un tipo a parte e non `Ancora`: da qui escono le chiavi del catalogo, e con
 * l'unione larga il compilatore avrebbe preteso due coppie di chiavi che
 * nessuno legge.
 */
export type NomePasso = Exclude<Ancora, "coda" | "testo"> | "benvenuto";

/**
 * Come si trova ogni ancora nel documento.
 *
 * Tutte uguali tranne la ricerca, il menù, le Cartelle e le scorciatoie: vedi
 * il preambolo.
 */
const SELETTORE: Record<Ancora, string> = {
  navigazione: '[data-giro="navigazione"]',
  ricerca: "[data-cerca]",
  "riga-brano": '[data-giro="riga-brano"]',
  menu: '[role="menu"]',
  cartelle: ".albero-cartelle",
  trasporto: '[data-giro="trasporto"]',
  giudizio: '[data-giro="giudizio"]',
  colonna: '[data-giro="colonna"]',
  coda: '[data-giro="coda"]',
  "in-riproduzione": '[data-giro="in-riproduzione"]',
  testo: '[data-giro="testo"]',
  playlist: '[data-giro="playlist"]',
  studio: '[data-giro="studio"]',
  impostazioni: '[data-giro="impostazioni"]',
  scorciatoie: "section.scheda:has(.scorciatoia)",
};

/**
 * I tredici passi, ognuno con le ancore che gli vanno bene in ordine di scelta.
 *
 * Quindici ancore e tredici passi, perché due passi parlano di due cose che sono la
 * stessa cosa vista da due posti: la colonna di destra e il pannello della
 * coda, la schermata a tutto schermo e il testo che ci sta dentro. Si illumina
 * la prima che c'è, e il testo nomina tutte e due — la seconda è dove si
 * finisce quando la prima è chiusa, e dirlo è metà del passo.
 *
 * Il nome del passo è la sua prima ancora, e da lì escono le due chiavi del
 * catalogo (`tour.step.<nome>.title` e `.body`) e il nome che arriva a
 * `onPrepara`. Un nome in più sarebbe stato un terzo elenco da tenere allineato
 * con gli altri due.
 */
const COPIONE: readonly (
  | readonly ["benvenuto"]
  | readonly [Exclude<NomePasso, "benvenuto">, ...Ancora[]]
)[] = [
  // Il solo passo senza ancora, ed è voluto: i riflettori dicono **dove**
  // stanno le cose, e nessuno diceva **cosa** fa il programma. Chi apre Rymembra
  // per la prima volta veniva portato sulla barra laterale senza sapere che
  // cosa stesse guardando. Il fumetto sta al centro e il velo non si scurisce:
  // non c'è niente da illuminare, e un velo scuro senza buco è la cosa che il
  // resto di questo file evita.
  ["benvenuto"],
  ["navigazione"],
  ["ricerca"],
  ["riga-brano"],
  // Subito dopo la riga, perché è la stessa riga col tasto destro: il passo di
  // prima lo nomina, questo lo apre. Su un menù vero, non disegnato: vedi
  // `apriIlMenuDiUnaRiga`.
  ["menu"],
  // Accanto ai Brani perché è l'altro modo di guardare la stessa libreria: per
  // come sta sul disco invece che per titolo.
  ["cartelle"],
  ["trasporto"],
  ["giudizio"],
  ["colonna", "coda"],
  ["in-riproduzione", "testo"],
  ["playlist"],
  ["studio"],
  ["impostazioni"],
  // Ultimo, dopo le Impostazioni che lo nominano: il passo prima dice dove
  // sono, questo le mostra. E da lì il giro finisce dove si cambiano.
  ["scorciatoie"],
];

/** La misura di un riflettore, in coordinate di finestra. */
interface Posa {
  /**
   * Il rettangolo del bersaglio: `top`, `left`, `width`, `height` in pixel.
   * `null` per il benvenuto, che non illumina niente.
   */
  buco: { top: number; left: number; width: number; height: number } | null;
  /** Il `border-radius` calcolato del bersaglio, copiato com'è. */
  raggio: string;
  /** Dove si posa il fumetto. */
  fumetto: { top: number; left: number };
}

/**
 * La prima ancora **disegnata** fra quelle che vanno bene per il passo.
 *
 * Si guarda il rettangolo e non `offsetParent`, perché qui la domanda non è
 * «esiste» ma «si può illuminare»: un nodo con `hidden`, dentro un pannello
 * chiuso o largo zero non ha niente da mostrare, e un buco di misura nulla
 * sarebbe un velo senza buco con un passaggio in più.
 *
 * L'ordine è quello del documento, quindi fra due candidati uguali vince quello
 * più in alto nell'albero — che è anche quel che serve quando un'ancora sta su
 * un contenitore e un'altra su un suo figlio: si illumina il contenitore, che è
 * il pezzo di cui il testo parla.
 */
function trova(ancore: readonly Ancora[]): HTMLElement | null {
  for (const nome of ancore) {
    for (const nodo of document.querySelectorAll<HTMLElement>(SELETTORE[nome])) {
      const r = nodo.getBoundingClientRect();
      if (r.width > 0 && r.height > 0) return nodo;
    }
  }
  return null;
}

/**
 * Apre il menù di un brano come lo apre il tasto destro: sulla prima riga di
 * brano disegnata, con un `contextmenu` vero.
 *
 * Non `setMenu` con delle voci scritte per il giro: il menù che si vede deve
 * essere quello che si troverà premendo il tasto destro — le stesse voci, nello
 * stesso ordine, con le stesse regole («Mostra nella cartella» solo su un file,
 * la radio solo su un brano). Un evento sulla riga passa dal gestore della riga,
 * e quel gestore è l'unico posto in cui quelle regole sono scritte.
 *
 * La riga può non esserci ancora: la vista dei Brani è appena stata chiesta, e
 * le righe arrivano con la prima pagina. Si riprova a ogni fotogramma per il
 * tempo che il giro aspetta un'ancora, e poi si lascia stare — il passo si
 * salterà da sé. Restituisce chi smette di provare: il passo lasciato prima che
 * la riga arrivi non deve aprire un menù sotto quello dopo.
 */
export function apriIlMenuDiUnaRiga(): () => void {
  const inizio = performance.now();
  let cercando = 0;
  let premuta = false;
  const prova = () => {
    const riga = trova(["riga-brano"]);
    if (riga === null) {
      if (performance.now() - inizio < ATTESA_ANCORA_MS)
        cercando = window.requestAnimationFrame(prova);
      return;
    }
    if (!premuta) {
      // Prima in vista, poi il clic: il menù nasce dove si è premuto, e una
      // riga mezza fuori dallo schermo lo farebbe nascere contro il bordo.
      premuta = true;
      riga.scrollIntoView({ block: "center", inline: "nearest" });
      cercando = window.requestAnimationFrame(prova);
      return;
    }
    const r = riga.getBoundingClientRect();
    riga.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        // Sul titolo, non sul bordo: è dove la mano lo aprirebbe, e il menù si
        // tira dentro la finestra da sé se sborda.
        clientX: r.left + Math.min(r.width / 3, 160),
        clientY: r.top + r.height / 2,
      }),
    );
  };
  cercando = window.requestAnimationFrame(prova);
  return () => window.cancelAnimationFrame(cercando);
}

/**
 * Un dialogo modale o un menù è aperto adesso, e si vede.
 *
 * Dal documento e non dallo stato di `App`, perché i modali non stanno tutti
 * lì: l'equalizzatore, la sincronia, le conferme delle Cartelle, la scelta del
 * testo tengono ciascuno il proprio. Quel che hanno in comune è quel che
 * promettono a chi legge lo schermo — `aria-modal="true"`, o `role="menu"` —
 * ed è proprio la cosa che conta qui: c'è qualcosa che aspetta una risposta.
 * «Si vede» perché lo Studio in prova resta montato e nascosto, con dentro il
 * suo menù degli effetti: un nodo che non si disegna non aspetta niente.
 *
 * Serve a `comeParteIlGiro` (`guida.ts`), che sopra un modale aspetta.
 */
export function unModaleAperto(): boolean {
  for (const nodo of document.querySelectorAll<HTMLElement>(
    '[aria-modal="true"], [role="menu"]',
  )) {
    if (nodo.getClientRects().length > 0) return true;
  }
  return false;
}

/** Quel che il giro chiede alla finestra di fare per lui. */
export interface GiroProps {
  /**
   * Mette l'applicazione dove il passo vive.
   *
   * Chiamata **prima** di cercare l'ancora, una volta per passo. Riceve il nome
   * del passo, cioè la sua prima ancora. Chi la implementa non deve garantire
   * niente: se dopo il cambio l'ancora non c'è — non c'è un brano in
   * riproduzione, la libreria è vuota — il passo si salta, ed è previsto.
   *
   * Può restituire chi disfa quel che ha fatto, e il giro lo chiama lasciando il
   * passo: vedi «Ogni passo si mette e si toglie» nel preambolo.
   */
  onPrepara: (passo: NomePasso) => (() => void) | void;
  /**
   * Il giro è finito, saltato o chiuso con Escape.
   *
   * Sono lo stesso esito di proposito: vedi il `//!` di
   * `apps/desktop/src-tauri/src/giro.rs`.
   */
  onChiudi: () => void;
}

/** Il giro guidato. */
export function Giro({ onPrepara, onChiudi }: GiroProps) {
  const schermo = useDati().misura;
  const [indice, setIndice] = useState(0);
  const [posa, setPosa] = useState<Posa | null>(null);
  /**
   * In che verso si stava andando.
   *
   * Serve a saltare: un passo senza ancora incontrato tornando indietro deve
   * far tornare indietro ancora, non rimbalzare avanti — che sarebbe un
   * «Indietro» che non torna da nessuna parte.
   */
  const verso = useRef(1);
  /**
   * I passi saltati perché il loro bersaglio non c'era.
   *
   * Il contatore li toglie dal conto: prima diceva «4 di 14» e poi «6 di 14»,
   * e chi legge cercava il quinto che non aveva visto. Si sanno solo quando ci
   * si passa, quindi il totale può scendere andando avanti — «5 di 13» —, che
   * è il vero: tredici sono quelli che si vedranno.
   */
  const [saltati, setSaltati] = useState<ReadonlySet<number>>(() => new Set());

  // Le due chiusure arrivano scritte sul posto da `App`, che si ridisegna venti
  // volte al secondo mentre suona: tenerle fra le dipendenze dell'effetto
  // vorrebbe dire rimisurare il riflettore a ogni fotogramma. È la stessa
  // ragione, e la stessa forma, di `useFinestrella`.
  const prepara = useRef(onPrepara);
  prepara.current = onPrepara;
  const chiudi = useRef(onChiudi);
  chiudi.current = onChiudi;

  const radice = useFinestrella<HTMLDivElement>(onChiudi);

  /*
   * Il fuoco che esce dal fumetto ci torna: vedi «Il fuoco resta nel fumetto»
   * nel preambolo.
   *
   * Sul documento e in `focusin`, che risale — `focus` no — e arriva **dopo**
   * che il fuoco si è spostato: lo si riprende invece di impedirlo, perché chi
   * lo prende (lo schermo intero, il menù) lo fa in un suo effetto, e non c'è un
   * modo pulito di dirgli di non farlo. Il pulsante è quello marcato
   * `data-fuoco-iniziale`, «Avanti»: è dove il fuoco stava, ed è quello che
   * Invio deve premere.
   *
   * Un fumetto staccato dal documento è un giro che si sta smontando: lì il
   * fuoco va dove `useFinestrella` lo rimette, e tirarlo indietro vorrebbe dire
   * metterlo su un nodo che sta sparendo.
   */
  useEffect(() => {
    const torna = (e: FocusEvent) => {
      const dentro = radice.current;
      if (dentro === null || !dentro.isConnected) return;
      if (e.target instanceof Node && dentro.contains(e.target)) return;
      const tasto =
        dentro.querySelector<HTMLElement>("[data-fuoco-iniziale]") ?? dentro;
      tasto.focus({ preventScroll: true });
    };
    document.addEventListener("focusin", torna);
    return () => document.removeEventListener("focusin", torna);
  }, [radice]);

  const vai = useCallback((passo: number) => {
    verso.current = passo;
    // La misura di prima non vale più, e lasciarla vorrebbe dire un fotogramma
    // col buco sul bersaglio vecchio e il testo del passo nuovo.
    setPosa(null);
    setIndice((prima) => prima + passo);
  }, []);

  const passo = COPIONE[indice];

  useEffect(() => {
    // Fuori dal copione: si è arrivati in fondo, o si è tornati indietro dal
    // primo. In tutti e due i casi il giro è finito.
    if (passo === undefined) {
      chiudi.current();
      return;
    }

    let vivo = true;
    let bersaglio: HTMLElement | null = null;
    let cercando = 0;
    let inCoda = 0;

    // Chi disfa il passo, se il passo ha qualcosa da disfare: vedi «Ogni passo
    // si mette e si toglie» nel preambolo. Si chiama in tutte e due le uscite
    // di questo effetto, quella del benvenuto compresa.
    const sgombra = prepara.current(passo[0]);
    const disfa = () => {
      if (typeof sgombra === "function") sgombra();
    };

    // Il benvenuto: niente da cercare, il fumetto al centro della finestra.
    if (passo[0] === "benvenuto") {
      const alCentro = () => {
        if (!vivo) return;
        const suo = radice.current?.getBoundingClientRect();
        const { larghezza, altezza } = schermo();
        setPosa({
          buco: null,
          raggio: "0",
          fumetto: {
            top: Math.max(DISTACCO, (altezza - (suo?.height ?? 0)) / 2),
            left: Math.max(DISTACCO, (larghezza - (suo?.width ?? 0)) / 2),
          },
        });
      };
      cercando = window.requestAnimationFrame(alCentro);
      window.addEventListener("resize", alCentro);
      return () => {
        vivo = false;
        window.cancelAnimationFrame(cercando);
        window.removeEventListener("resize", alCentro);
        disfa();
      };
    }
    const ancore = passo;
    const inizio = performance.now();

    const misura = () => {
      if (!vivo || bersaglio === null) return;
      const r = bersaglio.getBoundingClientRect();
      // Sparito mentre lo si guardava — la colonna chiusa a mano, la finestra
      // rimpicciolita fino a ritirare la barra. Si tiene l'ultima misura buona
      // invece di scurire tutto: il passo dopo rimetterà le cose a posto.
      if (r.width === 0 || r.height === 0) return;
      const buco = { top: r.top, left: r.left, width: r.width, height: r.height };
      const suo = radice.current?.getBoundingClientRect();
      setPosa({
        buco,
        raggio: window.getComputedStyle(bersaglio).borderRadius,
        fumetto: posaFumetto(
          buco,
          { width: suo?.width ?? 0, height: suo?.height ?? 0 },
          schermo(),
        ),
      });
    };

    const cerca = () => {
      if (!vivo) return;
      bersaglio = trova(ancore);
      if (bersaglio === null) {
        // A tempo e non a fotogrammi: vedi `ATTESA_ANCORA_MS`.
        if (performance.now() - inizio < ATTESA_ANCORA_MS) {
          cercando = window.requestAnimationFrame(cerca);
          return;
        }
        if (import.meta.env.DEV)
          console.warn(
            `[giro] nessuna ancora per «${passo[0]}»: il passo si salta`,
          );
        setSaltati((prima) => (prima.has(indice) ? prima : new Set(prima).add(indice)));
        vai(verso.current);
        return;
      }
      // Prima di misurare: un bersaglio fuori schermo darebbe un buco fuori
      // schermo, cioè un velo senza buco con la misura giusta.
      bersaglio.scrollIntoView({ block: "center", inline: "nearest" });
      // Un bersaglio che entra con una transizione — il menù cresce dal
      // puntatore, la colonna si allarga — misurato a metà darebbe un buco più
      // piccolo della cosa, e nessuno dei tre ascolti qui sotto lo vedrebbe
      // crescere: non scorre niente, e la finestra non cambia misura. La fine
      // della transizione è il momento giusto per rimisurare, e risale dai
      // figli: basta ascoltarla sul bersaglio.
      bersaglio.addEventListener("transitionend", strozza);
      bersaglio.addEventListener("animationend", strozza);
      // Nel fotogramma dopo, perché `scrollIntoView` sposta e
      // `getBoundingClientRect` legge: nello stesso fotogramma si leggerebbe il
      // rettangolo di prima dello scorrimento.
      cercando = window.requestAnimationFrame(misura);
    };
    cercando = window.requestAnimationFrame(cerca);

    /*
     * Tutto quel che rimisura passa da qui, e non chiama `misura` due volte per
     * fotogramma: uno scorrimento con la rotella manda decine di eventi al
     * secondo, e ognuno costerebbe una lettura di layout — cioè il modo di
     * trasformare un velo in un rallentamento.
     */
    const strozza = () => {
      if (inCoda !== 0) return;
      inCoda = window.requestAnimationFrame(() => {
        inCoda = 0;
        misura();
      });
    };

    // `capture` perché quel che scorre non è la finestra ma un contenitore
    // dentro di essa — l'elenco dei brani, l'indice delle impostazioni — e
    // `scroll` non risale. `passive` perché qui non si annulla niente e dirlo
    // lascia il browser scorrere senza aspettare questo gestore.
    window.addEventListener("resize", strozza);
    window.addEventListener("scroll", strozza, { capture: true, passive: true });
    // La radice e non il bersaglio: quel che sposta un riflettore è quasi
    // sempre qualcosa che cambia **attorno** — una barra che si stringe, un
    // pannello che si apre — e osservare il solo bersaglio non lo vedrebbe.
    const osserva = new ResizeObserver(strozza);
    osserva.observe(document.documentElement);

    return () => {
      vivo = false;
      window.cancelAnimationFrame(cercando);
      if (inCoda !== 0) window.cancelAnimationFrame(inCoda);
      window.removeEventListener("resize", strozza);
      window.removeEventListener("scroll", strozza, { capture: true });
      osserva.disconnect();
      if (bersaglio !== null) {
        bersaglio.removeEventListener("transitionend", strozza);
        bersaglio.removeEventListener("animationend", strozza);
      }
      disfa();
    };
    // `radice` è il riferimento di `useFinestrella`, cioè un oggetto stabile:
    // sta fra le dipendenze perché il controllo delle dipendenze non sa che lo
    // è, e non perché possa cambiare.
  }, [passo, indice, vai, radice, schermo]);

  if (passo === undefined) return null;

  const primo = indice === 0;
  const ultimo = indice === COPIONE.length - 1;

  return (
    <>
      {/* Il velo prende i clic e non chiude: vedi il preambolo. `aria-hidden`
          perché non ha niente da dire — quel che c'è da leggere sta nel
          fumetto, che è il dialogo. */}
      <Presenza parte="tour-veil">
        <div className="giro-velo tour-veil" aria-hidden="true">
          {posa !== null && posa.buco !== null && (
            <div
              className="giro-buco"
              style={{
                top: `${posa.buco.top}px`,
                left: `${posa.buco.left}px`,
                width: `${posa.buco.width}px`,
                height: `${posa.buco.height}px`,
                borderRadius: posa.raggio,
              }}
            />
          )}
        </div>
      </Presenza>
      <Presenza parte="tour-tooltip">
        <div
          ref={radice}
          className="giro-fumetto tour-tooltip"
          role="dialog"
          aria-modal="true"
          aria-labelledby="giro-titolo"
          /* Il fumetto resta lo stesso nodo per tutti i passi — è quel
             che tiene il fuoco sul pulsante «Avanti», così il giro si fa a
             Invio — e quindi il cambio di passo è un cambio di testo dentro un
             dialogo già aperto, che nessun lettore di schermo annuncerebbe. Il
             `polite` sulla radice lo fa annunciare: i tre pulsanti non cambiano
             mai, quindi non si ripetono. */
          aria-live="polite"
          style={
            posa === null
              ? undefined
              : { top: `${posa.fumetto.top}px`, left: `${posa.fumetto.left}px` }
          }
        >
          <div className="giro-testa">
            <h2 id="giro-titolo">{t(`tour.step.${passo[0]}.title`)}</h2>
            <span className="giro-passi">
              {t("tour.progress", {
                n: indice + 1 - [...saltati].filter((saltato) => saltato < indice).length,
                totale: COPIONE.length - saltati.size,
              })}
            </span>
          </div>
          <p>{t(`tour.step.${passo[0]}.body`)}</p>
          <div className="giro-tasti">
            {/* Primo nell'ordine del tabulatore e ultimo per importanza: chi
                vuole uscire lo trova subito, chi vuole andare avanti ha già il
                fuoco sul pulsante giusto e non lo sfiora mai. */}
            <button
              type="button"
              className="bottone minuto btn-ghost"
              onClick={onChiudi}
            >
              {t("tour.skip")}
            </button>
            <button
              type="button"
              className="bottone"
              disabled={primo}
              onClick={() => vai(-1)}
            >
              {t("tour.back")}
            </button>
            <button
              type="button"
              className="bottone primario btn-accent"
              data-fuoco-iniziale
              onClick={() => vai(1)}
            >
              {ultimo ? t("tour.done") : t("tour.next")}
            </button>
          </div>
        </div>
      </Presenza>
    </>
  );
}
