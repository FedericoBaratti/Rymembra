import { describe, it, expect } from 'vitest'
import { codecFamily, decideTranscode, qualityArgs, targetExtFor } from './transcodePlan'

describe('codecFamily', () => {
  it('maps music-metadata codec strings to families', () => {
    expect(codecFamily('MPEG 1 Layer 3', '.mp3')).toBe('mp3')
    expect(codecFamily('AAC', '.m4a')).toBe('aac')
    expect(codecFamily('MPEG-4/AAC', '.m4a')).toBe('aac')
    expect(codecFamily('FLAC', '.flac')).toBe('flac')
    expect(codecFamily('Opus', '.opus')).toBe('opus')
    expect(codecFamily('Vorbis I', '.ogg')).toBe('vorbis')
    expect(codecFamily('PCM', '.wav')).toBe('pcm')
  })
  it('detects ALAC before the generic aac/mp4a match', () => {
    expect(codecFamily('ALAC', '.m4a')).toBe('alac')
    expect(codecFamily('Apple Lossless', '.m4a')).toBe('alac')
  })
  it('treats non-MPEG-4 "MPEG audio" as mp3', () => {
    expect(codecFamily('MPEG Audio', '.mp3')).toBe('mp3')
    expect(codecFamily('MPEG-4', '.m4a')).not.toBe('mp3')
  })
  it('falls back to the extension when the codec string is missing', () => {
    expect(codecFamily(null, '.mp3')).toBe('mp3')
    expect(codecFamily(undefined, 'm4a')).toBe('aac')
    expect(codecFamily('', '.OGG')).toBe('vorbis')
    expect(codecFamily(null, '.aiff')).toBe('pcm')
    expect(codecFamily(null, '.xyz')).toBe('unknown')
  })
  it('prefers the codec string over a mismatched extension', () => {
    // e.g. an .m4a container actually holding ALAC
    expect(codecFamily('ALAC', '.m4a')).toBe('alac')
    expect(codecFamily('FLAC', '.ogg')).toBe('flac')
  })
})

describe('decideTranscode', () => {
  it('never re-encodes a source already in the target family', () => {
    expect(decideTranscode('MPEG 1 Layer 3', '.mp3', 'mp3-320')).toEqual({
      needed: false,
      reason: 'match',
      targetExt: '.mp3'
    })
    expect(decideTranscode('AAC', '.m4a', 'aac-256').reason).toBe('match')
    expect(decideTranscode('FLAC', '.flac', 'flac').reason).toBe('match')
  })
  it('skips lossy→flac upconversion', () => {
    for (const [codec, ext] of [
      ['MPEG 1 Layer 3', '.mp3'],
      ['AAC', '.m4a'],
      ['Opus', '.opus'],
      ['Vorbis I', '.ogg'],
      ['WMA', '.wma']
    ] as const) {
      const plan = decideTranscode(codec, ext, 'flac')
      expect(plan.needed).toBe(false)
      expect(plan.reason).toBe('skip-upconvert')
    }
  })
  it('converts lossless sources to a flac target', () => {
    expect(decideTranscode('ALAC', '.m4a', 'flac')).toEqual({
      needed: true,
      reason: 'convert',
      targetExt: '.flac'
    })
    expect(decideTranscode('PCM', '.wav', 'flac').reason).toBe('convert')
  })
  it('converts cross-family sources to a lossy target', () => {
    expect(decideTranscode('Opus', '.opus', 'mp3-320')).toEqual({
      needed: true,
      reason: 'convert',
      targetExt: '.mp3'
    })
    expect(decideTranscode('FLAC', '.flac', 'mp3-320').reason).toBe('convert')
    expect(decideTranscode('MPEG 1 Layer 3', '.mp3', 'aac-256')).toEqual({
      needed: true,
      reason: 'convert',
      targetExt: '.m4a'
    })
    expect(decideTranscode('Vorbis I', '.ogg', 'aac-256').reason).toBe('convert')
  })
  it('converts unknown codecs (better a standard file than an unreadable one)', () => {
    expect(decideTranscode(null, '.xyz', 'mp3-320').reason).toBe('convert')
  })
  it('uses the extension fallback when the codec string is absent', () => {
    expect(decideTranscode(null, '.mp3', 'mp3-320').reason).toBe('match')
    expect(decideTranscode(null, '.opus', 'flac').reason).toBe('skip-upconvert')
  })
})

describe('targetExtFor / qualityArgs', () => {
  it('maps each quality to its extension', () => {
    expect(targetExtFor('mp3-320')).toBe('.mp3')
    expect(targetExtFor('aac-256')).toBe('.m4a')
    expect(targetExtFor('flac')).toBe('.flac')
  })
  it('keeps the yt-dlp audio-format mapping', () => {
    expect(qualityArgs('mp3-320')).toEqual(['--audio-format', 'mp3', '--audio-quality', '0'])
    expect(qualityArgs('aac-256')).toEqual(['--audio-format', 'm4a', '--audio-quality', '256K'])
    expect(qualityArgs('flac')).toEqual(['--audio-format', 'flac'])
    // unknown values fall back to mp3 (matches historical behavior)
    expect(qualityArgs('whatever')).toEqual(['--audio-format', 'mp3', '--audio-quality', '0'])
  })
})
