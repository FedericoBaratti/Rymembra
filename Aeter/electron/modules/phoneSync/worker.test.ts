import { describe, it, expect, vi, beforeEach, afterAll } from 'vitest'
import { createHash } from 'node:crypto'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { PhoneTrackInfo } from '@shared/types'

// Idempotency guarantees of the repair worker, on a REAL phone_repair table
// (the migration chain replayed on an in-memory node:sqlite DB — the dynamic
// named-parameter SQL in patchRow/upsert deserves a real engine, not a fake):
//   1. a done row whose source file is unchanged is never re-queued;
//   2. a lost commit ack is detected from result_sha256 — the job completes
//      without re-downloading or re-uploading anything.
// The phone itself is fully mocked at the client module boundary.

const state = vi.hoisted(() => ({ tempRoot: '' }))

vi.mock('electron', () => ({
  app: { getPath: () => state.tempRoot }
}))
vi.mock('../db', async () => {
  const { DatabaseSync } = await import('node:sqlite')
  const { MIGRATIONS } = await vi.importActual<typeof import('../db')>('../db')
  const d = new DatabaseSync(':memory:')
  for (const step of MIGRATIONS) {
    // Function steps only use the exec/prepare/run/get/all subset DatabaseSync
    // shares with better-sqlite3 (same trick as db.schema.test.ts).
    if (typeof step === 'function') step(d as never)
    else d.exec(step)
  }
  return { getDb: () => d }
})
vi.mock('../settings', () => ({
  getSettings: () => ({ downloadQuality: 'mp3-320' })
}))
vi.mock('../events', () => ({ broadcast: vi.fn() }))
vi.mock('../logger', () => ({ logWarn: vi.fn() }))
vi.mock('../adaptiveConcurrency', () => ({
  thermalManager: { getConcurrency: (n: number) => n, onChange: () => () => {} }
}))
vi.mock('../download/validate', () => ({ validateAudioFile: vi.fn(async () => ({ ok: true })) }))
vi.mock('../audio/transcode', () => ({ transcodeFile: vi.fn(async () => {}) }))
vi.mock('../tagIO', () => ({ writeTags: vi.fn() }))
vi.mock('../enrichment/pipeline', () => ({ enrichFile: vi.fn() }))
vi.mock('music-metadata', () => ({ parseFile: vi.fn(async () => ({ common: {}, format: {} })) }))
vi.mock('./pairing', () => ({
  getPhonePeer: () => ({
    deviceId: 'dev-1',
    deviceName: 'Phone',
    lastHost: '192.168.1.2',
    lastPort: 40000,
    pairedAt: 0
  })
}))
vi.mock('./client', () => {
  class PhoneClientError extends Error {
    status?: number
    constructor(message: string, status?: number) {
      super(message)
      this.status = status
    }
  }
  return {
    PhoneClientError,
    connect: vi.fn(async () => ({ host: '192.168.1.2', port: 40000 })),
    listTracks: vi.fn(async () => []),
    getHash: vi.fn(),
    downloadFile: vi.fn(),
    uploadFile: vi.fn(),
    commitTrack: vi.fn(),
    deleteUpload: vi.fn(async () => ({ removed: 0 })),
    sessionStart: vi.fn(async () => ({ ok: true })),
    sessionHeartbeat: vi.fn(async () => ({ ok: true })),
    sessionEnd: vi.fn(async () => {})
  }
})

import { startRepair, isRepairRunning } from './worker'
import * as client from './client'
import { validateAudioFile } from '../download/validate'
import { getDb } from '../db'

state.tempRoot = mkdtempSync(join(tmpdir(), 'aether-phone-worker-'))
afterAll(() => rmSync(state.tempRoot, { recursive: true, force: true }))

function phoneInfo(overrides: Partial<PhoneTrackInfo> = {}): PhoneTrackInfo {
  return {
    id: 1,
    trackKey: 'artist|title|album',
    title: 'Title',
    artist: 'Artist',
    album: 'Album',
    year: 2020,
    genre: 'Rock',
    durationS: 200,
    bitrate: 128_000,
    sampleRate: 48_000,
    fileSize: 1000,
    mtimeMs: 123,
    codec: 'Opus', // off-target → the 'all' selection picks it up
    ext: '.opus',
    basename: 'song.opus',
    hasCover: true,
    enrichStatus: 'ok',
    ...overrides
  }
}

interface SeedRow {
  phone_track_id: number
  status: string
  attempts?: number
  src_size?: number | null
  src_mtime?: number | null
  result_sha256?: string | null
  result_ext?: string | null
}

