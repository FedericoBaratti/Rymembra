// Haptic feedback is a mobile-only affordance. On desktop these are no-ops so
// call sites (tap()/select()/impact()) can stay identical across platforms.

export const tap = (): void => {}
export const select = (): void => {}
export const impact = (): void => {}
export const haptics = { tap, select, impact }
export default haptics
