// Reconstructs `android/capacitor-cordova-android-plugins/libs/cdvnodejsmobile/`
// with the nodejs-mobile-cordova JNI glue + the (decompressed) prebuilt libnode,
// which the plugin's Gradle build needs.
//
// Why this is necessary under Capacitor:
//   nodejs-mobile-cordova is a *Cordova* plugin. Its `src/android/build.gradle`
//   hardcodes `externalNativeBuild { cmake { path "libs/cdvnodejsmobile/CMakeLists.txt" } }`
//   relative to the capacitor-cordova-android-plugins module root. But Capacitor's
//   cordova-plugin integration copies the plugin's native files under
//   `src/main/libs/cdvnodejsmobile/` (not `libs/cdvnodejsmobile/`) and never runs
//   the Cordova `after_prepare` hook that decompresses `libnode.so.gz` → `libnode.so`.
//   Result: `./gradlew assembleDebug` fails at configureCMake with
//     [CXX1400] cmake.path .../libs/cdvnodejsmobile/CMakeLists.txt doesn't exist.
//
//   This script assembles that directory exactly as CMakeLists.txt expects (its
//   sources are resolved relative to the CMakeLists location):
//     libs/cdvnodejsmobile/
//       CMakeLists.txt, native-lib.cpp, cordova-bridge.{cpp,h}
//       libnode/include/node/...                (headers, include_directories)
//       libnode/bin/<abi>/libnode.so            (IMPORTED_LOCATION, decompressed)
//
// Run AFTER every `cap sync android` (it is part of `npm run cap:sync`). A bare
// `cap sync` regenerates capacitor-cordova-android-plugins and drops this dir, so
// it must be re-run before each Gradle build that follows a sync.
import { cpSync, copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { gunzipSync } from 'node:zlib'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const plugin = resolve(root, 'node_modules/nodejs-mobile-cordova')
const cordovaPlugins = resolve(root, 'android/capacitor-cordova-android-plugins')
// TWO consumers need the same glue dir: the capacitor-cordova-android-plugins
// module AND the :app module — capacitor.build.gradle `apply from:`s the
// plugin's src/android/build.gradle into :app, whose `cmake.path
// "libs/cdvnodejsmobile/CMakeLists.txt"` resolves relative to android/app/.
// Without the app copy, assemble{Debug,Release} fails with CXX1400.
const dests = [resolve(cordovaPlugins, 'libs/cdvnodejsmobile'), resolve(root, 'android/app/libs/cdvnodejsmobile')]

if (!existsSync(plugin)) {
  console.error('[sync-cordova-nodejs-native] missing node_modules/nodejs-mobile-cordova — run "npm install"')
  process.exit(1)
}
if (!existsSync(cordovaPlugins)) {
  console.error('[sync-cordova-nodejs-native] missing android/capacitor-cordova-android-plugins — run "cap sync android" first')
  process.exit(1)
}

// Source files the CMakeLists references as siblings (see its relative paths).
const sources = {
  'CMakeLists.txt': 'src/android/CMakeLists.txt',
  'native-lib.cpp': 'src/android/jni/native-lib.cpp',
  'cordova-bridge.cpp': 'src/common/cordova-bridge/cordova-bridge.cpp',
  'cordova-bridge.h': 'src/common/cordova-bridge/cordova-bridge.h'
}

for (const dest of dests) {
  rmSync(dest, { recursive: true, force: true })
  mkdirSync(dest, { recursive: true })

  for (const [out, src] of Object.entries(sources)) {
    const from = resolve(plugin, src)
    if (!existsSync(from)) {
      console.error(`[sync-cordova-nodejs-native] missing plugin source: ${src}`)
      process.exit(1)
    }
    copyFileSync(from, resolve(dest, out))
  }

  // libnode headers (CMakeLists: include_directories(libnode/include/node/)).
  cpSync(resolve(plugin, 'libs/android/libnode/include'), resolve(dest, 'libnode/include'), { recursive: true })

  // Prebuilt libnode per ABI, shipped gzipped by the plugin. Decompress to the
  // uncompressed `libnode.so` CMake imports (and AGP packages into the APK).
  const abis = ['arm64-v8a', 'armeabi-v7a', 'x86', 'x86_64']
  let decompressed = 0
  for (const abi of abis) {
    const gz = resolve(plugin, `libs/android/libnode/bin/${abi}/libnode.so.gz`)
    if (!existsSync(gz)) continue
    const outDir = resolve(dest, `libnode/bin/${abi}`)
    mkdirSync(outDir, { recursive: true })
    writeFileSync(resolve(outDir, 'libnode.so'), gunzipSync(readFileSync(gz)))
    decompressed++
  }
  if (decompressed === 0) {
    console.error('[sync-cordova-nodejs-native] no libnode.so.gz found under the plugin — cannot assemble libnode')
    process.exit(1)
  }

  console.log(`[sync-cordova-nodejs-native] -> ${dest} (${decompressed} ABI${decompressed === 1 ? '' : 's'})`)
}
