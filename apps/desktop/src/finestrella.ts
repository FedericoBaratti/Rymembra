/**
 * Il fuoco di una finestrella: dove entra, dove resta, come se ne esce.
 *
 * Nell'albero ci sono dieci finestrelle e fino a ieri **una sola** si occupava
 * del fuoco: `Chiedi`, che per farlo portava due commenti più lunghi del suo
 * markup. Le altre nove lasciavano uscire il tabulatore dietro al velo, dove i
 * comandi della pagina coperta si vedono ma non rispondono al clic, e da dove
 * nessun tasto riporta indietro; in otto di esse Escape non chiudeva niente.
 * Questo modulo è quei due commenti diventati una funzione, e le altre nove che
 * la chiamano.
 *
 * # Un hook, non un componente
 *
 * Perché il markup delle dieci non si somiglia: due sono un `form`, sette un
 * `div` dentro un velo, una una schermata piena. Un componente avvolgente
 * avrebbe dovuto accettare classi, ruolo, etichetta e gestore del clic sul velo
 * come prop, cioè ricostruire in una firma quel che il JSX dice già meglio da
 * sé. `role="dialog"`, `aria-modal` e `aria-label`/`aria-labelledby` restano
 * quindi scritti a mano in ognuna delle dieci: spargerli da un oggetto li
 * renderebbe illeggibili proprio dove un lettore di schermo ha bisogno che si
 * leggano.
 *
 * # Il fuoco, all'apertura e alla chiusura
 *
 * All'apertura il fuoco va sul primo elemento che può prenderlo, o su quello
 * marcato `data-fuoco-iniziale` quando non è il primo; se è un campo di testo a
 * riga sola il contenuto si seleziona, perché rinominare vuol dire quasi sempre
 * sostituire e non aggiungere in coda. Il marchio vale anche sulla radice, e lì
 * vuol dire «a nessuno dei miei comandi»: lo usa `Primo`, dove nel primo istante
 * il comando principale è ancora spento e gli altri due aprono cose che nessuno
 * ha chiesto.
 *
 * Alla chiusura si prova a rimettere il fuoco
 * dov'era, e il ripristino vale quando chi ha aperto è un elemento
 * focalizzabile ancora montato: un tasto di `parti/Navigazione.tsx`, un comando
 * della barra. Quando la finestrella arriva da una voce di `Menu` non lo è:
 * quel bottone è già staccato dal DOM nel momento in cui il componente si
 * monta, e quel che si legge è il `<body>`. La guardia rende esplicito il
 * no-op invece di fingere un ritorno — il fuoco resta dove sarebbe caduto
 * comunque, cioè sul `<body>`, come prima.
 *
 * Un «ancoraggio» esplicito passato da chi apre coprirebbe anche quel caso, ma
 * vorrebbe dire una prop nuova in tutte e dieci e nei loro chiamanti: fuori dal
 * perimetro di un raffinamento a comportamento invariato. È il candidato
 * successivo.
 *
 * Chi aveva il fuoco va letto **prima** di prenderglielo: è il primo effetto
 * dell'hook, uno di disposizione, e l'ordine non è scambiabile con niente —
 * nemmeno con l'inerzia di `Dialogo`, che spegne proprio il nodo da leggere.
 *
 * # E poi è arrivato `Dialogo`
 *
 * Quel che sta scritto qui sopra contro un componente avvolgente era vero
 * finché il comportamento comune si fermava al fuoco. Con la 3.0 ne sono
 * arrivati altri due che il JSX di ogni finestrella non diceva meglio da sé, e
 * che dieci copie avrebbero sbagliato in dieci modi: il resto della finestra
 * `inert` mentre la finestrella è aperta, e il velo che chiude solo un clic
 * nato **e** finito su di lui. Quelle due cose le fa `Dialogo.tsx`, costruito
 * sopra questo hook; le finestrelle con una forma tutta loro — `Primo`, `Giro`,
 * `Regole`, `Sincronizza` — continuano a chiamare l'hook e basta.
 *
 * Le decisioni senza DOM — dove va il fuoco, chi diventa inerte, quando il velo
 * chiude, quali scorciatoie passano — stanno in `modale.ts`, dove si provano.
 *
 * # Il conto dei modali
 *
 * Ogni finestrella aperta fuori dall'anteprima si conta (`apriModale`), e
 * `tastiera.ts` legge il conto: con un modale aperto le scorciatoie globali non
 * passano, tranne lo zoom e lo schermo intero. Si conta qui e non in `Dialogo`
 * perché la regola vale per tutte le finestrelle, anche quelle che l'hook lo
 * chiamano da sole.
 *
 * # Il Tab resta dentro
 *
 * I fuochi si ricontano a ogni pressione e non una volta sola, perché in sei di
 * queste finestrelle compaiono e spariscono mentre si guardano — un piano che
 * arriva, un tasto che si accende, un pannello che si apre. Chi è disabilitato
 * esce dal giro, che è già quel che fa il browser da sé; chi non è disegnato
 * (`offsetParent === null`) pure, perché `Regole` e `Sincronizza` tengono in
 * piedi pannelli nascosti; e chi ha `tabindex="-1"` anche, perché il
 * radiogruppo di `NuovoTema` ne ha uno per skin installata e il tabulatore deve
 * attraversarlo in una fermata sola — parcheggiare il fuoco su un'opzione non
 * raggiungibile vorrebbe dire metterlo dove il Tab non sa tornare.
 *
 * La radice prende un `tabindex="-1"` se non ne ha già uno. Non la rende
 * raggiungibile col tabulatore (il `-1` dice esattamente il contrario): serve a
 * due cose, e in entrambe il punto è che il fuoco non finisca fuori — perché un
 * fuoco fuori è un Escape che non chiude e un Tab che non torna. Un clic sul bordo
 * della finestrella, sul titolo, su una riga di prosa — cioè su qualcosa che
 * non può prendere il fuoco — lo manderebbe sul `<body>`, e da lì i tasti non
 * passano più per questo nodo; col `-1` si fermano sulla radice. E
 * `Ripristino` e `ImportaPlaylist` si aprono con **tutti** i comandi spenti
 * perché il piano non è ancora arrivato: senza la radice non ci sarebbe, in
 * quell'istante, un solo posto dentro la finestrella dove mettere il fuoco.
 *
 * # Escape chiude, e si ferma qui
 *
 * Questo è il punto delicato, e va argomentato perché tocca una scala che è
 * centralizzata per scelta. L'ascoltatore sta sul **nodo** e non su `window`, e
 * chiama `stopPropagation()`.
 *
 * La scala di Escape vive in `tastiera.ts` (`useScorciatoie`, l'ascoltatore su
 * `window`) e il suo ultimo gradino è `azioni.chiudi()`, scritto in `App.tsx`:
 * schermo intero, menù, selezione, ricerca, album aperto, artista aperto — un
 * livello per pressione. Prima di quella scala c'è un gradino in più: se il
 * fuoco è in un campo di testo, Escape fa `blur()` e si ferma («prima si esce
 * dal campo, poi si chiude quel che c'è aperto»).
 *
 * Con una finestrella aperta e un ascoltatore suo su `window`, le due cose
 * succedevano **insieme**: con `Chiedi` aperto una pressione sola svuotava il
 * fuoco dal campo *e* chiudeva la finestrella, e col fuoco su un tasto invece
 * del campo chiudeva la finestrella *e* consumava un gradino della scala dietro
 * — cioè azzerava la selezione o la ricerca di una pagina che l'utente non
 * stava nemmeno guardando. Ascoltando sul nodo e fermando la propagazione, la
 * chiusura resta al primo colpo, il campo non viene sfocato per niente (si
 * smonta un istante dopo) e la scala dietro non perde un livello.
 *
 * La trappola del Tab e il `tabindex="-1"` della radice garantiscono che il
 * fuoco sia sempre dentro la finestrella, quindi l'evento passa sempre da
 * questo nodo. Senza quelle due cose `stopPropagation` su un nodo fuori dal
 * percorso dell'evento non sarebbe una scelta: sarebbe un Escape che non chiude
 * più, ed è per questo che i quattro pezzi di questo hook stanno insieme e non
 * si possono prendere uno alla volta.
 *
 * # `onChiudi` in un riferimento
 *
 * Per la ragione già scritta in `tastiera.ts`: i chiamanti passano una chiusura
 * scritta sul posto (`onChiudi={() => setQualcosa(null)}`), quindi la sua
 * identità cambia a ogni disegno di `App` — venti volte al secondo mentre
 * suona. Tenerla fra le dipendenze voleva dire rimontare la trappola del fuoco
 * a ogni disegno: `Chiedi` lo faceva davvero, e per giunta rileggeva
 * `document.activeElement` ogni volta. Con l'effetto a dipendenze vuote e la
 * chiusura in un `ref`, il fuoco si prende e si restituisce una volta sola, e
 * nessun chiamante deve ricordarsi di un `useCallback`.
 */
