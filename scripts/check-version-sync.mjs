// Guardia: tutti i package.json del monorepo dichiarano la stessa versione.
//
// Non e' pignoleria. La versione finisce in tre posti che l'utente vede — il
// nome dell'installer, `app.getVersion()` nel log di avvio e nella schermata
// informazioni, e il `version` che il telefono legge da /health per decidere se
// il protocollo e' compatibile — e quei tre posti la prendono da package.json
// DIVERSI. Con sei manifest e un bump fatto a mano, la domanda non e' se
// divergeranno ma quando: nel legacy la cartella release/ contiene ancora
// `Aether Setup 0.9.14.7.26.2.exe` accanto a `0.9.13.7.26.2`, cioe' proprio
// questo problema in forma di artefatti.
//
// La radice e' la fonte di verita'. `npm run version:set -- <versione>` allinea
// tutto; questo script fallisce se qualcuno l'ha fatto a mano e ne ha saltato uno.
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')

/** I manifest da tenere allineati: la radice piu' ogni workspace. */
export function manifestPaths(base = root) {
  const found = [join(base, 'package.json')]
  for (const group of ['packages', 'apps']) {
    const dir = join(base, group)
    let entries
    try {
      entries = readdirSync(dir)
    } catch {
      continue
    }
    for (const name of entries) {
      const manifest = join(dir, name, 'package.json')
      try {
        if (statSync(manifest).isFile()) found.push(manifest)
      } catch {
        /* una cartella senza manifest non e' un workspace */
      }
    }
  }
  return found
}

const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9a-z.-]+)?$/i

const manifests = manifestPaths()
const rootManifest = JSON.parse(readFileSync(manifests[0], 'utf8'))
const expected = rootManifest.version

const problems = []

if (typeof expected !== 'string' || !SEMVER.test(expected)) {
  problems.push(`la radice dichiara una versione non valida: ${JSON.stringify(expected)}`)
}

for (const path of manifests.slice(1)) {
  const manifest = JSON.parse(readFileSync(path, 'utf8'))
  const relative = path.slice(root.length + 1).replace(/\\/g, '/')
  if (manifest.version !== expected) {
    problems.push(`${relative}: ${manifest.version} invece di ${expected}`)
  }
}

// Un solo workspace scoperto vuol dire che la struttura e' cambiata sotto lo
// script: un OK su un manifest solo sarebbe un falso negativo, come in
// check-node12.
if (manifests.length < 2) {
  problems.push('nessun workspace trovato: aggiorna manifestPaths(), non fidarti di questo OK')
}

if (problems.length === 0) {
  console.log(
    `[check-version] OK — ${manifests.length} manifest tutti a ${expected}.`
  )
  process.exit(0)
}

console.error('[check-version] le versioni non sono allineate:\n')
for (const problem of problems) console.error(`  ${problem}`)
console.error('\nAllinea con: npm run version:set -- <versione>')
process.exit(1)