function seedRow(row: SeedRow): void {
  getDb()
    .prepare(
      `INSERT INTO phone_repair (device_id, phone_track_id, track_key, title, artist, album,
         status, action_transcode, action_enrich, attempts, src_size, src_mtime,
         result_sha256, result_ext, updated_at)
       VALUES ('dev-1', ?, 'k', 'Title', 'Artist', 'Album', ?, 1, 0, ?, ?, ?, ?, ?, 0)`
    )
    .run(
      row.phone_track_id,
      row.status,
      row.attempts ?? 0,
      row.src_size ?? null,
      row.src_mtime ?? null,
      row.result_sha256 ?? null,
      row.result_ext ?? null
    )
}

function getRepairRow(phoneTrackId: number): Record<string, unknown> {
  return getDb()
    .prepare('SELECT * FROM phone_repair WHERE device_id = ? AND phone_track_id = ?')
    .get('dev-1', phoneTrackId) as Record<string, unknown>
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('startRepair idempotency', () => {
  it('never re-queues a done row whose source size+mtime are unchanged', async () => {
    const info = phoneInfo({ id: 1, fileSize: 1000, mtimeMs: 123 })
    vi.mocked(client.listTracks).mockResolvedValue([info])
    seedRow({ phone_track_id: 1, status: 'done', src_size: 1000, src_mtime: 123 })

    await startRepair('all')

    // Nothing to do → no session, no pull, no run at all.
    expect(isRepairRunning()).toBe(false)
    expect(client.sessionStart).not.toHaveBeenCalled()
    expect(client.getHash).not.toHaveBeenCalled()
    expect(client.downloadFile).not.toHaveBeenCalled()
    expect(getRepairRow(1).status).toBe('done')
  })

  it('re-runs a done row when the source changed, and a corrupt pull fails PERMANENTLY', async () => {
    const bytes = 'PULLED-AUDIO'
    const sha = createHash('sha256').update(bytes).digest('hex')
    const info = phoneInfo({ id: 3, fileSize: 1000, mtimeMs: 999 }) // mtime moved
    vi.mocked(client.listTracks).mockResolvedValue([info])
    seedRow({ phone_track_id: 3, status: 'done', src_size: 1000, src_mtime: 123 })
    vi.mocked(client.getHash).mockResolvedValue({ sha256: sha, size: bytes.length, mtimeMs: 999 })
    vi.mocked(client.downloadFile).mockImplementation(async (_id, dest) => {
      writeFileSync(dest, bytes)
    })
    // The fresh pull turns out corrupt → terminal failure, nothing pushed back.
    vi.mocked(validateAudioFile).mockResolvedValueOnce({ ok: false, reason: 'unparsable' })

    await startRepair([3])
    await vi.waitFor(() => expect(isRepairRunning()).toBe(false), { timeout: 5000 })

    expect(client.sessionStart).toHaveBeenCalledOnce()
    expect(client.downloadFile).toHaveBeenCalledOnce() // the changed source WAS re-pulled
    const row = getRepairRow(3)
    expect(row.status).toBe('failed')
    expect(String(row.error)).toMatch(/^SRC_CORRUPT:/)
    expect(client.uploadFile).not.toHaveBeenCalled()
    expect(client.commitTrack).not.toHaveBeenCalled()
    expect(client.sessionEnd).toHaveBeenCalledOnce()
  })

  it('detects a commit that landed before the ack was lost and finishes without re-uploading', async () => {
    const RESULT_SHA = 'a'.repeat(64)
    const info = phoneInfo({ id: 2, fileSize: 2000, mtimeMs: 456 })
    vi.mocked(client.listTracks).mockResolvedValue([info])
    // Previous run: pushed + committed, ack lost mid-flight.
    seedRow({
      phone_track_id: 2,
      status: 'committing',
      attempts: 1,
      src_size: 2000,
      src_mtime: 456,
      result_sha256: RESULT_SHA,
      result_ext: '.mp3'
    })
    // The phone now serves exactly the bytes we uploaded.
    vi.mocked(client.getHash).mockResolvedValue({ sha256: RESULT_SHA, size: 2100, mtimeMs: 789 })

    await startRepair([2])
    await vi.waitFor(() => expect(isRepairRunning()).toBe(false), { timeout: 5000 })

    const row = getRepairRow(2)
    expect(row.status).toBe('done')
    expect(row.src_sha256).toBe(RESULT_SHA)
    // The whole transfer pipeline is skipped: no pull, no push, no re-commit.
    expect(client.downloadFile).not.toHaveBeenCalled()
    expect(client.uploadFile).not.toHaveBeenCalled()
    expect(client.commitTrack).not.toHaveBeenCalled()
    expect(client.sessionEnd).toHaveBeenCalledOnce()
  })
})
