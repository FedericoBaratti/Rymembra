//! La copia di un livello del testo a mano che un dispositivo collegato sostituisce.
//!
//! Un pezzo di `crate::testi`: dove un commento dice «questo modulo», intende `testi` intero.
//!
//! # Perché esiste
//!
//! Il testo sincronizzato a mano è ore di lavoro, e un dispositivo collegato lo
//! può sostituire con una correzione sua (`TestoScrivi`): è una scelta di prodotto
//! — la correzione fatta sul telefono vale sul PC —, ma senza una copia di prima
//! la sostituzione non si disfaceva. Adesso quel che sta per essere sostituito
//! resta, **di un livello**: una seconda sostituzione prende il posto della
//! copia, e quel che c'era ancora prima non si tiene.
//!
//! # Dove sta
//!
//! Nella tabella `lyrics`, in una riga a parte la cui chiave è quella del brano
//! seguita da un segno che nessuna chiave di brano contiene (`U+001F`, un
//! separatore di unità): non serve una migrazione, e la riga non si scambia per
//! il testo di nessun brano — nessuna `JOIN` su `tracks.track_key` la incontra.
//! Non viaggia: l'esportazione del profilo la salta ([`E_UNA_COPIA`]).
//!
//! La copia **non segue** il brano se la sua `track_key` cambia: il trigger
//! `lyrics_segue_track_key` sposta il testo e non lei, che resta dov'era senza
//! più un brano a cui tornare. È un limite dichiarato, non un guasto: costa una
//! riga di testo, e la copia di un livello non vale una migrazione.
//!
//! # Ripristinare è uno scambio
//!
//! Il testo di adesso diventa la copia, e la copia il testo di adesso: così
//! ripristinare si può disfare, e non c'è un secondo gesto da imparare.

use rusqlite::{Connection, OptionalExtension as _, params};
use rymembra_domain::errors::AppError;

use super::Riga;
use crate::library::db_error;

/// Il segno che, dopo la `track_key`, dice «la copia di prima».
const SEGNO: &str = "\u{1f}prima";

/// La condizione SQL, sulla colonna `track_key` di `lyrics`, di una riga che
/// **non** è una copia: per chi elenca la tabella intera e non vuole le copie.
pub const NON_E_UNA_COPIA: &str = "instr(track_key, char(31)) = 0";

/// La chiave con cui sta, in `lyrics`, la copia di prima del brano `track_key`.
fn chiave_della_copia(track_key: &str) -> String {
    format!("{track_key}{SEGNO}")
}

/// Quel che si sa della copia di un brano, per chi la offre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Copia {
    /// Quando il testo è stato sostituito, in millisecondi.
    pub sostituita_ms: i64,
    /// Quanto è lungo il testo che si ripristinerebbe, in byte.
    pub byte: usize,
}

/// Tiene `prima` come copia di un livello, al posto di quella che c'era.
///
/// Si chiama solo con un testo a mano che ha i tempi: quel che non è lavoro di
/// chi ascolta si riscarica, e non si copia.
///
/// # Errori
///
/// `db.queryFailed`.
pub(crate) fn tieni(
    connection: &Connection,
    track_key: &str,
    prima: &Riga,
    sostituita_ms: i64,
) -> Result<(), AppError> {
    connection
        .execute(
            "INSERT INTO lyrics
               (track_key, plain, synced, source, offset_ms, instrumental,
                duration_ms, updated_at, checked_at)
             VALUES (?1, NULL, ?2, 'mano', ?3, 0, ?4, ?5, ?6)
             ON CONFLICT(track_key) DO UPDATE SET
               synced = excluded.synced,
               offset_ms = excluded.offset_ms,
               duration_ms = excluded.duration_ms,
               updated_at = excluded.updated_at,
               checked_at = excluded.checked_at",
            params![
                chiave_della_copia(track_key),
                prima.synced,
                prima.scarto_ms,
                prima.durata_ms.and_then(|d| i64::try_from(d).ok()),
                prima.aggiornata_ms,
                sostituita_ms,
            ],
        )
        .map(|_| ())
        .map_err(|err| db_error("copia del testo a mano sostituito", &err))
}

/// La copia di prima del brano, se c'è.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn leggi(connection: &Connection, track_key: &str) -> Result<Option<Copia>, AppError> {
    connection
        .query_row(
            "SELECT length(CAST(synced AS BLOB)), COALESCE(checked_at, 0)
               FROM lyrics WHERE track_key = ?1 AND synced IS NOT NULL",
            [chiave_della_copia(track_key)],
            |riga| {
                Ok(Copia {
                    byte: usize::try_from(riga.get::<_, i64>(0)?).unwrap_or(0),
                    sostituita_ms: riga.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|err| db_error("lettura della copia del testo a mano", &err))
}

/// Scambia il testo di adesso con la copia di prima. `false` se la copia non c'è.
///
/// Il testo di adesso diventa la copia, se è un testo a mano con i tempi: così
/// ripristinare si può disfare. Altrimenti la copia si toglie, perché non resta
/// niente da scambiare. Il testo ripristinato porta l'ora di adesso: è un gesto di
/// chi ascolta, e come ogni gesto di chi ascolta vale più di una correzione
/// decisa prima altrove.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn ripristina(
    connection: &Connection,
    track_key: &str,
    adesso_ms: i64,
) -> Result<bool, AppError> {
    let guasto = |err: rusqlite::Error| db_error("ripristino del testo a mano", &err);
    let scambio = connection.unchecked_transaction().map_err(guasto)?;
    let copia: Option<(String, i32, Option<i64>)> = scambio
        .query_row(
            "SELECT synced, offset_ms, duration_ms FROM lyrics
              WHERE track_key = ?1 AND synced IS NOT NULL",
            [chiave_della_copia(track_key)],
            |riga| Ok((riga.get(0)?, riga.get(1)?, riga.get(2)?)),
        )
        .optional()
        .map_err(guasto)?;
    let Some((synced, scarto_ms, durata_ms)) = copia else {
        return Ok(false);
    };
    // Un guasto di lettura si dice: prenderlo per «non c'è» cancellerebbe la
    // copia e sovrascriverebbe il testo, cioè perderebbe tutti e due.
    match super::cerca_riga(&scambio, track_key)? {
        Some(attuale) if attuale.fonte.e_di_chi_ascolta() && attuale.ha_i_tempi() => {
            tieni(&scambio, track_key, &attuale, adesso_ms)?;
        }
        _ => {
            scambio
                .execute(
                    "DELETE FROM lyrics WHERE track_key = ?1",
                    [chiave_della_copia(track_key)],
                )
                .map_err(guasto)?;
        }
    }
    scambio
        .execute(
            "INSERT INTO lyrics
               (track_key, plain, synced, source, offset_ms, instrumental,
                duration_ms, updated_at, checked_at)
             VALUES (?1, NULL, ?2, 'mano', ?3, 0, ?4, ?5, ?5)
             ON CONFLICT(track_key) DO UPDATE SET
               plain = NULL,
               synced = excluded.synced,
               source = 'mano',
               offset_ms = excluded.offset_ms,
               instrumental = 0,
               duration_ms = excluded.duration_ms,
               updated_at = excluded.updated_at,
               checked_at = excluded.checked_at",
            params![track_key, synced, scarto_ms, durata_ms, adesso_ms],
        )
        .map_err(guasto)?;
    scambio.commit().map_err(guasto)?;
    Ok(true)
}
