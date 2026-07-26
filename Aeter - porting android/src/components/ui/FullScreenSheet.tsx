import { useTranslation } from 'react-i18next'
import { ArrowLeft } from 'lucide-react'
import { useBackDismiss } from '@/hooks/useBackDismiss'

/**
 * Mobile full-screen surface (M3 full-screen dialog): solid background, top
 * app bar with back affordance + optional trailing action (e.g. Save), body
 * scrollable. Used for keyboard-heavy tasks — search, metadata/lyrics editors —
 * where a centered dialog or bottom sheet would fight the soft keyboard.
 * Safe-area padded on top; the bottom padding shrinks above the IME via
 * --kb-height (pushed by MainActivity). Desktop keeps its floating dialogs.
 */
export default function FullScreenSheet({
  open,
  onClose,
  title,
  headerContent,
  actions,
  children,
  z = 50,
  scrollBody = true
}: {
  open: boolean
  onClose: () => void
  /** App-bar title (ignored when headerContent is provided). */
  title?: string
  /** Custom app-bar middle content (e.g. the search input). */
  headerContent?: React.ReactNode
  /** Trailing app-bar actions (e.g. a Save button). */
  actions?: React.ReactNode
  children: React.ReactNode
  /** Stacking context — match the overlay this replaces (default 50). */
  z?: number
  /** False when the children manage their own scrolling (fixed toolbar + inner list). */
  scrollBody?: boolean
}): React.JSX.Element | null {
  const { t } = useTranslation()
  // Android hardware back = the app-bar back arrow.
  useBackDismiss(open, onClose)
  if (!open) return null

  return (
    <div
      className="fs-sheet fixed inset-0 flex flex-col"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      style={{
        zIndex: z,
        background: 'var(--color-surface-0)',
        paddingTop: 'var(--sa-top, env(safe-area-inset-top, 0px))',
        paddingBottom:
          'calc(var(--kb-height, 0px) + var(--sa-bottom, env(safe-area-inset-bottom, 0px)))',
        animation: 'np-rise var(--dur-2) var(--ease-out-expo) both'
      }}
    >
      <div
        className="flex h-14 shrink-0 items-center gap-1 border-b px-1.5"
        style={{ borderColor: 'var(--hairline)' }}
      >
        <button
          className="icon-btn h-12 w-12 shrink-0"
          onClick={onClose}
          aria-label={t('common.close')}
        >
          <ArrowLeft size={22} />
        </button>
        {headerContent ?? (
          <div className="min-w-0 flex-1 truncate text-[16px] font-bold">{title}</div>
        )}
        {actions && <div className="flex shrink-0 items-center gap-1 pr-1.5">{actions}</div>}
      </div>
      <div
        className={
          scrollBody ? 'min-h-0 flex-1 overflow-y-auto' : 'flex min-h-0 flex-1 flex-col'
        }
      >
        {children}
      </div>
    </div>
  )
}
