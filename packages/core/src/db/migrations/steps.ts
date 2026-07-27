/**
 * I passi che non sono solo SQL: calcolano, o toccano i file.
 *
 * Sono identici nei due alberi legacy — li ho confrontati riga per riga — quindi
 * qui esistono in una copia sola e le due storie li ordinano diversamente.
 *
 * Rispetto al legacy cambiano tre cose, tutte per rendere i passi provabili:
 *
 *   1. il DB arriva come `SqliteDriver`, non come `Database.Database`;
 *   2. i file arrivano da `ctx.files`, non da `writeFileSync` e `coverPath`
 *      importati. Nel legacy il test di parità dello schema poteva girare solo
 *      perché su un DB vuoto il ramo dei file non si eseguiva mai;
 *   3. i parametri con nome si passano espliciti. Il legacy passava l'oggetto
 *      aggregato per intero anche dove l'INSERT nominava meno colonne — con
 *      better-sqlite3 funzionava, con un driver più severo no.
 */

import { aggregateAlbums, albumGroupKey, buildAlbumGroups } from '../../shared/albumKey'
import type { AlbumAgg, AlbumAggInput } from '../../shared/albumKey'
import { upgradeLegacyTrackKey } from '../../shared/trackKey'
import type { MigrationContext } from '../migrate'
import type { SqlRow, SqlValue } from '../driver'
import {
  ALBUMS_KEYED_ON_ALBUM_KEY,
  COVER_ART_INDEX_ONLY,
  FTS_SCHEMA,
  INITIAL_SCHEMA
} from './sql'

function str(value: SqlValue | undefined): string {
  return typeof value === 'string' ? value : value === null || value === undefined ? '' : String(value)
}

function nullableStr(value: SqlValue | undefined): string | null {
  return typeof value === 'string' ? value : null
}

function nullableNum(value: SqlValue | undefined): number | null {
  return typeof value === 'number' ? value : null
}

function num(value: SqlValue | undefined): number {
  return typeof value === 'number' ? value : 0
}

function toBytes(value: SqlValue | undefined): Uint8Array | null {
  return value instanceof Uint8Array ? value : null
}

/** I parametri dell'INSERT su albums, esattamente le colonne che l'SQL nomina. */
function albumParams(album: AlbumAgg, withIds: boolean): Record<string, SqlValue> {
  const base: Record<string, SqlValue> = {
    album_key: album.album_key,
    title: album.title,
    artist: album.artist,
    year: album.year,
    total_tracks: album.total_tracks,
    cover_art_hash: album.cover_art_hash
  }
  if (!withIds) return base
  return { ...base, mb_album_id: album.mb_album_id, spotify_id: album.spotify_id }
}

function readAggInput(rows: SqlRow[]): AlbumAggInput[] {
  return rows.map((row) => ({
    album_key: str(row['album_key']),
    album: str(row['album']),
    album_artist: nullableStr(row['album_artist']),
    artist: nullableStr(row['artist']),
    year: nullableNum(row['year']),
    cover_art_hash: nullableStr(row['cover_art_hash']),
    mb_release_group_id: nullableStr(row['mb_release_group_id']),
    mb_release_id: nullableStr(row['mb_release_id']),
    spotify_album_id: nullableStr(row['spotify_album_id']),
    cover_source: nullableStr(row['cover_source']),
    cover_w: nullableNum(row['cover_w']),
    cover_h: nullableNum(row['cover_h'])
  }))
}

/**
 * Lo schema iniziale, con FTS5 solo dove esiste.
 *
 * È l'unico passo legacy che non è una stringa pura, e il motivo è il vincolo del
 * mobile: creare `tracks_fts` con i suoi trigger su sql.js, che non ha FTS5,
 * farebbe fallire ogni INSERT su `tracks` per sempre.
 */
export const initialSchema = (ctx: MigrationContext): void => {
  ctx.db.exec(INITIAL_SCHEMA)
  if (ctx.fts5) ctx.db.exec(FTS_SCHEMA)
}

/**
 * Sposta le copertine dal BLOB al filesystem, tenendo in SQLite solo l'indice.
 *
 * Sul desktop serve a non far crescere il DB; sul mobile è più grave: sql.js
 * persiste serializzando l'INTERO database a ogni scrittura, quindi copertine da
 * qualche MB rendevano ogni aggiornamento di rating o conteggio ascolti una
 * riscrittura di decine di MB — lenta, esosa di batteria, e a rischio corruzione
 * se Android uccide l'app nel mezzo.
 */
