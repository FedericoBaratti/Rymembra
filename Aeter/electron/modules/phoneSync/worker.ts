import { app } from 'electron'
import { createHash } from 'node:crypto'
import { createReadStream } from 'node:fs'
import { mkdir, rename, unlink } from 'node:fs/promises'
import { join } from 'node:path'
import PQueue from 'p-queue'
import { parseFile } from 'music-metadata'
import type {
  DownloadQuality,
  PhoneRepairItem,
  PhoneRepairStatus,
  PhoneTrackInfo,
  PhoneTrackPlanned
} from '@shared/types'
import { getDb } from '../db'
import { broadcast } from '../events'
import { logWarn } from '../logger'
import { getSettings } from '../settings'
import { thermalManager } from '../adaptiveConcurrency'
import { validateAudioFile } from '../download/validate'
import { transcodeFile } from '../audio/transcode'
import { writeTags } from '../tagIO'
import { enrichFile } from '../enrichment/pipeline'
import { planTrack, type TrackPlan } from './plan'
import { getPhonePeer } from './pairing'
import * as client from './client'

/**
 * Desktop worker for the phone repair: pulls each phone track into a local
 * staging dir, runs the fix pipeline (integrity check → codec standardization
 * → metadata/cover enrichment), pushes the result back and asks the phone to
 * commit it in place. Job state is DURABLE (phone_repair table, one row per
 * (device_id, phone_track_id)) so an interrupted run resumes: a done row whose
 * source is unchanged is never re-processed, and a commit whose ack was lost
 * is detected by comparing the phone's current hash with result_sha256.
 *
 * Model: sync/fetchMissing.ts (reconcile → work queue → settle) + downloader.ts
 * (PQueue + per-job AbortController + bounded retries).
 */

const BASE_CONCURRENCY = 2
const MAX_ATTEMPTS = 3
// In-slot backoff between attempts of the same job (LAN hiccups are short).
const RETRY_DELAY_MS = [5_000, 15_000, 45_000]
const HEARTBEAT_MS = 20_000

interface RepairRow {
  id: number
  device_id: string
  phone_track_id: number
  track_key: string
  title: string
  artist: string
  album: string
  status: PhoneRepairStatus
  action_transcode: number
  action_enrich: number
  attempts: number
  next_retry_at: number | null
  error: string | null
  src_size: number | null
  src_mtime: number | null
  src_sha256: string | null
  result_sha256: string | null
  result_ext: string | null
  updated_at: number
}

function toItem(row: RepairRow): PhoneRepairItem {
  return {
    id: row.id,
    deviceId: row.device_id,
    phoneTrackId: row.phone_track_id,
    trackKey: row.track_key,
    title: row.title,
    artist: row.artist,
    album: row.album,
    status: row.status,
    actionTranscode: row.action_transcode === 1,
    actionEnrich: row.action_enrich === 1,
    attempts: row.attempts,
    error: row.error,
    updatedAt: row.updated_at
  }
}

function getRow(id: number): RepairRow | null {
  return (
    (getDb().prepare('SELECT * FROM phone_repair WHERE id = ?').get(id) as RepairRow | undefined) ??
    null
  )
}

function patchRow(id: number, patch: Partial<RepairRow>): void {
  const keys = Object.keys(patch) as (keyof RepairRow)[]
  if (keys.length === 0) return
  const sets = keys.map((k) => `${String(k)} = @${String(k)}`).join(', ')
  getDb()
    .prepare(`UPDATE phone_repair SET ${sets}, updated_at = @__now WHERE id = @__id`)
    .run({ ...patch, __now: Date.now(), __id: id })
  const row = getRow(id)
  if (row) broadcast('phoneRepair:updated', toItem(row))
}

function stagingDir(deviceId: string): string {
  return join(app.getPath('userData'), 'phone-staging', deviceId)
}

function sha256File(path: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const hash = createHash('sha256')
    const stream = createReadStream(path)
    stream.on('error', reject)
    stream.on('data', (chunk) => hash.update(chunk))
    stream.on('end', () => resolve(hash.digest('hex')))
  })
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) return reject(new Error('ABORTED'))
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort)
      resolve()
    }, ms)
    const onAbort = (): void => {
      clearTimeout(timer)
      reject(new Error('ABORTED'))
    }
    signal.addEventListener('abort', onAbort, { once: true })
  })
}

