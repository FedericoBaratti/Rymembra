import { handle } from './handle'
import type { EnrichmentBucket, TrackMetadataUpdate } from '@shared/types'
import {
  updateTrackMetadata,
  updateTracksMetadata,
  enrichTrack,
  getEnrichmentStats,
  getEnrichmentTracks,
  retryFailedEnrichment,
  backfillCovers,
  recheckCovers,
  findDuplicates,
  mergeDuplicates,
  deleteTracks,
  getLyrics,
  saveLyrics
} from '../modules/metadata'
import { rebuildAggregates } from '../modules/library'

export function registerMetadataIpc(): void {
  handle('updateTrackMetadata', (_e, trackId: number, update: TrackMetadataUpdate) =>
    updateTrackMetadata(trackId, update)
  )
  handle('updateTracksMetadata', (_e, trackIds: number[], update: TrackMetadataUpdate) =>
    updateTracksMetadata(Array.isArray(trackIds) ? trackIds : [], update)
  )
  // Manual re-enrich is an explicit user action, so allow replacing a cover
  // that may have been fetched from the wrong release.
  handle('enrichTrack', async (_e, trackId: number) => {
    const res = await enrichTrack(trackId, { replaceCover: true })
    // Enrichment may have changed the album (album_key); rebuild the materialized
    // albums table so the track lands under the right card without a full rescan.
    if (res.applied) rebuildAggregates()
    return res
  })
  handle('getEnrichmentStats', () => getEnrichmentStats())
  handle('getEnrichmentTracks', (_e, bucket: EnrichmentBucket, offset?: number, limit?: number) =>
    getEnrichmentTracks(bucket, offset, limit)
  )
  handle('retryFailedEnrichment', () => retryFailedEnrichment())
  handle('backfillCovers', () => backfillCovers())
  handle('recheckCovers', () => recheckCovers())
  handle('findDuplicates', () => findDuplicates())
  handle('mergeDuplicates', (_e, survivorId: number, victimIds: number[], deleteFiles: boolean) =>
    mergeDuplicates(survivorId, Array.isArray(victimIds) ? victimIds : [], deleteFiles)
  )
  handle('deleteTracks', (_e, trackIds: number[], deleteFiles: boolean) =>
    deleteTracks(trackIds, deleteFiles)
  )
  handle('getLyrics', (_e, trackId: number) => getLyrics(trackId))
  handle('refetchLyrics', (_e, trackId: number) => getLyrics(trackId, true))
  handle('saveLyrics', (_e, trackId: number, lyrics: string) => saveLyrics(trackId, lyrics))
}
