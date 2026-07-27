import { beforeEach, describe, expect, it, vi } from 'vitest'
import { clearRecentLogs, configureLogger } from '../logger'
import { createDbHandle } from './open'
import {
  BASELINE_VERSION,
  LEGACY_FINAL_VERSION,
  latestVersion,
  migrate,
  planMigration,
  validateChain,
  type Chain,
  type Migration,
  type MigrationFiles
} from './migrate'
import { createTestDriver } from './testDriver'
import type { SqliteDriver } from './driver'

beforeEach(() => {
  clearRecentLogs()
  configureLogger({ minLevel: 'trace', sinks: [] })
})

const noFiles: MigrationFiles = {
  exists: () => false,
  write: () => {},
  coverPath: (hash, thumb) => `/covers/${hash}${thumb === true ? '.t' : ''}.webp`
}

/**
 * Catena sintetica che riproduce la divergenza REALE fra i due alberi: le stesse
 * due colonne aggiunte in ordine opposto, così le versioni intermedie hanno
 * schemi diversi e il punto d'arrivo è identico. È il caso che rende impossibile
 * numerare da `user_version` senza sapere chi ha scritto il file.
 */
function chain(overrides: Partial<Chain> = {}): Chain {
  const base = `
    CREATE TABLE tracks (id INTEGER PRIMARY KEY, title TEXT NOT NULL DEFAULT '');
  `
  return {
    baseline: [
      { version: 1, name: 'schema-iniziale', up: base },
      { version: 2, name: 'liked', up: 'ALTER TABLE tracks ADD COLUMN liked INTEGER DEFAULT 0' },
      { version: 3, name: 'podcast', up: 'CREATE TABLE podcasts (id INTEGER PRIMARY KEY)' },
      { version: BASELINE_VERSION, name: 'baseline', up: '' }
    ],
    legacy: {
      desktop: [
        { version: 1, name: 'schema-iniziale', up: base },
        // desktop: prima sync, poi podcast
        { version: 2, name: 'sync', up: 'ALTER TABLE tracks ADD COLUMN liked INTEGER DEFAULT 0' },
        { version: 3, name: 'podcast', up: 'CREATE TABLE podcasts (id INTEGER PRIMARY KEY)' },
        ...spacer(4, LEGACY_FINAL_VERSION.desktop)
      ],
      android: [
        { version: 1, name: 'schema-iniziale', up: base },
        // android: prima podcast, poi sync — stesso arrivo, strada diversa
        { version: 2, name: 'podcast', up: 'CREATE TABLE podcasts (id INTEGER PRIMARY KEY)' },
        { version: 3, name: 'sync', up: 'ALTER TABLE tracks ADD COLUMN liked INTEGER DEFAULT 0' },
        ...spacer(4, LEGACY_FINAL_VERSION.android)
      ]
    },
    unified: [],
    ...overrides
  }
}

/** Passi senza effetti, solo per far arrivare la storia al suo capolinea congelato. */
function spacer(from: number, to: number): Migration[] {
  const steps: Migration[] = []
  for (let v = from; v <= to; v++) steps.push({ version: v, name: `v${v}`, up: '' })
  return steps
}

function schemaOf(driver: SqliteDriver): string[] {
  return driver
    .prepare("SELECT name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name")
    .all()
    .map((row) => String(row['name']))
    .concat(
      driver
        .prepare('SELECT name FROM pragma_table_info(?) ORDER BY name')
        .all(['tracks'])
        .map((row) => `tracks.${String(row['name'])}`)
    )
}

