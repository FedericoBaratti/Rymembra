import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AdaptiveConcurrencyManager } from './adaptiveConcurrency'

// Pure-logic tests on fresh instances (the exported thermalManager singleton is
// for production wiring). Fake timers drive the stale-fallback timer.

let mgr: AdaptiveConcurrencyManager

beforeEach(() => {
  vi.useFakeTimers()
  mgr = new AdaptiveConcurrencyManager()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('AdaptiveConcurrencyManager', () => {
  it('starts at normal: base concurrency untouched, nothing deferred', () => {
    expect(mgr.getState().level).toBe('normal')
    expect(mgr.getConcurrency(8)).toBe(8)
    expect(mgr.getConcurrency(3)).toBe(3)
    expect(mgr.shouldDefer('auto-enrich')).toBe(false)
  })

  it('warning halves (rounded up), critical serializes', () => {
    mgr.updateState({ level: 'warning' })
    expect(mgr.getConcurrency(8)).toBe(4)
    expect(mgr.getConcurrency(3)).toBe(2)
    expect(mgr.getConcurrency(1)).toBe(1)
    expect(mgr.shouldDefer('auto-enrich')).toBe(false)

    mgr.updateState({ level: 'critical' })
    expect(mgr.getConcurrency(8)).toBe(1)
    expect(mgr.getConcurrency(3)).toBe(1)
    expect(mgr.shouldDefer('auto-enrich')).toBe(true)
  })

  it('stores headroom and stamps the receipt time', () => {
    const now = Date.now()
    const state = mgr.updateState({ level: 'warning', headroom: 0.87 })
    expect(state).toEqual({ level: 'warning', headroom: 0.87, timestamp: now })
    expect(mgr.getState()).toEqual(state)
  })

  it('notifies onChange only on level transitions; unsubscribe works', () => {
    const seen: string[] = []
    const off = mgr.onChange((s) => seen.push(s.level))

    mgr.updateState({ level: 'warning' })
    mgr.updateState({ level: 'warning' }) // heartbeat, same level → no callback
    mgr.updateState({ level: 'critical' })
    expect(seen).toEqual(['warning', 'critical'])

    off()
    mgr.updateState({ level: 'normal' })
    expect(seen).toEqual(['warning', 'critical'])
  })

  it('a throwing listener does not break the others', () => {
    const seen: string[] = []
    mgr.onChange(() => {
      throw new Error('boom')
    })
    mgr.onChange((s) => seen.push(s.level))
    mgr.updateState({ level: 'warning' })
    expect(seen).toEqual(['warning'])
  })

  it('falls back to normal after 120s without fresh samples', () => {
    const seen: string[] = []
    mgr.onChange((s) => seen.push(s.level))
    mgr.updateState({ level: 'critical' })

    vi.advanceTimersByTime(119_999)
    expect(mgr.getState().level).toBe('critical')

    vi.advanceTimersByTime(1)
    expect(mgr.getState().level).toBe('normal')
    expect(mgr.getConcurrency(8)).toBe(8)
    expect(seen).toEqual(['critical', 'normal'])
  })

  it('each sample re-arms the stale timer', () => {
    mgr.updateState({ level: 'critical' })
    vi.advanceTimersByTime(60_000)
    mgr.updateState({ level: 'critical' }) // heartbeat at t=60s
    vi.advanceTimersByTime(60_001) // t=120s+: old deadline passed, new one hasn't
    expect(mgr.getState().level).toBe('critical')
    vi.advanceTimersByTime(60_000) // t=180s+: 120s after the last sample
    expect(mgr.getState().level).toBe('normal')
  })

  it('no stale timer while normal (nothing to fall back from)', () => {
    mgr.updateState({ level: 'normal' })
    expect(vi.getTimerCount()).toBe(0)
    mgr.updateState({ level: 'warning' })
    expect(vi.getTimerCount()).toBe(1)
    mgr.updateState({ level: 'normal' })
    expect(vi.getTimerCount()).toBe(0)
  })
})
