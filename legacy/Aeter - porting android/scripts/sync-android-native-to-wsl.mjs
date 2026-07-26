// Copies the hand-maintained Android NATIVE sources into the WSL build tree.
//
// Why this exists: the bulk Windows->WSL rsync uses `--exclude=android` (to
// protect the working nodejs-mobile native build under ~/aether/android), so the
// hand-written Capacitor plugin sources never reach the build machine and the APK
// ends up WITHOUT them -> Capacitor answers `"<Plugin>" is not implemented on
// android` (FileAccess/pickFolder, SecureStore, ...). Run this AFTER the bulk
// rsync to deliver just those files, then `./gradlew clean assembleDebug`.
//
// Run on WSL from the project root (~/aether):  node scripts/sync-android-native-to-wsl.mjs
// Override the Windows source if your path differs:  AETHER_WIN_REPO=/mnt/c/.../repo node scripts/...
import { cpSync, existsSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

// Destination = the WSL build tree this script lives in (root is one level up).
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const dstAndroid = resolve(root, 'android')

// Source = the Windows repo (mounted under /mnt/c on WSL). When running ON
// Windows the repo itself is the source, so default to root. Overridable.
const winRepo =
  process.env.AETHER_WIN_REPO ??
  (process.platform === 'win32'
    ? root
    : '/mnt/c/Users/Federico Baratti/Desktop/Aeter - porting android')
const srcAndroid = resolve(winRepo, 'android')

// The raw-AAR Node route: NOT registered in MainActivity and not compilable
// without the nodejs-mobile AAR on the classpath — copying it would break the
// Kotlin build. Keep it out of the WSL tree.
const EXCLUDE = new Set(['NodeBackendPlugin.kt', 'NodeRuntime.kt'])

const pkgRel = 'app/src/main/java/com/aether/player'
// Full resource tree: themes/palette (dark windowBackground kills the white
// launch flash), network security config, Android Auto descriptor, AND the
// Aether-branded splash.png/mipmap icons — without these the WSL tree keeps the
// default Capacitor template art (blue logo on white) on every launch/recreate.
const resRel = 'app/src/main/res'
// Hand-maintained build/manifest files (the Kotlin wiring + permissions).
const FILES = [
  'build.gradle',
  'variables.gradle',
  'app/build.gradle',
  'app/src/main/AndroidManifest.xml',
  // Capacitor runtime config copy (backgroundColor, androidScheme) — maintained
  // by hand, cap sync is not part of the WSL flow.
  'app/src/main/assets/capacitor.config.json',
  // cap-sync-generated wiring for npm Capacitor plugins (@capacitor/app: the
  // Android back button lives ENTIRELY in that plugin — without these three the
  // APK ships no back handling and gesture-back backgrounds the app with
  // dialogs still open). Regenerate on Windows with `npm run cap:sync` after
  // adding/removing any npm Capacitor plugin.
  'capacitor.settings.gradle',
  'app/capacitor.build.gradle',
  'app/src/main/assets/capacitor.plugins.json'
]

if (!existsSync(resolve(srcAndroid, pkgRel, 'FileAccessPlugin.kt'))) {
  console.error(`[sync-android-native] source not found under ${srcAndroid} — set AETHER_WIN_REPO to the Windows repo path`)
  process.exit(1)
}
// Running inside the Windows repo itself: the native sources ARE the source of
// truth here — self-copy would be a no-op at best (cpSync rejects same paths).
if (resolve(srcAndroid) === resolve(dstAndroid)) {
  console.log('[sync-android-native] source == destination (Windows repo) — native sources already in place')
  process.exit(0)
}
if (!existsSync(dstAndroid)) {
  console.error(`[sync-android-native] missing ${dstAndroid} — generate it first (cap add android / cap sync)`)
  process.exit(1)
}

for (const rel of FILES) {
  cpSync(resolve(srcAndroid, rel), resolve(dstAndroid, rel))
  console.log(`[sync-android-native] ${rel}`)
}

// Copy the plugin package, skipping the raw-AAR route files.
cpSync(resolve(srcAndroid, pkgRel), resolve(dstAndroid, pkgRel), {
  recursive: true,
  filter: (src) => !EXCLUDE.has(src.split(/[\\/]/).pop())
})
console.log(`[sync-android-native] ${pkgRel}/ (excl. ${[...EXCLUDE].join(', ')})`)

// Copy the whole res/ tree (overwrites the template's default splash/mipmaps;
// never deletes, so a WSL-only file would survive — there are none by design).
cpSync(resolve(srcAndroid, resRel), resolve(dstAndroid, resRel), { recursive: true })
console.log(`[sync-android-native] ${resRel}/ (branding: splash + mipmap icons)`)

console.log('[sync-android-native] done — next: cd android && ./gradlew clean assembleDebug')
