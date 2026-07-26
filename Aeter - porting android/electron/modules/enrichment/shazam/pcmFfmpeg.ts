// Default desktop PCM decoder: ffmpeg decodes the segment straight to s16le
// mono 16kHz on stdout (~384KB for 12s). Missing ffmpeg or any decode error
// → null, so the fingerprint provider silently skips (never blocks the
// textual path). On Android node-backend overrides this via setPcmDecoder.

import { execFile } from 'node:child_process'
import { getBinaries } from '../../binaries'
import { logWarn } from '../../logger'
import type { PcmResult } from './pcm'

const DECODE_TIMEOUT_MS = 30_000
const MAX_OUTPUT_BYTES = 8 * 1024 * 1024

export async function decodePcmWithFfmpeg(
  path: string,
  offsetSec: number,
  durationSec: number
): Promise<PcmResult | null> {
  const { ffmpeg } = getBinaries()
  if (!ffmpeg) return null

  const args = [
    '-v', 'error',
    '-ss', String(offsetSec),
    '-t', String(durationSec),
    '-i', path,
    '-ac', '1',
    '-ar', '16000',
    '-f', 's16le',
    'pipe:1'
  ]
  try {
    const stdout = await new Promise<Buffer>((resolve, reject) => {
      execFile(
        ffmpeg,
        args,
        { encoding: 'buffer', timeout: DECODE_TIMEOUT_MS, maxBuffer: MAX_OUTPUT_BYTES },
        (err, out) => (err ? reject(err) : resolve(out as Buffer))
      )
    })
    if (stdout.length < 2) return null
    const samples = new Int16Array(
      stdout.buffer.slice(stdout.byteOffset, stdout.byteOffset + stdout.length - (stdout.length % 2))
    )
    return { samples, sampleRate: 16000, channels: 1 }
  } catch (err) {
    logWarn('enrich', `Decodifica PCM ffmpeg fallita per ${path}`, err)
    return null
  }
}
