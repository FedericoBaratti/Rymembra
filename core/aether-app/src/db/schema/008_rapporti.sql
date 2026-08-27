-- Il rapporto di un'importazione sopravvive alla finestrella che lo mostrava.
--
-- Fin qui il rapporto viveva quanto una modale. La finestrella si chiude **da
-- sé** alla conferma — deliberatamente: il gesto da rendere facile era il link
-- successivo — e con lei spariva l'elenco dei brani che l'utente ha sul servizio
-- e non su questo disco. Quell'elenco è l'unica cosa dell'importazione che non
-- si può ricostruire dopo: i conteggi si ricalcolano, la playlist è in libreria,
-- la coda è in `desiderati`, ma «quali brani non avevi» esiste solo lì.
--
-- Chi chiudeva senza copiarli li perdeva, e non aveva modo di sapere che li
-- stava perdendo.

-- ── la tabella ──────────────────────────────────────────────────────────────
-- Una riga per importazione, la chiave è `source_id`: la stessa che
-- `desiderati.source_id` porta su ogni riga della coda, ed è così che la pagina
-- delle importazioni ritrova il rapporto della riga che si sta guardando.
--
-- Reimportare lo stesso link sovrascrive, e va bene: la seconda importazione
-- **è** la verità aggiornata di quel contenuto, e tenere due rapporti dello
-- stesso `source_id` vorrebbe dire dover scegliere quale mostrare.
--
-- # Perché JSON in una colonna e non venti colonne
--
-- Perché il rapporto ha ventidue campi, di cui uno è un elenco di brani a
-- lunghezza variabile: normalizzarlo sarebbe una tabella di venti colonne più
-- una seconda tabella per i mancanti, e nessuna interrogazione lo cercherebbe
-- mai per uno di quei campi — si legge sempre per `source_id`, intero.
--
-- E soprattutto: la forma salvata è **la stessa** che la conferma restituisce
-- alla finestra, serializzata dallo stesso `RapportoImport`. Un rapporto salvato
-- in una forma diversa da quello mostrato sono due verità da tenere allineate, e
-- la seconda comincia a mentire al primo campo aggiunto.
--
-- Il precedente in casa è `ui.shortcuts`, che tiene le associazioni da tastiera
-- nello stesso modo e per la stessa ragione.
CREATE TABLE import_reports (
  source_id TEXT PRIMARY KEY,
  -- Quando è stato scritto, in millisecondi. Serve a due cose: mettere in cima
  -- il più recente, e — il giorno in cui questa tabella dovesse essere potata —
  -- sapere quali togliere per primi.
  saved_at  INTEGER NOT NULL,
  -- Il `RapportoImport`, come lo vede l'IPC.
  payload   TEXT NOT NULL
);

-- Per l'elenco delle importazioni concluse: si chiede sempre «le ultime N», mai
-- «tutte». Senza indice sarebbe una scansione della tabella a ogni apertura
-- della pagina — trascurabile con cinquanta righe, non con cinquemila.
CREATE INDEX idx_import_reports_quando ON import_reports(saved_at DESC);
