import { create } from 'zustand'
import { useSettingsStore } from '@/store/useSettingsStore'
import { TOUR_STEPS } from '@/components/onboarding/tourSteps'

interface TourState {
  active: boolean
  stepIndex: number
  start: () => void
  next: () => void
  back: () => void
  finish: () => void
}

export const useTourStore = create<TourState>((set, get) => ({
  active: false,
  stepIndex: 0,

  start: () => set({ active: true, stepIndex: 0 }),

  next: () => {
    const { stepIndex, finish } = get()
    if (stepIndex >= TOUR_STEPS.length - 1) finish()
    else set({ stepIndex: stepIndex + 1 })
  },

  back: () => {
    const { stepIndex } = get()
    if (stepIndex > 0) set({ stepIndex: stepIndex - 1 })
  },

  finish: () => {
    set({ active: false, stepIndex: 0 })
    void useSettingsStore.getState().update({ hasSeenOnboarding: true })
  }
}))
