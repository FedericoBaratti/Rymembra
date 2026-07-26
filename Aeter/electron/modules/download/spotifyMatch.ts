import type { SpotifyTrack } from '../spotify/types'

// Pure matching/naming helpers for the Spotify engine (no electron imports →
// unit-tested). Kept separate from spotifyEngine.ts, which spawns yt-dlp.

export interface Candidate {
  url: string
  durationSec: number | null
  title: string
}

/** Filesystem-safe segment, ICU-free (no \p{} — small-ICU on nodejs-mobile). */
export function sanitizeSegment(s: string): string {
  let out = ''
  for (const ch of s) {
    const code = ch.charCodeAt(0)
    if (
      ch === '/' || ch === '\\' || ch === ':' || ch === '*' || ch === '?' || ch === '"' ||
      ch === '<' || ch === '>' || ch === '|' || code < 0x20
    ) {
      out += ' '
    } else {
      out += ch
    }
  }
  out = out.replace(/\s+/g, ' ').trim()
  return out.slice(0, 120) || 'Senza titolo'
}

export function buildQuery(track: SpotifyTrack): string {
  const artist = track.artist ?? ''
  return `${artist} ${track.title}`.replace(/\s+/g, ' ').trim()
}

/**
 * A looser query for a second search attempt when the exact-title query returns
 * nothing. Spotify titles are often decorated ("(feat. X)", "- Remastered 2011",
 * "(Live)") and the primary artist field may bundle features ("A, B & C"); these
 * decorations frequently make the strict YouTube search miss a track that is
 * otherwise available. We keep only the primary artist and strip the decorations.
 * ICU-free / Node 12-safe (no \p{}, no replaceAll).
 */
export function buildFallbackQuery(track: SpotifyTrack): string {
  const artist = (track.artist ?? '').split(/,|&|\bfeat\.?\b|\bft\.?\b/i)[0].trim()
  let title = track.title
  // Drop bracketed/parenthesised suffixes: (feat. X), [Live], {Bonus}, …
  title = title.replace(/[([{][^)\]}]*[)\]}]/g, ' ')
  // Drop " - <…> Version/Remaster/Mix/Edit/Live/…" tails.
  title = title.replace(
    /\s[-–]\s.*\b(version|remaster(?:ed)?|mix|edit|mono|stereo|live|radio|single|remix|acoustic|demo|deluxe|anniversary)\b.*$/i,
    ' '
  )
  title = title.replace(/\s+/g, ' ').trim()
  return `${artist} ${title}`.replace(/\s+/g, ' ').trim()
}

/** Pick the candidate whose duration is closest to the Spotify track (±30s), else the first. */
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
