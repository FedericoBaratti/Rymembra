// Pure encode/decode logic for the secrets store — no electron imports, so
// it stays unit-testable. The safeStorage-bound state lives in secrets.ts.

export const SECRET_KEYS = [
  'spotifyClientId',
  'spotifyClientSecret',
  'lastfmApiKey',
  'lastfmApiSecret',
  'lastfmSessionKey',
  'acoustidApiKey',
  'googleClientId',
  'googleClientSecret',
  'googleRefreshToken'
] as const

/**
 * Secrets that are NOT AppSettings fields: reversible tokens owned by backend
 * modules (never overlaid onto the settings object or shown in the UI).
 * Same encrypted file, separate list so settings.ts keeps iterating only the
 * settings-backed keys.
 */
export const EXTRA_SECRET_KEYS = [
  // Raw bearer token for the paired phone's transfer server (phoneSync/).
  'phoneTransferToken'
] as const

export const ALL_SECRET_KEYS = [...SECRET_KEYS, ...EXTRA_SECRET_KEYS] as const

export type SecretKey = (typeof ALL_SECRET_KEYS)[number]
export type SecretValues = Record<SecretKey, string>

export interface SecretsFileShape {
  encrypted: boolean
  values: Partial<Record<SecretKey, string>>
}

/** Codec seam: safeStorage in production, a fake in tests. */
export interface SecretCodec {
  available(): boolean
  encrypt(plain: string): string
  decrypt(encoded: string): string
}

export function emptyValues(): SecretValues {
  return {
    spotifyClientId: '',
    spotifyClientSecret: '',
    lastfmApiKey: '',
    lastfmApiSecret: '',
    lastfmSessionKey: '',
    acoustidApiKey: '',
    googleClientId: '',
    googleClientSecret: '',
    googleRefreshToken: '',
    phoneTransferToken: ''
  }
}

/** Plaintext values -> on-disk shape. Empty values are omitted. */
export function encodeSecrets(values: Partial<SecretValues>, codec: SecretCodec): SecretsFileShape {
  const encrypted = codec.available()
  const out: Partial<Record<SecretKey, string>> = {}
  for (const k of ALL_SECRET_KEYS) {
    const v = values[k]
    if (!v) continue
    out[k] = encrypted ? codec.encrypt(v) : v
  }
  return { encrypted, values: out }
}

/** On-disk shape -> plaintext values. A value that fails to decrypt (e.g.
    file copied from another machine) becomes '' instead of crashing. */
export function decodeSecrets(
  file: unknown,
  codec: SecretCodec,
  onError?: (key: SecretKey, err: unknown) => void
): SecretValues {
  const out = emptyValues()
  if (typeof file !== 'object' || file === null) return out
  const shape = file as SecretsFileShape
  if (typeof shape.values !== 'object' || shape.values === null) return out
  for (const k of ALL_SECRET_KEYS) {
    const raw = shape.values[k]
    if (typeof raw !== 'string' || !raw) continue
    if (!shape.encrypted) {
      out[k] = raw
      continue
    }
    try {
      out[k] = codec.decrypt(raw)
    } catch (err) {
      onError?.(k, err)
    }
  }
  return out
}
