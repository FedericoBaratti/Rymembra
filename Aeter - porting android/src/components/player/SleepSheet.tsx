import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import BottomSheet from '@/components/ui/BottomSheet'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { select } from '@/lib/haptics'

const OPTIONS = [15, 30, 45, 60, 90]

/**
 * Mobile sleep-timer picker. Same store action as the desktop SleepTimerMenu
 * (usePlayerStore.setSleepTimer), presented as a bottom sheet reachable from the
 * Now Playing tools row via useUiStore.sleepMenuOpen. BottomSheet renders nothing
 * while closed, so this can stay mounted.
 */
export default function SleepSheet(): React.JSX.Element {
  const { t } = useTranslation()
  const open = useUiStore((s) => s.sleepMenuOpen)
  const setOpen = useUiStore((s) => s.setSleepMenuOpen)
  const sleepEndsAt = usePlayerStore((s) => s.sleepEndsAt)
  const setSleepTimer = usePlayerStore((s) => s.setSleepTimer)
  const [, forceTick] = useState(0)

  // Re-render the "stops in N min" line as time passes.
  useEffect(() => {
    if (sleepEndsAt == null) return
    const iv = window.setInterval(() => forceTick((n) => n + 1), 30_000)
    return () => window.clearInterval(iv)
  }, [sleepEndsAt])

  const remaining = sleepEndsAt ? Math.max(0, Math.round((sleepEndsAt - Date.now()) / 60000)) : null

  const pick = (min: number | null): void => {
    select()
    setSleepTimer(min)
    setOpen(false)
  }

  return (
    <BottomSheet open={open} onClose={() => setOpen(false)} title={t('sleep.title')}>
      <div className="flex flex-col px-3 pb-2">
        {remaining != null && (
          <div className="px-3 pb-2 pt-1 text-center text-[13px] text-[var(--accent)]">
            {t('sleep.active', { time: `${remaining} min` })}
          </div>
        )}
        {OPTIONS.map((min) => (
          <button
            key={min}
            className="pressable flex h-12 items-center rounded-xl px-4 text-left text-[15px] text-text-1 transition-colors active:bg-white/[0.06]"
            onClick={() => pick(min)}
          >
            {t('sleep.minutes', { count: min })}
          </button>
        ))}
        <button
          className="pressable flex h-12 items-center rounded-xl px-4 text-left text-[15px] text-text-2 transition-colors active:bg-white/[0.06]"
          onClick={() => pick(null)}
        >
          {t('sleep.off')}
        </button>
      </div>
    </BottomSheet>
  )
}
