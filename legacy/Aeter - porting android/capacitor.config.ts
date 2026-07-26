import type { CapacitorConfig } from '@capacitor/cli'

/**
 * Capacitor config for the Android port of Aether.
 *
 * The renderer (src/) is built to `dist-mobile/` by `vite.config.mobile.ts`
 * and loaded inside the Android WebView. The Node "main process" runs inside
 * nodejs-mobile (see android/ NodeBackend plugin) and exposes the same
 * `window.aether` surface over a message bridge instead of Electron IPC.
 *
 * androidScheme is 'http' on purpose: the local media/cover server runs on
 * http://127.0.0.1:<port>, and serving the page over http://localhost avoids
 * mixed-content blocking when <audio>/<img> load from that origin.
 */
const config: CapacitorConfig = {
  appId: 'com.aether.player',
  appName: 'Aether',
  webDir: 'dist-mobile',
  // WebView background while the page loads (and across Activity recreates):
  // matches the app's --color-bg so launch/recreate never flashes white.
  backgroundColor: '#09090d',
  // webContentsDebuggingEnabled is intentionally NOT set: Capacitor's default
  // enables WebView inspection only in debuggable (debug) builds, so release
  // builds ship with remote debugging off.
  server: {
    androidScheme: 'http'
  }
}

export default config
