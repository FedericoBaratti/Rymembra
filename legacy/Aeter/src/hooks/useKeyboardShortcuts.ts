import { useEffect } from 'react'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useUiStore } from '@/store/useUiStore'
import { useTourStore } from '@/store/useTourStore'

function isTyping(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null
  if (!t) return false
  return (
    t.tagName === 'INPUT' ||
    t.tagName === 'TEXTAREA' ||
    t.tagName === 'SELECT' ||
    t.isContentEditable
  )
}

export function useKeyboardShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent): void => {
      if (useTourStore.getState().active) return
      const player = usePlayerStore.getState()
      const ui = useUiStore.getState()
      const mod = e.ctrlKey || e.metaKey

      if (mod && e.key.toLowerCase() === 'f') {
        e.preventDefault()
        ui.setSearchOpen(true)
        return
      }
      if (mod && e.key.toLowerCase() === 'l') {
        e.preventDefault()
        ui.setQueueOpen(true)
        return
      }

      if (isTyping(e)) return

      switch (e.key) {
        case ' ':
          e.preventDefault()
          player.togglePlay()
          break
        case 'ArrowLeft':
          e.preventDefault()
          if (e.shiftKey) player.previous()
          else player.seekBy(-5)
          break
        case 'ArrowRight':
          e.preventDefault()
          if (e.shiftKey) player.next()
          else player.seekBy(5)
          break
        case 'ArrowUp':
          e.preventDefault()
          player.adjustVolume(0.05)
          break
        case 'ArrowDown':
          e.preventDefault()
          player.adjustVolume(-0.05)
          break
        case 'm':
        case 'M':
          player.toggleMute()
          break
        case 'f':
        case 'F':
          if (player.currentTrack) ui.setFullscreenViz(!ui.fullscreenViz)
          break
        case 'r':
        case 'R':
          player.cycleRepeat()
          break
        case 's':
        case 'S':
          player.toggleShuffle()
          break
        case 'Escape':
          if (ui.fullscreenViz) ui.setFullscreenViz(false)
          else if (ui.searchOpen) ui.setSearchOpen(false)
          else if (ui.queueOpen) ui.setQueueOpen(false)
          else if (ui.eqOpen) ui.setEqOpen(false)
          break
      }
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])
}
