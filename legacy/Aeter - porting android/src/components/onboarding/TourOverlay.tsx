import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AudioWaveform } from 'lucide-react'
import { useTourStore } from '@/store/useTourStore'
import { useUiStore } from '@/store/useUiStore'
import { isMobile } from '@/lib/platform'
import { useBackDismiss } from '@/hooks/useBackDismiss'
import { getTourSteps, getTourSections, type TourPlacement } from './tourSteps'
import { useTargetRect } from './useTargetRect'

const TIP_GAP = 16
const EDGE = 12
// Touch builds: keep the card clear of the status bar (top) and the player +
// bottom navigation (bottom). Desktop has no such chrome at the edges.
const INSETS = isMobile
  ? { top: 56, bottom: 150, side: 12 }
  : { top: EDGE, bottom: EDGE, side: EDGE }
const SWIPE_THRESHOLD = 45

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
      ? p.top >= INSETS.top && p.top + th <= vh - INSETS.bottom
      : p.left >= INSETS.side && p.left + tw <= vw - INSETS.side

  let pos = calc(placement)
  if (!fits(pos, placement)) {
    const flipped = calc(OPPOSITE[placement])
    // Neither side fits (e.g. full-page targets): center the card over the hole
    pos = fits(flipped, OPPOSITE[placement]) ? flipped : { top: cy - th / 2, left: cx - tw / 2 }
  }
  return {
    top: clamp(pos.top, INSETS.top, Math.max(INSETS.top, vh - th - INSETS.bottom)),
    left: clamp(pos.left, INSETS.side, Math.max(INSETS.side, vw - tw - INSETS.side))
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
  const goToStep = useTourStore((s) => s.goToStep)
  const finish = useTourStore((s) => s.finish)

  // Hardware back dismisses the tour (it was invisible to the back handler
  // before: two presses on a root tab would exit the app mid-tour).
  useBackDismiss(true, finish)

  const steps = getTourSteps()
  const sections = getTourSections()
  const step = steps[stepIndex]
  const isFirst = stepIndex === 0
  const isLast = stepIndex === steps.length - 1
  const centered = !step.target
  const sectionIndex = sections.findIndex((s) => s.id === step.section)

  // Drive the chrome (search / now-playing sheet / queue / EQ) per step so the
  // target a step points at is actually mounted before we measure it. Resetting
  // every flag each step doubles as "close everything we opened".
  useEffect(() => {
    const ui = useUiStore.getState()
    ui.setSearchOpen(step.openUi === 'search')
    ui.setNowPlayingOpen(step.openUi === 'nowPlaying')
    ui.setQueueOpen(step.openUi === 'queue')
    ui.setEqOpen(step.openUi === 'eq')
    ui.setFullscreenViz(false)
    ui.setSleepMenuOpen(false)
  }, [step])

  // Close anything we opened when the tour ends.
  useEffect(() => {
    return () => {
      const ui = useUiStore.getState()
      ui.setSearchOpen(false)
      ui.setNowPlayingOpen(false)
      ui.setQueueOpen(false)
      ui.setEqOpen(false)
    }
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

  // Swipe left/right to move through steps on touch
  const touchX = useRef<number | null>(null)
  const onTouchStart = (e: React.TouchEvent): void => {
    touchX.current = e.touches[0]?.clientX ?? null
  }
  const onTouchEnd = (e: React.TouchEvent): void => {
    if (touchX.current == null) return
    const dx = (e.changedTouches[0]?.clientX ?? touchX.current) - touchX.current
    touchX.current = null
    if (dx <= -SWIPE_THRESHOLD) next()
    else if (dx >= SWIPE_THRESHOLD && !isFirst) back()
  }

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
        onTouchStart={onTouchStart}
        onTouchEnd={onTouchEnd}
        className={`tour-tooltip glass-modal fade-in flex flex-col rounded-2xl p-5 ${
          centered ? 'w-[400px] max-w-[calc(100vw-24px)]' : 'w-[340px] max-w-[calc(100vw-24px)]'
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

        {!isFirst && !isLast && sectionIndex >= 0 && (
          <div className="mb-1 text-[10.5px] font-semibold uppercase tracking-wider text-[var(--accent)]">
            {t('tour.chapter', { i: sectionIndex + 1, n: sections.length })} ·{' '}
            <span className="text-text-3">{t(`tour.sections.${step.section}`)}</span>
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

        {/* Chapter index: tap a segment to jump to that chapter. The visible
            bar stays 8px; the button around it is the real (36px) hit area. */}
        <div className="mt-2 flex justify-center gap-1">
          {sections.map((s, i) => (
            <button
              key={s.id}
              onClick={() => goToStep(s.firstStep)}
              aria-label={t(`tour.sections.${s.id}`)}
              className="flex h-9 flex-1 items-center justify-center"
              style={{ maxWidth: 32 }}
            >
              <span
                className="h-2 w-full rounded-full transition-colors duration-300"
                style={{
                  background:
                    i === sectionIndex
                      ? 'var(--accent)'
                      : i < sectionIndex
                        ? 'rgba(var(--accent-rgb) / 0.45)'
                        : 'rgba(255,255,255,0.16)'
                }}
              />
            </button>
          ))}
        </div>

        <div className="mt-4 flex items-center justify-between">
          {!isLast ? (
            <button
              className="rounded-lg px-3 py-2.5 text-[12.5px] font-medium text-text-3 transition-colors hover:text-text-1"
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
                className="btn-ghost rounded-lg px-3.5 py-2.5 text-[12.5px] font-medium"
                onClick={back}
              >
                {t('tour.back')}
              </button>
            )}
            <button
              className="btn-accent rounded-lg px-4 py-2.5 text-[12.5px]"
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
