-- L'identità di un brano, separata dai suoi metadati.
--
-- Fin qui `tracks.track_key` faceva due mestieri incompatibili, e li faceva
-- male appena l'utente correggeva qualcosa.
--
-- Il primo mestiere è **dire chi è questo brano agli altri dispositivi**:
-- `aether_sync::documento` e `aether_app::backup` scrivono quella chiave nei
-- documenti, ed è con quella che ascolti, voti e posizioni si riattaccano al
-- brano giusto sull'altro computer. Perché funzioni deve seguire i valori che
-- l'utente vede: se ha corretto «Pink Floyd» su un file taggato «unknown», la
-- chiave deve dire Pink Floyd, altrimenti l'altro dispositivo cerca un brano
-- che lì non esiste.
--
-- Il secondo mestiere è **riconoscere un file che si è spostato**:
-- `scan_plan::match_moved_tracks` confronta la chiave delle righe sparite con
-- quella dei file appena comparsi, e quando combaciano aggiorna il percorso
-- invece di cancellare la riga e inserirne una nuova — che è ciò che salva
-- ascolti, voto, preferiti e appartenenza alle playlist. Perché funzioni deve
-- essere **la stessa prima e dopo**, e i tag del file non cambiano quando
-- l'utente corregge la libreria.
--
-- I due requisiti si contraddicono, e la contraddizione costava dati: ogni
-- correzione allontanava `tracks.track_key` dai tag del file, finché lo
-- spostamento di quel file non veniva più riconosciuto. Il brano si duplicava,
-- la riga vecchia veniva cancellata, e con `ON DELETE CASCADE` se ne andava
-- anche la sovrascrittura in `track_overrides` — cioè la correzione stessa.
--
-- # Perché una colonna nuova e non un cambio di significato
--
-- Rendere grezza `track_key` sarebbe più pulito, e sarebbe **rompente**: i
-- documenti di sync già sul disco e sugli altri dispositivi contengono chiavi
-- calcolate dai campi corretti. Cambiarne il senso rilegherebbe in silenzio
-- ascolti e posizioni al brano sbagliato, che secondo le regole del CHANGELOG
-- è un major. Quindi `track_key` resta esattamente quel che era — la chiave
-- mostrata e di scambio — e l'identità di contenuto prende una colonna sua.
-- Il costo della scelta è quella colonna, ed è quello che si paga.

-- La chiave calcolata dai **soli tag grezzi**: vedi `aether_domain::keys::ContentKey`.
--
-- La scrivono `library::insert_track` e `library::update_track`, dai valori del
-- `TrackRow` — cioè da quel che il file ha detto, prima di qualunque
-- riapplicazione delle correzioni. `incerti::applica_correzioni` ed
-- `enrich::aggiorna_brano` non la toccano, ed è la ragione per cui esiste.
--
-- NULL è ammesso, e vuol dire «non si sa ancora»: vedi il riempimento qui
-- sotto. Chi legge usa `COALESCE(content_key, track_key)`.
ALTER TABLE tracks ADD COLUMN content_key TEXT;

-- L'unica interrogazione è «quale riga ha questa chiave», e la fa la scansione
-- una volta per lotto. Non è unico: due copie dello stesso brano in due
-- cartelle hanno la stessa identità di contenuto ed è giusto che l'abbiano —
-- `match_moved_tracks` le appaia in ordine e lo dice.
CREATE INDEX idx_tracks_content_key ON tracks(content_key);

-- ── il riempimento, ed è prudente apposta ───────────────────────────────────
--
-- Per una riga **senza** sovrascritture `track_key` è già calcolata dai tag del
-- file e da nient'altro: le due chiavi coincidono byte per byte, e copiarla è
-- esatto, non un'approssimazione.
--
-- Per una riga **con** una sovrascrittura non è così, e non c'è modo di
-- rimediare da qui: i tag grezzi di quel file non sono in nessuna tabella —
-- `track_overrides.campi` conserva quel che l'utente ha deciso, non quel che
-- c'era prima — e questa migrazione non può aprire migliaia di file per
-- rileggerli. Quelle righe restano quindi a NULL fino alla prima riscansione
-- che le rilegge, e nel frattempo `COALESCE(content_key, track_key)` le fa
-- comportare esattamente come si comportavano ieri: né meglio né peggio.
--
-- Non è una svista: è la sola parte di questa migrazione che potrebbe
-- sembrarlo. Scriverci `track_key` anche lì vorrebbe dire dichiarare grezza una
-- chiave che grezza non è, e cioè mettere in tabella un dato falso per la
-- soddisfazione di non vedere NULL.
UPDATE tracks
   SET content_key = track_key
 WHERE id NOT IN (SELECT track_id FROM track_overrides);

