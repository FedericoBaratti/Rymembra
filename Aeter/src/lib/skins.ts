import type { SkinId } from '@shared/types'

export type { SkinId }

/**
 * A skin is a visual style layer applied via the `data-skin` attribute on
 * <html>, orthogonal to the light/dark `data-theme`. Skins live as scoped CSS
 * blocks (`:root[data-skin='<id>']`) that override the design tokens in
 * global.css — no component code is skin-aware.
 *
 * Adding a skin = add a CSS block (its own file, imported in global.css) and one
 * entry here. That single registry drives both the Settings selector and the
 * accent logic, so the system stays in sync from one source of truth.
 */
export interface SkinMeta {
  id: SkinId
  /** i18n key under `settings.*` for the human label. */
  labelKey: string
  /**
   * Whether this skin lets the dynamic cover-art accent recolor the UI.
   * Plain does; Nothing is monochrome by design, so it pins its own accent and
   * useAccentColor must not overwrite it.
   */
  supportsDynamicAccent: boolean
}

export const SKINS: readonly SkinMeta[] = [
  { id: 'plain', labelKey: 'settings.skin_plain', supportsDynamicAccent: true },
  { id: 'nothing', labelKey: 'settings.skin_nothing', supportsDynamicAccent: false },
  { id: 'cyberpunk', labelKey: 'settings.skin_cyberpunk', supportsDynamicAccent: false }
] as const

export const DEFAULT_SKIN: SkinId = 'plain'

export function getSkinMeta(id: SkinId): SkinMeta {
  return SKINS.find((s) => s.id === id) ?? SKINS[0]
}

/**
 * Skin fonts load on FIRST activation instead of eagerly at startup: the
 * @fontsource CSS pulls in hundreds of KB of woff2 that users on the default
 * skin never pay for (the mobile cold start used to pay it in full). import()
 * is module-cached, so repeated switches are free. Inter — the base font of
 * every skin — stays eager in main.tsx / main.mobile.tsx.
 */
const SKIN_FONTS: Partial<Record<SkinId, () => Promise<unknown>>> = {
  nothing: () =>
    Promise.all([
      import('@fontsource-variable/space-grotesk'),
      import('@fontsource/space-mono'),
      import('@fontsource-variable/doto')
    ]),
  cyberpunk: () =>
    Promise.all([
      import('@fontsource-variable/orbitron'),
      import('@fontsource/rajdhani/400.css'),
      import('@fontsource/rajdhani/500.css'),
      import('@fontsource/rajdhani/600.css'),
      import('@fontsource/rajdhani/700.css'),
      // Space Mono is shared: cyberpunk reuses it for mono accents.
      import('@fontsource/space-mono')
    ])
}

/** Reads the skin currently applied to the document (falls back to default). */
export function getSkin(): SkinId {
  const id = document.documentElement.dataset.skin as SkinId | undefined
  return id && SKINS.some((s) => s.id === id) ? id : DEFAULT_SKIN
}

/**
 * Applies a skin by setting `<html data-skin>`. The scoped CSS in global.css
 * does the rest. Plain is the baseline, so we still set the attribute explicitly
 * (rather than removing it) to keep the value queryable and the switch symmetric.
 */
export function applySkin(skin: SkinId): void {
  const id = SKINS.some((s) => s.id === skin) ? skin : DEFAULT_SKIN
  // Fire-and-forget: the fonts swap in whenever the chunk lands, and a failure
  // just leaves the fallback stack — never block the skin switch on I/O.
  void SKIN_FONTS[id]?.().catch(() => {})
  document.documentElement.dataset.skin = id
}
