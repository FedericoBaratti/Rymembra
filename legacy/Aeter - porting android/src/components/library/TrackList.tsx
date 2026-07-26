import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useTranslation } from 'react-i18next'
import {
  Check,
  ChevronDown,
  ChevronUp,
  Folder,
  Heart,
  ListChecks,
  ListEnd,
  ListMusic,
  ListStart,
  MicVocal,
  Music2,
  Pencil,
  Play,
  Radio,
  Star,
  X
} from 'lucide-react'
import type { Track, TrackQuery } from '@shared/types'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { useDiscoveryStore } from '@/store/useDiscoveryStore'
import { toast } from '@/store/useToastStore'
import { coverUrl, formatDuration } from '@/lib/format'
import { isMobile } from '@/lib/platform'
import { useLongPress } from '@/hooks/useLongPress'
import { useBackDismiss } from '@/hooks/useBackDismiss'
import { usePullToRefresh } from '@/hooks/usePullToRefresh'
import { impact, select as hapticSelect } from '@/lib/haptics'
import BottomSheet from '@/components/ui/BottomSheet'
import CoverImage from '@/components/ui/CoverImage'

// Column templates live in global.css (.track-grid) and respond to the
// "content" container width; tg-album / tg-year cells toggle in lockstep.
const GRID = 'track-grid'

// Keep in sync with --player-clearance: desktop 92px player + 14px gap × 2;
// mobile 64px player + 56px bottom-nav + safe area + 14px gap.
const PLAYER_CLEARANCE = isMobile ? 160 : 120

// Row height must match the .track-grid rules in global.css: on mobile the row
// is a two-line list item (title + artist) with a 44px thumb → 64px (M3 dense).
const ROW_H = isMobile ? 64 : 48

interface MenuState {
  x: number
  y: number
  /** transform-origin so the menu grows from the cursor even when clamped */
  origin: string
  track: Track
  index: number
}

