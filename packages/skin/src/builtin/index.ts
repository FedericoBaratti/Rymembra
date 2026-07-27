/**
 * Le skin di serie, come sorgenti non ancora validate.
 *
 * Deliberatamente `unknown`: passano dallo STESSO `parseSkin` di un pacchetto
 * importato dall'esterno. Se una built-in è malformata lo si scopre in sviluppo,
 * con lo stesso messaggio che vedrebbe l'utente — invece di avere due percorsi, uno
 * fidato e uno controllato, di cui solo il secondo è provato.
 *
 * Nel legacy le skin erano CSS caricato dal bundle: un errore non veniva scoperto
 * affatto, dava una superficie del colore sbagliato.
 */

import { NOTHING_SKIN_SOURCE } from './nothing'
import { PLAIN_SKIN_SOURCE } from './plain'

export { NOTHING_SKIN_SOURCE } from './nothing'
export { PLAIN_SKIN_SOURCE } from './plain'

/** Nell'ordine in cui compaiono nel selettore delle impostazioni. */
export const BUILTIN_SKIN_SOURCES: readonly unknown[] = [PLAIN_SKIN_SOURCE, NOTHING_SKIN_SOURCE]

export const BUILTIN_SKIN_IDS = ['plain', 'nothing'] as const
