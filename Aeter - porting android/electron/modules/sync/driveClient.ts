/**
 * Minimal Google Drive REST v3 client for the sync file, living entirely inside
 * the app's private `appDataFolder`. Transfers are gzipped JSON. Network calls
 * go through `withRetry` (network / 5xx / 429 are retried; 4xx are not). A
 * corrupt download surfaces as {@link DriveCorruptError} so the caller can move
 * the bad file aside and recreate it rather than trusting garbage.
 */
import { gzipSync, gunzipSync } from 'node:zlib'
import { randomBytes } from 'node:crypto'
import { withRetry } from '../net/retry'
import { HttpError, RateLimitError } from '../net/errors'
import { parseRetryAfter, fetchWithTimeout } from '../net/http'
import { SYNC_FILE_NAME } from './schema'

const DRIVE_FILES = 'https://www.googleapis.com/drive/v3/files'
const DRIVE_UPLOAD = 'https://www.googleapis.com/upload/drive/v3/files'

export interface DriveFileMeta {
  id: string
  modifiedTime: string
  md5Checksum?: string
  size?: string
}

/** The remote file was present but not decodable (bad gzip or JSON). Not
 *  retryable — the caller renames it aside and recreates from local. */
export class DriveCorruptError extends Error {
  constructor(
    message: string,
    readonly cause?: unknown
  ) {
    super(message)
    this.name = 'DriveCorruptError'
  }
}

/** fetch + auth header + status classification into the shared error taxonomy. */
async function driveFetch(url: string, token: string, init: RequestInit = {}): Promise<Response> {
  // Long timeout: uploads/downloads of a large gzipped library can legitimately
  // take a while on slow links, but a dead socket must still surface.
  const res = await fetchWithTimeout(url, {
    timeoutMs: 120_000,
    init: {
      ...init,
      headers: { Authorization: `Bearer ${token}`, ...(init.headers ?? {}) }
    }
  })
  if (res.status === 429) {
    throw new RateLimitError(url, parseRetryAfter(res.headers.get('retry-after')))
  }
  if (!res.ok) {
    let body: string | undefined
    try {
      body = (await res.text()).slice(0, 300)
    } catch {
      /* body unavailable */
    }
    throw new HttpError(res.status, url, body)
  }
  return res
}

/** Locate the sync file in appDataFolder, or null if it does not exist yet. */
export function findFile(token: string): Promise<DriveFileMeta | null> {
  const params = new URLSearchParams({
    spaces: 'appDataFolder',
    q: `name = '${SYNC_FILE_NAME}' and trashed = false`,
    fields: 'files(id,name,modifiedTime,md5Checksum,size)',
    pageSize: '10'
  })
  return withRetry(async () => {
    const res = await driveFetch(`${DRIVE_FILES}?${params.toString()}`, token)
    const json = (await res.json()) as { files?: DriveFileMeta[] }
    const files = json.files ?? []
    return files.length > 0 ? files[0] : null
  })
}

/** Download + gunzip + JSON.parse. Throws {@link DriveCorruptError} on bad data. */
export function downloadJson(token: string, fileId: string): Promise<unknown> {
  return withRetry(async () => {
    const res = await driveFetch(`${DRIVE_FILES}/${fileId}?alt=media`, token)
    const buf = Buffer.from(await res.arrayBuffer())
    let text: string
    try {
      text = gunzipSync(buf).toString('utf-8')
    } catch (err) {
      throw new DriveCorruptError('gunzip failed', err)
    }
    try {
      return JSON.parse(text)
    } catch (err) {
      throw new DriveCorruptError('JSON parse failed', err)
    }
  })
}

export interface DriveUploadResult {
  id: string
  md5Checksum: string | null
}

/** Upload the value as gzipped JSON. Creates the file when `fileId` is null
 *  (multipart), otherwise replaces its content. Returns the id + md5. */
export function uploadJson(
  token: string,
  fileId: string | null,
  value: unknown
): Promise<DriveUploadResult> {
  const gz = gzipSync(Buffer.from(JSON.stringify(value), 'utf-8'))

  if (fileId) {
    return withRetry(async () => {
      const res = await driveFetch(
        `${DRIVE_UPLOAD}/${fileId}?uploadType=media&fields=id,md5Checksum`,
        token,
        {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/gzip' },
          body: gz as unknown as RequestInit['body']
        }
      )
      const json = (await res.json()) as { id?: string; md5Checksum?: string }
      return { id: json.id ?? fileId, md5Checksum: json.md5Checksum ?? null }
    })
  }

  return withRetry(async () => {
    const boundary = `aether-${randomBytes(8).toString('hex')}`
    const metadata = JSON.stringify({ name: SYNC_FILE_NAME, parents: ['appDataFolder'] })
    const body = Buffer.concat([
      Buffer.from(
        `--${boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n${metadata}\r\n`,
        'utf-8'
      ),
      Buffer.from(`--${boundary}\r\nContent-Type: application/gzip\r\n\r\n`, 'utf-8'),
      gz,
      Buffer.from(`\r\n--${boundary}--\r\n`, 'utf-8')
    ])
    const res = await driveFetch(
      `${DRIVE_UPLOAD}?uploadType=multipart&fields=id,md5Checksum`,
      token,
      {
        method: 'POST',
        headers: { 'Content-Type': `multipart/related; boundary=${boundary}` },
        body: body as unknown as RequestInit['body']
      }
    )
    const json = (await res.json()) as { id?: string; md5Checksum?: string }
    if (!json.id) throw new Error('DRIVE_UPLOAD_NO_ID')
    return { id: json.id, md5Checksum: json.md5Checksum ?? null }
  })
}

/** Rename a Drive file (used to move a corrupt sync file aside). */
export function renameFile(token: string, fileId: string, name: string): Promise<void> {
  return withRetry(async () => {
    await driveFetch(`${DRIVE_FILES}/${fileId}`, token, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name })
    })
  })
}
