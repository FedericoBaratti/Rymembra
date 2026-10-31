/**
 * Lo stato della riproduzione, dal lato della finestra.
 *
 * Non decide niente: il nucleo manda `riproduzione:stato` a ogni cambiamento e
 * questo modulo lo conserva. L'unica cosa che aggiunge è il tempo fra un colpo
 * e l'altro, per il motivo scritto sotto.
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import { creaArchivio, useArchivio, type Leggibile } from "./archivio";
import {
  ipc,
  type Brano,
  type ErroreIpc,
  type GuastoAudio,
  type StatoEq,
  type StatoRiproduzione,
  type Tempo,
} from "./ipc";
import { ETA, FINESTRA, SALTO, creaOrologio } from "./orologio";
import { useAscolto } from "./pagine";
import { useDati } from "./dati";
import { ARCHIVIO_FINESTRA } from "./posizione";
import { eventiEFotografia } from "./ultima";

/**
 * Ogni quanto la posizione interpolata arriva a React.
 *
 * Il nucleo manda la posizione quattro volte al secondo — `PASSO_TEMPO` in
 * `riproduzione/mod.rs` — e lascia alla finestra il compito di riempire i
 * buchi: sessanta eventi al secondo attraverso l'IPC sarebbero sessanta
 * serializzazioni per spostare un pixel.
 *
 * Riempirli a ogni fotogramma però sarebbe lo stesso spreco spostato di un
 * lato: sessanta render al secondo per una barra che su un brano di quattro
 * minuti avanza di due pixel al secondo. A 50 ms il passo è un decimo di pixel,
 * sotto la soglia di ciò che si vede e sopra quella di ciò che costa.
 */
const PASSO_INTERPOLAZIONE = 50;

/** Nessun brano, nessuna coda: com'è prima che il nucleo risponda. */
const FERMO: StatoRiproduzione = {
  brano: null,
  inPausa: true,
  posizioneMs: 0,
  durataMs: 0,
  shuffle: false,
  ripeti: "off",
  volume: 1,
  muto: false,
  coda: [],
  posizioneCoda: null,
  eqAttivo: false,
  eqGuadagni: [],
  // `normale`, come il motore: il valore di ripiego finché il nucleo non
  // risponde deve dire quel che sta succedendo davvero, non la posizione più
  // prudente.
  replaygain: "normale",
  spegnimentoMs: null,
  autoplay: false,
  dissolvenzaS: 0,
  latenzaMs: 0,
  // Zero **non** è il valore vero dell'anticipo: è «il nucleo non l'ha ancora
  // detto». Scriverlo qui sarebbe rimettere a mano la copia del numero che
  // questo giro è venuto a togliere — vedi `anticipoAdesso` — e nei pochi
  // millisecondi prima della prima risposta non anticipare niente è invisibile,
  // mentre anticipare di un numero inventato qui durerebbe per sempre.
  anticipoMs: 0,
  audio: null,
  motivoProssimo: null,
  uscita: null,
  // Niente, perché non c'è nessun brano: la riga dei dati tecnici si disegna
  // se e solo se il dato è arrivato, e finché il nucleo non risponde non è
  // arrivato niente.
  formato: null,
  // Fuori sessione finché il nucleo non dice il contrario.
  insieme: null,
};

// ── L'anticipo dei testi, fuori da React ────────────────────────────────────
//
// Lo stesso schema della posizione qui sotto, e per una ragione più semplice: il
// pannello dei testi cerca la riga accesa a ogni disegno, cioè venti volte al
// secondo, e il numero gli serve **dentro** quel calcolo. Farlo scendere per
// prop lo farebbe passare da `App`, che è precisamente la catena che l'archivio
// esterno della posizione esiste per non risvegliare.
//
// È `rymembra_domain::testo::ANTICIPO_MS`, e arriva dentro `riproduzione:stato`:
// fino a ieri stava scritto due volte, qui a mano e nel dominio, e la copia è
// durata finché nessuno ha toccato nessuna delle due.

