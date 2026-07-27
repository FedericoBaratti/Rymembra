/**
 * Percorsi e file del desktop.
 *
 * Nel legacy questi calcoli erano sparsi: `db.ts` faceva
 * `join(app.getPath('userData'), 'aether.db')`, `coverPaths.ts` calcolava le
 * copertine, `dbBackup.ts` i backup, e ognuno chiamava `app.getPath` da sé.
 * Conseguenza pratica: nessuno di quei moduli era provabile senza mockare
 * Electron, ed è il motivo per cui il test dello schema doveva dichiarare
 * `app: { getPath: () => 'db-schema-test-userdata-does-not-exist' }` — un percorso
 * finto che funzionava solo perché il ramo che tocca i file non veniva eseguito.
 *
 * Qui la radice arriva come parametro. Electron la fornisce una volta, all'avvio.
 */

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  renameSync,
  statSync,
  unlinkSync,
  writeFileSync
} from 'node:fs'
import { join } from 'node:path'
import { AppError } from '../../errors'
import { logger } from '../../logger'
import type { MigrationFiles } from '../../db'

const log = logger('jsonFile')

export interface DesktopPaths {
  /** Radice dei dati utente. Su Windows è %APPDATA%/Aether. */
  readonly userData: string
  readonly database: string
  readonly covers: string
  readonly backups: string
  readonly logs: string
  readonly skins: string
}

export function desktopPaths(userData: string): DesktopPaths {
  return {
    userData,
    database: join(userData, 'aether.db'),
    covers: join(userData, 'covers'),
    backups: join(userData, 'backups'),
    logs: join(userData, 'logs'),
    skins: join(userData, 'skins')
  }
}

/**
 * Crea le cartelle che servono, se mancano.
 *
 * Restituisce un Result invece di lanciare: una cartella che non si crea è un
 * problema reale (disco pieno, permessi) e deve arrivare all'utente come un
 * errore classificato, non come un crash all'avvio — che è precisamente ciò che
 * il supervisor esiste per evitare.
 */
export function ensureDirectories(paths: DesktopPaths): AppError | null {
  for (const directory of [paths.covers, paths.backups, paths.logs, paths.skins]) {
    try {
      mkdirSync(directory, { recursive: true })
    } catch (cause) {
      return AppError.from(cause).withContext({ directory })
    }
  }
  return null
}

/** Il percorso su disco di una copertina. `.t.webp` è la miniatura. */
export function coverPath(paths: DesktopPaths, hash: string, thumb = false): string {
  // L'hash arriva dal DB e non dall'utente, ma un separatore di percorso qui
  // significherebbe scrittura arbitraria: si taglia comunque.
  const safe = hash.replace(/[^a-zA-Z0-9]/g, '')
  return join(paths.covers, `${safe}${thumb ? '.t' : ''}.webp`)
}

/** I servizi file che i passi di migrazione richiedono. */
export function migrationFiles(paths: DesktopPaths): MigrationFiles {
  return {
    exists: (path) => existsSync(path),
    write: (path, data) => writeFileSync(path, data),
    coverPath: (hash, thumb) => coverPath(paths, hash, thumb === true)
  }
}

/** Quanti backup pre-migrazione tenere. Oltre, si buttano i più vecchi. */
const MAX_BACKUPS = 5

/**
 * Copia di sicurezza prima di migrare.
 *
 * Il nome porta la versione di partenza, non solo la data: se una migrazione
 * fallisce si vuole sapere DA QUALE versione ripartire, e con la sola data lo si
 * deduce a occhio.
 */
export function backupBeforeMigration(paths: DesktopPaths, fromVersion: number): void {
  if (!existsSync(paths.database)) return

  const stamp = new Date().toISOString().replace(/[:.]/g, '-')
  const target = join(paths.backups, `aether-v${fromVersion}-${stamp}.db`)
  copyFileSync(paths.database, target)
  log.info('copia di sicurezza pre-migrazione creata', { target, fromVersion })

  pruneBackups(paths)
}

function pruneBackups(paths: DesktopPaths): void {
  try {
    const entries = readdirSync(paths.backups)
      .filter((name) => name.startsWith('aether-v') && name.endsWith('.db'))
      .map((name) => {
        const full = join(paths.backups, name)
        return { full, mtime: statSync(full).mtimeMs }
      })
      .sort((a, b) => b.mtime - a.mtime)

    for (const stale of entries.slice(MAX_BACKUPS)) {
      unlinkSync(stale.full)
    }
  } catch (cause) {
    // Non riuscire a potare i backup non deve impedire di migrare: al peggio
    // occupano spazio, e lo spazio è un problema minore della perdita di dati.
    log.warn('potatura dei backup non riuscita', cause)
  }
}

/**
 * Sposta da parte un database corrotto.
 *
 * Senza questo, un file corrotto è un vicolo cieco: ogni avvio ritenta e
 * fallisce allo stesso modo, per sempre. Il file NON si cancella — può contenere
 * dati recuperabili con strumenti esterni, e la libreria di qualcuno vale più
 * dello spazio che occupa.
 */
export function quarantineDatabase(paths: DesktopPaths, path: string): string {
  const stamp = new Date().toISOString().replace(/[:.]/g, '-')
  const target = join(paths.backups, `aether-corrotto-${stamp}.db`)
  renameSync(path, target)

  // Anche i file laterali di WAL: lasciarli farebbe ritrovare a SQLite un
  // giornale che non corrisponde al database nuovo.
  for (const suffix of ['-wal', '-shm']) {
    try {
      if (existsSync(`${path}${suffix}`)) renameSync(`${path}${suffix}`, `${target}${suffix}`)
    } catch (cause) {
      log.warn('spostamento del file laterale non riuscito', cause, { suffix })
    }
  }

  return target
}
