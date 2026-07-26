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
  ListChecks,
  ListStart,
  ListEnd,
  MoreHorizontal
} from 'lucide-react'
import {
  DndContext,
  closestCenter,
  MouseSensor,
  TouchSensor,
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
// Editor mount-on-open: chunk separati, caricati alla prima apertura.
const SmartPlaylistEditor = lazy(() => import('@/components/library/SmartPlaylistEditor'))
const AddTracksPicker = lazy(() => import('@/components/library/AddTracksPicker'))
import ConfirmDialog from '@/components/ui/ConfirmDialog'
import BottomSheet from '@/components/ui/BottomSheet'
import EmptyState from '@/components/ui/EmptyState'
import { ListSkeleton } from '@/components/ui/Skeletons'
import Hero from '@/components/ui/Hero'
import CoverImage from '@/components/ui/CoverImage'
import { PlaylistCover } from './Playlists'
import { coverUrl, formatDuration, formatLongDuration } from '@/lib/format'
import { isMobile } from '@/lib/platform'
import { useLongPress } from '@/hooks/useLongPress'
import { useBackDismiss } from '@/hooks/useBackDismiss'
import { impact } from '@/lib/haptics'

/** Shared cover thumbnail used by every row variant. */
function Thumb({ hash }: { hash: string | null }): React.JSX.Element {
  const thumb = coverUrl(hash, true)
  return (
    <div className={`${isMobile ? 'h-11 w-11' : 'h-8 w-8'} shrink-0 overflow-hidden rounded bg-surface-3`}>
      <CoverImage
        src={thumb}
        className="h-full w-full object-cover"
        fallback={
          <div className="flex h-full w-full items-center justify-center">
            <Music2 size={isMobile ? 16 : 12} className="text-text-3" />
          </div>
        }
      />
    </div>
  )
}

// Touch rows are taller (≥56px) to meet the 48dp Material target; desktop stays
// compact. `pressable` opts the row into the tap-highlight reset, and the
// explicit :active background gives taps visible feedback.
const ROW_CLS = isMobile
  ? 'pressable flex items-center gap-3 rounded-lg px-2 py-2.5 text-[14px] transition-colors active:bg-white/[0.07]'
  : 'flex items-center gap-2.5 rounded-lg px-2 py-1.5 text-[13px] transition-colors'

function Row({
  track,
  index,
  onPlay,
  onRemove,
  onMenu
}: {
  track: Track
  index: number
  onPlay: () => void
  onRemove: () => void
  onMenu: (track: Track, index: number) => void
}): React.JSX.Element {
  const { t } = useTranslation()
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: `${index}-${track.id}`
  })
  const currentId = usePlayerStore((s) => s.currentTrack?.id)
  const isCurrent = currentId === track.id

  // Long-press → context bottom sheet (mobile only). useLongPress swallows the
  // finger-lift click itself, so no suppress-click guard is needed here.
  const longPress = useLongPress(() => {
    impact()
    onMenu(track, index)
  })

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition, opacity: isDragging ? 0.6 : 1 }}
      className={`group ${ROW_CLS} ${isCurrent ? 'bg-[var(--accent-soft)]' : 'hover:bg-white/[0.045]'}`}
    >
      {/* Tappable content: single tap plays on mobile, double-click on desktop. */}
      <div
        className="flex min-w-0 flex-1 items-center gap-3"
        onClick={isMobile ? onPlay : undefined}
        onDoubleClick={isMobile ? undefined : onPlay}
        onPointerDown={isMobile ? longPress.onPointerDown : undefined}
        onPointerMove={isMobile ? longPress.onPointerMove : undefined}
        onPointerUp={isMobile ? longPress.onPointerUp : undefined}
        onPointerCancel={isMobile ? longPress.onPointerCancel : undefined}
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

      {/* Drag handle — always visible on touch, hover-revealed on desktop. Drag
          listeners live only here so the row body stays scroll/tap friendly. */}
      <button
        className={`flex shrink-0 cursor-grab items-center justify-center text-text-3 ${
          isMobile ? 'h-11 w-11' : 'h-6 w-6 opacity-0 group-hover:opacity-100'
        }`}
        style={{ touchAction: 'none' }}
        aria-label={`${track.title} — ${track.artist}`}
        {...attributes}
        {...listeners}
      >
        <GripVertical size={isMobile ? 18 : 14} />
      </button>

      {/* Desktop inline remove; on mobile use the long-press menu instead. */}
      {!isMobile && (
        <button
          className="icon-btn h-6 w-6 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
          onClick={onRemove}
          aria-label={t('playlists.remove_track', { title: track.title })}
        >
          <X size={13} />
        </button>
      )}
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
      onClick={isMobile ? onPlay : undefined}
      onDoubleClick={isMobile ? undefined : onPlay}
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

