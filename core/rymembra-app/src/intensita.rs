//! Quanto forte suona ogni brano della libreria: misurarlo, conservarlo,
//! rileggerlo quando si suona.
//!
//! La misura la fa [`rymembra_play::intensita`]; qui c'è tutto il resto, con la
//! forma di [`crate::sonora`] — che è la passata sorella e il modello:
//!
//! ```text
//! candidati()  ──►  un lotto di schede            (lucchetto preso)
//! misura()     ──►  apre e decodifica tutto       (SENZA lucchetto)
//! registra()   ──►  le righe, in una transazione  (lucchetto preso)
//! ```
//!
//! La differenza che conta è nel costo della fase di mezzo: l'impronta legge
//! trenta secondi, questa **l'intero file**, perché l'intensità integrata è una
//! media su tutto il brano (il perché sta in `rymembra_play::intensita`). Un
//! brano costa qualche decimo di secondo su un disco locale, e il lotto è più
//! piccolo di conseguenza.
//!
//! # Tag prima, misura poi
//!
//! Chi legge queste righe per suonare — [`crate::playback`] — preferisce il
//! tag ReplayGain quando c'è: l'ha scritto qualcuno apposta, spesso con il
//! modo album calcolato sull'album suonato di fila. La misura vale quando il
//! tag manca, che nelle librerie vere è quasi sempre. Il picco invece viene
//! **solo** da qui: i tag lo portano di rado e mai il picco vero, e la colonna
//! non esiste in `tracks`.
//!
//! # La rete che cade non è una proprietà del brano
//!
//! Come per l'impronta: [`Misurato::NonRaggiungibile`] ferma il lotto e non
//! scrive niente, perché segnare illeggibili i brani di una share caduta
//! vorrebbe dire non riprovarli per un mese.

use rusqlite::{Connection, Transaction};

use rymembra_domain::errors::{AppError, ErrorCodeKind};
use rymembra_play::intensita::{self, Esito, Intensita};

use crate::files::MusicFiles;
use crate::library::db_error;
use crate::playback::{SchedaSorgente, collocazione_su_disco, sorgente_da_scheda};

/// Quanti brani per passata.
///
/// Sedici e non i sessantaquattro dell'impronta: qui ogni brano è una
/// decodifica completa, e sedici brani sono il lavoro che si accetta di perdere
/// se l'applicazione si chiude a metà lotto — una manciata di secondi.
pub const LOTTO: usize = 16;

/// Ogni quanto si ritenta un brano che non si è potuto misurare.
///
/// Un mese, come per l'impronta e per la stessa ragione: un file che non si
/// decodifica è una proprietà del file, e le proprietà dei file cambiano piano.
pub const RITENTA_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Ci sono i numeri.
pub const OK: &str = "ok";
/// Il file non si è potuto decodificare. Si ritenta fra [`RITENTA_MS`].
pub const ILLEGGIBILE: &str = "illeggibile";
/// Niente sopra la soglia assoluta di −70 LUFS. Terminale.
pub const MUTO: &str = "muto";

/// Cos'è successo provando a misurare un brano.
#[derive(Debug, Clone, PartialEq)]
pub enum Misurato {
    /// I numeri.
    Fatta {
        /// Quale brano.
        track_id: i64,
        /// Quel che si è misurato.
        intensita: Intensita,
    },
    /// Il file c'è e una misura non se ne ricava.
    Negata {
        /// Quale brano.
        track_id: i64,
        /// Perché: [`ILLEGGIBILE`] o [`MUTO`].
        esito: &'static str,
    },
    /// La rete non risponde. **Non si scrive niente e il lotto si ferma.**
    NonRaggiungibile,
}

/// Quali brani mancano, e a che condizioni ci si torna.
///
/// La stessa clausola per [`candidati`] e per [`quanti_mancano`], come in
/// `sonora::candidato_where`, e per la stessa ragione: due copie divergono.
/// Anche qui i brani di catalogo restano fuori — misurarne uno vorrebbe dire
/// scaricarlo per intero — e `'muto'` non torna mai.
///
/// Un brano d'immagine CD entra come gli altri, e si misura il **suo** pezzo
/// del disco: più brani della stessa immagine sono più misure dello stesso
/// file, ognuna sul suo tratto. Resta fuori solo una riga d'immagine che non
/// dice dove il brano comincia — non la scrive la scansione — perché misurarla
/// vorrebbe dire misurare un pezzo a caso del disco.
const fn candidato_where() -> &'static str {
    "t.path IS NOT NULL
     AND (t.immagine IS NULL OR t.inizio_cd IS NOT NULL)
     AND (m.track_id IS NULL
          OR m.versione <> ?1
          OR (m.lufs IS NULL AND m.esito <> 'muto' AND m.tentato_at < ?2))"
}

