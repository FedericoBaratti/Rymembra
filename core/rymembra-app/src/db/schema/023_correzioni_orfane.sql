-- Le correzioni a mano sopravvivono a un file che sparisce e ritorna.
--
-- # Il guasto
--
-- `track_overrides` è agganciata a `tracks(id)` con `ON DELETE CASCADE`, ed è
-- giusto: una correzione senza il suo brano non ha niente da correggere. Ma un
-- brano sparisce dalla libreria anche quando il **file** non è sparito: la
-- cartella tolta dalle sorvegliate e rimessa il giorno dopo, il disco esterno
-- scollegato durante una scansione, la share che non rispondeva. Il file torna,
-- la scansione lo inserisce come nuovo — con un `id` nuovo — e le correzioni
-- che l'utente aveva fatto a mano, titolo per titolo, se ne sono andate con la
-- riga vecchia. In silenzio, e senza modo di riaverle.
--
-- # Come
--
-- Prima che una riga di `tracks` con una correzione venga cancellata, la
-- correzione si mette da parte sotto la `content_key` del brano — la chiave
-- calcolata dai tag **grezzi** del file, che è la stessa quando lo stesso file
-- ritorna, e che la correzione non tocca (vedi `019_identita_e_metadati.sql`).
-- Quando una riga nuova entra con quella chiave, la correzione torna al suo
-- posto; a rimetterne i campi nella riga ci pensa la scansione, che chiama
-- `incerti::riapplica` subito dopo l'inserimento.
--
-- # Cosa non torna
--
-- `track_meta_arricchita` no. L'annotazione dell'arricchimento senza i campi
-- che descrive sarebbe una bugia sulla riga, e rimetterci i campi vorrebbe dire
-- sovrascrivere i tag di un file che magari nel frattempo è stato ritaggato;
-- l'arricchimento ripasserà da sé su una riga nuova, che è il suo mestiere.
-- Le correzioni invece sono decisioni, e una decisione non si ricalcola.
--
-- # Per quanto
--
-- Novanta giorni: abbastanza per un disco esterno lasciato in un cassetto per
-- l'estate, non tanto da tenere per sempre le correzioni di una libreria che si
-- è davvero buttata via. La pulizia sta nello stesso trigger che mette da parte:
-- le orfane nascono solo quando si cancella, ed è lì che si guarda se ce ne
-- sono di scadute.
--
-- # Tornare indietro
--
-- Come per ogni migrazione: una versione precedente rifiuta un database a
-- questa versione, e serve il backup di prima dell'aggiornamento.

CREATE TABLE correzioni_orfane (
  content_key TEXT    PRIMARY KEY,
  campi       TEXT    NOT NULL,
  set_at      INTEGER NOT NULL,
  orfana_at   INTEGER NOT NULL
);

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
