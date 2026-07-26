import { spawn } from 'node:child_process'
import { killTree } from '../spawn'

export interface SpawnLinesResult {
  code: number | null
  stderr: string
  aborted: boolean
}

/**
 * Spawns a long-running tool streaming line-buffered stdout/stderr to
 * `onLine`. The AbortSignal kills the whole process tree (used for user
 * cancellation). Resolves on exit with the code; rejects only on spawn
 * failure (missing binary).
 */
export function spawnLines(
  cmd: string,
  args: string[],
  opts: {
    signal: AbortSignal
    onLine: (line: string, stream: 'stdout' | 'stderr') => void
  }
): Promise<SpawnLinesResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, { windowsHide: true })
    let stderr = ''
    let bufOut = ''
    let bufErr = ''

    const onAbort = (): void => {
      if (child.pid) killTree(child.pid)
    }
    if (opts.signal.aborted) onAbort()
    else opts.signal.addEventListener('abort', onAbort, { once: true })

    const feed = (chunk: string, stream: 'stdout' | 'stderr'): void => {
      if (stream === 'stdout') bufOut += chunk
      else bufErr += chunk
      const buf = stream === 'stdout' ? bufOut : bufErr
      const lines = buf.split('\n')
      const rest = lines.pop() ?? ''
      if (stream === 'stdout') bufOut = rest
      else bufErr = rest
      for (const line of lines) opts.onLine(line, stream)
    }

    child.stdout.on('data', (d: Buffer) => feed(d.toString(), 'stdout'))
    child.stderr.on('data', (d: Buffer) => {
      stderr += d.toString()
      feed(d.toString(), 'stderr')
    })
    child.on('error', (err) => {
      opts.signal.removeEventListener('abort', onAbort)
      reject(err)
    })
    child.on('close', (code) => {
      opts.signal.removeEventListener('abort', onAbort)
      if (bufOut) opts.onLine(bufOut, 'stdout')
      if (bufErr) opts.onLine(bufErr, 'stderr')
      resolve({ code, stderr, aborted: opts.signal.aborted })
    })
  })
}
