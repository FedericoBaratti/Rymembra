-- Le app dell'utente: quelle che registra presso Google, Microsoft e Dropbox
-- perché Rymembra entri nei suoi account.
--
-- # Perché adesso
--
-- Fino alla 036 l'identificativo dell'app arrivava compilato nel binario, o da
-- una chiave di `settings` per piattaforma: `nuvola.client_id` per Google,
-- che il backup scriveva dalle Impostazioni, e `nuvola.microsoft.client_id` e
-- `nuvola.dropbox.app_key`, che nessuna schermata scriveva. Dalla 3.2 ogni app
-- è dell'utente — nessun identificativo nel binario — e può essercene più
-- d'una per piattaforma: un'app Google per il backup e una per la musica, se
-- chi le usa le vuole separate. Una chiave per piattaforma non basta più, ed è
-- un elenco di righe: una tabella, come `spotify_account` e `insieme_pari`.
--
-- # La tabella e le due colonne
--
-- - `nuvola_app`: un'app. `app` è `<piattaforma>-<8 caratteri casuali>` e
--   **non si riusa mai**: dà il nome alla voce del portachiavi del suo segreto
--   (`nuvola.app.<app>.segreto`), e un identificativo riusato darebbe a un'app
--   nuova il segreto rimasto di una vecchia. `(piattaforma, client_id)` è
--   unico: due righe per la stessa app registrata sarebbero due nomi per una
--   cosa sola, e cambiarne una lascerebbe l'altra a mentire. **Niente
--   segreti**: il segreto di un'app Google sta nel portachiavi.
-- - `nuvola_conti.app`: con quale app è collegato il conto. NULL per chi si
--   collega con un modulo (WebDAV), e per un conto la cui app è stata tolta.
-- - `nuvola_conti.client`: l'identificativo che l'app aveva **quando** il
--   conto ha dato il consenso. Se oggi l'app ne ha un altro, il refresh token
--   del conto appartiene all'app di prima, e il conto va ricollegato: lo dice
--   il confronto fra queste due colonne, senza toccare il portachiavi. Così
--   cambiare l'identificativo di un'app e rimetterlo com'era riporta i conti
--   com'erano.
--
-- Nessuna chiave esterna, come nella 036: togliere un'app non deve poter
-- togliere un conto (cioè le sue voci, le sue radici, e con loro la ragione
-- per cui i brani di quel conto stanno in libreria). Lo scollegamento lo fa il
-- codice, mettendo `app` a NULL.
--
-- # Il passaggio dalla 036
--
-- Chi aveva scritto nelle Impostazioni del backup il client del suo progetto
-- Google lo ritrova come app `google-ereditata`, già legata al backup
-- (`nuvola.app`) e ai conti di Google Drive che erano collegati con lei. Il suo
-- segreto resta dov'era, nel portachiavi alla voce `google.client_secret`, e
-- si legge da lì finché l'utente non lo riscrive: niente da spostare
-- all'avvio, e una 3.1 rimessa al suo posto lo ritrova. Le chiavi di prima si
-- cancellano: da qui in poi non le legge più nessuno.
--
-- # Tornare indietro
--
-- La 3.1.0 si ferma alla 035 e rifiuta un database alla 037 con
-- `db.versionAhead`. La strada è rimettere al posto di `rymembra.db` la copia
-- che l'apertura fa da sé prima di migrare (`rymembra.db.schema-036.bak`, o
-- la `-035` di prima della 3.2). A mano, per tornare alla 036:
--   `INSERT OR REPLACE INTO settings (key, value)
--      SELECT 'nuvola.client_id', client_id FROM nuvola_app
--       WHERE app = (SELECT value FROM settings WHERE key = 'nuvola.app')
--         AND piattaforma = 'google';
--    DELETE FROM settings WHERE key = 'nuvola.app';
--    ALTER TABLE nuvola_conti DROP COLUMN client;
--    ALTER TABLE nuvola_conti DROP COLUMN app;
--    DROP TABLE nuvola_app; PRAGMA user_version = 36;`

CREATE TABLE nuvola_app (
  app         TEXT    PRIMARY KEY,
  piattaforma TEXT    NOT NULL CHECK (piattaforma IN ('google', 'microsoft', 'dropbox')),
  nome        TEXT    NOT NULL CHECK (length(trim(nome)) > 0),
  client_id   TEXT    NOT NULL CHECK (length(trim(client_id)) > 0),
  creata_ms   INTEGER NOT NULL,
  UNIQUE (piattaforma, client_id)
);

ALTER TABLE nuvola_conti ADD COLUMN app TEXT;
ALTER TABLE nuvola_conti ADD COLUMN client TEXT;

INSERT INTO nuvola_app (app, piattaforma, nome, client_id, creata_ms)
  SELECT 'google-ereditata', 'google', 'Google', trim(value),
         CAST(strftime('%s', 'now') AS INTEGER) * 1000
    FROM settings
   WHERE key = 'nuvola.client_id' AND trim(value) <> '';

INSERT OR REPLACE INTO settings (key, value)
  SELECT 'nuvola.app', app FROM nuvola_app WHERE app = 'google-ereditata';

UPDATE nuvola_conti
   SET app = 'google-ereditata',
       client = (SELECT client_id FROM nuvola_app WHERE app = 'google-ereditata')
 WHERE servizio = 'gdrive'
   AND EXISTS (SELECT 1 FROM nuvola_app WHERE app = 'google-ereditata');

DELETE FROM settings
 WHERE key IN ('nuvola.client_id', 'nuvola.microsoft.client_id', 'nuvola.dropbox.app_key');
