import { describe, expect, it, beforeEach, vi } from 'vitest'
import { DatabaseSync } from 'node:sqlite'
import { createScrobbler, type ScrobbleDb, type SendResult } from './scrobbler'

// Mirrors migration v5.
const DDL = `
  CREATE TABLE scrobble_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    artist TEXT NOT NULL,
    title TEXT NOT NULL,
    album TEXT,
    duration INTEGER,
    played_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0
  );
`

let db: DatabaseSync

beforeEach(() => {
  db = new DatabaseSync(':memory:')
  db.exec(DDL)
})

const entry = (over: Partial<{ artist: string; title: string; playedAt: number }> = {}): {
  artist: string
  title: string
  album: string | null
  duration: number | null
  playedAt: number
} => ({
  artist: 'Artist',
  title: 'Song',
  album: 'Album',
  duration: 180,
  playedAt: 1_700_000_000,
  ...over
})

describe('createScrobbler', () => {
  it('enqueues and flushes successfully, clearing the queue', async () => {
    const send = vi.fn<(m: string, p: Record<string, string>) => Promise<SendResult>>(
      async () => 'ok'
    )
    const s = createScrobbler(db as unknown as ScrobbleDb, { send })
    s.enqueue(entry())
    s.enqueue(entry({ title: 'Second', playedAt: 1_700_000_100 }))
    expect(s.queuedCount()).toBe(2)

    const sent = await s.flush()
    expect(sent).toBe(2)
    expect(s.queuedCount()).toBe(0)

    expect(send).toHaveBeenCalledTimes(1)
    const [method, params] = send.mock.calls[0]
    expect(method).toBe('track.scrobble')
    expect(params['track[0]']).toBe('Song')
    expect(params['artist[0]']).toBe('Artist')
    expect(params['timestamp[0]']).toBe('1700000000')
    expect(params['album[0]']).toBe('Album')
    expect(params['duration[0]']).toBe('180')
    expect(params['track[1]']).toBe('Second')
  })

  it('keeps rows and increments attempts on transient failure', async () => {
    const s = createScrobbler(db as unknown as ScrobbleDb, { send: async () => 'transient' })
    s.enqueue(entry())
    expect(await s.flush()).toBe(0)
    expect(s.queuedCount()).toBe(1)
    const row = db.prepare('SELECT attempts FROM scrobble_queue').get() as { attempts: number }
    expect(row.attempts).toBe(1)
  })

  it('chunks batches of 50 per call', async () => {
    const send = vi.fn<(m: string, p: Record<string, string>) => Promise<SendResult>>(
      async () => 'ok'
    )
    const s = createScrobbler(db as unknown as ScrobbleDb, { send })
    for (let i = 0; i < 120; i++) s.enqueue(entry({ playedAt: 1_700_000_000 + i }))

    expect(await s.flush()).toBe(120)
    expect(send).toHaveBeenCalledTimes(3)
    expect(Object.keys(send.mock.calls[0][1]).filter((k) => k.startsWith('track['))).toHaveLength(50)
    expect(Object.keys(send.mock.calls[2][1]).filter((k) => k.startsWith('track['))).toHaveLength(20)
  })

  it('notifies on invalid session and keeps the rows', async () => {
    const onInvalidSession = vi.fn()
    const s = createScrobbler(db as unknown as ScrobbleDb, {
      send: async () => 'auth',
      onInvalidSession
    })
    s.enqueue(entry())
    expect(await s.flush()).toBe(0)
    expect(onInvalidSession).toHaveBeenCalledTimes(1)
    expect(s.queuedCount()).toBe(1)
  })

  it('omits null album/duration params', async () => {
    const send = vi.fn<(m: string, p: Record<string, string>) => Promise<SendResult>>(
      async () => 'ok'
    )
    const s = createScrobbler(db as unknown as ScrobbleDb, { send })
    s.enqueue({ artist: 'A', title: 'T', album: null, duration: null, playedAt: 1 })
    await s.flush()
    const params = send.mock.calls[0][1]
    expect(params['album[0]']).toBeUndefined()
    expect(params['duration[0]']).toBeUndefined()
  })
})
