import { createHash } from 'node:crypto'
import { createReadStream } from 'node:fs'
import { copyFile, mkdir, rename, stat, unlink } from 'node:fs/promises'
import { basename, dirname, extname, join } from 'node:path'
import { getDb } from '../../electron/modules/db'
import {
  TRASH_DIR_NAME,
  rebuildAggregates,
  upsertTrackFromFile
} from '../../electron/modules/library'
import { broadcast } from '../../electron/modules/events'
import { logWarn } from '../../electron/modules/logger'
import { callNative } from '../runtime'

/**
 * Final step of a phone repair: replace the on-device audio file with the
 * fixed version the desktop uploaded, WITHOUT losing the track's identity.
 * The tracks row is updated in place (same id), so playlists/liked/stats all
 * survive; the original file is parked in `.trash/` (excluded from scan and
 * watcher via TRASH_DIR_NAME) as a safety net instead of being deleted.
 *
 * Storage: on the All-Files-Access norm the external path is directly
 * writable from node. When it is not (scoped storage with only a SAF tree
 * grant), fall back to the FileAccess plugin: same-extension replaces reuse
 * saveFileViaSaf (in-place truncate+write), extension changes need
 * importFileViaSaf (create the new document) + deleteFile (drop the old one)
 * — no `.trash` parking in SAF mode.
 */

export interface CommitResult {
  trackId: number
  path: string
  changedExt: boolean
}

/** Streaming sha256 (files can be hundreds of MB — never readFileSync them). */
export function sha256File(path: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const hash = createHash('sha256')
    const stream = createReadStream(path)
    stream.on('error', reject)
    stream.on('data', (chunk) => hash.update(chunk))
    stream.on('end', () => resolve(hash.digest('hex')))
  })
}

async function exists(path: string): Promise<boolean> {
  try {
    await stat(path)
    return true
  } catch {
    return false
  }
}

/** EACCES/EPERM → the path needs the SAF fallback; anything else is a real error. */
function isPermissionError(err: unknown): boolean {
  const code = (err as NodeJS.ErrnoException | null)?.code
  return code === 'EACCES' || code === 'EPERM'
}

// One commit at a time: two concurrent commits on the same directory could
// race on `.trash` names and on rebuildAggregates. Serialized via chain.
let commitChain: Promise<unknown> = Promise.resolve()

export function commitReplace(
  trackId: number,
  uploadPath: string,
  ext: string,
  sha256: string
): Promise<CommitResult> {
  const run = commitChain.then(() => doCommit(trackId, uploadPath, ext, sha256))
  commitChain = run.catch(() => {})
  return run
}

async function doCommit(
  trackId: number,
  uploadPath: string,
  ext: string,
  sha256: string
): Promise<CommitResult> {
  const row = getDb().prepare('SELECT id, path FROM tracks WHERE id = ?').get(trackId) as
    | { id: number; path: string }
    | undefined
  if (!row) throw new Error('TRACK_NOT_FOUND')
  if (!(await exists(uploadPath))) throw new Error('UPLOAD_NOT_FOUND')

  // Integrity gate: the bytes about to overwrite a user file must be exactly
  // the ones the desktop hashed after processing (upload already checked them,
  // but the .part file sat in cache between the two requests).
  const actual = await sha256File(uploadPath)
  if (actual !== sha256) throw new Error('HASH_MISMATCH')

  const oldPath = row.path
  const dir = dirname(oldPath)
  const oldBase = basename(oldPath)
  const stem = oldBase.slice(0, oldBase.length - extname(oldBase).length)
  const newExt = ext.startsWith('.') ? ext.toLowerCase() : `.${ext.toLowerCase()}`
  let targetPath = join(dir, `${stem}${newExt}`)
  const changedExt = targetPath !== oldPath
  if (changedExt && (await exists(targetPath))) {
    // Another file already owns the new name (e.g. both song.opus and song.mp3
    // existed): never overwrite it — suffix ours.
    targetPath = join(dir, `${stem} (fixed)${newExt}`)
  }

  try {
    await commitDirect(oldPath, targetPath, uploadPath)
  } catch (err) {
    if (!isPermissionError(err)) throw err
    await commitViaSaf(oldPath, targetPath, uploadPath, changedExt)
  }

  // Same row, new path: id survives, so playlist_tracks/liked/play stats keep
  // pointing at this track. date_modified=0 forces upsertTrackFromFile past its
  // unchanged-mtime fast path even if the new file's mtime collides.
  getDb()
    .prepare('UPDATE tracks SET path = ?, date_modified = 0 WHERE id = ?')
    .run(targetPath, trackId)
  await upsertTrackFromFile(targetPath)
  rebuildAggregates()
  broadcast('library:changed', { reason: 'phone-repair' })

  await unlink(uploadPath).catch(() => {})
  return { trackId, path: targetPath, changedExt }
}

/** Direct-fs commit (All Files Access): trash the original, land the new file. */
async function commitDirect(
  oldPath: string,
  targetPath: string,
  uploadPath: string
): Promise<void> {
  const dir = dirname(oldPath)
  const trashDir = join(dir, TRASH_DIR_NAME)
  await mkdir(trashDir, { recursive: true })
  const trashPath = join(trashDir, `${basename(oldPath)}.${Date.now()}`)
  // Same directory → rename is atomic and cheap; keep a copy fallback for
  // exotic mounts where cross-link rename fails.
  try {
    await rename(oldPath, trashPath)
  } catch {
    await copyFile(oldPath, trashPath)
    await unlink(oldPath)
  }
  try {
    // cacheDir → external storage is cross-device: copy+unlink, never rename.
    await copyFile(uploadPath, targetPath)
  } catch (err) {
    // The original is already in .trash — put it back before failing, or the
    // user would lose the track entirely.
    await rename(trashPath, oldPath).catch((restoreErr) =>
      logWarn('transfer', `Ripristino da .trash fallito: ${oldPath}`, restoreErr)
    )
    throw err
  }
}

/** SAF fallback (no All Files Access): plugin-mediated write, no trash net. */
async function commitViaSaf(
  oldPath: string,
  targetPath: string,
  uploadPath: string,
  changedExt: boolean
): Promise<void> {
  if (!changedExt) {
    // Same name → in-place truncate+write keeps the document identity.
    await callNative('saveFileViaSaf', { originalPath: oldPath, tempPath: uploadPath }, 60000)
    return
  }
  // New extension → create the new document, then drop the old one.
  await callNative(
    'importFileViaSaf',
    { folderPath: dirname(targetPath), fileName: basename(targetPath), tempPath: uploadPath },
    60000
  )
  await callNative('deleteFile', { path: oldPath }, 15000)
}
