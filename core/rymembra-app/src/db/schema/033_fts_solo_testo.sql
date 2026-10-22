-- L'indice di ricerca si aggiorna solo quando cambia il testo che indicizza.
--
-- # Il guasto
--
-- `tracks_fts_update` scattava su **ogni** `UPDATE` di `tracks`: un ascolto
-- contato, un voto, la data di modifica rimessa a `-1` da «Rileggi tutti i tag»,
-- la chiave d'album riscritta dalla ricostruzione degli aggregati. Ogni volta,
-- per ogni riga, una cancellazione e un reinserimento nell'indice di testo —
-- con gli stessi titoli di prima. Sulla ricostruzione intera degli album, che
-- riscrive `album_key` su migliaia di righe, era la parte che teneva il
-- lucchetto della libreria per secondi mentre la finestra aspettava.
--
-- # Come
--
-- `AFTER UPDATE OF title, artist, album, album_artist, genre`: le cinque colonne
-- che l'indice contiene, e nient'altro. Il corpo è identico a quello di prima
-- (001, poi rifatto dalla 026): togliere la riga con i valori vecchi, rimetterla
-- con i nuovi. SQLite fa scattare il trigger quando una di quelle colonne sta
-- nel `SET`, anche se il valore non cambia: `update_track`, che le scrive
-- tutte, continua ad aggiornare l'indice come prima, e l'indice resta sempre
-- quel che la tabella dice.
--
-- # Chi ricostruisce `tracks` deve rifare anche questo
--
-- Come per la 026 e la 030: un `DROP TABLE tracks` si porta via i trigger, e
-- questo va ricreato **in questa forma**, non in quella della 001.
--
-- # Tornare indietro
--
-- Ricreare il trigger senza `OF …` riporta la 032. Nessun dato cambia.

DROP TRIGGER IF EXISTS tracks_fts_update;

CREATE TRIGGER tracks_fts_update
AFTER UPDATE OF title, artist, album, album_artist, genre ON tracks
BEGIN
  INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist, genre)
  VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist, old.genre);
  INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, genre)
  VALUES (new.id, new.title, new.artist, new.album, new.album_artist, new.genre);
END;
