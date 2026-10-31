/**
 * Annulla e ripeti, sul testo del documento.
 *
 * # Perché serve, e perché non basta quello del browser
 *
 * La sorgente è uno stato controllato, e ogni controllo — un cursore, un
 * elenco, una casella — la riscrive da capo. La pila di annullo del browser,
 * che tiene traccia di quel che l'utente ha *digitato*, non sa cosa farsene di
 * un valore comparso dall'esterno: si svuota. Quindi trascinare un cursore non
 * si poteva annullare, e cancellava anche la possibilità di annullare quel che
 * si era scritto a mano un momento prima.
 *
 * Il testo intero costa poco da conservare: un documento di skin sta in
 * qualche decina di kilobyte, e duecento passi sono qualche megabyte nel caso
 * peggiore. Un annullo strutturato — quale campo, quale valore prima — sarebbe
 * un secondo modello da tenere allineato al primo, cioè esattamente quel che
 * `patch.ts` esiste per non fare.
 *
 * # Un riduttore, e perché
 *
 * La prima versione era un hook con tre `useState` e i `setState` chiamati
 * **dentro** gli aggiornatori degli altri: in modalità rigorosa React li
 * chiama due volte, e un annullo spostava due passi nel futuro. Adesso la
 * storia è un valore e queste sono funzioni pure — si provano con
 * `node --test` senza un DOM, e lo Studio le chiama dal suo riduttore.
 *
 * # I passi hanno un nome
 *
 * «Annulla» da solo non dice cosa annulla. Ogni scrittura porta un'etichetta —
 * «Raggio di section-card: 14px» — e il tasto la mostra. L'etichetta è per chi
 * legge; il **gruppo** è per l'accorpamento: `parts.section-card.radius` resta
 * lo stesso mentre il numero cambia sotto il dito.
 *
 * # L'accorpamento: stesso gruppo, senza pause
 *
 * Un trascinamento produce trenta scritture in mezzo secondo, e trenta passi
 * per un gesto solo sono peggio di nessuno. Le scritture dello stesso gruppo
 * che si susseguono senza una pausa di mezzo secondo sono lo stesso gesto; un
 * gruppo diverso, o una pausa, apre un passo nuovo.
 *
 * # Ma un gesto non dura per sempre
 *
 * Chi scrive nel codice di seguito, senza mai fermarsi mezzo secondo, è una
 * raffica sola: fino alla 3.0 un minuto di battitura era **un** passo, e
 * Ctrl+Z buttava via tutto il minuto per togliere l'ultima parola. Adesso una
 * raffica si chiude da sé dopo [`DURATA_RAFFICA_MS`]: annullare toglie al più
 * gli ultimi due secondi di lavoro, che è quel che un annullo promette.
 */

/** Quanto silenzio separa due gesti. */
export const PAUSA_MS = 500;

/** Quanto può durare un gesto solo, anche senza pause. */
export const DURATA_RAFFICA_MS = 2_000;

/** Quanti passi si conservano. Oltre, i più vecchi cadono. */
export const PROFONDITA = 200;

/** Un punto della storia: il testo, e la modifica che ci ha portato. */
export interface Passo {
  readonly testo: string;
  /** Quel che si legge sul tasto: «Raggio di section-card: 14px». */
  readonly etichetta: string;
}

export interface Storia {
  /** I punti prima di questo, dal più vecchio. */
  readonly passato: readonly Passo[];
  readonly presente: Passo;
  /** I punti dopo questo, dal più vicino. */
  readonly futuro: readonly Passo[];
  /** Il gruppo dell'ultima scrittura, per l'accorpamento. `null` dopo un salto. */
  readonly gruppo: string | null;
  /** Quando è arrivata l'ultima scrittura, in millisecondi. */
  readonly ultimoTocco: number;
  /** Quando è cominciata la raffica aperta: vedi [`DURATA_RAFFICA_MS`]. */
  readonly inizioRaffica: number;
}

/** Una scrittura, con quel che serve a raccontarla e ad accorparla. */
export interface Scrittura {
  readonly testo: string;
  readonly etichetta: string;
  /** Scritture dello stesso gruppo, senza pause, sono un passo solo. */
  readonly gruppo: string;
  /** L'ora della scrittura: chi chiama la passa, così la funzione resta pura. */
  readonly adesso: number;
}

/**
 * Una storia che comincia da un testo, senza passato.
 *
 * È l'apertura del documento: annullare fino a «prima che il file esistesse»
 * non vuol dire niente, e lasciarlo fare mostrerebbe una stringa vuota al
 * posto della skin.
 */
