import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Heart, BarChart3, Sparkles, RefreshCw } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import SearchButton from '@/components/ui/SearchButton'
import { isMobile } from '@/lib/platform'
import EmptyState from '@/components/ui/EmptyState'
import { SectionHeading, TrackCarousel, ExternalCarousel } from '@/components/discovery/Carousels'
import { useLibraryStore } from '@/store/useLibraryStore'
import { usePullToRefresh } from '@/hooks/usePullToRefresh'
import type { HomeFeed } from '@shared/types'

export default function Home(): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const libraryLoaded = useLibraryStore((s) => s.loaded)
  const trackCount = useLibraryStore((s) => s.tracks.length)
  const [feed, setFeed] = useState<HomeFeed | null>(null)
  const [loading, setLoading] = useState(true)
  const [feedError, setFeedError] = useState(false)
  const [retryTick, setRetryTick] = useState(0)
  const fetchedOnce = useRef(false)
  const pullToRefresh = usePullToRefresh(async () => {
    const f = await window.aether.getHomeFeed()
    setFeed(f)
    setFeedError(false)
  })

  useEffect(() => {
    let alive = true
    const run = (): void => {
      window.aether
        .getHomeFeed()
        .then((f) => {
          if (alive) {
            setFeed(f)
            setFeedError(false)
          }
        })
        .catch(() => {
          // Keep the shortcuts usable and surface a retry instead of failing
          // silently into an empty-looking Home.
          if (alive) {
            setFeed((prev) => prev ?? { sections: [], likedCount: 0 })
            setFeedError(true)
          }
        })
        .finally(() => {
          if (alive) setLoading(false)
        })
    }
    // First fetch runs immediately; scan-driven refetches (trackCount steps up
    // once per debounced refreshAll) coalesce behind a trailing debounce so we
    // don't hammer getHomeFeed once per step.
    const first = !fetchedOnce.current
    fetchedOnce.current = true
    if (first) setLoading(true)
    const timer = setTimeout(run, first ? 0 : 800)
    return () => {
      alive = false
      clearTimeout(timer)
    }
    // refetch when the library size changes (scan finished / tracks added)
  }, [trackCount, retryTick])

  const shortcuts = (
    <div className="home-shortcuts grid grid-cols-2 gap-3 px-[var(--content-x)] pb-4">
      <button
        className="card-lift flex items-center gap-3 rounded-xl p-3 text-left"
        style={{ background: 'linear-gradient(135deg, rgba(var(--accent-rgb)/0.35), rgba(var(--accent-rgb)/0.12))' }}
        onClick={() => navigate('/liked')}
      >
        <span className="flex h-10 w-10 items-center justify-center rounded-lg bg-white/15">
          <Heart size={18} fill="currentColor" />
        </span>
        <div className="min-w-0">
          <div className="truncate text-[13.5px] font-semibold">{t('liked.title')}</div>
          <div className="truncate text-[11.5px] text-text-2">
            {t('liked.count', { count: feed?.likedCount ?? 0 })}
          </div>
        </div>
      </button>
      <button
        className="card-lift flex items-center gap-3 rounded-xl p-3 text-left hover:bg-white/[0.05]"
        style={{ background: 'rgba(255,255,255,0.05)' }}
        onClick={() => navigate('/stats')}
      >
        <span className="flex h-10 w-10 items-center justify-center rounded-lg bg-white/10">
          <BarChart3 size={18} />
        </span>
        <div className="min-w-0">
          <div className="truncate text-[13.5px] font-semibold">{t('stats.title')}</div>
          <div className="truncate text-[11.5px] text-text-2">{t('stats.subtitle')}</div>
        </div>
      </button>
    </div>
  )

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title={t('home.title')}
        subtitle={t('home.subtitle')}
        actions={isMobile ? <SearchButton /> : undefined}
      />
      {loading && !feed ? (
        <div className="flex flex-col gap-4 px-[var(--content-x)]">
          {Array.from({ length: 3 }).map((_, i) => (
            <div key={i} className="skeleton h-40" />
          ))}
        </div>
      ) : !libraryLoaded || trackCount === 0 ? (
        <EmptyState
          icon={Sparkles}
          title={t('home.empty_title')}
          subtitle={t('home.empty_subtitle')}
          action={
            <button className="btn-accent mt-1 rounded-lg px-4 py-2 text-[13px]" onClick={() => navigate('/settings')}>
              {t('library.add_folder')}
            </button>
          }
        />
      ) : (
        <div
          className="relative min-h-0 flex-1 overflow-y-auto pb-[var(--player-clearance)] pt-1"
          {...pullToRefresh.pullProps}
        >
          {pullToRefresh.indicator}
          {shortcuts}
          {feedError && (
            <div className="mx-[var(--content-x)] mb-4 flex items-center justify-between gap-3 rounded-xl bg-white/[0.05] px-4 py-3">
              <span className="text-[12.5px] text-text-2">{t('home.feed_error')}</span>
              <button
                className="btn-accent flex h-9 shrink-0 items-center gap-1.5 rounded-lg px-3 text-[12.5px]"
                onClick={() => setRetryTick((n) => n + 1)}
              >
                <RefreshCw size={14} />
                {t('common.retry')}
              </button>
            </div>
          )}
          {(feed?.sections ?? []).map((section) => (
            <section key={section.id} className="mb-5">
              <SectionHeading title={t(section.titleKey, { name: section.titleArg ?? '' })} />
              <div className="px-[var(--content-x)]">
                {section.kind === 'external' && section.external ? (
                  <ExternalCarousel tracks={section.external} />
                ) : section.tracks ? (
                  <TrackCarousel tracks={section.tracks} />
                ) : null}
              </div>
            </section>
          ))}
        </div>
      )}
    </div>
  )
}
