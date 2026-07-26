import { useTranslation } from 'react-i18next'
import { Copy } from 'lucide-react'
import { Section, FieldRow, Switch } from './controls'
import Select from '@/components/ui/Select'
import { useSettingsStore } from '@/store/useSettingsStore'

export default function DuplicatesSection({ index = 0 }: { index?: number }): React.JSX.Element | null {
  const { t } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)

  if (!settings) return null

  return (
    <Section
      title={t('settings.section_duplicates')}
      icon={Copy}
      index={index}
      dataTour="settings-duplicates"
    >
      <FieldRow
        label={t('settings.auto_remove_duplicates')}
        hint={t('settings.auto_remove_duplicates_hint')}
      >
        <Switch
          checked={settings.dedupeAutoRemove}
          label={t('settings.auto_remove_duplicates')}
          onChange={(checked) => void update({ dedupeAutoRemove: checked })}
        />
      </FieldRow>
      {settings.dedupeAutoRemove && (
        <FieldRow label={t('settings.keep_quality')}>
          <Select<typeof settings.dedupeKeep>
            title={t('settings.keep_quality')}
            ariaLabel={t('settings.keep_quality')}
            value={settings.dedupeKeep}
            options={[
              { value: 'higher', label: t('settings.keep_higher') },
              { value: 'lower', label: t('settings.keep_lower') }
            ]}
            onChange={(dedupeKeep) => void update({ dedupeKeep })}
          />
        </FieldRow>
      )}
    </Section>
  )
}
