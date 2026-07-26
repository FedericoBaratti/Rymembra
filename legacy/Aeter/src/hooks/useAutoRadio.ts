import { useEffect, useRef } from 'react'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useDiscoveryStore } from '@/store/useDiscoveryStore'

// Keeps a radio's queue topped up. When a radio is active and the upcoming
// buffer runs low, fetch tracks similar to whatever is playing now and enqueue
// the ones the user already owns (instant playback, no surprise downloads). The
// native ExoPlayer queue then auto-advances gapless in the background.
const LOW_WATERMARK = 2
const EXTEND_BY = 20

export function useAutoRadio(): void {
  const radioActive = useDiscoveryStore((s) => s.radioActive)
  const currentTrackId = usePlayerStore((s) => s.currentTrack?.id)
  const order = usePlayerStore((s) => s.order)
  const orderPos = usePlayerStore((s) => s.orderPos)

  const busy = useRef(false)
  // last seed we extended from, so we don't refetch repeatedly for one track
  const lastSeed = useRef<number | null>(null)

  const remaining = order.length - 1 - orderPos

  useEffect(() => {
    if (!radioActive || currentTrackId == null) return
    if (remaining > LOW_WATERMARK) return
    if (busy.current || lastSeed.current === currentTrackId) return

    busy.current = true
    lastSeed.current = currentTrackId
    void (async () => {
      try {
        const res = await window.aether.getSimilarTracks(currentTrackId, EXTEND_BY)
        // skip anything already in the queue
        const have = new Set(usePlayerStore.getState().queue.map((t) => t.id))
        const fresh = res.inLibrary.filter((t) => !have.has(t.id))
        if (fresh.length > 0) usePlayerStore.getState().enqueue(fresh)
      } catch {
        /* best-effort; a network hiccup just means the radio doesn't extend now */
      } finally {
        busy.current = false
      }
    })()
  }, [radioActive, currentTrackId, remaining])
}
