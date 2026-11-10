/**
 * Le decisioni dei gesti del telefono, senza il dito.
 *
 * Un trascinamento ha due metà: seguire il dito mentre si muove, e decidere
 * cosa vuol dire quando si stacca. La prima sta in `dito.ts`, ed è fatta di
 * eventi e di fotogrammi; la seconda sta qui, ed è fatta di numeri — quanto ci
 * si è spostati, quanto in fretta, in quale direzione. Separate perché la
 * seconda è quella che si sbaglia (un foglio che si chiude da solo, una
 * copertina che cambia brano a metà di uno scorrimento) e si prova con
 * `node --test` senza un telefono in mano.
 *
 * # Le soglie, e da dove vengono
 *
 * Non sono inventate: sono quelle a cui un telefono ha abituato la mano.
 *
 * - **Dieci pixel** prima di decidere l'asse: sotto, un tocco che trema è
 *   ancora un tocco. È la `touchSlop` di Android (8 dp), arrotondata in su
 *   perché la WebView misura in pixel CSS e un dito su vetro trema di più di
 *   uno stilo.
 * - **Un quarto** dell'altezza per chiudere un foglio trascinandolo: meno, e
 *   un foglio si chiude mentre si cercava di scorrerlo; di più, e per chiuderlo
 *   bisogna portare il dito a metà schermo.
 * - **Mezzo pixel al millisecondo** di velocità, cioè un colpo deciso: chiude
 *   anche se il dito ha fatto poca strada. È la differenza fra «lascio andare»
 *   e «butto via».
 * - **Ottanta millisecondi** di campioni per misurare la velocità: quella che
 *   conta è quella del rilascio, non la media del gesto. Un dito che ha
 *   trascinato piano e alla fine si è fermato non sta lanciando niente.
 */

/** Quanto il dito si muove prima che il gesto abbia una direzione. */
export const SOGLIA_ASSE = 10;

/** Il tratto di gesto che conta per la velocità del rilascio, in millisecondi. */
export const FINESTRA_VELOCITA = 80;

/** Un colpo: oltre questa velocità il gesto vale anche se è corto. */
export const VELOCITA_COLPO = 0.5;

/** Un punto del gesto, con quando. */
export interface Campione {
  t: number;
  x: number;
  y: number;
}

/**
 * In quale direzione va il gesto, o `null` finché non lo si sa.
 *
 * L'asse vince se è il doppio dell'altro: un dito che scende un po' in diagonale
 * sta scendendo. Pari o quasi, si aspetta ancora — e oltre la soglia vince il
 * più lungo, perché un gesto che resta indeciso per sempre è un gesto perso.
 */
export function asseDelGesto(
  dx: number,
  dy: number,
  soglia: number = SOGLIA_ASSE,
): "x" | "y" | null {
  const ax = Math.abs(dx);
  const ay = Math.abs(dy);
  if (Math.max(ax, ay) < soglia) return null;
  if (ax >= ay * 2) return "x";
  if (ay >= ax * 2) return "y";
  if (Math.max(ax, ay) < soglia * 2) return null;
  return ax > ay ? "x" : "y";
}

/**
 * La velocità del rilascio, in pixel al millisecondo.
 *
 * Dagli ultimi campioni che stanno dentro `FINESTRA_VELOCITA` dall'ultimo: il
 * primo e l'ultimo di quel tratto. Con meno di due campioni, o un tratto di
 * durata nulla, la velocità è zero — non c'è un lancio da misurare.
 */
export function velocitaDa(
  campioni: readonly Campione[],
  finestra: number = FINESTRA_VELOCITA,
): { vx: number; vy: number } {
  const ultimo = campioni.at(-1);
  if (ultimo === undefined) return { vx: 0, vy: 0 };
  let primo = ultimo;
  for (let i = campioni.length - 2; i >= 0; i--) {
    const c = campioni[i];
    if (c === undefined || ultimo.t - c.t > finestra) break;
    primo = c;
  }
  const dt = ultimo.t - primo.t;
  if (dt <= 0) return { vx: 0, vy: 0 };
  return { vx: (ultimo.x - primo.x) / dt, vy: (ultimo.y - primo.y) / dt };
}

/**
 * Un foglio trascinato verso il basso e lasciato: si chiude, o torna su?
 *
 * Si chiude oltre un quarto della sua altezza, o con un colpo verso il basso.
 * Un colpo verso l'**alto** lo rimette su anche da lontano: chi ha cambiato
 * idea a metà strada lo dice così.
 */
export function esitoTrascinamento({
  dy,
  vy,
  altezza,
}: {
  dy: number;
  vy: number;
  altezza: number;
}): "chiudi" | "torna" {
  if (vy <= -VELOCITA_COLPO) return "torna";
  if (vy >= VELOCITA_COLPO && dy > 0) return "chiudi";
  return dy > altezza / 4 ? "chiudi" : "torna";
}

/**
 * Una strisciata di lato: il brano dopo, quello prima, o niente.
 *
 * Verso sinistra è «dopo» — la copertina se ne va e arriva quella che stava a
 * destra, come si sfogliano le pagine — e verso destra è «prima». Vale oltre un
 * terzo della larghezza, o con un colpo nella stessa direzione dello
 * spostamento: un colpo contrario a quel che si è trascinato è un ripensamento.
 */
export function esitoStrisciata({
  dx,
  vx,
  larghezza,
}: {
  dx: number;
  vx: number;
  larghezza: number;
}): "prossimo" | "precedente" | null {
  const colpo = Math.abs(vx) >= VELOCITA_COLPO && Math.sign(vx) === Math.sign(dx);
  if (!colpo && Math.abs(dx) < larghezza / 3) return null;
  if (dx === 0) return null;
  return dx < 0 ? "prossimo" : "precedente";
}

/**
 * Lo spostamento contro un bordo che non cede.
 *
 * Un foglio tirato verso l'alto non ha dove andare, ma un foglio che non si
 * muove affatto sembra rotto: si sposta di poco, sempre meno quanto più lo si
 * tira, e non oltre `limite`. È la curva di iOS e di Android per lo stesso
 * caso, semplificata: tende a `limite` e non lo raggiunge.
 */
export function elastico(delta: number, limite: number): number {
  if (limite <= 0) return 0;
  const segno = Math.sign(delta);
  const a = Math.abs(delta);
  return segno * limite * (1 - 1 / (a / limite + 1));
}
