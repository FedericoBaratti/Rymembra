import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { resolve } from 'node:path'
import { readFileSync } from 'node:fs'

const pkgVersion: string = JSON.parse(
  readFileSync(resolve(__dirname, 'package.json'), 'utf8')
).version

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin()],
    define: {
      // App-level AcoustID key so audio-fingerprint enrichment is keyless for
      // users (empty string ⇒ AcoustID stays off unless the user sets their own).
      __ACOUSTID_APP_KEY__: JSON.stringify(process.env.ACOUSTID_APP_KEY ?? '')
    },
    build: {
      lib: { entry: resolve(__dirname, 'electron/main.ts') }
    },
    resolve: {
      alias: { '@shared': resolve(__dirname, 'shared') }
    }
  },
  preload: {
    plugins: [externalizeDepsPlugin()],
    build: {
      lib: { entry: resolve(__dirname, 'electron/preload.ts') },
      rollupOptions: {
        // Sandboxed preload scripts must be CommonJS
        output: { format: 'cjs', entryFileNames: '[name].cjs' }
      }
    },
    resolve: {
      alias: { '@shared': resolve(__dirname, 'shared') }
    }
  },
  renderer: {
    root: '.',
    define: {
      // App version shown in Settings; single source of truth is package.json.
      __APP_VERSION__: JSON.stringify(pkgVersion)
    },
    plugins: [react(), tailwindcss()],
    resolve: {
      alias: {
        '@': resolve(__dirname, 'src'),
        '@shared': resolve(__dirname, 'shared')
      }
    },
    build: {
      rollupOptions: { input: resolve(__dirname, 'index.html') }
    }
  }
})