import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";

import { useDati } from "./dati";
import {
  apriModale,
  daRendereInerti,
  fuocoDopoTab,
  fuocoIniziale,
} from "./modale";

/**
 * Chi può prendere il fuoco dentro una finestrella.
 *
 * Il selettore è generale di proposito: `Chiedi` aveva `input, button` perché i
 * suoi fuochi sono tre e li conosceva tutti, ma `Regole` ha dei `select`,
 * `ImportaPlaylist` il `summary` di un elenco che si apre, e una skin può mettere
 * un `a[href]` dove vuole. Il `summary` è nell'elenco pur non avendo un
 * `tabindex`: è focalizzabile di serie, e lasciarlo fuori vorrebbe dire una
 * fermata del tabulatore che la trappola non conosce. `:not(:disabled)` e `:not([disabled])` dicono al
 * selettore quel che il browser fa già da sé; il resto dei filtri, che un
 * selettore non sa esprimere, sta in `fuochiDi`.
 */
const FOCALIZZABILI =
  'a[href], button:not(:disabled), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary, [tabindex]:not([tabindex="-1"])';

/**
 * I fuochi della finestrella, in ordine di tabulazione.
 *
 * `offsetParent === null` è il modo a costo zero di chiedere «è disegnato?»:
 * prende `display: none` e i pannelli chiusi senza costringere a un
 * `getComputedStyle` per elemento. Dentro una finestrella non c'è niente in
 * posizione fissa, che è l'unico falso positivo noto di quella lettura.
 */
