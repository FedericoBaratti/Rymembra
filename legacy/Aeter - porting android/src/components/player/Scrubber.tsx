import { useEffect, useRef, useState, useCallback } from 'react'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useWaveform } from '@/hooks/useWaveform'
import { playerEngine } from '@/lib/player'
import { formatDuration } from '@/lib/format'
import { cssToken } from '@/lib/cssTokens'
import { tap, select as hapticSelect } from '@/lib/haptics'

/** Custom scrubber with a miniature waveform, hover timestamp and dragging. */
export default function Scrubber(): React.JSX.Element {
  const currentTrack = usePlayerStore((s) => s.currentTrack)
  const peaks = useWaveform(currentTrack?.id ?? null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const wrapRef = useRef<HTMLDivElement>(null)
  const [hoverX, setHoverX] = useState<number | null>(null)
  const [dragging, setDragging] = useState(false)
  const dragFrac = useRef(0)
  const [position, setPosition] = useState(0)
  const isPlaying = usePlayerStore((s) => s.isPlaying)
  const duration = currentTrack?.duration ?? 0

  // Poll playback position at display rate — but only while audible or being
  // dragged (mirrors PlayerBar's mobile hairline): a paused player doesn't need
  // 60 wake-ups per second. The one-shot sync keeps a paused seek accurate.
  useEffect(() => {
    if (!dragging) setPosition(playerEngine.position())
    if (!isPlaying && !dragging) return
    let raf = 0
    let lastUpdate = 0
    const tick = (now: number): void => {
      // ~10 fps is visually indistinguishable for a playhead moving a few px/s
      // and cuts the React re-render cost of the open sheet ~6x. Dragging
      // bypasses this loop entirely (onPointerMove calls draw() directly), so
      // the thumb still tracks the finger at full display rate.
      if (!dragging && now - lastUpdate >= 100) {
        lastUpdate = now
        setPosition(playerEngine.position())
      }
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [dragging, isPlaying])

  const draw = useCallback(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const dpr = window.devicePixelRatio || 1
    const w = canvas.clientWidth
    const h = canvas.clientHeight
    if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
      canvas.width = w * dpr
      canvas.height = h * dpr
    }
    const ctx = canvas.getContext('2d')!
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, w, h)

    const dur = playerEngine.duration() || duration || 1
    const frac = dragging ? dragFrac.current : Math.min(1, position / dur)
    // Playhead glow via token (--scrubber-glow, px): il canvas non è
    // raggiungibile dal CSS, quindi le skin piatte lo azzerano da global.css.
    // Onda non riprodotta via token (--scrubber-rest): le skin la ricolorano.
    // cssToken cachea le letture tra un cambio skin/accent e l'altro.
    const accent = cssToken('--accent')
    const glow = parseFloat(cssToken('--scrubber-glow'))
    const rest = cssToken('--scrubber-rest') || 'rgba(255,255,255,0.18)'

    if (peaks && peaks.length > 0) {
      const n = peaks.length
      const step = w / n
      const mid = h / 2
      for (let i = 0; i < n; i++) {
        const ph = Math.max(1, peaks[i] * (h - 4))
        const x = i * step
        const played = i / n <= frac
        ctx.fillStyle = played ? accent : rest
        ctx.globalAlpha = played ? 0.95 : 1
        ctx.fillRect(x, mid - ph / 2, Math.max(1, step * 0.7), ph)
      }
      ctx.globalAlpha = 1
    } else {
      // plain progress bar fallback while waveform loads
      const mid = h / 2
      ctx.fillStyle = rest
      ctx.fillRect(0, mid - 2, w, 4)
      ctx.fillStyle = accent
      ctx.fillRect(0, mid - 2, w * frac, 4)
    }

    // playhead
    ctx.fillStyle = accent
    ctx.shadowColor = accent
    ctx.shadowBlur = Number.isFinite(glow) ? glow : 6
    ctx.fillRect(w * frac - 1, 2, 2, h - 4)
    ctx.shadowBlur = 0
  }, [peaks, position, dragging, duration])

  useEffect(() => {
    draw()
  }, [draw])

  const fracFromEvent = (clientX: number): number => {
    const rect = wrapRef.current!.getBoundingClientRect()
    return Math.max(0, Math.min(1, (clientX - rect.left) / rect.width))
  }

  const onPointerDown = (e: React.PointerEvent): void => {
    if (!currentTrack) return
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    dragFrac.current = fracFromEvent(e.clientX)
    hapticSelect()
    setDragging(true)
  }

  const onPointerMove = (e: React.PointerEvent): void => {
    setHoverX(e.clientX - wrapRef.current!.getBoundingClientRect().left)
    if (dragging) {
      dragFrac.current = fracFromEvent(e.clientX)
      draw()
    }
  }

  const onPointerUp = (e: React.PointerEvent): void => {
    if (!dragging) return
    const frac = fracFromEvent(e.clientX)
    const dur = playerEngine.duration() || duration
    playerEngine.seek(frac * dur)
    setPosition(frac * dur)
    tap()
    setDragging(false)
  }

  // The OS can steal the pointer mid-drag (back gesture, notification shade):
  // pointercancel fires instead of pointerup — abort without seeking.
  const onPointerCancel = (): void => {
    setDragging(false)
    setHoverX(null)
  }

  const hoverTime =
    hoverX != null && wrapRef.current
      ? (hoverX / wrapRef.current.clientWidth) * (playerEngine.duration() || duration)
      : null

  // While dragging the bubble follows the drag position (touch has no hover):
  // essential on mobile where the finger covers the playhead.
  const bubbleX =
    dragging && wrapRef.current ? dragFrac.current * wrapRef.current.clientWidth : hoverX
  const bubbleTime = dragging
    ? dragFrac.current * (playerEngine.duration() || duration)
    : hoverTime

  return (
    <div className="scrub-row flex w-full items-center gap-2.5">
      <span className="tnum w-10 text-right text-[11px] text-text-3">
        {formatDuration(dragging ? dragFrac.current * duration : position)}
      </span>
      <div
        ref={wrapRef}
        className="scrub-wrap group relative h-9 flex-1 cursor-pointer touch-none"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerCancel}
        onPointerLeave={() => setHoverX(null)}
      >
        <canvas ref={canvasRef} className="h-full w-full" aria-hidden />
        {bubbleTime != null && bubbleX != null && (
          <div
            className={`tnum pointer-events-none absolute z-10 -translate-x-1/2 rounded-md bg-surface-3 text-text-1 shadow-lg ${
              dragging
                ? '-top-8 px-2 py-1 text-[12px]'
                : '-top-6 px-1.5 py-0.5 text-[10px] opacity-0 transition-opacity duration-100 group-hover:opacity-100'
            }`}
            style={{ left: bubbleX }}
          >
            {formatDuration(bubbleTime)}
          </div>
        )}
      </div>
      <span className="tnum w-10 text-[11px] text-text-3">{formatDuration(duration)}</span>
    </div>
  )
}
