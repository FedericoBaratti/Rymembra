// Shared Spotify resolver types (pure — no electron imports, safe to unit-test).

export interface SpotifyTrack {
  title: string
  artist: string | null
  album: string | null
  /** Album-level artist (Spotify `album.artists`). Used to group an album as ONE
   *  release even when individual tracks credit guests/features. */
  albumArtist?: string | null
  /** 1-based disc number from Spotify, when known. */
  discNumber?: number | null
  /** 1-based track number within its disc from Spotify, when known. */
  trackNumber?: number | null
  /** Release year from Spotify (`album.release_date`), when known. */
  year?: number | null
  durationMs: number | null
  isrc?: string | null
  coverUrl?: string | null
}

export interface SpotifyResolved {
  kind: 'track' | 'album' | 'artist' | 'playlist'
  title: string
  artist: string | null
  coverUrl: string | null
  tracks: SpotifyTrack[]
}

export interface SpotifyPreview {
  title: string
  artist: string | null
  coverUrl: string | null
  totalTracks: number
  durationMs: number | null
}