export function storiaDa(testo: string, etichetta = ""): Storia {
  return {
    passato: [],
    presente: { testo, etichetta },
    futuro: [],
    gruppo: null,
    ultimoTocco: 0,
    inizioRaffica: 0,
  };
}

/** Scrive, e apre un passo (o allunga quello aperto). */
export function scrivi(storia: Storia, scrittura: Scrittura): Storia {
  // Una scrittura che non cambia niente non è un passo. Capita più di quanto
  // sembri: `scriviIn` restituisce la sorgente intatta quando il testo non è
  // JSON, e senza questa guardia il documento rotto riempirebbe la pila.
  if (scrittura.testo === storia.presente.testo) return storia;

  const presente: Passo = { testo: scrittura.testo, etichetta: scrittura.etichetta };
  const stessaRaffica =
    storia.gruppo === scrittura.gruppo &&
    scrittura.adesso - storia.ultimoTocco < PAUSA_MS &&
    scrittura.adesso - storia.inizioRaffica < DURATA_RAFFICA_MS &&
    storia.passato.length > 0;

  return {
    // Dentro la raffica il passo resta uno: si sposta solo il punto d'arrivo,
    // e il punto di partenza è ancora quello di prima.
    passato: stessaRaffica
      ? storia.passato
      : [...storia.passato, storia.presente].slice(-PROFONDITA),
    presente,
    futuro: [],
    gruppo: scrittura.gruppo,
    ultimoTocco: scrittura.adesso,
    inizioRaffica: stessaRaffica ? storia.inizioRaffica : scrittura.adesso,
  };
}

/** Un passo indietro. La stessa storia, se non ce n'è. */
export function annulla(storia: Storia): Storia {
  const indietro = storia.passato[storia.passato.length - 1];
  if (indietro === undefined) return storia;
  return {
    passato: storia.passato.slice(0, -1),
    presente: indietro,
    futuro: [storia.presente, ...storia.futuro].slice(0, PROFONDITA),
    // Il prossimo tocco apre un passo nuovo: senza, scrivere subito dopo un
    // annullo si accorperebbe al passo che si è appena disfatto.
    gruppo: null,
    ultimoTocco: 0,
    inizioRaffica: 0,
  };
}

/** Un passo avanti. La stessa storia, se non ce n'è. */
export function ripeti(storia: Storia): Storia {
  const avanti = storia.futuro[0];
  if (avanti === undefined) return storia;
  return {
    passato: [...storia.passato, storia.presente].slice(-PROFONDITA),
    presente: avanti,
    futuro: storia.futuro.slice(1),
    gruppo: null,
    ultimoTocco: 0,
    inizioRaffica: 0,
  };
}

/** Quel che «Annulla» disfarebbe, o `null`. */
export function cosaAnnulla(storia: Storia): string | null {
  return storia.passato.length > 0 ? storia.presente.etichetta : null;
}

/** Quel che «Ripeti» rifarebbe, o `null`. */
export function cosaRipete(storia: Storia): string | null {
  return storia.futuro[0]?.etichetta ?? null;
}

/**
 * La stessa storia con ogni testo passato per `trasforma`: passato, presente
 * e futuro.
 *
 * Per i cambiamenti che non sono un gesto da annullare ma un fatto che vale
 * per ogni versione — un file tolto dal pacchetto non torna con Ctrl+Z, e le
 * dichiarazioni che lo usavano non devono tornare nemmeno loro (`risorse.ts`).
 * Un passo che dopo la trasformazione è uguale a quello prima non cambia più
 * niente, e si toglie: un annullo che non annulla niente direbbe una cosa
 * falsa. Le etichette restano quelle di chi ha fatto i passi.
 */
export function riscritta(storia: Storia, trasforma: (testo: string) => string): Storia {
  const passi = [...storia.passato, storia.presente, ...storia.futuro].map((p) => ({
    testo: trasforma(p.testo),
    etichetta: p.etichetta,
  }));
  const qui = storia.passato.length;
  const tenuti: Passo[] = [];
  let presente = 0;
  passi.forEach((p, i) => {
    const prima = tenuti[tenuti.length - 1];
    if (prima === undefined || prima.testo !== p.testo) tenuti.push(p);
    if (i === qui) presente = tenuti.length - 1;
  });
  return {
    passato: tenuti.slice(0, presente),
    presente: tenuti[presente] ?? storia.presente,
    futuro: tenuti.slice(presente + 1),
    // Il prossimo tocco apre un passo nuovo, come dopo un annullo.
    gruppo: null,
    ultimoTocco: 0,
    inizioRaffica: 0,
  };
}
