/**
 * Records explicit deletions into `sync_tombstones` so a delete made on one
 * device is not resurrected by a stale copy on another. Only user-initiated
 * deletions call this (deleteTracks / deletePlaylist); a file that merely
 * disappeared from the watcher is a device-local event and must NOT tombstone,
 * because the same file may still exist on another device.
 */
import { getDb } from '../db'

type Db = ReturnType<typeof getDb>
export type TombstoneKind = 'track' | 'playlist'

const UPSERT_TOMBSTONE = `
  INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES (?, ?, ?)
  ON CONFLICT(kind, key) DO UPDATE SET deleted_at = excluded.deleted_at
`

/** Record a single deletion. No-op on an empty key. */
export function recordTombstone(kind: TombstoneKind, key: string, db: Db = getDb()): void {
  if (!key) return
  db.prepare(UPSERT_TOMBSTONE).run(kind, key, Date.now())
}

/** Record many deletions of the same kind in one transaction. */
export function recordTombstones(kind: TombstoneKind, keys: string[], db: Db = getDb()): void {
  const stmt = db.prepare(UPSERT_TOMBSTONE)
  const now = Date.now()
  const tx = db.transaction(() => {
    for (const key of keys) if (key) stmt.run(kind, key, now)
  })
  tx()
}
