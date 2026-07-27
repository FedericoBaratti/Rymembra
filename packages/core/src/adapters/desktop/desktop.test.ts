import { beforeEach, describe, expect, it, vi } from 'vitest'
import { configureLogger } from '../../logger'
import { adaptBetterSqlite, type BetterSqliteLike } from './sqlite'
import { desktopCapabilities } from './index'
import { coverPath, desktopPaths } from './files'

beforeEach(() => {
  configureLogger({ minLevel: 'error', sinks: [] })
})

/**
 * Doppio di better-sqlite3 che registra come viene chiamato.
 *
 * Il binario nativo è compilato per l'ABI di Electron, quindi non si può caricare
 * qui — ed è esattamente il motivo per cui l'adattamento è separato
 * dall'apertura. Il doppio implementa la superficie DICHIARATA per intero, quindi
 * non è un'approssimazione: se l'adapter usasse un metodo in più, non compilerebbe.
 */
function fakeDatabase(): BetterSqliteLike & {
  calls: { sql: string; args: unknown[] }[]
  pragmas: string[]
} {
  const calls: { sql: string; args: unknown[] }[] = []
  const pragmas: string[] = []

  return {
    calls,
    pragmas,
    prepare: (sql) => ({
      all: (...args: unknown[]) => {
        calls.push({ sql, args })
        return [{ id: 1 }]
      },
      get: (...args: unknown[]) => {
        calls.push({ sql, args })
        return { id: 1 }
      },
      run: (...args: unknown[]) => {
        calls.push({ sql, args })
        // bigint di proposito: è ciò che better-sqlite3 restituisce quando l'id
        // supera 2^53, e il renderer non saprebbe leggerlo in JSON.
        return { changes: 1, lastInsertRowid: 42n }
      }
    }),
    exec: (sql) => calls.push({ sql, args: [] }),
    pragma: (source, options) => {
      pragmas.push(source)
      if (source === 'user_version' && options?.simple === true) return 17
      return []
    },
    transaction: <T>(fn: () => T) => fn,
    function: () => {},
    close: () => calls.push({ sql: '<close>', args: [] })
  }
}

describe('adattamento di better-sqlite3', () => {
  it('passa i parametri di un array sparsi, e un oggetto come singolo argomento', () => {
    // Le due forme sono entrambe necessarie: le query posizionali usano l'array,
    // gli INSERT delle migrazioni usano i parametri con nome.
    const database = fakeDatabase()
    const driver = adaptBetterSqlite(database)

    driver.prepare('SELECT ? , ?').all([1, 'x'])
    driver.prepare('INSERT INTO albums VALUES (@a)').run({ a: 'chiave' })

    expect(database.calls[0]?.args).toEqual([1, 'x'])
    expect(database.calls[1]?.args).toEqual([{ a: 'chiave' }])
  })

  it('non passa argomenti quando non ci sono parametri', () => {
    const database = fakeDatabase()
    adaptBetterSqlite(database).prepare('SELECT 1').get()
    expect(database.calls[0]?.args).toEqual([])
  })

  it('converte lastInsertRowid da bigint a numero', () => {
    // Un bigint attraverserebbe l'IPC come "42n" o farebbe fallire JSON.stringify.
    const info = adaptBetterSqlite(fakeDatabase()).prepare('INSERT INTO x VALUES (1)').run()
    expect(info.lastInsertRowid).toBe(42)
    expect(typeof info.lastInsertRowid).toBe('number')
  })

  it('legge user_version in forma semplice', () => {
    expect(adaptBetterSqlite(fakeDatabase()).userVersion()).toBe(17)
  })

  it('forza a intero la versione che scrive', () => {
    // I PRAGMA non accettano parametri, quindi il valore va interpolato: forzarlo
    // a intero è l'unica difesa, e va fatta qui.
    const database = fakeDatabase()
    adaptBetterSqlite(database).setUserVersion(100.9)
    expect(database.pragmas).toContain('user_version = 100')
  })

  it('usa la transazione di better-sqlite3, non un BEGIN scritto a mano', () => {
    // La sua implementazione gestisce i savepoint annidati: riscriverla la
    // perderebbe, e i passi di migrazione che chiamano helper transazionali sono
    // esattamente quel caso.
    const database = fakeDatabase()
    const spy = vi.spyOn(database, 'transaction')
    const result = adaptBetterSqlite(database).transaction(() => 'fatto')

    expect(result).toBe('fatto')
    expect(spy).toHaveBeenCalledTimes(1)
    expect(database.calls.some((call) => call.sql.includes('BEGIN'))).toBe(false)
  })
})

describe('percorsi', () => {
  const paths = desktopPaths('C:/Users/tizio/AppData/Roaming/Aether')

  it('deriva tutti i percorsi dalla radice, che arriva da fuori', () => {
    // Nel legacy ogni modulo chiamava app.getPath da sé, e nessuno era provabile
    // senza mockare Electron.
    expect(paths.database).toContain('aether.db')
    expect(paths.covers).toContain('covers')
    expect(paths.skins).toContain('skins')
  })

  it('taglia dall\'hash tutto ciò che non è alfanumerico', () => {
    // L'hash viene dal database e non dall'utente, ma un separatore di percorso
    // qui significherebbe scrittura arbitraria.
    const attempt = coverPath(paths, '../../../etc/passwd')
    expect(attempt).not.toContain('..')
    expect(attempt).toContain('etcpasswd.webp')
  })

  it('la miniatura ha il suo suffisso', () => {
    expect(coverPath(paths, 'abc', true)).toContain('abc.t.webp')
    expect(coverPath(paths, 'abc', false)).toContain('abc.webp')
  })
})

describe('capacità del desktop', () => {
  const caps = desktopCapabilities({ label: 'desktop-win32' })

  it('dichiara FTS5 e lo spawn, che il mobile non ha', () => {
    expect(caps.search.fts5).toBe(true)
    expect(caps.system.spawn).toBe(true)
  })

  it('può rilanciarsi dopo un guasto fatale', () => {
    // È la differenza che permette al supervisor di uscire dal processo qui e di
    // NON uscirne sul mobile, dove nodejs-mobile non si riavvia in-process.
    expect(caps.system.restartAfterFatal).toBe(true)
  })

  it('serve la LAN ma non il trasferimento, e genera QR ma non li scansiona', () => {
    // I due protocolli hanno lati opposti: il desktop serve l'API ai telefoni, il
    // telefono serve il trasferimento. E la fotocamera sta solo sul telefono.
    expect(caps.network.lanServer).toBe(true)
    expect(caps.network.transferServer).toBe(false)
    expect(caps.network.qrGenerate).toBe(true)
    expect(caps.network.qrScan).toBe(false)
  })

  it('non usa il motore audio nativo: qui è Web Audio nel renderer', () => {
    expect(caps.playback.nativeAudio).toBe(false)
    expect(caps.playback.mediaSession).toBe(false)
  })

  it('ha lo Studio, che sul telefono non c\'è', () => {
    expect(caps.appearance.skinStudio).toBe(true)
  })

  it('distingue "sa lanciare processi" da "yt-dlp è installato"', () => {
    // Nel legacy le due cose erano confuse: il pulsante di download appariva e poi
    // falliva con BINARY_MISSING.
    const senzaSharp = desktopCapabilities({ label: 'x', sharp: false })
    expect(senzaSharp.system.spawn).toBe(true)
    expect(senzaSharp.system.imageResize).toBe(false)
  })
})