let anticipoCorrente = 0;

/**
 * Di quanto una riga di testo si accende prima del suo tempo, in millisecondi.
 *
 * Zero finché il nucleo non ha risposto, e zero **vuol dire** «non anticipare»:
 * non c'è nessun ripiego scritto qui, perché un ripiego scritto qui sarebbe la
 * seconda copia del numero.
 */
export function anticipoAdesso(): number {
  return anticipoCorrente;
}

// ── La posizione, fuori da React ────────────────────────────────────────────
//
// # Perché non è uno `useState`
//
// Lo era, e stava in `App`. Venti volte al secondo — `PASSO_INTERPOLAZIONE` —
// quello stato cambiava, e con lui si rifaceva `App` **per intero**: `testa()`,
// `corpo()` che costruisce le duecento righe dell'elenco, l'oggetto `slot`, e
// `contesto`, il cui `useMemo` aveva `posizioneMs` fra le dipendenze e quindi
// non serviva a niente. React evitava di riscrivere il DOM, ma riconciliava
// duecento righe per otto elementi venti volte al secondo per muovere una barra
// di avanzamento — mentre `disegno-ux.md §9` dichiara che «nessun componente
// deve chiedere niente a quel ritmo».
//
// Con un archivio esterno si ridisegna **solo** chi la legge, che è una foglia
// sola: `Scrubber`. Tutto il resto della catena — `Lettore`, `Colonna`,
// `InRiproduzione`, `Impaginazione`, `App` — non la vede più passare.
//
// Di riproduzioni ce n'è una per finestra: `useRiproduzione` si chiama una
// volta sola, in `App`, ed è quella chiamata che alimenta l'archivio della
// finestra (`posizione.ts`). Chi disegna la posizione però lo legge dal
// contesto `Dati`, perché nell'anteprima dello Studio la posizione è quella
// di un trasporto finto, e la barra di avanzamento di un tema che si sta
// disegnando non deve correre col brano che si sta ascoltando.

function pubblica(ms: number): void {
  ARCHIVIO_FINESTRA.pubblica(ms);
}

/**
 * Porta una posizione stimata al passo con cui si pubblica.
 *
 * Il tetto e la quantizzazione in una funzione sola perché i due posti che
 * pubblicano — il ciclo dei fotogrammi e l'arrivo di un colpo — devono accordarsi
 * al millisecondo: se uno quantizza e l'altro no, la barra riceve un valore fuori
 * passo a ogni colpo del nucleo, cioè quattro scatti al secondo dentro un
 * meccanismo fatto per non averne.
 */
function alPasso(ms: number, durataMs: number): number {
  const limitata = durataMs > 0 ? Math.min(ms, durataMs) : ms;
  return Math.round(limitata / PASSO_INTERPOLAZIONE) * PASSO_INTERPOLAZIONE;
}

/**
 * La posizione **adesso**, senza iscriversi.
 *
 * Per chi la legge in risposta a un gesto invece che per disegnarla: le frecce
 * della tastiera vogliono sapere da dove saltare, non essere svegliate ogni
 * cinquanta millisecondi.
 */
export function posizioneAdesso(): number {
  return ARCHIVIO_FINESTRA.leggi();
}

/**
 * Segue la posizione interpolata.
 *
 * La chiama **solo** chi la disegna. Chiamarla in un componente che la passa
 * più in basso rimetterebbe esattamente il difetto che questo archivio esiste
 * per togliere.
 */
export function usePosizioneMs(): number {
  const { posizione } = useDati();
  return useSyncExternalStore(posizione.iscrivi, posizione.leggi, posizione.leggi);
}

