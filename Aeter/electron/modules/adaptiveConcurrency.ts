/**
 * Thermal-aware concurrency governor for CPU-heavy background work (library
 * scan, enrichment, cover maintenance).
 *
 * On Android, ThermalPlugin.kt maps the OS thermal status (PowerManager
 * THERMAL_STATUS_*) to a coarse level and the renderer forwards it here via the
 * `thermalUpdate` IPC (electron/ipc/thermal.ipc.ts). Consumers ask this
 * singleton how many parallel workers they may run instead of hardcoding a
 * count, and may subscribe to live changes to retune an already-running queue.
 *
 * On desktop nothing ever pushes an update, so the level stays 'normal' and
 * every queue keeps its configured concurrency — the module is behaviorally
 * inert there. Devices without a thermal API (Android < 10) likewise never
 * report, which must keep the app fully functional: absence of data is always
 * treated as 'normal'.
 */

import type { ThermalLevel, ThermalState } from '@shared/types'

export type { ThermalLevel, ThermalState }

/** With no fresh sample for this long, fail open back to 'normal': the native
 *  listener may be dead, or the device cooled while the WebView was frozen and
 *  the recovery update never crossed the bridge. */
const STALE_MS = 120_000

type ChangeListener = (state: ThermalState) => void

export class AdaptiveConcurrencyManager {
  private state: ThermalState = { level: 'normal', timestamp: 0 }
  private listeners: ChangeListener[] = []
  private staleTimer: ReturnType<typeof setTimeout> | null = null

  /** Records a new sample. Returns the stored state. Listeners fire only when
   *  the level actually changes, so queues aren't retuned on every heartbeat. */
  updateState(sample: { level: ThermalLevel; headroom?: number }): ThermalState {
    const prev = this.state.level
    this.state = { level: sample.level, headroom: sample.headroom, timestamp: Date.now() }
    this.armStaleTimer()
    if (sample.level !== prev) this.notify()
    return this.state
  }

  getState(): ThermalState {
    return this.state
  }

  /**
   * How many workers a queue configured for `base` may run right now:
   * normal → base, warning → half (rounded up), critical → 1.
   */
  getConcurrency(base: number): number {
    switch (this.state.level) {
      case 'critical':
        return 1
      case 'warning':
        return Math.max(1, Math.ceil(base / 2))
      default:
        return base
    }
  }

  /** True when optional background work (e.g. a fresh auto-enrich batch)
   *  should wait for the device to cool rather than start now. */
  shouldDefer(_operation: string): boolean {
    return this.state.level === 'critical'
  }

  /** Subscribe to level changes. Returns the unsubscribe function. */
  onChange(fn: ChangeListener): () => void {
    this.listeners.push(fn)
    return () => {
      const i = this.listeners.indexOf(fn)
      if (i >= 0) this.listeners.splice(i, 1)
    }
  }

  private notify(): void {
    for (const fn of this.listeners) {
      try {
        fn(this.state)
      } catch (err) {
        console.warn('[thermal] onChange listener failed', err)
      }
    }
  }

  /** While throttled, arm a one-shot fallback to 'normal' after STALE_MS —
   *  without it a dead native listener would pin queues at concurrency 1
   *  forever. Re-armed on every update; inert (no timer at all) at 'normal'. */
  private armStaleTimer(): void {
    if (this.staleTimer) {
      clearTimeout(this.staleTimer)
      this.staleTimer = null
    }
    if (this.state.level === 'normal') return
    this.staleTimer = setTimeout(() => {
      this.staleTimer = null
      this.updateState({ level: 'normal' })
    }, STALE_MS)
    this.staleTimer.unref?.()
  }
}

/** Process-wide instance every consumer shares. */
export const thermalManager = new AdaptiveConcurrencyManager()
