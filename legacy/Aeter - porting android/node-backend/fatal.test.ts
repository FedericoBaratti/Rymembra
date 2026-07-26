import { describe, expect, it } from 'vitest'
import { createFatalHandler } from './fatal'

// The crash handler is the last line of defence: it must flush everything it
// can, notify the renderer, survive its own dependencies throwing, and never
// recurse when a second failure arrives while it is already running.

function makeDeps(): {
  logs: string[]
  flushed: string[]
  notified: Array<{ kind: string; message: string }>
  deps: Parameters<typeof createFatalHandler>[0]
} {
  const logs: string[] = []
  const flushed: string[] = []
  const notified: Array<{ kind: string; message: string }> = []
  return {
    logs,
    flushed,
    notified,
    deps: {
      log: (message) => logs.push(message),
      flushes: [() => flushed.push('db'), () => flushed.push('settings')],
      notify: (kind, message) => notified.push({ kind, message })
    }
  }
}

describe('createFatalHandler', () => {
  it('logs, runs every flush and notifies with the error message', () => {
    const { logs, flushed, notified, deps } = makeDeps()
    const handler = createFatalHandler(deps)

    handler('uncaughtException', new Error('boom'))

    expect(logs).toEqual(['[fatal] uncaughtException'])
    expect(flushed).toEqual(['db', 'settings'])
    expect(notified).toEqual([{ kind: 'uncaughtException', message: 'boom' }])
  })

  it('stringifies non-Error reasons (unhandledRejection can reject anything)', () => {
    const { notified, deps } = makeDeps()
    createFatalHandler(deps)('unhandledRejection', 'plain string reason')
    expect(notified[0].message).toBe('plain string reason')
  })

  it('keeps flushing when one flush throws', () => {
    const flushed: string[] = []
    const handler = createFatalHandler({
      log: () => {},
      flushes: [
        () => {
          throw new Error('flush 1 broken')
        },
        () => flushed.push('second')
      ],
      notify: () => {}
    })
    handler('uncaughtException', new Error('x'))
    expect(flushed).toEqual(['second'])
  })

  it('survives a broken logger and a broken notify', () => {
    const flushed: string[] = []
    const handler = createFatalHandler({
      log: () => {
        throw new Error('logger broken')
      },
      flushes: [() => flushed.push('db')],
      notify: () => {
        throw new Error('renderer gone')
      }
    })
    expect(() => handler('uncaughtException', new Error('x'))).not.toThrow()
    expect(flushed).toEqual(['db'])
  })

  it('does not recurse when a flush raises a nested fatal', () => {
    const calls: string[] = []
    // The nested call happens while the handler is running: it must be a no-op.
    let handler: (kind: string, err: unknown) => void = () => {}
    handler = createFatalHandler({
      log: (m) => calls.push(m),
      flushes: [
        () => {
          handler('uncaughtException', new Error('nested'))
          calls.push('flush-done')
        }
      ],
      notify: () => {}
    })
    handler('uncaughtException', new Error('outer'))
    expect(calls).toEqual(['[fatal] uncaughtException', 'flush-done'])

    // Once the first invocation completed, a LATER fatal is handled again.
    handler('unhandledRejection', new Error('later'))
    expect(calls).toEqual([
      '[fatal] uncaughtException',
      'flush-done',
      '[fatal] unhandledRejection',
      'flush-done'
    ])
  })
})
