-- Gli ascolti, il voto, il preferito e il posto nelle playlist sopravvivono a
-- un file che sparisce e ritorna.
--
-- # Il guasto
--
-- `play_history` e `playlist_tracks` sono agganciate a `tracks(id)` con
-- `ON DELETE CASCADE`, e il voto, il preferito e il conteggio d'ascolto stanno
-- sulla riga stessa. Quando una riga se ne va, se ne va tutto: è giusto per un
-- brano buttato davvero, ed è una perdita secca per tutti gli altri modi in cui
-- una riga sparisce senza che il **file** sia sparito — la sottocartella che
-- per una passata non si lasciava leggere, la share che rispondeva a metà, il
-- disco esterno lasciato nel cassetto, la cartella tolta dalle sorvegliate e
-- rimessa il giorno dopo, «Togli dalla libreria» su un file che sta ancora
-- sotto una cartella sorvegliata. Il file torna, la scansione lo inserisce come
-- nuovo — con un `id` nuovo — e la sua storia non c'è più. Sono le sole cose in
-- tutta la libreria che una riscansione non sa ricostruire.
--
-- # Come
--
-- Lo stesso meccanismo delle correzioni a mano (`023_correzioni_orfane.sql`),
-- e per la stessa ragione: prima che una riga di `tracks` venga cancellata,
-- quel che l'utente ci ha costruito sopra si mette da parte sotto la
-- `content_key` del brano — la chiave calcolata dai tag **grezzi** del file,
-- che è la stessa quando lo stesso file ritorna, e che nessuna correzione
-- tocca (vedi `019_identita_e_metadati.sql`). Quando una riga nuova entra con
-- quella chiave, tutto torna al suo posto, nello stesso trigger.
--
-- Tre tabelle e non una, perché sono tre forme: i numeri della riga (una per
-- chiave), gli ascolti (molti per chiave) e l'appartenenza alle playlist (una
-- per ogni posto occupato). Le due di dettaglio pendono dalla prima con
-- `ON DELETE CASCADE`: scadere, o tornare, è un `DELETE` solo.
--
-- # Cosa si mette da parte
--
-- - `play_count`, `last_played_at`: quante volte e quando.
-- - `rating`, `rating_at`, `liked`, `liked_at`, `stats_updated_at`: il voto e
--   il preferito, con i loro orologi — la sincronia li fonde per data, e una
--   data persa per strada farebbe perdere al brano ritornato la fusione con
--   l'altro dispositivo.
-- - Ogni riga di `play_history`: la cronologia e le statistiche sono fatte di
--   quelle, non del conteggio.
-- - Ogni riga di `playlist_tracks`, **con la posizione**: il brano ritorna nel
--   buco che ha lasciato. Le playlist tollerano i buchi — l'ordine è quello di
--   `position`, e la finestra ragiona per indici dell'elenco letto — ed è la
--   cascata stessa a lasciarli. Se nel frattempo il posto è stato preso (un
--   riordino riscrive le posizioni da zero), il brano va in coda: meglio in
--   fondo che fuori, e meglio fuori che sopra un altro.
--
-- # Cosa non serve mettere da parte
--
-- La posizione d'ascolto a cui riprendere un brano lungo sta in
-- `sync_posizioni`, e i contributi d'ascolto per dispositivo in
-- `sync_ascolti`: tutte e due sono indicizzate per `track_key`, non per `id`,
-- e una riga cancellata non se le porta via. Tornano da sole col brano.
--
-- Le correzioni a mano hanno già la loro migrazione, la 023. L'arricchimento,
-- le impronte e le vicinanze no, e non devono: si ricalcolano, ed è il loro
-- mestiere.
--
-- # Solo i file
--
-- Un brano di catalogo non ha un percorso, e nessuna scansione lo farà mai
-- tornare: mettere da parte i suoi dati vorrebbe dire tenerli novanta giorni
-- per nessuno. E solo le righe che hanno qualcosa da perdere — un ascolto, un
-- voto, un cuore, una playlist: le altre non lascerebbero niente, e scrivere
-- una riga vuota per ognuna vorrebbe dire raddoppiare le scritture di una
-- scansione che toglie una cartella.
--
-- # Due copie dello stesso file
--
-- Due righe con la stessa `content_key` che se ne vanno — lo stesso album in
-- due cartelle, tolte tutte e due — si fondono in una: il conteggio più alto,
-- l'ascolto più recente, il voto e il preferito col loro orologio più recente,
-- e tutti gli ascolti e tutti i posti in playlist di tutte e due.
--
-- # Per quanto
--
-- Novanta giorni, come le correzioni: abbastanza per un disco esterno lasciato
-- in un cassetto per l'estate, non tanto da tenere per sempre la storia di una
-- libreria che si è davvero buttata via. Si pulisce in tutti e due i trigger —
-- quando qualcosa se ne va e quando qualcosa torna — così che una voce scaduta
-- non torni mai, anche se nel frattempo non si è cancellato niente.
--
-- # Se un giorno si ricostruisce `tracks`
--
-- Come ha fatto la 026: `DROP TABLE tracks` porta via i trigger, e questi due
-- vanno ricreati accanto a quelli delle correzioni. La 026 lo scrive in testa
-- ai suoi; questa migrazione è il motivo per cui l'elenco è più lungo.
--
-- # Tornare indietro
--
-- Una versione precedente rifiuta un database a questa versione, e serve il
-- backup di prima dell'aggiornamento — o la copia automatica che l'apertura
-- scrive prima di migrare. Il costo di quel backup è la storia messa da parte
-- dopo: le righe tolte fra l'aggiornamento e il ritorno indietro perdono di
-- nuovo tutto, come prima di questa migrazione. Togliere a mano le tre tabelle
-- e i due trigger, invece, non rompe niente: è la 027 esatta, con in meno
-- quel che era stato messo da parte.