/**
 * Segue la posizione interpolata, ma si ridisegna solo ogni `passoMs`.
 *
 * Per chi la disegna con una grana più grossa dei 50 ms con cui si pubblica:
 * la barra di avanzamento, che sceglie il passo con `passoDellaBarra`
 * (`posizione.ts`, dove sta il perché e la misura). L'iscrizione è la stessa
 * di [`usePosizioneMs`]. Cambia la fotografia, arrotondata per difetto al
 * passo: React confronta le fotografie e salta i colpi che non la cambiano.
 * Per difetto, così il secondo scritto cambia quando cambia quello vero e non
 * prima. Con `passoMs` a 50 è [`usePosizioneMs`], perché la posizione pubblicata
 * è già un multiplo di 50.
 */
export function usePosizioneAlPasso(passoMs: number): number {
  const { posizione } = useDati();
  const leggi = useCallback(
    () => Math.floor(posizione.leggi() / passoMs) * passoMs,
    [posizione, passoMs],
  );
  return useSyncExternalStore(posizione.iscrivi, leggi, leggi);
}

// ── Lo stato, fuori da React ────────────────────────────────────────────────
//
// La stessa mossa della posizione, un gradino più su. Lo stato del motore
// stava in uno `useState` di questo hook, cioè di `App`: un passo del cursore
// del volume, una pausa, una banda dell'equalizzatore, l'attesa dei dati che
// va e viene — ognuno rifaceva la finestra intera, libreria e Impostazioni
// comprese, per cambiare un'icona nella barra. Adesso sta in un archivio
// (`archivio.ts`), e lo legge chi lo disegna: i widget dello scafale
// (`Impaginazione.tsx`), lo schermo intero, l'avviso dell'audio, la scheda
// della riproduzione nelle Impostazioni. `App` ne sceglie soltanto se c'è un
// brano, che cambia quando si comincia o si smette di ascoltare; chi deve
// reagire a un brano nuovo senza disegnarlo — la Home che si rilegge,
// l'accento della copertina — si iscrive all'archivio in un effetto, e la
// durata per le frecce della tastiera si legge al tasto.

/** Lo stato della riproduzione, da leggere e da seguire: vedi sopra. */
export type ArchivioLettore = Leggibile<StatoRiproduzione>;

/**
 * L'archivio dello stato della finestra, per chi disegna più in basso.
 *
 * Di serie un archivio fermo, che non cambia mai: fuori da `App` — nei
 * frammenti dello Studio che non lo forniscono — si disegna il lettore senza
 * brano, che è quel che `FERMO` dice.
 */
export const LettoreDellaFinestra = createContext<ArchivioLettore>(creaArchivio(FERMO));

/**
 * Segue lo stato della riproduzione, o la parte che se ne sceglie.
 *
 * Con una scelta che risponde con un numero o un booleano il componente si
 * ridisegna solo quando quel numero cambia: è il modo di chiedere «c'è un
 * brano?» senza essere svegliati da ogni passo del volume. Vedi `useArchivio`.
 */
export function useLettore(): StatoRiproduzione;
export function useLettore<S>(
  scegli: (stato: StatoRiproduzione) => S,
  uguali?: (a: S, b: S) => boolean,
): S;
export function useLettore<S>(
  scegli?: (stato: StatoRiproduzione) => S,
  uguali?: (a: S, b: S) => boolean,
): StatoRiproduzione | S {
  const archivio = useContext(LettoreDellaFinestra);
  // Lo stesso hook nei due rami, chiamato una volta sola: l'ordine degli hook
  // del componente non cambia, qualunque ramo si prenda.
  return scegli === undefined
    ? useArchivio(archivio)
    : useArchivio(archivio, scegli, uguali);
}

