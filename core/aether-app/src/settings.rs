//! La tabella `settings`: due funzioni, e una sola copia dell'upsert.
//!
//! Non è un modulo che fa qualcosa di difficile — sono una `SELECT` e una
//! `INSERT … ON CONFLICT` — ed è proprio per questo che esiste. Fino a ieri lo
//! stesso paio di query stava ricopiato a mano in tre posti: qui in
//! [`crate::playback`] per la coda e il volume, nei comandi della finestra per
//! le cartelle sorvegliate, e nel modulo delle skin per quella scelta. Tre
//! copie che nessuno aveva scritto per distrazione: ognuna era nata dov'era
//! servita, senza vedere le altre.
//!
//! Il difetto di tre copie non è la lunghezza. È che la quarta e la quinta le
//! scrive chi arriva dopo, e che una di loro prima o poi tratterà una riga
//! mancante in modo diverso dalle altre — un `Err` invece di un `Ok(None)`, o
//! il contrario — e la differenza si vedrà solo il giorno in cui qualcuno apre
//! l'applicazione con un'impostazione mai scritta.
//!
//! # Un valore illeggibile non è un guasto
//!
//! [`read`] restituisce `Ok(None)` quando la chiave non c'è, e **non** un
//! errore: un'impostazione mai scritta è la condizione normale del primo avvio,
//! non un'anomalia. Chi legge decide cosa vale l'assenza, perché solo lui sa se
//! il valore di serie è `"plain"`, una lista vuota o un volume a metà.
//!
//! Un errore lo restituisce solo quando è il database a non rispondere, che è
//! l'unico caso in cui c'è davvero qualcosa di rotto.
//!
//! # I segreti non stanno qui
//!
//! Lo dice lo schema (`001_baseline.sql`, sezione «impostazioni») e vale la
//! pena ripeterlo nel punto in cui qualcuno sarebbe tentato di violarlo: token,
//! password e chiavi vanno nel portachiavi del sistema operativo. Questa
//! tabella sta dentro `aether.db`, che è un file leggibile da qualunque
//! processo giri come l'utente e che è **anche** ciò che un backup copia via.

use aether_domain::errors::{AppError, ErrorCode};
use rusqlite::Connection;

/// La chiave con cui le cartelle sorvegliate stanno in `settings`.
///
/// Qui e non nei comandi della finestra perché ora la leggono in due: chi
/// mostra le cartelle e chi le salva nel backup. Una costante sola è ciò che
/// impedisce che il secondo scriva `library.root` e passi inosservato finché
/// qualcuno non prova a ripristinare.
pub const CHIAVE_CARTELLE: &str = "library.roots";

/// La chiave con cui la skin scelta sta in `settings`.
pub const CHIAVE_SKIN: &str = "skin.active";

/// La chiave dell'accento che segue la copertina.
///
/// Assente vale **spento**, e non è timidezza: è un cambiamento visibile di
/// tutta la finestra, e una preferenza del genere si accende chiedendolo. La
/// skin ha comunque l'ultima parola con `capabilities.dynamicAccent`.
pub const CHIAVE_ACCENTO_DINAMICO: &str = "skin.dynamicAccent";

/// La chiave della cartella in cui finiscono i brani scaricati.
///
/// Di serie vale la **prima cartella sorvegliata**, e non è un ripiego comodo:
/// una cartella fuori da quelle sorvegliate produrrebbe file che nessuna
/// scansione trova mai, cioè uno scaricamento riuscito che l'utente non vede
/// comparire in libreria — indistinguibile, per lui, da uno fallito.
pub const CHIAVE_CARTELLA_DOWNLOAD: &str = "download.folder";

/// Traduce un guasto di SQLite nominando la chiave che lo ha causato.
///
/// La chiave nel dettaglio e non un messaggio generico: `settings.corrupt` su
/// una riga qualunque non dice niente a chi legge un registro, mentre
/// «`player.queue`» dice subito quale funzione smetterà di ricordarsi le cose.
fn db_error(chiave: &str, err: &rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::DbQueryFailed {
        detail: Some(chiave.to_owned()),
    })
    .with_cause(err.to_string())
}

/// Legge un'impostazione.
///
/// `Ok(None)` se la chiave non c'è.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Una chiave assente **non** è
/// un errore: vedi la nota in testa al modulo.
pub fn read(connection: &Connection, chiave: &str) -> Result<Option<String>, AppError> {
    connection
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [chiave],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(db_error(chiave, &altro)),
        })
}

/// Scrive un'impostazione, creandola se non c'era.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn write(connection: &Connection, chiave: &str, valore: &str) -> Result<(), AppError> {
    connection
        .execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![chiave, valore],
        )
        .map(|_| ())
        .map_err(|err| db_error(chiave, &err))
}

/// Toglie un'impostazione, se c'è. Dice se c'era.
///
/// # Perché non basta scriverci dentro una stringa vuota
///
/// Perché stringa vuota e assente sono due stati diversi, e chi legge lo sa solo
/// se glielo si ricorda. `nuvola.email` si «cancellava» così, e chi si scollegava
/// lasciava in tabella una riga `nuvola.email = ""`: [`read`] rispondeva
/// `Some("")` e ogni lettore doveva ricordarsi di aggiungere un
/// `.filter(|e| !e.is_empty())` per non mostrare un indirizzo vuoto al posto di
/// nessun indirizzo. Il primo che se lo dimentica non produce un errore —
/// produce una schermata che dice di essere collegata a un account senza nome.
///
/// C'è anche la ragione più semplice, e vale da sola: un'identità che l'utente
/// ha chiesto di dimenticare non deve restare scritta in un file che il backup
/// copia via.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Una chiave che non c'era
/// **non** è un errore: cancellare due volte è la stessa cosa che cancellare
/// una volta, ed è ciò che permette di chiamarla senza guardare prima.
pub fn forget(connection: &Connection, chiave: &str) -> Result<bool, AppError> {
    connection
        .execute("DELETE FROM settings WHERE key = ?1", [chiave])
        .map(|righe| righe > 0)
        .map_err(|err| db_error(chiave, &err))
}

