-- Gli indici che mancano ai vicini di ListenBrainz.
--
-- # Perché
--
-- A fine passata d'arricchimento `vicinanza::vicini` scrive i vicini di un
-- lotto di cento brani: per ognuno un `INSERT … SELECT` per ogni vicino che il
-- servizio ha dato — fino a venti —, tutti dentro una transazione col lucchetto
-- della libreria in mano. I vicini di registrazione si cercano per
-- `tracks.mb_recording_id`, quelli d'artista per `artists.mb_artist_id`, e
-- nessuna delle due colonne aveva un indice: ogni vicino era una lettura intera
-- della tabella, e una tabella sola non fa costruire a SQLite un indice
-- automatico. Su una libreria da decine di migliaia di brani sono secondi per
-- lotto, e in quei secondi un cuore, un voto o una playlist aspettano e poi
-- rinunciano con `db.locked`.
--
-- Parziali, `WHERE … IS NOT NULL`: la maggior parte dei brani non ha un
-- identificativo di MusicBrainz, e una riga d'indice per ognuno di loro sarebbe
-- spazio e scritture per niente a ogni scansione. Un indice parziale serve lo
-- stesso a `= ?`, perché un valore uguale a qualcosa non è NULL, e SQLite lo
-- sa. Lo prova `i_vicini_si_risolvono_per_indice`, sulle due query vere.
--
-- # Tornare indietro
--
-- Due indici e basta: nessuna riga cambia, nessuna tabella si rifà. Una
-- versione che si ferma a una migrazione di prima rifiuta un database alla 041
-- con `db.versionAhead`.
--
-- La strada di serie è la copia che l'apertura fa da sé prima di migrare:
-- `rymembra.db.schema-040.bak`, o `rymembra.db.schema-038.bak` per chi arriva
-- dalla 3.2.0. In Impostazioni › Libreria e dati, «Torna alla copia di prima
-- dell'aggiornamento» la rimette al suo posto. A mano: chiudi Rymembra, anche
-- dall'icona vicino all'orologio; togli `rymembra.db-wal` e `rymembra.db-shm`,
-- che sono del database di adesso e applicati alla copia la rovinerebbero;
-- **copia**, non rinominare, la `.bak` al posto di `rymembra.db`; e apri la
-- versione di prima, perché questa riaggiornerebbe subito la copia.
--
-- Senza la copia, per tornare alla 040:
--   `DROP INDEX idx_artists_mb; DROP INDEX idx_tracks_mb_recording; PRAGMA user_version = 40;`
-- E per tornare alla 038, la 3.2.0, in un blocco solo e in quest'ordine:
--   `DROP INDEX idx_artists_mb; DROP INDEX idx_tracks_mb_recording;
--    DROP TABLE insieme_indirizzi;
--    DROP TABLE dispositivi_ricevuti; DROP TABLE dispositivi_ascolti; DROP TABLE dispositivi;
--    PRAGMA user_version = 38;`
-- Il solo ritorno scritto in testa alla 039 lascia `insieme_indirizzi`: la
-- 3.2.0 apre lo stesso, ma il riaggiornamento si ferma sulla 040, che la trova
-- già lì. Tornando alla 038 si perdono, oltre agli indici, i dispositivi
-- collegati via internet (si rifanno col QR), i loro ascolti non ancora
-- arrivati e gli indirizzi ricordati di «Ascolta insieme».

CREATE INDEX idx_tracks_mb_recording ON tracks(mb_recording_id)
  WHERE mb_recording_id IS NOT NULL;

CREATE INDEX idx_artists_mb ON artists(mb_artist_id)
  WHERE mb_artist_id IS NOT NULL;
