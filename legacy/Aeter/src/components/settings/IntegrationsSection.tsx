import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Plug, ShieldAlert } from 'lucide-react'
import { Section, FieldRow, Switch, inputCls } from './controls'
import ScrobbleSettings from './ScrobbleSettings'
import { useSettingsStore } from '@/store/useSettingsStore'

export default function IntegrationsSection({ index = 0 }: { index?: number }): React.JSX.Element | null {
  const { t } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  const [secretsEncrypted, setSecretsEncrypted] = useState(true)

  useEffect(() => {
    window.aether
      .getSecurityStatus()
      .then((s) => setSecretsEncrypted(s.secretsEncrypted))
      .catch(() => {})
  }, [])

  if (!settings) return null

  return (
    <Section title={t('settings.section_integrations')} icon={Plug} index={index}>
      <FieldRow label={t('settings.notifications')}>
        <Switch
          checked={settings.notificationsOnTrackChange}
          label={t('settings.notifications')}
          onChange={(checked) => void update({ notificationsOnTrackChange: checked })}
        />
      </FieldRow>
      <FieldRow label={t('settings.media_keys')}>
        <Switch
          checked={settings.globalMediaKeys}
          label={t('settings.media_keys')}
          onChange={(checked) => void update({ globalMediaKeys: checked })}
        />
      </FieldRow>
      <FieldRow label={t('settings.spotify_credentials')} hint={t('settings.spotify_hint')}>
        <div className="flex flex-col gap-1.5">
          <input
            className={inputCls}
            placeholder={t('settings.client_id_ph')}
            defaultValue={settings.spotifyClientId}
            onBlur={(e) => void update({ spotifyClientId: e.target.value.trim() })}
          />
          <input
            className={inputCls}
            placeholder={t('settings.client_secret_ph')}
            type="password"
            defaultValue={settings.spotifyClientSecret}
            onBlur={(e) => void update({ spotifyClientSecret: e.target.value.trim() })}
          />
        </div>
      </FieldRow>
      <FieldRow label={t('settings.acoustid_key')} hint="acoustid.org/new-application">
        <input
          className={inputCls}
          placeholder={t('settings.api_key_ph')}
          defaultValue={settings.acoustidApiKey}
          onBlur={(e) => void update({ acoustidApiKey: e.target.value.trim() })}
        />
      </FieldRow>
      <FieldRow label={t('settings.lastfm_key')} hint="last.fm/api/account/create">
        <input
          className={inputCls}
          placeholder={t('settings.api_key_ph')}
          defaultValue={settings.lastfmApiKey}
          onBlur={(e) => void update({ lastfmApiKey: e.target.value.trim() })}
        />
      </FieldRow>
      <ScrobbleSettings />
      {!secretsEncrypted && (
        <div className="flex items-start gap-2 rounded-lg bg-[var(--warning-soft)] px-3 py-2 text-[12px] text-[var(--warning)]">
          <ShieldAlert size={14} className="mt-0.5 shrink-0" />
          {t('errors.secrets_plaintext')}
        </div>
      )}
    </Section>
  )
}
