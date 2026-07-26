import { useCallback, useEffect, useRef, useState } from 'react'
import { playerEngine } from '@/lib/player'
import { usePlayerStore } from '@/store/usePlayerStore'
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
  const isPlaying = usePlayerStore((s) => s.isPlaying)
  const listRef = useRef<HTMLDivElement>(null)
  // while the user is scrolling by hand, hold off the auto-centering
  const manualUntilRef = useRef(0)

  const lineFor = useCallback(
    (pos: number): number => {
      const synced = lyrics.synced
      if (!synced) return -1
      let idx = -1
      for (let i = 0; i < synced.length; i++) {
        if (synced[i].time <= pos) idx = i
        else break
      }
      return idx
    },
    [lyrics]
  )

  // Poll the position only while audible: paused lyrics are static, so a
  // one-shot compute on open/pause keeps the highlight correct without waking
  // up 5 times a second for nothing.
  useEffect(() => {
    if (!lyrics.synced) return
    setActiveLine(lineFor(playerEngine.position()))
    if (!isPlaying) return
    const iv = window.setInterval(() => {
      setActiveLine(lineFor(playerEngine.position()))
    }, 200)
    return () => window.clearInterval(iv)
  }, [lyrics, isPlaying, lineFor])

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

  const seekTo = useCallback(
    (time: number) => {
      playerEngine.seek(time)
      manualUntilRef.current = 0
      // While paused there's no poll running: reflect the tapped line now.
      setActiveLine(lineFor(time))
    },
    [lineFor]
  )

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
