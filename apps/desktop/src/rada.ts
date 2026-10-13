/**
 * Una lista rada: si sa quanto è lunga, e se ne tiene in mano solo quel che si
 * guarda.
 *
 * # Perché non bastavano le pagine che si allungano
 *
 * `usePagine` portava le righe a duecento per volta, dall'alto, e la pagina
 * dopo la chiedeva una sentinella in fondo all'elenco. Funzionava finché si
 * scorreva con la rotella. Con la barra di scorrimento no, per due ragioni che
 * sono la stessa: la barra rappresentava **le righe arrivate**, non la vista —
 * su cinquantamila brani all'apertura era lunga quanto duecento, e si
 * allungava a scatti mentre la si tirava — e per arrivare in fondo bisognava
 * passare da tutte le pagine di mezzo, duecentocinquanta richieste per
 * vedere le ultime dieci righe.
 *
 * Qui il conto delle righe arriva prima delle righe — `avvio.numeri`,
 * `cercaConteggio`, la lunghezza di una playlist — e la barra è lunga quanto
 * la vista dal primo istante. Le pagine si prendono **dove si guarda**, per
 * offset: tirando la barra fino in fondo si chiede l'ultima pagina e basta.
 *
 * # La generazione
 *
 * Ogni volta che la vista cambia identità (un altro ordinamento, un'altra
 * ricerca) o che il nucleo dice che quel che si ha in mano è vecchio, la
 * generazione cresce. Una risposta partita per una generazione precedente
 * arriva e si butta: è lo stesso patto del `giro` di `usePagine`, solo che qui
 * le richieste in volo possono essere molte e in qualunque ordine.
 *
 * # Vecchio non vuol dire vuoto
 *
 * Una pagina della generazione di prima **resta in mano** finché la nuova non
 * arriva: si disegna quel che si sa, e si chiede di nuovo. Svuotare subito
 * farebbe lampeggiare scheletri al posto delle righe a ogni voto dato, a ogni
 * brano tolto, a ogni scansione finita — per mostrare quasi sempre le stesse
 * righe di prima.
 *
 * # Puro
 *
 * Niente React e niente DOM: lo usa `useRada` in `pagine.ts`, e lo provano i
 * test di `test/rada.test.ts` con richieste finte che si risolvono nell'ordine
 * che vogliono.
 */

/**
 * Quante righe per pagina.
 *
 * Duecento come le pagine di prima: una finestra da trenta righe ne tocca una o
 * due, e una pagina sola costa al nucleo una query con `LIMIT 200`.
 */
export const MISURA_PAGINA = 200;

/**
 * Quante pagine tenere in mano al più: ottomila righe.
 *
 * Non è un tetto all'elenco, che può essere lungo quanto vuole: è quanto se ne
 * ricorda. Oltre, si dimentica la pagina guardata meno di recente — tornandoci
 * la si richiede, e costa quel che costa una pagina.
 */
export const CAPIENZA = 40;

/**
 * Quante righe chiedere oltre i bordi della finestra, per lato.
 *
 * Una finestra ferma a cavallo di due pagine non deve aspettare la seconda solo
 * quando la si vede: la si chiede quando è a mezza schermata di distanza.
 */
export const ANTICIPO = 60;

/**
 * Dopo quanto riprovare una pagina la cui richiesta è fallita.
 *
 * Non subito: la finestra chiede a ogni disegno, e un guasto che dura (un
 * database occupato da una scansione) diventerebbe un anello di richieste che
 * falliscono, ognuna con il suo avviso rosso. Neanche mai: un guasto passeggero
 * lascerebbe degli scheletri dove c'erano righe, per sempre.
 */
export const RIPROVA_MS = 2_000;

/**
 * Le pagine che coprono le righe `[primo, ultimo)`, dentro un elenco di
 * `totale`.
 *
 * Gli estremi si stringono dentro l'elenco: chi chiede una finestra con
 * l'anticipo non deve sapere dove finisce. Un elenco vuoto, o una finestra
 * vuota, non chiede niente.
 */
export function pagineDaPrendere(
  primo: number,
  ultimo: number,
  totale: number,
  misura: number = MISURA_PAGINA,
): number[] {
  const da = Math.max(primo, 0);
  const a = Math.min(ultimo, totale);
  if (a <= da || misura <= 0) return [];
  const prima = Math.floor(da / misura);
  const ultima = Math.floor((a - 1) / misura);
  const pagine: number[] = [];
  for (let p = prima; p <= ultima; p += 1) pagine.push(p);
  return pagine;
}

