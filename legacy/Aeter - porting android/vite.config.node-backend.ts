import { defineConfig, type Plugin } from 'vite'
import { resolve } from 'node:path'
import { builtinModules } from 'node:module'
import { readFileSync } from 'node:fs'

const pkg = JSON.parse(readFileSync(resolve(__dirname, 'package.json'), 'utf8')) as {
  version: string
}

// Node 12 (nodejs-mobile) doesn't support the `node:` import scheme (added in
// Node 14.18). Rewrite `node:fs` → `fs` (external) so the bundle emits plain
// require("fs"). Applies to our code AND bundled deps.
function stripNodePrefix(): Plugin {
  return {
    name: 'strip-node-prefix',
    enforce: 'pre',
    resolveId(id) {
      if (id.startsWith('node:')) return { id: id.slice(5), external: true }
      return null
    },
    // Belt-and-suspenders: some CommonJS deps emit runtime require("node:x")
    // that resolveId doesn't see. Rewrite them in the final chunk.
    renderChunk(code) {
      return code
        .replace(/require\((["'])node:/g, 'require($1')
        // Node 12 non ha i module-path `fs/promises` / `dns/promises`; usa
        // l'oggetto `.promises` del modulo base (dal Node 10, API identica).
        .replace(/require\((["'])(fs|dns)\/promises\1\)/g, 'require($1$2$1).promises')
        // sql.js è external (vedi `external`) ma non viaggia in node_modules
        // nell'APK: lo spediamo come file fratello `sql-wasm.js` (copy-node-project.mjs)
        // e lo carichiamo per path relativo, evitando la risoluzione node_modules.
        .replace(/require\((["'])sql\.js\1\)/g, 'require($1./sql-wasm.js$1)')
    }
  }
}

// nodejs-mobile is built with small-icu: the built-in TextDecoder only supports
// utf-8. Bundled deps (e.g. music-metadata) construct `new TextDecoder('ascii'|
// 'latin1'|…)` at module load time, which throws ERR_ENCODING_NOT_SUPPORTED and
// crashes the engine on startup. Prepend a polyfill that decodes those labels
// via Buffer (which Node supports without ICU) and delegates utf-8 to the native
// decoder so streaming/fatal behaviour is preserved.
const textDecoderPolyfill = `(function () {
  var Native = globalThis.TextDecoder;
  if (!Native) return;
  var toBuf = {
    'ascii': 'latin1', 'us-ascii': 'latin1', 'latin1': 'latin1',
    'iso-8859-1': 'latin1', 'windows-1252': 'latin1',
    'utf-16le': 'utf16le', 'utf-16': 'utf16le', 'ucs-2': 'utf16le'
  };
  function Patched(label, opts) {
    label = String(label == null ? 'utf-8' : label).toLowerCase();
    this._enc = toBuf[label];
    if (this._enc) { this._label = label; } else { this._native = new Native(label, opts); }
  }
  Object.defineProperty(Patched.prototype, 'encoding', {
    get: function () { return this._native ? this._native.encoding : this._label; }
  });
  Patched.prototype.decode = function (input, opts) {
    if (this._native) return this._native.decode(input, opts);
    if (input == null) return '';
    var u8 = input instanceof Uint8Array ? input
      : (input && input.buffer ? new Uint8Array(input.buffer, input.byteOffset, input.byteLength)
      : Uint8Array.from(input));
    return Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength).toString(this._enc);
  };
  globalThis.TextDecoder = Patched;
})();`

// nodejs-mobile-cordova 0.4.3 ships Node 12.19, which predates crypto.randomUUID
// (added in Node 14.17/16). Our code AND bundled deps can call it (e.g. the sharp
// shim's temp filenames), throwing "crypto.randomUUID is not a function" — which
// surfaced as "[cover] Copertina non processabile" during a library scan. Patch
// the real `require('crypto')` module object (cached + shared by every consumer)
// with an RFC 4122 v4 implementation built on randomBytes (which Node 12 has).
const cryptoRandomUuidPolyfill = `(function () {
  try {
    var c = require('crypto');
    if (typeof c.randomUUID !== 'function') {
      c.randomUUID = function () {
        var b = c.randomBytes(16);
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        var h = b.toString('hex');
        return h.slice(0, 8) + '-' + h.slice(8, 12) + '-' + h.slice(12, 16) + '-' + h.slice(16, 20) + '-' + h.slice(20);
      };
    }
  } catch (e) {}
})();`

// nodejs-mobile-cordova 0.4.3 ships Node 12.19 / V8 7.8, which predates ES2021:
// String.prototype.replaceAll and Array/String.prototype.at don't exist. esbuild's
// `target: node12` lowers SYNTAX (?./??/…) but never polyfills runtime built-ins,
// so `str.replaceAll(...)` and `arr.at(-1)` — used by our code AND bundled deps
// (music-metadata, node-taglib-sharp, the yt-dlp JSON parsing) — throw
// "x is not a function" on the device. Prepend a tiny polyfill. `split/join` for
// the string case avoids `$`/regex escaping headaches inside this template literal.
const es2021Polyfill = `(function () {
  if (!String.prototype.replaceAll) {
    String.prototype.replaceAll = function (search, replacement) {
      if (Object.prototype.toString.call(search) === '[object RegExp]') return this.replace(search, replacement);
      return this.split(search).join(replacement);
    };
  }
  function at(n) {
    n = Math.trunc(n) || 0;
    if (n < 0) n += this.length;
    return (n < 0 || n >= this.length) ? undefined : this[n];
  }
  if (!Array.prototype.at) Object.defineProperty(Array.prototype, 'at', { value: at, writable: true, configurable: true });
  if (!String.prototype.at) Object.defineProperty(String.prototype, 'at', { value: at, writable: true, configurable: true });
})();`

/**
 * Bundles the nodejs-mobile backend (node-backend/main.ts) into a single
 * CommonJS file that the Android NodeBackend plugin executes.
 *
 * Key points:
 * - `electron` is aliased to the shim so the reused electron/modules and
 *   electron/ipc code resolves there instead of the real (desktop) module.
 * - Native modules and the nodejs-mobile bridge stay external; they are
 *   provided by node_modules inside the nodejs-mobile project at runtime
 *   (cross-compiled for android-arm64).
 */
// Stay external (NOT bundled):
//   - better-sqlite3: native module, rebuilt for arm64 in nodejs-project
//   - cordova-bridge/rn-bridge: injected by nodejs-mobile at runtime
// Everything else (music-metadata, node-taglib-sharp, chokidar, p-queue, zod…)
// is pure JS and is BUNDLED via ssr.noExternal — this avoids ESM/CJS require()
// pitfalls on the nodejs-mobile Node runtime and shrinks the arm64 rebuild to a
// single module. `sharp` is aliased to a passthrough shim and bundled too.
const external = [
  ...builtinModules,
  ...builtinModules.map((m) => `node:${m}`),
  'cordova-bridge',
  'rn-bridge',
  // sql.js is emscripten glue: bundling it breaks its `module.exports = Module`
  // (the inlined `module` is undefined). Keep it external and load it as a real
  // CommonJS module from the runtime project's node_modules. Pure JS+WASM, no
  // per-arch build. The wasm is passed as a buffer by node-backend/sqlite-shim.ts.
  'sql.js'
]

export default defineConfig({
  plugins: [stripNodePrefix()],
  define: {
    // Mirror the desktop main-process key. AcoustID is dormant on Android
    // (no libfpcalc.so → computeFingerprint returns null) but the define keeps
    // the reference resolvable if fpcalc is ever bundled for Android.
    __ACOUSTID_APP_KEY__: JSON.stringify(process.env.ACOUSTID_APP_KEY ?? ''),
    // Keeps the outgoing User-Agent (net/http.ts) in sync with the app version.
    __APP_VERSION__: JSON.stringify(pkg.version)
  },
  resolve: {
    alias: {
      electron: resolve(__dirname, 'node-backend/electron-shim.ts'),
      sharp: resolve(__dirname, 'node-backend/sharp-shim.ts'),
      'better-sqlite3': resolve(__dirname, 'node-backend/sqlite-shim.ts'),
      '@shared': resolve(__dirname, 'shared')
    }
  },
  // In SSR builds Vite externalizes node_modules by default; force-bundle all
  // deps and keep only the native/runtime ones external.
  ssr: {
    noExternal: true,
    external: ['cordova-bridge', 'rn-bridge', 'sql.js']
  },
  build: {
    outDir: 'dist-node',
    emptyOutDir: true,
    // nodejs-mobile-cordova 0.4.3 ships Node 12.19 — no ??/?. etc. Lower the
    // target so esbuild transpiles modern syntax down to what Node 12 parses.
    target: 'node12',
    ssr: resolve(__dirname, 'node-backend/main.ts'),
    minify: false,
    rollupOptions: {
      external,
      output: {
        format: 'cjs',
        entryFileNames: 'main.js',
        inlineDynamicImports: true,
        banner: cryptoRandomUuidPolyfill + '\n' + es2021Polyfill + '\n' + textDecoderPolyfill
      }
    }
  }
})
