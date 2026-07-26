import { create } from 'zustand'
import { useSettingsStore } from '@/store/useSettingsStore'
import { getTourSteps } from '@/components/onboarding/tourSteps'

interface TourState {
  active: boolean
  stepIndex: number
  start: () => void
  next: () => void
  back: () => void
  goToStep: (index: number) => void
  finish: () => void
}

export const useTourStore = create<TourState>((set, get) => ({
  active: false,
  stepIndex: 0,

  start: () => set({ active: true, stepIndex: 0 }),

  next: () => {
    const { stepIndex, finish } = get()
    if (stepIndex >= getTourSteps().length - 1) finish()
    else set({ stepIndex: stepIndex + 1 })
  },

  back: () => {
    const { stepIndex } = get()
    if (stepIndex > 0) set({ stepIndex: stepIndex - 1 })
  },

  goToStep: (index) => {
    const last = getTourSteps().length - 1
    set({ stepIndex: Math.min(Math.max(index, 0), last) })
  },

  finish: () => {
    set({ active: false, stepIndex: 0 })
    void useSettingsStore.getState().update({ hasSeenOnboarding: true })
  }
}))
