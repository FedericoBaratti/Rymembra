-- I testi, e i tempi con cui scorrono.
--
-- `tracks.lyrics` esiste dalla migrazione 1 e non basta per due ragioni, e
-- nessuna delle due si risolve allargandola.
--
-- La prima è che è una colonna della **riga di un file**. Chi ha lo stesso
-- brano in FLAC e in mp3 ha due righe, e sincronizzare il testo a mano una volta
-- dovrebbe valere per entrambe — mentre spostare il file in un'altra cartella
-- non dovrebbe far perdere niente. La chiave giusta è quindi `track_key`, che è
-- già l'identità che attraversa la sincronizzazione fra dispositivi e che si
-- ricalcola a ogni scrittura dei tag.
--
-- La seconda è che una colonna sola non può dire **da dove viene** quel testo,
-- e da dove viene decide cosa se ne può fare: quello scritto a mano non si
-- sovrascrive mai, quello scaricato si può buttare e riprendere, e quello che
-- viene dal tag di un file è di chi ha taggato il file.
--
-- `tracks.lyrics` resta dov'è e continua a riempirsi dal tag durante la
-- scansione: è la fonte numero due della catena, non una copia di questa
-- tabella.

CREATE TABLE lyrics (
  -- L'identità del brano, non della riga: vedi sopra.
  track_key    TEXT    PRIMARY KEY,

  -- Il testo senza tempi. C'è anche quando `synced` c'è: è quel che si mostra
  -- se l'aderenza alla durata risulta sbagliata, ed è da qui che parte l'editor
  -- di sincronizzazione.
  plain        TEXT,

  -- L'LRC così com'è, byte per byte come sta nel file accanto alla musica. Non
  -- una forma interpretata: il giorno che l'interpretazione cambia — un tag in
  -- più che oggi si ignora — un testo già letto non deve essere già perduto.
  synced       TEXT,

  -- Da dove viene: 'sidecar' | 'tag' | 'lrclib' | 'mano'.
  -- Testo e non un intero: una riga di database che si legge senza una tabella
  -- di traduzione a fianco è una riga che si può diagnosticare alle undici di
  -- sera, e questi valori li scrive solo il nostro codice.
  source       TEXT    NOT NULL,

  -- L'identificativo nel catalogo remoto, quando è di là che è arrivato. Serve
  -- a non richiedere quel che si ha già e a poter dire da quale voce esatta.
  lrclib_id    INTEGER,

  -- La correzione dell'utente, nel verso dello standard LRC: positivo anticipa
  -- il testo. Si somma a quello che il file dichiara nel proprio `[offset:]` —
  -- vedi `aether_domain::testo::posizione_corretta`, che è l'unico posto in cui
  -- i due si sommano.
  offset_ms    INTEGER NOT NULL DEFAULT 0,

  -- Il brano non ha parole. È una **risposta**, non l'assenza di una: senza
  -- questa colonna uno strumentale resterebbe per sempre nella coda di quelli
  -- da cercare, e a ogni passata si tornerebbe a chiedere alla rete il testo di
  -- qualcosa che il testo non ce l'ha.
  instrumental INTEGER NOT NULL DEFAULT 0 CHECK (instrumental IN (0, 1)),

  -- La durata a cui questi tempi si riferiscono, in millisecondi.
  --
  -- È la colonna che permette di accorgersi che il testo è giusto ma
  -- l'edizione è un'altra: un remaster che dura venti secondi in più fa scorrere
  -- tutto in ritardo crescente, e senza un termine di paragone il sintomo si
  -- attribuisce al lettore invece che al testo.
  duration_ms  INTEGER,

  -- Quando questa riga è stata scritta l'ultima volta.
  updated_at   INTEGER NOT NULL,

  -- Quando si è chiesto alla rete, che la risposta sia stata sì o no.
  --
  -- Separata da `updated_at` perché risponde a un'altra domanda: `updated_at`
  -- dice quanto è vecchio il testo, questa dice se vale la pena richiederlo. Una
  -- riga con `plain` e `synced` a NULL e `checked_at` pieno è la memoria di un
  -- «non ce l'ho», ed è quel che impedisce alla passata di ricominciare ogni
  -- volta dai brani che nessuno conosce.
  checked_at   INTEGER
);

-- Chi ha un testo e chi no, per la copertura e per la coda della passata.
-- Parziale: le righe che interessano a quella domanda sono quelle **senza**
-- testo, e un indice su tutta la tabella costerebbe quanto la tabella per
-- rispondere a una query che ne guarda una frazione.
CREATE INDEX idx_lyrics_da_cercare ON lyrics(checked_at)
  WHERE synced IS NULL AND plain IS NULL AND instrumental = 0;
