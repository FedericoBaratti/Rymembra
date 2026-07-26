/** Shared types between the Electron main process and the renderer. */

export interface Track {
  id: number
  path: string
  title: string
  artist: string
  album: string
  album_artist: string | null
  /** Normalized album grouping key (schema v10): fold(stripEdition(album)) + folder. */
  album_key?: string | null
  year: number | null
  track_number: number | null
  disc_number: number | null
  duration: number
  bitrate: number | null
  sample_rate: number | null
  codec: string | null
  file_size: number
  date_added: number
  date_modified: number
  play_count: number
  last_played: number | null
  rating: number
  bpm: number | null
  key: string | null
  genre: string | null
  comment: string | null
  lyrics: string | null
  cover_art_hash: string | null
  is_local: number
  replaygain_track_gain: number | null
  replaygain_album_gain: number | null
  acoustid_fingerprint: string | null
  mb_recording_id: string | null
  /** Authoritative album identifiers (schema v11): read from file tags or written by
   *  the downloader. Used to merge releases split across folders/editions. */
  mb_release_group_id?: string | null
  mb_release_id?: string | null
  spotify_album_id?: string | null
  /** Enrichment tracking (schema v4): null = never attempted. 'needs-review' =
   *  a plausible match existed but the evidence didn't prove it; nothing applied. */
  enrich_status?: 'ok' | 'no-match' | 'needs-review' | 'error' | null
  enrich_attempted_at?: number | null
  /** Download provenance (schema v6): e.g. 'youtube'; null = local/unknown. */
  source?: string | null
  /** Liked Songs (schema v7): 1 = liked. liked_at is the toggle time. */
  liked?: number | null
  liked_at?: number | null
  /** Last-writer-wins clock for rating/liked across devices (schema v7). */
  stats_updated_at?: number | null
  /** Remote audio URL for streamed content (podcast episodes). When set, the
   *  player streams this directly instead of the local /media/:id server. Such
   *  synthetic tracks use a non-positive id so they skip play_count/scrobbling. */
  stream_url?: string | null
  /** Remote artwork URL for streamed content (podcast episodes). */
  stream_cover_url?: string | null
}

export interface Album {
  id: number
  /** Normalized album grouping key (schema v10) — the album's stable identity. */
  album_key?: string
  title: string
  artist: string
  year: number | null
  genre: string | null
  total_tracks: number
  cover_art_hash: string | null
  mb_album_id: string | null
  spotify_id: string | null
}

export interface Artist {
  id: number
  name: string
  bio: string | null
  image_hash: string | null
  mb_artist_id: string | null
  spotify_id: string | null
  album_count: number
  track_count: number
}

export interface Playlist {
  id: number
  name: string
  description: string | null
  created_at: number
  updated_at: number
  cover_art_hash: string | null
  track_count: number
  total_duration: number
  cover_hashes: string[]
  is_smart: number
  /** JSON-serialized SmartPlaylistRules when is_smart = 1 */
  rules: string | null
}

// ---- smart playlists ----

export type SmartField =
  | 'title'
  | 'artist'
  | 'album'
  | 'genre'
  | 'year'
  | 'rating'
  | 'play_count'
  | 'last_played'
  | 'date_added'

export type SmartOp =
  | 'eq'
  | 'neq'
  | 'contains'
  | 'not_contains'
  | 'gt'
  | 'gte'
  | 'lt'
  | 'lte'
  | 'in_last_days'

export interface SmartRule {
  field: SmartField
  op: SmartOp
  value: string | number
}

export interface SmartPlaylistRules {
  combinator: 'and' | 'or'
  rules: SmartRule[]
  limit?: number
  sortBy?: SmartField | 'random'
  sortDir?: 'asc' | 'desc'
}

export type DownloadStatus =
  | 'pending'
  | 'downloading'
  | 'paused'
  | 'completed'
  | 'error'
  | 'cancelled'

export type SourceType =
  | 'youtube-video'
  | 'youtube-playlist'
  | 'spotify-track'
  | 'spotify-album'
  | 'spotify-artist'
  | 'spotify-playlist'
  | 'search'

