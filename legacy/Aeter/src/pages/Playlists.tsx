import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { ListMusic, Plus, Music2, Sparkles } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import EmptyState from '@/components/ui/EmptyState'
import MediaGrid from '@/components/ui/MediaGrid'
import { GridSkeleton } from '@/components/ui/Skeletons'
import SmartPlaylistEditor from '@/components/library/SmartPlaylistEditor'
import { useLibraryStore } from '@/store/useLibraryStore'
import { toast } from '@/store/useToastStore'
import { coverUrl, formatLongDuration } from '@/lib/format'
import type { Playlist } from '@shared/types'

export function PlaylistCover({ playlist }: { playlist: Playlist }): React.JSX.Element {
  const hashes = playlist.cover_hashes
  if (hashes.length === 0) {
    return (
      <div className="flex h-full w-full items-center justify-center bg-surface-3">
        <Music2 size={32} className="text-text-3" />
      </div>
    )
  }
  if (hashes.length < 4) {
    return <img src={coverUrl(hashes[0])!} alt="" className="h-full w-full object-cover" />
  }
  return (
    <div className="grid h-full w-full grid-cols-2 grid-rows-2">
      {hashes.slice(0, 4).map((h, i) => (
        <img key={i} src={coverUrl(h)!} alt="" className="h-full w-full object-cover" />
      ))}
    </div>
  )
}

export default function Playlists(): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const playlists = useLibraryStore((s) => s.playlists)
  const loaded = useLibraryStore((s) => s.loaded)
  const refreshPlaylists = useLibraryStore((s) => s.refreshPlaylists)
  const [creating, setCreating] = useState(false)
  const [smartOpen, setSmartOpen] = useState(false)
  const [name, setName] = useState('')

  const create = async (): Promise<void> => {
    if (!name.trim()) return
    await window.aether.createPlaylist(name.trim())
    toast.success(t('toast.playlist_created'), name.trim())
    setName('')
    setCreating(false)
    void refreshPlaylists()
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title={t('playlists.title')}
        subtitle={t('playlists.tracks_count', { count: playlists.length })}
        actions={
          creating ? (
            <div className="flex items-center gap-2">
              <input
                autoFocus
                className="field-input h-9 w-48 rounded-full"
                placeholder={t('playlists.name_placeholder')}
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') void create()
                  if (e.key === 'Escape') setCreating(false)
                }}
              />
              <button
                className="btn-accent rounded-lg px-3 py-2 text-[13px]"
                onClick={() => void create()}
              >
                {t('playlists.create')}
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-2">
              <button
                className="btn-ghost flex items-center gap-2 rounded-lg px-3.5 py-2 text-[13px] font-medium"
                onClick={() => setCreating(true)}
              >
                <Plus size={15} /> {t('playlists.new')}
              </button>
              <button
                className="btn-ghost flex items-center gap-2 rounded-lg px-3.5 py-2 text-[13px] font-medium"
                onClick={() => setSmartOpen(true)}
              >
                <Sparkles size={15} /> {t('smart.new_smart')}
              </button>
            </div>
          )
        }
      />
      {!loaded ? (
        <GridSkeleton />
      ) : playlists.length === 0 ? (
        <EmptyState
          icon={ListMusic}
          title={t('playlists.empty')}
          action={
            <button
              className="btn-accent flex items-center gap-2 rounded-full px-4 py-2 text-[13px]"
              onClick={() => setCreating(true)}
            >
              <Plus size={14} /> {t('playlists.new')}
            </button>
          }
        />
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)]">
          <MediaGrid>
            {playlists.map((p) => (
              <button
                key={p.id}
                className="card-lift group fade-in flex w-full flex-col gap-2 rounded-xl p-3 text-left hover:bg-white/[0.05]"
                onClick={() => navigate(`/playlists/${p.id}`)}
              >
                <div className="relative aspect-square w-full overflow-hidden rounded-lg shadow-lg">
                  <PlaylistCover playlist={p} />
                  {p.is_smart === 1 && (
                    <span
                      className="absolute right-2 top-2 flex h-6 w-6 items-center justify-center rounded-full text-white"
                      style={{ background: 'var(--accent)', boxShadow: '0 2px 8px rgba(0,0,0,0.4)' }}
                      title={t('smart.smart_badge')}
                    >
                      <Sparkles size={12} />
                    </span>
                  )}
                </div>
                <div>
                  <div className="truncate text-[13.5px] font-semibold">{p.name}</div>
                  <div className="truncate text-[12px] text-text-3">
                    {t('playlists.tracks_count', { count: p.track_count })} ·{' '}
                    {formatLongDuration(p.total_duration)}
                  </div>
                </div>
              </button>
            ))}
          </MediaGrid>
        </div>
      )}
      {smartOpen && (
        <SmartPlaylistEditor
          onSaved={() => void refreshPlaylists()}
          onClose={() => setSmartOpen(false)}
        />
      )}
    </div>
  )
}