function fuochiDi(radice: HTMLElement): HTMLElement[] {
  return Array.from(radice.querySelectorAll<HTMLElement>(FOCALIZZABILI)).filter(
    (nodo) => nodo.offsetParent !== null && nodo.tabIndex >= 0,
  );
}

/**
 * Un campo il cui contenuto si seleziona quando prende il fuoco.
 *
 * Solo i campi a riga sola. Il `textarea` è escluso di proposito: l'unico
 * dell'albero dentro una finestrella è quello delle righe da battere di
 * `Sincronizza`, e selezionare quattordici righe di testo vorrebbe dire che il
 * primo tasto premuto cancella il lavoro di chi le ha incollate.
 */
function eUnCampoDaSostituire(nodo: HTMLElement): nodo is HTMLInputElement {
  if (!(nodo instanceof HTMLInputElement)) return false;
  const tipo = nodo.type;
  return (
    tipo === "text" ||
    tipo === "search" ||
    tipo === "url" ||
    tipo === "email" ||
    tipo === "tel" ||
    tipo === "password" ||
    tipo === "number"
  );
}

/**
 * Il comportamento comune delle finestrelle.
 *
 * Restituisce **solo** il riferimento da appendere alla radice della
 * finestrella — quella che porta `role="dialog"`, non il velo: il velo non deve
 * prendere il fuoco e i suoi figli non sono dentro il dialogo.
 *
 * `onChiudi` è quel che fa Escape, e può essere una chiusura scritta sul posto:
 * vedi «`onChiudi` in un riferimento» qui sopra.
 */
