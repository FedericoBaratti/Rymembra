/**
 * LIFO stack of dismiss handlers for the Android hardware back button.
 *
 * useAndroidBackButton only knew the overlays tracked as useUiStore flags;
 * overlays holding component-local open state (bottom-sheet menus, editors,
 * flows, tour) were invisible to it — Back would navigate away or even exit
 * the app with the overlay still on screen. Overlay primitives and bespoke
 * overlays push a dismiss handler here while open (via useBackDismiss); the
 * back handler pops the topmost first.
 *
 * LIFO matches visual stacking: the overlay opened last (rendered on top) is
 * dismissed first.
 */
type BackHandler = () => void

const stack: BackHandler[] = []

/** Push a handler; returns its unregister function. */
export function pushBack(handler: BackHandler): () => void {
  stack.push(handler)
  return () => {
    const i = stack.lastIndexOf(handler)
    if (i !== -1) stack.splice(i, 1)
  }
}

/** Dismiss the topmost registered overlay. True if one handled the press. */
export function popBack(): boolean {
  const handler = stack.pop()
  if (!handler) return false
  handler()
  return true
}

/** Current depth (tests). */
export function backStackSize(): number {
  return stack.length
}
