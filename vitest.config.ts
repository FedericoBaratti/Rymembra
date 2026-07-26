import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

// Gli alias dei pacchetti duplicano i `paths` di tsconfig.base.json: tsc usa i
// primi, Vite/Vitest i secondi, e devono restare in sync. Sono definiti una
// volta qui e riusati da tutti i config di build.
export const aetherAliases = {
  '@aether/core': resolve(import.meta.dirname, 'packages/core/src'),
  '@aether/skin': resolve(import.meta.dirname, 'packages/skin/src'),
  '@aether/ui': resolve(import.meta.dirname, 'packages/ui/src'),
  '@aether/skin-studio': resolve(import.meta.dirname, 'packages/skin-studio/src')
}

export default defineConfig({
  test: {
    projects: [
      {
        resolve: { alias: aetherAliases },
        test: {
          name: 'backend',
          environment: 'node',
          include: [
            'packages/core/**/*.test.ts',
            'packages/skin/**/*.test.ts',
            'apps/desktop/electron/**/*.test.ts',
            'apps/mobile/node-backend/**/*.test.ts'
          ],
          // packages/skin è isomorfo tranne il modulo apply/, che usa
          // adoptedStyleSheets: quei test si chiamano *.dom.test.ts e girano
          // nel progetto jsdom qui sotto, non qui.
          exclude: ['**/*.dom.test.ts', '**/node_modules/**'],
          // I binding nativi di better-sqlite3 sono più sicuri in processi
          // forkati che in worker thread.
          pool: 'forks'
        }
      },
      {
        plugins: [react()],
        resolve: { alias: aetherAliases },
        test: {
          name: 'dom',
          environment: 'jsdom',
          include: [
            'packages/ui/**/*.test.{ts,tsx}',
            'packages/skin-studio/**/*.test.{ts,tsx}',
            'packages/skin/**/*.dom.test.ts',
            'apps/*/src/**/*.test.{ts,tsx}'
          ],
          setupFiles: [resolve(import.meta.dirname, 'packages/ui/src/test/setup.ts')]
        }
      }
    ]
  }
})
