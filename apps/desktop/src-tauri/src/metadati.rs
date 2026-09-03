//! I brani con i metadati messi male: vederli, correggerli, chiudere la domanda.
//!
//! # La forma è quella del riordino
//!
//! `piano → esegui → annulla` è già il vocabolario dell'applicazione per le
//! operazioni su cui l'utente deve poter dire di no. Qui la stessa forma diventa
//! `elenco → correggi/conferma`, e l'annullamento non serve perché non si tocca
//! niente sul disco: la correzione vive in `track_overrides` e si disfa
//! riscrivendola.
//!
//! # Perché i comandi sono quattro e non uno
//!
//! Perché le risposte che l'utente dà sono due, e sono diverse. «Questo campo è
//! sbagliato, ecco quello giusto» è [`metadati_correggi`]; «la deduzione è
//! giusta, non chiedermelo più» è [`metadati_conferma`], ed è di gran lunga la
//! più frequente — la maggior parte delle deduzioni sono corrette, e senza un
//! gesto per dirlo l'elenco non si svuoterebbe mai.

use aether_app::incerti::{self, TracciaIncerta};
use aether_app::provenienza::Correzioni;
use tauri::State;

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Quanti brani aspettano che qualcuno li guardi.
///
/// Serve alla pastiglia sul pulsante: un numero che si legge senza aprire
/// niente, come `arricchimento_stato`.
#[tauri::command]
pub fn metadati_conteggio(stato: State<'_, Stato>) -> Esito<i64> {
    con_libreria(&stato, |libreria| incerti::quanti(&libreria.connection)).map_err(errore)
}

/// Una pagina di brani da guardare.
///
/// `(async)`: legge fino a `limite` righe con il loro JSON di provenienza, e su
/// una libreria appena importata da un disco esterno quelle righe sono molte.
#[tauri::command(async)]
pub fn metadati_incerti(
    stato: State<'_, Stato>,
    offset: i64,
    limite: i64,
) -> Esito<Vec<TracciaIncerta>> {
    con_libreria(&stato, |libreria| {
        incerti::elenco(&libreria.connection, offset, limite)
    })
    .map_err(errore)
}

/// Scrive la correzione dell'utente su un brano.
///
/// `(async)`: ricostruisce gli aggregati, che su una libreria grande è la parte
/// lenta — un artista o un album appena corretti devono esistere come schede, e
/// farlo sul filo della finestra la bloccherebbe.
#[tauri::command(async)]
pub fn metadati_correggi(
    stato: State<'_, Stato>,
    id: i64,
    campi: Correzioni,
) -> Esito<Option<TracciaIncerta>> {
    con_libreria(&stato, |libreria| {
        incerti::correggi(&mut libreria.connection, id, &campi)?;
        incerti::uno(&libreria.connection, id)
    })
    .map_err(errore)
}

/// «La deduzione è giusta»: si tiene quel che c'è e non se ne parla più.
///
/// `(async)` per la stessa ragione di [`metadati_correggi`].
#[tauri::command(async)]
pub fn metadati_conferma(stato: State<'_, Stato>, id: i64) -> Esito<()> {
    con_libreria(&stato, |libreria| {
        incerti::conferma(&mut libreria.connection, id)
    })
    .map_err(errore)
}
