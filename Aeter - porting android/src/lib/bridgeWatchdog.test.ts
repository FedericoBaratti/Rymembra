import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createBridgeWatchdog, type BridgeWatchdog } from './bridgeWatchdog'

describe('createBridgeWatchdog', () => {
  let probe: ReturnType<typeof vi.fn<() => void>>
  let onDead: ReturnType<typeof vi.fn<() => void>>
  let dog: BridgeWatchdog

  const make = (): BridgeWatchdog =>
    createBridgeWatchdog({ stallMs: 12_000, probeTimeoutMs: 10_000, checkEveryMs: 1_000, probe, onDead })

  beforeEach(() => {
    vi.useFakeTimers()
    probe = vi.fn<() => void>()
    onDead = vi.fn<() => void>()
    dog = make()
  })

  afterEach(() => {
    dog.stop()
    vi.useRealTimers()
  })

  it('declares the backend dead when the probe goes unanswered', () => {
    dog.noteSent()
    vi.advanceTimersByTime(13_000)
    expect(probe).toHaveBeenCalledTimes(1)
    expect(onDead).not.toHaveBeenCalled()
    vi.advanceTimersByTime(10_000)
    expect(onDead).toHaveBeenCalledTimes(1)
  })

  it('never kills a slow-but-alive call: any message settles the probe', () => {
    dog.noteSent()
    vi.advanceTimersByTime(13_000)
    expect(probe).toHaveBeenCalledTimes(1)
    dog.noteMessage() // probe reply (or any event) proves liveness
    vi.advanceTimersByTime(9_000)
    expect(onDead).not.toHaveBeenCalled()
    // After another full stall window of silence it probes again — still alive.
    vi.advanceTimersByTime(4_000)
    expect(probe).toHaveBeenCalledTimes(2)
    dog.noteMessage()
    vi.advanceTimersByTime(11_000)
    expect(onDead).not.toHaveBeenCalled()
  })

  it('does not probe while inbound traffic keeps flowing', () => {
    dog.noteSent()
    for (let i = 0; i < 10; i++) {
      vi.advanceTimersByTime(5_000)
      dog.noteMessage()
    }
    expect(probe).not.toHaveBeenCalled()
    expect(onDead).not.toHaveBeenCalled()
  })

  it('noteIdle disarms the checker', () => {
    dog.noteSent()
    vi.advanceTimersByTime(5_000)
    dog.noteIdle()
    vi.advanceTimersByTime(60_000)
    expect(probe).not.toHaveBeenCalled()
    expect(onDead).not.toHaveBeenCalled()
  })

  it('re-arms cleanly after a death', () => {
    dog.noteSent()
    vi.advanceTimersByTime(23_000)
    expect(onDead).toHaveBeenCalledTimes(1)
    dog.noteIdle() // the bridge clears pending in onDead and disarms
    dog.noteSent()
    vi.advanceTimersByTime(13_000)
    expect(probe).toHaveBeenCalledTimes(2)
    vi.advanceTimersByTime(10_000)
    expect(onDead).toHaveBeenCalledTimes(2)
  })
})
