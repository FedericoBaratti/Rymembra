import { similarity } from '../enrichment/match'

// Pure heuristics to clean YouTube-derived metadata. No electron imports.
// Live/remaster qualifiers are intentionally kept: normalizeForMatch already
// neutralizes them at match time and they carry real information.

const UNKNOWN_ARTIST = 'Artista sconosciuto'

// Tokens that mark video-platform noise, never part of a song title.
const NOISE =
  '(?:official\\s+(?:music\\s+|lyric(?:s)?\\s+)?(?:video|audio|visuali[sz]er)' +
  '|official' +
  '|(?:lyric(?:s)?|music)\\s+video' +
  '|lyrics?' +
  '|audio' +
  '|visuali[sz]er' +
  '|video\\s+ufficiale' +
  '|videoclip(?:\\s+ufficiale)?' +
  '|testo' +
  '|out\\s+now' +
  '|video' +
  '|hd|hq|full\\s+hd|4k|1080p|720p)'

// "(Official Video)", "[HD]", "(Official Music Video) [4K]" — the bracketed
// group must consist only of noise tokens, optionally joined by -, |, /, ·.
const BRACKETED_NOISE = new RegExp(
  `\\s*[([{]\\s*${NOISE}(?:\\s*[-|/·,]?\\s*${NOISE})*\\s*[)\\]}]`,
  'gi'
)
// "Song | Official Video", "Song // lyrics"
const PIPED_NOISE = new RegExp(`\\s*(?:\\||//)\\s*${NOISE}(?:\\s+${NOISE})*\\s*$`, 'i')
// bare trailing "Official Video" / "Video Ufficiale" without brackets
const TRAILING_NOISE = new RegExp(
  `\\s+(?:official\\s+(?:music\\s+)?(?:video|audio)|lyric(?:s)?\\s+video|video\\s+ufficiale)\\s*$`,
  'i'
)

export function cleanYoutubeTitle(raw: string): string {
  let s = raw.trim()
  let prev: string
  do {
    prev = s
    s = s
      .replace(BRACKETED_NOISE, '')
      .replace(PIPED_NOISE, '')
      .replace(TRAILING_NOISE, '')
      .replace(/^["'“”‘’«»](.+)["'“”‘’«»]$/, '$1')
      .replace(/[\s|·]+$/, '')
      .replace(/\s+/g, ' ')
      .trim()
  } while (s !== prev && s.length > 0)
  return s.length > 0 ? s : raw.trim()
}

export function cleanYoutubeArtist(raw: string): string {
  const s = raw
    .trim()
    .replace(/\s*-\s*Topic\s*$/i, '')
    .replace(/\s*VEVO\s*$/i, '')
    .replace(/\s+Official(?:\s+Channel)?\s*$/i, '')
    .trim()
  return s.length > 0 ? s : raw.trim()
}

export interface CleanedMeta {
  title: string
  artist: string
  changed: boolean
}

/**
 * Cleans both fields, then splits "Artist - Title" video titles when the
 * artist tag looks channel-derived (Topic/VEVO/Official suffix, unknown, or
 * already similar to the left side). A trustworthy unrelated artist blocks
 * the split so hyphenated song titles survive.
 */
export function cleanYoutubeMetadata(title: string, artist: string): CleanedMeta {
  const rawTitle = (title ?? '').trim()
  const rawArtist = (artist ?? '').trim()
  let t = cleanYoutubeTitle(rawTitle)
  let a = rawArtist ? cleanYoutubeArtist(rawArtist) : ''
  const artistWasChannel = rawArtist !== '' && a !== rawArtist

  const m = t.match(/^(.+?)\s+[-–—]\s+(.+)$/)
  if (m) {
    const left = m[1].trim()
    const right = m[2].trim()
    const artistUnreliable =
      a === '' || a === UNKNOWN_ARTIST || artistWasChannel || similarity(left, a) >= 0.65
    if (artistUnreliable && right.length > 0) {
      a = cleanYoutubeArtist(left)
      t = cleanYoutubeTitle(right)
    }
  }

  return { title: t, artist: a, changed: t !== rawTitle || a !== rawArtist }
}
