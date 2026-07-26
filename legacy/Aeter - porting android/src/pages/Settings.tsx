import { useTranslation } from 'react-i18next'
import { Sparkles } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import EnrichmentDashboard from '@/components/library/EnrichmentDashboard'
import { Section } from '@/components/settings/controls'
import LibrarySection from '@/components/settings/LibrarySection'
import DownloadsSection from '@/components/settings/DownloadsSection'
import SpotifyMigrationSection from '@/components/settings/SpotifyMigrationSection'
import YoutubeDownloadSection from '@/components/settings/YoutubeDownloadSection'
import PlaybackSection from '@/components/settings/PlaybackSection'
import AppearanceSection from '@/components/settings/AppearanceSection'
import IntegrationsSection from '@/components/settings/IntegrationsSection'
import SyncSection from '@/components/settings/SyncSection'
import RemoteConnectionSection from '@/components/settings/RemoteConnectionSection'
import PhoneRepairSection from '@/components/settings/PhoneRepairSection'
import DuplicatesSection from '@/components/settings/DuplicatesSection'
import ShortcutsSection from '@/components/settings/ShortcutsSection'
import { useSettingsStore } from '@/store/useSettingsStore'
import { isMobile } from '@/lib/platform'
import { isLanModeActive } from '@/lib/lanClient'

export default function Settings(): React.JSX.Element {
  const { t } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  // In LAN thin-client mode there's no on-device library/downloader/enrichment
  // — those sections call window.aether methods outside the LAN allow-list
  // (electron/modules/lan/routes.ts) and would just error. Appearance/Playback
  // still work (plain local prefs, see lanClient.ts's local getSettings/
  // setSettings) and RemoteConnectionSection manages the pairing itself.
  const lanMode = isLanModeActive()

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
          {!lanMode && (
            <>
              <LibrarySection index={0} />
              <DownloadsSection index={1} />
              <SpotifyMigrationSection index={2} />
              <YoutubeDownloadSection index={3} />
            </>
          )}
          <PlaybackSection index={4} />
          <AppearanceSection index={5} />
          {!lanMode && (
            <>
              <IntegrationsSection index={6} />
              <SyncSection index={7} />
            </>
          )}
          {/* LAN pairing only makes sense as a thin client, i.e. on mobile. */}
          {isMobile && <RemoteConnectionSection index={8} />}
          {/* Repair-from-PC needs the on-device library → standalone mode only. */}
          {isMobile && !lanMode && <PhoneRepairSection index={8} />}
          {!lanMode && (
            <>
              <Section title={t('enrich_dash.section')} icon={Sparkles} index={9} dataTour="settings-enrichment">
                <EnrichmentDashboard />
              </Section>
              <DuplicatesSection index={10} />
            </>
          )}
          {/* Keyboard shortcuts do nothing on touch — hide the section on mobile. */}
          {!isMobile && <ShortcutsSection index={11} />}
          <p className="pb-6 pt-4 text-center text-[12px] text-text-3">
            {t('settings.version', {
              version: typeof __APP_VERSION__ !== 'undefined' ? __APP_VERSION__ : 'dev'
            })}
          </p>
        </div>
      </div>
    </div>
  )
}
