import { requireYtDlp } from '../binaries'
import { spawnWithTimeout, type SpawnResult } from '../spawn'
import { spawnLines, type SpawnLinesResult } from './spawnLines'

/**
 * Platform seam for invoking yt-dlp.
 *
 * On desktop, yt-dlp is a real executable: we spawn it (one-shot JSON dump, or a
 * long-running download streaming line-buffered stdout). On Android (bionic) the
 * PyInstaller/glibc binary cannot run, so the nodejs-mobile backend installs an
 * override (see node-backend/ytdlp-shim.ts) that routes the same argv to the
 * youtubedl-android library through the YtDlp Capacitor plugin via reverse-RPC.
 *
 * The override is wired with setYtdlpRunner() at boot (DI, mirroring setEmit /
 * setMainWindow); when null we keep the desktop spawn behaviour unchanged.
 */

export interface YtdlpRunner {
  json(args: string[], opts?: { timeoutMs?: number }): Promise<SpawnResult>
  stream(
    args: string[],
    opts: { signal: AbortSignal; onLine: (line: string, stream: 'stdout' | 'stderr') => void }
  ): Promise<SpawnLinesResult>
}

let override: YtdlpRunner | null = null

/** Install a platform-specific runner (used by the Android backend). */
export function setYtdlpRunner(runner: YtdlpRunner | null): void {
  override = runner
}

/** Run yt-dlp collecting all stdout/stderr (e.g. --dump-single-json). */
export function ytdlpJson(args: string[], opts?: { timeoutMs?: number }): Promise<SpawnResult> {
  if (override) return override.json(args, opts)
  return spawnWithTimeout(requireYtDlp(), args, opts)
}

/** Run yt-dlp streaming line-buffered stdout/stderr to `onLine` (downloads). */
export function ytdlpStream(
  args: string[],
  opts: { signal: AbortSignal; onLine: (line: string, stream: 'stdout' | 'stderr') => void }
): Promise<SpawnLinesResult> {
  if (override) return override.stream(args, opts)
  return spawnLines(requireYtDlp(), args, opts)
}
