import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest'
import { useToastStore, toast } from './useToastStore'

describe('useToastStore', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    useToastStore.setState({ toasts: [] })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('pushes toasts with kind-dependent duration', () => {
    toast.success('Salvato')
    toast.error('Errore grave')
    const [ok, err] = useToastStore.getState().toasts
    expect(ok.kind).toBe('success')
    expect(ok.duration).toBe(4500)
    expect(err.kind).toBe('error')
    expect(err.duration).toBe(7000)
  })

  it('evicts the oldest toast beyond the stack cap', () => {
    toast.info('uno')
    toast.info('due')
    toast.info('tre')
    toast.info('quattro')
    const titles = useToastStore.getState().toasts.map((t) => t.title)
    expect(titles).toEqual(['due', 'tre', 'quattro'])
  })

  it('auto-dismisses after its duration plus the leave animation', () => {
    toast.success('Effimero')
    expect(useToastStore.getState().toasts).toHaveLength(1)
    vi.advanceTimersByTime(4500)
    expect(useToastStore.getState().toasts[0].leaving).toBe(true)
    vi.advanceTimersByTime(200)
    expect(useToastStore.getState().toasts).toHaveLength(0)
  })

  it('pause stops the timer and resume restarts the full window', () => {
    toast.success('In hover')
    const id = useToastStore.getState().toasts[0].id
    useToastStore.getState().pause(id)
    vi.advanceTimersByTime(10_000)
    expect(useToastStore.getState().toasts).toHaveLength(1)
    useToastStore.getState().resume(id)
    vi.advanceTimersByTime(4500 + 200)
    expect(useToastStore.getState().toasts).toHaveLength(0)
  })
})
