import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import {
  Play,
  Trash2,
  GripVertical,
  X,
  Music2,
  Sparkles,
  Pencil,
  ListMusic,
  Plus,
  Check,
  ListChecks
} from 'lucide-react'
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent
} from '@dnd-kit/core'
import {
  SortableContext,
  verticalListSortingStrategy,
  useSortable,
  arrayMove
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { Track } from '@shared/types'
import { useLibraryStore } from '@/store/useLibraryStore'
import { usePlayerStore } from '@/store/usePlayerStore'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
// Editors mount-on-open: separate chunks, loaded on first open.
const SmartPlaylistEditor = lazy(() => import('@/components/library/SmartPlaylistEditor'))
const AddTracksPicker = lazy(() => import('@/components/library/AddTracksPicker'))
import ConfirmDialog from '@/components/ui/ConfirmDialog'
import EmptyState from '@/components/ui/EmptyState'
import { ListSkeleton } from '@/components/ui/Skeletons'
import Hero from '@/components/ui/Hero'
import CoverImage from '@/components/ui/CoverImage'
import { PlaylistCover } from './Playlists'
import { coverUrl, formatDuration, formatLongDuration } from '@/lib/format'

/** Shared cover thumbnail used by every row variant. */
function Thumb({ hash }: { hash: string | null }): React.JSX.Element {
  const thumb = coverUrl(hash, true)
  return (
    <div className="h-8 w-8 shrink-0 overflow-hidden rounded bg-surface-3">
      <CoverImage
        src={thumb}
        className="h-full w-full object-cover"
        fallback={
          <div className="flex h-full w-full items-center justify-center">
            <Music2 size={12} className="text-text-3" />
          </div>
        }
      />
    </div>
  )
}

const ROW_CLS = 'flex items-center gap-2.5 rounded-lg px-2 py-1.5 text-[13px] transition-colors'

function Row({
  track,
  index,
  onPlay,
  onRemove
}: {
  track: Track
  index: number
  onPlay: () => void
  onRemove: () => void
}): React.JSX.Element {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: `${index}-${track.id}`
  })
  const currentId = usePlayerStore((s) => s.currentTrack?.id)
  const isCurrent = currentId === track.id
  const { t } = useTranslation()

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition, opacity: isDragging ? 0.6 : 1 }}
      className={`group ${ROW_CLS} ${isCurrent ? 'bg-[var(--accent-soft)]' : 'hover:bg-white/[0.045]'}`}
      onDoubleClick={onPlay}
    >
      <button
        className="cursor-grab text-text-3 opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
        aria-label={`${track.title} — ${track.artist}`}
        {...attributes}
        {...listeners}
      >
        <GripVertical size={14} />
      </button>
      <span className="tnum w-6 text-right text-[12px] text-text-3">{index + 1}</span>
      <Thumb hash={track.cover_art_hash} />
      <div className="min-w-0 flex-1">
        <div className={`truncate font-medium ${isCurrent ? 'text-[var(--accent)]' : ''}`}>
          {track.title}
        </div>
        <div className="truncate text-[11.5px] text-text-3">
          {track.artist} — {track.album}
        </div>
      </div>
      <span className="tnum text-[11.5px] text-text-3">{formatDuration(track.duration)}</span>
      <button
        className="icon-btn h-6 w-6 opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
        onClick={onRemove}
        aria-label={t('playlists.remove_track', { title: track.title })}
      >
        <X size={13} />
      </button>
    </div>
  )
}

