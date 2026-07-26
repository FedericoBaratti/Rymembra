import { useEffect, useRef } from 'react'
import { isMobile } from '@/lib/platform'
import { pushBack } from '@/lib/backStack'

/**
 * While `active` is true, register `dismiss` on the Android back stack so the
 * hardware back button closes this overlay instead of navigating/exiting.
 *
 * `dismiss` is read through a ref, so callers may pass an inline closure —
 * the registration itself only tracks `active`. No-op on desktop.
 */
export function useBackDismiss(active: boolean, dismiss: () => void): void {
  const dismissRef = useRef(dismiss)
  dismissRef.current = dismiss

  useEffect(() => {
    if (!active || !isMobile) return
    return pushBack(() => dismissRef.current())
  }, [active])
}
