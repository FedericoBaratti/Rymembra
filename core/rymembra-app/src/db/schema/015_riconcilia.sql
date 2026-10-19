-- La riconciliazione impara a farsi una volta sola per riga.
--
-- `riconcilia` rimetteva in playlist ogni riga di `desiderati` con una
-- posizione, a ogni passata, per sempre: l'unica protezione era l'`OR IGNORE`
-- sulla chiave primaria `(playlist_id, position)`. Ma le posizioni scritte
-- all'importazione sono gli indici sparsi di Spotify, mentre togliere o
-- riordinare un brano le ricompatta a `0..n`: appena l'utente tocca la
-- playlist, le posizioni registrate non descrivono più la realtà, gli slot si
-- liberano, e i brani tolti ricompaiono — o si duplicano — alla passata dopo.
--
-- La colonna dice: questa riga la riconciliazione l'ha già considerata, e non
-- la considererà più. `NULL` significa «mai considerata». Si marca anche la
-- riga la cui posizione era occupata: perdere l'aggiunta è il male minore
-- rispetto a spostare un brano che l'utente aveva messo lì di proposito — è
-- la stessa scelta che `riconcilia` documenta da sempre.
ALTER TABLE desiderati ADD COLUMN placed_at INTEGER;

-- Le righe già chiuse sono righe che una passata ha già processato: nei
-- database esistenti non devono rientrare in playlist da cui l'utente le ha
-- magari già tolte. Il valore preciso non conta, conta che non sia NULL.
UPDATE desiderati SET placed_at = COALESCE(updated_at, 0)
 WHERE download_state = 'fatto';
