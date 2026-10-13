-- Quanto forte suona ogni brano: l'intensità percepita e il picco vero,
-- misurati dal programma quando i tag non li dicono.
--
-- # Il guasto
--
-- La normalizzazione della 2.x leggeva `replaygain_track_db` e, senza tag, non
-- correggeva niente. Nelle librerie vere i tag ReplayGain sono l'eccezione:
-- chi non ha mai passato la collezione in un programma apposito ha la colonna
-- vuota ovunque, e l'interruttore «Normalizza il volume» suonava identico
-- acceso e spento. Un disco rimasterizzato nel 2010 arrivava dieci decibel
-- sopra uno del 1975, nella stessa coda.
--
-- # Cosa si conserva
--
-- La misura di EBU R128 (`aether_play::intensita`): l'intensità integrata in
-- LUFS, il picco vero lineare, e quanto audio si è misurato — che serve a
-- mettere insieme un album pesando i brani lunghi più di quelli corti. Il
-- guadagno **non** si conserva: è `-18 - lufs`, e ricavarlo alla lettura vuol
-- dire che un cambio di riferimento non chiede di rimisurare niente.
--
-- Il valore dell'album non ha una tabella: si calcola quando serve dai brani
-- con la stessa `album_key`, che sono una ventina di righe lette per chiave
-- primaria. Una tabella per gli album andrebbe tenuta in passo con ogni brano
-- che entra, esce o cambia disco, e sarebbe una seconda verità sulla stessa
-- cosa.
--
-- # Perché una tabella a parte, e non tre colonne di `tracks`
--
-- Per le stesse ragioni di `track_impronta` (017): la misura è di un'altra
-- passata, con un'altra cadenza e i suoi esiti falliti da ritentare, e le
-- colonne in `tracks` vorrebbero dire toccare la tabella più letta del
-- database a ogni brano misurato. E perché la 026 ha mostrato cosa costa
-- ricostruire `tracks`: qui non serve.
--
-- # La riga non si invalida quando il file cambia data
--
-- Esattamente come l'impronta, e la ragione è scritta per esteso nella 017:
-- scrivere un tag cambia la data di modifica, e i tag non cambiano il suono.
-- Solo `versione` rimette in coda — il giorno in cui la misura cambiasse metro.
--
-- # Tornare indietro
--
-- Una versione precedente rifiuta un database a questa versione con
-- `db.versionAhead`, e serve il backup di prima dell'aggiornamento — o la
-- copia che l'apertura scrive prima di migrare. Non si perde niente che non
-- si ricostruisca: le misure si rifanno da sole alla prossima passata.
-- Togliere a mano la tabella e il suo indice riporta esattamente la 028.

CREATE TABLE track_intensita (
  track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,

  -- Quale versione della misura ha prodotto questi numeri
  -- (`aether_play::intensita::VERSIONE`).
  versione INTEGER NOT NULL,

  -- L'intensità integrata, in LUFS. NULL quando la misura non è riuscita.
  lufs REAL,

  -- Il picco vero, lineare: 1.0 è il fondo scala, e un master moderno lo
  -- supera. NULL quando la misura non è riuscita.
  picco REAL,

  -- Quanto audio si è misurato, in millisecondi. NULL con la misura.
  durata_ms INTEGER,

  -- 'ok'          — ci sono i numeri.
  -- 'illeggibile' — il file non si decodifica. Si ritenta fra un mese.
  -- 'muto'        — niente sopra la soglia di −70 LUFS. Terminale.
  --
  -- Niente 'rete', come nella 017: una share caduta non è una proprietà del
  -- brano, e la passata si ferma senza scrivere.
  esito TEXT NOT NULL,

  -- Quando si è provato, riuscendo o no.
  tentato_at INTEGER NOT NULL
);

-- I falliti maturi per un altro tentativo, come `idx_track_impronta_falliti`.
CREATE INDEX idx_track_intensita_falliti
    ON track_intensita(tentato_at)
    WHERE lufs IS NULL;
