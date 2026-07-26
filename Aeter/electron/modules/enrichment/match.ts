import type { MbRecording } from './schemas'

// Pure matching heuristics for MusicBrainz candidates. No electron imports.

export const UNKNOWN_ARTIST = 'Artista sconosciuto'
export const UNKNOWN_ALBUM = 'Album sconosciuto'

/**
 * Replace punctuation/symbols with spaces. Implemented as a codepoint scan
 * instead of `\p{L}\p{N}` (Unicode property escapes): those require full ICU
 * data, which nodejs-mobile (small-ICU) lacks — there `\p{L}` throws "Invalid
 * property name in character class", which silently broke auto-enrichment on
 * Android. This drops ASCII + Latin-1 + general punctuation/symbols while
 * keeping letters of every script (>= U+00C0) and all digits.
 */
function stripPunct(s: string): string {
  let out = ''
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i)
    const isPunct =
      (c >= 0x0020 && c <= 0x002f) ||
      (c >= 0x003a && c <= 0x0040) ||
      (c >= 0x005b && c <= 0x0060) ||
      (c >= 0x007b && c <= 0x00bf) ||
      (c >= 0x2000 && c <= 0x206f)
    out += isPunct ? ' ' : s[i]
  }
  return out
}

/**
 * lowercase, NFD-strip diacritics, drop "(feat. …)" / trailing "feat. …",
 * drop bracketed remaster/live/edit qualifiers, strip punctuation,
 * collapse whitespace.
 */
export function normalizeForMatch(s: string): string {
  const cleaned = s
    .toLowerCase()
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/\s*[([](?:feat\.?|ft\.?|featuring)\b[^)\]]*[)\]]/g, '')
    .replace(/\s+(?:feat\.?|ft\.?|featuring)\s+.*$/, '')
    .replace(
      /\s*[([][^)\]]*\b(?:remaster(?:ed)?|live|demo|mono|stereo|edit|version|mix|deluxe)\b[^)\]]*[)\]]/g,
      ''
    )
    .replace(/\s+-\s+(?:remaster(?:ed)?(?:\s+\d{4})?|\d{4}\s+remaster(?:ed)?|live\b.*|single version|radio edit)\s*$/, '')
  return stripPunct(cleaned).replace(/\s+/g, ' ').trim()
}

/**
 * Strips noise that pollutes search queries built from messy tags (YouTube
 * downloads especially): bracketed tags, "(official video)" / "lyric video",
 * trailing "feat. …", " - Topic". ICU-free (no `\p{}`), no `.replaceAll()`.
 */
export function cleanQueryText(s: string): string {
  return s
    .replace(/\[[^\]]*\]/g, ' ')
    .replace(
      /\((?:[^)]*\b(?:official|video|audio|lyrics?|hd|hq|mv|visualizer|remaster(?:ed)?|live|explicit)\b[^)]*)\)/gi,
      ' '
    )
    .replace(/\s+(?:feat\.?|ft\.?|featuring)\s+.*$/i, ' ')
    .replace(/\s[-–—]\s*topic\s*$/i, ' ')
    .replace(
      /\b(?:official\s+(?:music\s+)?video|official\s+audio|lyrics?\s+video|visualizer|audio\s+only|full\s+album)\b/gi,
      ' '
    )
    .replace(/\s+/g, ' ')
    .trim()
}

function bigrams(s: string): Map<string, number> {
  const map = new Map<string, number>()
  for (let i = 0; i < s.length - 1; i++) {
    const bg = s.slice(i, i + 2)
    map.set(bg, (map.get(bg) ?? 0) + 1)
  }
  return map
}

/** Sørensen–Dice coefficient over character bigrams of normalized strings, 0..1. */
export function similarity(a: string, b: string): number {
  const na = normalizeForMatch(a)
  const nb = normalizeForMatch(b)
  if (na === nb) return na.length > 0 ? 1 : 0
  if (na.length < 2 || nb.length < 2) return 0
  const ba = bigrams(na)
  const bb = bigrams(nb)
  let overlap = 0
  for (const [bg, count] of ba) {
    const other = bb.get(bg)
    if (other) overlap += Math.min(count, other)
  }
  return (2 * overlap) / (na.length - 1 + nb.length - 1)
}

