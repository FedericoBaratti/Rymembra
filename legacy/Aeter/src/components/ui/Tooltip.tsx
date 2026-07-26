import { cloneElement, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

const SHOW_DELAY_MS = 450

/**
 * Styled replacement for native title= tooltips on chrome surfaces
 * (player bar, sidebar rail). Clones its single child to attach
 * hover/focus handlers; the pill renders in a portal so it escapes
 * overflow clipping. Do not use on virtualized list rows.
 */
export default function Tooltip({
  label,
  side = 'top',
  children
}: {
  label: string
  side?: 'top' | 'bottom'
  children: React.ReactElement<React.HTMLAttributes<HTMLElement>>
}): React.JSX.Element {
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null)
  const timer = useRef<number | null>(null)

  const clearTimer = (): void => {
    if (timer.current != null) {
      window.clearTimeout(timer.current)
      timer.current = null
    }
  }

  const schedule = (el: HTMLElement): void => {
    clearTimer()
    timer.current = window.setTimeout(() => {
      const r = el.getBoundingClientRect()
      setPos({ x: r.left + r.width / 2, y: side === 'top' ? r.top - 7 : r.bottom + 7 })
    }, SHOW_DELAY_MS)
  }

  const hide = (): void => {
    clearTimer()
    setPos(null)
  }

  useEffect(() => clearTimer, [])

  useEffect(() => {
    if (!pos) return
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') hide()
    }
    window.addEventListener('scroll', hide, true)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('scroll', hide, true)
      window.removeEventListener('keydown', onKey)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pos != null])

  const props = children.props
  const child = cloneElement(children, {
    onMouseEnter: (e: React.MouseEvent<HTMLElement>) => {
      props.onMouseEnter?.(e)
      schedule(e.currentTarget)
    },
    onMouseLeave: (e: React.MouseEvent<HTMLElement>) => {
      props.onMouseLeave?.(e)
      hide()
    },
    onFocus: (e: React.FocusEvent<HTMLElement>) => {
      props.onFocus?.(e)
      schedule(e.currentTarget)
    },
    onBlur: (e: React.FocusEvent<HTMLElement>) => {
      props.onBlur?.(e)
      hide()
    },
    onClick: (e: React.MouseEvent<HTMLElement>) => {
      props.onClick?.(e)
      hide()
    }
  })

  return (
    <>
      {child}
      {pos &&
        createPortal(
          <div
            className="tooltip-wrap"
            style={{
              left: pos.x,
              top: pos.y,
              transform: side === 'top' ? 'translate(-50%, -100%)' : 'translate(-50%, 0)'
            }}
          >
            <div
              className="tooltip-pill"
              role="tooltip"
              style={
                {
                  '--menu-origin': side === 'top' ? 'bottom center' : 'top center'
                } as React.CSSProperties
              }
            >
              {label}
            </div>
          </div>,
          document.body
        )}
    </>
  )
}
