/**
 * Platform flag. On the Electron desktop build there is no Capacitor runtime, so
 * this is always `false` and the desktop chrome (Sidebar, titlebar, no safe-area
 * insets) is always used. Kept as a function-guarded check purely so the value
 * is computed defensively; it never becomes true on desktop.
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

export const isMobile: boolean = nativeRuntimeMobile()
