-- L'affinità fra i brani: come suona un disco, e accanto a cosa viene ascoltato.
--
-- Fin qui di un brano il database sapeva quel che c'è scritto sopra — titolo,
-- artista, genere, anno — e quanto è stato ascoltato. Sono le colonne su cui
-- lavorano le playlist intelligenti e la cascata dell'autoplay, e per rispondere
-- a «continua il disco» o «un altro album dello stesso» bastano. Non bastano per
-- «un brano che somigli a questo»: due pezzi dello stesso artista e dello stesso
-- anno possono essere uno acustico e uno distorto, e il tag non lo dice.
--
-- Questa migrazione porta le due cose che mancano, e sono di natura diversa:
-- **come suona un brano**, che si ricava dal file e non chiede niente a nessuno,
-- e **accanto a cosa viene ascoltato**, che dal file non si ricava in nessun
-- modo e arriva dai dati aperti di ListenBrainz.

-- ── l'impronta sonora ───────────────────────────────────────────────────────
--
-- Una cinquantina di numeri ricavati decodificando trenta secondi dal centro del
-- brano: timbro, andamento dello spettro, dinamica, tempo, armonia. La distanza
-- fra due impronte è la somiglianza.
--
-- # Perché una tabella sua e non colonne su `tracks`
--
-- Non per pulizia. `smart::brani` legge `COLONNE_BRANO` da `tracks` a ogni
-- valutazione di una playlist intelligente e a ogni passo dell'autoplay: un BLOB
-- da duecento byte per riga abbassa le righe per pagina e fa leggere più pagine
-- a **ogni** attraversamento della libreria, per servire un consumatore che
-- legge una riga alla volta e non compare mai in un elenco.
--
-- E perché il ciclo di vita è diverso: `tracks` cambia a ogni ascolto —
-- `play_count`, `last_played_at` — mentre l'impronta di un brano cambia solo se
-- cambiano i descrittori.
--
-- # I tre stati, e perché sono gli stessi di `Voce`
--
-- Nessuna riga, riga con un vettore, riga con `vettore` NULL: sono «mai
-- provato», «provato e c'è», «provato e non c'è» — le tre voci di
-- `aether_meta::deposito::Voce`, per la ragione già scritta là. Un deposito che
-- collassasse le ultime due richiederebbe per sempre le stesse cose.
--
-- # Perché la riga NON si invalida quando il file cambia data
--
-- Vale la pena scriverlo per esteso, perché la strada sbagliata è quella che
-- viene in mente per prima. Sembra ovvio confrontare `tracks.date_modified` con
-- la data da cui l'impronta è stata ricavata, e rifarla quando non coincidono.
--
-- Solo che l'arricchimento **riscrive i tag** di centinaia di file a ogni
-- passata, e scrivere un tag cambia la data di modifica. Un'invalidazione sulla
-- data rianalizzerebbe quei file a ogni passata di arricchimento, per sempre,
-- leggendo qualche megabyte per ognuno — per scoprire ogni volta che i numeri
-- sono gli stessi, perché **i tag non cambiano il suono**.
--
-- L'unica cosa che invalida un'impronta è quindi `versione`: se cambiano i
-- descrittori, le impronte vecchie non sono confrontabili con le nuove. Un file
-- davvero ri-codificato sotto lo stesso nome resta con l'impronta di prima fino
-- al cambio di versione successivo: è il caso raro, e pagarlo con una rianalisi
-- continua sarebbe il baratto sbagliato.
CREATE TABLE track_impronta (
  track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,

  -- Quale versione dell'estrattore ha prodotto questi numeri.
  --
  -- Per riga e non in `settings`, così un aggiornamento che cambia i descrittori
  -- non deve cancellare tutto in blocco prima di ricominciare: rianalizza a poco
  -- a poco mentre le impronte vecchie restano al loro posto, e la somiglianza
  -- intanto lavora su quelle che sono già in passo.
  versione INTEGER NOT NULL,

  -- I descrittori, in virgola mobile a singola precisione e little-endian.
  --
  -- Un BLOB e non cinquanta colonne: nessuna di quelle colonne verrebbe mai
  -- interrogata da sola — la distanza si calcola sul vettore intero — e cinquanta
  -- nomi andrebbero tenuti allineati con l'estrattore a ogni cambiamento. È la
  -- stessa scelta di `enrich_cache.body`, per la stessa ragione: un dato che il
  -- database trasporta e non interpreta.
  --
  -- NULL quando l'analisi non è riuscita.
  vettore BLOB,

  -- 'ok'          — c'è un vettore.
  -- 'illeggibile' — il file non si decodifica: un formato che symphonia non
  --                 conosce (opus, wma), un file troncato.
  -- 'muto'        — trenta secondi di silenzio. Terminale: fra un mese sarà
  --                 ancora silenzio, e un'impronta di silenzio sarebbe una
  --                 calamita che attira tutto.
  -- 'corto'       — troppo pochi campioni perché i numeri vogliano dire qualcosa.
  --
  -- **Non esiste 'rete'**, ed è deliberato: una share caduta non è una proprietà
  -- del brano. Segnare illeggibili i brani incontrati mentre il portatile era
  -- fuori dalla wifi vorrebbe dire non riprovarli per un mese. Quando la rete
  -- manca la passata si ferma e non scrive niente — è la stessa disciplina di
  -- `enrich::Decisione::non_raggiungibile`, e qui lo stato è reso proprio
  -- irrappresentabile.
  --
  -- Testo e non un intero, come `meta_salute` nella migrazione 16 e come
  -- `enrich_status`: una riga letta a mano con `sqlite3` deve spiegarsi da sé.
  esito TEXT NOT NULL,

  -- Quando si è provato, riuscendo o no. Serve a distanziare i ritentativi.
  tentato_at INTEGER NOT NULL
);

