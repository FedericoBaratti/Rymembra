//! La pagina di un artista: i suoi brani e le sue partecipazioni.
//!
//! # Chi è «suo»
//!
//! Un brano è di un artista se l'artista **effettivo** del brano è lui
//! ([`ARTISTA_EFFETTIVO`]: l'artista del disco quando c'è, l'interprete se
//! manca). È la stessa chiave con cui [`super::list_artists`] raggruppa e conta,
//! e con cui [`super::albums_by_artist`] sceglie i dischi: così il «12 brani»
//! della testata, la griglia dei dischi e questo elenco dicono la stessa cosa.
//! Ci stanno anche i brani senza disco (`album_key` vuoto), che nessun album
//! contiene e che «Riproduci» sull'artista, passando dai dischi, non
//! raggiungeva mai.
//!
//! # Le partecipazioni
//!
//! Sono i brani in cui l'artista compare **senza** esserne l'artista effettivo,
//! in due specie che la pagina non deve mai confondere:
//!
//! - **esatte**: `artist` è il suo nome ma l'artista del disco è un altro. Sono
//!   le raccolte («Vari artisti»): un fatto dei tag, non un'interpretazione;
//! - **euristiche**: il suo nome compare fra i crediti di un brano di un altro,
//!   per esempio «Altro feat. Tale». Si cerca con l'indice di testo, sul solo
//!   campo `artist`, come una frase.
//!
//! Dividere «A feat. B» in due artisti resta **non** fatto, e non qui: è un
//! mestiere dell'arricchimento (vedi il commento di [`ARTISTA_EFFETTIVO`]). La
//! euristica è una domanda alla ricerca, non una divisione dei nomi: non scrive
//! niente, e la pagina la dichiara per quel che è.
//!
//! # Il piano delle query
//!
//! Le tre partono ciascuna da un indice: l'artista effettivo da
//! `idx_tracks_artista_effettivo` (scritta senza alias, parola per parola, come
//! `sql_degli_album_di_un_artista`: SQLite usa un indice su espressione solo se
//! la riconosce identica), le esatte da `idx_tracks_artist`, le euristiche
//! dall'indice di testo e poi per identificativo. Le prove ne guardano il piano.

use rusqlite::Connection;
use rymembra_domain::errors::AppError;

use super::interrogazioni::{COLONNE_BRANO, TrackSummary, dove, track_from_row};
use super::{ARTISTA_EFFETTIVO, db_error};
use crate::specchio::Origine;

/// Come ordinare i brani di un artista.
///
/// Un valore chiuso: quel che arriva dalla finestra non deve poter raggiungere
/// una clausola SQL (vedi [`OrdineBraniArtista::sql`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrdineBraniArtista {
    /// Per album, poi per disco e traccia; i brani senza album in fondo.
    Album,
    /// Dal più vecchio, e i brani senza anno in fondo.
    Anno,
    /// Per titolo.
    Titolo,
    /// I più ascoltati in cima.
    PiuAscoltati,
}

impl OrdineBraniArtista {
    /// La clausola `ORDER BY`.
    ///
    /// Titolo e album ordinano con le chiavi della migrazione 042 (senza
    /// accenti, senza l'articolo: D-104), come i Brani e gli Album. L'identificativo
    /// chiude ogni ordine: due brani pari restano nello stesso posto fra una
    /// pagina e l'altra, e «Seleziona tutto» ha lo stesso ordine delle pagine.
    const fn sql(self) -> &'static str {
        match self {
            // La chiave dell'album dopo il suo nome: due dischi omonimi non si
            // mescolano.
            Self::Album => {
                "(t.album_key IS NULL OR t.album_key = ''), t.ordine_album, t.album_key,
                 t.disc_number, t.track_number, t.ordine_titolo, t.id"
            }
            Self::Anno => {
                "t.year IS NULL, t.year, t.ordine_album, t.album_key,
                 t.disc_number, t.track_number, t.ordine_titolo, t.id"
            }
            Self::Titolo => "t.ordine_titolo, t.ordine_album, t.id",
            Self::PiuAscoltati => "t.play_count DESC, t.last_played_at DESC, t.ordine_titolo, t.id",
        }
    }
}

