import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Repeat,
  Repeat1,
  Shuffle,
  ListMusic,
  SlidersHorizontal,
  Maximize2,
  Moon,
  Music2
} from 'lucide-react'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { coverUrl } from '@/lib/format'
import { isMobile } from '@/lib/platform'
import { playerEngine } from '@/lib/player'
import { tap, select } from '@/lib/haptics'
import Tooltip from '@/components/ui/Tooltip'
import Scrubber from './Scrubber'
import CoverImage from '@/components/ui/CoverImage'
import MiniVisualizer from './MiniVisualizer'
import VolumeControl from './VolumeControl'
import SleepTimerMenu from './SleepTimerMenu'

export default function PlayerBar(): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const track = usePlayerStore((s) => s.currentTrack)
  const isPlaying = usePlayerStore((s) => s.isPlaying)
  const repeat = usePlayerStore((s) => s.repeat)
  const shuffle = usePlayerStore((s) => s.shuffle)
  const loadError = usePlayerStore((s) => s.loadError)
  const sleepEndsAt = usePlayerStore((s) => s.sleepEndsAt)
  const togglePlay = usePlayerStore((s) => s.togglePlay)
  const next = usePlayerStore((s) => s.next)
  const previous = usePlayerStore((s) => s.previous)
  const cycleRepeat = usePlayerStore((s) => s.cycleRepeat)
  const toggleShuffle = usePlayerStore((s) => s.toggleShuffle)
  // Only the flags actually rendered; actions come from getState() so the bar
  // doesn't re-render on unrelated UI-store changes.
  const sleepMenuOpen = useUiStore((s) => s.sleepMenuOpen)
  const eqOpen = useUiStore((s) => s.eqOpen)
  const queueOpen = useUiStore((s) => s.queueOpen)
  const ui = useUiStore.getState()

  const cover = coverUrl(track?.cover_art_hash)
  const RepeatIcon = repeat === 'one' ? Repeat1 : Repeat

  // Mobile mini-player: drive the top progress hairline by writing a --progress
  // (0–1) CSS var on the shell. Done via ref so playback position never
  // triggers a React re-render of the bar. A 500 ms interval is plenty: the
  // hairline advances ~1 px every few hundred ms on a phone-width track, so a
  // 60 fps rAF loop was 30x more wake-ups for zero visible difference.
  const shellRef = useRef<HTMLElement>(null)
  useEffect(() => {
    if (!isMobile) return
    const paint = (): void => {
      const el = shellRef.current
      if (!el) return
      const dur = playerEngine.duration() || track?.duration || 0
      const frac = dur > 0 ? Math.min(1, playerEngine.position() / dur) : 0
      el.style.setProperty('--progress', String(frac))
    }
    // While paused/stopped the position is fixed: paint once and idle.
    paint()
    if (!isPlaying) return
    const iv = window.setInterval(paint, 500)
    return () => window.clearInterval(iv)
  }, [isPlaying, track?.duration])

  const goToAlbum = (): void => {
    if (!track) return
    // The albums array is only needed on click — read it lazily so the player
    // bar doesn't re-render on every library refresh.
    const album = useLibraryStore
      .getState()
      .albums.find(
        (a) => a.title === track.album && a.artist === (track.album_artist ?? track.artist)
      )
    navigate(album ? `/albums/${album.id}` : '/albums')
  }

  // Mini-player swipe gestures (mobile): orizzontale = skip traccia (segue il
  // dito, commit oltre ±64px), verticale verso l'alto ≥48px = apre NowPlaying.
  // Le gesture che partono entro 32px dai bordi laterali sono ignorate: quella
  // fascia è del back gesture di sistema (lezione Spotify/YT Music). I bottoni
  // restano il percorso primario — lo swipe è solo un acceleratore.
  const EDGE_GUARD = 32
  const swipe = useRef<{ x: number; y: number; axis: 'h' | 'v' | null; active: boolean }>({
    x: 0,
    y: 0,
    axis: null,
    active: false
  })
  const suppressClick = useRef(false)
  const [swipeX, setSwipeX] = useState(0)

  const onSwipeStart = (e: React.PointerEvent): void => {
    if (!track) return
    if (e.clientX < EDGE_GUARD || e.clientX > window.innerWidth - EDGE_GUARD) return
    swipe.current = { x: e.clientX, y: e.clientY, axis: null, active: true }
    suppressClick.current = false
  }
  const onSwipeMove = (e: React.PointerEvent): void => {
    const s = swipe.current
    if (!s.active) return
    const dx = e.clientX - s.x
    const dy = e.clientY - s.y
    if (!s.axis) {
      if (Math.abs(dx) < 10 && Math.abs(dy) < 10) return
      s.axis = Math.abs(dx) > Math.abs(dy) ? 'h' : 'v'
      suppressClick.current = true
      if (s.axis === 'h') (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    }
    if (s.axis === 'h') setSwipeX(dx)
  }
  const onSwipeEnd = (e: React.PointerEvent): void => {
    const s = swipe.current
    if (!s.active) return
    s.active = false
    if (s.axis === 'h') {
      if (swipeX <= -64) {
        tap()
        next()
      } else if (swipeX >= 64) {
        tap()
        previous()
      }
      setSwipeX(0)
    } else if (s.axis === 'v' && s.y - e.clientY >= 48) {
      select()
      ui.setNowPlayingOpen(true)
    }
    s.axis = null
  }
  const onSwipeCancel = (): void => {
    swipe.current.active = false
    swipe.current.axis = null
    setSwipeX(0)
  }

  // Compact touch player: tap the cover/title to open the expanded NowPlaying
  // sheet; only essential transport on the bar itself.
  if (isMobile) {
    return (
      <footer
        ref={shellRef}
        className="player-shell glass-chrome slide-up-in flex items-center gap-3 px-3"
        data-playing={isPlaying}
        style={{ boxShadow: 'var(--shadow-player)' }}
      >
        {/* Glanceable progress hairline pinned to the top edge. */}
        <div className="player-progress" aria-hidden />
        <button
          className="pressable flex min-w-0 flex-1 touch-none items-center gap-3 text-left"
          data-tour="mini-player"
          onClick={() => {
            if (suppressClick.current) return
            if (track) ui.setNowPlayingOpen(true)
          }}
          onPointerDown={onSwipeStart}
          onPointerMove={onSwipeMove}
          onPointerUp={onSwipeEnd}
          onPointerCancel={onSwipeCancel}
          aria-label={t('player.queue_title')}
          style={{
            transform: swipeX ? `translateX(${swipeX}px)` : undefined,
            opacity: swipeX ? Math.max(0.35, 1 - Math.abs(swipeX) / 240) : undefined,
            transition: swipe.current.active
              ? 'none'
              : 'transform var(--dur-2) var(--ease-out-expo), opacity var(--dur-2) var(--ease-out-expo)'
          }}
        >
          <div className="flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-surface-3">
            <CoverImage
              src={cover}
              eager
              className="h-full w-full object-cover"
              fallback={<Music2 size={18} className="text-text-3" />}
            />
          </div>
          <div className="min-w-0">
            {track ? (
              <>
                <div className="truncate text-[13px] font-semibold leading-tight">{track.title}</div>
                <div className="truncate text-[11px] leading-tight text-text-2">{track.artist}</div>
              </>
            ) : (
              <div className="text-[12px] text-text-3">
                {loadError ? (
                  <span className="text-[var(--danger)]">{loadError}</span>
                ) : (
                  t('player.nothing_playing')
                )}
              </div>
            )}
          </div>
        </button>
        <button
          className="play-btn-primary pressable flex h-12 w-12 shrink-0 items-center justify-center rounded-full text-white"
          style={{ background: 'var(--accent)', boxShadow: '0 4px 20px var(--accent-glow)' }}
          onClick={() => {
            tap()
            togglePlay()
          }}
          aria-label={isPlaying ? t('player.pause') : t('player.play')}
        >
          {isPlaying ? <Pause size={20} fill="currentColor" /> : <Play size={20} fill="currentColor" className="ml-0.5" />}
        </button>
        <button
          className="icon-btn h-12 w-12 shrink-0"
          onClick={() => {
            tap()
            next()
          }}
          aria-label={t('player.next')}
        >
          <SkipForward size={20} fill="currentColor" />
        </button>
        <button
          className="icon-btn h-12 w-12 shrink-0"
          data-active={queueOpen}
          onClick={() => {
            select()
            ui.toggleQueue()
          }}
          aria-label={t('player.queue')}
        >
          <ListMusic size={19} />
        </button>
      </footer>
    )
  }

  return (
    <footer
      className="player-shell glass-chrome slide-up-in flex items-center gap-4 px-4"
      data-playing={isPlaying}
      style={{
        boxShadow:
          'inset 0 1px 0 rgba(255,255,255,0.06), 0 8px 40px rgba(0,0,0,0.5), 0 0 60px var(--accent-soft)'
      }}
    >
      {/* Cover + track info */}
      <div className="flex w-[180px] min-w-[150px] shrink-0 items-center gap-3 min-[900px]:w-[260px]">
        <div
          className="flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-surface-3"
          style={{
            boxShadow: track ? '0 0 24px var(--accent-glow)' : 'none',
            transition: 'box-shadow 800ms ease'
          }}
        >
          <CoverImage
            src={cover}
            eager
            className="h-full w-full object-cover"
            fallback={<Music2 size={20} className="text-text-3" />}
          />
        </div>
        <div className="min-w-0">
          {track ? (
            <>
              <div className="truncate text-[14px] font-bold leading-tight">{track.title}</div>
              <button
                className="block max-w-full truncate text-[12px] leading-tight text-text-2 transition-colors hover:text-[var(--accent)] hover:underline"
                onClick={() => navigate(`/artists/${encodeURIComponent(track.artist)}`)}
              >
                {track.artist}
              </button>
              <button
                className="hidden max-w-full truncate text-[11px] leading-tight text-text-3 transition-colors hover:text-[var(--accent)] hover:underline min-[800px]:block"
                onClick={goToAlbum}
              >
                {track.album}
              </button>
            </>
          ) : (
            <div className="text-[13px] text-text-3">
              {loadError ? (
                <span className="text-[var(--danger)]">{loadError}</span>
              ) : (
                t('player.nothing_playing')
              )}
            </div>
          )}
        </div>
      </div>

      {/* Controls + scrubber */}
      <div className="flex min-w-0 flex-1 flex-col items-center justify-center gap-0.5">
        <div className="flex items-center gap-1.5" data-tour="player-controls">
          <Tooltip label={`${t('player.shuffle')} (S)`}>
            <button
              className="icon-btn h-8 w-8"
              data-active={shuffle}
              onClick={toggleShuffle}
              aria-label={t('player.shuffle')}
              aria-pressed={shuffle}
            >
              <Shuffle size={15} />
            </button>
          </Tooltip>
          <Tooltip label={`${t('player.previous')} (Shift+←)`}>
            <button
              className="icon-btn h-9 w-9"
              onClick={previous}
              aria-label={t('player.previous')}
            >
              <SkipBack size={18} fill="currentColor" />
            </button>
          </Tooltip>
          <Tooltip label={`${isPlaying ? t('player.pause') : t('player.play')} (Space)`}>
            <button
              className="play-btn-primary no-drag flex h-12 w-12 items-center justify-center rounded-full text-white transition-all duration-150 hover:scale-105 active:scale-95"
              style={{
                background: 'var(--accent)',
                boxShadow: '0 4px 20px var(--accent-glow)'
              }}
              onClick={togglePlay}
              aria-label={isPlaying ? t('player.pause') : t('player.play')}
            >
              {isPlaying ? (
                <Pause size={20} fill="currentColor" />
              ) : (
                <Play size={20} fill="currentColor" className="ml-0.5" />
              )}
            </button>
          </Tooltip>
          <Tooltip label={`${t('player.next')} (Shift+→)`}>
            <button
              className="icon-btn h-9 w-9"
              onClick={() => next()}
              aria-label={t('player.next')}
            >
              <SkipForward size={18} fill="currentColor" />
            </button>
          </Tooltip>
          <Tooltip label={`${t('player.repeat')}: ${repeat} (R)`}>
            <button
              className="icon-btn h-8 w-8"
              data-active={repeat !== 'off'}
              onClick={cycleRepeat}
              aria-label={t('player.repeat')}
              aria-pressed={repeat !== 'off'}
            >
              <RepeatIcon size={15} />
            </button>
          </Tooltip>
        </div>
        <Scrubber />
      </div>

      {/* Right cluster */}
      <div className="flex shrink-0 items-center gap-1" data-tour="player-extras">
        <div className="mini-viz mr-2 hidden lg:block">
          <MiniVisualizer />
        </div>
        <SleepTimerMenu>
          <Tooltip label={t('sleep.title')}>
            <button
              className="icon-btn h-9 w-9"
              data-active={sleepEndsAt != null}
              onClick={() => ui.setSleepMenuOpen(!sleepMenuOpen)}
              aria-label={t('sleep.title')}
            >
              <Moon size={16} />
            </button>
          </Tooltip>
        </SleepTimerMenu>
        <Tooltip label={t('player.equalizer')}>
          <button
            className="icon-btn h-9 w-9"
            data-active={eqOpen}
            onClick={() => ui.setEqOpen(!eqOpen)}
            aria-label={t('player.equalizer')}
          >
            <SlidersHorizontal size={16} />
          </button>
        </Tooltip>
        <Tooltip label={`${t('player.queue')} (Ctrl+L)`}>
          <button
            className="icon-btn h-9 w-9"
            data-active={queueOpen}
            onClick={ui.toggleQueue}
            aria-label={t('player.queue')}
          >
            <ListMusic size={17} />
          </button>
        </Tooltip>
        <VolumeControl />
        <Tooltip label={`${t('player.fullscreen')} (F)`}>
          <button
            className="icon-btn h-9 w-9"
            onClick={() => track && ui.setFullscreenViz(true)}
            disabled={!track}
            aria-label={t('player.fullscreen')}
          >
            <Maximize2 size={16} />
          </button>
        </Tooltip>
      </div>
    </footer>
  )
}
