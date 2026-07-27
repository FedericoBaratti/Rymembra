// Porta tutti i manifest del monorepo alla stessa versione.
//
//   npm run version:set -- 1.1.0
//
// Riscrive il solo campo `version`, con una sostituzione mirata invece di un
// JSON.stringify dell'oggetto intero: rigenerare il file riordinerebbe le chiavi
// e cambierebbe la formattazione di sei manifest, seppellendo il bump — che e'
// una riga — dentro una diff di seicento. La diff di un rilascio deve essere
// leggibile in dieci secondi.
//
// Scrive in UTF-8 esplicito e senza passare da PowerShell, che legge con la
// codepage ANSI e corromperebbe gli accenti nelle descrizioni.
import { readFileSync, writeFileSync } from 'node:fs'
import { manifestPaths } from './check-version-sync.mjs'

const target = process.argv[2]
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9a-z.-]+)?$/i

if (!target || !SEMVER.test(target)) {
  console.error('Uso: npm run version:set -- <versione>   (es. 1.1.0, 1.1.0-rc.1)')
  process.exit(1)
}

let changed = 0
for (const path of manifestPaths()) {
  const raw = readFileSync(path, 'utf8')
  // Solo la PRIMA occorrenza di una chiave `version` di primo livello: un
  // `"version"` dentro dependencies o scripts non deve essere toccato.
  const replaced = raw.replace(/^(\s*"version"\s*:\s*)"[^"]*"/m, `$1"${target}"`)
  if (replaced === raw) continue
  writeFileSync(path, replaced, 'utf8')
  changed++
}

console.log(`[version:set] ${changed} manifest portati a ${target}.`)
console.log('Ricorda: la voce corrispondente in CHANGELOG.md e il tag v' + target + '.')
