interface QueueEntry {
  run: () => void
}

/**
 * FIFO rate limiter: guarantees a minimum interval between the *starts* of
 * consecutive calls and bounds in-flight concurrency.
 */
export class RateLimiter {
  readonly name: string
  private readonly minIntervalMs: number
  private readonly maxConcurrent: number
  private queue: QueueEntry[] = []
  private active = 0
  private nextSlotAt = 0
  private timer: ReturnType<typeof setTimeout> | null = null

  constructor(opts: { name: string; minIntervalMs: number; maxConcurrent?: number }) {
    this.name = opts.name
    this.minIntervalMs = opts.minIntervalMs
    this.maxConcurrent = opts.maxConcurrent ?? 1
  }

  get pending(): number {
    return this.queue.length + this.active
  }

  /** Push the next available slot out (call when a 429 is observed). */
  notifyRateLimited(retryAfterMs?: number): void {
    const pause = Math.max(retryAfterMs ?? 0, this.minIntervalMs * 4)
    this.nextSlotAt = Math.max(this.nextSlotAt, Date.now() + pause)
    this.pump()
  }

  schedule<T>(fn: () => Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      this.queue.push({
        run: () => {
          this.active++
          // Wrap in Promise.resolve().then so a *synchronous* throw from fn()
          // still rejects (not escapes), guaranteeing the slot is released.
          Promise.resolve()
            .then(() => fn())
            .then(resolve, reject)
            .finally(() => {
              this.active--
              this.pump()
            })
        }
      })
      this.pump()
    })
  }

  private pump(): void {
    if (this.queue.length === 0 || this.active >= this.maxConcurrent) return
    const now = Date.now()
    if (now < this.nextSlotAt) {
      if (!this.timer) {
        this.timer = setTimeout(() => {
          this.timer = null
          this.pump()
        }, this.nextSlotAt - now)
        this.timer.unref?.()
      }
      return
    }
    const entry = this.queue.shift()
    if (!entry) return
    this.nextSlotAt = now + this.minIntervalMs
    entry.run()
    // More slots may be available (maxConcurrent > 1)
    this.pump()
  }
}
