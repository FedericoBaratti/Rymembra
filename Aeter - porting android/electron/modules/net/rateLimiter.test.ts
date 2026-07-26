import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { RateLimiter } from './rateLimiter'

describe('RateLimiter', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('runs the first call immediately', async () => {
    const rl = new RateLimiter({ name: 't', minIntervalMs: 1_000 })
    const result = await rl.schedule(async () => 'a')
    expect(result).toBe('a')
  })

  it('spaces consecutive starts by minIntervalMs', async () => {
    const rl = new RateLimiter({ name: 't', minIntervalMs: 1_000 })
    const starts: number[] = []
    const task = (): Promise<void> => {
      starts.push(Date.now())
      return Promise.resolve()
    }
    const all = Promise.all([rl.schedule(task), rl.schedule(task), rl.schedule(task)])
    await vi.runAllTimersAsync()
    await all
    expect(starts[1] - starts[0]).toBeGreaterThanOrEqual(1_000)
    expect(starts[2] - starts[1]).toBeGreaterThanOrEqual(1_000)
  })

  it('preserves FIFO order', async () => {
    const rl = new RateLimiter({ name: 't', minIntervalMs: 10 })
    const order: number[] = []
    const all = Promise.all(
      [1, 2, 3, 4].map((i) =>
        rl.schedule(async () => {
          order.push(i)
        })
      )
    )
    await vi.runAllTimersAsync()
    await all
    expect(order).toEqual([1, 2, 3, 4])
  })

  it('bounds concurrency with maxConcurrent', async () => {
    const rl = new RateLimiter({ name: 't', minIntervalMs: 0, maxConcurrent: 2 })
    let active = 0
    let peak = 0
    const task = (): Promise<void> =>
      new Promise((resolve) => {
        active++
        peak = Math.max(peak, active)
        setTimeout(() => {
          active--
          resolve()
        }, 100)
      })
    const all = Promise.all([rl.schedule(task), rl.schedule(task), rl.schedule(task)])
    await vi.runAllTimersAsync()
    await all
    expect(peak).toBe(2)
  })

  it('notifyRateLimited pushes the next slot out', async () => {
    const rl = new RateLimiter({ name: 't', minIntervalMs: 100 })
    const starts: number[] = []
    await rl.schedule(async () => {
      starts.push(Date.now())
    })
    rl.notifyRateLimited(10_000)
    const second = rl.schedule(async () => {
      starts.push(Date.now())
    })
    await vi.runAllTimersAsync()
    await second
    expect(starts[1] - starts[0]).toBeGreaterThanOrEqual(10_000)
  })

  it('releases the slot when a task throws synchronously', async () => {
    const rl = new RateLimiter({ name: 't', minIntervalMs: 0, maxConcurrent: 1 })
    // A task whose function throws *before* returning a promise must still reject
    // and free its concurrency slot, otherwise the queue stalls forever.
    const boom = rl.schedule((): Promise<never> => {
      throw new Error('sync boom')
    })
    await expect(boom).rejects.toThrow('sync boom')
    // The next task must still be able to run (slot was released).
    const after = rl.schedule(async () => 'after')
    await vi.runAllTimersAsync()
    await expect(after).resolves.toBe('after')
    expect(rl.pending).toBe(0)
  })

  it('tracks pending count', async () => {
    const rl = new RateLimiter({ name: 't', minIntervalMs: 1_000 })
    const all = Promise.all([rl.schedule(async () => {}), rl.schedule(async () => {})])
    expect(rl.pending).toBeGreaterThan(0)
    await vi.runAllTimersAsync()
    await all
    expect(rl.pending).toBe(0)
  })
})
