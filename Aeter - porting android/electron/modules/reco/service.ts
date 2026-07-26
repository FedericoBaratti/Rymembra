import { getDb } from '../db'
import type {
  Track,
  RecoResultTracks,
  RadioSeed,
  HomeFeed,
  HomeSection,
  ExternalRecoTrack
} from '@shared/types'
import { fetchSimilarRecordings, fetchRadioByPrompt } from './listenbrainz'
import { fetchSimilarTracks, fetchSimilarArtists } from '../enrichment/services/lastfmSimilar'
import { deezerSearch } from '../enrichment/services/deezer'
import { logWarn } from '../logger'
import {
  blendCandidates,
  buildLibraryIndex,
  splitByLibrary,
  rankByAffinity,
  candidateKey,
  type WeightedList,
  type IndexableTrack,
  type LibraryIndex,
  type RecoResult
} from './engine'

// Wires the pure engine to the DB and the (cached, soft-failing) network
// services. Everything degrades gracefully: with no MBIDs and no Last.fm key the
// recommendations fall back to LB Radio prompts + local affinity, so discovery
// still works on a cold, keyless device.

const INDEX_COLS =
  'id, title, artist, mb_recording_id, play_count, last_played, rating, liked'

function loadIndexTracks(): IndexableTrack[] {
  return getDb().prepare(`SELECT ${INDEX_COLS} FROM tracks`).all() as IndexableTrack[]
}

function tracksByIds(ids: number[]): Track[] {
  if (ids.length === 0) return []
  const db = getDb()
  const byId = new Map<number, Track>()
  for (let i = 0; i < ids.length; i += 500) {
    const chunk = ids.slice(i, i + 500)
    const rows = db
      .prepare(`SELECT * FROM tracks WHERE id IN (${chunk.map(() => '?').join(',')})`)
      .all(...chunk) as Track[]
    for (const r of rows) byId.set(r.id, r)
  }
  // preserve ranking order
  return ids.map((id) => byId.get(id)).filter((t): t is Track => t != null)
}

function materialize(result: RecoResult, index: LibraryIndex, limit: number): RecoResultTracks {
  void index
  return {
    inLibrary: tracksByIds(result.inLibrary).slice(0, limit),
    external: result.external.slice(0, limit)
  }
}

/** Recommendations similar to a single seed track. */
export async function getSimilarTracksForTrack(trackId: number, limit = 40): Promise<RecoResultTracks> {
  const seed = getDb().prepare('SELECT * FROM tracks WHERE id = ?').get(trackId) as Track | undefined
  if (!seed) return { inLibrary: [], external: [] }

  const lists: WeightedList[] = []
  if (seed.mb_recording_id) {
    lists.push({ weight: 1, items: await fetchSimilarRecordings(seed.mb_recording_id) })
  }
  lists.push({ weight: 0.8, items: await fetchSimilarTracks(seed.artist, seed.title) })

  const index = buildLibraryIndex(loadIndexTracks())
  const blended = blendCandidates(lists)
  const result = splitByLibrary(blended, index)
  // never recommend the seed itself
  result.inLibrary = result.inLibrary.filter((id) => id !== trackId)
  return materialize(result, index, limit)
}

/** Build a radio queue from a track / artist / genre seed. */
export async function getRadioSeedTracks(seed: RadioSeed, limit = 50): Promise<RecoResultTracks> {
  if (seed.kind === 'track') {
    const base = await getSimilarTracksForTrack(seed.trackId, limit)
    const seedTrack = tracksByIds([seed.trackId])
    // lead with the seed track itself
    return { inLibrary: [...seedTrack, ...base.inLibrary].slice(0, limit), external: base.external }
  }

  const lists: WeightedList[] = []
  let localWhere = ''
  let localParam = ''
  if (seed.kind === 'artist') {
    lists.push({ weight: 0.6, items: await fetchSimilarArtists(seed.artist) })
    lists.push({ weight: 1, items: await fetchRadioByPrompt(`artist:(${seed.artist})`, 'medium') })
    localWhere = 'WHERE artist = ? OR album_artist = ?'
    localParam = seed.artist
  } else {
    lists.push({ weight: 1, items: await fetchRadioByPrompt(`tag:(${seed.genre})`, 'medium') })
    localWhere = 'WHERE genre = ?'
    localParam = seed.genre
  }

  const index = buildLibraryIndex(loadIndexTracks())
  const blended = blendCandidates(lists)
  const result = splitByLibrary(blended, index)

  // Fold in the user's own matching catalogue (artist's tracks / genre tracks),
  // most-affinity first, so a radio always has owned songs to play immediately.
  const params = seed.kind === 'artist' ? [localParam, localParam] : [localParam]
  const localRows = getDb()
    .prepare(`SELECT ${INDEX_COLS} FROM tracks ${localWhere}`)
    .all(...params) as IndexableTrack[]
  const localRanked = rankByAffinity(localRows, Date.now(), limit).map((t) => t.id)

  const seen = new Set<number>()
  const inLibrary: number[] = []
  for (const id of [...localRanked, ...result.inLibrary]) {
    if (!seen.has(id)) {
      seen.add(id)
      inLibrary.push(id)
    }
  }
  return materialize({ inLibrary, external: result.external }, index, limit)
}

