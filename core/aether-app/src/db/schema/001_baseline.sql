-- Lo schema di partenza.
--
-- Non è la baseline del vecchio albero: quella era il risultato di diciotto
-- passi accumulati in mesi, con colonne aggiunte accanto ad altre che facevano
-- quasi la stessa cosa. Qui si riparte, perché la libreria si ricostruisce da
-- una scansione del disco — i file sono il dato, il database è un indice.
--
-- Quel che NON si ricostruisce da una scansione (cronologia di ascolto,
-- preferiti, valutazioni, playlist) arriva dal vecchio database con
-- l'importatore una tantum.

-- ── brani ───────────────────────────────────────────────────────────────────
CREATE TABLE tracks (
  id            INTEGER PRIMARY KEY,

  -- Il percorso così com'è sul disco: serve ad aprire il file, quindi non è mai
  -- normalizzato. Il confronto fra percorsi passa da `path_key` nel dominio.
  path          TEXT    NOT NULL UNIQUE,

  -- L'identità fra dispositivi, materializzata invece che ricalcolata a ogni
  -- sincronizzazione. Si ricalcola a ogni scrittura dei tag: è derivata da
  -- artist/title/album e deve restare in passo con loro.
  -- Non è UNIQUE: due file possono legittimamente essere lo stesso brano
  -- (formati diversi, doppioni non ancora deduplicati).
  track_key     TEXT    NOT NULL,

  title         TEXT    NOT NULL,
  artist        TEXT    NOT NULL,
  album         TEXT    NOT NULL,
  album_artist  TEXT,
  -- Raggruppa le edizioni dello stesso album: cartella + titolo normalizzati,
  -- poi fuse quando condividono un identificativo autorevole.
  album_key     TEXT,

  year          INTEGER,
  track_number  INTEGER,
  disc_number   INTEGER,
  genre         TEXT,

  -- Millisecondi interi, non secondi in virgola mobile: la durata finisce in
  -- confronti con tolleranza, e un float rende quei confronti dipendenti dal
  -- percorso di arrotondamento invece che dai dati.
  duration_ms   INTEGER NOT NULL DEFAULT 0,

  bpm           REAL,
  musical_key   TEXT,
  comment       TEXT,
  lyrics        TEXT,
  cover_art_hash TEXT REFERENCES cover_art(hash) ON DELETE SET NULL,

  bitrate       INTEGER,
  sample_rate   INTEGER,
  channels      INTEGER,
  codec         TEXT,
  file_size     INTEGER NOT NULL,

  date_added    INTEGER NOT NULL,
  -- Data di modifica del file, in millisecondi GIÀ troncati all'intero: è la
  -- forma con cui la scansione confronta. Un valore con i decimali farebbe
  -- vedere «cambiato» ogni file a ogni passata.
  date_modified INTEGER NOT NULL,

  replaygain_track_db REAL,
  replaygain_album_db REAL,

  -- ── statistiche d'ascolto: è ciò che la sincronizzazione fonde ──
  play_count    INTEGER NOT NULL DEFAULT 0,
  last_played_at INTEGER,
  rating        INTEGER NOT NULL DEFAULT 0 CHECK (rating BETWEEN 0 AND 5),
  liked         INTEGER NOT NULL DEFAULT 0 CHECK (liked IN (0, 1)),
  liked_at      INTEGER,
  -- L'orologio unico di valutazione e preferito. Fondere due dispositivi
  -- richiede sapere QUANDO è stata presa la decisione, non solo quale sia:
  -- senza, l'ultimo che sincronizza vince sempre, anche se aveva il dato vecchio.
  stats_updated_at INTEGER NOT NULL DEFAULT 0,

  -- ── identificativi esterni ──
  mb_recording_id      TEXT,
  mb_release_group_id  TEXT,
  mb_release_id        TEXT,
  acoustid_fingerprint TEXT,
  spotify_album_id     TEXT,

  -- ── arricchimento ──
  enrich_status       TEXT,
  enrich_attempted_at INTEGER,

  -- Come è entrato in libreria: 'scan', 'youtube', 'spotify', 'transfer'.
  source        TEXT    NOT NULL DEFAULT 'scan'
);

CREATE INDEX idx_tracks_track_key ON tracks(track_key);
CREATE INDEX idx_tracks_artist    ON tracks(artist);
CREATE INDEX idx_tracks_album_key ON tracks(album_key);
CREATE INDEX idx_tracks_genre     ON tracks(genre);
CREATE INDEX idx_tracks_liked     ON tracks(liked, liked_at);
CREATE INDEX idx_tracks_added     ON tracks(date_added);

-- ── copertine, indirizzate dal contenuto ────────────────────────────────────
-- L'impronta è il nome: la stessa copertina condivisa da dodici brani sta su
-- disco una volta sola, e cambiare i tag di un brano non ne orfana la copia.
CREATE TABLE cover_art (
  hash       TEXT    PRIMARY KEY,
  mime_type  TEXT    NOT NULL,
  width      INTEGER,
  height     INTEGER,
  byte_size  INTEGER,
  -- Da dove viene: 'tag', 'download', 'musicbrainz', 'deezer', 'manual'.
  -- Serve a decidere se una copertina trovata online debba sostituirne una
  -- già presente, o lasciarla stare.
  source     TEXT    NOT NULL,
  created_at INTEGER NOT NULL
);