/// Quale specie di partecipazione.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Partecipazione {
    /// `artist` è il suo nome, l'artista del disco è un altro.
    Esatta,
    /// Il suo nome compare fra i crediti di un brano di un altro.
    Euristica,
}

/// Quante partecipazioni, per specie: la lunghezza dei due elenchi.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContoPartecipazioni {
    /// Quante esatte.
    pub esatte: i64,
    /// Quante euristiche.
    pub euristiche: i64,
}

/// Da dove si leggono i brani di un artista, con il nome in `?1`: vedi
/// [`brani_artista`].
///
/// A parte perché lo legge anche `identificativi`, che deve dare gli stessi
/// brani nello stesso ordine senza leggerne le colonne: due copie della
/// clausola sarebbero due ordini il giorno in cui una cambia.
pub(super) fn da_brani_artista(
    connection: &Connection,
    origine: &Origine,
    ordine: OrdineBraniArtista,
) -> Result<String, AppError> {
    Ok(format!(
        "FROM tracks t
         WHERE {ARTISTA_EFFETTIVO} = ?1 AND {}
         ORDER BY {}",
        dove(connection, origine, "t.")?,
        ordine.sql()
    ))
}

/// La domanda di [`brani_artista`], a parte perché la prova ne guardi il piano.
/// Parametri: il nome, il limite, la posizione.
fn sql_dei_brani(
    connection: &Connection,
    origine: &Origine,
    ordine: OrdineBraniArtista,
) -> Result<String, AppError> {
    Ok(format!(
        "SELECT {COLONNE_BRANO} {} LIMIT ?2 OFFSET ?3",
        da_brani_artista(connection, origine, ordine)?
    ))
}

/// Una pagina dei brani di un artista.
///
/// `nome` è l'artista effettivo come lo dà [`super::list_artists`]. Il confronto
/// è esatto (il risultato di `TRIM` non porta collazione, quindi `BINARY`): due
/// grafie diverse restano due artisti, come nella griglia.
///
/// # Errori
///
/// `ipc.payloadInvalid` per una chiave di dispositivo storta; `db.*` dalla
/// lettura.
pub fn brani_artista(
    connection: &Connection,
    origine: &Origine,
    nome: &str,
    ordine: OrdineBraniArtista,
    offset: i64,
    limit: i64,
) -> Result<Vec<TrackSummary>, AppError> {
    let sql = sql_dei_brani(connection, origine, ordine)?;
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("brani di un artista", &err))?;
    let righe = statement
        .query_map(rusqlite::params![nome, limit, offset], track_from_row)
        .map_err(|err| db_error("brani di un artista", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("brani di un artista", &err))
}

