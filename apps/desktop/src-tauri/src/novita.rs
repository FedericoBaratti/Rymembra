//! Le Novità: la finestra illustrata che si apre una volta, dopo un aggiornamento.
//!
//! Due comandi, e la memoria di quale versione si è già vista. Tutto il resto —
//! i passi, le illustrazioni, i testi in italiano e in inglese — sta nella
//! finestra (`apps/desktop/src/novita.ts` e `parti/Novita.tsx`): sono scritti a
//! mano, dentro l'app, e questa non sa nemmeno se una versione ne ha.
//!
//! # Perché la decisione sta qui, e perché la finestra aggiunge un'informazione
//!
//! La tabella di §11.3.2 (`rymembra_app::preferenze::novita_da_mostrare`) è una
//! riga sola con tre ingressi che il nucleo sa — la versione vista, la versione
//! di questa copia, se la libreria è nuova — e uno che sa solo la finestra: se
//! questa versione ha novità scritte. La finestra non dice «ho le novità» e
//! basta — non conosce nemmeno la propria versione, che è del nucleo —: manda
//! **l'elenco delle versioni che ne hanno**, e il nucleo guarda se c'è la sua. Il
//! numero della versione e il confronto restano in un posto solo, che è il modo
//! di non farli divergere.
//!
//! # Un'installazione nuova non mostra niente
//!
//! E **registra** la versione: senza, la prima riga della tabella non servirebbe
//! a niente — il giorno dopo la chiave sarebbe ancora assente, la libreria
//! piena, e la finestra si aprirebbe sopra chi ha appena cominciato. La
//! registrazione sta dentro la domanda e non in un secondo comando: se la
//! scrittura fallisce la domanda fallisce, e non si mostra né si registra
//! niente — un errore non diventa una finestra a metà.
//!
//! # La finestra non si apre da sola, e questo no
//!
//! Questo comando non apre niente: risponde. Quando la finestra si può aprire
//! (mai sopra `Primo`, lo Studio, l'attesa della libreria) lo decide la pagina,
//! come per il giro.

use serde::Serialize;
use tauri::AppHandle;

use crate::disparte::leggi_la_libreria;
use crate::errore::{Esito, errore};

use rymembra_app::preferenze::{self, EsitoNovita};

/// La risposta alla domanda «c'è una finestra delle Novità da mostrare?».
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NovitaDaMostrare {
    /// La versione di questa copia: quella le cui novità la finestra mostra, e
    /// quella che `novita_viste` ricorderà.
    pub versione: String,
    /// Aprire la finestra.
    pub mostra: bool,
}

/// C'è la finestra delle Novità da mostrare?
///
/// `con_novita` sono le versioni che hanno novità scritte (`novita.ts`): se
/// questa non c'è, la finestra non compare, e non è un errore. In
/// un'installazione nuova registra la versione e risponde di no.
///
/// # Errori
///
/// `db.queryFailed`: se non si riesce a leggere, o a registrare dove serve, non
/// si mostra niente.
#[tauri::command]
pub async fn novita_da_mostrare(
    app: AppHandle,
    con_novita: Vec<String>,
) -> Esito<NovitaDaMostrare> {
    let versione = app.package_info().version.to_string();
    let ci_sono = con_novita.contains(&versione);
    // Legge e, in un'installazione nuova, scrive: sul pool bloccante, come ogni
    // comando che chiede il lucchetto della libreria.
    let mostra = leggi_la_libreria(app, "novità da mostrare", {
        let versione = versione.clone();
        move |libreria| decidi(&libreria.connection, &versione, ci_sono)
    })
    .await
    .map_err(errore)?;
    Ok(NovitaDaMostrare { versione, mostra })
}

/// La decisione, su una connessione: legge, e in un'installazione nuova scrive.
fn decidi(
    connection: &rusqlite::Connection,
    versione: &str,
    ci_sono: bool,
) -> Result<bool, rymembra_domain::errors::AppError> {
    let visto = preferenze::novita_viste(connection)?;
    let vuota = preferenze::libreria_vuota(connection)?;
    match preferenze::novita_da_mostrare(visto.as_deref(), versione, vuota, ci_sono) {
        EsitoNovita::Mostra => Ok(true),
        EsitoNovita::Niente => Ok(false),
        EsitoNovita::Registra => {
            preferenze::imposta_novita_viste(connection, versione)?;
            Ok(false)
        }
    }
}

