import { useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { X, GripVertical, Save, Trash2, Music2 } from 'lucide-react'
import {
  DndContext,
  closestCenter,
  MouseSensor,
  useSensor,
  useSensors,
  type DragEndEvent
} from '@dnd-kit/core'
import {
  SortableContext,
  verticalListSortingStrategy,
  useSortable
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { useVirtualizer } from '@tanstack/react-virtual'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { coverUrl, formatDuration } from '@/lib/format'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import CoverImage from '@/components/ui/CoverImage'
import type { Track } from '@shared/types'

/** Animated 3-bar "now playing" indicator (reuses the .eq-bars styles). */
function NowPlayingBars({ playing }: { playing: boolean }): React.JSX.Element {
  return (
    <span className={`eq-bars ${playing ? '' : 'eq-bars--paused'}`} style={{ height: 11 }} aria-hidden>
      <span />
      <span />
      <span />
    </span>
  )
}

function QueueRow({
  track,
  orderIdx,
  queueIndex,
  isCurrent,
  isPlaying
}: {
  track: Track
  orderIdx: number
  queueIndex: number
  isCurrent: boolean
  isPlaying: boolean
}): React.JSX.Element {
  const { t } = useTranslation()
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: `${orderIdx}`
  })
  const playQueueIndex = usePlayerStore((s) => s.playQueueIndex)
  const removeFromQueue = usePlayerStore((s) => s.removeFromQueue)
  const cover = coverUrl(track.cover_art_hash, true)

  const ctrlVis = 'opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100'

  return (
    <div
      ref={setNodeRef}
      style={{
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isDragging ? 0.6 : 1
      }}
      data-num={String(orderIdx + 1).padStart(2, '0')}
      data-current={isCurrent}
      className={`group flex items-center gap-2 rounded-lg px-2 py-1.5 transition-colors ${
        isCurrent ? 'bg-[var(--accent-soft)]' : 'hover:bg-white/[0.05]'
      }`}
    >
      <button
        className={`shrink-0 cursor-grab touch-none text-text-3 active:cursor-grabbing ${ctrlVis}`}
        aria-label={`${track.title} — ${track.artist}`}
        {...attributes}
        {...listeners}
      >
        <GripVertical size={14} />
      </button>
      <div className="relative h-8 w-8 shrink-0 overflow-hidden rounded bg-surface-3">
        <CoverImage
          src={cover}
          className="h-full w-full object-cover"
          fallback={
            <div className="flex h-full w-full items-center justify-center">
              <Music2 size={12} className="text-text-3" />
            </div>
          }
        />
        {isCurrent && (
          <div className="absolute inset-0 flex items-center justify-center bg-black/45">
            <NowPlayingBars playing={isPlaying} />
          </div>
        )}
      </div>
      <button
        className="min-w-0 flex-1 text-left"
        onDoubleClick={() => playQueueIndex(queueIndex)}
      >
        <div
          className={`truncate text-[12.5px] font-medium ${isCurrent ? 'text-[var(--accent)]' : ''}`}
        >
          {track.title}
        </div>
        <div className="truncate text-[11px] text-text-3">{track.artist}</div>
      </button>
      <span className="tnum text-[11px] text-text-3">{formatDuration(track.duration)}</span>
      <button
        className={`icon-btn h-6 w-6 shrink-0 ${ctrlVis}`}
        onClick={() => removeFromQueue(queueIndex)}
        aria-label={t('playlists.remove_track', { title: track.title })}
      >
        <X size={13} />
      </button>
    </div>
  )
}

/** Title + save/clear/close action cluster. */
function QueueHeader({
  count,
  canSave,
  onSave,
  onClear,
  onClose
}: {
  count: number
  canSave: boolean
  onSave: () => void
  onClear: () => void
  onClose: () => void
}): React.JSX.Element {
  const { t } = useTranslation()
  return (
    <div className="flex items-center justify-between px-4 pb-2 pt-1">
      <h2 className="text-[14px] font-bold">{t('player.queue_title')}</h2>
      <div className="flex items-center gap-0.5">
        {/* Save needs at least one library track: podcast episodes (negative
            synthetic ids) can't live in a playlist. Clear works regardless. */}
        {count > 0 && (
          <>
            {canSave && (
              <button
                className="icon-btn h-7 w-7"
                onClick={onSave}
                title={t('player.save_as_playlist')}
                aria-label={t('player.save_as_playlist')}
              >
                <Save size={14} />
              </button>
            )}
            <button
              className="icon-btn h-7 w-7"
              onClick={onClear}
              title={t('player.clear_queue')}
              aria-label={t('player.clear_queue')}
            >
              <Trash2 size={14} />
            </button>
          </>
        )}
        <button className="icon-btn h-7 w-7" onClick={onClose} aria-label={t('common.close')}>
          <X size={15} />
        </button>
      </div>
    </div>
  )
}

/** Section label ("In riproduzione" / "A seguire") between queue groups. */
function SectionLabel({ children }: { children: React.ReactNode }): React.JSX.Element {
  return (
    <div className="px-3 pb-1 pt-2 text-[10.5px] font-semibold uppercase tracking-wider text-text-3">
      {children}
    </div>
  )
}

