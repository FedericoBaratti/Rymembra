/**
 * Il pannello «Cartelle»: la libreria come sta sul disco, come in foobar2000.
 *
 * A sinistra l'albero — cartelle e, aprendole, i loro file — e accanto, o sotto
 * quando la finestra è stretta, i brani della cartella su cui sta il cursore,
 * nell'elenco dei brani di sempre. È il pannello «Album List / Folders» di
 * foobar2000: si sceglie dove, e quel che c'è dentro è subito lì da suonare.
 *
 * # Perché non c'è nessuna attesa, mai
 *
 * L'albero viene dal **database** (`core/rymembra-app/src/cartelle.rs`), non dal
 * filesystem: aprire un nodo è un accesso a una mappa in RAM più, se ha dei
 * file, la lettura delle sue dieci righe in `tracks`, e nessuna espansione
 * tocca il disco o la rete. È la ragione per cui questa schermata si apre col
 * Wi-Fi staccato mentre una share è fra le cartelle sorvegliate — cosa che,
 * camminando sul disco, sarebbe stata la finestra ferma per i quaranta secondi
 * di timeout di Windows a ogni clic.
 *
 * L'unica cosa che tocca davvero la rete è la **sonda delle radici**, e sta
 * apposta in un comando a parte che nessuno aspetta: l'albero si disegna
 * subito, la sonda arriva dopo e si limita a spegnere le radici che non
 * rispondono. Costa fino a otto secondi sul caso cattivo — è la scadenza che
 * `files::radice_raggiungibile` ha già dentro — e per questo si rinfresca al
 * massimo una volta ogni [`FRESCHEZZA_MS`], non a ogni espansione.
 *
 * # Perché **niente** `role="group"`
 *
 * Questa è la scelta che qualcuno vorrà «correggere», quindi va scritta.
 *
 * L'albero è finestrato (`virtuale.ts`): a schermo ci sono una trentina di
 * righe, e un nodo che sta sopra o sotto la finestra **non esiste nel DOM**. Un
 * `role="group"` è un contenitore, e un contenitore vuole avere dentro i suoi
 * figli: con la finestra, i suoi figli possono essere fuori, o può esserlo lui
 * mentre i figli ci sono. Dichiararlo vorrebbe dire dichiarare una gerarchia
 * che nel documento non c'è, e la prima cosa che si romperebbe è proprio quel
 * che un gruppo serve a dire.
 *
 * La forma prevista per questo caso è l'**albero appiattito**: righe tutte
 * sorelle nel DOM, e la struttura detta dagli attributi — `aria-level`,
 * `aria-posinset`, `aria-setsize`. Non è un ripiego: è la variante descritta
 * proprio per gli alberi resi a elenco, ed è quel che i lettori di schermo
 * leggono per intero («livello 3, 4 di 12») senza dover attraversare niente. È
 * la stessa ragione per cui l'elenco dei brani è una `grid` con `aria-rowindex`
 * invece di una `listbox`: con una finestra, la verità la dicono i numeri, non
 * l'annidamento. I file sono `treeitem` come le cartelle, allo stesso livello
 * delle sorelle e contati con loro: si sentono «livello 3, 7 di 9», e senza
 * `aria-expanded` — che è il modo in cui un albero dice «foglia».
 *
 * # Perché il triangolo non è un bottone
 *
 * Perché sarebbe una seconda fermata di tabulazione per riga in un componente
 * che ne ha **una sola in tutto** (`fuoco.ts`), e perché non serve: lo stato
 * aperto/chiuso lo dice già `aria-expanded` sulla riga, e da tastiera lo si
 * cambia con ←→, che è la strada che chi naviga un albero cerca per prima. Un
 * bottone lì dentro ripeterebbe quel che la riga ha già annunciato, e lo farebbe
 * su ogni riga.
 *
 * # La selezione, e perché adesso c'è `aria-selected`
 *
 * Prima l'albero non aveva una selezione: aveva un cursore, e un
 * `aria-selected` sul cursore lo faceva leggere come «selezionato», che era
 * un'altra notizia. Adesso la selezione c'è, multipla e coi tasti dell'elenco
 * dei brani — Ctrl per aggiungere e togliere, Maiusc per l'intervallo, Spazio
 * dalla tastiera — quindi l'albero dichiara `aria-multiselectable` e **ogni**
 * riga dice `aria-selected`, vero o falso: in un albero a scelta multipla un
 * nodo senza l'attributo è un nodo che non si può scegliere. Il cursore resta
 * `data-fuoco`, e le due cose restano due.
 *
 * Spazio sceglie, e non apre più: ←→ aprono e chiudono, e uno Spazio che
 * facesse la stessa cosa lascerebbe la selezione senza una strada da tastiera.
 * È la stessa coppia dell'elenco dei brani, dove Spazio è il Ctrl+clic.
 *
 * # Trascinare in coda
 *
 * Col puntatore e non col trascinamento HTML5, per la ragione scritta in
 * `riordino.ts`: su Windows WebView2 tiene per sé il rilascio. Il gesto è lo
 * stesso — soglia di cinque pixel, Esc che annulla, il clic dopo il rilascio
 * inghiottito — e il bersaglio è la coda, nella terza colonna o nel pannello
 * flottante, cercata sotto il puntatore per classe. Nessun componente della
 * coda sa di questo gesto: lo si vede dall'attributo `data-verso-coda` sulla
 * radice del documento, che il foglio usa per segnarla.
 *
 * # Il conto dei nodi DOM
 *
 * Una cartella con 3.000 file espansa costa quanto una con trenta: si disegnano
 * le righe della finestra e due `.paglia` alte quanto quelle che mancano. Si
 * conta a mano con `document.querySelectorAll('.nodo-cartella').length` a
 * pannello tutto aperto e a fondo scorrimento: deve restare sulla trentina
 * qualunque sia il numero delle righe. Il pannello dei brani è una lista rada
 * (`rada.ts`): le righe si chiedono a pagine, dove si guarda.
 */
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent as EventoMouse,
  type PointerEvent as EventoPuntatore,
  type RefObject,
} from "react";

import {
  CIME,
  NESSUNA,
  VUOTO,
  apriTutti,
  appiattisci,
  attraversoLente,
  bersagli,
  chiave,
  chiaveRiga,
  chiudi,
  codaDaFile,
  conFigli,
  eAperto,
  figliDi,
  fratelliDi,
  indicePadre,
  lente as lenteDi,
  saltaA,
  seleziona,
  type GestoSelezione,
  type Lente,
  type RigaAlbero,
  type Selezione,
  type StatoAlbero,
} from "../albero";
import { useDati } from "../dati";
import { Dialogo } from "../Dialogo";
import { brani_, durata } from "../formato";
import { useFuoco } from "../fuoco";
import { ipc, type Brano, type NodoCartella } from "../ipc";
import { t, useLingua } from "../lingue";
import type { Apertura, Voce } from "../Menu";
import { useAscolto, useRada } from "../pagine";
import { Icona } from "../parti/Icone";
import { conIndice } from "../segnali";
import { useVirtuale } from "../virtuale";
import { ElencoBrani, ElencoFinto } from "./Elenco";
import { StatoVuoto } from "./Libreria";

/**
 * Per quanto si crede alla sonda delle radici prima di rifarla.
 *
 * Un minuto. La sonda costa fino a otto secondi quando una share è spenta, e
 * rifarla a ogni espansione vorrebbe dire otto secondi di lavoro in disparte
 * per un'informazione che, nel minuto in cui la si guarda, non è cambiata. Un
 * minuto è anche il tempo oltre il quale vale la pena riprovare da soli: una
 * rete che torna lo fa in secondi, non in ore.
 */
const FRESCHEZZA_MS = 60_000;

/**
 * Per quanto la digitazione ricorda quel che si è scritto prima.
 *
 * Ottocento millisecondi è la memoria di ogni elenco di Windows: sotto,
 * scrivere «rock» salta a quattro cartelle diverse; sopra, una lettera premuta
 * per sbaglio resta appesa e la successiva cerca una parola che nessuno ha
 * voluto scrivere.
 */
const MEMORIA_MS = 800;

/**
 * Oltre questi brani si chiede conferma prima di toccare la coda.
 *
 * Cinquemila. Su una cartella di primo livello «Riproduci» vuol dire sostituire
 * quel che si stava ascoltando con la libreria intera, e «Accoda» allungare la
 * coda di diciottomila righe. Il numero si sa **prima** di chiedere gli
 * identificativi: è già nei nodi, e arriva ricorsivo dal nucleo.
 */
const SOGLIA_CONFERMA = 5_000;

/** Quanto si aspetta prima di scrivere in `settings` com'è rimasto il pannello. */
const RITARDO_RICORDO_MS = 500;

/**
 * Quanto si aspetta, dopo l'ultima lettera, prima di chiedere il filtro.
 *
 * Il filtro costa una lettura di `tracks` — i nomi dei file non stanno nel
 * trie — e chi scrive «beatles» non vuole sette letture, ne vuole una.
 * Duecento millisecondi stanno sotto la soglia in cui una risposta sembra in
 * ritardo, e sopra l'intervallo fra due tasti di chi scrive di seguito.
 */
