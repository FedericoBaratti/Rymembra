import type { DownloadPreview } from '@shared/types'
import { getBinaries } from '../../binaries'
import { SpawnTimeoutError } from '../../spawn'
import type { SourceHandler, DownloadContext, DownloadOutcome } from '../types'
import { detectUrl, type ParsedUrl } from '../urlDetect'
import { parseYtdlpLine } from '../progress'
import { friendlyYtError, classifyDownloadFailure, DownloadError } from '../errors'
import { ytdlpJson, ytdlpStream } from '../ytdlpRunner'
import { cleanYoutubeArtist, cleanYoutubeMetadata } from '../youtubeClean'
import {
  YT_EXTRACTOR_ARGS,
  YT_NET_ARGS,
  YT_DOWNLOAD_ATTEMPTS,
  isRetryableDownloadError,
  buildYtDownloadArgs,
  type YtDownloadAttempt
} from '../ytArgs'
import { ytdlpTempDir } from '../tempDir'
import { logWarn } from '../../logger'
import { qualityArgs } from '../../audio/transcodePlan'

interface YtJson {
  title?: string
  uploader?: string
  channel?: string
  artist?: string
  album?: string
  thumbnail?: string
  thumbnails?: { url: string }[]
  duration?: number
  entries?: unknown[]
  playlist_count?: number
}

async function ytDumpJson(url: string, extraArgs: string[] = []): Promise<YtJson> {
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
        ...extraArgs,
        url
      ],
      { timeoutMs: 30_000 }
    )
  } catch (err) {
    if (err instanceof SpawnTimeoutError) {
      throw new Error('DL_YTDLP_TIMEOUT')
    }
    throw err
  }
  if (result.code !== 0) throw new Error(friendlyYtError(result.stderr))
  try {
    return JSON.parse(result.stdout) as YtJson
  } catch {
    throw new Error('DL_YTDLP_BAD_RESPONSE')
  }
}

