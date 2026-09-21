-- Un brano che non è un file.
--
-- Fino a qui `tracks.path` era `NOT NULL`, e quella sola parola diceva una cosa
-- precisa: ogni riga della libreria è un file sul disco di chi ascolta. Il
-- lettore però non l'ha mai creduto — `aether_play::Sorgente` prende dei byte e
-- non un percorso, e `FlussoHttp` quei byte li porta dalla rete senza scriverli
-- da nessuna parte. Mancava solo il posto dove annotare che quella riga lì non
-- ha un file, e senza quel posto il ramo dello streaming era raggiungibile nel
-- codice e irraggiungibile da chi usa il programma.
--
-- Ora un brano è **o** un file **o** un riferimento a un catalogo, mai né l'uno
-- né l'altro e mai tutti e due. Non è una convenzione scritta in un commento: è
-- il vincolo `CHECK` in fondo alla tabella, e il database rifiuta le righe che
-- lo violano anche se a scriverle fosse del codice nuovo scritto fra due anni.
--
-- ── Perché la tabella si ricostruisce invece di crescere ────────────────────
-- Perché SQLite non sa togliere un `NOT NULL`: `ALTER TABLE` aggiunge colonne e
-- nient'altro. Ricostruire è la procedura che la documentazione di SQLite
-- prescrive, e ha una trappola che qui è disinnescata a monte: dieci tabelle
-- puntano a `tracks(id)` con `ON DELETE CASCADE`, e un `DROP TABLE` con le
-- chiavi esterne accese esegue una cancellazione implicita che quelle cascate
-- le fa scattare — cronologia d'ascolto, playlist e valutazioni sparirebbero
-- in silenzio, dentro una migrazione riuscita. Per questo la voce in
-- `migrations.rs` è marcata `ricostruisce: true`: il motore delle migrazioni
-- spegne `foreign_keys` intorno a questa e solo a questa, e prima di chiudere
-- la transazione verifica con `PRAGMA foreign_key_check` che non sia rimasto
-- niente a puntare nel vuoto.

-- ── la tabella, com'era più cinque colonne ──────────────────────────────────
-- L'ordine delle colonne esistenti è quello di prima, riga per riga: chi
-- confronta questa migrazione con la `001` deve vedere cosa cambia, non un
-- rimescolamento.
CREATE TABLE tracks_nuova (
  id            INTEGER PRIMARY KEY,

  -- Il percorso così com'è sul disco: serve ad aprire il file, quindi non è mai
  -- normalizzato. Il confronto fra percorsi passa da `path_key` nel dominio.
  --
  -- `NULL` per un brano di catalogo, e il `NOT NULL` che se n'è andato è tutta
  -- la migrazione. `UNIQUE` regge comunque: SQLite ammette quanti `NULL` vuole
  -- in una colonna unica, quindi due flussi non litigano fra loro — a tenerli
  -- distinti è `idx_tracks_riferimento`, più in basso.
  path          TEXT    UNIQUE,

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
  -- `NULL` per un flusso: un brano che non si scarica non pesa niente sul disco
  -- di nessuno. Uno zero al suo posto sarebbe un numero che prima o poi
  -- qualcuno somma, e la somma direbbe una libreria più piccola del vero.
  file_size     INTEGER,

  date_added    INTEGER NOT NULL,
  -- Data di modifica del file, in millisecondi GIÀ troncati all'intero: è la
  -- forma con cui la scansione confronta. Un valore con i decimali farebbe
  -- vedere «cambiato» ogni file a ogni passata.
  --
  -- `NULL` per un flusso, che non sta su nessun disco e quindi non ha una data
  -- di modifica da confrontare. Le query della scansione filtrano
  -- `path IS NOT NULL` e non lo incontrano mai.
  date_modified INTEGER,

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

  -- Come è entrato in libreria: 'scan', 'catalogo', 'spotify', 'transfer'.
  source        TEXT    NOT NULL DEFAULT 'scan',

  -- ── quel che hanno aggiunto le migrazioni dalla 16 alla 23 ──
  isrc                TEXT,
  enrich_source       TEXT,
  enrich_confidence   REAL,
  rating_at           INTEGER,
  meta_salute         TEXT NOT NULL DEFAULT 'ok',
  meta_origine        TEXT,
  meta_problemi       TEXT,
  content_key         TEXT,

  -- ── il brano che non è un file ──────────────────────────────────────────
  -- Gli stessi nomi che `desiderati` ha dalla migrazione 10, e gli stessi
  -- valori: una riga che passa dalla coda alla libreria non deve essere
  -- tradotta, e due vocabolari per la stessa cosa sono due vocabolari che un
  -- giorno diranno `soloAscolto` e `solo-ascolto`.

  -- Da quale catalogo, col nome stabile di `aether_domain::esterno::Fonte`:
  -- `internet-archive`, `audius`, `jamendo`. `NULL` vuol dire «è un file», ed è
  -- la domanda che ogni funzione che apre un percorso deve farsi per prima.
  source_service      TEXT,

  -- L'indirizzo da cui escono i byte. Per l'Internet Archive un `https://…`
  -- intero; per Audius il percorso `/v1/tracks/<id>/stream`, senza nodo davanti,
  -- perché il nodo di oggi fra un mese non esiste più e a rimetterlo è
  -- `Audius::prepara` al momento di suonare.
  fonte_url           TEXT,

  -- La pagina pubblica del brano, da mostrare accanto a chi lo ascolta. Non è
  -- un di più estetico: per certe Creative Commons e per i termini di Audius il
  -- rimando visibile è un obbligo, e una colonna è l'unico posto in cui quel
  -- rimando sopravvive a un riavvio.
  fonte_pagina        TEXT,

  -- Il nome stabile di `Licenza`. Vale per la stessa ragione per cui vale su
  -- `desiderati`: «da dove viene e cosa ci posso fare» è una domanda che senza
  -- questa colonna resta senza risposta il giorno dopo.
  licenza             TEXT,

  -- `scaricabile`, `soloAscolto`, `soloAcquisto`. Quel che sta in libreria come
  -- flusso è `soloAscolto` per costruzione — ciò che si può tenere si tiene, e
  -- diventa un file — ma la colonna registra quel che la fonte ha dichiarato,
  -- non quel che abbiamo dedotto.
  disponibilita       TEXT,

  -- ── o un file, o un riferimento ────────────────────────────────────────
  -- Il vincolo che rende inutile ricordarsene. Senza, la prima riga scritta
  -- male — un flusso a cui manca la licenza, un file a cui qualcuno appiccica
  -- un `fonte_url` — passerebbe, e si manifesterebbe mesi dopo come un brano
  -- che non parte. `(x IS NOT NULL)` vale 0 o 1 e mai `NULL`, quindi il
  -- confronto è totale: non c'è una terza via in cui il `CHECK` non si esprime.
  CHECK (
    (path IS NOT NULL
       AND source_service IS NULL AND fonte_url IS NULL
       AND licenza IS NULL AND disponibilita IS NULL)
    OR
    (path IS NULL
       AND source_service IS NOT NULL AND fonte_url IS NOT NULL
       AND licenza IS NOT NULL AND disponibilita IS NOT NULL
       AND file_size IS NULL AND date_modified IS NULL)
  )
);

