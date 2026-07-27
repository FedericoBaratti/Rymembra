/**
 * Ogni modifica di schema, nominata una volta sola.
 *
 * Le due storie legacy sono due ORDINAMENTI di questi blocchi. Scritti così, la
 * divergenza fra i due alberi diventa visibile come dato — si legge nell'ordine
 * degli import in `legacy.ts` — invece di essere sepolta in due array di 550
 * righe che nessuno confronta mai.
 *
 * Il testo SQL è copiato alla lettera dal legacy, commenti compresi. Non va
 * "pulito": un DEFAULT, un NOT NULL o un nome di indice diversi produrrebbero uno
 * schema diverso da quello che sta sui dischi degli utenti, e il test di parità
 * non ha modo di sapere quale dei due sia quello giusto.
 */

/**
 * v1, senza la parte FTS5.
 *
 * Separata perché sul backend mobile sql.js è compilato senza FTS5: creare la
 * tabella virtuale e i suoi tre trigger farebbe fallire ogni INSERT su `tracks`.
 * Il legacy lo decideva con `process.platform === 'android'` dentro il passo;
 * qui la decisione arriva da `ctx.fts5`.
 */
export const INITIAL_SCHEMA = `
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
`

/** La tabella virtuale e i suoi trigger. Solo dove FTS5 esiste. */
export const FTS_SCHEMA = `
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

/** Playlist intelligenti e genere per traccia. */
export const SMART_PLAYLISTS_AND_GENRE = `
  ALTER TABLE playlists ADD COLUMN is_smart INTEGER NOT NULL DEFAULT 0;
  ALTER TABLE playlists ADD COLUMN rules TEXT;
  ALTER TABLE tracks ADD COLUMN genre TEXT;
  CREATE INDEX idx_tracks_genre ON tracks(genre);
  -- una ri-lettura dei tag alla prossima scansione, per riempire genre
  UPDATE tracks SET date_modified = 0 WHERE is_local = 1;
`

/** Contabilità dei ritentativi di download e cache delle risposte API. */
export const DOWNLOAD_RETRY_AND_API_CACHE = `
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
`

/** Stato dell'arricchimento per traccia (null = mai tentato). */
export const ENRICH_STATUS = `
  ALTER TABLE tracks ADD COLUMN enrich_status TEXT;
  ALTER TABLE tracks ADD COLUMN enrich_attempted_at INTEGER;
  UPDATE tracks SET enrich_status = 'ok' WHERE mb_recording_id IS NOT NULL;
  CREATE INDEX idx_tracks_enrich ON tracks(enrich_status);
`

/** Coda offline degli scrobble Last.fm (played_at = inizio ascolto, secondi unix). */
export const SCROBBLE_QUEUE = `
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
`

/** Provenienza della traccia, per la post-elaborazione dei download. */
export const TRACK_SOURCE = `
  ALTER TABLE tracks ADD COLUMN source TEXT;
  UPDATE tracks SET source = 'youtube' WHERE path IN (
    SELECT file_path FROM downloads
    WHERE source_type LIKE 'youtube%' AND file_path IS NOT NULL
  );
`

/**
 * Sync della libreria (Google Drive), versione desktop.
 *
 * Il desktop aggiunge qui `liked`/`liked_at` perché a quel punto non li aveva:
 * la build mobile li aveva già introdotti col suo blocco `discovery`. Le due
 * strade arrivano alle stesse colonne — è esattamente il tipo di divergenza per
 * cui le due storie non si possono fondere in una.
 */
export const SYNC_WITH_LIKED = `
  ALTER TABLE tracks ADD COLUMN liked INTEGER NOT NULL DEFAULT 0;
  ALTER TABLE tracks ADD COLUMN liked_at INTEGER;
  CREATE INDEX idx_tracks_liked ON tracks(liked, liked_at);
  ALTER TABLE tracks ADD COLUMN stats_updated_at INTEGER;

  CREATE TABLE sync_tombstones (
    kind TEXT NOT NULL,            -- 'track' | 'playlist'
    key TEXT NOT NULL,             -- trackKey / playlistKey
    deleted_at INTEGER NOT NULL,
    PRIMARY KEY (kind, key)
  );