// ---- listing / reconciliation --------------------------------------------

/** Connects to the phone and returns its tracks joined with plan + job rows. */
export async function refreshPhoneTracks(): Promise<PhoneTrackPlanned[]> {
  const peer = getPhonePeer()
  if (!peer) throw new client.PhoneClientError('PHONE_NOT_PAIRED')
  await client.connect()
  const infos = await client.listTracks()
  const quality = getSettings().downloadQuality
  const rows = getDb()
    .prepare('SELECT * FROM phone_repair WHERE device_id = ?')
    .all(peer.deviceId) as RepairRow[]
  const byTrackId = new Map(rows.map((r) => [r.phone_track_id, r]))
  return infos.map((info) => {
    const row = byTrackId.get(info.id) ?? null
    return {
      info,
      badge: planTrack(info, quality).badge,
      repair: row ? toItem(row) : null
    }
  })
}

// ---- run state -----------------------------------------------------------

let running = false
let runAbort: AbortController | null = null

export function isRepairRunning(): boolean {
  return running
}

export function cancelRepair(): void {
  runAbort?.abort()
}

/**
 * Queues a repair run. `ids` are PHONE track ids ('all' = every track whose
 * plan is not 'ok'). Throws PHONE_BUSY when a run is already active.
 */
export async function startRepair(ids: number[] | 'all'): Promise<void> {
  if (running) throw new Error('REPAIR_ALREADY_RUNNING')
  const peer = getPhonePeer()
  if (!peer) throw new client.PhoneClientError('PHONE_NOT_PAIRED')

  await client.connect()
  const infos = await client.listTracks()
  const quality = getSettings().downloadQuality
  const byId = new Map(infos.map((i) => [i.id, i]))

  const selected: { info: PhoneTrackInfo; plan: TrackPlan }[] = []
  for (const info of infos) {
    const plan = planTrack(info, quality)
    if (ids === 'all') {
      if (plan.badge !== 'ok') selected.push({ info, plan })
    } else if (ids.includes(info.id)) {
      selected.push({ info, plan })
    }
  }
  if (selected.length === 0) return

  // Reconcile durable rows: refresh metadata/plan, reset previously settled
  // rows — EXCEPT done rows whose source file is unchanged (idempotent re-run:
  // the repaired file is already on the phone, nothing to redo).
  const db = getDb()
  const upsert = db.prepare(
    `INSERT INTO phone_repair (device_id, phone_track_id, track_key, title, artist, album,
       status, action_transcode, action_enrich, attempts, next_retry_at, error, updated_at)
     VALUES (@device_id, @phone_track_id, @track_key, @title, @artist, @album,
       'pending', @action_transcode, @action_enrich, 0, NULL, NULL, @updated_at)
     ON CONFLICT(device_id, phone_track_id) DO UPDATE SET
       track_key = excluded.track_key,
       title = excluded.title, artist = excluded.artist, album = excluded.album,
       status = 'pending', action_transcode = excluded.action_transcode,
       action_enrich = excluded.action_enrich, attempts = 0, next_retry_at = NULL,
       error = NULL, updated_at = excluded.updated_at`
  )
  const jobIds: number[] = []
  for (const { info, plan } of selected) {
    const existing = db
      .prepare('SELECT * FROM phone_repair WHERE device_id = ? AND phone_track_id = ?')
      .get(peer.deviceId, info.id) as RepairRow | undefined
    if (
      existing &&
      existing.status === 'done' &&
      existing.src_size === info.fileSize &&
      existing.src_mtime === info.mtimeMs
    ) {
      continue
    }
    upsert.run({
      device_id: peer.deviceId,
      phone_track_id: info.id,
      track_key: info.trackKey,
      title: info.title,
      artist: info.artist,
      album: info.album,
      action_transcode: plan.transcode.needed ? 1 : 0,
      action_enrich: plan.enrich ? 1 : 0,
      updated_at: Date.now()
    })
    const row = db
      .prepare('SELECT * FROM phone_repair WHERE device_id = ? AND phone_track_id = ?')
      .get(peer.deviceId, info.id) as RepairRow
    broadcast('phoneRepair:updated', toItem(row))
    jobIds.push(row.id)
  }
  if (jobIds.length === 0) return

  await client.sessionStart().catch((err) => {
    throw err instanceof client.PhoneClientError && err.status === 409
      ? new Error('PHONE_BUSY')
      : err
  })

  running = true
  runAbort = new AbortController()
  const signal = runAbort.signal
  void runQueue(peer.deviceId, jobIds, byId, quality, signal).finally(() => {
    running = false
    runAbort = null
  })
}