export function useFinestrella<E extends HTMLElement>(
  onChiudi: () => void,
): RefObject<E | null> {
  const radice = useRef<E | null>(null);
  const chiudi = useRef(onChiudi);
  chiudi.current = onChiudi;
  // Nell'anteprima dello Studio una finestrella è un disegno: prenderle il
  // fuoco lo toglierebbe al campo in cui si sta scrivendo un colore.
  const { anteprima } = useDati();

  // Chi aveva il fuoco, letto prima di prenderglielo — e prima di tutto il
  // resto: in un effetto di disposizione, che gira prima di quello qui sotto
  // e prima di quello di `useVeloInerte` dichiarato dopo l'hook. Un nodo reso
  // inerte perde il fuoco, e leggerlo dopo voleva dire leggere il `<body>`.
  const chiAveva = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    if (anteprima) return;
    chiAveva.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
  }, [anteprima]);

  useEffect(() => {
    const dentro = radice.current;
    if (dentro === null || anteprima) return;
    const daRidare = chiAveva.current;

    // Da qui la finestrella conta fra i modali aperti: vedi «Il conto dei
    // modali» qui sopra.
    const chiudiModale = apriModale();

    if (!dentro.hasAttribute("tabindex")) dentro.tabIndex = -1;

    // Il marchio si cerca fra i fuochi veri e non nel DOM nudo: `focus()` su un
    // comando spento o non disegnato non fa niente, e lascerebbe il fuoco dietro
    // al velo senza dirlo. La radice può portarlo addosso lei, e allora il fuoco
    // si ferma lì: lo chiede `Primo`. La regola sta in `modale.ts`.
    const dove = fuocoIniziale(fuochiDi(dentro), dentro, (nodo) =>
      nodo.hasAttribute("data-fuoco-iniziale"),
    );
    dove.focus();
    if (eUnCampoDaSostituire(dove)) dove.select();

    const suTasto = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        chiudi.current();
        return;
      }
      if (e.key !== "Tab") return;
      const adesso =
        document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null;
      // Sulla radice o fuori: il giro ricomincia da un capo. Vedi
      // `fuocoDopoTab`, che dice anche perché vale per la radice.
      const dentroDavvero =
        adesso !== null && adesso !== dentro && dentro.contains(adesso);
      const verso = fuocoDopoTab(fuochiDi(dentro), adesso, dentroDavvero, e.shiftKey);
      if (verso !== null) {
        e.preventDefault();
        verso.focus();
      }
    };
    dentro.addEventListener("keydown", suTasto);

    return () => {
      chiudiModale();
      dentro.removeEventListener("keydown", suTasto);
      if (
        daRidare !== null &&
        daRidare !== document.body &&
        daRidare.isConnected
      )
        daRidare.focus();
    };
  }, [anteprima]);

  return radice;
}

// ── il resto della finestra, inerte ────────────────────────────────────────

/** I veli aperti, nell'ordine in cui si sono aperti: l'ultimo è quello sopra. */
const veliAperti: HTMLElement[] = [];

/**
 * I nodi resi inerti da qui, e soltanto quelli.
 *
 * Un `inert` che non è in questo insieme è di qualcun altro — React lo mette
 * sulla pagina a schermo intero e sulla finestra mentre lo Studio è aperto —
 * e non si tocca: né per metterlo, né per toglierlo.
 */
const spentiDaQui = new Set<HTMLElement>();