/** Quel che la finestra sa della riproduzione. */
export interface Riproduzione {
  /**
   * L'ultimo stato mandato dal nucleo, in un archivio: vedi «Lo stato, fuori
   * da React» qui sopra. Un'identità sola per tutta la vita della finestra.
   */
  archivio: ArchivioLettore;
  /**
   * Il dispositivo audio si è aperto.
   *
   * Quando è `false` non c'è niente da comandare: `StatoLettore::avvia`
   * conserva il guasto invece di far cadere l'avvio, e l'applicazione resta un
   * catalogo consultabile. La barra si toglie di mezzo e il motivo si legge
   * nell'errore.
   */
  disponibile: boolean;
  /**
   * L'ultimo guasto della riproduzione.
   *
   * `unknown` e non `ErroreIpc`: dal canale degli eventi arriva sempre un
   * record del nucleo, ma dal rifiuto di `riproduzioneStato` può arrivare
   * qualunque cosa, e `testoErrore` sa già leggere entrambi. Dichiararlo
   * `ErroreIpc` sarebbe una promessa che questo modulo non può mantenere.
   */
  errore: unknown;
  /** Scarta l'errore mostrato. */
  scartaErrore: () => void;
  /**
   * Ritocca il brano corrente senza aspettare il nucleo.
   *
   * # Perché serve
   *
   * Perché cuore e stelle si scrivono sul **brano**, e il brano che suona qui
   * dentro è una copia che il nucleo compone quando ha una notizia sua da dare:
   * `costruisci_stato` rilegge la riga dal database, ma lo fa in risposta a una
   * pausa, a un brano nuovo, a un salto — non a un cuoricino. Fra un evento e
   * l'altro la barra continuava a mostrare il valore di quando il brano era
   * partito, e la prima pausa lo faceva saltare al valore vero: acceso o spento
   * a seconda di com'era, cioè un cuore che si metteva e si toglieva da solo a
   * ogni play/pausa.
   *
   * Gli elenchi non hanno questo problema perché `App` li aggiorna sul posto;
   * questa è la stessa cura per l'unica copia che `App` non possiede.
   *
   * Non tocca niente se l'identificativo non è quello che sta suonando.
   */
  ritoccaBrano: (id: number, campi: Partial<Brano>) => void;
}