function Rating({ track, large = false }: { track: Track; large?: boolean }): React.JSX.Element {
  const { t } = useTranslation()
  const [hover, setHover] = useState(0)
  const [value, setValue] = useState(track.rating)

  useEffect(() => setValue(track.rating), [track.id, track.rating])

  return (
    <div className={`${large ? '' : 'tg-rating '}flex`} onMouseLeave={() => setHover(0)}>
      {[1, 2, 3, 4, 5].map((i) => (
        <button
          key={i}
          // On touch there is no hover, so the stars must always be visible to be
          // tappable — otherwise empty stars stay invisible and a track can't be
          // rated at all. On desktop they keep fading in on row hover (filled ones
          // stay visible via data-filled). See the touch rule in journal #19.
          // `large` = inside the long-press sheet: 44px M3 touch targets.
          className={`transition-[opacity,transform] duration-100 hover:scale-125 active:scale-95 ${
            large
              ? 'px-2.5 py-[11px] opacity-100'
              : isMobile
                ? 'px-0.5 py-[13px] opacity-100' // area tap ~40px in altezza — la larghezza è vincolata dalla colonna
                : 'p-0.5 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 data-[filled=true]:opacity-100'
          }`}
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
            size={large ? 22 : isMobile ? 14 : 12}
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
  className = '',
  onPullRefresh
}: {
  tracks: Track[]
  sortable?: boolean
  className?: string
  /** Enables pull-to-refresh on the list's scroll container (mobile only). */
  onPullRefresh?: () => Promise<unknown> | void
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
  // Mobile selection mode (M3 contextual action bar): entered from the
  // long-press menu, exited via the ✕ / hardware back / emptying the selection.
  const [selectMode, setSelectMode] = useState(false)
  const [playlistPicker, setPlaylistPicker] = useState(false)
  const pullToRefresh = usePullToRefresh(onPullRefresh)

  const exitSelectMode = (): void => {
    setSelectMode(false)
    setPlaylistPicker(false)
    setSelected(new Set())
  }

  // Hardware back (LIFO on the back stack): the context menu — a bespoke
  // bottom sheet, NOT the shared BottomSheet — closes first, then selection
  // mode. The playlist picker inside selection mode IS a BottomSheet and
  // registers itself.
  useBackDismiss(menu != null, () => {
    setMenu(null)
    setPlaylistSub(false)
  })
  useBackDismiss(selectMode, exitSelectMode)

  // Opens the row context menu. On desktop it's anchored to the cursor; on
  // mobile (long-press) coordinates are ignored and the menu renders as a
  // bottom sheet (see the `isMobile` branch in the menu markup).
  const openMenu = (clientX: number, clientY: number, track: Track, index: number): void => {
    const clampedX = clientX > window.innerWidth - 230
    const clampedY = clientY > window.innerHeight - 320
    setMenu({
      x: Math.min(clientX, window.innerWidth - 230),
      y: Math.min(clientY, window.innerHeight - 320),
      origin: `${clampedY ? 'bottom' : 'top'} ${clampedX ? 'right' : 'left'}`,
      track,
      index
    })
    setPlaylistSub(false)
  }

  // Long-press → context menu on touch. The hook lives at component scope (hook
  // rules); the row being pressed is captured in a ref by each row's pointerdown.
  const pressRow = useRef<{ track: Track; index: number } | null>(null)
  const longPress = useLongPress((e) => {
    const r = pressRow.current
    if (r) {
      impact()
      openMenu(e.clientX, e.clientY, r.track, r.index)
    }
  })

  const onRowClick = (e: React.MouseEvent, track: Track, index: number): void => {
    if (isMobile) {
      // Touch model (M3 / convenzione player musicali): tap riproduce, il
      // long-press apre il menu, la selezione multipla vive in selection mode.
      if (selectMode) {
        hapticSelect()
        setSelected((prev) => {
          const next = new Set(prev)
          if (next.has(track.id)) next.delete(track.id)
          else next.add(track.id)
          if (next.size === 0) setSelectMode(false)
          return next
        })
        return
      }
      impact()
      playTracks(tracks, index)
      return
    }
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
    estimateSize: () => ROW_H,
    overscan: 12,
    paddingEnd: PLAYER_CLEARANCE
  })

  useEffect(() => {
    if (!menu) return
    const close = (e: Event): void => {
      // useLongPress fa preventDefault sulla coda del gesto di apertura
      // (contextmenu nativo + click sintetico al rilascio): non è un "tap fuori".
      if (e.defaultPrevented) return
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
        className={`${GRID} track-head h-9 shrink-0 border-b text-[11px] font-semibold uppercase tracking-wide text-text-3`}
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

      <div ref={parentRef} className="relative min-h-0 flex-1 overflow-y-auto" {...pullToRefresh.pullProps}>
        {pullToRefresh.indicator}
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
                style={{ height: ROW_H, transform: `translateY(${vi.start}px)` }}
                onClick={(e) => onRowClick(e, track, vi.index)}
                onDoubleClick={isMobile ? undefined : () => playTracks(tracks, vi.index)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  e.stopPropagation()
                  openMenu(e.clientX, e.clientY, track, vi.index)
                }}
                onPointerDown={(e) => {
                  pressRow.current = { track, index: vi.index }
                  longPress.onPointerDown(e)
                }}
                onPointerMove={longPress.onPointerMove}
                onPointerUp={longPress.onPointerUp}
                onPointerCancel={longPress.onPointerCancel}
              >
                {/* data-idx: numero di riga 1-based per la skin Nothing (le liste
                    virtualizzate rendono inaffidabili i CSS counter). Inerte per
                    la skin plain. */}
                <div
                  className="tg-thumb relative h-6 w-6 overflow-hidden rounded bg-surface-3"
                  data-idx={String(vi.index + 1).padStart(2, '0')}
                >
                  <CoverImage
                    src={thumb}
                    className="h-full w-full object-cover"
                    fallback={
                      <div className="flex h-full w-full items-center justify-center">
                        <Music2 size={isMobile ? 16 : 11} className="text-text-3" />
                      </div>
                    }
                  />
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
                  {/* Selection mode: la cella iniziale diventa il checkbox (M3). */}
                  {isMobile && selectMode && (
                    <div
                      className={`absolute inset-0 z-10 flex items-center justify-center rounded ${
                        isSelected
                          ? 'bg-[var(--accent)]'
                          : 'bg-black/40 shadow-[inset_0_0_0_1.5px_rgba(255,255,255,0.35)]'
                      }`}
                    >
                      {isSelected && <Check size={20} className="text-[var(--color-surface-0)]" />}
                    </div>
                  )}
                </div>
                <div className={`tg-title truncate font-medium ${isCurrent ? 'text-[var(--accent)]' : ''}`}>
                  {track.title}
                </div>
                <div className="tg-artist truncate text-text-2">{track.artist}</div>
                <div className="tg-album truncate text-text-3">{track.album}</div>
                <div className="tg-year tnum text-text-3">{track.year ?? ''}</div>
                <div className="tg-dur tnum text-right text-text-3">{formatDuration(track.duration)}</div>
                <Rating track={track} />
              </div>
            )
          })}
        </div>
      </div>

      {/* Portal su body: dentro <main> (stacking context z-10) il menu
          finirebbe sotto player e bottom-nav (z-30 nel root context). */}
      {menu &&
        createPortal(
          (() => {
            const isMulti = selected.size > 1 && selected.has(menu.track.id)
            const selTracks = isMulti ? tracks.filter((tr) => selected.has(tr.id)) : [menu.track]
            const panel = (
            <div
              className={
                isMobile
                  ? 'glass-modal menu-pop fixed inset-x-0 bottom-0 z-50 rounded-t-2xl p-2 pb-[calc(var(--sa-bottom,env(safe-area-inset-bottom,0px))+8px)]'
                  : 'glass-modal menu-pop fixed z-50 w-[210px] rounded-xl p-1.5'
              }
              style={
                isMobile
                  ? ({ boxShadow: 'var(--shadow-2)', '--menu-origin': 'bottom center' } as React.CSSProperties)
                  : ({
                      left: menu.x,
                      top: menu.y,
                      boxShadow: 'var(--shadow-2)',
                      '--menu-origin': menu.origin
                    } as React.CSSProperties)
              }
              onClick={(e) => e.stopPropagation()}
            >
              {/* Mobile: il rating vive qui (la colonna stelle non entra nel
                  list item a due righe del telefono) con target 44px. */}
              {isMobile && !isMulti && (
                <div
                  className="mb-1 flex items-center justify-center border-b pb-1"
                  style={{ borderColor: 'var(--hairline)' }}
                >
                  <Rating track={menu.track} large />
                </div>
              )}
              {isMobile && (
                <button
                  className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-white/[0.07]"
                  onClick={() => {
                    hapticSelect()
                    setSelectMode(true)
                    setSelected(new Set(selTracks.map((tr) => tr.id)))
                    setMenu(null)
                  }}
                >
                  <ListChecks size={14} className="shrink-0 text-text-3" />
                  {t('library.select')}
                </button>
              )}
              {[
                {
                  label: t('player.play'),
                  icon: Play,
                  fn: () => (isMulti ? playTracks(selTracks, 0) : playTracks(tracks, menu.index))
                },
                { label: t('player.play_next'), icon: ListStart, fn: () => playNext(selTracks) },
                { label: t('player.add_to_queue'), icon: ListEnd, fn: () => enqueue(selTracks) },
                {
                  label: t('radio.start'),
                  icon: Radio,
                  fn: () =>
                    void useDiscoveryStore
                      .getState()
                      .startRadio({ kind: 'track', trackId: menu.track.id }, menu.track.title)
                }
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
                    className={
                      isMobile
                        ? 'glass-modal menu-pop relative z-50 mt-1 max-h-48 w-full overflow-y-auto rounded-xl p-1.5'
                        : 'glass-modal menu-pop absolute left-full top-0 z-50 ml-1 max-h-48 w-44 overflow-y-auto rounded-xl p-1.5'
                    }
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
                      const next = !menu.track.liked
                      void window.aether.setLiked(menu.track.id, next)
                      toast.success(next ? t('liked.added') : t('liked.removed'))
                      setMenu(null)
                    }}
                  >
                    <Heart
                      size={14}
                      className={`shrink-0 ${menu.track.liked ? 'fill-[var(--accent-like)] text-[var(--accent-like)]' : 'text-text-3'}`}
                    />
                    {menu.track.liked ? t('liked.remove') : t('liked.add')}
                  </button>
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
            if (!isMobile) return panel
            // Mobile: backdrop che intercetta i tap fuori (chiude senza avviare
            // la riga sottostante) e il contextmenu nativo di un long-press
            // sulla sheet aperta. Stessa resa del BottomSheet condiviso
            // (html[data-mobile] .overlay-in).
            return (
              <div
                className="overlay-in fixed inset-0 z-50 bg-black/55"
                role="dialog"
                aria-modal="true"
                onClick={() => {
                  setMenu(null)
                  setPlaylistSub(false)
                }}
                onContextMenu={(e) => {
                  e.preventDefault()
                  e.stopPropagation()
                }}
              >
                {panel}
              </div>
            )
          })(),
          document.body
        )}

      {/* Mobile selection mode: contextual action bar (M3) sopra il player.
          Portal per lo stesso motivo del menu (stacking context di <main>). */}
      {isMobile &&
        selectMode &&
        createPortal(
          (() => {
          const selTracks = tracks.filter((tr) => selected.has(tr.id))
          const ids = selTracks.map((tr) => tr.id)
          const act = (fn: () => void): void => {
            fn()
            exitSelectMode()
          }
          return (
            <>
              <div
                className="glass-modal slide-up-in fixed inset-x-2 z-40 flex items-center gap-0.5 rounded-2xl p-1.5"
                style={{ bottom: 'calc(var(--player-clearance) + 8px)', boxShadow: 'var(--shadow-2)' }}
              >
                <span className="min-w-0 flex-1 truncate px-2 text-[13px] font-semibold">
                  {t('library.selected_n', { count: selected.size })}
                </span>
                <button
                  className="icon-btn h-11 w-11 shrink-0"
                  aria-label={t('player.play')}
                  onClick={() => act(() => playTracks(selTracks, 0))}
                >
                  <Play size={18} />
                </button>
                <button
                  className="icon-btn h-11 w-11 shrink-0"
                  aria-label={t('player.play_next')}
                  onClick={() => act(() => playNext(selTracks))}
                >
                  <ListStart size={18} />
                </button>
                <button
                  className="icon-btn h-11 w-11 shrink-0"
                  aria-label={t('player.add_to_queue')}
                  onClick={() => act(() => enqueue(selTracks))}
                >
                  <ListEnd size={18} />
                </button>
                <button
                  className="icon-btn h-11 w-11 shrink-0"
                  aria-label={t('playlists.title')}
                  onClick={() => setPlaylistPicker(true)}
                >
                  <ListMusic size={18} />
                </button>
                <button
                  className="icon-btn h-11 w-11 shrink-0"
                  aria-label={t('metadata.edit_n', { count: selected.size })}
                  onClick={() =>
                    act(() => (ids.length === 1 ? setEditTrackId(ids[0]) : setBatchEditTrackIds(ids)))
                  }
                >
                  <Pencil size={18} />
                </button>
                <button
                  className="icon-btn h-11 w-11 shrink-0"
                  aria-label={t('common.close')}
                  onClick={exitSelectMode}
                >
                  <X size={18} />
                </button>
              </div>

              <BottomSheet
                open={playlistPicker}
                onClose={() => setPlaylistPicker(false)}
                title={t('playlists.title')}
              >
                <div className="px-3 pb-2">
                  {playlists.length === 0 && (
                    <div className="px-2.5 py-3 text-[13px] text-text-3">—</div>
                  )}
                  {playlists.map((p) => (
                    <button
                      key={p.id}
                      className="flex min-h-[48px] w-full items-center gap-3 rounded-lg px-3 text-left text-[14px] transition-colors active:bg-white/[0.07]"
                      onClick={() => {
                        void window.aether.addToPlaylist(p.id, ids).then(() => {
                          toast.success(
                            t('toast.added_to_playlist', { count: ids.length, name: p.name })
                          )
                        })
                        exitSelectMode()
                      }}
                    >
                      <ListMusic size={16} className="shrink-0 text-text-3" />
                      <span className="truncate">{p.name}</span>
                    </button>
                  ))}
                </div>
              </BottomSheet>
            </>
            )
          })(),
          document.body
        )}
    </div>
  )
}
