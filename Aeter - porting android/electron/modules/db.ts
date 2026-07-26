import Database from 'better-sqlite3'
import { app } from 'electron'
import { join } from 'node:path'
import { existsSync, writeFileSync } from 'node:fs'
import { preMigrationBackup } from './dbBackup'
import { coverPath } from './coverPaths'
import { logWarn } from './logger'
import { foldText } from '@shared/text'
import { upgradeLegacyTrackKey } from '@shared/trackKey'
import { albumGroupKey, aggregateAlbums, buildAlbumGroups, type AlbumAggInput } from './albumKey'

let db: Database.Database | null = null

// FTS5 is unavailable in the sql.js (WASM) build used on Android; skip the
// virtual table + its triggers there (the triggers would otherwise make every
// tracks INSERT/UPDATE fail). search() already falls back to LIKE queries.
const FTS_SCHEMA =
  process.platform === 'android'
    ? ''
    : `
  CREATE VIRTUAL TABLE tracks_fts USING fts5(
    title, artist, album, album_artist,
    content='tracks', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
  );

  CREATE TRIGGER tracks_ai AFTER INSERT ON tracks BEGIN
    INSERT INTO tracks_fts(rowid, title, artist, album, album_artist)
    VALUES (new.id, new.title, new.artist, new.album, new.album_artist);
  END;
  CREATE TRIGGER tracks_ad AFTER DELETE ON tracks BEGIN
    INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist)
    VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist);
  END;
  CREATE TRIGGER tracks_au AFTER UPDATE ON tracks BEGIN
    INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist)
    VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist);
    INSERT INTO tracks_fts(rowid, title, artist, album, album_artist)
    VALUES (new.id, new.title, new.artist, new.album, new.album_artist);
  END;
  `

/**
 * A migration is either a SQL script or a JS function (for steps that need to
 * touch the filesystem, e.g. moving cover BLOBs out to files). Each runs inside
 * a transaction that also bumps user_version.
 */
type Migration = string | ((d: Database.Database) => void)

