import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X, Save, RotateCcw } from 'lucide-react'
import { useUiStore } from '@/store/useUiStore'
import { useSettingsStore } from '@/store/useSettingsStore'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { applyEq, EQ_FREQUENCIES, EQ_PRESETS } from '@/lib/audio'
import { isMobile } from '@/lib/platform'
import BottomSheet from '@/components/ui/BottomSheet'

function freqLabel(f: number): string {
  return f >= 1000 ? `${f / 1000}k` : `${f}`
}

const FLAT = new Array(EQ_FREQUENCIES.length).fill(0) as number[]

function gainsMatch(a: number[], b: number[]): boolean {
  return a.length === b.length && a.every((v, i) => Math.abs(v - (b[i] ?? 0)) < 0.05)
}

/** Smooth-ish SVG curve of the current EQ response, drawn above the sliders. */
function ResponseCurve({ gains, enabled }: { gains: number[]; enabled: boolean }): React.JSX.Element {
  const n = gains.length
  const pts = gains.map((g, i) => {
    const x = (i / (n - 1)) * 100
    const y = 20 - (Math.max(-12, Math.min(12, g)) / 12) * 17
    return [x, y] as const
  })
  const line = pts.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ')
  const area = `${line} L100,40 L0,40 Z`
  return (
    <svg
      viewBox="0 0 100 40"
      preserveAspectRatio="none"
      className={`h-12 w-full ${enabled ? '' : 'opacity-40'}`}
      aria-hidden
    >
      <line x1="0" y1="20" x2="100" y2="20" stroke="var(--hairline)" strokeWidth="0.4" />
      <path d={area} fill="var(--accent-soft)" />
      <path d={line} fill="none" stroke="var(--accent)" strokeWidth="1.2" vectorEffect="non-scaling-stroke" />
    </svg>
  )
}

