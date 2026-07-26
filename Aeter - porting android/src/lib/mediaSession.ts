import { Capacitor, registerPlugin } from '@capacitor/core'
import { usePlayerStore } from '@/store/usePlayerStore'
import { playerEngine } from '@/lib/player'
import { coverUrl } from '@/lib/format'

/**
 * Mobile MediaSession integration: mirrors the WebView player into the Android
 * media notification + lock-screen controls, and routes hardware/notification
 * transport actions back into the player store. Audio itself stays in the
 * WebView (Howler); the MediaSessionPlugin runs a foreground service so
 * playback survives backgrounding.
 *
 * Renderer-only — initialised from src/main.mobile.tsx when isMobile.
 */
interface MediaSessionPlugin {
  setMetadata(o: {
    title: string
    artist: string
    album: string
    artworkUrl: string
    duration: number
  }): Promise<void>
  setPlaybackState(o: { playing: boolean; position: number }): Promise<void>
  stop(): Promise<void>
  addListener(
    event: 'transport',
    cb: (d: { action: string; value?: number }) => void
  ): Promise<{ remove: () => Promise<void> }>
}

const MS = registerPlugin<MediaSessionPlugin>('MediaSession')

export function initMediaSession(): void {
  // Skip cleanly if the native plugin isn't registered in this APK (otherwise
  // every MS.* call rejects with "not implemented on android" as an unhandled
  // promise rejection). Lock-screen/notification controls are non-essential.
  if (!Capacitor.isPluginAvailable('MediaSession')) return

  const store = usePlayerStore

  // Transport (notification / lock screen / headset) → player store.
  void MS.addListener('transport', ({ action, value }) => {
    const s = store.getState()
    switch (action) {
      case 'play':
      case 'pause':
        s.togglePlay()
        break
      case 'next':
        s.next()
        break
      case 'previous':
        s.previous()
        break
      case 'stop':
        s.clearQueue()
        void MS.stop()
        break
      case 'seek':
        if (value != null) playerEngine.seek(value / 1000)
        break
    }
  })

  // Push metadata whenever the track changes.
  let lastTrackId: number | null = null
  let lastPlaying: boolean | null = null
  const sync = (): void => {
    const { currentTrack, isPlaying } = store.getState()
    if (currentTrack && currentTrack.id !== lastTrackId) {
      lastTrackId = currentTrack.id
      void MS.setMetadata({
        title: currentTrack.title,
        artist: currentTrack.artist,
        album: currentTrack.album,
        artworkUrl: coverUrl(currentTrack.cover_art_hash) ?? '',
        duration: currentTrack.duration
      })
    }
    if (!currentTrack && lastTrackId !== null) {
      lastTrackId = null
      void MS.stop()
    }
    if (isPlaying !== lastPlaying) {
      lastPlaying = isPlaying
      void MS.setPlaybackState({ playing: isPlaying, position: playerEngine.position() })
    }
  }
  store.subscribe(sync)
  sync()

  // Keep the notification scrubber roughly in sync while playing. The timer
  // exists only while something is audible — no periodic wake-ups at rest.
  let posTimer: ReturnType<typeof setInterval> | null = null
  const syncPosTimer = (): void => {
    const { currentTrack, isPlaying } = store.getState()
    const want = Boolean(currentTrack && isPlaying)
    if (want && posTimer == null) {
      posTimer = setInterval(() => {
        void MS.setPlaybackState({ playing: true, position: playerEngine.position() })
      }, 2000)
    } else if (!want && posTimer != null) {
      clearInterval(posTimer)
      posTimer = null
    }
  }
  store.subscribe(syncPosTimer)
  syncPosTimer()
}
