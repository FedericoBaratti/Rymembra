-- Lo scrobbling: una coda che sopravvive alla chiusura, e agli ascolti che
-- descrive.
--
-- Mandare un ascolto a ListenBrainz o a Last.fm è una richiesta HTTP che può
-- fallire per una ragione qualsiasi: non c'è rete, il servizio è in
-- manutenzione, il portatile è stato chiuso a metà canzone. Senza una coda su
-- disco, ognuna di quelle ragioni cancella un ascolto — in silenzio, perché non
-- c'è nessuno da avvisare nel momento in cui succede.
--
-- E la cronologia non basta a rimediare dopo. `play_history` sa **cosa** è stato
-- ascoltato, non **cosa è già stato mandato a chi**: senza quest'altra
-- informazione, riprovare vorrebbe dire rimandare tutto — e uno scrobble
-- duplicato, su Last.fm, resta duplicato per sempre.

-- ── perché i tag stanno nella riga invece di un `track_id` ──────────────────
-- È la decisione che dà forma a tutta la tabella, e la ragione è che fra
-- l'ascolto e l'invio passa del tempo. In quel tempo il brano può essere stato
-- cancellato, spostato da un riordino, o ritaggato da un arricchimento.
--
-- Una coda che nomina `tracks.id` è una coda che:
--   * perde gli ascolti dei brani cancellati (o li trascina con un `ON DELETE`
--     che nessuno si aspetta);
--   * manda al servizio i tag di **oggi** per un ascolto di ieri, cioè mente
--     esattamente nel caso in cui i tag sono stati corretti nel frattempo.
--
-- Quel che va mandato è cosa si è ascoltato allora. Costa sei colonne di testo
-- ripetuto su una tabella che in condizioni normali ha qualche decina di righe.
-- È lo stesso ragionamento per cui `spotify_wanted` si tiene titolo e artista
-- invece di puntare alla libreria.
CREATE TABLE scrobble_queue (
  id           INTEGER PRIMARY KEY,

  -- `listenbrainz` | `lastfm`. Testo e non un intero, come `download_state` e
  -- `enrich_source`: una riga letta a mano con `sqlite3` deve dire dove stava
  -- andando senza una tabella di corrispondenze in un file Rust.
  --
  -- Il `CHECK` è la stessa guardia di `play_history.source`, e per la stessa
  -- ragione: un valore storto non darebbe nessun sintomo — la coda smetterebbe
  -- semplicemente di svuotarsi per quel servizio, che è indistinguibile da «non
  -- è collegato».
  service      TEXT NOT NULL CHECK (service IN ('listenbrainz', 'lastfm')),

  -- Quando l'ascolto è **cominciato**, in millisecondi. Come
  -- `play_history.played_at`, e per la ragione scritta in `listen::Listen`:
  -- registrare la fine sposterebbe ogni ascolto avanti della durata del brano.
  --
  -- Millisecondi e non secondi anche se tutti e due i protocolli vogliono
  -- secondi: qui dentro l'unità del tempo è una sola, e la conversione sta nel
  -- punto in cui si compone la richiesta.
  played_at    INTEGER NOT NULL,

  -- I due campi obbligatori di tutti e due i protocolli.
  artist       TEXT NOT NULL,
  title        TEXT NOT NULL,

  album        TEXT,
  album_artist TEXT,
  duration_ms  INTEGER,
  track_number INTEGER,

  -- `tracks.mb_recording_id`, quando l'arricchimento c'è passato. È la
  -- differenza fra un ascolto attribuito alla canzone giusta e uno attribuito a
  -- una cover omonima.
  mbid         TEXT,

  -- Quante volte ci si è provati, e com'è andata l'ultima. Il freno: senza,
  -- un guasto che non si risolve mai riproverebbe per sempre, e un ascolto che
  -- il servizio rifiuta terrebbe occupata la testa della coda impedendo a
  -- quelli dietro di partire.
  attempts     INTEGER NOT NULL DEFAULT 0,
  last_error   TEXT,

  queued_at    INTEGER NOT NULL,

  -- ── l'unicità che rende l'accodamento ripetibile ──
  -- Serve a due cose diverse che finiscono nello stesso vincolo:
  --   * riaccodare la cronologia appena importata da Spotify una seconda volta
  --     non raddoppia niente;
  --   * due sorgenti che segnalano lo stesso ascolto — la riproduzione qui e
  --     l'importazione — ne accodano uno.
  -- Su `(service, …)` e non solo sull'ascolto: lo stesso brano va mandato a
  -- tutti e due i servizi, e sono due righe legittime.
  UNIQUE (service, played_at, artist, title)
);

-- L'interrogazione calda è una sola: «cosa resta da mandare a questo servizio,
-- fra quelle che non hanno ancora finito i tentativi». `attempts` sta in mezzo
-- perché è un confronto d'intervallo, e senza di lui una coda con diecimila
-- righe abbandonate le rileggerebbe tutte a ogni passata per scartarle.
CREATE INDEX idx_scrobble_da_mandare
  ON scrobble_queue(service, attempts, played_at);
