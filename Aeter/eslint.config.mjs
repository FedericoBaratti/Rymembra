// Flat ESLint config — kept IDENTICAL in Aeter/ (desktop) and
// "Aeter - porting android"/ (mobile), like every shared source file.
// typescript-eslint recommended (NON type-checked: fast, no tsconfig project
// resolution) + the react-hooks rules on the renderer. No formatting rules —
// formatting stays as-is, lint only flags real defects.
import tseslint from 'typescript-eslint'
import reactHooks from 'eslint-plugin-react-hooks'

export default tseslint.config(
  {
    ignores: [
      'node_modules',
      'out',
      'dist',
      'dist-mobile',
      'dist-node',
      'release',
      'android',
      // Bundled node project synced into the Android app (build artifact).
      'nodejs-assets',
      'graphify-out',
      'coverage'
    ]
  },
  ...tseslint.configs.recommended,
  {
    // React renderer: hook correctness is the highest-value automated check.
    files: ['src/**/*.{ts,tsx}'],
    plugins: { 'react-hooks': reactHooks },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn'
    }
  },
  {
    rules: {
      // Match the compiler settings already enforced by tsc (noUnusedLocals):
      // underscore-prefixed = intentionally unused; unused catch bindings are
      // idiomatic in this codebase ("catch { /* best effort */ }").
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrors: 'none' }
      ]
    }
  },
  {
    // Dev-machine CommonJS scripts: require() is the point of .cjs.
    files: ['**/*.cjs'],
    rules: { '@typescript-eslint/no-require-imports': 'off' }
  }
)