`

/** Sync della libreria, versione mobile: `liked` esiste già dal blocco discovery. */
export const SYNC_STATS_AND_TOMBSTONES = `
  ALTER TABLE tracks ADD COLUMN stats_updated_at INTEGER;

  CREATE TABLE sync_tombstones (
    kind TEXT NOT NULL,            -- 'track' | 'playlist'
    key TEXT NOT NULL,             -- trackKey / playlistKey
    deleted_at INTEGER NOT NULL,
    PRIMARY KEY (kind, key)
  );
`

/** Strato scoperta (mobile): brani piaciuti più cronologia degli ascolti. */
export const DISCOVERY = `
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
`

/** Cronologia degli ascolti (desktop): `liked` era già arrivato col blocco sync. */
export const PLAY_HISTORY = `
  CREATE TABLE play_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id INTEGER NOT NULL,
    played_at INTEGER NOT NULL,
    ms_played INTEGER
  );
  CREATE INDEX idx_play_history_played_at ON play_history(played_at);
  CREATE INDEX idx_play_history_track ON play_history(track_id);
`

/** Recupero automatico delle tracce mancanti dopo un sync. */
export const LIBRARY_FETCH = `
  CREATE TABLE library_fetch (
    track_key     TEXT PRIMARY KEY,   -- trackKey della traccia mancante
    title         TEXT NOT NULL DEFAULT '',
    artist        TEXT NOT NULL DEFAULT '',
    album         TEXT NOT NULL DEFAULT '',
    duration      REAL NOT NULL DEFAULT 0,
    status        TEXT NOT NULL DEFAULT 'pending', -- pending|searching|queued|done|failed
    attempts      INTEGER NOT NULL DEFAULT 0,
    next_retry_at INTEGER,
    download_id   INTEGER,            -- downloads.id collegato, per i tag al termine
    error         TEXT,
    updated_at    INTEGER NOT NULL
  );
  CREATE INDEX idx_library_fetch_status ON library_fetch(status, next_retry_at);
`

/** Podcast: feed RSS pubblici, nessun account e nessun server. */
export const PODCASTS = `
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
`

/**
 * Migrazione Spotify persistente, versione desktop: la tabella nasce già con i
 * metadati completi dell'album.
 *
 * Sul mobile le stesse colonne arrivano in due passi, perché lì la tabella
 * esisteva da prima. Il motivo per cui i metadati dell'album si persistono per
 * traccia: una ripresa dopo che Android ha ucciso il processo deve riscrivere gli
 * STESSI tag, altrimenti l'album si ri-divide.
 */
export const SPOTIFY_MIGRATION_FULL = `
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
`

/** Migrazione Spotify, versione mobile: prima parte, senza i metadati album. */
export const SPOTIFY_MIGRATION_BASE = `
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
`

/** Migrazione Spotify, versione mobile: seconda parte, i metadati album. */
export const SPOTIFY_MIGRATION_ALBUM_COLUMNS = `
  ALTER TABLE spotify_migration_tracks ADD COLUMN album_artist TEXT;
  ALTER TABLE spotify_migration_tracks ADD COLUMN disc_number INTEGER;
  ALTER TABLE spotify_migration_tracks ADD COLUMN track_number INTEGER;
  ALTER TABLE spotify_migration_tracks ADD COLUMN year INTEGER;
`

/**
 * Riparazione dei brani del telefono via WiFi.
 *
 * È stato desktop (il telefono viene riparato DA un desktop e non scrive mai
 * questa tabella), creata anche sul mobile solo per tenere gli schemi identici.
 */
export const PHONE_REPAIR = `
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

/** La tabella cover_art senza i BLOB: solo l'indice leggero. */
export const COVER_ART_INDEX_ONLY = `
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
`

/** La tabella albums riscritta per essere indicizzata su album_key. */
export const ALBUMS_KEYED_ON_ALBUM_KEY = `
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
`
