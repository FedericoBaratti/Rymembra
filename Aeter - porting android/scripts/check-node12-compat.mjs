// Build-time guard against JS APIs that DON'T exist on nodejs-mobile's runtime.
//
// nodejs-mobile-cordova 0.4.3 ships Node 12.19 / V8 7.8 (2019), built small-ICU.
// A surprising share of this port's "bugs" were modern APIs that compiled fine and
// passed the desktop tests, but threw only on the device (journal #2/#4/#6/#11).
// This script scans the code that ACTUALLY runs inside nodejs-mobile and fails the
// build when it spots an API with no runtime polyfill — moving those failures off
// the device and onto this machine.
//
// SCOPE: only the dirs bundled into dist-node/main.js and run on device —
//   electron/modules, node-backend, shared. NOT scripts/ (dev-machine Node 20+) and
//   NOT *.test.* (vitest runs on the dev Node, not on the device).
//
// INTENTIONALLY NOT FLAGGED (already polyfilled, so flagging would be a false
// positive on safe code):
//   - String.prototype.replaceAll, Array/String.prototype.at  → es2021Polyfill banner
//   - crypto.randomUUID                                        → cryptoRandomUuidPolyfill
//   - new TextDecoder('latin1'|…)                              → textDecoderPolyfill
//   - global fetch / AbortController / AbortSignal.timeout     → node-backend/net-polyfill.ts
//   (all in vite.config.node-backend.ts / node-backend/main.ts)
//
// Escape hatch: put `node12-ok` in a comment on the offending line to suppress it
// (e.g. a desktop-only branch guarded by isMobile).
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SCAN_DIRS = ['electron/modules', 'node-backend', 'shared']

