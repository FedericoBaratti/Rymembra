import { memo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Disc3, Play } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import EmptyState from '@/components/ui/EmptyState'
import MediaGrid from '@/components/ui/MediaGrid'
import { GridSkeleton } from '@/components/ui/Skeletons'
import { useLibraryStore } from '@/store/useLibraryStore'
import { usePlayerStore } from '@/store/usePlayerStore'
import { coverUrl } from '@/lib/format'
import type { Album } from '@shared/types'

export const AlbumCard = memo(function AlbumCard({ album }: { album: Album }): React.JSX.Element {
  const navigate = useNavigate()
  const playTracks = usePlayerStore((s) => s.playTracks)
  const cover = coverUrl(album.cover_art_hash)

  const playAlbum = async (e: React.MouseEvent): Promise<void> => {
    e.stopPropagation()
    const tracks = await window.aether.getAlbumTracks(album.id)
    if (tracks.length > 0) playTracks(tracks, 0)
  }

  return (
    <button
      className="card-lift group fade-in flex w-full flex-col gap-2 rounded-xl p-3 text-left hover:bg-white/[0.05]"
      onClick={() => navigate(`/albums/${album.id}`)}
    >
      <div className="relative aspect-square w-full overflow-hidden rounded-lg bg-surface-3 shadow-lg">
        {cover ? (
          <img
            src={cover}
            alt=""
            loading="lazy"
            draggable={false}
            className="h-full w-full object-cover"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center">
            <Disc3 size={36} className="text-text-3" />
          </div>
        )}
        <div className="absolute inset-0 flex items-end justify-end bg-gradient-to-t from-black/50 to-transparent p-3 opacity-0 transition-opacity duration-200 group-hover:opacity-100">
          <span
            className="flex h-11 w-11 translate-y-1 items-center justify-center rounded-full text-white shadow-xl transition-transform duration-200 hover:scale-105 group-hover:translate-y-0"
            style={{ background: 'var(--accent)', boxShadow: '0 4px 20px var(--accent-glow)' }}
            onClick={(e) => void playAlbum(e)}
          >
            <Play size={18} fill="currentColor" className="ml-0.5" />
          </span>
        </div>
      </div>
      <div>
        <div className="truncate text-[13.5px] font-semibold">{album.title}</div>
        <div className="truncate text-[12px] text-text-3">
          {album.artist}
          {album.year ? ` · ${album.year}` : ''}
        </div>
      </div>
    </button>
  )
})

export default function Albums(): React.JSX.Element {
  const { t } = useTranslation()
  const albums = useLibraryStore((s) => s.albums)
  const loaded = useLibraryStore((s) => s.loaded)

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader title={t('albums.title')} subtitle={t('albums.albums_count', { count: albums.length })} />
      {!loaded ? (
        <GridSkeleton />
      ) : albums.length === 0 ? (
        <EmptyState icon={Disc3} title={t('library.empty_title')} subtitle={t('library.empty_subtitle')} />
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)]">
          <MediaGrid>
            {albums.map((album) => (
              <AlbumCard key={album.id} album={album} />
            ))}
          </MediaGrid>
        </div>
      )}
    </div>
  )
}