// Exported for db.schema.test.ts, which replays the chain on an in-memory DB
// and compares the resulting schema against the fixture shared with the
// desktop tree. NEVER renumber or edit shipped steps — append only.
export const MIGRATIONS: Migration[] = [
  // v1 — initial schema
  `
  CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    artist TEXT NOT NULL DEFAULT '',
    album TEXT NOT NULL DEFAULT '',
    album_artist TEXT,
    year INTEGER,
    track_number INTEGER,
    disc_number INTEGER,
    duration REAL NOT NULL DEFAULT 0,
    bitrate INTEGER,
    sample_rate INTEGER,
    codec TEXT,
    file_size INTEGER NOT NULL DEFAULT 0,
    date_added INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    play_count INTEGER NOT NULL DEFAULT 0,
    last_played INTEGER,
    rating INTEGER NOT NULL DEFAULT 0,
    bpm REAL,
    key TEXT,
    comment TEXT,
    lyrics TEXT,
    cover_art_hash TEXT,
    is_local INTEGER NOT NULL DEFAULT 1,
    replaygain_track_gain REAL,
    replaygain_album_gain REAL,
    acoustid_fingerprint TEXT,
    mb_recording_id TEXT
  );

  CREATE INDEX idx_tracks_artist ON tracks(artist);
  CREATE INDEX idx_tracks_album ON tracks(album);
  CREATE INDEX idx_tracks_title ON tracks(title);

  CREATE TABLE albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    year INTEGER,
    genre TEXT,
    total_tracks INTEGER NOT NULL DEFAULT 0,
    cover_art_hash TEXT,
    mb_album_id TEXT,
    spotify_id TEXT,
    UNIQUE(title, artist)
  );

  CREATE TABLE artists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    bio TEXT,
    image_hash TEXT,
    mb_artist_id TEXT,
    spotify_id TEXT
  );

  CREATE TABLE playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    cover_art_hash TEXT
  );

  CREATE TABLE playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, position)
  );

  CREATE TABLE downloads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_url TEXT NOT NULL,
    source_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    progress REAL NOT NULL DEFAULT 0,
    title TEXT NOT NULL DEFAULT '',
    artist TEXT,
    album TEXT,
    cover_url TEXT,
    total_tracks INTEGER NOT NULL DEFAULT 1,
    completed_tracks INTEGER NOT NULL DEFAULT 0,
    current_file TEXT,
    file_path TEXT,
    created_at INTEGER NOT NULL,
    error_message TEXT
  );

  CREATE TABLE cover_art (
    hash TEXT PRIMARY KEY,
    data BLOB NOT NULL,
    thumb BLOB,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    mime_type TEXT NOT NULL
  );

  CREATE TABLE waveforms (
    track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    peaks BLOB NOT NULL
  );
  ` + FTS_SCHEMA,
  // v2 — smart playlists + per-track genre
  `
  ALTER TABLE playlists ADD COLUMN is_smart INTEGER NOT NULL DEFAULT 0;
  ALTER TABLE playlists ADD COLUMN rules TEXT;
  ALTER TABLE tracks ADD COLUMN genre TEXT;
  CREATE INDEX idx_tracks_genre ON tracks(genre);
  -- force a one-off re-parse on the next scan so genre gets backfilled from file tags
  UPDATE tracks SET date_modified = 0 WHERE is_local = 1;
  `,
  // v3 — download retry bookkeeping + API response cache
  `
  ALTER TABLE downloads ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
  ALTER TABLE downloads ADD COLUMN next_retry_at INTEGER;
  ALTER TABLE downloads ADD COLUMN last_failure_class TEXT;
  CREATE INDEX idx_downloads_pending ON downloads(status, next_retry_at);

  CREATE TABLE api_cache (
    service TEXT NOT NULL,
    key TEXT NOT NULL,
    payload TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (service, key)
  ) WITHOUT ROWID;
  CREATE INDEX idx_api_cache_expiry ON api_cache(expires_at);
  `,
  // v4 — per-track enrichment status tracking (null = never attempted)
  `
  ALTER TABLE tracks ADD COLUMN enrich_status TEXT;
  ALTER TABLE tracks ADD COLUMN enrich_attempted_at INTEGER;
  UPDATE tracks SET enrich_status = 'ok' WHERE mb_recording_id IS NOT NULL;
  CREATE INDEX idx_tracks_enrich ON tracks(enrich_status);
  `,
  // v5 — offline queue for Last.fm scrobbles (played_at = listen start, unix seconds)
  `
  CREATE TABLE scrobble_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    artist TEXT NOT NULL,
    title TEXT NOT NULL,
    album TEXT,
    duration INTEGER,
    played_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0
  );
  `,
  // v6 — track provenance for download post-processing (e.g. YouTube metadata fix)
  `
  ALTER TABLE tracks ADD COLUMN source TEXT;
  UPDATE tracks SET source = 'youtube' WHERE path IN (
    SELECT file_path FROM downloads
    WHERE source_type LIKE 'youtube%' AND file_path IS NOT NULL
  );
  `,
  // v7 — persistent Spotify migration so a long run survives an app/process kill
  // (nodejs-mobile is killed when Android reclaims a backgrounded app) and
  // resumes the not-yet-done tracks instead of losing everything. Single-row
  // header (id=1) + one row per track keyed by its original index.
  `
  CREATE TABLE spotify_migration (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    migration_id TEXT NOT NULL,
    status TEXT NOT NULL,
    source_url TEXT NOT NULL,
    kind TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    cover_url TEXT,
    recreate_playlist INTEGER NOT NULL DEFAULT 0,
    playlist_id INTEGER,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
  );

  CREATE TABLE spotify_migration_tracks (
    idx INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    artist TEXT,
    album TEXT,
    duration_ms INTEGER,
    cover_url TEXT,
    status TEXT NOT NULL DEFAULT 'pending'
  );
  `,
  // v8 — persist the full album metadata of a Spotify migration so a resume after
  // an app/process kill re-writes the SAME album_artist/disc/track/year tags and
  // the album does NOT re-split. Added as nullable columns (older rows = NULL).
  `
  ALTER TABLE spotify_migration_tracks ADD COLUMN album_artist TEXT;
  ALTER TABLE spotify_migration_tracks ADD COLUMN disc_number INTEGER;
  ALTER TABLE spotify_migration_tracks ADD COLUMN track_number INTEGER;
  ALTER TABLE spotify_migration_tracks ADD COLUMN year INTEGER;
  `,
  // v9 — move cover art BLOBs out of the DB onto the filesystem. sql.js persists
  // by serializing the WHOLE database on every write, so multi-MB cover BLOBs
  // made every recordPlay/rating/enrichment update rewrite tens of MB (slow,
  // battery-hungry, and a corruption risk on a background kill). Extract each
  // BLOB to <coversDir>/<hash>.webp (+ .t.webp), then rebuild cover_art keeping
  // only the lightweight index. Fresh installs have an empty cover_art, so this
  // is instant for them; the rebuild table pattern avoids relying on DROP COLUMN.
  (d: Database.Database): void => {
    const rows = d.prepare('SELECT hash, data, thumb FROM cover_art').all() as {
      hash: string
      data: Buffer | Uint8Array | null
      thumb: Buffer | Uint8Array | null
    }[]
    for (const r of rows) {
      try {
        if (r.data && !existsSync(coverPath(r.hash))) {
          writeFileSync(coverPath(r.hash), Buffer.from(r.data))
        }
        if (r.thumb && !existsSync(coverPath(r.hash, true))) {
          writeFileSync(coverPath(r.hash, true), Buffer.from(r.thumb))
        }
      } catch (err) {
        // best effort per cover: a missing file just means a re-fetch later —
        // but say WHICH cover failed, or disk-full/permission bugs stay invisible
        logWarn('db', `Migrazione v9: estrazione copertina ${r.hash} fallita`, err)
      }
    }
    d.exec(`
      CREATE TABLE cover_art_new (
        hash TEXT PRIMARY KEY,
        width INTEGER NOT NULL DEFAULT 0,
        height INTEGER NOT NULL DEFAULT 0,
        mime_type TEXT NOT NULL
      );
      INSERT INTO cover_art_new (hash, width, height, mime_type)
        SELECT hash, width, height, mime_type FROM cover_art;
      DROP TABLE cover_art;
      ALTER TABLE cover_art_new RENAME TO cover_art;
    `)
  },
  // v10 — stable album grouping. The album identity used to be the EXACT
  // (album, album_artist) text, so a single release split into multiple album cards
  // on any inconsistency: case/whitespace/diacritics, an edition suffix
  // ("(Deluxe Edition)"), or a per-track album_artist that fell back to a guest
  // credit. Introduce a normalized `album_key` (fold(stripEdition(album)) + folder,
  // see albumKey.ts) computed per track, and rebuild the `albums` table to key on it
  // (was UNIQUE(title, artist) — which could not keep two same-titled releases in
  // different folders apart). album_key is derived from album+path only, so it is
  // computed at upsert time and backfilled here.
  (d: Database.Database): void => {
    d.exec('ALTER TABLE tracks ADD COLUMN album_key TEXT')

    const tracks = d.prepare('SELECT id, album, path FROM tracks').all() as {
      id: number
      album: string
      path: string
    }[]
    const setKey = d.prepare('UPDATE tracks SET album_key = ? WHERE id = ?')
    for (const t of tracks) setKey.run(albumGroupKey(t.album ?? '', t.path), t.id)

    d.exec('CREATE INDEX idx_tracks_album_key ON tracks(album_key)')

    // Rebuild albums keyed on album_key (rebuild-table pattern, like v9). Repopulate
    // immediately with the shared aggregation so existing libraries show correct
    // albums without waiting for a rescan (desktop boot does not call repair).
    d.exec(`
      CREATE TABLE albums_new (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        album_key TEXT NOT NULL UNIQUE,
        title TEXT NOT NULL,
        artist TEXT NOT NULL,
        year INTEGER,
        genre TEXT,
        total_tracks INTEGER NOT NULL DEFAULT 0,
        cover_art_hash TEXT,
        mb_album_id TEXT,
        spotify_id TEXT
      );
      DROP TABLE albums;
      ALTER TABLE albums_new RENAME TO albums;
    `)

    const aggInput = d
      .prepare('SELECT album_key, album, album_artist, artist, year, cover_art_hash FROM tracks')
      .all() as AlbumAggInput[]
    const insert = d.prepare(
      `INSERT INTO albums (album_key, title, artist, year, total_tracks, cover_art_hash)
       VALUES (@album_key, @title, @artist, @year, @total_tracks, @cover_art_hash)`
    )
    for (const a of aggregateAlbums(aggInput)) insert.run(a)
  },
  // v11 — mature "Persistent ID" album identity (Navidrome-style). The album_key was
  // folder + a WEAKLY-normalized title (only case/diacritics), so a release still split
  // on internal whitespace or punctuation (curly vs straight quotes, en/em-dash vs
  // hyphen, ellipsis); and authoritative ids in file tags (MusicBrainz release/release-
  // group) were ignored entirely. This migration: (1) adds the id columns, (2) recomputes
  // album_key with the hardened normalizer, (3) forces a one-off re-parse so the next
  // scan backfills the ids from tags, and (4) rebuilds `albums` with the shared
  // merge-aware builder (which unites — never splits — base groups sharing an id).
  (d: Database.Database): void => {
    d.exec(`
      ALTER TABLE tracks ADD COLUMN mb_release_group_id TEXT;
      ALTER TABLE tracks ADD COLUMN mb_release_id TEXT;
      ALTER TABLE tracks ADD COLUMN spotify_album_id TEXT;
    `)

    const tracks = d.prepare('SELECT id, album, path FROM tracks').all() as {
      id: number
      album: string
      path: string
    }[]
    const setKey = d.prepare('UPDATE tracks SET album_key = ? WHERE id = ?')
    for (const t of tracks) setKey.run(albumGroupKey(t.album ?? '', t.path), t.id)

    // Re-read tags on the next scan to backfill the new ids (older rows have none yet);
    // that scan's rebuild then merges editions/copies. Same pattern as the v2 genre backfill.
    d.exec('UPDATE tracks SET date_modified = 0 WHERE is_local = 1')

    const aggInput = d
      .prepare(
        `SELECT album_key, album, album_artist, artist, year, cover_art_hash,
                mb_release_group_id, mb_release_id, spotify_album_id FROM tracks`
      )
      .all() as AlbumAggInput[]
    const { albums, remap } = buildAlbumGroups(aggInput)
    const reKey = d.prepare('UPDATE tracks SET album_key = ? WHERE album_key = ?')
    for (const [base, canonical] of remap) if (base !== canonical) reKey.run(canonical, base)

    // Clean rebuild: album_key values change wholesale (new normalizer + canonicalization),
    // so DELETE+INSERT is simpler and safe (buildAlbumGroups yields one row per key).
    d.exec('DELETE FROM albums')
    const insert = d.prepare(
      `INSERT INTO albums (album_key, title, artist, year, total_tracks, cover_art_hash, mb_album_id, spotify_id)
       VALUES (@album_key, @title, @artist, @year, @total_tracks, @cover_art_hash, @mb_album_id, @spotify_id)`
    )
    for (const a of albums) insert.run(a)
  },
  // v12 — discovery layer. Two additions that power the Spotify-style features
  // (Home, Radio/autoplay, Liked Songs, listening stats) without touching the
  // hot scan path: (1) a "Liked Songs" flag on tracks (liked + liked_at, so the
  // collection is `WHERE liked=1 ORDER BY liked_at DESC`), and (2) a play_history
  // table — one row per completed play — so stats/recommendations can reason about
  // recency and frequency beyond the aggregate play_count/last_played. Recommendation
  // API responses are cached in the existing api_cache table (no new table needed).
  `
  ALTER TABLE tracks ADD COLUMN liked INTEGER NOT NULL DEFAULT 0;
  ALTER TABLE tracks ADD COLUMN liked_at INTEGER;
  CREATE INDEX idx_tracks_liked ON tracks(liked, liked_at);

  CREATE TABLE play_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id INTEGER NOT NULL,
    played_at INTEGER NOT NULL,
    ms_played INTEGER
  );
  CREATE INDEX idx_play_history_played_at ON play_history(played_at);
  CREATE INDEX idx_play_history_track ON play_history(track_id);
  `,
  // v13 — podcasts. The one Spotify content pillar that fits a local-first,
  // account-less app: feeds are public RSS (audio over HTTP), so no server and no
  // login. `podcasts` holds the subscription, `podcast_episodes` the items. Audio
  // is streamed from `audio_url` (the player's stream_url override); `progress_sec`
  // + `played` track resume/played state per episode.
  `
  CREATE TABLE podcasts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    author TEXT,
    description TEXT,
    image_url TEXT,
    added_at INTEGER NOT NULL,
    last_refreshed INTEGER
  );

  CREATE TABLE podcast_episodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    podcast_id INTEGER NOT NULL,
    guid TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    audio_url TEXT NOT NULL,
    image_url TEXT,
    duration INTEGER,
    published_at INTEGER,
    progress_sec INTEGER NOT NULL DEFAULT 0,
    played INTEGER NOT NULL DEFAULT 0,
    UNIQUE (podcast_id, guid)
  );
  CREATE INDEX idx_episodes_podcast ON podcast_episodes(podcast_id, published_at);
  `,
  // v14 — cover provenance + canonical album cover. cover_art.source records
  // where each image came from (tag|provider|spotify|caa|unknown) so the album
  // cover can be picked deterministically (provenance > member count > pixel
  // area > hash) instead of "first member's hash, sticky forever". Existing
  // rows get 'unknown'; the recompute below re-picks every albums.cover_art_hash
  // with the new selection (rebuildAggregates keeps it recomputed from now on).
  (d: Database.Database): void => {
    d.exec(`ALTER TABLE cover_art ADD COLUMN source TEXT NOT NULL DEFAULT 'unknown'`)

    const aggInput = d
      .prepare(
        `SELECT t.album_key, t.album, t.album_artist, t.artist, t.year, t.cover_art_hash,
                t.mb_release_group_id, t.mb_release_id, t.spotify_album_id,
                c.source AS cover_source, c.width AS cover_w, c.height AS cover_h
         FROM tracks t LEFT JOIN cover_art c ON c.hash = t.cover_art_hash`
      )
      .all() as AlbumAggInput[]
    const { albums } = buildAlbumGroups(aggInput)
    const setCover = d.prepare('UPDATE albums SET cover_art_hash = ? WHERE album_key = ?')
    for (const a of albums) setCover.run(a.cover_art_hash, a.album_key)
  },
  // v15 — library sync (Google Drive). `stats_updated_at` gives rating/liked a
  // last-writer-wins clock so the newer of two devices' edits wins on merge; it
  // stays NULL for rows untouched since this migration (snapshot maps NULL→0, so
  // pre-existing ratings simply don't clobber — they converge once re-touched).
  // `sync_tombstones` records ONLY explicit deletions (deleteTracks /
  // deletePlaylist) so a delete on one device isn't resurrected by a stale copy
  // on another; a track whose file still exists elsewhere is reborn regardless.
  `
  ALTER TABLE tracks ADD COLUMN stats_updated_at INTEGER;

  CREATE TABLE sync_tombstones (
    kind TEXT NOT NULL,            -- 'track' | 'playlist'
    key TEXT NOT NULL,             -- trackKey / playlistKey
    deleted_at INTEGER NOT NULL,
    PRIMARY KEY (kind, key)
  );
  `,
  // v16 — auto-fetch of missing tracks. A track present in the synced metadata
  // (remote-only pass-through) but absent on this device is recorded here so it
  // can be re-downloaded from the download sources (YouTube search). The row
  // snapshots the metadata needed for the search, and tracks the attempt state
  // so a permanent no-match isn't retried forever. Rows are removed once the
  // track becomes local (downloaded) or leaves the merged library.
  `
  CREATE TABLE library_fetch (
    track_key     TEXT PRIMARY KEY,   -- trackKey of the missing track
    title         TEXT NOT NULL DEFAULT '',
    artist        TEXT NOT NULL DEFAULT '',
    album         TEXT NOT NULL DEFAULT '',
    duration      REAL NOT NULL DEFAULT 0,
    status        TEXT NOT NULL DEFAULT 'pending', -- pending|searching|queued|done|failed
    attempts      INTEGER NOT NULL DEFAULT 0,
    next_retry_at INTEGER,
    download_id   INTEGER,            -- linked downloads.id, to stamp tags on completion
    error         TEXT,
    updated_at    INTEGER NOT NULL
  );
  CREATE INDEX idx_library_fetch_status ON library_fetch(status, next_retry_at);
  `,
  // v17 — trackKey v2: the sync key dropped its duration segment (encode drift
  // between devices made equal tracks look distinct → endless re-downloads).
  // Re-key track tombstones in place (collisions keep the newest deletion) and
  // drop v1-keyed fetch rows — the next sync recreates them under v2 keys.
  (d) => {
    const tombs = d
      .prepare("SELECT key, deleted_at FROM sync_tombstones WHERE kind = 'track'")
      .all() as { key: string; deleted_at: number }[]
    const delTomb = d.prepare("DELETE FROM sync_tombstones WHERE kind = 'track' AND key = ?")
    const upsertTomb = d.prepare(
      `INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES ('track', ?, ?)
       ON CONFLICT(kind, key) DO UPDATE SET
         deleted_at = MAX(deleted_at, excluded.deleted_at)`
    )
    for (const { key, deleted_at } of tombs) {
      const upgraded = upgradeLegacyTrackKey(key)
      if (upgraded === key) continue
      delTomb.run(key)
      upsertTomb.run(upgraded, deleted_at)
    }
    const fetches = d.prepare('SELECT track_key FROM library_fetch').all() as {
      track_key: string
    }[]
    const delFetch = d.prepare('DELETE FROM library_fetch WHERE track_key = ?')
    for (const { track_key } of fetches) {
      if (upgradeLegacyTrackKey(track_key) !== track_key) delFetch.run(track_key)
    }
  },
  // v18 — phone repair over WiFi. The table is DESKTOP-side state (the phone is
  // repaired BY a desktop; this device never writes it) — created here too so
  // the two trees keep an identical schema (twin fixture db.schema.expected.json).
  // Job identity is (device_id, phone_track_id); src_* snapshot the phone file
  // for idempotent re-runs, result_* let a retry detect an already-landed commit.
  `
  CREATE TABLE phone_repair (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id        TEXT NOT NULL,
    phone_track_id   INTEGER NOT NULL,
    track_key        TEXT NOT NULL,
    title            TEXT NOT NULL DEFAULT '',
    artist           TEXT NOT NULL DEFAULT '',
    album            TEXT NOT NULL DEFAULT '',
    status           TEXT NOT NULL DEFAULT 'pending',
    action_transcode INTEGER NOT NULL DEFAULT 0,
    action_enrich    INTEGER NOT NULL DEFAULT 0,
    attempts         INTEGER NOT NULL DEFAULT 0,
    next_retry_at    INTEGER,
    error            TEXT,
    src_size         INTEGER,
    src_mtime        INTEGER,
    src_sha256       TEXT,
    result_sha256    TEXT,
    result_ext       TEXT,
    updated_at       INTEGER NOT NULL,
    UNIQUE (device_id, phone_track_id)
  );
  CREATE INDEX idx_phone_repair_status ON phone_repair(status, next_retry_at);
  `
]