/// Legge un'impostazione interpretandola come JSON.
///
/// Un valore che non si interpreta vale **come se non ci fosse**, e non come un
/// errore. È la regola che i comandi della finestra già applicavano alle
/// cartelle sorvegliate, con la sua ragione: al massimo l'utente riseleziona le
/// cartelle, mentre un avvio che fallisce per un'impostazione corrotta non gli
/// lascia nessun modo di correggerla — l'unico posto da cui potrebbe farlo è
/// l'applicazione che non parte.
///
/// # Errori
///
/// Solo `db.queryFailed`: il JSON storto non ne produce nessuno.
pub fn read_json<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    chiave: &str,
) -> Result<Option<T>, AppError> {
    Ok(read(connection, chiave)?.and_then(|raw| serde_json::from_str(&raw).ok()))
}

/// Scrive un'impostazione serializzandola in JSON.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde; `internal.unexpected` se il
/// valore non si serializza — cosa che per i tipi che passano di qui non può
/// succedere, ma che non si nasconde dietro un valore di ripiego: scrivere
/// `"[]"` al posto di una lista che non si è serializzata cancellerebbe le
/// cartelle sorvegliate dell'utente senza dirlo a nessuno.
pub fn write_json<T: serde::Serialize>(
    connection: &Connection,
    chiave: &str,
    valore: &T,
) -> Result<(), AppError> {
    let raw = serde_json::to_string(valore).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some(format!("serializzazione di {chiave}")),
        })
        .with_cause(err.to_string())
    })?;
    write(connection, chiave, &raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un database con il solo schema che serve a questo modulo.
    ///
    /// In memoria e non da `db::open`: qui si prova la coppia di query, non le
    /// migrazioni, e una tabella sola rende evidente che è tutto ciò che
    /// servirebbe per farlo girare altrove.
    fn connessione() -> Connection {
        let connection = Connection::open_in_memory().expect("database in memoria");
        connection
            .execute(
                "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
                [],
            )
            .expect("creazione della tabella");
        connection
    }

    #[test]
    fn una_chiave_mai_scritta_non_e_un_errore() {
        // La condizione del primo avvio. Se questa diventasse un `Err`,
        // l'applicazione non si aprirebbe finché qualcuno non scrive a mano una
        // riga in un database che non sa di avere.
        let connection = connessione();
        assert_eq!(read(&connection, "mai.scritta"), Ok(None));
    }

    #[test]
    fn scrivere_due_volte_sostituisce() {
        let connection = connessione();
        assert_eq!(write(&connection, "chiave", "prima"), Ok(()));
        assert_eq!(write(&connection, "chiave", "dopo"), Ok(()));
        assert_eq!(read(&connection, "chiave"), Ok(Some("dopo".to_owned())));
    }

    #[test]
    fn un_json_storto_vale_come_assente() {
        // Il caso vero: un'impostazione scritta da una versione precedente con
        // una forma diversa. Vale «non c'è», così chi legge applica il suo
        // valore di serie e l'utente può ricorreggerla dall'interfaccia.
        let connection = connessione();
        assert_eq!(write(&connection, "cartelle", "{non json"), Ok(()));
        assert_eq!(
            read_json::<Vec<String>>(&connection, "cartelle"),
            Ok(None),
            "un valore illeggibile non deve impedire l'avvio"
        );
    }

    #[test]
    fn un_json_di_un_altro_tipo_vale_come_assente() {
        let connection = connessione();
        assert_eq!(write(&connection, "cartelle", "\"una stringa\""), Ok(()));
        assert_eq!(read_json::<Vec<String>>(&connection, "cartelle"), Ok(None));
    }

    #[test]
    fn cancellare_toglie_la_riga_invece_di_svuotarla() {
        // La differenza che conta: dopo `forget` la chiave torna a essere
        // «mai scritta», che è lo stato in cui chi legge applica il suo valore
        // di serie. Con una stringa vuota resterebbe «scritta, e vuota».
        let connection = connessione();
        assert_eq!(
            write(&connection, "nuvola.email", "tizio@esempio.it"),
            Ok(())
        );
        assert_eq!(forget(&connection, "nuvola.email"), Ok(true));
        assert_eq!(read(&connection, "nuvola.email"), Ok(None));
    }

    #[test]
    fn cancellare_una_chiave_che_non_c_e_non_e_un_errore() {
        // È ciò che permette di scollegarsi due volte, o di scollegarsi senza
        // essersi mai collegati, senza guardare prima se c'è qualcosa.
        let connection = connessione();
        assert_eq!(forget(&connection, "mai.scritta"), Ok(false));
    }

    #[test]
    fn il_json_fa_andata_e_ritorno() {
        let connection = connessione();
        let cartelle = vec!["C:\\Musica".to_owned(), "D:\\Altro".to_owned()];
        assert_eq!(write_json(&connection, "cartelle", &cartelle), Ok(()));
        assert_eq!(
            read_json::<Vec<String>>(&connection, "cartelle"),
            Ok(Some(cartelle))
        );
    }
}
