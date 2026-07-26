import { File as TagFile, Picture, PictureType, ByteVector } from 'node-taglib-sharp'
import type { TrackMetadataUpdate } from '@shared/types'

// File-tag I/O shared by manual editing (metadata.ts) and the enrichment
// pipeline (enrichment/pipeline.ts). Kept apart to avoid import cycles.

export function writeTags(
  path: string,
  update: TrackMetadataUpdate,
  coverBuffer: Buffer | null
): void {
  const file = TagFile.createFromPath(path)
  try {
    const tag = file.tag
    if (update.title !== undefined) tag.title = update.title ?? ''
    if (update.artist !== undefined) tag.performers = update.artist ? [update.artist] : []
    if (update.album !== undefined) tag.album = update.album ?? ''
    if (update.album_artist !== undefined)
      tag.albumArtists = update.album_artist ? [update.album_artist] : []
    if (update.year !== undefined) tag.year = update.year ?? 0
    if (update.track_number !== undefined) tag.track = update.track_number ?? 0
    if (update.disc_number !== undefined) tag.disc = update.disc_number ?? 0
    if (update.genre !== undefined) tag.genres = update.genre ? [update.genre] : []
    if (update.bpm !== undefined) tag.beatsPerMinute = Math.round(update.bpm ?? 0)
    if (update.comment !== undefined) tag.comment = update.comment ?? ''
    if (update.lyrics !== undefined) tag.lyrics = update.lyrics ?? ''
    if (coverBuffer) {
      const pic = Picture.fromData(ByteVector.fromByteArray(coverBuffer))
      pic.type = PictureType.FrontCover
      tag.pictures = [pic]
    }
    file.save()
  } finally {
    file.dispose()
  }
}

/**
 * Re-reads the file after a save and checks that the requested fields
 * round-tripped. Returns the names of fields that did not. Empty string and
 * 0 are normalized to "unset", mirroring writeTags.
 */
export function verifyTags(path: string, expected: TrackMetadataUpdate): string[] {
  const file = TagFile.createFromPath(path)
  try {
    const tag = file.tag
    const mismatches: string[] = []
    const str = (v: string | null | undefined): string => v ?? ''
    const num = (v: number | null | undefined): number => v ?? 0

    if (expected.title !== undefined && str(expected.title) !== str(tag.title))
      mismatches.push('title')
    if (expected.artist !== undefined && str(expected.artist) !== str(tag.performers?.[0]))
      mismatches.push('artist')
    if (expected.album !== undefined && str(expected.album) !== str(tag.album))
      mismatches.push('album')
    if (
      expected.album_artist !== undefined &&
      str(expected.album_artist) !== str(tag.albumArtists?.[0])
    )
      mismatches.push('album_artist')
    if (expected.year !== undefined && num(expected.year) !== num(tag.year)) mismatches.push('year')
    if (expected.track_number !== undefined && num(expected.track_number) !== num(tag.track))
      mismatches.push('track_number')
    if (expected.disc_number !== undefined && num(expected.disc_number) !== num(tag.disc))
      mismatches.push('disc_number')
    if (expected.genre !== undefined && str(expected.genre) !== str(tag.genres?.[0]))
      mismatches.push('genre')
    if (
      expected.bpm !== undefined &&
      Math.round(num(expected.bpm)) !== Math.round(num(tag.beatsPerMinute))
    )
      mismatches.push('bpm')
    if (expected.comment !== undefined && str(expected.comment) !== str(tag.comment))
      mismatches.push('comment')
    if (expected.lyrics !== undefined && str(expected.lyrics) !== str(tag.lyrics))
      mismatches.push('lyrics')
    return mismatches
  } finally {
    file.dispose()
  }
}
