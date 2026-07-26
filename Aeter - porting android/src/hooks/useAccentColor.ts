import { useEffect } from 'react'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useSettingsStore } from '@/store/useSettingsStore'
import { coverUrl } from '@/lib/format'
import { extractPaletteCached, FALLBACK_PALETTE } from '@/hooks/usePalette'
import { getSkinMeta, DEFAULT_SKIN } from '@/lib/skins'

const ACCENT_VARS = [
  '--accent',
  '--accent-rgb',
  '--accent-soft',
  '--accent-glow',
  '--ambient-1',
  '--ambient-2'
] as const

function applyAccent(hex: string, rgb: [number, number, number]): void {
  const root = document.documentElement.style
  const [r, g, b] = rgb
  root.setProperty('--accent', hex)
  root.setProperty('--accent-rgb', `${r} ${g} ${b}`)
  root.setProperty('--accent-soft', `rgba(${r}, ${g}, ${b}, 0.16)`)
  root.setProperty('--accent-glow', `rgba(${r}, ${g}, ${b}, 0.35)`)
  root.setProperty('--ambient-1', `rgba(${r}, ${g}, ${b}, 0.12)`)
  root.setProperty('--ambient-2', `rgba(${Math.round(r * 0.5)}, ${Math.round(g * 0.5)}, ${Math.round(b * 0.6)}, 0.08)`)
}

/**
 * Removes the inline accent overrides so the values from the active skin's CSS
 * (`:root[data-skin='…']`) take effect. Inline custom properties beat stylesheet
 * rules, so a monochrome skin (Nothing) only stays monochrome if we clear these.
 */
function clearAccent(): void {
  const root = document.documentElement.style
  for (const v of ACCENT_VARS) root.removeProperty(v)
}

/**
 * Derives the dynamic accent from the playing track's cover art — but only for
 * skins that opt into it (see SKINS in src/lib/skins.ts). Monochrome skins clear
 * the inline accent instead, letting their fixed CSS accent show through. Re-runs
 * on both cover and skin changes.
 */
export function useAccentColor(): void {
  const hash = usePlayerStore((s) => s.currentTrack?.cover_art_hash ?? null)
  const skin = useSettingsStore((s) => s.settings?.skin ?? DEFAULT_SKIN)

  useEffect(() => {
    if (!getSkinMeta(skin).supportsDynamicAccent) {
      clearAccent()
      return
    }

    let cancelled = false
    const url = coverUrl(hash)
    if (!url || !hash) {
      applyAccent(FALLBACK_PALETTE.hex, FALLBACK_PALETTE.rgb)
      return
    }
    extractPaletteCached(hash, url)
      .then((p) => {
        if (!cancelled) applyAccent(p.hex, p.rgb)
      })
      .catch(() => {
        if (!cancelled) applyAccent(FALLBACK_PALETTE.hex, FALLBACK_PALETTE.rgb)
      })
    return () => {
      cancelled = true
    }
  }, [hash, skin])
}
