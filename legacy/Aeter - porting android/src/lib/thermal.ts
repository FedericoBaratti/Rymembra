import { registerPlugin } from '@capacitor/core'
import type { ThermalLevel } from '@shared/types'

/**
 * Relays device thermal levels from ThermalPlugin.kt to the node backend.
 *
 * Native side maps PowerManager THERMAL_STATUS_* to normal/warning/critical and
 * emits `thermalchanged` only on transitions; each sample is forwarded to the
 * backend's `thermalUpdate` IPC, where adaptiveConcurrency.ts retunes the scan
 * and enrichment queues (electron/ipc/thermal.ipc.ts). Every call is
 * best-effort: a device without the thermal API or an unreachable backend must
 * never surface an error — no thermal data simply means full speed.
 */

interface ThermalSample {
  level: ThermalLevel
  headroom?: number
  timestamp: number
}

interface ThermalPluginNative {
  getState(): Promise<ThermalSample>
  addListener(
    eventName: 'thermalchanged',
    cb: (sample: ThermalSample) => void
  ): Promise<{ remove: () => Promise<void> }>
}

export function initThermal(usedLan: boolean): void {
  // LAN mode: the heavy work (scan/enrichment) runs on the paired desktop, and
  // the local node backend isn't even started — nothing to throttle here. The
  // native-side mitigations (1 Hz tick, crossfade skip) work regardless.
  if (usedLan) return
  const Thermal = registerPlugin<ThermalPluginNative>('Thermal')
  const forward = (sample: ThermalSample): void => {
    console.debug('[thermal]', sample.level, sample.headroom ?? '')
    window.aether
      .thermalUpdate({ level: sample.level, headroom: sample.headroom })
      .catch(() => {})
  }
  void Thermal.addListener('thermalchanged', forward).catch(() => {})
  // Seed the backend with the current level: the listener only fires on
  // transitions, so an app opened on an already-hot device would otherwise
  // run at 'normal' until the next change.
  Thermal.getState().then(forward).catch(() => {})
}
