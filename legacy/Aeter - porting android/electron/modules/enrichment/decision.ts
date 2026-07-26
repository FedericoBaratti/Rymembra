// Pure decision layer separating candidate *ranking* (scoreCandidate) from
// the decision to *apply* metadata/covers. Only positive evidence can push a
// candidate to 'apply': an unknown axis (null) never counts toward it, unlike
// the neutral 0.5 that scoreCandidate uses for ranking. No electron imports.

import { MATCH_THRESHOLD } from './match'

export type FingerprintStatus = 'none' | 'single' | 'corroborated'

/**
 * Comparison evidence between a library track and the winning candidate.
 * `null` means the axis is unknown on at least one side (missing duration,
 * placeholder artist/album) — unknown is NOT agreement.
 */
export interface Evidence {
  titleSim: number
  artistSim: number | null
  /** Absolute duration difference in seconds; null when either side is unknown. */
  durationDeltaSec: number | null
  albumSim: number | null
  /** Strict cross-provider consensus: title+artist AND album-or-duration agreement. */
  consensus: boolean
  fingerprint: FingerprintStatus
  /** Whether the textual candidate is coherent with the fingerprint result. */
  fingerprintAgrees?: boolean
}

export type Verdict = 'apply' | 'needs-review' | 'no-match'

export interface Decision {
  verdict: Verdict
  confidence: number
}

// Strong-axis thresholds: each axis must be *known* and clearly agreeing.
// Album never counts toward apply — it only vetoes (contradiction) and
// tightens cross-provider consensus in resolve.ts.
const STRONG_TITLE = 0.9
const STRONG_ARTIST = 0.85
const STRONG_DURATION_SEC = 4

// A known axis this dissimilar means the candidate describes something else
// (karaoke/tribute/remix album, different artist): never auto-apply on text.
const CONTRADICTION = 0.35

/**
 * Confidence floor for writing a cover (first application AND replacement —
 * same bar, so a fresh track can't get a wrong cover a re-enrich would refuse).
 */
export const COVER_CONFIDENCE = 0.82

/**
 * Decides whether the best-ranked candidate is safe to apply. Abstention
 * (`needs-review`) is preferred over a plausible-but-unproven match: nothing
 * gets written and the track is queued for a later retry (with fingerprint).
 *
 * Apply requires at least one of:
 *  1. corroborated fingerprint with a coherent textual candidate;
 *  2. >= 2 strong axes among {title, artist, duration};
 *  3. strong title + strict cross-provider consensus.
 * Rules 2-3 are vetoed when a known axis outright contradicts (< 0.35).
 */
export function decide(evidence: Evidence, baseScore: number): Decision {
  const fingerprintProven = evidence.fingerprint === 'corroborated' && evidence.fingerprintAgrees === true

  const strongTitle = evidence.titleSim >= STRONG_TITLE
  const strongArtist = evidence.artistSim != null && evidence.artistSim >= STRONG_ARTIST
  const strongDuration =
    evidence.durationDeltaSec != null && evidence.durationDeltaSec <= STRONG_DURATION_SEC
  const strongAxes = [strongTitle, strongArtist, strongDuration].filter(Boolean).length

  const contradiction =
    (evidence.artistSim != null && evidence.artistSim < CONTRADICTION) ||
    (evidence.albumSim != null && evidence.albumSim < CONTRADICTION)

  const textualApply = !contradiction && (strongAxes >= 2 || (strongTitle && evidence.consensus))

  let confidence = baseScore
  if (evidence.consensus) confidence += 0.1
  if (evidence.fingerprint === 'corroborated') confidence += 0.25
  else if (evidence.fingerprint === 'single' && evidence.fingerprintAgrees) confidence += 0.1
  confidence = Math.min(1, confidence)

  if (fingerprintProven || textualApply) return { verdict: 'apply', confidence }
  if (baseScore >= MATCH_THRESHOLD) return { verdict: 'needs-review', confidence }
  return { verdict: 'no-match', confidence }
}
