//! Le impronte sonore della libreria: misurarle, conservarle, rileggerle.
//!
//! I numeri li produce [`aether_play::impronta`], il confronto lo fa
//! [`aether_domain::affinita`]: qui in mezzo c'è tutto il resto — quali brani
//! mancano, in che ordine, cosa si scrive quando la misura riesce e cosa quando
//! no, e come si rilegge una scala da mettere in mano a chi deve ordinare dei
//! candidati in un millisecondo.
//!
//! # Le tre fasi, come nell'arricchimento
//!
//! Ricalca [`crate::enrich`], che è il modello di casa e funziona:
//!
//! ```text
//! candidati()  ──►  un lotto di schede            (lucchetto preso)
//! misura()     ──►  apre, cerca, decodifica       (SENZA lucchetto)
//! registra()   ──►  le righe, in una transazione  (lucchetto preso)
//! ```
//!
//! Tre e non quattro: manca `scrivi_file`, perché **questa passata non tocca
//! mai i file dell'utente**. È la differenza che conta rispetto
//! all'arricchimento, e ne discende tutto il resto: non serve un giornale di
//! annullamento, non serve fotografare lo stato di prima, non serve un comando
//! «annulla». L'intestazione di `arricchimento.rs` esiste per giustificare una
//! passata che scrive dentro i file di qualcun altro; questa non ha niente da
//! giustificare.
//!
//! # La fase di mezzo è lenta e non tiene niente in mano
//!
//! [`misura`] apre un file: su una condivisione morta sono i quaranta secondi di
//! Windows. Non prende il lucchetto della libreria e non lo deve prendere — e
//! chi la chiama deve metterle addosso una scadenza, come fa `brano_di` in
//! `riproduzione.rs`. La scadenza non sta qui dentro per la stessa ragione per
//! cui non sta dentro [`crate::playback::sorgente_da_scheda`]: è una decisione
//! di chi ha un orologio e un filo da spendere, non di chi apre.
//!
//! # La rete che cade non è una proprietà del brano
//!
//! [`Misurato::NonRaggiungibile`] è una terza risposta accanto a «fatta» e
//! «negata», e la ragione è la stessa scritta in `enrich::Decisione`: segnare
//! illeggibili i brani incontrati mentre il portatile era fuori dalla wifi
//! vorrebbe dire non riprovarli per un mese. Quando arriva, il lotto si
//! interrompe e per quel brano **non si scrive niente**.

use std::collections::HashMap;
use std::collections::HashSet;

use rusqlite::{Connection, Transaction};

use aether_domain::affinita::Scala;
use aether_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use aether_play::impronta::{self, Esito};

use crate::files::MusicFiles;
use crate::playback::{SchedaSorgente, sorgente_da_scheda};

/// Quanti brani per passata.
///
/// Sessantaquattro, contro i dodici dell'arricchimento, e la differenza è tutta
/// nel cancello che qui non c'è: là ogni gruppo d'album costa da due a quattro
/// richieste a un servizio che ne concede una al secondo, qui ogni brano costa
/// una lettura di tre megabyte e mezzo che non chiede permesso a nessuno.
/// Sessantaquattro brani sono una decina di secondi, che è il lavoro che si
/// accetta di perdere se l'applicazione viene chiusa a metà lotto.
pub const LOTTO: usize = 64;

/// Ogni quanto si ritenta un brano che non si è potuto misurare.
///
/// Un mese, come `RITENTA_NESSUNO_MS` dell'arricchimento: un file che non si
/// decodifica è una proprietà del file, e le proprietà dei file cambiano piano.
/// Chi ricodifica un brano passa comunque da una scansione, che è l'altro modo
/// in cui una riga torna in coda.
pub const RITENTA_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Sotto quanti brani con impronta non si calcola una scala.
///
/// È la soglia del dominio, ripetuta qui solo per comodità di chi legge:
/// [`aether_domain::affinita::MINIMO_BRANI`].
pub const MINIMO_BRANI: usize = aether_domain::affinita::MINIMO_BRANI;

