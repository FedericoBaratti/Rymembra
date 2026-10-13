/**
 * Le regole di un modale che non hanno bisogno di un DOM.
 *
 * # Perché un modulo a parte
 *
 * `finestrella.ts` e `Dialogo.tsx` le applicano a nodi veri, e lì dentro si
 * intrecciano con React, con `useDati` e con l'albero della finestra: provarle
 * vorrebbe dire montare un'applicazione. Le decisioni però sono piccole e
 * secche — chi prende il fuoco all'apertura, dove va il Tab all'ultimo comando,
 * quando un clic sul velo vuol dire «chiudi», chi diventa inerte, quali
 * scorciatoie passano — e ognuna è stata sbagliata almeno una volta. Qui stanno
 * senza niente intorno, e `test/modale.test.ts` le prova con oggetti finti.
 *
 * Niente `import` da React né dal DOM, di proposito: `node --test` carica
 * questo file così com'è.
 */

// ── quanti modali sono aperti ──────────────────────────────────────────────

/**
 * Quanti modali sono aperti adesso.
 *
 * Un contatore e non una lettura del DOM (`[aria-modal='true']`), per due
 * ragioni. La tela dello Studio disegna una finestrella finta che porta lo
 * stesso attributo, e resta nel DOM — nascosta da `<Activity>` — anche mentre
 * si prova una skin sull'app: la lettura del DOM direbbe «c'è un modale» a una
 * finestra che non ne ha. E il contatore lo tengono le finestrelle vere, quelle
 * che `useFinestrella` monta fuori dall'anteprima: è la stessa definizione di
 * «modale aperto» che decide il fuoco, scritta una volta.
 */
let aperti = 0;

/**
 * Dichiara un modale aperto, e restituisce la funzione che lo chiude.
 *
 * La chiusura vale una volta sola: chiamata due volte — un effetto smontato e
 * rimontato da StrictMode in sviluppo — non porta il conto sotto quello vero.
 */
export function apriModale(): () => void {
  aperti += 1;
  let chiuso = false;
  return () => {
    if (chiuso) return;
    chiuso = true;
    aperti -= 1;
  };
}

/** C'è almeno un modale aperto? */
export function modaleAperto(): boolean {
  return aperti > 0;
}

// ── le scorciatoie sotto un modale ─────────────────────────────────────────

/**
 * Le scorciatoie globali che passano anche con un modale aperto.
 *
 * Solo quelle che cambiano **come si vede** la finestra e non cosa c'è dentro:
 * lo zoom e lo schermo intero. Chi ha la vista stanca deve poter ingrandire
 * anche la domanda che ha davanti; tutto il resto — «Importa da un link»,
 * «In riproduzione», le importazioni, il salto di cinque secondi — aprirebbe
 * o cambierebbe qualcosa **dietro** il velo, dove l'utente non sta guardando e
 * dove nessun tasto lo riporta.
 *
 * I nomi sono quelli di `Comando` in `tastiera.ts`, scritti come stringhe per
 * non far dipendere questo modulo da quello.
 */
const SOTTO_UN_MODALE: ReadonlySet<string> = new Set([
  "ingrandisci",
  "rimpicciolisci",
  "zoomNormale",
  "schermoIntero",
]);

/**
 * Questa scorciatoia passa, adesso?
 *
 * Senza modali passano tutte: le altre regole — il campo in cui si scrive, il
 * comando che si tiene il tasto — le applica `tastiera.ts` dopo.
 */
export function passaLaScorciatoia(comando: string, conUnModale: boolean): boolean {
  return !conUnModale || SOTTO_UN_MODALE.has(comando);
}

// ── il velo ────────────────────────────────────────────────────────────────

/** Il velo: ricorda dove è cominciato il gesto, e dice se il clic chiude. */
export interface Velo {
  /** Il `pointerdown`: dove è cominciato. */
  premuto(bersaglio: unknown, velo: unknown): void;
  /** Il `click`: chiude solo se **anche** la pressione era sul velo. */
  clic(bersaglio: unknown, velo: unknown): boolean;
}

/**
 * Il velo che chiude solo un clic nato e finito su di lui.
 *
 * # Perché due eventi e non uno
 *
 * Perché il browser manda il `click` all'antenato comune di dove il tasto si è
 * abbassato e di dove si è alzato. Chi seleziona il testo di un campo
 * trascinando verso destra e lascia il tasto oltre il bordo della finestrella
 * produce un `click` il cui bersaglio è il velo — e il velo chiudeva, portandosi
 * via quel che si stava scrivendo. Guardare anche dove il gesto è cominciato
 * distingue «ho cliccato fuori» da «ho finito di trascinare fuori».
 *
 * Il ricordo si consuma a ogni clic, riuscito o no: una pressione sul velo
 * seguita da un clic altrove non deve armare il clic dopo.
 */
