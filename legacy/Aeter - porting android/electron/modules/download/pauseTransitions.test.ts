import { describe, expect, it } from 'vitest'
import { canPause, canResume, statusOnAbort } from './pauseTransitions'

describe('canPause', () => {
  it('allows pausing queued and active downloads', () => {
    expect(canPause('pending')).toBe(true)
    expect(canPause('downloading')).toBe(true)
  })

  it('rejects terminal and already-paused states', () => {
    expect(canPause('paused')).toBe(false)
    expect(canPause('completed')).toBe(false)
    expect(canPause('error')).toBe(false)
    expect(canPause('cancelled')).toBe(false)
  })
})

describe('canResume', () => {
  it('only allows resuming paused downloads', () => {
    expect(canResume('paused')).toBe(true)
    expect(canResume('pending')).toBe(false)
    expect(canResume('downloading')).toBe(false)
    expect(canResume('completed')).toBe(false)
    expect(canResume('error')).toBe(false)
    expect(canResume('cancelled')).toBe(false)
  })
})

describe('statusOnAbort', () => {
  it('maps a pause-intended abort to paused', () => {
    expect(statusOnAbort(true)).toBe('paused')
  })

  it('maps a plain abort to cancelled', () => {
    expect(statusOnAbort(false)).toBe('cancelled')
  })
})
