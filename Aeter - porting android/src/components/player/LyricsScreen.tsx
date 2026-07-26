import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, Pencil, Play, Pause, SkipBack, SkipForward, Music2 } from 'lucide-react'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { coverUrl } from '@/lib/format'
import { tap, select } from '@/lib/haptics'
import LyricsView from './LyricsView'
import type { LyricsResult } from '@shared/types'

/**
 * Mobile-only full-screen lyrics surface, opened from the Now Playing tools row.
 * On Android the audio runs through native ExoPlayer with no Web Audio graph, so
 * the desktop CircularSpectrum visualizer would be inert; this surface delivers
 * the valuable part that lived inside it — synced/plain lyrics that follow the
 * track — over a blurred-cover backdrop. Rendered (gated isMobile) from App.tsx
 * and shown when useUiStore.lyricsOpen is true.
 */
export default function LyricsScreen(): React.JSX.Element | null {
  const { t } = useTranslation()
  const open = useUiStore((s) => s.lyricsOpen)
  const setOpen = useUiStore((s) => s.setLyricsOpen)
  const setLyricsEditTrackId = useUiStore((s) => s.setLyricsEditTrackId)
  const track = usePlayerStore((s) => s.currentTrack)
  const isPlaying = usePlayerStore((s) => s.isPlaying)
  const togglePlay = usePlayerStore((s) => s.togglePlay)
  const next = usePlayerStore((s) => s.next)
  const previous = usePlayerStore((s) => s.previous)
  const [lyrics, setLyrics] = useState<LyricsResult | null>(null)
  const [loading, setLoading] = useState(false)

  const trackId = track?.id
  // Fetch when the surface is open and the track changes; skip while closed so we
  // don't hit the DB/network for lyrics nobody is looking at.
  useEffect(() => {
    if (!open || trackId == null) return
    let cancelled = false
    setLyrics(null)
    setLoading(true)
    window.aether
      .getLyrics(trackId)
      .then((l) => {
        if (!cancelled) setLyrics(l)
      })
      .catch(() => {
        if (!cancelled) setLyrics(null)
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [open, trackId])

  if (!open) return null

  // Thumb (64px) upscalata per il backdrop: già sfocata dall'upscale, basta un
  // blur piccolo — blur(80px) a schermo intero costava troppo sulla GPU mobile.
  const cover = coverUrl(track?.cover_art_hash, true)
  const hasLyrics = !!lyrics && (!!lyrics.synced?.length || !!lyrics.plain)

  const fetchOnline = (): void => {
    if (trackId == null) return
    select()
    setLoading(true)
    window.aether
      .refetchLyrics(trackId)
      .then((l) => setLyrics(l))
      .catch(() => undefined)
      .finally(() => setLoading(false))
  }

  return (
    <div
      className="lyrics-screen overlay-in fixed inset-0 z-[62] flex flex-col overflow-hidden bg-black"
      data-playing={isPlaying}
      style={{
        paddingTop: 'calc(var(--sa-top, env(safe-area-inset-top, 0px)) + 8px)',
        paddingBottom: 'calc(var(--sa-bottom, env(safe-area-inset-bottom, 0px)) + 16px)'
      }}
    >
      {/* blurred cover backdrop */}
      {cover && (
        <div
          className="absolute inset-0 scale-125 bg-cover bg-center"
          style={{ backgroundImage: `url(${cover})`, filter: 'blur(24px) brightness(0.4)' }}
          aria-hidden
        />
      )}
      <div
        className="absolute inset-0"
        style={{
          background: 'radial-gradient(ellipse at center, rgba(0,0,0,0.35) 0%, rgba(0,0,0,0.82) 100%)'
        }}
        aria-hidden
      />

      {/* header: close + track + edit */}
      <div className="relative z-10 flex items-center justify-between px-4 py-2">
        <button
          className="icon-btn pressable h-12 w-12 text-white/85"
          onClick={() => {
            select()
            setOpen(false)
          }}
          aria-label={t('common.close')}
        >
          <ChevronDown size={24} />
        </button>
        <div className="min-w-0 flex-1 px-2 text-center">
          <div className="truncate text-[14px] font-semibold text-white">{track?.title}</div>
          <div className="truncate text-[12px] text-white/55">{track?.artist}</div>
        </div>
        {track && (
          <button
            className="icon-btn pressable h-12 w-12 text-white/85"
            onClick={() => {
              // The mobile lyrics editor is a FullScreenSheet at z=50 (below this
              // surface at z-62), so close this first to avoid it rendering behind.
              select()
              setLyricsEditTrackId(track.id)
              setOpen(false)
            }}
            aria-label={t('lyrics.edit')}
          >
            <Pencil size={18} />
          </button>
        )}
      </div>

      {/* lyrics / empty / loading */}
      <div className="relative z-10 flex min-h-0 flex-1 items-center px-7">
        {hasLyrics && lyrics ? (
          <LyricsView key={trackId} lyrics={lyrics} className="max-h-full" />
        ) : (
          <div className="mx-auto flex w-full flex-col items-center gap-4 text-center">
            {loading ? (
              <div className="text-[15px] text-white/60">{t('lyrics.fetching')}</div>
            ) : (
              <>
                <Music2 size={40} className="text-white/25" />
                <div className="text-[15px] text-white/60">{t('lyrics.empty')}</div>
                {track && (
                  <button
                    className="pressable rounded-full px-5 py-2.5 text-[14px] font-semibold text-white"
                    style={{ background: 'var(--accent)', boxShadow: '0 4px 20px var(--accent-glow)' }}
                    onClick={fetchOnline}
                  >
                    {t('lyrics.fetch')}
                  </button>
                )}
              </>
            )}
          </div>
        )}
      </div>

      {/* minimal transport */}
      <div className="relative z-10 mx-auto flex w-full max-w-[420px] items-center justify-center gap-6 px-6 pt-4">
        <button
          className="icon-btn pressable h-12 w-12 text-white/85"
          onClick={() => {
            tap()
            previous()
          }}
          aria-label={t('player.previous')}
        >
          <SkipBack size={24} fill="currentColor" />
        </button>
        <button
          className="play-btn-primary pressable flex h-14 w-14 items-center justify-center rounded-full text-white"
          style={{ background: 'var(--accent)', boxShadow: '0 6px 24px var(--accent-glow)' }}
          onClick={() => {
            tap()
            togglePlay()
          }}
          aria-label={isPlaying ? t('player.pause') : t('player.play')}
        >
          {isPlaying ? (
            <Pause size={26} fill="currentColor" />
          ) : (
            <Play size={26} fill="currentColor" className="ml-1" />
          )}
        </button>
        <button
          className="icon-btn pressable h-12 w-12 text-white/85"
          onClick={() => {
            tap()
            next()
          }}
          aria-label={t('player.next')}
        >
          <SkipForward size={24} fill="currentColor" />
        </button>
      </div>
    </div>
  )
}
