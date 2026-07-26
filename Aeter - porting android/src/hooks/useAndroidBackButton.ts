import { useEffect, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { App } from '@capacitor/app'
import type { PluginListenerHandle } from '@capacitor/core'
import i18n from '@/i18n'
import { isMobile } from '@/lib/platform'
import { popBack } from '@/lib/backStack'
import { useUiStore } from '@/store/useUiStore'
import { toast } from '@/store/useToastStore'

/**
 * Hardware back button for Android (journal #9 gap: the app used to just exit on
 * every back press because Capacitor's default `backButton` behaviour bubbles to
 * `App.exitApp()`).
 *
 * Precedence on each press:
 *   1. If any overlay/sheet is open, dismiss the topmost one and stop.
 *   2. Otherwise, if we're on a detail route, navigate back within the SPA.
 *   3. Otherwise (a root tab, nothing open), require a second press within
 *      EXIT_WINDOW_MS to exit — so an accidental tap doesn't close the app.
 *
 * No-op on desktop. The listener is registered once and removed on unmount.
 */
// Includes '/home' (the default landing tab) and '/' (the instant before the
// router redirects to it) so a back press there triggers press-again-to-exit
// rather than navigate(-1) into empty SPA history.
const ROOT_PATHS = new Set(['/', '/home', '/library', '/albums', '/playlists', '/settings'])
const EXIT_WINDOW_MS = 2000

/** Current SPA route, read from the HashRouter hash (e.g. "#/albums/12" → "/albums/12"). */
function currentPath(): string {
  const hash = window.location.hash || '#/'
  const path = hash.replace(/^#/, '')
  return path.split('?')[0] || '/'
}

export function useAndroidBackButton(): void {
  const navigate = useNavigate()
  const lastBackAt = useRef(0)

  useEffect(() => {
    if (!isMobile) return

    // Ordered most-modal → least-modal so the topmost layer is closed first.
    const dismissTopmost = (): boolean => {
      // Layer 0: overlays with component-local state (bottom sheets, editors,
      // flows, tour, selection modes) self-register on the back stack while
      // open — they render above the store-tracked overlays.
      if (popBack()) return true
      const ui = useUiStore.getState()
      const layers: Array<[boolean, () => void]> = [
        [ui.lyricsEditTrackId !== null, () => ui.setLyricsEditTrackId(null)],
        [ui.editTrackId !== null, () => ui.setEditTrackId(null)],
        [ui.batchEditTrackIds !== null, () => ui.setBatchEditTrackIds(null)],
        [ui.sleepMenuOpen, () => ui.setSleepMenuOpen(false)],
        [ui.searchOpen, () => ui.setSearchOpen(false)],
        [ui.eqOpen, () => ui.setEqOpen(false)],
        [ui.fullscreenViz, () => ui.setFullscreenViz(false)],
        [ui.lyricsOpen, () => ui.setLyricsOpen(false)],
        [ui.nowPlayingOpen, () => ui.setNowPlayingOpen(false)],
        [ui.queueOpen, () => ui.setQueueOpen(false)]
      ]
      const layer = layers.find(([open]) => open)
      if (!layer) return false
      layer[1]()
      return true
    }

    const onBack = (): void => {
      if (dismissTopmost()) return

      if (!ROOT_PATHS.has(currentPath())) {
        navigate(-1)
        return
      }

      // At a root with nothing open: confirm exit with a second press.
      const now = Date.now()
      if (now - lastBackAt.current < EXIT_WINDOW_MS) {
        App.exitApp().catch((err) => {
          console.error('[backButton] App.exitApp failed', err)
        })
        return
      }
      lastBackAt.current = now
      toast.info(i18n.t('common.pressBackToExit'))
    }

    let handle: PluginListenerHandle | undefined
    let cancelled = false
    App.addListener('backButton', onBack)
      .then((h) => {
        if (cancelled) void h.remove()
        else handle = h
      })
      // A rejection here means the App plugin's NATIVE half is missing from the
      // APK (cap sync not re-run / capacitor.plugins.json empty): the system
      // then handles Back itself and backgrounds the app with overlays still
      // open. Fail loudly so logcat names the culprit instead of a silent no-op.
      .catch((err) => {
        console.error(
          '[backButton] App.addListener failed — @capacitor/app native plugin missing from the APK?',
          err
        )
      })

    return () => {
      cancelled = true
      void handle?.remove()
    }
  }, [navigate])
}
