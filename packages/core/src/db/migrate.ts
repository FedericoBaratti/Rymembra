/**
 * Migrazioni.
 *
 * Nel legacy erano un array di stringhe SQL dentro `db.ts` (591 righe), replicato
 * in due alberi. E c'era un problema più profondo di dove stavano scritte: **le
 * due storie hanno divergito.** Stessa `user_version`, schema diverso.
 *
 *   desktop v7  = sync Google Drive        android v7+v8 = migrazione Spotify
 *   desktop v8  = library_fetch            android v13   = podcast
 *   desktop v13 = play_history             android v15   = sync Google Drive
 *   desktop v15 = migrazione Spotify       android v16   = library_fetch
 *   desktop v17 = phone_repair (fine)      android v18   = phone_repair (fine)
 *
 * I due alberi tenevano allineato solo il PUNTO D'ARRIVO, con un test di parità
 * contro un file di schema atteso committato identico in entrambi. Cioè: le due
 * catene passano da strade diverse e arrivano allo stesso schema, e la prova che
 * ci arrivano è un test, non la struttura del codice.
 *
 * Nel core unificato la catena è una. Ma i file già sui dischi degli utenti no:
 * un'installazione Android a v18 e una desktop a v17 hanno lo stesso schema con
 * numeri diversi, e un'installazione a metà catena ha uno schema che dipende da
 * quale app l'ha scritta. Continuare a numerare da `user_version` sarebbe
 * sbagliato in entrambi i casi.
 *
 * La soluzione qui: le due storie legacy restano CONGELATE come due catene
 * separate e immutabili, il cui unico scopo è portare un file esistente fino al
 * suo capolinea. Arrivato lì, il file viene timbrato con `BASELINE_VERSION` — e da
 * quel momento esiste una sola catena, quella unificata. Un file nuovo salta i
 * legacy e parte direttamente dalla baseline.
 *
 * Fra 18 e 100 c'è un vuoto deliberato: è uno steccato, così nessun passo
 * unificato futuro potrà mai collidere con un numero di versione legacy.
 */

import { AppError } from '../errors'
import { logger } from '../logger'
import { err, ok, type Result } from '../result'
import type { SqliteDriver } from './driver'

const log = logger('db')

/**
 * Versione che segna "il file è allineato allo schema unificato".
 *
 * NON cambiare: è scritta nei file degli utenti. La catena unificata continua da
 * qui in avanti, un numero per passo.
 */
export const BASELINE_VERSION = 100

/** Le due storie legacy, e il numero al quale ciascuna è finita. */
export const LEGACY_FINAL_VERSION = {
  desktop: 17,
  android: 18
} as const

export type LegacyHistoryName = keyof typeof LEGACY_FINAL_VERSION

/**
 * Servizi che un passo di migrazione può richiedere oltre al DB.
 *
 * Esistono perché nel legacy il passo v9 (estrazione delle copertine dal BLOB al
 * filesystem) chiamava direttamente `writeFileSync` e `coverPath`, e il test di
 * parità dello schema poteva girare solo perché su un DB vuoto quel ramo non
 * veniva mai eseguito. Iniettati, il passo si può provare per davvero.
 */
export interface MigrationFiles {
  exists(path: string): boolean
  write(path: string, data: Uint8Array): void
  /** Percorso su disco della copertina, miniatura compresa. */
  coverPath(hash: string, thumb?: boolean): string
}

export interface MigrationContext {
  readonly db: SqliteDriver
  readonly files: MigrationFiles
  /**
   * Se questo DB deve avere la tabella virtuale FTS5.
   *
   * Non è una preferenza: sul backend mobile sql.js è compilato senza FTS5, e
   * creare `tracks_fts` con i suoi trigger renderebbe fallire OGNI insert su
   * `tracks`. Il legacy lo risolveva con `process.platform === 'android' ? '' :
   * FTS_SCHEMA` dentro il passo v1 — cioè una decisione di piattaforma dentro una
   * migrazione. Qui è un dato che arriva dall'adapter, e la conseguenza è che lo
   * schema di un dispositivo Android differisce legittimamente da quello del
   * desktop per questa tabella e questi tre trigger.
   */
  readonly fts5: boolean
  /**
   * Un guasto non bloccante dentro un passo (una copertina su 10.000 che non si
   * estrae). Va NOMINATO: nel legacy la v9 aveva già la cura di dire quale
   * copertina fallisse, perché altrimenti disco pieno e permessi negati restano
   * invisibili.
   */
  warn(message: string, error?: unknown): void
}

