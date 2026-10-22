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
//! # Perché non basta `rymembra_app::scadenza::con_scadenza`
//!
//! Perché rispondono a due domande diverse. `con_scadenza` dice **quanto** si è
//! disposti ad aspettare, e alla scadenza lascia il lavoro indietro; questo dice
//! **dove** il lavoro gira, e aspetta comunque la fine. Servono tutte e due, e in
//! genere insieme: la scansione ha una scadenza per ogni file *e* gira qui,
//! perché senza la prima aspetterebbe per sempre un file solo, e senza la
//! seconda aspetterebbe occupando un posto che serve a tutti gli altri comandi.
//!
//! # Quali comandi passano di qui
//!
//! Ogni comando che aspetta qualcosa che non è la CPU: un file in una cartella
//! che l'utente ha scelto e che può stare su una share — la scansione, il
//! pannello «Cartelle», il profilo, il testo accanto al brano — e ogni comando
//! che parla con la rete: le importazioni da un link o da un archivio, la
//! ricerca nei cataloghi, i testi, la conversazione con l'IA, la cronologia
//! mandata a ListenBrainz. Un elenco scritto qui invecchierebbe al primo
//! comando spostato, ed era successo: la fonte è `grep 'in_disparte("'`.
//!
//! Il caso peggiore è il profilo: apre un file che l'utente ha scelto, e le
//! due destinazioni naturali di un profilo sono una chiavetta e una cartella di
//! rete. Un archivio da trecento megabyte scritto su una share che smette di
//! rispondere è la scansione lenta, moltiplicata per la quantità di byte.
//!
//! # E le letture lunghe della libreria
//!
//! Dalla 3.0 passano di qui anche i comandi che **leggono molto**, attraverso
//! [`leggi_la_libreria`]: gli elenchi di brani, album e artisti, la ricerca e i
//! suoi conteggi, la cronologia, la Home, l'avvio, e le playlist — l'elenco con
//! i conteggi delle intelligenti, i loro brani, l'anteprima delle regole. Non
//! aspettano il disco, ma su cinquantamila brani l'elenco degli artisti o una
//! ricerca per prefisso durano decine di millisecondi, e un comando sincrono
//! quei millisecondi li toglie al filo che ridisegna la finestra: una lista che
//! scorre a scatti mentre la pagina successiva arriva. La regola che ne esce è
//! semplice: un comando che legge più di una riga per chiave non è sincrono.
//!
//! Gli altri `#[tauri::command(async)]` restano dove sono: spostarli tutti
//! insieme sarebbe un gesto che nessuna prova copre, e la maggior parte fa una
//! query di microsecondi. Fra quelli che toccano ancora il disco o la rete
//! restano `arricchimento::arricchimento_annulla`,
//! `arricchimento::arricchimento_riporta_nei_file`, `testi::testo_candidati`,
//! `playlist::playlist_file_piano` e `playlist::playlist_esporta`.
//! `arricchimento_riporta_nei_file` non tiene più il lucchetto della libreria
//! mentre riscrive i file — lo lascia fra una fase e l'altra — ma il suo corpo
//! gira ancora su un worker del runtime.

use rymembra_domain::errors::{AppError, ErrorCode};

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

/// Una lettura della libreria, in disparte: [`in_disparte`] più il lucchetto.
///
/// È la forma di quasi tutti i comandi che leggono molto, scritta una volta:
/// prendere lo stato dal registro di Tauri, prendere il lucchetto, leggere.
/// Prende un `AppHandle` e non uno `State` per la ragione scritta su
/// `comandi::cartelle_candidate`: quel che va sul pool dev'essere `'static`,
/// e un prestito dello stato lì dentro non entra.
///
/// # Errori
///
/// `internal.aborted` se lo stato non c'è più — la finestra si sta chiudendo —
/// o se la lettura cade; altrimenti quello della lettura.
pub async fn leggi_la_libreria<T: Send + 'static>(
    app: tauri::AppHandle,
    nome: &'static str,
    lettura: impl FnOnce(&mut crate::stato::Libreria) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    in_disparte(nome, move || {
        use tauri::Manager as _;
        // `try_state` e non `state`: questo corpo gira sul pool bloccante, che
        // può ritrovarsi vivo mentre gli stati gestiti vengono lasciati cadere.
        let stato = app.try_state::<crate::stato::Stato>().ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some(nome.to_owned()),
            })
            .with_cause("la libreria non è più fra gli stati gestiti")
        })?;
        crate::stato::con_libreria(&stato, lettura)
    })
    .await?
}