export interface DownloadItem {
  id: number
  source_url: string
  source_type: SourceType
  status: DownloadStatus
  progress: number
  title: string
  artist: string | null
  album: string | null
  cover_url: string | null
  total_tracks: number
  completed_tracks: number
  current_file: string | null
  file_path: string | null
  created_at: number
  error_message: string | null
  /** Automatic-retry bookkeeping (schema v3, optional for renderer compat). */
  attempts?: number
  next_retry_at?: number | null
  last_failure_class?: string | null
}

export interface DownloadPreview {
  source_url: string
  source_type: SourceType
  title: string
  artist: string | null
  album: string | null
  cover_url: string | null
  total_tracks: number
  duration: number | null
  estimated_size_mb: number | null
}

export interface ScanProgress {
  phase: 'discovering' | 'scanning' | 'done'
  current: number
  total: number
  file: string | null
}

export interface SearchResults {
  tracks: Track[]
  albums: Album[]
  artists: Artist[]
}

export type RepeatMode = 'off' | 'one' | 'all'
export type ThemeMode = 'dark' | 'light' | 'system'
/** Visual skin (style layer, orthogonal to ThemeMode light/dark). */
export type SkinId = 'plain' | 'nothing' | 'cyberpunk'
export type DownloadQuality = 'mp3-320' | 'flac' | 'aac-256'

export interface EqPreset {
  name: string
  gains: number[] // 10 bands, dB values -12..12
}

export interface AppSettings {
  watchFolders: string[]
  downloadFolder: string
  downloadQuality: DownloadQuality
  downloadConcurrency: number
  autoFixYoutubeMetadata: boolean
  crossfadeSeconds: number
  replayGainEnabled: boolean
  replayGainTargetDb: number
  eqEnabled: boolean
  eqGains: number[]
  eqCustomPresets: EqPreset[]
  theme: ThemeMode
  skin: SkinId
  language: 'it' | 'en'
  volume: number
  muted: boolean
  notificationsOnTrackChange: boolean
  globalMediaKeys: boolean
  hasSeenOnboarding: boolean
  spotifyClientId: string
  spotifyClientSecret: string
  lastfmApiKey: string
  lastfmApiSecret: string
  lastfmSessionKey: string
  lastfmUsername: string
  scrobblingEnabled: boolean
  acoustidApiKey: string
  /** Master switch for automatic background metadata enrichment: the sweeps
   *  after each scan/download and the idle re-sweep timer (default on). When
   *  off, metadata is enriched only on explicit user action. */
  autoEnrichEnabled: boolean
  /** Keyless fingerprint (AcoustID) during enrichment (kill-switch, default on). */
  enrichFingerprint: boolean
  /** Automatic duplicate removal after each scan/download (default on). When on,
   *  duplicate groups are collapsed to a single track with no user interaction;
   *  the losing files are trashed. */
  dedupeAutoRemove: boolean
  /** Which copy survives an automatic dedupe: 'higher' keeps the best quality
   *  (lossless → bitrate → sample-rate → size), 'lower' keeps the worst. */
  dedupeKeep: 'higher' | 'lower'
  // ---- Google Drive library sync ----
  /** OAuth "Desktop app" client id from the user's Google Cloud project. Entered
   *  in the UI (like the Spotify credentials); '' until configured. */
  googleClientId: string
  /** OAuth client secret paired with googleClientId (a SECRET_KEY: encrypted at
   *  rest, blanked in settings.json). '' until configured. */
  googleClientSecret: string
  /** Auto-sync the library (metadata only) to Google Drive when connected. */
  driveSyncEnabled: boolean
  /** Epoch ms of the last successful sync, or null if never. */
  driveSyncLastAt: number | null
  /** Connected Google account email (shown in the UI; '' when disconnected). */
  googleDriveEmail: string
  /** Stable per-install id used to attribute sync writes. Generated lazily. */
  syncDeviceId: string
  /** Drive fileId of the sync file, cached to skip the lookup (null until known). */
  driveFileId: string | null
  /** Content hash of the local snapshot at the last successful sync, to skip a
   *  sync when the local library is unchanged (null until known). */
  driveLastLocalHash: string | null
  /** Remote file md5 at the last successful sync; if it changed, another device
   *  wrote and we must merge even when our local snapshot is unchanged. */
  driveLastRemoteMd5: string | null
  /** OAuth refresh token (a SECRET_KEY: encrypted at rest, blanked in
   *  settings.json, never shown in the UI). '' when disconnected. */
  googleRefreshToken: string
  /** Auto-download tracks that appear in the synced metadata but have no local
   *  audio file, re-fetching them from the download sources (YouTube search). */
  autoFetchMissing: boolean
  /** Network policy for the auto-fetch (Android gates on it; desktop ignores it,
   *  behaving as 'any'). 'wifi' = only on Wi-Fi, 'any' = also on mobile data. */
  autoFetchNetwork: 'wifi' | 'any'
  // ---- LAN remote access (thin-client phone: play/browse/search only) ----
  /** Whether the local HTTP/WS server for LAN-paired phone clients is running. */
  lanServerEnabled: boolean
  /** TCP port the LAN server binds to (both the REST API and the WS upgrade). */
  lanServerPort: number
  /** Phone-side transfer server for the "repair from PC" feature (Android only;
   *  inert on desktop — kept for settings parity between the two trees). */
  transferServerEnabled: boolean
}