export const extractCoversToFiles = (ctx: MigrationContext): void => {
  const rows = ctx.db.prepare('SELECT hash, data, thumb FROM cover_art').all()

  for (const row of rows) {
    const hash = str(row['hash'])
    const data = toBytes(row['data'])
    const thumb = toBytes(row['thumb'])
    try {
      if (data !== null && !ctx.files.exists(ctx.files.coverPath(hash))) {
        ctx.files.write(ctx.files.coverPath(hash), data)
      }
      if (thumb !== null && !ctx.files.exists(ctx.files.coverPath(hash, true))) {
        ctx.files.write(ctx.files.coverPath(hash, true), thumb)
      }
    } catch (error) {
      // Al massimo dell'impegno, copertina per copertina: un file mancante
      // significa solo che verrà ri-scaricato. Ma va detto QUALE è fallita,
      // altrimenti disco pieno e permessi negati restano invisibili.
      ctx.warn(`estrazione della copertina ${hash} non riuscita`, error)
    }
  }

  ctx.db.exec(COVER_ART_INDEX_ONLY)
}

/**
 * Raggruppamento stabile degli album.
 *
 * Prima l'identità di un album era il testo ESATTO di (album, album_artist),
 * quindi una sola pubblicazione si spezzava in più schede per qualunque
 * incoerenza: maiuscole, spazi, diacritici, un suffisso di edizione ("(Deluxe
 * Edition)"), o un album_artist per traccia caduto su un ospite. Si introduce un
 * `album_key` normalizzato e si riscrive `albums` per indicizzarlo — prima era
 * UNIQUE(title, artist), che non riusciva a tenere distinte due pubblicazioni
 * omonime in cartelle diverse.
 */
export const backfillAlbumKey = (ctx: MigrationContext): void => {
  ctx.db.exec('ALTER TABLE tracks ADD COLUMN album_key TEXT')

  const tracks = ctx.db.prepare('SELECT id, album, path FROM tracks').all()
  const setKey = ctx.db.prepare('UPDATE tracks SET album_key = ? WHERE id = ?')
  for (const track of tracks) {
    setKey.run([albumGroupKey(str(track['album']), str(track['path'])), num(track['id'])])
  }

  ctx.db.exec('CREATE INDEX idx_tracks_album_key ON tracks(album_key)')
  ctx.db.exec(ALBUMS_KEYED_ON_ALBUM_KEY)

  // Ripopolata subito con l'aggregazione condivisa, così le librerie esistenti
  // mostrano gli album giusti senza attendere una nuova scansione.
  const input = readAggInput(
    ctx.db
      .prepare('SELECT album_key, album, album_artist, artist, year, cover_art_hash FROM tracks')
      .all()
  )
  const insert = ctx.db.prepare(
    `INSERT INTO albums (album_key, title, artist, year, total_tracks, cover_art_hash)
     VALUES (@album_key, @title, @artist, @year, @total_tracks, @cover_art_hash)`
  )
  for (const album of aggregateAlbums(input)) insert.run(albumParams(album, false))
}

/**
 * Identità dell'album maturata, in stile "Persistent ID" (come Navidrome).
 *
 * L'`album_key` era cartella più un titolo normalizzato DEBOLMENTE (solo
 * maiuscole e diacritici), quindi una pubblicazione si spezzava ancora su spazi
 * interni o punteggiatura — apici curvi contro dritti, lineetta contro trattino,
 * puntini di sospensione; e gli identificatori autorevoli nei tag (MusicBrainz
 * release e release-group) erano ignorati del tutto.
 */