-- L'interrogazione calda è l'anti-giunzione «di cosa non ho l'impronta», e quella
-- la serve già la chiave primaria: si scorre `tracks` e si sonda qui per chiave.
-- Questo indice serve all'altra metà — i falliti maturi per un altro tentativo —
-- ed è parziale come `idx_tracks_meta_salute`: dopo la prima passata le righe
-- senza vettore sono una minoranza permanente, e su una libreria sana sono zero.
CREATE INDEX idx_track_impronta_falliti
    ON track_impronta(tentato_at)
    WHERE vettore IS NULL;

-- ── la scala su cui si misura ───────────────────────────────────────────────
--
-- Media e scarto quadratico medio di ogni dimensione, calcolati su tutta la
-- libreria. Servono a normalizzare prima di confrontare: senza, la dimensione
-- con la varianza più grande deciderebbe la distanza per un accidente di scala
-- invece che perché descrive qualcosa di più importante.
--
-- Materializzata e non ricalcolata a ogni domanda, per la stessa ragione di
-- `tracks.track_key`: la calcola una passata che ha già tutto in mano, e la
-- legge un percorso che ha un millisecondo di tempo — perché gira sul filo che
-- prepara il brano successivo mentre quello corrente sta ancora suonando.
--
-- Una riga per versione dell'estrattore: durante una rianalisi coesistono due
-- popolazioni, e la media dell'una non descrive l'altra.
CREATE TABLE impronta_scala (
  versione     INTEGER PRIMARY KEY,
  medie        BLOB    NOT NULL,
  scarti       BLOB    NOT NULL,

  -- Su quanti brani è stata calcolata. Sotto un minimo non si usa: uno scarto
  -- misurato su dieci brani non descrive una libreria, descrive dieci brani.
  brani        INTEGER NOT NULL,

  calcolata_at INTEGER NOT NULL
);

-- ── i vicini, secondo chi ascolta ───────────────────────────────────────────
--
-- Quel che l'analisi del suono non può sapere: due brani possono suonare quasi
-- identici e appartenere a mondi che non si toccano, e due che non si somigliano
-- affatto possono stare nella stessa serata di mille persone. Questo lo dicono
-- gli ascolti aggregati di ListenBrainz — dati aperti, interrogati per
-- identificativo di brano e senza dire chi sta chiedendo.
--
-- # Solo i vicini che questa libreria possiede
--
-- Il punteggio serve a ordinare dei candidati locali, e un identificativo che qui
-- non c'è non è un candidato. La risoluzione da `mb_recording_id` a `track_id`
-- avviene nella finestra in cui si tiene il lucchetto, dopo che la rete ha già
-- risposto — che è anche l'unico posto in cui **può** avvenire, visto che il
-- modulo che parla con la rete non vede `rusqlite`.
--
-- La conseguenza da tenere a mente: questa tabella va rivista quando entrano
-- brani nuovi in libreria, perché un vicino che prima non c'era adesso c'è.
CREATE TABLE brano_vicino (
  track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  vicino_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,

  -- Già normalizzato in 0..1 **dentro la risposta a cui apparteneva**, e non
  -- sull'insieme di tutte le risposte. Il punteggio grezzo è un conteggio di
  -- co-occorrenze: il primo vicino di un successo mondiale vale diecimila, quello
  -- di un disco di nicchia venti, e normalizzare globalmente vorrebbe dire far
  -- vincere sempre i brani famosi — cioè costruire esattamente il difetto per cui
  -- si fa questo lavoro.
  punteggio REAL NOT NULL CHECK (punteggio BETWEEN 0 AND 1),

  -- 'lb-registrazione' — vicino di questo preciso brano.
  -- 'lb-artista'       — ripiego per i brani senza `mb_recording_id`: vicino
  --                      dell'artista, quindi più largo e con un tetto più basso.
  fonte TEXT NOT NULL CHECK (fonte IN ('lb-registrazione', 'lb-artista')),

  raccolto_at INTEGER NOT NULL,

  PRIMARY KEY (track_id, vicino_id, fonte)
);

-- I vicini di un brano, dal più forte. È l'unica direzione in cui si legge.
CREATE INDEX idx_brano_vicino_da ON brano_vicino(track_id, punteggio DESC);

-- Quando si è chiesto e con che esito, per non richiedere ogni sei ore quel che
-- ListenBrainz non ha.
--
-- Separata da `brano_vicino` perché «ho chiesto e non c'era» è una riga sola,
-- mentre in `brano_vicino` sarebbe zero righe — indistinguibili da «non ho ancora
-- chiesto». È la terza voce di `Voce` un'altra volta, e la si paga con una
-- tabella invece che con una richiesta ripetuta all'infinito.
CREATE TABLE brano_vicino_stato (
  track_id   INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
  chiesto_at INTEGER NOT NULL,
  quanti     INTEGER NOT NULL
);
