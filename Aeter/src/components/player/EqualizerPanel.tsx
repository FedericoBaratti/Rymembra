import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X, Save } from 'lucide-react'
import { useUiStore } from '@/store/useUiStore'
import { useSettingsStore } from '@/store/useSettingsStore'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { audioGraph, EQ_FREQUENCIES, EQ_PRESETS } from '@/lib/audio'

function freqLabel(f: number): string {
  return f >= 1000 ? `${f / 1000}k` : `${f}`
}

export default function EqualizerPanel(): React.JSX.Element | null {
  const { t } = useTranslation()
  const open = useUiStore((s) => s.eqOpen)
  const setOpen = useUiStore((s) => s.setEqOpen)
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  const [savingName, setSavingName] = useState<string | null>(null)
  const trapRef = useFocusTrap<HTMLDivElement>(open, () => setOpen(false))

  if (!open || !settings) return null

  const gains = settings.eqGains
  const enabled = settings.eqEnabled

  const apply = (nextGains: number[], nextEnabled: boolean): void => {
    audioGraph.setEq(nextGains, nextEnabled)
    void update({ eqGains: nextGains, eqEnabled: nextEnabled })
  }

  const setBand = (i: number, value: number): void => {
    const next = [...gains]
    next[i] = value
    apply(next, enabled)
  }

  const allPresets: { name: string; gains: number[] }[] = [
    ...Object.entries(EQ_PRESETS).map(([name, g]) => ({ name, gains: g })),
    ...settings.eqCustomPresets
  ]

  const savePreset = (): void => {
    const name = savingName?.trim()
    if (!name) return
    const customs = settings.eqCustomPresets.filter((p) => p.name !== name)
    void update({ eqCustomPresets: [...customs, { name, gains: [...gains] }] })
    setSavingName(null)
  }

  return (
    <div
      ref={trapRef}
      role="dialog"
      aria-modal="true"
      aria-label={t('eq.title')}
      className="glass-modal scale-in fixed right-4 z-40 w-[460px] max-w-[calc(100vw-32px)] p-4"
      style={{
        bottom: 'calc(var(--player-clearance) + 8px)',
        borderRadius: 'var(--radius-panel)',
        boxShadow: 'var(--shadow-3)'
      }}
    >
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-[14px] font-bold">{t('eq.title')}</h2>
        <div className="flex items-center gap-2">
          <label className="flex cursor-pointer items-center gap-2 text-[12px] text-text-2">
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
          <button className="icon-btn h-7 w-7" onClick={() => setOpen(false)} aria-label={t('common.close')}>
            <X size={15} />
          </button>
        </div>
      </div>

      <div className="mb-3 flex items-center gap-2">
        <select
          className="field-input h-8 flex-1 text-[12.5px]"
          value=""
          onChange={(e) => {
            const preset = allPresets.find((p) => p.name === e.target.value)
            if (preset) apply([...preset.gains], true)
          }}
        >
          <option value="" disabled>
            {t('eq.preset')}
          </option>
          {allPresets.map((p) => (
            <option key={p.name} value={p.name}>
              {p.name}
            </option>
          ))}
        </select>
        {savingName == null ? (
          <button
            className="icon-btn h-8 w-8"
            onClick={() => setSavingName('')}
            title={t('eq.save_preset')}
          >
            <Save size={15} />
          </button>
        ) : (
          <div className="flex items-center gap-1">
            <input
              autoFocus
              className="field-input h-8 w-32 text-[12.5px]"
              placeholder={t('eq.preset_name')}
              value={savingName}
              onChange={(e) => setSavingName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && savePreset()}
            />
            <button className="icon-btn h-8 w-8" onClick={savePreset}>
              <Save size={15} />
            </button>
          </div>
        )}
      </div>

      <div className={`flex justify-between gap-1 ${enabled ? '' : 'opacity-40'}`}>
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
            />
            <span className="text-[10px] text-text-3">{freqLabel(freq)}</span>
          </div>
        ))}
      </div>

      <style>{`
        .eq-slider {
          -webkit-appearance: none;
          appearance: none;
          writing-mode: vertical-lr;
          direction: rtl;
          width: 22px;
          height: 130px;
          background: transparent;
        }
        .eq-slider::-webkit-slider-runnable-track {
          width: 4px;
          background: rgba(255,255,255,0.14);
          border-radius: 2px;
        }
        .eq-slider::-webkit-slider-thumb {
          -webkit-appearance: none;
          width: 14px;
          height: 14px;
          margin-left: -5px;
          border-radius: 50%;
          background: var(--accent);
          box-shadow: 0 0 10px var(--accent-glow);
          cursor: pointer;
          transition: transform 150ms;
        }
        .eq-slider::-webkit-slider-thumb:hover {
          transform: scale(1.2);
        }
      `}</style>
    </div>
  )
}
