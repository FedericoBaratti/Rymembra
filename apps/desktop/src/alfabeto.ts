/**
 * Sotto quale lettera sta un nome, per l'indice alfabetico degli artisti.
 *
 * # Perché il confronto lo fa `Intl.Collator`
 *
 * Fino alla 3.0 la lettera era il primo carattere della chiave d'ordinamento,
 * cercato fra le ventisei dell'alfabeto: «é» non è «e», e un artista che
 * cominciava con una lettera accentata finiva sotto «#», insieme ai numeri.
 * La chiave del nucleo (`aether_domain::keys::sort_name`) toglie già gran parte
 * dei diacritici, ma non tutti — «ø», «ł» restano quel che sono — e soprattutto
 * non sa **chi legge**: per un italiano «ö» è una «o» con i puntini, per uno
 * svedese è l'ultima lettera dell'alfabeto, e metterla sotto «O» sarebbe
 * sbagliato quanto, per l'italiano, metterla sotto «#».
 *
 * `Intl.Collator` con `sensitivity: "base"` dice esattamente questo: due
 * caratteri sono la stessa lettera **per quella lingua** quando differiscono
 * solo per accenti e maiuscole che la lingua non conta. È lo stesso confronto
 * che `albero.ts` usa per l'ordine naturale dei file, con la lingua invece del
 * sistema: qui la domanda è proprio «come lo ordina chi legge».
 *
 * # Cosa non fa
 *
 * Non riordina gli artisti: l'ordine è una regola del dominio e lo decide il
 * nucleo, uguale su ogni dispositivo. Qui si decide soltanto sotto quale delle
 * ventisei voci dell'indice cade ciascuno, e dove comincia ogni voce.
 */

/** L'alfabeto dell'indice. */
export const LETTERE: readonly string[] = "abcdefghijklmnopqrstuvwxyz".split("");

/** La voce di chi non comincia per una delle ventisei: numeri, altri alfabeti. */
export const ALTRO = "#";

/** Le lettere di ogni nome, quali voci ne hanno almeno uno, e dove comincia ognuna. */
export interface IndiceAlfabetico {
  /** La voce di ciascun nome, nello stesso ordine. */
  lettere: string[];
  /** Le voci con almeno un nome. */
  piene: Set<string>;
  /** La posizione del primo nome di ogni voce: è lì che salta l'indice. */
  ancore: Map<string, number>;
}

/** Il confronto per lettera della lingua data, o di quella di sistema. */
function collatoreDi(lingua: string): Intl.Collator {
  try {
    return new Intl.Collator(lingua, { sensitivity: "base" });
  } catch {
    // Un codice malformato: `Intl` lo rifiuta invece di ripiegare.
    return new Intl.Collator(undefined, { sensitivity: "base" });
  }
}

/** La voce di una chiave d'ordinamento, col confronto dato. */
export function letteraDi(chiave: string, collatore: Intl.Collator): string {
  const punto = chiave.codePointAt(0);
  if (punto === undefined) return ALTRO;
  const primo = String.fromCodePoint(punto);
  return LETTERE.find((l) => collatore.compare(primo, l) === 0) ?? ALTRO;
}

/**
 * L'indice di un elenco di chiavi d'ordinamento, nella lingua di chi legge.
 *
 * Il confronto si fa una volta per carattere iniziale e non per nome: duemila
 * artisti cominciano con qualche decina di caratteri diversi.
 */
export function indiceAlfabetico(chiavi: readonly string[], lingua: string): IndiceAlfabetico {
  const collatore = collatoreDi(lingua);
  const giaVisti = new Map<string, string>();
  const lettere: string[] = [];
  const ancore = new Map<string, number>();
  chiavi.forEach((chiave, i) => {
    const punto = chiave.codePointAt(0);
    const primo = punto === undefined ? "" : String.fromCodePoint(punto);
    let lettera = giaVisti.get(primo);
    if (lettera === undefined) {
      lettera = letteraDi(primo, collatore);
      giaVisti.set(primo, lettera);
    }
    lettere.push(lettera);
    if (!ancore.has(lettera)) ancore.set(lettera, i);
  });
  return { lettere, piene: new Set(lettere), ancore };
}