/** A single action row inside a mobile bottom-sheet menu. */
function SheetAction({
  icon: Icon,
  label,
  onClick,
  danger = false
}: {
  icon: typeof Play
  label: string
  onClick: () => void
  danger?: boolean
}): React.JSX.Element {
  return (
    <button
      className="flex w-full items-center gap-3 rounded-xl px-4 py-3 text-left text-[14px] transition-colors hover:bg-white/[0.06]"
      style={danger ? { color: '#ff6b6e' } : undefined}
      onClick={onClick}
    >
      <Icon size={18} className={`shrink-0 ${danger ? '' : 'text-text-3'}`} />
      {label}
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
  const playNext = usePlayerStore((s) => s.playNext)
  const enqueue = usePlayerStore((s) => s.enqueue)
  const [tracks, setTracks] = useState<Track[]>([])
  const [loading, setLoading] = useState(true)
  const [rulesOpen, setRulesOpen] = useState(false)
  const [pickerOpen, setPickerOpen] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [selectMode, setSelectMode] = useState(false)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [rowMenu, setRowMenu] = useState<{ track: Track; index: number } | null>(null)
  const [heroMenu, setHeroMenu] = useState(false)
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

  // Mouse on desktop (small distance), touch needs a hold so list scrolling
  // isn't captured as a drag (dnd-kit touch best practice: delay + tolerance).
  const sensors = useSensors(
    useSensor(MouseSensor, { activationConstraint: { distance: 4 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 250, tolerance: 6 } })
  )

  const sortableIds = useMemo(() => tracks.map((tr, i) => `${i}-${tr.id}`), [tracks])

  // Virtualized rows: a multi-thousand-track playlist used to mount one
  // dnd-kit sortable per row — seconds of freeze on open. Only the visible
  // window mounts; SortableContext still gets the full id list.
  const virtualizer = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => (isMobile ? 64 : 44),
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

  // Hardware back exits selection mode instead of leaving the page.
  useBackDismiss(selectMode, () => {
    setSelectMode(false)
    setSelected(new Set())
  })

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

  // Desktop shows every action inline; mobile keeps Play primary and folds the
  // rest into a "⋯" bottom sheet so the hero never overflows on a phone.
  const heroActions = (
    <>
      <button
        className="flex items-center gap-2 rounded-full px-5 py-2 text-[13px] font-semibold text-white transition-[transform,filter] hover:scale-[1.03] hover:brightness-110 active:scale-95 disabled:opacity-40"
        style={{ background: 'var(--accent)', boxShadow: '0 4px 16px var(--accent-glow)' }}
        onClick={() => playTracks(tracks, 0)}
        disabled={tracks.length === 0}
      >
        <Play size={15} fill="currentColor" /> {t('player.play')}
      </button>

      {isMobile ? (
        <button
          className="icon-btn h-11 w-11 rounded-full bg-white/[0.06]"
          onClick={() => setHeroMenu(true)}
          aria-label={t('playlists.more')}
          title={t('playlists.more')}
        >
          <MoreHorizontal size={18} />
        </button>
      ) : (
        <>
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
      )}
    </>
  )

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
        actions={heroActions}
      />

      {/* Selection bar lives ABOVE the scroll container: always visible, no
          sticky backdrop-filter re-blurring every scrolled frame. Solid
          surface on mobile (nested blur is broken on Android WebView anyway). */}
      {selectMode && (
        <div
          className={`${isMobile ? '' : 'glass-chrome'} flex shrink-0 items-center gap-2 px-4 py-2`}
          style={{
            borderColor: 'var(--hairline)',
            background: isMobile ? 'var(--color-surface-2)' : undefined
          }}
        >
          <button
            className={`icon-btn ${isMobile ? 'h-11 w-11' : 'h-8 w-8'}`}
            onClick={exitSelect}
            aria-label={t('common.cancel')}
          >
            <X size={16} />
          </button>
          <span className="text-[13px] font-medium">
            {t('playlists.selected_count', { count: selected.size })}
          </span>
          <div className="ml-auto flex items-center gap-2">
            <button
              className={`btn-ghost rounded-full px-3 text-[12px] font-medium ${isMobile ? 'py-2.5' : 'py-1.5'}`}
              onClick={() =>
                setSelected(
                  selected.size === tracks.length ? new Set() : new Set(tracks.map((_, i) => i))
                )
              }
            >
              {selected.size === tracks.length ? t('playlists.deselect_all') : t('playlists.select_all')}
            </button>
            <button
              className={`flex items-center gap-1.5 rounded-full px-3 text-[12px] font-semibold text-white disabled:opacity-40 ${
                isMobile ? 'py-2.5' : 'py-1.5'
              }`}
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
                  onMenu={(tr, idx) => setRowMenu({ track: tr, index: idx })}
                />
              ))}
            </SortableContext>
          </DndContext>
        )}
      </div>

      {/* Mobile per-row action sheet (long-press) */}
      {rowMenu && (
        <BottomSheet open onClose={() => setRowMenu(null)} title={rowMenu.track.title}>
          <div className="px-2 pb-2">
            <SheetAction
              icon={Play}
              label={t('player.play')}
              onClick={() => {
                playTracks(tracks, rowMenu.index)
                setRowMenu(null)
              }}
            />
            <SheetAction
              icon={ListStart}
              label={t('player.play_next')}
              onClick={() => {
                playNext([rowMenu.track])
                setRowMenu(null)
              }}
            />
            <SheetAction
              icon={ListEnd}
              label={t('player.add_to_queue')}
              onClick={() => {
                enqueue([rowMenu.track])
                setRowMenu(null)
              }}
            />
            <SheetAction
              icon={Trash2}
              label={t('playlists.remove')}
              danger
              onClick={() => {
                removeAt(rowMenu.index)
                setRowMenu(null)
              }}
            />
          </div>
        </BottomSheet>
      )}

      {/* Mobile hero overflow sheet */}
      {heroMenu && (
        <BottomSheet open onClose={() => setHeroMenu(false)} title={playlist?.name}>
          <div className="px-2 pb-2">
            {!isSmart && (
              <SheetAction
                icon={Plus}
                label={t('playlists.add_tracks')}
                onClick={() => {
                  setHeroMenu(false)
                  setPickerOpen(true)
                }}
              />
            )}
            {!isSmart && tracks.length > 0 && (
              <SheetAction
                icon={ListChecks}
                label={t('playlists.select')}
                onClick={() => {
                  setHeroMenu(false)
                  setSelectMode(true)
                }}
              />
            )}
            {isSmart && (
              <SheetAction
                icon={Pencil}
                label={t('smart.edit_rules')}
                onClick={() => {
                  setHeroMenu(false)
                  setRulesOpen(true)
                }}
              />
            )}
            <SheetAction
              icon={Trash2}
              label={t('playlists.delete')}
              danger
              onClick={() => {
                setHeroMenu(false)
                setConfirmOpen(true)
              }}
            />
          </div>
        </BottomSheet>
      )}

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
