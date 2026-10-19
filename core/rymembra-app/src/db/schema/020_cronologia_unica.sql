-- La cronologia d'ascolto smette di poter contenere la stessa riga due volte.
--
-- # Il guasto che questa migrazione impedisce, e che senza di lei arriva
--
-- Dalla 2.3.1 il profilo porta anche la libreria, e la cronologia con lei. Un
-- profilo si esporta oggi, si importa domani, e magari si reimporta la
-- settimana dopo perché non si ricordava se era già stato fatto: è il gesto
-- normale di chi ha un file su una chiavetta, non un abuso.
--
-- `play_history` non ha mai avuto un vincolo di unicità. Ogni riga è
-- `(track_id, played_at, ms_played)` e nient'altro, e due `INSERT` identici
-- producono due ascolti. Reimportare lo stesso archivio raddoppierebbe quindi
-- la storia d'ascolto — non i conteggi, che vengono da `sync_ascolti` e si
-- fondono con un CRDT, ma le **righe**: le statistiche, il grafico dell'anno,
-- «cosa ho ascoltato quel pomeriggio». E lo farebbe in silenzio, perché nessuna
-- delle due righe è sbagliata da sola.
--
-- L'importazione potrebbe difendersi da sé con una `WHERE NOT EXISTS`, ed è
-- infatti quel che fa `import_legacy`. Non basta: sarebbe una guardia scritta
-- in un posto, valida finché nessuno scrive un secondo importatore. La regola
-- vera è che **due ascolti dello stesso brano nello stesso millisecondo sono lo
-- stesso ascolto**, e quella regola va dove non la si può dimenticare.
--
-- # Perché `(track_id, played_at)` e non anche `ms_played`
--
-- Perché `ms_played` è quanto se ne è sentito, e due misure diverse dello stesso
-- ascolto — una qui e una arrivata da un altro dispositivo, che ha smesso di
-- guardare un istante prima — resterebbero due righe. La coppia che identifica
-- un ascolto è il brano e l'istante in cui è cominciato; il resto lo descrive.
--
-- Ne segue che l'inserimento deve essere un `INSERT OR IGNORE`: la prima misura
-- che arriva è quella che resta. Vince la locale perché arriva prima, ed è la
-- scelta giusta — è quella misurata dal lettore di questa macchina, non quella
-- riferita da un file.
--
-- # Chi scrive qui dentro, e perché a nessuno serve cambiare riga
--
-- Sono quattro. `import_legacy` e `import_account` hanno già la guardia
-- `WHERE NOT EXISTS (… track_id = ?1 AND played_at = ?2)`, e da oggi quella
-- guardia è una ripetizione della regola invece dell'unico posto in cui la
-- regola esiste. `profilo::biblioteca` nasce con `INSERT OR IGNORE`, che è la
-- stessa cosa detta al database invece che con una sottointerrogazione.
--
-- Il quarto è `playback::record_play`, che inserisce senza guardia, e resta
-- così di proposito: scrive **un** ascolto per sessione di riproduzione, con
-- `played_at` uguale all'istante in cui il brano è cominciato. Due inserimenti
-- con la stessa coppia vorrebbero dire lo stesso ascolto registrato due volte,
-- che oggi produce in silenzio due righe e da oggi fa fallire la transazione.
-- È il verso giusto del guasto: un doppio conteggio muto è un dato sbagliato
-- che nessuno scopre, un errore è un difetto che si corregge.

-- ── prima si deduplica, o l'indice non si crea nemmeno ──────────────────────
-- Una libreria che ha già importato due volte lo stesso archivio ha già le
-- righe doppie, e `CREATE UNIQUE INDEX` su una tabella che le contiene
-- fallisce: la migrazione si fermerebbe e il database resterebbe alla 19,
-- cioè l'applicazione non si aprirebbe più. Si tiene la riga con l'`id` più
-- basso — la prima registrata — e si buttano le altre.
--
-- Su una libreria sana questa `DELETE` non tocca niente e costa una passata
-- sull'indice `(track_id, played_at)`, che c'è dalla 5.
DELETE FROM play_history
 WHERE id NOT IN (
   SELECT MIN(id) FROM play_history GROUP BY track_id, played_at
 );

-- E ora la regola, dove non la si può dimenticare.
--
-- # Perché lo **stesso nome** dell'indice della migrazione 5
--
-- Perché è lo stesso indice. `idx_play_history_brano_quando` esiste dalla 5
-- sulla stessa coppia e nello stesso ordine, creato per la `WHERE NOT EXISTS`
-- che l'importazione consuma quarantamila volte; qui non se ne aggiunge un
-- secondo, gli si aggiunge un **vincolo**. SQLite non sa rendere unico un
-- indice che c'è, quindi lo si butta e lo si rifà — ma buttarlo per crearne
-- uno con un nome nuovo lascerebbe in giro due nomi per la stessa cosa, e la
-- prosa della migrazione 5 che spiega perché quella coppia esiste smetterebbe
-- di puntare a niente.
--
-- Il costo è un nome che non annuncia da sé di essere unico. Sta scritto qui,
-- che è dove si va a guardare cos'è cambiato.
DROP INDEX idx_play_history_brano_quando;
CREATE UNIQUE INDEX idx_play_history_brano_quando
  ON play_history(track_id, played_at);
