import { useTranslation } from 'react-i18next'
import { isMobile } from '@/lib/platform'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import BottomSheet from './BottomSheet'

/**
 * Lightweight confirmation dialog replacing window.confirm(), so destructive
 * actions (e.g. deleting a playlist) match the app chrome. Bottom sheet on
 * mobile, centered modal on desktop.
 */
export default function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  danger = false,
  onConfirm,
  onClose
}: {
  open: boolean
  title: string
  message?: string
  confirmLabel?: string
  danger?: boolean
  onConfirm: () => void
  onClose: () => void
}): React.JSX.Element | null {
  const { t } = useTranslation()
  // Desktop only: BottomSheet already owns focus/Escape handling on mobile.
  const trapRef = useFocusTrap<HTMLDivElement>(open && !isMobile, onClose)
  if (!open) return null

  const confirm = (): void => {
    onConfirm()
    onClose()
  }

  const buttons = (
    <div className="flex items-center justify-end gap-2 px-5 pb-1 pt-3">
      <button className="btn-ghost rounded-lg px-4 py-2 text-[13px] font-medium text-text-2" onClick={onClose}>
        {t('common.cancel')}
      </button>
      <button
        autoFocus={!isMobile}
        className={`rounded-lg px-4 py-2 text-[13px] font-semibold text-white ${danger ? '' : 'btn-accent'}`}
        style={danger ? { background: 'var(--danger)' } : undefined}
        onClick={confirm}
      >
        {confirmLabel ?? t('common.confirm')}
      </button>
    </div>
  )

  if (isMobile) {
    return (
      <BottomSheet open={open} onClose={onClose} title={title}>
        {message && <p className="px-5 pb-1 text-[13px] text-text-2">{message}</p>}
        {buttons}
      </BottomSheet>
    )
  }

  return (
    <div
      className="overlay-in fixed inset-0 z-[60] flex items-center justify-center bg-black/55 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="glass-modal scale-in w-[min(420px,calc(100vw-48px))] rounded-2xl py-4"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="px-5 pb-1 text-[15px] font-bold">{title}</h2>
        {message && <p className="px-5 text-[13px] text-text-2">{message}</p>}
        {buttons}
      </div>
    </div>
  )
}