/**
 * The queue body: section labels + drag-and-drop sortable rows, VIRTUALIZED.
 * A shuffled 10k queue used to mount 10k dnd-kit sortables (+2 store
 * subscriptions each) — a noticeable freeze. Only the visible window mounts
 * now; SortableContext still receives the full id list so reorders resolve
 * correctly, and dnd-kit auto-scroll drives the container.
 */
function QueueList({ scrollRef }: { scrollRef: React.RefObject<HTMLDivElement | null> }): React.JSX.Element {
  const { t } = useTranslation()
  const queue = usePlayerStore((s) => s.queue)
  const order = usePlayerStore((s) => s.order)
  const orderPos = usePlayerStore((s) => s.orderPos)
  const isPlaying = usePlayerStore((s) => s.isPlaying)
  const reorderQueue = usePlayerStore((s) => s.reorderQueue)

  const sensors = useSensors(useSensor(MouseSensor, { activationConstraint: { distance: 4 } }))

  const ids = useMemo(() => order.map((_, i) => `${i}`), [order])

  const virtualizer = useVirtualizer({
    count: order.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 46,
    overscan: 10
  })

  // Land on the playing track when the queue opens (the component mounts on
  // open), instead of at the top of a possibly huge list.
  const didInitialScroll = useRef(false)
  useEffect(() => {
    if (didInitialScroll.current) return
    didInitialScroll.current = true
    if (orderPos > 0) virtualizer.scrollToIndex(orderPos, { align: 'center' })
  }, [orderPos, virtualizer])

  const onDragEnd = (e: DragEndEvent): void => {
    const { active, over } = e
    if (!over || active.id === over.id) return
    // Sortable ids ARE the order indices, so no O(N) indexOf scans.
    const from = Number(active.id)
    const to = Number(over.id)
    if (Number.isInteger(from) && Number.isInteger(to) && from >= 0 && to >= 0) {
      reorderQueue(from, to)
    }
  }

  if (queue.length === 0) {
    return (
      <div className="px-3 py-10 text-center text-[12.5px] text-text-3">{t('player.queue_empty')}</div>
    )
  }

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
      <SortableContext items={ids} strategy={verticalListSortingStrategy}>
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map((vi) => {
            const orderIdx = vi.index
            const queueIndex = order[orderIdx]
            const track = queue[queueIndex]
            if (!track) return null
            const isCurrent = orderIdx === orderPos
            return (
              <div
                key={`${orderIdx}-${track.id}`}
                data-index={vi.index}
                // The two rows carrying a SectionLabel are taller than the
                // estimate — measureElement keeps positions exact.
                ref={virtualizer.measureElement}
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  transform: `translateY(${vi.start}px)`
                }}
              >
                {isCurrent && orderPos >= 0 && <SectionLabel>{t('player.now_playing')}</SectionLabel>}
                {orderIdx === orderPos + 1 && orderPos >= 0 && (
                  <SectionLabel>{t('player.up_next')}</SectionLabel>
                )}
                <QueueRow
                  track={track}
                  orderIdx={orderIdx}
                  queueIndex={queueIndex}
                  isCurrent={isCurrent}
                  isPlaying={isPlaying}
                />
              </div>
            )
          })}
        </div>
      </SortableContext>
    </DndContext>
  )
}

export default function QueueDrawer(): React.JSX.Element | null {
  const { t } = useTranslation()
  const open = useUiStore((s) => s.queueOpen)
  const setOpen = useUiStore((s) => s.setQueueOpen)
  const queue = usePlayerStore((s) => s.queue)
  const order = usePlayerStore((s) => s.order)
  const clearQueue = usePlayerStore((s) => s.clearQueue)
  const refreshPlaylists = useLibraryStore((s) => s.refreshPlaylists)
  const scrollRef = useRef<HTMLDivElement>(null)
  const trapRef = useFocusTrap<HTMLElement>(open, () => setOpen(false))

  if (!open) return null

  const saveAsPlaylist = async (): Promise<void> => {
    // Podcast episodes ride the queue with synthetic negative ids that have no
    // tracks row — persisting them would violate the playlist_tracks FK.
    const ids = order.map((i) => queue[i].id).filter((id) => id > 0)
    if (ids.length === 0) return
    const name = t('player.queue_playlist_name', { date: new Date().toLocaleDateString() })
    try {
      await window.aether.createPlaylist(name, undefined, ids)
      toast.success(t('player.queue_saved'))
      void refreshPlaylists()
    } catch (e) {
      toast.error(ipcErrorMessage(e))
    }
  }

  return (
    <aside
      ref={trapRef}
      role="dialog"
      aria-modal="true"
      aria-label={t('player.queue_title')}
      className="glass-chrome slide-in-right vt-queue fixed z-30 flex flex-col"
      style={{
        right: 'var(--player-gap)',
        top: 48,
        bottom: 'var(--player-clearance)',
        width: 'min(340px, calc(100vw - var(--rail-w) - 48px))',
        borderRadius: 'var(--radius-panel)',
        boxShadow: 'var(--shadow-3)'
      }}
    >
      <div className="pt-3">
        <QueueHeader
          count={queue.length}
          canSave={queue.some((q) => q.id > 0)}
          onSave={() => void saveAsPlaylist()}
          onClear={clearQueue}
          onClose={() => setOpen(false)}
        />
      </div>
      <div ref={scrollRef} className="queue-list min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        <QueueList scrollRef={scrollRef} />
      </div>
    </aside>
  )
}