/** Una pagina in mano. */
interface Pagina<T> {
  righe: T[];
  /** La generazione per cui è arrivata: se non è quella di adesso, è vecchia. */
  generazione: number;
  /** Quando la si è guardata l'ultima volta: decide chi si dimentica. */
  usata: number;
}

/**
 * Quel che un elenco disegnato vuole da una lista rada: quanto è lunga, una
 * riga per volta, e come chiedere quelle che mancano.
 *
 * Un'interfaccia e non la classe, perché chi disegna non deve poter fare
 * altro: aggiornare, invalidare e azzerare sono di chi possiede la lista.
 */
export interface SorgenteRada<T> {
  /** Quante righe ha la vista, arrivate o no. */
  readonly totale: number;
  /** La riga, se è in mano; `undefined` se non è ancora arrivata. */
  riga(indice: number): T | undefined;
  /** Chiede le pagine che servono a disegnare `[primo, ultimo)`. */
  richiedi(primo: number, ultimo: number): void;
  /** Per `useSyncExternalStore`: chiama `ascoltatore` a ogni cambiamento. */
  iscrivi(ascoltatore: () => void): () => void;
  /** Per `useSyncExternalStore`: un numero che cambia a ogni cambiamento. */
  istantanea(): number;
}

/** Come si costruisce una lista rada. */
export interface OpzioniRada<T> {
  /** Una pagina: le righe da `offset`, al più `limite`. */
  chiedi: (offset: number, limite: number) => Promise<T[]>;
  /** Una richiesta fallita, una volta per pagina e per generazione. */
  onErrore?: ((errore: unknown) => void) | undefined;
  misura?: number | undefined;
  capienza?: number | undefined;
  /** L'orologio, in millisecondi. Di serie `Date.now`: i test ne passano uno. */
  adesso?: (() => number) | undefined;
}

/**
 * La lista rada: le pagine in mano, quelle in volo, e chi ascolta.
 *
 * `iscrivi` e `istantanea` sono campi e non metodi, e non per gusto:
 * `useSyncExternalStore` li riceve staccati dall'oggetto, e un metodo staccato
 * perde il suo `this`.
 */
export class ArchivioRado<T> implements SorgenteRada<T> {
  readonly misura: number;
  readonly capienza: number;
  private readonly chiedi: (offset: number, limite: number) => Promise<T[]>;
  private readonly onErrore: (errore: unknown) => void;
  private readonly adesso: () => number;

  /** Quale vista si sta tenendo: cambiarla butta tutto. */
  private chiave: string | null = null;
  /** Il totale come l'ha detto chi possiede la lista, l'ultima volta. */
  private totaleDetto = 0;
  private _totale = 0;
  private _generazione = 0;
  private readonly pagine = new Map<number, Pagina<T>>();
  private readonly inVolo = new Map<
    number,
    { generazione: number; promessa: Promise<T[] | null> }
  >();
  private readonly fallite = new Map<number, { generazione: number; quando: number }>();
  private orologio = 0;
  private versione = 0;
  private readonly ascoltatori = new Set<() => void>();

  constructor(opzioni: OpzioniRada<T>) {
    this.chiedi = opzioni.chiedi;
    this.onErrore = opzioni.onErrore ?? (() => undefined);
    this.misura = opzioni.misura ?? MISURA_PAGINA;
    this.capienza = opzioni.capienza ?? CAPIENZA;
    this.adesso = opzioni.adesso ?? Date.now;
  }

  /** Quante righe ha la vista. */
  get totale(): number {
    return this._totale;
  }

  /** La generazione di adesso: le risposte delle altre si buttano. */
  get generazione(): number {
    return this._generazione;
  }

  /** Quante pagine sono in mano, vecchie comprese. Per i test e il diario. */
  get pagineInMano(): number {
    return this.pagine.size;
  }

  readonly iscrivi = (ascoltatore: () => void): (() => void) => {
    this.ascoltatori.add(ascoltatore);
    return () => {
      this.ascoltatori.delete(ascoltatore);
    };
  };

  readonly istantanea = (): number => this.versione;

