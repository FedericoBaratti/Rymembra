//! Il lavoro che aspetta il disco si fa in disparte.
//!
//! # Il difetto che questo modulo toglie
//!
//! `#[tauri::command]` senza altro genera un comando **sincrono**: il corpo gira
//! sul filo principale, quello che pompa i messaggi della finestra. Su una
//! condivisione di rete che non risponde, una `File::open` lì dentro vale i
//! quaranta secondi di timeout di Windows, e in quei quaranta secondi la
//! finestra non si ridisegna: il sistema la dichiara «non risponde», e chi
//! guarda termina il processo.
//!
//! `#[tauri::command(async)]` sembra la risposta e non lo è del tutto. Su una
//! `fn` **non** asincrona, `tauri-macros` avvolge il corpo in un blocco `async`
//! e lo manda a `tauri::async_runtime::spawn`: il corpo bloccante finisce su un
//! *worker* del runtime, e i worker sono tanti quanti i processori. Il filo
//! principale è salvo — ed è già molto — ma tre o quattro comandi fermi su una
//! share morta saturano il runtime, e da lì in poi non parte più **nessun**
//! comando asincrono, nemmeno quelli che non toccano il disco.
//!
//! [`in_disparte`] li manda invece sul pool bloccante, che esiste per questo e
//! cresce fino a centinaia di fili: un lavoro fermo lì non toglie il posto a
//! nessuno.
//!
//! # Perché non basta `aether_app::scadenza::con_scadenza`
//!
//! Perché rispondono a due domande diverse. `con_scadenza` dice **quanto** si è
//! disposti ad aspettare, e alla scadenza lascia il lavoro indietro; questo dice
//! **dove** il lavoro gira, e aspetta comunque la fine. Servono tutte e due, e in
//! genere insieme: la scansione ha una scadenza per ogni file *e* gira qui,
//! perché senza la prima aspetterebbe per sempre un file solo, e senza la
//! seconda aspetterebbe occupando un posto che serve a tutti gli altri comandi.
//!
//! # Quanti comandi passano di qui
//!
//! Tre — `comandi::scansiona`, `comandi::cartelle_candidate`,
//! `testi::testo_brano` — e sono i tre che aprono file di cui non si sa niente,
//! in cartelle che l'utente ha scelto e che possono stare su una share. Gli altri
//! settantacinque `#[tauri::command(async)]` restano dove sono: spostarli tutti
//! insieme sarebbe un gesto che nessuna prova copre, e la maggior parte fa una
//! query di microsecondi. Il prossimo candidato è `riordino::esegui_riordino`,
//! che sposta file veri **dentro** il lucchetto della libreria: stesso difetto,
//! dominio diverso, e va con lo stesso schema completo (`Deposito`, [`Turno`], un
//! annullamento).
//!
//! [`Turno`]: crate::stato::Turno

use aether_domain::errors::{AppError, ErrorCode};

/// Esegue `lavoro` sul pool bloccante e ne aspetta l'esito.
///
/// `nome` è quel che finisce nella causa dell'errore se il lavoro non arriva mai
/// a un risultato, quindi va scritto per essere letto in un diario: «scansione»,
/// non «task 3».
///
/// # Errori
///
/// `internal.aborted` quando il join fallisce, cioè quando il lavoro è caduto in
/// panico o il runtime è stato spento sotto di lui. Un errore e non un panico
/// nostro: il gancio dei panici ha già scritto cos'era successo, e chi ha chiesto
/// il comando merita una risposta invece di una finestra che si chiude.
pub async fn in_disparte<T: Send + 'static>(
    nome: &'static str,
    lavoro: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(lavoro)
        .await
        .map_err(|err| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some(nome.to_owned()),
            })
            .with_cause(err.to_string())
        })
}