export function nuovoVelo(): Velo {
  let giuSulVelo = false;
  return {
    premuto(bersaglio, velo) {
      giuSulVelo = bersaglio === velo;
    },
    clic(bersaglio, velo) {
      const chiude = giuSulVelo && bersaglio === velo;
      giuSulVelo = false;
      return chiude;
    },
  };
}

// ── il fuoco ───────────────────────────────────────────────────────────────

/**
 * Dove va il fuoco quando la finestrella si apre.
 *
 * Il marchio sulla radice vuol dire «a nessuno dei miei comandi» (`Primo`); un
 * marchio su un comando lo sceglie; altrimenti il primo; e se non ce n'è
 * nessuno — tutti spenti mentre il piano arriva — la radice. `fuochi` sono già
 * quelli veri: accesi, disegnati, raggiungibili col Tab.
 */
export function fuocoIniziale<T>(
  fuochi: readonly T[],
  radice: T,
  marcato: (nodo: T) => boolean,
): T {
  if (marcato(radice)) return radice;
  return fuochi.find(marcato) ?? fuochi[0] ?? radice;
}

/**
 * Dove va il fuoco a un Tab, o `null` se lo si lascia fare al browser.
 *
 * Il giro ricomincia da un capo in tre casi: il fuoco è sull'ultimo (o sul
 * primo, all'indietro), è sulla radice, o è fuori. Dalla radice il Tab nudo
 * andrebbe al primo da sé, ma lo Shift+Tab uscirebbe dal velo, e un anello che
 * tiene in un verso solo non è un anello. In mezzo, il browser sa già dove
 * andare.
 */
export function fuocoDopoTab<T>(
  fuochi: readonly T[],
  adesso: T | null,
  dentro: boolean,
  indietro: boolean,
): T | null {
  const primo = fuochi[0];
  const ultimo = fuochi[fuochi.length - 1];
  if (primo === undefined || ultimo === undefined) return null;
  if (!dentro || adesso === null || adesso === (indietro ? primo : ultimo)) {
    return indietro ? ultimo : primo;
  }
  return null;
}

// ── chi diventa inerte ─────────────────────────────────────────────────────

/** Quel che serve di un nodo per salire l'albero: il genitore e i figli. */
export interface NodoDiAlbero {
  readonly tagName: string;
  readonly parentElement: NodoDiAlbero | null;
  readonly children: ArrayLike<NodoDiAlbero>;
}

/**
 * I nodi che non si disegnano: renderli inerti non toglie niente a nessuno.
 *
 * `HEAD` è il fratello di `BODY` all'ultimo gradino della salita.
 */
const NON_DISEGNATI: ReadonlySet<string> = new Set([
  "HEAD",
  "SCRIPT",
  "STYLE",
  "LINK",
  "META",
  "TEMPLATE",
  "NOSCRIPT",
]);

/**
 * Chi rendere inerte perché resti vivo soltanto `velo`.
 *
 * # Perché i fratelli lungo la salita
 *
 * Un elemento inerte rende inerti tutti i suoi discendenti, e non c'è modo di
 * dire «tutto tranne questo ramo». Le finestrelle non stanno in un portale:
 * nascono dove le monta chi le apre — in fondo a `App`, o dentro la pagina di
 * Cartelle — quindi «il resto della finestra» non è un nodo solo. È, a ogni
 * gradino fra il velo e la cima del documento, ogni fratello del gradino:
 * tutto quel che non è un antenato del velo e non sta sotto di lui.
 */
export function daRendereInerti<N extends NodoDiAlbero>(velo: N): N[] {
  const inerti: N[] = [];
  let gradino: NodoDiAlbero = velo;
  let genitore = gradino.parentElement;
  while (genitore !== null) {
    for (const fratello of Array.from(genitore.children)) {
      if (fratello !== gradino && !NON_DISEGNATI.has(fratello.tagName)) {
        // Lo stesso tipo del velo: sono nodi dello stesso albero.
        inerti.push(fratello as N);
      }
    }
    gradino = genitore;
    genitore = gradino.parentElement;
  }
  return inerti;
}