/** Chi guarda i gradini della salita, per i nodi che arrivano dopo. */
let sentinella: MutationObserver | null = null;

/**
 * Rifà da capo l'inerzia per il velo in cima.
 *
 * Da capo e non a differenze, perché i casi sono tre e si somigliano poco: un
 * velo che si apre sopra un altro (la conferma di una scansione che arriva
 * mentre si importa un link), uno che si chiude lasciando sotto il precedente,
 * e un nodo nuovo che compare dietro mentre il velo è aperto — un avviso, la
 * pila dei toast. Togliere tutto quel che si era messo e rimetterlo per il velo
 * in cima li copre tutti e tre, e succede nello stesso giro di codice, quindi
 * non c'è un fotogramma in cui la pagina dietro torni cliccabile.
 *
 * La sentinella guarda solo i figli diretti di ogni gradino (`childList`, non
 * `subtree`): un nodo nuovo **dentro** un fratello già inerte è inerte da sé, e
 * le modifiche agli attributi — compresi gli `inert` scritti qui — non la
 * svegliano.
 */
function riapplicaInerzia(): void {
  for (const nodo of spentiDaQui) nodo.inert = false;
  spentiDaQui.clear();
  sentinella?.disconnect();
  const cima = veliAperti[veliAperti.length - 1];
  if (cima === undefined) return;
  for (const nodo of daRendereInerti<Element>(cima)) {
    if (!(nodo instanceof HTMLElement) || nodo.inert) continue;
    nodo.inert = true;
    spentiDaQui.add(nodo);
  }
  sentinella ??= new MutationObserver(riapplicaInerzia);
  for (let gradino = cima.parentElement; gradino !== null; gradino = gradino.parentElement) {
    sentinella.observe(gradino, { childList: true });
  }
}

/**
 * Il resto della finestra `inert` finché questo velo è aperto.
 *
 * # Perché non bastava la trappola del Tab
 *
 * La trappola tiene dentro il **tabulatore**, e basta. Un lettore di schermo
 * percorre la pagina con i suoi tasti, non col Tab, e dietro il velo trovava
 * tutta l'applicazione come se niente fosse — `aria-modal` è un suggerimento
 * che non tutti seguono. Un clic del tasto centrale, un trascinamento, un
 * `accesskey`: tutte strade che la trappola non vede. `inert` le chiude tutte
 * insieme, ed è quel che `<dialog>.showModal()` farebbe da sé — se le
 * finestrelle potessero essere `<dialog>` senza cambiare il modo in cui le skin
 * le disegnano, che è il vincolo di questo albero.
 *
 * # Perché un effetto di disposizione
 *
 * Perché deve arrivare **prima** dell'effetto di `useFinestrella`, che sposta
 * il fuoco: un nodo diventato inerte perde il fuoco, e il fuoco deve già essere
 * dove andrà. E alla chiusura deve togliersi prima che l'hook restituisca il
 * fuoco a chi l'aveva — che è dietro il velo, cioè inerte fino a un istante
 * prima. React smonta gli effetti di disposizione prima di quelli passivi, e
 * l'ordine viene da sé.
 *
 * Nell'anteprima dello Studio non fa niente: la finestrella è un disegno sulla
 * tela, e spegnere lo Studio intorno a lei lo renderebbe inservibile.
 */
export function useVeloInerte(velo: RefObject<HTMLElement | null>): void {
  const { anteprima } = useDati();
  useLayoutEffect(() => {
    const nodo = velo.current;
    if (nodo === null || anteprima) return;
    veliAperti.push(nodo);
    riapplicaInerzia();
    return () => {
      const dove = veliAperti.indexOf(nodo);
      if (dove !== -1) veliAperti.splice(dove, 1);
      riapplicaInerzia();
    };
    // `velo` è un riferimento: la sua identità non cambia, e il nodo si legge
    // dentro l'effetto, dove è già appeso.
  }, [velo, anteprima]);
}
