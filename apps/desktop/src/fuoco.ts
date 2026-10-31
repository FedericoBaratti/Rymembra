/**
 * Il fuoco a rotazione: una fermata di tabulazione per un elenco intero.
 *
 * # Il conto che questo modulo chiude
 *
 * Ogni riga dell'elenco espone **sette** fermate di tabulazione: l'indice, le
 * cinque stelle, il cuore — e dentro una playlist anche la × che la toglie.
 * Sono sette per riga, e le righe su una libreria vera sono 18.534: oltre
 * **centomila pressioni di Tab** per attraversare l'elenco, se si arriva in
 * fondo. Non è scomodo, è assente: chi naviga da tastiera non può raggiungere né
 * la riga dopo né quel che viene **dopo l'elenco**, perché la strada per uscirne
 * è più lunga di quanto una persona stia a premere un tasto.
 *
 * La forma prevista per questo caso ha un nome ed è vecchia quanto le griglie:
 * **una** fermata di tabulazione per il componente, e dentro si gira con le
 * frecce. La riga attiva porta `tabIndex={0}`, tutte le altre `-1`, e i comandi
 * dentro le righe passano tutti a `-1` — non sono irraggiungibili, si
 * raggiungono con ←→ invece che con Tab. Da oltre centomila fermate a una.
 *
 * # Due assi, e perché non uno
 *
 * ↑↓ cambiano riga, ←→ girano fra i comandi **della riga col fuoco**. Un asse
 * solo — ↑↓ su tutto, comandi compresi — metterebbe nella stessa fila la riga
 * successiva e la terza stella della riga corrente, cioè due cose che non si
 * scelgono mai nello stesso momento: scorrere un elenco e votare un brano.
 *
 * # Perché lo scorrimento viene prima del fuoco
 *
 * Con una finestra virtuale (`virtuale.ts`) la riga a cui si vuole andare può non
 * avere un nodo nel DOM: premendo Fine su diciottomila brani la riga 18.533 non
 * è disegnata. `focus()` su un nodo che non c'è non fa niente e non dà errore —
 * il fuoco resta dov'era, o cade sul `<body>`, che è il modo peggiore di
 * sbagliare perché non si vede. Quindi prima si porta lo scorrevole al posto
 * giusto, si lascia che il disegno arrivi, e **poi** si focalizza: è un effetto
 * che passa di qui due volte, e la seconda trova il nodo.
 *
 * # Generico perché lo consuma anche il pannello «Cartelle»
 *
 * Come `virtuale.ts`: le righe qui sono `.riga`, là `.nodo-cartella`, e i due
 * selettori sono parametri proprio perché non ci siano due copie di questa
 * logica da tenere d'accordo.
 *
 * # Le griglie: due assi veri
 *
 * In una griglia di schede (gli album, gli artisti) ogni cella è un comando
 * solo, e i due assi diventano quelli della griglia: ↑↓ di una riga, ←→ di una
 * cella, Inizio e Fine al primo e all'ultimo. Il conto sta in `passoGriglia`
 * (`finestra.ts`), puro e provato; qui c'è solo chi lo chiama. Le colonne le
 * dice chi disegna, che le ha misurate: sono quelle di `useVirtuale`.
 *
 * Una fermata di tabulazione per griglia anche qui: novecento schede erano
 * novecento pressioni di Tab per arrivare a quel che viene dopo.
 *
 * # Le pagine, e i due capi
 *
 * PagSu e PagGiù spostano il fuoco di una schermata — le righe che entrano
 * nello scorrevole meno una, perché la riga da cui si parte resti in vista e
 * si sappia dove si è arrivati — e in una griglia restano sulla stessa
 * colonna. Inizio e Fine portano al primo e all'ultimo della vista **intera**,
 * anche a cinquantamila righe: la riga d'arrivo non c'è nel DOM, e ci arriva
 * con lo scorrimento (vedi sopra). `Ctrl+Inizio` e `Ctrl+Fine` fanno lo
 * stesso: sono i tasti che il modello delle griglie ARIA dà per i due capi, e
 * chi li prova non deve trovare un tasto morto.
 *
 * # La fermata, quando la riga attiva non c'è
 *
 * La riga attiva può uscire dalla finestra — una rotellata la porta via — e
 * allora nel DOM non c'è nessuna riga con `tabIndex={0}`: Tab salterebbe
 * l'elenco intero. `fermata` è la riga che porta lo zero: quella attiva se è
 * disegnata, altrimenti la prima che si vede. Entrandoci con Tab diventa lei
 * l'attiva (`segna`); tornando con lo scorrimento sulla vecchia, lo zero torna
 * là. L'elenco dei brani non ne ha bisogno — tiene montata la riga attiva —
 * ma la coda, l'albero di «Cartelle» e le griglie sì.
 *
 * # Quale riga è questa
 *
 * Chi scrive `data-indice` sulle righe può disegnarle fuori ordine — l'elenco
 * dei brani tiene montata la riga col fuoco quando esce dalla finestra — e
 * allora la posizione nel DOM non dice quale riga sia: si legge l'attributo.
 * Chi non lo scrive disegna solo la finestra, e la posizione basta.
 */
