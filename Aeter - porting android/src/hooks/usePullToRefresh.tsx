import { useRef, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { isMobile } from '@/lib/platform'
import { select } from '@/lib/haptics'

// Damped pull distance (px) that commits a refresh; finger travel is ~2x.
const THRESHOLD = 70
const MAX_PULL = 100

/**
 * Android pull-to-refresh for a page's scroll container. Spread `pullProps`
 * on the scrollable element (which must be `relative`) and render `indicator`
 * as its first child. Only arms when the touch starts at scrollTop 0 and moves
 * downward, so normal list scrolling is untouched; the content itself never
 * moves — only the indicator follows the drag (cheap, no layout churn).
 * No-op on desktop.
 */
export function usePullToRefresh(onRefresh?: () => Promise<unknown> | void): {
  pullProps: Partial<React.DOMAttributes<HTMLDivElement>>
  indicator: React.JSX.Element | null
} {
  const [pull, setPull] = useState(0)
  const [dragging, setDragging] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const start = useRef<{ y: number; armed: boolean; buzzed: boolean }>({
    y: 0,
    armed: false,
    buzzed: false
  })

  if (!isMobile || !onRefresh) return { pullProps: {}, indicator: null }

  const reset = (): void => {
    start.current.armed = false
    setDragging(false)
    setPull(0)
  }

  const pullProps: Partial<React.DOMAttributes<HTMLDivElement>> = {
    onTouchStart: (e) => {
      if (refreshing) return
      start.current = {
        y: e.touches[0].clientY,
        armed: e.currentTarget.scrollTop <= 0,
        buzzed: false
      }
    },
    onTouchMove: (e) => {
      const s = start.current
      if (!s.armed || refreshing) return
      // The list scrolled in the meantime (or the gesture goes up): normal scroll.
      if (e.currentTarget.scrollTop > 0) {
        reset()
        return
      }
      const dy = e.touches[0].clientY - s.y
      if (dy <= 0) {
        if (pull !== 0) setPull(0)
        return
      }
      const damped = Math.min(dy / 2, MAX_PULL)
      if (damped >= THRESHOLD && !s.buzzed) {
        s.buzzed = true
        select()
      }
      setDragging(true)
      setPull(damped)
    },
    onTouchEnd: () => {
      const commit = start.current.armed && pull >= THRESHOLD
      reset()
      if (!commit) return
      setRefreshing(true)
      void Promise.resolve()
        .then(() => onRefresh())
        .catch(() => undefined)
        .finally(() => setRefreshing(false))
    },
    onTouchCancel: reset
  }

  const progress = Math.min(pull / THRESHOLD, 1)
  const shown = refreshing || pull > 0
  const indicator = shown ? (
    <div
      aria-hidden
      className="ptr-indicator pointer-events-none absolute left-1/2 z-10"
      style={{
        top: -44,
        transform: `translateX(-50%) translateY(${refreshing ? 60 : pull}px)`,
        transition: dragging ? 'none' : 'transform 200ms ease'
      }}
    >
      <div
        className="ptr-disc flex h-9 w-9 items-center justify-center rounded-full border border-[var(--hairline)] shadow-lg"
        style={{ background: 'var(--color-surface-2)' }}
      >
        <RefreshCw
          size={16}
          className={refreshing ? 'animate-spin' : ''}
          style={{
            transform: refreshing ? undefined : `rotate(${progress * 270}deg)`,
            opacity: refreshing ? 1 : 0.4 + 0.6 * progress,
            color: refreshing || progress >= 1 ? 'var(--accent)' : 'var(--color-text-2)'
          }}
        />
      </div>
    </div>
  ) : null

  return { pullProps, indicator }
}
