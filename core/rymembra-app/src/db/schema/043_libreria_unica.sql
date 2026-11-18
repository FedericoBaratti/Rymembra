-- La libreria unica: i brani degli altri dispositivi collegati, in `tracks`.
--
-- # Il modello
--
-- Ogni brano che un dispositivo collegato condivide diventa una **riga
-- specchio** di `tracks`, con `path = 'lontano://<chiave 64hex>/<id remoto>'` e
-- `source = 'lontano'`. È la scelta di `nuvola://`: una riga di libreria vera,
-- che Home, Cerca, Artisti, playlist, mi piace e autoplay vedono senza che
-- nessuna query lo sappia. Il `CHECK` della 026 regge — la riga ha un `path` e
-- nessuna `source_service` — e `file_size` e `date_modified` vengono dal
-- proprietario.
--
-- Le righe specchio le riempie un riconciliatore (`src-tauri/src/lontano/
-- specchio.rs`, con la parte pura in `core/rymembra-app/src/specchio.rs`), non
-- la camminata della scansione: i metadati arrivano già letti dal proprietario.
-- Il loro `content_key` resta **NULL**, ed è voluto: i trigger della 026 e
-- della 028 (`correzioni_*`, `statistiche_*`) scattano solo per le righe che
-- hanno una chiave di contenuto, e senza di lei la copia delle statistiche del
-- proprietario non diventa «orfana» quando la riga se ne va, e non si
-- riattacca a un file locale che torna. La chiave del proprietario sta in
-- `lontano_brani.content_key`.
--
-- # Le tabelle
--
-- `lontano_cataloghi`: uno stato per dispositivo — l'ultima generazione della
-- sua libreria che si è vista, se il catalogo è stato letto per intero, e
-- quando. `lontano_secchi`: per ogni dispositivo, i secchi di 500 id e
-- l'impronta che avevano all'ultima lettura: il riconciliatore chiede solo
-- quelli la cui impronta è cambiata. `lontano_brani`: quel che una riga
-- specchio ha in più di una riga vera — l'estensione per il decodificatore, la
-- versione del file, l'impronta d'album e la chiave di contenuto del
-- proprietario, l'impronta della sua copertina, il pezzo di un'immagine CD,
-- l'impronta della riga com'è stata scritta e, se il proprietario non la manda
-- più, da quando.
-- `dispositivi_posta`: la posta in uscita verso i proprietari — un voto o un mi
-- piace dato qui a un loro brano (`valuta`), un testo corretto (`testo`) — che
-- aspetta se il dispositivo non risponde. Una sola voce per (dispositivo,
-- tipo, chiave): la scrittura più nuova prende il posto di quella che aspettava.
--
-- # `tracks.copia_di`
--
-- Il brano è lo stesso su più dispositivi — o due volte sullo stesso — quando
-- ha la stessa `COALESCE(content_key, track_key)`. `copia_di` dice, per le
-- righe di un gruppo che **non** vengono mostrate in «Tutti», quale riga le
-- rappresenta; `NULL` vuol dire visibile in «Tutti» (il rappresentante, o un
-- brano che non ha doppioni). Lo riscrive il riconciliatore dopo ogni giro e
-- dopo ogni scansione. L'indice è parziale: serve a trovare le copie di un
-- rappresentante, e quasi tutte le righe lo hanno `NULL`. Senza chiave
-- esterna: `ALTER TABLE … DROP COLUMN` rifiuta una colonna che ne ha una, e la
-- ricetta di ritorno qui sotto deve poterla togliere.
--
-- Una riga specchio che il proprietario non manda più (`lontano_brani.
-- sparito_ms`) resta per qualche giorno — la grazia — perché se il brano
-- ritorna, anche con un id nuovo, trovi ancora il suo posto nelle playlist; e
-- in quei giorni **non si suona e non si conta da nessuna parte**: ha
-- `copia_di` uguale al proprio `id`, che per «Tutti» vuol dire «nascosta» e che
-- nessun rappresentante può essere. Lo schema non cambia.
--
-- # Fatti di questa macchina
--
-- Niente di tutto questo viaggia nel profilo, nel backup o nella sincronia: le
-- righe specchio sono la musica di un altro dispositivo, e le loro statistiche
-- una copia in sola lettura di quelle del proprietario.
--
-- # Tornare indietro
--
-- Una versione che si ferma a una migrazione di prima rifiuta un database alla
-- 043 con `db.versionAhead`. La strada di serie è la copia che l'apertura fa da
-- sé prima di migrare, `rymembra.db.schema-042.bak`: in Impostazioni › Libreria
-- e dati, «Torna alla copia di prima dell'aggiornamento» la rimette al suo
-- posto. A mano: chiudi Rymembra, anche dall'icona vicino all'orologio; togli
-- `rymembra.db-wal` e `rymembra.db-shm`; **copia**, non rinominare, la `.bak` al
-- posto di `rymembra.db`; e apri la versione di prima.
--
-- Senza la copia, per tornare alla 042: **le playlist perdono i brani
-- remoti** (le loro righe spariscono con le righe specchio), insieme alla
-- cronologia d'ascolto fatta su quei brani, e i mi piace e le stelle dati a un
-- brano remoto e non ancora arrivati al proprietario non partono più. Nessun
-- brano di questa macchina si perde. `DROP COLUMN` riscrive la tabella dei
-- brani, che su una libreria grande vuol dire qualche secondo e lo spazio di
-- una seconda copia della tabella durante il lavoro.
-- La ricetta va lanciata a Rymembra chiusa, e **comincia accendendo le chiavi
-- esterne**: il programma `sqlite3` le tiene spente di serie, e spente la
-- cascata non porta via le voci di playlist e la cronologia delle righe
-- cancellate. Quel `PRAGMA foreign_keys = ON` va eseguito **fuori da una
-- transazione** (dentro un `BEGIN` SQLite lo ignora senza dire niente, e la
-- ricetta sembra riuscire con la cascata spenta) e prima di tutto il resto.
-- Spenta, le voci e la cronologia delle righe cancellate restano a puntare
-- nel vuoto — e `tracks.id`, senza
-- `AUTOINCREMENT`, può riusare quegli id per brani nuovi, che si ritroverebbero
-- nelle playlist di altri. Gli album e gli artisti che non hanno più nessun
-- brano si tolgono a mano, e il segno `aggregati = 0` fa rifare gli altri —
-- quelli che i brani remoti avevano contribuito a contare — alla prima
-- scansione:
--   `PRAGMA foreign_keys = ON;
--    DROP TABLE dispositivi_posta; DROP TABLE lontano_brani;
--    DROP TABLE lontano_secchi; DROP TABLE lontano_cataloghi;
--    DELETE FROM tracks WHERE path >= 'lontano://' AND path < 'lontano:/0';
--    DELETE FROM albums WHERE album_key NOT IN (SELECT album_key FROM tracks WHERE album_key IS NOT NULL);
--    DELETE FROM artists WHERE name NOT IN (SELECT DISTINCT TRIM(COALESCE(NULLIF(TRIM(album_artist), ''), artist)) FROM tracks);
--    UPDATE libreria_stato SET aggregati = 0;
--    DROP INDEX idx_tracks_copia_di; ALTER TABLE tracks DROP COLUMN copia_di;
--    PRAGMA user_version = 42;`
-- E per tornare alla 038, la 3.2.0, in un blocco solo e in quest'ordine:
--   `PRAGMA foreign_keys = ON;
--    DROP TABLE dispositivi_posta; DROP TABLE lontano_brani;
--    DROP TABLE lontano_secchi; DROP TABLE lontano_cataloghi;
--    DELETE FROM tracks WHERE path >= 'lontano://' AND path < 'lontano:/0';
--    DELETE FROM albums WHERE album_key NOT IN (SELECT album_key FROM tracks WHERE album_key IS NOT NULL);
--    DELETE FROM artists WHERE name NOT IN (SELECT DISTINCT TRIM(COALESCE(NULLIF(TRIM(album_artist), ''), artist)) FROM tracks);
--    UPDATE libreria_stato SET aggregati = 0;
--    DROP INDEX idx_tracks_copia_di; ALTER TABLE tracks DROP COLUMN copia_di;
--    DROP TRIGGER chiavi_d_ordine_da_rifare;
--    DROP INDEX idx_tracks_titolo_per_nome; DROP INDEX idx_tracks_scaffale_per_nome;
--    ALTER TABLE tracks DROP COLUMN ordine_album; ALTER TABLE tracks DROP COLUMN ordine_artista;
--    ALTER TABLE tracks DROP COLUMN ordine_titolo;
--    DROP INDEX idx_artists_mb; DROP INDEX idx_tracks_mb_recording;
--    DROP TABLE insieme_indirizzi;
--    DROP TABLE dispositivi_ricevuti; DROP TABLE dispositivi_ascolti; DROP TABLE dispositivi;
--    PRAGMA user_version = 38;`
-- Tornando alla 038 si perdono, oltre a quanto detto, le chiavi d'ordine e gli
-- indici, i dispositivi collegati via internet (si rifanno col QR), i loro
-- ascolti non ancora arrivati e gli indirizzi ricordati di «Ascolta insieme».

