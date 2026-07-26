import { describe, expect, it } from 'vitest'
import type { PodcastEpisode, Track } from '@shared/types'
import { episodeToTrack, isEpisodeTrack } from './podcast'

const EPISODE: PodcastEpisode = {
  id: 7,
  podcast_id: 1,
  guid: 'ep-7',
  title: 'Episodio 7',
  description: null,
  audio_url: 'https://cdn.example.com/ep7.mp3',
  image_url: null,
  duration: 1800,
  published_at: null,
  progress_sec: 0,
  played: 0,
  podcast_title: 'Il Podcast'
}

describe('isEpisodeTrack', () => {
  it('recognises the synthetic track produced by episodeToTrack', () => {
    const track = episodeToTrack(EPISODE)
    expect(track.id).toBe(-7)
    expect(isEpisodeTrack(track)).toBe(true)
  })

  it('rejects ordinary library tracks (positive id, no stream_url)', () => {
    const track = { id: 42, stream_url: null } as unknown as Track
    expect(isEpisodeTrack(track)).toBe(false)
  })

  it('rejects a negative id WITHOUT stream_url (not an episode)', () => {
    const track = { id: -3, stream_url: null } as unknown as Track
    expect(isEpisodeTrack(track)).toBe(false)
  })

  it('rejects null/undefined', () => {
    expect(isEpisodeTrack(null)).toBe(false)
    expect(isEpisodeTrack(undefined)).toBe(false)
  })
})
