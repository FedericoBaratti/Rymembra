import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Disc3, Play, Shuffle } from 'lucide-react'
import type { Track } from '@shared/types'
import TrackList from '@/components/library/TrackList'
import Hero from '@/components/ui/Hero'
import EmptyState from '@/components/ui/EmptyState'
import { ListSkeleton } from '@/components/ui/Skeletons'
import { useLibraryStore } from '@/store/useLibraryStore'
import { usePlayerStore } from '@/store/usePlayerStore'
import { coverUrl, formatLongDuration } from '@/lib/format'

export default function AlbumDetail(): React.JSX.Element {
  const { id } = useParams()
  const navigate = useNavigate()
  const { t } = useTranslation()
  const albums = useLibraryStore((s) => s.albums)
  const libraryLoaded = useLibraryStore((s) => s.loaded)
  const playTracks = usePlayerStore((s) => s.playTracks)
  const toggleShuffle = usePlayerStore((s) => s.toggleShuffle)
  const shuffle = usePlayerStore((s) => s.shuffle)
  const [tracks, setTracks] = useState<Track[]>([])
  const [loading, setLoading] = useState(true)

  const album = albums.find((a) => a.id === Number(id))

  useEffect(() => {
    if (!id) return
    let alive = true
    setLoading(true)
    void window.aether
      .getAlbumTracks(Number(id))
      .then((rows) => {
        if (alive) setTracks(rows)
      })
      .catch(() => {
        // Backend reject (booting / dead): fall through to the empty state
        // instead of leaving the page half-rendered forever.
        if (alive) setTracks([])
      })
      .finally(() => {
        if (alive) setLoading(false)
      })
    return () => {
      alive = false
    }
  }, [id])

  // Deep link to an id that no longer exists (or never did): show a clear
  // not-found state instead of a blank hero with an empty list.
  if (libraryLoaded && !loading && !album && tracks.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <EmptyState
          icon={Disc3}
          title={t('albums.not_found')}
          action={
            <button
              className="btn-accent mt-1 rounded-lg px-4 py-2 text-[13px]"
              onClick={() => navigate('/albums')}
            >
              {t('common.back')}
            </button>
          }
        />
      </div>
    )
  }

  const cover = coverUrl(album?.cover_art_hash)
  const totalDuration = tracks.reduce((sum, tr) => sum + tr.duration, 0)

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Hero
        image={cover}
        fallbackIcon={Disc3}
        eyebrow={t('albums.title')}
        title={album?.title ?? ''}
        paletteHash={album?.cover_art_hash}
        meta={
          <>
            <button
              className="font-medium hover:text-[var(--accent)] hover:underline"
              onClick={() => album && navigate(`/artists/${encodeURIComponent(album.artist)}`)}
            >
              {album?.artist}
            </button>
            {album?.year ? ` · ${album.year}` : ''} ·{' '}
            {t('library.tracks_count', { count: tracks.length })} ·{' '}
            {formatLongDuration(totalDuration)}
          </>
        }
        actions={
          <>
            <button
              className="flex items-center gap-2 rounded-full px-5 py-2 text-[13px] font-semibold text-white transition-all hover:scale-[1.03] hover:brightness-110 active:scale-95 disabled:opacity-40"
              style={{ background: 'var(--accent)', boxShadow: '0 4px 16px var(--accent-glow)' }}
              onClick={() => playTracks(tracks, 0)}
              disabled={tracks.length === 0}
            >
              <Play size={15} fill="currentColor" /> {t('player.play')}
            </button>
            <button
              className="icon-btn h-9 w-9 rounded-full bg-white/[0.06]"
              data-active={shuffle}
              onClick={() => {
                if (!shuffle) toggleShuffle()
                playTracks(tracks, Math.floor(Math.random() * Math.max(1, tracks.length)))
              }}
              title={t('player.shuffle')}
            >
              <Shuffle size={15} />
            </button>
          </>
        }
      />
      {loading && tracks.length === 0 ? (
        <div className="px-3 pt-2">
          <ListSkeleton count={8} />
        </div>
      ) : (
        <TrackList tracks={tracks} className="px-3 pt-2" />
      )}
    </div>
  )
}
