/**
 * Quando dire a chi ascolta lo schermo quale brano è partito.
 *
 * # Il difetto
 *
 * «Brano successivo» e «Brano precedente» cambiavano il titolo lontano dal
 * fuoco, e nessuna regione viva lo riportava: chi usa un lettore di schermo
 * doveva andare a cercare il titolo a mano dopo ogni pressione, e il passaggio
 * da solo al brano dopo non lo sentiva affatto.
 *
 * # La regola (D-71)
 *
 * Si dice **ogni** cambio di brano, anche quello che arriva da sé alla fine di
 * un brano, in una regione `polite` (`AnnuncioDelBrano` in
 * `parti/Annuncio.tsx`): aspetta una pausa di chi sta leggendo, non lo
 * interrompe. Si dice il passaggio, non lo stato: lo stesso brano che va in
 * pausa o riparte non è una notizia.
 *
 * Tranne uno: il brano che la finestra ritrova all'avvio, rimesso dov'era e in
 * pausa. Non è partito niente — l'avvio non suona da solo — e dirlo a ogni
 * apertura sarebbe rumore. Se invece il primo brano che si vede sta già
 * suonando, l'ha fatto partire qualcuno, e si dice.
 *
 * Puro: `test/annuncioBrano.test.ts` lo prova con `node --test`.
 */

/** Del brano, quel che serve per dirlo. */
export interface BranoDaDire {
  id: number;
  titolo: string;
  artista: string;
  inPausa: boolean;
}

/**
 * Il brano da dire, o `null`.
 *
 * `prima` è il brano dell'ultima volta: `undefined` finché non se n'è visto
 * nessuno, `null` dopo che la coda è finita.
 */
export function branoDaDire(
  prima: BranoDaDire | null | undefined,
  adesso: BranoDaDire | null,
): BranoDaDire | null {
  if (adesso === null) return null;
  // Il primo brano che si vede: quello ritrovato all'avvio, in pausa, non si
  // dice; uno che suona già sì.
  if (prima === undefined) return adesso.inPausa ? null : adesso;
  if (prima !== null && prima.id === adesso.id) return null;
  return adesso;
}

/** Quel che si ricorda dopo aver guardato `adesso`: vedi `branoDaDire`. */
export function branoVisto(
  prima: BranoDaDire | null | undefined,
  adesso: BranoDaDire | null,
): BranoDaDire | null | undefined {
  // Un `null` prima di ogni brano è lo stato vuoto della finestra che
  // aspetta il nucleo, non una coda finita: si resta a «non visto».
  return adesso === null && prima === undefined ? undefined : adesso;
}
