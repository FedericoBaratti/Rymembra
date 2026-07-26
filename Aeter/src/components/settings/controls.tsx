import type { LucideIcon } from 'lucide-react'

export const selectCls = 'field-input h-9'
export const inputCls = 'field-input h-9'

export function Section({
  title,
  icon: Icon,
  children,
  dataTour,
  index = 0
}: {
  title: string
  icon: LucideIcon
  children: React.ReactNode
  dataTour?: string
  index?: number
}): React.JSX.Element {
  return (
    <section
      className="slide-up-in mb-7"
      data-tour={dataTour}
      style={{ animationDelay: `${index * 40}ms` }}
    >
      <h2 className="mb-3 flex items-center gap-2 text-[13px] font-bold uppercase tracking-wider text-text-3">
        <span
          className="section-icon flex h-6 w-6 items-center justify-center rounded-lg"
          style={{
            background: 'var(--accent-soft)',
            boxShadow: 'inset 0 0 0 1px rgba(var(--accent-rgb) / 0.2)'
          }}
        >
          <Icon size={13} style={{ color: 'var(--accent)' }} />
        </span>
        {title}
      </h2>
      <div
        className="section-card row-lift flex flex-col gap-4 border border-[var(--hairline)] p-4"
        style={{
          background: 'linear-gradient(180deg, rgba(255,255,255,0.035), rgba(255,255,255,0.015))',
          borderRadius: 'var(--radius-card)'
        }}
      >
        {children}
      </div>
    </section>
  )
}

export function Switch({
  checked,
  onChange,
  label
}: {
  checked: boolean
  onChange: (checked: boolean) => void
  label: string
}): React.JSX.Element {
  return (
    <label className="switch">
      <input
        type="checkbox"
        checked={checked}
        aria-label={label}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="switch-track" />
    </label>
  )
}

export function FieldRow({
  label,
  children,
  hint
}: {
  label: string
  children: React.ReactNode
  hint?: string
}): React.JSX.Element {
  return (
    <div className="flex items-center justify-between gap-6">
      <div>
        <div className="text-[13.5px] font-medium">{label}</div>
        {hint && <div className="mt-0.5 max-w-md text-[11.5px] leading-relaxed text-text-3">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  )
}