CREATE TABLE brani_orfani (
  content_key      TEXT    PRIMARY KEY,
  play_count       INTEGER NOT NULL DEFAULT 0,
  last_played_at   INTEGER,
  rating           INTEGER NOT NULL DEFAULT 0,
  rating_at        INTEGER,
  liked            INTEGER NOT NULL DEFAULT 0,
  liked_at         INTEGER,
  stats_updated_at INTEGER NOT NULL DEFAULT 0,
  -- Quando la riga è stata tolta: è l'orologio della scadenza, e si rinnova se
  -- un'altra copia dello stesso file se ne va dopo.
  orfano_at        INTEGER NOT NULL
);

CREATE TABLE ascolti_orfani (
  content_key TEXT    NOT NULL REFERENCES brani_orfani(content_key) ON DELETE CASCADE,
  played_at   INTEGER NOT NULL,
  ms_played   INTEGER NOT NULL,
  -- Da dove viene l'ascolto (migrazione 5): un ascolto importato da Spotify che
  -- tornasse come 'local' non si lascerebbe più disfare con l'importazione.
  source      TEXT    NOT NULL DEFAULT 'local',
  -- La stessa unicità di `idx_play_history_brano_quando` (migrazione 20): un
  -- ascolto è un brano in un istante, e due copie fuse non lo raddoppiano.
  PRIMARY KEY (content_key, played_at)
) WITHOUT ROWID;

CREATE TABLE playlist_orfane (
  content_key TEXT    NOT NULL REFERENCES brani_orfani(content_key) ON DELETE CASCADE,
  -- Una playlist cancellata nel frattempo si porta via anche il posto che
  -- teneva per chi era partito.
  playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
  position    INTEGER NOT NULL,
  PRIMARY KEY (content_key, playlist_id, position)
) WITHOUT ROWID;

-- Per la cascata dal lato `playlists`: senza, cancellare una playlist
-- scorrerebbe l'intera tabella.
CREATE INDEX idx_playlist_orfane_playlist ON playlist_orfane(playlist_id);

CREATE TRIGGER statistiche_restano_orfane
BEFORE DELETE ON tracks
WHEN OLD.path IS NOT NULL
  AND OLD.content_key IS NOT NULL
  AND (OLD.play_count > 0
       OR OLD.last_played_at IS NOT NULL
       OR OLD.rating > 0
       OR OLD.liked = 1
       OR EXISTS (SELECT 1 FROM play_history WHERE track_id = OLD.id)
       OR EXISTS (SELECT 1 FROM playlist_tracks WHERE track_id = OLD.id))
