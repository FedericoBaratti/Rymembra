//! I brani da guardare, e la parola dell'utente su di loro.
//!
//! # Cosa risolve
//!
//! Fin qui un brano con i metadati messi male non si distingueva da uno a
//! posto. «Artista sconosciuto» si vedeva, certo, ma un titolo mojibake, un
//! `Unknown Artist` scritto dal ripper e un album dedotto dalla cartella
//! avevano tutti l'aria di dati veri — e i file che non si leggevano proprio
//! non c'erano nemmeno, ridotti a un numero in fondo alla scansione.
//!
//! Con `tracks.meta_salute` la domanda «cosa devo guardare» ha una risposta che
//! costa un indice, e con `tracks.meta_origine` ha anche un **perché** per ogni
//! campo. Questo modulo li legge e li lascia correggere.
//!
//! # Perché la correzione non tocca i file
//!
//! Perché è reversibile a costo zero e perché non cambia la data di modifica —
//! che, cambiata, farebbe rileggere e riscrivere quel file a ogni scansione
//! successiva. Il file resta la fonte; la correzione si rimette sopra a ogni
//! passata, da [`crate::provenienza::Correzioni`] salvate in `track_overrides`.
//!
//! Chi vuole i tag scritti dentro ha già l'arricchimento, che lo fa con un
//! registro di annullamento.

use aether_domain::album::album_group_key;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::keys::{TrackKey, TrackKeyInput};
use aether_domain::ricostruzione::{Origine, Salute};
use rusqlite::Connection;
use serde::Serialize;

use crate::library::{db_error, now_ms, rebuild_aggregates};
use crate::provenienza::{Correzioni, Origini};

/// Un brano da guardare, con quel che serve a deciderlo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracciaIncerta {
    /// L'identificativo della riga.
    pub id: i64,
    /// Il percorso, che è l'unica cosa sempre vera di un brano messo male.
    pub path: String,
    /// Titolo.
    pub title: String,
    /// Interprete.
    pub artist: String,
    /// Album.
    pub album: String,
    /// Artista dell'album.
    pub album_artist: Option<String>,
    /// Genere.
    pub genre: Option<String>,
    /// Anno.
    pub year: Option<i64>,
    /// Numero di traccia.
    pub track_number: Option<i64>,
    /// Numero di disco.
    pub disc_number: Option<i64>,
    /// Durata in millisecondi. Zero su un brano degradato: non si è potuta leggere.
    pub duration_ms: i64,
    /// L'impronta della copertina, per mostrarla.
    pub cover_art_hash: Option<String>,
    /// `dedotto` o `degradato`.
    pub health: String,
    /// I nomi dei problemi, da `ricostruzione::Problema::as_str`.
    pub problems: Vec<String>,
    /// Da dove viene ogni campo, e com'era prima se è stato riparato.
    pub origins: Origini,
}

/// Le colonne che servono a una [`TracciaIncerta`], nell'ordine di lettura.
const COLONNE: &str = "id, path, title, artist, album, album_artist, genre, year,
     track_number, disc_number, duration_ms, cover_art_hash,
     meta_salute, meta_problemi, meta_origine";

fn traccia_da_riga(row: &rusqlite::Row<'_>) -> rusqlite::Result<TracciaIncerta> {
    let problemi: Option<String> = row.get(13)?;
    let origine: Option<String> = row.get(14)?;
    Ok(TracciaIncerta {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        album: row.get(4)?,
        album_artist: row.get(5)?,
        genre: row.get(6)?,
        year: row.get(7)?,
        track_number: row.get(8)?,
        disc_number: row.get(9)?,
        duration_ms: row.get(10)?,
        cover_art_hash: row.get(11)?,
        health: row.get(12)?,
        // Un JSON illeggibile non fa fallire l'elenco: si perde il dettaglio,
        // non la riga — e la riga è quella che dice all'utente che c'è qualcosa
        // da guardare.
        problems: problemi
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
        origins: Origini::da_json(origine.as_deref()),
    })
}