export interface Migration {
  /** Numero di arrivo: dopo questo passo `user_version` vale questo. */
  readonly version: number
  /** Nome breve: compare nei log e negli errori. Il numero da solo non dice niente. */
  readonly name: string
  /** SQL puro, oppure una funzione quando il passo deve calcolare o toccare i file. */
  readonly up: string | ((ctx: MigrationContext) => void)
}

export interface MigrationPlan {
  /** Versione di partenza letta dal file. */
  readonly from: number
  /** Versione di arrivo. */
  readonly to: number
  readonly steps: readonly Migration[]
  /**
   * Vero se il piano attraversa la catena legacy: è il caso in cui conviene
   * fare un backup prima di toccare qualcosa.
   */
  readonly crossesLegacy: boolean
}

export interface Chain {
  /** La catena unificata: passi con versione > BASELINE_VERSION. */
  readonly unified: readonly Migration[]
  /** Lo schema di partenza per un file nuovo, che porta a BASELINE_VERSION. */
  readonly baseline: readonly Migration[]
  /** Le due storie congelate. */
  readonly legacy: Readonly<Record<LegacyHistoryName, readonly Migration[]>>
}

/** La versione più alta che questo binario sa produrre. */
export function latestVersion(chain: Chain): number {
  return chain.unified.reduce((max, step) => Math.max(max, step.version), BASELINE_VERSION)
}

/**
 * Decide cosa eseguire, senza eseguire niente.
 *
 * Separato dall'esecuzione perché è la parte che va provata su tutte le forme di
 * file che esistono nel mondo reale — nuovo, a metà di una delle due catene
 * legacy, al capolinea legacy, già unificato, o scritto da una versione più
 * nuova dell'app.
 */
export function planMigration(
  currentVersion: number,
  chain: Chain,
  history: LegacyHistoryName
): Result<MigrationPlan, AppError> {
  const latest = latestVersion(chain)

  if (currentVersion > latest) {
    // Il caso che il legacy NON gestiva: un file scritto da una build più nuova
    // veniva aperto, la migrazione non partiva (il ciclo non ha iterazioni), e
    // poi le query fallivano su colonne inesistenti — con un errore che parlava
    // di SQL, non di versioni. Qui non si tocca nulla e si dice cos'è.
    return err(
      AppError.of('db.versionAhead', { dbVersion: currentVersion, appVersion: latest })
    )
  }

  const steps: Migration[] = []
  let crossesLegacy = false

  if (currentVersion === 0) {
    // File nuovo: salta le storie legacy e nasce già allineato alla baseline.
    steps.push(...chain.baseline)
  } else if (currentVersion < BASELINE_VERSION) {
    const finalLegacy = LEGACY_FINAL_VERSION[history]

    if (currentVersion > finalLegacy) {
      // Un file legacy oltre il capolinea della sua storia non è interpretabile:
      // vuol dire che viene dall'ALTRA storia, o da una build ignota. Migrarlo
      // alla cieca romperebbe lo schema in silenzio.
      return err(
        AppError.of('db.versionAhead', { dbVersion: currentVersion, appVersion: finalLegacy })
      )
    }

    steps.push(...chain.legacy[history].filter((step) => step.version > currentVersion))
    // Timbro di confluenza. Non esegue SQL — i passi legacy hanno già prodotto lo
    // schema della baseline — ma rende esplicito nel piano il momento in cui le
    // due storie diventano una, invece di nasconderlo in un setUserVersion.
    steps.push({ version: BASELINE_VERSION, name: `confluenza-${history}`, up: '' })
    crossesLegacy = true
  }

  const unifiedFrom = currentVersion < BASELINE_VERSION ? BASELINE_VERSION : currentVersion
  steps.push(...chain.unified.filter((step) => step.version > unifiedFrom))

  return ok({
    from: currentVersion,
    to: steps.length > 0 ? latest : currentVersion,
    steps,
    crossesLegacy
  })
}

/**
 * Controlla che una catena sia ben formata, prima di eseguirla.
 *
 * Serve perché gli errori che previene sono silenziosi e permanenti: un passo
 * unificato numerato sotto la baseline non verrebbe MAI eseguito (il filtro lo
 * scarta), e due passi con lo stesso numero lascerebbero il file a una versione
 * che non corrisponde allo schema. Nel legacy niente controllava questo: la
 * versione era l'indice nell'array, quindi rinumerare per sbaglio significava
 * ri-eseguire o saltare passi già applicati sui dischi degli utenti.
 */
