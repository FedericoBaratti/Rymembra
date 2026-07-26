import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Podcast as PodcastIcon, Search, Plus, Loader2, Play } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import SearchButton from '@/components/ui/SearchButton'
import EmptyState from '@/components/ui/EmptyState'
import CoverImage from '@/components/ui/CoverImage'
import { remoteImageUrl, formatDuration } from '@/lib/format'
import { playEpisode } from '@/lib/podcast'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { isMobile } from '@/lib/platform'
import { usePullToRefresh } from '@/hooks/usePullToRefresh'
import type { Podcast, PodcastSearchResult, PodcastEpisode } from '@shared/types'

export default function Podcasts(): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [subs, setSubs] = useState<Podcast[] | null>(null)
  const [latest, setLatest] = useState<PodcastEpisode[]>([])
  const [term, setTerm] = useState('')
  const [searching, setSearching] = useState(false)
  const [hits, setHits] = useState<PodcastSearchResult[] | null>(null)
  const [adding, setAdding] = useState<string | null>(null)
  const debounce = useRef<number | null>(null)

  const refresh = (): void => {
    void window.aether.getPodcasts().then(setSubs).catch(() => setSubs([]))
    void window.aether.getLatestEpisodes(12).then(setLatest).catch(() => setLatest([]))
  }
  useEffect(refresh, [])

  // Pull-to-refresh: ricontrolla i feed RSS delle iscrizioni (nuove puntate),
  // poi ricarica le liste dal DB. allSettled: un feed morto non blocca gli altri.
  const pullToRefresh = usePullToRefresh(async () => {
    const list = subs ?? []
    await Promise.allSettled(list.map((p) => window.aether.refreshPodcast(p.id)))
    refresh()
  })

  useEffect(() => {
    if (debounce.current) window.clearTimeout(debounce.current)
    const q = term.trim()
    if (!q) {
      setHits(null)
      setSearching(false)
      return
    }
    setSearching(true)
    debounce.current = window.setTimeout(() => {
      void window.aether
        .searchPodcasts(q)
        .then(setHits)
        .catch(() => setHits([]))
        .finally(() => setSearching(false))
    }, 450)
  }, [term])

  const add = async (feedUrl: string): Promise<void> => {
    if (!feedUrl || adding) return
    setAdding(feedUrl)
    try {
      const p = await window.aether.addPodcast(feedUrl)
      toast.success(t('podcasts.added', { name: p.title }))
      setTerm('')
      setHits(null)
      refresh()
    } catch (e) {
      toast.error(ipcErrorMessage(e))
    } finally {
      setAdding(null)
    }
  }

  // Allow pasting a raw RSS URL directly into the search box.
  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault()
    const q = term.trim()
    if (/^https?:\/\//i.test(q)) void add(q)
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title={t('podcasts.title')}
        subtitle={t('podcasts.subtitle')}
        actions={isMobile ? <SearchButton /> : undefined}
      />

      <div
        className="relative min-h-0 flex-1 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)]"
        {...pullToRefresh.pullProps}
      >
        {pullToRefresh.indicator}
        <form onSubmit={onSubmit} className="mb-4 flex items-center gap-2 rounded-xl bg-white/[0.05] px-3">
          <Search size={16} className="text-text-3" />
          <input
            className="h-11 flex-1 bg-transparent text-[14px] outline-none placeholder:text-text-3"
            placeholder={t('podcasts.search_placeholder')}
            value={term}
            onChange={(e) => setTerm(e.target.value)}
          />
          {searching && <Loader2 size={15} className="animate-spin text-text-3" />}
        </form>

        {/* search results */}
        {hits != null && (
          <section className="mb-5">
            {hits.length === 0 && !searching ? (
              <div className="px-1 py-4 text-[13px] text-text-3">{t('podcasts.no_results')}</div>
            ) : (
              hits.slice(0, 12).map((h) => (
                <div key={h.feedUrl} className="flex items-center gap-3 rounded-lg px-1 py-1.5">
                  <div className="h-11 w-11 shrink-0 overflow-hidden rounded bg-surface-3">
                    <CoverImage
                      src={h.imageUrl ? remoteImageUrl(h.imageUrl) : null}
                      className="h-full w-full object-cover"
                    />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[13.5px] font-medium">{h.title}</div>
                    <div className="truncate text-[11.5px] text-text-3">{h.author}</div>
                  </div>
                  <button
                    className={`icon-btn shrink-0 disabled:opacity-60 ${isMobile ? 'h-11 w-11' : 'h-9 w-9'}`}
                    onClick={() => void add(h.feedUrl)}
                    disabled={adding === h.feedUrl}
                    aria-label={t('podcasts.subscribe')}
                  >
                    {adding === h.feedUrl ? <Loader2 size={16} className="animate-spin" /> : <Plus size={16} />}
                  </button>
                </div>
              ))
            )}
          </section>
        )}

        {/* latest episodes across subscriptions */}
        {hits == null && latest.length > 0 && (
          <section className="mb-5">
            <h2 className="mb-2 text-[15px] font-bold tracking-tight">{t('podcasts.latest')}</h2>
            {latest.map((ep) => (
              <button
                key={ep.id}
                className="flex w-full items-center gap-3 rounded-lg px-1 py-1.5 text-left transition-colors hover:bg-white/[0.05]"
                onClick={() => playEpisode(ep)}
              >
                <div className="relative h-11 w-11 shrink-0 overflow-hidden rounded bg-surface-3">
                  <CoverImage
                    src={ep.image_url ? remoteImageUrl(ep.image_url) : null}
                    className="h-full w-full object-cover"
                  />
                  <span className="absolute inset-0 flex items-center justify-center bg-black/35">
                    <Play size={15} className="text-white" fill="currentColor" />
                  </span>
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px] font-medium">{ep.title}</div>
                  <div className="truncate text-[11.5px] text-text-3">
                    {ep.podcast_title}
                    {ep.duration ? ` · ${formatDuration(ep.duration)}` : ''}
                  </div>
                </div>
              </button>
            ))}
          </section>
        )}

        {/* subscriptions */}
        {hits == null && (
          <section>
            <h2 className="mb-2 text-[15px] font-bold tracking-tight">{t('podcasts.subscriptions')}</h2>
            {subs == null ? (
              <div className="flex flex-col gap-2">
                {Array.from({ length: 4 }).map((_, i) => (
                  <div key={i} className="skeleton h-14" />
                ))}
              </div>
            ) : subs.length === 0 ? (
              <EmptyState icon={PodcastIcon} title={t('podcasts.empty_title')} subtitle={t('podcasts.empty_subtitle')} />
            ) : (
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
                {subs.map((p) => (
                  <button
                    key={p.id}
                    className="card-lift flex flex-col gap-2 rounded-xl p-2 text-left hover:bg-white/[0.05]"
                    onClick={() => navigate(`/podcasts/${p.id}`)}
                  >
                    <div className="aspect-square w-full overflow-hidden rounded-lg bg-surface-3">
                      <CoverImage
                        src={p.image_url ? remoteImageUrl(p.image_url) : null}
                        className="h-full w-full object-cover"
                        fallback={
                          <div className="flex h-full w-full items-center justify-center">
                            <PodcastIcon size={28} className="text-text-3" />
                          </div>
                        }
                      />
                    </div>
                    <div>
                      <div className="truncate text-[12.5px] font-semibold">{p.title}</div>
                      <div className="truncate text-[11px] text-text-3">
                        {t('podcasts.episode_count', { count: p.episode_count ?? 0 })}
                      </div>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </section>
        )}
      </div>
    </div>
  )
}
