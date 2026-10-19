-- I testi seguono il brano quando la sua identità cambia.
--
-- # Il guasto
--
-- `lyrics` è chiavata su `track_key` (vedi `011_testi.sql`, e il perché è
-- giusto: lo stesso brano in FLAC e in mp3 ha un testo solo). Ma `track_key` si
-- ricalcola da artista, titolo e album, e quei tre cambiano: lo riscrive
-- l'arricchimento quando trova il nome vero, lo riscrive la conferma di un
-- incerto, lo riscrive la scansione quando i tag del file cambiano. Nessuno di
-- quei posti toccava `lyrics`, quindi a ogni correzione il testo restava sotto
-- la chiave vecchia: il testo sincronizzato a mano, lo scarto che si era
-- regolato a orecchio, la risposta del catalogo — tutto orfano, e il brano
-- corretto tornava a mostrare un pannello vuoto.
--
-- # Perché un trigger, e non una riga in ciascuno di quei posti
--
-- Perché i posti sono cinque, sparsi in tre moduli, e il sesto arriverà: una
-- regola che deve valere per ogni scrittura di `track_key` sta dove passano
-- tutte le scritture. Il giorno in cui qualcuno aggiunge un sesto `UPDATE`,
-- questa regola vale anche per lui senza che debba saperlo.
--
-- # Quando si sposta, e quando no
--
-- * Solo se **nessun altro brano** porta ancora la chiave vecchia: due file dello
--   stesso brano condividono il testo, e correggere i tag di uno solo non deve
--   portarlo via all'altro. Quando anche il secondo verrà corretto, il suo
--   `UPDATE` troverà la chiave libera e sposterà la riga.
-- * Solo le righe che **portano qualcosa**: un testo, uno strumentale, uno
--   scarto, o la firma di chi l'ha scritto a mano. Una riga vuota è soltanto la
--   memoria di «il catalogo non conosceva quel brano» — e «quel brano» era il
--   nome sbagliato: spostarla vorrebbe dire non cercare il nome giusto per due
--   settimane.
-- * Se la chiave nuova ha già una riga **vuota**, quella lascia il posto. Se ne
--   ha una **piena**, non si tocca niente: fra due testi non si sceglie da qui, e
--   quello vecchio resta dov'è invece di sparire.
--
-- # Quel che il trigger chiede a chi scrive `track_key`
--
-- Di cambiarla **una volta**, per davvero. Un giro di andata e ritorno nella
-- stessa transazione — i tag del file, poi la correzione dell'utente sopra —
-- non è neutro: all'andata il testo segue, al ritorno può restare dov'è, se
-- un'altra copia del brano porta la chiave di mezzo. Chi rifà la riga dai tag
-- lascia quindi stare la chiave dei brani corretti: vedi
-- `incerti::riapplica_sopra_i_tag`.
--
-- # Tornare indietro
--
-- Una versione precedente rifiuta un database a questa versione, come per ogni
-- migrazione: serve il backup di prima dell'aggiornamento.

CREATE TRIGGER lyrics_segue_track_key
AFTER UPDATE OF track_key ON tracks
WHEN OLD.track_key <> NEW.track_key
  AND NOT EXISTS (SELECT 1 FROM tracks WHERE track_key = OLD.track_key)
BEGIN
  DELETE FROM lyrics
   WHERE track_key = NEW.track_key
     AND plain IS NULL AND synced IS NULL
     AND instrumental = 0 AND offset_ms = 0 AND source <> 'mano'
     AND EXISTS (
       SELECT 1 FROM lyrics
        WHERE track_key = OLD.track_key
          AND (plain IS NOT NULL OR synced IS NOT NULL OR instrumental <> 0
               OR offset_ms <> 0 OR source = 'mano')
     );

  UPDATE lyrics
     SET track_key = NEW.track_key
   WHERE track_key = OLD.track_key
     AND (plain IS NOT NULL OR synced IS NOT NULL OR instrumental <> 0
          OR offset_ms <> 0 OR source = 'mano')
     AND NOT EXISTS (SELECT 1 FROM lyrics WHERE track_key = NEW.track_key);
END;
