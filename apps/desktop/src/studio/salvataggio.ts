/**
 * Le regole del salvataggio dello Studio che non hanno bisogno del DOM.
 *
 * Stanno qui e non in `Guscio.tsx` per potersi provare con `node --test`
 * (`test/salvataggio.test.ts`).
 */

/**
 * Un'istantanea «manuale» di questo testo serve.
 *
 * No, se l'ultima manuale presa in questa sessione è dello stesso testo. Il
 * nucleo ne tiene venti e pota le più vecchie senza guardare la causa: fino
 * alla 3.2 ogni Ctrl+S — e ogni domanda della chat in modalità agent — ne
 * prendeva una anche senza modifiche, e venti pressioni d'abitudine
 * spingevano fuori «salvata», «esportata» e soprattutto «migrata», l'unica
 * copia del testo del formato di prima. Solo per la causa «manuale»: una
 * «salvata» o una «esportata» uguale a una manuale dice qualcosa in più, cioè
 * quando quel testo è stato installato o portato fuori.
 */
export function daFotografare(testo: string, ultimaManuale: string | null): boolean {
  return testo !== ultimaManuale;
}

/**
 * Fa `poi` solo a salvataggio riuscito; un guasto va a `onErrore` e `poi`
 * non parte.
 *
 * È la regola di «Esci» (`Guscio.tsx`, `esci`): ogni strada che smonta lo
 * Studio passa di qui, perché smontarlo spegne il timer del salvataggio
 * automatico e quel che non era ancora sul disco andrebbe perso.
 */
export function poiSeSalvato(
  salva: () => Promise<void>,
  poi: () => void,
  onErrore: (e: unknown) => void,
): Promise<void> {
  return salva().then(poi, onErrore);
}
