// Per-track fingerprint orchestrator. Samples TWO 12s segments (~25% and ~60%
// of the track, avoiding intro/outro; one centered segment under 30s) and calls
// the keyless Shazam endpoint for each. Corroboration = both segments resolve
// to the SAME track key — a local double-check that kills remix/cover false
// positives without extra providers. EVERY failure (no decoder, breaker open,
// HTTP, schema drift) returns null: the textual path is never blocked.

import { createHash } from 'node:crypto'
import { stat } from 'node:fs/promises'
import { getSettings } from '../../settings'
import { getApiCache } from '../../net/apiCacheSingleton'
import { TTL } from '../../net/apiCache'
import { logWarn } from '../../logger'
import { getPcmDecoder, toMono16k, type PcmDecoder } from './pcm'
import { decodePcmWithFfmpeg } from './pcmFfmpeg'
import { SignatureGenerator } from './signature'
import { shazamRecognize, type ShazamMatch } from '../services/shazam'

const SEGMENT_SECONDS = 12

export interface FingerprintResult extends ShazamMatch {
  /** Both segments recognized the same track key. */
  corroborated: boolean
}

/** Segment start offsets; audio shorter than 30s gets one centered segment. */
export function segmentOffsets(durationSec: number): number[] {
  if (!durationSec || durationSec < 30) {
    return [Math.max(0, (durationSec || SEGMENT_SECONDS) / 2 - SEGMENT_SECONDS / 2)]
  }
  const clamp = (off: number): number => Math.max(0, Math.min(off, durationSec - SEGMENT_SECONDS))
  return [clamp(durationSec * 0.25), clamp(durationSec * 0.6)]
}

/**
 * Fingerprints a local audio file. Results (including misses) are cached in
 * api_cache keyed by path|size|mtime, so a library re-pass costs nothing.
 * Returns null when disabled, unavailable, or unrecognized.
 */
export async function recognizeTrack(
  path: string,
  durationSec: number
): Promise<FingerprintResult | null> {
  try {
    if (getSettings().enrichFingerprint === false) return null
    const st = await stat(path)
    const mtime = Math.floor(st.mtimeMs ?? 0)
    const cacheKey = createHash('sha1').update(`${path}|${st.size}|${mtime}`).digest('hex')
    return await getApiCache().cachedJson<FingerprintResult>({
      service: 'shazam-track',
      key: cacheKey,
      ttlMs: TTL.ACOUSTID,
      missTtlMs: TTL.NEGATIVE,
      fetcher: () => recognizeUncached(path, durationSec)
    })
  } catch (err) {
    logWarn('enrich', `Fingerprint Shazam fallito per ${path}`, err)
    return null
  }
}

async function recognizeUncached(
  path: string,
  durationSec: number
): Promise<FingerprintResult | null> {
  const decoder: PcmDecoder = getPcmDecoder() ?? decodePcmWithFfmpeg

  const matches: ShazamMatch[] = []
  const offsets = segmentOffsets(durationSec)
  for (const offset of offsets) {
    const pcm = await decoder(path, offset, SEGMENT_SECONDS)
    if (!pcm || pcm.samples.length === 0) return null
    const mono = toMono16k(pcm.samples, pcm.sampleRate, pcm.channels)
    if (mono.length < 16000) return null // < 1s of usable audio
    const signature = new SignatureGenerator().getSignature(mono)
    const match = await shazamRecognize(signature.encodeToUri(), signature.samplems())
    if (match) matches.push(match)
  }

  if (matches.length === 0) return null
  const corroborated =
    offsets.length === 2 && matches.length === 2 && matches[0].key === matches[1].key
  return { ...matches[0], corroborated }
}