/** Segue la riproduzione per tutta la vita della finestra. */
export function useRiproduzione(): Riproduzione {
  // Lo stato com'è arrivato dal nucleo, l'attesa dei dati, e quel che se ne
  // mostra: tre archivi e non tre `useState`, per la ragione scritta in «Lo
  // stato, fuori da React». Il terzo si ricompone da sé quando cambia uno dei
  // primi due, e solo allora — è il `useMemo` che stava qui, con la stessa
  // identità quando niente di quel che mostra è cambiato. In pausa non si
  // aspetta niente: la scritta non deve sopravvivere a chi ha fermato la
  // musica mentre aspettava.
  const [archivi] = useState(() => {
    const grezzo = creaArchivio<StatoRiproduzione>(FERMO);
    const inAttesa = creaArchivio(false);
    const visto = creaArchivio<StatoRiproduzione>(FERMO);
    const ricomponi = () => {
      const stato = grezzo.leggi();
      visto.scrivi(
        inAttesa.leggi() && !stato.inPausa ? { ...stato, inAttesa: true } : stato,
      );
    };
    grezzo.iscrivi(ricomponi);
    inAttesa.iscrivi(ricomponi);
    return { grezzo, inAttesa, visto };
  });
  const setStato = archivi.grezzo.scrivi;
  const setInAttesa = archivi.inAttesa.scrivi;
  const [disponibile, setDisponibile] = useState(true);
  const [errore, setErrore] = useState<unknown>(null);

  // L'orologio che tiene l'origine del brano. In un `useRef` e non in un
  // `useMemo`: porta dentro la storia degli ancoraggi, e un `useMemo` React può
  // ricalcolarlo quando gli pare — perderebbe la finestra dei campioni senza che
  // niente lo dica. Il perché del meccanismo sta in `./orologio`.
  const orologio = useRef(
    creaOrologio({ finestra: FINESTRA, eta: ETA, salto: SALTO }),
  );

  // Quel che il ciclo dei fotogrammi deve sapere e l'orologio non dice: se si è
  // fermi, e dove finisce il brano. In un ref e non nello stato, perché lo legge
  // il ciclo e non deve far ridisegnare niente per sapere che ora è.
  const ancora = useRef({ inPausa: true, durataMs: 0 });

  // Quale brano era, per riconoscere il cambio. Un brano nuovo è un'origine
  // nuova, e la storia di prima parlava di un'altra canzone.
  const branoPrima = useRef<number | null>(null);

  // Il prossimo passo del ciclo in attesa, o `0` quando il ciclo è spento.
  const prossimo = useRef(0);

  // Si aspettano dati che non arrivano: l'ultimo colpo d'orologio l'ha detto.
  // Il ref per l'ancoraggio di uno stato intero, che non lo porta e non deve
  // far ripartire il cursore per un quarto di secondo; l'archivio per chi lo
  // disegna. Vedi `Tempo.inAttesa`.
  const attesa = useRef(false);

  /**
   * Accende il ciclo che pubblica la posizione stimata, se non è già acceso.
   *
   * # Perché si spegne invece di girare a vuoto
   *
   * Perché un ciclo che si riarma comunque sveglia la finestra per **tutta la
   * sua vita**, anche con niente in riproduzione — cioè quasi sempre. Prima
   * l'uscita per la pausa stava dopo il riarmo, quindi il ciclo non si fermava
   * mai: costava una sveglia a ogni giro per non fare niente.
   *
   * # Perché un timer e non `requestAnimationFrame`
   *
   * Si pubblica a [`PASSO_INTERPOLAZIONE`], venti volte al secondo, e un
   * `requestAnimationFrame` gira a sessanta: due giri su tre non avevano niente
   * da pubblicare, e ognuno chiedeva comunque un fotogramma al compositore.
   * Misurato su `prima.exe` (30/10, musica ferma, ciclo iniettato): il solo
   * riarmo a vuoto costava l'1,2% di un core nel renderer e lo 0,3% nel
   * processo della GPU. Un timer al passo sveglia la finestra solo quando c'è
   * un numero nuovo. A finestra nascosta il browser lo rallenta da sé, e i
   * colpi del nucleo continuano ad ancorare la posizione quattro volte al
   * secondo.
   *
   * È la stessa disciplina per cui il nucleo manda la posizione quattro volte al
   * secondo e non sessanta, e per cui questo modulo esiste (`disegno-ux.md §9`:
   * «nessun componente deve chiedere niente a quel ritmo»).
   */
  const accendi = useCallback(() => {
    if (prossimo.current !== 0) return;
    let ultima = -1;
    const passo = () => {
      const { inPausa, durataMs } = ancora.current;
      // Fermo: la posizione è quella dell'ancora, e l'ha già scritta
      // `ancoraggio`. Non ci si riarma — riaccende lei quando riparte.
      if (inPausa) {
        prossimo.current = 0;
        return;
      }
      prossimo.current = window.setTimeout(passo, PASSO_INTERPOLAZIONE);
      // Dall'orologio e non dall'ultimo colpo: `posizione + (adesso − arrivo)`
      // portava dentro il ritardo di consegna di **quel** colpo, e siccome quel
      // ritardo varia la stima scattava indietro a ogni messaggio arrivato
      // tardi. Vedi `./orologio`.
      const quantizzata = alPasso(
        orologio.current.stima(performance.now()),
        durataMs,
      );
      if (quantizzata !== ultima) {
        ultima = quantizzata;
        pubblica(quantizzata);
      }
    };
    prossimo.current = window.setTimeout(passo, PASSO_INTERPOLAZIONE);
  }, []);

  /**
   * Un colpo del nucleo arriva all'orologio.
   *
   * `nuovoBrano` lo sa solo chi guarda lo stato intero: i colpi dell'orologio
   * portano il tempo e non il brano. La **transizione di pausa** invece si
   * riconosce da qui, e vale come un brano nuovo: durante la pausa il tempo di
   * parete avanza e la posizione no, quindi ogni campione di prima dichiarerebbe
   * un'origine troppo indietro e la ripresa partirebbe avanti.
   */
  const ancoraggio = useCallback(
    (tempo: Tempo, nuovoBrano: boolean) => {
      const adesso = performance.now();
      // In attesa dei dati la musica è ferma quanto in pausa, e per il cursore
      // vale lo stesso: correre da solo vorrebbe dire andare avanti su un
      // brano fermo e tornare indietro di colpo al colpo dopo. E la ripresa è
      // una transizione come dopo una pausa: l'origine di prima è vecchia
      // quanto l'attesa.
      const fermo = tempo.inPausa || tempo.inAttesa === true;
      const transizione = fermo !== ancora.current.inPausa;
      orologio.current.ancora(tempo.posizioneMs, adesso, nuovoBrano || transizione);
      ancora.current = {
        inPausa: fermo,
        durataMs: tempo.durataMs,
      };
      // Mentre suona si pubblica la **stima**, non il numero arrivato: quel
      // numero porta dentro il ritardo di consegna di questo colpo, ed è
      // precisamente lo scatto indietro che l'orologio esiste per togliere.
      // Pubblicarlo qui dopo averlo scartato là vorrebbe dire togliere lo
      // strappo dall'interpolazione e rimetterlo a ogni messaggio.
      //
      // In pausa invece vale il numero arrivato, e non è un'incoerenza: la stima
      // è «origine più tempo di parete», e in pausa il tempo di parete scorre
      // mentre la musica no.
      pubblica(
        fermo
          ? alPasso(tempo.posizioneMs, tempo.durataMs)
          : alPasso(orologio.current.stima(adesso), tempo.durataMs),
      );
      // Riparte da qui, e solo da qui: è l'unico posto che sa che si è tornati
      // a suonare.
      if (!fermo) accendi();
    },
    [accendi],
  );

  /**
   * Quel che si fa di uno stato intero: l'anticipo, il brano, e poi l'ancora.
   *
   * In una funzione sola perché i due posti che ricevono uno stato intero — la
   * prima lettura e l'evento — devono fare le stesse tre cose, e due copie di tre
   * righe sono due occasioni di dimenticarne una.
   */
  const accogli = useCallback(
    (carico: StatoRiproduzione) => {
      anticipoCorrente = carico.anticipoMs;
      const idOra = carico.brano?.id ?? null;
      const nuovoBrano = idOra !== branoPrima.current;
      branoPrima.current = idOra;
      // Un brano nuovo non eredita l'attesa di quello di prima; lo stesso
      // brano sì, finché il colpo d'orologio dopo non dice altro.
      if (nuovoBrano) {
        attesa.current = false;
        setInAttesa(false);
      }
      ancoraggio({ ...carico, inAttesa: attesa.current }, nuovoBrano);
    },
    [ancoraggio],
  );

  // Lo stato all'apertura. La coda di ieri è già stata ripresa dal nucleo in
  // `riprendi_coda`, ma la finestra non c'era: senza questa chiamata la barra
  // resterebbe vuota fino al primo comando.
  //
  // Chiesta nel `poi` dell'ascolto, cioè ad ascolto aperto, e non in un effetto
  // a parte che partiva prima: un `riproduzione:stato` emesso nel mezzo — le
  // cuffie Bluetooth che si collegano all'avvio e il motore che si riapre —
  // non trovava nessuno ad ascoltarlo, e la fascia «non c'è audio» restava
  // finché l'utente non faceva qualcosa. E la risposta si adotta solo se nel
  // frattempo non è arrivato nessun evento: calcolata prima, posata sopra un
  // evento più nuovo lo cancellerebbe. Vedi `poi` in `pagine.ts` e
  // `eventiEFotografia` in `ultima.ts`.
  const [conto] = useState(eventiEFotografia);
  const vivo = useRef(true);
  useEffect(() => {
    vivo.current = true;
    return () => {
      vivo.current = false;
    };
  }, []);
  const fotografia = () => {
    const valida = conto.chiedi();
    ipc
      .riproduzioneStato()
      .then((iniziale) => {
        if (!vivo.current || !valida()) return;
        setStato(iniziale);
        accogli(iniziale);
      })
      .catch((e: unknown) => {
        if (!vivo.current) return;
        setDisponibile(false);
        setErrore(e);
      });
  };

  useAscolto<StatoRiproduzione>(
    "riproduzione:stato",
    (carico) => {
      conto.evento();
      setStato(carico);
      accogli(carico);
    },
    true,
    fotografia,
  );

  // I colpi dell'orologio non cambiano brano: il nucleo manda `riproduzione:stato`
  // quando cambia, e lì il confronto si fa. Qui `false` non è una semplificazione
  // — è quel che rende questi colpi i campioni **onesti** su cui l'orologio
  // costruisce la sua finestra.
  //
  // L'attesa dei dati arriva solo da qui, e sveglia qualcuno solo quando
  // cambia: `setInAttesa` con lo stesso valore non avvisa nessuno, e quattro
  // colpi al secondo che ridisegnassero lo scafale per ripetere «no»
  // sarebbero il difetto che l'archivio della posizione esiste per togliere.
  useAscolto<Tempo>("riproduzione:tempo", (carico) => {
    attesa.current = carico.inAttesa === true;
    ancoraggio(carico, false);
    setInAttesa(attesa.current);
  });

  // La curva da sola. Non tocca l'ancora del tempo: il nucleo la manda
  // proprio per non dover comporre lo stato intero a ogni cursore mosso, e
  // riancorare qui rifarebbe il lavoro dall'altro lato.
  useAscolto<StatoEq>("riproduzione:eq", (carico) =>
    setStato((prima) => ({
      ...prima,
      eqAttivo: carico.attivo,
      eqGuadagni: carico.guadagni,
    })),
  );

  // Il volume da solo, per la stessa ragione della curva: arriva a ogni passo
  // del cursore, e comporre lo stato intero voleva dire una lettura del brano
  // dal database per ogni passo.
  useAscolto<{ volume: number; muto: boolean }>("riproduzione:volume", (carico) =>
    setStato((prima) =>
      prima.volume === carico.volume && prima.muto === carico.muto
        ? prima
        : { ...prima, volume: carico.volume, muto: carico.muto },
    ),
  );

  useAscolto<ErroreIpc>("riproduzione:errore", setErrore);

  // Il dispositivo sparito. Arriva **una volta** — l'orologio annuncia il
  // passaggio, non lo stato — e va nello stato invece che fra gli errori:
  // non è un'operazione fallita da mostrare e poi scordare, è una
  // condizione che dura finché qualcuno non riapre.
  useAscolto<GuastoAudio>("riproduzione:audio", (carico) =>
    setStato((prima) => ({ ...prima, audio: carico, inPausa: true })),
  );

  // Il ciclo non si avvia qui: lo accende `ancoraggio` quando il nucleo dice
  // che si sta suonando. Questo effetto esiste solo per spegnerlo alla chiusura
  // della finestra, che è l'unico caso in cui il ciclo può restare acceso senza
  // che nessuno lo fermi.
  useEffect(
    () => () => {
      if (prossimo.current !== 0) {
        window.clearTimeout(prossimo.current);
        prossimo.current = 0;
      }
    },
    [],
  );

  const scartaErrore = useCallback(() => setErrore(null), []);

  const ritoccaBrano = useCallback((id: number, campi: Partial<Brano>) => {
    setStato((prima) =>
      prima.brano === null || prima.brano.id !== id
        ? prima
        : { ...prima, brano: { ...prima.brano, ...campi } },
    );
  }, []);

  return {
    archivio: archivi.visto,
    disponibile,
    errore,
    scartaErrore,
    ritoccaBrano,
  };
}
