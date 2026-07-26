import { describe, it, expect } from 'vitest'
import { extractNextData, parseEmbedHtml } from './embed'

function page(json: unknown): string {
  return `<!doctype html><html><body>
    <script id="__NEXT_DATA__" type="application/json">${JSON.stringify(json)}</script>
  </body></html>`
}

describe('spotify embed parsing', () => {
  it('extracts the __NEXT_DATA__ JSON blob', () => {
    const data = extractNextData(page({ hello: 'world' }))
    expect(data).toEqual({ hello: 'world' })
  })

  it('returns null when the script tag is absent or malformed', () => {
    expect(extractNextData('<html></html>')).toBeNull()
    expect(
      extractNextData('<script id="__NEXT_DATA__">{not json}</script>')
    ).toBeNull()
  })

  it('parses a playlist tracklist (title, artist, durationMs) at any depth', () => {
    const html = page({
      props: {
        pageProps: {
          state: {
            data: {
              entity: {
                name: 'Chill Mix',
                coverArt: { sources: [{ url: 'http://img/lo.jpg' }, { url: 'http://img/hi.jpg' }] },
                trackList: [
                  { title: 'Song A', subtitle: 'Artist A', duration: 210000 },
                  { title: 'Song B', subtitle: 'Artist B', duration: 185000 }
                ]
              }
            }
          }
        }
      }
    })
    const res = parseEmbedHtml(html, 'playlist')
    expect(res).not.toBeNull()
    expect(res!.title).toBe('Chill Mix')
    expect(res!.coverUrl).toBe('http://img/hi.jpg')
    expect(res!.tracks).toHaveLength(2)
    expect(res!.tracks[0]).toMatchObject({ title: 'Song A', artist: 'Artist A', durationMs: 210000 })
  })

  it('handles object-form duration (totalMilliseconds)', () => {
    const html = page({
      data: {
        entity: {
          name: 'Album X',
          trackList: [{ title: 'Track', subtitle: 'Band', duration: { totalMilliseconds: 99000 } }]
        }
      }
    })
    const res = parseEmbedHtml(html, 'album')
    expect(res!.tracks[0].durationMs).toBe(99000)
  })

  it('falls back to a single pseudo-track when only a name is present', () => {
    const html = page({ entity: { name: 'Just A Song', coverArt: { sources: [{ url: 'c' }] } } })
    const res = parseEmbedHtml(html, 'track')
    expect(res!.title).toBe('Just A Song')
    expect(res!.tracks).toHaveLength(1)
  })

  it('returns null when there is neither a name nor a tracklist', () => {
    expect(parseEmbedHtml(page({ nothing: true }), 'playlist')).toBeNull()
  })

  it('stamps a consistent album name + album artist on every album track', () => {
    const html = page({
      data: {
        entity: {
          name: 'Thriller',
          subtitle: 'Michael Jackson',
          coverArt: { sources: [{ url: 'cover' }] },
          trackList: [
            { title: 'Wanna Be Startin', subtitle: 'Michael Jackson', duration: 1000 },
            // Featured guest as the track artist must NOT change the album artist.
            { title: 'The Girl Is Mine', subtitle: 'Michael Jackson, Paul McCartney', duration: 2000 }
          ]
        }
      }
    })
    const res = parseEmbedHtml(html, 'album')!
    expect(res.tracks).toHaveLength(2)
    for (const tr of res.tracks) {
      expect(tr.album).toBe('Thriller')
      expect(tr.albumArtist).toBe('Michael Jackson')
    }
    // Per-track artist still reflects the credited performers.
    expect(res.tracks[1].artist).toBe('Michael Jackson, Paul McCartney')
  })
})
