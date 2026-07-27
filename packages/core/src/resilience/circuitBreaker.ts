/**
 * Interruttore: smette di chiamare un servizio che sta chiaramente giù.
 *
 * Porta `legacy/Aeter/electron/modules/net/circuitBreaker.ts` con la stessa
 * macchina a stati (chiuso → aperto → semiaperto con una sola sonda) e la stessa
 * cura nel caso difficile, quello che il legacy aveva già risolto bene: se la
 * sonda in stato semiaperto fallisce per un motivo che NON conta come guasto del
 * servizio (un 404, uno schema sbagliato), il cooldown si riarma. Altrimenti
 * l'interruttore resterebbe semiaperto e ogni chiamata successiva passerebbe come
 * una nuova sonda, cioè l'interruttore non interromperebbe più niente.
 *
 * Cambia una cosa sola, e in meglio: `countsAsFailure`.
 *
 * Nel legacy era `err instanceof NetworkError || err instanceof RateLimitError ||
 * (err instanceof HttpError && err.status >= 500)`. Ora è `error.retryable`, che
 * il catalogo calcola una volta — e per i codici `net.*` dà esattamente le stesse
 * risposte: offline sì, timeout sì, 429 sì, 5xx sì, 404 no, schema no. La
 * differenza è che ora funziona anche per un dominio che non sia `net`, quindi
 * l'interruttore si può mettere davanti a un plugin nativo o a un telefono
 * accoppiato che non risponde.
 */

import { AppError } from '../errors'
import { err, ok, type Result } from '../result'
import { runFallible, type Fallible } from './run'

export type CircuitState = 'closed' | 'open' | 'half-open'

export interface CircuitBreakerOptions {
  /** Nome del servizio: finisce nell'errore mostrato e nei log. */
  name: string
  failureThreshold?: number
  cooldownMs?: number
  /**
   * Quali guasti indicano che il SERVIZIO è malato, invece che la singola
   * richiesta sbagliata. Default: quelli che il catalogo dice ritentabili.
   */
  countsAsFailure?: (error: AppError) => boolean
  onStateChange?: (state: CircuitState, name: string) => void
  /** Iniettabile per i test. */
  now?: () => number
}

/**
 * Un interruttore già aperto a monte non conta come guasto del servizio a valle:
 * altrimenti due interruttori in serie si aprirebbero a vicenda a catena.
 */
function defaultCountsAsFailure(error: AppError): boolean {
  return error.retryable && error.code !== 'net.circuitOpen'
}

export class CircuitBreaker {
  readonly name: string
  private readonly failureThreshold: number
  private readonly cooldownMs: number
  private readonly countsAsFailure: (error: AppError) => boolean
  private readonly onStateChange: ((state: CircuitState, name: string) => void) | undefined
  private readonly now: () => number

  private failures = 0
  private openedAt = 0
  private halfOpenProbe = false
  private lastNotified: CircuitState = 'closed'

  constructor(options: CircuitBreakerOptions) {
    this.name = options.name
    this.failureThreshold = options.failureThreshold ?? 5
    this.cooldownMs = options.cooldownMs ?? 60_000
    this.countsAsFailure = options.countsAsFailure ?? defaultCountsAsFailure
    this.onStateChange = options.onStateChange
    this.now = options.now ?? Date.now
  }

  get state(): CircuitState {
    if (this.failures < this.failureThreshold) return 'closed'
    if (this.now() - this.openedAt >= this.cooldownMs) return 'half-open'
    return 'open'
  }

  get failureCount(): number {
    return this.failures
  }

  /** Millisecondi prima della prossima sonda, 0 se non è aperto. */
  get retryAfterMs(): number {
    if (this.state !== 'open') return 0
    return Math.max(0, this.cooldownMs - (this.now() - this.openedAt))
  }

  /** Riporta l'interruttore a chiuso. Serve dopo una riparazione manuale. */
  reset(): void {
    this.failures = 0
    this.openedAt = 0
    this.halfOpenProbe = false
    this.notify()
  }

  /**
   * Esegue `fn` se l'interruttore lo consente, altrimenti fallisce subito con
   * `net.circuitOpen` — che porta `retryAfterMs`, così chi ritenta sa aspettare
   * la fine del cooldown invece di bussare a vuoto.
   */
  async exec<T>(fn: Fallible<T>): Promise<Result<T, AppError>> {
    const state = this.state
    if (state === 'open') return err(this.openError())

    const wasHalfOpen = state === 'half-open'
    if (wasHalfOpen) {
      // Passa una sonda sola; tutti gli altri cadono subito finché non si sa.
      if (this.halfOpenProbe) return err(this.openError())
      this.halfOpenProbe = true
    }

    try {
      const result = await runFallible(fn)

      if (result.ok) {
        this.failures = 0
        return ok(result.value)
      }

      if (this.countsAsFailure(result.error)) {
        this.failures++
        if (this.failures >= this.failureThreshold) this.openedAt = this.now()
      } else if (wasHalfOpen) {
        // La sonda è caduta per un motivo che non conta (4xx, schema). Riarmare
        // il cooldown: lasciare l'interruttore semiaperto significherebbe far
        // passare ogni chiamata successiva come una nuova sonda.
        this.openedAt = this.now()
      }
      return err(result.error)
    } finally {
      this.halfOpenProbe = false
      this.notify()
    }
  }

  private openError(): AppError {
    const retryAfterMs = this.retryAfterMs
    return AppError.of('net.circuitOpen', {
      service: this.name,
      ...(retryAfterMs > 0 ? { retryAfterMs } : {})
    })
  }

  private notify(): void {
    const state = this.state
    if (state === this.lastNotified) return
    this.lastNotified = state
    this.onStateChange?.(state, this.name)
  }
}
