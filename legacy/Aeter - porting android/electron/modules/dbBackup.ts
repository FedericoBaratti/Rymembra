import type Database from 'better-sqlite3'
import { app } from 'electron'
import { existsSync, mkdirSync, readdirSync, statSync, unlinkSync } from 'node:fs'
import { join } from 'node:path'
import { logError, logWarn } from './logger'

const KEEP_AUTO = 5
const KEEP_PRE = 3
const AUTO_INTERVAL_MS = 24 * 60 * 60 * 1000

function backupDir(): string {
  const dir = join(app.getPath('userData'), 'backups')
  mkdirSync(dir, { recursive: true })
  return dir
}

function timestamp(): string {
  return new Date().toISOString().slice(0, 16).replace(/:/g, '-')
}

function prune(dir: string, prefix: string, keep: number): void {
  const old = readdirSync(dir)
    .filter((f) => f.startsWith(prefix) && f.endsWith('.db'))
    .map((f) => ({ f, mtime: statSync(join(dir, f)).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime)
    .slice(keep)
  for (const { f } of old) {
    try {
      unlinkSync(join(dir, f))
    } catch (err) {
      logWarn('backup', `prune failed for ${f}`, err)
    }
  }
}

/** Synchronous consistent snapshot before running migrations (VACUUM INTO is
    WAL-safe). Never throws: bricking startup is worse than a missing backup. */
export function preMigrationBackup(d: Database.Database, fromVersion: number): void {
  try {
    const dir = backupDir()
    const target = join(dir, `aether-pre-v${fromVersion}-${timestamp()}.db`)
    if (existsSync(target)) return
    d.exec(`VACUUM INTO '${target.replace(/'/g, "''")}'`)
    prune(dir, 'aether-pre-', KEEP_PRE)
  } catch (err) {
    logError('backup', `pre-migration backup failed (v${fromVersion})`, err)
  }
}

/** Online backup at most once per 24h; call after the window is interactive. */
export async function runStartupBackup(d: Database.Database): Promise<void> {
  try {
    const dir = backupDir()
    const newest = readdirSync(dir)
      .filter((f) => f.startsWith('aether-auto-') && f.endsWith('.db'))
      .map((f) => statSync(join(dir, f)).mtimeMs)
      .sort((a, b) => b - a)[0]
    if (newest && Date.now() - newest < AUTO_INTERVAL_MS) return
    await d.backup(join(dir, `aether-auto-${timestamp()}.db`))
    prune(dir, 'aether-auto-', KEEP_AUTO)
  } catch (err) {
    logError('backup', 'startup backup failed', err)
  }
}
