/**
 * Il giro guidato: dieci riflettori sui comandi veri.
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
 * (`onPrepara`), aspetta qualche fotogramma che il disegno arrivi, e se dopo
 * quelli l'ancora non c'è **passa oltre** nella direzione in cui si stava
 * andando. In sviluppo scrive una riga in console; in rilascio no, perché
 * saltare un passo è un esito previsto e non un guasto.
 *
 * Mai un velo senza buco: finché una misura non c'è, il velo non si scurisce.
 *
 * # La prosa, e le due copie lunghe
 *
 * I dieci testi sono **corti apposta** e stanno in `lingue/{it,en}.json` sotto
 * `tour.step.*`. Le versioni lunghe delle stesse cose sono due e vivono
 * altrove: **`README.md` § Interface/Customization (righe 160-226)** e
 * **`site/index.html` (righe 332-420)**. Sono tre copie in due lingue, e
 * l'allineamento si tiene **a mano**, rileggendo queste tre righe quando una
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

import { useFinestrella } from "./finestrella";
import { t } from "./lingue";

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
  | "trasporto"
  | "giudizio"
  | "colonna"
  | "coda"
  | "in-riproduzione"
  | "testo"
  | "playlist"
  | "studio"
  | "impostazioni";

/**
 * I nomi dei passi, che sono dieci delle dodici ancore.
 *
 * `coda` e `testo` non ci sono perché non intitolano nessun passo: sono i
 * ripieghi di «colonna» e di «In riproduzione», e un passo che si illumina
 * sull'uno o sull'altro racconta comunque la stessa cosa con lo stesso testo.
 * Un tipo a parte e non `Ancora`: da qui escono le chiavi del catalogo, e con
 * l'unione larga il compilatore avrebbe preteso due coppie di chiavi che
 * nessuno legge.
 */
export type NomePasso = Exclude<Ancora, "coda" | "testo">;

/**
 * Come si trova ogni ancora nel documento.
 *
 * Tutte uguali tranne la ricerca: vedi il preambolo.
 */
const SELETTORE: Record<Ancora, string> = {
  navigazione: '[data-giro="navigazione"]',
  ricerca: "[data-cerca]",
  "riga-brano": '[data-giro="riga-brano"]',
  trasporto: '[data-giro="trasporto"]',
  giudizio: '[data-giro="giudizio"]',
  colonna: '[data-giro="colonna"]',
  coda: '[data-giro="coda"]',
  "in-riproduzione": '[data-giro="in-riproduzione"]',
  testo: '[data-giro="testo"]',
  playlist: '[data-giro="playlist"]',
  studio: '[data-giro="studio"]',
  impostazioni: '[data-giro="impostazioni"]',
};

/**
 * I dieci passi, ognuno con le ancore che gli vanno bene in ordine di scelta.
 *
 * Dodici ancore e dieci passi, perché due passi parlano di due cose che sono la
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
const COPIONE: readonly (readonly [NomePasso, ...Ancora[]])[] = [
  ["navigazione"],
  ["ricerca"],
  ["riga-brano"],
  ["trasporto"],
  ["giudizio"],
  ["colonna", "coda"],
  ["in-riproduzione", "testo"],
  ["playlist"],
  ["studio"],
  ["impostazioni"],
];

/**
 * Quanti fotogrammi si aspetta un'ancora prima di rinunciare al passo.
 *
 * Trenta, cioè mezzo secondo scarso a sessanta hertz. `onPrepara` cambia dello
 * stato di React — una vista, una colonna che si apre — e fra quella chiamata e
 * il nodo disegnato ci sono un ridisegno, a volte una transizione di pagina, e
 * per la colonna di destra anche una richiesta al nucleo. Aspettare un
 * fotogramma solo avrebbe saltato quasi tutti i passi che il giro esiste per
 * mostrare; aspettarne trecento avrebbe lasciato il giro fermo su un velo
 * scuro, che è il modo di farlo sembrare rotto.
 */
const FOTOGRAMMI_DI_ATTESA = 30;

/** Quanto sta il fumetto lontano dal bordo del buco, e dal bordo della finestra. */
const DISTACCO = 12;