describe('validateChain', () => {
  it('accetta una catena ben formata', () => {
    expect(validateChain(chain())).toEqual({ ok: true, value: true })
  })

  it('rifiuta un passo unificato numerato sotto la baseline', () => {
    // È l'errore silenzioso e permanente che la guardia esiste per prevenire: il
    // filtro del piano lo scarterebbe, quindi non verrebbe MAI eseguito e
    // nessuno se ne accorgerebbe.
    const result = validateChain(
      chain({ unified: [{ version: 42, name: 'sbagliato', up: '' }] })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('internal.invariantViolated')
      expect(String(result.error.params['what'])).toContain('sbagliato')
    }
  })

  it('rifiuta due passi con numeri non crescenti', () => {
    const result = validateChain(
      chain({
        unified: [
          { version: 101, name: 'a', up: '' },
          { version: 101, name: 'b', up: '' }
        ]
      })
    )
    expect(result.ok).toBe(false)
  })

  it('rifiuta una baseline che non arriva alla versione di confluenza', () => {
    const result = validateChain(
      chain({ baseline: [{ version: 1, name: 'solo-questo', up: '' }] })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) expect(String(result.error.params['what'])).toContain('baseline')
  })

  it('rifiuta una storia legacy che non finisce dove è congelata', () => {
    const broken = chain()
    const result = validateChain({
      ...broken,
      legacy: { ...broken.legacy, desktop: [{ version: 5, name: 'troncata', up: '' }] }
    })
    expect(result.ok).toBe(false)
    if (!result.ok) expect(String(result.error.params['what'])).toContain('congelata')
  })
})

describe('planMigration', () => {
  const c = chain({ unified: [{ version: 101, name: 'skin', up: '' }] })

  it('un file nuovo nasce dalla baseline e non tocca le storie legacy', () => {
    const plan = planMigration(0, c, 'desktop')
    expect(plan.ok).toBe(true)
    if (!plan.ok) return
    expect(plan.value.crossesLegacy).toBe(false)
    expect(plan.value.steps.map((s) => s.name)).toEqual([
      'schema-iniziale',
      'liked',
      'podcast',
      'baseline',
      'skin'
    ])
    expect(plan.value.to).toBe(101)
  })

  it('un file legacy a metà catena continua nella SUA storia', () => {
    const desktop = planMigration(2, c, 'desktop')
    const android = planMigration(2, c, 'android')
    expect(desktop.ok && android.ok).toBe(true)
    if (!desktop.ok || !android.ok) return

    // Alla v2 il desktop ha già sync e gli manca podcast; sull'android è il
    // contrario. Lo stesso numero, due situazioni diverse.
    expect(desktop.value.steps[0]?.name).toBe('podcast')
    expect(android.value.steps[0]?.name).toBe('sync')
  })

  it('un file al capolinea legacy passa per la confluenza e poi nell\'unificata', () => {
    const plan = planMigration(LEGACY_FINAL_VERSION.android, c, 'android')
    expect(plan.ok).toBe(true)
    if (!plan.ok) return
    expect(plan.value.steps.map((s) => s.name)).toEqual(['confluenza-android', 'skin'])
    expect(plan.value.crossesLegacy).toBe(true)
  })

  it('un file già unificato esegue solo i passi nuovi', () => {
    const plan = planMigration(BASELINE_VERSION, c, 'desktop')
    expect(plan.ok).toBe(true)
    if (!plan.ok) return
    expect(plan.value.steps.map((s) => s.name)).toEqual(['skin'])
    expect(plan.value.crossesLegacy).toBe(false)
  })

  it('un file aggiornato non esegue niente', () => {
    const plan = planMigration(101, c, 'desktop')
    expect(plan.ok).toBe(true)
    if (!plan.ok) return
    expect(plan.value.steps).toEqual([])
    expect(plan.value.to).toBe(101)
  })

  it('un file scritto da una build più nuova viene rifiutato senza toccarlo', () => {
    // Il caso che il legacy NON gestiva: il ciclo non aveva iterazioni, la
    // migrazione "riusciva", e poi le query fallivano su colonne inesistenti con
    // un errore che parlava di SQL invece di versioni.
    const plan = planMigration(999, c, 'desktop')
    expect(plan.ok).toBe(false)
    if (!plan.ok) {
      expect(plan.error.code).toBe('db.versionAhead')
      expect(plan.error.params['dbVersion']).toBe(999)
      expect(plan.error.params['appVersion']).toBe(101)
      expect(plan.error.severity).toBe('fatal')
    }
  })

  it('un file legacy oltre il capolinea della sua storia viene rifiutato', () => {
    // Un file a v18 letto come storia desktop (che finisce a 17) viene dall'altra
    // storia o da una build ignota: migrarlo alla cieca romperebbe lo schema in
    // silenzio.
    const plan = planMigration(18, c, 'desktop')
    expect(plan.ok).toBe(false)
    if (!plan.ok) expect(plan.error.code).toBe('db.versionAhead')
  })

  it('latestVersion non scende sotto la baseline nemmeno a catena unificata vuota', () => {
    expect(latestVersion(chain())).toBe(BASELINE_VERSION)
  })
})

