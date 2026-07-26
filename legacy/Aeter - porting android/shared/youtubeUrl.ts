// Pure URL helper, no electron/react imports (used by renderer and main, unit-tested).

export interface YoutubeUrlSplit {
  /** URL with only the video id — playlist/mix context stripped. */
  videoUrl: string
  /** Original URL untouched: RD mixes only resolve with the watch context. */
  playlistUrl: string
  /** list ids starting with "RD" are auto-generated Mix/Radio, not real playlists. */
  isMix: boolean
}

/**
 * Detects YouTube watch URLs that carry both a video id and a list param
 * (e.g. a video copied while a Mix/Radio or playlist was open) so the UI
 * can ask whether to download the single video or the whole list.
 * Returns null when the URL is unambiguous.
 */
export function splitYoutubeWatchUrl(raw: string): YoutubeUrlSplit | null {
  let url: URL
  try {
    url = new URL(raw.trim())
  } catch {
    return null
  }

  const host = url.hostname.replace(/^www\./, '')
  const list = url.searchParams.get('list')
  if (!list) return null

  let videoId: string | null = null
  let videoUrl: string | null = null
  if ((host === 'youtube.com' || host === 'music.youtube.com') && url.pathname === '/watch') {
    videoId = url.searchParams.get('v')
    if (videoId) videoUrl = `${url.origin}/watch?v=${videoId}`
  } else if (host === 'youtu.be') {
    videoId = url.pathname.slice(1).split('/')[0] || null
    if (videoId) videoUrl = `https://youtu.be/${videoId}`
  }
  if (!videoId || !videoUrl) return null

  return { videoUrl, playlistUrl: raw.trim(), isMix: list.startsWith('RD') }
}
