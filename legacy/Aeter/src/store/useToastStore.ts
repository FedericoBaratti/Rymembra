import { create } from 'zustand'

export type ToastKind = 'success' | 'error' | 'info'

export interface ToastItem {
  id: number
  kind: ToastKind
  title: string
  message?: string
  leaving?: boolean
  /** Auto-dismiss duration in ms; errors stay longer */
  duration: number
}

interface ToastState {
  toasts: ToastItem[]
  push: (kind: ToastKind, title: string, message?: string) => void
  dismiss: (id: number) => void
  pause: (id: number) => void
  resume: (id: number) => void
}

const MAX_STACK = 3
const LEAVE_MS = 200

let nextId = 1
const timers = new Map<number, ReturnType<typeof setTimeout>>()

function clearTimer(id: number): void {
  const t = timers.get(id)
  if (t) {
    clearTimeout(t)
    timers.delete(id)
  }
}

export const useToastStore = create<ToastState>((set, get) => ({
  toasts: [],

  push: (kind, title, message) => {
    const id = nextId++
    const duration = kind === 'error' ? 7000 : 4500
    let toasts = [...get().toasts, { id, kind, title, message, duration }]
    // Evict the oldest non-leaving toast beyond the stack cap
    const active = toasts.filter((t) => !t.leaving)
    if (active.length > MAX_STACK) {
      const oldest = active[0]
      clearTimer(oldest.id)
      toasts = toasts.filter((t) => t.id !== oldest.id)
    }
    set({ toasts })
    timers.set(
      id,
      setTimeout(() => get().dismiss(id), duration)
    )
  },

  dismiss: (id) => {
    clearTimer(id)
    if (!get().toasts.some((t) => t.id === id)) return
    set({ toasts: get().toasts.map((t) => (t.id === id ? { ...t, leaving: true } : t)) })
    setTimeout(() => {
      set({ toasts: get().toasts.filter((t) => t.id !== id) })
    }, LEAVE_MS)
  },

  pause: (id) => {
    clearTimer(id)
  },

  resume: (id) => {
    const item = get().toasts.find((t) => t.id === id)
    if (!item || item.leaving || timers.has(id)) return
    // Restart the full window on mouseleave — simpler than tracking elapsed time
    timers.set(
      id,
      setTimeout(() => get().dismiss(id), item.duration)
    )
  }
}))

/** Imperative facade for callers outside React (bootstrap handlers, stores). */
export const toast = {
  success: (title: string, message?: string) =>
    useToastStore.getState().push('success', title, message),
  error: (title: string, message?: string) =>
    useToastStore.getState().push('error', title, message),
  info: (title: string, message?: string) => useToastStore.getState().push('info', title, message)
}
