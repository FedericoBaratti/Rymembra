import type { SourceType } from '@shared/types'

// Pure URL detection. No electron imports (unit-tested).

export interface ParsedUrl {
  type: SourceType
  spotifyKind?: 'track' | 'album' | 'artist' | 'playlist'
  spotifyId?: string
}

export function detectUrl(url: string): ParsedUrl | null {
  const sp = /open\.spotify\.com\/(?:intl-[a-z]+\/)?(track|album|artist|playlist)\/([A-Za-z0-9]+)/.exec(url)
  if (sp) {
    const kind = sp[1] as 'track' | 'album' | 'artist' | 'playlist'
    return { type: `spotify-${kind}` as SourceType, spotifyKind: kind, spotifyId: sp[2] }
  }
  if (/(?:youtube\.com|music\.youtube\.com)\/.*[?&]list=|youtube\.com\/playlist/.test(url)) {
    return { type: 'youtube-playlist' }
  }
  if (/youtu\.be\/[\w-]+|(?:youtube\.com|music\.youtube\.com)\/watch\?/.test(url)) {
    return { type: 'youtube-video' }
  }
  return null
}
