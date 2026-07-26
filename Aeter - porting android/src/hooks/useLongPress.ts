import { useCallback, useEffect, useRef } from 'react'

/**
 * Touch long-press → context-menu trigger. The desktop UI opens row menus on
 * right-click (`onContextMenu`); touch devices have no right-click, so we map a
 * ~450ms press-and-hold to the same handler. The press is cancelled if the
 * finger moves past a small slop radius (i.e. the user is scrolling) or lifts
 * early.
 *
 * The gesture has a tail the callback's UI must not see: Android fires its
 * native `contextmenu` ~500ms into the press, and the browser synthesizes a
 * `click` when the finger lifts. Both would land on whatever is under the
 * finger by then (the just-opened menu, a row, or — retargeted — `body`) and
 * trip "close on outside click" listeners or start playback. The hook swallows
 * them with one-shot capture-phase window listeners, so consumers must NOT
 * keep their own suppress-click guards (the click never reaches them, and a
 * ref-based guard would go stale and eat the next legitimate tap).
 *
 * Returns props to spread on the target element. The callback receives the
 * originating pointer event so callers can place the menu at the touch point.
 */
export interface LongPressHandlers {
  onPointerDown: (e: React.PointerEvent) => void
  onPointerUp: () => void
  onPointerMove: (e: React.PointerEvent) => void
  onPointerCancel: () => void
}

/**
 * One-shot capture-phase window suppressor; self-removes on first event.
 * Capture on `window` is the first stop in every propagation path, so it
 * preempts React's delegated handlers (attached on the root/portal containers)
 * and any raw bubble listeners regardless of the event's target.
 */
function armSuppressor(
  slot: React.MutableRefObject<(() => void) | null>,
  type: 'contextmenu' | 'click'
): void {
  slot.current?.()
  const handler = (e: Event): void => {
    e.preventDefault()
    e.stopImmediatePropagation()
    slot.current?.()
  }
  window.addEventListener(type, handler, true)
  slot.current = (): void => {
    slot.current = null
    window.removeEventListener(type, handler, true)
  }
}

export function useLongPress(
  onLongPress: (e: React.PointerEvent) => void,
  { delay = 450, moveTolerance = 10 }: { delay?: number; moveTolerance?: number } = {}
): LongPressHandlers {
  const timer = useRef<number | undefined>(undefined)
  const origin = useRef<{ x: number; y: number } | null>(null)
  const fired = useRef(false)
  const removeCtxSuppressor = useRef<(() => void) | null>(null)
  const removeClickSuppressor = useRef<(() => void) | null>(null)
  const teardownTimer = useRef<number | undefined>(undefined)

  const clear = useCallback(() => {
    window.clearTimeout(timer.current)
    timer.current = undefined
    origin.current = null
  }, [])

  const disarmSuppressors = useCallback(() => {
    window.clearTimeout(teardownTimer.current)
    teardownTimer.current = undefined
    removeCtxSuppressor.current?.()
    removeClickSuppressor.current?.()
  }, [])

  useEffect(() => {
    return (): void => {
      window.clearTimeout(timer.current)
      disarmSuppressors()
    }
  }, [disarmSuppressors])

  const onPointerDown = useCallback(
    (e: React.PointerEvent) => {
      // A rapid re-press within the post-lift grace window must reclaim its
      // own click, or an armed suppressor would swallow the new tap.
      disarmSuppressors()
      fired.current = false
      // Touch/pen only — mouse keeps its native contextmenu path.
      if (e.pointerType === 'mouse') return
      origin.current = { x: e.clientX, y: e.clientY }
      // The event object is pooled; snapshot the fields the callback needs.
      const snapshot = {
        clientX: e.clientX,
        clientY: e.clientY,
        pointerType: e.pointerType,
        preventDefault: () => {},
        stopPropagation: () => {}
      } as unknown as React.PointerEvent
      timer.current = window.setTimeout(() => {
        fired.current = true
        // Android's native long-press contextmenu lands ~50ms from now; on
        // Windows touchscreens it arrives at finger-lift instead, so the
        // suppressor stays armed through the post-lift grace period.
        armSuppressor(removeCtxSuppressor, 'contextmenu')
        onLongPress(snapshot)
        clear()
      }, delay)
    },
    [onLongPress, delay, clear, disarmSuppressors]
  )

  const onPointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!origin.current) return
      const dx = Math.abs(e.clientX - origin.current.x)
      const dy = Math.abs(e.clientY - origin.current.y)
      if (dx > moveTolerance || dy > moveTolerance) clear()
    },
    [moveTolerance, clear]
  )

  const onPointerUp = useCallback((): void => {
    if (fired.current) {
      fired.current = false
      // The synthesized click follows pointerup within ~10ms. If the finger
      // moved after firing no click ever comes, so a teardown reaps the
      // suppressors before they can eat a later legitimate event.
      armSuppressor(removeClickSuppressor, 'click')
      teardownTimer.current = window.setTimeout(disarmSuppressors, 150)
    }
    clear()
  }, [clear, disarmSuppressors])

  const onPointerCancel = useCallback((): void => {
    // No click or contextmenu follows a cancelled pointer.
    fired.current = false
    disarmSuppressors()
    clear()
  }, [clear, disarmSuppressors])

  return { onPointerDown, onPointerUp, onPointerMove, onPointerCancel }
}