/// Quanti brani aspettano ancora una misura.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn quanti_mancano(connection: &Connection, adesso: i64) -> Result<i64, AppError> {
    let sql = format!(
        "SELECT COUNT(*) FROM tracks t
         LEFT JOIN track_intensita m ON m.track_id = t.id
         WHERE {}",
        candidato_where()
    );
    connection
        .query_row(
            &sql,
            rusqlite::params![versione(), scadenza(adesso)],
            |row| row.get(0),
        )
        .map_err(|err| db_error("conteggio dei brani da misurare", &err))
}

/// A che punto è la misura della libreria: quanti brani con un file sono già
/// passati di qui, su quanti.
///
/// «Passati» e non «misurati»: un file illeggibile o muto è un brano di cui la
/// passata si è già occupata, e contarlo fra quelli che mancano terrebbe
/// l'avanzamento sotto il cento per cento per sempre.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn avanzamento(connection: &Connection, adesso: i64) -> Result<(i64, i64), AppError> {
    let totale: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE path IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .map_err(|err| db_error("conteggio dei brani con un file", &err))?;
    let mancano = quanti_mancano(connection, adesso)?;
    Ok((totale.saturating_sub(mancano).max(0), totale))
}

/// I prossimi brani da misurare, i più recenti per primi.
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
        "SELECT t.id, t.path, t.duration_ms, t.immagine, t.inizio_cd, t.fine_cd
         FROM tracks t
         LEFT JOIN track_intensita m ON m.track_id = t.id
         WHERE {}
         ORDER BY t.date_added DESC
         LIMIT ?3",
        candidato_where()
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| db_error("elenco dei brani da misurare", &err))?;
    let righe = statement
        .query_map(
            rusqlite::params![
                versione(),
                scadenza(adesso),
                i64::try_from(quanti).unwrap_or(i64::MAX),
            ],
            |row| {
                // `candidato_where` ha escluso i brani senza file. Per un
                // brano d'immagine la collocazione porta il pezzo, così si
                // misura il brano e non il disco intero.
                let track_id: i64 = row.get(0)?;
                let percorso: String = row.get(1)?;
                let durata_ms: i64 = row.get(2)?;
                let collocazione =
                    collocazione_su_disco(&percorso, row.get(3)?, row.get(4)?, row.get(5)?);
                Ok(collocazione.map(|collocazione| SchedaSorgente {
                    track_id,
                    collocazione,
                    durata_ms,
                    // La misura non ne ha bisogno: si misura il file com'è.
                    replaygain_db: None,
                    picco: None,
                }))
            },
        )
        .map_err(|err| db_error("elenco dei brani da misurare", &err))?;
    // Una riga d'immagine con dei confini che non stanno in un CD si salta:
    // vedi `collocazione_su_disco`. La scansione non ne scrive.
    Ok(righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei brani da misurare", &err))?
        .into_iter()
        .flatten()
        .collect())
}

