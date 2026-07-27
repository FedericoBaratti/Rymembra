/**
 * Le implementazioni del contratto.
 *
 * La proprietà da notare è come ognuna tratta un database che potrebbe non
 * essersi aperto: `db.get()` restituisce un Result e l'handler lo propaga. Nel
 * legacy `registerLibraryIpc()` chiamava `getDb()` PRIMA di registrare qualunque
 * cosa, quindi un database guasto significava zero handler e un renderer in
 * attesa per sempre.
 *
 * Qui la diagnostica e le impostazioni funzionano comunque — ed è da lì che
 * l'utente scopre cosa è andato storto.
 */

import {
  AppError,
  err,
  ok,
  queryOne,
  recentLogs,
  type Capabilities,
  type DbHandle,
  type HandlerMap,
  type Result
} from '@aether/core'
import { compileSkin, parseSkin, type SkinDocument } from '@aether/skin'
import type { CONTRACT } from './contract'

export interface SettingsStore {
  read(): {
    skin: string
    theme: 'dark' | 'light'
    volume: number
    motionIntensity: 'none' | 'essential' | 'full' | 'maximum'
  }
  write(patch: Record<string, unknown>): void
}

export interface HandlerDeps {
  readonly db: DbHandle
  readonly capabilities: Capabilities
  readonly settings: SettingsStore
  readonly dbPath: string
  /** Le skin disponibili, per id. Nella Fase 4 arriveranno da `builtin/`. */
  readonly skins: ReadonlyMap<string, { document: SkinDocument; builtin: boolean }>
}

function dbStatus(deps: HandlerDeps): {
  status: 'closed' | 'open' | 'failed'
  version: number
  fts5: boolean
  path: string
  errorCode?: string
} {
  const current = deps.db.get()
  if (current.ok) {
    return {
      status: 'open',
      version: current.value.version,
      fts5: current.value.fts5,
      path: deps.dbPath
    }
  }
  // Il guasto arriva al renderer come CODICE, non come messaggio: la frase la
  // costruisce la UI dalla chiave i18n, e il codice resta confrontabile.
  return {
    status: deps.db.status,
    version: 0,
    fts5: false,
    path: deps.dbPath,
    errorCode: current.error.code
  }
}

/** Conta le righe di una tabella, o zero se la tabella non c'è ancora. */
function countRows(deps: HandlerDeps, table: 'tracks' | 'albums' | 'artists'): Result<number, AppError> {
  const current = deps.db.get()
  if (!current.ok) return err(current.error)
  // Il nome della tabella viene da un'unione chiusa, non dall'ingresso: è l'unico
  // modo lecito di interpolarlo in SQL.
  const result = queryOne(current.value.driver, `SELECT COUNT(*) AS n FROM ${table}`)
  if (!result.ok) return err(result.error)
  const value = result.value?.['n']
  return ok(typeof value === 'number' ? value : 0)
}

export function createHandlers(deps: HandlerDeps): HandlerMap<typeof CONTRACT> {
  return {
    'diagnostics:db': () => dbStatus(deps),

    'diagnostics:capabilities': () => ({
      label: deps.capabilities.label,
      fts5: deps.capabilities.search.fts5,
      spawn: deps.capabilities.system.spawn,
      skinStudio: deps.capabilities.appearance.skinStudio,
      lanServer: deps.capabilities.network.lanServer
    }),

    'diagnostics:recentLogs': ({ limit }) =>
      recentLogs()
        .slice(-limit)
        .map((record) => ({
          ts: record.ts,
          level: record.level,
          scope: record.scope,
          message: record.message,
          ...(record.error !== undefined ? { errorCode: record.error.code } : {})
        })),

    'diagnostics:reopenDb': () => {
      deps.db.reopen()
      return dbStatus(deps)
    },

    'settings:get': () => deps.settings.read(),

    'settings:set': (patch) => {
      deps.settings.write(patch)
      return deps.settings.read()
    },

    'skins:list': () =>
      [...deps.skins.entries()].map(([id, entry]) => ({
        id,
        name: entry.document.meta.name,
        author: entry.document.meta.author,
        version: entry.document.meta.version,
        builtin: entry.builtin
      })),

    'skins:css': ({ id }) => {
      const entry = deps.skins.get(id)
      if (entry === undefined) return err(AppError.of('skin.notFound', { id }))
      const compiled = compileSkin(entry.document)
      if (!compiled.ok) return err(compiled.error)
      return ok({ id, css: compiled.value.css, cost: compiled.value.cost })
    },

    'library:counts': () => {
      const tracks = countRows(deps, 'tracks')
      if (!tracks.ok) return err(tracks.error)
      const albums = countRows(deps, 'albums')
      if (!albums.ok) return err(albums.error)
      const artists = countRows(deps, 'artists')
      if (!artists.ok) return err(artists.error)
      return ok({ tracks: tracks.value, albums: albums.value, artists: artists.value })
    }
  }
}

/**
 * Carica una skin da un documento non fidato, riportando il motivo del rifiuto.
 *
 * Esportata perché la usano sia il caricamento delle built-in all'avvio sia
 * l'importazione di un pacchetto: lo stesso percorso, quindi una built-in
 * malformata viene scoperta in sviluppo con lo stesso messaggio che vedrebbe
 * l'utente.
 */
export function loadSkinDocument(raw: unknown): Result<SkinDocument, AppError> {
  return parseSkin(raw)
}
