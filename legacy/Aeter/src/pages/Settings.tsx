import { useTranslation } from 'react-i18next'
import { Sparkles } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import EnrichmentDashboard from '@/components/library/EnrichmentDashboard'
import { Section } from '@/components/settings/controls'
import LibrarySection from '@/components/settings/LibrarySection'
import DownloadsSection from '@/components/settings/DownloadsSection'
import PlaybackSection from '@/components/settings/PlaybackSection'
import AppearanceSection from '@/components/settings/AppearanceSection'
import IntegrationsSection from '@/components/settings/IntegrationsSection'
import SpotifyMigrationSection from '@/components/settings/SpotifyMigrationSection'
import SyncSection from '@/components/settings/SyncSection'
import RemoteAccessSection from '@/components/settings/RemoteAccessSection'
import DuplicatesSection from '@/components/settings/DuplicatesSection'
import ShortcutsSection from '@/components/settings/ShortcutsSection'
import { useSettingsStore } from '@/store/useSettingsStore'

export default function Settings(): React.JSX.Element {
  const { t } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)

  if (!settings) {
    return (
      <div className="flex flex-col gap-3 p-8 pt-14">
        {Array.from({ length: 5 }).map((_, i) => (
          <div key={i} className="skeleton h-24" />
        ))}
      </div>
    )
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader title={t('settings.title')} />
      <div className="min-h-0 flex-1 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)]">
        <div className="max-w-[880px]">
          <LibrarySection index={0} />
          <DownloadsSection index={1} />
          <PlaybackSection index={2} />
          <AppearanceSection index={3} />
          <IntegrationsSection index={4} />
          <SpotifyMigrationSection index={5} />
          <SyncSection index={6} />
          <RemoteAccessSection index={7} />
          <Section title={t('enrich_dash.section')} icon={Sparkles} index={8}>
            <EnrichmentDashboard />
          </Section>
          <DuplicatesSection index={9} />
          <ShortcutsSection index={10} />
        </div>
      </div>
    </div>
  )
}