describe('migrate', () => {
  function run(driver: SqliteDriver, c: Chain, history: 'desktop' | 'android' = 'desktop') {
    return migrate({ db: driver, files: noFiles, chain: c, history })
  }

  it('porta un file nuovo alla baseline', () => {
    const driver = createTestDriver()
    const result = run(driver, chain())
    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.value.from).toBe(0)
      expect(result.value.to).toBe(BASELINE_VERSION)
    }
    expect(driver.userVersion()).toBe(BASELINE_VERSION)
    driver.close()
  })

  it('le due storie legacy convergono sullo STESSO schema', () => {
    // È l'invariante che nel legacy era garantita solo da un test di parità
    // contro un file di schema atteso committato in due alberi. Qui la si prova
    // direttamente: due strade, stesso arrivo, stessa versione finale.
    const fromDesktop = createTestDriver()
    fromDesktop.exec('CREATE TABLE tracks (id INTEGER PRIMARY KEY, title TEXT NOT NULL DEFAULT \'\')')
    fromDesktop.setUserVersion(1)
    run(fromDesktop, chain(), 'desktop')

    const fromAndroid = createTestDriver()
    fromAndroid.exec('CREATE TABLE tracks (id INTEGER PRIMARY KEY, title TEXT NOT NULL DEFAULT \'\')')
    fromAndroid.setUserVersion(1)
    run(fromAndroid, chain(), 'android')

    expect(schemaOf(fromDesktop)).toEqual(schemaOf(fromAndroid))
    expect(fromDesktop.userVersion()).toBe(fromAndroid.userVersion())
    expect(fromDesktop.userVersion()).toBe(BASELINE_VERSION)

    fromDesktop.close()
    fromAndroid.close()
  })

  it('un passo che fallisce lascia il file alla versione precedente', () => {
    const driver = createTestDriver()
    const c = chain({
      unified: [
        { version: 101, name: 'buono', up: 'CREATE TABLE ok_table (id INTEGER)' },
        { version: 102, name: 'rotto', up: 'CREATE TABLE ; sintassi non valida' }
      ]
    })

    const result = migrate({ db: driver, files: noFiles, chain: c, history: 'desktop' })
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('db.migrationFailed')
      expect(result.error.params['from']).toBe(101)
      expect(result.error.params['to']).toBe(102)
      // Il nome del passo, non solo il numero: il numero da solo non dice cosa
      // stava facendo.
      expect(result.error.params['step']).toBe('rotto')
    }
    // I passi riusciti restano applicati e il file è a 101: il prossimo avvio
    // riparte da lì invece di rifare tutto.
    expect(driver.userVersion()).toBe(101)
    expect(driver.prepare("SELECT name FROM sqlite_master WHERE name = 'ok_table'").get()).toBeTruthy()
    driver.close()
  })

  it('il rollback del passo fallito non lascia mezze tabelle', () => {
    const driver = createTestDriver()
    const c = chain({
      unified: [
        {
          version: 101,
          name: 'due-cose-di-cui-una-rotta',
          up: `
            CREATE TABLE prima (id INTEGER);
            CREATE TABLE seconda (id INTEGER, colonna REFERENCES tabella_che_non_esiste(x));
            INSERT INTO tabella_che_non_esiste VALUES (1);
          `
        }
      ]
    })
    const result = migrate({ db: driver, files: noFiles, chain: c, history: 'desktop' })
    expect(result.ok).toBe(false)
    expect(driver.prepare("SELECT name FROM sqlite_master WHERE name = 'prima'").get()).toBeUndefined()
    expect(driver.userVersion()).toBe(BASELINE_VERSION)
    driver.close()
  })

  it('un passo funzione riceve DB, file e canale di avviso', () => {
    const driver = createTestDriver()
    const written: string[] = []
    const files: MigrationFiles = {
      exists: () => false,
      write: (path) => written.push(path),
      coverPath: (hash, thumb) => `/covers/${hash}${thumb === true ? '.t' : ''}.webp`
    }

    const c = chain({
      unified: [
        {
          version: 101,
          name: 'estrai-copertine',
          up: (ctx) => {
            ctx.db.exec('CREATE TABLE cover_art (hash TEXT PRIMARY KEY)')
            ctx.db.prepare('INSERT INTO cover_art (hash) VALUES (?)').run(['abc'])
            const rows = ctx.db.prepare('SELECT hash FROM cover_art').all()
            for (const row of rows) {
              const hash = String(row['hash'])
              if (!ctx.files.exists(ctx.files.coverPath(hash))) {
                ctx.files.write(ctx.files.coverPath(hash), new Uint8Array([1, 2, 3]))
              }
            }
            ctx.warn('copertina di prova non estratta', new Error('disco pieno'))
          }
        }
      ]
    })

    const result = migrate({ db: driver, files, chain: c, history: 'desktop' })
    expect(result.ok).toBe(true)
    expect(written).toEqual(['/covers/abc.webp'])
    driver.close()
  })

  it('fa la copia di sicurezza solo per un file esistente', () => {
    const fresh = createTestDriver()
    const backupFresh = vi.fn()
    migrate({ db: fresh, files: noFiles, chain: chain(), history: 'desktop', backup: backupFresh })
    expect(backupFresh).not.toHaveBeenCalled()
    fresh.close()

    const existing = createTestDriver()
    existing.setUserVersion(2)
    existing.exec('CREATE TABLE tracks (id INTEGER PRIMARY KEY, liked INTEGER)')
    const backupExisting = vi.fn()
    migrate({
      db: existing,
      files: noFiles,
      chain: chain(),
      history: 'desktop',
      backup: backupExisting
    })
    expect(backupExisting).toHaveBeenCalledWith(2)
    existing.close()
  })

  it('una copia di sicurezza che fallisce non blocca l\'avvio', () => {
    const driver = createTestDriver()
    driver.setUserVersion(3)
    driver.exec('CREATE TABLE tracks (id INTEGER PRIMARY KEY, liked INTEGER)')
    driver.exec('CREATE TABLE podcasts (id INTEGER PRIMARY KEY)')

    const result = migrate({
      db: driver,
      files: noFiles,
      chain: chain(),
      history: 'desktop',
      backup: () => { throw new Error('disco pieno') }
    })
    expect(result.ok).toBe(true)
    driver.close()
  })
})

