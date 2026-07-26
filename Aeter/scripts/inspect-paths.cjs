const Database = require('better-sqlite3')
const path = require('node:path')
const db = new Database(path.join(process.env.APPDATA, 'aether', 'aether.db'), { readonly: true })
console.log('total tracks:', db.prepare('SELECT COUNT(*) n FROM tracks').get().n)
const dirs = db
  .prepare(`SELECT path FROM tracks LIMIT 5`)
  .all()
  .map((r) => r.path)
console.log('sample paths:', JSON.stringify(dirs, null, 1))
console.log('albums:', db.prepare('SELECT COUNT(*) n FROM albums').get().n)
console.log('artists:', db.prepare('SELECT COUNT(*) n FROM artists').get().n)
console.log('covers:', db.prepare('SELECT COUNT(*) n FROM cover_art').get().n)
console.log('waveforms:', db.prepare('SELECT COUNT(*) n FROM waveforms').get().n)
console.log(
  'test tracks:',
  JSON.stringify(db.prepare("SELECT title, artist, album FROM tracks WHERE path LIKE '%AetherTest%'").all(), null, 1)
)
console.log(
  'fts(aurora):',
  JSON.stringify(db.prepare("SELECT t.title FROM tracks_fts f JOIN tracks t ON t.id=f.rowid WHERE tracks_fts MATCH '\"aurora\"*' LIMIT 5").all())
)
db.close()
