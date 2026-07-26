import { handle } from './handle'
import {
  cancelSpotifyMigration,
  getSpotifyMigration,
  previewSpotifyMigration,
  startSpotifyMigration
} from '../modules/spotifyMigration'

export function registerSpotifyMigrationIpc(): void {
  handle('spotifyMigrationPreview', (_e, url: string) => previewSpotifyMigration(url))
  handle('spotifyMigrationStart', (_e, opts: { url: string; recreatePlaylist: boolean }) =>
    startSpotifyMigration(opts)
  )
  handle('spotifyMigrationCancel', () => {
    cancelSpotifyMigration()
  })
  handle('getSpotifyMigration', () => getSpotifyMigration())
}