const RITARDO_FILTRO_MS = 200;

/**
 * Quanto deve restare fermo il cursore su una cartella perché il pannello dei
 * brani la segua.
 *
 * Tenendo premuta ↓ il cursore attraversa venti cartelle al secondo, e il
 * pannello non deve leggerle tutte: segue quella su cui ci si ferma.
 */
const RITARDO_PANNELLO_MS = 150;

/** Quanto deve muoversi il puntatore perché una pressione diventi una presa. */
const SOGLIA_PRESA_PX = 5;

/** Dove si può lasciare una presa: la coda della terza colonna, o il pannello. */
const BERSAGLIO_CODA = ".coda-in-colonna, .pannello-coda";

/** Il selettore di una riga: lo vogliono `virtuale.ts` e `fuoco.ts`. */
const RIGA = ".nodo-cartella";

/** Nessun brano: un'identità sola, così le dipendenze non cambiano per niente. */
const NESSUN_ID: readonly number[] = [];

/**
 * Cosa fa alla coda un gesto del pannello: la sostituisce, si mette in fondo,
 * o va subito dopo il brano in ascolto.
 */
type GestoCoda = "sostituisce" | "accoda" | "dopo";

/** Cosa si sta per fare, in attesa che qualcuno dica di sì. */
interface DaConfermare {
  brani: number;
  /** Quel che succede alla coda: è la frase che la conferma deve dire. */
  gesto: GestoCoda;
  /** Su quante righe: una cartella, o più elementi scelti. */
  righe: number;
  vai: () => void;
}

/**
 * Quel che il pannello dei brani prende da `App`: le stesse mani dell'elenco
 * della libreria.
 *
 * Il pannello riusa `ElencoBrani` così com'è, e un elenco di brani di questa
 * finestra deve comportarsi come gli altri: lo stesso menù contestuale, gli
 * stessi cuori e le stesse stelle, la stessa riga accesa sul brano che suona.
 * Quelle cose le tiene `App` per tutti, e qui arrivano in un oggetto solo.
 * Facoltativo, perché lo Studio disegna questa pagina con dati finti e senza
 * nessuna di queste mani: lì il pannello si vede, e i gesti non arrivano.
 */
export interface ManiDeiBrani {
  /** Il brano che suona, per la riga accesa. */
  inAscolto: number | null;
  inPausa: boolean;
  /** C'è un dispositivo audio: senza, il tasto della riga è spento. */
  suonabile: boolean;
  onPreferito: (b: Brano) => void;
  onVoto: (b: Brano, stelle: number) => void;
  /** Il menù contestuale dei brani: lo stesso dell'elenco della libreria. */
  onMenu: (e: EventoMouse, brani: number[]) => void;
  /** «Aggiungi a playlist…»: la finestrella che tiene `App`. */
  onAggiungiAPlaylist: (brani: number[]) => void;
}

/** La selezione dentro il pannello dei brani: per brano, con l'àncora. */
interface SceltaBrani {
  ids: ReadonlySet<number>;
  ancora: number | null;
}

const NESSUN_BRANO: SceltaBrani = { ids: new Set(), ancora: null };

/** Le righe dell'albero finto: piene e corte alternate, come i nomi veri. */
const RIGHE_FINTE: readonly boolean[] = [false, true, false, false, true, false];

/**
 * L'albero mentre le cartelle sorvegliate non si sanno ancora.
 *
 * Prima di `avvio` le radici erano un elenco vuoto, e la pagina diceva
 * «nessuna cartella sorvegliata» a chi ne ha cinque — per il tempo della prima
 * risposta, o per sempre se quella risposta non arrivava. Qui c'è la colonna
 * dell'albero con le sue righe in attesa, che è la forma di quel che arriverà:
 * la stessa ragione di `ElencoFinto` e `GrigliaFinta`.
 *
 * Le classi delle righe sono quelle vere, e lo scheletro è `skeleton`: una skin
 * che veste l'albero veste anche la sua attesa. `role="status"` sul contenitore
 * e `aria-hidden` sulle righe: chi ascolta lo schermo sente «sto caricando» una
 * volta, non sei rettangoli.
 */
