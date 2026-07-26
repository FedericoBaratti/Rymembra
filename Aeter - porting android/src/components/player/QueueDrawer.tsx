import { useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { X, GripVertical, Save, Trash2, Music2 } from 'lucide-react'
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
  useSortable
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { useVirtualizer } from '@tanstack/react-virtual'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { isMobile } from '@/lib/platform'
import { coverUrl, formatDuration } from '@/lib/format'
import { select } from '@/lib/haptics'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import BottomSheet from '@/components/ui/BottomSheet'
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

  // On touch there is no hover, so the grip + remove controls must always be
  // visible; on desktop they fade in on row hover or keyboard focus.
  const ctrlVis = isMobile
    ? 'opacity-70'
    : 'opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100'

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
      className={`group flex items-center gap-2 rounded-lg px-2 transition-colors ${
        isMobile ? 'py-2 active:bg-white/[0.06]' : 'py-1.5'
      } ${isCurrent ? 'bg-[var(--accent-soft)]' : 'hover:bg-white/[0.05]'}`}
    >
      <button
        className={`shrink-0 cursor-grab touch-none text-text-3 active:cursor-grabbing ${ctrlVis}`}
        aria-label={`${track.title} — ${track.artist}`}
        {...attributes}
        {...listeners}
      >
        <GripVertical size={isMobile ? 16 : 14} />
      </button>
      <div
        className={`relative shrink-0 overflow-hidden rounded bg-surface-3 ${
          isMobile ? 'h-10 w-10' : 'h-8 w-8'
        }`}
      >
        <CoverImage
          src={cover}
          className="h-full w-full object-cover"
          fallback={
            <div className="flex h-full w-full items-center justify-center">
              <Music2 size={isMobile ? 14 : 12} className="text-text-3" />
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
        onClick={isMobile ? () => { select(); playQueueIndex(queueIndex) } : undefined}
        onDoubleClick={isMobile ? undefined : () => playQueueIndex(queueIndex)}
      >
        <div
          className={`truncate font-medium ${isMobile ? 'text-[13.5px]' : 'text-[12.5px]'} ${
            isCurrent ? 'text-[var(--accent)]' : ''
          }`}
        >
          {track.title}
        </div>
        <div className={`truncate text-text-3 ${isMobile ? 'text-[12px]' : 'text-[11px]'}`}>
          {track.artist}
        </div>
      </button>
      <span className="tnum text-[11px] text-text-3">{formatDuration(track.duration)}</span>
      <button
        className={`icon-btn shrink-0 ${isMobile ? 'h-11 w-11' : 'h-6 w-6'} ${ctrlVis}`}
        onClick={() => removeFromQueue(queueIndex)}
        aria-label={t('playlists.remove_track', { title: track.title })}
      >
        <X size={isMobile ? 16 : 13} />
      </button>
    </div>
  )
}

/** Title + save/clear/close action cluster, shared by both layouts. */
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
  const btn = `icon-btn ${isMobile ? 'h-11 w-11' : 'h-8 w-8'}`
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
                className={btn}
                onClick={onSave}
                title={t('player.save_as_playlist')}
                aria-label={t('player.save_as_playlist')}
              >
                <Save size={15} />
              </button>
            )}
            <button
              className={btn}
              onClick={onClear}
              title={t('player.clear_queue')}
              aria-label={t('player.clear_queue')}
            >
              <Trash2 size={15} />
            </button>
          </>
        )}
        <button className={btn} onClick={onClose} aria-label={t('common.close')}>
          <X size={16} />
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
 * subscriptions each) — a multi-second freeze on a phone. Only the visible
 * window mounts now; SortableContext still receives the full id list so
 * reorders resolve correctly, and dnd-kit auto-scroll drives the container.
 */
function QueueList({ scrollRef }: { scrollRef: React.RefObject<HTMLDivElement | null> }): React.JSX.Element {
  const { t } = useTranslation()
  const queue = usePlayerStore((s) => s.queue)
  const order = usePlayerStore((s) => s.order)
  const orderPos = usePlayerStore((s) => s.orderPos)
  const isPlaying = usePlayerStore((s) => s.isPlaying)
  const reorderQueue = usePlayerStore((s) => s.reorderQueue)

  // Hold-to-drag on touch (250ms, like PlaylistDetail) so scrolling the queue
  // doesn't grab rows; plain 4px threshold with a mouse.
  const sensors = useSensors(
    useSensor(MouseSensor, { activationConstraint: { distance: 4 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 250, tolerance: 6 } })
  )

  const ids = useMemo(() => order.map((_, i) => `${i}`), [order])

  const virtualizer = useVirtualizer({
    count: order.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => (isMobile ? 58 : 46),
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
  // Desktop only: BottomSheet already owns focus/Escape handling on mobile.
  const trapRef = useFocusTrap<HTMLElement>(open && !isMobile, () => setOpen(false))

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

  const header = (
    <QueueHeader
      count={queue.length}
      canSave={queue.some((q) => q.id > 0)}
      onSave={() => void saveAsPlaylist()}
      onClear={clearQueue}
      onClose={() => setOpen(false)}
    />
  )

  // Mobile: Material bottom sheet that rises above every view (z-70), so the
  // queue is reachable both from the compact player bar and from the
  // full-screen NowPlaying sheet (z-60). Its own drag-handle/backdrop close it.
  if (isMobile) {
    return (
      <BottomSheet open={open} onClose={() => setOpen(false)} scrollBody={false}>
        {/* Opaque header (nested backdrop-filter renders transparent on Android
            WebView); it sits OUTSIDE the scroll area so the virtualized list
            below owns the only scroll container. */}
        <div className="shrink-0" style={{ background: 'var(--color-surface-2)' }}>
          {header}
        </div>
        <div ref={scrollRef} className="queue-list min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          <QueueList scrollRef={scrollRef} />
        </div>
      </BottomSheet>
    )
  }

  // Desktop: floating glass panel anchored above the player (unchanged).
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
      <div className="pt-3">{header}</div>
      <div ref={scrollRef} className="queue-list min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        <QueueList scrollRef={scrollRef} />
      </div>
    </aside>
  )
}
