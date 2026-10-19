-- La libreria che non rifà tutto a ogni gesto: un indice per i file trascinati,
-- uno per gli artisti, e un segno che cambia a ogni scrittura dei brani.
--
-- # Il guasto
--
-- Tre letture che su cinquantamila brani si pagavano per intero, ogni volta.
--
-- - **I file trascinati.** `library::ids_by_path` cerca prima il percorso
--   esatto, che passa dall'indice `UNIQUE` di `path`, e poi — dove il disco
--   ignora le maiuscole — lo stesso confronto con `COLLATE NOCASE`. Nessun
--   indice aveva quella forma: ogni file lasciato cadere con una grafia diversa
--   da quella della camminata leggeva la tabella intera, un file alla volta.
-- - **Gli artisti.** Dopo ogni scansione `library::rebuild_aggregates`
--   chiedeva «questo nome sta ancora su qualche brano?» per tutti gli artisti
--   insieme, cioè una lettura intera di `tracks` per sapere quel che una
--   modifica a tre brani non poteva aver cambiato. La ricostruzione adesso
--   guarda solo i nomi toccati, e per chiederlo di un nome solo serve un indice
--   sull'espressione con cui la libreria lo calcola.
-- - **I conteggi delle playlist intelligenti.** L'elenco nella barra laterale
--   rifaceva l'interrogazione di ogni playlist intelligente a ogni apertura, e
--   una regola sul testo passa ogni riga dalla piegatura dei diacritici. Il
--   risultato non cambia finché nessuno scrive su `tracks`: serve sapere
--   **quando** qualcuno lo fa, ed è la colonna `generazione` qui sotto.
--
-- # La generazione, e perché è un numero a caso
--
-- Ogni scrittura su `tracks` — inserimento, aggiornamento, cancellazione, da
-- qualunque connessione e da qualunque modulo — la cambia, con tre trigger.
-- Nei trigger e non nel codice che scrive perché chi scrive su `tracks` sono
-- una ventina di moduli, e un contatore che ciascuno deve ricordarsi di
-- incrementare è un contatore che uno solo dimentica: la cache resterebbe
-- indietro proprio lì, in silenzio. Il trigger non lo dimentica nessuno.
--
-- Un numero **a caso** (`random()`, sessantaquattro bit) e non `+ 1`: la cache
-- che lo legge vive nel processo, e il processo può vedere lo stesso database
-- tornare indietro — un profilo importato, un ripristino da backup — con un
-- contatore che rifarebbe gli stessi numeri su un contenuto diverso. Due
-- estrazioni uguali su due contenuti diversi sono un evento da una su diciotto
-- miliardi di miliardi; due contatori uguali dopo un ripristino sono la regola.
-- Il valore dice soltanto «è cambiato qualcosa», e nessuno lo confronta per
-- ordine.
--
-- # La versione degli aggregati
--
-- Nella stessa riga, perché è la stessa specie di dato: uno stato della
-- libreria che non viaggia con i backup e non riguarda nessun altro
-- dispositivo. Dice con quale regola sono stati costruiti `albums` e `artists`
-- l'ultima volta che li si è rifatti **per intero**: la ricostruzione per le
-- sole chiavi toccate è esatta soltanto se il resto della tabella è già quello
-- che la regola di adesso produrrebbe. Zero vuol dire «non si sa», e la prima
-- ricostruzione dopo questa migrazione sarà intera. Una migrazione futura che
-- cambiasse il modo di raggruppare gli album rimette lo zero, e ottiene lo
-- stesso effetto senza che nessuno debba ricordarsene altrove.
--
-- # Chi ricostruisce `tracks` deve rifare i trigger
--
-- Come la 026 ha dovuto rifare quelli della ricerca e delle correzioni: un
-- `DROP TABLE tracks` si porta via i trigger che le stanno sopra. Senza i tre
-- qui sotto la cache delle playlist intelligenti mostrerebbe numeri vecchi
-- dopo ogni scansione — l'unica cosa che questa migrazione promette di non far
-- succedere mai.
--
-- # Cosa costa
--
-- Due indici: qualche centinaio di kilobyte su una libreria vera e un poco di
-- lavoro in più a ogni scrittura di `tracks`. Il trigger aggiunge, per ogni
-- riga scritta, la riscrittura di una riga sola di una tabella di una riga: su
-- una scansione da cinquantamila file è nel rumore accanto a quel che fa già
-- `tracks_fts_update`.
--
-- # Tornare indietro
--
-- Una versione precedente rifiuta un database a questa versione con
-- `db.versionAhead`, e serve il backup di prima dell'aggiornamento — o la copia
-- che l'apertura scrive prima di migrare. Niente di quel che si aggiunge qui è
-- un dato: togliere a mano i due indici, i tre trigger e la tabella
-- `libreria_stato` riporta esattamente la 029, e non si perde niente.

CREATE INDEX idx_tracks_percorso_piegato ON tracks(path COLLATE NOCASE);

-- L'espressione è **la stessa** di `library::ARTISTA_EFFETTIVO`, lettera per
-- lettera: SQLite usa un indice su un'espressione solo per una domanda scritta
-- identica. Una prova in `library.rs` guarda il piano e fallisce se le due
-- divergono.
CREATE INDEX idx_tracks_artista_effettivo
    ON tracks(TRIM(COALESCE(NULLIF(TRIM(album_artist), ''), artist)));

CREATE TABLE libreria_stato (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  -- Cambia a ogni scrittura su `tracks`: vedi sopra.
  generazione INTEGER NOT NULL,
  -- La versione della regola con cui `albums` e `artists` sono stati rifatti
  -- per intero l'ultima volta: `library::VERSIONE_AGGREGATI`. Zero = non si sa.
  aggregati INTEGER NOT NULL DEFAULT 0
);

INSERT INTO libreria_stato (id, generazione, aggregati) VALUES (1, random(), 0);

CREATE TRIGGER generazione_dopo_inserimento AFTER INSERT ON tracks BEGIN
  UPDATE libreria_stato SET generazione = random() WHERE id = 1;
END;

CREATE TRIGGER generazione_dopo_aggiornamento AFTER UPDATE ON tracks BEGIN
  UPDATE libreria_stato SET generazione = random() WHERE id = 1;
END;

CREATE TRIGGER generazione_dopo_cancellazione AFTER DELETE ON tracks BEGIN
  UPDATE libreria_stato SET generazione = random() WHERE id = 1;
END;
