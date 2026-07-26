import { useState } from 'react'
import { Check, ChevronDown } from 'lucide-react'
import { isMobile } from '@/lib/platform'
import BottomSheet from './BottomSheet'

export interface SelectOption<T extends string | number> {
  value: T
  label: React.ReactNode
}

/**
 * Single-choice picker.
 *
 * On desktop it stays a native <select>: accessible, keyboard-friendly, and
 * rendered fine by Chromium/Electron. On mobile the Android System WebView
 * renders native <select> popups inconsistently and badly — options flung to
 * the screen edges with a huge gap and stray dividers — so there we open a
 * Material-style bottom sheet with a clean single-choice list instead.
 *
 * In the mobile build `isMobile` is a compile-time constant `true`, so the
 * native-<select> branch tree-shakes away entirely.
 */
export default function Select<T extends string | number>({
  value,
  options,
  onChange,
  title,
  ariaLabel,
  triggerClassName = 'field-input h-9',
  fill = false
}: {
  value: T
  options: SelectOption<T>[]
  onChange: (value: T) => void
  /** Heading shown at the top of the mobile picker sheet. */
  title?: string
  ariaLabel?: string
  /** Base field styling for the trigger button / native control. */
  triggerClassName?: string
  /** Stretch the trigger to fill its container (e.g. a grid cell) instead of hugging its content. */
  fill?: boolean
}): React.JSX.Element {
  const [open, setOpen] = useState(false)

  if (!isMobile) {
    return (
      <select
        className={triggerClassName}
        value={String(value)}
        aria-label={ariaLabel}
        onChange={(e) => {
          const next = options.find((o) => String(o.value) === e.target.value)
          if (next) onChange(next.value)
        }}
      >
        {options.map((o) => (
          <option key={String(o.value)} value={String(o.value)}>
            {o.label}
          </option>
        ))}
      </select>
    )
  }

  const selected = options.find((o) => o.value === value)

  return (
    <>
      <button
        type="button"
        className={`${triggerClassName} ${
          fill ? 'flex w-full justify-between' : 'inline-flex max-w-[60vw]'
        } items-center gap-1.5`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onClick={() => setOpen(true)}
      >
        <span className="min-w-0 truncate">{selected?.label ?? ''}</span>
        <ChevronDown size={15} className="shrink-0 text-text-3" aria-hidden />
      </button>

      <BottomSheet open={open} onClose={() => setOpen(false)} title={title}>
        <div role="listbox" aria-label={title ?? ariaLabel} className="px-2 pb-1">
          {options.map((o) => {
            const active = o.value === value
            return (
              <button
                key={String(o.value)}
                type="button"
                role="option"
                aria-selected={active}
                className="flex w-full items-center justify-between gap-4 rounded-xl px-4 py-3.5 text-left text-[15px] transition-colors active:bg-white/[0.08]"
                onClick={() => {
                  onChange(o.value)
                  setOpen(false)
                }}
              >
                <span
                  className="min-w-0 truncate"
                  style={active ? { color: 'var(--accent)', fontWeight: 600 } : undefined}
                >
                  {o.label}
                </span>
                {active && (
                  <Check size={19} className="shrink-0" style={{ color: 'var(--accent)' }} aria-hidden />
                )}
              </button>
            )
          })}
        </div>
      </BottomSheet>
    </>
  )
}