/// Gli identificativi di tutti i brani di un artista, nell'ordine di
/// [`brani_artista`].
///
/// Per «Seleziona tutto» e per suonare da una riga: la finestra tiene in mano le
/// pagine che guarda, e chi suona vuole l'elenco intero.
///
/// # Errori
///
/// Come [`brani_artista`].
pub fn id_dei_brani_artista(
    connection: &Connection,
    origine: &Origine,
    nome: &str,
    ordine: OrdineBraniArtista,
) -> Result<Vec<i64>, AppError> {
    let sql = format!(
        "SELECT t.id {}",
        da_brani_artista(connection, origine, ordine)?
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("identificativi dei brani di un artista", &err))?;
    let righe = statement
        .query_map([nome], |riga| riga.get::<_, i64>(0))
        .map_err(|err| db_error("identificativi dei brani di un artista", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("identificativi dei brani di un artista", &err))
}

/// Le condizioni delle partecipazioni esatte, con il nome in `?1`.
///
/// `artist = ?1` e non `TRIM(artist) = ?1`: è l'espressione di `idx_tracks_artist`.
/// Un interprete scritto con uno spazio in testa non è un artista come un altro,
/// e la griglia lo conta già sotto un altro nome.
fn da_esatte(connection: &Connection, origine: &Origine) -> Result<String, AppError> {
    Ok(format!(
        "FROM tracks t
         WHERE t.artist = ?1 AND {ARTISTA_EFFETTIVO} <> ?1 AND {}
         ORDER BY {}",
        dove(connection, origine, "t.")?,
        OrdineBraniArtista::Album.sql()
    ))
}

/// Le condizioni delle partecipazioni euristiche: l'espressione di testo in
/// `?1`, il nome in `?2`.
///
/// `IN (SELECT rowid …)` e non un `JOIN` con la tabella di testo: `artist` e
/// `album_artist` esistono in tutte e due, e l'artista effettivo — scritto senza
/// alias come lo vuole l'indice — diventerebbe ambiguo.
fn da_euristiche(connection: &Connection, origine: &Origine) -> Result<String, AppError> {
    Ok(format!(
        "FROM tracks t
         WHERE t.id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1)
           AND t.artist <> ?2 AND {ARTISTA_EFFETTIVO} <> ?2 AND {}
         ORDER BY {}",
        dove(connection, origine, "t.")?,
        OrdineBraniArtista::Album.sql()
    ))
}

/// La domanda delle esatte, a parte per il piano. Parametri: il nome, il
/// limite, la posizione.
fn sql_delle_esatte(connection: &Connection, origine: &Origine) -> Result<String, AppError> {
    Ok(format!(
        "SELECT {COLONNE_BRANO} {} LIMIT ?2 OFFSET ?3",
        da_esatte(connection, origine)?
    ))
}

/// La domanda delle euristiche, a parte per il piano. Parametri: l'espressione,
/// il nome, il limite, la posizione.
fn sql_delle_euristiche(connection: &Connection, origine: &Origine) -> Result<String, AppError> {
    Ok(format!(
        "SELECT {COLONNE_BRANO} {} LIMIT ?3 OFFSET ?4",
        da_euristiche(connection, origine)?
    ))
}

/// Il nome come frase da cercare nel solo campo `artist`, o `None` se non ha una
/// parola da cercare.
///
/// Una frase fra virgolette, con le virgolette interne raddoppiate come le
/// protegge FTS5: `AC/DC`, `NOT`, `a*b` e un nome con le virgolette diventano
/// parole, non operatori. Un nome di sola punteggiatura (`!!!`) non ha parole
/// per il tokenizzatore, e una frase vuota non è una domanda: niente da cercare.
fn frase_dei_crediti(nome: &str) -> Option<String> {
    let nome = nome.trim();
    if !nome.chars().any(char::is_alphanumeric) {
        return None;
    }
    Some(format!("artist : \"{}\"", nome.replace('"', "\"\"")))
}

/// Una pagina di partecipazioni di una specie.
///
/// L'ordine è per album, perché è così che si cerca «in quale raccolta?».
///
/// # Errori
///
/// Come [`brani_artista`].
pub fn partecipazioni(
    connection: &Connection,
    origine: &Origine,
    nome: &str,
    tipo: Partecipazione,
    offset: i64,
    limit: i64,
) -> Result<Vec<TrackSummary>, AppError> {
    let cosa = "partecipazioni di un artista";
    match tipo {
        Partecipazione::Esatta => {
            if nome.is_empty() {
                return Ok(Vec::new());
            }
            let sql = sql_delle_esatte(connection, origine)?;
            let mut statement = connection
                .prepare_cached(&sql)
                .map_err(|err| db_error(cosa, &err))?;
            let righe = statement
                .query_map(rusqlite::params![nome, limit, offset], track_from_row)
                .map_err(|err| db_error(cosa, &err))?;
            righe
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| db_error(cosa, &err))
        }
        Partecipazione::Euristica => {
            let Some(frase) = frase_dei_crediti(nome) else {
                return Ok(Vec::new());
            };
            let sql = sql_delle_euristiche(connection, origine)?;
            let mut statement = connection
                .prepare_cached(&sql)
                .map_err(|err| db_error(cosa, &err))?;
            let righe = statement
                .query_map(
                    rusqlite::params![frase, nome, limit, offset],
                    track_from_row,
                )
                .map_err(|err| db_error(cosa, &err))?;
            righe
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| db_error(cosa, &err))
        }
    }
}

