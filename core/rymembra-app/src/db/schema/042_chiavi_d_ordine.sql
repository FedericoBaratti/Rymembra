-- La chiave con cui i Brani si ordinano per nome: la stessa degli Artisti.
--
-- # Perché
--
-- La vista Brani «per scaffale» e «per titolo» ordinava con `COLLATE NOCASE`,
-- che piega solo l'ASCII e confronta per byte il resto: «È sera», «Ça plane
-- pour moi» ed «Édith Piaf» finivano dopo la Z, e «The Beatles» stava sotto T,
-- mentre Artisti e Album ordinano con `keys::sort_name` — senza accenti e
-- senza l'articolo in testa — e lo mettono sotto B (D-104). La regola è del
-- dominio, in Rust: SQLite non la sa calcolare da sé.
--
-- Chiamarla dall'`ORDER BY` con una funzione registrata funziona, ma nessun
-- indice la serve e ogni pagina ordina la libreria intera: su cinquantamila
-- brani, in debug, la prima pagina per scaffale passava da 1,4 a 1.210 ms. È il
-- guasto che la 024 aveva tolto. Quindi la chiave si **scrive**: tre colonne,
-- `ordine_titolo`, `ordine_artista`, `ordine_album`, e due indici della forma
-- degli ordinamenti, che tornano a leggere una pagina scorrendo l'indice.
--
-- # Chi le riempie, e perché non un trigger che le calcola
--
-- Le scrive Rust, `library::riempi_le_chiavi_d_ordine`, con `sort_name`: in
-- testa a ogni ricostruzione degli aggregati — che segue ogni scrittura dei
-- nomi: scansione, «Rileggi tutti i tag», arricchimento, correzioni a mano,
-- catalogo, profilo — e a ogni apertura della libreria, che è quando le righe
-- di prima di questa migrazione le ricevono la prima volta. Toccano solo le
-- righe che non le hanno, trovate per indice: una ricostruzione dopo un
-- ascolto non riscrive niente.
--
-- Un trigger o una colonna generata che chiamassero la funzione la
-- vorrebbero registrata su **ogni** connessione che scrive `tracks`, compresa
-- quella di uno strumento esterno o di un controllo d'integrità: senza, la
-- scrittura fallirebbe. Il trigger qui sotto è SQL puro e fa la sola cosa che
-- SQL sa fare da sé: quando cambiano titolo, artista o album **rimette a NULL**
-- le chiavi, e la prossima ricostruzione le rifà. Un brano nuovo nasce con le
-- chiavi a NULL. Chi scrive i nomi non deve ricordarsi di niente.
--
-- Una riga con le chiavi a NULL, fra la scrittura e la ricostruzione, sta in
-- cima all'ordine per nome (in SQLite NULL viene prima di tutto) per il tempo
-- di una transazione: è il prezzo della regola in un posto solo.
--
-- # Gli indici di prima
--
-- `idx_tracks_scaffale` e `idx_tracks_titolo` (la 024, rifatti dalla 026) non
-- li usa più nessun ordinamento, ma restano: la migrazione aggiunge e non
-- toglie, e la ricetta di ritorno qui sotto resta di sole cancellazioni. Una
-- migrazione dopo li può togliere.
--
-- # Chi ricostruisce `tracks`
--
-- Come per i trigger della 030: un `DROP TABLE tracks` si porta via le tre
-- colonne, i due indici e il trigger, e una migrazione che rifà la tabella
-- deve rifarli. Le chiavi si possono lasciare a NULL: le riempie l'apertura.
--
-- # Tornare indietro
--
-- Una versione che si ferma a una migrazione di prima rifiuta un database alla
-- 042 con `db.versionAhead`. La strada di serie è la copia che l'apertura fa da
-- sé prima di migrare, `rymembra.db.schema-041.bak` (o quella della versione da
-- cui si arriva): in Impostazioni › Libreria e dati, «Torna alla copia di prima
-- dell'aggiornamento» la rimette al suo posto. A mano: chiudi Rymembra, anche
-- dall'icona vicino all'orologio; togli `rymembra.db-wal` e `rymembra.db-shm`;
-- **copia**, non rinominare, la `.bak` al posto di `rymembra.db`; e apri la
-- versione di prima.
--
-- Senza la copia, per tornare alla 041 (nessuna riga si perde; `DROP COLUMN`
-- riscrive la tabella dei brani, che su una libreria grande vuol dire qualche
-- secondo e lo spazio di una seconda copia della tabella durante il lavoro):
--   `DROP TRIGGER chiavi_d_ordine_da_rifare;
--    DROP INDEX idx_tracks_titolo_per_nome; DROP INDEX idx_tracks_scaffale_per_nome;
--    ALTER TABLE tracks DROP COLUMN ordine_album; ALTER TABLE tracks DROP COLUMN ordine_artista;
--    ALTER TABLE tracks DROP COLUMN ordine_titolo;
--    PRAGMA user_version = 41;`
-- E per tornare alla 038, la 3.2.0, in un blocco solo e in quest'ordine:
--   `DROP TRIGGER chiavi_d_ordine_da_rifare;
--    DROP INDEX idx_tracks_titolo_per_nome; DROP INDEX idx_tracks_scaffale_per_nome;
--    ALTER TABLE tracks DROP COLUMN ordine_album; ALTER TABLE tracks DROP COLUMN ordine_artista;
--    ALTER TABLE tracks DROP COLUMN ordine_titolo;
--    DROP INDEX idx_artists_mb; DROP INDEX idx_tracks_mb_recording;
--    DROP TABLE insieme_indirizzi;
--    DROP TABLE dispositivi_ricevuti; DROP TABLE dispositivi_ascolti; DROP TABLE dispositivi;
--    PRAGMA user_version = 38;`
-- Tornando alla 038 si perdono, oltre alle chiavi e agli indici, i dispositivi
-- collegati via internet (si rifanno col QR), i loro ascolti non ancora
-- arrivati e gli indirizzi ricordati di «Ascolta insieme».

ALTER TABLE tracks ADD COLUMN ordine_titolo TEXT;
ALTER TABLE tracks ADD COLUMN ordine_artista TEXT;
ALTER TABLE tracks ADD COLUMN ordine_album TEXT;

-- La forma di `TrackOrder::Shelf`: artista, album, disco, traccia, titolo.
CREATE INDEX idx_tracks_scaffale_per_nome ON tracks(
  ordine_artista,
  ordine_album,
  disc_number,
  track_number,
  ordine_titolo
);

-- La forma di `TrackOrder::Title`, ed è anche quella con cui si trovano le
-- righe da riempire (`ordine_titolo IS NULL` sta in testa all'indice).
CREATE INDEX idx_tracks_titolo_per_nome ON tracks(ordine_titolo, ordine_artista);

-- Un nome cambiato rimette a NULL le chiavi: le rifà Rust alla prossima
-- ricostruzione. `IS NOT` e non `<>`, perché un nome che diventa o smette di
-- essere NULL è un cambiamento anche lui.
CREATE TRIGGER chiavi_d_ordine_da_rifare
AFTER UPDATE OF title, artist, album ON tracks
WHEN OLD.title IS NOT NEW.title
  OR OLD.artist IS NOT NEW.artist
  OR OLD.album IS NOT NEW.album
BEGIN
  UPDATE tracks
     SET ordine_titolo = NULL, ordine_artista = NULL, ordine_album = NULL
   WHERE id = NEW.id;
END;
