import type { AetherAPI } from '@shared/types'

declare global {
  interface Window {
    aether: AetherAPI
  }
}

export {}