/// Gli identificativi di tutte le partecipazioni di una specie, nell'ordine di
/// [`partecipazioni`].
///
/// Per suonare dalla riga premuta: la finestra tiene in mano le pagine che
/// guarda, e chi suona vuole l'elenco intero. Le stesse condizioni e lo stesso
/// ordine delle pagine, per costruzione ([`da_esatte`], [`da_euristiche`]).
///
/// # Errori
///
/// Come [`brani_artista`].
pub fn id_delle_partecipazioni(
    connection: &Connection,
    origine: &Origine,
    nome: &str,
    tipo: Partecipazione,
) -> Result<Vec<i64>, AppError> {
    let cosa = "identificativi delle partecipazioni";
    let (sql, frase) = match tipo {
        Partecipazione::Esatta => {
            if nome.is_empty() {
                return Ok(Vec::new());
            }
            (
                format!("SELECT t.id {}", da_esatte(connection, origine)?),
                None,
            )
        }
        Partecipazione::Euristica => {
            let Some(frase) = frase_dei_crediti(nome) else {
                return Ok(Vec::new());
            };
            (
                format!("SELECT t.id {}", da_euristiche(connection, origine)?),
                Some(frase),
            )
        }
    };
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error(cosa, &err))?;
    let leggi = |riga: &rusqlite::Row<'_>| riga.get::<_, i64>(0);
    let righe = match &frase {
        None => statement.query_map([nome], leggi),
        Some(frase) => statement.query_map(rusqlite::params![frase, nome], leggi),
    }
    .map_err(|err| db_error(cosa, &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))
}

/// Quante partecipazioni ha un artista, per specie.
///
/// Separato dalle pagine: la lista rada vuole il totale prima delle righe
/// (`rada.ts`), e le due specie hanno ciascuna il suo.
///
/// # Errori
///
/// Come [`brani_artista`].
pub fn partecipazioni_conta(
    connection: &Connection,
    origine: &Origine,
    nome: &str,
) -> Result<ContoPartecipazioni, AppError> {
    let cosa = "conteggio delle partecipazioni";
    let conta = |sql: String, parametri: &[&dyn rusqlite::ToSql]| -> Result<i64, AppError> {
        connection
            .prepare_cached(&sql)
            .and_then(|mut statement| statement.query_row(parametri, |riga| riga.get(0)))
            .map_err(|err| db_error(cosa, &err))
    };
    let dove_t = dove(connection, origine, "t.")?;
    let esatte = if nome.is_empty() {
        0
    } else {
        conta(
            format!(
                "SELECT COUNT(*) FROM tracks t
                  WHERE t.artist = ?1 AND {ARTISTA_EFFETTIVO} <> ?1 AND {dove_t}"
            ),
            &[&nome],
        )?
    };
    let euristiche = match frase_dei_crediti(nome) {
        None => 0,
        Some(frase) => conta(
            format!(
                "SELECT COUNT(*) FROM tracks t
                  WHERE t.id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1)
                    AND t.artist <> ?2 AND {ARTISTA_EFFETTIVO} <> ?2 AND {dove_t}"
            ),
            &[&frase, &nome],
        )?,
    };
    Ok(ContoPartecipazioni { esatte, euristiche })
}

#[cfg(test)]
#[path = "prove_artista.rs"]
mod prove;
