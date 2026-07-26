import { logWarn } from '../logger'
import { CircuitOpenError, HttpError, NetworkError } from '../net/errors'
import {
  MATCH_THRESHOLD,
  UNKNOWN_ARTIST,
  albumSimilarity,
  candidateArtist,
  durationDelta,
  normalizeForMatch,
  scoreCandidate,
  similarity,
  type MetaCandidate,
  type TrackForMatch
} from './match'
import { decide, type Evidence, type FingerprintStatus, type Verdict } from './decision'
import type { FingerprintResult } from './shazam/recognizeTrack'
import { mbSearchRecordings, mbGetRecording } from './services/musicbrainz'
import { itunesSearch } from './services/itunes'
import { deezerSearch } from './services/deezer'
import type { MbRecording } from './schemas'

export interface ResolveResult {
  best: MetaCandidate
  /** 0..1 — composite match score, boosted by consensus/fingerprint. */
  confidence: number
  /** apply = safe to write; needs-review = abstain, write nothing. */
  verdict: Verdict
  evidence: Evidence
}

/**
 * Strict agreement between two candidates: same normalized title+artist AND
 * agreement on album or duration (±4s) — title+artist alone can pair a studio
 * track with a karaoke/remix release and graft the wrong cover.
 */
function agreesStrictly(a: MetaCandidate, b: MetaCandidate): boolean {
  if (normalizeForMatch(a.title) !== normalizeForMatch(b.title)) return false
  if (normalizeForMatch(a.artist) !== normalizeForMatch(b.artist)) return false
  const albumAgree =
    a.album != null && b.album != null && a.album !== '' && b.album !== '' &&
    normalizeForMatch(a.album) === normalizeForMatch(b.album)
  const durationAgree =
    a.durationMs != null && b.durationMs != null && Math.abs(a.durationMs - b.durationMs) <= 4000
  return Boolean(albumAgree) || durationAgree
}

/** Picks the release best suited for cover art (Official album first). */
function pickReleaseInfo(rec: MbRecording): {
  releaseGroupId?: string
  releaseIds: string[]
} {
  const releases = rec.releases ?? []
  const rank = (r: (typeof releases)[number]): number => {
    let s = 0
    if ((r.status ?? '').toLowerCase() === 'official') s += 2
    if ((r['release-group']?.['primary-type'] ?? '').toLowerCase() === 'album') s += 1
    return s
  }
  const sorted = releases.slice().sort((a, b) => rank(b) - rank(a))
  return {
    releaseGroupId: sorted[0]?.['release-group']?.id ?? undefined,
    releaseIds: releases.map((r) => r.id)
  }
}

export function mbToCandidate(rec: MbRecording): MetaCandidate {
  const info = pickReleaseInfo(rec)
  const firstRelease = rec.releases?.[0]
  const year = firstRelease?.date ? Number(firstRelease.date.slice(0, 4)) || null : null
  return {
    source: 'mb',
    title: rec.title,
    artist: candidateArtist(rec),
    album: firstRelease?.title ?? null,
    year,
    durationMs: rec.length ?? null,
    mbRecordingId: rec.id,
    mbReleaseGroupId: info.releaseGroupId,
    mbReleaseIds: info.releaseIds,
    mbScore: rec.score
  }
}

interface ProviderOutcome {
  candidates: MetaCandidate[]
  failed: boolean
}

async function runProvider(
  fn: () => Promise<MetaCandidate[]>,
  label: string
): Promise<ProviderOutcome> {
  try {
    return { candidates: await fn(), failed: false }
  } catch (err) {
    // network/circuit/5xx → provider unavailable (don't poison no-match);
    // other errors are logged but treated as "no candidates".
    const unavailable =
      err instanceof NetworkError ||
      err instanceof CircuitOpenError ||
      (err instanceof HttpError && err.status >= 500)
    logWarn('enrich', `Provider ${label} fallito`, err)
    return { candidates: [], failed: unavailable }
  }
}

async function mbCandidates(track: TrackForMatch): Promise<MetaCandidate[]> {
  const recs = await mbSearchRecordings(track.title, track.artist)
  const out: MetaCandidate[] = []
  for (const rec of recs) {
    if (rec.score !== undefined && rec.score < 50) continue
    out.push(mbToCandidate(rec))
  }
  return out
}

/**
 * Resolves authoritative metadata for a track across keyless providers
 * (MusicBrainz + iTunes + Deezer, plus the optional Shazam fingerprint and an
 * optional AcoustID recording id) and returns the highest-scoring candidate
 * above {@link MATCH_THRESHOLD}. Cross-provider agreement and an audio
 * fingerprint (Shazam or AcoustID) boost confidence; the decision layer
 * (decision.ts) rules whether the result is safe to apply.
 *
 * `acoustidRecordingId` (when the caller ran fpcalc→AcoustID) is a hard audio
 * identifier: the matching MusicBrainz recording is trusted over the textual
 * best and treated exactly like a corroborated fingerprint.
 *
 * Throws NetworkError only when *every* provider was unreachable (so the
 * caller can leave enrich_status untouched instead of persisting no-match).
 */