/// Cos'è successo provando a misurare un brano.
#[derive(Debug, Clone, PartialEq)]
pub enum Misurato {
    /// I numeri.
    Fatta {
        /// Quale brano.
        track_id: i64,
        /// L'impronta grezza, non ancora normalizzata.
        vettore: Vec<f32>,
    },
    /// Il file c'è e una misura non se ne ricava. Si ricorda, e secondo il caso
    /// si ritenta fra un mese o mai più.
    Negata {
        /// Quale brano.
        track_id: i64,
        /// Perché: `illeggibile`, `muto` o `corto`.
        esito: &'static str,
    },
    /// La rete non risponde. **Non si scrive niente e il lotto si ferma.**
    NonRaggiungibile,
}

/// Il file non si è potuto decodificare. Si ritenta fra [`RITENTA_MS`].
pub const ILLEGGIBILE: &str = "illeggibile";
/// Trenta secondi di silenzio. Terminale.
pub const MUTO: &str = "muto";
/// Troppo poco audio perché i numeri dicano qualcosa. Si ritenta.
pub const CORTO: &str = "corto";
/// C'è un vettore.
pub const OK: &str = "ok";

/// Quali brani mancano, e a che condizioni ci si torna.
///
/// Una funzione sola perché la stessa clausola serve a [`candidati`] e a
/// [`quanti_mancano`], e due copie divergono — è la ragione per cui
/// `enrich::candidato_where` esiste.
///
/// `'muto'` non compare fra i ritentabili apposta: trenta secondi di silenzio
/// oggi saranno trenta secondi di silenzio fra un mese, e un'impronta di
/// silenzio sarebbe una calamita che attira ogni ricerca di somiglianza.
const fn candidato_where() -> &'static str {
    "i.track_id IS NULL
     OR i.versione <> ?1
     OR (i.vettore IS NULL AND i.esito <> 'muto' AND i.tentato_at < ?2)"
}

/// Quanti brani aspettano ancora un'impronta.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn quanti_mancano(connection: &Connection, adesso: i64) -> Result<i64, AppError> {
    let sql = format!(
        "SELECT COUNT(*) FROM tracks t
         LEFT JOIN track_impronta i ON i.track_id = t.id
         WHERE {}",
        candidato_where()
    );
    connection
        .query_row(
            &sql,
            rusqlite::params![versione(), scadenza(adesso)],
            |row| row.get(0),
        )
        .map_err(|err| db_error("conteggio dei brani da analizzare", &err))
}

