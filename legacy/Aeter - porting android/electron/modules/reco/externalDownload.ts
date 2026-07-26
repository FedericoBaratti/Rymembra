import type { DownloadItem, DownloadPreview } from '@shared/types'
import { ytdlpJson } from '../download/ytdlpRunner'
import { YT_EXTRACTOR_ARGS, YT_NET_ARGS } from '../download/ytArgs'
import { pickBestCandidate, type Candidate } from '../download/spotifyMatch'
import { startDownload } from '../downloader'
import { DownloadError, friendlyYtError, classifyDownloadFailure } from '../download/errors'
import { SpawnTimeoutError } from '../spawn'
import { logWarn } from '../logger'

// Turns an external recommendation (artist + title, no URL) into a real download.
// It resolves the best YouTube match (same ytsearch + duration-pick the Spotify
// migration uses) and funnels it through the EXISTING download queue, so the
// progress bar, foreground-service notification and library upsert are all reused.

export interface ExternalTrackMeta {
  artist: string
  title: string
  durationMs?: number | null
  /** Remote cover from the catalogue result (Deezer), shown in the queue row. */
  coverUrl?: string | null
}

const SEARCH_TIMEOUT_MS = 60_000

interface ResolvedMatch {
  url: string
  durationSec: number | null
}

/**
 * ytsearch for the best YouTube candidate. Returns null ONLY when the search
 * succeeded but produced no usable match; every real failure (timeout, yt-dlp
 * error, bad JSON) throws a coded DownloadError so callers/UI can tell
 * "not found" apart from "search broke".
 */
async function resolveYoutubeUrl(meta: ExternalTrackMeta): Promise<ResolvedMatch | null> {
  const query = `${meta.artist} ${meta.title}`.replace(/\s+/g, ' ').trim()
  if (!query) return null
  let result
  try {
    result = await ytdlpJson(
      [
        '--dump-single-json',
        '--flat-playlist',
        '--no-warnings',
        '--no-check-certificates',
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
    throw new DownloadError(
      friendlyYtError(result.stderr),
      classifyDownloadFailure(result.stderr)
    )
  }
  let json: { entries?: { id?: string; url?: string; title?: string; duration?: number | null }[] }
  try {
    json = JSON.parse(result.stdout)
  } catch {
    throw new DownloadError('DL_YTDLP_BAD_RESPONSE', 'transient')
  }
  const candidates: Candidate[] = (json.entries ?? [])
    .map((e) => ({
      url: e.url ?? (e.id ? `https://www.youtube.com/watch?v=${e.id}` : ''),
      durationSec: e.duration ?? null,
      title: e.title ?? ''
    }))
    .filter((c) => c.url)
  const best = pickBestCandidate(candidates, meta.durationMs ?? null)
  return best ? { url: best.url, durationSec: best.durationSec ?? null } : null
}

/**
 * Resolve an external recommendation to a YouTube video and enqueue it in the
 * existing download queue. Returns the queued DownloadItem, or null when the
 * search worked but found NO match. Real failures (network, 403, timeout, …)
 * are logged and rethrown with a stable DL_* code so the renderer can show the
 * actual cause (see src/lib/ipcError.ts) instead of a generic "not found".
 */
export async function downloadExternalTrack(meta: ExternalTrackMeta): Promise<DownloadItem | null> {
  try {
    const match = await resolveYoutubeUrl(meta)
    if (!match) return null
    // Build the preview locally: the display fields come from the catalogue
    // result anyway, so a second yt-dlp JSON dump would only add another 30s
    // Python spawn and another chance to hit a 403 before the queue even starts.
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
    logWarn('reco', `download esterno fallito (${meta.artist} — ${meta.title})`, err)
    throw err
  }
}
