import { useEffect, useRef, useState } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useTranslation } from 'react-i18next'
import {
  ChevronDown,
  ChevronUp,
  Folder,
  ListEnd,
  ListMusic,
  ListStart,
  MicVocal,
  Music2,
  Pencil,
  Play,
  Star
} from 'lucide-react'
import type { Track, TrackQuery } from '@shared/types'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { toast } from '@/store/useToastStore'
import { coverUrl, formatDuration } from '@/lib/format'

// Column templates live in global.css (.track-grid) and respond to the
// "content" container width; tg-album / tg-year cells toggle in lockstep.
const GRID = 'track-grid'

// Keep in sync with --player-clearance (92px player + 14px gap × 2)
const PLAYER_CLEARANCE = 120

interface MenuState {
  x: number
  y: number
  /** transform-origin so the menu grows from the cursor even when clamped */
  origin: string
  track: Track
  index: number
}

function Rating({ track }: { track: Track }): React.JSX.Element {
  const { t } = useTranslation()
  const [hover, setHover] = useState(0)
  const [value, setValue] = useState(track.rating)

  useEffect(() => setValue(track.rating), [track.id, track.rating])

  return (
    <div className="flex" onMouseLeave={() => setHover(0)}>
      {[1, 2, 3, 4, 5].map((i) => (
        <button
          key={i}
          className="p-0.5 opacity-0 transition-[opacity,transform] duration-100 group-hover:opacity-100 focus-visible:opacity-100 hover:scale-125 active:scale-95 data-[filled=true]:opacity-100"
          data-filled={i <= value}
          aria-label={t('library.rate_n', { n: i })}
          onMouseEnter={() => setHover(i)}
          onClick={(e) => {
            e.stopPropagation()
            const next = i === value ? 0 : i
            setValue(next)
            void window.aether.setRating(track.id, next)
          }}
        >
          <Star
            size={12}
            className={
              i <= (hover || value) ? 'fill-[var(--accent)] text-[var(--accent)]' : 'text-text-3'
            }
          />
        </button>
      ))}
    </div>
  )
}

