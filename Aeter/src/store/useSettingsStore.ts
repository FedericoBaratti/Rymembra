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
    const settings = await window.aether.getSettings()
    set({ settings })
    return settings
  },

  update: async (patch) => {
    const current = get().settings
    if (current) set({ settings: { ...current, ...patch } })
    const settings = await window.aether.setSettings(patch)
    set({ settings })
  }
}))
