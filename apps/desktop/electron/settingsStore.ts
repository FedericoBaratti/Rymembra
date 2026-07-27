/**
 * Le impostazioni su file, con scrittura atomica.
 *
 * Perché atomica e non un `writeFileSync` diretto: nel legacy le impostazioni si
 * salvavano su debounce, e i quattro flush di chiusura non erano guardati — un
 * throw nel primo saltava gli altri tre. Se la corrente andava a metà scrittura,
 * il file restava troncato e all'avvio successivo l'app perdeva tutto senza dire
 * niente.
 *
 * Scrivere su un temporaneo e poi rinominare rende la sostituzione indivisibile:
 * o si legge il file vecchio, o quello nuovo. Mai metà.
 */

import { existsSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { logger } from '@aether/core'
import { z } from 'zod'
import type { SettingsStore } from './handlers'

const log = logger('settings')

const settingsSchema = z.object({
  skin: z.string().default('plain'),
  theme: z.enum(['dark', 'light']).default('dark'),
  volume: z.number().min(0).max(1).default(0.8),
  motionIntensity: z.enum(['none', 'essential', 'full', 'maximum']).default('full')
})

export type Settings = z.infer<typeof settingsSchema>

const DEFAULTS: Settings = {
  skin: 'plain',
  theme: 'dark',
  volume: 0.8,
  motionIntensity: 'full'
}

export function createSettingsStore(userData: string): SettingsStore {
  const path = join(userData, 'settings.json')
  let current = read()

  function read(): Settings {
    if (!existsSync(path)) return DEFAULTS
    try {
      const parsed = settingsSchema.safeParse(JSON.parse(readFileSync(path, 'utf8')))
      if (parsed.success) return parsed.data
      // Un file corrotto o scritto da una versione futura non deve impedire
      // l'avvio: si riparte dai valori di base e lo si dice.
      log.warn('impostazioni non valide, si usano i valori di base', undefined, {
        detail: parsed.error.issues[0]?.message
      })
      return DEFAULTS
    } catch (cause) {
      log.warn('lettura delle impostazioni non riuscita', cause, { path })
      return DEFAULTS
    }
  }

  function persist(): void {
    const temporary = `${path}.tmp`
    try {
      writeFileSync(temporary, `${JSON.stringify(current, null, 2)}\n`, 'utf8')
      renameSync(temporary, path)
    } catch (cause) {
      // Va detto, e va detto come errore: perdere le impostazioni in silenzio è
      // esattamente il comportamento che stiamo eliminando.
      log.error('salvataggio delle impostazioni non riuscito', cause, { path })
    }
  }

  return {
    read: () => current,
    write: (patch) => {
      const merged = settingsSchema.safeParse({ ...current, ...patch })
      if (!merged.success) {
        log.warn('modifica alle impostazioni rifiutata', undefined, {
          detail: merged.error.issues[0]?.message
        })
        return
      }
      current = merged.data
      persist()
    }
  }
}
