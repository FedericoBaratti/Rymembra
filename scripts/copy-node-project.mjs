// Copies the bundled backend (dist-node/main.js) into the nodejs-mobile project
// location(s). Run automatically after `npm run build:node`.
//
// - nodejs-assets/nodejs-project  → consumed by nodejs-mobile-cordova (recommended)
// - android/.../assets/nodejs-project → consumed by the custom NodeBackend plugin
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { transformSync } from 'esbuild'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const src = resolve(root, 'dist-node/main.js')
const wasmSrc = resolve(root, 'node_modules/sql.js/dist/sql-wasm.wasm')
// sql.js stays external (its emscripten glue can't be bundled) and is shipped as a
// top-level sibling so it doesn't depend on node_modules being packaged into the APK.
const sqlJsSrc = resolve(root, 'node_modules/sql.js/dist/sql-wasm.js')

// nodejs-mobile-cordova 0.4.3 ships Node 12.19, which can't parse the modern
// operators (||=, ??, ?.) sql.js 1.14's prebuilt sql-wasm.js uses → SyntaxError
// at require() time, crashing the engine. main.js is already transpiled to node12
// by Vite/esbuild; do the same for the externalized sql-wasm.js here before
// shipping it. emscripten's `module.exports = Module` is preserved (transform-only,
// no bundling).
const sqlJsCode = existsSync(sqlJsSrc)
  ? transformSync(readFileSync(sqlJsSrc, 'utf8'), {
      target: 'node12',
      format: 'cjs',
      loader: 'js'
    }).code
      // Node 12.19 (nodejs-mobile-cordova 0.4.3) non supporta lo schema `node:`
      // per i builtin (aggiunto in Node 12.20). esbuild non lo strippa → lo facciamo qui.
      .replace(/require\((["'])node:([a-z_]+)\1\)/g, 'require($1$2$1)')
  : null

if (!existsSync(src)) {
  console.error(`[copy-node-project] missing ${src} — run "npm run build:node" first`)
  process.exit(1)
}

const projectDirs = [
  resolve(root, 'nodejs-assets/nodejs-project'),
  resolve(root, 'android/app/src/main/assets/nodejs-project')
]

for (const dir of projectDirs) {
  mkdirSync(dir, { recursive: true })
  copyFileSync(src, resolve(dir, 'main.js'))
  // sql.js loader (transpiled to node12 above) + WASM binary — required by main.js
  // (require("./sql-wasm.js")) and loaded by node-backend/sqlite-shim.ts at runtime.
  if (sqlJsCode) writeFileSync(resolve(dir, 'sql-wasm.js'), sqlJsCode)
  if (existsSync(wasmSrc)) copyFileSync(wasmSrc, resolve(dir, 'sql-wasm.wasm'))
  console.log(`[copy-node-project] -> ${dir} (main.js + sql-wasm.js + sql-wasm.wasm)`)
}
