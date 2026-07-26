import type { LucideIcon } from 'lucide-react'

export default function EmptyState({
  icon: Icon,
  title,
  subtitle,
  action
}: {
  icon: LucideIcon
  title: string
  subtitle?: string
  action?: React.ReactNode
}): React.JSX.Element {
  return (
    <div className="empty-state fade-in relative flex flex-1 flex-col items-center justify-center gap-3 px-8 pb-16 text-center">
      <div
        className="pointer-events-none absolute inset-0"
        style={{
          background:
            'radial-gradient(ellipse 40% 35% at 50% 45%, var(--accent-soft), transparent 70%)',
          opacity: 0.6
        }}
      />
      <div
        className="empty-icon icon-float relative flex h-20 w-20 items-center justify-center rounded-3xl"
        style={{
          background: 'var(--accent-soft)',
          boxShadow: 'inset 0 0 0 1px rgba(var(--accent-rgb) / 0.25), 0 0 40px var(--accent-soft)'
        }}
      >
        <Icon size={32} style={{ color: 'var(--accent)' }} />
      </div>
      <h2 className="fade-in relative text-[17px] font-bold" style={{ animationDelay: '60ms' }}>
        {title}
      </h2>
      {subtitle && (
        <p
          className="fade-in relative max-w-sm text-[13px] leading-relaxed text-text-3"
          style={{ animationDelay: '120ms' }}
        >
          {subtitle}
        </p>
      )}
      {action && (
        <div className="fade-in relative" style={{ animationDelay: '180ms' }}>
          {action}
        </div>
      )}
    </div>
  )
}
