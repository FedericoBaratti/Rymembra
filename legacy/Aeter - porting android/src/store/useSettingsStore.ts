import { create } from 'zustand'
import type { AppSettings } from '@shared/types'

interface SettingsState {
  settings: AppSettings | null
  load: () => Promise<AppSettings>
  update: (patch: Partial<AppSettings>) => Promise<void>
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,

  load: async () => {
    // Retry on transient rejections: on cold start the backend may still be
    // booting/registering IPC handlers when the first request arrives. Without
    // this a single early failure would leave `settings` null forever and the
    // app stuck (no theme/volume/language applied). The backend's getSettings
    // always resolves once it is up (falls back to defaults on a bad file).
    let lastErr: unknown
    for (let attempt = 0; attempt < 5; attempt++) {
      try {
        const settings = await window.aether.getSettings()
        set({ settings })
        return settings
      } catch (err) {
        lastErr = err
        await new Promise((r) => setTimeout(r, 300))
      }
    }
    console.error('[settings] load failed after retries', lastErr)
    throw lastErr
  },

  update: async (patch) => {
    const current = get().settings
    if (current) set({ settings: { ...current, ...patch } })
    const settings = await window.aether.setSettings(patch)
    set({ settings })
  }
}))
