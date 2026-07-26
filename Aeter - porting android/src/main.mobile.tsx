import React from 'react'
import ReactDOM from 'react-dom/client'
import { HashRouter } from 'react-router-dom'
import '@fontsource-variable/inter'
// Skin fonts (Nothing/Cyberpunk) load lazily on first activation — see
// SKIN_FONTS in lib/skins.ts. Only Inter, the base font, is eager.
import './styles/global.css'
import './i18n'
import App from './App'
import ErrorBoundary from '@/components/ui/ErrorBoundary'
import { installAetherBridge, bridgeReady } from '@/lib/bridge'
import { installLanBridge } from '@/lib/lanClient'
import { initMediaSession } from '@/lib/mediaSession'
import { initThermal } from '@/lib/thermal'

// Mobile entry point. Mirrors src/main.tsx but installs window.aether before
// the app mounts (replaces the Electron preload). Two transports coexist:
// once a desktop has been paired (src/pages/PairDevice.tsx), LAN mode takes
// over on the next launch — but only if the desktop answers the boot probe
// inside installLanBridge() (health check + short mDNS browse, ~2-4s worst
// case with the PC off). Otherwise — nothing paired, or PC unreachable — the
// on-device nodejs-mobile backend boots, so the phone always comes up as a
// working standalone player instead of a dead thin client.
const bridgeInit = installLanBridge().then(async (usedLan) => {
  if (!usedLan) {
    installAetherBridge()
    await Promise.race([bridgeReady(), new Promise((r) => setTimeout(r, 3000))])
  }
  // Thermal relay: native ThermalPlugin → node backend (adaptive concurrency).
  // After the bridge settles so the first forwarded sample can actually land;
  // no-op in LAN mode (the heavy work runs on the desktop).
  initThermal(usedLan)
})

// Safety net: a missed .catch on a bridge promise must at least leave a trace
// (chromium console → logcat) instead of vanishing silently on device.
window.addEventListener('unhandledrejection', (e) => {
  console.error('[unhandledrejection]', e.reason)
})

// Keep the focused field visible above the soft keyboard. The edge-to-edge
// WebView never resizes on IME show, so Chromium has no idea the bottom half
// of the page is covered and never auto-scrolls. --kb-height (pushed by
// MainActivity's inset listener) tells us the keyboard is actually up; the
// delay lets it land after the IME animation starts.
window.addEventListener('focusin', (e) => {
  const el = e.target
  if (
    !(el instanceof HTMLInputElement) &&
    !(el instanceof HTMLTextAreaElement) &&
    !(el instanceof HTMLSelectElement)
  ) {
    return
  }
  window.setTimeout(() => {
    if (document.activeElement !== el) return
    const kb = parseFloat(
      getComputedStyle(document.documentElement).getPropertyValue('--kb-height')
    )
    if (!kb || kb <= 0) return
    el.scrollIntoView({ block: 'center', behavior: 'smooth' })
  }, 350)
})

// Marks the document so the mobile-only CSS rules in global.css (html[data-mobile])
// take effect: bottom-nav layout, full-bleed player, safe-area insets, no titlebar.
document.documentElement.setAttribute('data-mobile', '')

// Immediate paint: a tiny branded shell while the LAN probe / nodejs-mobile
// boot resolves (~1-3s worst case) — the WebView used to sit on a black screen
// for that whole window. Design tokens with inlined fallbacks so it looks right
// even in the instant before global.css applies. React's createRoot clears the
// container on the real render below, so no teardown is needed — and the
// bridge init order above is untouched (storage permissions must still be
// requested before the bridge comes up).
document.getElementById('root')!.innerHTML = `
  <style>@keyframes aether-splash { from { transform: translateX(0) } to { transform: translateX(88px) } }</style>
  <div style="position:fixed;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:22px;background:var(--color-surface-0,#09090d)">
    <div style="font:600 28px/1 'Inter Variable',system-ui,sans-serif;letter-spacing:.18em;color:var(--color-text-1,rgba(255,255,255,0.92))">AETHER</div>
    <div style="width:132px;height:3px;border-radius:999px;background:var(--color-surface-2,#16161f);overflow:hidden">
      <div style="width:44px;height:100%;border-radius:999px;background:var(--accent,#8b7cf6);animation:aether-splash 0.9s ease-in-out infinite alternate"></div>
    </div>
  </div>
`

// Mirror playback into the Android media notification / lock-screen controls.
initMediaSession()

function render(): void {
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <ErrorBoundary>
        <HashRouter>
          <App />
        </HashRouter>
      </ErrorBoundary>
    </React.StrictMode>
  )
}

// Wait (briefly) for the backend/LAN bridge so the media-server base is set
// before first render; never block the UI indefinitely if it's slow to start.
bridgeInit.finally(render)
