import { lazy, Suspense, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowRight, Loader2, RefreshCw, Youtube } from 'lucide-react'
import { Section } from '@/components/settings/controls'
import { useDownloadsStore } from '@/store/useDownloadsStore'
import { isMobile } from '@/lib/platform'
import { YtDlpNative } from '@/lib/nativeRpc'
import { toast } from '@/store/useToastStore'
// Mount-on-open: chunk separato, caricato alla prima apertura del flow.
const YoutubeDownloadFlow = lazy(() => import('./YoutubeDownloadFlow'))

const YT_RED = '#FF0000'

/** Branded YouTube glyph (shared look with the flow header). */
function YoutubeGlyph({ size = 18 }: { size?: number }): React.JSX.Element {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill={YT_RED} aria-hidden>
      <path d="M23.5 6.2a3.02 3.02 0 0 0-2.12-2.14C19.5 3.55 12 3.55 12 3.55s-7.5 0-9.38.51A3.02 3.02 0 0 0 .5 6.2C0 8.08 0 12 0 12s0 3.92.5 5.8a3.02 3.02 0 0 0 2.12 2.14c1.88.51 9.38.51 9.38.51s7.5 0 9.38-.51a3.02 3.02 0 0 0 2.12-2.14C24 15.92 24 12 24 12s0-3.92-.5-5.8zM9.55 15.57V8.43L15.82 12l-6.27 3.57z" />
    </svg>
  )
}

/** Mobile-only diagnostics row: the yt-dlp version currently installed by the
 *  native plugin, plus a forced-update button (bypasses the 24h auto-update
 *  cache — useful the day YouTube breaks old yt-dlp builds again). */
function YtdlpUpdateRow(): React.JSX.Element {
  const { t } = useTranslation()
  const [version, setVersion] = useState<string | null>(null)
  const [updating, setUpdating] = useState(false)

  useEffect(() => {
    YtDlpNative.version()
      .then((r) => setVersion(r.version || null))
      .catch(() => setVersion(null))
  }, [])

  const forceUpdate = async (): Promise<void> => {
    setUpdating(true)
    try {
      const r = await YtDlpNative.update()
      setVersion(r.version || null)
      toast.success(t('youtubeDownload.ytdlp_updated', { version: r.version || '?' }))
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      if (/YTDLP_BUSY/i.test(msg)) toast.info(t('youtubeDownload.ytdlp_busy'))
      else toast.error(t('youtubeDownload.ytdlp_update_failed'))
    } finally {
      setUpdating(false)
    }
  }

  return (
    <div className="mt-2 flex items-center justify-between gap-3 rounded-xl border border-[var(--hairline)] px-4 py-2.5">
      <div className="min-w-0">
        <div className="text-[12px] font-semibold">{t('youtubeDownload.ytdlp_label')}</div>
        <div className="mt-0.5 truncate text-[11px] text-text-3">
          {version ?? t('youtubeDownload.ytdlp_unknown')}
        </div>
      </div>
      <button
        className="flex h-9 shrink-0 items-center gap-2 rounded-lg border border-[var(--hairline)] px-3 text-[12px] font-semibold text-text-2 transition active:scale-[0.97] disabled:opacity-50"
        onClick={() => void forceUpdate()}
        disabled={updating}
      >
        <RefreshCw size={14} className={updating ? 'animate-spin' : undefined} />
        {t('youtubeDownload.ytdlp_update')}
      </button>
    </div>
  )
}

export default function YoutubeDownloadSection({ index = 0 }: { index?: number }): React.JSX.Element {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const items = useDownloadsStore((s) => s.items)
  const active = items.find(
    (i) =>
      i.source_type.startsWith('youtube') &&
      (i.status === 'downloading' || i.status === 'pending' || i.status === 'paused')
  )
  const pct = active && active.total_tracks > 0 ? Math.round(active.progress * 100) : 0

  return (
    <Section title={t('youtubeDownload.section')} icon={Youtube} index={index} dataTour="settings-youtube">
      <button
        className="row-lift relative flex items-center gap-4 overflow-hidden rounded-xl border border-[var(--hairline)] p-4 text-left transition active:scale-[0.995]"
        style={{ background: `linear-gradient(120deg, ${YT_RED}1f, transparent 65%)` }}
        onClick={() => setOpen(true)}
      >
        <div
          className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl"
          style={{ background: `${YT_RED}1f`, boxShadow: `0 0 24px ${YT_RED}33` }}
        >
          <YoutubeGlyph size={24} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[14px] font-bold">{t('youtubeDownload.cta_title')}</div>
          <div className="mt-0.5 text-[12px] text-text-2">
            {active
              ? t('youtubeDownload.running_status', {
                  done: active.completed_tracks,
                  total: active.total_tracks,
                  pct
                })
              : t('youtubeDownload.cta_subtitle')}
          </div>
        </div>
        {active ? (
          <Loader2 size={18} className="shrink-0 animate-spin" style={{ color: YT_RED }} />
        ) : (
          <ArrowRight size={18} className="shrink-0 text-text-3" />
        )}
      </button>

      {isMobile && <YtdlpUpdateRow />}

      <Suspense fallback={null}>{open && <YoutubeDownloadFlow onClose={() => setOpen(false)} />}</Suspense>
    </Section>
  )
}
