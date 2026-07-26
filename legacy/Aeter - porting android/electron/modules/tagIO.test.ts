import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { copyFileSync, mkdtempSync, rmSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { writeTags, verifyTags, setTagWriteBack } from './tagIO'

const FIXTURE = join(import.meta.dirname, '__fixtures__', 'silence.mp3')

let dir: string
let file: string

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'aether-tagio-'))
  file = join(dir, 'track.mp3')
  copyFileSync(FIXTURE, file)
})

afterEach(() => {
  rmSync(dir, { recursive: true, force: true })
})

describe('writeTags / verifyTags', () => {
  it('round-trips a full update', async () => {
    const update = {
      title: 'Notte Stellata',
      artist: 'Ludovico',
      album: 'Cieli',
      album_artist: 'Vari',
      year: 2024,
      track_number: 3,
      disc_number: 1,
      genre: 'Classica',
      bpm: 92,
      comment: 'test',
      lyrics: 'la la la'
    }
    await writeTags(file, update, null)
    expect(verifyTags(file, update)).toEqual([])
  })

  it('applies a partial update without touching other fields', async () => {
    await writeTags(file, { title: 'Prima', artist: 'Uno' }, null)
    await writeTags(file, { title: 'Dopo' }, null)
    expect(verifyTags(file, { title: 'Dopo', artist: 'Uno' })).toEqual([])
  })

  it('treats null as unset (empty string / zero)', async () => {
    await writeTags(file, { title: 'X', year: 2020, genre: 'Rock' }, null)
    await writeTags(file, { year: null, genre: undefined, comment: '' }, null)
    expect(verifyTags(file, { title: 'X', year: null, genre: 'Rock', comment: '' })).toEqual([])
  })

  it('reports mismatching fields by name', async () => {
    await writeTags(file, { title: 'Reale', year: 2020 }, null)
    const mismatches = verifyTags(file, { title: 'Atteso', year: 1999, artist: '' })
    expect(mismatches.sort()).toEqual(['title', 'year'])
  })

  it('embeds a front cover without corrupting the file', async () => {
    // 1x1 red pixel PNG
    const png = Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
      'base64'
    )
    await writeTags(file, { title: 'Con cover' }, png)
    expect(verifyTags(file, { title: 'Con cover' })).toEqual([])
  })
})

describe('writeTags Android SAF write-back seam', () => {
  afterEach(() => setTagWriteBack(null))

  it('edits a private temp copy and hands it to the write-back', async () => {
    const calls: Array<{ original: string; temp: string }> = []
    let tempExistedDuringCallback = false
    // Simulate the native SAF copy-back (temp -> original).
    setTagWriteBack(async (original, temp) => {
      tempExistedDuringCallback = existsSync(temp)
      calls.push({ original, temp })
      copyFileSync(temp, original)
    }, dir)

    await writeTags(file, { title: 'Via SAF' }, null)

    expect(calls).toHaveLength(1)
    expect(calls[0].original).toBe(file)
    expect(calls[0].temp).not.toBe(file)
    expect(tempExistedDuringCallback).toBe(true)
    // temp is cleaned up afterwards
    expect(existsSync(calls[0].temp)).toBe(false)
    // and the original ended up with the new tag (via the simulated SAF copy)
    expect(verifyTags(file, { title: 'Via SAF' })).toEqual([])
  })

  it('cleans up the temp file even when the write-back throws', async () => {
    let tempPath = ''
    setTagWriteBack(async (_original, temp) => {
      tempPath = temp
      throw new Error('saf-write-failed')
    }, dir)

    await expect(writeTags(file, { title: 'Boom' }, null)).rejects.toThrow('saf-write-failed')
    expect(tempPath).not.toBe('')
    expect(existsSync(tempPath)).toBe(false)
  })
})
