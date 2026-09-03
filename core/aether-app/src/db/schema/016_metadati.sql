-- La provenienza dei metadati, e la correzione che l'utente decide.
--
-- Fin qui una riga di `tracks` diceva *cosa* un brano si chiama e non *chi lo
-- ha detto*. La differenza sembra accademica finché non si guarda cosa costa:
-- `enrich::titolo_di_ripiego` deve **indovinare** se un titolo è vero
-- confrontandolo con la radice del nome del file, perché è l'unico indizio che
-- gli resta; l'interfaccia non può distinguere un artista letto dai tag da uno
-- dedotto dalla cartella; e l'utente non ha modo di sapere quali brani vale la
-- pena guardare.
--
-- Tutte e tre le domande hanno la stessa risposta, e va scritta una volta sola
-- nel punto in cui la si conosce con certezza: quando il file viene letto.

-- ── quanto ci si può fidare ─────────────────────────────────────────────────
-- `ok` — ogni campo obbligatorio viene dai tag, o dall'utente.
-- `dedotto` — almeno uno viene dalla cartella, da una riparazione di codifica o
--             da un segnaposto.
-- `degradato` — il file non si è potuto leggere: c'è solo quel che dice il
--             percorso, e la durata non si conosce.
--
-- Il testo e non un intero, come per `download_state` ed `enrich_source`: una
-- riga letta a mano con `sqlite3` deve dire cosa è successo senza una tabella di
-- corrispondenze da un'altra parte.
--
-- `DEFAULT 'ok'` e non NULL: una libreria scritta prima di questa migrazione non
-- ha niente da segnalare finché non viene riscansionata, e una colonna nulla
-- costringerebbe ogni interrogazione a un COALESCE.
ALTER TABLE tracks ADD COLUMN meta_salute TEXT NOT NULL DEFAULT 'ok';

-- ── da dove viene ogni campo ────────────────────────────────────────────────
-- JSON: `{"titolo":{"da":"percorso"},"artista":{"da":"tag-riparato","prima":"BjÃ¶rk"}}`.
--
-- # Perché JSON e non una colonna per campo
--
-- Perché i campi con una provenienza sono sette e non se ne interroga mai uno
-- solo: si leggono tutti insieme, per una riga sola, quando qualcuno apre
-- l'elenco «da sistemare». Sette colonne quasi sempre nulle su ogni riga di
-- libreria sarebbero spazio speso per un'operazione rara. Il precedente in casa
-- è `enrich_undo.tags`, e prima ancora `ui.shortcuts`.
--
-- `prima` c'è solo per i campi riparati, ed è ciò che permette all'interfaccia
-- di mostrare «prima → dopo» invece di chiedere all'utente di fidarsi.
ALTER TABLE tracks ADD COLUMN meta_origine TEXT;

-- Cosa non andava: `["senza-tag","mojibake","segnaposto-artista"]`.
-- I nomi sono quelli di `ricostruzione::Problema::as_str`.
ALTER TABLE tracks ADD COLUMN meta_problemi TEXT;

-- L'unica interrogazione calda su queste colonne: «quali brani devo guardare».
--
-- Parziale, al contrario di `idx_tracks_arricchimento` e per la ragione opposta
-- scritta lì: quello indicizza una condizione che è vera per la maggioranza
-- delle righe finché la prima passata non è finita, questo una che riguarda una
-- minoranza per sempre — su una libreria taggata bene, nessuna riga.
CREATE INDEX idx_tracks_meta_salute ON tracks(meta_salute) WHERE meta_salute <> 'ok';

-- ── quel che l'utente ha corretto a mano ────────────────────────────────────
-- I valori dedotti stanno in libreria, non nei file: correggerli è un fatto di
-- database. Ed è qui che nasce il problema che questa tabella risolve.
--
-- `update_track` riscrive titolo, artista e album **dai tag del file** ogni
-- volta che la data di modifica cambia. Una correzione salvata nella sola
-- `tracks` sopravviverebbe fino alla prima riscansione di quel file e poi
-- sparirebbe, senza che niente lo dica. Tenendola qui, la scansione la ritrova e
-- la riapplica: il file resta la fonte, la correzione resta l'ultima parola.
--
-- # Perché l'identità è il brano e non il percorso
--
-- Perché fra la correzione e la riscansione può passarci un riordino, che sposta
-- il file e riscrive `tracks.path`. È la stessa scelta di `enrich_undo`, per la
-- stessa ragione, e `plan_scan` riconosce i file spostati dalla chiave
-- conservando la riga con il suo `id`.
--
-- Con `ON DELETE CASCADE` un brano tolto dalla libreria si porta via la propria
-- correzione: se la riga non c'è più, il file è sparito dal disco o è uscito
-- dalle cartelle sorvegliate, e tenere una correzione per un brano che la
-- libreria non conosce vorrebbe dire farla riapparire, un giorno, su un file che
-- nel frattempo può essere diventato un altro.
CREATE TABLE track_overrides (
  track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,

  -- Solo i campi che l'utente ha deciso, come JSON:
  -- `{"artista":"Pink Floyd","album":"The Wall"}`. Un campo che non c'è non è
  -- «da svuotare»: è «non l'ho toccato». Non esiste un modo di dire «cancellalo»
  -- e non deve esistere, per la stessa ragione per cui non esiste in
  -- `enrich::Fields`.
  campi TEXT NOT NULL,

  set_at INTEGER NOT NULL
);
