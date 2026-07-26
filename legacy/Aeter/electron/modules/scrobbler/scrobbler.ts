// Offline scrobble queue core. DB-injected (better-sqlite3 or node:sqlite in
// tests) and network-agnostic via the send() dependency — no electron imports.

const BATCH_SIZE = 50

export interface ScrobbleEntry {
  artist: string
  title: string
  album: string | null
  duration: number | null
  /** Listen start, unix seconds (Last.fm scrobble identity). */
  playedAt: number
}

export interface ScrobbleDb {
  prepare(sql: string): {
    get(...params: unknown[]): unknown
    all(...params: unknown[]): unknown[]
    run(...params: unknown[]): unknown
  }
}

/** ok = accepted; transient = keep queued and retry later; auth = invalid session. */
export type SendResult = 'ok' | 'transient' | 'auth'

export interface ScrobblerDeps {
  /** Sends a signed, authenticated Last.fm call. Never throws. */
  send(method: string, params: Record<string, string>): Promise<SendResult>
  onInvalidSession?: () => void
}

interface QueueRow {
  id: number
  artist: string
  title: string
  album: string | null
  duration: number | null
  played_at: number
  attempts: number
}

export interface Scrobbler {
  enqueue(entry: ScrobbleEntry): void
  /** Drains the queue in batches of 50. Resolves the number of scrobbles sent. */
  flush(): Promise<number>
  queuedCount(): number
}

export function createScrobbler(db: ScrobbleDb, deps: ScrobblerDeps): Scrobbler {
  let flushing = false

  const flush = async (): Promise<number> => {
    if (flushing) return 0
    flushing = true
    let sent = 0
    try {
      for (;;) {
        const rows = db
          .prepare(
            `SELECT id, artist, title, album, duration, played_at, attempts
             FROM scrobble_queue ORDER BY played_at LIMIT ${BATCH_SIZE}`
          )
          .all() as QueueRow[]
        if (rows.length === 0) break

        const params: Record<string, string> = {}
        rows.forEach((row, i) => {
          params[`artist[${i}]`] = row.artist
          params[`track[${i}]`] = row.title
          params[`timestamp[${i}]`] = String(row.played_at)
          if (row.album) params[`album[${i}]`] = row.album
          if (row.duration) params[`duration[${i}]`] = String(Math.round(row.duration))
        })

        const result = await deps.send('track.scrobble', params)
        if (result === 'ok') {
          const ph = rows.map(() => '?').join(', ')
          db.prepare(`DELETE FROM scrobble_queue WHERE id IN (${ph})`).run(
            ...rows.map((r) => r.id)
          )
          sent += rows.length
          if (rows.length < BATCH_SIZE) break
        } else {
          const ph = rows.map(() => '?').join(', ')
          db.prepare(`UPDATE scrobble_queue SET attempts = attempts + 1 WHERE id IN (${ph})`).run(
            ...rows.map((r) => r.id)
          )
          if (result === 'auth') deps.onInvalidSession?.()
          break
        }
      }
    } finally {
      flushing = false
    }
    return sent
  }

  return {
    enqueue(entry) {
      db.prepare(
        `INSERT INTO scrobble_queue (artist, title, album, duration, played_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?)`
      ).run(entry.artist, entry.title, entry.album, entry.duration, entry.playedAt, Date.now())
    },
    flush,
    queuedCount() {
      const row = db.prepare('SELECT COUNT(*) AS n FROM scrobble_queue').get() as { n: number }
      return row.n
    }
  }
}
