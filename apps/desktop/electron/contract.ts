/**
 * Il contratto dell'app desktop.
 *
 * È la fetta verticale che prova l'architettura: pochi canali, ma reali —
 * diagnostica, impostazioni, skin. Da qui si derivano il tipo di `window.aether`,
 * i nomi dei canali e la validazione, e un handler dimenticato non compila.
 *
 * Nel legacy questa stessa informazione stava in quattro posti da allineare a
 * mano — `shared/ipcMethods.ts`, l'interfaccia `AetherAPI` di 200 righe,
 * 105 chiamate `handle()` e il ciclo del preload chiuso da un cast. Qui c'è una
 * volta.
 */

import { channel } from '@aether/core'
import { z } from 'zod'

/** Lo stato del database, per il pannello diagnostico. */
const dbStatusSchema = z.object({
  status: z.enum(['closed', 'open', 'failed']),
  version: z.number(),
  fts5: z.boolean(),
  path: z.string(),
  /** Presente quando l'apertura è fallita: il codice, non un messaggio. */
  errorCode: z.string().optional()
})

const capabilitiesSchema = z.object({
  label: z.string(),
  fts5: z.boolean(),
  spawn: z.boolean(),
  skinStudio: z.boolean(),
  lanServer: z.boolean()
})

const logRecordSchema = z.object({
  ts: z.number(),
  level: z.string(),
  scope: z.string(),
  message: z.string(),
  errorCode: z.string().optional()
})

const settingsSchema = z.object({
  skin: z.string(),
  theme: z.enum(['dark', 'light']),
  volume: z.number().min(0).max(1),
  motionIntensity: z.enum(['none', 'essential', 'full', 'maximum'])
})

const skinSummarySchema = z.object({
  id: z.string(),
  name: z.string(),
  author: z.string(),
  version: z.string(),
  builtin: z.boolean()
})

export const CONTRACT = {
  /** Stato del DB. Risponde ANCHE quando il DB non si è aperto: è il punto. */
  'diagnostics:db': channel(z.void(), dbStatusSchema),
  /** Le capacità della piattaforma, così il renderer non indovina. */
  'diagnostics:capabilities': channel(z.void(), capabilitiesSchema),
  /** Le ultime righe di log dal ring buffer, senza rileggere il file. */
  'diagnostics:recentLogs': channel(
    z.object({ limit: z.number().int().min(1).max(500).default(100) }),
    z.array(logRecordSchema)
  ),
  /** Riprova ad aprire il database. Per i lock e i volumi che tornano. */
  'diagnostics:reopenDb': channel(z.void(), dbStatusSchema),

  'settings:get': channel(z.void(), settingsSchema),
  'settings:set': channel(settingsSchema.partial(), settingsSchema),

  'skins:list': channel(z.void(), z.array(skinSummarySchema)),
  /** Il CSS compilato di una skin, pronto per adoptedStyleSheets. */
  'skins:css': channel(
    z.object({ id: z.string() }),
    z.object({ id: z.string(), css: z.string(), cost: z.number() }),
    // Validazione dell'uscita accesa: qui il dato viene da un pacchetto skin,
    // cioè da fuori.
    { validateOutput: true }
  ),

  'library:counts': channel(
    z.void(),
    z.object({ tracks: z.number(), albums: z.number(), artists: z.number() })
  )
} as const
