// Places the nodejs-mobile project where nodejs-mobile-cordova expects it under
// Capacitor.
//
// nodejs-mobile-cordova is a Cordova plugin: both its Gradle script and its
// runtime look for `src/main/assets/www/nodejs-project` *inside the
// capacitor-cordova-android-plugins subproject* (PROJECT_ROOT = "www/nodejs-project").
// Capacitor doesn't create that `www` folder, so we copy our node project there
// after every `cap sync`. The subproject's assets are merged into the final APK,
// so the project also ends up at assets/www/nodejs-project at runtime.
import { cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const srcProject = resolve(root, 'nodejs-assets/nodejs-project')
const cordovaPlugins = resolve(root, 'android/capacitor-cordova-android-plugins')

if (!existsSync(resolve(srcProject, 'main.js'))) {
  console.error('[sync-android-node] missing nodejs-assets/nodejs-project/main.js — run "npm run build:node"')
  process.exit(1)
}
if (!existsSync(cordovaPlugins)) {
  console.error('[sync-android-node] missing android/capacitor-cordova-android-plugins — run "cap sync android" first')
  process.exit(1)
}

const wwwDir = resolve(cordovaPlugins, 'src/main/assets/www')
const destProject = resolve(wwwDir, 'nodejs-project')

rmSync(destProject, { recursive: true, force: true })
mkdirSync(wwwDir, { recursive: true })
// Copy the project (bundled main.js, package.json, sql-wasm assets). There are
// no native modules anymore (better-sqlite3 → sql.js WASM, bundled into main.js),
// so skip node_modules — copying it would only bloat the APK with dead weight.
cpSync(srcProject, destProject, {
  recursive: true,
  filter: (src) => !/[\\/]node_modules([\\/]|$)/.test(src)
})

// No native modules anymore (better-sqlite3 → sql.js WASM, bundled). Tell
// nodejs-mobile-cordova NOT to attempt a native rebuild.
writeFileSync(resolve(wwwDir, 'NODEJS_MOBILE_BUILD_NATIVE_MODULES_VALUE.txt'), '0\n')

console.log(`[sync-android-node] ${srcProject} -> ${destProject}`)
