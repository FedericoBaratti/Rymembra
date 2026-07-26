/**
 * Le chiavi i18n si DERIVANO dal catalogo degli errori.
 *
 * Era il punto in cui il legacy aveva già divergito: 37 chiavi `errors.*` nei
 * file di traduzione desktop contro ~24 mappate in `ipcError.ts`, con 8 chiavi
 * orfane (backend_unreachable, dl_forbidden, dl_network, dl_yt_error,
 * ext_search_failed, ytdlp_corrupted, ytdlp_busy, enrich_needs_review) rimaste
 * indietro perché l'i18n era stato allineato dal mobile e la tabella dei codici no.
 *
 * Qui l'elenco delle chiavi necessarie è una funzione del catalogo, e un test
 * confronta i bundle di traduzione con quell'elenco: aggiungere un codice senza
 * la sua traduzione fa fallire la suite invece di produrre in silenzio un
 * messaggio mancante in UI.
 */

import { ERROR_CODES, type ErrorCode } from './catalog'
import { i18nKeyFor } from './appError'

/** Tutte le chiavi che ogni bundle di traduzione deve definire. */
export function requiredI18nKeys(): string[] {
  return ERROR_CODES.map(i18nKeyFor).sort()
}

/**
 * Naviga un bundle i18n annidato risolvendo una chiave a punti.
 *
 * `errors.net.http` può essere sia una chiave piatta sia annidata
 * (`{ errors: { net: { http: "…" } } }`): i18next accetta entrambe, quindi
 * entrambe vanno considerate presenti.
 */
function resolveKey(bundle: unknown, key: string): unknown {
  if (typeof bundle !== 'object' || bundle === null) return undefined

  const flat = (bundle as Record<string, unknown>)[key]
  if (flat !== undefined) return flat

  let node: unknown = bundle
  for (const part of key.split('.')) {
    if (typeof node !== 'object' || node === null) return undefined
    node = (node as Record<string, unknown>)[part]
    if (node === undefined) return undefined
  }
  return node
}

/**
 * I codici le cui traduzioni mancano nel bundle, in ordine.
 * Vuoto = copertura completa.
 */
export function missingI18nKeys(bundle: unknown): ErrorCode[] {
  return ERROR_CODES.filter((code) => {
    const value = resolveKey(bundle, i18nKeyFor(code))
    return typeof value !== 'string' || value.length === 0
  }).sort()
}