export function validateChain(chain: Chain): Result<true, AppError> {
  const problems: string[] = []

  const ascending = (steps: readonly Migration[], label: string): void => {
    for (let i = 1; i < steps.length; i++) {
      const previous = steps[i - 1]
      const current = steps[i]
      if (previous === undefined || current === undefined) continue
      if (current.version <= previous.version) {
        problems.push(
          `${label}: il passo '${current.name}' (v${current.version}) non segue '${previous.name}' (v${previous.version})`
        )
      }
    }
  }

  ascending(chain.baseline, 'baseline')
  const lastBaseline = chain.baseline[chain.baseline.length - 1]
  if (lastBaseline === undefined) {
    problems.push('baseline: nessun passo')
  } else if (lastBaseline.version !== BASELINE_VERSION) {
    problems.push(
      `baseline: l'ultimo passo deve arrivare a v${BASELINE_VERSION}, arriva a v${lastBaseline.version}`
    )
  }

  ascending(chain.unified, 'unified')
  for (const step of chain.unified) {
    if (step.version <= BASELINE_VERSION) {
      problems.push(
        `unified: '${step.name}' ha v${step.version}, non oltre la baseline: non verrebbe mai eseguito`
      )
    }
  }

  for (const history of Object.keys(LEGACY_FINAL_VERSION) as LegacyHistoryName[]) {
    const steps = chain.legacy[history]
    ascending(steps, `legacy/${history}`)
    const last = steps[steps.length - 1]
    const expected = LEGACY_FINAL_VERSION[history]
    if (steps.length > 0 && last !== undefined && last.version !== expected) {
      problems.push(
        `legacy/${history}: la storia è congelata a v${expected}, l'ultimo passo arriva a v${last.version}`
      )
    }
  }

  if (problems.length > 0) {
    return err(AppError.of('internal.invariantViolated', { what: problems.join('; ') }))
  }
  return ok(true)
}

export interface MigrateDeps {
  readonly db: SqliteDriver
  readonly files: MigrationFiles
  readonly chain: Chain
  readonly history: LegacyHistoryName
  /** Vedi `MigrationContext.fts5`. Default: assente, la scelta prudente. */
  readonly fts5?: boolean
  /**
   * Copia di sicurezza prima di migrare un file esistente. Nel legacy c'era
   * (`preMigrationBackup`) e va tenuta: una migrazione che fallisce a metà su un
   * DB da 100.000 tracce senza copia è una perdita di dati.
   */
  readonly backup?: (version: number) => void
}

export interface MigrateOutcome {
  readonly from: number
  readonly to: number
  readonly applied: readonly string[]
}

/**
 * Applica il piano. Non lancia.
 *
 * Ogni passo è in una transazione con dentro il proprio `user_version`: se cade,
 * il file resta esattamente alla versione precedente, e il tentativo successivo
 * riparte da lì. Il legacy faceva già così, ed è la cosa giusta.
 */
export function migrate(deps: MigrateDeps): Result<MigrateOutcome, AppError> {
  const { db, files, chain, history, backup } = deps

  let currentVersion: number
  try {
    currentVersion = db.userVersion()
  } catch (cause) {
    return err(AppError.of('db.openFailed', {}, { cause }))
  }

  const planned = planMigration(currentVersion, chain, history)
  if (!planned.ok) return planned

  const plan = planned.value
  if (plan.steps.length === 0) {
    return ok({ from: plan.from, to: currentVersion, applied: [] })
  }

  if (plan.from > 0 && backup !== undefined) {
    try {
      backup(plan.from)
    } catch (cause) {
      // Il backup che non riesce non deve bloccare l'avvio, ma va detto: è
      // l'unica rete se il passo successivo va male.
      log.warn('copia di sicurezza pre-migrazione non riuscita', cause, { version: plan.from })
    }
  }

  const ctx: MigrationContext = {
    db,
    files,
    fts5: deps.fts5 ?? false,
    warn: (message, error) => log.warn(message, error, { phase: 'migration' })
  }

  const applied: string[] = []
  let version = plan.from

  for (const step of plan.steps) {
    try {
      db.transaction(() => {
        if (typeof step.up === 'function') step.up(ctx)
        else if (step.up.trim().length > 0) db.exec(step.up)
        db.setUserVersion(step.version)
      })
    } catch (cause) {
      return err(
        AppError.of(
          'db.migrationFailed',
          { from: version, to: step.version, step: step.name },
          { cause, context: { history, applied } }
        )
      )
    }
    version = step.version
    applied.push(step.name)
  }

  log.info('schema aggiornato', { from: plan.from, to: version, steps: applied.length })
  return ok({ from: plan.from, to: version, applied })
}
