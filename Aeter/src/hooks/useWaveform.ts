import { useEffect, useState } from 'react'
import { mediaUrl } from '@/lib/format'

const PEAK_COUNT = 800
const cache = new Map<number, number[]>()
let decodeCtx: AudioContext | null = null
// In-flight computes, keyed by trackId, so concurrent consumers of the same
// track share one decode and all receive the result (a plain Set would let the
// second consumer bail out and stay null forever).
const pending = new Map<number, Promise<number[] | null>>()

async function computePeaks(trackId: number): Promise<number[] | null> {
  try {
    const res = await fetch(mediaUrl(trackId))
    const buf = await res.arrayBuffer()
    decodeCtx ??= new AudioContext({ sampleRate: 44100 })
    const audio = await decodeCtx.decodeAudioData(buf)
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
