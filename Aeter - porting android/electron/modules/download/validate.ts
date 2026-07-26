import { stat } from 'node:fs/promises'
import { parseFile } from 'music-metadata'

/** Smallest plausible real audio file; anything below is a truncated leftover. */
export const MIN_BYTES = 32 * 1024

export interface FileValidation {
  ok: boolean
  reason?: 'missing' | 'too-small' | 'unparsable' | 'zero-duration'
}

/**
 * Post-download integrity check: the file exists, is at least 32 KB, parses
 * as audio and has a positive duration. Catches truncated/corrupted results
 * that yt-dlp/spotdl occasionally leave behind on flaky connections.
 */
export async function validateAudioFile(path: string): Promise<FileValidation> {
  let size: number
  try {
    size = (await stat(path)).size
  } catch {
    return { ok: false, reason: 'missing' }
  }
  if (size < MIN_BYTES) return { ok: false, reason: 'too-small' }
  try {
    const meta = await parseFile(path, { duration: true })
    if (!meta.format.duration || meta.format.duration <= 0) {
      return { ok: false, reason: 'zero-duration' }
    }
  } catch {
    return { ok: false, reason: 'unparsable' }
  }
  return { ok: true }
}
