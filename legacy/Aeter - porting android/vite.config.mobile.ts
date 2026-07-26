import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { resolve } from 'node:path'
import { renameSync, existsSync, readFileSync } from 'node:fs'

const pkgVersion: string = JSON.parse(
  readFileSync(resolve(__dirname, 'package.json'), 'utf8')
).version

// Capacitor requires the webDir entry to be named index.html. Our source HTML
// is index.mobile.html (to coexist with the desktop index.html), so rename the
// emitted file after the bundle is written.
function renameIndexHtml(): Plugin {
  return {
    name: 'rename-mobile-index',
    closeBundle() {
      const from = resolve(__dirname, 'dist-mobile/index.mobile.html')
      const to = resolve(__dirname, 'dist-mobile/index.html')
      if (existsSync(from)) renameSync(from, to)
    }
  }
}

/**
 * Mobile renderer build (Capacitor / Android).
 *
 * Builds ONLY the React renderer from `index.mobile.html` + `src/`, with the
 * same `@`/`@shared` aliases as the Electron renderer. No Electron main/preload
 * targets — those are replaced by the nodejs-mobile backend at runtime.
 *
 * Output goes to `dist-mobile/`, which is `webDir` in capacitor.config.ts.
 */
export default defineConfig({
  root: '.',
  base: './',
  define: {
    // Lets src/lib/platform.ts collapse to the mobile branch at build time.
    __AETHER_MOBILE__: 'true',
    // App version shown in Settings; single source of truth is package.json.
    __APP_VERSION__: JSON.stringify(pkgVersion)
  },
  plugins: [react(), tailwindcss(), renameIndexHtml()],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
      '@shared': resolve(__dirname, 'shared')
    }
  },
  build: {
    outDir: 'dist-mobile',
    emptyOutDir: true,
    rollupOptions: {
      input: resolve(__dirname, 'index.mobile.html'),
      output: {
        // Vendor split: the big third-party libs land in stable chunks that
        // cache independently of app code, and the entry chunk the WebView
        // parses at cold start shrinks accordingly.
        manualChunks: {
          'vendor-react': ['react', 'react-dom', 'react-router-dom'],
          'vendor-i18n': ['i18next', 'react-i18next'],
          'vendor-icons': ['lucide-react'],
          'vendor-audio': ['howler']
        }
      }
    }
  }
})