-- ── e una riparazione, che con l'identità non c'entra ───────────────────────
--
-- Sta qui perché una migrazione è l'unico posto da cui si può riparare quel che
-- è già scritto, e queste righe sono già scritte.
--
-- Il difetto: fino alla 2.3.1 `testi::imposta_scarto` inseriva la riga di
-- `lyrics` con `source = 'mano'` anche quando un testo non c'era ancora — cioè
-- ogni volta che qualcuno toccava il cursore della correzione mentre il
-- catalogo stava ancora rispondendo. Da quel momento gli `UPSERT` di
-- `testi::ricorda` e `testi::ricorda_esito`, che hanno `WHERE lyrics.source <>
-- 'mano'` per non calpestare mai un testo scritto dall'utente, non scrivevano
-- più niente per quel brano: nessun testo, mai più, e nessun modo di
-- accorgersene. Il difetto è chiuso — adesso si scrive il valore neutro, la
-- stringa vuota di `Fonte::Nessuna` — ma le righe bloccate restano bloccate.
--
-- La condizione non può dare falsi positivi. Un testo scritto davvero a mano lo
-- scrive `testi::salva_a_mano`, che mette sempre l'LRC in `synced`: una riga
-- `'mano'` con `synced` a NULL **e** `plain` a NULL **e** non dichiarata
-- strumentale non è un testo di nessuno — è una riga che esiste solo per
-- portare un `offset_ms`, e quello resta dov'è.
--
-- Dopo la riparazione quei brani rientrano da soli nella coda dei testi da
-- cercare: `checked_at` su quelle righe è rimasto NULL — gli `UPSERT` che lo
-- avrebbero scritto sono proprio quelli che non passavano — e
-- `idx_lyrics_da_cercare` le indicizza già.
UPDATE lyrics
   SET source = ''
 WHERE source = 'mano'
   AND synced IS NULL
   AND plain IS NULL
   AND instrumental = 0;

-- ── e i metadati arricchiti, che dai file escono ────────────────────────────
--
-- Fino alla 2.3.0 l'arricchimento scriveva quel che aveva trovato **dentro i
-- file dell'utente**: una passata automatica, ogni mezz'ora, che apriva i mp3
-- di qualcun altro e ne riscriveva i tag senza che nessuno guardasse prima.
-- Dalla 2.3.1 non lo fa più: quel che trova finisce qui, e il file resta com'è.
--
-- # Perché una tabella sua e non una colonna `origine` in `track_overrides`
--
-- Perché `track_overrides` è documentata — nella 016, e in `incerti.rs` — come
-- «quel che l'utente ha deciso», ed è l'unica tabella che lo dice. Metterci
-- dentro anche quel che ha deciso MusicBrainz, distinguendole per una colonna,
-- renderebbe «dimentica l'arricchimento» una `DELETE ... WHERE origine =
-- 'arricchimento'` su una tabella in cui vivono anche le correzioni a mano: il
-- primo `WHERE` sbagliato — o la prima riga scritta senza quella colonna da una
-- versione più vecchia — porterebbe via le correzioni insieme all'arricchimento.
-- Due tabelle rendono quel guasto **impossibile da scrivere**, che è la stessa
-- ragione per cui `aether-meta` non riceve mai una `Connection`.
--
-- Sono anche due cose diverse per natura: una correzione è una parola
-- definitiva e non ha una confidenza; una corrispondenza è un'ipotesi provata,
-- ha una fonte e un punteggio, e si dimentica in blocco.
--
-- # L'ordine di risoluzione
--
-- tag grezzi → `track_meta_arricchita` → `track_overrides`. Sta scritto per
-- esteso, una volta sola, nel `//!` di `aether_app::incerti`.
--
-- `campi` è il JSON dei soli campi che l'arricchimento ha davvero scritto sulla
-- riga (la forma è `enrich::CampiArricchiti`): serve a sapere **cosa** viene di
-- lì, non a rimetterlo — a rimettere la riga ai tag del file ci pensa
-- `enrich::dimentica`, che il file lo rilegge, perché il file è intatto ed è
-- tutto il punto del cambiamento.
--
-- `ON DELETE CASCADE`: sparito il brano, l'annotazione non descrive più niente.
CREATE TABLE track_meta_arricchita (
  track_id   INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
  campi      TEXT NOT NULL,
  fonte      TEXT,
  confidenza REAL,
  set_at     INTEGER NOT NULL
);