-- ── il travaso ──────────────────────────────────────────────────────────────
-- Colonne nominate una per una e non `SELECT *`: se un domani questa migrazione
-- venisse riletta accanto a una tabella cambiata, un elenco esplicito fallisce
-- rumorosamente invece di spostare i dati nella colonna sbagliata.
INSERT INTO tracks_nuova (
  id, path, track_key, title, artist, album, album_artist, album_key,
  year, track_number, disc_number, genre, duration_ms, bpm, musical_key,
  comment, lyrics, cover_art_hash, bitrate, sample_rate, channels, codec,
  file_size, date_added, date_modified, replaygain_track_db, replaygain_album_db,
  play_count, last_played_at, rating, liked, liked_at, stats_updated_at,
  mb_recording_id, mb_release_group_id, mb_release_id, acoustid_fingerprint,
  spotify_album_id, enrich_status, enrich_attempted_at, source,
  isrc, enrich_source, enrich_confidence, rating_at,
  meta_salute, meta_origine, meta_problemi, content_key
)
SELECT
  id, path, track_key, title, artist, album, album_artist, album_key,
  year, track_number, disc_number, genre, duration_ms, bpm, musical_key,
  comment, lyrics, cover_art_hash, bitrate, sample_rate, channels, codec,
  file_size, date_added, date_modified, replaygain_track_db, replaygain_album_db,
  play_count, last_played_at, rating, liked, liked_at, stats_updated_at,
  mb_recording_id, mb_release_group_id, mb_release_id, acoustid_fingerprint,
  spotify_album_id, enrich_status, enrich_attempted_at, source,
  isrc, enrich_source, enrich_confidence, rating_at,
  meta_salute, meta_origine, meta_problemi, content_key
FROM tracks;

DROP TABLE tracks;
ALTER TABLE tracks_nuova RENAME TO tracks;

