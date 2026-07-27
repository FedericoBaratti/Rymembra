/**
 * Lo schema di partenza per un file nuovo.
 *
 * È lo stesso schema al quale arrivano le due storie legacy, scritto per esteso
 * invece che accumulato in diciotto passi. Un'installazione nuova non deve
 * ripercorrere l'archeologia: niente ALTER su tabelle vuote, niente tabelle
 * ricostruite due volte, niente ri-lettura dei tag richiesta da migrazioni che
 * non hanno dati da convertire.
 *
 * Il rischio di scriverlo a mano è ovvio — un DEFAULT o un NOT NULL diverso e le
 * installazioni nuove divergono da quelle aggiornate, in silenzio. Per questo il
 * test `parity.test.ts` non si limita a confrontare con un file atteso: replica
 * tutte e TRE le strade (baseline, storia desktop, storia android) e pretende che
 * producano lo stesso schema normalizzato. È l'invariante che nel legacy era
 * affidata a un file di fixture copiato a mano in due alberi.
 */

import type { Migration } from '../migrate'
import { BASELINE_VERSION } from '../migrate'
import { FTS_SCHEMA } from './sql'

const BASELINE_SCHEMA = `
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
    mb_recording_id TEXT,
    genre TEXT,
    enrich_status TEXT,
    enrich_attempted_at INTEGER,
    source TEXT,
    liked INTEGER NOT NULL DEFAULT 0,
    liked_at INTEGER,
    stats_updated_at INTEGER,
    album_key TEXT,
    mb_release_group_id TEXT,
    mb_release_id TEXT,
    spotify_album_id TEXT
  );

  CREATE INDEX idx_tracks_artist ON tracks(artist);
  CREATE INDEX idx_tracks_album ON tracks(album);
  CREATE INDEX idx_tracks_title ON tracks(title);
  CREATE INDEX idx_tracks_genre ON tracks(genre);
  CREATE INDEX idx_tracks_enrich ON tracks(enrich_status);
  CREATE INDEX idx_tracks_liked ON tracks(liked, liked_at);
  CREATE INDEX idx_tracks_album_key ON tracks(album_key);

  CREATE TABLE albums (
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
    cover_art_hash TEXT,
    is_smart INTEGER NOT NULL DEFAULT 0,
    rules TEXT
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
    error_message TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_retry_at INTEGER,
    last_failure_class TEXT
  );
  CREATE INDEX idx_downloads_pending ON downloads(status, next_retry_at);

  -- Solo l'indice: i pixel stanno su disco, serviti via aether://art. Tenerli
  -- qui gonfiava il file, e sul mobile ogni scrittura ri-serializzava l'intero
  -- database.
  CREATE TABLE cover_art (
    hash TEXT PRIMARY KEY,
    width INTEGER NOT NULL DEFAULT 0,
    height INTEGER NOT NULL DEFAULT 0,
    mime_type TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'unknown'
  );

  CREATE TABLE waveforms (
    track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    peaks BLOB NOT NULL
  );

  CREATE TABLE api_cache (
    service TEXT NOT NULL,
    key TEXT NOT NULL,
    payload TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (service, key)
  ) WITHOUT ROWID;
  CREATE INDEX idx_api_cache_expiry ON api_cache(expires_at);

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

  CREATE TABLE sync_tombstones (
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    deleted_at INTEGER NOT NULL,
    PRIMARY KEY (kind, key)
  );

  CREATE TABLE library_fetch (
    track_key     TEXT PRIMARY KEY,
    title         TEXT NOT NULL DEFAULT '',
    artist        TEXT NOT NULL DEFAULT '',
    album         TEXT NOT NULL DEFAULT '',
    duration      REAL NOT NULL DEFAULT 0,
    status        TEXT NOT NULL DEFAULT 'pending',
    attempts      INTEGER NOT NULL DEFAULT 0,
    next_retry_at INTEGER,
    download_id   INTEGER,
    error         TEXT,
    updated_at    INTEGER NOT NULL
  );
  CREATE INDEX idx_library_fetch_status ON library_fetch(status, next_retry_at);

  CREATE TABLE play_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id INTEGER NOT NULL,
    played_at INTEGER NOT NULL,
    ms_played INTEGER
  );
  CREATE INDEX idx_play_history_played_at ON play_history(played_at);
  CREATE INDEX idx_play_history_track ON play_history(track_id);

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
    status TEXT NOT NULL DEFAULT 'pending',
    album_artist TEXT,
    disc_number INTEGER,
    track_number INTEGER,
    year INTEGER
  );

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

export const BASELINE: readonly Migration[] = [
  {
    version: BASELINE_VERSION,
    name: 'baseline',
    up: (ctx) => {
      ctx.db.exec(BASELINE_SCHEMA)
      if (ctx.fts5) ctx.db.exec(FTS_SCHEMA)
    }
  }
]
