import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// commitReplace choreography on a real temp directory: id survival (same DB
// row), ext change, collision suffix, .trash parking, restore-on-failure and
// the SAF fallback. The DB is a fixture map (only the two statements commit.ts
// runs), the library side-effects are spies.

const dbState = vi.hoisted(() => ({
  tracks: new Map<number, { id: number; path: string }>()
}))
const fsState = vi.hoisted(() => ({
  failCopyTo: null as string | null,
  failCode: 'EIO'
}))

vi.mock('../../electron/modules/db', () => ({
  getDb: () => ({
    prepare: (sql: string) =>
      sql.startsWith('SELECT')
        ? { get: (id: number) => dbState.tracks.get(id) }
        : {
            run: (path: string, id: number) => {
              const t = dbState.tracks.get(id)
              if (t) t.path = path
            }
          }
  })
}))
vi.mock('../../electron/modules/library', () => ({
  TRASH_DIR_NAME: '.trash',
  upsertTrackFromFile: vi.fn(async () => {}),
  rebuildAggregates: vi.fn()
}))
vi.mock('../../electron/modules/events', () => ({ broadcast: vi.fn() }))
vi.mock('../../electron/modules/logger', () => ({ logWarn: vi.fn() }))
vi.mock('../runtime', () => ({ callNative: vi.fn(async () => ({})) }))
vi.mock('node:fs/promises', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:fs/promises')>()
  return {
    ...actual,
    copyFile: async (src: string, dest: string) => {
      if (fsState.failCopyTo && dest === fsState.failCopyTo) {
        const err = new Error(`${fsState.failCode}: injected copy failure`) as NodeJS.ErrnoException
        err.code = fsState.failCode
        throw err
      }
      return actual.copyFile(src, dest)
    }
  }
})

import { commitReplace } from './commit'
import { upsertTrackFromFile, rebuildAggregates } from '../../electron/modules/library'
import { broadcast } from '../../electron/modules/events'
import { callNative } from '../runtime'

let musicDir: string
let cacheDir: string

beforeEach(() => {
  musicDir = mkdtempSync(join(tmpdir(), 'aether-commit-music-'))
  cacheDir = mkdtempSync(join(tmpdir(), 'aether-commit-cache-'))
  dbState.tracks.clear()
  fsState.failCopyTo = null
  fsState.failCode = 'EIO'
  vi.clearAllMocks()
})

afterEach(() => {
  rmSync(musicDir, { recursive: true, force: true })
  rmSync(cacheDir, { recursive: true, force: true })
})

function seedTrack(id: number, name: string, content: string): string {
  const path = join(musicDir, name)
  writeFileSync(path, content)
  dbState.tracks.set(id, { id, path })
  return path
}

function stageUpload(content: string): { uploadPath: string; sha: string } {
  const uploadPath = join(cacheDir, `upl-1-${Date.now()}.bin`)
  writeFileSync(uploadPath, content)
  return { uploadPath, sha: createHash('sha256').update(content).digest('hex') }
}

function trashEntries(): string[] {
  const dir = join(musicDir, '.trash')
  return existsSync(dir) ? readdirSync(dir) : []
}

