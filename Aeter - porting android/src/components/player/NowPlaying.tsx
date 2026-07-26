import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import {
  ChevronDown,
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Repeat,
  Repeat1,
  Shuffle,
  ListMusic,
  SlidersHorizontal,
  Music2,
  Heart,
  Radio,
  MicVocal,
  Moon,
  Volume2,
  Volume1,
  VolumeX
} from 'lucide-react'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { useDiscoveryStore } from '@/store/useDiscoveryStore'
import { coverUrl } from '@/lib/format'
import { tap, select } from '@/lib/haptics'
import { isEpisodeTrack } from '@/lib/podcast'
import Scrubber from './Scrubber'
import CoverImage from '@/components/ui/CoverImage'

/**
 * Mobile-only full-screen "now playing" sheet, opened by tapping the compact
 * PlayerBar. Reuses the existing player store and Scrubber; mirrors the desktop
 * transport in a touch-sized vertical layout. Rendered (gated by isMobile) from
 * App.tsx and shown when useUiStore.nowPlayingOpen is true.
 */
export default function NowPlaying(): React.JSX.Element | null {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const open = useUiStore((s) => s.nowPlayingOpen)
  const setOpen = useUiStore((s) => s.setNowPlayingOpen)
  // Only the flags actually rendered; actions come from getState() so the
  // sheet doesn't re-render on unrelated UI-store changes.
  const lyricsOpen = useUiStore((s) => s.lyricsOpen)
  const eqOpen = useUiStore((s) => s.eqOpen)
  const queueOpen = useUiStore((s) => s.queueOpen)
  const sleepMenuOpen = useUiStore((s) => s.sleepMenuOpen)
  const ui = useUiStore.getState()
  const track = usePlayerStore((s) => s.currentTrack)
  const isPlaying = usePlayerStore((s) => s.isPlaying)
  const repeat = usePlayerStore((s) => s.repeat)
  const shuffle = usePlayerStore((s) => s.shuffle)
  const volume = usePlayerStore((s) => s.volume)
  const muted = usePlayerStore((s) => s.muted)
  const setVolume = usePlayerStore((s) => s.setVolume)
  const toggleMute = usePlayerStore((s) => s.toggleMute)
  const togglePlay = usePlayerStore((s) => s.togglePlay)
  const next = usePlayerStore((s) => s.next)
  const previous = usePlayerStore((s) => s.previous)
  const cycleRepeat = usePlayerStore((s) => s.cycleRepeat)
  const toggleShuffle = usePlayerStore((s) => s.toggleShuffle)

  // Swipe-down-to-dismiss (convenzione gesture Android per le sheet a tutto
  // schermo). Riprende il pattern pointer di BottomSheet: trascina solo verso il
  // basso, oltre 110px chiude, altrimenti torna a posto. Lo scrubber (canvas) e i
  // bottoni catturano i propri pointer, quindi li escludiamo dall'avvio del drag.
  const drag = useRef<{ startY: number; active: boolean }>({ startY: 0, active: false })
  const [dragY, setDragY] = useState(0)
  // L'animazione d'entrata (np-rise, fill both) sovrascriverebbe il transform
  // inline del drag finché è in corso; la rimuoviamo a fine entrata.
  const [entering, setEntering] = useState(true)

  // Swipe orizzontale sull'artwork = cambio traccia (convenzione player mobile):
  // l'asse si decide nei primi ~10px; orizzontale → l'art segue il dito e oltre
  // ±80px committa next/previous; verticale → ricade nel dismiss della sheet
  // (stesso stato dragY). Gesture ignorate entro 32px dai bordi laterali (zona
  // del back gesture di sistema); i bottoni skip restano il percorso primario.
  const artSwipe = useRef<{ x: number; y: number; axis: 'h' | 'v' | null; active: boolean }>({
    x: 0,
    y: 0,
    axis: null,
    active: false
  })
  const [artX, setArtX] = useState(0)

  if (!open) return null

  const cover = coverUrl(track?.cover_art_hash)
  const RepeatIcon = repeat === 'one' ? Repeat1 : Repeat
  const VolIcon = muted || volume === 0 ? VolumeX : volume < 0.5 ? Volume1 : Volume2
  const isEpisode = !!track && isEpisodeTrack(track)

  const onDragStart = (e: React.PointerEvent): void => {
    // L'artwork ha la propria gesture a doppio asse (vedi onArt*).
    if ((e.target as HTMLElement).closest('button, canvas, input, a, [role="slider"], [data-np-art]'))
      return
    drag.current = { startY: e.clientY, active: true }
  }
  const onDragMove = (e: React.PointerEvent): void => {
    if (!drag.current.active) return
    const dy = e.clientY - drag.current.startY
    setDragY(dy > 0 ? dy : 0)
  }
  const onDragEnd = (): void => {
    if (!drag.current.active) return
    drag.current.active = false
    if (dragY > 110) {
      select()
      setOpen(false)
    }
    setDragY(0)
  }

  const onArtStart = (e: React.PointerEvent): void => {
    if (!track) return
    if (e.clientX < 32 || e.clientX > window.innerWidth - 32) return
    artSwipe.current = { x: e.clientX, y: e.clientY, axis: null, active: true }
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }
  const onArtMove = (e: React.PointerEvent): void => {
    const s = artSwipe.current
    if (!s.active) return
    const dx = e.clientX - s.x
    const dy = e.clientY - s.y
    if (!s.axis) {
      if (Math.abs(dx) < 10 && Math.abs(dy) < 10) return
      s.axis = Math.abs(dx) > Math.abs(dy) ? 'h' : 'v'
    }
    if (s.axis === 'h') setArtX(dx)
    else setDragY(dy > 0 ? dy : 0) // verticale: stesso dismiss della sheet
  }
  const onArtEnd = (): void => {
    const s = artSwipe.current
    if (!s.active) return
    s.active = false
    if (s.axis === 'h') {
      if (artX <= -80) {
        tap()
        next()
      } else if (artX >= 80) {
        tap()
        previous()
      }
      setArtX(0)
    } else if (s.axis === 'v') {
      if (dragY > 110) {
        select()
        setOpen(false)
      }
      setDragY(0)
    }
    s.axis = null
  }
  const onArtCancel = (): void => {
    artSwipe.current.active = false
    artSwipe.current.axis = null
    setArtX(0)
    setDragY(0)
  }

  return (
    <div
      className={`np-screen fixed inset-0 z-[60] flex flex-col ${entering ? 'np-rise' : ''}`}
      data-playing={isPlaying}
      onAnimationEnd={(e) => {
        if (e.target === e.currentTarget) setEntering(false)
      }}
      onPointerDown={onDragStart}
      onPointerMove={onDragMove}
      onPointerUp={onDragEnd}
      onPointerCancel={onDragEnd}
      style={{
        background: 'var(--color-surface-0)',
        paddingTop: 'calc(var(--sa-top, env(safe-area-inset-top, 0px)) + 8px)',
        paddingBottom: 'calc(var(--sa-bottom, env(safe-area-inset-bottom, 0px)) + 16px)',
        transform: dragY ? `translateY(${dragY}px)` : undefined,
        transition:
          drag.current.active || artSwipe.current.active
            ? 'none'
            : 'transform var(--dur-2) var(--ease-out-expo)',
        // sfuma leggermente mentre si trascina via, come una sheet M3
        opacity: dragY ? Math.max(0.4, 1 - dragY / 600) : undefined
      }}
    >
      {/* Subtle accent scrim from the top so the chrome reads as "now playing"
          without a full blurred-cover backdrop (kept solid for the WebView). */}
      <div
        className="np-scrim pointer-events-none absolute inset-x-0 top-0 h-[42%]"
        aria-hidden
        style={{
          background:
            'linear-gradient(180deg, var(--accent-soft) 0%, rgba(var(--accent-rgb) / 0.05) 40%, transparent 100%)'
        }}
      />

      {/* Maniglia di trascinamento (affordance swipe-down M3). */}
      <div className="np-handle relative flex h-3 shrink-0 items-center justify-center" aria-hidden>
        <span className="h-1.5 w-10 rounded-full bg-white/25" />
      </div>

      {/* Top bar */}
      <div className="relative flex items-center justify-between px-4 py-2">
        <button
          className="icon-btn pressable h-12 w-12"
          onClick={() => {
            select()
            setOpen(false)
          }}
          aria-label={t('common.close')}
        >
          <ChevronDown size={24} />
        </button>
        <div className="np-tools flex items-center gap-0.5" data-tour="np-tools">
          {/* Radio/Like/Lyrics are library-only: a podcast episode's negative id
              has no tracks row, so radio would toast an error, like would lie and
              there are no lyrics to fetch. */}
          {track && !isEpisode && (
            <button
              className="icon-btn pressable h-12 w-12"
              onClick={() => {
                select()
                void useDiscoveryStore
                  .getState()
                  .startRadio({ kind: 'track', trackId: track.id }, track.title)
              }}
              aria-label={t('radio.start')}
            >
              <Radio size={19} />
            </button>
          )}
          {track && !isEpisode && (
            <button
              className="icon-btn pressable h-12 w-12"
              data-active={lyricsOpen}
              onClick={() => {
                select()
                ui.setLyricsOpen(true)
              }}
              aria-label={t('lyrics.title')}
            >
              <MicVocal size={19} />
            </button>
          )}
          <button
            className="icon-btn pressable h-12 w-12"
            data-active={eqOpen}
            onClick={() => ui.setEqOpen(!eqOpen)}
            aria-label={t('player.equalizer')}
          >
            <SlidersHorizontal size={19} />
          </button>
          <button
            className="icon-btn pressable h-12 w-12"
            data-active={queueOpen}
            onClick={() => {
              select()
              ui.toggleQueue()
            }}
            aria-label={t('player.queue')}
          >
            <ListMusic size={19} />
          </button>
          <button
            className="icon-btn pressable h-12 w-12"
            data-active={sleepMenuOpen}
            onClick={() => {
              select()
              ui.setSleepMenuOpen(true)
            }}
            aria-label={t('sleep.title')}
          >
            <Moon size={19} />
          </button>
        </div>
      </div>

      {/* Cover — key={track.id}: al cambio traccia l'art si rimonta, così le
          animazioni per-traccia (scale-in; matrix reveal nella skin Nothing)
          si riavviano. Swipe orizzontale = skip (vedi onArt*). */}
      <div className="relative flex min-h-0 flex-1 items-center justify-center px-8">
        <div
          key={track?.id ?? 'empty'}
          data-np-art
          className="np-art scale-in flex aspect-square w-full max-w-[min(78vw,400px)] touch-none items-center justify-center overflow-hidden rounded-2xl bg-surface-3"
          onPointerDown={onArtStart}
          onPointerMove={onArtMove}
          onPointerUp={onArtEnd}
          onPointerCancel={onArtCancel}
          style={{
            boxShadow: track
              ? 'var(--shadow-3), 0 0 70px var(--accent-glow)'
              : 'var(--shadow-3)',
            transform: artX ? `translateX(${artX}px)` : undefined,
            opacity: artX ? Math.max(0.3, 1 - Math.abs(artX) / 360) : undefined,
            transition: artSwipe.current.active
              ? 'none'
              : 'transform var(--dur-2) var(--ease-out-expo), opacity var(--dur-2) var(--ease-out-expo)'
          }}
        >
          <CoverImage
            src={cover}
            eager
            className="h-full w-full object-cover"
            fallback={<Music2 size={72} className="text-text-3" />}
          />
        </div>
      </div>

      {/* Metadata + controls: centered column so they don't stretch on large
          screens (Android 16/API 36 fills the window on displays ≥600dp). */}
      <div className="np-meta relative mx-auto w-full max-w-[480px]">
        {/* Track info + Like */}
        <div className="flex items-center gap-3 px-6 pt-6">
          <div className="min-w-0 flex-1 text-center">
            {/* key: rimonta il titolo per traccia → riavvia le animazioni della
                skin (print-in Nothing); inerte per la skin plain. */}
            <div
              key={track?.id ?? 'empty'}
              className="np-title truncate text-[22px] font-bold leading-tight tracking-tight"
              data-text={track?.title ?? t('player.nothing_playing')}
            >
              {track?.title ?? t('player.nothing_playing')}
            </div>
            {track ? (
              <button
                className="np-artist max-w-full truncate pt-1 text-[14px] text-text-2 transition-colors active:text-[var(--accent)]"
                onClick={() => {
                  select()
                  navigate(`/artists/${encodeURIComponent(track.artist)}`)
                  setOpen(false)
                }}
              >
                {track.artist}
              </button>
            ) : (
              <div className="truncate pt-1 text-[14px] text-text-2" />
            )}
          </div>
          {track && !isEpisodeTrack(track) && (
            <button
              className="icon-btn pressable h-12 w-12 shrink-0"
              onClick={() => {
                const next = !track.liked
                tap()
                void window.aether.setLiked(track.id, next)
                usePlayerStore.getState().updateTrack({ ...track, liked: next ? 1 : 0 })
              }}
              aria-label={track.liked ? t('liked.remove') : t('liked.add')}
              aria-pressed={!!track.liked}
            >
              <Heart
                size={22}
                className={track.liked ? 'fill-[var(--accent-like)] text-[var(--accent-like)]' : ''}
              />
            </button>
          )}
        </div>

        {/* Scrubber */}
        <div className="np-scrubber px-6 pt-5">
          <Scrubber />
        </div>

        {/* Transport */}
        <div className="np-transport flex items-center justify-center gap-4 px-6 pt-4" data-tour="np-transport">
          <button
            className="icon-btn pressable h-12 w-12"
            data-active={shuffle}
            onClick={() => {
              select()
              toggleShuffle()
            }}
            aria-label={t('player.shuffle')}
            aria-pressed={shuffle}
          >
            <Shuffle size={20} />
          </button>
          <button
            className="icon-btn pressable h-14 w-14"
            onClick={() => {
              tap()
              previous()
            }}
            aria-label={t('player.previous')}
          >
            <SkipBack size={26} fill="currentColor" />
          </button>
          <button
            className="play-btn-primary pressable flex h-16 w-16 items-center justify-center rounded-full text-white"
            style={{ background: 'var(--accent)', boxShadow: '0 6px 24px var(--accent-glow)' }}
            onClick={() => {
              tap()
              togglePlay()
            }}
            aria-label={isPlaying ? t('player.pause') : t('player.play')}
          >
            {isPlaying ? <Pause size={28} fill="currentColor" /> : <Play size={28} fill="currentColor" className="ml-1" />}
          </button>
          <button
            className="icon-btn pressable h-14 w-14"
            onClick={() => {
              tap()
              next()
            }}
            aria-label={t('player.next')}
          >
            <SkipForward size={26} fill="currentColor" />
          </button>
          <button
            className="icon-btn pressable h-12 w-12"
            data-active={repeat !== 'off'}
            onClick={() => {
              select()
              cycleRepeat()
            }}
            aria-label={t('player.repeat')}
            aria-pressed={repeat !== 'off'}
          >
            <RepeatIcon size={20} />
          </button>
        </div>

        {/* Volume + mute (mobile). ExoPlayer's setVolume is an attenuation on top
            of the system volume — it can lower but not boost past the OS level, so
            this is a fine trim; the hardware keys stay the primary control. */}
        {track && (
          <div className="np-volume flex items-center gap-3 px-6 pt-3">
            <button
              className="icon-btn pressable h-10 w-10 shrink-0"
              onClick={() => {
                select()
                toggleMute()
              }}
              aria-label={t('player.mute')}
              data-active={muted}
            >
              <VolIcon size={20} />
            </button>
            <input
              type="range"
              min={0}
              max={100}
              value={muted ? 0 : Math.round(volume * 100)}
              onChange={(e) => setVolume(Number(e.target.value) / 100)}
              className="np-vol-slider flex-1"
              aria-label={t('player.volume')}
              style={
                {
                  background: `linear-gradient(to right, var(--accent) ${muted ? 0 : volume * 100}%, rgba(255,255,255,0.15) ${muted ? 0 : volume * 100}%)`,
                  // livello esposto come token: le skin ridisegnano la barra
                  // (es. celle RAM cyberpunk) senza perdere il fill
                  '--vol-pct': `${muted ? 0 : volume * 100}%`
                } as React.CSSProperties
              }
            />
            <style>{`
              .np-vol-slider {
                -webkit-appearance: none;
                appearance: none;
                height: 4px;
                border-radius: 2px;
                outline: none;
              }
              .np-vol-slider::-webkit-slider-thumb {
                -webkit-appearance: none;
                appearance: none;
                width: 16px;
                height: 16px;
                border-radius: 50%;
                background: white;
                box-shadow: 0 0 8px var(--accent-glow);
              }
              html[data-mobile] .np-vol-slider {
                height: 6px;
                border-radius: 3px;
              }
              html[data-mobile] .np-vol-slider::-webkit-slider-thumb {
                width: 24px;
                height: 24px;
              }
            `}</style>
          </div>
        )}
      </div>
    </div>
  )
}
