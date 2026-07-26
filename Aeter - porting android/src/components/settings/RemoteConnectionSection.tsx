import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Smartphone } from 'lucide-react'
import { Section, FieldRow } from './controls'
import { getStoredPairing } from '@/lib/lanClient'

/** Entry point into the LAN pairing flow (src/pages/PairDevice.tsx) — mobile only. */
export default function RemoteConnectionSection({ index }: { index?: number }): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [host, setHost] = useState<string | null>(null)

  useEffect(() => {
    void getStoredPairing().then((p) => setHost(p?.host ?? null))
  }, [])

  return (
    <Section title={t('pair_device.section_title')} icon={Smartphone} index={index}>
      <p className="max-w-xl text-[11.5px] leading-relaxed text-text-3">{t('pair_device.section_subtitle')}</p>
      <FieldRow
        label={host ? t('pair_device.connected_to', { host }) : t('pair_device.title')}
      >
        <button
          className="btn-accent rounded-lg px-3 py-2 text-[12.5px]"
          onClick={() => navigate('/settings/pair-device')}
        >
          {host ? t('pair_device.manage') : t('pair_device.scan_button')}
        </button>
      </FieldRow>
    </Section>
  )
}