describe('commitReplace', () => {
  it('replaces the file in place (same ext), parks the original in .trash and keeps the row id', async () => {
    const oldPath = seedTrack(1, 'song.mp3', 'OLD-BYTES')
    const { uploadPath, sha } = stageUpload('NEW-BYTES')

    const result = await commitReplace(1, uploadPath, '.mp3', sha)

    expect(result).toEqual({ trackId: 1, path: oldPath, changedExt: false })
    expect(readFileSync(oldPath, 'utf-8')).toBe('NEW-BYTES')
    // Same row, same id — only the path column may change (here it doesn't).
    expect(dbState.tracks.get(1)!.path).toBe(oldPath)
    const trash = trashEntries()
    expect(trash).toHaveLength(1)
    expect(trash[0].startsWith('song.mp3.')).toBe(true)
    expect(readFileSync(join(musicDir, '.trash', trash[0]), 'utf-8')).toBe('OLD-BYTES')
    expect(upsertTrackFromFile).toHaveBeenCalledWith(oldPath)
    expect(rebuildAggregates).toHaveBeenCalled()
    expect(broadcast).toHaveBeenCalledWith('library:changed', { reason: 'phone-repair' })
    // The staged upload is consumed.
    expect(existsSync(uploadPath)).toBe(false)
  })

  it('lands a changed extension under the same stem and updates the row path', async () => {
    const oldPath = seedTrack(1, 'song.opus', 'OPUS-BYTES')
    const { uploadPath, sha } = stageUpload('MP3-BYTES')

    const result = await commitReplace(1, uploadPath, '.mp3', sha)

    const newPath = join(musicDir, 'song.mp3')
    expect(result).toEqual({ trackId: 1, path: newPath, changedExt: true })
    expect(readFileSync(newPath, 'utf-8')).toBe('MP3-BYTES')
    expect(existsSync(oldPath)).toBe(false)
    expect(dbState.tracks.get(1)!.path).toBe(newPath)
    expect(trashEntries()[0].startsWith('song.opus.')).toBe(true)
  })

  it('suffixes the target when another file already owns the new name', async () => {
    seedTrack(1, 'song.opus', 'OPUS-BYTES')
    const squatter = join(musicDir, 'song.mp3')
    writeFileSync(squatter, 'SOMEONE-ELSE')
    const { uploadPath, sha } = stageUpload('MP3-BYTES')

    const result = await commitReplace(1, uploadPath, '.mp3', sha)

    expect(result.path).toBe(join(musicDir, 'song (fixed).mp3'))
    expect(readFileSync(result.path, 'utf-8')).toBe('MP3-BYTES')
    // The squatter is never overwritten.
    expect(readFileSync(squatter, 'utf-8')).toBe('SOMEONE-ELSE')
  })

  it('refuses a hash mismatch before touching anything', async () => {
    const oldPath = seedTrack(1, 'song.mp3', 'OLD-BYTES')
    const { uploadPath } = stageUpload('NEW-BYTES')

    await expect(commitReplace(1, uploadPath, '.mp3', 'ab'.repeat(32))).rejects.toThrow(
      'HASH_MISMATCH'
    )
    expect(readFileSync(oldPath, 'utf-8')).toBe('OLD-BYTES')
    expect(trashEntries()).toHaveLength(0)
    expect(upsertTrackFromFile).not.toHaveBeenCalled()
  })

  it('restores the original from .trash when landing the new file fails', async () => {
    const oldPath = seedTrack(1, 'song.mp3', 'OLD-BYTES')
    const { uploadPath, sha } = stageUpload('NEW-BYTES')
    fsState.failCopyTo = oldPath // same-ext target IS the old path

    await expect(commitReplace(1, uploadPath, '.mp3', sha)).rejects.toThrow('EIO')
    // The user's file is back where it was, intact.
    expect(readFileSync(oldPath, 'utf-8')).toBe('OLD-BYTES')
    expect(dbState.tracks.get(1)!.path).toBe(oldPath)
    expect(upsertTrackFromFile).not.toHaveBeenCalled()
  })

  it('falls back to saveFileViaSaf when direct write is denied and the ext is unchanged', async () => {
    const oldPath = seedTrack(1, 'song.mp3', 'OLD-BYTES')
    const { uploadPath, sha } = stageUpload('NEW-BYTES')
    fsState.failCopyTo = oldPath
    fsState.failCode = 'EACCES'

    const result = await commitReplace(1, uploadPath, '.mp3', sha)

    expect(result.changedExt).toBe(false)
    expect(callNative).toHaveBeenCalledWith(
      'saveFileViaSaf',
      { originalPath: oldPath, tempPath: uploadPath },
      60000
    )
    expect(upsertTrackFromFile).toHaveBeenCalledWith(oldPath)
  })

  it('falls back to importFileViaSaf + deleteFile when direct write is denied and the ext changes', async () => {
    const oldPath = seedTrack(1, 'song.opus', 'OPUS-BYTES')
    const newPath = join(musicDir, 'song.mp3')
    const { uploadPath, sha } = stageUpload('MP3-BYTES')
    fsState.failCopyTo = newPath
    fsState.failCode = 'EPERM'

    const result = await commitReplace(1, uploadPath, '.mp3', sha)

    expect(result).toMatchObject({ path: newPath, changedExt: true })
    expect(callNative).toHaveBeenCalledWith(
      'importFileViaSaf',
      { folderPath: musicDir, fileName: 'song.mp3', tempPath: uploadPath },
      60000
    )
    expect(callNative).toHaveBeenCalledWith('deleteFile', { path: oldPath }, 15000)
    expect(dbState.tracks.get(1)!.path).toBe(newPath)
  })

  it('rejects unknown tracks and missing uploads', async () => {
    await expect(commitReplace(99, join(cacheDir, 'nope.bin'), '.mp3', 'ab'.repeat(32)))
      .rejects.toThrow('TRACK_NOT_FOUND')

    seedTrack(1, 'song.mp3', 'OLD-BYTES')
    await expect(commitReplace(1, join(cacheDir, 'nope.bin'), '.mp3', 'ab'.repeat(32)))
      .rejects.toThrow('UPLOAD_NOT_FOUND')
  })
})
