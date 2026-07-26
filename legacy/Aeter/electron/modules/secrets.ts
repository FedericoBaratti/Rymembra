import { app, safeStorage } from 'electron'
import { mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { createDebouncedJsonFile } from './jsonFile'
import { logError } from './logger'
import {
  SECRET_KEYS,
  decodeSecrets,
  encodeSecrets,
  type SecretCodec,
  type SecretValues,
  type SecretsFileShape
} from './secretsCore'

// Encrypted-at-rest storage for API credentials (userData/secrets.json).
// Values go through Electron safeStorage when the OS keychain/DPAPI is
// available; otherwise they are kept plaintext (no worse than the old
// settings.json) with `encrypted: false` so the UI can warn about it.

export { SECRET_KEYS }
export type { SecretValues }

const safeStorageCodec: SecretCodec = {
  available: () => safeStorage.isEncryptionAvailable(),
  encrypt: (plain) => safeStorage.encryptString(plain).toString('base64'),
  decrypt: (encoded) => safeStorage.decryptString(Buffer.from(encoded, 'base64'))
}

let cache: SecretValues | null = null

function secretsPath(): string {
  return join(app.getPath('userData'), 'secrets.json')
}

const secretsFile = createDebouncedJsonFile<SecretsFileShape>(secretsPath)

/** Reads and decrypts secrets.json bypassing the cache (migration verify). */
export function readSecretsFromDisk(): SecretValues {
  let parsed: unknown = null
  try {
    parsed = JSON.parse(readFileSync(secretsPath(), 'utf-8'))
  } catch {
    parsed = null
  }
  return decodeSecrets(parsed, safeStorageCodec, (k, err) =>
    logError('secrets', `decrypt failed for ${k}`, err)
  )
}

export function loadSecrets(): SecretValues {
  if (!cache) cache = readSecretsFromDisk()
  return cache
}

export function setSecrets(patch: Partial<SecretValues>): SecretValues {
  const next = { ...loadSecrets(), ...patch }
  cache = next
  secretsFile.write(encodeSecrets(next, safeStorageCodec))
  return next
}

/** Synchronous tmp+rename write used by the one-time migration. */
export function writeSecretsSync(values: SecretValues): void {
  const path = secretsPath()
  mkdirSync(dirname(path), { recursive: true })
  const tmp = `${path}.tmp`
  writeFileSync(tmp, JSON.stringify(encodeSecrets(values, safeStorageCodec), null, 2), 'utf-8')
  renameSync(tmp, path)
  cache = values
}

/** Persists any pending secrets write synchronously (call on quit). */
export function flushSecretsSync(): void {
  secretsFile.flushSync()
}

export function secretsEncryptionAvailable(): boolean {
  return safeStorageCodec.available()
}