async function runQueue(
  deviceId: string,
  jobIds: number[],
  infos: Map<number, PhoneTrackInfo>,
  quality: DownloadQuality,
  signal: AbortSignal
): Promise<void> {
  let done = 0
  const total = jobIds.length

  // The phone's FGS stays alive on this heartbeat; losing it for 60s ends the
  // session server-side, so it runs for the whole queue regardless of load.
  const beat = setInterval(() => {
    void client.sessionHeartbeat(done, total).catch(() => {})
  }, HEARTBEAT_MS)
  void client.sessionHeartbeat(0, total).catch(() => {})

  const queue = new PQueue({ concurrency: thermalManager.getConcurrency(BASE_CONCURRENCY) })
  const offThermal = thermalManager.onChange(() => {
    queue.concurrency = thermalManager.getConcurrency(BASE_CONCURRENCY)
  })

  try {
    await mkdir(stagingDir(deviceId), { recursive: true })
    await queue.addAll(
      jobIds.map((jobId) => async () => {
        if (signal.aborted) return
        try {
          await runJob(jobId, infos, quality, signal)
        } catch (err) {
          // runJob settles its own row; this catch only guards the queue.
          logWarn('phoneSync', `Job ${jobId} uscito con errore non gestito`, err)
        }
        done++
      })
    )
  } finally {
    offThermal()
    clearInterval(beat)
    await client.sessionEnd()
  }
}

// ---- single-job pipeline -------------------------------------------------

class JobFailure extends Error {
  constructor(
    message: string,
    readonly permanent: boolean
  ) {
    super(message)
  }
}

async function runJob(
  jobId: number,
  infos: Map<number, PhoneTrackInfo>,
  quality: DownloadQuality,
  signal: AbortSignal
): Promise<void> {
  const row = getRow(jobId)
  if (!row) return
  const info = infos.get(row.phone_track_id)
  if (!info) {
    patchRow(jobId, { status: 'failed', error: 'TRACK_GONE' })
    return
  }

  for (let attempt = 0; ; attempt++) {
    if (signal.aborted) {
      patchRow(jobId, { status: 'pending', error: 'ABORTED' })
      return
    }
    try {
      await attemptJob(jobId, info, quality, signal)
      return
    } catch (err) {
      if (signal.aborted || (err as Error).message === 'ABORTED') {
        patchRow(jobId, { status: 'pending', error: 'ABORTED' })
        return
      }
      const failure = err instanceof JobFailure ? err : new JobFailure(String((err as Error).message ?? err), false)
      const attempts = attempt + 1
      if (failure.permanent || attempts >= MAX_ATTEMPTS) {
        patchRow(jobId, { status: 'failed', attempts, error: failure.message.slice(0, 300) })
        return
      }
      patchRow(jobId, {
        status: 'pending',
        attempts,
        next_retry_at: Date.now() + RETRY_DELAY_MS[attempt],
        error: failure.message.slice(0, 300)
      })
      try {
        await sleep(RETRY_DELAY_MS[attempt], signal)
      } catch {
        patchRow(jobId, { status: 'pending', error: 'ABORTED' })
        return
      }
      // The phone may have changed address between attempts (WiFi blip).
      await client.connect().catch(() => {})
    }
  }
}

