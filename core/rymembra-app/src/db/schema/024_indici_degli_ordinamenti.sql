-- Gli indici degli ordinamenti che gli elenchi usano davvero.
--
-- # Il guasto
--
-- La vista Brani «per scaffale» ordina per `artist COLLATE NOCASE, album
-- COLLATE NOCASE, disc_number, track_number, title COLLATE NOCASE` e prende
-- duecento righe con `LIMIT … OFFSET …`; la vista Album ordina per
-- `artist COLLATE NOCASE, year, title COLLATE NOCASE`. Nessun indice aveva quella
-- forma — `idx_tracks_artist` è sull'artista **senza** `NOCASE`, e SQLite non lo
-- usa per un ordinamento che piega le maiuscole — quindi **ogni pagina**
-- ordinava la tabella intera in un B-tree temporaneo per tenerne duecento righe.
-- Su diciottomila brani è il tempo che si vede scorrendo: una pagina che arriva
-- in ritardo, e la sentinella in fondo che resta a lungo sullo schermo.
--
-- Con un indice della stessa forma l'ordinamento non si fa: si scorre l'indice
-- e ci si ferma a duecento. Una prova in `library.rs` guarda il piano di SQLite
-- e fallisce se torna il B-tree temporaneo.
--
-- # Cosa costa
--
-- Spazio su disco, qualche centinaio di kilobyte su una libreria vera, e un
-- poco di lavoro in più a ogni scrittura di `tracks` — cioè durante la
-- scansione, che scrive a lotti in una transazione sola.
--
-- # Tornare indietro
--
-- Come per ogni migrazione: una versione precedente rifiuta un database a
-- questa versione, e serve il backup di prima dell'aggiornamento.

CREATE INDEX idx_tracks_scaffale ON tracks(
  artist COLLATE NOCASE,
  album COLLATE NOCASE,
  disc_number,
  track_number,
  title COLLATE NOCASE
);

CREATE INDEX idx_tracks_titolo ON tracks(
  title COLLATE NOCASE,
  artist COLLATE NOCASE
);

CREATE INDEX idx_tracks_piu_ascoltati ON tracks(
  play_count DESC,
  last_played_at DESC,
  title COLLATE NOCASE
);

CREATE INDEX idx_albums_scaffale ON albums(
  artist COLLATE NOCASE,
  year,
  title COLLATE NOCASE
);