export function CartelleFinte() {
  return (
    <div className="vista-cartelle">
      <div className="colonna-albero">
        <div
          className="albero-cartelle folder-tree"
          role="status"
          aria-label={t("list.loading")}
        >
          {RIGHE_FINTE.map((corta, i) => (
            <div
              key={i}
              className="nodo-cartella list-row folder-row"
              style={{ "--livello-nodo": 1 } as CSSProperties}
              aria-hidden="true"
            >
              <span className="icona-nodo">
                <Icona nome="i-folder" dim={14} />
              </span>
              <div className="nome-nodo">
                <div className={corta ? "riga-finta corta skeleton" : "riga-finta skeleton"} />
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export function Cartelle({
  radici,
  onMenu,
  onErrore,
  mani,
}: {
  /** Le cartelle sorvegliate, come le ha date l'utente. */
  radici: string[];
  /**
   * Non serve più: l'albero e il pannello dei brani scorrono ciascuno per conto
   * suo, come i due riquadri di foobar2000. Resta nel tipo perché lo Studio
   * (`studio/Tela.tsx`) lo passa ancora.
   */
  scorrevole?: RefObject<HTMLDivElement | null> | undefined;
  /** Apre il menù contestuale: lo disegna `App`, che lo tiene per tutti. */
  onMenu: (apertura: Apertura) => void;
  onErrore: (e: unknown) => void;
  /** Vedi [`ManiDeiBrani`]. */
  mani?: ManiDeiBrani | undefined;
}) {
  // Le domande che il pannello fa da sé: al nucleo, o ai dati finti dello Studio.
  const { ipc: letture, anteprima } = useDati();
  /** Lo scorrevole dell'albero, e l'albero dentro di lui. */
  const scorreAlbero = useRef<HTMLDivElement>(null);
  const albero = useRef<HTMLDivElement>(null);
  /** Lo scorrevole del pannello dei brani. */
  const scorrePannello = useRef<HTMLDivElement>(null);
  const idInterruttore = useId();

  const [stato, setStato] = useState<StatoAlbero>(VUOTO);
  /** Le radici che rispondono, per chiave. Assente = non ancora sondata. */
  const [vive, setVive] = useState<ReadonlyMap<string, boolean>>(new Map());
  /** Il nodo da cui l'ultima espansione è stata rifiutata, se c'è stata. */
  const [bloccata, setBloccata] = useState<string | null>(null);
  const [daConfermare, setDaConfermare] = useState<DaConfermare | null>(null);
  /** Le preferenze sono arrivate: prima di allora non si scrive niente. */
  const [ripristinato, setRipristinato] = useState(false);
  /** Quel che è scritto nel filtro, e la lente che è arrivata per lui. */
  const [testoFiltro, setTestoFiltro] = useState("");
  const [filtro, setFiltro] = useState<Lente | null>(null);
  const [selezione, setSelezione] = useState<Selezione>(NESSUNA);
  /** Una presa verso la coda in corso: quanti brani, e se è sopra la coda. */
  const [trascino, setTrascino] = useState<{ quanti: number; sopra: boolean } | null>(
    null,
  );
  /** La cartella che il pannello dei brani mostra. */
  const [cartellaScelta, setCartellaScelta] = useState<string | null>(null);
  const [conSottocartelle, setConSottocartelle] = useState(true);
  /** Gli identificativi del pannello, per quale cartella e in quale modo. */
  const [idsPannello, setIdsPannello] = useState<{
    percorso: string;
    ricorsivo: boolean;
    ids: readonly number[];
  } | null>(null);
  const [sceltaBrani, setSceltaBrani] = useState<SceltaBrani>(NESSUN_BRANO);
  /** Cresce a ogni `cartelle:cambiate`: rifà il filtro e il pannello. */
  const [giroLibreria, setGiroLibreria] = useState(0);

  /** Quali espansioni sono già in volo, per non chiederle due volte. */
  const inVolo = useRef(new Set<string>());
  /**
   * Quante volte l'albero è stato rifatto da capo.
   *
   * Una risposta partita prima di un giro nuovo parla dei brani di prima, e
   * arrivando dopo rimetterebbe le figlie vecchie sopra quelle nuove.
   */
  const giro = useRef(0);
  /** Quando è tornata l'ultima sonda. */
  const ultimaSonda = useRef(0);
  /** Quel che si è digitato di recente, e quando. */
  const digitato = useRef({ testo: "", quando: 0 });
  /** Il percorso su cui rimettere il fuoco quando la sua riga compare. */
  const sceltaRipresa = useRef<string | null>(null);
  /** Quel che c'è da ricordare e non è ancora stato scritto. */
  const daScrivere = useRef<{ aperte: string[]; scelta: string } | null>(null);
  /** Come smontare una presa in corso, se il pannello se ne va a metà. */
  const smontaPresa = useRef<(() => void) | null>(null);
  // Letti dagli effetti senza esserne dipendenze: `radici` cambia identità a
  // ogni disegno di `App`, e `onErrore` non è detto che sia stabile.
  const radiciRef = useRef(radici);
  radiciRef.current = radici;
  const onErroreRef = useRef(onErrore);
  onErroreRef.current = onErrore;

  // Un identificativo stabile dell'insieme delle radici: cambiarle rifà
  // l'albero, e confrontare due array per identità direbbe «cambiate» a ogni
  // disegno di `App`.
  //
  // Il separatore è scritto come sequenza di escape e non come il byte che è:
  // un NUL dentro il sorgente fa vedere a git un file binario, e da lì le
  // modifiche a questo file smettono di comparire nei diff.
  const firmaRadici = useMemo(() => radici.join("\0"), [radici]);

  /*
   * Le radici si disegnano **subito**, dai percorsi che si conoscono già.
   *
   * Il nucleo risponde in una trentina di millisecondi — è il tempo di
   * costruire il trie leggendo `tracks` — e in quei due fotogrammi il pannello
   * mostrerebbe il vuoto. Qui invece mostra i nomi, e il resto arriva con la
   * risposta vera, che li sostituisce. Niente compare prima con un valore
   * sbagliato: un conteggio a zero non si scrive, e la freccia si disegna solo
   * quando si sa che dietro c'è qualcosa.
   */
  const conRadici = useMemo(() => {
    if (stato.figli.get(CIME) !== undefined) return stato;
    const finte: NodoCartella[] = radici.map((percorso) => ({
      percorso,
      nome: percorso,
      brani: 0,
      sottocartelle: 0,
      radice: true,
    }));
    return conFigli(stato, null, finte);
  }, [stato, radici]);

  /** L'albero che si guarda: quello vero, o quello attraverso il filtro. */
  const vista = useMemo(
    () => (filtro === null ? conRadici : attraversoLente(conRadici, filtro)),
    [conRadici, filtro],
  );
  // La lingua fra le dipendenze: un brano d'immagine CD senza titolo si chiama
  // «Traccia 03 · Album», e cambiando lingua deve cambiare anche lui.
  const lingua = useLingua();
  const righe = useMemo(
    () =>
      appiattisci(vista, (numero, album) => t("folders.imageTrack", { numero, album })),
    [vista, lingua],
  );

  // ── quel che si chiede al nucleo ────────────────────────────────────────

  /**
   * Le figlie di un nodo e, se ne ha, le sue foglie: in una risposta sola per
   * la finestra, così le due cose compaiono nello stesso disegno.
   *
   * Le foglie passano da `letture` come le figlie: nello Studio le dà il mondo
   * finto, con i suoi file finti, e non il nucleo — chiederle a lui vorrebbe
   * dire leggere la libreria di chi sta disegnando un tema.
   */
  const leggiNodo = useCallback(
    (percorso: string | null, conFile: boolean) =>
      Promise.all([
        letture.cartelleFiglie(radici, percorso),
        percorso !== null && conFile
          ? letture.cartelleFile(radici, percorso)
          : Promise.resolve(undefined),
      ]),
    [radici, letture],
  );

  /** Chiede le figlie di un nodo, una volta sola. */
  const chiediFiglie = useCallback(
    (percorso: string | null, conFile: boolean) => {
      const k = percorso === null ? CIME : chiave(percorso);
      if (inVolo.current.has(k)) return;
      inVolo.current.add(k);
      const mio = giro.current;
      leggiNodo(percorso, conFile)
        .then(([figlie, file]) => {
          if (mio === giro.current) {
            setStato((prima) => conFigli(prima, percorso, figlie, file));
          }
        })
        .catch(onErrore)
        .finally(() => {
          if (mio === giro.current) inVolo.current.delete(k);
        });
    },
    [leggiNodo, onErrore],
  );

  /**
   * Chiede quali radici rispondono.
   *
   * `forza` salta la freschezza, ed è quel che fa «Ricontrolla»: chi lo preme
   * sa qualcosa che il pannello non sa — ha appena riattaccato il cavo — e
   * fargli aspettare la scadenza sarebbe dargli un tasto che non fa niente.
   */
  const sonda = useCallback(
    (forza: boolean) => {
      if (radici.length === 0) return;
      const adesso = Date.now();
      if (!forza && adesso - ultimaSonda.current < FRESCHEZZA_MS) return;
      // Si segna **prima** della risposta: due espansioni ravvicinate non
      // devono far partire due sonde da otto secondi ciascuna.
      ultimaSonda.current = adesso;
      ipc
        .cartelleRadiciVive(radici)
        .then((risposte) => {
          ultimaSonda.current = Date.now();
          setVive(
            new Map(
              radici.map((percorso, i) => [chiave(percorso), risposte[i] ?? false]),
            ),
          );
        })
        .catch(onErrore);
    },
    [radici, onErrore],
  );

  /*
   * Le radici, la sonda e quel che era rimasto aperto: tutto alla prima
   * apertura della vista, e da capo se le cartelle sorvegliate cambiano —
   * allora l'albero di prima risponderebbe a una domanda che non è più quella.
   *
   * Le dipendenze sono `firmaRadici` e basta, di proposito: `radici` cambia
   * identità a ogni disegno di `App`, e rifare l'albero a ogni disegno sarebbe
   * il contrario di quel che questo pannello promette. `chiediFiglie` e `sonda`
   * dipendono da `radici` e quindi cambierebbero insieme a lei; qui servono per
   * quel che fanno, non per l'identità che hanno.
   */
  useEffect(() => {
    setStato(VUOTO);
    setVive(new Map());
    setBloccata(null);
    setRipristinato(false);
    setSelezione(NESSUNA);
    setCartellaScelta(null);
    giro.current += 1;
    inVolo.current.clear();
    ultimaSonda.current = 0;
    chiediFiglie(null, false);
    sonda(false);
    let vivo = true;
    ipc
      .cartelleUi()
      .then((ricordo) => {
        if (!vivo) return;
        sceltaRipresa.current = ricordo.scelta;
        setStato((prima) => apriTutti(prima, ricordo.aperte));
        setRipristinato(true);
      })
      .catch(() => {
        // Un ricordo che non si legge non è un guasto da mostrare: il pannello
        // si apre chiuso, che è come si apre la prima volta di sempre.
        if (vivo) setRipristinato(true);
      });
    return () => {
      vivo = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [firmaRadici]);

  /**
   * Riempie i nodi aperti di cui non si sanno ancora le figlie.
   *
   * Un effetto solo per tre casi che sono lo stesso: il clic che apre un nodo
   * mai visto, il ripristino, che apre in un colpo i nodi di ieri, e il filtro,
   * dentro cui si può aprire una cartella trovata. Da tutti e tre si arriva qui
   * con una riga `aperto` e una cache vuota; la risposta fa comparire i figli,
   * che a loro volta possono essere aperti, e il giro ricomincia un livello più
   * in giù. Il ripristino si srotola così, da sé, e **solo** lungo i rami che si
   * vedono davvero: un ramo di ieri sepolto sotto un padre chiuso non costa
   * niente finché non lo si riapre.
   */
  useEffect(() => {
    for (const riga of righe) {
      if (!riga.aperto || !riga.figli || riga.brano !== null) continue;
      if (figliDi(vista, riga.percorso) !== undefined) continue;
      chiediFiglie(riga.percorso, riga.file > 0);
    }
  }, [righe, vista, chiediFiglie]);

  /*
   * Una scansione o un profilo importato hanno cambiato i brani: si richiede
   * quel che si sta guardando.
   *
   * Solo le cime e i nodi aperti che si vedono, tutti insieme, e la cache si
   * sostituisce **in un colpo** quando sono tornati. Svuotarla subito farebbe
   * sparire l'albero per il tempo della risposta, e con lui lo scorrimento, che
   * senza righe torna in cima. Le figlie dei nodi chiusi si buttano e basta: si
   * richiedono quando li si riapre. Aperti e scelta restano come sono, e quel
   * che una risposta nuova fa comparire aperto lo riempie l'effetto qui sopra.
   * Il filtro e il pannello dei brani si rifanno anche loro: parlano della
   * stessa libreria.
   */
  useAscolto("cartelle:cambiate", () => {
    giro.current += 1;
    const mio = giro.current;
    inVolo.current.clear();
    const richieste = [
      { percorso: null as string | null, conFile: false },
      ...righe
        .filter((riga) => riga.aperto && riga.brano === null)
        .map((riga) => ({ percorso: riga.percorso as string | null, conFile: riga.file > 0 })),
    ];
    void Promise.allSettled(
      richieste.map((r) => leggiNodo(r.percorso, r.conFile)),
    ).then((risposte) => {
      if (mio !== giro.current) return;
      setStato((prima) =>
        richieste.reduce<StatoAlbero>(
          (dopo, r, i) => {
            const risposta = risposte[i];
            return risposta?.status === "fulfilled"
              ? conFigli(dopo, r.percorso, risposta.value[0], risposta.value[1])
              : dopo;
          },
          { figli: new Map(), file: new Map(), aperti: prima.aperti },
        ),
      );
    });
    setGiroLibreria((n) => n + 1);
  });

  // ── il filtro ───────────────────────────────────────────────────────────

  /*
   * Il filtro si chiede dopo una pausa nella digitazione, e la risposta di un
   * testo superato si butta. Svuotato, la lente se ne va subito: l'albero di
   * prima è lì com'era, aperti compresi, perché il filtro non li ha mai
   * toccati.
   */
  useEffect(() => {
    const testo = testoFiltro.trim();
    if (testo.length === 0) {
      setFiltro(null);
      return;
    }
    if (anteprima) return;
    let vivo = true;
    const attesa = window.setTimeout(() => {
      ipc
        .cartelleFiltra(radiciRef.current, testo)
        .then((risposta) => {
          if (vivo) setFiltro(lenteDi(risposta));
        })
        .catch((e: unknown) => {
          if (vivo) onErroreRef.current(e);
        });
    }, RITARDO_FILTRO_MS);
    return () => {
      vivo = false;
      window.clearTimeout(attesa);
    };
  }, [testoFiltro, firmaRadici, giroLibreria, anteprima]);

  // ── aprire, chiudere ────────────────────────────────────────────────────

  /** Una radice sondata e muta: espanderla non porterebbe da nessuna parte. */
  const spenta = useCallback(
    (riga: RigaAlbero) => riga.radice && vive.get(chiave(riga.percorso)) === false,
    [vive],
  );

  /*
   * Aprire e chiudere toccano gli aperti di chi si sta guardando: quelli
   * dell'albero, o quelli della lente. Gli aperti del filtro non hanno il tetto
   * dei duecento — non si ricordano fra due avvii, e un filtro con duemila
   * corrispondenze può aprire più strade di così.
   */
  const inFiltro = filtro !== null;
  const apriNodi = useCallback(
    (percorsi: readonly string[]) => {
      if (!inFiltro) {
        setStato((prima) => apriTutti(prima, percorsi));
        return;
      }
      const chiavi = percorsi.map(chiave);
      setFiltro((prima) =>
        prima === null
          ? prima
          : {
              ...prima,
              aperti: [...prima.aperti.filter((k) => !chiavi.includes(k)), ...chiavi],
            },
      );
    },
    [inFiltro],
  );
  const chiudiNodo = useCallback(
    (percorso: string) => {
      if (!inFiltro) {
        setStato((prima) => chiudi(prima, percorso));
        return;
      }
      const k = chiave(percorso);
      setFiltro((prima) =>
        prima === null ? prima : { ...prima, aperti: prima.aperti.filter((a) => a !== k) },
      );
    },
    [inFiltro],
  );

  const apriRiga = useCallback(
    (riga: RigaAlbero) => {
      if (spenta(riga)) {
        // Non si apre, e non in silenzio. Aprire un ramo di una share morta
        // darebbe un elenco vuoto indistinguibile da una cartella davvero
        // vuota, che è la bugia peggiore che questo pannello possa dire —
        // perché è quella che si crede.
        setBloccata(riga.percorso);
        sonda(false);
        return;
      }
      setBloccata(null);
      apriNodi([riga.percorso]);
    },
    [spenta, sonda, apriNodi],
  );

  const alterna = useCallback(
    (riga: RigaAlbero) => {
      if (!riga.figli) return;
      if (riga.aperto) chiudiNodo(riga.percorso);
      else apriRiga(riga);
    },
    [apriRiga, chiudiNodo],
  );

  // ── i brani di quel che si è scelto ─────────────────────────────────────

  /** Le righe su cui agisce un comando: la selezione, senza doppioni. */
  const scelte = useMemo(
    () => bersagli(righe, selezione.chiavi),
    [righe, selezione.chiavi],
  );
  /** Quante righe scelte si vedono: è il numero che la striscia dice. */
  const quanteScelte = useMemo(
    () => righe.filter((riga) => selezione.chiavi.has(chiaveRiga(riga))).length,
    [righe, selezione.chiavi],
  );

  /**
   * Su cosa agisce un gesto fatto su `riga`.
   *
   * Su una riga scelta agisce su tutta la selezione; su una riga fuori dalla
   * selezione agisce su quella sola, e non la cambia. È la convenzione
   * dell'elenco dei brani (`menuSuSelezione` in `App`), e di ogni gestore di
   * file.
   */
  const suCosa = useCallback(
    (riga: RigaAlbero): readonly RigaAlbero[] =>
      selezione.chiavi.has(chiaveRiga(riga)) && scelte.length > 0 ? scelte : [riga],
    [selezione.chiavi, scelte],
  );

  /**
   * Gli identificativi di un gruppo di righe, nell'ordine dell'albero.
   *
   * Una cartella porta i suoi brani nell'ordine dell'album, sottocartelle
   * comprese; un file porta sé stesso. I doppioni si tolgono — `bersagli` toglie
   * già i file dentro una cartella scelta, ma due cartelle annidate scelte da
   * due radici sovrapposte no — tenendo la prima volta.
   */
  const idsDi = useCallback(
    async (gruppo: readonly RigaAlbero[]) => {
      const pezzi = await Promise.all(
        gruppo.map((riga) =>
          riga.brano !== null
            ? Promise.resolve([riga.brano])
            : letture.cartelleBrani(radici, riga.percorso),
        ),
      );
      return [...new Set(pezzi.flat())];
    },
    [letture, radici],
  );

  /**
   * Prende gli identificativi e li dà a chi di dovere, chiedendo se sono tanti.
   *
   * Il conteggio si guarda **prima** della richiesta: è già nelle righe, arriva
   * ricorsivo dal nucleo, e chiedere diciottomila identificativi per poi
   * scoprire che l'utente non li voleva sarebbe la lettura più cara del
   * pannello fatta per niente.
   *
   * `gesto` non cambia quel che si fa — lo fa `fai` — ma quel che la conferma
   * dice: vedi `ConfermaCoda`.
   */
  const conBrani = useCallback(
    (
      gruppo: readonly RigaAlbero[],
      fai: (brani: number[]) => Promise<void>,
      gesto: GestoCoda,
    ) => {
      const quanti = gruppo.reduce((n, riga) => n + riga.brani, 0);
      if (quanti === 0) return;
      const vai = () => {
        idsDi(gruppo)
          .then((ids) => (ids.length > 0 ? fai(ids) : undefined))
          .catch(onErrore);
      };
      if (quanti > SOGLIA_CONFERMA) {
        setDaConfermare({ brani: quanti, gesto, righe: gruppo.length, vai });
        return;
      }
      vai();
    },
    [idsDi, onErrore],
  );

  const riproduci = useCallback(
    (gruppo: readonly RigaAlbero[]) =>
      conBrani(gruppo, (brani) => ipc.suona(brani, 0), "sostituisce"),
    [conBrani],
  );

  /** «Aggiungi a playlist…» con gli identificativi già risolti. */
  const aggiungiAPlaylist = useCallback(
    (gruppo: readonly RigaAlbero[]) => {
      if (mani === undefined) return;
      idsDi(gruppo)
        .then((ids) => {
          if (ids.length > 0) mani.onAggiungiAPlaylist(ids);
        })
        .catch(onErrore);
    },
    [mani, idsDi, onErrore],
  );

  /**
   * Il doppio clic su un file, come in foobar2000: suona la sua cartella **da
   * lì**, nell'ordine in cui la si vede.
   */
  const suonaDaFile = useCallback(
    (indice: number) => {
      const coda = codaDaFile(righe, indice);
      if (coda !== null) ipc.suona(coda.brani, coda.partenza).catch(onErrore);
    },
    [righe, onErrore],
  );

  const menuDi = useCallback(
    (riga: RigaAlbero, x: number, y: number) => {
      const gruppo = suCosa(riga);
      const voci: Voce[] = [
        { etichetta: t("action.play"), azione: () => riproduci(gruppo) },
        {
          etichetta: t("action.playNext"),
          azione: () => conBrani(gruppo, ipc.codaDopo, "dopo"),
        },
        {
          etichetta: t("action.enqueue"),
          azione: () => conBrani(gruppo, ipc.codaAccoda, "accoda"),
        },
      ];
      if (mani !== undefined) {
        voci.push({
          etichetta: t("action.addToPlaylist"),
          azione: () => aggiungiAPlaylist(gruppo),
        });
      }
      // «Mostra nella cartella» su una riga sola: Esplora risorse apre un posto
      // alla volta. Un file si nomina per identificativo, come nell'elenco dei
      // brani; una cartella per percorso, e solo sotto una radice vera — il
      // nucleo rifiuta le altre, e una voce che porta a un rifiuto non si
      // mostra.
      const sola = gruppo.length === 1 ? gruppo[0] : undefined;
      if (sola !== undefined && !anteprima) {
        const brano = sola.brano;
        if (brano !== null) {
          voci.push({
            etichetta: t("menu.showInFolder"),
            azione: () => {
              ipc.branoMostraNellaCartella(brano).catch(onErrore);
            },
          });
        } else if (sola.sorvegliata) {
          voci.push({
            etichetta: t("menu.showInFolder"),
            azione: () => {
              ipc.cartellaMostra(sola.percorso).catch(onErrore);
            },
          });
        }
      }
      onMenu({ x, y, voci });
    },
    [suCosa, riproduci, conBrani, mani, aggiungiAPlaylist, anteprima, onMenu, onErrore],
  );

  // ── il fuoco e la finestra ──────────────────────────────────────────────

  const finestra = useVirtuale({
    totale: righe.length,
    contenitore: scorreAlbero,
    ancora: albero,
    selettoreRiga: RIGA,
  });

  /** Invio fa quel che fa il doppio clic. */
  const invioIndice = useCallback(
    (indice: number) => {
      const riga = righe[indice];
      if (riga === undefined) return;
      if (riga.brano !== null) suonaDaFile(indice);
      else riproduci([riga]);
    },
    [righe, suonaDaFile, riproduci],
  );

  const scegli = useCallback(
    (indice: number, gesto: GestoSelezione) =>
      setSelezione((prima) => seleziona(prima, righe, indice, gesto)),
    [righe],
  );

  /** Spazio sceglie, come nell'elenco dei brani: vedi il preambolo. */
  const spazioIndice = useCallback((indice: number) => scegli(indice, "alterna"), [scegli]);

  const fuoco = useFuoco({
    totale: righe.length,
    ancora: albero,
    finestra,
    scorriA: finestra.scorriA,
    selettoreRiga: RIGA,
    onInvio: invioIndice,
    onSpazio: spazioIndice,
  });

  /*
   * Il fuoco torna dov'era, **senza prenderselo**.
   *
   * `segna` e non `vaiA`: `vaiA` chiama `focus()`, e chiamarlo al montaggio
   * vorrebbe dire strappare il fuoco a chi ha appena premuto «Cartelle» nella
   * barra laterale. Quel che serve è che la riga di ieri sia quella con
   * `tabIndex={0}`, cioè quella su cui Tab entra: è precisamente ciò che
   * `segna` fa, e `vaiA` fa in più.
   */
  useEffect(() => {
    const dove = sceltaRipresa.current;
    if (dove === null || righe.length === 0) return;
    const k = chiave(dove);
    const indice = righe.findIndex((riga) => chiave(riga.percorso) === k);
    if (indice < 0) return;
    sceltaRipresa.current = null;
    fuoco.segna(indice);
    finestra.scorriA(indice);
  }, [righe, fuoco, finestra]);

  /**
   * Il fuoco è arrivato su una riga perché **qualcuno** ce l'ha portato.
   *
   * Solo allora la ripresa si dichiara conclusa. La differenza conta nel caso
   * storto: se la cartella di ieri non c'è più, `sceltaRipresa` resterebbe
   * appesa per sempre e nessuna scelta nuova verrebbe più ricordata. Un clic o
   * un Tab la chiudono, e il ricordo riparte da dove si è davvero.
   */
  const segnaFuoco = useCallback(
    (indice: number) => {
      sceltaRipresa.current = null;
      fuoco.segna(indice);
    },
    [fuoco],
  );

  // ── quel che si ricorda ─────────────────────────────────────────────────

  const scelta = righe[fuoco.attivo]?.percorso ?? "";

  /*
   * Si scrive con calma, e si scrive comunque uscendo.
   *
   * Mezzo secondo di attesa perché tenere premuta ↓ su un albero grande sono
   * venti scritture al secondo in `settings`, cioè venti transazioni per un
   * dato che interessa solo al prossimo avvio. E una scrittura all'uscita
   * perché senza, chi apre un nodo e cambia vista subito dopo ritroverebbe il
   * pannello com'era prima: mezzo secondo non è passato, e il ricordo si
   * perderebbe proprio nel gesto più comune. Si ricordano gli aperti
   * dell'albero, non quelli del filtro: il filtro non sopravvive a un avvio.
   */
  useEffect(() => {
    if (!ripristinato) return;
    // Finché la ripresa non è arrivata a destinazione si riscrive il percorso
    // di ieri, non `righe[0]`: fra la lettura delle preferenze e la comparsa
    // della riga passa il tempo di un'espansione, e in mezzo `fuoco.attivo` è
    // ancora zero. Senza questa riga, aprire e chiudere il pannello in fretta
    // sposterebbe il ricordo sulla prima cartella dell'elenco.
    daScrivere.current = {
      aperte: [...stato.aperti],
      scelta: sceltaRipresa.current ?? scelta,
    };
    const attesa = window.setTimeout(() => {
      const cosa = daScrivere.current;
      daScrivere.current = null;
      if (cosa !== null) {
        letture.impostaCartelleUi(cosa.aperte, cosa.scelta).catch(onErrore);
      }
    }, RITARDO_RICORDO_MS);
    return () => window.clearTimeout(attesa);
  }, [stato.aperti, scelta, ripristinato, onErrore, letture]);

  useEffect(
    () => () => {
      const cosa = daScrivere.current;
      daScrivere.current = null;
      // Niente `onErrore`: il componente sta morendo, e una fascia rossa su una
      // schermata che non c'è più non la vedrebbe nessuno.
      if (cosa !== null) void letture.impostaCartelleUi(cosa.aperte, cosa.scelta);
    },
    [letture],
  );

  // ── il pannello dei brani ───────────────────────────────────────────────

  /*
   * Il pannello segue la cartella su cui sta il cursore, quando ci si ferma.
   *
   * Non prima che il ricordo di ieri sia arrivato a destinazione: al montaggio
   * il cursore sta sulla prima cima per il tempo di un'espansione, e seguirla
   * vorrebbe dire leggere la libreria intera per poi buttarla. Su un file il
   * pannello resta sulla cartella di prima — il file sta lì dentro.
   */
  const sottoIlCursore = righe[fuoco.attivo];
  const cartellaDelCursore =
    sottoIlCursore !== undefined && sottoIlCursore.brano === null
      ? sottoIlCursore.percorso
      : null;
  useEffect(() => {
    if (!ripristinato || cartellaDelCursore === null || sceltaRipresa.current !== null) {
      return;
    }
    const attesa = window.setTimeout(
      () => setCartellaScelta(cartellaDelCursore),
      RITARDO_PANNELLO_MS,
    );
    return () => window.clearTimeout(attesa);
  }, [cartellaDelCursore, ripristinato]);

  /*
   * Gli identificativi della cartella scelta: tutti, e subito, perché costano
   * otto byte l'uno e sono quel che va in coda. Le righe invece arrivano a
   * pagine, dove si guarda (`useRada`), come in libreria.
   */
  useEffect(() => {
    if (cartellaScelta === null) {
      setIdsPannello(null);
      return;
    }
    let vivo = true;
    letture
      .cartelleBrani(radiciRef.current, cartellaScelta, conSottocartelle)
      .then((ids) => {
        if (!vivo) return;
        setIdsPannello({ percorso: cartellaScelta, ricorsivo: conSottocartelle, ids });
      })
      .catch((e: unknown) => {
        if (vivo) onErroreRef.current(e);
      });
    return () => {
      vivo = false;
    };
  }, [cartellaScelta, conSottocartelle, firmaRadici, giroLibreria, letture]);

  const ids = idsPannello?.ids ?? NESSUN_ID;
  const chiavePannello =
    idsPannello === null ? "" : `${idsPannello.percorso}|${String(idsPannello.ricorsivo)}`;
  const archivio = useRada<Brano>({
    chiave: chiavePannello,
    totale: ids.length,
    chiedi: (offset, limite) => {
      const pezzo = ids.slice(offset, offset + limite);
      return pezzo.length === 0 ? Promise.resolve([]) : letture.braniPerId(pezzo);
    },
    onErrore,
  });

  /*
   * La stessa cartella con identificativi nuovi — una scansione è finita — è
   * la stessa vista cambiata sotto: le righe restano a schermo e si
   * richiedono. Una cartella nuova invece la svuota da sé `useRada`, con la
   * chiave, e la selezione del pannello se ne va con lei.
   */
  const chiaveDiPrima = useRef("");
  useEffect(() => {
    if (chiaveDiPrima.current === chiavePannello) archivio.invalida();
    else setSceltaBrani(NESSUN_BRANO);
    chiaveDiPrima.current = chiavePannello;
  }, [ids, chiavePannello, archivio]);

  const suonaDalPannello = useCallback(
    (indice: number) => {
      // Per identificativo e non per posizione: un brano sparito fra la lettura
      // degli identificativi e quella delle righe accorcia le pagine, e la
      // posizione nella pagina non direbbe più quale brano si è premuto.
      const brano = archivio.riga(indice);
      const da = brano === undefined ? indice : ids.indexOf(brano.id);
      ipc.suona([...ids], Math.max(da, 0)).catch(onErrore);
    },
    [archivio, ids, onErrore],
  );

  /** La selezione del pannello: le regole dell'elenco della libreria. */
  const scegliBrano = useCallback(
    (e: EventoMouse, indice: number) => {
      const brano = archivio.riga(indice);
      if (brano === undefined) return;
      const { ancora } = sceltaBrani;
      if (e.shiftKey && ancora !== null) {
        e.preventDefault();
        void archivio
          .intervallo(Math.min(ancora, indice), Math.max(ancora, indice))
          .then((presi) =>
            setSceltaBrani({
              ids: new Set(presi.flatMap((b) => (b === undefined ? [] : [b.id]))),
              ancora,
            }),
          );
        return;
      }
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        setSceltaBrani((prima) => {
          const dopo = new Set(prima.ids);
          if (!dopo.delete(brano.id)) dopo.add(brano.id);
          return { ids: dopo, ancora: indice };
        });
        return;
      }
      setSceltaBrani({ ids: new Set(), ancora: indice });
    },
    [archivio, sceltaBrani],
  );

  const alternaBrano = useCallback(
    (indice: number) => {
      const brano = archivio.riga(indice);
      if (brano === undefined) return;
      setSceltaBrani((prima) => {
        const dopo = new Set(prima.ids);
        if (!dopo.delete(brano.id)) dopo.add(brano.id);
        return { ids: dopo, ancora: indice };
      });
    },
    [archivio],
  );

  /**
   * `Ctrl+A` nel pannello: tutti i brani della cartella, anche quelli le cui
   * righe non sono arrivate. Gli identificativi sono già tutti in mano — sono
   * quel che va in coda — quindi non c'è niente da chiedere al nucleo.
   */
  const sceglieTuttiIBrani = useCallback(
    () => setSceltaBrani({ ids: new Set(ids), ancora: null }),
    [ids],
  );

  /** Il menù su una riga del pannello, che sa della selezione: come in `App`. */
  const menuBrani = useCallback(
    (e: EventoMouse, brani: number[]) => {
      e.preventDefault();
      if (mani === undefined) return;
      const dentro = brani.some((id) => sceltaBrani.ids.has(id));
      const scelti = ids.filter((id) => sceltaBrani.ids.has(id));
      mani.onMenu(e, dentro && scelti.length > 0 ? scelti : brani);
    },
    [mani, sceltaBrani.ids, ids],
  );

  /*
   * Cuore e stelle si accendono qui subito, come nell'elenco della libreria,
   * e la scrittura la fa `App`, che tiene anche le altre copie dello stesso
   * brano: la barra, i Preferiti, l'elenco che sta sotto.
   */
  const preferito = useCallback(
    (b: Brano) => {
      archivio.aggiorna((righe) =>
        righe.map((r) => (r.id === b.id ? { ...r, liked: !b.liked } : r)),
      );
      mani?.onPreferito(b);
    },
    [archivio, mani],
  );
  const voto = useCallback(
    (b: Brano, stelle: number) => {
      archivio.aggiorna((righe) =>
        righe.map((r) => (r.id === b.id ? { ...r, rating: stelle } : r)),
      );
      mani?.onVoto(b, stelle);
    },
    [archivio, mani],
  );

  const suonaTuttoIlPannello = useCallback(() => {
    if (ids.length === 0) return;
    const vai = () => {
      ipc.suona([...ids], 0).catch(onErrore);
    };
    if (ids.length > SOGLIA_CONFERMA)
      setDaConfermare({ brani: ids.length, gesto: "sostituisce", righe: 1, vai });
    else vai();
  }, [ids, onErrore]);

  // ── trascinare in coda ──────────────────────────────────────────────────

  useEffect(() => () => smontaPresa.current?.(), []);

  /**
   * La pressione su una riga che può diventare una presa verso la coda.
   *
   * Il gesto è quello di `riordino.ts`, riscritto qui perché là il bersaglio è
   * una riga dello stesso elenco o una fessura, e qui è un pannello che non sa
   * niente di questo: la coda si riconosce per classe, sotto il puntatore.
   * Quel che si porta si decide alla pressione, con la regola del menù: la
   * selezione se la riga ne fa parte, la riga sola se no.
   */
  const presaVersoCoda = useCallback(
    (e: EventoPuntatore<HTMLElement>, riga: RigaAlbero) => {
      // Ctrl e Maiusc sono la selezione: quel clic non è una presa. Il tatto
      // nemmeno: lì trascinare vuol dire scorrere.
      if (e.button !== 0 || e.pointerType === "touch") return;
      if (e.ctrlKey || e.shiftKey || e.metaKey || e.altKey || anteprima) return;
      const gruppo = suCosa(riga);
      const quanti = gruppo.reduce((n, r) => n + r.brani, 0);
      if (quanti === 0) return;
      smontaPresa.current?.();

      const radice = document.documentElement;
      const puntatore = e.pointerId;
      const partenza = { x: e.clientX, y: e.clientY };
      let attiva = false;
      let sopra: Element | null = null;

      const mira = (x: number, y: number) => {
        const qui = document.elementFromPoint(x, y)?.closest(BERSAGLIO_CODA) ?? null;
        if (qui === sopra) return;
        sopra = qui;
        radice.dataset.versoCoda = qui === null ? "" : "mira";
        setTrascino({ quanti, sopra: qui !== null });
      };
      const smonta = () => {
        window.removeEventListener("pointermove", muovi, true);
        window.removeEventListener("pointerup", alza, true);
        window.removeEventListener("pointercancel", annulla, true);
        window.removeEventListener("keydown", tasto, true);
        delete radice.dataset.versoCoda;
        delete radice.dataset.riordinando;
        smontaPresa.current = null;
        setTrascino(null);
      };
      function muovi(ev: PointerEvent) {
        if (ev.pointerId !== puntatore) return;
        if (!attiva) {
          if (Math.hypot(ev.clientX - partenza.x, ev.clientY - partenza.y) < SOGLIA_PRESA_PX) {
            return;
          }
          attiva = true;
          // Il cursore e la selezione del testo per tutta la pagina: è
          // l'attributo del riordino, e il foglio lo sa già disegnare.
          radice.dataset.riordinando = "";
          radice.dataset.versoCoda = "";
          window.getSelection()?.removeAllRanges();
          setTrascino({ quanti, sopra: false });
        }
        ev.preventDefault();
        mira(ev.clientX, ev.clientY);
      }
      function alza(ev: PointerEvent) {
        if (ev.pointerId !== puntatore) return;
        const eraAttiva = attiva;
        const dove = sopra;
        smonta();
        if (!eraAttiva) return;
        if (dove !== null) conBrani(gruppo, ipc.codaAccoda, "accoda");
        // Il clic che segue il rilascio aprirebbe la cartella da cui si è
        // partiti: si inghiotte, come in `riordino.ts`.
        const inghiotti = (c: MouseEvent) => {
          c.preventDefault();
          c.stopPropagation();
        };
        window.addEventListener("click", inghiotti, { capture: true, once: true });
        window.setTimeout(
          () => window.removeEventListener("click", inghiotti, { capture: true }),
          0,
        );
      }
      function annulla(ev: PointerEvent) {
        if (ev.pointerId === puntatore) smonta();
      }
      function tasto(ev: globalThis.KeyboardEvent) {
        if (ev.key !== "Escape" || !attiva) return;
        ev.preventDefault();
        ev.stopPropagation();
        smonta();
      }
      window.addEventListener("pointermove", muovi, true);
      window.addEventListener("pointerup", alza, true);
      window.addEventListener("pointercancel", annulla, true);
      window.addEventListener("keydown", tasto, true);
      smontaPresa.current = smonta;
    },
    [suCosa, conBrani, anteprima],
  );

  // ── la tastiera ─────────────────────────────────────────────────────────

  /**
   * I tasti dell'albero, e poi quelli dell'elenco.
   *
   * ↓↑, Inizio, Fine, Invio e Spazio sono di `fuoco.ts` e non si riscrivono.
   * Quel che è dell'albero — ←→, `*`, Maiusc+↑↓, Esc sulla selezione, la
   * digitazione, il tasto menù — si intercetta **prima**, perché ←→ in
   * `fuoco.ts` girano fra i comandi della riga, e qui dentro comandi non ce ne
   * sono: la riga è tutta un bersaglio, e le due frecce servono ad aprire e
   * chiudere.
   */
  const daTastiera = useCallback(
    (e: KeyboardEvent<HTMLDivElement>) => {
      const indice = fuoco.attivo;
      const riga = righe[indice];
      if (riga === undefined || e.altKey || e.ctrlKey || e.metaKey) {
        fuoco.daTastiera(e);
        return;
      }
      const preso = () => {
        e.preventDefault();
        e.stopPropagation();
      };

      // Il tasto menù, e Shift+F10 che è il suo sinonimo sulle tastiere che
      // quel tasto non ce l'hanno. Il menù si apre sulla **riga** e non dove
      // sta il puntatore: in questo gesto il puntatore non c'entra.
      if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
        preso();
        const nodo = e.target instanceof HTMLElement ? e.target : null;
        const dove = nodo?.closest(RIGA)?.getBoundingClientRect();
        menuDi(riga, dove?.left ?? 0, dove?.bottom ?? 0);
        return;
      }

      // Maiusc+↑↓: il cursore si sposta e la selezione si allarga dall'àncora,
      // che se manca è la riga da cui si parte. È il Maiusc+clic detto da
      // tastiera, come Spazio è il Ctrl+clic.
      if (e.shiftKey && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
        preso();
        const dove = Math.min(
          Math.max(indice + (e.key === "ArrowDown" ? 1 : -1), 0),
          righe.length - 1,
        );
        setSelezione((prima) =>
          seleziona(
            prima.ancora === null ? { ...prima, ancora: indice } : prima,
            righe,
            dove,
            "intervallo",
          ),
        );
        fuoco.vaiA(dove);
        return;
      }

      switch (e.key) {
        case "Escape": {
          // Esc toglie la selezione, se c'è; se no passa, e fa quel che fa
          // fuori di qui.
          if (selezione.chiavi.size === 0) return;
          preso();
          setSelezione((prima) => ({ chiavi: new Set(), ancora: prima.ancora }));
          return;
        }
        case "ArrowRight": {
          preso();
          if (!riga.figli) return;
          // Chiuso: si apre. Aperto: si scende al primo figlio, che
          // nell'albero appiattito è semplicemente la riga dopo.
          if (!riga.aperto) {
            apriRiga(riga);
            return;
          }
          const sotto = righe[indice + 1];
          if (sotto !== undefined && sotto.livello > riga.livello) {
            fuoco.vaiA(indice + 1);
          }
          return;
        }
        case "ArrowLeft": {
          preso();
          // Aperto: si chiude. Chiuso, o senza figli: si risale al padre. È la
          // coppia che rende ← l'unico tasto per uscire da un ramo, invece di
          // farne uno per chiudere e uno per risalire.
          if (riga.aperto && riga.figli) {
            chiudiNodo(riga.percorso);
            return;
          }
          const padre = indicePadre(righe, indice);
          if (padre >= 0) fuoco.vaiA(padre);
          return;
        }
        case "*": {
          preso();
          // Tutte le cartelle sorelle del livello in un colpo: è la scorciatoia
          // che ogni albero ha, e serve a vedere la forma di un ramo senza
          // aprirlo nodo per nodo.
          const fratelli = fratelliDi(righe, indice).filter(
            (percorso) => !eAperto(vista, percorso),
          );
          if (fratelli.length > 0) apriNodi(fratelli);
          return;
        }
        default:
          break;
      }

      // La digitazione. Una lettera sola e nessun modificatore; Spazio escluso,
      // perché quello è di `fuoco.ts` e qui sceglie la riga.
      if (e.key.length !== 1 || e.key === " ") {
        fuoco.daTastiera(e);
        return;
      }
      // Una lettera dentro l'albero è **sempre** della digitazione, anche
      // quando non trova niente: lasciarla passare vorrebbe dire che scrivere
      // «zz» su una libreria senza cartelle che cominciano per zeta fa partire
      // due scorciatoie di trasporto, perché quelle vivono su un ascolto
      // appeso a `window` che non sa chi ha il fuoco (`tastiera.ts`).
      preso();
      const adesso = performance.now();
      const memoria = digitato.current;
      const testo =
        adesso - memoria.quando > MEMORIA_MS ? e.key : memoria.testo + e.key;
      digitato.current = { testo, quando: adesso };
      // Con una parola che cresce si riparte **da sé stessi**: chi scrive «ro»
      // dopo «r» sta precisando la stessa scelta, non chiedendo la prossima.
      // Con una lettera sola si riparte da dopo, ed è quel che fa girare fra
      // tre cartelle premendo «r» tre volte.
      const dove = saltaA(righe, testo, testo.length > 1 ? indice - 1 : indice);
      if (dove >= 0) fuoco.vaiA(dove);
    },
    [fuoco, righe, vista, selezione.chiavi.size, apriRiga, chiudiNodo, apriNodi, menuDi],
  );

  // ── il disegno ──────────────────────────────────────────────────────────

  const spente = useMemo(
    () => radici.filter((percorso) => vive.get(chiave(percorso)) === false).length,
    [radici, vive],
  );

  if (radici.length === 0) {
    return (
      <StatoVuoto
        icona="i-folder"
        titolo={t("folders.empty.title")}
        testo={t("folders.empty.hint")}
      />
    );
  }

  const testoCercato = testoFiltro.trim();
  /**
   * Quel che il filtro ha da dire: niente trovato, o troppo. Quando ha trovato
   * una quantità giusta tace — le righe lo dicono da sé.
   */
  const notaFiltro =
    filtro === null || testoCercato.length === 0
      ? null
      : righe.length === 0
        ? t("folders.filter.none", { testo: testoCercato })
        : filtro.troncato
          ? t("folders.filter.truncated", { n: filtro.corrispondenze })
          : null;
  /** Il nome della cartella del pannello: come la scrive la sua riga. */
  const nomePannello =
    cartellaScelta === null
      ? ""
      : (righe.find((riga) => chiave(riga.percorso) === chiave(cartellaScelta))?.nome ??
        cartellaScelta.split(/[\\/]/).filter((parte) => parte.length > 0).pop() ??
        cartellaScelta);

  return (
    <div className="vista-cartelle">
      <div className="colonna-albero">
        <div className="filtro-cartelle">
          <div className="cerca search-field">
            <Icona nome="i-search" dim={15} />
            <input
              className="campo field-input"
              type="search"
              value={testoFiltro}
              placeholder={t("folders.filter")}
              aria-label={t("folders.filter")}
              spellCheck={false}
              onChange={(e) => setTestoFiltro(e.currentTarget.value)}
              onKeyDown={(e) => {
                // ↓ porta nell'albero, dove il filtro ha appena lasciato le
                // righe: è la strada che chi scrive in una casella sopra un
                // elenco cerca per prima. Esc svuota, e si ferma qui — fuori
                // farebbe chiudere altro.
                if (e.key === "ArrowDown" && righe.length > 0) {
                  e.preventDefault();
                  fuoco.vaiA(fuoco.attivo);
                } else if (e.key === "Escape" && testoFiltro.length > 0) {
                  e.preventDefault();
                  e.stopPropagation();
                  setTestoFiltro("");
                }
              }}
            />
          </div>
        </div>

        {spente > 0 && (
          <div className="nota">
            <span>
              {bloccata === null
                ? t("folders.offline", { n: spente })
                : t("folders.blocked", { nome: bloccata })}
            </span>
            <button type="button" className="bottone" onClick={() => sonda(true)}>
              {t("folders.recheck")}
            </button>
          </div>
        )}

        {notaFiltro !== null && (
          <p className="nota-cartelle" role="status">
            {notaFiltro}
          </p>
        )}

        {quanteScelte > 0 && (
          <div className="scelta-cartelle">
            <span className="conteggio-nodo">{t("folders.selected", { n: quanteScelte })}</span>
            <button
              type="button"
              className="bottone minuto btn-ghost"
              onClick={() => riproduci(scelte)}
            >
              {t("action.play")}
            </button>
            <button
              type="button"
              className="bottone minuto btn-ghost"
              onClick={() => conBrani(scelte, ipc.codaAccoda, "accoda")}
            >
              {t("action.enqueue")}
            </button>
            {mani !== undefined && (
              <button
                type="button"
                className="bottone minuto btn-ghost"
                onClick={() => aggiungiAPlaylist(scelte)}
              >
                {t("action.addToPlaylist")}
              </button>
            )}
            <button
              type="button"
              className="tasto icon-btn"
              aria-label={t("folders.selection.clear")}
              onClick={() => setSelezione((prima) => ({ chiavi: new Set(), ancora: prima.ancora }))}
            >
              <Icona nome="i-x" dim={14} />
            </button>
          </div>
        )}

        {trascino !== null && (
          <p className="nota-cartelle" role="status">
            {trascino.sopra
              ? t("folders.drag.drop", { n: trascino.quanti })
              : t("folders.drag.hint", { n: trascino.quanti })}
          </p>
        )}

        <div className="scorre-albero" ref={scorreAlbero}>
          <div
            className="albero-cartelle folder-tree"
            data-scorso={finestra.scorso || undefined}
            ref={albero}
            role="tree"
            aria-label={t("folders.aria")}
            aria-multiselectable="true"
            /* Un gestore solo, sul contenitore, e non uno per riga: è la stessa
               regola dell'elenco dei brani, e per la stessa ragione — una prop
               con identità nuova per ogni riga a ogni disegno. */
            onKeyDown={daTastiera}
          >
            {/* Il posto che occupano le righe non disegnate. `presentation`
                perché non sono righe: un `tree` con dentro due `div`
                sconosciuti direbbe di avere due nodi che non ha. */}
            <div
              className="paglia"
              style={{ height: finestra.sopra * finestra.altezza }}
              role="presentation"
            />
            {righe.slice(finestra.primo, finestra.ultimo).map((riga, k) => {
              const i = finestra.primo + k;
              const morta = spenta(riga);
              const file = riga.brano !== null;
              const sceltaRiga = selezione.chiavi.has(chiaveRiga(riga));
              return (
                <div
                  key={riga.percorso}
                  className={
                    file
                      ? "nodo-cartella list-row folder-row nodo-file"
                      : morta
                        ? "nodo-cartella list-row folder-row nodo-spento"
                        : "nodo-cartella list-row folder-row"
                  }
                  role="treeitem"
                  aria-level={riga.livello}
                  /* Solo su una cartella che si apre: su una foglia
                     `aria-expanded` annuncerebbe un comando che non esiste, e
                     chi lo sente lo cerca. */
                  aria-expanded={riga.figli ? riga.aperto : undefined}
                  /* Su ogni riga, vero o falso: vedi il preambolo. */
                  aria-selected={sceltaRiga}
                  /* Il cursore delle frecce, che non è la selezione. */
                  data-fuoco={i === fuoco.attivo || undefined}
                  data-trovato={
                    (filtro !== null && filtro.trovate.has(chiaveRiga(riga))) || undefined
                  }
                  aria-posinset={riga.posizione}
                  aria-setsize={riga.fratelli}
                  aria-disabled={morta || undefined}
                  /* Una sola fermata di tabulazione per tutto l'albero: `fuoco.ts`.
                     La fermata e non la riga attiva: se una rotellata ha portato
                     la riga attiva fuori dalla finestra, lo zero va alla prima
                     in vista, e Tab entra comunque nell'albero. */
                  tabIndex={i === fuoco.fermata ? 0 : -1}
                  /* Il rientro lo decide il foglio a partire dal livello: quanto
                     valga un livello è ritmo, e il ritmo è della skin. */
                  style={conIndice(k, { "--livello-nodo": riga.livello } as CSSProperties)}
                  onFocus={() => segnaFuoco(i)}
                  onPointerDown={(e) => presaVersoCoda(e, riga)}
                  /* Maiusc+clic selezionerebbe anche il testo delle righe in
                     mezzo: il clic serve all'intervallo, non al testo. */
                  onMouseDown={(e) => {
                    if (e.shiftKey) e.preventDefault();
                  }}
                  /* Il clic sulla riga apre anche la cartella, se è chiusa:
                     cliccare sul nome è il gesto che tutti fanno per primo.
                     Solo aprire e non chiudere: il doppio clic che fa suonare
                     passa da due clic, e un'alternanza la chiuderebbe al
                     secondo. Chiudere resta del triangolo e di ←. Con Ctrl o
                     Maiusc il clic sceglie, e basta. */
                  onClick={(e) => {
                    fuoco.vaiA(i);
                    if (e.ctrlKey || e.metaKey) {
                      scegli(i, "alterna");
                      return;
                    }
                    if (e.shiftKey) {
                      scegli(i, "intervallo");
                      return;
                    }
                    scegli(i, "azzera");
                    if (riga.figli && !riga.aperto) apriRiga(riga);
                  }}
                  onDoubleClick={(e) => {
                    if (e.ctrlKey || e.shiftKey || e.metaKey) return;
                    invioIndice(i);
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    fuoco.vaiA(i);
                    menuDi(riga, e.clientX, e.clientY);
                  }}
                >
                  <span
                    className="freccia-nodo"
                    aria-hidden="true"
                    onClick={(e) => {
                      // Il triangolo apre e chiude, e il clic si ferma qui:
                      // senza, lo stesso gesto conterebbe anche come clic sulla
                      // riga.
                      e.stopPropagation();
                      fuoco.vaiA(i);
                      alterna(riga);
                    }}
                  >
                    {riga.figli ? (
                      <Icona nome={riga.aperto ? "i-chev-d" : "i-chev-r"} dim={14} />
                    ) : null}
                  </span>
                  <span className="icona-nodo" aria-hidden="true">
                    <Icona nome={file ? "i-track" : "i-folder"} dim={14} />
                  </span>
                  {/* `dir="auto"`: un nome di cartella o di file in arabo o in
                      ebraico si legge da destra. */}
                  <span className="nome-nodo" title={riga.percorso} dir="auto">
                    {riga.nome}
                  </span>
                  {file
                    ? riga.durataMs !== null &&
                      riga.durataMs > 0 && (
                        <span className="conteggio-nodo">{durata(riga.durataMs)}</span>
                      )
                    : riga.brani > 0 && (
                        <span className="conteggio-nodo">{brani_(riga.brani)}</span>
                      )}
                </div>
              );
            })}
            <div
              className="paglia"
              style={{ height: finestra.sotto * finestra.altezza }}
              role="presentation"
            />
          </div>
        </div>
      </div>

      <section
        className="pannello-cartella"
        aria-label={
          cartellaScelta === null ? t("folders.panel.empty") : t("folders.panel.aria", { nome: nomePannello })
        }
      >
        {cartellaScelta === null ? (
          <p className="vuoto-pannello-cartella">{t("folders.panel.empty")}</p>
        ) : (
          <>
            <div className="testa-pannello-cartella">
              <div className="titolo-pannello-cartella">
                <h2 className="nome-pannello-cartella" title={cartellaScelta} dir="auto">
                  {nomePannello}
                </h2>
                {idsPannello !== null && (
                  <span className="conteggio-nodo">{brani_(ids.length)}</span>
                )}
              </div>
              <div className="comandi-pannello-cartella">
                <span className="interruttore-sottocartelle">
                  <span id={idInterruttore}>{t("folders.panel.subfolders")}</span>
                  <button
                    type="button"
                    className="interruttore switch"
                    role="switch"
                    aria-checked={conSottocartelle}
                    aria-labelledby={idInterruttore}
                    onClick={() => setConSottocartelle((prima) => !prima)}
                  >
                    <span className="pista switch-track" aria-hidden="true">
                      <span className="pallina" />
                    </span>
                  </button>
                </span>
                <button
                  type="button"
                  className="pillola btn-accent"
                  disabled={ids.length === 0 || mani?.suonabile === false}
                  onClick={suonaTuttoIlPannello}
                >
                  <Icona nome="i-play" dim={14} />
                  {t("action.play")}
                </button>
              </div>
            </div>
            <div className="scorre-pannello" ref={scorrePannello}>
              {idsPannello === null ? (
                <ElencoFinto />
              ) : ids.length === 0 ? (
                <p className="vuoto-pannello-cartella">{t("folders.panel.noTracks")}</p>
              ) : (
                <ElencoBrani
                  rada={archivio}
                  chiave={chiavePannello}
                  scorrevole={scorrePannello}
                  inAscolto={mani?.inAscolto ?? null}
                  inPausa={mani?.inPausa ?? false}
                  suonabile={mani?.suonabile ?? true}
                  selezione={sceltaBrani.ids}
                  onSuona={suonaDalPannello}
                  onPreferito={preferito}
                  onVoto={voto}
                  onMenu={menuBrani}
                  onSeleziona={scegliBrano}
                  onAlternaSelezione={alternaBrano}
                  onSelezionaTutto={sceglieTuttiIBrani}
                  /* Senza le sottocartelle la cartella è, di solito, un disco:
                     lì `#` è il numero sulla custodia, come in un album. */
                  numeroTraccia={!conSottocartelle}
                />
              )}
            </div>
          </>
        )}
      </section>

      {daConfermare !== null && (
        <ConfermaCoda
          cosa={daConfermare}
          onChiudi={() => setDaConfermare(null)}
          onConferma={() => {
            const cosa = daConfermare;
            setDaConfermare(null);
            cosa.vai();
          }}
        />
      )}
    </div>
  );
}

/**
 * «Sono cinquemila: sicuro?»
 *
 * Una finestrella qui dentro e non un componente condiviso perché la domanda è
 * di questo pannello: è l'unico posto dell'applicazione in cui un gesto solo
 * può mettere in coda una libreria intera. Il comportamento — il fuoco che
 * entra, la trappola del Tab, Escape, il fuoco che torna a chi l'aveva, il
 * resto della finestra inerte — viene tutto da `Dialogo`, ed è il motivo per
 * cui questo componente è venti righe invece di centocinquanta.
 *
 * # La frase dice quel che succede davvero
 *
 * Fino alla 3.0 diceva «questa cartella… prende il posto della coda» a ogni
 * gesto, anche ad «Accoda» e a «Riproduci dopo», che la coda non la toccano —
 * e anche a una selezione di più righe, che una cartella non è. Una conferma
 * che descrive un altro gesto insegna a non leggerla. Adesso le frasi sono sei:
 * tre gesti, per una cartella sola o per più elementi scelti. L'«Annulla»
 * dell'avviso si promette solo dove c'è, cioè quando la coda si sostituisce.
 */
function ConfermaCoda({
  cosa,
  onConferma,
  onChiudi,
}: {
  cosa: DaConfermare;
  onConferma: () => void;
  onChiudi: () => void;
}) {
  const valori = { n: cosa.brani, righe: cosa.righe };
  const una = cosa.righe <= 1;
  const frase =
    cosa.gesto === "sostituisce"
      ? una
        ? t("folders.confirm.replace.folder", valori)
        : t("folders.confirm.replace.selection", valori)
      : cosa.gesto === "accoda"
        ? una
          ? t("folders.confirm.enqueue.folder", valori)
          : t("folders.confirm.enqueue.selection", valori)
        : una
          ? t("folders.confirm.next.folder", valori)
          : t("folders.confirm.next.selection", valori);
  return (
    <Dialogo
      titolo={t("folders.confirm.title")}
      descrizione={frase}
      ruolo="alertdialog"
      classe="finestrella stretta glass-modal"
      onChiudi={onChiudi}
    >
      <div className="tasti-finestrella">
        <button type="button" className="bottone" onClick={onChiudi}>
          {t("common.cancel")}
        </button>
        <button type="button" className="bottone primario" onClick={onConferma}>
          {t("folders.confirm.ok")}
        </button>
      </div>
    </Dialogo>
  );
}
