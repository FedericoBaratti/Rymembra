import { handle } from './handle'
import {
  lastfmStartAuth,
  lastfmCompleteAuth,
  lastfmDisconnect,
  submitScrobble,
  updateNowPlaying,
  getScrobbleStatus,
  flushPendingScrobbles
} from '../modules/scrobbler/service'

export function registerScrobbleIpc(): void {
  // drain anything left queued from the previous session
  flushPendingScrobbles()

  handle('lastfmStartAuth', () => lastfmStartAuth())
  handle('lastfmCompleteAuth', () => lastfmCompleteAuth())
  handle('lastfmDisconnect', () => lastfmDisconnect())
  handle('nowPlaying', (_e, trackId: number) => updateNowPlaying(trackId))
  handle('submitScrobble', (_e, trackId: number, playedSec: number, startedAtSec: number) =>
    submitScrobble(trackId, playedSec, startedAtSec)
  )
  handle('getScrobbleStatus', () => getScrobbleStatus())
}
