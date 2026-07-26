import type { DownloadPreview } from '@shared/types'
import { previewSpotify, resolveSpotify } from '../../spotify/keyless'
import type { SourceHandler, DownloadContext, DownloadOutcome } from '../types'
import { detectUrl, type ParsedUrl } from '../urlDetect'
import { downloadSpotifyTrack } from '../spotifyEngine'
import { DownloadError } from '../errors'

// Spotdl-free Spotify handler. Metadata comes from the keyless resolver
// (anonymous Web-Player token → official REST, or embed scraping); each track is
// then matched + downloaded from YouTube via yt-dlp (shared spotifyEngine). This
// is what makes Spotify import work on Android, where spotdl is not shipped.

// Album/playlist tracks download through a small worker pool: YouTube throttles
// per connection, so sequential downloads leave most of the pipe idle. Kept
// modest to stay clear of rate limiting (this multiplies with the queue's own
// item concurrency and yt-dlp's concurrent fragments).
const SPOTIFY_TRACK_CONCURRENCY = 3

export const spotifyHandler: SourceHandler = {
  id: 'spotify',

  detect(url: string): ParsedUrl | null {
    const parsed = detectUrl(url)
    return parsed?.spotifyKind ? parsed : null
  },

  async preview(url: string, parsed: ParsedUrl): Promise<DownloadPreview> {
    const info = await previewSpotify(parsed)
    const tracks = Math.max(1, info.totalTracks)
    return {
      source_url: url,
      source_type: parsed.type,
      title: info.title,
      artist: info.artist,
      album: parsed.spotifyKind === 'album' ? info.title : null,
      cover_url: info.coverUrl,
      total_tracks: info.totalTracks,
      duration: info.durationMs ? info.durationMs / 1000 : null,
      estimated_size_mb: Math.round(tracks * 8.5)
    }
  },

  async download(ctx: DownloadContext): Promise<DownloadOutcome> {
    const parsed = detectUrl(ctx.item.source_url)
    if (!parsed?.spotifyKind) throw new DownloadError('DL_INVALID_URL', 'permanent')

    const resolved = await resolveSpotify(parsed)
    const total = resolved.tracks.length
    if (total === 0) throw new DownloadError('DL_NO_RESULTS', 'permanent')

    const files: string[] = []
    let notFound = 0
    let completed = 0
    let cursor = 0
    // fraction of each in-flight track, keyed by its playlist index
    const activeFractions = new Map<number, number>()

    const reportProgress = (): void => {
      let inFlight = 0
      for (const f of activeFractions.values()) inFlight += f
      ctx.onProgress({ progress: (completed + inFlight) / total })
    }

    // Shared-cursor worker: each grabs the next index, so the original track
    // order (→ track_number in tags) is preserved regardless of who downloads it.
    const worker = async (): Promise<void> => {
      while (!ctx.signal.aborted) {
        const i = cursor++
        if (i >= resolved.tracks.length) return
        const track = resolved.tracks[i]
        ctx.onProgress({ current_file: track.title, total_tracks: total })
        let res: Awaited<ReturnType<typeof downloadSpotifyTrack>>
        try {
          res = await downloadSpotifyTrack(track, i, {
            settings: ctx.settings,
            signal: ctx.signal,
            onFraction: (f) => {
              activeFractions.set(i, f)
              reportProgress()
            }
          })
        } catch {
          // a single track's tooling error must not kill the whole batch
          res = { ok: false, reason: 'failed' }
        }
        activeFractions.delete(i)
        if (res.ok) files.push(res.path)
        else notFound++
        completed++
        ctx.onProgress({ completed_tracks: completed })
        reportProgress()
      }
    }

    const workers: Promise<void>[] = []
    for (let w = 0; w < Math.min(SPOTIFY_TRACK_CONCURRENCY, total); w++) {
      workers.push(worker())
    }
    await Promise.all(workers)

    return { files, partialFailures: notFound }
  }
}