  /**
   * Dice quale vista si tiene, e quanto è lunga.
   *
   * Una chiave nuova butta tutto: è un altro elenco, e le righe di prima non ne
   * sono una versione vecchia. Lo stesso elenco con un totale nuovo — un
   * preferito tolto, una scansione finita — è un elenco cambiato sotto, e si
   * invalida senza buttare: le righe restano a schermo finché non arrivano le
   * nuove.
   *
   * Il totale conta solo quando **cambia quello detto**: la lista lo corregge
   * da sé quando una pagina arriva più corta o più lunga del previsto, e senza
   * questo confronto il valore vecchio di chi la possiede la riporterebbe
   * indietro a ogni disegno.
   */
  imposta(chiave: string, totale: number): void {
    const n = Math.max(0, Math.floor(totale));
    if (chiave !== this.chiave) {
      this.chiave = chiave;
      this.totaleDetto = n;
      this._totale = n;
      this.pagine.clear();
      this.inVolo.clear();
      this.fallite.clear();
      this._generazione += 1;
      this.cambia();
      return;
    }
    if (n === this.totaleDetto) return;
    this.totaleDetto = n;
    this._totale = n;
    this.togliOltre();
    this.invalida();
  }

  /**
   * Quel che si ha in mano è vecchio: si continua a mostrarlo, e si richiede.
   *
   * Non chiede niente da sé. Le pagine si richiedono quando qualcuno le
   * guarda, cioè quando l'elenco visibile chiama `richiedi`: una vista nascosta
   * invalidata non costa niente finché non torna a schermo.
   */
  invalida(): void {
    this._generazione += 1;
    this.fallite.clear();
    this.cambia();
  }

  /** La riga, se è in mano: anche da una pagina vecchia. */
  riga(indice: number): T | undefined {
    if (indice < 0 || indice >= this._totale) return undefined;
    const p = Math.floor(indice / this.misura);
    return this.pagine.get(p)?.righe[indice - p * this.misura];
  }

  /** La prima riga in mano che risponde a `predicato`, in qualunque pagina. */
  cerca(predicato: (riga: T) => boolean): T | undefined {
    for (const pagina of this.pagine.values()) {
      const trovata = pagina.righe.find(predicato);
      if (trovata !== undefined) return trovata;
    }
    return undefined;
  }

  richiedi(primo: number, ultimo: number): void {
    for (const p of pagineDaPrendere(
      primo - ANTICIPO,
      ultimo + ANTICIPO,
      this._totale,
      this.misura,
    )) {
      void this.prendi(p);
    }
  }

  /**
   * Le righe `[da, a]`, comprese, chiedendo le pagine che mancano.
   *
   * Serve alla selezione con Maiusc, che può prendere un intervallo più lungo
   * di quel che è in mano. Una riga che non arriva — la generazione è cambiata
   * nel mezzo, una pagina è fallita — resta `undefined`, e chi chiama decide.
   */
  async intervallo(da: number, a: number): Promise<(T | undefined)[]> {
    const pagine = pagineDaPrendere(da, a + 1, this._totale, this.misura);
    await Promise.all(pagine.map((p) => this.prendi(p)));
    const righe: (T | undefined)[] = [];
    for (let i = Math.max(da, 0); i <= Math.min(a, this._totale - 1); i += 1) {
      righe.push(this.riga(i));
    }
    return righe;
  }

  /**
   * Riscrive le righe in mano, pagina per pagina.
   *
   * Per gli aggiornamenti ottimistici: un voto, un cuore, una riga tolta. Una
   * funzione che **toglie** righe accorcia la sua pagina, e le righe dopo dentro
   * la stessa pagina salgono di un posto: è giusto per un attimo, finché il
   * totale nuovo non invalida e la verità torna dal nucleo.
   */
  aggiorna(f: (righe: T[]) => T[]): void {
    for (const [p, pagina] of this.pagine) {
      this.pagine.set(p, { ...pagina, righe: f(pagina.righe) });
    }
    this.cambia();
  }

