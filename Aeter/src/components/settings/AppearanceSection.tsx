import { useTranslation } from 'react-i18next'
import { Palette, Sparkles } from 'lucide-react'
import { Section, FieldRow, selectCls } from './controls'
import { useSettingsStore } from '@/store/useSettingsStore'
import { useTourStore } from '@/store/useTourStore'
import { applyTheme } from '@/hooks/useAppBootstrap'
import { SKINS, applySkin } from '@/lib/skins'

export default function AppearanceSection({ index = 0 }: { index?: number }): React.JSX.Element | null {
  const { t, i18n } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  if (!settings) return null

  return (
    <Section title={t('settings.section_appearance')} icon={Palette} index={index}>
      <FieldRow label={t('settings.theme')}>
        <select
          className={selectCls}
          value={settings.theme}
          onChange={(e) => {
            const theme = e.target.value as typeof settings.theme
            applyTheme(theme)
            void update({ theme })
          }}
        >
          <option value="dark">{t('settings.theme_dark')}</option>
          <option value="light">{t('settings.theme_light')}</option>
          <option value="system">{t('settings.theme_system')}</option>
        </select>
      </FieldRow>
      <FieldRow label={t('settings.skin')}>
        <select
          className={selectCls}
          value={settings.skin}
          onChange={(e) => {
            const skin = e.target.value as typeof settings.skin
            applySkin(skin)
            void update({ skin })
          }}
        >
          {SKINS.map((s) => (
            <option key={s.id} value={s.id}>
              {t(s.labelKey)}
            </option>
          ))}
        </select>
      </FieldRow>
      <FieldRow label={t('settings.language')}>
        <select
          className={selectCls}
          value={settings.language}
          onChange={(e) => {
            const language = e.target.value as 'it' | 'en'
            void i18n.changeLanguage(language)
            void update({ language })
          }}
        >
          <option value="it">Italiano</option>
          <option value="en">English</option>
        </select>
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
