import { callNative } from './runtime'
import { SpawnTimeoutError, type SpawnResult } from '../electron/modules/spawn'
import type { SpawnLinesResult } from '../electron/modules/download/spawnLines'
import type { YtdlpRunner } from '../electron/modules/download/ytdlpRunner'

/**
 * Android implementation of the yt-dlp seam (electron/modules/download/ytdlpRunner.ts).
 *
 * The official yt-dlp standalone binary is a PyInstaller/glibc build and cannot
 * exec on Android's bionic libc. Instead we drive the youtubedl-android library
 * (bundled Python + ffmpeg) through the YtDlp Capacitor plugin: callNative emits
 * an `nrpc` the renderer turns into YtDlp.run(args) (see src/lib/nativeRpc.ts),
 * which returns the aggregated process output.
 *
 * Because the plugin call is one-shot (no live stdout), stream() replays the
 * aggregated output line-by-line so the existing marker parsers (AETHER_F /
 * AETHER_D / AETHER_P in download/progress.ts) still fire — the final file path
 * and per-track "done" arrive, only the intra-track progress bar isn't live.
 */

interface RawRun {
  code: number
  stdout: string
  stderr: string
}

// yt-dlp downloads can take minutes; keep the reverse-RPC well above that so a
// real download is never killed by the transport timeout (cancellation is
// handled by the AbortSignal at the call sites).
const DOWNLOAD_TIMEOUT_MS = 30 * 60_000

let processSeq = 0
/** A unique id per native yt-dlp run so cancel() can target this exact process
 *  (youtubedl-android destroyProcessById). Required for real cancellation of an
 *  in-flight download, since the one-shot reverse-RPC can't be interrupted. */
function nextProcessId(): string {
  return `dl-${Date.now()}-${++processSeq}`
}

/**
 * youtubedl-android injects its own `--ffmpeg-location <bundled ffmpeg>` on
 * every execute(). The shared arg builders (electron/modules/download) may add
 * one too, pointing at getBinaries().ffmpeg — on Android that is the split-APK
 * libffmpeg.so, which yt-dlp cannot use ("does not exist! Continuing without
 * ffmpeg" → audio extraction fails). Strip the pair and let the library's own
 * location win.
 */
function stripFfmpegLocation(args: string[]): string[] {
  const out: string[] = []
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--ffmpeg-location') {
      i++ // skip its value too
      continue
    }
    out.push(args[i])
  }
  return out
}

async function run(args: string[], timeoutMs: number, processId?: string): Promise<RawRun> {
  const res = (await callNative(
    'ytdlpRun',
    { args: stripFfmpegLocation(args), processId },
    timeoutMs
  )) as Partial<RawRun>
  return {
    code: typeof res?.code === 'number' ? res.code : 0,
    stdout: res?.stdout ?? '',
    stderr: res?.stderr ?? ''
  }
}

export const androidYtdlpRunner: YtdlpRunner = {
  async json(args: string[], opts?: { timeoutMs?: number }): Promise<SpawnResult> {
    const timeoutMs = opts?.timeoutMs ?? 35_000
    try {
      const res = await run(args, timeoutMs)
      return { code: res.code, stdout: res.stdout, stderr: res.stderr }
    } catch (err) {
      // Map the transport timeout onto SpawnTimeoutError so callers that branch
      // on it (youtube.ts → DL_YTDLP_TIMEOUT, spotifyEngine.ts → []) behave the
      // same as on desktop.
      if (err instanceof Error && /timed out/.test(err.message)) {
        throw new SpawnTimeoutError('yt-dlp', timeoutMs)
      }
      throw err
    }
  },

  async stream(
    args: string[],
    opts: { signal: AbortSignal; onLine: (line: string, stream: 'stdout' | 'stderr') => void }
  ): Promise<SpawnLinesResult> {
    if (opts.signal.aborted) return { code: null, stderr: '', aborted: true }
    // Tie this run to a processId and kill it natively on abort. Without this the
    // one-shot reverse-RPC keeps the youtubedl-android process running until it
    // finishes even after the user cancels (the call site only checks the signal
    // before/after), so cancellation would not actually stop the download.
    const processId = nextProcessId()
    const onAbort = (): void => {
      void callNative('ytdlpCancel', { processId }, 8000).catch(() => {})
    }
    opts.signal.addEventListener('abort', onAbort, { once: true })
    let res: RawRun
    try {
      res = await run(args, DOWNLOAD_TIMEOUT_MS, processId)
    } catch (err) {
      if (opts.signal.aborted) return { code: null, stderr: '', aborted: true }
      throw err
    } finally {
      opts.signal.removeEventListener('abort', onAbort)
    }
    for (const line of res.stdout.split('\n')) opts.onLine(line, 'stdout')
    for (const line of res.stderr.split('\n')) opts.onLine(line, 'stderr')
    return { code: res.code, stderr: res.stderr, aborted: opts.signal.aborted }
  }
}