function EqContent(): React.JSX.Element {
  const { t } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  const [savingName, setSavingName] = useState<string | null>(null)

  if (!settings) return <div className="p-4" />

  const gains = settings.eqGains
  const enabled = settings.eqEnabled
  const customPresets = settings.eqCustomPresets

  const apply = (nextGains: number[], nextEnabled: boolean): void => {
    applyEq(nextGains, nextEnabled)
    void update({ eqGains: nextGains, eqEnabled: nextEnabled })
  }

  const setBand = (i: number, value: number): void => {
    const next = [...gains]
    next[i] = value
    apply(next, enabled)
  }

  const builtIn = Object.entries(EQ_PRESETS).map(([name, g]) => ({ name, gains: g, custom: false }))
  const customs = customPresets.map((p) => ({ name: p.name, gains: p.gains, custom: true }))
  const allPresets = [...builtIn, ...customs]
  const activeName = allPresets.find((p) => gainsMatch(gains, p.gains))?.name ?? null
  const nameExists = (savingName?.trim() ?? '') !== '' && customPresets.some((p) => p.name === savingName?.trim())

  const savePreset = (): void => {
    const name = savingName?.trim()
    if (!name) return
    const others = customPresets.filter((p) => p.name !== name)
    void update({ eqCustomPresets: [...others, { name, gains: [...gains] }] })
    setSavingName(null)
  }

  const deletePreset = (name: string): void => {
    void update({ eqCustomPresets: customPresets.filter((p) => p.name !== name) })
  }

  return (
    <div className="px-4 pb-2">
      {/* Enabled + reset */}
      <div className="mb-3 flex items-center justify-between">
        <label className="flex cursor-pointer items-center gap-2 text-[12.5px] text-text-2">
          <span className="switch">
            <input
              type="checkbox"
              checked={enabled}
              aria-label={t('eq.enabled')}
              onChange={(e) => apply(gains, e.target.checked)}
            />
            <span className="switch-track" />
          </span>
          {t('eq.enabled')}
        </label>
        <button
          className="btn-ghost flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12px] font-medium"
          onClick={() => apply([...FLAT], enabled)}
          disabled={gainsMatch(gains, FLAT)}
        >
          <RotateCcw size={13} /> {t('eq.reset')}
        </button>
      </div>

      {/* Response curve */}
      <ResponseCurve gains={gains} enabled={enabled} />

      {/* Preset chips */}
      <div className="-mx-1 mb-3 mt-2 flex gap-1.5 overflow-x-auto px-1 pb-1">
        {allPresets.map((p) => {
          const active = activeName === p.name
          return (
            <span key={(p.custom ? 'c:' : 'b:') + p.name} className="relative shrink-0">
              <button
                className={`rounded-full px-3 py-1.5 text-[12px] font-medium transition-colors ${
                  active
                    ? 'bg-[var(--accent)] text-white'
                    : 'bg-surface-3 text-text-2 hover:bg-white/[0.08]'
                } ${p.custom ? 'pr-7' : ''}`}
                onClick={() => apply([...p.gains], true)}
              >
                {p.name}
              </button>
              {p.custom && (
                <button
                  className="absolute right-1 top-1/2 -translate-y-1/2 rounded-full p-0.5 text-text-3 hover:text-text-1"
                  onClick={() => deletePreset(p.name)}
                  aria-label={t('eq.delete_preset', { name: p.name })}
                >
                  <X size={12} />
                </button>
              )}
            </span>
          )
        })}
      </div>

      {/* Save preset */}
      <div className="mb-3 flex items-center gap-2">
        {savingName == null ? (
          <button
            className="btn-ghost flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12px] font-medium"
            onClick={() => setSavingName('')}
          >
            <Save size={13} /> {t('eq.save_preset')}
          </button>
        ) : (
          <>
            <input
              autoFocus
              className="field-input h-8 flex-1 text-[12.5px]"
              placeholder={t('eq.preset_name')}
              value={savingName}
              onChange={(e) => setSavingName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') savePreset()
                if (e.key === 'Escape') setSavingName(null)
              }}
            />
            <button className="btn-accent rounded-lg px-3 py-1.5 text-[12.5px]" onClick={savePreset} disabled={!savingName.trim()}>
              {t('metadata.save')}
            </button>
          </>
        )}
        {nameExists && <span className="text-[11px] text-text-3">{t('eq.preset_overwrite')}</span>}
      </div>

      {/* Sliders */}
      <div className={`flex justify-between gap-1 ${enabled ? '' : 'opacity-50'}`}>
        {EQ_FREQUENCIES.map((freq, i) => (
          <div key={freq} className="flex flex-col items-center gap-1">
            <span className="tnum text-[10px] text-text-3">
              {gains[i] > 0 ? `+${gains[i].toFixed(0)}` : gains[i].toFixed(0)}
            </span>
            <input
              type="range"
              min={-12}
              max={12}
              step={0.5}
              value={gains[i] ?? 0}
              disabled={!enabled}
              onChange={(e) => setBand(i, Number(e.target.value))}
              className="eq-slider"
              onDoubleClick={() => setBand(i, 0)}
              aria-label={`${freqLabel(freq)}Hz`}
            />
            <span className="text-[10px] text-text-3">{freqLabel(freq)}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

export default function EqualizerPanel(): React.JSX.Element | null {
  const { t } = useTranslation()
  const open = useUiStore((s) => s.eqOpen)
  const setOpen = useUiStore((s) => s.setEqOpen)
  const settings = useSettingsStore((s) => s.settings)
  const trapRef = useFocusTrap<HTMLDivElement>(open && !isMobile, () => setOpen(false))

  if (!open || !settings) return null

  // Mobile: native Android-style bottom sheet.
  if (isMobile) {
    return (
      <BottomSheet open={open} onClose={() => setOpen(false)} title={t('eq.title')}>
        <EqContent />
      </BottomSheet>
    )
  }

  // Desktop: floating glass panel anchored above the player.
  return (
    <div
      ref={trapRef}
      role="dialog"
      aria-modal="true"
      aria-label={t('eq.title')}
      className="glass-modal scale-in fixed right-4 z-40 w-[460px] max-w-[calc(100vw-32px)] pt-3"
      style={{
        bottom: 'calc(var(--player-clearance) + 8px)',
        borderRadius: 'var(--radius-panel)',
        boxShadow: 'var(--shadow-3)'
      }}
    >
      <div className="mb-1 flex items-center justify-between px-4">
        <h2 className="text-[14px] font-bold">{t('eq.title')}</h2>
        <button className="icon-btn h-7 w-7" onClick={() => setOpen(false)} aria-label={t('common.close')}>
          <X size={15} />
        </button>
      </div>
      <EqContent />
    </div>
  )
}