export default function TrackList({
  tracks,
  sortable = false,
  className = ''
}: {
  tracks: Track[]
  sortable?: boolean
  className?: string
}): React.JSX.Element {
  const { t } = useTranslation()
  const parentRef = useRef<HTMLDivElement>(null)
  const playTracks = usePlayerStore((s) => s.playTracks)
  const playNext = usePlayerStore((s) => s.playNext)
  const enqueue = usePlayerStore((s) => s.enqueue)
  const currentTrackId = usePlayerStore((s) => s.currentTrack?.id)
  const isPlaying = usePlayerStore((s) => s.isPlaying)
  const setEditTrackId = useUiStore((s) => s.setEditTrackId)
  const setBatchEditTrackIds = useUiStore((s) => s.setBatchEditTrackIds)
  const setLyricsEditTrackId = useUiStore((s) => s.setLyricsEditTrackId)
  const trackQuery = useLibraryStore((s) => s.trackQuery)
  const setSort = useLibraryStore((s) => s.setSort)
  const playlists = useLibraryStore((s) => s.playlists).filter((p) => !p.is_smart)
  const [menu, setMenu] = useState<MenuState | null>(null)
  const [playlistSub, setPlaylistSub] = useState(false)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const anchorIndex = useRef<number | null>(null)

  const onRowClick = (e: React.MouseEvent, track: Track, index: number): void => {
    if (e.ctrlKey || e.metaKey) {
      setSelected((prev) => {
        const next = new Set(prev)
        if (next.has(track.id)) next.delete(track.id)
        else next.add(track.id)
        return next
      })
      anchorIndex.current = index
    } else if (e.shiftKey && anchorIndex.current != null) {
      const [from, to] = [Math.min(anchorIndex.current, index), Math.max(anchorIndex.current, index)]
      setSelected(new Set(tracks.slice(from, to + 1).map((t) => t.id)))
    } else {
      setSelected(new Set([track.id]))
      anchorIndex.current = index
    }
  }

  useEffect(() => {
    if (selected.size === 0) return
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') setSelected(new Set())
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [selected.size])

  // selection may reference tracks that were removed/filtered out
  useEffect(() => {
    setSelected((prev) => {
      if (prev.size === 0) return prev
      const visible = new Set(tracks.map((t) => t.id))
      const next = new Set([...prev].filter((id) => visible.has(id)))
      return next.size === prev.size ? prev : next
    })
  }, [tracks])

  const virtualizer = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 48,
    overscan: 12,
    paddingEnd: PLAYER_CLEARANCE
  })

  useEffect(() => {
    if (!menu) return
    const close = (): void => {
      setMenu(null)
      setPlaylistSub(false)
    }
    window.addEventListener('click', close)
    window.addEventListener('contextmenu', close)
    return () => {
      window.removeEventListener('click', close)
      window.removeEventListener('contextmenu', close)
    }
  }, [menu])

  const columns: { key: NonNullable<TrackQuery['sortBy']> | null; label: string; cls?: string }[] = [
    { key: null, label: '' },
    { key: 'title', label: t('library.col_title') },
    { key: 'artist', label: t('library.col_artist') },
    { key: 'album', label: t('library.col_album'), cls: 'tg-album' },
    { key: 'year', label: t('library.col_year'), cls: 'tg-year' },
    { key: 'duration', label: t('library.col_duration'), cls: 'text-right' },
    { key: 'rating', label: t('library.col_rating') }
  ]

  return (
    <div className={`flex min-h-0 flex-1 flex-col ${className}`}>
      <div
        className={`${GRID} h-9 shrink-0 border-b text-[11px] font-semibold uppercase tracking-wide text-text-3`}
        style={{ borderColor: 'var(--hairline)' }}
      >
        {columns.map((col, i) => (
          <button
            key={i}
            className={`flex items-center gap-1 truncate text-left transition-colors ${
              sortable && col.key ? 'hover:text-text-1' : 'cursor-default'
            } ${col.cls ?? ''}`}
            onClick={() => sortable && col.key && setSort(col.key)}
          >
            {col.label}
            {sortable &&
              col.key === trackQuery.sortBy &&
              (trackQuery.sortDir === 'asc' ? <ChevronUp size={11} /> : <ChevronDown size={11} />)}
          </button>
        ))}
      </div>

      <div ref={parentRef} className="min-h-0 flex-1 overflow-y-auto">
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map((vi) => {
            const track = tracks[vi.index]
            const isCurrent = track.id === currentTrackId
            const isSelected = selected.has(track.id)
            const thumb = coverUrl(track.cover_art_hash, true)
            return (
              <div
                key={track.id}
                aria-selected={isSelected}
                className={`${GRID} group absolute left-0 top-0 w-full cursor-default rounded-lg text-[13px] transition-colors duration-100 ${
                  isSelected
                    ? 'bg-white/[0.09]'
                    : isCurrent
                      ? 'bg-[var(--accent-soft)]'
                      : 'hover:bg-white/[0.055] hover:shadow-[inset_0_0_0_1px_rgba(255,255,255,0.04)]'
                }`}
                style={{ height: 48, transform: `translateY(${vi.start}px)` }}
                onClick={(e) => onRowClick(e, track, vi.index)}
                onDoubleClick={() => playTracks(tracks, vi.index)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  e.stopPropagation()
                  const clampedX = e.clientX > window.innerWidth - 230
                  const clampedY = e.clientY > window.innerHeight - 320
                  setMenu({
                    x: Math.min(e.clientX, window.innerWidth - 230),
                    y: Math.min(e.clientY, window.innerHeight - 320),
                    origin: `${clampedY ? 'bottom' : 'top'} ${clampedX ? 'right' : 'left'}`,
                    track,
                    index: vi.index
                  })
                  setPlaylistSub(false)
                }}
              >
                <div className="relative h-6 w-6 overflow-hidden rounded bg-surface-3">
                  {thumb ? (
                    <img src={thumb} alt="" className="h-full w-full object-cover" loading="lazy" draggable={false} />
                  ) : (
                    <div className="flex h-full w-full items-center justify-center">
                      <Music2 size={11} className="text-text-3" />
                    </div>
                  )}
                  {isCurrent && (
                    <div className="absolute inset-0 flex items-center justify-center bg-black/45">
                      <span
                        className={`eq-bars ${isPlaying ? '' : 'eq-bars--paused'}`}
                        style={{ height: 10 }}
                      >
                        <span />
                        <span />
                        <span />
                      </span>
                    </div>
                  )}
                </div>
                <div className={`truncate font-medium ${isCurrent ? 'text-[var(--accent)]' : ''}`}>
                  {track.title}
                </div>
                <div className="truncate text-text-2">{track.artist}</div>
                <div className="tg-album truncate text-text-3">{track.album}</div>
                <div className="tg-year tnum text-text-3">{track.year ?? ''}</div>
                <div className="tnum text-right text-text-3">{formatDuration(track.duration)}</div>
                <Rating track={track} />
              </div>
            )
          })}
        </div>
      </div>

      {menu &&
        (() => {
          const isMulti = selected.size > 1 && selected.has(menu.track.id)
          const selTracks = isMulti ? tracks.filter((tr) => selected.has(tr.id)) : [menu.track]
          return (
            <div
              className="glass-modal menu-pop fixed z-50 w-[210px] rounded-xl p-1.5"
              style={
                {
                  left: menu.x,
                  top: menu.y,
                  boxShadow: 'var(--shadow-2)',
                  '--menu-origin': menu.origin
                } as React.CSSProperties
              }
              onClick={(e) => e.stopPropagation()}
            >
              {[
                {
                  label: t('player.play'),
                  icon: Play,
                  fn: () => (isMulti ? playTracks(selTracks, 0) : playTracks(tracks, menu.index))
                },
                { label: t('player.play_next'), icon: ListStart, fn: () => playNext(selTracks) },
                { label: t('player.add_to_queue'), icon: ListEnd, fn: () => enqueue(selTracks) }
              ].map((item) => (
                <button
                  key={item.label}
                  className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-white/[0.07]"
                  onClick={() => {
                    item.fn()
                    setMenu(null)
                  }}
                >
                  <item.icon size={14} className="shrink-0 text-text-3" />
                  {item.label}
                </button>
              ))}

              <div className="relative">
                <button
                  className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-white/[0.07]"
                  onClick={() => setPlaylistSub(!playlistSub)}
                >
                  <ListMusic size={14} className="shrink-0 text-text-3" />
                  {t('playlists.title')} ›
                </button>
                {playlistSub && (
                  <div
                    className="glass-modal menu-pop absolute left-full top-0 z-50 ml-1 max-h-48 w-44 overflow-y-auto rounded-xl p-1.5"
                    style={{ boxShadow: 'var(--shadow-2)' }}
                  >
                    {playlists.length === 0 && (
                      <div className="px-2.5 py-1.5 text-[12px] text-text-3">—</div>
                    )}
                    {playlists.map((p) => (
                      <button
                        key={p.id}
                        className="block w-full truncate rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-white/[0.07]"
                        onClick={() => {
                          void window.aether
                            .addToPlaylist(p.id, selTracks.map((tr) => tr.id))
                            .then(() => {
                              toast.success(
                                t('toast.added_to_playlist', { count: selTracks.length, name: p.name })
                              )
                            })
                          setMenu(null)
                        }}
                      >
                        {p.name}
                      </button>
                    ))}
                  </div>
                )}
              </div>

              <div className="mx-2 my-1 h-px" style={{ background: 'var(--hairline)' }} />

              {isMulti ? (
                <button
                  className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-white/[0.07]"
                  onClick={() => {
                    setBatchEditTrackIds(selTracks.map((tr) => tr.id))
                    setMenu(null)
                  }}
                >
                  <Pencil size={14} className="shrink-0 text-text-3" />
                  {t('metadata.edit_n', { count: selTracks.length })}
                </button>
              ) : (
                <>
                  <button
                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-white/[0.07]"
                    onClick={() => {
                      setEditTrackId(menu.track.id)
                      setMenu(null)
                    }}
                  >
                    <Pencil size={14} className="shrink-0 text-text-3" />
                    {t('metadata.edit')}
                  </button>
                  <button
                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-white/[0.07]"
                    onClick={() => {
                      setLyricsEditTrackId(menu.track.id)
                      setMenu(null)
                    }}
                  >
                    <MicVocal size={14} className="shrink-0 text-text-3" />
                    {t('lyrics_editor.title')}
                  </button>
                  <button
                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-white/[0.07]"
                    onClick={() => {
                      void window.aether.showInFolder(menu.track.id)
                      setMenu(null)
                    }}
                  >
                    <Folder size={14} className="shrink-0 text-text-3" />
                    {t('metadata.show_in_folder')}
                  </button>
                </>
              )}
            </div>
          )
        })()}
    </div>
  )
}
