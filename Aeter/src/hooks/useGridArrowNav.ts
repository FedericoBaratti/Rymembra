import { useCallback } from 'react'

/**
 * Arrow-key navigation between focusable cards in a grid container.
 * Attach the returned handler to the container's onKeyDown: ArrowLeft/Right
 * move focus between direct card buttons, Home/End jump to first/last.
 */
export function useGridArrowNav(): (e: React.KeyboardEvent<HTMLElement>) => void {
  return useCallback((e: React.KeyboardEvent<HTMLElement>) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return
    const container = e.currentTarget
    const cards = [...container.querySelectorAll<HTMLElement>(':scope > button, :scope > a')]
    if (cards.length === 0) return
    const idx = cards.indexOf(document.activeElement as HTMLElement)
    if (idx < 0) return
    e.preventDefault()
    let next = idx
    if (e.key === 'ArrowLeft') next = Math.max(0, idx - 1)
    else if (e.key === 'ArrowRight') next = Math.min(cards.length - 1, idx + 1)
    else if (e.key === 'Home') next = 0
    else next = cards.length - 1
    cards[next].focus()
  }, [])
}