export const youtubeHandler: SourceHandler = {
  id: 'youtube',

  detect(url: string): ParsedUrl | null {
    const parsed = detectUrl(url)
    return parsed && !parsed.spotifyKind ? parsed : null
  },

  async preview(url: string, parsed: ParsedUrl): Promise<DownloadPreview> {
    // a video URL may still carry a stray list= param: never expand it
    const json = await ytDumpJson(url, parsed.type === 'youtube-video' ? ['--no-playlist'] : [])
    const isPlaylist = Array.isArray(json.entries)
    const count = isPlaylist ? (json.playlist_count ?? json.entries!.length) : 1
    // playlist titles are container names: never split/clean them as song titles
    const rawArtist = json.artist ?? json.uploader ?? json.channel ?? null
    const cleaned = isPlaylist
      ? { title: json.title ?? url, artist: rawArtist ? cleanYoutubeArtist(rawArtist) : null }
      : cleanYoutubeMetadata(json.title ?? url, rawArtist ?? '')
    return {
      source_url: url,
      source_type: parsed.type,
      title: cleaned.title,
      artist: cleaned.artist || null,
      album: json.album ?? null,
      cover_url: json.thumbnail ?? json.thumbnails?.at(-1)?.url ?? null,
      total_tracks: count,
      duration: json.duration ?? null,
      estimated_size_mb: json.duration
        ? Math.round((json.duration * 40) / 1024 / 8) || 1
        : Math.round(count * 8.5)
    }
  },

  async download(ctx: DownloadContext): Promise<DownloadOutcome> {
    const { ffmpeg } = getBinaries()
    // Prefer PLAYLIST-level fields for the directory so every entry of one playlist/album
    // lands in the SAME folder (per-entry album/artist diverge and used to scatter a
    // release across folders → split albums). Single videos have no playlist_* → they
    // fall back to the per-track album/artist exactly as before.
    // RELATIVE template + -P home/temp (ytPathArgs): yt-dlp ignores -P with an
    // absolute -o, and intermediates must never be born inside the watched
    // library folder (a killed conversion would leave an unplayable file there).
    const outTpl =
      '%(playlist_uploader,artist,creator,uploader|Sconosciuto)s/%(playlist_title,album|Singoli)s/%(track_number,playlist_index|00)02d - %(track,title)s.%(ext)s'
    const tmpDir = ytdlpTempDir()

    const buildArgs = (attempt: YtDownloadAttempt): string[] =>
      buildYtDownloadArgs({
        format: attempt.format,
        extractorArgs: attempt.extractorArgs,
        qualityArgs: qualityArgs(ctx.settings.downloadQuality),
        outTpl,
        homeDir: ctx.settings.downloadFolder,
        tmpDir,
        ffmpegLocation: ffmpeg,
        noPlaylist: ctx.item.source_type === 'youtube-video',
        url: ctx.item.source_url
      })

    const files: string[] = []
    let completedTracks = 0
    let totalTracks = Math.max(1, ctx.item.total_tracks)

    const runAttempt = (attempt: YtDownloadAttempt): ReturnType<typeof ytdlpStream> =>
      ytdlpStream(buildArgs(attempt), {
        signal: ctx.signal,
        onLine: (line, stream) => {
          if (stream !== 'stdout') return
          const ev = parseYtdlpLine(line)
          if (!ev) return
          switch (ev.kind) {
            case 'file-start':
              completedTracks = Math.max(completedTracks, ev.index - 1)
              totalTracks = Math.max(totalTracks, ev.total)
              ctx.onProgress({
                completed_tracks: completedTracks,
                total_tracks: totalTracks,
                current_file: ev.title || null
              })
              break
            case 'item':
              completedTracks = Math.max(completedTracks, ev.index - 1)
              totalTracks = Math.max(totalTracks, ev.total)
              ctx.onProgress({ completed_tracks: completedTracks, total_tracks: totalTracks })
              break
            case 'progress':
              if (ev.fraction !== null) {
                ctx.onProgress({ progress: (completedTracks + ev.fraction) / totalTracks })
              }
              break
            case 'legacy-percent':
              ctx.onProgress({ progress: (completedTracks + ev.fraction) / totalTracks })
              break
            case 'file-done':
              files.push(ev.path)
              completedTracks = Math.max(completedTracks, files.length)
              ctx.onProgress({
                completed_tracks: completedTracks,
                progress: completedTracks / totalTracks
              })
              break
            case 'destination':
              ctx.onProgress({ current_file: ev.file })
              break
          }
        }
      })

    // Walk the player-client profiles: on Android YouTube often rejects the
    // primary client with 403/signature errors that a different client resolves,
    // so a retryable failure with zero files moves on to the next profile
    // WITHIN this single queue run (the queue's own retry/backoff stays on top).
    let result = await runAttempt(YT_DOWNLOAD_ATTEMPTS[0])
    for (let i = 1; i < YT_DOWNLOAD_ATTEMPTS.length; i++) {
      if (result.aborted || result.code === 0 || files.length > 0) break
      if (!isRetryableDownloadError(result.stderr)) break
      logWarn(
        'download',
        `yt-dlp fallito con il profilo ${i - 1} (retryable), provo il profilo ${i}`,
        result.stderr.slice(0, 300)
      )
      result = await runAttempt(YT_DOWNLOAD_ATTEMPTS[i])
    }

    if (result.aborted) return { files, partialFailures: 0 }
    if (result.code !== 0 && files.length === 0) {
      throw new DownloadError(
        friendlyYtError(result.stderr),
        classifyDownloadFailure(result.stderr)
      )
    }
    // --ignore-errors can mask per-entry playlist failures: compare what
    // finished against what yt-dlp said it would download
    const partialFailures = files.length > 0 ? Math.max(0, totalTracks - files.length) : 0
    return { files, partialFailures }
  }
}
