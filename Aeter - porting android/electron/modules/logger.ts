import { app } from 'electron'
import { appendFile, rename } from 'node:fs/promises'
import { mkdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

const MAX_LOG_BYTES = 1024 * 1024
const SIZE_CHECK_EVERY = 50

let logDir: string | null = null
let writesSinceCheck = 0

function logFile(): string | null {
  if (!logDir) {
    try {
      logDir = join(app.getPath('userData'), 'logs')
      mkdirSync(logDir, { recursive: true })
    } catch {
      return null
    }
  }
  return join(logDir, 'main.log')
}

function formatErr(err: unknown): string {
  if (err == null) return ''
  if (err instanceof Error) return `: ${err.message}`
  return `: ${String(err)}`
}

async function rotateIfNeeded(file: string): Promise<void> {
  writesSinceCheck++
  if (writesSinceCheck < SIZE_CHECK_EVERY) return
  writesSinceCheck = 0
  try {
    if (statSync(file).size > MAX_LOG_BYTES) await rename(file, `${file}.1`)
  } catch {
    // file may not exist yet or rotation may race; either way appendFile recreates it
  }
}

function write(level: 'WARN' | 'ERROR', scope: string, message: string, err?: unknown): void {
  const line = `[${new Date().toISOString()}] [${level}] [${scope}] ${message}${formatErr(err)}`
  if (level === 'ERROR') console.error(line)
  else console.warn(line)
  const file = logFile()
  if (!file) return
  void rotateIfNeeded(file)
    .then(() => appendFile(file, line + '\n', 'utf-8'))
    .catch(() => {})
}

export function logWarn(scope: string, message: string, err?: unknown): void {
  write('WARN', scope, message, err)
}

export function logError(scope: string, message: string, err?: unknown): void {
  write('ERROR', scope, message, err)
}
