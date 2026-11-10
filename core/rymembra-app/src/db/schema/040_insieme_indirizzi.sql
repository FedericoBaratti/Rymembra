-- L'ultimo indirizzo a cui un PC fidato di «Ascolta insieme» ha risposto.
--
-- # Perché
--
-- La scoperta di «Ascolta insieme» è un broadcast sulla rete di casa, più una
-- domanda a ogni indirizzo scritto a mano. Sulle reti che il broadcast non lo
-- passano — molte Wi-Fi per gli ospiti, le reti con l'isolamento dei client,
-- un ufficio con due sottoreti — e per un host che ha dovuto prendere una
-- porta fuori da 47811–47820, il PC fidato di ieri oggi non si ritrova, finché
-- qualcuno non ne scrive l'indirizzo. Qui si ricorda da sé: l'`ip:porta`
-- dell'ultima connessione di controllo riuscita verso di lui, e la scoperta gli
-- manda anche lì la sua domanda, come a un indirizzo scritto a mano.
--
-- Un indirizzo ricordato non è una fiducia: chi risponde lì deve avere la
-- chiave del PC fidato, o la connessione si chiude (`collega_atteso`). E un
-- indirizzo vecchio, che adesso è di un altro, riceve al più una domanda di
-- quaranta byte senza niente di chi la manda.
--
-- # Una tabella e non una colonna
--
-- Per la chiave, come `insieme_pari`, e da sola: tornare indietro è un `DROP
-- TABLE`, e `insieme_pari` resta com'era riga per riga. «Dimentica» toglie
-- anche l'indirizzo (`rymembra_app::insieme::dimentica`).
--
-- # Fatti di questa macchina
--
-- Come `insieme_pari`: non viaggia nel profilo, nel backup o nella sincronia.
--
-- # Tornare indietro
--
-- `DROP TABLE insieme_indirizzi; PRAGMA user_version = 39;`. Si perdono gli
-- indirizzi ricordati, che tornano alla prima connessione.

CREATE TABLE insieme_indirizzi (
  chiave     TEXT    PRIMARY KEY
                     CHECK (length(chiave) = 64 AND chiave NOT GLOB '*[^0-9a-f]*'),
  indirizzo  TEXT    NOT NULL,
  visto_ms   INTEGER NOT NULL
) WITHOUT ROWID;