  /**
   * Una pagina della generazione di adesso: quella in mano, quella già in
   * volo, o una richiesta nuova. `null` se non arriva.
   */
  private prendi(p: number): Promise<T[] | null> {
    const generazione = this._generazione;
    const pagina = this.pagine.get(p);
    if (pagina !== undefined && pagina.generazione === generazione) {
      this.orologio += 1;
      pagina.usata = this.orologio;
      return Promise.resolve(pagina.righe);
    }
    const volo = this.inVolo.get(p);
    if (volo !== undefined && volo.generazione === generazione) return volo.promessa;
    const fallita = this.fallite.get(p);
    if (
      fallita !== undefined &&
      fallita.generazione === generazione &&
      this.adesso() - fallita.quando < RIPROVA_MS
    ) {
      return Promise.resolve(null);
    }

    let richiesta: Promise<T[]>;
    try {
      richiesta = this.chiedi(p * this.misura, this.misura);
    } catch (errore) {
      richiesta = Promise.reject(errore);
    }
    const promessa = richiesta.then(
      (righe) => (this.accetta(p, generazione, righe) ? righe : null),
      (errore: unknown) => {
        this.rifiuta(p, generazione, errore);
        return null;
      },
    );
    this.inVolo.set(p, { generazione, promessa });
    return promessa;
  }

  /**
   * Una pagina arrivata. Falso se era per un'altra generazione, e allora si
   * butta.
   *
   * Qui il totale si corregge da sé: una pagina più corta della misura che
   * finisce prima del totale dice dove l'elenco finisce davvero, una che va
   * oltre dice che l'elenco è cresciuto. Il conteggio che si è ricevuto può
   * essere di un istante prima — una scansione in corso, un preferito messo da
   * un altro dispositivo — e uno scheletro che non si riempie mai, in fondo a
   * un elenco, sarebbe la forma peggiore di quell'istante.
   */
  private accetta(p: number, generazione: number, righe: T[]): boolean {
    const volo = this.inVolo.get(p);
    if (volo !== undefined && volo.generazione === generazione) this.inVolo.delete(p);
    if (generazione !== this._generazione) return false;

    const inizio = p * this.misura;
    const fine = inizio + righe.length;
    if (righe.length < this.misura && fine < this._totale) {
      this._totale = fine;
      this.togliOltre();
    } else if (fine > this._totale) {
      this._totale = fine;
    }
    this.orologio += 1;
    this.pagine.set(p, { righe, generazione, usata: this.orologio });
    this.fallite.delete(p);
    this.sfoltisci();
    this.cambia();
    return true;
  }

  private rifiuta(p: number, generazione: number, errore: unknown): void {
    const volo = this.inVolo.get(p);
    if (volo !== undefined && volo.generazione === generazione) this.inVolo.delete(p);
    if (generazione !== this._generazione) return;
    this.fallite.set(p, { generazione, quando: this.adesso() });
    this.onErrore(errore);
  }

  /** Le pagine che stanno tutte oltre la fine: non sono più di nessuno. */
  private togliOltre(): void {
    for (const p of [...this.pagine.keys()]) {
      if (p * this.misura >= this._totale) this.pagine.delete(p);
    }
  }

  /** Dimentica le pagine guardate meno di recente, oltre la capienza. */
  private sfoltisci(): void {
    while (this.pagine.size > this.capienza) {
      let vecchia: number | null = null;
      let quando = Number.POSITIVE_INFINITY;
      for (const [p, pagina] of this.pagine) {
        if (pagina.usata < quando) {
          quando = pagina.usata;
          vecchia = p;
        }
      }
      if (vecchia === null) return;
      this.pagine.delete(vecchia);
    }
  }

  private cambia(): void {
    this.versione += 1;
    for (const ascoltatore of [...this.ascoltatori]) ascoltatore();
  }
}

/**
 * Una lista piena vista come rada: tutto in mano, niente da chiedere.
 *
 * Serve a chi tratta allo stesso modo l'elenco che si guarda, da qualunque
 * parte venga: una playlist e un album arrivano interi, la libreria no.
 */
export interface Elenco<T> {
  /** Quante righe ha la vista. */
  readonly totale: number;
  /** La riga, se è in mano. */
  riga(indice: number): T | undefined;
  /** La prima riga in mano che risponde a `predicato`. */
  cerca(predicato: (riga: T) => boolean): T | undefined;
}

/** Un array come `Elenco`. */
export function elencoPieno<T>(righe: readonly T[]): Elenco<T> {
  return {
    totale: righe.length,
    riga: (indice) => righe[indice],
    cerca: (predicato) => righe.find(predicato),
  };
}