/** 1.0 within ±3s, linear falloff to 0 at ±15s; 0.5 (neutral) when unknown. */
function durationScore(trackSeconds: number, recordingMs: number | null | undefined): number {
  if (!trackSeconds || recordingMs == null || recordingMs <= 0) return 0.5
  const diff = Math.abs(trackSeconds - recordingMs / 1000)
  if (diff <= 3) return 1
  if (diff >= 15) return 0
  return 1 - (diff - 3) / 12
}

/** Absolute duration difference in seconds; null when either side is unknown. */
export function durationDelta(
  trackSeconds: number | null | undefined,
  recordingMs: number | null | undefined
): number | null {
  if (!trackSeconds || recordingMs == null || recordingMs <= 0) return null
  return Math.abs(trackSeconds - recordingMs / 1000)
}

/**
 * Album similarity for the decision layer; null (unknown, NOT agreement)
 * when either side is missing or the placeholder 'Album sconosciuto'.
 */
export function albumSimilarity(
  trackAlbum: string | null | undefined,
  candAlbum: string | null | undefined
): number | null {
  if (!trackAlbum || trackAlbum === UNKNOWN_ALBUM) return null
  if (!candAlbum) return null
  return similarity(trackAlbum, candAlbum)
}

export function candidateArtist(rec: MbRecording): string {
  return rec['artist-credit']?.map((a) => a.name).join(', ') ?? ''
}

export interface TrackForMatch {
  title: string
  artist: string
  duration: number
  album?: string | null
}

/** Minimal shape any provider candidate must expose to be scored. */
export interface CandidateMeta {
  title: string
  artist: string
  durationMs?: number | null
  album?: string | null
}

/** Confidence threshold a candidate must clear to be accepted as a match. */
export const MATCH_THRESHOLD = 0.65

/**
 * Composite confidence: title similarity (0.4) + artist similarity (0.3) +
 * duration agreement (0.3). Unknown artist or missing duration score as
 * neutral (0.5). Provider-agnostic — used for MusicBrainz, iTunes and Deezer.
 */
export function scoreCandidate(track: TrackForMatch, cand: CandidateMeta): number {
  const titleSim = similarity(track.title, cand.title)
  const artistKnown = track.artist && track.artist !== UNKNOWN_ARTIST
  const artistSim = artistKnown && cand.artist ? similarity(track.artist, cand.artist) : 0.5
  return 0.4 * titleSim + 0.3 * artistSim + 0.3 * durationScore(track.duration, cand.durationMs)
}

/**
 * Unified candidate from any metadata provider. Pure data type (no electron),
 * so providers and the resolver can share it without import cycles.
 */
export interface MetaCandidate {
  source: 'mb' | 'itunes' | 'deezer' | 'acoustid' | 'shazam'
  title: string
  artist: string
  album?: string | null
  year?: number | null
  genre?: string | null
  durationMs?: number | null
  /** Direct cover URL (iTunes artwork / Deezer cover_xl / Spotify), if any. */
  coverUrl?: string | null
  mbRecordingId?: string | null
  mbReleaseGroupId?: string | null
  /** All MB release ids for this recording (CAA per-release fallback). */
  mbReleaseIds?: string[]
  /** MusicBrainz' own search score (sanity gate), when the source is MB. */
  mbScore?: number
}

/**
 * Returns the best candidate above the threshold. MB's own search score acts
 * as a sanity gate (drops < 50) without entering the composite.
 */
export function pickBestRecording(
  track: TrackForMatch,
  candidates: MbRecording[]
): MbRecording | null {
  let best: { rec: MbRecording; score: number } | null = null
  for (const rec of candidates) {
    if (rec.score !== undefined && rec.score < 50) continue
    const score = scoreCandidate(track, {
      title: rec.title,
      artist: candidateArtist(rec),
      durationMs: rec.length
    })
    if (!best || score > best.score) best = { rec, score }
  }
  return best && best.score >= MATCH_THRESHOLD ? best.rec : null
}
