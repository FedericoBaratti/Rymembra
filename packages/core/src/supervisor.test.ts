import { beforeEach, describe, expect, it, vi } from 'vitest'
import { AppError } from './errors'
import {
  clearRecentLogs,
  configureLogger,
  createMemorySink,
  formatLogLine,
  logger,
  recentLogs
} from './logger'
import { createSupervisor, type FlushStep } from './supervisor'

beforeEach(() => {
  clearRecentLogs()
  configureLogger({ minLevel: 'trace', sinks: [] })
})

describe('logger', () => {
  it('conserva lo stack e la catena delle cause — ciò che il legacy scartava', () => {
    const sink = createMemorySink()
    configureLogger({ minLevel: 'trace', sinks: [sink] })

    const cause = new Error('EIO: i/o error')
    logger('metadata').error('scrittura tag fallita', AppError.of(
      'metadata.tagWriteFailed',
      { path: 'x.flac' },
      { cause }
    ))

    const record = sink.records[0]
    expect(record?.scope).toBe('metadata')
    expect(record?.level).toBe('error')
    expect(record?.error?.code).toBe('metadata.tagWriteFailed')
    expect(record?.error?.stack).toBeTruthy()
    expect(record?.error?.causes?.[0]?.message).toBe('EIO: i/o error')
    // Il traceId lega la riga di log all'errore mostrato all'utente.
    expect(record?.traceId).toBe(record?.error?.traceId)
  })

  it('normalizza qualunque valore passato come errore', () => {
    const sink = createMemorySink()
    configureLogger({ minLevel: 'trace', sinks: [sink] })

    logger('download').warn('caduto', 'DL_FORBIDDEN')
    logger('db').error('errno', Object.assign(new Error('x'), { code: 'ENOSPC' }))

    expect(sink.records[0]?.error?.code).toBe('download.forbidden')
    expect(sink.records[1]?.error?.code).toBe('fs.diskFull')
  })

  it('tiene i campi strutturati separati dal messaggio', () => {
    const sink = createMemorySink()
    configureLogger({ minLevel: 'trace', sinks: [sink] })

    logger('scan').info('scansione conclusa', { files: 1204, durationMs: 8300 })
    expect(sink.records[0]?.fields).toEqual({ files: 1204, durationMs: 8300 })
    expect(sink.records[0]?.message).toBe('scansione conclusa')
  })

  it('with() lega campi impliciti a ogni riga', () => {
    const sink = createMemorySink()
    configureLogger({ minLevel: 'trace', sinks: [sink] })

    const sessionLog = logger('transfer').with({ sessionId: 'ab12' })
    sessionLog.info('avviata')
    sessionLog.info('conclusa', { files: 3 })

    expect(sink.records[0]?.fields).toEqual({ sessionId: 'ab12' })
    expect(sink.records[1]?.fields).toEqual({ sessionId: 'ab12', files: 3 })
  })

  it('filtra i sink sul livello minimo ma tiene tutto nel ring buffer', () => {
    // Il ring buffer serve proprio a questo: quando arriva l'errore si vogliono
    // le righe di debug che lo precedono, che su file non sarebbero mai finite.
    const sink = createMemorySink()
    configureLogger({ minLevel: 'warn', sinks: [sink] })

    logger('net').debug('richiesta partita')
    logger('net').warn('richiesta caduta')

    expect(sink.records).toHaveLength(1)
    expect(recentLogs().map((r) => r.level)).toEqual(['debug', 'warn'])
  })

  it('un sink rotto non fa fallire l\'operazione che stava loggando', () => {
    const broken = { write: () => { throw new Error('sink morto') } }
    const good = createMemorySink()
    configureLogger({ minLevel: 'trace', sinks: [broken, good] })

    expect(() => logger('boot').info('ciao')).not.toThrow()
    expect(good.records).toHaveLength(1)
  })

  it('formatLogLine include stack e cause', () => {
    const line = formatLogLine({
      ts: 0,
      level: 'error',
      scope: 'db',
      message: 'query fallita',
      fields: { table: 'tracks' },
      error: AppError.of('db.queryFailed', { detail: 'near SELCT' }, {
        cause: new Error('sqlite3 syntax error')
      }).toPayload()
    })
    expect(line).toContain('ERROR')
    expect(line).toContain('[db]')
    expect(line).toContain('table="tracks"')
    expect(line).toContain('error=db.queryFailed')
    expect(line).toContain('caused by Error: sqlite3 syntax error')
  })
})

