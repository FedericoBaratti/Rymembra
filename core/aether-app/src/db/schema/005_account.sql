-- L'importazione di un account Spotify intero: da dove viene un ascolto, e
-- quale playlist di lassù è quale playlist di qua.
--
-- Fin qui da Spotify si importava un link alla volta, e quel che ne restava era
-- una playlist più le righe di `spotify_wanted`. Un account intero porta due
-- cose che prima non arrivavano: la **cronologia d'ascolto** — anni di righe,
-- non un pugno — e l'**identità** di quel che si è importato, che serve la
-- seconda volta.

-- ── da dove viene un ascolto ────────────────────────────────────────────────
-- `play_history` conteneva solo ascolti veri: brani suonati da Aether, misurati
-- da `ListenTracker`. Con l'archivio di Spotify ci entrano ascolti che nessuno
-- ha misurato qui, e la differenza va **detta nella riga**.
--
-- # Perché non basta importarli e basta
--
-- Perché è l'unica scrittura di tutta l'applicazione che non si può disfare
-- guardandola. Un riordino ha il suo giornale, un arricchimento ha `enrich_undo`,
-- una scansione riconosce gli spostamenti. Quarantamila righe di cronologia
-- versate in mezzo a quelle vere sarebbero indistinguibili il giorno dopo, e
-- l'unico modo di tornare indietro sarebbe ripristinare un backup — cioè perdere
-- anche tutto quel che si è fatto nel frattempo.
--
-- Con questa colonna, «dimentica gli ascolti importati» è una `DELETE` mirata
-- seguita da un ricalcolo di `play_count`. Una colonna larga sette caratteri
-- contro un'operazione che altrimenti non esiste.
--
-- # Perché testo e non un intero
--
-- Stessa ragione di `download_state` e di `enrich_source`: una riga letta a mano
-- con `sqlite3` deve dire cosa è successo senza andare a cercare una tabella di
-- corrispondenze in un file Rust.
--
-- Il `CHECK` invece è nuovo rispetto a quelle due, e sta qui per una ragione
-- precisa: un valore scritto storto non darebbe nessun sintomo finché qualcuno
-- non prova a disfare l'importazione, e a quel punto la `DELETE` non troverebbe
-- le righe. Un errore che si manifesta solo nel momento in cui serve rimediare è
-- il tipo di errore che vale la pena rendere impossibile.
ALTER TABLE play_history ADD COLUMN source TEXT NOT NULL DEFAULT 'local'
  CHECK (source IN ('local', 'spotify'));

-- Parziale, come `idx_tracks_isrc`: gli ascolti importati sono una minoranza —
-- e in una libreria che non ha mai visto Spotify sono zero. Un indice pieno
-- costerebbe una voce per ogni ascolto vero per rispondere a una domanda che
-- riguarda solo gli altri.
CREATE INDEX idx_play_history_importati ON play_history(source)
  WHERE source <> 'local';

-- ── l'indice che l'importazione consuma quarantamila volte ──────────────────
-- L'importazione è idempotente per la stessa guardia di `import_legacy`:
-- `WHERE NOT EXISTS (SELECT 1 FROM play_history WHERE track_id = ? AND played_at = ?)`.
-- Su `idx_play_history_track` quella domanda trova il brano e poi **scorre tutti
-- i suoi ascolti** cercando l'istante: su una canzone sentita cinquecento volte
-- sono cinquecento righe lette per decidere di una sola, moltiplicate per le
-- decine di migliaia di righe dell'archivio.
--
-- Con la coppia diventa una ricerca sola.
CREATE INDEX idx_play_history_brano_quando ON play_history(track_id, played_at);

-- E il vecchio se ne va: `(track_id, played_at)` comincia per `track_id`, quindi
-- risponde a tutto quel che rispondeva lui. Tenerli tutti e due vorrebbe dire
-- mantenere due alberi a ogni inserimento — su una tabella che durante
-- un'importazione ne riceve decine di migliaia — per non guadagnare niente.
DROP INDEX idx_play_history_track;

-- ── quale playlist di lassù è quale playlist di qua ─────────────────────────
-- `playlists.playlist_key` nasce dal **nome**, ed è una scelta deliberata
-- (vedi `keys::PlaylistKey`): rinominare una playlist ne fa una nuova. Regge
-- finché le playlist le fa l'utente qui dentro.
--
-- Con un account che si sincronizza più volte non regge più. Chi rinomina «Corsa»
-- in «Corsa 2026» su Spotify, alla seconda importazione si ritroverebbe due
-- playlist: la vecchia col nome vecchio e i suoi brani, e una nuova identica
-- accanto. L'identificativo di Spotify è l'unica cosa che il rinominare non
-- tocca.
ALTER TABLE playlists ADD COLUMN spotify_playlist_id TEXT;

-- Parziale, e senza `UNIQUE`. Parziale perché le playlist che vengono da Spotify
-- sono una parte di quelle che esistono. Senza `UNIQUE` perché la cosa giusta da
-- fare, se per un guasto due righe finissero con lo stesso identificativo, è
-- accorgersene e fonderle — non far fallire l'importazione successiva con un
-- vincolo violato, che è il momento in cui l'utente può fare meno di tutti per
-- rimediare.
CREATE INDEX idx_playlists_spotify ON playlists(spotify_playlist_id)
  WHERE spotify_playlist_id IS NOT NULL;

-- ── di chi è l'account ──────────────────────────────────────────────────────
-- Una tabella per quella che sarà quasi sempre una riga sola. La strada già
-- battuta sarebbe stata `settings`, come fa il backup con `nuvola.email`,
-- `nuvola.file_id` e compagnia — ed è proprio guardando quella che si vede
-- perché no: là «scollega» scrive **stringa vuota** in quattro chiavi, e le
-- righe restano. Un account scollegato e uno mai collegato si distinguono
-- confrontando stringhe vuote, e prima o poi qualcuno ne dimentica una.
--
-- Qui scollegare è `DELETE FROM spotify_account`, e non c'è nessuno stato
-- intermedio da ricordarsi di ripulire.
CREATE TABLE spotify_account (
  -- L'identificativo dell'utente su Spotify. Chiave primaria, così ricollegare
  -- lo stesso account aggiorna la riga invece di aggiungerne una seconda.
  spotify_user_id TEXT PRIMARY KEY,

  -- Il nome visualizzato, per poter scrivere «collegato come Tizio» invece di
  -- una stringa di ventidue caratteri casuali.
  display_name    TEXT,

  -- Quando è finita l'ultima importazione riuscita.
  last_sync_at    INTEGER,

  -- Da quale delle due strade: `api` o `archivio`. Serve a dire la verità sulla
  -- cronologia — l'API ne dà cinquanta righe, l'archivio le dà tutte — così che
  -- «3 ascolti importati» si legga come il limite di un endpoint e non come un
  -- guasto.
  last_source     TEXT NOT NULL CHECK (last_source IN ('api', 'archivio'))
);
