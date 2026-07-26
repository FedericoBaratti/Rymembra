import type { Track, PodcastEpisode } from '@shared/types'
import { usePlayerStore } from '@/store/usePlayerStore'

// Podcast episodes reuse the whole player/queue/native-ExoPlayer pipeline by
// presenting as a synthetic Track: a NEGATIVE id (= -episodeId, so it never
// collides with a library row and is skipped by play_count/scrobbling) plus a
// stream_url the engine plays directly instead of the local media server.
export function episodeToTrack(ep: PodcastEpisode): Track {
  return {
    id: -ep.id,
    path: ep.audio_url,
    title: ep.title,
    artist: ep.podcast_title ?? '',
    album: ep.podcast_title ?? '',
    album_artist: null,
    year: null,
    track_number: null,
    disc_number: null,
    duration: ep.duration ?? 0,
    bitrate: null,
    sample_rate: null,
    codec: null,
    file_size: 0,
    date_added: 0,
    date_modified: 0,
    play_count: 0,
    last_played: null,
    rating: 0,
    bpm: null,
    key: null,
    genre: 'Podcast',
    comment: null,
    lyrics: null,
    cover_art_hash: null,
    is_local: 0,
    replaygain_track_gain: null,
    replaygain_album_gain: null,
    acoustid_fingerprint: null,
    mb_recording_id: null,
    stream_url: ep.audio_url,
    stream_cover_url: ep.image_url ?? null
  }
}

/**
 * Single source of truth for "is this Track actually a podcast episode?".
 * Episodes must be kept out of library-only paths (setLiked/radio/playlists/
 * play_count/scrobbling): their negative id matches no `tracks` row.
 */
export function isEpisodeTrack(t: Track | null | undefined): boolean {
  return !!t && t.id < 0 && !!t.stream_url
}

// Pending resume positions, keyed by the synthetic (negative) track id. The
// store consumes the entry once, when playback of that episode actually begins,
// so the engine can seek to the saved position only after the source is ready.
const pendingSeek = new Map<number, number>()

/** Consume (read-and-clear) a pending resume position for a synthetic track id. */
export function consumePendingEpisodeSeek(trackId: number): number | null {
  const at = pendingSeek.get(trackId)
  if (at == null) return null
  pendingSeek.delete(trackId)
  return at
}

/** Play a podcast episode through the normal player, resuming where we left off. */
export function playEpisode(ep: PodcastEpisode): void {
  const track = episodeToTrack(ep)
  const dur = ep.duration ?? 0
  // Resume only a meaningful in-progress listen: past the intro, not at the very end.
  if (ep.progress_sec > 5 && (dur === 0 || ep.progress_sec < dur - 15)) {
    pendingSeek.set(track.id, ep.progress_sec)
  }
  usePlayerStore.getState().playTracks([track], 0)
}
