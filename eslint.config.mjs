// Flat ESLint config unica per tutto il monorepo (nel legacy erano due file
// identici duplicati a mano). typescript-eslint recommended NON type-checked:
// veloce, nessuna risoluzione di progetto. Nessuna regola di formattazione — il
// lint segnala difetti reali, non stile.
import tseslint from 'typescript-eslint'
import reactHooks from 'eslint-plugin-react-hooks'

export default tseslint.config(
  {
    ignores: [
      '**/node_modules/**',
      '**/out/**',
      '**/dist/**',
      '**/dist-mobile/**',
      '**/dist-node/**',
      '**/release/**',
      '**/android/**',
      // Progetto node impacchettato nell'app Android: artefatto di build.
      '**/nodejs-assets/**',
      'graphify-out/**',
      '**/coverage/**',
      // L'albero di riferimento pre-riscrittura: si consulta, non si corregge.
      'legacy/**'
    ]
  },
  ...tseslint.configs.recommended,
  {
    // Renderer React: la correttezza degli hook è il controllo automatico che
    // rende di più.
    files: ['packages/{ui,skin-studio}/**/*.{ts,tsx}', 'apps/*/src/**/*.{ts,tsx}'],
    plugins: { 'react-hooks': reactHooks },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn'
    }
  },
  {
    // Il core non può dipendere da un ambiente browser: se serve il DOM, la cosa
    // vive in un adapter o nel renderer. tsconfig.node.json lo impedisce già a
    // livello di tipi; questa regola dà il messaggio d'errore giusto.
    files: ['packages/core/**/*.ts'],
    rules: {
      'no-restricted-globals': [
        'error',
        { name: 'window', message: 'Il core non ha DOM: usa un adapter di piattaforma.' },
        { name: 'document', message: 'Il core non ha DOM: usa un adapter di piattaforma.' },
        { name: 'localStorage', message: 'Il core non ha DOM: usa lo store di piattaforma.' }
      ]
    }
  },
  {
    rules: {
      // Allineata a noUnusedLocals di tsc: prefisso underscore = inutilizzato
      // di proposito. I binding di catch inutilizzati restano ammessi perché
      // "catch { /* best effort */ }" è idiomatico in questo codice.
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrors: 'none' }
      ]
    }
  },
  {
    // Script CommonJS da macchina di sviluppo: require() è il senso del .cjs.
    files: ['**/*.cjs'],
    rules: { '@typescript-eslint/no-require-imports': 'off' }
  }
)
