import { describe, it, expect } from 'vitest'
import { buildYtDownloadArgs, ytPathArgs, YT_NET_ARGS, YT_SPEED_ARGS, type YtDownloadArgvInput } from './ytArgs'

// The -P home/temp routing keeps yt-dlp intermediates out of the watched
// library folder; yt-dlp silently IGNORES -P when --output is absolute, so the
// relative-template invariant is load-bearing and pinned here.
describe('ytPathArgs', () => {
  it('routes final files to home and intermediates to temp', () => {
    expect(ytPathArgs('/music', '/tmp/aether-ytdlp')).toEqual([
      '-P', 'home:/music',
      '-P', 'temp:/tmp/aether-ytdlp'
    ])
  })
})

describe('buildYtDownloadArgs', () => {
  const base: YtDownloadArgvInput = {
    format: 'bestaudio/best',
    extractorArgs: ['--extractor-args', 'youtube:player_client=tv'],
    qualityArgs: ['--audio-format', 'mp3', '--audio-quality', '0'],
    outTpl: '%(uploader)s/%(title)s.%(ext)s',
    homeDir: '/storage/emulated/0/Download/Music',
    tmpDir: '/data/cache/aether-ytdlp',
    ffmpegLocation: null,
    noPlaylist: false,
    url: 'https://www.youtube.com/watch?v=x'
  }

  it('keeps the output template relative and adds both -P paths', () => {
    const args = buildYtDownloadArgs(base)
    const outIdx = args.indexOf('--output')
    expect(outIdx).toBeGreaterThan(-1)
    expect(args[outIdx + 1]).toBe(base.outTpl)
    expect(args[outIdx + 1].startsWith('/')).toBe(false)
    expect(args).toContain('-P')
    expect(args).toContain('home:' + base.homeDir)
    expect(args).toContain('temp:' + base.tmpDir)
  })

  it('ends with the URL and includes extraction + sentinel prints', () => {
    const args = buildYtDownloadArgs(base)
    expect(args[args.length - 1]).toBe(base.url)
    expect(args).toContain('--extract-audio')
    expect(args.join(' ')).toContain('after_move:AETHER_D:%(filepath)s')
    expect(args.join(' ')).toContain('before_dl:AETHER_F:')
  })

  it('adds --no-playlist only for single videos', () => {
    expect(buildYtDownloadArgs(base)).not.toContain('--no-playlist')
    expect(buildYtDownloadArgs({ ...base, noPlaylist: true })).toContain('--no-playlist')
  })

  it('adds --ffmpeg-location only when a binary path is provided', () => {
    expect(buildYtDownloadArgs(base)).not.toContain('--ffmpeg-location')
    const withFfmpeg = buildYtDownloadArgs({ ...base, ffmpegLocation: '/bin/ffmpeg' })
    const i = withFfmpeg.indexOf('--ffmpeg-location')
    expect(i).toBeGreaterThan(-1)
    expect(withFfmpeg[i + 1]).toBe('/bin/ffmpeg')
  })

  it('includes the throughput args (concurrent fragments + chunked requests)', () => {
    const args = buildYtDownloadArgs(base)
    const cf = args.indexOf('--concurrent-fragments')
    expect(cf).toBeGreaterThan(-1)
    expect(args[cf + 1]).toBe('6')
    const cs = args.indexOf('--http-chunk-size')
    expect(cs).toBeGreaterThan(-1)
    expect(args[cs + 1]).toBe('10M')
  })
})

describe('YT_SPEED_ARGS', () => {
  it('is download-only: the shared net args must NOT contain speed flags', () => {
    // YT_NET_ARGS is reused by search/preview, which transfer no media.
    expect(YT_NET_ARGS).not.toContain('--concurrent-fragments')
    expect(YT_NET_ARGS).not.toContain('--http-chunk-size')
    expect(YT_SPEED_ARGS).toContain('--concurrent-fragments')
    expect(YT_SPEED_ARGS).toContain('--http-chunk-size')
  })
})