BEGIN
  DELETE FROM brani_orfani
   WHERE orfano_at < (CAST(strftime('%s', 'now') AS INTEGER) - 90 * 86400) * 1000;

  INSERT INTO brani_orfani (content_key, play_count, last_played_at, rating, rating_at,
                            liked, liked_at, stats_updated_at, orfano_at)
  VALUES (OLD.content_key, OLD.play_count, OLD.last_played_at, OLD.rating, OLD.rating_at,
          OLD.liked, OLD.liked_at, OLD.stats_updated_at,
          CAST(strftime('%s', 'now') AS INTEGER) * 1000)
  ON CONFLICT(content_key) DO UPDATE SET
    play_count = max(brani_orfani.play_count, excluded.play_count),
    last_played_at = NULLIF(max(COALESCE(brani_orfani.last_played_at, 0),
                                COALESCE(excluded.last_played_at, 0)), 0),
    rating = CASE WHEN excluded.stats_updated_at >= brani_orfani.stats_updated_at
                  THEN excluded.rating ELSE brani_orfani.rating END,
    rating_at = CASE WHEN excluded.stats_updated_at >= brani_orfani.stats_updated_at
                     THEN excluded.rating_at ELSE brani_orfani.rating_at END,
    liked = CASE WHEN excluded.stats_updated_at >= brani_orfani.stats_updated_at
                 THEN excluded.liked ELSE brani_orfani.liked END,
    liked_at = CASE WHEN excluded.stats_updated_at >= brani_orfani.stats_updated_at
                    THEN excluded.liked_at ELSE brani_orfani.liked_at END,
    stats_updated_at = max(brani_orfani.stats_updated_at, excluded.stats_updated_at),
    orfano_at = excluded.orfano_at;

  INSERT OR IGNORE INTO ascolti_orfani (content_key, played_at, ms_played, source)
  SELECT OLD.content_key, played_at, ms_played, source
    FROM play_history
   WHERE track_id = OLD.id;

  INSERT OR IGNORE INTO playlist_orfane (content_key, playlist_id, position)
  SELECT OLD.content_key, playlist_id, position
    FROM playlist_tracks
   WHERE track_id = OLD.id;
END;

CREATE TRIGGER statistiche_tornano
AFTER INSERT ON tracks
WHEN NEW.path IS NOT NULL
  AND NEW.content_key IS NOT NULL
  AND EXISTS (SELECT 1 FROM brani_orfani WHERE content_key = NEW.content_key)
BEGIN
  -- Prima la pulizia: una voce scaduta non torna, anche se fra la sua uscita e
  -- questo ritorno non si è cancellato niente che la facesse scadere.
  DELETE FROM brani_orfani
   WHERE orfano_at < (CAST(strftime('%s', 'now') AS INTEGER) - 90 * 86400) * 1000;

  UPDATE tracks
     SET play_count       = (SELECT play_count FROM brani_orfani
                              WHERE content_key = NEW.content_key),
         last_played_at   = (SELECT last_played_at FROM brani_orfani
                              WHERE content_key = NEW.content_key),
         rating           = (SELECT rating FROM brani_orfani
                              WHERE content_key = NEW.content_key),
         rating_at        = (SELECT rating_at FROM brani_orfani
                              WHERE content_key = NEW.content_key),
         liked            = (SELECT liked FROM brani_orfani
                              WHERE content_key = NEW.content_key),
         liked_at         = (SELECT liked_at FROM brani_orfani
                              WHERE content_key = NEW.content_key),
         stats_updated_at = (SELECT stats_updated_at FROM brani_orfani
                              WHERE content_key = NEW.content_key)
   WHERE id = NEW.id
     AND EXISTS (SELECT 1 FROM brani_orfani WHERE content_key = NEW.content_key);

  INSERT OR IGNORE INTO play_history (track_id, played_at, ms_played, source)
  SELECT NEW.id, played_at, ms_played, source
    FROM ascolti_orfani
   WHERE content_key = NEW.content_key;

  -- Nel buco che ha lasciato, dove il buco c'è ancora…
  INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position)
  SELECT playlist_id, NEW.id, position
    FROM playlist_orfane
   WHERE content_key = NEW.content_key;

  -- …e in coda dove il posto è stato preso, uno dopo l'altro nell'ordine che
  -- avevano. Il massimo si legge prima di scrivere — SQLite materializza una
  -- `SELECT` che legge la tabella in cui si inserisce — e `ROW_NUMBER` tiene
  -- distinti due posti della stessa playlist.
  INSERT INTO playlist_tracks (playlist_id, track_id, position)
  SELECT o.playlist_id, NEW.id,
         (SELECT COALESCE(MAX(p.position), -1) FROM playlist_tracks p
           WHERE p.playlist_id = o.playlist_id)
         + ROW_NUMBER() OVER (PARTITION BY o.playlist_id ORDER BY o.position)
    FROM playlist_orfane o
   WHERE o.content_key = NEW.content_key
     AND NOT EXISTS (SELECT 1 FROM playlist_tracks p
                      WHERE p.playlist_id = o.playlist_id
                        AND p.position = o.position
                        AND p.track_id = NEW.id);

  -- Le playlist che hanno ritrovato un brano sono state toccate: è
  -- `updated_at` che dice alla finestra di rileggerle.
  UPDATE playlists
     SET updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000
   WHERE id IN (SELECT playlist_id FROM playlist_orfane
                 WHERE content_key = NEW.content_key);

  DELETE FROM brani_orfani WHERE content_key = NEW.content_key;
END;