export interface DuplicateGroup {
  reason: 'tags' | 'fingerprint'
  tracks: Track[]
}

export interface MergeOutcome {
  merged: number
  playlistsUpdated: number
}

export interface EnrichmentResult {
  trackId: number
  applied: boolean
  fields: Partial<Track>
  message: string
}

export type EnrichmentBucket = 'no-match' | 'needs-review' | 'error' | 'pending'

export interface EnrichmentStats {
  total: number
  ok: number
  noMatch: number
  /** Plausible match found but not proven — nothing was applied. */
  needsReview: number
  error: number
  /** Candidates never attempted (unknown artist/album or missing cover). */
  pending: number
  missingCovers: number
}

export interface TrackMetadataUpdate {
  title?: string
  artist?: string
  album?: string
  album_artist?: string
  year?: number | null
  track_number?: number | null
  disc_number?: number | null
  genre?: string
  bpm?: number | null
  comment?: string
  lyrics?: string
  rating?: number
  /** base64-encoded image to set as cover, or null to keep */
  coverImageBase64?: string | null
  coverImageMime?: string | null
}

export interface LibraryStats {
  tracks: number
  albums: number
  artists: number
  totalDuration: number
}

export interface TrackQuery {
  sortBy?: 'title' | 'artist' | 'album' | 'year' | 'duration' | 'rating' | 'date_added' | 'play_count'
  sortDir?: 'asc' | 'desc'
  offset?: number
  limit?: number
  albumId?: number
  artistName?: string
}

export interface LyricsResult {
  synced: { time: number; text: string }[] | null
  plain: string | null
}

/** Playback queue snapshot persisted across restarts. */
export interface PersistedQueue {
  /** Track ids in queue order. */
  trackIds: number[]
  /** Playback order as indices into trackIds. */
  order: number[]
  orderPos: number
  shuffle: boolean
  repeat: RepeatMode
}

// ---- Spotify migration (keyless) ----

export type SpotifyEntityKind = 'track' | 'album' | 'artist' | 'playlist'

export interface SpotifyMigrationPreview {
  kind: SpotifyEntityKind
  title: string
  artist: string | null
  coverUrl: string | null
  totalTracks: number
}

// 'failed' = a transient failure (network/timeout/403) that will be retried;
// distinct from 'notfound' (no match on YouTube / unavailable — terminal).
export type SpotifyMigrationTrackStatus =
  | 'pending'
  | 'downloading'
  | 'done'
  | 'notfound'
  | 'failed'

export interface SpotifyMigrationTrack {
  title: string
  artist: string | null
  status: SpotifyMigrationTrackStatus
}

export type SpotifyMigrationStatus =
  | 'idle'
  | 'resolving'
  | 'running'
  | 'done'
  | 'error'
  | 'cancelled'

export interface SpotifyMigrationState {
  id: string
  status: SpotifyMigrationStatus
  sourceUrl: string
  kind: string
  title: string
  coverUrl: string | null
  recreatePlaylist: boolean
  playlistId: number | null
  total: number
  done: number
  matched: number
  currentTitle: string | null
  tracks: SpotifyMigrationTrack[]
  notFound: { title: string; artist: string | null }[]
  error: string | null
}

/** Live status of the Google Drive library sync, for the settings UI. */
export interface SyncStatus {
  /** True once an OAuth client is available (bundled in source or entered in the
   *  UI) — the "Sign in with Google" button is actionable only then. */
  configured: boolean
  connected: boolean
  email: string
  enabled: boolean
  syncing: boolean
  lastSyncAt: number | null
  lastError: string | null
}

