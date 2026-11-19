//! Il «vassoio», su Android: non c'è.
//!
//! Sul computer l'icona nell'area di notifica è il modo di tornare a una
//! finestra nascosta mentre la musica continua. Su Android quel posto lo tiene
//! la notifica del lettore, e la finestra non si chiude con una X: si lascia
//! con il tasto Home o con Indietro, e il sistema decide quando portarla via.
//!
//! Resta la preferenza «secondo piano», letta e scritta come sul computer:
//! viaggia con il profilo e con la sincronia, e un telefono che la azzerasse
//! la azzererebbe anche al computer con cui si sincronizza.
//!
//! Le firme sono quelle di `crate::vassoio` sul computer: vedi `lib.rs`.

use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::riproduzione::StatoRiproduzione;
use crate::stato::{Stato, con_libreria};

/// Lo stato del vassoio: niente da tenere.
pub struct StatoVassoio;

impl StatoVassoio {
    /// Uno stato vuoto.
    #[must_use]
    pub const fn nuovo() -> Self {
        Self
    }
}

/// La chiusura deve nascondere invece di chiudere? Su Android non c'è una
/// chiusura da intercettare: no.
pub const fn nasconde(_app: &tauri::AppHandle) -> bool {
    false
}

/// Riporta davanti la finestra.
///
/// Su Android la finestra è l'attività, e davanti la riporta chi la tocca; qui
/// si fa soltanto il gesto che esiste su tutte le piattaforme.
pub(crate) fn mostra(app: &tauri::AppHandle) {
    if let Some(finestra) = app.get_webview_window("main") {
        let _ = finestra.show();
    }
}

/// Toglie l'icona all'uscita: non c'è icona.
pub const fn stacca(_app: &tauri::AppHandle) {}

/// Fa comparire l'icona se la preferenza lo chiede: non c'è icona.
pub const fn avvia(_app: &tauri::AppHandle) {}

/// Se il tasto di chiusura manda in secondo piano.
#[tauri::command]
pub async fn secondo_piano(app: tauri::AppHandle) -> Esito<bool> {
    crate::disparte::leggi_la_libreria(app, "secondo piano", |libreria| {
        rymembra_app::preferenze::secondo_piano(&libreria.connection)
    })
    .await
    .map_err(errore)
}

/// Accende o spegne il secondo piano. Riporta com'è rimasto, riletto.
#[tauri::command(async)]
pub fn secondo_piano_attiva(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    attivo: bool,
) -> Esito<bool> {
    let _ = app;
    con_libreria(&stato, |libreria| {
        rymembra_app::preferenze::imposta_secondo_piano(&libreria.connection, attivo)?;
        rymembra_app::preferenze::secondo_piano(&libreria.connection)
    })
    .map_err(errore)
}

/// Le etichette del menù del vassoio. Non c'è menù.
#[tauri::command(async)]
pub fn vassoio_lingua(
    app: tauri::AppHandle,
    mostra: String,
    esci: String,
    riproduci: Option<String>,
    pausa: Option<String>,
    successivo: Option<String>,
    precedente: Option<String>,
) {
    let _ = (app, mostra, esci, riproduci, pausa, successivo, precedente);
}

/// Rispecchia lo stato del lettore nell'icona. Non c'è icona.
pub(crate) const fn riflette(_app: &tauri::AppHandle, _stato: &StatoRiproduzione) {}
