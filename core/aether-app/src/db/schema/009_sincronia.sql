-- Il conteggio d'ascolto smette di essere un numero e diventa una somma.
--
-- `tracks.play_count` è sempre stato un numero solo, e con un numero solo la
-- fusione fra due dispositivi ha due sole scelte, entrambe sbagliate. Sommare
-- raddoppia la storia ogni volta che si rifà una passata — è la ragione per cui
-- `merge_stats` prende il massimo, e su un'importazione ripetuta è la scelta
-- giusta. Ma il massimo, fra due dispositivi, **perde**: cinque ascolti sul
-- portatile e tre sul telefono fanno cinque.
--
-- Da qui `sync_ascolti`, che tiene il numero di **ciascun** dispositivo. Ogni
-- numero sale solo per mano di chi lo possiede, quindi fra due versioni dello
-- stesso vince il massimo; e il totale è la somma di numeri che non si
-- sovrappongono. Commutativo, idempotente, e giusto.
--
-- `tracks.play_count` resta dov'è e non cambia significato per chi legge: da qui
-- in poi è la **somma** di queste righe, ricalcolata a ogni ascolto e a ogni
-- passata di sincronia. Nessuna delle centoventi interrogazioni che lo leggono
-- cambia di una virgola.

-- ── il conteggio, per dispositivo ───────────────────────────────────────────
-- La chiave è `track_key` e non `track_id`: un identificativo di riga non
-- attraversa una reinstallazione, e due file dello stesso brano sono due righe
-- con la stessa identità. È la stessa scelta che `backup.rs` fa già quando
-- scrive un salvataggio.
--
-- `WITHOUT ROWID` perché la chiave primaria è già l'identità completa della
-- riga: un rowid in più sarebbe un indice in più su una tabella che, su una
-- libreria da diecimila brani e tre dispositivi, ha trentamila righe.
CREATE TABLE sync_ascolti (
  track_key   TEXT    NOT NULL,
  -- Chi ha ascoltato. Otto byte casuali in base64url, gli stessi che
  -- `aether_oauth::identificativo()` produce già per `nuvola.dispositivo`.
  --
  -- Il valore riservato `importazione` non è un dispositivo: è lo storico che
  -- arriva dal vecchio database o dall'archivio che Spotify spedisce, e che
  -- nessuno farà mai salire. Metterlo qui invece che in una colonna a parte lo
  -- fa entrare nella somma senza casi speciali, e gli dà la stessa idempotenza
  -- di tutti gli altri invece di una regola da ricordarsi.
  dispositivo TEXT    NOT NULL,
  quanti      INTEGER NOT NULL DEFAULT 0 CHECK (quanti >= 0),
  PRIMARY KEY (track_key, dispositivo)
) WITHOUT ROWID;

-- Lo storico che c'è diventa il contributo del dispositivo `importazione`.
-- Senza questa riga, la prima passata di sincronia direbbe che nessuno ha mai
-- ascoltato niente — e un `play_count` ricalcolato a zero cancellerebbe l'unica
-- cosa in tutta la libreria che una scansione non sa ricostruire.
INSERT INTO sync_ascolti (track_key, dispositivo, quanti)
SELECT track_key, 'importazione', MAX(play_count)
  FROM tracks
 WHERE play_count > 0
 GROUP BY track_key;

-- ── i dispositivi che conosciamo ────────────────────────────────────────────
-- Serve a due cose che senza di lui non si possono fare: dire all'utente **da
-- quale** dispositivo arrivano gli ascolti che vede comparire, e sapere se di un
-- dispositivo ci si fida. La fiducia è al primo incontro: un documento nuovo che
-- compare nella cartella condivisa è ignoto finché qualcuno non lo accetta.
CREATE TABLE sync_dispositivi (
  id       TEXT PRIMARY KEY,
  -- Come si chiama, per mostrarlo. `NULL` finché non lo si è accoppiato: un
  -- dispositivo che compare da solo nella cartella non ha un nome da esibire, e
  -- inventargliene uno lo farebbe sembrare già conosciuto.
  nome     TEXT,
  -- La chiave pubblica Ed25519, base64url.
  chiave   TEXT,
  fidato   INTEGER NOT NULL DEFAULT 0 CHECK (fidato IN (0, 1)),
  -- L'ultima volta che il suo documento è stato letto, in millisecondi.
  visto_ms INTEGER
);

