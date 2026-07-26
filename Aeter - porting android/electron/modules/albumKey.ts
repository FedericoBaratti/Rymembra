import { foldText } from '@shared/text'

// Deterministic album-grouping key. The album identity in the library is NOT the
// raw (album, album_artist) text — that splits a single release across multiple
// album cards on any inconsistency (case/whitespace/diacritics, an edition suffix,
// or a per-track `album_artist` that falls back to a guest credit). Instead we
// derive a stable key from the NORMALIZED album title + the CONTAINING FOLDER:
//
//   albumGroupKey = fold(stripEdition(album)) + ' ' + fold(albumFolder(path))
//
// Rationale (matches how Jellyfin/Plex/Navidrome treat a local library — "one
// folder = one album"): tracks of the same release live in the same folder, so the
// folder absorbs album_artist inconsistencies; two different albums that happen to
// share a title sit in different folders and stay separate.
//
// Everything here is ICU-free (no `\p{}` regex) so it runs on nodejs-mobile's
// Node 12 / V8 7.8 (small-ICU). Validated against a battery of real-world cases
// (see albumKey.test.ts).

// Keywords that mark a trailing "(...)"/"[...]" as an edition/disc qualifier rather
// than part of the real title. Matched case/diacritic-insensitively via foldText.
const EDITION_KEYWORDS = new Set([
  'deluxe', 'remaster', 'remastered', 'bonus', 'expanded', 'anniversary',
  'edition', 'version', 'reissue', 'mono', 'stereo', 'explicit', 'clean',
  'single', 'ep', 'disc', 'disco', 'disk', 'cd', 'vol', 'volume'
])

const DISC_FOLDER_RE = /^(cd|disc|disco|disk)\s*\d+$/
const TRAILING_GROUP_RE = /[([]([^()[\]]*)[)\]]\s*$/

function looksLikeEdition(inner: string): boolean {
  const f = foldText(inner.trim())
  if (!f) return false
  if (DISC_FOLDER_RE.test(f)) return true
  // WHOLE-WORD match: a substring check would strip "(Deep Cuts)" on the "ep" in
  // "deep". Split into alphanumeric tokens and test membership.
  for (const token of f.split(/[^a-z0-9]+/)) {
    if (token && EDITION_KEYWORDS.has(token)) return true
  }
  return false
}

/**
 * Strip trailing edition/disc qualifiers wrapped in (...) or [...] — e.g.
 * "(Deluxe Edition)", "[2019 Remaster]", "(CD1)", "(Disc 2)". A parenthetical that
 * does NOT contain an edition keyword is kept (e.g. "Songs (For Drella)"). Never
 * returns empty: if stripping would empty the title, the original trimmed title is
 * returned. Exported for unit testing.
 */
export function stripEditionSuffix(album: string): string {
  let s = String(album ?? '').trim()
  for (;;) {
    const m = s.match(TRAILING_GROUP_RE)
    if (m && looksLikeEdition(m[1])) {
      s = s.slice(0, m.index).trim()
      continue
    }
    break
  }
  return s || String(album ?? '').trim()
}

function splitPath(path: string): string[] {
  return String(path ?? '').split(/[\\/]+/).filter(Boolean)
}

/**
 * The full containing-DIRECTORY path of a track (not just the leaf folder name —
 * two artists can both have a "Greatest Hits" folder). A leaf folder that is a disc
 * marker ("CD1", "Disc 2", "Disco 1") collapses to its parent so multi-disc albums
 * split across subfolders group as one release. Exported for unit testing.
 */
export function albumFolder(path: string): string {
  const parts = splitPath(path)
  parts.pop() // drop the filename
  if (parts.length >= 2 && DISC_FOLDER_RE.test(foldText(parts[parts.length - 1]))) {
    parts.pop()
  }
  return parts.join('/')
}

