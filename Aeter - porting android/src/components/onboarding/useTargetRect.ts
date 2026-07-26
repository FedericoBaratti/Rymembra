import { useEffect, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import type { TourStep } from './tourSteps'
import { isMobile } from '@/lib/platform'

export interface SpotRect {
  top: number
  left: number
  width: number
  height: number
}

// Mobile devices mount async pages noticeably slower than desktop; a short
// timeout made tour steps auto-skip before their target ever appeared.
const FIND_TIMEOUT_MS = isMobile ? 2500 : 1500
const SETTLE_MS = 350

/**
 * Resolves the on-screen rect of a tour step's target element.
 * Navigates to the step's route if needed, polls until the element mounts
 * (skeletons/async pages), keeps re-reading while mount animations settle,
 * then tracks the element via ResizeObserver + window resize.
 * Returns null for centered steps (no target).
 */
export function useTargetRect(step: TourStep, onMissing: () => void): SpotRect | null {
  const [rect, setRect] = useState<SpotRect | null>(null)
  const navigate = useNavigate()
  const location = useLocation()

  useEffect(() => {
    if (!step.target) {
      setRect(null)
      return
    }
    if (step.route && location.pathname !== step.route) {
      navigate(step.route)
      return // location change re-runs this effect
    }

    let raf = 0
    let ro: ResizeObserver | null = null
    let el: HTMLElement | null = null
    const started = performance.now()
    let foundAt = 0

    const read = (): void => {
      if (!el) return
      const r = el.getBoundingClientRect()
      setRect((prev) =>
        prev &&
        prev.top === r.top &&
        prev.left === r.left &&
        prev.width === r.width &&
        prev.height === r.height
          ? prev
          : { top: r.top, left: r.left, width: r.width, height: r.height }
      )
    }

    const tick = (): void => {
      const now = performance.now()
      if (!el) {
        el = document.querySelector<HTMLElement>(`[data-tour="${step.target}"]`)
        if (el) {
          foundAt = now
          el.scrollIntoView({ block: 'nearest' })
          ro = new ResizeObserver(read)
          ro.observe(el)
          window.addEventListener('resize', read)
          read()
        } else if (now - started > FIND_TIMEOUT_MS) {
          console.warn(`[tour] target not found, skipping step: ${step.target}`)
          onMissing()
          return
        }
      } else {
        read()
        // After the mount animation settles, ResizeObserver/resize keep us fresh
        if (now - foundAt > SETTLE_MS) return
      }
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)

    return () => {
      cancelAnimationFrame(raf)
      ro?.disconnect()
      window.removeEventListener('resize', read)
    }
  }, [step, location.pathname, navigate, onMissing])

  return rect
}
