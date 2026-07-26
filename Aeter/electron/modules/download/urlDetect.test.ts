import { describe, expect, it } from 'vitest'
import { detectUrl } from './urlDetect'

describe('detectUrl', () => {
  it('detects Spotify tracks', () => {
    const r = detectUrl('https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC')
    expect(r).toEqual({
      type: 'spotify-track',
      spotifyKind: 'track',
      spotifyId: '4uLU6hMCjMI75M1A2tKUQC'
    })
  })

  it('detects intl-prefixed Spotify URLs', () => {
    const r = detectUrl('https://open.spotify.com/intl-it/album/2noRn2Aes5aoNVsU6iWThc')
    expect(r?.type).toBe('spotify-album')
    expect(r?.spotifyId).toBe('2noRn2Aes5aoNVsU6iWThc')
  })

  it('detects Spotify playlists and artists', () => {
    expect(detectUrl('https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M')?.type).toBe(
      'spotify-playlist'
    )
    expect(detectUrl('https://open.spotify.com/artist/0k17h0D3J5VfsdmQ1iZtE9')?.type).toBe(
      'spotify-artist'
    )
  })

  it('detects YouTube videos (watch, youtu.be, music)', () => {
    expect(detectUrl('https://www.youtube.com/watch?v=dQw4w9WgXcQ')?.type).toBe('youtube-video')
    expect(detectUrl('https://youtu.be/dQw4w9WgXcQ')?.type).toBe('youtube-video')
    expect(detectUrl('https://music.youtube.com/watch?v=dQw4w9WgXcQ')?.type).toBe('youtube-video')
  })

  it('detects YouTube playlists (list param wins over watch)', () => {
    expect(
      detectUrl('https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLabc123')?.type
    ).toBe('youtube-playlist')
    expect(detectUrl('https://www.youtube.com/playlist?list=PLabc123')?.type).toBe(
      'youtube-playlist'
    )
  })

  it('rejects unrelated URLs', () => {
    expect(detectUrl('https://example.com/watch?v=x')).toBeNull()
    expect(detectUrl('https://soundcloud.com/artist/track')).toBeNull()
    expect(detectUrl('not a url')).toBeNull()
  })
})
