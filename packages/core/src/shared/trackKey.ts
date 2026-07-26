/**
 * Cross-device identity keys for the Google Drive library sync.
 *
 * `id` and `path` are local to each device, so the sync file identifies tracks
 * and playlists by a stable string derived from their tags. The key builds on
 * `foldText` (NFD diacritic strip + lowercase) and additionally unifies the
 * punctuation that differs between taggers and strips it entirely, so the same
 * release tagged slightly differently on two devices still folds to one key.
 *
 * Plain regex literals only (no \p{} unicode-property classes) so this stays
 * Node-12 / small-ICU safe for nodejs-mobile. Letters of any script survive
 * folding — only ASCII punctuation is removed — so non-latin titles keep their
 * distinctness rather than collapsing to an empty key.
 */
import { foldText } from './text'

// Curly quotes / primes → straight, every dash variant → '-', ellipsis → '.'
// (same set unified by the album grouping key) so they normalize identically.
const SINGLE_QUOTE_RE = /[‘’‚‛′‵]/g
const DOUBLE_QUOTE_RE = /[“”„‟″‶]/g
const DASH_RE = /[‐‑‒–—―−]/g
const ELLIPSIS_RE = /…/g
// ASCII punctuation removed after folding. Kept as an explicit char class rather
// than a unicode category so it runs on Node 12.
const PUNCT_RE = /[!"#$%&'()*+,\-./:;<=>?@[\]^_`{|}~]/g
const WHITESPACE_RE = /\s+/g

/**
 * Normalize a tag value for use inside a sync key: fold (diacritics + case),
 * unify then strip punctuation, collapse whitespace. Exported for unit testing.
 */
export function normalizeKey(input: string | null | undefined): string {
  const unified = String(input ?? '')
    .replace(SINGLE_QUOTE_RE, "'")
    .replace(DOUBLE_QUOTE_RE, '"')
    .replace(DASH_RE, '-')
    .replace(ELLIPSIS_RE, '.')
  return foldText(unified).replace(PUNCT_RE, '').replace(WHITESPACE_RE, ' ').trim()
}

export interface TrackKeyInput {
  artist?: string | null
  title?: string | null
  album?: string | null
}

/**
 * Stable cross-device track identity: normalized artist|title|album.
 * Duration is deliberately NOT part of the key: the same song re-encoded from
 * different sources (YouTube vs Spotify, mp3 vs m4a) drifts by whole seconds,
 * which made equal tracks look distinct across devices and triggered endless
 * re-downloads. Album disambiguates same-title tracks.
 */
export function trackKey(t: TrackKeyInput): string {
  const artist = normalizeKey(t.artist)
  const title = normalizeKey(t.title)
  const album = normalizeKey(t.album)
  return `${artist}|${title}|${album}`
}

const LEGACY_DURATION_TAIL_RE = /^\d+$/

/**
 * Upgrade a v1 track key (`artist|title|album|duration`) to the v2 form by
 * dropping the numeric duration tail. Segments can never contain `|` (PUNCT_RE
 * strips it), so a key with exactly 4 parts whose last is all digits is v1.
 * v2 keys (and playlist keys) pass through unchanged; idempotent.
 */
export function upgradeLegacyTrackKey(key: string): string {
  const parts = key.split('|')
  // `tail` estratto in una variabile perché noUncheckedIndexedAccess non
  // restringe l'accesso per indice tramite il controllo su length.
  const tail = parts[3]
  if (parts.length === 4 && tail !== undefined && LEGACY_DURATION_TAIL_RE.test(tail)) {
    return parts.slice(0, 3).join('|')
  }
  return key
}

/** Playlists are identified by their normalized name (renaming = new entity). */
export function playlistKey(name: string | null | undefined): string {
  return normalizeKey(name)
}
