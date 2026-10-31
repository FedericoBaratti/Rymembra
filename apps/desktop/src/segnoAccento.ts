/**
 * Il segno per cui l'accento della copertina si ricalcola, e quando non deve.
 *
 * L'accento lo calcola il nucleo sulle capacità e sulle superfici della skin
 * **scelta** (`accento_copertina` in `skin.rs`), e lo scrive l'effetto di
 * `aspettoFinestra.ts`. Durante «Prova sull'app» la finestra indossa una
 * bozza dello Studio che non è la skin scelta: il colore calcolato lì era
 * quello di un'altra skin — col contrasto di un'altra skin — e una bozza che
 * dichiara `dynamicAccent: false` lo riceveva lo stesso. È la regola che
 * `skinAttiva.ts` già segue per l'anteprima: un'anteprima senza accento è una
 * risposta onesta, una con l'accento sbagliato no.
 *
 * Un modulo a parte perché `skinAttiva.ts` porta con sé le lingue
 * (`import.meta.glob`, che solo Vite risolve) e `node --test` non lo può
 * importare: la regola si prova in `test/segnoAccento.test.ts`.
 */

/**
 * Il segno da passare all'accento: `giro`, o `null` mentre la finestra
 * indossa la bozza provata.
 *
 * Si guarda la skin **indossata**, per identità, e non se la prova è aperta:
 * «Tienila» chiude la prova prima che la scelta risponda, e in quel giro la
 * finestra porta ancora la bozza mentre il nucleo ha ancora la skin scelta di
 * prima — un accento chiesto lì si calcolerebbe su quella e finirebbe sopra
 * la nuova. Il segno torna quando a schermo arriva un'altra skin: quella di
 * prima con «Torna», quella appena scelta con «Tienila», o una scelta dalle
 * Impostazioni a prova ancora aperta. Passando da `null` a un numero cambia, e
 * l'effetto dell'accento riparte da sé.
 */
export function segnoAccento<T>(indossata: T | null, bozza: T | null, giro: number): number | null {
  return bozza !== null && indossata === bozza ? null : giro;
}
