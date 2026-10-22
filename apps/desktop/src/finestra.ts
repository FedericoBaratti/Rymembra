/**
 * I conti della finestra virtuale, senza il DOM.
 *
 * Stavano dentro `useVirtuale`, mescolati alle letture di `getBoundingClientRect`,
 * e lì non si potevano provare: un `cima` sbagliato o una colonna di troppo si
 * vedono solo scorrendo, in fondo a una libreria vera. Qui sono numeri che
 * entrano e numeri che escono, e li provano i test di `test/finestra.test.ts`.
 *
 * Quattro conti: quali righe disegnare per uno scorrimento dato — con un passo
 * solo, o su righe di altezza diversa (la coda) —, quante colonne ha una
 * griglia che si dispone da sé (`repeat(auto-fill, …)`), e dove va il fuoco in
 * una griglia quando si preme una freccia.
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
 * Dove comincia ogni riga, quando le righe non sono alte uguale.
 *
 * `scala[i]` è la cima della riga `i` dall'inizio dell'elenco, e `scala[totale]`
 * l'altezza dell'elenco intero: una voce in più delle righe perché la paglia di
 * sotto sia `scala[totale] - scala[ultimo]` senza un caso a parte per la fine.
 *
 * Una riga mai disegnata non ha una misura e vale `stima`: quanto è alta davvero
 * lo dice il disegno, e fino ad allora il suo posto è la paglia. Diciottomila
 * somme sono meno di un millesimo di secondo, e chi chiama la rifà solo quando
 * una misura cambia.
 */
export function scalaDa(
  altezze: ReadonlyMap<number, number>,
  totale: number,
  stima: number,
): Float64Array {
  const righe = Math.max(totale, 0);
  const scala = new Float64Array(righe + 1);
  let somma = 0;
  for (let i = 0; i < righe; i++) {
    somma += altezze.get(i) ?? stima;
    scala[i + 1] = somma;
  }
  return scala;
}

/**
 * La riga che contiene il punto `y`: l'ultima che comincia prima di lui o lì.
 *
 * Una ricerca binaria e non una divisione, perché con righe diverse non c'è un
 * passo per cui dividere. Fuori dai bordi dà la prima riga o la fine.
 */
function rigaA(scala: ArrayLike<number>, y: number): number {
  let basso = 0;
  let alto = scala.length - 1;
  while (basso < alto) {
    const mezzo = (basso + alto + 1) >> 1;
    if ((scala[mezzo] ?? 0) <= y) basso = mezzo;
    else alto = mezzo - 1;
  }
  return basso;
}

/** Quel che serve per la finestra su righe di altezza diversa. */
export interface MisureScala {
  /** `scrollTop` dello scorrevole. */
  scorrimento: number;
  /** Dove comincia la prima riga dentro lo scorrevole, in pixel. */
  cima: number;
  /** `clientHeight` dello scorrevole. */
  altezzaVista: number;
  /** Da `scalaDa`: una voce più delle righe. */
  scala: ArrayLike<number>;
  /** Righe in più per lato. */
  margine: number;
  /** La finestra disegnata adesso: la si tiene finché basta (vedi sotto). */
  tenere?: { primo: number; ultimo: number } | undefined;
}

/**
 * Le righe da disegnare, come `finestraDa`, ma sulla scala invece che su un
 * passo.
 *
 * Il margine resta in **righe** e non in pixel: serve a coprire il fotogramma
 * che il calcolo arriva in ritardo, e ogni riga di margine vale almeno una riga
 * di rotella. `visibili` — le righe intere che entrano dalla prima in vista —
 * qui si conta, perché non è più l'altezza della vista divisa per un passo.
 *
 * `tenere` è la finestra disegnata adesso, e resta quella finché copre la vista
 * con almeno metà del margine per lato. Senza, la finestra scattava avanti e
 * indietro di una riga all'infinito: una riga misurata mezzo pixel diversa da
 * come è disegnata sposta il contenuto di quel mezzo pixel, lo scroll anchoring
 * del motore lo corregge, la correzione si arrotonda al pixel del dispositivo
 * (1,11 px CSS con lo zoom a 0,9), e lo scorrimento passa il confine fra due
 * righe — che sceglie l'altra finestra, che corregge all'indietro.
 */
export function finestraDaScala(m: MisureScala): Intervallo & { visibili: number } {
  const totale = Math.max(m.scala.length - 1, 0);
  const fine = Math.max(totale - 1, 0);
  const alto = m.scorrimento - m.cima;
  const visibileDa = stretto(rigaA(m.scala, alto), 0, fine);
  const inFondo = Math.max(rigaA(m.scala, alto + m.altezzaVista), visibileDa);
  const cimaVisibile = m.scala[visibileDa] ?? 0;
  const entrano = rigaA(m.scala, cimaVisibile + m.altezzaVista) - visibileDa;
  const visibili = Math.max(entrano, 1);
  const t = m.tenere;
  if (t !== undefined && t.primo < t.ultimo && t.ultimo <= totale) {
    const mezzo = Math.floor(m.margine / 2);
    if (
      t.primo <= Math.max(visibileDa - mezzo, 0) &&
      t.ultimo >= Math.min(inFondo + 1 + mezzo, totale)
    ) {
      return { primo: t.primo, ultimo: t.ultimo, visibileDa, visibili };
    }
  }
  const primo = stretto(visibileDa - m.margine, 0, fine);
  const ultimo = Math.min(inFondo + 1 + m.margine, totale);
  return { primo, ultimo, visibileDa, visibili };
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