/// I prossimi brani da misurare.
///
/// I più recenti per primi, come l'arricchimento e per la stessa ragione: chi ha
/// appena importato un disco vuole che la radio lo conosca, non che conosca
/// meglio qualcosa che ha in libreria da due anni.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn candidati(
    connection: &Connection,
    adesso: i64,
    quanti: usize,
) -> Result<Vec<SchedaSorgente>, AppError> {
    let sql = format!(
        "SELECT t.id, t.path, t.duration_ms, t.replaygain_track_db
         FROM tracks t
         LEFT JOIN track_impronta i ON i.track_id = t.id
         WHERE {}
         ORDER BY t.date_added DESC
         LIMIT ?3",
        candidato_where()
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| db_error("elenco dei brani da analizzare", &err))?;
    let righe = statement
        .query_map(
            rusqlite::params![
                versione(),
                scadenza(adesso),
                i64::try_from(quanti).unwrap_or(i64::MAX),
            ],
            |row| {
                Ok(SchedaSorgente {
                    track_id: row.get(0)?,
                    path: row.get(1)?,
                    durata_ms: row.get(2)?,
                    replaygain_db: row.get(3)?,
                })
            },
        )
        .map_err(|err| db_error("elenco dei brani da analizzare", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei brani da analizzare", &err))
}

/// Misura **un** brano. Non tocca il database.
///
/// Un brano alla volta e non un lotto, ed è una scelta di firma: è ciò che
/// permette a chi chiama di guardare fra l'uno e l'altro se il filo che prepara
/// il brano successivo sta aprendo qualcosa, e di fermarsi lì invece che a metà
/// di una decodifica. Fermarsi a metà lascerebbe un file aperto per tutta la
/// durata dell'attesa, che è il contrario di quel che serve.
///
/// Non restituisce `Result`: ogni modo in cui può andare storto è già uno dei
/// tre esiti, e un chiamante che dovesse distinguere un errore da un esito
/// finirebbe per riscrivere qui la stessa classificazione.
#[must_use]
pub fn misura(files: &dyn MusicFiles, scheda: &SchedaSorgente) -> Misurato {
    let sorgente = match sorgente_da_scheda(files, scheda) {
        Ok(sorgente) => sorgente,
        Err(err) => return da_errore(scheda.track_id, &err),
    };
    let durata = u64::try_from(scheda.durata_ms).unwrap_or(0);
    match impronta::calcola(sorgente, durata) {
        Ok(Esito::Fatta(vettore)) => Misurato::Fatta {
            track_id: scheda.track_id,
            vettore: vettore.to_vec(),
        },
        Ok(Esito::Muto) => Misurato::Negata {
            track_id: scheda.track_id,
            esito: MUTO,
        },
        Ok(Esito::Corto) => Misurato::Negata {
            track_id: scheda.track_id,
            esito: CORTO,
        },
        Err(err) => da_errore(scheda.track_id, &err),
    }
}

/// Come si legge un guasto: se sotto c'è la rete, non è colpa del brano.
fn da_errore(track_id: i64, err: &AppError) -> Misurato {
    if err.code().kind() == ErrorCodeKind::FsNetworkUnavailable {
        return Misurato::NonRaggiungibile;
    }
    Misurato::Negata {
        track_id,
        esito: ILLEGGIBILE,
    }
}

/// Scrive nel database quel che si è misurato.
///
/// Restituisce quante righe sono state toccate. [`Misurato::NonRaggiungibile`]
/// non ne tocca nessuna: è il suo intero significato.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn registra(
    tx: &Transaction<'_>,
    misurati: &[Misurato],
    adesso: i64,
) -> Result<usize, AppError> {
    let mut statement = tx
        .prepare_cached(
            "INSERT INTO track_impronta (track_id, versione, vettore, esito, tentato_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(track_id) DO UPDATE SET
               versione   = excluded.versione,
               vettore    = excluded.vettore,
               esito      = excluded.esito,
               tentato_at = excluded.tentato_at",
        )
        .map_err(|err| db_error("scrittura di un'impronta", &err))?;

    let mut scritte = 0_usize;
    for misurato in misurati {
        let (track_id, vettore, esito) = match misurato {
            Misurato::Fatta { track_id, vettore } => (*track_id, Some(in_byte(vettore)), OK),
            Misurato::Negata { track_id, esito } => (*track_id, None, *esito),
            // Non è successo niente che riguardi questo brano: la rete è caduta
            // sotto, e domani il file sarà quello di sempre.
            Misurato::NonRaggiungibile => continue,
        };
        statement
            .execute(rusqlite::params![
                track_id,
                versione(),
                vettore,
                esito,
                adesso
            ])
            .map_err(|err| db_error("scrittura di un'impronta", &err))?;
        scritte = scritte.saturating_add(1);
    }
    Ok(scritte)
}

/// Ricalcola medie e scarti su tutta la libreria e li conserva.
///
/// `None` se le impronte non bastano ancora a descrivere una libreria: sotto
/// [`MINIMO_BRANI`] uno scarto quadratico medio descrive il campione, non la
/// collezione, e normalizzarci sopra farebbe sembrare enormi delle differenze
/// che sono solo il numero dei brani.
///
/// Va chiamata **a fine passata**, mai dentro il ciclo: legge tutte le impronte,
/// e farlo dopo ogni lotto significherebbe rileggere l'intera libreria una volta
/// ogni sessantaquattro brani.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn ritara(tx: &Transaction<'_>, adesso: i64) -> Result<Option<usize>, AppError> {
    let vettori = tutte_le_impronte(tx)?;
    if vettori.len() < MINIMO_BRANI {
        return Ok(None);
    }
    let fette: Vec<&[f32]> = vettori.iter().map(Vec::as_slice).collect();
    let Some(scala) = Scala::misura(fette) else {
        return Ok(None);
    };

    tx.execute(
        "INSERT INTO impronta_scala (versione, medie, scarti, brani, calcolata_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(versione) DO UPDATE SET
           medie = excluded.medie,
           scarti = excluded.scarti,
           brani = excluded.brani,
           calcolata_at = excluded.calcolata_at",
        rusqlite::params![
            versione(),
            in_byte(scala.medie()),
            in_byte(scala.scarti()),
            i64::try_from(scala.brani()).unwrap_or(i64::MAX),
            adesso,
        ],
    )
    .map_err(|err| db_error("scrittura della scala", &err))?;
    Ok(Some(scala.brani()))
}

