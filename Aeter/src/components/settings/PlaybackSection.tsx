import { useTranslation } from 'react-i18next'
import { AudioLines } from 'lucide-react'
import { Section, FieldRow, Switch } from './controls'
import { useSettingsStore } from '@/store/useSettingsStore'
import { usePlayerStore } from '@/store/usePlayerStore'
import { playerEngine } from '@/lib/player'

export default function PlaybackSection({ index = 0 }: { index?: number }): React.JSX.Element | null {
  const { t } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  const playbackRate = usePlayerStore((s) => s.playbackRate)
  const setPlaybackRate = usePlayerStore((s) => s.setPlaybackRate)
  if (!settings) return null

  return (
    <Section title={t('settings.section_playback')} icon={AudioLines} index={index}>
      <FieldRow label={`${t('settings.crossfade')}: ${settings.crossfadeSeconds}s`}>
        <input
          type="range"
          min={0}
          max={12}
          step={1}
          value={settings.crossfadeSeconds}
          className="range-accent w-44"
          onChange={(e) => {
            const v = Number(e.target.value)
            playerEngine.configure({ crossfadeSec: v })
            void update({ crossfadeSeconds: v })
          }}
        />
      </FieldRow>
      <FieldRow label={t('settings.replaygain')}>
        <Switch
          checked={settings.replayGainEnabled}
          label={t('settings.replaygain')}
          onChange={(checked) => {
            playerEngine.configure({ rgEnabled: checked })
            void update({ replayGainEnabled: checked })
          }}
        />
      </FieldRow>
      <FieldRow label={`${t('settings.playback_rate')}: ${playbackRate.toFixed(2)}x`}>
        <input
          type="range"
          min={0.5}
          max={2}
          step={0.05}
          value={playbackRate}
          className="range-accent w-44"
          onChange={(e) => setPlaybackRate(Number(e.target.value))}
          onDoubleClick={() => setPlaybackRate(1)}
        />
      </FieldRow>
      {settings.replayGainEnabled && (
        <FieldRow label={`${t('settings.replaygain_target')}: ${settings.replayGainTargetDb} dB`}>
          <input
            type="range"
            min={-23}
            max={-12}
            step={1}
            value={settings.replayGainTargetDb}
            className="range-accent w-44"
            onChange={(e) => {
              const v = Number(e.target.value)
              playerEngine.configure({ rgTargetDb: v })
              void update({ replayGainTargetDb: v })
            }}
          />
        </FieldRow>
      )}
    </Section>
  )
}
