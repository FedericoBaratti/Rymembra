import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import type { AppSettings } from '@shared/types'
import { getBinaries } from '../binaries'
import { SpawnTimeoutError } from '../spawn'
import { ytdlpJson, ytdlpStream } from './ytdlpRunner'
import { parseYtdlpLine } from './progress'
import { writeTags } from '../tagIO'
import { logWarn } from '../logger'
import { validateCoverBuffer } from '../coverValidation'
import type { SpotifyTrack } from '../spotify/types'
import {
  type Candidate,
  buildFallbackQuery,
  buildQuery,
  pickBestCandidate,
  sanitizeSegment
} from './spotifyMatch'
import { qualityArgs } from '../audio/transcodePlan'
import {
  YT_DOWNLOAD_ATTEMPTS,
  YT_EXTRACTOR_ARGS,
  YT_NET_ARGS,
  YT_SPEED_ARGS,
  isRetryableDownloadError,
  ytPathArgs
} from './ytArgs'
import { ytdlpTempDir } from './tempDir'
import { classifyDownloadFailure, friendlyYtError } from './errors'

// Spotdl-free engine: turn a Spotify track (title/artist/duration/cover) into a
// local audio file by searching YouTube with yt-dlp and matching on duration,
// then writing Spotify's authoritative tags + cover. Shared by the Downloader
// queue handler (sources/spotify.ts) and the migration orchestrator.

export interface TrackDlContext {
  settings: AppSettings
  signal: AbortSignal
  /** 0..1 progress within this single track's download. */
  onFraction?(fraction: number): void
}

/**
 * Outcome of a single track download. We deliberately distinguish a genuine
 * "no match on YouTube / unavailable" (`not-found`, terminal) from a transient
 * failure (`failed`, search timeout / yt-dlp error / network), so the migration
 * orchestrator can RETRY the transient ones instead of silently skipping them.
 */
export type TrackDownloadResult =
  | { ok: true; path: string }
  | { ok: false; reason: 'not-found' }
  | { ok: false; reason: 'failed'; error?: string }

// A cold yt-dlp/Python start on mobile (plus concurrent load) is far slower than
// on desktop; 30s caused spurious "not found". Give the search room to answer.
const SEARCH_TIMEOUT_MS = 60_000

type SearchOnce = { ok: true; candidates: Candidate[] } | { ok: false }

/**
 * One YouTube search. Returns ok:false ONLY on a transient failure
 * (timeout / non-zero exit / unparseable output); a successful search with zero
 * hits returns ok:true with an empty list, so the caller can tell "search
 * failed, retry later" from "genuinely no results".
 */
async function searchYoutubeOnce(query: string, signal: AbortSignal): Promise<SearchOnce> {
  if (signal.aborted) return { ok: false }
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
    if (err instanceof SpawnTimeoutError) return { ok: false }
    throw err
  }
  if (result.code !== 0) return { ok: false }
  let json: { entries?: { id?: string; url?: string; title?: string; duration?: number | null }[] }
  try {
    json = JSON.parse(result.stdout)
  } catch {
    return { ok: false }
  }
  const candidates = (json.entries ?? [])
    .map((e) => ({
      url: e.url ?? (e.id ? `https://www.youtube.com/watch?v=${e.id}` : ''),
      durationSec: typeof e.duration === 'number' ? e.duration : null,
      title: e.title ?? ''
    }))
    .filter((c) => c.url)
  return { ok: true, candidates }
}

type SearchResult =
  | { ok: true; candidates: Candidate[] }
  | { ok: false; reason: 'failed' }

/**
 * Search YouTube for a Spotify track: the exact query first, then — only if it
 * came back empty — a looser fallback query (decorations stripped). Any transient
 * failure short-circuits to {ok:false, reason:'failed'} so the track is retried,
 * never silently dropped.
 */
async function searchYoutube(track: SpotifyTrack, signal: AbortSignal): Promise<SearchResult> {
  const primaryQuery = buildQuery(track)
  const primary = await searchYoutubeOnce(primaryQuery, signal)
  if (!primary.ok) return { ok: false, reason: 'failed' }
  if (primary.candidates.length > 0) return { ok: true, candidates: primary.candidates }

  const fallbackQuery = buildFallbackQuery(track)
  if (!fallbackQuery || fallbackQuery === primaryQuery) return { ok: true, candidates: [] }
  if (signal.aborted) return { ok: false, reason: 'failed' }
  const second = await searchYoutubeOnce(fallbackQuery, signal)
  if (!second.ok) return { ok: false, reason: 'failed' }
  return { ok: true, candidates: second.candidates }
}

async function fetchCover(url: string | null | undefined): Promise<Buffer | null> {
  if (!url) return null
  try {
    // 30s: cover downloads are small but CDNs can stall; never hang the queue.
    const res = await fetch(url, { signal: AbortSignal.timeout(30_000) })
    if (!res.ok) return null
    const buf = Buffer.from(await res.arrayBuffer())
    // Same gate as every other cover source: an HTML error body or truncated
    // download must not end up embedded in the file tags.
    return validateCoverBuffer(buf) ? buf : null
  } catch {
    return null
  }
}

/**
 * Download one Spotify track via YouTube. See TrackDownloadResult: `not-found`
 * is terminal (no acceptable match / video unavailable), `failed` is transient
 * and should be retried by the caller.
 */
