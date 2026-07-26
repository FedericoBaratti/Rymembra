import { describe, expect, it } from 'vitest'
import { createHash } from 'node:crypto'
import { apiSig, shouldScrobble } from './signature'

const md5 = (s: string): string => createHash('md5').update(s, 'utf8').digest('hex')

describe('apiSig', () => {
  it('concatenates key+value pairs sorted by key, then the secret', () => {
    const sig = apiSig({ method: 'auth.gettoken', api_key: 'abc' }, 'secret')
    expect(sig).toBe(md5('api_keyabcmethodauth.gettoken' + 'secret'))
  })

  it('excludes format and callback from the signature', () => {
    const withFormat = apiSig(
      { method: 'auth.gettoken', api_key: 'abc', format: 'json', callback: 'cb' },
      'secret'
    )
    const without = apiSig({ method: 'auth.gettoken', api_key: 'abc' }, 'secret')
    expect(withFormat).toBe(without)
  })

  it('hashes UTF-8 input correctly', () => {
    const sig = apiSig({ artist: 'Björk', track: 'Jóga' }, 's')
    expect(sig).toBe(md5('artistBjörktrackJóga' + 's'))
  })
})

describe('shouldScrobble', () => {
  it('rejects tracks shorter than 30 seconds', () => {
    expect(shouldScrobble(29, 29)).toBe(false)
    expect(shouldScrobble(null, 100)).toBe(false)
    expect(shouldScrobble(0, 100)).toBe(false)
  })

  it('requires half the duration for short tracks', () => {
    expect(shouldScrobble(200, 99)).toBe(false)
    expect(shouldScrobble(200, 100)).toBe(true)
  })

  it('caps the requirement at 4 minutes for long tracks', () => {
    expect(shouldScrobble(1200, 239)).toBe(false)
    expect(shouldScrobble(1200, 240)).toBe(true)
  })

  it('accepts the boundary 30s track at half listened', () => {
    expect(shouldScrobble(30, 15)).toBe(true)
    expect(shouldScrobble(30, 14)).toBe(false)
  })
})
