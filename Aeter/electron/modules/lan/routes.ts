import type { IncomingMessage, ServerResponse } from 'node:http'
import type { TrackQuery } from '@shared/types'
import {
  queryTracks,
  getTrackCount,
  getTrackById,
  getTracksByIds,
  getAlbums,
  getAlbumTracks,
  getArtists,
  getArtistAlbums,
  getLibraryStats,
  searchLibrary,
  getPlaylists,
  getPlaylistTracks,
  recordPlay,
  setRating,
  setLiked,
  getLikedTracks
} from '../libraryQueries'
import { getLyrics } from '../metadata'
import { sendJson, readJsonBody } from './httpJson'

function parseTrackQuery(url: URL): TrackQuery {
  const q: TrackQuery = {}
  const sortBy = url.searchParams.get('sortBy')
  if (sortBy) q.sortBy = sortBy as TrackQuery['sortBy']
  const sortDir = url.searchParams.get('sortDir')
  if (sortDir === 'asc' || sortDir === 'desc') q.sortDir = sortDir
  if (url.searchParams.has('albumId')) q.albumId = Number(url.searchParams.get('albumId'))
  const artistName = url.searchParams.get('artistName')
  if (artistName) q.artistName = artistName
  if (url.searchParams.has('limit')) q.limit = Number(url.searchParams.get('limit'))
  if (url.searchParams.has('offset')) q.offset = Number(url.searchParams.get('offset'))
  return q
}

/**
 * REST API for the LAN thin client: read-heavy library browse/search, plus the
 * three stats-writing endpoints (play/liked/rating) a phone needs so its
 * listening counts toward the desktop library. `segments` has the leading
 * `api` path component already stripped. Returns false if no route matched.
 */
export async function handleApiRoute(
  req: IncomingMessage,
  res: ServerResponse,
  url: URL,
  segments: string[]
): Promise<boolean> {
  const method = req.method ?? 'GET'

  if (method === 'GET' && segments.length === 1 && segments[0] === 'tracks') {
    sendJson(res, 200, queryTracks(parseTrackQuery(url)))
    return true
  }
  if (method === 'GET' && segments.length === 2 && segments[0] === 'tracks' && segments[1] === 'count') {
    sendJson(res, 200, getTrackCount())
    return true
  }
  // Literal segment: must be matched before the `tracks/:id` catch-all below.
  if (method === 'GET' && segments.length === 2 && segments[0] === 'tracks' && segments[1] === 'liked') {
    sendJson(res, 200, getLikedTracks())
    return true
  }
  if (method === 'GET' && segments.length === 3 && segments[0] === 'tracks' && segments[2] === 'lyrics') {
    sendJson(res, 200, await getLyrics(Number(segments[1])))
    return true
  }
  if (method === 'POST' && segments.length === 2 && segments[0] === 'tracks' && segments[1] === 'byIds') {
    const body = await readJsonBody<{ ids?: number[] }>(req)
    sendJson(res, 200, getTracksByIds(Array.isArray(body?.ids) ? body.ids : []))
    return true
  }
  if (method === 'POST' && segments.length === 3 && segments[0] === 'tracks' && segments[2] === 'play') {
    const body = await readJsonBody<{ msPlayed?: number }>(req)
    recordPlay(Number(segments[1]), body?.msPlayed)
    sendJson(res, 200, { ok: true })
    return true
  }
  if (method === 'PUT' && segments.length === 3 && segments[0] === 'tracks' && segments[2] === 'liked') {
    const body = await readJsonBody<{ liked?: boolean }>(req)
    sendJson(res, 200, setLiked(Number(segments[1]), !!body?.liked))
    return true
  }
  if (method === 'PUT' && segments.length === 3 && segments[0] === 'tracks' && segments[2] === 'rating') {
    const body = await readJsonBody<{ rating?: number }>(req)
    setRating(Number(segments[1]), Number(body?.rating ?? 0))
    sendJson(res, 200, { ok: true })
    return true
  }
  if (method === 'GET' && segments.length === 2 && segments[0] === 'tracks') {
    const track = getTrackById(Number(segments[1]))
    if (!track) {
      sendJson(res, 404, { error: 'not found' })
      return true
    }
    sendJson(res, 200, track)
    return true
  }

  if (method === 'GET' && segments.length === 1 && segments[0] === 'albums') {
    sendJson(res, 200, getAlbums())
    return true
  }
  if (method === 'GET' && segments.length === 3 && segments[0] === 'albums' && segments[2] === 'tracks') {
    sendJson(res, 200, getAlbumTracks(Number(segments[1])))
    return true
  }

  if (method === 'GET' && segments.length === 1 && segments[0] === 'artists') {
    sendJson(res, 200, getArtists())
    return true
  }
  if (method === 'GET' && segments.length === 3 && segments[0] === 'artists' && segments[2] === 'albums') {
    sendJson(res, 200, getArtistAlbums(decodeURIComponent(segments[1])))
    return true
  }

  if (method === 'GET' && segments.length === 1 && segments[0] === 'stats') {
    sendJson(res, 200, getLibraryStats())
    return true
  }

  if (method === 'GET' && segments.length === 1 && segments[0] === 'search') {
    sendJson(res, 200, searchLibrary(url.searchParams.get('q') ?? ''))
    return true
  }

  if (method === 'GET' && segments.length === 1 && segments[0] === 'playlists') {
    sendJson(res, 200, getPlaylists())
    return true
  }
  if (method === 'GET' && segments.length === 3 && segments[0] === 'playlists' && segments[2] === 'tracks') {
    sendJson(res, 200, getPlaylistTracks(Number(segments[1])))
    return true
  }

  return false
}
