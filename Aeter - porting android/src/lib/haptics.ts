import { isMobile } from './platform'

/**
 * Lightweight haptic feedback for the Android WebView. Uses the Vibration API
 * (navigator.vibrate), which is available in the Capacitor WebView — no native
 * plugin / cap sync required, so this ships through the renderer-only deploy
 * path (sync:android-web). No-ops on desktop and where the API is absent
 * (e.g. user denied it, or non-mobile builds), so callers can fire it freely.
 *
 * Durations are intentionally tiny: haptics should confirm an action, not
 * announce it. Heavy/long buzzes feel cheap and drain attention.
 */

const canVibrate = isMobile && typeof navigator !== 'undefined' && typeof navigator.vibrate === 'function'

function buzz(ms: number): void {
  if (!canVibrate) return
  try {
    navigator.vibrate(ms)
  } catch {
    /* some WebViews throw if vibration is disabled at the OS level */
  }
}

/** Primary action: play/pause, skip, confirm. */
export const tap = (): void => buzz(10)

/** Light selection: nav change, list selection, toggle. */
export const select = (): void => buzz(8)

/** Stronger confirmation: long-press menu opens, destructive action. */
export const impact = (): void => buzz(16)

export const haptics = { tap, select, impact }
export default haptics
