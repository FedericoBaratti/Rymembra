-- Lo specchio dei servizi cloud: cosa c'è nelle cartelle scelte, senza
-- chiederlo alla rete.
--
-- # Perché nel database
--
-- Perché all'avvio la libreria del cloud deve esserci già. La camminata della
-- scansione su una radice del cloud legge questo specchio in memoria
-- (`nuvola::Specchio`), e lo specchio si ricarica da qui con una SELECT: la rete
-- serve solo a sapere cosa è cambiato nel frattempo, col cursore del servizio.
-- Senza, ogni avvio dovrebbe rifare l'elenco intero di ogni cartella — minuti,
-- su un Drive grande — prima che un brano del cloud sia suonabile.
--
-- # Le tre tabelle
--
-- - `nuvola_conti`: un conto collegato, col suo nome da mostrare, i dettagli
--   non segreti (l'indirizzo WebDAV, l'id del drive) e il cursore del flusso dei
--   cambiamenti. **Niente segreti**: token, sessioni e password stanno nel
--   portachiavi di Windows, e questo file è leggibile da ogni processo
--   dell'utente.
-- - `nuvola_voci`: le cartelle e i file che interessano (audio, `.cue`, `.lrc`)
--   sotto le radici scelte, più la catena degli antenati. `segmento` è il nome
--   come sta nel percorso, **assegnato una volta e ricordato**: due file con lo
--   stesso nome nella stessa cartella (Drive e MEGA lo permettono) ricevono un
--   segmento diverso, e il primo arrivato non lo perde quando ne arriva un
--   secondo. Ricalcolarlo a ogni avvio vorrebbe dire cambiare il percorso — cioè
--   la riga di libreria, con ascolti e playlist — di un brano che non si è
--   mosso. `tag` è il JSON di quel che il servizio sa già dei tag
--   (`TagNoti`: la faccetta `audio` di OneDrive).
-- - `nuvola_radici`: le cartelle scelte, con l'identificativo della cartella sul
--   servizio e se il primo elenco completo è arrivato in fondo. Finché non lo è,
--   l'assenza di un file dallo specchio non prova niente.
--
-- L'indice `(conto, genitore)` serve a chi cancella un conto e a chi rilegge i
-- figli di una cartella; la chiave primaria `(conto, id)` è quella con cui il
-- flusso dei cambiamenti nomina le voci.
--
-- # Una cache, e quindi fuori da tutto
--
-- Non viaggia: né nel profilo (`profilo/mod.rs`, che porta solo le tabelle che
-- nomina), né nel backup su Drive (`backup.rs`, forma fissa), né nella sincronia
-- (`sincronia.rs`, idem). Sono fotografie di un servizio prese da **questo** PC;
-- su un altro PC lo stesso conto si rielenca, e una fotografia vecchia portata
-- là direbbe che ci sono file che nel frattempo il servizio ha tolto. Le prove
-- delle tre esclusioni stanno nei tre moduli.
--
-- Nessuna chiave esterna, né in entrata né in uscita: `tracks` nomina i brani
-- del cloud per percorso, come tutti gli altri, e non per `(conto, id)`. Quindi
-- buttare lo specchio non tocca una riga di libreria.
--
-- # Tornare indietro
--
-- Non costa niente: lo specchio si ricostruisce con un elenco. La 3.1.0 si ferma
-- alla 035 e rifiuta un database alla 036 con `db.versionAhead`, come per ogni
-- migrazione; le strade sono due:
--
-- - rimettere al posto di `rymembra.db` la copia che l'apertura fa da sé prima
--   di migrare, `rymembra.db.schema-035.bak`;
-- - oppure, a mano:
--   `DROP INDEX idx_nuvola_voci_genitore; DROP TABLE nuvola_voci;
--    DROP TABLE nuvola_radici; DROP TABLE nuvola_conti; PRAGMA user_version = 35;`
--   Le righe dei brani del cloud restano in `tracks` (la 3.1.0 non le sa aprire
--   e le mostra come file che non si trovano), le cartelle `nuvola://…` restano
--   in `library.roots`; il segreto di ogni conto resta nel portachiavi finché
--   non lo si scollega da una versione che lo conosce.

CREATE TABLE nuvola_conti (
  conto     TEXT    PRIMARY KEY,
  servizio  TEXT    NOT NULL,
  nome      TEXT    NOT NULL,
  dettagli  TEXT,
  cursore   TEXT,
  ultimo_ms INTEGER
);

CREATE TABLE nuvola_voci (
  conto         TEXT    NOT NULL,
  id            TEXT    NOT NULL,
  genitore      TEXT,
  nome          TEXT    NOT NULL,
  segmento      TEXT    NOT NULL,
  cartella      INTEGER NOT NULL CHECK (cartella IN (0, 1)),
  dimensione    INTEGER NOT NULL,
  modificato_ms INTEGER NOT NULL,
  versione      TEXT,
  tag           TEXT,
  PRIMARY KEY (conto, id)
);

CREATE INDEX idx_nuvola_voci_genitore ON nuvola_voci (conto, genitore);

CREATE TABLE nuvola_radici (
  percorso TEXT    PRIMARY KEY,
  conto    TEXT    NOT NULL,
  cima     TEXT    NOT NULL,
  completa INTEGER NOT NULL CHECK (completa IN (0, 1))
);
