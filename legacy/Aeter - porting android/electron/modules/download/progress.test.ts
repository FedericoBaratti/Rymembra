import { describe, expect, it } from 'vitest'
import { parseYtdlpLine, parseSpotdlLine } from './progress'

describe('parseYtdlpLine', () => {
  it('parses AETHER_P progress with byte counts', () => {
    expect(parseYtdlpLine('AETHER_P:512000/1024000')).toEqual({
      kind: 'progress',
      downloadedBytes: 512000,
      totalBytes: 1024000,
      fraction: 0.5
    })
  })

  it('handles NA totals with a null fraction', () => {
    const ev = parseYtdlpLine('AETHER_P:512000/NA')
    expect(ev).toMatchObject({ kind: 'progress', fraction: null })
  })

  it('parses AETHER_F file-start lines, including titles with colons', () => {
    expect(parseYtdlpLine('AETHER_F:3/12:Artist: The Song')).toEqual({
      kind: 'file-start',
      index: 3,
      total: 12,
      title: 'Artist: The Song'
    })
  })

  it('treats NA playlist fields as a single download', () => {
    expect(parseYtdlpLine('AETHER_F:NA/NA:Solo Track')).toMatchObject({
      kind: 'file-start',
      index: 1,
      total: 1
    })
  })

  it('parses AETHER_D done lines preserving Windows drive colons', () => {
    expect(parseYtdlpLine('AETHER_D:C:\\Music\\Artist\\01 - Song.mp3')).toEqual({
      kind: 'file-done',
      path: 'C:\\Music\\Artist\\01 - Song.mp3'
    })
  })

  it('falls back to legacy item lines', () => {
    expect(parseYtdlpLine('[download] Downloading item 3 of 12')).toEqual({
      kind: 'item',
      index: 3,
      total: 12
    })
  })

  it('falls back to legacy percent lines', () => {
    expect(parseYtdlpLine('[download]  42.3% of 8.21MiB at 2.91MiB/s ETA 00:02')).toEqual({
      kind: 'legacy-percent',
      fraction: 0.423
    })
  })

  it('parses Destination lines to a bare filename', () => {
    expect(parseYtdlpLine('[ExtractAudio] Destination: C:\\Music\\x\\01 - Song.mp3')).toEqual({
      kind: 'destination',
      file: '01 - Song.mp3'
    })
  })

  it('ignores unrelated lines', () => {
    expect(parseYtdlpLine('[youtube] dQw4w9WgXcQ: Downloading webpage')).toBeNull()
    expect(parseYtdlpLine('')).toBeNull()
    expect(parseYtdlpLine('random garbage')).toBeNull()
  })
})

describe('parseSpotdlLine', () => {
  it('parses Downloaded lines', () => {
    expect(parseSpotdlLine('Downloaded "Pink Floyd - Wish You Were Here"')).toEqual({
      kind: 'downloaded',
      title: 'Pink Floyd - Wish You Were Here'
    })
  })

  it('strips ANSI color codes before matching', () => {
    const colored = '[32mDownloaded "Song Title"[0m'
    expect(parseSpotdlLine(colored)).toEqual({ kind: 'downloaded', title: 'Song Title' })
  })

  it('parses Processing lines as current-file updates', () => {
    expect(parseSpotdlLine('Processing "Artist - Song"')).toMatchObject({ kind: 'processing' })
  })

  it('ignores query lines and garbage', () => {
    expect(parseSpotdlLine('Processing query "something"')).toBeNull()
    expect(parseSpotdlLine('')).toBeNull()
    expect(parseSpotdlLine('unrelated output')).toBeNull()
  })
})