async function attemptJob(
  jobId: number,
  info: PhoneTrackInfo,
  quality: DownloadQuality,
  signal: AbortSignal
): Promise<void> {
  const row = getRow(jobId)
  if (!row) return
  const plan = planTrack(info, quality)
  const dir = stagingDir(row.device_id)
  const srcStage = join(dir, `${info.id}${info.ext || '.bin'}`)
  const cleanup: string[] = [srcStage]

  try {
    // ---- pull -----------------------------------------------------------
    patchRow(jobId, { status: 'pulling', error: null })
    const remote = await client.getHash(info.id)

    // A retry after a lost commit ack: the phone already carries our repaired
    // bytes — the job is done, don't repair the repaired file again.
    if (row.result_sha256 && remote.sha256 === row.result_sha256) {
      patchRow(jobId, {
        status: 'done',
        src_size: remote.size,
        src_mtime: remote.mtimeMs,
        src_sha256: remote.sha256
      })
      return
    }

    const part = `${srcStage}.part`
    cleanup.push(part)
    await client.downloadFile(info.id, part, signal)
    const pulledSha = await sha256File(part)
    if (pulledSha !== remote.sha256) {
      throw new JobFailure('PULL_HASH_MISMATCH', false)
    }
    await rename(part, srcStage)
    patchRow(jobId, {
      src_size: remote.size,
      src_mtime: remote.mtimeMs,
      src_sha256: remote.sha256
    })

    // ---- validate -------------------------------------------------------
    patchRow(jobId, { status: 'validating' })
    const check = await validateAudioFile(srcStage)
    if (!check.ok) {
      // Corrupt at the source: pushing anything back would only destroy what
      // little the phone still has. Terminal, never re-pushed.
      throw new JobFailure(`SRC_CORRUPT:${check.reason}`, true)
    }

    // Embedded cover snapshot BEFORE any processing: ffmpeg's -vn drops
    // pictures, and enrichment may not find a replacement.
    let srcCover: Buffer | null = null
    let srcDuration: number | null = info.durationS || null
    try {
      const meta = await parseFile(srcStage)
      const pics = meta.common.picture ?? []
      const pic = pics.find((p) => /front/i.test(p.type ?? '')) ?? pics[0]
      srcCover = pic ? Buffer.from(pic.data) : null
      srcDuration = meta.format.duration ?? srcDuration
    } catch {
      /* parse is best-effort; validateAudioFile already vouched for the file */
    }

    // ---- transcode ------------------------------------------------------
    let workPath = srcStage
    let workExt = info.ext
    let changed = false
    if (plan.transcode.needed) {
      patchRow(jobId, { status: 'transcoding' })
      const out = join(dir, `${info.id}.out${plan.transcode.targetExt}`)
      cleanup.push(out)
      await transcodeFile(srcStage, out, quality, signal)
      if (srcCover) {
        try {
          writeTags(out, {}, srcCover)
        } catch (err) {
          logWarn('phoneSync', `Re-embed cover fallito su ${out}`, err)
        }
      }
      workPath = out
      workExt = plan.transcode.targetExt
      changed = true
    }

    // ---- enrich ---------------------------------------------------------
    if (plan.enrich) {
      patchRow(jobId, { status: 'enriching' })
      const outcome = await enrichFile(
        {
          path: workPath,
          title: info.title,
          artist: info.artist,
          album: info.album || null,
          duration: srcDuration,
          year: info.year,
          mbRecordingId: null,
          hasCover: srcCover !== null || Boolean(info.hasCover && !plan.transcode.needed)
        },
        { storeCoverSidecar: false }
      )
      // no-match / needs-review / unavailable: proceed with what we have —
      // codec standardization alone is still worth pushing.
      if (outcome.status === 'applied') changed = true
    }

    if (!changed) {
      patchRow(jobId, { status: 'skipped', error: null })
      return
    }

    // ---- push -----------------------------------------------------------
    patchRow(jobId, { status: 'pushing' })
    const resultSha = await sha256File(workPath)
    const upload = await client.uploadFile(info.id, workPath, resultSha, workExt, signal)

    // ---- commit ---------------------------------------------------------
    patchRow(jobId, { status: 'committing', result_sha256: resultSha, result_ext: workExt })
    const committed = await client.commitTrack(info.id, {
      uploadId: upload.uploadId,
      sha256: resultSha,
      ext: workExt
    })
    if (!committed.ok) throw new JobFailure('COMMIT_REFUSED', false)
    patchRow(jobId, { status: 'done', error: null })
  } catch (err) {
    // Leftover staged upload on the phone from a failed commit: best-effort GC.
    if (getRow(jobId)?.status === 'committing') {
      void client.deleteUpload(info.id).catch(() => {})
    }
    throw err
  } finally {
    for (const path of cleanup) {
      await unlink(path).catch(() => {})
    }
  }
}
