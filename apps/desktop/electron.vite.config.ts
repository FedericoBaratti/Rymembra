/**
 * Build del desktop.
 *
 * Tre bundle separati con tre ambienti diversi — main e preload girano su Node
 * dentro Electron, il renderer nel browser — e la ragione per cui la
 * configurazione è esplicita: i pacchetti del monorepo esportano TypeScript
 * sorgente, senza passo di build. Vanno quindi risolti per alias e inclusi nella
 * transpilazione, non trattati come dipendenze esterne già compilate.
 */

import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import react from '@vitejs/plugin-react'
import tailwind from '@tailwindcss/vite'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '../..')

/**
 * I pacchetti del monorepo NON vanno esternalizzati.
 *
 * `externalizeDepsPlugin` lascia fuori dal bundle tutto ciò che è in
 * `dependencies`, che è la cosa giusta per i moduli nativi e per le librerie già
 * compilate. Ma questi pacchetti esportano TypeScript SORGENTE — è la scelta
 * "just-in-time packages" del monorepo, nessun passo di build — quindi lasciarli
 * fuori significa che a runtime Electron farebbe `require('@aether/core')` e
 * troverebbe un `.ts` che non sa caricare. Vanno transpilati dentro.
 */
const WORKSPACE_PACKAGES = ['@aether/core', '@aether/skin', '@aether/ui', '@aether/skin-studio']

const alias = {
  '@aether/core': resolve(root, 'packages/core/src'),
  '@aether/skin': resolve(root, 'packages/skin/src'),
  '@aether/ui': resolve(root, 'packages/ui/src'),
  '@aether/skin-studio': resolve(root, 'packages/skin-studio/src')
}

export default defineConfig({
  main: {
    // better-sqlite3 resta esterno: è un modulo nativo, il bundler non lo può
    // includere. `externalizeDepsPlugin` lo lascia come require a runtime.
    plugins: [externalizeDepsPlugin({ exclude: WORKSPACE_PACKAGES })],
    resolve: { alias },
    build: {
      lib: { entry: resolve(here, 'electron/main.ts') },
      outDir: resolve(here, 'out/main')
    }
  },
  preload: {
    plugins: [externalizeDepsPlugin({ exclude: WORKSPACE_PACKAGES })],
    resolve: { alias },
    build: {
      lib: {
        entry: resolve(here, 'electron/preload.ts'),
        // Il preload NON può essere un modulo ES: Electron lo carica in un
        // contesto che richiede CommonJS quando sandbox è attivo o il contesto è
        // isolato.
        formats: ['cjs']
      },
      outDir: resolve(here, 'out/preload')
    }
  },
  renderer: {
    root: here,
    plugins: [react(), tailwind()],
    resolve: { alias },
    build: {
      outDir: resolve(here, 'out/renderer'),
      rollupOptions: { input: resolve(here, 'index.html') }
    }
  }
})
