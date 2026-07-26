import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import type { Track } from '@shared/types'
import { useLibraryStore } from './useLibraryStore'

function track(id: number, title: string): Track {
  return { id, title } as unknown as Track
}

describe('useLibraryStore.patchTrack', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    useLibraryStore.setState({ tracks: [track(1, 'Uno'), track(2, 'Due'), track(3, 'Tre')] })
  })
  afterEach(() => {
    // Drain any pending flush so module-level buffer state doesn't leak
    // between tests.
    vi.runAllTimers()
    vi.useRealTimers()
  })

  it('replaces a single track in place by id (no full reload)', () => {
    const before = useLibraryStore.getState().tracks
    useLibraryStore.getState().patchTrack(track(2, 'Due (enriched)'))
    // Patches are buffered and folded in on the flush tick.
    expect(useLibraryStore.getState().tracks).toBe(before)
    vi.advanceTimersByTime(50)
    const after = useLibraryStore.getState().tracks
    expect(after.map((t) => t.title)).toEqual(['Uno', 'Due (enriched)', 'Tre'])
    // untouched rows keep their identity (cheap re-render)
    expect(after[0]).toBe(before[0])
    expect(after[2]).toBe(before[2])
  })

  it('coalesces a burst of patches into a single array pass', () => {
    const before = useLibraryStore.getState().tracks
    useLibraryStore.getState().patchTrack(track(1, 'Uno v2'))
    useLibraryStore.getState().patchTrack(track(2, 'Due v2'))
    useLibraryStore.getState().patchTrack(track(2, 'Due v3'))
    vi.advanceTimersByTime(50)
    const after = useLibraryStore.getState().tracks
    expect(after.map((t) => t.title)).toEqual(['Uno v2', 'Due v3', 'Tre'])
    expect(after[2]).toBe(before[2])
  })

  it('is a no-op for a track not currently in the list', () => {
    const before = useLibraryStore.getState().tracks
    useLibraryStore.getState().patchTrack(track(99, 'Ghost'))
    vi.advanceTimersByTime(50)
    expect(useLibraryStore.getState().tracks).toBe(before)
  })
})

describe('useLibraryStore.refreshAllDebounced', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('coalesces a burst of calls into a single refreshAll', () => {
    const refreshAll = vi.fn(async () => {})
    useLibraryStore.setState({ refreshAll })
    const { refreshAllDebounced } = useLibraryStore.getState()
    refreshAllDebounced()
    refreshAllDebounced()
    refreshAllDebounced()
    expect(refreshAll).not.toHaveBeenCalled()
    vi.advanceTimersByTime(500)
    expect(refreshAll).toHaveBeenCalledTimes(1)
  })
})
