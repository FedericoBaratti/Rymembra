/**
 * Gli «Annulla» a tempo: la coda sostituita o svuotata (`parti/AvvisoCoda.tsx`)
 * e la playlist eliminata (`App.tsx`).
 *
 * # Il difetto
 *
 * Il riquadro con «Annulla» restava a schermo dieci secondi e poi spariva,
 * comunque. Chi usa la tastiera o un lettore di schermo lo trovava lontano dal
 * fuoco — in cima alla pagina, dopo un menù aperto col tasto destro o un Invio
 * su una riga — e per arrivarci servivano molti Tab: spesso il riquadro se ne
 * andava proprio mentre il fuoco ci arrivava, e il fuoco cadeva sul `<body>`.
 * Per la playlist, che non chiedeva niente prima, l'unica rete di sicurezza
 * era in pratica irraggiungibile.
 *
 * # Le tre cose che lo rimettono a portata (D-40)
 *
 * 1. **Il conto si ferma** mentre il riquadro ha il fuoco o il puntatore
 *    sopra, e riparte **da capo** quando li perde: chi ci è arrivato non se
 *    lo vede sparire sotto le dita (`contoAnnulla`).
 * 2. **`Ctrl+Z` fa l'ultimo «Annulla» ancora a schermo**, dovunque sia il
 *    fuoco (`ANNULLI`, letto da `tastiera.ts`). Mentre si scrive in un campo
 *    `Ctrl+Z` resta al campo.
 * 3. Eliminare una playlist chiede prima conferma: lo fa `App`, con
 *    `ConfermaGesto`. L'«Annulla» dopo resta.
 *
 * # Un posto solo per lo stato
 *
 * Tutti e due i riquadri si disegnano in due posti — nella pagina e sopra
 * «In riproduzione» — e prima ogni copia teneva il suo stato e il suo conto.
 * Con `Ctrl+Z` questo non andava più: il gesto avrebbe chiuso una copia e
 * lasciato l'altra col suo «Annulla» ancora premibile, cioè un secondo
 * ripristino dello stesso gesto. Qui lo stato, il conto e l'offerta a `Ctrl+Z`
 * stanno una volta (`annullaATempo`), e le copie li leggono.
 *
 * Puro e senza DOM: `test/annullaATempo.test.ts` lo prova con un orologio
 * finto.
 */
import { creaArchivio, type Leggibile } from "./archivio";

/**
 * Quanto resta a schermo un «Annulla» che nessuno tiene, in millisecondi.
 *
 * Dieci secondi, più dei sei dell'uscita spostata in `AvvisoAudio`: quello si
 * legge e basta, questo chiede un gesto, e chi si accorge di aver perso la coda
 * se ne accorge quando la musica cambia — un paio di secondi dopo il clic.
 */
export const DURATA_ANNULLA = 10_000;

/** Il tempo, da fuori: quello della finestra, o uno finto nelle prove. */
export interface Orologio {
  imposta(fai: () => void, ms: number): number;
  togli(id: number): void;
}

const OROLOGIO_DELLA_FINESTRA: Orologio = {
  imposta: (fai, ms) => window.setTimeout(fai, ms),
  togli: (id) => window.clearTimeout(id),
};

/** Chi tiene fermo il conto. */
export type Presa = "fuoco" | "puntatore";

/** Il conto di un «Annulla» a schermo. */
export interface ContoAnnulla {
  /** Un avviso nuovo: il conto ricomincia da capo, se nessuno lo tiene. */
  parti(): void;
  /** Il fuoco o il puntatore sono sul riquadro: il conto si ferma. */
  tieni(chi: Presa): void;
  /** Se ne sono andati: quando nessuno lo tiene più, riparte da capo. */
  lascia(chi: Presa): void;
  /** Il riquadro non c'è più: niente scadenza, e niente prese. */
  ferma(): void;
}

/** Il conto: vedi la nota del modulo, punto 1. */
export function contoAnnulla(
  scade: () => void,
  durata = DURATA_ANNULLA,
  orologio: Orologio = OROLOGIO_DELLA_FINESTRA,
): ContoAnnulla {
  let timer: number | null = null;
  let acceso = false;
  const prese = new Set<Presa>();
  const togli = () => {
    if (timer === null) return;
    orologio.togli(timer);
    timer = null;
  };
  const arma = () => {
    togli();
    if (!acceso || prese.size > 0) return;
    timer = orologio.imposta(() => {
      timer = null;
      acceso = false;
      scade();
    }, durata);
  };
  return {
    parti: () => {
      acceso = true;
      arma();
    },
    tieni: (chi) => {
      prese.add(chi);
      togli();
    },
    lascia: (chi) => {
      if (prese.delete(chi)) arma();
    },
    ferma: () => {
      acceso = false;
      // Un riquadro che sparisce col puntatore sopra non manda
      // `pointerleave`: la presa si dimentica qui.
      prese.clear();
      togli();
    },
  };
}

