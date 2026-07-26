import { handle } from './handle'
import type { RadioSeed } from '@shared/types'
import {
  getSimilarTracksForTrack,
  getRadioSeedTracks,
  buildHomeFeed,
  searchExternalCatalog
} from '../modules/reco/service'
import { downloadExternalTrack, type ExternalTrackMeta } from '../modules/reco/externalDownload'

// Discovery / recommendation surface (Spotify-style Home + Radio). The heavy
// lifting lives in modules/reco; these handlers are thin pass-throughs. Likes
// and listening stats are in library.ipc.ts (they're plain DB queries).
export function registerDiscoveryIpc(): void {
  handle('getHomeFeed', () => buildHomeFeed())
  handle('getSimilarTracks', (_e, trackId: number, limit?: number) =>
    getSimilarTracksForTrack(trackId, limit)
  )
  handle('getRadioSeedTracks', (_e, seed: RadioSeed, limit?: number) =>
    getRadioSeedTracks(seed, limit)
  )
  handle('searchExternalCatalog', (_e, term: string) => searchExternalCatalog(term))
  // Resolve an external recommendation → YouTube → existing download queue.
  handle('downloadExternalTrack', (_e, meta: ExternalTrackMeta) => downloadExternalTrack(meta))
}
