-- I nomi smettono di mentire: da YouTube non si scarica soltanto, si importa.
--
-- Fin qui l'unico servizio che *diceva i nomi* era Spotify, e chiamare
-- `spotify_wanted` la tabella dei brani da prendere era esatto. Con
-- l'importazione da YouTube non lo è più: dentro la stessa tabella finiscono
-- righe che con Spotify non hanno niente a che fare, e un nome così è
-- precisamente il tipo di bugia che questo albero si prende la briga di
-- evitare altrove (vedi la nota su `source` e `source_id` in `import_esterno`,
-- scritta per un'ambiguità molto più piccola di questa).
--
-- Nessun dato si sposta e nessuna riga cambia significato: solo i nomi, più due
-- colonne che dicono *da quale servizio*.

-- ── la tabella ──────────────────────────────────────────────────────────────
ALTER TABLE spotify_wanted RENAME TO desiderati;

-- Da quale servizio viene la riga. `spotify` come valore predefinito, e non
-- `NULL`: le righe che esistono già sono di Spotify — è l'unico servizio che
-- fino a questa migrazione poteva scriverle — e una colonna che ammette il
-- vuoto costringerebbe ogni interrogazione futura a decidere cosa significa
-- quel vuoto. Qui non significa niente, perché non può capitare.
ALTER TABLE desiderati ADD COLUMN source_service TEXT NOT NULL DEFAULT 'spotify';

-- ── l'indice unico ──────────────────────────────────────────────────────────
-- `(track_key, source_id)` resta l'identità di una riga, e il servizio **non**
-- entra nella coppia. Non è una svista: due identificativi di playlist di due
-- servizi diversi non collidono nella pratica, e allargare l'indice
-- richiederebbe ricostruire la tabella per proteggersi da una collisione che
-- nessuno ha mai visto. Se un giorno capitasse, il sintomo è visibile — due
-- importazioni che si fondono — e la correzione è questa riga, scritta allora.

-- ── le playlist ─────────────────────────────────────────────────────────────
-- `spotify_playlist_id` (migrazione 5) è l'aggancio che permette di
-- riconoscere una playlist già importata anche dopo che l'utente l'ha
-- rinominata lassù. Vale identico per YouTube, quindi la colonna resta e cambia
-- nome.
ALTER TABLE playlists RENAME COLUMN spotify_playlist_id TO source_playlist_id;

-- E il servizio accanto, perché senza di lui l'aggancio è ambiguo: una playlist
-- di Spotify e una di YouTube con lo stesso identificativo verrebbero
-- riconosciute come la stessa, e la seconda importazione cancellerebbe il
-- contenuto della prima. Improbabile, ma il costo di renderlo impossibile è una
-- colonna.
--
-- `NULL` qui è ammesso, a differenza di `desiderati.source_service`, e significa
-- una cosa precisa: **questa playlist non viene da nessun servizio**, l'ha fatta
-- l'utente qui dentro. È la maggioranza delle righe, e un valore predefinito la
-- farebbe sembrare importata da Spotify.
ALTER TABLE playlists ADD COLUMN source_service TEXT;

-- L'indice della migrazione 5 puntava alla colonna col nome vecchio: `RENAME
-- COLUMN` lo segue da sé, ma il suo *nome* resta `idx_playlists_spotify` e
-- diventa illeggibile. Si rifà.
DROP INDEX idx_playlists_spotify;
CREATE INDEX idx_playlists_sorgente ON playlists(source_service, source_playlist_id)
  WHERE source_playlist_id IS NOT NULL;