// Patterns that genuinely throw / misbehave on Node 12.19 with no polyfill in place.
const RULES = [
  { re: /\\[pP]\{/, why: 'Unicode property escape \\p{…} — nodejs-mobile is small-ICU and throws "Invalid property name" at runtime (journal #11). Fold/scan by codepoint instead.' },
  { re: /withFileTypes/, why: 'readdir(…,{withFileTypes}) — on Android sdcardfs/FUSE returns DT_UNKNOWN and breaks (journal #4). Use readdir(dir) + stat().' },
  { re: /\brmSync\s*\(/, why: 'fs.rmSync — added in Node 14.14; absent on Node 12. Use unlinkSync (files) / rmdirSync.' },
  { re: /\bstructuredClone\s*\(/, why: 'structuredClone — added in Node 17.' },
  { re: /\.findLast(Index)?\s*\(/, why: 'Array.prototype.findLast/findLastIndex — added in Node 18.' },
  { re: /\.(toSorted|toReversed)\s*\(/, why: 'Array.prototype.toSorted/toReversed — added in Node 20.' },
  { re: /\bObject\.hasOwn\s*\(/, why: 'Object.hasOwn — added in Node 16.9. Use Object.prototype.hasOwnProperty.call.' },
  { re: /\b(Object|Map)\.groupBy\s*\(/, why: 'Object.groupBy/Map.groupBy — added in Node 21.' },
  { re: /\.fromAsync\s*\(/, why: 'Array.fromAsync — added in Node 22.' }
]

// Keywords after which a `/` begins a regex literal rather than a division.
const REGEX_KEYWORDS = new Set([
  'return', 'typeof', 'instanceof', 'in', 'of', 'case', 'do', 'else',
  'yield', 'void', 'delete', 'throw', 'new', 'await'
])

/** Heuristic: does a `/` in code position start a regex literal (vs division)?
 *  `word` is the identifier ending right before the slash (empty if the previous
 *  significant char isn't part of one). */
function startsRegex(lastSig, word) {
  if (lastSig === '') return true // start of input — nothing can be divided
  if (/[\w$]/.test(lastSig)) return REGEX_KEYWORDS.has(word) // after ident/number/keyword
  // After a value end, `/` is division; after any operator/open-bracket it's a regex.
  if (')]}'.includes(lastSig) || lastSig === "'" || lastSig === '"' || lastSig === '`') return false
  return true
}

/** Replace comment bodies with spaces, preserving offsets and newlines, so a token
 *  named only inside a comment (e.g. "// no \p{} regex") is not a false positive.
 *  Regex literals are copied verbatim (so a \p{…} inside one is still flagged) but
 *  parsed as their own state, so a quote inside a regex (e.g. /['"]/) can't desync
 *  the scanner into thinking it has entered a string. */
function blankComments(src) {
  let out = ''
  let i = 0
  let state = 'code' // 'code' | 'line' | 'block' | 'sq' | 'dq' | 'tpl' | 'regex'
  let lastSig = '' // last significant (non-whitespace) char seen in code state
  let word = '' // identifier currently being accumulated in code state
  let lastWord = '' // last complete identifier flushed before the cursor
  let inClass = false // inside a [...] char class while in 'regex'
  while (i < src.length) {
    const c = src[i]
    const n = src[i + 1]
    if (state === 'code') {
      if (c === '/' && n === '/') { out += '  '; i += 2; state = 'line'; continue }
      if (c === '/' && n === '*') { out += '  '; i += 2; state = 'block'; continue }
      if (c === '/' && startsRegex(lastSig, word || lastWord)) {
        if (word) { lastWord = word; word = '' }
        out += c; i++; state = 'regex'; inClass = false; lastSig = '/'; continue
      }
      if (c === "'" || c === '"' || c === '`') {
        if (word) { lastWord = word; word = '' }
        out += c; i++; lastSig = c
        state = c === "'" ? 'sq' : c === '"' ? 'dq' : 'tpl'
        continue
      }
      out += c
      if (/[\w$]/.test(c)) { word += c; lastSig = c }
      else { if (word) { lastWord = word; word = '' } if (/\S/.test(c)) lastSig = c }
      i++; continue
    }
    if (state === 'line') {
      if (c === '\n') { out += c; i++; state = 'code'; continue }
      out += c === '\t' ? '\t' : ' '; i++; continue
    }
    if (state === 'block') {
      if (c === '*' && n === '/') { out += '  '; i += 2; state = 'code'; continue }
      out += c === '\n' || c === '\t' ? c : ' '; i++; continue
    }
    if (state === 'regex') {
      if (c === '\\') { out += c + (src[i + 1] ?? ''); i += 2; continue }
      out += c
      if (c === '[') inClass = true
      else if (c === ']') inClass = false
      else if (c === '/' && !inClass) { state = 'code'; lastSig = ')'; word = ''; lastWord = '' }
      i++; continue
    }
    // string/template states: copy verbatim (real code), handle escapes + terminators
    if (c === '\\') { out += c + (src[i + 1] ?? ''); i += 2; continue }
    out += c
    if ((state === 'sq' && c === "'") || (state === 'dq' && c === '"') || (state === 'tpl' && c === '`')) {
      state = 'code'
    }
    i++
  }
  return out
}

function walk(dir) {
  const files = []
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, e.name)
    if (e.isDirectory()) {
      if (e.name === 'node_modules') continue
      files.push(...walk(full))
    } else if (/\.(ts|tsx)$/.test(e.name) && !/\.(test|spec)\.(ts|tsx)$/.test(e.name) && !e.name.endsWith('.d.ts')) {
      files.push(full)
    }
  }
  return files
}

const violations = []
for (const rel of SCAN_DIRS) {
  let files
  try {
    files = walk(resolve(root, rel))
  } catch {
    continue // dir may not exist in every checkout
  }
  for (const file of files) {
    const raw = readFileSync(file, 'utf8')
    const code = blankComments(raw)
    const rawLines = raw.split('\n')
    const lines = code.split('\n')
    for (let li = 0; li < lines.length; li++) {
      if (/node12-ok/.test(rawLines[li] ?? '')) continue
      for (const rule of RULES) {
        if (rule.re.test(lines[li])) {
          violations.push({ file: file.slice(root.length + 1).replace(/\\/g, '/'), line: li + 1, why: rule.why, text: (rawLines[li] ?? '').trim() })
        }
      }
    }
  }
}

if (violations.length === 0) {
  console.log('[check-node12] OK — no unpolyfilled Node-12-incompatible APIs in device code.')
  process.exit(0)
}

console.error(`[check-node12] ${violations.length} Node-12-incompatible API use(s) found:\n`)
for (const v of violations) {
  console.error(`  ${v.file}:${v.line}`)
  console.error(`    ${v.text}`)
  console.error(`    → ${v.why}`)
  console.error('    (suppress with a `node12-ok` comment on this line if intentionally desktop-only)\n')
}
process.exit(1)
