import { z } from 'zod'
import { handle } from './handle'
import { broadcast } from '../modules/events'
import { thermalManager, type ThermalState } from '../modules/adaptiveConcurrency'

// Samples come from the WebView (src/lib/thermal.ts ← ThermalPlugin.kt), i.e.
// from outside the backend's type system — validate before trusting.
const thermalSampleSchema = z.object({
  level: z.enum(['normal', 'warning', 'critical']),
  headroom: z.number().finite().optional()
})

export function registerThermalIpc(): void {
  // Level transitions are worth a log line (spec: keep thermal samples
  // traceable) and a renderer broadcast for UI/debugging.
  thermalManager.onChange((state) => {
    // Rare (level transitions only) — goes to logcat on device.
    console.log(
      `[thermal] livello ${state.level}` +
        (state.headroom != null ? ` (headroom ${state.headroom.toFixed(2)})` : '')
    )
    broadcast('thermal:changed', state)
  })

  handle('thermalUpdate', (_e, sample: unknown): ThermalState => {
    return thermalManager.updateState(thermalSampleSchema.parse(sample))
  })
  handle('getThermalState', (): ThermalState => thermalManager.getState())
}
