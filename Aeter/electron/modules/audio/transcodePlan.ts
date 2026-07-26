import type { DownloadQuality } from '@shared/types'

// Pure codec-standardization logic (no electron/ffmpeg imports → unit-tested).
// Single source of truth for the target-format mapping that used to be
// duplicated in download/sources/youtube.ts and download/spotifyMatch.ts.

/** yt-dlp/spotdl `--audio-format` args for a download-quality setting. */
export function qualityArgs(quality: string): string[] {
  switch (quality) {
    case 'flac':
      return ['--audio-format', 'flac']
    case 'aac-256':
      return ['--audio-format', 'm4a', '--audio-quality', '256K']
    default:
      return ['--audio-format', 'mp3', '--audio-quality', '0']
  }
}

export type CodecFamily =
  | 'mp3'
  | 'aac'
  | 'alac'
  | 'flac'
  | 'opus'
  | 'vorbis'
  | 'wma'
  | 'pcm'
  | 'unknown'

const LOSSY_FAMILIES: ReadonlySet<CodecFamily> = new Set(['mp3', 'aac', 'opus', 'vorbis', 'wma'])

/**
 * Codec family from the DB `codec` column (music-metadata's `format.codec ??
 * format.container`, e.g. "MPEG 1 Layer 3", "AAC", "Opus") with the file
 * extension as fallback when the string is missing or unrecognized.
 */
export function codecFamily(codec: string | null | undefined, ext: string): CodecFamily {
  const c = (codec ?? '').toLowerCase()
  if (c) {
    if (c.includes('alac') || c.includes('apple lossless')) return 'alac'
    if (c.includes('flac')) return 'flac'
    if (c.includes('opus')) return 'opus'
    if (c.includes('vorbis') || c.includes('ogg')) return 'vorbis'
    if (c.includes('wma') || c.includes('windows media')) return 'wma'
    if (c.includes('mp3') || c.includes('layer 3') || c.includes('layer iii')) return 'mp3'
    if (c.includes('aac') || c.includes('mp4a')) return 'aac'
    if (c.includes('pcm') || c.includes('wave') || c.includes('aiff')) return 'pcm'
    // "MPEG audio" containers without a layer hint are mp3 in practice
    if (c.includes('mpeg') && !c.includes('mpeg-4')) return 'mp3'
  }
  switch (ext.toLowerCase().replace(/^\./, '')) {
    case 'mp3':
      return 'mp3'
    case 'm4a':
    case 'aac':
    case 'mp4':
      return 'aac'
    case 'flac':
      return 'flac'
    case 'opus':
      return 'opus'
    case 'ogg':
    case 'oga':
      return 'vorbis'
    case 'wma':
      return 'wma'
    case 'wav':
    case 'aiff':
    case 'aif':
      return 'pcm'
    default:
      return 'unknown'
  }
}

export interface TranscodePlan {
  needed: boolean
  reason: 'match' | 'convert' | 'skip-upconvert'
  /** Extension the file will have when `needed` (always the target's). */
  targetExt: '.mp3' | '.flac' | '.m4a'
}

export function targetExtFor(target: DownloadQuality): TranscodePlan['targetExt'] {
  switch (target) {
    case 'flac':
      return '.flac'
    case 'aac-256':
      return '.m4a'
    default:
      return '.mp3'
  }
}

function targetFamily(target: DownloadQuality): CodecFamily {
  switch (target) {
    case 'flac':
      return 'flac'
    case 'aac-256':
      return 'aac'
    default:
      return 'mp3'
  }
}

/**
 * Decides whether a file should be converted to the standard format.
 * - Same family as the target → never re-encode (lossy→lossy of the same
 *   codec only degrades; flac→flac is pointless).
 * - Lossy source with a flac target → skip: inflating lossy audio into a
 *   lossless container gains nothing.
 * - Everything else (other lossy families, alac/pcm lossless sources) →
 *   convert to the target.
 */
export function decideTranscode(
  codec: string | null | undefined,
  ext: string,
  target: DownloadQuality
): TranscodePlan {
  const targetExt = targetExtFor(target)
  const family = codecFamily(codec, ext)
  if (family === targetFamily(target)) return { needed: false, reason: 'match', targetExt }
  if (target === 'flac' && LOSSY_FAMILIES.has(family)) {
    return { needed: false, reason: 'skip-upconvert', targetExt }
  }
  return { needed: true, reason: 'convert', targetExt }
}

/** ffmpeg encoder args (no input/output paths) for a target quality. */
export function ffmpegEncodeArgs(target: DownloadQuality): string[] {
  switch (target) {
    case 'flac':
      return ['-codec:a', 'flac']
    case 'aac-256':
      return ['-codec:a', 'aac', '-b:a', '256k', '-movflags', '+faststart']
    default:
      return ['-codec:a', 'libmp3lame', '-b:a', '320k', '-id3v2_version', '3']
  }
}
