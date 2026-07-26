import { useTranslation } from 'react-i18next'
import { Palette, Sparkles } from 'lucide-react'
import { Section, FieldRow } from './controls'
import Select from '@/components/ui/Select'
import { useSettingsStore } from '@/store/useSettingsStore'
import { useTourStore } from '@/store/useTourStore'
import { applyTheme, applySkin } from '@/hooks/useAppBootstrap'
import { SKINS } from '@/lib/skins'

export default function AppearanceSection({ index = 0 }: { index?: number }): React.JSX.Element | null {
  const { t, i18n } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  if (!settings) return null

  return (
    <Section title={t('settings.section_appearance')} icon={Palette} index={index} dataTour="settings-appearance">
      <FieldRow label={t('settings.theme')}>
        <Select<typeof settings.theme>
          title={t('settings.theme')}
          ariaLabel={t('settings.theme')}
          value={settings.theme}
          options={[
            { value: 'dark', label: t('settings.theme_dark') },
            { value: 'light', label: t('settings.theme_light') },
            { value: 'system', label: t('settings.theme_system') }
          ]}
          onChange={(theme) => {
            applyTheme(theme)
            void update({ theme })
          }}
        />
      </FieldRow>
      <FieldRow label={t('settings.skin')}>
        <Select<typeof settings.skin>
          title={t('settings.skin')}
          ariaLabel={t('settings.skin')}
          value={settings.skin}
          options={SKINS.map((s) => ({ value: s.id, label: t(s.labelKey) }))}
          onChange={(skin) => {
            applySkin(skin)
            void update({ skin })
          }}
        />
      </FieldRow>
      <FieldRow label={t('settings.language')}>
        <Select<'it' | 'en'>
          title={t('settings.language')}
          ariaLabel={t('settings.language')}
          value={settings.language}
          options={[
            { value: 'it', label: 'Italiano' },
            { value: 'en', label: 'English' }
          ]}
          onChange={(language) => {
            void i18n.changeLanguage(language)
            void update({ language })
          }}
        />
      </FieldRow>
      <FieldRow label={t('settings.replay_tour')}>
        <button
          className="btn-ghost flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium"
          onClick={() => useTourStore.getState().start()}
        >
          <Sparkles size={13} /> {t('settings.replay_tour_btn')}
        </button>
      </FieldRow>
    </Section>
  )
}
