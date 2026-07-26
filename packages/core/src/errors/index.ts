/**
 * Errori dell'engine. Punto d'ingresso unico.
 *
 * Le tre regole che tengono in piedi il resto:
 *
 * 1. Ai confini si usa `AppError.from(x)`. Accetta qualunque cosa e non lancia
 *    mai, quindi non esiste un percorso in cui un errore arrivi come stringa.
 * 2. Dentro il core le funzioni fallibili restituiscono `Result<T, AppError>`.
 *    Il `throw` resta per le invarianti violate, che sono bug da correggere.
 * 3. Il testo per l'utente non si scrive nel codice: si passa da `i18nKey` più
 *    `params`. Il codice non conosce la lingua.
 */

export {
  CATALOG,
  ERROR_CODES,
  legacyCodeToErrorCode,
  type ErrorCode,
  type ErrorDomain,
  type ErrorMeta,
  type ErrorParams,
  type ErrorSeverity,
  type NoParams
} from './catalog'

export {
  AppError,
  i18nKeyFor,
  isAppError,
  isAppErrorPayload,
  type AppErrorOptions,
  type AppErrorPayload,
  type CauseInfo
} from './appError'

export { requiredI18nKeys, missingI18nKeys } from './i18nKeys'
