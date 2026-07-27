/**
 * Resilienza: ritentare, arrendersi, rallentare, scadere.
 *
 * Perché sta qui e non sotto `net/`. Nel legacy questi componenti esistevano già
 * ed erano fatti bene, ma vivevano dentro `electron/modules/net/` e parlavano solo
 * le classi di errore di quel sottoalbero. Il risultato misurabile: **nessun**
 * altro modulo li usava. `library`, `metadata`, `db`, `settings`, `sync`, `lan` e
 * `phoneSync` facevano log-and-continue con `logWarn` — 58 catch silenziosi e 50
 * `.catch(() => {})` — dove un retry con backoff era la risposta giusta.
 *
 * Ora il linguaggio comune è `Result<T, AppError>` e la ritentabilità è un dato
 * sull'errore, letto dal catalogo. Un componente in più non deve conoscere il
 * dominio dell'errore per decidere se ritentarlo.
 *
 * Si compongono in questo ordine, da fuori a dentro:
 *
 *     limiter.schedule(() => breaker.exec(() => withRetry((n) => fetchPage(n))))
 *
 * Il limitatore protegge il servizio remoto, l'interruttore protegge noi dal
 * servizio, il retry protegge la singola operazione. Invertire limitatore e retry
 * significherebbe che i tentativi ripetuti non contano nel budget di traffico.
 */

export { runFallible, sleep, abortError, abortedIfSignalled, type Fallible } from './run'
export { withRetry, retryThrowing, nextDelayMs, type RetryOptions } from './retry'
export {
  CircuitBreaker,
  type CircuitBreakerOptions,
  type CircuitState
} from './circuitBreaker'
export { RateLimiter, type RateLimiterOptions } from './rateLimiter'
export {
  withDeadline,
  createDeadline,
  type Deadline,
  type DeadlineOptions
} from './timeout'
export {
  decideRetry,
  normalizeDownloadError,
  MAX_ATTEMPTS,
  RATE_LIMIT_PAUSE_MS,
  RETRY_BASE_MS,
  type DownloadFailureClass,
  type RetryDecision
} from './downloadPolicy'