/** La misura di un riflettore, in coordinate di finestra. */
interface Posa {
  /** Il rettangolo del bersaglio: `top`, `left`, `width`, `height` in pixel. */
  buco: { top: number; left: number; width: number; height: number };
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
 * Dove si posa il fumetto rispetto al buco.
 *
 * Sotto se ci sta, sopra altrimenti, e in tutti e due i casi centrato sul buco
 * e poi rientrato dentro la finestra. Non si prova a stare *accanto*: con un
 * bersaglio largo quanto una barra laterale non ci sarebbe spazio, e la regola
 * «sotto, se no sopra» dà una posizione prevedibile — chi ha letto il passo
 * prima sa già dove guardare per il prossimo.
 */
function posaFumetto(
  buco: { top: number; left: number; width: number; height: number },
  misura: { width: number; height: number },
): { top: number; left: number } {
  const stretta = (valore: number, minimo: number, massimo: number) =>
    Math.max(minimo, Math.min(valore, Math.max(minimo, massimo)));

  const sotto = buco.top + buco.height + DISTACCO;
  const sopra = buco.top - DISTACCO - misura.height;
  const top =
    sotto + misura.height + DISTACCO <= window.innerHeight
      ? sotto
      : sopra >= DISTACCO
        ? sopra
        : // Non ci sta né sotto né sopra: il bersaglio è alto quasi quanto la
          // finestra. Si appoggia in alto e si accetta la sovrapposizione, che
          // è meno peggio di un fumetto mezzo fuori dallo schermo.
          DISTACCO;

  const left = buco.left + buco.width / 2 - misura.width / 2;
  return {
    top: stretta(top, DISTACCO, window.innerHeight - misura.height - DISTACCO),
    left: stretta(left, DISTACCO, window.innerWidth - misura.width - DISTACCO),
  };
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
   */
  onPrepara: (passo: NomePasso) => void;
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

  // Le due chiusure arrivano scritte sul posto da `App`, che si ridisegna venti
  // volte al secondo mentre suona: tenerle fra le dipendenze dell'effetto
  // vorrebbe dire rimisurare il riflettore a ogni fotogramma. È la stessa
  // ragione, e la stessa forma, di `useFinestrella`.
  const prepara = useRef(onPrepara);
  prepara.current = onPrepara;
  const chiudi = useRef(onChiudi);
  chiudi.current = onChiudi;

  const radice = useFinestrella<HTMLDivElement>(onChiudi);

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
    let tentativi = 0;
    let bersaglio: HTMLElement | null = null;
    let cercando = 0;
    let inCoda = 0;

    prepara.current(passo[0]);

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
        fumetto: posaFumetto(buco, {
          width: suo?.width ?? 0,
          height: suo?.height ?? 0,
        }),
      });
    };

    const cerca = () => {
      if (!vivo) return;
      bersaglio = trova(passo);
      if (bersaglio === null) {
        tentativi += 1;
        if (tentativi <= FOTOGRAMMI_DI_ATTESA) {
          cercando = window.requestAnimationFrame(cerca);
          return;
        }
        if (import.meta.env.DEV)
          console.warn(
            `[giro] nessuna ancora per «${passo[0]}»: il passo si salta`,
          );
        vai(verso.current);
        return;
      }
      // Prima di misurare: un bersaglio fuori schermo darebbe un buco fuori
      // schermo, cioè un velo senza buco con la misura giusta.
      bersaglio.scrollIntoView({ block: "center", inline: "nearest" });
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
    };
    // `radice` è il riferimento di `useFinestrella`, cioè un oggetto stabile:
    // sta fra le dipendenze perché il controllo delle dipendenze non sa che lo
    // è, e non perché possa cambiare.
  }, [passo, vai, radice]);

  if (passo === undefined) return null;

  const primo = indice === 0;
  const ultimo = indice === COPIONE.length - 1;

  return (
    <>
      {/* Il velo prende i clic e non chiude: vedi il preambolo. `aria-hidden`
          perché non ha niente da dire — quel che c'è da leggere sta nel
          fumetto, che è il dialogo. */}
      <div className="giro-velo" aria-hidden="true">
        {posa !== null && (
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
      <div
        ref={radice}
        className="giro-fumetto tour-tooltip"
        role="dialog"
        aria-modal="true"
        aria-labelledby="giro-titolo"
        /* Il fumetto resta lo stesso nodo per tutti e dieci i passi — è quel
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
            {t("tour.progress", { n: indice + 1, totale: COPIONE.length })}
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
    </>
  );
}