/// Misura **un** brano, dall'inizio alla fine. Non tocca il database.
///
/// Un brano alla volta, come [`crate::sonora::misura`]: chi chiama guarda fra
/// l'uno e l'altro se la musica è ripartita, e si ferma lì invece che a metà di
/// una decodifica. E come lei chiede a chi chiama una scadenza addosso: su una
/// share morta l'apertura sono i quaranta secondi di Windows.
#[must_use]
pub fn misura(files: &dyn MusicFiles, scheda: &SchedaSorgente) -> Misurato {
    let sorgente = match sorgente_da_scheda(files, scheda) {
        Ok(sorgente) => sorgente,
        Err(err) => return da_errore(scheda.track_id, &err),
    };
    match intensita::misura(sorgente) {
        Ok(Esito::Misurata(intensita)) => Misurato::Fatta {
            track_id: scheda.track_id,
            intensita,
        },
        Ok(Esito::Muto) => Misurato::Negata {
            track_id: scheda.track_id,
            esito: MUTO,
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

/// Scrive nel database quel che si è misurato; restituisce quante righe.
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
            "INSERT INTO track_intensita
               (track_id, versione, lufs, picco, durata_ms, esito, tentato_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(track_id) DO UPDATE SET
               versione   = excluded.versione,
               lufs       = excluded.lufs,
               picco      = excluded.picco,
               durata_ms  = excluded.durata_ms,
               esito      = excluded.esito,
               tentato_at = excluded.tentato_at",
        )
        .map_err(|err| db_error("scrittura di una misura d'intensità", &err))?;

    let mut scritte = 0_usize;
    for misurato in misurati {
        let (track_id, numeri, esito) = match misurato {
            Misurato::Fatta {
                track_id,
                intensita,
            } => (*track_id, Some(*intensita), OK),
            Misurato::Negata { track_id, esito } => (*track_id, None, *esito),
            Misurato::NonRaggiungibile => continue,
        };
        statement
            .execute(rusqlite::params![
                track_id,
                versione(),
                numeri.map(|n| n.lufs),
                numeri.map(|n| n.picco),
                numeri.map(|n| i64::try_from(n.durata_ms).unwrap_or(i64::MAX)),
                esito,
                adesso
            ])
            .map_err(|err| db_error("scrittura di una misura d'intensità", &err))?;
        scritte = scritte.saturating_add(1);
    }
    Ok(scritte)
}

/// La misura di un brano, se c'è ed è di questa versione.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn del_brano(connection: &Connection, track_id: i64) -> Result<Option<Intensita>, AppError> {
    let letta: Option<(f64, f64, Option<i64>)> = connection
        .query_row(
            "SELECT lufs, picco, durata_ms FROM track_intensita
             WHERE track_id = ?1 AND versione = ?2
               AND lufs IS NOT NULL AND picco IS NOT NULL",
            rusqlite::params![track_id, versione()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(db_error("lettura di una misura d'intensità", &altro)),
        })?;
    Ok(letta.map(|(lufs, picco, durata_ms)| Intensita {
        lufs,
        picco,
        durata_ms: durata_ms.and_then(|d| u64::try_from(d).ok()).unwrap_or(0),
    }))
}

/// La misura dell'album di un brano, messa insieme dai brani misurati dello
/// stesso disco.
///
/// «Lo stesso disco» è la stessa `album_key`, cioè titolo e cartella — la
/// chiave con cui la libreria costruisce gli album. Un brano senza titolo
/// d'album non ha un album: i file sciolti di una cartella condividono la
/// chiave, e mettere insieme la loro intensità vorrebbe dire trattare da disco
/// una cartella di singoli.
///
/// Conta solo i brani già misurati: a metà della prima passata l'album è una
/// stima fatta con quel che c'è, e migliora da sola. `None` se non ce n'è
/// nessuno.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn dell_album_di(
    connection: &Connection,
    track_id: i64,
) -> Result<Option<Intensita>, AppError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT m.lufs, m.picco, m.durata_ms
               FROM tracks t
               JOIN tracks s ON s.album_key = t.album_key
               JOIN track_intensita m ON m.track_id = s.id
              WHERE t.id = ?1
                AND t.album IS NOT NULL AND TRIM(t.album) <> ''
                AND m.versione = ?2
                AND m.lufs IS NOT NULL AND m.picco IS NOT NULL",
        )
        .map_err(|err| db_error("lettura dell'intensità di un album", &err))?;
    let brani = statement
        .query_map(rusqlite::params![track_id, versione()], |row| {
            let durata_ms: Option<i64> = row.get(2)?;
            Ok(Intensita {
                lufs: row.get(0)?,
                picco: row.get(1)?,
                durata_ms: durata_ms.and_then(|d| u64::try_from(d).ok()).unwrap_or(0),
            })
        })
        .map_err(|err| db_error("lettura dell'intensità di un album", &err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("lettura dell'intensità di un album", &err))?;
    Ok(intensita::dell_album(&brani))
}

fn versione() -> i64 {
    i64::from(intensita::VERSIONE)
}

fn scadenza(adesso: i64) -> i64 {
    adesso.saturating_sub(RITENTA_MS)
}