export async function resolveMetadata(
  track: TrackForMatch,
  fingerprint?: FingerprintResult | null,
  acoustidRecordingId?: string | null
): Promise<ResolveResult | null> {
  const [mb, itunes, deezer] = await Promise.all([
    runProvider(() => mbCandidates(track), 'MusicBrainz'),
    runProvider(() => itunesSearch(track.title, track.artist), 'iTunes'),
    runProvider(() => deezerSearch(track.title, track.artist), 'Deezer')
  ])

  // AcoustID positively identified the recording by its audio: fetch the
  // authoritative MB recording (carries MB ids + the CAA cover chain). A failed
  // lookup degrades to null so the textual path still runs.
  let acoustidCand: MetaCandidate | null = null
  if (acoustidRecordingId) {
    try {
      const rec = await mbGetRecording(acoustidRecordingId)
      if (rec) acoustidCand = mbToCandidate(rec)
    } catch (err) {
      logWarn('enrich', 'Lookup MusicBrainz da AcoustID fallito', err)
    }
  }

  const fpCand: MetaCandidate | null = fingerprint
    ? {
        source: 'shazam',
        title: fingerprint.title,
        artist: fingerprint.artist,
        album: fingerprint.album ?? null,
        year: fingerprint.year ?? null,
        coverUrl: fingerprint.coverUrl ?? null,
        durationMs: null
      }
    : null

  const all = [...mb.candidates, ...itunes.candidates, ...deezer.candidates]
  if (fpCand) all.push(fpCand)
  if (acoustidCand) all.push(acoustidCand)
  if (all.length === 0) {
    if (mb.failed && itunes.failed && deezer.failed) {
      throw new NetworkError('Nessun provider di metadati raggiungibile')
    }
    return null
  }

  const rank = (cand: MetaCandidate): number =>
    scoreCandidate(track, {
      title: cand.title,
      artist: cand.artist,
      durationMs: cand.durationMs
    })

  let best: { cand: MetaCandidate; score: number } | null = null
  for (const cand of all) {
    const score = rank(cand)
    if (!best || score > best.score) best = { cand, score }
  }

  // True when a candidate names the same song the fingerprint identified.
  const matchesFingerprint = (c: MetaCandidate): boolean =>
    fingerprint != null &&
    (c.source === 'shazam' ||
      (similarity(c.title, fingerprint.title) >= 0.8 &&
        similarity(c.artist, fingerprint.artist) >= 0.8))

  if (acoustidCand) {
    // AcoustID identified the recording by its audio (score > 0.7): trust the
    // authoritative MB recording over the textual best — the local tags may be
    // wrong but the audio isn't. Corroborated below like a fingerprint match.
    best = { cand: acoustidCand, score: rank(acoustidCand) }
  } else if (fingerprint && fingerprint.corroborated) {
    // The audio itself identified the track (twice). Prefer the best textual
    // candidate naming the same song — it carries MB ids and the CAA cover
    // chain — else fall back to the Shazam candidate, whatever its text score
    // against the (possibly garbage) local tags.
    let preferred: { cand: MetaCandidate; score: number } | null = null
    for (const cand of all) {
      if (cand.source === 'shazam' || !matchesFingerprint(cand)) continue
      const score = rank(cand)
      if (!preferred || score > preferred.score) preferred = { cand, score }
    }
    best =
      preferred && preferred.score >= MATCH_THRESHOLD
        ? preferred
        : { cand: fpCand as MetaCandidate, score: rank(fpCand as MetaCandidate) }
  } else if (!best || best.score < MATCH_THRESHOLD) {
    return null
  }

  // Consensus: another provider naming the same release (strict: title+artist
  // plus album-or-duration agreement) raises confidence.
  const agreement = all.some(
    (c) => c !== best!.cand && c.source !== best!.cand.source && agreesStrictly(c, best!.cand)
  )

  // Prefer a direct cover URL from a strictly-agreeing provider when the
  // winner lacks one — title+artist alone could borrow another release's art.
  if (!best.cand.coverUrl) {
    const withCover = all.find((c) => c.coverUrl && agreesStrictly(c, best!.cand))
    if (withCover) best.cand.coverUrl = withCover.coverUrl
  }

  // An AcoustID hit is an audio identification just like a corroborated Shazam
  // match, so it flows through the same fingerprint channel of the decision.
  const acoustidProven = acoustidCand != null
  const fpStatus: FingerprintStatus = acoustidProven
    ? 'corroborated'
    : !fingerprint
      ? 'none'
      : fingerprint.corroborated
        ? 'corroborated'
        : 'single'
  const artistKnown = Boolean(track.artist) && track.artist !== UNKNOWN_ARTIST
  const evidence: Evidence = {
    titleSim: similarity(track.title, best.cand.title),
    artistSim: artistKnown && best.cand.artist ? similarity(track.artist, best.cand.artist) : null,
    durationDeltaSec: durationDelta(track.duration, best.cand.durationMs),
    albumSim: albumSimilarity(track.album, best.cand.album),
    consensus: agreement,
    fingerprint: fpStatus,
    fingerprintAgrees: acoustidProven
      ? true
      : fpStatus === 'none'
        ? undefined
        : matchesFingerprint(best.cand)
  }
  const decision = decide(evidence, best.score)
  let confidence = decision.confidence
  // The audio positively identified the track (corroborated Shazam or an
  // AcoustID hit) AND the chosen candidate names that recording: near-certainty
  // regardless of how dirty the local tags are.
  if (fpStatus === 'corroborated' && evidence.fingerprintAgrees) {
    confidence = Math.max(confidence, 0.95)
  }

  return { best: best.cand, confidence, verdict: decision.verdict, evidence }
}