/** Live status of the LAN server that lets a paired phone stream/browse/search
 *  the desktop library, for the "Connetti telefono" settings UI. */
export interface LanStatus {
  running: boolean
  port: number
  /** Non-link-local LAN IPv4 addresses this machine currently has. */
  addresses: string[]
  advertising: boolean
  /** True when no usable LAN address exists — promotes the hotspot button to
   *  the primary action instead of a secondary/collapsed one. */
  needsHotspot: boolean
  /** Why the server failed to start (e.g. port already in use); null when
   *  healthy. Lets the UI show the failure instead of a phantom "running". */
  lastError: string | null
}

/** A phone paired with the LAN server. Never carries the device's bearer
 *  token or its hash — only what the Settings UI needs to show/revoke it. */
export interface PairedDevice {
  deviceId: string
  deviceName: string
  pairedAt: number
  lastSeenAt: number
}

/** QR pairing payload for the Settings UI. `qrDataUrl` is null when no LAN
 *  address is available yet (see `error`). */
export interface PairingCode {
  qrDataUrl: string | null
  expiresAt: number | null
  host: string | null
  port: number
  error?: 'no-lan-address'
}

/** Progress of the auto-download of tracks that are in the synced metadata but
 *  have no local audio file, re-fetched from the download sources. */
export interface MissingFetchStatus {
  /** Waiting to be searched/downloaded (includes backoff between retries). */
  pending: number
  /** Currently downloading. */
  active: number
  /** Gave up after repeated failures (no match / download error). */
  failed: number
  /** pending + active + failed (excludes completed). */
  total: number
}

// ---- Thermal management (Android; see electron/modules/adaptiveConcurrency.ts) ----

/** Coarse device thermal level mapped from PowerManager THERMAL_STATUS_* by
 *  ThermalMonitor.kt. Desktop never reports, so it is permanently 'normal'. */
export type ThermalLevel = 'normal' | 'warning' | 'critical'

/** Last thermal sample the backend received. */
export interface ThermalState {
  level: ThermalLevel
  /** PowerManager.getThermalHeadroom() sample, when the device provides one. */
  headroom?: number
  /** Epoch ms when the backend received the sample. */
  timestamp: number
}

// ---- Phone repair over WiFi (desktop = client; electron/modules/phoneSync/) ----

/** One track as listed by the phone's transfer server (GET /api/tracks). */
export interface PhoneTrackInfo {
  id: number
  /** Cross-device identity (shared/trackKey.ts v2: artist|title|album). */
  trackKey: string
  title: string
  artist: string
  album: string
  year: number | null
  genre: string | null
  durationS: number
  codec: string | null
  bitrate: number | null
  sampleRate: number | null
  fileSize: number
  mtimeMs: number
  /** Lowercased extension with the dot, e.g. '.opus'. */
  ext: string
  /** File name without directory (extension included). */
  basename: string
  hasCover: boolean
  enrichStatus: string | null
}

/** The paired phone as seen from the desktop. The bearer token lives in the
 *  encrypted secrets store (`phoneTransferToken`), never in this metadata. */
export interface PhonePeer {
  deviceId: string
  deviceName: string
  /** Last endpoint that answered — refreshed by mDNS rediscovery. */
  lastHost: string
  lastPort: number
  pairedAt: number
}

/** QR payload state for the phone-pairing modal. */
export interface PhonePairingCode {
  qrDataUrl: string | null
  expiresAt: number | null
  error?: 'no-lan-address'
}

export interface PhoneSyncState {
  paired: boolean
  peer: PhonePeer | null
  /** True while the one-shot pairing listener is waiting for the phone. */
  pairing: boolean
  /** Result of the last connection attempt (null = never tried). */
  online: boolean | null
}

/** What the desktop pipeline plans to do with a phone track. */
export type PhonePlanBadge = 'ok' | 'needs-codec' | 'needs-enrich' | 'both'

export type PhoneRepairStatus =
  | 'pending'
  | 'pulling'
  | 'validating'
  | 'transcoding'
  | 'enriching'
  | 'pushing'
  | 'committing'
  | 'done'
  | 'skipped'
  | 'failed'

