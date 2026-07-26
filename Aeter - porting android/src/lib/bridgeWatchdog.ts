/**
 * Liveness watchdog for the nodejs-mobile bridge (src/lib/bridge.ts).
 *
 * Problem: invoke() promises settle only when the backend replies. If the node
 * engine dies (background kill, native crash) every pending promise hangs
 * forever → infinite spinners across the app.
 *
 * Per-method timeouts are the wrong tool here: several calls are legitimately
 * slow (yt-dlp preview 60s+, rescan/repairs minutes) and a timeout map rots.
 * What we actually need to detect is "the backend stopped talking". So:
 *
 *  - While requests are pending, a low-frequency check runs.
 *  - If the oldest pending request AND the last inbound message are both older
 *    than `stallMs`, fire ONE cheap probe request (any reply proves liveness —
 *    slow-but-alive calls are never killed).
 *  - If nothing (probe reply or any other message) arrives within
 *    `probeTimeoutMs`, declare the backend dead via `onDead()` (the bridge
 *    rejects all pending requests there).
 *
 * Freeze-safe on Android: while the WebView is frozen no timers run; on resume
 * the stale timestamps trigger a PROBE (not a rejection), so a live backend
 * answers within ms and nothing is dropped.
 */

export interface BridgeWatchdogOptions {
  /** Silence + pending age required before probing. */
  stallMs?: number
  /** How long the probe may go unanswered before onDead(). */
  probeTimeoutMs?: number
  /** Internal check cadence while requests are pending. */
  checkEveryMs?: number
  /** Fire a lightweight request whose reply arrives as a normal message. */
  probe: () => void
  /** The backend is confirmed unreachable. */
  onDead: () => void
}

export interface BridgeWatchdog {
  /** A request was sent (arms the checker if idle). */
  noteSent(): void
  /** Any inbound message arrived (reply, event, …) — proves liveness. */
  noteMessage(): void
  /** No requests are pending anymore — disarm the checker. */
  noteIdle(): void
  /** Tear down the internal timer (tests). */
  stop(): void
}

export function createBridgeWatchdog(opts: BridgeWatchdogOptions): BridgeWatchdog {
  // Lenient defaults: first-boot on device (asset extraction + sql.js load +
  // migrations) can be silent for a while — a false "dead" there would reject
  // the very first getSettings/refreshAll. 12s + 10s ≈ 22s of TOTAL silence
  // with traffic pending before declaring death.
  const stallMs = opts.stallMs ?? 12_000
  const probeTimeoutMs = opts.probeTimeoutMs ?? 10_000
  const checkEveryMs = opts.checkEveryMs ?? 2_500

  let timer: ReturnType<typeof setInterval> | null = null
  let pendingSince: number | null = null
  let lastMessageAt = Date.now()
  let probeSentAt: number | null = null

  const disarm = (): void => {
    pendingSince = null
    probeSentAt = null
    if (timer != null) {
      clearInterval(timer)
      timer = null
    }
  }

  const check = (): void => {
    if (pendingSince == null) return
    const now = Date.now()
    if (probeSentAt != null) {
      if (now - probeSentAt >= probeTimeoutMs) {
        // Self-disarm BEFORE the callback: onDead clears the pending map, and
        // the checker must not fire another probe off the stale timestamps.
        disarm()
        opts.onDead()
      }
      return
    }
    if (now - pendingSince >= stallMs && now - lastMessageAt >= stallMs) {
      probeSentAt = now
      opts.probe()
    }
  }

  return {
    noteSent() {
      if (pendingSince == null) pendingSince = Date.now()
      if (timer == null) timer = setInterval(check, checkEveryMs)
    },
    noteMessage() {
      lastMessageAt = Date.now()
      probeSentAt = null
    },
    noteIdle: disarm,
    stop: disarm
  }
}
