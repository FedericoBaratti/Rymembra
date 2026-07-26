import { describe, expect, it } from 'vitest'
import { splitYoutubeWatchUrl } from './youtubeUrl'

describe('splitYoutubeWatchUrl', () => {
  it('splits a watch URL carrying a Mix/Radio list (the start_radio bug)', () => {
    const r = splitYoutubeWatchUrl(
      'https://www.youtube.com/watch?v=-WmkjgwmUB0&list=RD-WmkjgwmUB0&start_radio=1'
    )
    expect(r).toEqual({
      videoUrl: 'https://www.youtube.com/watch?v=-WmkjgwmUB0',
      playlistUrl: 'https://www.youtube.com/watch?v=-WmkjgwmUB0&list=RD-WmkjgwmUB0&start_radio=1',
      isMix: true
    })
  })

  it('splits a watch URL carrying a real playlist', () => {
    const r = splitYoutubeWatchUrl('https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLabc123')
    expect(r?.videoUrl).toBe('https://www.youtube.com/watch?v=dQw4w9WgXcQ')
    expect(r?.isMix).toBe(false)
  })

  it('preserves the music.youtube.com host in videoUrl', () => {
    const r = splitYoutubeWatchUrl('https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=RDAMVMx')
    expect(r?.videoUrl).toBe('https://music.youtube.com/watch?v=dQw4w9WgXcQ')
    expect(r?.isMix).toBe(true)
  })

  it('splits youtu.be short links with a list param', () => {
    const r = splitYoutubeWatchUrl('https://youtu.be/dQw4w9WgXcQ?list=RDdQw4w9WgXcQ')
    expect(r?.videoUrl).toBe('https://youtu.be/dQw4w9WgXcQ')
    expect(r?.isMix).toBe(true)
  })

  it('returns null for unambiguous or unrelated URLs', () => {
    expect(splitYoutubeWatchUrl('https://www.youtube.com/watch?v=dQw4w9WgXcQ')).toBeNull()
    expect(splitYoutubeWatchUrl('https://www.youtube.com/playlist?list=PLabc123')).toBeNull()
    expect(splitYoutubeWatchUrl('https://youtu.be/dQw4w9WgXcQ')).toBeNull()
    expect(
      splitYoutubeWatchUrl('https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC')
    ).toBeNull()
    expect(splitYoutubeWatchUrl('not a url')).toBeNull()
  })
})
