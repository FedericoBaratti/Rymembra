import { describe, it, expect, afterEach } from 'vitest'
import { SKINS, DEFAULT_SKIN, applySkin, getSkin, getSkinMeta } from './skins'

afterEach(() => {
  delete document.documentElement.dataset.skin
})

describe('skin registry', () => {
  it('exposes plain (dynamic accent), nothing and cyberpunk (fixed palettes)', () => {
    expect(SKINS.map((s) => s.id)).toEqual(['plain', 'nothing', 'cyberpunk'])
    expect(getSkinMeta('plain').supportsDynamicAccent).toBe(true)
    expect(getSkinMeta('nothing').supportsDynamicAccent).toBe(false)
    expect(getSkinMeta('cyberpunk').supportsDynamicAccent).toBe(false)
  })

  it('applySkin sets the data-skin attribute and getSkin reads it back', () => {
    applySkin('nothing')
    expect(document.documentElement.dataset.skin).toBe('nothing')
    expect(getSkin()).toBe('nothing')
  })

  it('falls back to the default skin for unknown ids', () => {
    // @ts-expect-error — exercising the runtime guard with an invalid value
    applySkin('bogus')
    expect(document.documentElement.dataset.skin).toBe(DEFAULT_SKIN)
    expect(getSkin()).toBe(DEFAULT_SKIN)
  })
})
