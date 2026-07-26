/**
 * Single source of truth for the `window.aether.<method>` invoke surface.
 *
 * Imported by electron/preload.ts so a new IPC method is registered in ONE
 * place. Backend → renderer events (`on(...)`) are NOT in this list — only
 * methods the renderer invokes on the backend.
 */
export const INVOKE_METHODS = [
  'getTracks', 'getTrackCount', 'getTrackById', 'getTracksByIds', 'getAlbums', 'getAlbumTracks',
  'repairSplitAlbums',
  'getArtists', 'getArtistAlbums', 'getLibraryStats', 'search', 'rescanLibrary', 'flushNow',
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
  'getLanStatus', 'generatePairingCode', 'getPairedDevices', 'revokeDevice', 'openHotspotSettings',
  'thermalUpdate', 'getThermalState',
  'phonePairStart', 'phonePairCancel', 'phoneGetState', 'phoneListTracks',
  'phoneRepairStart', 'phoneRepairCancel', 'phoneForget'
] as const

export type InvokeMethod = (typeof INVOKE_METHODS)[number]
