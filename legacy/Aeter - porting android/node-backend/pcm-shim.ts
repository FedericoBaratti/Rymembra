/**
 * Android PCM decoder seam (electron/modules/enrichment/shazam/pcm.ts
 * setPcmDecoder). The desktop default spawns ffmpeg; on Android the ffmpeg
 * binary isn't shipped, so the segment is decoded natively via reverse-RPC
 * (callNative('audioDecode') → AudioDecodePlugin.kt, MediaExtractor +
 * MediaCodec) into a temp raw s16le file, read back here. Resampling to mono
 * 16 kHz stays in TS (toMono16k). Any failure → null: the fingerprint
 * provider is skipped, never blocking textual enrichment.
 */
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { readFile, unlink } from 'node:fs/promises'
import { randomBytes } from 'node:crypto'
import { callNative } from './runtime'
import type { PcmResult } from '../electron/modules/enrichment/shazam/pcm'

export async function androidPcmDecoder(
  path: string,
  offsetSec: number,
  durationSec: number
): Promise<PcmResult | null> {
  const id = randomBytes(16).toString('hex')
  const destPath = join(tmpdir(), `pcm_${id}.raw`)
  try {
    // Above the renderer's 30s (nativeRpc.ts RPC_TIMEOUT_MS.audioDecode) so
    // the more informative renderer-side timeout error wins the race.
    const res = (await callNative(
      'audioDecode',
      { srcPath: path, destPath, offsetSec, durationSec },
      40_000
    )) as { sampleRate?: number; channels?: number } | null
    if (!res || !res.sampleRate || !res.channels) return null
    const buf = await readFile(destPath)
    const evenLength = buf.length - (buf.length % 2)
    if (evenLength === 0) return null
    const samples = new Int16Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + evenLength))
    return { samples, sampleRate: res.sampleRate, channels: res.channels }
  } catch (err) {
    // stderr reaches logcat (console.log does not)
    console.error('[pcm-shim] audioDecode fallito:', err)
    return null
  } finally {
    await unlink(destPath).catch(() => {})
  }
}
