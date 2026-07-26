// Pre-flight gate that fails LOUDLY when the APK would ship a STALE bundle.
//
// The single most expensive class of bug in this port (journal #4/#7/#14) was not
// wrong code — it was a correct fix that never reached the device because one of the
// three sync steps was skipped, so the running bundle was old. The trap is that the
// bundle that actually runs is NOT the obvious build output:
//   - renderer  : android/app/src/main/assets/public            (NOT dist-mobile/)
//   - Node back. : android/capacitor-cordova-android-plugins/src/main/assets/www/
//                  nodejs-project/main.js   (the Cordova bundle — gotcha #4, NOT
//                  dist-node/ nor android/app/.../assets/nodejs-project/)
//
// This script compares the freshly built artifacts against those deployed copies and
// exits 1 if they diverge, naming the stale path. Run it as the last step of
// `npm run deploy:android`, after all the sync scripts.
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')

// Cordova bridge glue lives in assets/public but is NOT emitted by `vite build`
// (see sync-android-web-to-wsl.mjs). Compare only the Vite output, not the glue.
const GLUE = new Set(['cordova.js', 'cordova_plugins.js', 'plugins'])

const errors = []
const warnings = []

function hashFile(p) {
  return createHash('sha256').update(readFileSync(p)).digest('hex')
}

/**
 * Smallest PT_LOAD p_align across an ELF's program headers, or null if the file
 * isn't a parseable 64-bit little-endian ELF. A value < 16384 (0x4000) means the
 * lib's LOAD segments are 4 KB-aligned and it CANNOT load on a 16 KB-page device
 * without OS backcompat — the exact "app won't start" class this preflight guards.
 */
function minLoadAlign(p) {
  const b = readFileSync(p)
  if (b.length < 64 || b[0] !== 0x7f || b[1] !== 0x45 || b[2] !== 0x4c || b[3] !== 0x46) return null
  if (b[4] !== 2 || b[5] !== 1) return null // require ELF64 + little-endian (arm64-v8a)
  const phoff = Number(b.readBigUInt64LE(32))
  const phentsize = b.readUInt16LE(54)
  const phnum = b.readUInt16LE(56)
  let min = null
  for (let i = 0; i < phnum; i++) {
    const off = phoff + i * phentsize
    if (off + 56 > b.length) break
    if (b.readUInt32LE(off) !== 1) continue // PT_LOAD
    const align = Number(b.readBigUInt64LE(off + 48))
    if (min === null || align < min) min = align
  }
  return min
}

/**
 * 16 KB page-size alignment guard. Android 15/16 devices with 16 KB pages refuse
 * 4 KB-aligned .so at dlopen → the app fails to start (Android 16 falls back to
 * the manifest's pageSizeCompat mode; Android 15 has no backcompat and crashes).
 *
 *  - libnodejs-mobile-cordova-native-lib.so is OURS to build (build.gradle passes
 *    ANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON). A 4 KB result = a real regression
 *    (stale NDK / flag dropped) → BLOCKING error, if a build output exists yet.
 *  - libnode.so is an IMPORTED 2019 prebuilt we can't relink; it's 4 KB until a
 *    16 KB-aligned prebuilt is sourced. It only boots on 16 KB devices via Android
 *    16 pageSizeCompat → WARNING (won't block the sideload build).
 */
function checkNativeAlignment() {
  const GOOD = 16384
  const prebuilt = resolve(root, 'android/app/libs/cdvnodejsmobile/libnode/bin/arm64-v8a/libnode.so')
  if (existsSync(prebuilt)) {
    const a = minLoadAlign(prebuilt)
    if (a !== null && a < GOOD) {
      warnings.push(
        `libnode.so is ${a}-byte (4 KB) aligned — it will FAIL to load on a 16 KB-page device ` +
          `EXCEPT via Android 16's pageSizeCompat. On Android 15 (16 KB) the app won't start. ` +
          `Fix: replace ${rel(prebuilt)} (and the .so.gz it's decompressed from) with a 16 KB-aligned libnode.`
      )
    }
  }
  // The CMake-built lib only exists after a Gradle build; check the last output if present.
  const builtDirs = [
    'android/app/build/intermediates/merged_native_libs/release/mergeReleaseNativeLibs/out/lib/arm64-v8a',
    'android/app/build/intermediates/merged_native_libs/debug/mergeDebugNativeLibs/out/lib/arm64-v8a'
  ]
  for (const d of builtDirs) {
    const lib = resolve(root, d, 'libnodejs-mobile-cordova-native-lib.so')
    if (!existsSync(lib)) continue
    const a = minLoadAlign(lib)
    if (a !== null && a < GOOD) {
      errors.push(
        `native STALE: libnodejs-mobile-cordova-native-lib.so is ${a}-byte (4 KB) aligned in ${d} — ` +
          `build.gradle's ANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON did not take (stale NDK < r26 or a cached ` +
          `CMake build). Run "cd android && ./gradlew clean" then rebuild so it links 16 KB-aligned.`
      )
    }
    break // newest configured variant only
  }
}