/**
 * Free-text catalogue search (keyless, via Deezer) so the user can find and
 * download ANY song — not just what they already own. Results are flagged
 * `owned` when an identical track is in the library, so the UI can offer "play"
 * vs "download". This is the "play anything" half of being a Spotify substitute.
 */
export async function searchExternalCatalog(term: string): Promise<ExternalRecoTrack[]> {
  const cleaned = term.trim()
  if (!cleaned) return []
  let candidates: { title: string; artist: string; album: string | null; durationMs: number | null; coverUrl: string | null }[]
  try {
    const rows = await deezerSearch(cleaned, '')
    candidates = rows.map((r) => ({
      title: r.title,
      artist: r.artist,
      album: r.album ?? null,
      durationMs: r.durationMs ?? null,
      coverUrl: r.coverUrl ?? null
    }))
  } catch (err) {
    // A network/circuit-breaker failure must NOT masquerade as "no results":
    // the UI shows a retry affordance for this code (see SearchOverlay).
    logWarn('reco', `ricerca catalogo web fallita ("${cleaned}")`, err)
    throw new Error('EXT_SEARCH_FAILED')
  }

  const index = buildLibraryIndex(loadIndexTracks())
  const seen = new Set<string>()
  const out: ExternalRecoTrack[] = []
  for (const c of candidates) {
    if (!c.title || !c.artist) continue
    const key = candidateKey(c.artist, c.title)
    if (seen.has(key)) continue
    seen.add(key)
    out.push({
      title: c.title,
      artist: c.artist,
      mbid: null,
      score: 0,
      sources: ['deezer'],
      coverUrl: c.coverUrl,
      durationMs: c.durationMs,
      owned: index.byName.has(key)
    })
  }
  return out
}

/** Assemble the Spotify-style Home feed from local data + one "made for you" mix. */
export async function buildHomeFeed(): Promise<HomeFeed> {
  const db = getDb()
  const sections: HomeSection[] = []

  const recently = db
    .prepare('SELECT * FROM tracks WHERE last_played IS NOT NULL ORDER BY last_played DESC LIMIT 18')
    .all() as Track[]
  if (recently.length > 0) {
    sections.push({ id: 'recent', kind: 'tracks', titleKey: 'home.recently_played', tracks: recently })
  }

  // Jump back in: highest-affinity owned tracks (frequency + recency + likes).
  const indexTracks = loadIndexTracks()
  const affinityIds = rankByAffinity(indexTracks, Date.now(), 18).map((t) => t.id)
  const jumpBack = tracksByIds(affinityIds)
  if (jumpBack.length > 0) {
    sections.push({ id: 'jump', kind: 'tracks', titleKey: 'home.jump_back_in', tracks: jumpBack })
  }

  // "Made for you" — similar to the user's single most-played-recently track.
  const topSeed = recently[0]
  if (topSeed) {
    try {
      const mix = await getSimilarTracksForTrack(topSeed.id, 30)
      if (mix.inLibrary.length > 0) {
        sections.push({
          id: 'because',
          kind: 'tracks',
          titleKey: 'home.because_you_listened',
          titleArg: topSeed.artist,
          tracks: mix.inLibrary
        })
      }
      if (mix.external.length > 0) {
        sections.push({
          id: 'discover',
          kind: 'external',
          titleKey: 'home.discover_more',
          titleArg: topSeed.artist,
          external: mix.external.slice(0, 20)
        })
      }
    } catch {
      /* discovery is best-effort; a network hiccup just drops these sections */
    }
  }

  const likedCount = (db.prepare('SELECT COUNT(*) AS n FROM tracks WHERE liked = 1').get() as { n: number }).n
  return { sections, likedCount }
}
