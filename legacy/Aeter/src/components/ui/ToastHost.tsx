import { CheckCircle2, AlertCircle, Info, X } from 'lucide-react'
import { useToastStore, type ToastKind } from '@/store/useToastStore'

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

  return (
    <div
      className="fixed z-40 flex w-[320px] flex-col-reverse gap-2"
      style={{ right: 'var(--player-gap)', bottom: 'var(--player-clearance)' }}
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
            <button className="icon-btn h-6 w-6 shrink-0" onClick={() => dismiss(t.id)}>
              <X size={13} />
            </button>
            {!t.leaving && <div className="toast-progress" />}
          </div>
        )
      })}
    </div>
  )
}
