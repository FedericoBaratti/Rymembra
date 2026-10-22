-- I brani d'immagine CD: un file audio per tutto il disco, e un `.cue` che lo
-- taglia in brani.
--
-- # Le righe
--
-- Ogni brano d'immagine è una riga di `tracks` come le altre, con un percorso
-- virtuale `<percorso del .cue>|NN` (vedi `rymembra_domain::cue`): il `|` non sta
-- nel nome di nessun file Windows, quindi `UNIQUE(path)`, il piano della
-- scansione, l'albero delle cartelle e le playlist continuano a funzionare
-- senza sapere che quel percorso non si apre.
--
-- Il file che si apre davvero sta in `immagine`, e il pezzo da suonare in
-- `inizio_cd` e `fine_cd`, in fotogrammi CD da 1/75 di secondo dall'inizio del
-- file: l'unità del foglio, esatta. Arrotondarla in millisecondi vorrebbe dire
-- due brani contigui che si sovrappongono o lasciano un buco di qualche
-- campione, e su un disco dal vivo il buco si sente. `fine_cd` è `NULL` per
-- l'ultimo brano, che finisce dove finisce il file.
--
-- **Una riga è un brano d'immagine se e solo se `immagine IS NOT NULL`.** Per
-- quelle righe `file_size` è la dimensione dell'immagine e `date_modified` la
-- data più recente fra il `.cue` e l'immagine: esattamente i due numeri della
-- voce virtuale della camminata, ed è quel che fa sì che una scansione su un
-- disco invariato non rilegga i cinquemila brani d'immagine.
--
-- Solo `ADD COLUMN`, nessuna tabella ricostruita: le righe che ci sono restano
-- dove sono, con le tre colonne a `NULL`, che vuol dire «un file intero».
--
-- # L'indice parziale
--
-- Su `immagine`, solo dove c'è: chi chiede «quali brani stanno in questo file»
-- — la riproduzione che passa da un brano al contiguo, la cancellazione che
-- vuole sapere se ha scelto tutti i brani di un'immagine — non deve leggere la
-- tabella intera, e le righe di file normali, che sono quasi tutte, non pesano
-- niente nell'indice.
--
-- # La cache dei fogli
--
-- `fogli_cue` tiene quel che un `.cue` diceva l'ultima volta che lo si è letto,
-- con la dimensione e la data che aveva: un foglio invariato non si riapre a
-- ogni scansione, e su un NAS sono seicento aperture risparmiate. `chiave` è la
-- chiave del percorso del `.cue` (`paths::path_key`); `foglio` è il foglio
-- letto in JSON, oppure `null` per un `.cue` che non descrive un'immagine — i
-- fogli dei brani separati, con un `FILE` per brano — che va ricordato anche
-- lui, altrimenti si riaprirebbe ogni volta per sentirsi dire di nuovo di no.
--
-- La cache serve anche quando la rete singhiozza: un `.cue` cambiato che non si
-- riesce a rileggere si tratta come quel che diceva l'ultima volta, invece di
-- far tornare la sua immagine un brano unico per una passata.
--
-- # Tornare indietro
--
-- Una versione precedente rifiuta un database a questa versione con
-- `db.versionAhead`. Togliere a mano le tre colonne, l'indice e la tabella
-- riporta la 033, ma le righe dei brani d'immagine resterebbero lì con un
-- percorso che non si apre: prima vanno cancellate (`WHERE immagine IS NOT
-- NULL`), e la scansione di prima ritroverà le immagini come file interi.

ALTER TABLE tracks ADD COLUMN immagine TEXT;
ALTER TABLE tracks ADD COLUMN inizio_cd INTEGER;
ALTER TABLE tracks ADD COLUMN fine_cd INTEGER;

CREATE INDEX idx_tracks_immagine ON tracks(immagine) WHERE immagine IS NOT NULL;

CREATE TABLE fogli_cue (
  chiave        TEXT    PRIMARY KEY,
  dimensione    INTEGER NOT NULL,
  modificato_ms INTEGER NOT NULL,
  foglio        TEXT    NOT NULL
);