// Punctuation that varies between taggers for the SAME release and must not split
// it: curly quotes / primes → straight, every dash variant → '-', ellipsis → '...'.
// Plain regex literals (no \p{}) so this stays Node-12 / small-ICU safe.
const SINGLE_QUOTE_RE = /[‘’‚‛′‵]/g
const DOUBLE_QUOTE_RE = /[“”„‟″‶]/g
const DASH_RE = /[‐‑‒–—―−]/g
const ELLIPSIS_RE = /…/g
const WHITESPACE_RE = /\s+/g

/**
 * Normalize a string for use inside an album grouping key. Builds on `foldText`
 * (lowercase + diacritic strip) but ALSO unifies the punctuation that differs
 * between taggers (curly vs straight quotes, en/em-dash vs hyphen, ellipsis) and
 * collapses internal whitespace runs — the variations that previously gave the
 * same release two different keys. Exported for unit testing.
 */
export function normalizeKeyText(input: string): string {
  const replaced = String(input ?? '')
    .replace(SINGLE_QUOTE_RE, "'")
    .replace(DOUBLE_QUOTE_RE, '"')
    .replace(DASH_RE, '-')
    .replace(ELLIPSIS_RE, '...')
  return foldText(replaced).replace(WHITESPACE_RE, ' ').trim()
}

/** Stable per-track grouping key: normalized album title + containing folder. */
export function albumGroupKey(album: string, path: string): string {
  return `${normalizeKeyText(stripEditionSuffix(album))} ${normalizeKeyText(albumFolder(path))}`
}

export interface ArtistRow {
  artist?: string | null
  album_artist?: string | null
}

/** The effective grouping artist: album_artist when present, else the track artist. */
function effectiveArtist(r: ArtistRow): string {
  return (r.album_artist ?? r.artist ?? '').trim()
}

/**
 * Pick the canonical album artist for a group: the most frequent effective artist;
 * ties broken by the shortest value (drops a "feat. …" suffix), then alphabetically
 * for determinism. Exported for unit testing.
 */
export function pickCanonical(rows: ArtistRow[]): string {
  const counts = new Map<string, number>()
  for (const r of rows) {
    const v = effectiveArtist(r)
    if (v) counts.set(v, (counts.get(v) ?? 0) + 1)
  }
  let best = ''
  let bestCount = -1
  for (const [value, count] of counts) {
    if (
      count > bestCount ||
      (count === bestCount && value.length < best.length) ||
      (count === bestCount && value.length === best.length && value < best)
    ) {
      best = value
      bestCount = count
    }
  }
  return best
}

/** Alias kept for clarity at call sites that build the album row's display artist. */
export const canonicalAlbumArtist = pickCanonical

export interface AlbumAggInput {
  album_key: string
  album: string
  album_artist: string | null
  artist: string | null
  year: number | null
  cover_art_hash: string | null
  // Authoritative release identifiers read from file tags (MusicBrainz Picard et al.)
  // or written by the downloader. Used to MERGE base groups that are the same release;
  // never to split. Optional so existing callers/tests need not provide them.
  mb_release_group_id?: string | null
  mb_release_id?: string | null
  spotify_album_id?: string | null
  // Cover provenance/quality (schema v14, LEFT JOIN cover_art). Feed the
  // canonical-cover pick; optional so existing callers/tests need not provide them.
  cover_source?: string | null
  cover_w?: number | null
  cover_h?: number | null
}

export interface AlbumAgg {
  album_key: string
  title: string
  artist: string
  year: number | null
  total_tracks: number
  cover_art_hash: string | null
  mb_album_id: string | null
  spotify_id: string | null
}

// Cover provenance rank: an image embedded in the file tags (or hand-picked by
// the user) beats a provider/Spotify match, which beats a Cover Art Archive
// guess; legacy rows without provenance rank last.
const COVER_SOURCE_RANK: Record<string, number> = { tag: 3, provider: 2, spotify: 2, caa: 1 }

/**
 * Canonical album cover: for each distinct hash among the members, order by
 * (best provenance, member count, pixel area, hash asc) and take the max.
 * Deterministic and recomputable — replaces the old "first member's hash,
 * sticky forever" pick, which let one odd track hijack the whole album.
 * Exported for unit testing.
 */
