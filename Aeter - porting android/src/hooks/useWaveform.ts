import { useEffect, useState } from 'react'
import { mediaUrl } from '@/lib/format'
import { isMobile } from '@/lib/platform'

const PEAK_COUNT = 800
const cache = new Map<number, number[]>()
// In-flight computes, keyed by trackId, so concurrent consumers of the same
// track share one decode and all receive the result (a plain Set would let the
// second consumer bail out and stay null forever).
const pending = new Map<number, Promise<number[] | null>>()

/** On mobile, wait for a frame the main thread has nothing better to do —
 *  decoding a full track for its waveform must never compete with a track
 *  change or a sheet animation. Resolves immediately on desktop. */
function waitForIdle(): Promise<void> {
  if (!isMobile) return Promise.resolve()
  return new Promise((resolve) => {
    if (typeof requestIdleCallback === 'function') {
      requestIdleCallback(() => resolve(), { timeout: 2000 })
    } else {
      setTimeout(resolve, 250)
    }
  })
}

async function computePeaks(trackId: number): Promise<number[] | null> {
  // Backgrounded on mobile: don't burn CPU decoding audio nobody can see. The
  // result isn't cached, so the next visible consumer just retries.
  if (isMobile && document.hidden) return null
  await waitForIdle()
  // Per-decode context, closed in finally: a lingering AudioContext keeps the
  // platform audio stack (and its power state) alive long after the decode.
  const ctx = new AudioContext({ sampleRate: 44100 })
  try {
    const res = await fetch(mediaUrl(trackId))
    const buf = await res.arrayBuffer()
    const audio = await ctx.decodeAudioData(buf)
    const data = audio.getChannelData(0)
    const bucket = Math.max(1, Math.floor(data.length / PEAK_COUNT))
    const peaks: number[] = []
    for (let i = 0; i < PEAK_COUNT; i++) {
      let max = 0
      const start = i * bucket
      const end = Math.min(start + bucket, data.length)
      for (let j = start; j < end; j += 16) {
        const v = Math.abs(data[j])
        if (v > max) max = v
      }
      peaks.push(max)
    }
    const top = Math.max(...peaks, 0.01)
    return peaks.map((p) => p / top)
  } catch {
    return null
  } finally {
    void ctx.close().catch(() => {})
  }
}

/** Cached waveform peaks (0..1) for a track; computes and stores them on first request. */
export function useWaveform(trackId: number | null): number[] | null {
  const [peaks, setPeaks] = useState<number[] | null>(null)

  useEffect(() => {
    let cancelled = false
    setPeaks(null)
    if (trackId == null) return

    const cached = cache.get(trackId)
    if (cached) {
      setPeaks(cached)
      return
    }

    void (async () => {
      const stored = await window.aether.getWaveform(trackId)
      if (stored && stored.length > 0) {
        cache.set(trackId, stored)
        if (!cancelled) setPeaks(stored)
        return
      }
      let compute = pending.get(trackId)
      if (!compute) {
        compute = computePeaks(trackId)
          .then((computed) => {
            if (computed) {
              cache.set(trackId, computed)
              void window.aether.saveWaveform(trackId, computed)
            }
            return computed
          })
          .finally(() => {
            pending.delete(trackId)
          })
        pending.set(trackId, compute)
      }
      const computed = await compute
      if (computed && !cancelled) setPeaks(computed)
    })()

    return () => {
      cancelled = true
    }
  }, [trackId])

  return peaks
}
