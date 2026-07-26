import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AudioWaveform } from 'lucide-react'
import { useTourStore } from '@/store/useTourStore'
import { useUiStore } from '@/store/useUiStore'
import { TOUR_STEPS, type TourPlacement } from './tourSteps'
import { useTargetRect } from './useTargetRect'

const TIP_GAP = 16
const EDGE = 12

interface Hole {
  top: number
  left: number
  width: number
  height: number
  radius: number
}

const OPPOSITE: Record<TourPlacement, TourPlacement> = {
  top: 'bottom',
  bottom: 'top',
  left: 'right',
  right: 'left'
}

function clamp(v: number, min: number, max: number): number {
  return Math.min(Math.max(v, min), max)
}

function computeTipPos(
  hole: Hole,
  placement: TourPlacement,
  tw: number,
  th: number,
  vw: number,
  vh: number
): { top: number; left: number } {
  const cx = hole.left + hole.width / 2
  const cy = hole.top + hole.height / 2

  const calc = (p: TourPlacement): { top: number; left: number } => {
    switch (p) {
      case 'right':
        return { left: hole.left + hole.width + TIP_GAP, top: cy - th / 2 }
      case 'left':
        return { left: hole.left - TIP_GAP - tw, top: cy - th / 2 }
      case 'top':
        return { left: cx - tw / 2, top: hole.top - TIP_GAP - th }
      case 'bottom':
        return { left: cx - tw / 2, top: hole.top + hole.height + TIP_GAP }
    }
  }
  // Only the placement axis matters: the cross axis gets clamped into view below
  const fits = (p: { top: number; left: number }, axis: TourPlacement): boolean =>
    axis === 'top' || axis === 'bottom'
      ? p.top >= EDGE && p.top + th <= vh - EDGE
      : p.left >= EDGE && p.left + tw <= vw - EDGE

  let pos = calc(placement)
  if (!fits(pos, placement)) {
    const flipped = calc(OPPOSITE[placement])
    // Neither side fits (e.g. full-page targets): center the card over the hole
    pos = fits(flipped, OPPOSITE[placement]) ? flipped : { top: cy - th / 2, left: cx - tw / 2 }
  }
  return {
    top: clamp(pos.top, EDGE, Math.max(EDGE, vh - th - EDGE)),
    left: clamp(pos.left, EDGE, Math.max(EDGE, vw - tw - EDGE))
  }
}

export default function TourOverlay(): React.JSX.Element | null {
  const active = useTourStore((s) => s.active)
  if (!active) return null
  return <TourOverlayInner />
}

