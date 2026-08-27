-- La Home, e l'indice che le serve.
--
-- Aether apriva su un elenco. Le quattro destinazioni della libreria — album,
-- artisti, brani, preferiti — sono tutte e quattro un elenco, e nessuna di
-- loro risponde alla domanda che uno si fa aprendo un lettore musicale:
-- «cosa stavo ascoltando». La Home la fa, e per farlo ha bisogno di sapere
-- **quando** una cosa è stata suonata l'ultima volta.
--
-- La colonna c'era già — `tracks.last_played_at`, scritta da `record_play` a
-- ogni ascolto — ma non era indicizzata, e nessuno la ordinava. Gli indici su
-- `tracks` erano `track_key`, `artist`, `album_key`, `genre`,
-- `(liked, liked_at)` e `date_added`: tutte le domande che l'applicazione
-- sapeva fare, e nessuna era questa.
--
-- Senza indice, «gli ultimi venti ascoltati» è una scansione dell'intera
-- tabella più un ordinamento, a ogni apertura della finestra. Su una libreria
-- da qualche migliaio di righe non si vedrebbe; su quella di chi tiene
-- vent'anni di musica sì, e si vedrebbe **all'avvio**, che è il momento
-- peggiore in cui farsi aspettare.
--
-- `WHERE last_played_at IS NOT NULL`: un indice parziale, perché la Home i
-- mai ascoltati non li chiede mai — li mette in fondo, e in fondo ci vanno
-- senza bisogno di essere indicizzati. Su una libreria appena scansionata,
-- dove *nessun* brano è ancora stato suonato, questo indice è vuoto e non
-- costa niente.
CREATE INDEX IF NOT EXISTS idx_tracks_last_played
    ON tracks(last_played_at DESC)
    WHERE last_played_at IS NOT NULL;