describe('createDbHandle', () => {
  function deps(overrides: Partial<Parameters<typeof createDbHandle>[0]> = {}) {
    return {
      path: ':memory:',
      openDriver: () => createTestDriver(),
      chain: chain(),
      history: 'desktop' as const,
      files: noFiles,
      ...overrides
    }
  }

  it('apre, migra e riporta la versione', () => {
    const handle = createDbHandle(deps())
    const result = handle.open()
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.value.version).toBe(BASELINE_VERSION)
    expect(handle.status).toBe('open')
    handle.close()
    expect(handle.status).toBe('closed')
  })

  it('apre una volta sola', () => {
    const openDriver = vi.fn(() => createTestDriver())
    const handle = createDbHandle(deps({ openDriver }))
    handle.open()
    handle.open()
    handle.get()
    expect(openDriver).toHaveBeenCalledTimes(1)
    handle.close()
  })

  it('RICORDA il guasto e risponde subito, invece di far aspettare', () => {
    // Il bug del legacy: getDb() lanciava dentro registerLibraryIpc() prima di
    // registrare gli handler, quindi il renderer chiamava e non riceveva NULLA.
    // Qui l'apertura fallita è un valore, quindi ogni handler ha una risposta.
    const openDriver = vi.fn(() => {
      throw Object.assign(new Error('EACCES: permission denied'), { code: 'EACCES', path: '/db' })
    })
    const handle = createDbHandle(deps({ openDriver }))

    const first = handle.get()
    const second = handle.get()

    expect(handle.status).toBe('failed')
    expect(first.ok).toBe(false)
    expect(second.ok).toBe(false)
    // Non si ritenta ad ogni chiamata: sarebbe un'attesa per ogni click.
    expect(openDriver).toHaveBeenCalledTimes(1)
    if (!first.ok) {
      // Un errno di filesystem resta tale: dice all'utente cosa fare.
      expect(first.error.code).toBe('fs.permissionDenied')
      expect(first.error.context?.['dbPath']).toBe(':memory:')
    }
  })

  it('reopen dimentica il guasto e riprova — i lock passano', () => {
    let attempts = 0
    const handle = createDbHandle(
      deps({
        openDriver: () => {
          attempts++
          if (attempts === 1) throw new Error('SQLITE_BUSY: database is locked')
          return createTestDriver()
        }
      })
    )

    const failed = handle.get()
    expect(failed.ok).toBe(false)
    if (!failed.ok) {
      expect(failed.error.code).toBe('db.locked')
      // Ritentabile: è il dato che dice alla UI di offrire "riprova".
      expect(failed.error.retryable).toBe(true)
    }

    const reopened = handle.reopen()
    expect(reopened.ok).toBe(true)
    expect(handle.status).toBe('open')
    handle.close()
  })

  it('mette in quarantena un file corrotto, così l\'avvio successivo non ricade', () => {
    const quarantine = vi.fn(() => '/data/aether.db.corrupt-1')
    const handle = createDbHandle(
      deps({
        openDriver: () => { throw new Error('SQLITE_CORRUPT: database disk image is malformed') },
        quarantine
      })
    )

    const result = handle.get()
    expect(quarantine).toHaveBeenCalledWith(':memory:')
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('db.corrupt')
      expect(result.error.params['quarantinedAs']).toBe('/data/aether.db.corrupt-1')
    }
  })

  it('una migrazione fallita NON mette in quarantena: il file è ancora buono', () => {
    const quarantine = vi.fn(() => '/mai')
    const handle = createDbHandle(
      deps({
        chain: chain({ unified: [{ version: 101, name: 'rotto', up: 'SINTASSI NON VALIDA' }] }),
        quarantine
      })
    )

    const result = handle.get()
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('db.migrationFailed')
    // Il file resta leggibile dalla build che l'ha scritto: spostarlo sarebbe
    // distruggere dati recuperabili.
    expect(quarantine).not.toHaveBeenCalled()
  })

  it('rifiuta una catena mal formata prima di aprire', () => {
    const openDriver = vi.fn(() => createTestDriver())
    const handle = createDbHandle(
      deps({ openDriver, chain: chain({ unified: [{ version: 3, name: 'sotto-baseline', up: '' }] }) })
    )
    const result = handle.get()
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('internal.invariantViolated')
    expect(openDriver).not.toHaveBeenCalled()
  })

  it('configura la connessione e rileva FTS5', () => {
    const configure = vi.fn()
    const handle = createDbHandle(deps({ configure, detectFts5: () => true }))
    const result = handle.open()
    expect(configure).toHaveBeenCalledTimes(1)
    if (result.ok) expect(result.value.fts5).toBe(true)
    handle.close()
  })

  it('un rilevamento FTS5 che lancia non impedisce l\'apertura', () => {
    // Non sapere se c'è FTS5 significa ripiegare su afold(), che è la strada che
    // il mobile usa sempre: non è un motivo per non aprire il database.
    const handle = createDbHandle(
      deps({ detectFts5: () => { throw new Error('no such module: fts5') } })
    )
    const result = handle.open()
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.value.fts5).toBe(false)
    handle.close()
  })
})
