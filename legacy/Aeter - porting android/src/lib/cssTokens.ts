/**
 * Cached reader for CSS custom properties on <html>. Calling getComputedStyle
 * on every canvas frame forces a style recalc; the values only actually change
 * when the skin/theme attributes flip or the dynamic accent is written to the
 * root style — one MutationObserver invalidates the cache on exactly those
 * events, so per-frame reads become Map lookups.
 */
const cache = new Map<string, string>()
let observing = false

function ensureObserver(): void {
  if (observing || typeof MutationObserver === 'undefined') return
  observing = true
  new MutationObserver(() => cache.clear()).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['style', 'class', 'data-skin', 'data-theme']
  })
}

export function cssToken(name: string): string {
  ensureObserver()
  let value = cache.get(name)
  if (value === undefined) {
    value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
    cache.set(name, value)
  }
  return value
}
