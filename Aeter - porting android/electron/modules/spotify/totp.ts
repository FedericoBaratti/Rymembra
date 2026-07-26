import { createHmac } from 'node:crypto'

// Pure RFC-6238 TOTP used to mint Spotify's *anonymous* Web-Player access token
// (the keyless path — no Client ID/Secret). No electron imports → unit-tested.
//
// Spotify's web player signs its token request with an HMAC-SHA1 TOTP (period
// 30s, 6 digits) whose secret is derived from a versioned integer "cipher"
// embedded in the player bundle. When Spotify rotates it, the token endpoint
// answers "Invalid TOTP" and the resolver falls back to embed/oEmbed scraping
// (so small playlists/albums/tracks keep working).
//
// ⚠️ VOLATILE: Spotify rotates the secret every few days — the versions below
// WILL go stale, which is exactly why the resolver degrades to embed/oEmbed.
// Verified online (2026-07): the mechanism is unchanged (XOR (i%33)+9 → the
// decimal string's UTF-8 bytes as the HMAC key — the hex/base32 round-trip seen
// in pyotp-based tools is only to feed pyotp a base32 secret and nets the same
// key). Community-maintained fresh secrets are published at
// github.com/misiektoja/spotify_monitor (SECRET_CIPHER_DICT / its remote URL).
// To refresh: prepend the newest {version, cipher} from there (or extract it
// from the open.spotify.com web-player JS) — nothing else changes. Keep this
// list in sync with the desktop build's electron/modules/spotify/totp.ts.

export interface TotpSecret {
  /** Sent to Spotify as `totpVer`; must match the server-side secret version. */
  version: number
  /** Integer cipher transformed into the HMAC key (see deriveKey). */
  cipher: number[]
}

/** Known cipher(s), newest first. The resolver tries each until one mints a token. */
export const SECRET_CIPHERS: TotpSecret[] = [
  // Verified current 2026-07 (github.com/misiektoja/spotify_monitor main branch).
  { version: 61, cipher: [44, 55, 47, 42, 70, 40, 34, 114, 76, 74, 50, 111, 120, 97, 75, 76, 94, 102, 43, 69, 49, 120, 118, 80, 64, 78] },
  // Recent prior versions (kept as fallbacks — harmless to try).
  { version: 14, cipher: [62, 54, 109, 83, 107, 77, 41, 103, 45, 93, 114, 38, 41, 97, 64, 51, 95, 94, 95, 94] },
  { version: 13, cipher: [59, 92, 64, 70, 99, 78, 117, 75, 99, 103, 116, 67, 103, 51, 87, 63, 93, 59, 70, 45, 32] },
  { version: 12, cipher: [12, 56, 76, 33, 88, 44, 88, 33, 78, 78, 11, 66, 22, 22, 55, 69, 54] },
  { version: 11, cipher: [111, 45, 40, 73, 89, 53, 67, 47, 76, 105, 65, 116, 100, 45, 51, 78, 50] },
  { version: 8, cipher: [37, 84, 32, 76, 87, 90, 87, 47, 13, 75, 48, 54, 44, 28, 19, 21, 22] }
]

/** HMAC key = UTF-8 bytes of the digit string formed by transforming the cipher. */
export function deriveKey(cipher: number[]): Buffer {
  const transformed = cipher.map((n, i) => n ^ ((i % 33) + 9))
  return Buffer.from(transformed.join(''), 'utf8')
}

/** RFC-4226 HOTP for an 8-byte big-endian counter. */
function hotp(key: Buffer, counter: number): string {
  const buf = Buffer.alloc(8)
  buf.writeUInt32BE(Math.floor(counter / 0x100000000), 0)
  buf.writeUInt32BE(counter >>> 0, 4)
  const hmac = createHmac('sha1', key).update(buf).digest()
  const offset = hmac[hmac.length - 1] & 0x0f
  const bin =
    ((hmac[offset] & 0x7f) << 24) |
    ((hmac[offset + 1] & 0xff) << 16) |
    ((hmac[offset + 2] & 0xff) << 8) |
    (hmac[offset + 3] & 0xff)
  return (bin % 1_000_000).toString().padStart(6, '0')
}

/** 6-digit TOTP for a given wall-clock time (ms), period 30s. */
export function generateTotp(secret: TotpSecret, timeMs: number): string {
  return hotp(deriveKey(secret.cipher), Math.floor(timeMs / 1000 / 30))
}