ALTER TABLE tracks ADD COLUMN copia_di INTEGER;

CREATE INDEX idx_tracks_copia_di ON tracks(copia_di) WHERE copia_di IS NOT NULL;

CREATE TABLE lontano_cataloghi (
  dispositivo TEXT    PRIMARY KEY
                      CHECK (length(dispositivo) = 64 AND dispositivo NOT GLOB '*[^0-9a-f]*'),
  -- La generazione della sua libreria all'ultimo riassunto letto.
  generazione INTEGER NOT NULL DEFAULT 0,
  -- Tutti i secchi sono stati letti almeno una volta senza errori.
  completo    INTEGER NOT NULL DEFAULT 0 CHECK (completo IN (0, 1)),
  -- L'ultimo giro finito, ms.
  ultimo_ms   INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE TABLE lontano_secchi (
  dispositivo TEXT    NOT NULL,
  da_id       INTEGER NOT NULL,
  a_id        INTEGER NOT NULL,
  impronta    TEXT    NOT NULL,
  PRIMARY KEY (dispositivo, da_id)
) WITHOUT ROWID;

CREATE TABLE lontano_brani (
  track_id       INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
  dispositivo    TEXT    NOT NULL,
  id_remoto      INTEGER NOT NULL CHECK (id_remoto > 0),
  -- Per il decodificatore: la riga specchio non ha un nome di file.
  estensione     TEXT    NOT NULL DEFAULT '',
  -- Cambia se il file cambia, per chi legge un brano a pezzi.
  versione       TEXT    NOT NULL DEFAULT '',
  -- L'impronta della chiave d'album **del proprietario**.
  album_impronta TEXT,
  -- La sua chiave di contenuto: in `tracks` resta NULL, vedi sopra.
  content_key    TEXT,
  -- L'impronta della sua copertina, anche prima che qui ci sia il file.
  cover_remota   TEXT,
  -- Per un brano d'immagine CD, il pezzo da suonare (fotogrammi da 1/75 s):
  -- in `tracks` le colonne dei brani d'immagine vogliono un file locale.
  inizio_cd      INTEGER,
  fine_cd        INTEGER,
  -- L'impronta della riga come l'ha mandata il proprietario: una riga uguale a
  -- quella di prima non riscrive `tracks` (ogni scrittura rifà l'indice di
  -- ricerca e cambia la generazione).
  impronta       TEXT    NOT NULL DEFAULT '',
  -- Da quando il proprietario non la manda più, ms: la riga aspetta la
  -- conferma (o la fine del giro) per andarsene. `NULL` finché c'è.
  sparito_ms     INTEGER
);

CREATE UNIQUE INDEX idx_lontano_brani_remoto ON lontano_brani(dispositivo, id_remoto);

-- «Dimentica dispositivo» cerca, per ogni voce di playlist, una copia dello
-- stesso brano con la chiave di contenuto del proprietario: senza l'indice è
-- una scansione di `lontano_brani` per chiave, dentro la transazione che tiene
-- il lucchetto (tremila voci su ottantamila righe: oltre due minuti). E la
-- copertina di una riga si cerca per la sua impronta, per sapere a chi
-- chiederla.
CREATE INDEX idx_lontano_brani_content_key ON lontano_brani(content_key);
CREATE INDEX idx_lontano_brani_cover ON lontano_brani(cover_remota);

-- Le righe che il proprietario non manda più (in grazia, o in attesa di una
-- conferma): poche, e ogni giro le conta, le nasconde e toglie le scadute. Un
-- indice parziale le serve da solo, senza leggere le righe di un catalogo da
-- centomila.
CREATE INDEX idx_lontano_brani_sparite ON lontano_brani(dispositivo, sparito_ms)
  WHERE sparito_ms IS NOT NULL;

CREATE TABLE dispositivi_posta (
  id          INTEGER PRIMARY KEY,
  dispositivo TEXT    NOT NULL,
  -- `valuta`, `testo`.
  tipo        TEXT    NOT NULL,
  -- Quel che distingue due voci dello stesso tipo: per un voto, il brano.
  chiave      TEXT    NOT NULL,
  -- Il contenuto, in JSON.
  corpo       TEXT    NOT NULL,
  -- Quando è stata decisa, ms: l'orologio con cui l'altro sceglie.
  quando_ms   INTEGER NOT NULL,
  UNIQUE (dispositivo, tipo, chiave)
);
