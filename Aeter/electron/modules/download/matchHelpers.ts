// Pure matching/query helpers for resolving a bare (artist, title) into the best
// YouTube candidate. No electron imports → unit-tested in isolation. Kept ICU-free
// / Node-12-safe (no \p{} regex, no replaceAll) so the same source can be bundled
// into the nodejs-mobile backend on the porting build.

export interface Candidate {
  url: string
  durationSec: number | null
  title: string
}

/** The primary YouTube search query for a track. */
export function buildSearchQuery(artist: string, title: string): string {
  return `${artist ?? ''} ${title ?? ''}`.replace(/\s+/g, ' ').trim()
}

/**
 * A looser query for a second attempt when the strict one returns nothing.
 * Titles are often decorated ("(feat. X)", "- Remastered 2011", "(Live)") and
 * the artist field may bundle features ("A, B & C"); these frequently make the
 * strict search miss a track that is otherwise available. Keep only the primary
 * artist and strip the decorations. ICU-free / Node 12-safe.
 */
export function buildFallbackQuery(artist: string, title: string): string {
  const primaryArtist = (artist ?? '').split(/,|&|\bfeat\.?\b|\bft\.?\b/i)[0].trim()
  let t = title ?? ''
  // Drop bracketed/parenthesised suffixes: (feat. X), [Live], {Bonus}, …
  t = t.replace(/[([{][^)\]}]*[)\]}]/g, ' ')
  // Drop " - <…> Version/Remaster/Mix/Edit/Live/…" tails.
  t = t.replace(
    /\s[-–]\s.*\b(version|remaster(?:ed)?|mix|edit|mono|stereo|live|radio|single|remix|acoustic|demo|deluxe|anniversary)\b.*$/i,
    ' '
  )
  t = t.replace(/\s+/g, ' ').trim()
  return `${primaryArtist} ${t}`.replace(/\s+/g, ' ').trim()
}

/** Pick the candidate whose duration is closest to the target (±30s), else the first. */
export function pickBestCandidate(candidates: Candidate[], targetMs: number | null): Candidate | null {
  if (candidates.length === 0) return null
  if (!targetMs) return candidates[0]
  const targetSec = targetMs / 1000
  let best: Candidate | null = null
  let bestDiff = Infinity
  for (const c of candidates) {
    if (c.durationSec === null) continue
    const diff = Math.abs(c.durationSec - targetSec)
    if (diff < bestDiff) {
      bestDiff = diff
      best = c
    }
  }
  if (best && bestDiff <= 30) return best
  // no candidate within tight tolerance: accept a looser one, else the first hit
  return best && bestDiff <= 60 ? best : candidates[0]
}
