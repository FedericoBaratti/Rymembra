/**
 * Quale forma di una chiave si legge, dato un numero.
 *
 * # Perché le forme sono più di due
 *
 * Fino alla 3.0 il motore delle lingue conosceva due forme: `chiave.uno` quando
 * `n === 1`, e la chiave nuda per tutto il resto. Per l'italiano e l'inglese
 * basta, ed è il motivo per cui ha retto così a lungo. Ma è una regola
 * dell'italiano scritta come se fosse di tutti: in russo «21 брано» vuole la
 * stessa forma di «1», «3» ne vuole un'altra e «5» una terza; in arabo le forme
 * sono sei; in giapponese una sola. Una lingua così, oggi, non si potrebbe
 * aggiungere con un file — ed è la promessa di `lingue/index.ts`.
 *
 * Le regole vere le ha già il motore: `Intl.PluralRules` dice, per una lingua e
 * un numero, a quale **categoria** il numero appartiene — `zero`, `one`, `two`,
 * `few`, `many`, `other` — con i dati del CLDR, che sono quelli che usano tutti.
 * Qui si usano quei nomi come suffissi: `brani.one`, `brani.few`, `brani.many`.
 *
 * # L'ordine in cui si cerca
 *
 * 1. `chiave.uno`, quando `n` è esattamente 1. È la forma che le chiavi di oggi
 *    hanno già — novanta e passa, in italiano e in inglese — e resta valida col
 *    suo significato di sempre: **proprio uno**, come `=1` nei formati ICU. Vince
 *    sulla categoria perché è più precisa: in russo `one` vale anche per 21, e
 *    «Una cartella non risponde» non si può dire di ventuno cartelle.
 * 2. `chiave.<categoria>`, la forma che la lingua chiede per quel numero.
 * 3. `chiave.other`, la forma generica che una lingua può scrivere a parte.
 * 4. La chiave nuda, che c'è sempre: è il tipo di [`t`] a pretenderla, ed è il
 *    ripiego che fa funzionare le chiavi scritte prima di questo file.
 *
 * Il ripiego non salta di lingua a metà: se il tedesco ha la chiave in una
 * forma qualunque, la frase resta tedesca. Si passa all'inglese solo per una
 * chiave che il dizionario tedesco non ha **in nessuna forma** — e lì si
 * applicano le regole dell'inglese, perché la frase è inglese. Lo fa chi
 * chiama, dizionario per dizionario: questo modulo ne guarda uno solo.
 *
 * # Perché un modulo a parte
 *
 * Perché è puro, e quindi si prova: `lingue/index.ts` nasce da `import.meta.glob`
 * e da un `import` di JSON, che fuori da Vite non si caricano. Qui non c'è
 * niente da caricare.
 */

/** Il suffisso della forma «proprio uno»: vedi l'ordine nella nota del modulo. */
export const SUFFISSO_UNO = "uno";

/**
 * Le categorie del CLDR, nell'ordine in cui le elenca.
 *
 * Servono anche a `strumenti/lingue.js`, che deve riconoscere una variante
 * plurale da una chiave che per caso finisce con una di queste parole.
 */
export const CATEGORIE = ["zero", "one", "two", "few", "many", "other"] as const;

/** Una categoria plurale del CLDR. */
export type Categoria = (typeof CATEGORIE)[number];

/**
 * Le regole già costruite, per lingua.
 *
 * Costruirle costa: dentro c'è il caricamento dei dati del CLDR per quella
 * lingua. Un elenco di diciottomila righe chiede il plurale a ogni riga, e la
 * lingua attiva è una sola.
 */
const REGOLE = new Map<string, Intl.PluralRules>();

/**
 * La categoria di un numero, nella lingua data.
 *
 * Una lingua che `Intl` non riconosce ripiega da sé su quella di sistema, che
 * è il meglio che si possa fare con un codice sconosciuto: non si lancia, e una
 * frase si trova comunque — al peggio la chiave nuda.
 */
export function categoria(lingua: string, n: number): Categoria {
  let regole = REGOLE.get(lingua);
  if (regole === undefined) {
    try {
      regole = new Intl.PluralRules(lingua);
    } catch {
      // Un codice malformato: `Intl` lo rifiuta invece di ripiegare.
      regole = new Intl.PluralRules();
    }
    REGOLE.set(lingua, regole);
  }
  return regole.select(n);
}

/**
 * Il testo di una chiave per il numero `n`, cercato in **un** dizionario.
 *
 * `cerca` legge una chiave dal dizionario e dice `null` quando non c'è.
 * Restituisce `null` se il dizionario non ha la chiave in nessuna forma: chi
 * chiama passa al dizionario di ripiego. Vedi la nota del modulo per l'ordine.
 */
export function formaPlurale(
  cerca: (chiave: string) => string | null,
  lingua: string,
  chiave: string,
  n: number,
): string | null {
  if (n === 1) {
    const uno = cerca(`${chiave}.${SUFFISSO_UNO}`);
    if (uno !== null) return uno;
  }
  // Un numero che non è un numero non ha categoria: `select(NaN)` dice `other`,
  // ed è giusto così — ma lo si dice qui, invece di affidarsi al motore.
  const quale = Number.isFinite(n) ? categoria(lingua, n) : "other";
  if (quale !== "other") {
    const forma = cerca(`${chiave}.${quale}`);
    if (forma !== null) return forma;
  }
  return cerca(`${chiave}.other`) ?? cerca(chiave);
}
