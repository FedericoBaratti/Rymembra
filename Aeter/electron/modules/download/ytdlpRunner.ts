import { requireYtDlp } from '../binaries'
import { spawnWithTimeout, type SpawnResult } from '../spawn'
import { spawnLines, type SpawnLinesResult } from './spawnLines'

/**
 * Platform seam for invoking yt-dlp.
 *
 * On desktop, yt-dlp is a real executable: we spawn it (one-shot JSON dump, or a
 * long-running download streaming line-buffered stdout). The override hook exists
 * only so the Android backend can route the same argv elsewhere; on desktop it
 * stays null and the real spawn path is used.
 */

export interface YtdlpRunner {
  json(args: string[], opts?: { timeoutMs?: number }): Promise<SpawnResult>
  stream(
    args: string[],
    opts: { signal: AbortSignal; onLine: (line: string, stream: 'stdout' | 'stderr') => void }
  ): Promise<SpawnLinesResult>
}

let override: YtdlpRunner | null = null

/** Install a platform-specific runner (used by the Android backend; unused on desktop). */
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
