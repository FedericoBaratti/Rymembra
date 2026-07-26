import { useCallback, useEffect, useRef, useState } from 'react'
import { playerEngine } from '@/lib/player'
import type { LyricsResult } from '@shared/types'

/**
 * Scrollable synced/plain lyrics list: highlights the current line (polling the
 * player position), auto-centres it, holds off auto-centring for 3s while the
 * user scrolls by hand, and seeks on line tap. Assumes a dark backdrop (white
 * text). The caller owns positioning/sizing of the surrounding container.
 *
 * Extracted from the desktop FullscreenVisualizer's LyricsPanel so the mobile
 * LyricsScreen can reuse the exact same behaviour.
 */
export default function LyricsView({
  lyrics,
  className = ''
}: {
  lyrics: LyricsResult
  className?: string
}): React.JSX.Element {
  const [activeLine, setActiveLine] = useState(-1)
  const listRef = useRef<HTMLDivElement>(null)
  // while the user is scrolling by hand, hold off the auto-centering
  const manualUntilRef = useRef(0)

  useEffect(() => {
    if (!lyrics.synced) return
    const iv = window.setInterval(() => {
      const pos = playerEngine.position()
      const synced = lyrics.synced!
      let idx = -1
      for (let i = 0; i < synced.length; i++) {
        if (synced[i].time <= pos) idx = i
        else break
      }
      setActiveLine(idx)
    }, 200)
    return () => window.clearInterval(iv)
  }, [lyrics])

  useEffect(() => {
    const list = listRef.current
    if (activeLine < 0 || !list) return
    if (Date.now() < manualUntilRef.current) return
    const el = list.children[activeLine] as HTMLElement | undefined
    if (!el) return
    // scroll only this container (scrollIntoView would also scroll ancestors)
    list.scrollTo({
      top: el.offsetTop - list.clientHeight / 2 + el.clientHeight / 2,
      behavior: 'smooth'
    })
  }, [activeLine])

  const markManual = useCallback(() => {
    manualUntilRef.current = Date.now() + 3000
  }, [])

  const seekTo = useCallback((time: number) => {
    playerEngine.seek(time)
    manualUntilRef.current = 0
  }, [])

  return (
    <div
      ref={listRef}
      onWheel={markManual}
      onTouchMove={markManual}
      className={`lyrics-list no-scrollbar pointer-events-auto relative max-h-full w-full overflow-y-auto [mask-image:linear-gradient(to_bottom,transparent,black_20%,black_80%,transparent)] ${className}`}
    >
      {lyrics.synced ? (
        lyrics.synced.map((line, i) => (
          <p
            key={i}
            onClick={() => seekTo(line.time)}
            data-state={i === activeLine ? 'active' : i < activeLine ? 'past' : 'next'}
            className={`lyric-line cursor-pointer select-none py-2 text-[19px] font-semibold leading-snug transition-[color,transform,filter] duration-300 first:pt-[40vh] last:pb-[40vh] ${
              i === activeLine
                ? 'origin-left scale-[1.06] text-white drop-shadow-[0_0_14px_var(--accent-glow)]'
                : i < activeLine
                  ? 'text-white/25 hover:text-white/60'
                  : 'text-white/40 hover:text-white/70'
            }`}
          >
            {line.text || '♪'}
          </p>
        ))
      ) : (
        <pre className="whitespace-pre-wrap py-8 font-sans text-[15px] leading-relaxed text-white/70">
          {lyrics.plain}
        </pre>
      )}
    </div>
  )
}
