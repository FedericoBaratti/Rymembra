-- I dispositivi collegati via internet (`rymembra-lontano`), e gli ascolti
-- fatti qui dei loro brani.
--
-- # `dispositivi`
--
-- Una riga per dispositivo associato col QR: la sua chiave **pubblica**
-- ed25519 (64 cifre esadecimali minuscole, la forma con cui iroh la scrive),
-- il nome con cui lo si mostra, la piattaforma, l'identificativo che ha nella
-- sincronia (`dispositivo`, per unire le voci della pagina «Dispositivi») e la
-- sua chiave di «Ascolta insieme», se l'ha data. `indirizzi` è un JSON
-- `["ip:porta", …]`: gli ultimi visti, i primi da provare. La chiave segreta di
-- **questo** dispositivo non c'è e non ci deve stare: sta nel portachiavi.
--
-- `revocato` tiene la riga senza fidarsene più: un dispositivo dimenticato
-- dall'altra parte, o tolto da qui mentre era spento.
--
-- # `dispositivi_ascolti`
--
-- Un ascolto fatto qui di un brano di un altro dispositivo, da contare
-- **là**: è lui il proprietario del brano, delle statistiche e dello scrobble.
-- Resta finché non è arrivato (`inviato`), e `chiave` lo rende innocuo se
-- arriva due volte. Dall'altra parte `dispositivi_ricevuti` ricorda le chiavi
-- già contate.
--
-- # Fatti di questa macchina
--
-- Come `insieme_pari`: nessuna di queste tabelle viaggia nel profilo, nel
-- backup o nella sincronia, che portano solo quel che elencano. Un'associazione
-- è una fiducia data da questo dispositivo a quella chiave.
--
-- Nessuna chiave esterna: `dispositivi_ascolti.brano` è un id della libreria
-- **dell'altro**, non di questa.
--
-- # Tornare indietro
--
-- `DROP TABLE dispositivi_ricevuti; DROP TABLE dispositivi_ascolti;
-- DROP TABLE dispositivi; PRAGMA user_version = 38;`. Si perdono i
-- collegamenti (si rifanno col QR) e gli ascolti non ancora arrivati.

CREATE TABLE dispositivi (
  id             INTEGER PRIMARY KEY,
  chiave         TEXT    NOT NULL UNIQUE
                         CHECK (length(chiave) = 64 AND chiave NOT GLOB '*[^0-9a-f]*'),
  nome           TEXT    NOT NULL,
  piattaforma    TEXT    NOT NULL DEFAULT '',
  dispositivo    TEXT    NOT NULL DEFAULT '',
  chiave_insieme TEXT,
  indirizzi      TEXT    NOT NULL DEFAULT '[]',
  associato_ms   INTEGER NOT NULL,
  visto_ms       INTEGER,
  revocato       INTEGER NOT NULL DEFAULT 0 CHECK (revocato IN (0, 1))
);

CREATE TABLE dispositivi_ascolti (
  id           INTEGER PRIMARY KEY,
  dispositivo  TEXT    NOT NULL,
  brano        INTEGER NOT NULL,
  quando_ms    INTEGER NOT NULL,
  ascoltato_ms INTEGER NOT NULL,
  chiave       TEXT    NOT NULL UNIQUE,
  inviato      INTEGER NOT NULL DEFAULT 0 CHECK (inviato IN (0, 1))
);

CREATE INDEX dispositivi_ascolti_da_inviare
  ON dispositivi_ascolti (dispositivo) WHERE inviato = 0;

CREATE TABLE dispositivi_ricevuti (
  chiave      TEXT    PRIMARY KEY,
  ricevuto_ms INTEGER NOT NULL
) WITHOUT ROWID;
