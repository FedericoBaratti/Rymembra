import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Minimize2, Play, Pause, SkipBack, SkipForward, MicVocal, Pencil } from 'lucide-react'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { audioGraph } from '@/lib/audio'
import { playerEngine } from '@/lib/player'
import { coverUrl, formatDuration } from '@/lib/format'
import { cssToken } from '@/lib/cssTokens'
import LyricsView from '@/components/player/LyricsView'
import type { LyricsResult } from '@shared/types'

const BAR_COUNT = 360
// horizontal center of the spectrum as a fraction of width: shifted left when lyrics are open
const CENTER_X_DEFAULT = 0.5
const CENTER_X_SHIFTED = 0.32

function CircularSpectrum({
  coverSrc,
  shifted,
  isPlaying
}: {
  coverSrc: string | null
  shifted: boolean
  isPlaying: boolean
}): React.JSX.Element {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const imgRef = useRef<HTMLImageElement | null>(null)
  const shiftedRef = useRef(shifted)
  shiftedRef.current = shifted
  const isPlayingRef = useRef(isPlaying)
  // The loop parks itself when there is nothing to animate (paused + ring
  // settled, or tab hidden); these effects kick it awake again.
  const scheduleRef = useRef<(() => void) | null>(null)
  useEffect(() => {
    isPlayingRef.current = isPlaying
    scheduleRef.current?.()
  }, [isPlaying])
  useEffect(() => {
    scheduleRef.current?.()
  }, [shifted])

  useEffect(() => {
    if (!coverSrc) {
      imgRef.current = null
      return
    }
    const img = new Image()
    img.src = coverSrc
    img.onload = () => {
      imgRef.current = img
      scheduleRef.current?.()
    }
  }, [coverSrc])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    let raf = 0
    let queued = false
    let data: Uint8Array<ArrayBuffer> | null = null
    const smoothed = new Float32Array(BAR_COUNT)
    let cxFrac = shiftedRef.current ? CENTER_X_SHIFTED : CENTER_X_DEFAULT

    const schedule = (): void => {
      if (queued) return
      queued = true
      raf = requestAnimationFrame(render)
    }

    const render = (): void => {
      queued = false
      const ctx = canvas.getContext('2d')
      if (!ctx) return
      // Keep animating only while audible or while the ring centre is still
      // easing; a paused/hidden visualizer renders one static frame and stops.
      const settling =
        Math.abs((shiftedRef.current ? CENTER_X_SHIFTED : CENTER_X_DEFAULT) - cxFrac) > 0.002
      if ((isPlayingRef.current || settling) && !document.hidden) schedule()
      const dpr = window.devicePixelRatio || 1
      const w = canvas.clientWidth
      const h = canvas.clientHeight
      if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
        canvas.width = w * dpr
        canvas.height = h * dpr
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      ctx.clearRect(0, 0, w, h)

      // ease the center towards its target so toggling lyrics slides the ring
      const cxTarget = shiftedRef.current ? CENTER_X_SHIFTED : CENTER_X_DEFAULT
      cxFrac += (cxTarget - cxFrac) * 0.08
      const cx = w * cxFrac
      const cy = h / 2
      const coverRadius = Math.min(w, h) * 0.21
      const innerR = coverRadius + 26
      const maxLen = Math.min(w, h) * 0.16

      const analyser = audioGraph.analyser
      let bass = 0
      if (analyser) {
        if (!data || data.length !== analyser.frequencyBinCount) {
          data = new Uint8Array(analyser.frequencyBinCount)
        }
        analyser.getByteFrequencyData(data)
        for (let i = 0; i < 24; i++) bass += data[i]
        bass = bass / 24 / 255
      }

      // Visualizer identity comes from skinnable tokens: --viz-primary drives
      // the spectrum bars, --viz-secondary the bass ring, --viz-glow its halo.
      // They default to --accent in global.css so unskinned themes look the
      // same. cssToken caches the reads between skin/accent changes.
      const secondary = cssToken('--viz-secondary') || cssToken('--accent')
      const glowBase = parseFloat(cssToken('--viz-glow')) || 20
      const primaryRgb = (cssToken('--viz-primary-rgb') || cssToken('--accent-rgb'))
        .split(' ')
        .map(Number)
      const [ar, ag, ab] = primaryRgb.length === 3 ? primaryRgb : [139, 124, 246]
      const secondaryRgb = (cssToken('--viz-secondary-rgb') || cssToken('--accent-rgb'))
        .split(' ')
        .map(Number)
      const [sr, sg, sb] = secondaryRgb.length === 3 ? secondaryRgb : [ar, ag, ab]

      // bars
      for (let i = 0; i < BAR_COUNT; i++) {
        let v = 0
        if (data) {
          // mirror the spectrum left/right for symmetry
          const half = i < BAR_COUNT / 2 ? i : BAR_COUNT - 1 - i
          const t = half / (BAR_COUNT / 2)
          const idx = Math.min(data.length - 1, Math.floor(Math.pow(t, 1.6) * data.length * 0.55))
          v = data[idx] / 255
        }
        smoothed[i] = smoothed[i] * 0.72 + v * 0.28
        const len = 2 + smoothed[i] * maxLen
        const angle = (i / BAR_COUNT) * Math.PI * 2 - Math.PI / 2
        const cos = Math.cos(angle)
        const sin = Math.sin(angle)

        const x0 = cx + cos * innerR
        const y0 = cy + sin * innerR
        const x1 = cx + cos * (innerR + len)
        const y1 = cy + sin * (innerR + len)

        const alpha = 0.25 + smoothed[i] * 0.75
        const light = Math.round(120 + smoothed[i] * 135)
        ctx.strokeStyle = `rgba(${Math.min(255, ar + light - 120)}, ${Math.min(255, ag + light - 120)}, ${Math.min(255, ab + light - 120)}, ${alpha})`
        ctx.lineWidth = 2.2
        ctx.lineCap = 'round'
        ctx.beginPath()
        ctx.moveTo(x0, y0)
        ctx.lineTo(x1, y1)
        ctx.stroke()
      }

      // glow ring
      ctx.beginPath()
      ctx.arc(cx, cy, innerR - 10, 0, Math.PI * 2)
      ctx.strokeStyle = `rgba(${sr}, ${sg}, ${sb}, ${0.35 + bass * 0.4})`
      ctx.lineWidth = 1.5
      ctx.shadowColor = secondary
      ctx.shadowBlur = glowBase + bass * 40
      ctx.stroke()
      ctx.shadowBlur = 0

      // cover art pulsing with the bass
      const img = imgRef.current
      const r = coverRadius * (1 + bass * 0.035)
      ctx.save()
      ctx.beginPath()
      ctx.arc(cx, cy, r, 0, Math.PI * 2)
      ctx.clip()
      if (img) {
        ctx.drawImage(img, cx - r, cy - r, r * 2, r * 2)
      } else {
        ctx.fillStyle = `rgba(${ar}, ${ag}, ${ab}, 0.25)`
        ctx.fillRect(cx - r, cy - r, r * 2, r * 2)
      }
      ctx.restore()
    }

    scheduleRef.current = schedule
    const onVisibility = (): void => {
      if (!document.hidden) schedule()
    }
    document.addEventListener('visibilitychange', onVisibility)
    schedule()
    return () => {
      document.removeEventListener('visibilitychange', onVisibility)
      scheduleRef.current = null
      cancelAnimationFrame(raf)
    }
  }, [])

  return <canvas ref={canvasRef} className="absolute inset-0 h-full w-full" aria-hidden />
}

