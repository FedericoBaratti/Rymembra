// Refreshes the mobile RENDERER in the Android assets tree, WITHOUT destroying
// the Capacitor/Cordova bridge glue.
//
// Why this exists: the WSL deploy flow updates the Node backend bundle
// (build:node + sync-android-node-project.mjs) but the bulk Windows->WSL rsync
// uses `--exclude=android` and never runs `cap copy`/`cap sync` — so the renderer
// under android/app/src/main/assets/public stays frozen and drifts from the Node +
// Kotlin sides (e.g. the ImageResize base64->temp-file refactor), making the native
// plugin reject with `srcPath and destPath are required` (no cover art).
//
// CRITICAL: `android/app/src/main/assets/public` is NOT only the Vite output. It also
// holds the cordova bridge glue that Capacitor injects into the WebView at runtime:
//   - cordova.js
//   - cordova_plugins.js   (its clobbers:["nodejs"] is what creates window.nodejs)
//   - plugins/nodejs-mobile-cordova/www/{nodejs_apis,nodejs_events}.js
// `vite build` (dist-mobile) does NOT contain these. If they're deleted, window.nodejs
// never exists, src/lib/bridge.ts getTransport() falls back to the unregistered
// `NodeBackend` plugin, and the app dies with `"NodeBackend" plugin is not implemented
// on android` (blank settings/library). So we must refresh ONLY the Vite output and
// preserve/restore the glue. We never touch the hand-maintained native tree, so there
// is zero risk to the cdvnodejsmobile native build (unlike `cap copy`/`cap sync`).
//
// Source of the static glue: the Windows repo's android/ tree (same convention as
// sync-android-native-to-wsl.mjs). Override with AETHER_WIN_REPO. The glue is static
// (changes only if the cordova plugin set changes), so this is always correct and
// idempotent.
//
// Usage (after `npm run build:mobile`, from the project root, on WSL):
//   node scripts/sync-android-web-to-wsl.mjs
import { cpSync, existsSync, mkdirSync, rmSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const srcWeb = resolve(root, 'dist-mobile')
const destWeb = resolve(root, 'android/app/src/main/assets/public')

// Windows repo — source of truth for the cap-generated glue. On WSL it is the
// /mnt/c mount; when this script runs ON Windows the repo itself IS the Windows
// repo, so default to root (the glue is already in place there).
const winRepo =
  process.env.AETHER_WIN_REPO ??
  (process.platform === 'win32'
    ? root
    : '/mnt/c/Users/Federico Baratti/Desktop/Aeter - porting android')
const winWeb = resolve(winRepo, 'android/app/src/main/assets/public')

// Files/dirs Capacitor injects at runtime; they live in assets/public but are NOT
// emitted by `vite build`, so they must survive the renderer refresh.
const CORDOVA_GLUE = ['cordova.js', 'cordova_plugins.js', 'plugins']

if (!existsSync(resolve(srcWeb, 'index.html'))) {
  console.error('[sync-android-web] missing dist-mobile/index.html — run "npm run build:mobile" first')
  process.exit(1)
}
if (!existsSync(resolve(root, 'android'))) {
  console.error('[sync-android-web] missing android/ — generate it first (cap add android / cap sync)')
  process.exit(1)
}
if (!existsSync(resolve(winWeb, 'cordova.js'))) {
  console.error(
    `[sync-android-web] cordova glue not found under ${winWeb} — run "npm run cap:sync" on Windows once ` +
      '(or set AETHER_WIN_REPO to the Windows repo path)'
  )
  process.exit(1)
}

// 1. Refresh ONLY the Vite output: drop the stale hashed bundle dir, then copy the
//    fresh build. cpSync does not delete files absent from the source, so the glue
//    (cordova.js / cordova_plugins.js / plugins/) at the destination root is untouched.
mkdirSync(destWeb, { recursive: true })
rmSync(resolve(destWeb, 'assets'), { recursive: true, force: true })
cpSync(srcWeb, destWeb, { recursive: true })
console.log(`[sync-android-web] renderer ${srcWeb} -> ${destWeb}`)

// 2. Restore the static cordova glue from the Windows tree (the previous, destructive
//    version of this script may have deleted it from the WSL tree). Idempotent.
//    GUARD: when source == destination (running inside the Windows repo itself) the
//    rm-then-copy would DELETE the glue and fail to restore it — skip, it's in place.
if (resolve(winWeb) === resolve(destWeb)) {
  console.log('[sync-android-web] glue already in place (source == destination) — skipped')
} else {
  for (const entry of CORDOVA_GLUE) {
    const from = resolve(winWeb, entry)
    const to = resolve(destWeb, entry)
    if (!existsSync(from)) continue
    rmSync(to, { recursive: true, force: true })
    cpSync(from, to, { recursive: true })
    console.log(`[sync-android-web] glue ${entry}`)
  }
}

console.log('[sync-android-web] done — next: cd android && ./gradlew assembleDebug && adb install -r ...')
