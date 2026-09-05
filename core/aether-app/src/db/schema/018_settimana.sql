-- Le raccolte del lunedì: l'appuntamento, non l'algoritmo.
--
-- La Home ha quattro ripiani calcolati in una query — Riprendi, Recenti,
-- Aggiunti, Trascurati — e sono gli stessi oggi e fra un mese se non si tocca
-- niente. Sono utili e non sono una ragione per riaprire il programma.
--
-- Questa migrazione porta la cosa che manca: **qualcosa che cambia da solo, di
-- lunedì**. Due raccolte, calcolate in locale mentre il computer è fermo, che
-- restano ferme per sette giorni e poi diventano altre.
--
-- # Perché si scrive invece di ricalcolare a ogni apertura
--
-- Perché il calcolo non è deterministico rispetto alla libreria: `Ripescati`
-- guarda `last_played_at`, e `last_played_at` cambia **proprio ascoltando la
-- raccolta**. Una raccolta ricalcolata a ogni apertura si consumerebbe mentre
-- la si ascolta — apri, senti tre brani, torni indietro e quei tre non ci sono
-- più, sostituiti da altri. Sarebbe un difetto che nessuno riesce a
-- raccontare: «sparisce mentre la uso».
--
-- Lo stesso vale per `Ancora`: `raccogli` è deterministico a parità di
-- libreria, ma un disco aggiunto mercoledì cambierebbe le raccolte a metà
-- settimana. Il patto è che siano quelle di lunedì fino al lunedì dopo.
--
-- # Perché non sono playlist
--
-- Le playlist sono dell'utente: si rinominano, si riordinano, si cancellano, e
-- la sincronizzazione le porta in giro. Queste scadono da sole dopo un mese e
-- nessuno le ha chieste. Metterle in `playlists` vorrebbe dire riempire di
-- roba automatica il posto in cui uno tiene le proprie cose — e poi dover
-- distinguere le une dalle altre con una colonna, che è la stessa tabella in
-- due travestimenti.

-- ── la raccolta ─────────────────────────────────────────────────────────────
CREATE TABLE settimana_raccolta (
  id INTEGER PRIMARY KEY,

  -- L'istante universale in cui è cominciato il lunedì di chi guarda.
  --
  -- Universale e non locale, perché è la forma in cui il resto del database
  -- tiene il tempo; il fuso entra una volta sola, quando lo si calcola, e da
  -- lì in poi è un numero come gli altri. Vedi `aether_domain::settimana::lunedi`.
  lunedi INTEGER NOT NULL,

  -- 'ripescati' — dalla tua libreria, quel che hai amato e non senti da mesi.
  -- 'ancora'    — gruppi coerenti trovati sulle impronte sonore.
  --
  -- La terza del progetto — 'fuori', musica nuova dai cataloghi liberi — non è
  -- qui perché non è ancora costruibile: i cataloghi che Aether conosce sanno
  -- rispondere soltanto a «hai questo preciso brano?», non a «cosa somiglia a
  -- questo». Il vincolo `CHECK` la lascia fuori invece di ammetterla e non
  -- produrla mai: uno stato che non si sa raggiungere non va dichiarato
  -- rappresentabile.
  genere TEXT NOT NULL CHECK (genere IN ('ripescati', 'ancora')),

  -- Quale delle `ancora` è: 0, 1, 2. Per `ripescati` è sempre 0.
  ordine INTEGER NOT NULL,

  -- Come si chiama, secondo quello che contiene davvero.
  --
  -- Non un titolo già scritto — «Mix 3» è il modo di dire che non si è
  -- guardato dentro — ma il **materiale** con cui l'interfaccia compone la
  -- frase nella lingua di chi legge: un nome di genere, o un nome d'artista,
  -- o niente. La traduzione sta in `lingue/`, come tutto il resto del testo
  -- che l'utente vede; qui sta il dato.
  --
  -- NULL quando il gruppo non ha una maggioranza chiara: succede, ed è
  -- preferibile a battezzarlo col primo artista che capita.
  etichetta TEXT,

  -- 'genere' | 'artista', e dice come leggere `etichetta`. NULL con essa.
  etichetta_tipo TEXT CHECK (etichetta_tipo IN ('genere', 'artista')),

  -- Quando è stata aperta la prima volta, o NULL.
  --
  -- Serve al ripiano dell'annuncio, che sparisce quando le raccolte sono
  -- state guardate: senza questa colonna l'unico modo di far sparire un
  -- annuncio sarebbe un pulsante «ho capito», che è la stessa cosa detta
  -- peggio.
  aperta_at INTEGER,

  generata_at INTEGER NOT NULL,

  -- Un lunedì produce una raccolta sola per posto. È anche ciò che rende
  -- ripetibile la generazione: si prova a generare a ogni avvio, e la seconda
  -- volta non scrive niente.
  UNIQUE (lunedi, genere, ordine)
);

-- Il verso in cui si legge: «le raccolte di questo lunedì», e «quelle vecchie
-- da buttare». Sono la stessa colonna in due direzioni.
CREATE INDEX idx_settimana_raccolta_lunedi ON settimana_raccolta(lunedi DESC);

-- ── i brani dentro ──────────────────────────────────────────────────────────
--
-- `ON DELETE CASCADE` in tutte e due le direzioni: un brano tolto dalla
-- libreria esce dalle raccolte, e una raccolta scaduta si porta via le proprie
-- righe. Una raccolta che si accorcia perché hai cancellato dei file è il
-- comportamento giusto — l'alternativa sarebbe una playlist con dei buchi.
CREATE TABLE settimana_brano (
  raccolta_id INTEGER NOT NULL REFERENCES settimana_raccolta(id) ON DELETE CASCADE,
  track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,

  -- L'ordine deciso dal calcolo, che non è quello di nessuna colonna: per
  -- `ripescati` è il punteggio di ripescaggio, per `ancora` è la vicinanza al
  -- seme del gruppo. Senza, l'ordine sarebbe quello di `rowid`, cioè quello
  -- in cui i file sono entrati in libreria — un ordine che non vuol dire
  -- niente e che si nota subito.
  posizione INTEGER NOT NULL,

  PRIMARY KEY (raccolta_id, posizione)
);

-- Serve alla cancellazione a cascata dal lato `tracks`, che senza indice è una
-- scansione per ogni brano tolto. La stessa ragione di `idx_play_history_track`.
CREATE INDEX idx_settimana_brano_track ON settimana_brano(track_id);
