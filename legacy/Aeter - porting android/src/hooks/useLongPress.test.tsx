import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, fireEvent } from '@testing-library/react'
import { useLongPress } from './useLongPress'

/**
 * The hook must swallow the tail of the opening gesture (the native
 * `contextmenu` Android fires mid-press and the `click` synthesized on
 * finger-lift) with one-shot capture-phase window suppressors — but ONLY that
 * tail: later legitimate events must pass through untouched.
 */

function Probe({ onLongPress }: { onLongPress: (e: React.PointerEvent) => void }): React.JSX.Element {
  const handlers = useLongPress(onLongPress)
  return <div data-testid="probe" {...handlers} />
}

const touch = { pointerType: 'touch', clientX: 10, clientY: 10 }

function dispatch(type: 'click' | 'contextmenu'): MouseEvent {
  const evt = new MouseEvent(type, { bubbles: true, cancelable: true })
  document.body.dispatchEvent(evt)
  return evt
}

describe('useLongPress', () => {
  const clickSpy = vi.fn()
  const ctxSpy = vi.fn()

  beforeEach(() => {
    vi.useFakeTimers()
    // Bubble-phase window listeners stand in for TrackList's raw
    // "close on outside click/contextmenu" listeners.
    window.addEventListener('click', clickSpy)
    window.addEventListener('contextmenu', ctxSpy)
  })

  afterEach(() => {
    window.removeEventListener('click', clickSpy)
    window.removeEventListener('contextmenu', ctxSpy)
    clickSpy.mockReset()
    ctxSpy.mockReset()
    vi.useRealTimers()
  })

  it('fires once after the delay on touch', () => {
    const cb = vi.fn()
    const { getByTestId } = render(<Probe onLongPress={cb} />)
    fireEvent.pointerDown(getByTestId('probe'), touch)
    vi.advanceTimersByTime(449)
    expect(cb).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(cb).toHaveBeenCalledTimes(1)
  })

  it('does not fire on a short tap, and the tap click passes through', () => {
    const cb = vi.fn()
    const { getByTestId } = render(<Probe onLongPress={cb} />)
    fireEvent.pointerDown(getByTestId('probe'), touch)
    vi.advanceTimersByTime(200)
    fireEvent.pointerUp(getByTestId('probe'))
    vi.advanceTimersByTime(1000)
    expect(cb).not.toHaveBeenCalled()
    const evt = dispatch('click')
    expect(clickSpy).toHaveBeenCalledTimes(1)
    expect(evt.defaultPrevented).toBe(false)
  })

  it('never arms for mouse (native right-click path stays intact)', () => {
    const cb = vi.fn()
    const { getByTestId } = render(<Probe onLongPress={cb} />)
    fireEvent.pointerDown(getByTestId('probe'), { ...touch, pointerType: 'mouse' })
    vi.advanceTimersByTime(1000)
    expect(cb).not.toHaveBeenCalled()
    dispatch('contextmenu')
    expect(ctxSpy).toHaveBeenCalledTimes(1)
  })

  it('suppresses the native contextmenu after firing, one-shot', () => {
    const { getByTestId } = render(<Probe onLongPress={vi.fn()} />)
    fireEvent.pointerDown(getByTestId('probe'), touch)
    vi.advanceTimersByTime(450)
    // Android's native long-press contextmenu, ~50ms after our timer.
    const first = dispatch('contextmenu')
    expect(ctxSpy).not.toHaveBeenCalled()
    expect(first.defaultPrevented).toBe(true)
    // One-shot: a second contextmenu reaches listeners normally.
    const second = dispatch('contextmenu')
    expect(ctxSpy).toHaveBeenCalledTimes(1)
    expect(second.defaultPrevented).toBe(false)
  })

  it('suppresses the click synthesized on finger-lift, one-shot', () => {
    const { getByTestId } = render(<Probe onLongPress={vi.fn()} />)
    fireEvent.pointerDown(getByTestId('probe'), touch)
    vi.advanceTimersByTime(450)
    fireEvent.pointerUp(getByTestId('probe'))
    const first = dispatch('click')
    expect(clickSpy).not.toHaveBeenCalled()
    expect(first.defaultPrevented).toBe(true)
    const second = dispatch('click')
    expect(clickSpy).toHaveBeenCalledTimes(1)
    expect(second.defaultPrevented).toBe(false)
  })

  it('reaps armed suppressors after the post-lift grace if no click ever comes', () => {
    const { getByTestId } = render(<Probe onLongPress={vi.fn()} />)
    fireEvent.pointerDown(getByTestId('probe'), touch)
    vi.advanceTimersByTime(450)
    fireEvent.pointerUp(getByTestId('probe'))
    // Finger moved after firing → the browser never synthesizes the click.
    vi.advanceTimersByTime(150)
    dispatch('click')
    dispatch('contextmenu')
    expect(clickSpy).toHaveBeenCalledTimes(1)
    expect(ctxSpy).toHaveBeenCalledTimes(1)
  })

  it('disarms immediately on pointercancel (no click follows a cancel)', () => {
    const { getByTestId } = render(<Probe onLongPress={vi.fn()} />)
    fireEvent.pointerDown(getByTestId('probe'), touch)
    vi.advanceTimersByTime(450)
    fireEvent.pointerCancel(getByTestId('probe'))
    dispatch('click')
    dispatch('contextmenu')
    expect(clickSpy).toHaveBeenCalledTimes(1)
    expect(ctxSpy).toHaveBeenCalledTimes(1)
  })

  it('a rapid re-press within the grace window reclaims its own tap click', () => {
    const { getByTestId } = render(<Probe onLongPress={vi.fn()} />)
    fireEvent.pointerDown(getByTestId('probe'), touch)
    vi.advanceTimersByTime(450)
    fireEvent.pointerUp(getByTestId('probe'))
    // Immediately tap again: the new pointerdown must disarm the old suppressor.
    fireEvent.pointerDown(getByTestId('probe'), touch)
    vi.advanceTimersByTime(100)
    fireEvent.pointerUp(getByTestId('probe'))
    const evt = dispatch('click')
    expect(clickSpy).toHaveBeenCalledTimes(1)
    expect(evt.defaultPrevented).toBe(false)
  })

  it('removes armed suppressors on unmount', () => {
    const { getByTestId, unmount } = render(<Probe onLongPress={vi.fn()} />)
    fireEvent.pointerDown(getByTestId('probe'), touch)
    vi.advanceTimersByTime(450)
    unmount()
    dispatch('contextmenu')
    expect(ctxSpy).toHaveBeenCalledTimes(1)
  })
})
