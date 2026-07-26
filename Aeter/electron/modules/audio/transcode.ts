import { spawn } from 'node:child_process'
import { unlink } from 'node:fs/promises'
import type { DownloadQuality } from '@shared/types'
import { getBinaries, binaryMissingError } from '../binaries'
import { killTree } from '../spawn'
import { validateAudioFile } from '../download/validate'
import { ffmpegEncodeArgs } from './transcodePlan'

export { qualityArgs, decideTranscode, codecFamily, targetExtFor } from './transcodePlan'
export type { TranscodePlan, CodecFamily } from './transcodePlan'

const TRANSCODE_TIMEOUT_MS = 10 * 60 * 1000

/**
 * Re-encodes `src` into `dest` at the standard target quality with the bundled
 * ffmpeg. Tags are carried over (`-map_metadata 0`); embedded pictures are NOT
 * (`-vn` keeps the output predictable across containers) — callers that must
 * preserve a cover re-embed it via writeTags afterwards. The output is
 * integrity-checked; on any failure the partial `dest` is removed.
 */
export async function transcodeFile(
  src: string,
  dest: string,
  target: DownloadQuality,
  signal?: AbortSignal
): Promise<void> {
  const { ffmpeg } = getBinaries()
  if (!ffmpeg) throw binaryMissingError('ffmpeg')
  if (signal?.aborted) throw new Error('TRANSCODE_ABORTED')

  const args = ['-y', '-nostdin', '-i', src, '-vn', '-map_metadata', '0', ...ffmpegEncodeArgs(target), dest]

  try {
    await new Promise<void>((resolve, reject) => {
      const child = spawn(ffmpeg, args)
      let stderr = ''
      let settled = false

      const finish = (err?: Error): void => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        signal?.removeEventListener('abort', onAbort)
        if (err) reject(err)
        else resolve()
      }
      const onAbort = (): void => {
        if (child.pid) killTree(child.pid)
        finish(new Error('TRANSCODE_ABORTED'))
      }
      const timer = setTimeout(() => {
        if (child.pid) killTree(child.pid)
        finish(new Error('TRANSCODE_TIMEOUT'))
      }, TRANSCODE_TIMEOUT_MS)

      signal?.addEventListener('abort', onAbort, { once: true })
      child.stderr.on('data', (d) => {
        stderr = (stderr + String(d)).slice(-2000)
      })
      child.on('error', (e) => finish(e))
      child.on('close', (code) => {
        if (code === 0) finish()
        else finish(new Error(`TRANSCODE_FAILED:${code}:${stderr.slice(-300)}`))
      })
    })

    const check = await validateAudioFile(dest)
    if (!check.ok) throw new Error(`TRANSCODE_INVALID_OUTPUT:${check.reason}`)
  } catch (err) {
    await unlink(dest).catch(() => {})
    throw err
  }
}
