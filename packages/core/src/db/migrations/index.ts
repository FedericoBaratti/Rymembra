/**
 * La catena di Aether.
 *
 * Tre parti, e la differenza fra loro è il punto di tutto il modulo:
 *
 *   BASELINE          dove nasce un file nuovo
 *   DESKTOP/ANDROID   dove si trova un file esistente, e come raggiunge la baseline
 *   UNIFIED           tutto ciò che viene dopo — da qui in avanti la storia è una
 *
 * I passi nuovi si aggiungono SOLO a `UNIFIED`, con numeri oltre
 * `BASELINE_VERSION` e crescenti, e ogni volta va aggiornata anche `BASELINE` con
 * lo schema equivalente. Il test di parità pretende che le tre strade convergano,
 * quindi dimenticare uno dei due lati non compila oltre la suite.
 */

import type { Chain, Migration } from '../migrate'
import { BASELINE } from './baseline'
import { ANDROID_HISTORY, DESKTOP_HISTORY } from './legacy'

/**
 * La catena unificata. Vuota per ora: lo schema è quello a cui arrivavano i due
 * alberi, e i passi della Fase 3 (skin installate, libreria skin) si aggiungono
 * qui a partire da v101.
 */
export const UNIFIED: readonly Migration[] = []

export const AETHER_CHAIN: Chain = {
  baseline: BASELINE,
  legacy: {
    desktop: DESKTOP_HISTORY,
    android: ANDROID_HISTORY
  },
  unified: UNIFIED
}

export { BASELINE } from './baseline'
export { ANDROID_HISTORY, DESKTOP_HISTORY } from './legacy'
