import type { LucideIcon } from 'lucide-react'

/**
 * A single action row inside a mobile bottom-sheet menu (48px M3 target).
 * Same look as the local copies in PlaylistDetail; shared for new menus.
 */
export default function SheetAction({
  icon: Icon,
  label,
  onClick,
  danger = false
}: {
  icon: LucideIcon
  label: string
  onClick: () => void
  danger?: boolean
}): React.JSX.Element {
  return (
    <button
      className="sheet-action flex w-full items-center gap-3 rounded-xl px-4 py-3 text-left text-[14px] transition-colors hover:bg-white/[0.06] active:bg-white/[0.08]"
      style={danger ? { color: '#ff6b6e' } : undefined}
      onClick={onClick}
    >
      <Icon size={18} className={`shrink-0 ${danger ? '' : 'text-text-3'}`} />
      {label}
    </button>
  )
}