/** Gli «Annulla» a schermo, per `Ctrl+Z`: vedi la nota del modulo, punto 2. */
export interface PilaAnnulli {
  /** Un «Annulla» è a schermo. Restituisce come lo si ritira. */
  offri(fai: () => void): () => void;
  /** Fa l'ultimo offerto ancora a schermo. `false` se non ce n'è nessuno. */
  annullaLUltimo(): boolean;
}

export function pilaAnnulli(): PilaAnnulli {
  const voci: { fai: () => void }[] = [];
  return {
    offri: (fai) => {
      const voce = { fai };
      voci.push(voce);
      return () => {
        const dove = voci.indexOf(voce);
        if (dove !== -1) voci.splice(dove, 1);
      };
    },
    annullaLUltimo: () => {
      const voce = voci.pop();
      if (voce === undefined) return false;
      voce.fai();
      return true;
    },
  };
}

/** La pila della finestra: la leggono `tastiera.ts` e chi mostra un «Annulla». */
export const ANNULLI = pilaAnnulli();

/** I gestori da mettere sul riquadro: il fuoco e il puntatore lo tengono. */
export interface GestoriPresa {
  onFocus: () => void;
  onBlur: (e: { currentTarget: Node; relatedTarget: EventTarget | null }) => void;
  onPointerEnter: () => void;
  onPointerLeave: () => void;
}

function gestoriDi(conto: ContoAnnulla): GestoriPresa {
  return {
    onFocus: () => conto.tieni("fuoco"),
    // Il fuoco che passa da «Annulla» alla × resta dentro: non è un lasciare.
    onBlur: (e) => {
      if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
      conto.lascia("fuoco");
    },
    onPointerEnter: () => conto.tieni("puntatore"),
    onPointerLeave: () => conto.lascia("puntatore"),
  };
}

/** Un «Annulla» a tempo intero: lo stato, il conto, l'offerta a `Ctrl+Z`. */
export interface AnnullaATempo<T> {
  /** L'avviso a schermo, o `null`: si legge e si segue (`useArchivio`). */
  readonly avviso: Leggibile<T | null>;
  /** Mostra un avviso nuovo — il conto riparte — o lo toglie con `null`. */
  mostra(avviso: T | null): void;
  /** «Annulla» (il tasto, o `Ctrl+Z`): toglie l'avviso e fa il gesto. */
  annulla(): void;
  /** Da mettere sul riquadro, su ogni copia. */
  readonly presa: GestoriPresa;
}

/**
 * Lo stato di un «Annulla», una volta per tutte le copie che lo disegnano.
 *
 * `fai` è il gesto che rimette le cose com'erano; parte una volta sola per
 * avviso, che venga dal tasto o da `Ctrl+Z`, perché l'avviso si toglie prima.
 */
export function annullaATempo<T>(
  fai: (avviso: T) => void,
  opzioni: { durata?: number; orologio?: Orologio; pila?: PilaAnnulli } = {},
): AnnullaATempo<T> {
  const archivio = creaArchivio<T | null>(null);
  const pila = opzioni.pila ?? ANNULLI;
  let ritira: (() => void) | null = null;
  const mostra = (avviso: T | null) => {
    ritira?.();
    ritira = null;
    archivio.scrivi(avviso);
    if (avviso === null) {
      conto.ferma();
      return;
    }
    conto.parti();
    ritira = pila.offri(() => annullaQuesto(avviso));
  };
  const annullaQuesto = (avviso: T) => {
    if (archivio.leggi() !== avviso) return;
    mostra(null);
    fai(avviso);
  };
  const conto = contoAnnulla(
    () => mostra(null),
    opzioni.durata ?? DURATA_ANNULLA,
    opzioni.orologio ?? OROLOGIO_DELLA_FINESTRA,
  );
  return {
    avviso: archivio,
    mostra,
    annulla: () => {
      const avviso = archivio.leggi();
      if (avviso !== null) annullaQuesto(avviso);
    },
    presa: gestoriDi(conto),
  };
}
