import { describe, it, expect, vi } from 'vitest'
import type { ScanProgress } from '@shared/types'

// Coalescing test for scanFolders: a scan requested while one is running must
// not be dropped (a download landing mid-scan would stay un-ingested) and must
// collapse into ONE follow-up pass carrying the LATEST arguments. Everything
// around the scheduling logic is stubbed: empty folder list → no filesystem,
// no-op statements → no real DB.
vi.mock('electron', () => ({
  app: { getPath: () => 'library-scan-test-userdata-does-not-exist' }
}))
vi.mock('./db', () => {
  const stmt = { all: () => [], run: () => ({ lastInsertRowid: 1 }), get: () => undefined }
  return {
    getDb: () => ({
      prepare: () => stmt,
      exec: () => {},
      transaction: (fn: () => void) => fn
    })
  }
})
vi.mock('music-metadata', () => ({ parseFile: vi.fn() }))
vi.mock('./coverArt', () => ({ storeCover: vi.fn() }))
vi.mock('./adaptiveConcurrency', () => ({
  thermalManager: { getConcurrency: (n: number) => n, onChange: () => () => {} }
}))

import { scanFolders, isScanning } from './library'

describe('scanFolders', () => {
  it('runs start-to-finish when idle', async () => {
    const phases: ScanProgress['phase'][] = []
    await scanFolders([], (p) => phases.push(p.phase))
    expect(phases[0]).toBe('discovering')
    expect(phases[phases.length - 1]).toBe('done')
    expect(isScanning()).toBe(false)
  })

  it('coalesces scans requested mid-scan into one follow-up with the latest args', async () => {
    const finished: string[] = []
    const tag = (name: string) => (p: ScanProgress) => {
      if (p.phase === 'done') finished.push(name)
    }
    // scanFolders flips `scanning` synchronously, so both re-entrant calls
    // below observe the first scan as still running and coalesce.
    const first = scanFolders([], tag('first'))
    const dropped = scanFolders([], tag('dropped'))
    const latest = scanFolders([], tag('latest'))
    await Promise.all([first, dropped, latest])

    // Exactly ONE follow-up ran, with the latest callback — the middle request
    // was superseded, not queued up as a third pass.
    expect(finished).toEqual(['first', 'latest'])
    expect(isScanning()).toBe(false)
  })
})