-- ── l'ordine delle playlist ─────────────────────────────────────────────────
-- Oggi l'ordine di una playlist è l'ultimo che ha scritto, e il commento in
-- `restore.rs` spiega perché: «non esiste una mezza fusione difendibile di un
-- elenco ordinato». Il prezzo si paga in un caso solo, ma capita — due
-- dispositivi aggiungono un brano ciascuno mentre sono scollegati, e uno dei due
-- non c'è più.
--
-- Qui l'ordine non è memorizzato: è la visita di questi elementi, ognuno dei
-- quali sa **dopo chi** è stato inserito. `playlist_tracks.position` resta la
-- vista compattata da `0` a `n`, che è quel che tutte le interrogazioni leggono.
CREATE TABLE sync_sequenza (
  playlist_key TEXT NOT NULL,
  -- `<dispositivo>:<numero>`: chi lo ha inserito, e il quantesimo è.
  elemento     TEXT NOT NULL,
  -- Dopo quale elemento. `NULL` vuol dire in testa.
  dopo         TEXT,
  track_key    TEXT NOT NULL,
  -- Quando è stato tolto. `NULL` vuol dire che c'è ancora.
  --
  -- Una lapide e non una cancellazione: togliere davvero vorrebbe dire che il
  -- dispositivo che non ha visto la rimozione rimanda indietro il brano alla
  -- prima passata, che è il modo più rapido di far perdere fiducia in una
  -- sincronia.
  tolto_ms     INTEGER,
  PRIMARY KEY (playlist_key, elemento)
) WITHOUT ROWID;

-- Solo gli elementi vivi: è l'interrogazione che si fa a ogni apertura di una
-- playlist, e su una playlist rimaneggiata cento volte le lapidi sono la
-- maggioranza delle righe.
CREATE INDEX idx_sync_sequenza_vivi ON sync_sequenza(playlist_key)
  WHERE tolto_ms IS NULL;

-- ── dove si era arrivati ────────────────────────────────────────────────────
-- Il riascolto: si chiude l'applicazione a metà di un pezzo lungo e lo si
-- riprende dal telefono. Non sta in `tracks` perché non è una proprietà del
-- brano ma dell'ascolto, e perché una colonna in più su `tracks` la pagherebbero
-- in lettura tutte le interrogazioni della libreria per una riga che riguarda i
-- pochi brani lasciati a metà.
CREATE TABLE sync_posizioni (
  track_key TEXT PRIMARY KEY,
  -- Il punto, in millisecondi dall'inizio.
  ms        INTEGER NOT NULL CHECK (ms >= 0),
  -- Quando ci si è arrivati: è l'orologio che decide fra due dispositivi, e
  -- vince il più recente — non il più avanti. Chi ha riascoltato ieri sa dove è
  -- arrivato meglio di chi era andato più in là il mese scorso.
  at_ms     INTEGER NOT NULL
) WITHOUT ROWID;

-- ── l'orologio del voto ─────────────────────────────────────────────────────
-- `stats_updated_at` è sempre stato l'orologio di voto **e** preferito insieme,
-- e finché la fusione trattava lo zero come «nessun voto» bastava. Non basta più:
-- con un contatore per dispositivo si vuole che anche *togliere* un voto viaggi,
-- e per farlo viaggiare serve sapere quando è stato tolto.
--
-- Un orologio condiviso non lo può dire. Un ascolto tocca `stats_updated_at`, e
-- un brano ascoltato ieri e mai votato sarebbe indistinguibile da un brano il cui
-- voto è stato tolto ieri: spedirlo come «voto zero, ieri» cancellerebbe sul
-- telefono le cinque stelle messe l'anno scorso. Da qui una colonna sua.
--
-- `NULL` vuol dire «mai deciso», che è diverso da «deciso zero» — ed è la stessa
-- distinzione che `liked_at` fa già per il cuoricino.
ALTER TABLE tracks ADD COLUMN rating_at INTEGER;

-- I voti che ci sono già si datano con l'orologio che c'era: è un'approssimazione
-- per eccesso — quel momento può essere l'ultimo ascolto e non l'ultimo voto — ma
-- è l'unica data disponibile, e sbagliare per eccesso qui significa che un voto
-- esistente vince su un altro voto esistente, mai che ne cancella uno.
UPDATE tracks SET rating_at = stats_updated_at WHERE rating > 0;