export function pickAlbumCover(members: AlbumAggInput[]): string | null {
  interface CoverCand { hash: string; rank: number; count: number; pixels: number }
  const byHash = new Map<string, CoverCand>()
  for (const m of members) {
    const hash = m.cover_art_hash
    if (!hash) continue
    const rank = COVER_SOURCE_RANK[m.cover_source ?? ''] ?? 0
    const pixels = (m.cover_w ?? 0) * (m.cover_h ?? 0)
    const cur = byHash.get(hash)
    if (cur) {
      cur.count++
      if (rank > cur.rank) cur.rank = rank
      if (pixels > cur.pixels) cur.pixels = pixels
    } else {
      byHash.set(hash, { hash, rank, count: 1, pixels })
    }
  }
  let best: CoverCand | null = null
  for (const c of byHash.values()) {
    if (
      !best ||
      c.rank > best.rank ||
      (c.rank === best.rank && c.count > best.count) ||
      (c.rank === best.rank && c.count === best.count && c.pixels > best.pixels) ||
      (c.rank === best.rank && c.count === best.count && c.pixels === best.pixels && c.hash < best.hash)
    ) {
      best = c
    }
  }
  return best ? best.hash : null
}

/** Most frequent non-empty value of a field across members; ties → lexicographically smallest. */
function dominant(members: AlbumAggInput[], get: (m: AlbumAggInput) => string | null | undefined): string | null {
  const counts = new Map<string, number>()
  for (const m of members) {
    const v = (get(m) ?? '').trim()
    if (v) counts.set(v, (counts.get(v) ?? 0) + 1)
  }
  let best = ''
  let bestCount = -1
  for (const [value, count] of counts) {
    if (count > bestCount || (count === bestCount && value < best)) {
      best = value
      bestCount = count
    }
  }
  return best || null
}

/** Aggregate a set of member rows into a single album row under the given key. */
function aggregateGroup(album_key: string, members: AlbumAggInput[]): AlbumAgg {
  // most frequent original album title (deterministic ties via shortest then alphabetical)
  const titleCounts = new Map<string, number>()
  for (const m of members) {
    const t = (m.album ?? '').trim()
    if (t) titleCounts.set(t, (titleCounts.get(t) ?? 0) + 1)
  }
  let title = ''
  let titleBest = -1
  for (const [t, c] of titleCounts) {
    if (
      c > titleBest ||
      (c === titleBest && t.length < title.length) ||
      (c === titleBest && t.length === title.length && t < title)
    ) {
      title = t
      titleBest = c
    }
  }

  let year: number | null = null
  for (const m of members) {
    if (m.year != null && (year == null || m.year > year)) year = m.year
  }

  return {
    album_key,
    title: title || 'Album sconosciuto',
    artist: pickCanonical(members) || 'Artista sconosciuto',
    year,
    total_tracks: members.length,
    cover_art_hash: pickAlbumCover(members),
    // mb_album_id = the release (album) MBID when present, else fall back to the
    // release-group MBID so the column is populated either way.
    mb_album_id: dominant(members, (m) => m.mb_release_id) ?? dominant(members, (m) => m.mb_release_group_id),
    spotify_id: dominant(members, (m) => m.spotify_album_id)
  }
}

/**
 * Aggregate track rows into one album per `album_key` (NO cross-group merge).
 * Kept for the v10 migration backfill, which keys albums on the raw per-track key.
 */
export function aggregateAlbums(rows: AlbumAggInput[]): AlbumAgg[] {
  const groups = new Map<string, AlbumAggInput[]>()
  for (const r of rows) {
    const arr = groups.get(r.album_key)
    if (arr) arr.push(r)
    else groups.set(r.album_key, [r])
  }
  const out: AlbumAgg[] = []
  for (const [album_key, members] of groups) out.push(aggregateGroup(album_key, members))
  return out
}

