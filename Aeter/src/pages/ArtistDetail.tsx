import { useEffect, useState } from 'react'
import { useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Play, User } from 'lucide-react'
import type { Album, Track } from '@shared/types'
import { AlbumCard } from './Albums'
import TrackList from '@/components/library/TrackList'
import Hero from '@/components/ui/Hero'
import { useLibraryStore } from '@/store/useLibraryStore'
import { usePlayerStore } from '@/store/usePlayerStore'
import { coverUrl } from '@/lib/format'

export default function ArtistDetail(): React.JSX.Element {
  const { name } = useParams()
  const { t } = useTranslation()
  const playTracks = usePlayerStore((s) => s.playTracks)
  const artists = useLibraryStore((s) => s.artists)
  const [albums, setAlbums] = useState<Album[]>([])
  const [tracks, setTracks] = useState<Track[]>([])
  const artistName = name ? decodeURIComponent(name) : ''
  const artist = artists.find((a) => a.name === artistName)
  const image = coverUrl(artist?.image_hash)

  useEffect(() => {
    if (!artistName) return
    void window.aether.getArtistAlbums(artistName).then(setAlbums).catch(() => setAlbums([]))
    void window.aether
      .getTracks({ artistName, sortBy: 'album', sortDir: 'asc' })
      .then(setTracks)
      .catch(() => setTracks([]))
  }, [artistName])

  return (
    // Non-scrolling column: TrackList virtualizes on its OWN scroll container,
    // so a scrollable page here would nest two competing scroll areas.
    <div className="flex min-h-0 flex-1 flex-col">
      <Hero
        image={image}
        fallbackIcon={User}
        shape="circle"
        eyebrow={t('artists.title')}
        title={artistName}
        paletteHash={artist?.image_hash}
        meta={
          <>
            {t('albums.albums_count', { count: albums.length })} ·{' '}
            {t('library.tracks_count', { count: tracks.length })}
          </>
        }
        actions={
          <button
            className="flex items-center gap-2 rounded-full px-5 py-2 text-[13px] font-semibold text-white transition-all hover:scale-[1.03] hover:brightness-110 active:scale-95 disabled:opacity-40"
            style={{ background: 'var(--accent)', boxShadow: '0 4px 16px var(--accent-glow)' }}
            onClick={() => playTracks(tracks, 0)}
            disabled={tracks.length === 0}
          >
            <Play size={15} fill="currentColor" /> {t('player.play')}
          </button>
        }
      />

      {albums.length > 0 && (
        <div className="no-scrollbar shrink-0 snap-x snap-mandatory overflow-x-auto px-[var(--content-x)] pt-2">
          <div className="flex w-max gap-2">
            {albums.map((album) => (
              <div key={album.id} className="w-[180px] shrink-0 snap-start">
                <AlbumCard album={album} />
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col px-3 pb-4 pt-2">
        <TrackList tracks={tracks} />
      </div>
    </div>
  )
}