#[cfg(test)]
mod prove {
    use rymembra_domain::errors::ErrorCode;

    use super::*;
    use crate::db;
    use crate::playback::Collocazione;

    fn libreria() -> Connection {
        db::open_in_memory()
            .expect("database in memoria")
            .connection
    }

    /// Un brano del disco `album`, nella cartella `cartella`.
    fn brano_del_disco(connection: &Connection, id: i64, album: &str, cartella: &str) {
        let percorso = format!("/musica/{cartella}/{id}.flac");
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, 'T', 'A', ?4, ?5, 180000, 1, ?1, 0)",
                rusqlite::params![
                    id,
                    percorso,
                    format!("k{id}"),
                    album,
                    rymembra_domain::album::album_group_key(album, &percorso),
                ],
            )
            .expect("inserimento del brano");
    }

    fn brano(connection: &Connection, id: i64) {
        brano_del_disco(connection, id, "Disco", "disco");
    }

    fn fatta(track_id: i64, lufs: f64, picco: f64) -> Misurato {
        Misurato::Fatta {
            track_id,
            intensita: Intensita {
                lufs,
                picco,
                durata_ms: 180_000,
            },
        }
    }

    fn scrivi(connection: &mut Connection, misurati: &[Misurato], adesso: i64) -> usize {
        let tx = connection.transaction().expect("transazione");
        let scritte = registra(&tx, misurati, adesso).expect("registrazione");
        tx.commit().expect("commit");
        scritte
    }

    #[test]
    fn un_brano_senza_misura_e_un_candidato_e_poi_non_piu() {
        let mut connection = libreria();
        brano(&connection, 1);
        assert_eq!(quanti_mancano(&connection, 0).expect("conteggio"), 1);
        assert_eq!(
            candidati(&connection, 0, 10)
                .expect("candidati")
                .first()
                .map(|c| c.track_id),
            Some(1)
        );
        scrivi(&mut connection, &[fatta(1, -9.0, 0.98)], 1_000);
        assert_eq!(quanti_mancano(&connection, 2_000).expect("conteggio"), 0);
        assert_eq!(
            avanzamento(&connection, 2_000).expect("avanzamento"),
            (1, 1)
        );
    }

    #[test]
    fn un_brano_d_immagine_si_misura_sul_suo_pezzo() {
        // Due brani della stessa immagine, e una riga d'immagine senza
        // inizio — che la scansione non scrive — che non deve entrare né
        // nell'elenco né nel conto: le due domande rispondono insieme.
        let connection = libreria();
        connection
            .execute_batch(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, duration_ms,
                                     file_size, date_added, date_modified,
                                     immagine, inizio_cd, fine_cd)
                 VALUES (1, '/musica/disco/Album.cue|03', 'k1', 'T', 'A', 'Disco', 200000,
                         1, 3, 0, '/musica/disco/Album.flac', 30000, 45000),
                        (2, '/musica/disco/Album.cue|04', 'k2', 'T', 'A', 'Disco', 300000,
                         1, 2, 0, '/musica/disco/Album.flac', 45000, NULL),
                        (3, '/musica/disco/Album.cue|05', 'k3', 'T', 'A', 'Disco', 1000,
                         1, 1, 0, '/musica/disco/Album.flac', NULL, NULL);",
            )
            .expect("brani d'immagine");
        let schede = candidati(&connection, 0, 10).expect("candidati");
        let viste: Vec<(i64, Collocazione, i64)> = schede
            .into_iter()
            .map(|s| (s.track_id, s.collocazione, s.durata_ms))
            .collect();
        assert_eq!(
            viste,
            vec![
                (
                    1,
                    Collocazione::Segmento {
                        immagine: "/musica/disco/Album.flac".to_owned(),
                        inizio_cd: 30_000,
                        fine_cd: Some(45_000),
                    },
                    200_000
                ),
                (
                    2,
                    Collocazione::Segmento {
                        immagine: "/musica/disco/Album.flac".to_owned(),
                        inizio_cd: 45_000,
                        fine_cd: None,
                    },
                    300_000
                ),
            ]
        );
        assert_eq!(quanti_mancano(&connection, 0).expect("conteggio"), 2);
    }

    #[test]
    fn la_misura_si_rilegge() {
        let mut connection = libreria();
        brano(&connection, 1);
        scrivi(&mut connection, &[fatta(1, -9.5, 1.12)], 1_000);
        let letta = del_brano(&connection, 1).expect("lettura").expect("c'è");
        assert!((letta.lufs + 9.5).abs() < 1e-9);
        assert!((letta.picco - 1.12).abs() < 1e-9);
        assert_eq!(letta.durata_ms, 180_000);
        assert_eq!(del_brano(&connection, 2).expect("lettura"), None);
    }

    #[test]
    fn un_illeggibile_torna_dopo_un_mese_e_un_muto_mai() {
        let mut connection = libreria();
        brano(&connection, 1);
        brano(&connection, 2);
        scrivi(
            &mut connection,
            &[
                Misurato::Negata {
                    track_id: 1,
                    esito: ILLEGGIBILE,
                },
                Misurato::Negata {
                    track_id: 2,
                    esito: MUTO,
                },
            ],
            1_000,
        );
        assert_eq!(quanti_mancano(&connection, 1_000).expect("subito"), 0);
        assert_eq!(
            quanti_mancano(&connection, 2_000 + RITENTA_MS).expect("dopo un mese"),
            1
        );
        // Tutti e due passati di qui: l'avanzamento è completo.
        assert_eq!(
            avanzamento(&connection, 1_000).expect("avanzamento"),
            (2, 2)
        );
        assert_eq!(del_brano(&connection, 1).expect("lettura"), None);
    }

    #[test]
    fn la_rete_caduta_non_scrive_niente() {
        let mut connection = libreria();
        brano(&connection, 1);
        assert_eq!(
            scrivi(&mut connection, &[Misurato::NonRaggiungibile], 1_000),
            0
        );
        assert_eq!(quanti_mancano(&connection, 1_000).expect("conteggio"), 1);
    }

    #[test]
    fn riscrivere_non_raddoppia_e_cancellare_il_brano_porta_via_la_misura() {
        let mut connection = libreria();
        brano(&connection, 1);
        for _ in 0..3 {
            scrivi(&mut connection, &[fatta(1, -10.0, 0.9)], 1_000);
        }
        let quante = |c: &Connection| -> i64 {
            c.query_row("SELECT COUNT(*) FROM track_intensita", [], |r| r.get(0))
                .expect("conteggio")
        };
        assert_eq!(quante(&connection), 1);
        connection
            .execute("DELETE FROM tracks WHERE id = 1", [])
            .expect("cancellazione");
        assert_eq!(quante(&connection), 0);
    }

    #[test]
    fn l_album_mette_insieme_i_brani_dello_stesso_disco() {
        let mut connection = libreria();
        brano_del_disco(&connection, 1, "Disco", "uno");
        brano_del_disco(&connection, 2, "Disco", "uno");
        // Stesso titolo, un'altra cartella: un altro disco.
        brano_del_disco(&connection, 3, "Disco", "due");
        scrivi(
            &mut connection,
            &[
                fatta(1, -10.0, 1.1),
                fatta(2, -20.0, 0.3),
                fatta(3, -30.0, 0.1),
            ],
            1_000,
        );
        let album = dell_album_di(&connection, 2)
            .expect("lettura")
            .expect("c'è");
        // La media delle energie di −10 e −20, e il picco più alto dei due.
        assert!((album.lufs + 12.596).abs() < 0.01, "album a {}", album.lufs);
        assert!((album.picco - 1.1).abs() < 1e-9);
        let altro = dell_album_di(&connection, 3)
            .expect("lettura")
            .expect("c'è");
        assert!((altro.lufs + 30.0).abs() < 1e-9);
    }

    #[test]
    fn i_file_senza_titolo_d_album_non_fanno_un_disco() {
        let mut connection = libreria();
        brano_del_disco(&connection, 1, "", "sciolti");
        brano_del_disco(&connection, 2, "", "sciolti");
        scrivi(
            &mut connection,
            &[fatta(1, -10.0, 1.0), fatta(2, -20.0, 0.5)],
            1_000,
        );
        assert_eq!(dell_album_di(&connection, 1).expect("lettura"), None);
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
            collocazione: Collocazione::File("/musica/7.flac".to_owned()),
            durata_ms: 180_000,
            replaygain_db: None,
            picco: None,
        }
    }

    #[test]
    fn la_rete_giu_non_diventa_una_colpa_del_brano() {
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
