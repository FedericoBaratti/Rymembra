import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { useSettingsStore } from '@/store/useSettingsStore'
import { useDownloadsStore } from '@/store/useDownloadsStore'
import { useTourStore } from '@/store/useTourStore'
import { useSpotifyMigrationStore } from '@/store/useSpotifyMigrationStore'
import { usePhoneSyncStore } from '@/store/usePhoneSyncStore'
import { toast } from '@/store/useToastStore'
import { playerEngine } from '@/lib/player'
import { audioGraph } from '@/lib/audio'
import { coverUrl } from '@/lib/format'
import { translateErrorCode } from '@/lib/ipcError'
import { applySkin } from '@/lib/skins'
import i18next from 'i18next'

export function applyTheme(theme: 'dark' | 'light' | 'system'): void {
  const dark =
    theme === 'dark' ||
    (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.dataset.theme = dark ? 'dark' : 'light'
}

/** One-time app init: settings, IPC event subscriptions, MediaSession, gapless preload. */
export function useAppBootstrap(): void {
  const { i18n } = useTranslation()
  const currentTrack = usePlayerStore((s) => s.currentTrack)
  const isPlaying = usePlayerStore((s) => s.isPlaying)

  useEffect(() => {
    void useSettingsStore
      .getState()
      .load()
      .then((s) => {
        usePlayerStore.setState({ volume: s.volume, muted: s.muted })
        playerEngine.setVolume(s.volume, s.muted)
        playerEngine.configure({
          crossfadeSec: s.crossfadeSeconds,
          rgEnabled: s.replayGainEnabled,
          rgTargetDb: s.replayGainTargetDb
        })
        audioGraph.setEq(s.eqGains, s.eqEnabled)
        void i18n.changeLanguage(s.language)
        applyTheme(s.theme)
        applySkin(s.skin)
        if (!s.hasSeenOnboarding) {
          // Let the first paint settle before the welcome card fades in
          setTimeout(() => useTourStore.getState().start(), 600)
        }
      })

    // Restore the last playback queue (paused, no autoplay)
    void window.aether.getQueueState().then(async (q) => {
      if (!q || q.trackIds.length === 0) return
      const tracks = await window.aether.getTracksByIds(q.trackIds)
      if (tracks.length > 0) usePlayerStore.getState().restoreQueue(tracks, q)
    })

    void useLibraryStore.getState().refreshAll()
    void useDownloadsStore.getState().refresh()
    // Pick up any Spotify migration resumed on the backend at boot.
    void useSpotifyMigrationStore.getState().refresh()

    const offs = [
      window.aether.on('spotify:migration', (state) => {
        useSpotifyMigrationStore.getState().set(state)
      }),
      window.aether.on('scan:progress', (p) => {
        useLibraryStore.getState().setScanProgress(p.phase === 'done' ? null : p)
      }),
      window.aether.on('library:changed', () => {
        useLibraryStore.getState().refreshAllDebounced()
      }),
      window.aether.on('track:updated', (track) => {
        usePlayerStore.getState().updateTrack(track)
        // Patch the one row in place; a full getTracks() per event was O(N²)
        // during enrichment of large libraries (see useLibraryStore.patchTrack).
        useLibraryStore.getState().patchTrack(track)
      }),
      window.aether.on('download:updated', (item) => {
        // Toast only on the transition into a terminal status, not on re-emits.
        // A first-seen item (no prev in the store — e.g. queued while the
        // renderer was reloading) must still notify, so prev == null counts
        // as a transition too.
        const prev = useDownloadsStore.getState().items.find((i) => i.id === item.id)
        if (!prev || prev.status !== item.status) {
          if (item.status === 'completed') {
            toast.success(i18next.t('toast.download_complete'), item.title)
          } else if (item.status === 'error') {
            const reason = item.error_message ? translateErrorCode(item.error_message) : ''
            toast.error(
              i18next.t('toast.download_failed'),
              reason ? `${item.title} — ${reason}` : item.title
            )
          }
        }
        useDownloadsStore.getState().upsert(item)
      }),
      window.aether.on('media-key', (action) => {
        const p = usePlayerStore.getState()
        if (action === 'play-pause') p.togglePlay()
        else if (action === 'next') p.next()
        else if (action === 'previous') p.previous()
        else if (action === 'stop' && p.isPlaying) p.togglePlay()
      }),
      window.aether.on('phone:state', (state) => {
        usePhoneSyncStore.getState().setState(state)
      }),
      window.aether.on('phoneRepair:updated', (item) => {
        usePhoneSyncStore.getState().applyRepairUpdate(item)
      })
    ]

    // Gapless: preload the next track when the current one is close to its end
    const preloadTicker = window.setInterval(() => {
      const p = usePlayerStore.getState()
      if (!p.isPlaying || !p.currentTrack) return
      const remaining = playerEngine.duration() - playerEngine.position()
      if (remaining > 0 && remaining < 10) {
        playerEngine.preload(p.peekNext())
      }
    }, 1000)

    const ms = navigator.mediaSession
    ms.setActionHandler('play', () => usePlayerStore.getState().togglePlay())
    ms.setActionHandler('pause', () => usePlayerStore.getState().togglePlay())
    ms.setActionHandler('nexttrack', () => usePlayerStore.getState().next())
    ms.setActionHandler('previoustrack', () => usePlayerStore.getState().previous())
    ms.setActionHandler('seekto', (d) => {
      if (d.seekTime != null) playerEngine.seek(d.seekTime)
    })

    return () => {
      offs.forEach((off) => off())
      window.clearInterval(preloadTicker)
    }
  }, [i18n])

  // MediaSession metadata + OS notification on track change
  useEffect(() => {
    if (!currentTrack) {
      navigator.mediaSession.metadata = null
      return
    }
    const art = coverUrl(currentTrack.cover_art_hash)
    navigator.mediaSession.metadata = new MediaMetadata({
      title: currentTrack.title,
      artist: currentTrack.artist,
      album: currentTrack.album,
      artwork: art ? [{ src: art, sizes: '512x512', type: 'image/webp' }] : []
    })
    const settings = useSettingsStore.getState().settings
    if (settings?.notificationsOnTrackChange && document.hidden) {
      try {
        const n = new Notification(currentTrack.title, {
          body: `${currentTrack.artist} — ${currentTrack.album}`,
          icon: art ?? undefined,
          silent: true
        })
        setTimeout(() => n.close(), 5000)
      } catch {
        // notifications unavailable
      }
    }
  }, [currentTrack])

  useEffect(() => {
    navigator.mediaSession.playbackState = isPlaying ? 'playing' : 'paused'
  }, [isPlaying])
}
