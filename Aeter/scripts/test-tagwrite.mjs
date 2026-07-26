// Verifies the M6 tag-writing path: node-taglib-sharp write → music-metadata re-read.
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir, homedir } from 'node:os'
import { createRequire } from 'node:module'
import { parseFile } from 'music-metadata'

const require = createRequire(import.meta.url)
const { File: TagFile, Picture, ByteVector, PictureType } = require('node-taglib-sharp')

const src = join(homedir(), 'Music', 'AetherTest', 'unknown_song.flac')
const dir = mkdtempSync(join(tmpdir(), 'aether-tag-'))
const dst = join(dir, 'test.flac')
copyFileSync(src, dst)

// fake 1x1 png cover
const png = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  'base64'
)

const f = TagFile.createFromPath(dst)
f.tag.title = 'Titolo Test'
f.tag.performers = ['Artista Test']
f.tag.albumArtists = ['Artista Test']
f.tag.album = 'Album Test'
f.tag.year = 2024
f.tag.track = 7
f.tag.genres = ['Ambient']
const pic = Picture.fromData(ByteVector.fromByteArray(png))
pic.type = PictureType.FrontCover
f.tag.pictures = [pic]
f.save()
f.dispose()

const meta = await parseFile(dst)
console.log(
  JSON.stringify(
    {
      title: meta.common.title,
      artist: meta.common.artist,
      album: meta.common.album,
      year: meta.common.year,
      track: meta.common.track.no,
      genre: meta.common.genre,
      pictures: meta.common.picture?.length ?? 0
    },
    null,
    1
  )
)
rmSync(dir, { recursive: true, force: true })
