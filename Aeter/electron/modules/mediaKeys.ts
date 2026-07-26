import { globalShortcut } from 'electron'
import { getSettings } from './settings'
import { broadcast } from './events'

export function registerMediaKeys(): void {
  globalShortcut.unregisterAll()
  if (!getSettings().globalMediaKeys) return
  globalShortcut.register('MediaPlayPause', () => broadcast('media-key', 'play-pause'))
  globalShortcut.register('MediaNextTrack', () => broadcast('media-key', 'next'))
  globalShortcut.register('MediaPreviousTrack', () => broadcast('media-key', 'previous'))
  globalShortcut.register('MediaStop', () => broadcast('media-key', 'stop'))
}
