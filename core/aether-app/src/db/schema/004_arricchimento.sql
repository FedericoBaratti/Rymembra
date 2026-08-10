-- L'arricchimento dei metadati: quel che si è deciso, e come tornare indietro.
--
-- La baseline dichiarava già `enrich_status` ed `enrich_attempted_at` — le due
-- colonne che dicono *se* e *quando* — e gli identificativi MusicBrainz che
-- l'arricchimento riempie. Qui si aggiunge quel che serve perché la passata sia
-- automatica: da chi viene la decisione, quanto ci si credeva, e soprattutto
-- come disfarla.

-- ── come tornare indietro ───────────────────────────────────────────────────
-- L'arricchimento scrive nei file dell'utente e nessuno guarda prima. Questa
-- tabella è ciò che rende quella scelta accettabile invece che avventata: prima
-- di toccare i tag di un file si fotografa com'era, e la fotografia resta.
--
-- # Una volta sola, e la prima
--
-- Si scrive con `INSERT OR IGNORE`: è lo stato **prima che Aether ci mettesse
-- le mani**, non quello del passo precedente. Un secondo arricchimento non la
-- sovrascrive, altrimenti «annulla» riporterebbe il file a una versione che
-- l'utente non ha mai visto — quella scritta da noi la volta prima.
--
-- # Perché una tabella e non due colonne su `tracks`
--
-- Perché i campi da ricordare sono nove, e servono tutti insieme solo il giorno
-- in cui qualcuno annulla. Nove colonne quasi sempre nulle su ogni riga di
-- libreria sarebbero spazio speso per un'operazione rara; una riga per brano
-- **toccato** costa quanto i brani toccati.
CREATE TABLE enrich_undo (
  -- L'identità è il brano, non il percorso: fra la scrittura e l'annullamento
  -- può passarci un riordino, che sposta il file e riscrive `tracks.path`.
  -- Chi annulla rilegge il percorso di adesso da `tracks`.
  track_id   INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,

  -- Il percorso di allora. Non è ridondante: serve a **riconoscere** che si sta
  -- per riscrivere il file giusto, e a dire quale file era se la riga di
  -- libreria non c'è più.
  path       TEXT    NOT NULL,

  -- I tag di prima, come JSON. Un JSON e non nove colonne perché è un blocco
  -- che si legge e si riscrive tutto insieme, e non se ne interroga mai un
  -- campo solo: indicizzare `enrich_undo.title` non serve a nessuno.
  tags       TEXT    NOT NULL,

  written_at INTEGER NOT NULL
);

-- Con `ON DELETE CASCADE` un brano tolto dalla libreria si porta via il proprio
-- annullamento. È voluto: se la riga non c'è più il file è sparito dal disco o è
-- uscito dalle cartelle sorvegliate, e riscrivere i tag dentro un file che la
-- libreria non conosce sarebbe peggio che non poterlo fare.
-- Uno spostamento invece non perde niente: `plan_scan` riconosce i file spostati
-- dalla chiave e conserva la riga con il suo `id`.

CREATE INDEX idx_enrich_undo_quando ON enrich_undo(written_at);

-- ── la memoria delle risposte ───────────────────────────────────────────────
-- MusicBrainz concede una richiesta al secondo. Senza questa tabella ogni
-- passata richiederebbe da capo le stesse cose, e le più costose sarebbero
-- quelle inutili: i trecento file che nessun catalogo riconoscerà mai.
--
-- `body` NULL significa «ha risposto che non ce l'ha». È la distinzione che
-- vale di più: senza, un «non c'è» non si potrebbe ricordare e si
-- richiederebbe per sempre.
CREATE TABLE enrich_cache (
  service    TEXT    NOT NULL,
  key        TEXT    NOT NULL,
  body       BLOB,
  expires_at INTEGER NOT NULL,
  PRIMARY KEY (service, key)
) WITHOUT ROWID;

-- Serve a una cosa sola: buttare via quel che è scaduto senza scorrere tutto.
CREATE INDEX idx_enrich_cache_scadenza ON enrich_cache(expires_at);

-- ── da chi viene la decisione ───────────────────────────────────────────────
-- `mb-release` quando ha vinto l'abbinamento dell'album intero, `mb-recording` /
-- `itunes` / `deezer` quando ha vinto un candidato per brano. Il testo e non un
-- intero, come per `download_state`: una riga letta a mano con `sqlite3` deve
-- dire cosa è successo senza una tabella di corrispondenze da un'altra parte.
--
-- Serve a rispondere alla domanda che si fa quando un brano è arricchito male:
-- *chi lo ha detto?* Senza, la risposta richiede di rifare la ricerca a mano.
ALTER TABLE tracks ADD COLUMN enrich_source TEXT;

-- Quanto ci si credeva, da 0 a 1. Per l'abbinamento d'album è `1 - distanza`.
--
-- Non entra in nessuna decisione — quelle le prende `aether_domain::enrich` sul
-- momento, con le prove davanti — ed è deliberato: un numero salvato invita a
-- confrontarlo con una soglia scritta altrove, e da lì nasce una seconda idea di
-- cosa sia una corrispondenza buona. Sta qui per essere **guardato**, quando si
-- vuole tarare la decisione su una libreria vera.
ALTER TABLE tracks ADD COLUMN enrich_confidence REAL;

-- L'unica interrogazione calda: «cosa resta da arricchire, e cosa è ora di
-- ritentare». Non parziale, al contrario di `idx_tracks_isrc` e
-- `idx_spotify_wanted_da_scaricare`: quelle indicizzano una minoranza di righe,
-- qui invece `enrich_status IS NULL` è la condizione della **maggioranza** —
-- almeno finché la prima passata non è finita, che è esattamente quando questo
-- indice serve.
CREATE INDEX idx_tracks_arricchimento ON tracks(enrich_status, enrich_attempted_at);
