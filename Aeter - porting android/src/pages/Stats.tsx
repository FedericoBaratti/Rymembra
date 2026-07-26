import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { BarChart3, Music2, Mic2, Tag } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import EmptyState from '@/components/ui/EmptyState'
import CoverImage from '@/components/ui/CoverImage'
import { usePlayerStore } from '@/store/usePlayerStore'
import { coverUrl, formatLongDuration } from '@/lib/format'
import type { ListeningStats, Track } from '@shared/types'

const PERIODS: { days: number; key: string }[] = [
  { days: 7, key: 'stats.period_7' },
  { days: 30, key: 'stats.period_30' },
  { days: 365, key: 'stats.period_365' },
  { days: 0, key: 'stats.period_all' }
]

function RankRow({ rank, title, subtitle, plays, cover, onClick }: {
  rank: number
  title: string
  subtitle: string
  plays: number
  cover?: string | null
  onClick?: () => void
}): React.JSX.Element {
  const { t } = useTranslation()
  return (
    <button
      className="rank-row flex w-full items-center gap-3 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-white/[0.05]"
      onClick={onClick}
      disabled={!onClick}
    >
      <span className="rank-num w-5 shrink-0 text-center text-[13px] font-bold text-text-3">{rank}</span>
      <span className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded bg-surface-3">
        <CoverImage
          src={cover}
          className="h-full w-full object-cover"
          fallback={<Music2 size={14} className="text-text-3" />}
        />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-medium">{title}</span>
        <span className="block truncate text-[11.5px] text-text-3">{subtitle}</span>
      </span>
      <span className="rank-plays shrink-0 text-[11.5px] text-text-3">{t('stats.plays', { count: plays })}</span>
    </button>
  )
}

export default function Stats(): React.JSX.Element {
  const { t } = useTranslation()
  const playTracks = usePlayerStore((s) => s.playTracks)
  const [period, setPeriod] = useState(30)
  const [stats, setStats] = useState<ListeningStats | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let alive = true
    setLoading(true)
    window.aether
      .getListeningStats(period)
      .then((s) => alive && setStats(s))
      .catch(() => alive && setStats(null))
      .finally(() => alive && setLoading(false))
    return () => {
      alive = false
    }
  }, [period])

  const hasData = stats && stats.totals.plays > 0

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader title={t('stats.title')} subtitle={t('stats.subtitle')} />

      <div className="stats-periods flex gap-2 px-[var(--content-x)] pb-3">
        {PERIODS.map((p) => (
          <button
            key={p.days}
            data-active={period === p.days}
            className={`pressable rounded-full px-3.5 text-[12px] font-medium transition-colors max-[639px]:py-2 sm:py-1 ${
              period === p.days ? 'bg-[var(--accent)] text-white' : 'bg-white/[0.06] text-text-2 hover:bg-white/[0.1]'
            }`}
            onClick={() => setPeriod(p.days)}
          >
            {t(p.key)}
          </button>
        ))}
      </div>

      {loading && !stats ? (
        <div className="flex flex-col gap-2 px-6">
          {Array.from({ length: 8 }).map((_, i) => (
            <div key={i} className="skeleton h-10" />
          ))}
        </div>
      ) : !hasData ? (
        <EmptyState icon={BarChart3} title={t('stats.empty_title')} subtitle={t('stats.empty_subtitle')} />
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)]">
          {/* headline totals */}
          <div className="mb-5 grid grid-cols-3 gap-3">
            {[
              { label: t('stats.total_plays'), value: String(stats!.totals.plays) },
              { label: t('stats.unique_tracks'), value: String(stats!.totals.unique_tracks) },
              {
                label: t('stats.time'),
                value: formatLongDuration(Math.round((stats!.totals.ms_played || 0) / 1000))
              }
            ].map((card) => (
              // clamp + truncate: su 360dp ogni cella ha ~100px, "1g 22h 45m"
              // a 18px fissi sborderebbe dalla card.
              <div key={card.label} className="stat-card min-w-0 rounded-xl bg-white/[0.05] p-3 text-center">
                <div className="stat-number tnum truncate text-[clamp(14px,4.2vw,18px)] font-bold">
                  {card.value}
                </div>
                <div className="mt-0.5 truncate text-[11px] text-text-3">{card.label}</div>
              </div>
            ))}
          </div>

          <Section icon={Music2} title={t('stats.top_tracks')}>
            {stats!.topTracks.slice(0, 20).map((tr: Track & { plays: number }, i) => (
              <RankRow
                key={tr.id}
                rank={i + 1}
                title={tr.title}
                subtitle={tr.artist}
                plays={tr.plays}
                cover={coverUrl(tr.cover_art_hash, true)}
                onClick={() => playTracks(stats!.topTracks, i)}
              />
            ))}
          </Section>

          <Section icon={Mic2} title={t('stats.top_artists')}>
            {stats!.topArtists.slice(0, 15).map((ar, i) => (
              <RankRow
                key={ar.name}
                rank={i + 1}
                title={ar.name}
                subtitle={t('stats.n_tracks', { count: ar.tracks })}
                plays={ar.plays}
              />
            ))}
          </Section>

          {stats!.topGenres.length > 0 && (
            <Section icon={Tag} title={t('stats.top_genres')}>
              {stats!.topGenres.slice(0, 12).map((g, i) => (
                <RankRow key={g.name} rank={i + 1} title={g.name} subtitle="" plays={g.plays} />
              ))}
            </Section>
          )}
        </div>
      )}
    </div>
  )
}

function Section({
  icon: Icon,
  title,
  children
}: {
  icon: typeof Music2
  title: string
  children: React.ReactNode
}): React.JSX.Element {
  return (
    <div className="mb-5">
      <h2 className="mb-1.5 flex items-center gap-2 text-[14px] font-bold tracking-tight">
        <Icon size={15} className="text-text-3" />
        {title}
      </h2>
      <div className="flex flex-col">{children}</div>
    </div>
  )
}
