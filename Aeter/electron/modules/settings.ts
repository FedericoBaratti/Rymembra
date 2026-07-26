import { app, nativeTheme } from 'electron'
import { copyFileSync, existsSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { randomUUID } from 'node:crypto'
import { join } from 'node:path'
import type { AppSettings } from '@shared/types'
import { createDebouncedJsonFile } from './jsonFile'
import { DEFAULTS, parseSettings } from './settingsSchema'
import { logError, logWarn } from './logger'
import {
  SECRET_KEYS,
  loadSecrets,
  readSecretsFromDisk,
  setSecrets,
  writeSecretsSync,
  type SecretValues
} from './secrets'

let cache: AppSettings | null = null

function settingsPath(): string {
  return join(app.getPath('userData'), 'settings.json')
}

export function getSettings(): AppSettings {
  if (cache) return cache
  let loaded: AppSettings
  const path = settingsPath()
  try {
    const raw = readFileSync(path, 'utf-8')
    loaded = parseSettings(JSON.parse(raw))
  } catch (err) {
    // Never silently destroy the user's file: keep a copy before resetting.
    if (existsSync(path)) {
      logError('settings', 'settings.json unreadable, falling back to defaults', err)
      try {
        copyFileSync(path, `${path}.corrupt-${Date.now()}`)
      } catch {
        // best effort
      }
    }
    loaded = { ...DEFAULTS }
  }
  if (!loaded.downloadFolder) {
    loaded.downloadFolder = join(app.getPath('music'), 'Aether')
  }
  // Overlay decrypted credentials so every consumer (and the Settings UI)
  // keeps seeing them on the settings object, transparently.
  const secrets = loadSecrets()
  for (const k of SECRET_KEYS) {
    if (secrets[k]) loaded[k] = secrets[k]
  }
  // Assign a stable per-install id on first read; persist it directly (not via
  // setSettings, which would re-enter getSettings) so it survives restarts.
  if (!loaded.syncDeviceId) {
    loaded.syncDeviceId = randomUUID()
    cache = loaded
    settingsFile.write(stripSecrets(loaded))
    return loaded
  }
  cache = loaded
  return loaded
}

/** Keeps Chromium's native widgets (select popups, scrollbars) on the app
    theme — they follow nativeTheme, not the renderer's CSS color-scheme. */
export function syncNativeTheme(): void {
  nativeTheme.themeSource = getSettings().theme
}

const settingsFile = createDebouncedJsonFile<AppSettings>(settingsPath)

/** Credentials never land in settings.json: they are blanked on write. */
function stripSecrets(s: AppSettings): AppSettings {
  const out = { ...s }
  for (const k of SECRET_KEYS) out[k] = ''
  return out
}

export function setSettings(patch: Partial<AppSettings>): AppSettings {
  const secretPatch: Partial<SecretValues> = {}
  for (const k of SECRET_KEYS) {
    if (k in patch) secretPatch[k] = String(patch[k] ?? '')
  }
  if (Object.keys(secretPatch).length > 0) setSecrets(secretPatch)
  const next = { ...getSettings(), ...patch }
  cache = next
  settingsFile.write(stripSecrets(next))
  return next
}

/** Persists any pending settings write synchronously (call on quit). */
export function flushSettingsSync(): void {
  settingsFile.flushSync()
}

/**
 * One-time migration of plaintext credentials out of settings.json into the
 * encrypted secrets store. Order is deliberate: write secrets first, verify
 * them by re-reading from disk, and only then blank settings.json. On any
 * failure the plaintext file is left untouched (no worse than before).
 */
export function migratePlaintextSecrets(): void {
  const path = settingsPath()
  let raw: Record<string, unknown>
  try {
    raw = JSON.parse(readFileSync(path, 'utf-8')) as Record<string, unknown>
  } catch {
    return
  }
  const found: Partial<SecretValues> = {}
  for (const k of SECRET_KEYS) {
    const v = raw[k]
    if (typeof v === 'string' && v) found[k] = v
  }
  if (Object.keys(found).length === 0) return
  try {
    if (!existsSync(`${path}.pre-secrets.bak`)) {
      copyFileSync(path, `${path}.pre-secrets.bak`)
    }
    // Existing secrets-store values win over stale plaintext leftovers.
    const current = readSecretsFromDisk()
    const merged: SecretValues = { ...current }
    for (const k of SECRET_KEYS) {
      if (!merged[k] && found[k]) merged[k] = found[k]
    }
    writeSecretsSync(merged)
    const verify = readSecretsFromDisk()
    for (const k of SECRET_KEYS) {
      if (verify[k] !== merged[k]) throw new Error(`verifica fallita per ${k}`)
    }
    for (const k of SECRET_KEYS) raw[k] = ''
    const tmp = `${path}.tmp`
    writeFileSync(tmp, JSON.stringify(raw, null, 2), 'utf-8')
    renameSync(tmp, path)
    cache = null // next getSettings() reloads and overlays the secrets store
    logWarn('settings', 'credenziali migrate nello store cifrato (secrets.json)')
  } catch (err) {
    logError('settings', 'migrazione segreti fallita; settings.json lasciato invariato', err)
  }
}