/** Durable per-track repair job (phone_repair table). */
export interface PhoneRepairItem {
  id: number
  deviceId: string
  phoneTrackId: number
  trackKey: string
  title: string
  artist: string
  album: string
  status: PhoneRepairStatus
  actionTranscode: boolean
  actionEnrich: boolean
  attempts: number
  error: string | null
  updatedAt: number
}

/** A phone track joined with its plan badge and any repair-job row. */
export interface PhoneTrackPlanned {
  info: PhoneTrackInfo
  badge: PhonePlanBadge
  repair: PhoneRepairItem | null
}

/** Events pushed main -> renderer */
export interface AetherEvents {
  'sync:status': SyncStatus
  'spotify:migration': SpotifyMigrationState
  'scan:progress': ScanProgress
  'library:changed': { reason: string }
  'download:updated': DownloadItem
  'enrichment:updated': EnrichmentResult
  'enrichment:progress': { phase: 'enrich' | 'covers'; done: number; total: number }
  'track:updated': Track
  'batch-metadata:progress': { done: number; total: number; errors: number }
  'media-key': 'play-pause' | 'next' | 'previous' | 'stop'
  /** Emitted after an automatic dedupe pass removed one or more duplicates. */
  'duplicates:removed': { count: number }
  /** Device thermal level transition (Android; never fires on desktop). */
  'thermal:changed': ThermalState
  /** Phone pairing/connection state changed (desktop phoneSync). */
  'phone:state': PhoneSyncState
  /** A phone-repair job row changed status/progress. */
  'phoneRepair:updated': PhoneRepairItem
}

export type AetherEventName = keyof AetherEvents

// ---- Discovery / recommendations (Spotify-style layer) ----

/** A recommended track not in the library — downloadable via the yt-dlp infra. */
export interface ExternalRecoTrack {
  title: string
  artist: string
  mbid: string | null
  /** Blended relevance score. */
  score: number
  /** Which sources endorsed it ('listenbrainz' | 'lastfm' | 'deezer'). */
  sources: string[]
  /** Remote cover URL (catalogue search results only; reco picks have none). */
  coverUrl?: string | null
  /** Track length in ms, when known — improves YouTube match accuracy. */
  durationMs?: number | null
  /** True when an identical track is already in the local library. */
  owned?: boolean
}

/** Owned tracks to play now + external picks to download. */
export interface RecoResultTracks {
  inLibrary: Track[]
  external: ExternalRecoTrack[]
}

/** Radio seed: a track, an artist, or a genre/tag. */
export type RadioSeed =
  | { kind: 'track'; trackId: number }
  | { kind: 'artist'; artist: string }
  | { kind: 'genre'; genre: string }

export interface HomeSection {
  id: string
  kind: 'tracks' | 'albums' | 'external'
  /** i18n key for the section heading. */
  titleKey: string
  /** Optional interpolation value (e.g. an artist name). */
  titleArg?: string
  tracks?: Track[]
  albums?: Album[]
  external?: ExternalRecoTrack[]
}

export interface HomeFeed {
  sections: HomeSection[]
  likedCount: number
}

export interface ArtistStat {
  name: string
  plays: number
  tracks: number
}

export interface GenreStat {
  name: string
  plays: number
}

export interface ListeningStats {
  periodDays: number
  totals: { plays: number; unique_tracks: number; ms_played: number }
  topTracks: (Track & { plays: number })[]
  topArtists: ArtistStat[]
  topGenres: GenreStat[]
}

// ---- Podcasts (RSS, keyless) ----

export interface Podcast {
  id: number
  feed_url: string
  title: string
  author: string | null
  description: string | null
  image_url: string | null
  added_at: number
  last_refreshed: number | null
  /** Populated by getPodcasts(). */
  episode_count?: number
}

export interface PodcastEpisode {
  id: number
  podcast_id: number
  guid: string
  title: string
  description: string | null
  audio_url: string
  image_url: string | null
  duration: number | null
  published_at: number | null
  progress_sec: number
  played: number
  /** Joined from the parent podcast for standalone rendering. */
  podcast_title?: string
}

/** A podcast search hit (iTunes Search, keyless). */
export interface PodcastSearchResult {
  title: string
  author: string | null
  feedUrl: string
  imageUrl: string | null
}