describe('supervisor', () => {
  function steps(record: string[]): FlushStep[] {
    return [
      { name: 'settings', run: () => record.push('settings') },
      { name: 'secrets', run: () => { throw new Error('keystore non disponibile') } },
      { name: 'queueState', run: () => record.push('queueState') },
      { name: 'pairingStore', run: () => record.push('pairingStore') }
    ]
  }

  it('esegue TUTTI i flush anche se uno lancia — il bug di will-quit', () => {
    // Nel legacy main.ts:210 concatenava quattro flush non guardati: il primo
    // throw saltava gli altri tre, perdendo coda e store di accoppiamento.
    const done: string[] = []
    const supervisor = createSupervisor({ flushes: steps(done) })

    const outcomes = supervisor.runFlushes('quit')

    expect(done).toEqual(['settings', 'queueState', 'pairingStore'])
    expect(outcomes.map((o) => [o.name, o.ok])).toEqual([
      ['settings', true],
      ['secrets', false],
      ['queueState', true],
      ['pairingStore', true]
    ])
  })

  it('dà un nome al flush fallito, invece di ingoiarlo', () => {
    const sink = createMemorySink()
    configureLogger({ minLevel: 'trace', sinks: [sink] })

    const supervisor = createSupervisor({ flushes: steps([]) })
    const outcomes = supervisor.runFlushes('quit')

    const failed = outcomes.find((o) => !o.ok)
    expect(failed?.name).toBe('secrets')
    expect(failed?.error?.context?.['flush']).toBe('secrets')

    const logged = sink.records.find((r) => r.level === 'error')
    expect(logged?.message).toContain('secrets')
  })

  it('mette in salvo i dati e avvisa il renderer su guasto fatale', () => {
    const done: string[] = []
    const notify = vi.fn()
    const supervisor = createSupervisor({
      flushes: [{ name: 'settings', run: () => done.push('settings') }],
      notify
    })

    const event = supervisor.handleFatal('uncaughtException', new Error('boom'))

    expect(done).toEqual(['settings'])
    expect(event.error.code).toBe('internal.unexpected')
    expect(event.error.context?.['trigger']).toBe('uncaughtException')
    expect(event.flushes).toEqual([{ name: 'settings', ok: true }])
    expect(notify).toHaveBeenCalledWith(event)
  })

  it('allega le righe di log che PRECEDONO il guasto', () => {
    configureLogger({ minLevel: 'trace', sinks: [] })
    logger('scan').debug('sto leggendo la cartella X')
    logger('scan').debug('sto leggendo la cartella Y')

    const supervisor = createSupervisor({ flushes: [] })
    const event = supervisor.handleFatal('uncaughtException', new Error('boom'))

    const messages = event.recent.map((r) => r.message)
    expect(messages).toContain('sto leggendo la cartella X')
    expect(messages).toContain('sto leggendo la cartella Y')
    // Lo snapshot è preso prima di loggare il guasto stesso.
    expect(messages).not.toContain('guasto di processo: uncaughtException')
  })

  it('non si richiama a vicenda se notify lancia a sua volta', () => {
    let calls = 0
    const supervisor = createSupervisor({
      flushes: [],
      notify: () => {
        calls++
        throw new Error('renderer morto')
      }
    })

    expect(() => supervisor.handleFatal('uncaughtException', new Error('boom'))).not.toThrow()
    expect(calls).toBe(1)
  })

  it('un secondo guasto durante la gestione del primo viene ingoiato', () => {
    const notify = vi.fn()
    // Riferimento a sé stesso dentro il flush: il flush gira dopo, quindi la
    // const è già inizializzata quando viene letta.
    const supervisor: ReturnType<typeof createSupervisor> = createSupervisor({
      flushes: [
        {
          name: 'rientrante',
          run: () => {
            // Simula un guasto che nasce dentro la gestione di un guasto.
            supervisor.handleFatal('unhandledRejection', new Error('secondo'))
          }
        }
      ],
      notify
    })

    supervisor.handleFatal('uncaughtException', new Error('primo'))
    // Un solo avviso: il secondo è stato riconosciuto come rientrante.
    expect(notify).toHaveBeenCalledTimes(1)
  })

  it('non esce dal processo per default', () => {
    // Sul mobile uscire significa app morta: nodejs-mobile non si riavvia
    // in-process. Degradato con un banner batte spento.
    const exit = vi.fn()
    createSupervisor({ flushes: [], exit }).handleFatal('uncaughtException', new Error('x'))
    expect(exit).not.toHaveBeenCalled()
  })

  it('esce se richiesto esplicitamente, ma solo dopo i flush', () => {
    const order: string[] = []
    const exit = vi.fn(() => order.push('exit'))
    createSupervisor({
      flushes: [{ name: 'settings', run: () => order.push('flush') }],
      exitOnFatal: true,
      exit
    }).handleFatal('uncaughtException', new Error('x'))

    expect(order).toEqual(['flush', 'exit'])
  })

  it('collega e scollega gli handler di processo', () => {
    const before = process.listenerCount('uncaughtException')
    const supervisor = createSupervisor({ flushes: [] })
    const uninstall = supervisor.installProcessHandlers()

    expect(process.listenerCount('uncaughtException')).toBe(before + 1)
    expect(process.listenerCount('unhandledRejection')).toBeGreaterThan(0)

    uninstall()
    expect(process.listenerCount('uncaughtException')).toBe(before)
  })

  it('una rejection non gestita viene catturata e messa in salvo', () => {
    const done: string[] = []
    const notify = vi.fn()
    const supervisor = createSupervisor({
      flushes: [{ name: 'queueState', run: () => done.push('queueState') }],
      notify
    })
    const uninstall = supervisor.installProcessHandlers()
    try {
      process.emit('unhandledRejection', 'DL_NETWORK', Promise.resolve())
      expect(done).toEqual(['queueState'])
      expect(notify).toHaveBeenCalledTimes(1)
      // Anche una stringa lanciata diventa un errore classificato.
      expect(notify.mock.calls[0]?.[0].error.code).toBe('download.network')
    } finally {
      uninstall()
    }
  })
})