function LyricsPanel({ lyrics }: { lyrics: LyricsResult }): React.JSX.Element {
  // The synced list + auto-centre + seek-on-tap behaviour lives in the reusable
  // LyricsView; this panel just positions it within the fullscreen layout.
  return (
    <div className="pointer-events-none absolute bottom-44 right-0 top-16 z-10 flex w-[38%] items-center pr-12">
      <LyricsView lyrics={lyrics} />
    </div>
  )
}

export default function FullscreenVisualizer(): React.JSX.Element | null {
  const { t } = useTranslation()
  const setFullscreenViz = useUiStore((s) => s.setFullscreenViz)
  const setLyricsEditTrackId = useUiStore((s) => s.setLyricsEditTrackId)
  const track = usePlayerStore((s) => s.currentTrack)
  const isPlaying = usePlayerStore((s) => s.isPlaying)
  const togglePlay = usePlayerStore((s) => s.togglePlay)
  const next = usePlayerStore((s) => s.next)
  const previous = usePlayerStore((s) => s.previous)
  const [controlsVisible, setControlsVisible] = useState(true)
  const [showLyrics, setShowLyrics] = useState(true)
  const [lyrics, setLyrics] = useState<LyricsResult | null>(null)
  const hideTimer = useRef<number | null>(null)
  const [position, setPosition] = useState(0)

  const poke = useCallback(() => {
    setControlsVisible(true)
    if (hideTimer.current) window.clearTimeout(hideTimer.current)
    hideTimer.current = window.setTimeout(() => setControlsVisible(false), 3000)
  }, [])

  useEffect(() => {
    poke()
    return () => {
      if (hideTimer.current) window.clearTimeout(hideTimer.current)
    }
  }, [poke])

  useEffect(() => {
    setPosition(playerEngine.position())
    if (!isPlaying) return
    const iv = window.setInterval(() => setPosition(playerEngine.position()), 500)
    return () => window.clearInterval(iv)
  }, [isPlaying])

  const trackId = track?.id
  useEffect(() => {
    if (trackId == null) return
    let cancelled = false
    setLyrics(null)
    window.aether
      .getLyrics(trackId)
      .then((l) => {
        if (!cancelled) setLyrics(l)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [trackId])

  const hasLyrics = useMemo(
    () => !!lyrics && (!!lyrics.synced?.length || !!lyrics.plain),
    [lyrics]
  )
  const lyricsVisible = showLyrics && hasLyrics

  if (!track) return null
  const cover = coverUrl(track.cover_art_hash)

  return (
    <div
      className="viz-screen fixed inset-0 z-50 overflow-hidden bg-black"
      data-playing={isPlaying}
      onMouseMove={poke}
      onDoubleClick={() => setFullscreenViz(false)}
      style={{ cursor: controlsVisible ? 'default' : 'none' }}
    >
      {/* blurred cover backdrop */}
      {cover && (
        <div
          className="absolute inset-0 scale-125 bg-cover bg-center"
          style={{ backgroundImage: `url(${cover})`, filter: 'blur(80px) brightness(0.45)' }}
        />
      )}
      <div
        className="absolute inset-0"
        style={{
          background:
            'radial-gradient(ellipse at center, rgba(0,0,0,0.25) 0%, rgba(0,0,0,0.75) 100%)'
        }}
      />

      <CircularSpectrum coverSrc={cover} shifted={lyricsVisible} isPlaying={isPlaying} />
      {lyricsVisible && lyrics && <LyricsPanel key={track.id} lyrics={lyrics} />}

      {/* overlay controls */}
      <div
        className={`viz-controls absolute inset-x-0 bottom-0 z-20 flex flex-col items-center gap-3 pb-8 pt-20 transition-opacity duration-500 ${
          controlsVisible ? 'opacity-100' : 'pointer-events-none opacity-0'
        }`}
        style={{ background: 'linear-gradient(transparent, rgba(0,0,0,0.7))' }}
      >
        <div className="max-w-[70%] text-center">
          <div className="viz-title truncate text-[20px] font-bold text-white" data-text={track.title}>
            {track.title}
          </div>
          <div className="viz-sub truncate text-[14px] text-white/60">
            {track.artist} — {track.album}
          </div>
        </div>
        <div className="flex items-center gap-3">
          <button
            className="icon-btn h-10 w-10 text-white/80"
            onClick={previous}
            aria-label={t('player.previous')}
          >
            <SkipBack size={20} fill="currentColor" />
          </button>
          <button
            className="play-btn-primary flex h-14 w-14 items-center justify-center rounded-full text-white transition-transform hover:scale-105 active:scale-95"
            style={{ background: 'var(--accent)', boxShadow: '0 4px 30px var(--accent-glow)' }}
            onClick={togglePlay}
            aria-label={isPlaying ? t('player.pause') : t('player.play')}
          >
            {isPlaying ? (
              <Pause size={24} fill="currentColor" />
            ) : (
              <Play size={24} fill="currentColor" className="ml-0.5" />
            )}
          </button>
          <button
            className="icon-btn h-10 w-10 text-white/80"
            onClick={() => next()}
            aria-label={t('player.next')}
          >
            <SkipForward size={20} fill="currentColor" />
          </button>
        </div>
        <div className="tnum text-[12px] text-white/50">
          {formatDuration(position)} / {formatDuration(track.duration)}
        </div>
      </div>

      <div
        className={`viz-topbar absolute right-5 top-5 z-20 flex gap-2 transition-opacity duration-500 ${
          controlsVisible ? 'opacity-100' : 'pointer-events-none opacity-0'
        }`}
      >
        {hasLyrics && (
          <button
            className="icon-btn h-10 w-10 bg-black/30 text-white/80 backdrop-blur"
            data-active={showLyrics}
            onClick={() => setShowLyrics(!showLyrics)}
            title={t('lyrics.title')}
            aria-label={t('lyrics.title')}
          >
            <MicVocal size={18} />
          </button>
        )}
        <button
          className="icon-btn h-10 w-10 bg-black/30 text-white/80 backdrop-blur"
          onClick={() => setLyricsEditTrackId(track.id)}
          title={t('lyrics_editor.title')}
        >
          <Pencil size={17} />
        </button>
        <button
          className="icon-btn h-10 w-10 bg-black/30 text-white/80 backdrop-blur"
          onClick={() => setFullscreenViz(false)}
          title={`${t('common.close')} (Esc)`}
        >
          <Minimize2 size={18} />
        </button>
      </div>
    </div>
  )
}
