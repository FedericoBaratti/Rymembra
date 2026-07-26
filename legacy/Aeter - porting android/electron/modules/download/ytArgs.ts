// Shared yt-dlp arguments for YouTube robustness.
//
// Pure module (no electron imports) so it is safe to bundle into the
// nodejs-mobile backend. ICU-free / Node 12-safe: no \p{} regex, no String.at /
// replaceAll (see memory #11/#2).
//
// Why this exists: on Android yt-dlp runs through youtubedl-android. YouTube now
// rejects the default client streams with "HTTP Error 403: Forbidden" unless we
// (a) pick player clients that hand back formats without a GVS PO token and
// (b) tolerate formats that lack a PO token instead of letting yt-dlp filter
// them all out. These args are shared by search, preview and download so the
// client that matches a video is the same one that downloads it.

/** Player-client selection + PO-token tolerance. Primary, most-compatible set. */
export const YT_EXTRACTOR_ARGS: readonly string[] = [
  '--extractor-args',
  'youtube:player_client=default,tv,web_safari;formats=missing_pot'
]

/**
 * Network hardening shared by every yt-dlp invocation. --force-ipv4 avoids a
 * class of 403/throttle responses tied to IPv6; the retry counts absorb the
 * transient 403s YouTube sprays under SABR streaming.
 */
export const YT_NET_ARGS: readonly string[] = [
  '--force-ipv4',
  '--retries', '5',
  '--fragment-retries', '10',
  '--extractor-retries', '3'
]

/**
 * Download-only throughput args (NOT in YT_NET_ARGS: search/preview don't
 * transfer media). YouTube throttles per-connection, so the way to fill the
 * pipe is parallelism: 6 concurrent fragments, and chunked requests (10M) so
 * even single-fragment formats download over parallel range requests. Audio
 * quality is untouched — these only change how bytes are fetched.
 */
export const YT_SPEED_ARGS: readonly string[] = [
  '--concurrent-fragments', '6',
  '--http-chunk-size', '10M'
]

/**
 * One download attempt: a player-client/PO-token profile plus the --format
 * selector. Downloads walk these in order, retrying the next profile only when
 * the failure looks like a retryable 403/signature problem (see
 * isRetryableDownloadError). Separate audio streams take more 403s under SABR,
 * so later attempts fall back to combined formats which are more 403-resistant.
 */
export interface YtDownloadAttempt {
  readonly extractorArgs: readonly string[]
  readonly format: string
}

export const YT_DOWNLOAD_ATTEMPTS: readonly YtDownloadAttempt[] = [
  {
    extractorArgs: ['--extractor-args', 'youtube:player_client=default,tv,web_safari;formats=missing_pot'],
    format: 'bestaudio[ext=m4a]/bestaudio/best'
  },
  {
    extractorArgs: ['--extractor-args', 'youtube:player_client=tv;formats=missing_pot'],
    format: 'bestaudio/best'
  },
  {
    extractorArgs: ['--extractor-args', 'youtube:player_client=web_safari'],
    format: 'best'
  }
]

/**
 * True when a failed download should be retried with the next attempt profile:
 * a 403/Forbidden, or a signature/format failure that a different client may
 * resolve. Distinguishes a real "no audio reachable" from "no match found".
 */
export function isRetryableDownloadError(stderr: string): boolean {
  return /HTTP Error 403|403:?\s*Forbidden|nsig extraction failed|signature extraction failed|Requested format is not available|player_client/i.test(
    stderr
  )
}

/**
 * yt-dlp path routing: final files under `homeDir`, ALL intermediates (.part
 * fragments, the pre-conversion container, ffmpeg output mid-write) under
 * `tmpDir`. Keeps half-written files out of the watched library folder, so the
 * scan/watcher can never ingest a truncated (unplayable) track.
 * NOTE: yt-dlp IGNORES -P when --output is an absolute path — every output
 * template passed alongside these args MUST be relative.
 */
export function ytPathArgs(homeDir: string, tmpDir: string): string[] {
  return ['-P', 'home:' + homeDir, '-P', 'temp:' + tmpDir]
}

/** Inputs for one full download argv (see buildYtDownloadArgs). */
export interface YtDownloadArgvInput {
  readonly format: string
  readonly extractorArgs: readonly string[]
  readonly qualityArgs: readonly string[]
  /** RELATIVE output template (see ytPathArgs). */
  readonly outTpl: string
  readonly homeDir: string
  readonly tmpDir: string
  readonly ffmpegLocation?: string | null
  readonly noPlaylist: boolean
  readonly url: string
}

/**
 * Complete yt-dlp argv for a YouTube video/playlist download: audio extraction,
 * embedded metadata/thumbnail, the AETHER_* progress sentinels parsed by
 * progress.ts, and home/temp path routing.
 */
export function buildYtDownloadArgs(i: YtDownloadArgvInput): string[] {
  const args = [
    '--format', i.format,
    '--extract-audio',
    ...i.qualityArgs,
    ...i.extractorArgs,
    ...YT_NET_ARGS,
    ...YT_SPEED_ARGS,
    '--embed-metadata',
    '--embed-thumbnail',
    '--newline',
    '--ignore-errors',
    '--continue',
    // sentinel-based progress: robust to localized/reformatted output
    '--no-quiet',
    '--progress-template',
    'download:AETHER_P:%(progress.downloaded_bytes|NA)s/%(progress.total_bytes,progress.total_bytes_estimate|NA)s',
    '--print', 'before_dl:AETHER_F:%(playlist_index|NA)s/%(n_entries|NA)s:%(title)s',
    '--print', 'after_move:AETHER_D:%(filepath)s',
    '--output', i.outTpl,
    ...ytPathArgs(i.homeDir, i.tmpDir)
  ]
  if (i.ffmpegLocation) args.push('--ffmpeg-location', i.ffmpegLocation)
  if (i.noPlaylist) args.push('--no-playlist')
  args.push(i.url)
  return args
}
