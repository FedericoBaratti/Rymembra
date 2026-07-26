// 100k-row spot check: insert synthetic tracks into a temp DB using the production
// schema, then time the getTracks-style sorted query and an FTS search.
const Database = require('better-sqlite3')
const { mkdtempSync, rmSync } = require('node:fs')
const { join } = require('node:path')
const { tmpdir } = require('node:os')

const dir = mkdtempSync(join(tmpdir(), 'aether-perf-'))
const db = new Database(join(dir, 'perf.db'))
db.pragma('journal_mode = WAL')

db.exec(`
CREATE TABLE tracks (
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  title TEXT, artist TEXT, album TEXT, album_artist TEXT,
  year INTEGER, track_number INTEGER, duration REAL, codec TEXT,
  play_count INTEGER DEFAULT 0, rating INTEGER, added_at INTEGER
);
CREATE INDEX idx_tracks_artist ON tracks(artist);
CREATE INDEX idx_tracks_album ON tracks(album);
CREATE VIRTUAL TABLE tracks_fts USING fts5(title, artist, album, content='tracks', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
`)

const words = ['Aurora', 'Nebula', 'Tide', 'Echo', 'Vento', 'Luce', 'Notte', 'Fiamma', 'Orbit', 'Pulse']
const ins = db.prepare(
  'INSERT INTO tracks (path, title, artist, album, year, track_number, duration, codec, added_at) VALUES (?,?,?,?,?,?,?,?,?)'
)
const t0 = Date.now()
db.transaction(() => {
  for (let i = 0; i < 100000; i++) {
    const w = words[i % 10]
    ins.run(
      `C:/fake/${i}.mp3`, `${w} ${i}`, `Artist ${i % 700}`, `Album ${i % 2500}`,
      1970 + (i % 55), (i % 14) + 1, 120 + (i % 300), 'MPEG 1 Layer 3', Date.now()
    )
  }
})()
db.exec(`INSERT INTO tracks_fts(rowid, title, artist, album) SELECT id, title, artist, album FROM tracks`)
console.log('insert 100k + fts:', Date.now() - t0, 'ms')

let t = Date.now()
const rows = db.prepare('SELECT * FROM tracks ORDER BY artist COLLATE NOCASE, album, track_number').all()
console.log('full sorted SELECT (' + rows.length + ' rows):', Date.now() - t, 'ms')

t = Date.now()
const hits = db
  .prepare(`SELECT t.id FROM tracks_fts f JOIN tracks t ON t.id = f.rowid WHERE tracks_fts MATCH '"aurora"*' LIMIT 50`)
  .all()
console.log('FTS search (' + hits.length + ' hits):', Date.now() - t, 'ms')

db.close()
rmSync(dir, { recursive: true, force: true })
