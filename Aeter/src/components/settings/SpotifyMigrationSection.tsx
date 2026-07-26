import { lazy, Suspense, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowRight, Loader2, Music2, Wrench } from 'lucide-react'
import { Section } from '@/components/settings/controls'
import { useSpotifyMigrationStore } from '@/store/useSpotifyMigrationStore'
import { toast } from '@/store/useToastStore'
import ConfirmDialog from '@/components/ui/ConfirmDialog'
// Mount-on-open: chunk separato, caricato alla prima apertura del flow.
const SpotifyMigrationFlow = lazy(() => import('./SpotifyMigrationFlow'))

const SPOTIFY_GREEN = '#1DB954'

/** Branded Spotify glyph (shared look with the flow header). */
function SpotifyGlyph({ size = 18 }: { size?: number }): React.JSX.Element {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill={SPOTIFY_GREEN} aria-hidden>
      <path d="M12 0C5.4 0 0 5.4 0 12s5.4 12 12 12 12-5.4 12-12S18.66 0 12 0zm5.5 17.3a.75.75 0 0 1-1.03.25c-2.82-1.72-6.37-2.11-10.55-1.16a.75.75 0 1 1-.33-1.46c4.57-1.04 8.5-.59 11.66 1.34.36.22.47.69.25 1.03zm1.47-3.27a.94.94 0 0 1-1.29.31c-3.23-1.98-8.15-2.56-11.97-1.4a.94.94 0 1 1-.54-1.8c4.37-1.32 9.79-.67 13.5 1.6.44.27.58.85.3 1.29zm.13-3.4C15.78 8.26 8.9 8.03 5.1 9.18a1.12 1.12 0 1 1-.65-2.15c4.37-1.33 11.96-1.07 16.27 1.5a1.12 1.12 0 1 1-1.15 1.93z" />
    </svg>
  )
}

export default function SpotifyMigrationSection({ index = 0 }: { index?: number }): React.JSX.Element {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [confirmRepair, setConfirmRepair] = useState(false)
  const [repairing, setRepairing] = useState(false)
  const migration = useSpotifyMigrationStore((s) => s.state)
  const running = migration?.status === 'running' || migration?.status === 'resolving'
  const pct = migration && migration.total > 0 ? Math.round((migration.done / migration.total) * 100) : 0

  const runRepair = async (): Promise<void> => {
    setRepairing(true)
    try {
      const res = await window.aether.repairSplitAlbums()
      if (res.retagged > 0) {
        toast.success(
          t('spotifyMigration.repair_done_title'),
          t('spotifyMigration.repair_done_msg', { groups: res.groups, tracks: res.retagged })
        )
      } else {
        toast.info(t('spotifyMigration.repair_none_title'), t('spotifyMigration.repair_none_msg'))
      }
    } catch {
      toast.error(t('spotifyMigration.repair_error'))
    } finally {
      setRepairing(false)
    }
  }

  return (
    <Section title={t('spotifyMigration.section')} icon={Music2} index={index} dataTour="settings-spotify">
      <button
        className="row-lift relative flex items-center gap-4 overflow-hidden rounded-xl border border-[var(--hairline)] p-4 text-left transition active:scale-[0.995]"
        style={{ background: `linear-gradient(120deg, ${SPOTIFY_GREEN}1f, transparent 65%)` }}
        onClick={() => setOpen(true)}
      >
        <div
          className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl"
          style={{ background: `${SPOTIFY_GREEN}1f`, boxShadow: `0 0 24px ${SPOTIFY_GREEN}33` }}
        >
          <SpotifyGlyph size={24} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[14px] font-bold">{t('spotifyMigration.cta_title')}</div>
          <div className="mt-0.5 text-[12px] text-text-2">
            {running
              ? t('spotifyMigration.running_status', {
                  done: migration!.done,
                  total: migration!.total,
                  pct
                })
              : t('spotifyMigration.cta_subtitle')}
          </div>
        </div>
        {running ? (
          <Loader2 size={18} className="shrink-0 animate-spin" style={{ color: SPOTIFY_GREEN }} />
        ) : (
          <ArrowRight size={18} className="shrink-0 text-text-3" />
        )}
      </button>

      <button
        className="row-lift mt-2 flex w-full items-center gap-3 rounded-xl border border-[var(--hairline)] p-3 text-left transition active:scale-[0.995] disabled:opacity-60"
        onClick={() => setConfirmRepair(true)}
        disabled={repairing}
      >
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-[var(--surface-2)]">
          {repairing ? <Loader2 size={16} className="animate-spin" /> : <Wrench size={16} className="text-text-2" />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-semibold">{t('spotifyMigration.repair_title')}</div>
          <div className="mt-0.5 text-[12px] text-text-3">{t('spotifyMigration.repair_subtitle')}</div>
        </div>
      </button>

      <Suspense fallback={null}>{open && <SpotifyMigrationFlow onClose={() => setOpen(false)} />}</Suspense>

      <ConfirmDialog
        open={confirmRepair}
        title={t('spotifyMigration.repair_title')}
        message={t('spotifyMigration.repair_confirm')}
        confirmLabel={t('spotifyMigration.repair_cta')}
        onConfirm={() => void runRepair()}
        onClose={() => setConfirmRepair(false)}
      />
    </Section>
  )
}
