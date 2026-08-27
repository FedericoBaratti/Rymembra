-- Le risposte del catalogo che si erano fermate al testo piatto.
--
-- `aether_meta::lrclib::cerca` faceva due domande e si fermava alla prima che
-- rispondeva. Ma LRCLIB tiene **più voci per lo stesso brano** — una per ogni
-- edizione che qualcuno ha caricato — e `/api/get` ne restituisce una sola:
-- quella la cui firma coincide con la domanda. Non è detto che sia quella con i
-- tempi, e quando non lo era la ricerca finiva lì, con un testo che non scorre
-- e l'etichetta «senza tempi» addosso a un brano di cui il catalogo l'LRC ce
-- l'aveva — su un'altra voce, a una richiesta di distanza.
--
-- Adesso la seconda domanda si fa comunque. Restano però in tabella le risposte
-- di prima: righe con `plain` pieno, `synced` a NULL e `checked_at` scritto, che
-- nessuno rimetterebbe più in coda proprio perché la domanda risulta già fatta.
-- Sono quelle che questa migrazione rimanda a chiedere.
--
-- `checked_at = NULL` e non `DELETE`: il testo piatto è quel che si mostra
-- intanto, ed è tutto quel che si ha se il portatile è staccato dalla rete o se
-- l'interruttore dei testi è spento. Cancellare la riga per riscriverla identica
-- un minuto dopo vorrebbe dire, in quel minuto, un pannello vuoto.
--
-- `source = 'lrclib'` restringe a quel che ha risposto il catalogo: il sidecar,
-- il tag e la sincronizzazione a mano non c'entrano — nessuno dei tre ha mai
-- scritto `checked_at`, e nessuno dei tre va rimesso in discussione da qui.
UPDATE lyrics
   SET checked_at = NULL
 WHERE source = 'lrclib'
   AND synced IS NULL
   AND instrumental = 0;

-- L'indice diceva «le righe senza testo», che era la definizione di «da
-- cercare» di allora. Adesso quel che manca a un brano sono i **tempi**, non il
-- testo: una riga con il solo piatto è una domanda ancora aperta, e con il
-- vecchio predicato parziale non sarebbe stata indicizzata.
DROP INDEX IF EXISTS idx_lyrics_da_cercare;
CREATE INDEX idx_lyrics_da_cercare ON lyrics(checked_at)
  WHERE synced IS NULL AND instrumental = 0;
