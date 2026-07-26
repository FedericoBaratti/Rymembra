import type { DownloadItem, DownloadPreview } from '@shared/types'
import { requireYtDlp } from '../binaries'
import { spawnWithTimeout, SpawnTimeoutError } from '../spawn'
import { startDownload } from '../downloader'
import { DownloadError, friendlyYtError, classifyDownloadFailure } from './errors'
import { buildSearchQuery, buildFallbackQuery, pickBestCandidate, type Candidate } from './matchHelpers'
import { YT_EXTRACTOR_ARGS, YT_NET_ARGS } from './ytArgs'
import { logWarn } from '../logger'

// Turns a bare track (artist + title, no URL) into a real download. It resolves
// the best YouTube match (ytsearch + duration-pick) and funnels it through the
// EXISTING download queue, so the progress bar and library upsert are reused.
// Used by the sync auto-fetch of tracks that exist in the synced metadata but
// have no local audio file on this device.

export interface ExternalTrackMeta {
  artist: string
  title: string
  durationMs?: number | null
  /** Remote cover URL, shown in the queue row (optional). */
  coverUrl?: string | null
}

const SEARCH_TIMEOUT_MS = 60_000

interface ResolvedMatch {
  url: string
  durationSec: number | null
}

interface SearchEntry {
  id?: string
  url?: string
  title?: string
  duration?: number | null
}

/** One ytsearch5 pass for a query; throws a coded DownloadError on real failure. */
async function searchCandidates(query: string): Promise<Candidate[]> {
  if (!query) return []
  let result
  try {
    result = await spawnWithTimeout(
      requireYtDlp(),
      [
        '--dump-single-json',
        '--flat-playlist',
        '--no-warnings',
        '--no-check-certificates',
        // Same bot-detection avoidance the main YouTube download uses: a bare
        // ytsearch increasingly hits "Sign in to confirm you're not a bot" / 403,
        // which made the auto-fetch of missing tracks fail systematically.
        ...YT_EXTRACTOR_ARGS,
        ...YT_NET_ARGS,
        `ytsearch5:${query}`
      ],
      { timeoutMs: SEARCH_TIMEOUT_MS }
    )
  } catch (err) {
    if (err instanceof SpawnTimeoutError) throw new DownloadError('DL_YTDLP_TIMEOUT', 'transient')
    throw err
  }
  if (result.code !== 0) {
    throw new DownloadError(friendlyYtError(result.stderr), classifyDownloadFailure(result.stderr))
  }
  let json: { entries?: SearchEntry[] }
  try {
    json = JSON.parse(result.stdout)
  } catch {
    throw new DownloadError('DL_YTDLP_BAD_RESPONSE', 'transient')
  }
  return (json.entries ?? [])
    .map((e) => ({
      url: e.url ?? (e.id ? `https://www.youtube.com/watch?v=${e.id}` : ''),
      durationSec: e.duration ?? null,
      title: e.title ?? ''
    }))
    .filter((c) => c.url)
}

/**
 * Resolve the best YouTube video for a track. Returns null ONLY when the search
 * succeeded but produced no usable match; every real failure (timeout, yt-dlp
 * error, bad JSON) throws a coded DownloadError so callers can tell "not found"
 * apart from "search broke".
 */
async function resolveYoutubeUrl(meta: ExternalTrackMeta): Promise<ResolvedMatch | null> {
  const targetMs = meta.durationMs ?? null
  let candidates = await searchCandidates(buildSearchQuery(meta.artist, meta.title))
  if (candidates.length === 0) {
    const fallback = buildFallbackQuery(meta.artist, meta.title)
    if (fallback && fallback !== buildSearchQuery(meta.artist, meta.title)) {
      candidates = await searchCandidates(fallback)
    }
  }
  const best = pickBestCandidate(candidates, targetMs)
  return best ? { url: best.url, durationSec: best.durationSec ?? null } : null
}

/**
 * Resolve a bare track to a YouTube video and enqueue it in the existing
 * download queue. Returns the queued DownloadItem, or null when the search
 * worked but found NO match. Real failures are logged and rethrown with a
 * stable DL_* code.
 */
export async function downloadExternalTrack(meta: ExternalTrackMeta): Promise<DownloadItem | null> {
  try {
    const match = await resolveYoutubeUrl(meta)
    if (!match) return null
    const preview: DownloadPreview = {
      source_url: match.url,
      source_type: 'youtube-video',
      title: meta.title,
      artist: meta.artist || null,
      album: null,
      cover_url: meta.coverUrl ?? null,
      total_tracks: 1,
      duration: match.durationSec,
      estimated_size_mb: match.durationSec
        ? Math.round((match.durationSec * 40) / 1024 / 8) || 1
        : null
    }
    return startDownload(preview)
  } catch (err) {
    logWarn('sync', `download brano mancante fallito (${meta.artist} — ${meta.title})`, err)
    throw err
  }
}
