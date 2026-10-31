/**
 * Il peso di un profilo esportato, in megabyte: il numero da mostrare.
 *
 * Megabyte fissi e non l'unità più adatta, per la ragione scritta accanto a
 * chi lo mostra (`Profilo.tsx`): due esportazioni si confrontano a colpo
 * d'occhio solo nella stessa unità. Ma «0 MB» non è un confronto, è un numero
 * falso, e un profilo appena installato — solo preferenze, niente copertine né
 * cronologia — pesa qualche decina di kB: sotto il mega si dice «meno di 1 MB»
 * (`null`), e sotto i dieci si tiene un decimale, che fra 1,4 e 1,6 è la
 * differenza che si vuole vedere.
 *
 * Puro e senza lingue: il numero lo formatta `t()`, con i separatori della
 * lingua attiva. `test/profiloScheda.test.ts` lo prova.
 */
export function pesoInMegabyte(byte: number): number | null {
  const mb = byte / (1024 * 1024);
  if (mb < 1) return null;
  return mb < 10 ? Math.round(mb * 10) / 10 : Math.round(mb);
}
