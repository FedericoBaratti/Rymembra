const Database = require('better-sqlite3')
const path = require('node:path')
const db = new Database(path.join(process.env.APPDATA, 'aether', 'aether.db'), { readonly: true })

const tracks = db
  .prepare(
    `SELECT id, title, artist, album, year, track_number, round(duration,1) AS dur, codec,
     (cover_art_hash IS NOT NULL) AS has_cover FROM tracks ORDER BY id`
  )
  .all()
console.log('tracks:', JSON.stringify(tracks, null, 1))
console.log('albums:', JSON.stringify(db.prepare('SELECT id,title,artist,total_tracks,(cover_art_hash IS NOT NULL) AS has_cover FROM albums').all()))
console.log('artists:', JSON.stringify(db.prepare('SELECT name FROM artists').all()))
console.log('covers:', db.prepare('SELECT COUNT(*) AS n FROM cover_art').get().n)
console.log(
  'fts(aurora):',
  JSON.stringify(
    db.prepare("SELECT t.title FROM tracks_fts f JOIN tracks t ON t.id=f.rowid WHERE tracks_fts MATCH '\"aurora\"*'").all()
  )
)
db.close()
