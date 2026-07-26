import { useTranslation } from 'react-i18next'
import { Keyboard } from 'lucide-react'
import { Section } from './controls'

const SHORTCUTS: [string, string][] = [
  ['Space', 'play_pause'],
  ['← / →', 'seek'],
  ['Shift + ← / →', 'prev_next_track'],
  ['↑ / ↓', 'volume'],
  ['M', 'mute'],
  ['F', 'visualizer'],
  ['Ctrl/Cmd + F', 'search'],
  ['Ctrl/Cmd + L', 'queue'],
  ['R', 'repeat'],
  ['S', 'shuffle']
]

export default function ShortcutsSection({ index = 0 }: { index?: number }): React.JSX.Element {
  const { t } = useTranslation()
  return (
    <Section title={t('settings.section_shortcuts')} icon={Keyboard} index={index}>
      <div className="grid grid-cols-2 gap-x-8 gap-y-1.5">
        {SHORTCUTS.map(([keys, action]) => (
          <div key={keys} className="flex items-center justify-between text-[12.5px]">
            <span className="text-text-2">{t(`shortcuts.${action}`)}</span>
            <kbd
              className="rounded border px-1.5 py-0.5 text-[10.5px] text-text-3"
              style={{ borderColor: 'var(--hairline)' }}
            >
              {keys}
            </kbd>
          </div>
        ))}
      </div>
    </Section>
  )
}