export interface AlbumBuildResult {
  /** One album per super-group, sorted by canonical key for deterministic output. */
  albums: AlbumAgg[]
  /** Every per-track base `album_key` → the canonical key its album ended up under. */
  remap: Map<string, string>
}

/**
 * Build the album rows applying the "Persistent ID" identity model (Navidrome-style):
 *
 *  1. group tracks by their per-track base `album_key` (folder + normalized title);
 *  2. MERGE base groups that share an authoritative id — MusicBrainz release-group,
 *     then release, then Spotify album — via union-find;
 *  3. give each super-group a canonical key: the shared id (`mbrg:`/`mbr:`/`sp:`) when
 *     present, else the lexicographically smallest base key in the union.
 *
 * Ids only ever UNION groups, never split them, so partial tagging (some tracks of an
 * album carry the id, others don't) can never re-introduce a split. Returns the album
 * rows plus a remap so callers can rewrite `tracks.album_key` to the canonical value
 * (keeping the tracks⋈albums join and the artist album-count consistent).
 */
export function buildAlbumGroups(rows: AlbumAggInput[]): AlbumBuildResult {
  const baseGroups = new Map<string, AlbumAggInput[]>()
  for (const r of rows) {
    const arr = baseGroups.get(r.album_key)
    if (arr) arr.push(r)
    else baseGroups.set(r.album_key, [r])
  }
  const baseKeys = [...baseGroups.keys()]

  // union-find over base keys
  const parent = new Map<string, string>(baseKeys.map((k) => [k, k]))
  const find = (x: string): string => {
    let root = x
    while (parent.get(root) !== root) root = parent.get(root) as string
    while (parent.get(x) !== root) {
      const next = parent.get(x) as string
      parent.set(x, root)
      x = next
    }
    return root
  }
  const union = (a: string, b: string): void => {
    const ra = find(a)
    const rb = find(b)
    if (ra !== rb) parent.set(ra < rb ? rb : ra, ra < rb ? ra : rb)
  }

  // link base groups that share a dominant id, for each id tier
  const linkBy = (get: (m: AlbumAggInput) => string | null | undefined): void => {
    const firstKeyForId = new Map<string, string>()
    for (const k of baseKeys) {
      const id = dominant(baseGroups.get(k) as AlbumAggInput[], get)
      if (!id) continue
      const seen = firstKeyForId.get(id)
      if (seen) union(seen, k)
      else firstKeyForId.set(id, k)
    }
  }
  linkBy((m) => m.mb_release_group_id)
  linkBy((m) => m.mb_release_id)
  linkBy((m) => m.spotify_album_id)

  // collect super-groups by root
  const membersByRoot = new Map<string, AlbumAggInput[]>()
  const baseKeysByRoot = new Map<string, string[]>()
  for (const k of baseKeys) {
    const root = find(k)
    const mem = membersByRoot.get(root) ?? []
    mem.push(...(baseGroups.get(k) as AlbumAggInput[]))
    membersByRoot.set(root, mem)
    const bks = baseKeysByRoot.get(root) ?? []
    bks.push(k)
    baseKeysByRoot.set(root, bks)
  }

  const albums: AlbumAgg[] = []
  const remap = new Map<string, string>()
  for (const [root, members] of membersByRoot) {
    const rg = dominant(members, (m) => m.mb_release_group_id)
    const rel = dominant(members, (m) => m.mb_release_id)
    const sp = dominant(members, (m) => m.spotify_album_id)
    const memberBaseKeys = (baseKeysByRoot.get(root) as string[]).slice().sort()
    const canonical = rg ? `mbrg:${rg}` : rel ? `mbr:${rel}` : sp ? `sp:${sp}` : memberBaseKeys[0]
    albums.push(aggregateGroup(canonical, members))
    for (const bk of memberBaseKeys) remap.set(bk, canonical)
  }
  albums.sort((a, b) => (a.album_key < b.album_key ? -1 : a.album_key > b.album_key ? 1 : 0))
  return { albums, remap }
}
