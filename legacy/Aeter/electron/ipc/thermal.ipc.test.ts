import { beforeAll, describe, expect, it, vi } from 'vitest'

// Exercises the real registerThermalIpc → handle() → thermalManager chain with
// ipcMain mocked via vi.mock('electron') (repo convention for handler tests).
// Broadcasts are observed through events.ts's real onBroadcast hook — no
// BrowserWindow needed.

const handlers = new Map<string, (event: unknown, ...args: unknown[]) => unknown>()

vi.mock('electron', () => ({
  ipcMain: {
    handle: (channel: string, fn: (event: unknown, ...args: unknown[]) => unknown) => {
      handlers.set(channel, fn)
    }
  },
  // logger.ts calls app.getPath lazily and falls back to console-only on throw.
  app: {
    getPath: () => {
      throw new Error('no userData in tests')
    }
  }
}))

async function invoke(channel: string, ...args: unknown[]): Promise<unknown> {
  const fn = handlers.get(channel)
  if (!fn) throw new Error(`handler non registrato: ${channel}`)
  return fn({}, ...args)
}

const broadcasts: { event: string; payload: unknown }[] = []

beforeAll(async () => {
  const { onBroadcast } = await import('../modules/events')
  onBroadcast((event, payload) => broadcasts.push({ event, payload }))
  const { registerThermalIpc } = await import('./thermal.ipc')
  registerThermalIpc()
})

describe('thermal.ipc', () => {
  it('registers both handlers', () => {
    expect(handlers.has('thermalUpdate')).toBe(true)
    expect(handlers.has('getThermalState')).toBe(true)
  })

  it('rejects malformed samples', async () => {
    await expect(invoke('thermalUpdate', { level: 'molto-caldo' })).rejects.toThrow()
    await expect(invoke('thermalUpdate', null)).rejects.toThrow()
    await expect(
      invoke('thermalUpdate', { level: 'warning', headroom: Infinity })
    ).rejects.toThrow()
    // Nothing reached the manager: still pristine.
    const state = (await invoke('getThermalState')) as { level: string }
    expect(state.level).toBe('normal')
  })

  it('applies valid samples and broadcasts thermal:changed on transitions only', async () => {
    const updated = (await invoke('thermalUpdate', { level: 'warning', headroom: 0.9 })) as {
      level: string
      headroom?: number
    }
    expect(updated.level).toBe('warning')
    expect(updated.headroom).toBe(0.9)

    // Same level again: state refreshed, but no second broadcast.
    await invoke('thermalUpdate', { level: 'warning' })
    await invoke('thermalUpdate', { level: 'critical' })

    const thermal = broadcasts.filter((b) => b.event === 'thermal:changed')
    expect(thermal.map((b) => (b.payload as { level: string }).level)).toEqual([
      'warning',
      'critical'
    ])

    const state = (await invoke('getThermalState')) as { level: string }
    expect(state.level).toBe('critical')
  })
})
