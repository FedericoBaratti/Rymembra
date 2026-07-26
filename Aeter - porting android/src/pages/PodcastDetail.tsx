import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { ArrowLeft, Play, RefreshCw, Trash2, Loader2, CheckCircle2 } from 'lucide-react'
import EmptyState from '@/components/ui/EmptyState'
import CoverImage from '@/components/ui/CoverImage'
import { remoteImageUrl, formatDuration } from '@/lib/format'
import { playEpisode } from '@/lib/podcast'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { isMobile } from '@/lib/platform'
import type { Podcast, PodcastEpisode } from '@shared/types'

// 44px minimum touch target on mobile; compact on desktop.
const headerBtn = isMobile ? 'h-11 w-11' : 'h-10 w-10'

export default function PodcastDetail(): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { id } = useParams<{ id: string }>()
  const podcastId = Number(id)
  const [podcast, setPodcast] = useState<Podcast | null>(null)
  const [episodes, setEpisodes] = useState<PodcastEpisode[] | null>(null)
  const [refreshing, setRefreshing] = useState(false)

  const load = (): void => {
    void window.aether
      .getPodcasts()
      .then((all) => setPodcast(all.find((p) => p.id === podcastId) ?? null))
      .catch(() => setPodcast(null))
    void window.aether.getPodcastEpisodes(podcastId).then(setEpisodes).catch(() => setEpisodes([]))
  }
  useEffect(load, [podcastId])

  const refresh = async (): Promise<void> => {
    setRefreshing(true)
    try {
      const { added } = await window.aether.refreshPodcast(podcastId)
      toast.success(t('podcasts.refreshed', { count: added }))
      load()
    } catch (e) {
      toast.error(ipcErrorMessage(e))
    } finally {
      setRefreshing(false)
    }
  }

  const remove = async (): Promise<void> => {
    try {
      await window.aether.removePodcast(podcastId)
      toast.success(t('podcasts.removed'))
      navigate('/podcasts')
    } catch (e) {
      toast.error(ipcErrorMessage(e))
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-2 px-[var(--content-x)] pt-3">
        <button className={`icon-btn ${headerBtn}`} onClick={() => navigate('/podcasts')} aria-label={t('common.back')}>
          <ArrowLeft size={20} />
        </button>
        <div className="flex-1" />
        <button className={`icon-btn disabled:opacity-60 ${headerBtn}`} onClick={() => void refresh()} disabled={refreshing} aria-label={t('podcasts.refresh')}>
          {refreshing ? <Loader2 size={18} className="animate-spin" /> : <RefreshCw size={18} />}
        </button>
        <button className={`icon-btn ${headerBtn}`} onClick={() => void remove()} aria-label={t('podcasts.remove')}>
          <Trash2 size={18} />
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)]">
        {podcast && (
          <div className="mb-5 flex items-end gap-4 pt-2">
            <div className="h-28 w-28 shrink-0 overflow-hidden rounded-xl bg-surface-3 shadow-lg">
              <CoverImage
                src={podcast.image_url ? remoteImageUrl(podcast.image_url) : null}
                eager
                className="h-full w-full object-cover"
              />
            </div>
            <div className="min-w-0">
              <h1 className="truncate text-[20px] font-bold tracking-tight">{podcast.title}</h1>
              <div className="truncate text-[13px] text-text-3">{podcast.author}</div>
            </div>
          </div>
        )}

        {episodes == null ? (
          <div className="flex flex-col gap-2">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="skeleton h-12" />
            ))}
          </div>
        ) : episodes.length === 0 ? (
          <EmptyState icon={Play} title={t('podcasts.no_episodes')} subtitle="" />
        ) : (
          episodes.map((ep) => {
            const pct =
              ep.duration && ep.progress_sec > 0 ? Math.min(100, (ep.progress_sec / ep.duration) * 100) : 0
            return (
              <button
                key={ep.id}
                className="flex w-full items-center gap-3 rounded-lg px-1 py-2 text-left transition-colors hover:bg-white/[0.05]"
                onClick={() => playEpisode(ep)}
              >
                <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full" style={{ background: 'var(--accent)' }}>
                  <Play size={15} className="ml-0.5 text-white" fill="currentColor" />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-1.5">
                    {ep.played === 1 && <CheckCircle2 size={13} className="shrink-0 text-[var(--accent)]" />}
                    <span className="truncate text-[13px] font-medium">{ep.title}</span>
                  </span>
                  <span className="block truncate text-[11.5px] text-text-3">
                    {ep.published_at ? new Date(ep.published_at).toLocaleDateString() : ''}
                    {ep.duration ? ` · ${formatDuration(ep.duration)}` : ''}
                  </span>
                  {pct > 0 && (
                    <span className="mt-1 block h-0.5 w-full overflow-hidden rounded-full bg-white/10">
                      <span className="block h-full rounded-full" style={{ width: `${pct}%`, background: 'var(--accent)' }} />
                    </span>
                  )}
                </span>
              </button>
            )
          })
        )}
      </div>
    </div>
  )
}