import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent, RefObject } from "react";

import { passoGriglia } from "./finestra";
import { indiceDi, type Finestra } from "./virtuale";

/**
 * Per quanto aspettare che una riga entri nella finestra, prima di lasciar
 * perdere.
 *
 * Serve un tetto: se lo scorrevole è già in fondo e la riga chiesta resta fuori
 * — un elenco più corto di quanto si credeva, un'altezza appena rimisurata — il
 * fuoco in sospeso resterebbe in sospeso per sempre e proverebbe a prendersi la
 * scena a ogni disegno successivo, anche mezz'ora dopo.
 *
 * È un tempo e **non un numero di disegni**, ed è il punto: gli elenchi che
 * usano questo modulo non si ridisegnano a un ritmo fisso, ma quando arriva una
 * pagina, si sposta la finestra o cambia una riga — la posizione di quel che
 * suona sta fuori da React (`riproduzione.ts`) e non li ridisegna. Un tetto
 * contato in disegni non ha quindi nessun rapporto col tempo: su un elenco
 * fermo potrebbe non scadere mai, e lasciare la riga in sospeso per sempre;
 * durante una raffica di pagine scadrebbe prima che lo scorrimento e il
 * ricalcolo abbiano finito. Trecento millisecondi sono venti fotogrammi:
 * abbastanza per uno scorrimento e la rimisura, troppo pochi perché qualcuno se
 * ne accorga.
 */
const ATTESA_MS = 300;

/**
 * La riga `dove` è fuori dalla finestra disegnata, e `vaiA` deve scorrere.
 *
 * Fuori vuol dire fuori da `[primo, ultimo)`: la riga col fuoco può restare
 * montata anche lontano dalla finestra (la vista Brani la tiene), ma non è
 * dentro quel che si vede.
 */
export function daScorrere(dove: number, finestra: Pick<Finestra, "primo" | "ultimo">): boolean {
  return dove < finestra.primo || dove >= finestra.ultimo;
}

/** Il fuoco dentro un elenco, e i modi di spostarlo. */
export interface Fuoco {
  /**
   * La riga che porta il fuoco: la **sola** con `tabIndex={0}`.
   *
   * È un indice nell'elenco intero, non nella finestra disegnata: la riga attiva
   * può essere fuori dallo schermo, e resta quella a cui Tab riporta.
   */
  attivo: number;
  /**
   * La riga con `tabIndex={0}` adesso: `attivo` se è disegnata, la prima in
   * vista se no. Vedi «La fermata» nella nota del modulo.
   */
  fermata: number;
  /**
   * Porta il fuoco su una riga, scorrendo prima se è fuori dalla finestra.
   *
   * Lo chiamano le frecce, Inizio/Fine e `Alt+↑↓`: quest'ultimo sposta una riga
   * dentro una playlist e il fuoco deve seguire la riga spostata, non restare
   * sulla posizione.
   */
  vaiA: (indice: number) => void;
  /**
   * Registra che il fuoco è **già** arrivato su una riga, senza spostarlo.
   *
   * Lo chiama `onFocus` della riga. Serve perché il fuoco può arrivare senza
   * passare da qui — un clic, un Tab da fuori — e se `attivo` non lo seguisse la
   * riga col fuoco e la riga con `tabIndex={0}` sarebbero due righe diverse:
   * uscire e rientrare con Tab riporterebbe altrove.
   */
  segna: (indice: number) => void;
  /** Da appendere al contenitore delle righe: è lui che legge i tasti. */
  daTastiera: (e: KeyboardEvent<HTMLElement>) => void;
}

