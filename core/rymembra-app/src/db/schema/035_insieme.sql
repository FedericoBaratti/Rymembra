-- I PC associati di «Ascolta insieme».
--
-- # Cosa c'è, e cosa no
--
-- Una riga per PC con cui ci si è associati confermando il codice di quattro
-- cifre: la sua chiave **pubblica** X25519 (64 cifre esadecimali minuscole, la
-- forma di `ChiavePubblica::in_testo` del trasporto), il nome con cui lo si
-- mostra, e se è bloccato. La chiave segreta di **questo** PC non c'è e non ci
-- deve stare: sta nel portachiavi di Windows, e questo file è leggibile da ogni
-- processo dell'utente oltre che quello che una copia di sicurezza porta via.
--
-- Non è `sync_dispositivi`, e la somiglianza inganna: quella tabella dice quali
-- dispositivi hanno scritto nello stesso Drive, questa quali PC della rete di
-- casa possono entrare in una sessione. Mescolarle vorrebbe dire che collegare
-- un account rende fidato un PC, o che dimenticare un PC tocca il backup.
--
-- # Fatti di questa macchina
--
-- La tabella non viaggia: né nel profilo (`profilo/mod.rs`, che porta solo le
-- tabelle che elenca), né nel backup su Drive (`backup.rs`, che ha una forma
-- fissa), né nella sincronia (`sincronia.rs`, idem). Un'associazione è una
-- fiducia data **da questo PC** a quella chiave; riportata su un altro computer
-- direbbe che si fida di qualcuno a cui nessuno, di là, ha mai confermato un
-- codice. Le prove delle tre esclusioni stanno nei tre moduli.
--
-- # I vincoli
--
-- `chiave` unica, perché una chiave è un PC: riassociarsi aggiorna la riga che
-- c'è (`insieme::associa`) invece di farne una seconda, e il blocco di un PC
-- non si aggira riassociandosi sotto un'altra riga. Il `CHECK` sulla forma è la
-- stessa guardia di `play_history.source` e per la stessa ragione: una chiave
-- in maiuscolo o tagliata non darebbe nessun sintomo, semplicemente quel PC
-- non sarebbe mai riconosciuto come fidato e nessuno capirebbe perché.
--
-- `visto_ms` è `NULL` finché il PC non si è fatto vedere dopo l'associazione:
-- non è un dato che si possa inventare.
--
-- Nessuna chiave esterna, né in entrata né in uscita: la tabella non nomina
-- brani e nessuno la nomina. È quel che rende il ritorno indietro economico.
--
-- # Tornare indietro
--
-- La 3.0.2 si ferma alla 034 e rifiuta un database alla 035 con
-- `db.versionAhead`, come per ogni migrazione. Le strade sono due:
--
-- - rimettere al posto di `rymembra.db` la copia che l'apertura fa da sé prima
--   di migrare, `rymembra.db.schema-034.bak`: è la libreria del giorno
--   dell'aggiornamento, e quel che si è ascoltato o votato dopo non c'è;
-- - oppure, a mano, `DROP TABLE insieme_pari;` e `PRAGMA user_version = 34;`.
--   Si perdono **solo** le associazioni (si rifanno col codice), e le chiavi
--   `insieme.*` che restano in `settings` la 3.0.2 non le legge. Niente altro
--   dipende da questa tabella.

CREATE TABLE insieme_pari (
  id           INTEGER PRIMARY KEY,
  chiave       TEXT    NOT NULL UNIQUE
                       CHECK (length(chiave) = 64 AND chiave NOT GLOB '*[^0-9a-f]*'),
  nome         TEXT    NOT NULL,
  bloccato     INTEGER NOT NULL DEFAULT 0 CHECK (bloccato IN (0, 1)),
  associato_ms INTEGER NOT NULL,
  visto_ms     INTEGER
);