/** Every Vite output file in dist-mobile must exist, identical, under assets/public. */
function checkRenderer() {
  const built = resolve(root, 'dist-mobile')
  const deployed = resolve(root, 'android/app/src/main/assets/public')
  if (!existsSync(join(built, 'index.html'))) {
    errors.push(`renderer: missing ${rel(built)}/index.html — run "npm run build:mobile" (or sync:android-web) first`)
    return
  }
  if (!existsSync(deployed)) {
    errors.push(`renderer: missing deployed tree ${rel(deployed)} — run "npm run sync:android-web"`)
    return
  }
  for (const f of walk(built)) {
    const r = relative(built, f).replace(/\\/g, '/')
    if (GLUE.has(r.split('/')[0])) continue
    const dep = join(deployed, r)
    if (!existsSync(dep)) {
      errors.push(`renderer STALE: ${r} present in dist-mobile but missing under assets/public — run "npm run sync:android-web"`)
    } else if (hashFile(f) !== hashFile(dep)) {
      errors.push(`renderer STALE: ${r} differs between dist-mobile and assets/public — run "npm run sync:android-web"`)
    }
  }
}

/** The built backend (dist-node/main.js) must be byte-identical to the Cordova
 *  bundle's main.js (the one nodejs-mobile actually executes on device). */
function checkBackend() {
  const built = resolve(root, 'dist-node/main.js')
  const cordova = resolve(
    root,
    'android/capacitor-cordova-android-plugins/src/main/assets/www/nodejs-project/main.js'
  )
  if (!existsSync(built)) {
    errors.push(`backend: missing ${rel(built)} — run "npm run build:node" first`)
    return
  }
  if (!existsSync(cordova)) {
    errors.push(`backend: missing Cordova bundle ${rel(cordova)} — run "node scripts/sync-android-node-project.mjs"`)
    return
  }
  if (hashFile(built) !== hashFile(cordova)) {
    errors.push(
      `backend STALE: dist-node/main.js differs from the Cordova bundle main.js ` +
        `(${rel(cordova)}). The APK would run OLD backend code — re-run ` +
        `"npm run build:node && node scripts/sync-android-node-project.mjs" (gotcha #4).`
    )
  }
}

/** The npm Capacitor plugins must be wired into the Gradle build AND the runtime
 *  registration list. Journal: @capacitor/app carries the WHOLE Android back
 *  handling (OnBackPressedCallback in AppPlugin.load); when these files miss it,
 *  gesture-back backgrounds the app with dialogs still open. */
function checkCapacitorPlugins() {
  const checks = [
    ['android/capacitor.settings.gradle', ":capacitor-app", 'include for the @capacitor/app Gradle project'],
    ['android/app/capacitor.build.gradle', "project(':capacitor-app')", '@capacitor/app dependency'],
    ['android/app/src/main/assets/capacitor.plugins.json', 'com.capacitorjs.plugins.app.AppPlugin', 'AppPlugin runtime registration']
  ]
  for (const [relPath, needle, what] of checks) {
    const p = resolve(root, relPath)
    if (!existsSync(p)) {
      errors.push(`capacitor: missing ${relPath} — run "npm run cap:sync" on Windows, then sync:android-native`)
    } else if (!readFileSync(p, 'utf8').includes(needle)) {
      errors.push(
        `capacitor STALE: ${relPath} lacks the ${what} — the APK would ship WITHOUT the native ` +
          `back-button plugin (Back would background the app instead of closing dialogs). ` +
          `Re-run "npm run cap:sync" on Windows, then "npm run sync:android-native".`
      )
    }
  }
}

/** On the WSL tree, the branded res/ (dark Aether splash + launcher icons) must
 *  match the Windows repo — otherwise every launch/recreate flashes the default
 *  white Capacitor template art. No-op on Windows (source == dest). */
function checkBranding() {
  const winRepo =
    process.env.AETHER_WIN_REPO ??
    (process.platform === 'win32' ? root : '/mnt/c/Users/Federico Baratti/Desktop/Aeter - porting android')
  const sentinel = 'android/app/src/main/res/drawable/splash.png'
  const src = resolve(winRepo, sentinel)
  const dst = resolve(root, sentinel)
  if (resolve(winRepo) === root) return // running in the Windows repo itself
  if (!existsSync(src)) return // Windows repo not mounted — nothing to compare against
  if (!existsSync(dst) || hashFile(src) !== hashFile(dst)) {
    errors.push(
      `branding STALE: ${sentinel} differs from the Windows repo — the APK would ship the ` +
        `default white Capacitor splash. Re-run "npm run sync:android-native".`
    )
  }
}

function walk(dir) {
  const out = []
  for (const name of readdirSync(dir)) {
    const full = join(dir, name)
    if (statSync(full).isDirectory()) out.push(...walk(full))
    else out.push(full)
  }
  return out
}

function rel(p) {
  return relative(root, p).replace(/\\/g, '/')
}

checkRenderer()
checkBackend()
checkCapacitorPlugins()
checkBranding()
checkNativeAlignment()

if (warnings.length > 0) {
  console.warn(`[preflight-android] ${warnings.length} warning(s):`)
  for (const w of warnings) console.warn(`  ⚠ ${w}`)
  console.warn('')
}

if (errors.length === 0) {
  console.log('[preflight-android] OK — renderer + backend bundles are in sync with the latest build.')
  console.log('[preflight-android] next: cd android && ./gradlew clean assembleDebug && adb install -r app/build/outputs/apk/debug/app-debug.apk')
  process.exit(0)
}

console.error(`[preflight-android] ${errors.length} problem(s) — DO NOT build the APK yet:\n`)
for (const e of errors) console.error(`  ✗ ${e}`)
console.error('\n[preflight-android] fix the above (usually: re-run the missed sync step), then retry.')
process.exit(1)