/** API exposed on window.aether via contextBridge */
export interface AetherAPI {
  // library
  getTracks(query?: TrackQuery): Promise<Track[]>
  getTrackCount(): Promise<number>
  getTrackById(id: number): Promise<Track | null>
  getTracksByIds(ids: number[]): Promise<Track[]>
  getAlbums(): Promise<Album[]>
  getAlbumTracks(albumId: number): Promise<Track[]>
  /** Merge albums that got split by an inconsistent album_artist (legacy imports). */
  repairSplitAlbums(): Promise<{ groups: number; retagged: number }>
  getArtists(): Promise<Artist[]>
  getArtistAlbums(artistName: string): Promise<Album[]>
  getLibraryStats(): Promise<LibraryStats>
  search(term: string): Promise<SearchResults>
  rescanLibrary(): Promise<void>
  /** Persist all pending DB/settings/queue writes (called when backgrounded). */
  flushNow(): Promise<void>
  recordPlay(trackId: number, msPlayed?: number): Promise<void>
  setRating(trackId: number, rating: number): Promise<void>
  showInFolder(trackId: number): Promise<void>

  // discovery / recommendations
  getHomeFeed(): Promise<HomeFeed>
  getSimilarTracks(trackId: number, limit?: number): Promise<RecoResultTracks>
  getRadioSeedTracks(seed: RadioSeed, limit?: number): Promise<RecoResultTracks>
  /** Search a keyless catalogue (Deezer) so the user can find & download ANY song. */
  searchExternalCatalog(term: string): Promise<ExternalRecoTrack[]>
  /** Resolve an external recommendation → YouTube → existing download queue.
   *  Returns null only on a genuine no-match; real failures reject with a DL_* code. */
  downloadExternalTrack(meta: {
    artist: string
    title: string
    durationMs?: number | null
    coverUrl?: string | null
  }): Promise<DownloadItem | null>
  setLiked(trackId: number, liked: boolean): Promise<{ liked: boolean }>
  getLikedTracks(): Promise<Track[]>
  getListeningStats(periodDays?: number): Promise<ListeningStats>

  // podcasts (RSS, keyless)
  searchPodcasts(term: string): Promise<PodcastSearchResult[]>
  addPodcast(feedUrl: string): Promise<Podcast>
  removePodcast(podcastId: number): Promise<void>
  refreshPodcast(podcastId: number): Promise<{ added: number }>
  getPodcasts(): Promise<Podcast[]>
  getPodcastEpisodes(podcastId: number): Promise<PodcastEpisode[]>
  getLatestEpisodes(limit?: number): Promise<PodcastEpisode[]>
  setEpisodeProgress(episodeId: number, progressSec: number, played: boolean): Promise<void>

  // waveform cache
  getWaveform(trackId: number): Promise<number[] | null>
  saveWaveform(trackId: number, peaks: number[]): Promise<void>

  // playlists
  getPlaylists(): Promise<Playlist[]>
  getPlaylistTracks(playlistId: number): Promise<Track[]>
  createPlaylist(name: string, description?: string, trackIds?: number[]): Promise<Playlist>
  renamePlaylist(playlistId: number, name: string, description?: string): Promise<void>
  deletePlaylist(playlistId: number): Promise<void>
  addToPlaylist(playlistId: number, trackIds: number[]): Promise<void>
  removeFromPlaylist(playlistId: number, positions: number[]): Promise<void>
  reorderPlaylist(playlistId: number, trackIdsInOrder: number[]): Promise<void>
  createSmartPlaylist(name: string, rules: SmartPlaylistRules): Promise<Playlist>
  setSmartPlaylistRules(playlistId: number, name: string, rules: SmartPlaylistRules): Promise<void>
  previewSmartPlaylist(rules: SmartPlaylistRules): Promise<Track[]>

  // downloads
  previewDownload(url: string): Promise<DownloadPreview>
  startDownload(preview: DownloadPreview): Promise<DownloadItem>
  cancelDownload(id: number): Promise<void>
  pauseDownload(id: number): Promise<void>
  resumeDownload(id: number): Promise<void>
  retryDownload(id: number): Promise<void>
  clearFinishedDownloads(): Promise<void>
  getDownloads(): Promise<DownloadItem[]>
  updateYtDlp(): Promise<{ updated: boolean; version: string }>
  getBinaryStatus(): Promise<Record<'yt-dlp' | 'ffmpeg' | 'spotdl' | 'fpcalc', { found: boolean; dir: string }>>

