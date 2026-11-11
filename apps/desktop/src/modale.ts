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

/** Chi vuole sapere quando un modale si apre: vedi `quandoSiApreUnModale`. */
const allApertura = new Set<() => void>();

/**
 * Dichiara un modale aperto, e restituisce la funzione che lo chiude.
 *
 * La chiusura vale una volta sola: chiamata due volte — un effetto smontato e
 * rimontato da StrictMode in sviluppo — non porta il conto sotto quello vero.
 */
export function apriModale(): () => void {
  aperti += 1;
  // Una copia: chi sente l'apertura può smettere di seguire proprio adesso.
  for (const ascolta of [...allApertura]) ascolta();
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

/**
 * Quanti modali sono aperti. Per chi deve togliere dal conto uno che sa
 * aperto lui: il primo avvio, per i file trascinati (`trascinati.ts`).
 */
export function modaliAperti(): number {
  return aperti;
}

/**
 * Chiama `ascolta` a ogni modale che si apre, finché non si chiama la
 * funzione restituita.
 *
 * # Per il menù del telefono
 *
 * Sul telefono il menù è un foglio (`parti/Foglio.tsx`) che sta un gradino
 * sopra le finestrelle (`--strato-menu` sopra `--strato-velo`), e tutti e due
 * stanno in fondo allo schermo. Una finestrella che arriva da sola con un menù
 * aperto — «X vuole collegarsi», un link da un altro dispositivo — si
 * disegnava sotto il menù e lo rendeva inerte: il menù si vedeva ma non
 * rispondeva, e l'unico gesto vivo era Indietro, che chiude prima la
 * finestrella in cima (`tastiera.ts`). Cioè la rifiutava, senza che fosse
 * mai stata vista. Il menù è la cosa di passaggio: quando si apre una
 * finestrella scende lui, e la lascia vedere.
 *
 * Solo le aperture, e non le chiusure: a chi segue interessa che qualcosa si
 * sia messo davanti, non quanti ce ne siano.
 */
export function quandoSiApreUnModale(ascolta: () => void): () => void {
  allApertura.add(ascolta);
  return () => {
    allApertura.delete(ascolta);
  };
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

// ── una risposta sola ──────────────────────────────────────────────────────

/**
 * Il cancello di una domanda: `true` alla prima risposta, `false` a tutte le
 * altre.
 *
 * # Perché il dialogo non si chiude abbastanza in fretta
 *
 * Perché le conferme di `App` si chiudono con `useStatoPresente`, che per
 * animare l'uscita passa da `startTransition`: un aggiornamento che React può
 * interrompere e rimandare — e sotto carico lo fa: ogni aggiornamento
 * sincrono che arriva intanto, lo stato del lettore o una pagina
 * dell'elenco, gli passa davanti. Finché non è applicato la
 * finestrella resta montata, il suo tasto risponde, e la chiusura che il clic
 * chiama vede ancora la domanda aperta: un doppio clic su «Togli», o Invio
 * tenuto giù su «Nuova playlist», rifaceva il gesto — due `rimozioniConferma`
 * sullo stesso gettone, due playlist con lo stesso nome. Chiudere in modo
 * sincrono avrebbe tolto l'animazione per salvare un clic; la finestrella
 * invece sa da sé che ha già avuto la sua risposta.
 *
 * Uno per apertura, creato quando la finestrella si monta: la domanda di dopo
 * è un'altra domanda. E vale per tutte le uscite insieme — «Togli» seguito da
 * Escape non deve mandare anche lo scarto dello stesso elenco.
 */
export function unaVolta(): () => boolean {
  let data = false;
  return () => {
    if (data) return false;
    data = true;
    return true;
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

// ── la × della finestra ────────────────────────────────────────────────────

/** Quel che la × deve sapere della finestrella in cima. */
export interface CimaPerLaX {
  /**
   * È una domanda (`Dialogo`): una conferma, un consenso, un nome da dare.
   * Le finestrelle con una forma tutta loro — il primo avvio, il giro, i
   * pannelli, l'editor delle regole — non lo sono.
   */
  domanda: boolean;
  /** Si può chiudere adesso: spento mentre la domanda sta scrivendo. */
  chiudibile: boolean;
}

/**
 * Cosa fa la × della finestra, guardando quel che sta in cima.
 *
 * # Prima la domanda, poi la finestra
 *
 * Con una domanda aperta la × la chiude, e vale come «no»: è quel che fa
 * Escape, ed è la sola lettura che non perde niente. Chi voleva chiudere la
 * finestra la richiude un istante dopo; chi stava ancora leggendo «X vuole
 * collegarsi» non si ritrova la finestra sparita nel vassoio con la domanda
 * ancora in sospeso dall'altra parte.
 *
 * In tutti gli altri casi la finestra si chiude come sempre, cioè come Alt+F4:
 * senza niente in cima, sopra una domanda che sta scrivendo (chiuderla a metà
 * lascerebbe il lavoro senza nessuno che ne dica l'esito, e il nucleo decide
 * lui fra vassoio e uscita), e sopra le finestrelle che non sono domande — il
 * primo avvio, dove una × che salta la schermata invece di chiudere il
 * programma toglierebbe a chi apre per la prima volta la sua strada d'ingresso.
 * Chiudere la finestra resta l'unico comando che funziona sempre: vedi
 * `.controlli-finestra` in `stile.css`.
 */
export function cosaFaLaX(cima: CimaPerLaX | null): "rispondiNo" | "chiudiLaFinestra" {
  return cima !== null && cima.domanda && cima.chiudibile ? "rispondiNo" : "chiudiLaFinestra";
}

// ── chi diventa inerte ─────────────────────────────────────────────────────

/** Quel che serve di un nodo per salire l'albero: il genitore e i figli. */
export interface NodoDiAlbero {
  readonly tagName: string;
  readonly parentElement: NodoDiAlbero | null;
  readonly children: ArrayLike<NodoDiAlbero>;
  /** Per i marchi: vedi `SEMPRE_VIVO` e `ANNUNCIO`. */
  hasAttribute(nome: string): boolean;
  /**
   * Già inerte. Lo porta `HTMLElement`; per gli altri nodi manca, e vale
   * «no». Vedi «Un velo già sepolto».
   */
  readonly inert?: boolean;
}

/**
 * Il marchio di chi non diventa mai inerte: i tre comandi della finestra.
 *
 * Stanno fuori dall'applicazione apposta (`main.tsx`), cioè proprio dove la
 * salita li trovava, e una finestrella aperta li spegneva: restavano disegnati
 * sopra il velo, ma un nodo inerte è trasparente al puntatore, e il clic sulla
 * × cadeva sul velo sotto — che chiudeva la domanda invece della finestra, o
 * non faceva niente.
 */
export const SEMPRE_VIVO = "data-sempre-vivo";

/**
 * Il marchio delle regioni d'annuncio (`parti/Annuncio.tsx`,
 * `AnnuncioScansione`).
 *
 * Sono solo testo, senza un fuoco né un clic: lasciarle vive non apre
 * nessuna strada dietro il velo, e spegnerle voleva dire tacere le notizie
 * che arrivano mentre una finestrella è aperta — la fine di una scansione,
 * «il suono si è spostato». E quando l'inerzia se ne andava il testo era già
 * lì, quindi la regione non cambiava e taceva anche dopo.
 */
export const ANNUNCIO = "data-annuncio";

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
 *
 * Tranne chi porta uno dei due marchi (`SEMPRE_VIVO`, `ANNUNCIO`), e chi è
 * in `accanto`: il foglio del menù del telefono ha il velo e il pannello
 * fratelli, e il pannello è proprio quel che deve restare vivo.
 *
 * # Un velo già sepolto non spegne niente
 *
 * Se un antenato del velo è già inerte — e chi chiama ha appena tolto i suoi
 * `inert`, quindi è di qualcun altro: la finestra sotto lo Studio — la
 * finestrella non si vede e non prende il fuoco. Spegnere anche il resto
 * voleva dire congelare la finestra intera, Studio compreso, per una domanda
 * che nessuno poteva leggere. Le domande che arrivano da sole stanno fuori da
 * quella finestra, sopra lo Studio (`.strato-domande` in `App.tsx`); questo è
 * il ripiego per le altre.
 */
export function daRendereInerti<N extends NodoDiAlbero>(
  velo: N,
  accanto: readonly N[] = [],
): N[] {
  for (let su = velo.parentElement; su !== null; su = su.parentElement) {
    if (su.inert === true) return [];
  }
  const inerti: N[] = [];
  let gradino: NodoDiAlbero = velo;
  let genitore = gradino.parentElement;
  while (genitore !== null) {
    for (const fratello of Array.from(genitore.children)) {
      if (
        fratello !== gradino &&
        !NON_DISEGNATI.has(fratello.tagName) &&
        !fratello.hasAttribute(SEMPRE_VIVO) &&
        !fratello.hasAttribute(ANNUNCIO) &&
        !(accanto as readonly NodoDiAlbero[]).includes(fratello)
      ) {
        // Lo stesso tipo del velo: sono nodi dello stesso albero.
        inerti.push(fratello as N);
      }
    }
    gradino = genitore;
    genitore = gradino.parentElement;
  }
  return inerti;
}
