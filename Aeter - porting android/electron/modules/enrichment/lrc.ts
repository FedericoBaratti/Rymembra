// LRC lyrics parsing. Pure module, no electron imports.

export interface LrcLine {
  time: number
  text: string
}

const META_TAG = /^\[(ti|ar|al|au|by|offset|length|re|ve|tool):(.*)\]$/i
const TIMESTAMP = /\[(\d+):(\d{1,2}(?:\.\d{1,3})?)\]/g

/**
 * Parses LRC text into sorted, offset-adjusted lines. Supports multiple
 * timestamps per line ("[00:12.00][00:50.00]chorus") and the [offset:±ms]
 * tag (positive offset shifts lyrics earlier, per the LRC convention).
 * Returns null when fewer than 3 timed lines are found (matches the
 * previous behavior: such payloads are treated as plain text).
 */
export function parseLrc(lrc: string): LrcLine[] | null {
  const lines: LrcLine[] = []
  let offsetMs = 0

  for (const raw of lrc.split(/\r?\n/)) {
    const line = raw.trim()
    if (!line) continue

    const meta = META_TAG.exec(line)
    if (meta) {
      if (meta[1].toLowerCase() === 'offset') {
        const v = Number(meta[2].trim())
        if (Number.isFinite(v)) offsetMs = v
      }
      continue
    }

    TIMESTAMP.lastIndex = 0
    const times: number[] = []
    let cursor = 0
    let m: RegExpExecArray | null
    while ((m = TIMESTAMP.exec(line))) {
      // timestamps must form a contiguous prefix of the line
      if (m.index !== cursor) break
      times.push(Number(m[1]) * 60 + Number(m[2]))
      cursor = TIMESTAMP.lastIndex
    }
    if (times.length === 0) continue
    const text = line.slice(cursor).trim()
    for (const t of times) lines.push({ time: t, text })
  }

  if (lines.length <= 2) return null
  const shift = offsetMs / 1000
  for (const l of lines) l.time = Math.max(0, l.time - shift)
  lines.sort((a, b) => a.time - b.time)
  return lines
}