-- ── aggregati, ricostruibili dai brani ──────────────────────────────────────
-- Chiave naturale invece di un id progressivo: l'identità di un album È la sua
-- chiave di raggruppamento, e un id in più sarebbe solo un'altra cosa da tenere
-- allineata quando gli album si fondono.
CREATE TABLE albums (
  album_key      TEXT PRIMARY KEY,
  title          TEXT NOT NULL,
  artist         TEXT NOT NULL,
  year           INTEGER,
  genre          TEXT,
  total_tracks   INTEGER NOT NULL DEFAULT 0,
  cover_art_hash TEXT REFERENCES cover_art(hash) ON DELETE SET NULL,
  mb_release_group_id TEXT,
  spotify_id     TEXT
);

CREATE TABLE artists (
  name         TEXT PRIMARY KEY,
  bio          TEXT,
  image_hash   TEXT REFERENCES cover_art(hash) ON DELETE SET NULL,
  mb_artist_id TEXT,
  spotify_id   TEXT
);

-- ── playlist ────────────────────────────────────────────────────────────────
CREATE TABLE playlists (
  id           INTEGER PRIMARY KEY,
  -- Il nome normalizzato: l'identità che attraversa la sincronizzazione.
  playlist_key TEXT    NOT NULL UNIQUE,
  name         TEXT    NOT NULL,
  description  TEXT,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL,
  cover_art_hash TEXT REFERENCES cover_art(hash) ON DELETE SET NULL,
  -- Le playlist automatiche non hanno appartenenza salvata: ogni dispositivo la
  -- ricalcola dalle regole, così restano vere anche sui brani che l'altro
  -- dispositivo non ha.
  is_smart     INTEGER NOT NULL DEFAULT 0 CHECK (is_smart IN (0, 1)),
  rules        TEXT
);

CREATE TABLE playlist_tracks (
  playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
  track_id    INTEGER NOT NULL REFERENCES tracks(id)    ON DELETE CASCADE,
  position    INTEGER NOT NULL,
  -- La posizione è parte della chiave: due brani non possono occupare lo stesso
  -- posto. Lo stesso brano può invece comparire più volte in una playlist, ed è
  -- voluto.
  PRIMARY KEY (playlist_id, position)
);

CREATE INDEX idx_playlist_tracks_track ON playlist_tracks(track_id);

-- ── cronologia d'ascolto ────────────────────────────────────────────────────
-- Righe, non un contatore: `play_count` dice quante volte, questa tabella dice
-- quando e per quanto. Serve alle statistiche e allo smart shuffle, che deve
-- sapere cosa è stato ascoltato di recente per non riproporlo.
CREATE TABLE play_history (
  id        INTEGER PRIMARY KEY,
  track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  played_at INTEGER NOT NULL,
  ms_played INTEGER NOT NULL
);

CREATE INDEX idx_play_history_played_at ON play_history(played_at);
CREATE INDEX idx_play_history_track     ON play_history(track_id);

-- ── lapidi della sincronizzazione ───────────────────────────────────────────
-- Una cancellazione deve poter viaggiare. Senza, l'altro dispositivo vedrebbe
-- solo «a me manca un brano» e lo ri-scaricherebbe: il brano tolto tornerebbe
-- da solo, che è il modo più sicuro di far perdere fiducia in una sincronia.
CREATE TABLE sync_tombstones (
  kind       TEXT    NOT NULL CHECK (kind IN ('track', 'playlist')),
  key        TEXT    NOT NULL,
  deleted_at INTEGER NOT NULL,
  PRIMARY KEY (kind, key)
);

-- ── impostazioni ────────────────────────────────────────────────────────────
-- Nel database e non in un JSON accanto: così una scrittura di impostazioni
-- partecipa alle stesse transazioni dei dati che descrive, e un backup del
-- database è un backup completo. I SEGRETI non stanno qui — vanno nel
-- portachiavi di sistema.
CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- ── ricerca ─────────────────────────────────────────────────────────────────
-- FTS5 su entrambe le piattaforme, perché SQLite è compilato dentro il binario.
-- Il vecchio porting Android girava su sql.js, che FTS5 non ce l'ha, e doveva
-- ripiegare su `LIKE` più una funzione di piegatura scritta a mano: due strade
-- diverse per la stessa ricerca, cioè due comportamenti da tenere allineati.
--
-- `remove_diacritics 2` fa la piegatura dentro il tokenizzatore: cercare
-- «Bjork» trova «Björk» senza che nessuno debba normalizzare a mano.
CREATE VIRTUAL TABLE tracks_fts USING fts5(
  title,
  artist,
  album,
  album_artist,
  genre,
  content = 'tracks',
  content_rowid = 'id',
  tokenize = "unicode61 remove_diacritics 2"
);

-- L'indice segue la tabella: con `content='tracks'` i dati non sono duplicati,
-- ma gli aggiornamenti vanno propagati a mano. Un indice che si scorda una
-- riga produce brani che esistono e non si trovano cercandoli.
CREATE TRIGGER tracks_fts_insert AFTER INSERT ON tracks BEGIN
  INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, genre)
  VALUES (new.id, new.title, new.artist, new.album, new.album_artist, new.genre);
END;

CREATE TRIGGER tracks_fts_delete AFTER DELETE ON tracks BEGIN
  INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist, genre)
  VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist, old.genre);
END;

CREATE TRIGGER tracks_fts_update AFTER UPDATE ON tracks BEGIN
  INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist, genre)
  VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist, old.genre);
  INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, genre)
  VALUES (new.id, new.title, new.artist, new.album, new.album_artist, new.genre);
END;