function StaticRow({
  track,
  index,
  onPlay
}: {
  track: Track
  index: number
  onPlay: () => void
}): React.JSX.Element {
  const currentId = usePlayerStore((s) => s.currentTrack?.id)
  const isCurrent = currentId === track.id

  return (
    <div
      className={`group ${ROW_CLS} ${isCurrent ? 'bg-[var(--accent-soft)]' : 'hover:bg-white/[0.045]'}`}
      onDoubleClick={onPlay}
    >
      <span className="tnum w-6 text-right text-[12px] text-text-3">{index + 1}</span>
      <Thumb hash={track.cover_art_hash} />
      <div className="min-w-0 flex-1">
        <div className={`truncate font-medium ${isCurrent ? 'text-[var(--accent)]' : ''}`}>
          {track.title}
        </div>
        <div className="truncate text-[11.5px] text-text-3">
          {track.artist} — {track.album}
        </div>
      </div>
      <span className="tnum text-[11.5px] text-text-3">{formatDuration(track.duration)}</span>
    </div>
  )
}

function SelectRow({
  track,
  index,
  selected,
  onToggle
}: {
  track: Track
  index: number
  selected: boolean
  onToggle: () => void
}): React.JSX.Element {
  return (
    <button
      className={`w-full text-left ${ROW_CLS} ${selected ? 'bg-[var(--accent-soft)]' : 'hover:bg-white/[0.045]'}`}
      onClick={onToggle}
    >
      <span
        className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-md border ${
          selected ? 'border-[var(--accent)] bg-[var(--accent)] text-white' : 'border-white/25'
        }`}
      >
        {selected && <Check size={13} />}
      </span>
      <span className="tnum w-5 text-right text-[12px] text-text-3">{index + 1}</span>
      <Thumb hash={track.cover_art_hash} />
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{track.title}</div>
        <div className="truncate text-[11.5px] text-text-3">
          {track.artist} — {track.album}
        </div>
      </div>
      <span className="tnum text-[11.5px] text-text-3">{formatDuration(track.duration)}</span>
    </button>
  )
}

export default function PlaylistDetail(): React.JSX.Element {
  const { id } = useParams()
  const navigate = useNavigate()
  const { t } = useTranslation()
  const playlists = useLibraryStore((s) => s.playlists)
  const libraryLoaded = useLibraryStore((s) => s.loaded)
  const refreshPlaylists = useLibraryStore((s) => s.refreshPlaylists)
  const playTracks = usePlayerStore((s) => s.playTracks)
  const [tracks, setTracks] = useState<Track[]>([])
  const [loading, setLoading] = useState(true)
  const [rulesOpen, setRulesOpen] = useState(false)
  const [pickerOpen, setPickerOpen] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [selectMode, setSelectMode] = useState(false)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const scrollRef = useRef<HTMLDivElement>(null)
  const playlistId = Number(id)
  const playlist = playlists.find((p) => p.id === playlistId)
  const isSmart = playlist?.is_smart === 1

  const reload = useCallback(async (): Promise<void> => {
    try {
      setTracks(await window.aether.getPlaylistTracks(playlistId))
    } catch (err) {
      // Backend reject (booting / dead): keep whatever is shown; the loading
      // flag still clears so the page never hangs on skeletons.
      console.error('[playlist] load failed', err)
    } finally {
      setLoading(false)
    }
  }, [playlistId])

  useEffect(() => {
    setLoading(true)
    void reload()
  }, [reload])

  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }))

  const sortableIds = useMemo(() => tracks.map((tr, i) => `${i}-${tr.id}`), [tracks])

  // Virtualized rows: a multi-thousand-track playlist used to mount one
  // dnd-kit sortable per row — seconds of freeze on open. Only the visible
  // window mounts; SortableContext still gets the full id list.
  const virtualizer = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 44,
    overscan: 10
  })

  const renderVirtual = (renderRow: (track: Track, i: number) => React.JSX.Element): React.JSX.Element => (
    <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
      {virtualizer.getVirtualItems().map((vi) => {
        const track = tracks[vi.index]
        if (!track) return null
        return (
          <div
            key={`${vi.index}-${track.id}`}
            data-index={vi.index}
            ref={virtualizer.measureElement}
            style={{
              position: 'absolute',
              top: 0,
              left: 0,
              width: '100%',
              transform: `translateY(${vi.start}px)`
            }}
          >
            {renderRow(track, vi.index)}
          </div>
        )
      })}
    </div>
  )

  const onDragEnd = (e: DragEndEvent): void => {
    const { active, over } = e
    if (!over || active.id === over.id) return
    // Ids are `${index}-${trackId}` — parse the leading index, no O(N) scans.
    const from = parseInt(String(active.id), 10)
    const to = parseInt(String(over.id), 10)
    if (!Number.isInteger(from) || !Number.isInteger(to) || from < 0 || to < 0) return
    const next = arrayMove(tracks, from, to)
    setTracks(next)
    void window.aether
      .reorderPlaylist(playlistId, next.map((tr) => tr.id))
      .then(() => refreshPlaylists())
      .catch(() => void reload())
  }

  const removeAt = (index: number): void => {
    const next = tracks.filter((_, i) => i !== index)
    setTracks(next)
    void window.aether
      .removeFromPlaylist(playlistId, [index])
      .then(() => refreshPlaylists())
      .catch(() => void reload())
  }

  const doDeletePlaylist = async (): Promise<void> => {
    try {
      await window.aether.deletePlaylist(playlistId)
      void refreshPlaylists()
      navigate('/playlists')
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    }
  }

  const toggleSelect = (index: number): void => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
  }

  const exitSelect = (): void => {
    setSelectMode(false)
    setSelected(new Set())
  }

  const removeSelected = (): void => {
    if (selected.size === 0) return
    const indices = [...selected]
    const next = tracks.filter((_, i) => !selected.has(i))
    setTracks(next)
    void window.aether
      .removeFromPlaylist(playlistId, indices)
      .then(() => refreshPlaylists())
      .catch(() => void reload())
    exitSelect()
  }

  const total = tracks.reduce((s, tr) => s + tr.duration, 0)
  const firstCoverHash = playlist?.cover_hashes[0] ?? null

  // Deep link to a playlist that no longer exists: a clear not-found state
  // instead of a blank hero over an empty list.
  if (libraryLoaded && !loading && !playlist && tracks.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <EmptyState
          icon={ListMusic}
          title={t('playlists.not_found')}
          action={
            <button
              className="btn-accent mt-1 rounded-lg px-4 py-2 text-[13px]"
              onClick={() => navigate('/playlists')}
            >
              {t('common.back')}
            </button>
          }
        />
      </div>
    )
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Hero
        image={coverUrl(firstCoverHash)}
        artwork={playlist ? <PlaylistCover playlist={playlist} /> : undefined}
        fallbackIcon={ListMusic}
        eyebrow={isSmart ? t('smart.smart_badge') : t('playlists.title')}
        title={playlist?.name ?? ''}
        paletteHash={firstCoverHash}
        meta={
          <span className="flex items-center gap-2">
            {isSmart && <Sparkles size={15} className="shrink-0 text-[var(--accent)]" />}
            {t('playlists.tracks_count', { count: tracks.length })} · {formatLongDuration(total)}
          </span>
        }
        actions={
          <>
            <button
              className="flex items-center gap-2 rounded-full px-5 py-2 text-[13px] font-semibold text-white transition-[transform,filter] hover:scale-[1.03] hover:brightness-110 active:scale-95 disabled:opacity-40"
              style={{ background: 'var(--accent)', boxShadow: '0 4px 16px var(--accent-glow)' }}
              onClick={() => playTracks(tracks, 0)}
              disabled={tracks.length === 0}
            >
              <Play size={15} fill="currentColor" /> {t('player.play')}
            </button>
            {!isSmart && (
              <button
                className="btn-ghost flex items-center gap-2 rounded-full px-3.5 py-2 text-[13px] font-medium"
                onClick={() => setPickerOpen(true)}
              >
                <Plus size={15} /> {t('playlists.add_tracks')}
              </button>
            )}
            {!isSmart && tracks.length > 0 && (
              <button
                className="icon-btn h-9 w-9 rounded-full bg-white/[0.06]"
                onClick={() => setSelectMode(true)}
                title={t('playlists.select')}
                aria-label={t('playlists.select')}
              >
                <ListChecks size={16} />
              </button>
            )}
            {isSmart && (
              <button
                className="btn-ghost flex items-center gap-2 rounded-full px-3.5 py-2 text-[13px] font-medium"
                onClick={() => setRulesOpen(true)}
              >
                <Pencil size={14} /> {t('smart.edit_rules')}
              </button>
            )}
            <button
              className="icon-btn h-9 w-9 rounded-full bg-white/[0.06]"
              onClick={() => setConfirmOpen(true)}
              title={t('playlists.delete')}
              aria-label={t('playlists.delete')}
            >
              <Trash2 size={16} />
            </button>
          </>
        }
      />

      {/* Selection bar lives ABOVE the scroll container: always visible, no
          sticky backdrop-filter re-blurring every scrolled frame. */}
      {selectMode && (
        <div
          className="glass-chrome flex shrink-0 items-center gap-2 px-4 py-2"
          style={{ borderColor: 'var(--hairline)' }}
        >
          <button className="icon-btn h-8 w-8" onClick={exitSelect} aria-label={t('common.cancel')}>
            <X size={16} />
          </button>
          <span className="text-[13px] font-medium">
            {t('playlists.selected_count', { count: selected.size })}
          </span>
          <div className="ml-auto flex items-center gap-2">
            <button
              className="btn-ghost rounded-full px-3 py-1.5 text-[12px] font-medium"
              onClick={() =>
                setSelected(
                  selected.size === tracks.length ? new Set() : new Set(tracks.map((_, i) => i))
                )
              }
            >
              {selected.size === tracks.length ? t('playlists.deselect_all') : t('playlists.select_all')}
            </button>
            <button
              className="flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12px] font-semibold text-white disabled:opacity-40"
              style={{ background: 'var(--danger)' }}
              onClick={removeSelected}
              disabled={selected.size === 0}
            >
              <Trash2 size={13} /> {t('playlists.remove')}
            </button>
          </div>
        </div>
      )}

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto px-4 pb-[var(--player-clearance)] pt-2">
        {loading && tracks.length === 0 ? (
          <ListSkeleton count={8} />
        ) : isSmart ? (
          renderVirtual((track, i) => (
            <StaticRow track={track} index={i} onPlay={() => playTracks(tracks, i)} />
          ))
        ) : selectMode ? (
          renderVirtual((track, i) => (
            <SelectRow
              track={track}
              index={i}
              selected={selected.has(i)}
              onToggle={() => toggleSelect(i)}
            />
          ))
        ) : (
          <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
            <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
              {renderVirtual((track, i) => (
                <Row
                  track={track}
                  index={i}
                  onPlay={() => playTracks(tracks, i)}
                  onRemove={() => removeAt(i)}
                />
              ))}
            </SortableContext>
          </DndContext>
        )}
      </div>

      <Suspense fallback={null}>
        {rulesOpen && playlist && (
          <SmartPlaylistEditor
            playlist={playlist}
            onSaved={() => {
              void refreshPlaylists()
              void reload()
            }}
            onClose={() => setRulesOpen(false)}
          />
        )}
        {pickerOpen && (
          <AddTracksPicker
            playlistId={playlistId}
            onAdded={() => {
              void refreshPlaylists()
              void reload()
            }}
            onClose={() => setPickerOpen(false)}
          />
        )}
      </Suspense>
      <ConfirmDialog
        open={confirmOpen}
        title={t('playlists.delete')}
        message={playlist ? t('playlists.delete_confirm', { name: playlist.name }) : undefined}
        confirmLabel={t('playlists.delete')}
        danger
        onConfirm={() => void doDeletePlaylist()}
        onClose={() => setConfirmOpen(false)}
      />
    </div>
  )
}
