-- Da «dove sta il video» a «cosa se ne può fare».
--
-- La colonna `youtube_url` è nata quando la fonte dell'audio era una sola e
-- l'unica domanda da farsi era «lo cerco o ce l'ho già?». Adesso le fonti sono
-- più d'una e nessuna dà tutto: prima di prendere dei byte bisogna sapere se si
-- possono prendere, e quella è una domanda che la vecchia colonna non poteva
-- nemmeno formulare.
--
-- Tre colonne nuove, e nessun dato che si sposta. La regola del file di
-- migrazione — aggiungere in fondo, mai in mezzo — vale anche qui: le
-- migrazioni da 1 a 9 restano quelle che erano, e un database scritto ieri si
-- apre oggi.

-- ── da quale catalogo, e a che indirizzo ────────────────────────────────────
-- `youtube_url` resta dov'è e non si scrive più. Cancellarla vorrebbe dire
-- ricostruire la tabella per recuperare qualche kilobyte, e in cambio buttare
-- via l'unica traccia di come una riga vecchia era stata risolta — che è
-- esattamente ciò che serve a capire, fra sei mesi, perché un file si chiama
-- così. `fonte_url` la sostituisce e dice la stessa cosa senza nominare
-- nessuno.
ALTER TABLE desiderati ADD COLUMN fonte_url TEXT;

-- Sotto che licenza sta il brano che si è preso. `NULL` fino a quando non lo si
-- prende davvero, e dopo mai più: senza questa colonna, «da dove viene questo
-- file e cosa ci posso fare» diventa una domanda senza risposta il giorno dopo.
--
-- Il valore è il nome stabile di `aether_domain::esterno::Licenza` —
-- `pubblicoDominio`, `cc-by-nc-sa`, `liberaNonCommerciale` — e non un URL: un
-- indirizzo va a prendersi, un nome si interroga.
ALTER TABLE desiderati ADD COLUMN licenza TEXT;

-- Che cosa se ne può fare, secondo la fonte che l'ha dato: `scaricabile`,
-- `soloAscolto`, `soloAcquisto`. Predefinito `soloAcquisto` e non `NULL`, per la
-- stessa ragione per cui `Licenza::Sconosciuta` non permette la copia: il valore
-- che si trova quando nessuno ha ancora deciso niente dev'essere il più
-- restrittivo, non il più comodo.
ALTER TABLE desiderati ADD COLUMN disponibilita TEXT NOT NULL DEFAULT 'soloAcquisto';

-- ── i nomi delle fonti ──────────────────────────────────────────────────────
-- `source_service` conteneva `spotify` (migrazione 7) e, per un periodo,
-- `youtube`. Il primo diventa `archivio-spotify`, che è la sola via da cui oggi
-- arrivi qualcosa da Spotify; il secondo non esiste più come fonte e le sue
-- righe tornano a dire soltanto «qualcuno mi ha nominato», che è tutto quel che
-- una riga in attesa ha mai voluto dire.
--
-- `Fonte::da_testo` rilegge comunque da sé qualunque valore ignoto come
-- `ArchivioSpotify`, quindi questo `UPDATE` non è ciò che rende il database
-- leggibile: è ciò che lo rende *interrogabile a mano* senza doversi ricordare
-- la storia.
UPDATE desiderati SET source_service = 'archivio-spotify'
 WHERE source_service IN ('spotify', 'youtube');
UPDATE playlists SET source_service = 'archivio-spotify'
 WHERE source_service IN ('spotify', 'youtube');

-- E i brani che erano entrati in libreria dalla vecchia coda. `tracks.source`
-- distingue un file trovato da una scansione (`scan`) da uno che è arrivato
-- perché qualcuno l'ha chiesto: la parola cambia, il significato no. È lo stesso
-- valore che `enrich` interroga per sapere quali brani hanno metadati da
-- ricontrollare.
UPDATE tracks SET source = 'catalogo' WHERE source = 'youtube';

-- ── l'indice della coda ─────────────────────────────────────────────────────
-- Quello parziale della migrazione 3 (`idx_spotify_wanted_da_scaricare`) resta
-- valido: guarda `download_state` e `download_attempts`, che non cambiano. Il
-- nome però nomina una tabella che dalla migrazione 7 si chiama in un altro
-- modo, e un indice che mente su quale tabella serve è la cosa che fa perdere
-- mezz'ora a chi legge un `EXPLAIN QUERY PLAN`.
DROP INDEX IF EXISTS idx_spotify_wanted_da_scaricare;
CREATE INDEX IF NOT EXISTS idx_desiderati_da_prendere
  ON desiderati(download_state, download_attempts, id)
  WHERE download_state = 'attesa';