/// Quanti brani aspettano che qualcuno li guardi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn quanti(connection: &Connection) -> Result<i64, AppError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE meta_salute <> 'ok'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| db_error("conteggio dei brani da sistemare", &err))
}

/// Una pagina di brani da guardare.
///
/// I degradati per primi — un file che non si legge è un problema più grosso di
/// un album dedotto dalla cartella — e a parità i più recenti, che sono quelli
/// che l'utente ha appena aggiunto e di cui si ricorda.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn elenco(
    connection: &Connection,
    offset: i64,
    limite: i64,
) -> Result<Vec<TracciaIncerta>, AppError> {
    let sql = format!(
        "SELECT {COLONNE} FROM tracks
         WHERE meta_salute <> 'ok'
         ORDER BY meta_salute = 'degradato' DESC, date_added DESC, id DESC
         LIMIT ?1 OFFSET ?2"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| db_error("elenco dei brani da sistemare", &err))?;
    let righe = statement
        .query_map([limite, offset], traccia_da_riga)
        .map_err(|err| db_error("elenco dei brani da sistemare", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei brani da sistemare", &err))
}

/// Un brano solo, da guardare.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn uno(connection: &Connection, track_id: i64) -> Result<Option<TracciaIncerta>, AppError> {
    let sql = format!("SELECT {COLONNE} FROM tracks WHERE id = ?1");
    match connection.query_row(&sql, [track_id], traccia_da_riga) {
        Ok(traccia) => Ok(Some(traccia)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(db_error("lettura di un brano da sistemare", &err)),
    }
}

/// Il brano non c'è.
fn non_trovato(track_id: i64) -> AppError {
    AppError::new(ErrorCode::DbQueryFailed {
        detail: Some("correzione di un brano".to_owned()),
    })
    .with_message(format!("il brano {track_id} non è in libreria"))
}

/// Applica una correzione dell'utente a un brano.
///
/// Fa tre cose in una transazione sola, e devono stare insieme:
///
/// 1. **salva la correzione** in `track_overrides`, perché la scansione la
///    rimetta sopra ogni volta che rilegge quel file — senza, sparirebbe alla
///    prima riscansione senza che niente lo dica;
/// 2. **riscrive la riga**, chiavi comprese: `track_key` e `album_key` derivano
///    dai tre campi obbligatori, e non ricalcolarle lascerebbe il brano
///    agganciato all'identità che aveva quando si chiamava «Artista
///    sconosciuto»;
/// 3. **ricostruisce gli aggregati**, perché un artista o un album appena
///    corretti devono esistere come schede.
///
/// I campi corretti diventano [`Origine::Manuale`], che non è solo
/// un'etichetta: `enrich::plan_write` non tocca un campo manuale con nessun
/// verdetto, nemmeno quando è sicuro di sé.
///
/// # Errori
///
/// `db.queryFailed` se il brano non c'è o il database non risponde.
pub fn correggi(
    connection: &mut Connection,
    track_id: i64,
    correzioni: &Correzioni,
) -> Result<(), AppError> {
    if correzioni.e_vuoto() {
        return Ok(());
    }
    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione", &err))?;

    let (path, mut title, mut artist, mut album, origine): (
        String,
        String,
        String,
        String,
        Option<String>,
    ) = tx
        .query_row(
            "SELECT path, title, artist, album, meta_origine FROM tracks WHERE id = ?1",
            [track_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => non_trovato(track_id),
            altro => db_error("lettura del brano da correggere", &altro),
        })?;

    // Le correzioni si accumulano: chi corregge il titolo oggi e l'artista
    // domani non deve perdere la prima. È la stessa ragione per cui
    // `enrich_undo` si scrive con `INSERT OR IGNORE` — solo al contrario, qui la
    // verità è l'ultima parola e non la prima.
    let mut tutte: Correzioni = tx
        .query_row(
            "SELECT campi FROM track_overrides WHERE track_id = ?1",
            [track_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    fondi(&mut tutte, correzioni);

    let mut origini = Origini::da_json(origine.as_deref());
    let manuale = || {
        Some(crate::provenienza::Voce {
            da: Origine::Manuale.as_str().to_owned(),
            prima: None,
        })
    };
    if let Some(nuovo) = tutte.titolo.clone() {
        title = nuovo;
        origini.titolo = manuale();
    }
    if let Some(nuovo) = tutte.artista.clone() {
        artist = nuovo;
        origini.artista = manuale();
    }
    if let Some(nuovo) = tutte.album.clone() {
        album = nuovo;
        origini.album = manuale();
    }
    for (valore, voce) in [
        (tutte.album_artist.is_some(), &mut origini.album_artist),
        (tutte.genere.is_some(), &mut origini.genere),
        (tutte.anno.is_some(), &mut origini.anno),
        (tutte.traccia.is_some(), &mut origini.traccia),
        (tutte.disco.is_some(), &mut origini.disco),
    ] {
        if valore {
            *voce = manuale();
        }
    }

    let track_key = TrackKey::compute(TrackKeyInput {
        artist: Some(&artist),
        title: Some(&title),
        album: Some(&album),
    })
    .into_string();
    let album_key = album_group_key(&album, &path);

    tx.execute(
        "UPDATE tracks SET
           title = ?2, artist = ?3, album = ?4,
           album_artist = COALESCE(?5, album_artist),
           genre = COALESCE(?6, genre),
           year = COALESCE(?7, year),
           track_number = COALESCE(?8, track_number),
           disc_number = COALESCE(?9, disc_number),
           track_key = ?10, album_key = ?11,
           meta_origine = ?12,
           -- Una risposta dell'utente chiude la domanda: il brano esce
           -- dall'elenco anche se qualche campo che non ha toccato resta
           -- dedotto. Chiederglielo di nuovo sarebbe non aver ascoltato.
           meta_salute = ?13, meta_problemi = NULL
         WHERE id = ?1",
        rusqlite::params![
            track_id,
            title,
            artist,
            album,
            tutte.album_artist,
            tutte.genere,
            tutte.anno,
            tutte.traccia,
            tutte.disco,
            track_key,
            album_key,
            serde_json::to_string(&origini).ok(),
            Salute::Ok.as_str(),
        ],
    )
    .map_err(|err| db_error("scrittura del brano corretto", &err))?;

    let campi = serde_json::to_string(&tutte)
        .map_err(|err| non_trovato(track_id).with_cause(err.to_string()))?;
    tx.execute(
        "INSERT INTO track_overrides (track_id, campi, set_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(track_id) DO UPDATE SET campi = ?2, set_at = ?3",
        rusqlite::params![track_id, campi, now_ms()],
    )
    .map_err(|err| db_error("salvataggio della correzione", &err))?;

    rebuild_aggregates(&tx)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione", &err))
}

/// Fonde una correzione nuova sopra quelle già salvate.
///
/// Un campo assente nella nuova non cancella il vecchio: assente vuol dire «non
/// l'ho toccato», mai «svuotalo».
fn fondi(tutte: &mut Correzioni, nuove: &Correzioni) {
    macro_rules! prendi {
        ($campo:ident) => {
            if nuove.$campo.is_some() {
                tutte.$campo = nuove.$campo.clone();
            }
        };
    }
    prendi!(titolo);
    prendi!(artista);
    prendi!(album);
    prendi!(album_artist);
    prendi!(genere);
    prendi!(anno);
    prendi!(traccia);
    prendi!(disco);
}

/// «La deduzione è giusta»: si tiene quel che c'è e non se ne parla più.
///
/// È la risposta più frequente, e senza di essa l'elenco dei brani da guardare
/// non si svuoterebbe mai: la maggior parte delle deduzioni sono corrette, e
/// l'utente vuole poterlo dire con un gesto invece di riscrivere a mano quel che
/// già legge giusto.
///
/// Tecnicamente è una [`correggi`] con i valori che il brano ha adesso: così la
/// conferma sopravvive alla riscansione — la scansione rimetterebbe i valori
/// dedotti dal file, e questa li rimette sopra — e i campi diventano
/// [`Origine::Manuale`], che l'arricchimento non tocca.
///
/// # Errori
///
/// `db.queryFailed` se il brano non c'è o il database non risponde.
pub fn conferma(connection: &mut Connection, track_id: i64) -> Result<(), AppError> {
    let Some(traccia) = uno(connection, track_id)? else {
        return Err(non_trovato(track_id));
    };
    // Si confermano **solo** i campi che erano dedotti. Fissare come «manuale»
    // anche quelli letti dai tag vorrebbe dire che una riscansione di un file
    // ri-taggato dall'utente non aggiornerebbe più niente.
    let dedotto = |voce: &Option<crate::provenienza::Voce>| {
        voce.as_ref().is_some_and(|v| v.origine() != Origine::Tag)
    };
    let o = &traccia.origins;
    let correzioni = Correzioni {
        titolo: dedotto(&o.titolo).then(|| traccia.title.clone()),
        artista: dedotto(&o.artista).then(|| traccia.artist.clone()),
        album: dedotto(&o.album).then(|| traccia.album.clone()),
        album_artist: dedotto(&o.album_artist)
            .then(|| traccia.album_artist.clone())
            .flatten(),
        genere: dedotto(&o.genere).then(|| traccia.genre.clone()).flatten(),
        anno: dedotto(&o.anno).then_some(traccia.year).flatten(),
        traccia: dedotto(&o.traccia)
            .then_some(traccia.track_number)
            .flatten(),
        disco: dedotto(&o.disco).then_some(traccia.disc_number).flatten(),
    };

    if correzioni.e_vuoto() {
        // Niente da fissare — un brano degradato senza nemmeno un indizio dal
        // percorso — ma la domanda va comunque chiusa: se l'elenco lo
        // riproponesse, il pulsante «va bene così» non farebbe niente.
        connection
            .execute(
                "UPDATE tracks SET meta_salute = ?2, meta_problemi = NULL WHERE id = ?1",
                rusqlite::params![track_id, Salute::Ok.as_str()],
            )
            .map(|_| ())
            .map_err(|err| db_error("conferma di un brano", &err))
    } else {
        correggi(connection, track_id, &correzioni)
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Una riga di brano come la scriverebbe una scansione, senza scansione.
    fn brano(connection: &Connection, path: &str, salute: &str, origine: Option<&str>) -> i64 {
        connection
            .execute(
                "INSERT INTO tracks (
                   path, track_key, title, artist, album, album_key, duration_ms,
                   file_size, date_added, date_modified, meta_salute, meta_origine,
                   meta_problemi, source
                 ) VALUES (?1, 'k', 'Senza tag', 'Artista sconosciuto',
                   'Album sconosciuto', 'ak', 0, 1, 1, 1, ?2, ?3, ?4, 'scan')",
                rusqlite::params![path, salute, origine, Some(r#"["senza-tag"]"#.to_owned())],
            )
            .expect("inserimento");
        connection.last_insert_rowid()
    }

    fn apri() -> Connection {
        crate::db::open_in_memory().expect("database").connection
    }

    #[test]
    fn lelenco_mostra_solo_quel_che_ce_da_guardare() {
        let connection = apri();
        brano(&connection, "C:/M/a.mp3", "ok", None);
        brano(&connection, "C:/M/b.mp3", "dedotto", None);
        brano(&connection, "C:/M/c.mp3", "degradato", None);

        assert_eq!(quanti(&connection).expect("conteggio"), 2);
        let elenco = elenco(&connection, 0, 10).expect("elenco");
        assert_eq!(elenco.len(), 2);
        // I degradati per primi: un file che non si legge è un problema più
        // grosso di un album dedotto dalla cartella.
        assert_eq!(elenco.first().map(|t| t.health.as_str()), Some("degradato"));
    }

    #[test]
    fn una_correzione_riscrive_la_riga_e_le_sue_chiavi() {
        let mut connection = apri();
        let id = brano(
            &connection,
            "C:/M/Pink Floyd/The Wall/a.mp3",
            "dedotto",
            None,
        );

        correggi(
            &mut connection,
            id,
            &Correzioni {
                artista: Some("Pink Floyd".to_owned()),
                album: Some("The Wall".to_owned()),
                ..Correzioni::default()
            },
        )
        .expect("correzione");

        let (artista, album, chiave, salute, origine): (
            String,
            String,
            String,
            String,
            Option<String>,
        ) = connection
            .query_row(
                "SELECT artist, album, track_key, meta_salute, meta_origine
                 FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("riga");
        assert_eq!(artista, "Pink Floyd");
        assert_eq!(album, "The Wall");
        assert_eq!(salute, "ok");
        // La chiave si ricalcola: senza, il brano resterebbe agganciato
        // all'identità che aveva quando si chiamava «Artista sconosciuto», e la
        // sincronizzazione lo cercherebbe con quel nome su ogni altro
        // dispositivo.
        assert_ne!(chiave, "k");
        assert!(chiave.contains("pink floyd"), "chiave: {chiave}");
        // …e i campi corretti si dichiarano «manuale», che `plan_write` non tocca.
        let origine = origine.expect("provenienza");
        assert!(origine.contains("manuale"), "origine: {origine}");
    }

    #[test]
    fn le_correzioni_si_accumulano() {
        // Chi corregge il titolo oggi e l'artista domani non deve perdere la
        // prima: quel che sta in `track_overrides` è l'ultima parola su ogni
        // campo, non l'ultima operazione.
        let mut connection = apri();
        let id = brano(&connection, "C:/M/a.mp3", "dedotto", None);

        correggi(
            &mut connection,
            id,
            &Correzioni {
                titolo: Some("Hey You".to_owned()),
                ..Correzioni::default()
            },
        )
        .expect("prima");
        correggi(
            &mut connection,
            id,
            &Correzioni {
                artista: Some("Pink Floyd".to_owned()),
                ..Correzioni::default()
            },
        )
        .expect("seconda");

        let campi: String = connection
            .query_row(
                "SELECT campi FROM track_overrides WHERE track_id = ?1",
                [id],
                |r| r.get(0),
            )
            .expect("correzioni");
        assert!(campi.contains("Hey You"), "campi: {campi}");
        assert!(campi.contains("Pink Floyd"), "campi: {campi}");
    }

    #[test]
    fn confermare_fissa_solo_quel_che_era_dedotto() {
        let mut connection = apri();
        let origine = r#"{"titolo":{"da":"percorso"},"artista":{"da":"tag"}}"#;
        let id = brano(&connection, "C:/M/a.mp3", "dedotto", Some(origine));

        conferma(&mut connection, id).expect("conferma");

        let campi: String = connection
            .query_row(
                "SELECT campi FROM track_overrides WHERE track_id = ?1",
                [id],
                |r| r.get(0),
            )
            .expect("correzioni");
        // Il titolo era dedotto: si fissa, così la riscansione non lo rifà.
        assert!(campi.contains("Senza tag"), "campi: {campi}");
        // L'interprete veniva dai tag: fissarlo vorrebbe dire che ri-taggare il
        // file a mano non aggiornerebbe più niente.
        assert!(!campi.contains("Artista sconosciuto"), "campi: {campi}");
        assert_eq!(quanti(&connection).expect("conteggio"), 0);
    }

    #[test]
    fn confermare_un_brano_senza_niente_da_fissare_chiude_lo_stesso_la_domanda() {
        // Un file degradato di cui nemmeno il percorso dice niente: non c'è
        // nessun campo da rendere manuale, ma il pulsante «va bene così» deve
        // comunque toglierlo dall'elenco. Altrimenti non farebbe niente.
        let mut connection = apri();
        let id = brano(&connection, "C:/M/a.mp3", "degradato", None);
        conferma(&mut connection, id).expect("conferma");
        assert_eq!(quanti(&connection).expect("conteggio"), 0);
    }

    #[test]
    fn un_brano_che_non_ce_non_fa_cadere_niente() {
        let mut connection = apri();
        assert!(conferma(&mut connection, 999).is_err());
        assert!(
            correggi(
                &mut connection,
                999,
                &Correzioni {
                    titolo: Some("x".to_owned()),
                    ..Correzioni::default()
                }
            )
            .is_err()
        );
    }
}
