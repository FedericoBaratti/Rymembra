/**
 * Last-resort crash safety net for the nodejs-mobile backend.
 *
 * The engine CANNOT be restarted in-process (a second start aborts with
 * `Check failed: !platform_`), so the default behaviour of an uncaught throw —
 * kill the event loop — turns any stray bug into a dead app AND loses every
 * debounced write still in memory (sql.js flush 1.5s fg / 60s bg,
 * settings/queue/secrets JSON debounce).
 *
 * Instead: log it, persist everything that is flushable synchronously, tell
 * the renderer (so it can surface the failure instead of showing infinite
 * spinners), and keep the process alive — degraded is strictly better than
 * dead here. Every step is individually guarded, and a second failure raised
 * while the handler is already running is swallowed so it can never recurse.
 *
 * Kept dependency-injected (no imports from the electron tree) so it is unit
 * testable without booting the whole backend.
 */

export interface FatalHandlerDeps {
  /** Log sink (console.error → logcat on device). */
  log: (message: string, err: unknown) => void
  /** Synchronous best-effort flushes, run in order; each is guarded. */
  flushes: Array<() => void>
  /** Notify the renderer (broadcast 'backend:fatal'); guarded. */
  notify: (kind: string, message: string) => void
}

export function createFatalHandler(deps: FatalHandlerDeps): (kind: string, err: unknown) => void {
  let handling = false
  return (kind, err) => {
    if (handling) return
    handling = true
    try {
      try {
        deps.log(`[fatal] ${kind}`, err)
      } catch {
        /* even the logger may be broken — keep going */
      }
      for (const flush of deps.flushes) {
        try {
          flush()
        } catch {
          /* best effort — the next flush still runs */
        }
      }
      try {
        deps.notify(kind, err instanceof Error ? err.message : String(err))
      } catch {
        /* renderer gone — the log line above is the only trace */
      }
    } finally {
      handling = false
    }
  }
}

/** Wire the handler to the process-level events. */
export function installFatalHandlers(handler: (kind: string, err: unknown) => void): void {
  process.on('uncaughtException', (err) => handler('uncaughtException', err))
  process.on('unhandledRejection', (reason) => handler('unhandledRejection', reason))
}