-- ── gli indici, gli stessi di prima ─────────────────────────────────────────
-- Se ne sono andati col `DROP`, e vanno rimessi tutti: uno dimenticato non
-- rompe niente, rende lenta una schermata e basta — che è il modo peggiore di
-- rompersi, perché nessuno se ne accorge finché la libreria è piccola.
CREATE INDEX idx_tracks_track_key ON tracks(track_key);
CREATE INDEX idx_tracks_artist    ON tracks(artist);
CREATE INDEX idx_tracks_album_key ON tracks(album_key);
CREATE INDEX idx_tracks_genre     ON tracks(genre);
CREATE INDEX idx_tracks_liked     ON tracks(liked, liked_at);
CREATE INDEX idx_tracks_added     ON tracks(date_added);
CREATE INDEX idx_tracks_arricchimento ON tracks(enrich_status, enrich_attempted_at);
CREATE INDEX idx_tracks_isrc ON tracks(isrc) WHERE isrc IS NOT NULL;
CREATE INDEX idx_tracks_content_key ON tracks(content_key);
CREATE INDEX idx_tracks_meta_salute ON tracks(meta_salute) WHERE meta_salute <> 'ok';
CREATE INDEX idx_tracks_last_played
    ON tracks(last_played_at DESC)
    WHERE last_played_at IS NOT NULL;
CREATE INDEX idx_tracks_album_added
    ON tracks(album_key, date_added)
    WHERE album_key IS NOT NULL;
CREATE INDEX idx_tracks_piu_ascoltati ON tracks(
  play_count DESC,
  last_played_at DESC,
  title COLLATE NOCASE
);
CREATE INDEX idx_tracks_scaffale ON tracks(
  artist COLLATE NOCASE,
  album COLLATE NOCASE,
  disc_number,
  track_number,
  title COLLATE NOCASE
);
CREATE INDEX idx_tracks_titolo ON tracks(
  title COLLATE NOCASE,
  artist COLLATE NOCASE
);

-- ── e uno nuovo: due volte lo stesso flusso non si aggiunge ─────────────────
-- Quel che `UNIQUE` su `path` fa per i file, questo lo fa per i riferimenti.
-- La coppia e non il solo indirizzo, perché l'indirizzo di Audius è un percorso
-- relativo e in teoria due cataloghi potrebbero coniarne uno uguale.
CREATE UNIQUE INDEX idx_tracks_riferimento
    ON tracks(source_service, fonte_url)
    WHERE fonte_url IS NOT NULL;

-- ── i trigger, gli stessi di prima ──────────────────────────────────────────
-- Questi invece rompono forte: senza i tre di `tracks_fts` un brano esiste e
-- non si trova cercandolo, e senza quelli delle correzioni le correzioni fatte
-- a mano si perdono quando un file sparisce.
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

CREATE TRIGGER correzioni_restano_orfane
BEFORE DELETE ON tracks
WHEN OLD.content_key IS NOT NULL
  AND EXISTS (SELECT 1 FROM track_overrides WHERE track_id = OLD.id)
BEGIN
  DELETE FROM correzioni_orfane
   WHERE orfana_at < (CAST(strftime('%s', 'now') AS INTEGER) - 90 * 86400) * 1000;

  INSERT INTO correzioni_orfane (content_key, campi, set_at, orfana_at)
  SELECT OLD.content_key, campi, set_at, CAST(strftime('%s', 'now') AS INTEGER) * 1000
    FROM track_overrides
   WHERE track_id = OLD.id
  ON CONFLICT(content_key) DO UPDATE SET
    campi = excluded.campi,
    set_at = excluded.set_at,
    orfana_at = excluded.orfana_at
  WHERE excluded.set_at >= correzioni_orfane.set_at;
END;

CREATE TRIGGER correzioni_tornano
AFTER INSERT ON tracks
WHEN NEW.content_key IS NOT NULL
  AND EXISTS (SELECT 1 FROM correzioni_orfane WHERE content_key = NEW.content_key)
BEGIN
  INSERT OR IGNORE INTO track_overrides (track_id, campi, set_at)
  SELECT NEW.id, campi, set_at
    FROM correzioni_orfane
   WHERE content_key = NEW.content_key;

  DELETE FROM correzioni_orfane WHERE content_key = NEW.content_key;
END;

CREATE TRIGGER lyrics_segue_track_key
AFTER UPDATE OF track_key ON tracks
WHEN OLD.track_key <> NEW.track_key
  AND NOT EXISTS (SELECT 1 FROM tracks WHERE track_key = OLD.track_key)
BEGIN
  DELETE FROM lyrics
   WHERE track_key = NEW.track_key
     AND plain IS NULL AND synced IS NULL
     AND instrumental = 0 AND offset_ms = 0 AND source <> 'mano'
     AND EXISTS (
       SELECT 1 FROM lyrics
        WHERE track_key = OLD.track_key
          AND (plain IS NOT NULL OR synced IS NOT NULL OR instrumental <> 0
               OR offset_ms <> 0 OR source = 'mano')
     );

  UPDATE lyrics
     SET track_key = NEW.track_key
   WHERE track_key = OLD.track_key
     AND (plain IS NOT NULL OR synced IS NOT NULL OR instrumental <> 0
          OR offset_ms <> 0 OR source = 'mano')
     AND NOT EXISTS (SELECT 1 FROM lyrics WHERE track_key = NEW.track_key);
END;
