/**
 * Il «quando» della cronologia, deciso senza lingue: quale forma usare per un
 * ascolto, per giorni di calendario locali e non per finestre mobili.
 *
 * Le finestre mobili sbagliavano ai bordi dei giorni. Alle 10 di mattina un
 * ascolto di ieri alle 14 (venti ore fa, sotto le ventiquattro) si scriveva
 * «14:00», cioè un orario di oggi ancora da venire; un ascolto di venerdì
 * scorso alle 18, guardato venerdì alle 10 (sei giorni e sedici ore, sotto i
 * sette), si scriveva «ven», cioè il giorno di oggi. Qui si contano i giorni
 * del calendario — anno, mese e giorno locali, così anche il giorno di
 * venticinque ore del cambio d'ora resta un giorno — e il componente ne fa la
 * frase.
 *
 * Puro e senza lingue: `test/quandoCronologia.test.ts` lo prova con l'ora
 * iniettata.
 */

/** La forma del «quando». Il testo lo scrive `Cronologia.tsx`. */
export type FasciaQuando =
  | { tipo: "futuro" }
  | { tipo: "ora" }
  | { tipo: "minuti"; minuti: number }
  | { tipo: "oggi" }
  | { tipo: "ieri" }
  | { tipo: "settimana" }
  | { tipo: "data"; anno: boolean };

/** Il giorno di calendario locale di un istante, come numero di giorni. */
function giorno(ms: number): number {
  const d = new Date(ms);
  return Math.round(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / 86_400_000);
}

/**
 * - Nel futuro (un orologio spostato): la data.
 * - Sotto il minuto: «adesso»; sotto l'ora: «N min fa», che resta vero anche a
 *   cavallo della mezzanotte.
 * - Lo stesso giorno: l'ora, che distingue due ascolti dello stesso pomeriggio.
 * - Il giorno prima: «ieri» con l'ora.
 * - Fino a sei giorni di calendario: il giorno della settimana, che così non è
 *   mai quello di oggi.
 * - Oltre: la data, con l'anno dopo dieci mesi — chi guarda la cronologia di un
 *   anno fa non cerca «alle 14:32», cerca «a marzo».
 */
export function fasciaQuando(ms: number, adesso: number): FasciaQuando {
  const passati = adesso - ms;
  if (passati < 0) return { tipo: "futuro" };
  if (passati < 60_000) return { tipo: "ora" };
  if (passati < 3_600_000) return { tipo: "minuti", minuti: Math.round(passati / 60_000) };
  const giorni = giorno(adesso) - giorno(ms);
  if (giorni <= 0) return { tipo: "oggi" };
  if (giorni === 1) return { tipo: "ieri" };
  if (giorni < 7) return { tipo: "settimana" };
  return { tipo: "data", anno: passati > 300 * 86_400_000 };
}
