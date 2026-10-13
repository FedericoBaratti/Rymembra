/**
 * I conti della finestra virtuale, senza il DOM.
 *
 * Stavano dentro `useVirtuale`, mescolati alle letture di `getBoundingClientRect`,
 * e lì non si potevano provare: un `cima` sbagliato o una colonna di troppo si
 * vedono solo scorrendo, in fondo a una libreria vera. Qui sono numeri che
 * entrano e numeri che escono, e li provano i test di `test/finestra.test.ts`.
 *
 * Tre conti: quali righe disegnare per uno scorrimento dato, quante colonne ha
 * una griglia che si dispone da sé (`repeat(auto-fill, …)`), e dove va il fuoco
 * in una griglia quando si preme una freccia.
 */

/** Quel che serve per sapere quali righe disegnare. */
export interface Misure {
  /** `scrollTop` dello scorrevole. */
  scorrimento: number;
  /** Dove comincia la prima riga dentro lo scorrevole, in pixel. */
  cima: number;
  /** `clientHeight` dello scorrevole. */
  altezzaVista: number;
  /** Di quanto scende una riga: la sua altezza, più lo spazio fra due righe. */
  passo: number;
  /** Quante righe ci sono in tutto. */
  totale: number;
  /** Righe in più per lato. */
  margine: number;
}

/** Le righe da disegnare, e la prima che si vede davvero. */
export interface Intervallo {
  /** Prima riga disegnata, compresa. */
  primo: number;
  /** Riga dopo l'ultima disegnata. */
  ultimo: number;
  /**
   * La prima riga in vista, senza il margine.
   *
   * Non serve a disegnare: serve a chi vuole sapere **cosa** si sta guardando —
   * l'indice alfabetico degli artisti accende la lettera di questa riga.
   */
  visibileDa: number;
}

/** Fra due estremi, compresi. */
function stretto(valore: number, minimo: number, massimo: number): number {
  return Math.min(Math.max(valore, minimo), massimo);
}

/**
 * Le righe da disegnare.
 *
 * `quante` copre l'altezza della vista più il margine dei due lati; `primo`
 * parte dalla riga in cima meno il margine. Un passo non positivo — una riga
 * che non si è ancora potuta misurare — vale uno, e non una divisione per zero:
 * disegna troppe righe per un fotogramma invece di nessuna per sempre.
 */
export function finestraDa(m: Misure): Intervallo {
  const passo = m.passo > 0 ? m.passo : 1;
  const fine = Math.max(m.totale - 1, 0);
  const quante = Math.ceil(m.altezzaVista / passo) + 2 * m.margine;
  const visibileDa = stretto(Math.floor((m.scorrimento - m.cima) / passo), 0, fine);
  const primo = stretto(visibileDa - m.margine, 0, fine);
  const ultimo = Math.min(primo + quante, m.totale);
  return { primo, ultimo, visibileDa };
}

/**
 * Quante colonne ha una griglia, da `grid-template-columns` **calcolato**.
 *
 * Il valore calcolato di una griglia disegnata non è la regola del foglio
 * (`repeat(auto-fill, minmax(164px, 1fr))`) ma le tracce che ne sono uscite:
 * `"171.5px 171.5px 171.5px"`. Contarle è contare le colonne che il motore ha
 * deciso, con le misure della skin e la larghezza di adesso — che è la ragione
 * per cui si contano invece di ricalcolarle qui con un minimo scritto a mano.
 *
 * I nomi di linea (`[nome]`) non sono tracce e si saltano. Un valore non
 * risolto — la griglia non è disegnata, è dentro un pannello nascosto — dà
 * `null`: chi chiama tiene il numero di prima invece di crollare a una colonna.
 */
export function colonneDa(modello: string): number | null {
  const tracce = tracceDi(modello);
  if (tracce === null) return null;
  return Math.max(tracce.length, 1);
}

/**
 * La larghezza della prima colonna, in pixel, dallo stesso valore calcolato.
 *
 * Serve a `sizes` delle copertine: è quanto è larga una scheda **adesso**, e
 * il motore sceglie da lì quale delle misure scaricare.
 */
export function larghezzaColonnaDa(modello: string): number | null {
  const prima = tracceDi(modello)?.[0];
  if (prima === undefined || !prima.endsWith("px")) return null;
  const valore = Number.parseFloat(prima);
  return Number.isFinite(valore) && valore > 0 ? valore : null;
}

function tracceDi(modello: string): string[] | null {
  const pulito = modello.replace(/\[[^\]]*\]/g, " ").trim();
  if (pulito === "" || pulito === "none") return [];
  if (pulito.includes("(")) return null;
  return pulito.split(/\s+/);
}

/**
 * Dove va il fuoco in una griglia, premendo un tasto.
 *
 * ↑↓ di una riga, cioè di `colonne`; ←→ di una cella; Inizio e Fine al primo e
 * all'ultimo. `null` vuol dire «resta dove sei»: la freccia contro un bordo non
 * fa niente invece di andare a capo, perché una griglia che a destra
 * dell'ultima colonna ricomincia dalla prima della riga dopo fa perdere il conto
 * di dove si è.
 *
 * ↓ sull'ultima riga **piena** quando sotto c'è una riga corta, senza niente in
 * verticale: si va all'ultima cella. Fermarsi lì sarebbe dire che sotto non c'è
 * niente, e sotto c'è un disco.
 */
export function passoGriglia(
  indice: number,
  tasto: string,
  colonne: number,
  totale: number,
): number | null {
  if (totale <= 0) return null;
  const c = Math.max(Math.floor(colonne), 1);
  switch (tasto) {
    case "ArrowRight":
      return indice + 1 < totale ? indice + 1 : null;
    case "ArrowLeft":
      return indice > 0 ? indice - 1 : null;
    case "ArrowDown": {
      const sotto = indice + c;
      if (sotto < totale) return sotto;
      const riga = Math.floor(indice / c);
      const ultimaRiga = Math.floor((totale - 1) / c);
      return ultimaRiga > riga ? totale - 1 : null;
    }
    case "ArrowUp":
      return indice - c >= 0 ? indice - c : null;
    case "Home":
      return 0;
    case "End":
      return totale - 1;
    default:
      return null;
  }
}

/**
 * Quante righe di griglia servono per `totale` celle su `colonne` colonne.
 *
 * A parte perché la divisione per difetto più uno è il conto che si sbaglia: con
 * `totale` multiplo esatto di `colonne` darebbe una riga vuota in fondo.
 */
export function righeDiGriglia(totale: number, colonne: number): number {
  const c = Math.max(Math.floor(colonne), 1);
  return totale <= 0 ? 0 : Math.ceil(totale / c);
}