/// La scala corrente, se una passata ne ha già calcolata una per questa
/// versione dei descrittori.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn scala(connection: &Connection) -> Result<Option<Scala>, AppError> {
    let letta: Option<(Vec<u8>, Vec<u8>, i64)> = connection
        .query_row(
            "SELECT medie, scarti, brani FROM impronta_scala WHERE versione = ?1",
            [versione()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(db_error("lettura della scala", &altro)),
        })?;

    let Some((medie, scarti, brani)) = letta else {
        return Ok(None);
    };
    let (Some(medie), Some(scarti)) = (da_byte(&medie), da_byte(&scarti)) else {
        // Una scala illeggibile non è un guasto da propagare: la passata
        // successiva la riscrive, e nel frattempo chi chiede ripiega su quel che
        // faceva prima. È la stessa disciplina della coda che non si ricarica.
        return Ok(None);
    };
    Ok(Scala::da_parti(
        medie,
        scarti,
        usize::try_from(brani).unwrap_or(0),
    ))
}

/// Le impronte grezze di un pugno di brani.
///
/// Grezze e non normalizzate: la scala si legge una volta e si applica a tutti,
/// e passarla qui dentro vorrebbe dire leggerla a ogni chiamata.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn impronte(connection: &Connection, ids: &[i64]) -> Result<HashMap<i64, Vec<f32>>, AppError> {
    let mut fuori = HashMap::new();
    if ids.is_empty() {
        return Ok(fuori);
    }
    let mut statement = connection
        .prepare_cached(
            "SELECT vettore FROM track_impronta
             WHERE track_id = ?1 AND versione = ?2 AND vettore IS NOT NULL",
        )
        .map_err(|err| db_error("lettura di un'impronta", &err))?;

    // Uno alla volta e non con un `IN (…)`: la lista cambia lunghezza a ogni
    // chiamata, e una query costruita a pezzi non si può tenere in cache. Con
    // `prepare_cached` e la chiave primaria, duecento letture sono meno di un
    // millisecondo — e duecento è quanti ne chiede chi sta ordinando dei
    // candidati.
    let visti: HashSet<i64> = ids.iter().copied().collect();
    for id in visti {
        let letto: Option<Vec<u8>> = statement
            .query_row(rusqlite::params![id, versione()], |row| row.get(0))
            .map(Some)
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                altro => Err(db_error("lettura di un'impronta", &altro)),
            })?;
        if let Some(vettore) = letto.as_deref().and_then(da_byte) {
            fuori.insert(id, vettore);
        }
    }
    Ok(fuori)
}