export function getDb(): Database.Database {
  if (db) return db
  const file = join(app.getPath('userData'), 'aether.db')
  db = new Database(file)
  db.pragma('journal_mode = WAL')
  db.pragma('foreign_keys = ON')
  db.pragma('synchronous = NORMAL')
  migrate(db)
  registerFunctions(db)
  return db
}

/**
 * Register SQL scalar functions. `afold()` = diacritic-insensitive lowercase,
 * used by search() so the LIKE fallback (the only path on Android, where FTS5 is
 * absent) matches accents and case the way FTS5 does on desktop.
 */
function registerFunctions(d: Database.Database): void {
  d.function('afold', (value: unknown): string => (value == null ? '' : foldText(String(value))))
}

function migrate(d: Database.Database): void {
  const version = d.pragma('user_version', { simple: true }) as number
  if (version > 0 && version < MIGRATIONS.length) {
    preMigrationBackup(d, version)
    // Android sql.js shim: refresh `<file>.bak` on the flush that follows the
    // migration, whatever the rate limit says (schema changes are the riskiest
    // writes). No-op on desktop better-sqlite3.
    ;(d as unknown as { forceBakOnNextWrite?: () => void }).forceBakOnNextWrite?.()
  }
  for (let v = version; v < MIGRATIONS.length; v++) {
    const step = MIGRATIONS[v]
    const run = d.transaction(() => {
      if (typeof step === 'function') step(d)
      else d.exec(step)
      d.pragma(`user_version = ${v + 1}`)
    })
    run()
  }
}

export function closeDb(): void {
  db?.close()
  db = null
}
