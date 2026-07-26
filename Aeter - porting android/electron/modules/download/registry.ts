import type { SourceType } from '@shared/types'
import type { SourceHandler } from './types'
import type { ParsedUrl } from './urlDetect'
import { spotifyHandler } from './sources/spotify'
import { youtubeHandler } from './sources/youtube'

// Adding a new source (SoundCloud, Bandcamp, …) = one file in sources/
// plus one entry here. Order matters: first detect() win takes the URL.
const handlers: SourceHandler[] = [spotifyHandler, youtubeHandler]

export function findHandler(url: string): { handler: SourceHandler; parsed: ParsedUrl } | null {
  for (const handler of handlers) {
    const parsed = handler.detect(url)
    if (parsed) return { handler, parsed }
  }
  return null
}

export function handlerForType(sourceType: SourceType): SourceHandler {
  return sourceType.startsWith('spotify') ? spotifyHandler : youtubeHandler
}
