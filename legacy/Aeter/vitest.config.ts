import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

export default defineConfig({
  test: {
    projects: [
      {
        resolve: {
          alias: {
            '@shared': resolve(__dirname, 'shared')
          }
        },
        test: {
          name: 'backend',
          environment: 'node',
          include: ['electron/**/*.test.ts', 'shared/**/*.test.ts'],
          // better-sqlite3 native bindings are safest in forked processes
          pool: 'forks'
        }
      },
      {
        plugins: [react()],
        resolve: {
          alias: {
            '@': resolve(__dirname, 'src'),
            '@shared': resolve(__dirname, 'shared')
          }
        },
        test: {
          name: 'dom',
          environment: 'jsdom',
          include: ['src/**/*.test.{ts,tsx}'],
          setupFiles: ['src/test/setup.ts']
        }
      }
    ]
  }
})
