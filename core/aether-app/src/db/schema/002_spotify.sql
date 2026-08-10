-- L'importazione da Spotify: l'ISRC sui brani, e i brani che ancora non ci sono.

-- ── l'ISRC ──────────────────────────────────────────────────────────────────
-- Nella baseline c'erano già `mb_recording_id`, `acoustid_fingerprint` e
-- `spotify_album_id`, ma non questo, che è l'unico identificativo davvero
-- portabile fra servizi: lo stesso codice a dodici caratteri nomina la stessa
-- registrazione su Spotify, su MusicBrainz e nei tag di un file ben taggato.
--
-- Arriva da Pathfinder, che lo espone ancora. La Web API pubblica no: a febbraio
-- 2026 `external_ids` è stato rimosso dall'oggetto brano, il che rende questa
-- colonna riempibile solo dal percorso keyless — e un'ottima ragione perché quel
-- percorso esista.
ALTER TABLE tracks ADD COLUMN isrc TEXT;

-- Parziale: la stragrande maggioranza delle righe ha `isrc` a NULL finché non
-- passa un'importazione, e indicizzare centinaia di NULL è spazio speso per
-- niente.
CREATE INDEX idx_tracks_isrc ON tracks(isrc) WHERE isrc IS NOT NULL;

-- ── i brani desiderati ──────────────────────────────────────────────────────
-- Cosa c'era nella playlist di Spotify e non c'è sul disco.
--
-- # Perché una tabella a sé e non righe in `tracks`
--
-- Perché `tracks.path` è `NOT NULL UNIQUE` e la riconciliazione della scansione
-- toglie le righe il cui file non esiste più. Un brano «desiderato» inserito lì
-- con un percorso finto sopravvivrebbe fino alla prima scansione e poi
-- sparirebbe da solo, portandosi dietro la voce di playlist che lo nominava. Il
-- guasto arriverebbe giorni dopo l'importazione, senza un gesto a cui
-- ricondurlo: il modo peggiore in cui un difetto può presentarsi.
--
-- Separati, invece, non partecipano a niente di tutto ciò: non compaiono nei
-- conteggi della libreria, non finiscono nelle playlist, non si riproducono.
-- Sono un elenco di cose da cercare, ed è esattamente quel che sono.
CREATE TABLE spotify_wanted (
  id            INTEGER PRIMARY KEY,

  -- La stessa chiave d'identità dei brani veri: il giorno in cui il file entra
  -- in libreria, ritrovare la riga corrispondente è un confronto di stringhe.
  track_key     TEXT    NOT NULL,

  title         TEXT    NOT NULL,
  artist        TEXT,
  album         TEXT,
  album_artist  TEXT,
  duration_ms   INTEGER,
  isrc          TEXT,

  spotify_track_id TEXT,
  spotify_album_id TEXT,
  cover_url        TEXT,

  -- Da dove veniva: genere, identificativo e nome del contenitore su Spotify.
  -- Servono a dire all'utente «mancano da questa playlist» invece di presentare
  -- un elenco di brani senza provenienza.
  source_kind   TEXT    NOT NULL,
  source_id     TEXT    NOT NULL,
  source_title  TEXT    NOT NULL,

  -- La playlist di Aether in cui sarebbe andato, e in che posizione. Se la
  -- playlist viene cancellata resta il desiderio, senza il posto: il brano
  -- manca ancora, ed è ancora un'informazione.
  playlist_id   INTEGER REFERENCES playlists(id) ON DELETE SET NULL,
  position      INTEGER,

  added_at      INTEGER NOT NULL
);

-- Reimportare la stessa playlist non accumula doppioni: la coppia
-- «quale brano, da quale contenitore» è l'identità della riga.
CREATE UNIQUE INDEX idx_spotify_wanted_identita
  ON spotify_wanted(track_key, source_id);

CREATE INDEX idx_spotify_wanted_playlist ON spotify_wanted(playlist_id);
