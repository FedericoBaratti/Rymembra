/**
 * Limitatore FIFO: garantisce un intervallo minimo fra gli AVVII di due chiamate
 * consecutive e limita la concorrenza.
 *
 * Porta `legacy/Aeter/electron/modules/net/rateLimiter.ts` mantenendone le due
 * scelte non ovvie:
 *
 *   - il vincolo è sull'avvio, non sulla fine: un servizio che chiede "una
 *     richiesta al secondo" intende una al secondo, non una al secondo più la
 *     durata della precedente;
 *   - la coda è FIFO stretta, quindi l'ordine di arrivo è l'ordine di partenza.
 *     Conta per l'arricchimento metadati, dove il primo album chiesto è quello
 *     che l'utente sta guardando.
 *
 * Cambia la firma: `schedule` parla Result come il resto del core, e un `throw`
 * sincrono dentro la funzione passata non può più lasciare un permesso occupato.
 */

import type { AppError } from '../errors'
import type { Result } from '../result'
import { runFallible, type Fallible } from './run'

interface QueueEntry {
  run: () => void
}

export interface RateLimiterOptions {
  name: string
  minIntervalMs: number
  maxConcurrent?: number
}

export class RateLimiter {
  readonly name: string
  private readonly minIntervalMs: number
  private readonly maxConcurrent: number
  private readonly queue: QueueEntry[] = []
  private active = 0
  private nextSlotAt = 0
  private timer: ReturnType<typeof setTimeout> | null = null

  constructor(options: RateLimiterOptions) {
    this.name = options.name
    this.minIntervalMs = options.minIntervalMs
    this.maxConcurrent = options.maxConcurrent ?? 1
  }

  /** In coda più in esecuzione: è la misura utile per il pannello diagnostico. */
  get pending(): number {
    return this.queue.length + this.active
  }

  get inFlight(): number {
    return this.active
  }

  /**
   * Sposta in avanti il prossimo permesso. Da chiamare quando si osserva un 429:
   * il servizio ha detto di rallentare, e la pausa vale per tutta la coda, non
   * solo per la richiesta che l'ha scoperto.
   */
  notifyRateLimited(retryAfterMs?: number): void {
    const pause = Math.max(retryAfterMs ?? 0, this.minIntervalMs * 4)
    this.nextSlotAt = Math.max(this.nextSlotAt, Date.now() + pause)
    this.pump()
  }

  schedule<T>(fn: Fallible<T>): Promise<Result<T, AppError>> {
    return new Promise<Result<T, AppError>>((resolve) => {
      this.queue.push({
        run: () => {
          this.active++
          // runFallible non lancia: il permesso si libera sempre, anche se `fn`
          // esplode prima del primo await.
          void runFallible(fn).then((result) => {
            this.active--
            this.pump()
            resolve(result)
          })
        }
      })
      this.pump()
    })
  }

  /** Ferma il timer pendente. Serve alla chiusura e ai test. */
  dispose(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer)
      this.timer = null
    }
  }

  private pump(): void {
    if (this.queue.length === 0 || this.active >= this.maxConcurrent) return

    const now = Date.now()
    if (now < this.nextSlotAt) {
      if (this.timer === null) {
        this.timer = setTimeout(() => {
          this.timer = null
          this.pump()
        }, this.nextSlotAt - now)
        ;(this.timer as { unref?: () => void }).unref?.()
      }
      return
    }

    const entry = this.queue.shift()
    if (entry === undefined) return
    this.nextSlotAt = now + this.minIntervalMs
    entry.run()
    // Con maxConcurrent > 1 possono esserci altri permessi disponibili subito.
    this.pump()
  }
}