function TourOverlayInner(): React.JSX.Element {
  const { t } = useTranslation()
  const stepIndex = useTourStore((s) => s.stepIndex)
  const next = useTourStore((s) => s.next)
  const back = useTourStore((s) => s.back)
  const finish = useTourStore((s) => s.finish)

  const step = TOUR_STEPS[stepIndex]
  const isFirst = stepIndex === 0
  const isLast = stepIndex === TOUR_STEPS.length - 1
  const centered = !step.target

  // Close any open chrome (search, EQ, queue…) so nothing fights the overlay
  useEffect(() => {
    const ui = useUiStore.getState()
    ui.setSearchOpen(false)
    ui.setQueueOpen(false)
    ui.setEqOpen(false)
    ui.setFullscreenViz(false)
    ui.setSleepMenuOpen(false)
  }, [])

  // Capture-phase keyboard nav; stops keys from reaching app shortcuts
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') finish()
      else if (e.key === 'ArrowRight' || e.key === 'Enter') next()
      else if (e.key === 'ArrowLeft') back()
      else return
      e.preventDefault()
      e.stopPropagation()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [next, back, finish])

  const [vp, setVp] = useState({ w: window.innerWidth, h: window.innerHeight })
  useEffect(() => {
    const onResize = (): void => setVp({ w: window.innerWidth, h: window.innerHeight })
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])

  const onMissing = useCallback(() => next(), [next])
  const rect = useTargetRect(step, onMissing)

  const pad = step.padding ?? 8
  const hole: Hole = rect
    ? {
        top: rect.top - pad,
        left: rect.left - pad,
        width: rect.width + pad * 2,
        height: rect.height + pad * 2,
        radius: step.radius ?? 12
      }
    : { top: vp.h / 2, left: vp.w / 2, width: 0, height: 0, radius: 0 }

  const tipRef = useRef<HTMLDivElement>(null)
  const [tipPos, setTipPos] = useState<{ top: number; left: number } | null>(null)
  useLayoutEffect(() => {
    const tip = tipRef.current
    if (!tip) return
    setTipPos(
      computeTipPos(
        hole,
        step.placement ?? 'bottom',
        tip.offsetWidth,
        tip.offsetHeight,
        vp.w,
        vp.h
      )
    )
    // hole is a fresh object every render; its primitive fields are the real
    // inputs (depending on `hole` itself would re-run on every render).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hole.top, hole.left, hole.width, hole.height, step, vp.w, vp.h])

  return (
    <div className="no-drag fixed inset-0 z-[100]">
      <div
        className={`tour-spotlight ${centered ? 'tour-spotlight--full' : ''}`}
        style={{
          top: hole.top,
          left: hole.left,
          width: hole.width,
          height: hole.height,
          borderRadius: hole.radius
        }}
      />

      <div
        ref={tipRef}
        className={`tour-tooltip glass-modal fade-in flex flex-col rounded-2xl p-5 ${
          centered ? 'w-[400px]' : 'w-[340px]'
        }`}
        style={{
          top: tipPos?.top ?? -9999,
          left: tipPos?.left ?? -9999,
          visibility: tipPos ? 'visible' : 'hidden',
          boxShadow: '0 24px 80px rgba(0,0,0,0.7)'
        }}
      >
        {(isFirst || isLast) && (
          <div
            className="mb-4 flex h-12 w-12 items-center justify-center self-center rounded-xl"
            style={{
              background: 'linear-gradient(135deg, var(--accent), rgba(var(--accent-rgb) / 0.5))',
              boxShadow: '0 0 24px var(--accent-glow)'
            }}
          >
            <AudioWaveform size={22} color="white" strokeWidth={2.4} />
          </div>
        )}

        <div className={`text-[15px] font-bold ${isFirst || isLast ? 'text-center' : ''}`}>
          {t(`tour.${step.id}.title`)}
        </div>
        <div
          className={`mt-1.5 text-[12.5px] leading-relaxed text-text-2 ${
            isFirst || isLast ? 'text-center' : ''
          }`}
        >
          {t(`tour.${step.id}.body`)}
        </div>

        {step.id === 'search' && (
          <kbd
            className="mt-3 self-center rounded border px-2 py-1 text-[11px] text-text-2"
            style={{ borderColor: 'var(--hairline)', background: 'rgba(255,255,255,0.04)' }}
          >
            Ctrl + F
          </kbd>
        )}

        <div className="mt-4 flex justify-center gap-1.5">
          {TOUR_STEPS.map((s, i) => (
            <span
              key={s.id}
              className="h-1.5 rounded-full transition-all duration-300"
              style={{
                width: i === stepIndex ? 16 : 6,
                background: i === stepIndex ? 'var(--accent)' : 'rgba(255,255,255,0.18)'
              }}
            />
          ))}
        </div>

        <div className="mt-4 flex items-center justify-between">
          {!isLast ? (
            <button
              className="rounded-lg px-2.5 py-1.5 text-[12px] font-medium text-text-3 transition-colors hover:text-text-1"
              onClick={finish}
            >
              {t('tour.skip')}
            </button>
          ) : (
            <span />
          )}
          <div className="flex items-center gap-2">
            {!isFirst && !isLast && (
              <button
                className="btn-ghost rounded-lg px-3 py-1.5 text-[12.5px] font-medium"
                onClick={back}
              >
                {t('tour.back')}
              </button>
            )}
            <button
              className="btn-accent rounded-lg px-4 py-1.5 text-[12.5px]"
              onClick={isLast ? finish : next}
            >
              {isLast ? t('tour.done') : isFirst ? t('tour.start') : t('tour.next')}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
