-- La memoria dei file che il lettore ha già rifiutato.
--
-- # Il guasto
--
-- Una scansione rilegge ogni file che il piano dà per nuovo o cambiato, e un
-- file che il lettore dei tag rifiuta non entra mai in libreria: alla passata
-- dopo è di nuovo «nuovo», e lo si riapre. Sul NAS misurato erano 186 file —
-- WMA che nessuno sa leggere, MP3 con dei frame ID3 scritti male — riaperti via
-- SMB a ogni scansione, ognuno con la sua scadenza addosso: secondi pagati ogni
-- volta per ottenere ogni volta lo stesso no.
--
-- # Cosa si ricorda, e perché proprio questo
--
-- Una riga per file rifiutato, con quel che basta a sapere che **è ancora lo
-- stesso rifiuto**: la chiave del percorso (`paths::path_key`, la forma con cui
-- la scansione confronta), la dimensione e la data di modifica che il file aveva,
-- e la versione del lettore che l'ha rifiutato (`metadata::VERSIONE_LETTORE`).
-- La scansione salta il file finché tutte e quattro coincidono. Cambia il file —
-- lo si ritagga, lo si sostituisce — e si riprova; cambia il lettore — una
-- versione che sa leggere quel che prima non sapeva — e si riprova. «Rileggi
-- tutti i tag» svuota la tabella, e si riprova tutto.
--
-- Causa e ora sono per chi guarda: perché quel file non c'è, e da quando.
--
-- # Cosa non si ricorda mai
--
-- Solo i rifiuti **deterministici** entrano qui: il lettore ha avuto i byte e
-- ha detto di no (`metadata.tagReadFailed`), o il formato non è di quelli che si
-- sanno suonare (`playback.formatUnsupported`). Una scadenza, una rete che cade,
-- un permesso negato, un lettore che si schianta non dicono niente del file, e
-- ricordarli vorrebbe dire nascondere per sempre un brano buono che quel giorno
-- non è arrivato. Lo decide `library.rs`, che sa distinguere i casi.
--
-- # Chiave primaria sul percorso
--
-- Un file ha un rifiuto solo, l'ultimo: un nuovo no su un file cambiato
-- sostituisce quello vecchio, e un sì lo cancella. Le righe di file che non ci
-- sono più si potano a ogni scansione che guarda la loro cartella.
--
-- # Tornare indietro
--
-- È una cache: toglierla a mano costa una scansione che riapre i rifiutati una
-- volta, e nient'altro.

CREATE TABLE file_illeggibili (
  chiave        TEXT    PRIMARY KEY,
  dimensione    INTEGER NOT NULL,
  modificato_ms INTEGER NOT NULL,
  versione      INTEGER NOT NULL,
  causa         TEXT    NOT NULL,
  visto_ms      INTEGER NOT NULL
);