/// Tutte le impronte valide, col brano a cui appartengono.
///
/// # Perché tutte insieme e non a lotti
///
/// Perché chi chiama raggruppa, e un raggruppamento su un campione non è il
/// raggruppamento della libreria: prendere la metà dei brani darebbe gruppi che
/// cambiano ogni volta per il solo fatto di aver guardato altre righe.
///
/// L'aritmetica dice che si può: quarantasette `f32` sono centottantotto byte,
/// e cinquantamila brani stanno in dieci megabyte — che è meno di quanto costa
/// una copertina aperta. `tetto` esiste comunque, perché una libreria dieci
/// volte più grande di così è una libreria che non ho mai visto e non voglio
/// scoprire con un esaurimento di memoria.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn tutte(connection: &Connection, tetto: usize) -> Result<Vec<(i64, Vec<f32>)>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT track_id, vettore FROM track_impronta
             WHERE versione = ?1 AND vettore IS NOT NULL
             ORDER BY track_id
             LIMIT ?2",
        )
        .map_err(|err| db_error("lettura delle impronte", &err))?;
    let righe = statement
        .query_map(
            rusqlite::params![versione(), i64::try_from(tetto).unwrap_or(i64::MAX)],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .map_err(|err| db_error("lettura delle impronte", &err))?;

    let mut fuori = Vec::new();
    for riga in righe {
        let (id, byte) = riga.map_err(|err| db_error("lettura delle impronte", &err))?;
        if let Some(vettore) = da_byte(&byte) {
            fuori.push((id, vettore));
        }
    }
    Ok(fuori)
}

/// Tutte le impronte valide di questa versione.
fn tutte_le_impronte(connection: &Connection) -> Result<Vec<Vec<f32>>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT vettore FROM track_impronta
             WHERE versione = ?1 AND vettore IS NOT NULL",
        )
        .map_err(|err| db_error("lettura delle impronte", &err))?;
    let righe = statement
        .query_map([versione()], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|err| db_error("lettura delle impronte", &err))?;

    let mut fuori = Vec::new();
    for riga in righe {
        let byte = riga.map_err(|err| db_error("lettura delle impronte", &err))?;
        // Una riga illeggibile si salta invece di far fallire il ricalcolo: è
        // una riga sola, e la scala di tutta la libreria non deve dipendere
        // dalla peggiore delle sue righe.
        if let Some(vettore) = da_byte(&byte) {
            fuori.push(vettore);
        }
    }
    Ok(fuori)
}

/// La versione dei descrittori, come la vuole il database.
fn versione() -> i64 {
    i64::from(impronta::VERSIONE_ESTRATTORE)
}

/// Prima di quando un tentativo fallito si può ripetere.
fn scadenza(adesso: i64) -> i64 {
    adesso.saturating_sub(RITENTA_MS)
}

/// Da vettore a byte: `f32` in little-endian, di fila.
///
/// Little-endian e non l'ordine della macchina: un database può viaggiare fra
/// due computer — è il punto della sincronia — e un'impronta letta al contrario
/// non si annuncia, dà solo distanze assurde.
fn in_byte(vettore: &[f32]) -> Vec<u8> {
    let mut fuori = Vec::with_capacity(vettore.len().saturating_mul(4));
    for valore in vettore {
        fuori.extend_from_slice(&valore.to_le_bytes());
    }
    fuori
}

/// Da byte a vettore. `None` se il blocco non ha la misura giusta.
///
/// Il controllo sulla lunghezza non è difensivo: è ciò che fa da rete quando
/// `VERSIONE_ESTRATTORE` cambia numero di dimensioni e una riga vecchia sfugge
/// al filtro sulla versione. Meglio nessuna impronta che una letta storta.
fn da_byte(byte: &[u8]) -> Option<Vec<f32>> {
    if byte.len() != impronta::DIMENSIONI.saturating_mul(4) {
        return None;
    }
    let mut fuori = Vec::with_capacity(impronta::DIMENSIONI);
    for pezzo in byte.chunks_exact(4) {
        let quattro: [u8; 4] = pezzo.try_into().ok()?;
        let valore = f32::from_le_bytes(quattro);
        if !valore.is_finite() {
            return None;
        }
        fuori.push(valore);
    }
    Some(fuori)
}