/// Segna le Novità di questa versione come viste.
///
/// La chiama la finestra quando si chiude, comunque si chiuda (l'ultimo passo,
/// «Chiudi», Esc): sono tutti modi di dire «le ho viste», e distinguerli
/// vorrebbe dire riproporle a chi le ha già chiuse una volta.
///
/// # Errori
///
/// `db.queryFailed`.
#[tauri::command]
pub async fn novita_viste(app: AppHandle, versione: String) -> Esito<()> {
    leggi_la_libreria(app, "novità viste", move |libreria| {
        preferenze::imposta_novita_viste(&libreria.connection, &versione)
    })
    .await
    .map_err(errore)
}

#[cfg(test)]
mod prove {
    use super::*;

    fn libreria() -> rusqlite::Connection {
        rymembra_app::db::open_in_memory()
            .expect("un database in memoria si apre sempre")
            .connection
    }

    fn con_un_brano(connection: &rusqlite::Connection) {
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     genre, duration_ms, file_size, date_added, date_modified)
                 VALUES (1, 'x.mp3', 'kx', 'x', 'x', 'x', 'x|x', 'x', 1000, 1, 1, 1)",
                [],
            )
            .expect("un brano");
    }

    /// §11.3.2 sul database: un'installazione nuova registra e non mostra; un
    /// aggiornamento mostra; già vista, no. E il database lo conferma.
    #[test]
    fn la_tabella_sul_database() {
        // Installazione nuova: niente, ma la versione si ricorda.
        let nuova = libreria();
        assert_eq!(decidi(&nuova, "3.2.1", true), Ok(false));
        assert_eq!(
            preferenze::novita_viste(&nuova),
            Ok(Some("3.2.1".to_owned())),
            "la versione non è stata registrata: il giorno dopo la finestra si aprirebbe"
        );
        assert_eq!(decidi(&nuova, "3.2.1", true), Ok(false));

        // Aggiornamento da una versione senza Novità: libreria piena, chiave
        // assente. Mostra, e **non** registra: si registra alla chiusura.
        let piena = libreria();
        con_un_brano(&piena);
        assert_eq!(decidi(&piena, "3.2.1", true), Ok(true));
        assert_eq!(preferenze::novita_viste(&piena), Ok(None));
        // Senza novità scritte per questa versione: niente, e non è un errore.
        assert_eq!(decidi(&piena, "3.2.1", false), Ok(false));

        // Dopo che le ha viste, non più; con una versione nuova, sì.
        preferenze::imposta_novita_viste(&piena, "3.2.1").expect("scrittura");
        assert_eq!(decidi(&piena, "3.2.1", true), Ok(false));
        assert_eq!(decidi(&piena, "3.3.0", true), Ok(true));
    }

    /// La domanda non scrive in un caso soltanto: l'installazione nuova.
    #[test]
    fn solo_l_installazione_nuova_scrive_nella_domanda() {
        let sorgente = include_str!("novita.rs");
        let corpo = sorgente
            .split("fn decidi(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert_eq!(
            corpo.matches("imposta_novita_viste(").count(),
            1,
            "la domanda scrive in più di un caso"
        );
        let registra = corpo
            .find("EsitoNovita::Registra")
            .expect("il ramo Registra");
        let scrive = corpo.find("imposta_novita_viste(").expect("la scrittura");
        assert!(registra < scrive, "la scrittura non sta nel ramo Registra");
    }

    /// I due comandi sono registrati.
    #[test]
    fn i_due_comandi_sono_registrati() {
        let lib = include_str!("lib.rs");
        for comando in ["novita::novita_da_mostrare,", "novita::novita_viste,"] {
            assert!(lib.contains(comando), "{comando} non è registrato");
        }
        assert!(lib.contains("mod novita;"));
    }
}