export const persistentAlbumIdentity = (ctx: MigrationContext): void => {
  ctx.db.exec(`
    ALTER TABLE tracks ADD COLUMN mb_release_group_id TEXT;
    ALTER TABLE tracks ADD COLUMN mb_release_id TEXT;
    ALTER TABLE tracks ADD COLUMN spotify_album_id TEXT;
  `)

  const tracks = ctx.db.prepare('SELECT id, album, path FROM tracks').all()
  const setKey = ctx.db.prepare('UPDATE tracks SET album_key = ? WHERE id = ?')
  for (const track of tracks) {
    setKey.run([albumGroupKey(str(track['album']), str(track['path'])), num(track['id'])])
  }

  // Ri-lettura dei tag alla prossima scansione per riempire i nuovi id: le righe
  // vecchie non ne hanno ancora. Stesso schema del riempimento di genre.
  ctx.db.exec('UPDATE tracks SET date_modified = 0 WHERE is_local = 1')

  const input = readAggInput(
    ctx.db
      .prepare(
        `SELECT album_key, album, album_artist, artist, year, cover_art_hash,
                mb_release_group_id, mb_release_id, spotify_album_id FROM tracks`
      )
      .all()
  )
  const { albums, remap } = buildAlbumGroups(input)

  const reKey = ctx.db.prepare('UPDATE tracks SET album_key = ? WHERE album_key = ?')
  for (const [base, canonical] of remap) {
    if (base !== canonical) reKey.run([canonical, base])
  }

  // Ricostruzione pulita: i valori di album_key cambiano in blocco (nuovo
  // normalizzatore più canonicalizzazione), quindi DELETE+INSERT è più semplice e
  // sicuro — buildAlbumGroups produce una riga per chiave.
  ctx.db.exec('DELETE FROM albums')
  const insert = ctx.db.prepare(
    `INSERT INTO albums (album_key, title, artist, year, total_tracks, cover_art_hash, mb_album_id, spotify_id)
     VALUES (@album_key, @title, @artist, @year, @total_tracks, @cover_art_hash, @mb_album_id, @spotify_id)`
  )
  for (const album of albums) insert.run(albumParams(album, true))
}

/**
 * Provenienza della copertina e copertina canonica dell'album.
 *
 * `cover_art.source` registra da dove viene ogni immagine (tag, provider,
 * spotify, caa, unknown) così la copertina dell'album si può scegliere in modo
 * deterministico — provenienza, poi numero di membri, poi area in pixel, poi hash
 * — invece di "l'hash del primo membro, e per sempre".
 */
export const recomputeCoverProvenance = (ctx: MigrationContext): void => {
  ctx.db.exec(`ALTER TABLE cover_art ADD COLUMN source TEXT NOT NULL DEFAULT 'unknown'`)

  const input = readAggInput(
    ctx.db
      .prepare(
        `SELECT t.album_key, t.album, t.album_artist, t.artist, t.year, t.cover_art_hash,
                t.mb_release_group_id, t.mb_release_id, t.spotify_album_id,
                c.source AS cover_source, c.width AS cover_w, c.height AS cover_h
         FROM tracks t LEFT JOIN cover_art c ON c.hash = t.cover_art_hash`
      )
      .all()
  )
  const { albums } = buildAlbumGroups(input)
  const setCover = ctx.db.prepare('UPDATE albums SET cover_art_hash = ? WHERE album_key = ?')
  for (const album of albums) setCover.run([album.cover_art_hash, album.album_key])
}

/**
 * trackKey v2: la chiave di sync ha perso il segmento della durata.
 *
 * Lo scarto di durata fra due codifiche della stessa canzone faceva sembrare
 * distinte tracce identiche su due dispositivi, e il risultato erano ri-download
 * senza fine. Si ri-chiavano le pietre tombali (in caso di collisione vince la
 * cancellazione più recente) e si buttano le righe di library_fetch con chiave v1
 * — il prossimo sync le ricrea con la chiave nuova.
 */
export const rekeyTrackKeysV2 = (ctx: MigrationContext): void => {
  const tombstones = ctx.db
    .prepare("SELECT key, deleted_at FROM sync_tombstones WHERE kind = 'track'")
    .all()
  const deleteTombstone = ctx.db.prepare(
    "DELETE FROM sync_tombstones WHERE kind = 'track' AND key = ?"
  )
  const upsertTombstone = ctx.db.prepare(
    `INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES ('track', ?, ?)
     ON CONFLICT(kind, key) DO UPDATE SET
       deleted_at = MAX(deleted_at, excluded.deleted_at)`
  )

  for (const row of tombstones) {
    const key = str(row['key'])
    const upgraded = upgradeLegacyTrackKey(key)
    if (upgraded === key) continue
    deleteTombstone.run([key])
    upsertTombstone.run([upgraded, num(row['deleted_at'])])
  }

  const fetches = ctx.db.prepare('SELECT track_key FROM library_fetch').all()
  const deleteFetch = ctx.db.prepare('DELETE FROM library_fetch WHERE track_key = ?')
  for (const row of fetches) {
    const key = str(row['track_key'])
    if (upgradeLegacyTrackKey(key) !== key) deleteFetch.run([key])
  }
}