/// L'errore del database, con il verbo giusto.
fn db_error(cosa: &str, err: &rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::DbQueryFailed {
        detail: Some(cosa.to_owned()),
    })
    .with_cause(err.to_string())
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::db;

    fn libreria() -> Connection {
        db::open_in_memory()
            .expect("database in memoria")
            .connection
    }

    /// Inserisce un brano con quel che serve alle query di questo modulo.
    fn brano(connection: &Connection, id: i64, aggiunto: i64) {
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album,
                                     duration_ms, file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, 'T', 'A', 'AL', 180000, 1, ?4, 0)",
                rusqlite::params![id, format!("/musica/{id}.flac"), format!("k{id}"), aggiunto],
            )
            .expect("inserimento del brano");
    }

    fn vettore(seme: f32) -> Vec<f32> {
        (0..impronta::DIMENSIONI)
            .map(|i| seme + i as f32 * 0.5)
            .collect()
    }

    // ── i byte ──────────────────────────────────────────────────────────────

    #[test]
    fn un_vettore_torna_com_era() {
        let v = vettore(1.5);
        let tornato = da_byte(&in_byte(&v)).expect("rilettura");
        assert_eq!(v, tornato);
    }

    #[test]
    fn un_blocco_della_misura_sbagliata_non_si_legge() {
        assert_eq!(da_byte(&[]), None);
        assert_eq!(da_byte(&[0, 0, 0, 0]), None);
        let mut troppo = in_byte(&vettore(0.0));
        troppo.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(da_byte(&troppo), None);
    }

    #[test]
    fn un_blocco_con_un_numero_assurdo_non_si_legge() {
        let mut v = vettore(0.0);
        if let Some(posto) = v.get_mut(3) {
            *posto = f32::NAN;
        }
        assert_eq!(da_byte(&in_byte(&v)), None);
    }

    // ── quali brani mancano ─────────────────────────────────────────────────

    #[test]
    fn un_brano_senza_impronta_e_un_candidato() {
        let connection = libreria();
        brano(&connection, 1, 100);
        assert_eq!(quanti_mancano(&connection, 0).expect("conteggio"), 1);
        let candidati = candidati(&connection, 0, 10).expect("candidati");
        assert_eq!(candidati.len(), 1);
        assert_eq!(candidati.first().map(|c| c.track_id), Some(1));
    }

    #[test]
    fn un_brano_con_impronta_non_lo_e_piu() {
        let mut connection = libreria();
        brano(&connection, 1, 100);
        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Misurato::Fatta {
                track_id: 1,
                vettore: vettore(0.0),
            }],
            1_000,
        )
        .expect("registrazione");
        tx.commit().expect("commit");
        assert_eq!(quanti_mancano(&connection, 2_000).expect("conteggio"), 0);
    }

    #[test]
    fn un_illeggibile_torna_candidato_dopo_un_mese_e_non_prima() {
        let mut connection = libreria();
        brano(&connection, 1, 100);
        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Misurato::Negata {
                track_id: 1,
                esito: ILLEGGIBILE,
            }],
            1_000,
        )
        .expect("registrazione");
        tx.commit().expect("commit");

        assert_eq!(quanti_mancano(&connection, 1_000).expect("subito"), 0);
        assert_eq!(
            quanti_mancano(&connection, 1_000 + RITENTA_MS - 1).expect("quasi"),
            0
        );
        assert_eq!(
            quanti_mancano(&connection, 2_000 + RITENTA_MS).expect("dopo"),
            1
        );
    }

    #[test]
    fn un_muto_non_torna_candidato_mai() {
        let mut connection = libreria();
        brano(&connection, 1, 100);
        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Misurato::Negata {
                track_id: 1,
                esito: MUTO,
            }],
            1_000,
        )
        .expect("registrazione");
        tx.commit().expect("commit");
        assert_eq!(
            quanti_mancano(&connection, 1_000 + RITENTA_MS * 12).expect("un anno dopo"),
            0
        );
    }

    #[test]
    fn la_rete_caduta_non_scrive_niente() {
        let mut connection = libreria();
        brano(&connection, 1, 100);
        let tx = connection.transaction().expect("transazione");
        let scritte = registra(&tx, &[Misurato::NonRaggiungibile], 1_000).expect("registrazione");
        tx.commit().expect("commit");
        assert_eq!(scritte, 0);
        // E il brano è ancora da fare: è tutto il punto.
        assert_eq!(quanti_mancano(&connection, 1_000).expect("conteggio"), 1);
    }

    #[test]
    fn i_piu_recenti_per_primi() {
        let connection = libreria();
        brano(&connection, 1, 100);
        brano(&connection, 2, 300);
        brano(&connection, 3, 200);
        let candidati = candidati(&connection, 0, 10).expect("candidati");
        let ordine: Vec<i64> = candidati.iter().map(|c| c.track_id).collect();
        assert_eq!(ordine, vec![2, 3, 1]);
    }

    #[test]
    fn una_riscrittura_non_raddoppia_la_riga() {
        let mut connection = libreria();
        brano(&connection, 1, 100);
        for _ in 0..3 {
            let tx = connection.transaction().expect("transazione");
            registra(
                &tx,
                &[Misurato::Fatta {
                    track_id: 1,
                    vettore: vettore(0.0),
                }],
                1_000,
            )
            .expect("registrazione");
            tx.commit().expect("commit");
        }
        let quante: i64 = connection
            .query_row("SELECT COUNT(*) FROM track_impronta", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(quante, 1);
    }

    #[test]
    fn cancellare_un_brano_ne_cancella_l_impronta() {
        let mut connection = libreria();
        brano(&connection, 1, 100);
        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Misurato::Fatta {
                track_id: 1,
                vettore: vettore(0.0),
            }],
            1_000,
        )
        .expect("registrazione");
        tx.commit().expect("commit");
        connection
            .execute("DELETE FROM tracks WHERE id = 1", [])
            .expect("cancellazione");
        let quante: i64 = connection
            .query_row("SELECT COUNT(*) FROM track_impronta", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(quante, 0);
    }

    // ── la scala ────────────────────────────────────────────────────────────

    /// Riempie la libreria di impronte che variano, per poterne misurare la scala.
    fn con_impronte(connection: &mut Connection, quante: usize) {
        for i in 0..quante {
            let id = i64::try_from(i).unwrap_or(0) + 1;
            brano(connection, id, id);
        }
        let tx = connection.transaction().expect("transazione");
        let misurati: Vec<Misurato> = (0..quante)
            .map(|i| Misurato::Fatta {
                track_id: i64::try_from(i).unwrap_or(0) + 1,
                vettore: vettore(i as f32),
            })
            .collect();
        registra(&tx, &misurati, 1_000).expect("registrazione");
        tx.commit().expect("commit");
    }

    #[test]
    fn una_libreria_piccola_non_produce_una_scala() {
        let mut connection = libreria();
        con_impronte(&mut connection, MINIMO_BRANI - 1);
        let tx = connection.transaction().expect("transazione");
        assert_eq!(ritara(&tx, 1_000).expect("taratura"), None);
        tx.commit().expect("commit");
        assert!(scala(&connection).expect("lettura").is_none());
    }

    #[test]
    fn una_libreria_abbastanza_grande_la_produce_e_si_rilegge() {
        let mut connection = libreria();
        con_impronte(&mut connection, MINIMO_BRANI);
        let tx = connection.transaction().expect("transazione");
        assert_eq!(ritara(&tx, 1_000).expect("taratura"), Some(MINIMO_BRANI));
        tx.commit().expect("commit");

        let letta = scala(&connection).expect("lettura").expect("c'è");
        assert_eq!(letta.dimensioni(), impronta::DIMENSIONI);
        assert_eq!(letta.brani(), MINIMO_BRANI);
        // E normalizza davvero: il brano che sta alla media dà zero ovunque.
        let z = letta.normalizza(letta.medie()).expect("normalizzazione");
        assert!(z.iter().all(|x| x.abs() < 1e-3), "z non centrato: {z:?}");
    }

    #[test]
    fn ritarare_due_volte_non_aggiunge_una_riga() {
        let mut connection = libreria();
        con_impronte(&mut connection, MINIMO_BRANI);
        for _ in 0..2 {
            let tx = connection.transaction().expect("transazione");
            ritara(&tx, 1_000).expect("taratura");
            tx.commit().expect("commit");
        }
        let quante: i64 = connection
            .query_row("SELECT COUNT(*) FROM impronta_scala", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(quante, 1);
    }

    // ── rileggere le impronte ───────────────────────────────────────────────

    #[test]
    fn le_impronte_si_rileggono_per_identificativo() {
        let mut connection = libreria();
        con_impronte(&mut connection, 3);
        let lette = impronte(&connection, &[1, 2, 99]).expect("lettura");
        assert_eq!(lette.len(), 2);
        assert_eq!(lette.get(&1), Some(&vettore(0.0)));
        assert_eq!(lette.get(&2), Some(&vettore(1.0)));
        assert!(!lette.contains_key(&99));
    }

    #[test]
    fn un_brano_negato_non_ha_un_impronta_da_rileggere() {
        let mut connection = libreria();
        brano(&connection, 1, 100);
        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Misurato::Negata {
                track_id: 1,
                esito: ILLEGGIBILE,
            }],
            1_000,
        )
        .expect("registrazione");
        tx.commit().expect("commit");
        assert!(impronte(&connection, &[1]).expect("lettura").is_empty());
    }

    #[test]
    fn chiedere_niente_non_legge_niente() {
        let connection = libreria();
        assert!(impronte(&connection, &[]).expect("lettura").is_empty());
    }

    // ── la misura, con un finto che non apre niente ─────────────────────────

    struct FilesRotti(ErrorCode);

    impl MusicFiles for FilesRotti {
        fn walk(&self, _root: &str) -> Result<crate::files::Camminata, AppError> {
            Ok(crate::files::Camminata {
                file: Vec::new(),
                completa: true,
            })
        }
        fn open(
            &self,
            _path: &str,
        ) -> Result<Box<dyn crate::files::ReadSeek + Send + Sync>, AppError> {
            Err(AppError::new(self.0.clone()))
        }
    }

    fn scheda() -> SchedaSorgente {
        SchedaSorgente {
            track_id: 7,
            path: "/musica/7.flac".to_owned(),
            durata_ms: 180_000,
            replaygain_db: None,
        }
    }

    #[test]
    fn la_rete_giu_si_riconosce_e_non_diventa_una_colpa_del_brano() {
        let files = FilesRotti(ErrorCode::FsNetworkUnavailable {
            path: Some("//nas/musica".to_owned()),
        });
        assert_eq!(misura(&files, &scheda()), Misurato::NonRaggiungibile);
    }

    #[test]
    fn un_file_che_non_si_apre_e_illeggibile() {
        let files = FilesRotti(ErrorCode::FsReadFailed {
            path: "/musica/7.flac".to_owned(),
            detail: None,
        });
        assert_eq!(
            misura(&files, &scheda()),
            Misurato::Negata {
                track_id: 7,
                esito: ILLEGGIBILE,
            }
        );
    }
}
