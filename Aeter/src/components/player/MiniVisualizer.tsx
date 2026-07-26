import { useEffect, useRef } from 'react'
import { audioGraph } from '@/lib/audio'
import { usePlayerStore } from '@/store/usePlayerStore'
import { cssToken } from '@/lib/cssTokens'

const BIN_COUNT = 48

/** Real-time spectrum bars in the player bar (AnalyserNode FFT → canvas, 60fps). */
export default function MiniVisualizer(): React.JSX.Element {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const isPlaying = usePlayerStore((s) => s.isPlaying)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    let raf = 0
    let data: Uint8Array<ArrayBuffer> | null = null

    const render = (): void => {
      raf = requestAnimationFrame(render)
      const analyser = audioGraph.analyser
      const ctx = canvas.getContext('2d')
      if (!ctx) return
      const dpr = window.devicePixelRatio || 1
      const w = canvas.clientWidth
      const h = canvas.clientHeight
      if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
        canvas.width = w * dpr
        canvas.height = h * dpr
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      ctx.clearRect(0, 0, w, h)
      if (!analyser) return

      if (!data || data.length !== analyser.frequencyBinCount) {
        data = new Uint8Array(analyser.frequencyBinCount)
      }
      analyser.getByteFrequencyData(data)

      const accent = cssToken('--viz-primary') || cssToken('--accent')
      const barW = w / BIN_COUNT
      ctx.fillStyle = accent

      for (let i = 0; i < BIN_COUNT; i++) {
        // logarithmic-ish bin mapping for a musical spectrum
        const t = i / BIN_COUNT
        const idx = Math.min(
          data.length - 1,
          Math.floor(Math.pow(t, 1.8) * (data.length * 0.7))
        )
        const v = data[idx] / 255
        const bh = Math.max(1.5, v * h)
        ctx.globalAlpha = 0.35 + v * 0.65
        ctx.fillRect(i * barW, h - bh, Math.max(1, barW * 0.65), bh)
      }
      ctx.globalAlpha = 1
    }

    if (isPlaying) {
      render()
    } else {
      // draw one static frame, then stop
      render()
      cancelAnimationFrame(raf)
    }
    return () => cancelAnimationFrame(raf)
  }, [isPlaying])

  return <canvas ref={canvasRef} className="h-8 w-[120px] opacity-90" aria-hidden />
}
