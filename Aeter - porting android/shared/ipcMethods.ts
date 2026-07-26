/**
 * Single source of truth for the `window.aether.<method>` invoke surface.
 *
 * This list used to be duplicated by hand in TWO places — electron/preload.ts
 * (desktop, ipcRenderer.invoke) and src/lib/bridge.ts (Android, nodejs-mobile
 * JSON bridge). When a new IPC method was added to one and not the other, it was
 * silently `undefined` on the platform that wasn't updated (e.g. the Spotify
 * migration methods → `spotifyMigrationPreview is not a function` on device).
 *
 * Both bridges now import this array, so a new IPC method is registered in ONE
 * place. Backend → renderer events (`on(...)`) and the reverse-RPC channel
 * (`nrpc`/`nres`, src/lib/nativeRpc.ts) are NOT in this list — only methods the
 * renderer invokes on the backend.
 */
export const INVOKE_METHODS = [
  'getTracks', 'getTrackCount', 'getTrackById', 'getTracksByIds', 'getAlbums', 'getAlbumTracks',
  'repairSplitAlbums',
  'getArtists', 'getArtistAlbums', 'getLibraryStats', 'search', 'rescanLibrary', 'flushNow', 'setAppState',
  'recordPlay', 'setRating', 'showInFolder',
  'getHomeFeed', 'getSimilarTracks', 'getRadioSeedTracks', 'searchExternalCatalog', 'downloadExternalTrack', 'setLiked', 'getLikedTracks', 'getListeningStats',
  'searchPodcasts', 'addPodcast', 'removePodcast', 'refreshPodcast', 'getPodcasts', 'getPodcastEpisodes', 'getLatestEpisodes', 'setEpisodeProgress',
  'getWaveform', 'saveWaveform',
  'getQueueState', 'saveQueueState',
  'getPlaylists', 'getPlaylistTracks', 'createPlaylist', 'renamePlaylist',
  'deletePlaylist', 'addToPlaylist', 'removeFromPlaylist', 'reorderPlaylist',
  'createSmartPlaylist', 'setSmartPlaylistRules', 'previewSmartPlaylist',
  'previewDownload', 'startDownload', 'cancelDownload', 'pauseDownload', 'resumeDownload', 'retryDownload',
  'clearFinishedDownloads', 'getDownloads', 'updateYtDlp', 'getBinaryStatus',
  'spotifyMigrationPreview', 'spotifyMigrationStart', 'spotifyMigrationCancel', 'getSpotifyMigration',
  'updateTrackMetadata', 'updateTracksMetadata', 'enrichTrack', 'getEnrichmentStats', 'getEnrichmentTracks', 'retryFailedEnrichment', 'backfillCovers', 'recheckCovers',
  'findDuplicates', 'mergeDuplicates', 'deleteTracks', 'getLyrics', 'refetchLyrics', 'saveLyrics',
  'getSettings', 'setSettings', 'pickFolder', 'getSecurityStatus',
  'lastfmStartAuth', 'lastfmCompleteAuth', 'lastfmDisconnect',
  'nowPlaying', 'submitScrobble', 'getScrobbleStatus',
  'driveConnect', 'driveDisconnect', 'driveSyncNow', 'driveSyncStatus',
  'syncMissingStatus', 'syncRetryMissing',
  'thermalUpdate', 'getThermalState',
  'transferGetState', 'transferSetEnabled', 'transferPairWithQr', 'transferRevokePeer'
] as const

export type InvokeMethod = (typeof INVOKE_METHODS)[number]