export async function downloadSpotifyTrack(
  track: SpotifyTrack,
  index: number,
  ctx: TrackDlContext
): Promise<TrackDownloadResult> {
  const { ffmpeg } = getBinaries()
  const qArgs = qualityArgs(ctx.settings.downloadQuality)

  const search = await searchYoutube(track, ctx.signal)
  if (ctx.signal.aborted) return { ok: false, reason: 'failed' }
  if (!search.ok) return { ok: false, reason: 'failed', error: 'search failed' }
  const chosen = pickBestCandidate(search.candidates, track.durationMs)
  if (!chosen) return { ok: false, reason: 'not-found' }

  // Use the ALBUM artist (not the per-track artist) for the folder so an album
  // with featured guests stays in ONE folder on disk instead of scattering.
  const artistDir = sanitizeSegment(track.albumArtist ?? track.artist ?? 'Sconosciuto')
  const albumDir = sanitizeSegment(track.album ?? 'Singoli')
  const fileBase = `${String(index + 1).padStart(2, '0')} - ${sanitizeSegment(track.title)}`
  // RELATIVE base + -P home/temp (ytPathArgs): yt-dlp ignores -P with an
  // absolute -o, and intermediates must stay out of the watched library folder.
  const relBase = `${artistDir}/${albumDir}/${fileBase}`
  const tmpDir = ytdlpTempDir()
  mkdirSync(join(ctx.settings.downloadFolder, artistDir, albumDir), { recursive: true })

  // Walk the attempt profiles: a 403/signature failure on the first (preferred,
  // separate-audio) profile retries with a more 403-resistant client/combined
  // format before we give up. This stops a YouTube 403 from being silently
  // reported as "track not found".
  let filePath: string | null = null
  let lastStderr = ''
  for (let attempt = 0; attempt < YT_DOWNLOAD_ATTEMPTS.length; attempt++) {
    if (ctx.signal.aborted) return { ok: false, reason: 'failed' }
    const profile = YT_DOWNLOAD_ATTEMPTS[attempt]

    const args = [
      '--format', profile.format,
      '--extract-audio',
      ...qArgs,
      ...profile.extractorArgs,
      ...YT_NET_ARGS,
      ...YT_SPEED_ARGS,
      '--no-playlist',
      '--newline',
      '--no-quiet',
      '--progress-template', 'download:AETHER_P:%(progress.downloaded_bytes|NA)s/%(progress.total_bytes,progress.total_bytes_estimate|NA)s',
      '--print', 'after_move:AETHER_D:%(filepath)s',
      '--output', `${relBase}.%(ext)s`,
      ...ytPathArgs(ctx.settings.downloadFolder, tmpDir)
    ]
    if (ffmpeg) args.push('--ffmpeg-location', ffmpeg)
    args.push(chosen.url)

    filePath = null
    const result = await ytdlpStream(args, {
      signal: ctx.signal,
      onLine: (line, stream) => {
        if (stream !== 'stdout') return
        const ev = parseYtdlpLine(line)
        if (!ev) return
        // AETHER_D (after_move) carries the full final path; 'destination' is only
        // a basename, so it can't be used as the file path.
        if (ev.kind === 'file-done') filePath = ev.path
        else if ((ev.kind === 'progress' || ev.kind === 'legacy-percent') && ev.fraction !== null) {
          ctx.onFraction?.(ev.fraction)
        }
      }
    })

    if (result.aborted) return { ok: false, reason: 'failed' }
    if (filePath) break
    lastStderr = result.stderr ?? ''

    const moreAttempts = attempt < YT_DOWNLOAD_ATTEMPTS.length - 1
    if (moreAttempts && isRetryableDownloadError(lastStderr)) {
      logWarn(
        'download',
        `Download Spotify 403/retry per "${track.title}" (tentativo ${attempt + 1}/${YT_DOWNLOAD_ATTEMPTS.length})`,
        lastStderr.slice(-300)
      )
      continue
    }
    break
  }

  if (!filePath) {
    // Only an EVIDENCED permanent failure (video removed/private/age-gated, with
    // matching stderr) is a genuine terminal miss. Anything else — 403/network,
    // or an empty/unknown error — is treated as transient so the caller retries
    // it rather than silently dropping the track.
    if (lastStderr.trim() && classifyDownloadFailure(lastStderr) === 'permanent') {
      return { ok: false, reason: 'not-found' }
    }
    return { ok: false, reason: 'failed', error: friendlyYtError(lastStderr) }
  }

  // Authoritative tags from Spotify (more reliable than YouTube's).
  try {
    const cover = await fetchCover(track.coverUrl)
    writeTags(
      filePath,
      {
        title: track.title,
        artist: track.artist ?? undefined,
        album: track.album ?? undefined,
        // album_artist is the key to grouping the album as ONE release (see
        // library.rebuildAggregates). Fall back to the track artist only if
        // Spotify gave us nothing, so the field is never left unset.
        album_artist: track.albumArtist ?? track.artist ?? undefined,
        disc_number: track.discNumber ?? undefined,
        year: track.year ?? undefined,
        // Prefer Spotify's real track number; fall back to download order.
        track_number: track.trackNumber ?? index + 1
      },
      cover
    )
  } catch (err) {
    logWarn('download', `Tagging Spotify fallito per ${filePath}`, err)
  }

  return { ok: true, path: filePath }
}
