import { CheckCircle2, AlertCircle, Info, X } from 'lucide-react'
import { useToastStore, type ToastKind } from '@/store/useToastStore'
import { isMobile } from '@/lib/platform'

const KIND_ICON: Record<ToastKind, typeof Info> = {
  success: CheckCircle2,
  error: AlertCircle,
  info: Info
}

const KIND_BAR: Record<ToastKind, string> = {
  success: 'var(--accent)',
  error: 'rgb(248, 113, 113)',
  info: 'rgba(255, 255, 255, 0.35)'
}

export default function ToastHost(): React.JSX.Element | null {
  const toasts = useToastStore((s) => s.toasts)
  const dismiss = useToastStore((s) => s.dismiss)
  const pause = useToastStore((s) => s.pause)
  const resume = useToastStore((s) => s.resume)

  if (toasts.length === 0) return null

  // z-[120]: i toast devono restare leggibili sopra BottomSheet (z-70),
  // NowPlaying e TourOverlay (z-100/101).
  return (
    <div
      className={`fixed z-[120] flex flex-col-reverse gap-2 ${isMobile ? '' : 'w-[320px]'}`}
      style={
        isMobile
          ? // Su mobile: full-width sopra il player, safe-area aware (player-clearance
            // include già nav + inset gestuale), centrato sui display larghi.
            { left: '8px', right: '8px', bottom: 'var(--player-clearance)', maxWidth: '520px', marginInline: 'auto' }
          : { right: 'var(--player-gap)', bottom: 'var(--player-clearance)' }
      }
    >
      {toasts.map((t) => {
        const Icon = KIND_ICON[t.kind]
        return (
          <div
            key={t.id}
            role={t.kind === 'error' ? 'alert' : 'status'}
            aria-live={t.kind === 'error' ? 'assertive' : 'polite'}
            data-kind={t.kind}
            className={`toast-card flex items-start gap-3 py-3 pl-4 pr-2 ${t.leaving ? 'toast-card--leaving' : ''}`}
            style={
              {
                '--toast-bar': KIND_BAR[t.kind],
                '--toast-dur': `${t.duration}ms`
              } as React.CSSProperties
            }
            onMouseEnter={() => pause(t.id)}
            onMouseLeave={() => resume(t.id)}
          >
            <Icon
              size={17}
              className="mt-0.5 shrink-0"
              style={{ color: 'var(--toast-bar)' }}
            />
            <div className="min-w-0 flex-1">
              <p className="truncate text-[13px] font-semibold">{t.title}</p>
              {t.message && <p className="mt-0.5 truncate text-[12px] text-text-2">{t.message}</p>}
            </div>
            <button
              className={`icon-btn shrink-0 ${isMobile ? 'h-9 w-9' : 'h-6 w-6'}`}
              onClick={() => dismiss(t.id)}
              aria-label="Close"
            >
              <X size={isMobile ? 16 : 13} />
            </button>
            {!t.leaving && <div className="toast-progress" />}
          </div>
        )
      })}
    </div>
  )
}
