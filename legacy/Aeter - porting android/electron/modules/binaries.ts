import { app } from 'electron'
import { join } from 'node:path'
import { existsSync, chmodSync } from 'node:fs'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)
const isWin = process.platform === 'win32'
// nodejs-mobile reports 'android'. There, binaries are shipped as lib*.so in
// the app's nativeLibraryDir (the only place Android 14+/16 allows executing
// files), and that dir is injected by the NodeBackend plugin via env var.
const isAndroid = process.platform === 'android'

function binDir(): string {
  if (isAndroid) return process.env['AETHER_NATIVE_LIB_DIR'] ?? ''
  return app.isPackaged
    ? join((process as unknown as { resourcesPath: string }).resourcesPath, 'bin')
    : join(app.getAppPath(), 'resources', 'bin')
}

function binPath(name: string): string {
  if (isAndroid) {
    // jniLibs entries must match lib<name>.so with no dashes (e.g. libytdlp.so).
    return join(binDir(), `lib${name.replace(/-/g, '')}.so`)
  }
  return join(binDir(), isWin ? `${name}.exe` : name)
}

export interface BinaryPaths {
  ytdlp: string | null
  ffmpeg: string | null
  spotdl: string | null
  fpcalc: string | null
}

export function getBinaries(): BinaryPaths {
  const resolve = (name: string): string | null => {
    const p = binPath(name)
    if (!existsSync(p)) return null
    if (!isWin) {
      try {
        chmodSync(p, 0o755)
      } catch {
        // already executable or read-only resource dir
      }
    }
    return p
  }
  return {
    ytdlp: resolve('yt-dlp'),
    ffmpeg: resolve('ffmpeg'),
    spotdl: resolve('spotdl'),
    fpcalc: resolve('fpcalc')
  }
}

export type BinaryName = 'yt-dlp' | 'ffmpeg' | 'spotdl' | 'fpcalc'

/** Per-binary availability for the renderer (banners, settings status). */
export function getBinaryStatus(): Record<BinaryName, { found: boolean; dir: string }> {
  const bins = getBinaries()
  const dir = binDir()
  return {
    'yt-dlp': { found: bins.ytdlp != null, dir },
    ffmpeg: { found: bins.ffmpeg != null, dir },
    spotdl: { found: bins.spotdl != null, dir },
    fpcalc: { found: bins.fpcalc != null, dir }
  }
}

/** Stable code crossing IPC; the renderer translates it (see ipcError.ts). */
export function binaryMissingError(name: BinaryName): Error {
  return new Error(`BINARY_MISSING:${name}:${binDir()}`)
}

export function requireYtDlp(): string {
  const { ytdlp } = getBinaries()
  if (!ytdlp) throw binaryMissingError('yt-dlp')
  return ytdlp
}

/** Self-update yt-dlp via its built-in updater, then report the version. */
export async function updateYtDlp(): Promise<{ updated: boolean; version: string }> {
  const ytdlp = requireYtDlp()
  let updated = false
  try {
    const { stdout } = await execFileAsync(ytdlp, ['-U'], { timeout: 120_000 })
    updated = !/is up to date/i.test(stdout)
  } catch {
    // update can fail (no write permission); still report current version
  }
  const { stdout: version } = await execFileAsync(ytdlp, ['--version'], { timeout: 30_000 })
  return { updated, version: version.trim() }
}
