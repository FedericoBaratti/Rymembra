import { spawn } from 'node:child_process'

export interface SpawnResult {
  code: number | null
  stdout: string
  stderr: string
}

export class SpawnTimeoutError extends Error {
  readonly code = 'ETIMEDOUT'
  constructor(cmd: string, timeoutMs: number) {
    super(`${cmd} non ha risposto entro ${Math.round(timeoutMs / 1000)} secondi`)
  }
}

export function killTree(pid: number): void {
  if (process.platform === 'win32') {
    spawn('taskkill', ['/pid', String(pid), '/T', '/F']).on('error', () => {})
  } else {
    try {
      process.kill(pid, 'SIGTERM')
      setTimeout(() => {
        try {
          process.kill(pid, 'SIGKILL')
        } catch {
          // already exited
        }
      }, 2000).unref()
    } catch {
      // already exited
    }
  }
}

/**
 * Spawns a short-lived process collecting stdout/stderr. Resolves on exit
 * (any code — the caller inspects `code`); rejects on spawn failure or when
 * the timeout expires, killing the process tree.
 */
export function spawnWithTimeout(
  cmd: string,
  args: string[],
  opts?: { timeoutMs?: number }
): Promise<SpawnResult> {
  const timeoutMs = opts?.timeoutMs ?? 30_000
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args)
    let out = ''
    let err = ''
    let settled = false

    const timer = setTimeout(() => {
      if (settled) return
      settled = true
      if (child.pid) killTree(child.pid)
      reject(new SpawnTimeoutError(cmd, timeoutMs))
    }, timeoutMs)

    child.stdout.on('data', (d) => (out += d))
    child.stderr.on('data', (d) => (err += d))
    child.on('error', (e) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      reject(e)
    })
    child.on('close', (code) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolve({ code, stdout: out, stderr: err })
    })
  })
}
