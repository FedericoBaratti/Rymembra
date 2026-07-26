import { describe, it, expect, vi } from 'vitest'
import {
  ALL_SECRET_KEYS,
  decodeSecrets,
  emptyValues,
  encodeSecrets,
  type SecretCodec
} from './secretsCore'

/** Reversible fake: "enc:" prefix marks encrypted payloads. */
function fakeCodec(available = true): SecretCodec {
  return {
    available: () => available,
    encrypt: (plain) => `enc:${Buffer.from(plain).toString('base64')}`,
    decrypt: (encoded) => {
      if (!encoded.startsWith('enc:')) throw new Error('not encrypted')
      return Buffer.from(encoded.slice(4), 'base64').toString()
    }
  }
}

describe('encodeSecrets / decodeSecrets', () => {
  it('round-trips with encryption available', () => {
    const codec = fakeCodec(true)
    const values = { ...emptyValues(), lastfmApiKey: 'chiave', spotifyClientId: 'id123' }
    const file = encodeSecrets(values, codec)
    expect(file.encrypted).toBe(true)
    expect(file.values.lastfmApiKey).not.toContain('chiave')
    expect(decodeSecrets(file, codec)).toEqual(values)
  })

  it('round-trips in plaintext fallback', () => {
    const codec = fakeCodec(false)
    const values = { ...emptyValues(), acoustidApiKey: 'abc' }
    const file = encodeSecrets(values, codec)
    expect(file.encrypted).toBe(false)
    expect(file.values.acoustidApiKey).toBe('abc')
    expect(decodeSecrets(file, codec)).toEqual(values)
  })

  it('omits empty values on encode', () => {
    const file = encodeSecrets({ lastfmApiKey: '' }, fakeCodec())
    expect(Object.keys(file.values)).toHaveLength(0)
  })

  it('returns empty values for corrupt input', () => {
    const codec = fakeCodec()
    expect(decodeSecrets(null, codec)).toEqual(emptyValues())
    expect(decodeSecrets('garbage', codec)).toEqual(emptyValues())
    expect(decodeSecrets({ encrypted: true, values: null }, codec)).toEqual(emptyValues())
  })

  it('turns undecryptable values into empty strings and reports them', () => {
    const codec = fakeCodec()
    const onError = vi.fn()
    const got = decodeSecrets(
      { encrypted: true, values: { lastfmApiKey: 'broken-payload', acoustidApiKey: codec.encrypt('ok') } },
      codec,
      onError
    )
    expect(got.lastfmApiKey).toBe('')
    expect(got.acoustidApiKey).toBe('ok')
    expect(onError).toHaveBeenCalledWith('lastfmApiKey', expect.any(Error))
  })

  it('ignores unknown keys in the file', () => {
    const got = decodeSecrets(
      { encrypted: false, values: { evil: 'x', lastfmApiKey: 'k' } },
      fakeCodec()
    )
    expect(Object.keys(got).sort()).toEqual([...ALL_SECRET_KEYS].sort())
    expect(got.lastfmApiKey).toBe('k')
  })
})
