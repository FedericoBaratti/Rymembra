import { useEffect, useState } from 'react'
import { Vibrant } from 'node-vibrant/browser'
import { coverUrl } from '@/lib/format'

export interface Palette {
  hex: string
  rgb: [number, number, number]
}

export const FALLBACK_PALETTE: Palette = { hex: '#8b7cf6', rgb: [139, 124, 246] }

export async function extractPalette(url: string): Promise<Palette> {
  const palette = await Vibrant.from(url).getPalette()
  const swatch = palette.Vibrant ?? palette.LightVibrant ?? palette.Muted ?? palette.DarkVibrant
  if (!swatch) return FALLBACK_PALETTE
  const [r, g, b] = swatch.rgb.map(Math.round) as [number, number, number]
  return { hex: swatch.hex, rgb: [r, g, b] }
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
    if (!url) {
      setRgb(FALLBACK_PALETTE.rgb)
      return
    }
    extractPalette(url)
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