/**
 * Il fuoco a rotazione su un elenco finestrato.
 *
 * Un gestore solo, sul contenitore, e non uno per riga: le righe sono `memo` e
 * una prop nuova per riga con un'identità nuova a ogni disegno annullerebbe il
 * confronto, che è il secondo conto che questa release paga. La riga da cui
 * viene il tasto si ricava dal bersaglio dell'evento — `closest` — e la sua
 * posizione nella finestra dice l'indice.
 */
export function useFuoco(opzioni: {
  /** Quante righe ha l'elenco in tutto. */
  totale: number;
  /**
   * L'elenco dentro lo scorrevole.
   *
   * Lo scorrevole non serve: a muoverlo ci pensa `scorriA`, che lo conosce già.
   * Chiederlo anche qui sarebbe un secondo riferimento da tenere d'accordo col
   * primo per niente.
   */
  ancora: RefObject<HTMLElement | null>;
  /** Cos'è disegnato adesso: da `useVirtuale`. */
  finestra: Finestra;
  /** Da `useVirtuale`: porta lo scorrevole all'inizio di una riga. */
  scorriA: (indice: number) => void;
  /** Come si riconosce una riga. Di serie `.riga`. */
  selettoreRiga?: string | undefined;
  /** Come si riconosce un comando dentro la riga. Di serie `button`. */
  selettoreComando?: string | undefined;
  /** Invio sulla riga: di solito «suona». */
  onInvio?: ((indice: number) => void) | undefined;
  /** Spazio sulla riga: di solito «aggiungi alla selezione». */
  onSpazio?: ((indice: number) => void) | undefined;
  /**
   * Una griglia di `colonne` colonne invece di un elenco: le frecce si muovono
   * sui due assi della griglia. Vedi «Le griglie» nella nota del modulo.
   */
  colonne?: number | undefined;
}): Fuoco {
  const {
    totale,
    ancora,
    finestra,
    scorriA,
    selettoreRiga = ".riga",
    selettoreComando = "button",
    onInvio,
    onSpazio,
    colonne,
  } = opzioni;

  const [attivoStato, setAttivo] = useState(0);
  // Stretto in lettura e non con un effetto: l'elenco si accorcia — si cambia
  // vista, si toglie una riga da una playlist — e un effetto che corregge
  // `attivo` costerebbe un disegno in più per dire una cosa che si sa già.
  const attivo = stretto(attivoStato, totale);

  /** La riga che aspetta il fuoco, e da quando. */
  const inSospeso = useRef<{ indice: number; da: number } | null>(null);

  /** Le righe disegnate adesso, nell'ordine del DOM. */
  const righe = useCallback((): HTMLElement[] => {
    const elenco = ancora.current;
    if (elenco === null) return [];
    return Array.from(elenco.querySelectorAll<HTMLElement>(selettoreRiga));
  }, [ancora, selettoreRiga]);

  /**
   * La finestra di **adesso**, leggibile da una chiusura memoizzata.
   *
   * `vaiA` non può avere `finestra` fra le dipendenze: passerebbe di identità a
   * ogni scorrimento, e con lui le prop di `RigaBrano` — cioè il `memo` che
   * questa release esiste anche per difendere.
   */
  const finestraRef = useRef(finestra);
  finestraRef.current = finestra;

  /**
   * Il nodo della riga `indice`, se è disegnato.
   *
   * Per `data-indice` quando le righe lo scrivono, per posizione nella finestra
   * quando no: vedi «Quale riga è questa» nella nota del modulo.
   */
  const nodoDi = useCallback(
    (indice: number): HTMLElement | null => {
      const disegnate = righe();
      if (disegnate[0]?.dataset.indice !== undefined) {
        return disegnate.find((n) => n.dataset.indice === String(indice)) ?? null;
      }
      return disegnate[indice - finestraRef.current.primo] ?? null;
    },
    [righe],
  );

  const vaiA = useCallback(
    (indice: number) => {
      const dove = stretto(indice, totale);
      setAttivo(dove);
      const f = finestraRef.current;
      if (!daScorrere(dove, f)) {
        // Già disegnata: il fuoco ci va adesso, e non c'è niente da mettere in
        // sospeso. Metterlo in sospeso comunque sarebbe un errore silenzioso:
        // `setAttivo` con lo stesso valore non ridisegna — ↑ sulla prima riga,
        // ↓ sull'ultima — quindi l'effetto non girerebbe, la riga resterebbe in
        // sospeso, e al primo disegno altrui (un clic su un'altra riga, mezzo
        // minuto dopo) il fuoco salterebbe indietro da solo.
        inSospeso.current = null;
        nodoDi(dove)?.focus();
        return;
      }
      inSospeso.current = { indice: dove, da: performance.now() };
      // E si scorre adesso, senza aspettare l'effetto che lo fa dopo ogni
      // disegno: quando la riga d'arrivo è già quella attiva — Inizio sulla
      // prima dopo aver scorso con la rotella, Fine di nuovo sull'ultima, ↑
      // sulla prima, ↓ sull'ultima — `setAttivo` riceve lo stesso valore, il
      // disegno non c'è, l'effetto non gira, e il tasto preso non faceva
      // niente. Lo scorrimento rifà la finestra, e il disegno che segue porta
      // il fuoco come sempre; l'effetto, se gira, riscrive lo stesso
      // `scrollTop`.
      scorriA(dove);
    },
    [totale, nodoDi, scorriA],
  );

  const segna = useCallback((indice: number) => setAttivo(indice), []);

  /**
   * Porta il fuoco dove `vaiA` ha chiesto, appena la riga esiste.
   *
   * Senza elenco di dipendenze **di proposito**: deve girare dopo ogni disegno,
   * perché il disegno che fa comparire la riga è proprio quello che segue lo
   * scorrimento. Non fa niente quando non c'è niente in sospeso, che è
   * praticamente sempre.
   */
  useLayoutEffect(() => {
    const sospeso = inSospeso.current;
    if (sospeso === null) return;
    const { indice } = sospeso;
    if (indice >= finestra.primo && indice < finestra.ultimo) {
      inSospeso.current = null;
      nodoDi(indice)?.focus();
      return;
    }
    if (performance.now() - sospeso.da > ATTESA_MS) {
      inSospeso.current = null;
      return;
    }
    // Prima lo scorrimento: vedi il docblock del modulo. Lo scorrimento fa
    // ricalcolare la finestra, la finestra rifà il disegno, e il disegno ripassa
    // da qui con la riga dentro. Ripeterlo a ogni disegno nel frattempo non
    // costa niente: `scorriA` scrive lo stesso `scrollTop` di prima.
    scorriA(indice);
  });

  /** Il nodo che aveva il fuoco all'ultimo disegno, se stava in questo elenco. */
  const colFuoco = useRef<HTMLElement | null>(null);

  /**
   * Il fuoco che un disegno ha fatto cadere, rimesso sulla sua riga.
   *
   * Lo scheletro di una riga e la riga vera che lo sostituisce sono due
   * componenti diversi, e React tiene un nodo solo quando sono uguali chiave
   * **e** tipo: alla stessa chiave smonta lo scheletro e monta la riga, e il
   * nodo col fuoco sparisce dal DOM — il fuoco cade sul `<body>`. Capita
   * proprio a chi naviga da tastiera: Fine su cinquantamila brani porta il
   * fuoco su uno scheletro, e la pagina che arriva un istante dopo glielo
   * toglieva, con Ctrl+Inizio, le frecce e Ctrl+A che da lì in poi andavano
   * alla pagina invece che all'elenco. Vale per le righe, per le schede delle
   * griglie e per chiunque usi questo modulo.
   *
   * Si rimette **solo** se il nodo di prima non è più nel DOM e il fuoco sta sul
   * `<body>`: è la firma di un nodo rifatto da un disegno. Un clic altrove o un
   * Tab fuori dall'elenco lasciano il nodo dov'era, e qui non si tocca niente.
   * `preventScroll` perché la riga nuova sta dove stava la vecchia: scorrere
   * adesso sarebbe un salto che nessuno ha chiesto.
   *
   * Senza elenco di dipendenze, come l'effetto sopra e dopo di lui: se il fuoco
   * in sospeso è appena arrivato, il `<body>` non ce l'ha più e qui non succede
   * niente.
   */
  useLayoutEffect(() => {
    const prima = colFuoco.current;
    const ora = document.activeElement;
    if (prima !== null && !prima.isConnected && (ora === null || ora === document.body)) {
      nodoDi(attivo)?.focus({ preventScroll: true });
    }
    const adesso = document.activeElement;
    colFuoco.current =
      adesso instanceof HTMLElement && ancora.current?.contains(adesso) === true ? adesso : null;
  });

  const daTastiera = useCallback(
    (e: KeyboardEvent<HTMLElement>) => {
      // `Alt+↑↓` non è di qui: sposta la riga dentro una playlist, e il gestore
      // sta sulla riga. Intercettarlo significherebbe spostare il fuoco invece
      // di spostare la canzone. Con `Ctrl` passano soltanto Inizio e Fine: vedi
      // «Le pagine, e i due capi».
      const capo = e.key === "Home" || e.key === "End";
      if (e.altKey || e.metaKey) return;
      if (e.ctrlKey && (!capo || e.shiftKey)) return;

      const bersaglio = e.target instanceof HTMLElement ? e.target : null;
      const riga = bersaglio?.closest<HTMLElement>(selettoreRiga) ?? null;
      if (riga === null) return;
      const disegnate = righe();
      const dentro = disegnate.indexOf(riga);
      if (dentro < 0) return;
      const indice = indiceDi(riga, finestra.primo + dentro);

      /*
       * Preso **e fermato**.
       *
       * `preventDefault` non basta: le scorciatoie globali vivono su un ascolto
       * appeso a `window` che non guarda `defaultPrevented` (`tastiera.ts:345-379`),
       * e tre dei tasti di qui sono associati a un comando di trasporto —
       * Spazio è «pausa», ←→ sono «avanti/indietro di cinque secondi». Senza
       * fermare la propagazione, lo Spazio su una riga selezionerebbe **e**
       * metterebbe in pausa.
       *
       * Si ferma sul nodo invece di insegnare a `tastiera.ts` a guardare
       * `defaultPrevented` per la stessa ragione per cui lo fa `useFinestrella`
       * con Escape: il tasto lo ha consumato chi ha il fuoco, e chi ha il fuoco
       * è qui dentro.
       */
      const preso = () => {
        e.preventDefault();
        e.stopPropagation();
      };

      // Una schermata più in là: vedi «Le pagine, e i due capi». Le righe che
      // entrano le sa la finestra; una griglia disegnata intera non ne ha una,
      // e la pagina è tutta la griglia.
      if (e.key === "PageDown" || e.key === "PageUp") {
        preso();
        const f = finestraRef.current;
        const schermo = f.visibili ?? Math.max(f.ultimo - f.primo, 1);
        vaiA(
          passoPagina(
            indice,
            e.key === "PageDown" ? 1 : -1,
            Math.max(schermo - 1, 1),
            colonne ?? 1,
            totale,
          ),
        );
        return;
      }

      // In una griglia le frecce sono dei due assi: vedi «Le griglie». Prese
      // anche contro un bordo, dove `passoGriglia` dice di restare: una freccia
      // che a fine riga facesse scorrere il brano partirebbe per sbaglio.
      if (colonne !== undefined) {
        const tasti = ["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight", "Home", "End"];
        if (tasti.includes(e.key)) {
          preso();
          const dove = passoGriglia(indice, e.key, colonne, totale);
          if (dove !== null) vaiA(dove);
          return;
        }
      }

      switch (e.key) {
        case "ArrowDown":
          preso();
          vaiA(indice + 1);
          return;
        case "ArrowUp":
          preso();
          vaiA(indice - 1);
          return;
        case "Home":
          preso();
          vaiA(0);
          return;
        case "End":
          preso();
          vaiA(totale - 1);
          return;
        case "ArrowRight":
        case "ArrowLeft": {
          // Consumato anche ai due capi, e non lasciato cadere sul trasporto:
          // una freccia che fa scorrere il brano **solo** quando i comandi della
          // riga sono finiti è una freccia che parte per sbaglio. Finché il fuoco
          // è dentro la griglia, ←→ sono della griglia.
          preso();
          // I comandi **visibili e accesi**: sotto i 560 px di contenitore le
          // stelle sono `display: none` e senza dispositivo audio l'indice è
          // spento. Un giro che si fermasse su quelli sembrerebbe rotto.
          const comandi = Array.from(
            riga.querySelectorAll<HTMLElement>(selettoreComando),
          ).filter(
            (n) =>
              n.offsetParent !== null &&
              !(n instanceof HTMLButtonElement && n.disabled),
          );
          // `-1` è la riga stessa: da lì ← non ha dove andare, e → porta al
          // primo comando. Non è un anello: arrivato in fondo si ferma, perché
          // un anello su sette comandi fa perdere il conto di dove si è.
          const fra = comandi.findIndex((n) => n === document.activeElement);
          const dove = fra + (e.key === "ArrowRight" ? 1 : -1);
          if (dove === -1) riga.focus();
          else comandi[dove]?.focus();
          return;
        }
        case "Enter":
        case " ":
          // La propagazione si ferma sempre, anche quando il tasto non è per
          // noi: su un comando lo consuma il comando — Invio su «Riproduci»
          // suona già — ma senza questa riga lo Spazio che vota una stella
          // metterebbe **anche** in pausa, perché il trasporto ascolta su
          // `window` e non sa niente di chi ha il fuoco.
          e.stopPropagation();
          // `preventDefault` no, però: su un bottone toglierebbe l'attivazione
          // nativa, cioè il clic che lo Spazio **è**.
          if (e.target !== riga) return;
          {
            // Senza un gesto suo il tasto resta al nodo: una scheda della
            // griglia è un bottone, e Invio e Spazio la premono già da soli.
            const gesto = e.key === "Enter" ? onInvio : onSpazio;
            if (gesto === undefined) return;
            e.preventDefault();
            gesto(indice);
          }
          return;
        default:
          return;
      }
    },
    [
      finestra.primo,
      righe,
      selettoreRiga,
      selettoreComando,
      totale,
      vaiA,
      onInvio,
      onSpazio,
      colonne,
    ],
  );

  // La fermata: vedi «La fermata, quando la riga attiva non c'è».
  const { primo, ultimo } = finestra;
  const fermata =
    totale === 0 || (attivo >= primo && attivo < ultimo)
      ? attivo
      : stretto(Math.max(finestra.visibileDa ?? primo, primo), Math.max(ultimo, primo + 1));

  // Memoizzato per la stessa ragione di `useVirtuale`: l'oggetto entra nelle
  // dipendenze di chi disegna.
  return useMemo(
    () => ({ attivo, fermata, vaiA, segna, daTastiera }),
    [attivo, fermata, vaiA, segna, daTastiera],
  );
}

/**
 * Dove va il fuoco con PagSu (`verso` −1) o PagGiù (+1).
 *
 * `righe` righe più in là, sulla stessa colonna in una griglia. Contro un capo
 * si ferma sulla prima o sull'ultima riga, ancora nella stessa colonna se c'è:
 * PagGiù sull'ultima pagina porta in fondo, non fuori. Puro, e provato.
 */
export function passoPagina(
  indice: number,
  verso: 1 | -1,
  righe: number,
  colonne: number,
  totale: number,
): number {
  if (totale <= 0) return 0;
  const c = Math.max(Math.floor(colonne), 1);
  const dove = indice + verso * Math.max(Math.floor(righe), 1) * c;
  if (dove < 0) return Math.min(indice % c, totale - 1);
  if (dove > totale - 1) {
    const ultimaRiga = Math.floor((totale - 1) / c);
    return Math.min(ultimaRiga * c + (indice % c), totale - 1);
  }
  return dove;
}

/** Dentro l'elenco: da 0 all'ultimo indice, e 0 se l'elenco è vuoto. */
function stretto(indice: number, totale: number): number {
  return Math.min(Math.max(indice, 0), Math.max(totale - 1, 0));
}