  // Spotify migration (keyless)
  spotifyMigrationPreview(url: string): Promise<SpotifyMigrationPreview>
  spotifyMigrationStart(opts: { url: string; recreatePlaylist: boolean }): Promise<SpotifyMigrationState>
  spotifyMigrationCancel(): Promise<void>
  getSpotifyMigration(): Promise<SpotifyMigrationState | null>

  // metadata
  updateTrackMetadata(trackId: number, update: TrackMetadataUpdate): Promise<Track>
  updateTracksMetadata(
    trackIds: number[],
    update: TrackMetadataUpdate
  ): Promise<{ updated: number; errors: number }>
  enrichTrack(trackId: number): Promise<EnrichmentResult>
  getEnrichmentStats(): Promise<EnrichmentStats>
  getEnrichmentTracks(bucket: EnrichmentBucket, offset?: number, limit?: number): Promise<Track[]>
  retryFailedEnrichment(): Promise<{ reset: number }>
  backfillCovers(): Promise<{ updated: number; total: number }>
  recheckCovers(): Promise<{ updated: number; total: number }>
  findDuplicates(): Promise<DuplicateGroup[]>
  mergeDuplicates(survivorId: number, victimIds: number[], deleteFiles: boolean): Promise<MergeOutcome>
  deleteTracks(trackIds: number[], deleteFiles: boolean): Promise<void>
  getLyrics(trackId: number): Promise<LyricsResult>
  refetchLyrics(trackId: number): Promise<LyricsResult>
  saveLyrics(trackId: number, lyrics: string): Promise<Track>

  // queue persistence
  getQueueState(): Promise<PersistedQueue | null>
  saveQueueState(state: PersistedQueue): Promise<void>

  // settings
  getSettings(): Promise<AppSettings>
  setSettings(patch: Partial<AppSettings>): Promise<AppSettings>
  pickFolder(): Promise<string | null>
  /** Whether stored API credentials are encrypted at rest (safeStorage). */
  getSecurityStatus(): Promise<{ secretsEncrypted: boolean }>

  // Last.fm scrobbling
  lastfmStartAuth(): Promise<void>
  lastfmCompleteAuth(): Promise<{ username: string }>
  lastfmDisconnect(): Promise<void>
  nowPlaying(trackId: number): Promise<void>
  submitScrobble(trackId: number, playedSec: number, startedAtSec: number): Promise<void>
  getScrobbleStatus(): Promise<{ queued: number; connected: boolean; username: string }>

  // Google Drive library sync
  driveConnect(): Promise<SyncStatus>
  driveDisconnect(): Promise<SyncStatus>
  driveSyncNow(): Promise<SyncStatus>
  driveSyncStatus(): Promise<SyncStatus>
  syncMissingStatus(): Promise<MissingFetchStatus>
  syncRetryMissing(): Promise<MissingFetchStatus>

  // LAN remote access (thin-client phone: play/browse/search only)
  getLanStatus(): Promise<LanStatus>
  generatePairingCode(): Promise<PairingCode>
  getPairedDevices(): Promise<PairedDevice[]>
  revokeDevice(deviceId: string): Promise<void>
  openHotspotSettings(): Promise<void>

  // Thermal management (samples pushed by the Android ThermalPlugin via
  // src/lib/thermal.ts; on desktop the handlers exist but nothing calls them)
  thermalUpdate(sample: { level: ThermalLevel; headroom?: number }): Promise<ThermalState>
  getThermalState(): Promise<ThermalState>

  // Phone repair over WiFi (desktop = client of the phone's transfer server)
  phonePairStart(): Promise<PhonePairingCode>
  phonePairCancel(): Promise<void>
  phoneGetState(): Promise<PhoneSyncState>
  /** Connect (or rediscover via mDNS) and list the phone's tracks with plans. */
  phoneListTracks(): Promise<PhoneTrackPlanned[]>
  /** Queue a repair run for the given phone track ids, or every non-ok track. */
  phoneRepairStart(ids: number[] | 'all'): Promise<void>
  phoneRepairCancel(): Promise<void>
  phoneForget(): Promise<void>

  // events
  on<E extends AetherEventName>(event: E, cb: (payload: AetherEvents[E]) => void): () => void
}
