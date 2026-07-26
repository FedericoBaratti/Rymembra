import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useBackDismiss } from '@/hooks/useBackDismiss'

/**
 * Material-style bottom sheet for mobile: a panel that rises from the bottom edge
 * with a drag handle, safe-area padding, swipe-to-dismiss, backdrop tap-to-close
 * and Escape support. Used for the equalizer, track actions and the add-tracks
 * picker on Android. Desktop keeps its own floating panels/modals instead.
 */
export default function BottomSheet({
  open,
  onClose,
  title,
  children,
  maxHeight = '85vh',
  scrollBody = true
}: {
  open: boolean
  onClose: () => void
  title?: React.ReactNode
  children: React.ReactNode
  maxHeight?: string
  /**
   * Default: the body is the scroll container. Pass false when a child owns
   * its own scrolling (e.g. a virtualized list) so the two don't nest.
   */
  scrollBody?: boolean
}): React.JSX.Element | null {
  const [dragY, setDragY] = useState(0)
  const startY = useRef<number | null>(null)

  // Android hardware back closes the sheet (covers every BottomSheet-based
  // menu — row/hero menus, sort, EQ, queue, confirm — in one place).
  useBackDismiss(open, onClose)

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])

  useEffect(() => {
    if (open) setDragY(0)
  }, [open])

  if (!open) return null

  const onPointerDown = (e: React.PointerEvent): void => {
    startY.current = e.clientY
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
  }
  const onPointerMove = (e: React.PointerEvent): void => {
    if (startY.current == null) return
    setDragY(Math.max(0, e.clientY - startY.current))
  }
  const onPointerUp = (): void => {
    if (startY.current == null) return
    if (dragY > 110) onClose()
    else setDragY(0)
    startY.current = null
  }
  // The OS can steal the pointer mid-drag (system back-gesture, incoming call,
  // notification shade): pointercancel fires instead of pointerup. Without this
  // the sheet stays translated down with transition disabled — frozen off-screen.
  const onPointerCancel = (): void => {
    startY.current = null
    setDragY(0)
  }

  // Portal su body: montata dentro una pagina, la sheet erediterebbe lo
  // stacking context z-10 di <main> e finirebbe SOTTO player e bottom-nav
  // (z-30 nel root context) nonostante lo z-[70].
  return createPortal(
    <div
      className="overlay-in fixed inset-0 z-[70] flex flex-col justify-end bg-black/55"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
    >
      <div
        className="glass-modal flex flex-col rounded-t-3xl"
        style={{
          // Inline maxHeight wins over the .glass-modal CSS rule, so make it
          // keyboard-aware too: never exceed the band above the soft keyboard.
          maxHeight: `min(${maxHeight}, calc(100vh - var(--kb-height, 0px) - var(--sa-top, 0px) - 24px))`,
          transform: `translateY(${dragY}px)`,
          transition: startY.current == null ? 'transform var(--dur-2) var(--ease-out-expo)' : 'none',
          animation: dragY === 0 && startY.current == null ? 'sheet-up var(--dur-2) var(--ease-out-expo) both' : undefined,
          paddingBottom: 'calc(var(--sa-bottom, env(safe-area-inset-bottom, 0px)) + 12px)',
          boxShadow: '0 -8px 40px rgba(0,0,0,0.5)'
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Drag handle — full 44px hit strip (the visible pill alone was a
            ~20px target), brighter grabber for clearer affordance */}
        <div
          className="sheet-handle flex h-11 shrink-0 cursor-grab touch-none items-center justify-center active:cursor-grabbing"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerCancel}
        >
          <span className="h-1.5 w-11 rounded-full bg-white/30 transition-colors" />
        </div>

        {title != null && (
          <div className="sheet-title shrink-0 px-5 pb-2 pt-1 text-center text-[15px] font-bold">{title}</div>
        )}

        <div className={scrollBody ? 'min-h-0 flex-1 overflow-y-auto' : 'flex min-h-0 flex-1 flex-col overflow-hidden'}>
          {children}
        </div>
      </div>
    </div>,
    document.body
  )
}
