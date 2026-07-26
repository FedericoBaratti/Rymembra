import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { useSettingsStore } from '@/store/useSettingsStore'
import { useDownloadsStore } from '@/store/useDownloadsStore'
import { useSpotifyMigrationStore } from '@/store/useSpotifyMigrationStore'
import { useTourStore } from '@/store/useTourStore'
import { toast } from '@/store/useToastStore'
import { playerEngine } from '@/lib/player'
import { applyEq } from '@/lib/audio'
import { translateErrorCode } from '@/lib/ipcError'
import { coverUrl } from '@/lib/format'
import { isMobile } from '@/lib/platform'
import { ensureAndroidMusicStorage } from '@/lib/androidStorage'
import { applySkin } from '@/lib/skins'
import i18next from 'i18next'

export function applyTheme(theme: 'dark' | 'light' | 'system'): void {
  const dark =
    theme === 'dark' ||
    (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.dataset.theme = dark ? 'dark' : 'light'
}

// Re-exported so callers (AppearanceSection) import skin + theme appliers from
// one place; the implementation lives in the skin registry (src/lib/skins.ts).
export { applySkin }

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
          rgTargetDb: s.replayGainTargetDb,
          offloadEnabled: s.audioOffloadEnabled
        })
        applyEq(s.eqGains, s.eqEnabled)
        void i18n.changeLanguage(s.language)
        applyTheme(s.theme)
        applySkin(s.skin)
        if (!s.hasSeenOnboarding) {
          // Let the first paint settle before the welcome card fades in
          setTimeout(() => useTourStore.getState().start(), 600)
        }
        // Android: auto-create/load the Download/Music library folder (asks for
        // All Files Access on first launch). No-op on desktop/web.
        if (isMobile) void ensureAndroidMusicStorage(s)
      })

    // Restore the last playback queue (paused, no autoplay). Best-effort: an
    // early IPC reject (backend not ready yet on Android) must not surface as an
    // unhandled rejection — the queue simply isn't restored.
    void window.aether
      .getQueueState()
      .then(async (q) => {
        if (!q || q.trackIds.length === 0) return
        const tracks = await window.aether.getTracksByIds(q.trackIds)
        if (tracks.length > 0) usePlayerStore.getState().restoreQueue(tracks, q)
      })
      .catch(() => {})

    void useLibraryStore.getState().refreshAll()
    void useDownloadsStore.getState().refresh()
    void useSpotifyMigrationStore.getState().refresh()

    const offs = [
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
      window.aether.on('duplicates:removed', ({ count }) => {
        if (count > 0) toast.success(i18next.t('toast.duplicates_removed', { count }))
      }),
      window.aether.on('spotify:migration', (s) => {
        useSpotifyMigrationStore.getState().set(s)
      }),
      // The backend hit an uncaught error (Android crash safety net): it stays
      // alive but may be degraded. Surface it — silence here would just turn
      // into unexplained weirdness later.
      window.aether.on('backend:fatal', () => {
        toast.error(i18next.t('errors.backend_fatal'))
      }),
      window.aether.on('media-key', (action) => {
        const p = usePlayerStore.getState()
        if (action === 'play-pause') p.togglePlay()
        else if (action === 'next') p.next()
        else if (action === 'previous') p.previous()
        else if (action === 'stop' && p.isPlaying) p.togglePlay()
      })
    ]

    // Persist backend state when the app is backgrounded/hidden. Android kills
    // the nodejs-mobile process without warning, so flush the debounced
    // DB/settings/queue writes now. Mobile only (desktop flushes on quit); this
    // mirrors the native MainActivity.onPause hook — both are best-effort.
    const flushOnHide = (): void => {
      if (document.visibilityState === 'hidden') {
        void window.aether.flushNow()
        // Tell the backend we're backgrounded so it pauses periodic work
        // (missing-fetch worker, Drive sync) and relaxes the DB flush cadence
        // while the node process is kept alive by a foreground service.
        void window.aether.setAppState('background')
      }
      // On resume, reconcile the downloads queue: `download:updated` events
      // pushed while the process/WebView was backgrounded (or killed) are lost,
      // which used to leave frozen progress bars and stale button states.
      if (document.visibilityState === 'visible') {
        void window.aether.setAppState('foreground')
        void useDownloadsStore.getState().refresh()
      }
    }
    if (isMobile) {
      document.addEventListener('visibilitychange', flushOnHide)
      window.addEventListener('pagehide', flushOnHide)
      // Initial state (the app may bootstrap already hidden, e.g. relaunched
      // from the media notification with the screen off).
      void window.aether.setAppState(
        document.visibilityState === 'hidden' ? 'background' : 'foreground'
      )
    }

    // MediaSession is unavailable in some WebViews; guard before using it.
    const ms = navigator.mediaSession
    if (ms) {
      ms.setActionHandler('play', () => usePlayerStore.getState().togglePlay())
      ms.setActionHandler('pause', () => usePlayerStore.getState().togglePlay())
      ms.setActionHandler('nexttrack', () => usePlayerStore.getState().next())
      ms.setActionHandler('previoustrack', () => usePlayerStore.getState().previous())
      ms.setActionHandler('seekto', (d) => {
        if (d.seekTime != null) playerEngine.seek(d.seekTime)
      })
    }

    return () => {
      offs.forEach((off) => off())
      if (isMobile) {
        document.removeEventListener('visibilitychange', flushOnHide)
        window.removeEventListener('pagehide', flushOnHide)
      }
    }
  }, [i18n])

  // Gapless: preload the next track when the current one is close to its end.
  // Desktop only — on mobile the native ExoPlayer queue pre-buffers by itself
  // and NativeAudio.preload() is a documented no-op, so the timer would just
  // wake the CPU once a second for nothing. Keyed on isPlaying so it doesn't
  // tick while paused/idle either.
  useEffect(() => {
    if (isMobile || !isPlaying) return
    const preloadTicker = window.setInterval(() => {
      const p = usePlayerStore.getState()
      if (!p.currentTrack) return
      const remaining = playerEngine.duration() - playerEngine.position()
      if (remaining > 0 && remaining < 10) {
        playerEngine.preload(p.peekNext())
      }
    }, 1000)
    return () => window.clearInterval(preloadTicker)
  }, [isPlaying])

  // MediaSession metadata + OS notification on track change
  useEffect(() => {
    const ms = navigator.mediaSession
    if (!currentTrack) {
      if (ms) ms.metadata = null
      return
    }
    const art = coverUrl(currentTrack.cover_art_hash)
    if (ms && typeof MediaMetadata !== 'undefined') {
      ms.metadata = new MediaMetadata({
        title: currentTrack.title,
        artist: currentTrack.artist,
        album: currentTrack.album,
        artwork: art ? [{ src: art, sizes: '512x512', type: 'image/webp' }] : []
      })
    }
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
    if (navigator.mediaSession) navigator.mediaSession.playbackState = isPlaying ? 'playing' : 'paused'
  }, [isPlaying])
}
