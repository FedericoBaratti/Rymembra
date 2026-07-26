/** LRC serialization, shared between the main process and the renderer. */

export interface TimedLine {
  time: number
  text: string
}

/** Formats seconds as an LRC timestamp body, e.g. 75.3 -> "01:15.30". */
export function formatLrcTime(seconds: number): string {
  const s = Math.max(0, seconds)
  const m = Math.floor(s / 60)
  const rest = (s - m * 60).toFixed(2).padStart(5, '0')
  return `${String(m).padStart(2, '0')}:${rest}`
}

/** Serializes timed lines to LRC text sorted by time. Inverse of parseLrc. */
export function serializeLrc(lines: TimedLine[]): string {
  return [...lines]
    .sort((a, b) => a.time - b.time)
    .map((l) => `[${formatLrcTime(l.time)}]${l.text}`)
    .join('\n')
}
