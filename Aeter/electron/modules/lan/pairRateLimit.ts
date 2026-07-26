// Brute-force guard on /api/pair: pairing codes are short-lived but guessable,
// so cap attempts per source IP. Sliding window, in-memory (resets with the
// server — fine, the pairing token TTL is shorter than any realistic restart).
// Own module so server.ts stays routing-only and the window logic is
// unit-testable with an injected clock.

export const PAIR_WINDOW_MS = 60_000
export const PAIR_MAX_ATTEMPTS = 5

const pairAttempts = new Map<string, number[]>()

/**
 * Counts an attempt from `ip` and reports whether it exceeded the budget of
 * PAIR_MAX_ATTEMPTS per sliding PAIR_WINDOW_MS. A limited call does NOT burn
 * an extra slot, so the caller recovers as soon as the window slides.
 */
export function pairRateLimited(ip: string, now = Date.now()): boolean {
  const recent = (pairAttempts.get(ip) ?? []).filter((t) => now - t < PAIR_WINDOW_MS)
  if (recent.length >= PAIR_MAX_ATTEMPTS) {
    pairAttempts.set(ip, recent)
    return true
  }
  recent.push(now)
  pairAttempts.set(ip, recent)
  // Opportunistic cleanup so long-lived servers don't accumulate dead IPs.
  if (pairAttempts.size > 100) {
    for (const [k, v] of pairAttempts) {
      if (v.every((t) => now - t >= PAIR_WINDOW_MS)) pairAttempts.delete(k)
    }
  }
  return false
}

/** Test hook: clears the per-IP window state. */
export function resetPairRateLimit(): void {
  pairAttempts.clear()
}
