import { useEffect, useState } from 'react'
import { coverUrl } from '@/lib/format'

export interface Palette {
  hex: string
  rgb: [number, number, number]
}

export const FALLBACK_PALETTE: Palette = { hex: '#8b7cf6', rgb: [139, 124, 246] }

export async function extractPalette(url: string): Promise<Palette> {
  // Dynamic import: node-vibrant esce dal bundle di avvio; il chunk viene
  // caricato (una volta) alla prima estrazione colore.
  const { Vibrant } = await import('node-vibrant/browser')
  const palette = await Vibrant.from(url).getPalette()
  const swatch = palette.Vibrant ?? palette.LightVibrant ?? palette.Muted ?? palette.DarkVibrant
  if (!swatch) return FALLBACK_PALETTE
  const [r, g, b] = swatch.rgb.map(Math.round) as [number, number, number]
  return { hex: swatch.hex, rgb: [r, g, b] }
}

// --- Palette cache ---------------------------------------------------------
// Vibrant decodes the full cover bitmap and quantizes it on the main thread —
// noticeable work on a phone, and covers are immutable (keyed by content
// hash). Cache the tiny result per hash in localStorage, evicting the least
// recently used beyond PALETTE_CACHE_MAX (~300 entries ≈ a few tens of KB).

const PALETTE_CACHE_KEY = 'aether:palette-cache'
const PALETTE_CACHE_MAX = 300

type PaletteCacheEntry = { p: Palette; at: number }
let paletteCache: Map<string, PaletteCacheEntry> | null = null

function getPaletteCache(): Map<string, PaletteCacheEntry> {
  if (!paletteCache) {
    paletteCache = new Map()
    try {
      const raw = localStorage.getItem(PALETTE_CACHE_KEY)
      if (raw) {
        for (const [k, v] of Object.entries(JSON.parse(raw) as Record<string, PaletteCacheEntry>)) {
          paletteCache.set(k, v)
        }
      }
    } catch {
      // corrupt/unavailable storage: start from an empty cache
    }
  }
  return paletteCache
}

function persistPaletteCache(): void {
  try {
    const c = getPaletteCache()
    if (c.size > PALETTE_CACHE_MAX) {
      const oldestFirst = [...c.entries()].sort((a, b) => a[1].at - b[1].at)
      for (const [k] of oldestFirst.slice(0, c.size - PALETTE_CACHE_MAX)) c.delete(k)
    }
    localStorage.setItem(PALETTE_CACHE_KEY, JSON.stringify(Object.fromEntries(c)))
  } catch {
    // quota/unavailable storage: the in-memory cache still works
  }
}

/** Resolves when the document is visible (immediately when it already is). */
function whenVisible(): Promise<void> {
  if (!document.hidden) return Promise.resolve()
  return new Promise((resolve) => {
    const onVis = (): void => {
      if (!document.hidden) {
        document.removeEventListener('visibilitychange', onVis)
        resolve()
      }
    }
    document.addEventListener('visibilitychange', onVis)
  })
}

/**
 * extractPalette memoized by the cover's content hash. Cache hits are
 * synchronous-cheap; misses defer the bitmap work until the document is
 * visible (extracting a color nobody can see just burns battery).
 */
export async function extractPaletteCached(hash: string, url: string): Promise<Palette> {
  const c = getPaletteCache()
  const hit = c.get(hash)
  if (hit) {
    hit.at = Date.now()
    return hit.p
  }
  await whenVisible()
  const p = await extractPalette(url)
  c.set(hash, { p, at: Date.now() })
  persistPaletteCache()
  return p
}

/**
 * Tint for a page hero derived from its own artwork. Returns a style object
 * carrying --hero-rgb for .hero-scrim — the global accent (set from the
 * playing track by useAccentColor) is untouched.
 */
export function usePagePalette(hash: string | null | undefined): React.CSSProperties {
  const [rgb, setRgb] = useState<[number, number, number]>(FALLBACK_PALETTE.rgb)

  useEffect(() => {
    let cancelled = false
    const url = coverUrl(hash)
    if (!url || !hash) {
      setRgb(FALLBACK_PALETTE.rgb)
      return
    }
    extractPaletteCached(hash, url)
      .then((p) => {
        if (!cancelled) setRgb(p.rgb)
      })
      .catch(() => {
        if (!cancelled) setRgb(FALLBACK_PALETTE.rgb)
      })
    return () => {
      cancelled = true
    }
  }, [hash])

  return { '--hero-rgb': `${rgb[0]} ${rgb[1]} ${rgb[2]}` } as React.CSSProperties
}
