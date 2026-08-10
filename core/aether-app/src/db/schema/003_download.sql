-- Lo scaricamento dei desiderati: da elenco a coda.
--
-- `spotify_wanted` è nata nella migrazione 2 come una tabella in cui si scrive e
-- basta: fuori da `legacy/` non ha mai avuto una sola `SELECT`. L'importazione
-- ci depositava i brani mancanti e finiva lì — ed è precisamente ciò che
-- l'utente vedeva come «trova le canzoni ma non le scarica».
--
-- Queste colonne sono ciò che trasforma un elenco in una coda: senza uno stato
-- per riga non c'è modo di riprendere dopo una chiusura, di sapere cosa è già
-- stato preso, o di distinguere un brano da ritentare da uno che su YouTube non
-- esiste.

-- ── lo stato dello scaricamento ─────────────────────────────────────────────
-- `attesa` | `fatto` | `fallito` | `introvabile`.
--
-- **`introvabile` separato da `fallito` non è pedanteria.** È la distinzione che
-- `TrackDownloadResult` (`legacy/.../download/spotifyEngine.ts:49-52`) documenta
-- come essenziale, e la ragione è asimmetrica: ritentare all'infinito un brano
-- che su YouTube non c'è consuma la coda e — peggio — nasconde quelli che un
-- ritentativo lo meritavano davvero, che finiscono in fondo a un elenco di
-- fallimenti perpetui. Distinti, l'interfaccia può offrire «riprova i falliti»
-- senza rimettere in fila i casi persi.
--
-- Il testo e non un intero: una riga letta a mano con `sqlite3` deve dire cosa
-- è successa senza una tabella di corrispondenze da un'altra parte.
ALTER TABLE spotify_wanted ADD COLUMN download_state TEXT NOT NULL DEFAULT 'attesa';

-- Quante volte ci si è provati. È il freno: senza, un guasto ritentabile che non
-- si risolve mai — la rete di casa che non va — riprova per sempre.
ALTER TABLE spotify_wanted ADD COLUMN download_attempts INTEGER NOT NULL DEFAULT 0;

-- L'ultimo errore, come codice del catalogo più causa. Serve a dire *perché* un
-- brano manca, che è l'unica forma utile di «non ce l'ho fatta».
ALTER TABLE spotify_wanted ADD COLUMN download_error TEXT;

-- Dove è finito il file, quando è andata bene. Non è ridondante rispetto a
-- `tracks.path`: fra lo scaricamento e la scansione che lo porta in libreria
-- passa del tempo, e in quel tempo questa è l'unica riga che sa dove sta.
ALTER TABLE spotify_wanted ADD COLUMN download_path TEXT;

-- Quale video è stato scelto. Serve a due cose concrete: capire *perché* un
-- brano scaricato è quello sbagliato, e non riscegliere da capo al ritentativo.
ALTER TABLE spotify_wanted ADD COLUMN youtube_url TEXT;

ALTER TABLE spotify_wanted ADD COLUMN updated_at INTEGER;

-- ── i tag che Spotify sa e la tabella non teneva ────────────────────────────
-- Numero di traccia, disco e anno esistono su `SpotifyTrack` fin dalla
-- migrazione 2, ma non venivano salvati: finché i desiderati erano un elenco da
-- guardare non servivano a niente.
--
-- Adesso sì, e servono in modo preciso. I tag scritti sul file scaricato sono
-- quelli di Spotify proprio perché sono più affidabili di quelli che YouTube
-- ricava dal titolo del video; senza queste tre colonne, `track_number`
-- ripiegherebbe sulla posizione nella playlist — che per un album importato da
-- una playlist mista è un numero inventato, e in libreria si vedrebbe come un
-- album con le tracce nell'ordine sbagliato.
ALTER TABLE spotify_wanted ADD COLUMN track_number INTEGER;
ALTER TABLE spotify_wanted ADD COLUMN disc_number INTEGER;
ALTER TABLE spotify_wanted ADD COLUMN year INTEGER;

-- Parziale, come `idx_tracks_isrc` in `002_spotify.sql:18` e per la stessa
-- ragione: a coda finita quasi tutte le righe sono `fatto` o `introvabile`, e
-- indicizzarle costerebbe spazio per rispondere a una domanda che nessuno fa.
-- L'unica interrogazione calda è «cosa resta da prendere».
CREATE INDEX idx_spotify_wanted_da_scaricare
  ON spotify_wanted(download_state) WHERE download_state = 'attesa';
