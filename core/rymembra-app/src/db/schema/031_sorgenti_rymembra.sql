-- Le sorgenti con il nome di adesso: `aether:...` diventa `rymembra:...`.
--
-- # Il guasto
--
-- Le importazioni da un archivio (`import_account`) non hanno un identificativo
-- di Spotify da scrivere in `desiderati.source_id` — «Brani che ti piacciono» non
-- è una playlist, e `Playlist1.json` non porta l'identificativo delle sue — e se
-- ne fabbricano uno col nome del programma davanti: `<prefisso>spotify:preferiti`,
-- `<prefisso>archivio:playlist:<chiave>`, `<prefisso>archivio:album:<chiave>`.
-- Il prefisso dichiara che non è un indirizzo, e nessun'altra riga ne ha uno
-- simile. Cambiando il nome del programma il codice nuovo scrive il prefisso
-- nuovo, e il database di chi veniva da prima ha ancora il vecchio: la stessa
-- playlist importata una seconda volta non si riconoscerebbe, e la coda di
-- scaricamento mostrerebbe due elenchi per ogni importazione fatta in passato.
--
-- # Dove stanno, e cosa si riscrive
--
-- - `desiderati.source_id`: l'identità di una riga è `(track_key, source_id)`
--   (indice unico `idx_spotify_wanted_identita`, il cui nome è rimasto quello
--   della tabella di prima). Qui il vincolo conta: due righe che dopo la
--   riscrittura avessero la stessa coppia non possono coesistere.
-- - `playlists.source_playlist_id`: l'aggancio con cui si riconosce una playlist
--   già importata. Nessun indice unico, ma `prepara_playlist` lo confronta per
--   uguaglianza esatta (insieme al servizio), e un prefisso rimasto vecchio
--   vorrebbe dire creare la playlist una seconda volta.
-- - `import_reports.source_id` (chiave primaria) e, dentro il suo `payload`, il
--   campo `sourceId` del rapporto: chi mostra la pagina delle importazioni legge
--   il rapporto dal JSON e lo confronta con il `source_id` delle righe della
--   coda, quindi le due metà devono dire la stessa cosa.
--
-- Non c'è nessun'altra tabella con un valore che porti il nome di prima: le
-- impostazioni (`settings`) tengono chiavi e valori senza marchio, gli
-- identificativi di dispositivo sono casuali, e i nomi dei file (database,
-- diario, skin, copie del profilo) non stanno nel database ma sul disco, dove li
-- rinomina il trasloco.
--
-- # Il prefisso esatto, non un `LIKE`
--
-- `substr(x, 1, 7) = 'aether:'` e non `x LIKE 'aether:%'`: `LIKE` ignora le
-- maiuscole e tratta `_` e `%` come caratteri speciali. Il confronto con `=` è
-- binario, quindi riscrive esattamente quel che il codice di prima scriveva, e
-- solo quello. Sette sono i caratteri di `aether:`, e `substr(x, 8)` è tutto il
-- resto.
--
-- # La collisione, e perché una riga vecchia si butta
--
-- `UPDATE OR IGNORE` lascia com'è la riga che, riscritta, violerebbe l'indice
-- unico. Succede solo se la riga col nome nuovo c'è già: significa che lo stesso
-- brano della stessa importazione è stato scritto due volte, una con ciascun
-- prefisso, e la riga nuova — quella che il codice di adesso riconosce — vince.
-- Le righe vecchie che restano dopo l'`UPDATE` sono per costruzione quei
-- doppioni, e si tolgono: lasciarle vorrebbe dire una seconda importazione fantasma
-- nella coda, con il vecchio prefisso, che nessun codice nuovo saprebbe più
-- aggiornare.
--
-- La migrazione è idempotente per costruzione: dopo di lei nessuna riga comincia
-- per `aether:`, quindi rieseguirla non trova niente da fare.

-- ── i desiderati ────────────────────────────────────────────────────────────
UPDATE OR IGNORE desiderati
   SET source_id = 'rymembra:' || substr(source_id, 8)
 WHERE substr(source_id, 1, 7) = 'aether:';

DELETE FROM desiderati
 WHERE substr(source_id, 1, 7) = 'aether:';

-- ── le playlist ─────────────────────────────────────────────────────────────
-- `updated_at` non si tocca: il contenuto della playlist non è cambiato, e un
-- orario nuovo la farebbe sembrare modificata qui, cioè più recente di quel che
-- un altro dispositivo ha scritto davvero.
UPDATE playlists
   SET source_playlist_id = 'rymembra:' || substr(source_playlist_id, 8)
 WHERE substr(source_playlist_id, 1, 7) = 'aether:';

-- ── i rapporti d'importazione ───────────────────────────────────────────────
-- Il `payload` si riscrive solo se è un JSON valido: un rapporto che non lo è
-- non si rilegge comunque (`rapporto` lo restituisce come assente), e farlo
-- fallire qui farebbe fallire l'intera migrazione per una riga che l'app già
-- ignora. Nella stessa istruzione, perché le espressioni di `SET` vedono la riga
-- com'era prima: il `source_id` di partenza serve a entrambe le colonne.
UPDATE OR IGNORE import_reports
   SET payload = CASE
                   WHEN json_valid(payload)
                   THEN json_set(payload, '$.sourceId', 'rymembra:' || substr(source_id, 8))
                   ELSE payload
                 END,
       source_id = 'rymembra:' || substr(source_id, 8)
 WHERE substr(source_id, 1, 7) = 'aether:';

DELETE FROM import_reports
 WHERE substr(source_id, 1, 7) = 'aether:';
