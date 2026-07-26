import { describe, it, expect } from 'vitest'
import { parseFeed } from './rss'

const FEED = `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>My Show</title>
    <itunes:author>Jane Doe</itunes:author>
    <description><![CDATA[A <b>great</b> show]]></description>
    <itunes:image href="https://cdn.example.com/show.jpg"/>
    <item>
      <title>Episode 2 &amp; more</title>
      <guid isPermaLink="false">ep-2</guid>
      <pubDate>Tue, 10 Jun 2025 09:00:00 +0000</pubDate>
      <itunes:duration>1:02:03</itunes:duration>
      <itunes:image href="https://cdn.example.com/ep2.jpg"/>
      <enclosure url="https://cdn.example.com/ep2.mp3" type="audio/mpeg" length="123"/>
    </item>
    <item>
      <title>Episode 1</title>
      <guid>ep-1</guid>
      <pubDate>Mon, 03 Jun 2025 09:00:00 +0000</pubDate>
      <itunes:duration>754</itunes:duration>
      <enclosure url="https://cdn.example.com/ep1.mp3" type="audio/mpeg"/>
    </item>
    <item>
      <title>No audio</title>
      <guid>ep-0</guid>
    </item>
  </channel>
</rss>`

describe('parseFeed', () => {
  const feed = parseFeed(FEED)

  it('parses channel metadata (CDATA, entities, itunes:image)', () => {
    expect(feed.title).toBe('My Show')
    expect(feed.author).toBe('Jane Doe')
    expect(feed.description).toBe('A <b>great</b> show')
    expect(feed.imageUrl).toBe('https://cdn.example.com/show.jpg')
  })

  it('skips items without playable audio', () => {
    expect(feed.episodes).toHaveLength(2)
    expect(feed.episodes.some((e) => e.title === 'No audio')).toBe(false)
  })

  it('decodes entities in titles and reads enclosure url', () => {
    const ep2 = feed.episodes.find((e) => e.guid === 'ep-2')!
    expect(ep2.title).toBe('Episode 2 & more')
    expect(ep2.audioUrl).toBe('https://cdn.example.com/ep2.mp3')
    expect(ep2.imageUrl).toBe('https://cdn.example.com/ep2.jpg')
  })

  it('parses HH:MM:SS and plain-seconds durations', () => {
    expect(feed.episodes.find((e) => e.guid === 'ep-2')!.duration).toBe(3723)
    expect(feed.episodes.find((e) => e.guid === 'ep-1')!.duration).toBe(754)
  })

  it('parses RFC-822 pubDate to a timestamp', () => {
    const ep1 = feed.episodes.find((e) => e.guid === 'ep-1')!
    expect(ep1.publishedAt).toBe(Date.parse('Mon, 03 Jun 2025 09:00:00 +0000'))
  })

  it('falls back to the audio url when guid is missing', () => {
    const noGuid = parseFeed(
      `<rss><channel><title>x</title><item><title>e</title><enclosure url="https://a/b.mp3" type="audio/mpeg"/></item></channel></rss>`
    )
    expect(noGuid.episodes[0].guid).toBe('https://a/b.mp3')
  })
})
