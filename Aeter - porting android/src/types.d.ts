import type { AetherAPI } from '@shared/types'

declare global {
  interface Window {
    aether: AetherAPI
  }

  /** Injected by vite.config.mobile.ts on the Android build; undefined on desktop. */
  const __AETHER_MOBILE__: boolean | undefined

  /** package.json version, injected at build time (vite.config.mobile.ts and electron.vite.config.ts). */
  const __APP_VERSION__: string | undefined
}

export {}
