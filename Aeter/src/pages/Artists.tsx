import { memo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Users, User } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import EmptyState from '@/components/ui/EmptyState'
import MediaGrid from '@/components/ui/MediaGrid'
import { GridSkeleton } from '@/components/ui/Skeletons'
import { useLibraryStore } from '@/store/useLibraryStore'
import { coverUrl } from '@/lib/format'
import type { Artist } from '@shared/types'

const ArtistCard = memo(function ArtistCard({ artist }: { artist: Artist }): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const img = coverUrl(artist.image_hash)
  return (
    <button
      className="card-lift group fade-in flex w-full flex-col items-center gap-3 rounded-xl p-4 hover:bg-white/[0.05]"
      onClick={() => navigate(`/artists/${encodeURIComponent(artist.name)}`)}
    >
      <div className="aspect-square w-full max-w-[150px] overflow-hidden rounded-full bg-surface-3 shadow-lg transition-shadow duration-200 group-hover:shadow-[0_0_24px_var(--accent-soft)]">
        {img ? (
          <img src={img} alt="" loading="lazy" className="h-full w-full object-cover" />
        ) : (
          <div className="flex h-full w-full items-center justify-center">
            <User size={36} className="text-text-3" />
          </div>
        )}
      </div>
      <div className="w-full text-center">
        <div className="truncate text-[13.5px] font-semibold">{artist.name}</div>
        <div className="truncate text-[11.5px] text-text-3">
          {t('albums.albums_count', { count: artist.album_count })} ·{' '}
          {t('library.tracks_count', { count: artist.track_count })}
        </div>
      </div>
    </button>
  )
})

export default function Artists(): React.JSX.Element {
  const { t } = useTranslation()
  const artists = useLibraryStore((s) => s.artists)
  const loaded = useLibraryStore((s) => s.loaded)
  const visible = artists.filter((a) => a.track_count > 0)

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title={t('artists.title')}
        subtitle={t('artists.artists_count', { count: visible.length })}
      />
      {!loaded ? (
        <GridSkeleton dense />
      ) : visible.length === 0 ? (
        <EmptyState icon={Users} title={t('library.empty_title')} subtitle={t('library.empty_subtitle')} />
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)]">
          <MediaGrid dense>
            {visible.map((artist) => (
              <ArtistCard key={artist.id} artist={artist} />
            ))}
          </MediaGrid>
        </div>
      )}
    </div>
  )
}
