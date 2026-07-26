/**
 * Build-time platform flag.
 *
 * The mobile renderer build (vite.config.mobile.ts) injects
 * `define: { __AETHER_MOBILE__: true }`, so on Android this collapses to a
 * constant `true` and the desktop-only branches tree-shake away. On the
 * Electron build the symbol is never defined, so the `typeof` guard keeps it a
 * safe `false` at runtime.
 */
function buildTimeMobile(): boolean {
  return typeof __AETHER_MOBILE__ !== 'undefined' && __AETHER_MOBILE__ === true
}

/**
 * Runtime fallback: inside the Android WebView, Capacitor injects a global
 * `window.Capacitor` with `isNativePlatform() === true`. This guarantees the
 * mobile layout (BottomNav instead of the desktop Sidebar, safe-area insets, no
 * titlebar) even if the `__AETHER_MOBILE__` build define is ever missing from
 * the bundle that ships in the APK. On desktop Electron `window.Capacitor` is
 * undefined, so this stays false and the desktop chrome is unaffected.
 */
function nativeRuntimeMobile(): boolean {
  if (typeof window === 'undefined') return false
  const cap = (window as unknown as { Capacitor?: { isNativePlatform?: () => boolean; platform?: string } })
    .Capacitor
  try {
    if (cap?.isNativePlatform?.() === true) return true
    return cap?.platform === 'android' || cap?.platform === 'ios'
  } catch {
    return false
  }
}

export const isMobile: boolean = buildTimeMobile() || nativeRuntimeMobile()
