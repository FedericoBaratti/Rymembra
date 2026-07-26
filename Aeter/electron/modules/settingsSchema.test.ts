import { describe, it, expect } from 'vitest'
import { DEFAULTS, parseSettings } from './settingsSchema'

describe('parseSettings', () => {
  it('returns defaults for an empty object', () => {
    expect(parseSettings({})).toEqual(DEFAULTS)
  })

  it('returns defaults for non-object input', () => {
    expect(parseSettings(null)).toEqual(DEFAULTS)
    expect(parseSettings('garbage')).toEqual(DEFAULTS)
    expect(parseSettings(42)).toEqual(DEFAULTS)
  })

  it('passes a valid file through unchanged', () => {
    const valid = { ...DEFAULTS, volume: 0.5, theme: 'light' as const, language: 'en' as const }
    expect(parseSettings(valid)).toEqual(valid)
  })

  it('falls back per-field without resetting the rest', () => {
    const parsed = parseSettings({
      volume: 99,
      theme: 'neon',
      language: 'en',
      downloadFolder: 'D:/Music'
    })
    expect(parsed.volume).toBe(DEFAULTS.volume)
    expect(parsed.theme).toBe(DEFAULTS.theme)
    expect(parsed.language).toBe('en')
    expect(parsed.downloadFolder).toBe('D:/Music')
  })

  it('rejects malformed eqGains but keeps eqEnabled', () => {
    const parsed = parseSettings({ eqEnabled: true, eqGains: [1, 2, 3] })
    expect(parsed.eqEnabled).toBe(true)
    expect(parsed.eqGains).toEqual(DEFAULTS.eqGains)
  })

  it('rejects out-of-range numeric fields', () => {
    const parsed = parseSettings({ downloadConcurrency: 0, crossfadeSeconds: -1 })
    expect(parsed.downloadConcurrency).toBe(DEFAULTS.downloadConcurrency)
    expect(parsed.crossfadeSeconds).toBe(DEFAULTS.crossfadeSeconds)
  })

  it('rejects wrongly typed secrets but keeps valid ones', () => {
    const parsed = parseSettings({ lastfmApiKey: 123, acoustidApiKey: 'abc' })
    expect(parsed.lastfmApiKey).toBe('')
    expect(parsed.acoustidApiKey).toBe('abc')
  })

  it('ignores unknown keys', () => {
    const parsed = parseSettings({ legacyField: true })
    expect(parsed).toEqual(DEFAULTS)
    expect('legacyField' in parsed).toBe(false)
  })
})
