import { useEffect } from 'react'

/**
 * On Android this consumes the hardware back button to dismiss an overlay. On
 * desktop there is no back button, so it maps the equivalent gesture — Escape —
 * to the same dismiss, giving modals/sheets a keyboard close. Inert when
 * `active` is false.
 */
export function useBackDismiss(active: boolean, dismiss: () => void): void {
  useEffect(() => {
    if (!active) return
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') {
        e.preventDefault()
        dismiss()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [active, dismiss])
}
