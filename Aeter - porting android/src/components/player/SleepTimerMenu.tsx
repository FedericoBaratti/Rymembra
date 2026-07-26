import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'

const OPTIONS = [15, 30, 45, 60, 90]

export default function SleepTimerMenu({
  children
}: {
  children: React.ReactNode
}): React.JSX.Element {
  const { t } = useTranslation()
  const open = useUiStore((s) => s.sleepMenuOpen)
  const setOpen = useUiStore((s) => s.setSleepMenuOpen)
  const sleepEndsAt = usePlayerStore((s) => s.sleepEndsAt)
  const setSleepTimer = usePlayerStore((s) => s.setSleepTimer)
  const ref = useRef<HTMLDivElement>(null)
  const [, forceTick] = useState(0)

  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent): void => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [open, setOpen])

  useEffect(() => {
    if (sleepEndsAt == null) return
    const iv = window.setInterval(() => forceTick((n) => n + 1), 30_000)
    return () => window.clearInterval(iv)
  }, [sleepEndsAt])

  const remaining = sleepEndsAt ? Math.max(0, Math.round((sleepEndsAt - Date.now()) / 60000)) : null

  return (
    <div className="relative" ref={ref}>
      {children}
      {open && (
        <div
          className="glass-modal scale-in absolute bottom-full right-0 z-30 mb-2 w-44 rounded-xl p-1.5"
          style={{ boxShadow: 'var(--shadow-2)' }}
        >
          <div className="px-2.5 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-text-3">
            {t('sleep.title')}
          </div>
          {remaining != null && (
            <div className="px-2.5 pb-1.5 text-[11px] text-[var(--accent)]">
              {t('sleep.active', { time: `${remaining} min` })}
            </div>
          )}
          {OPTIONS.map((min) => (
            <button
              key={min}
              className="block w-full rounded-lg px-2.5 py-1.5 text-left text-[13px] text-text-2 transition-colors hover:bg-white/[0.07] hover:text-text-1"
              onClick={() => {
                setSleepTimer(min)
                setOpen(false)
              }}
            >
              {t('sleep.minutes', { count: min })}
            </button>
          ))}
          <button
            className="block w-full rounded-lg px-2.5 py-1.5 text-left text-[13px] text-text-2 transition-colors hover:bg-white/[0.07] hover:text-text-1"
            onClick={() => {
              setSleepTimer(null)
              setOpen(false)
            }}
          >
            {t('sleep.off')}
          </button>
        </div>
      )}
    </div>
  )
}
