import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { writeTags, verifyTags } from './tagIO'

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
  it('round-trips a full update', () => {
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
    writeTags(file, update, null)
    expect(verifyTags(file, update)).toEqual([])
  })

  it('applies a partial update without touching other fields', () => {
    writeTags(file, { title: 'Prima', artist: 'Uno' }, null)
    writeTags(file, { title: 'Dopo' }, null)
    expect(verifyTags(file, { title: 'Dopo', artist: 'Uno' })).toEqual([])
  })

  it('treats null as unset (empty string / zero)', () => {
    writeTags(file, { title: 'X', year: 2020, genre: 'Rock' }, null)
    writeTags(file, { year: null, genre: undefined, comment: '' }, null)
    expect(verifyTags(file, { title: 'X', year: null, genre: 'Rock', comment: '' })).toEqual([])
  })

  it('reports mismatching fields by name', () => {
    writeTags(file, { title: 'Reale', year: 2020 }, null)
    const mismatches = verifyTags(file, { title: 'Atteso', year: 1999, artist: '' })
    expect(mismatches.sort()).toEqual(['title', 'year'])
  })

  it('embeds a front cover without corrupting the file', () => {
    // 1x1 red pixel PNG
    const png = Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
      'base64'
    )
    writeTags(file, { title: 'Con cover' }, png)
    expect(verifyTags(file, { title: 'Con cover' })).toEqual([])
  })
})
