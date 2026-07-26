import '@testing-library/jest-dom/vitest'
import { afterEach, vi } from 'vitest'
import { cleanup } from '@testing-library/react'
import type { AetherAPI } from '@aether/core'

// react-testing-library only auto-registers cleanup with vitest globals on.
afterEach(() => cleanup())

/** Installs window.aether as vi.fn() stubs; pass overrides per test. */
export function mockAether(overrides: Partial<AetherAPI> = {}): AetherAPI {
  const api = new Proxy(
    {},
    {
      get(target: Record<string | symbol, unknown>, prop) {
        if (!(prop in target)) target[prop] = vi.fn(() => Promise.resolve(undefined))
        return target[prop]
      }
    }
  ) as unknown as AetherAPI
  Object.assign(api, { on: vi.fn(() => () => {}) }, overrides)
  ;(window as unknown as { aether: AetherAPI }).aether = api
  return api
}

mockAether()

// jsdom lacks matchMedia and mediaSession, both touched during bootstrap.
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn()
  }))
})

Object.defineProperty(navigator, 'mediaSession', {
  writable: true,
  value: { setActionHandler: vi.fn(), metadata: null, playbackState: 'none' }
})

// jsdom implements no scrolling APIs (LyricsView auto-centers via scrollTo).
Element.prototype.scrollTo = vi.fn()
