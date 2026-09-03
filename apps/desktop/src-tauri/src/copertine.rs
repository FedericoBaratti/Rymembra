//! Servire le copertine alla finestra.
//!
//! Un protocollo dedicato invece di mandare i byte attraverso l'IPC. La
//! differenza non è di stile: una griglia di novecento album chiederebbe
//! novecento immagini, e passarle come JSON in base64 significherebbe
//! serializzarle, gonfiarle di un terzo, e tenerle tutte vive in memoria nel
//! processo della finestra. Con un protocollo le chiede il motore di rendering
//! come chiederebbe qualunque immagine: in parallelo, con la sua cache, e
//! scartando quelle che escono dallo schermo.
//!
//! L'indirizzo è `aether-cover://localhost/<impronta>` per la copertina piena e
//! `aether-cover://localhost/<impronta>.t` per la miniatura.
//!
//! # Perché l'impronta e non il percorso
//!
//! Un protocollo che accettasse un percorso sarebbe una lettura arbitraria di
//! file: la finestra chiede `../../qualcosa` e il processo nativo glielo legge.
//! Qui l'unico argomento è un'impronta, e il percorso lo compone lo store. Una
//! richiesta con dentro un separatore non è un percorso da normalizzare — è una
//! richiesta che non può essere legittima, e si rifiuta.

use tauri::http;
use tauri::{Manager as _, UriSchemeContext, UriSchemeResponder};

use crate::stato::Stato;

/// Un'impronta è esadecimale e basta.
///
/// Il controllo è sull'alfabeto e non sui separatori: elencare i caratteri
/// vietati è una lista da tenere aggiornata (`/`, `\`, `..`, `%2e`, gli
/// equivalenti Unicode…), elencare quelli permessi no.
///
/// Sta qui e non è privata perché il protocollo non è l'unica porta d'ingresso:
/// anche [`crate::skin::accento_copertina`] riceve un'impronta dalla finestra e
/// la dà allo store, che con essa compone un percorso. Due controlli scritti
/// due volte sono due controlli che possono divergere, e quello che divergesse
/// sarebbe una lettura di file arbitraria.
pub(crate) fn e_un_impronta(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn risposta(status: u16, tipo: &str, corpo: Vec<u8>) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, tipo)
        // Il contenuto è indirizzato dall'impronta: gli stessi byte daranno
        // sempre lo stesso indirizzo, quindi non c'è niente da rivalidare.
        .header(
            http::header::CACHE_CONTROL,
            "public, max-age=31536000, immutable",
        )
        .body(corpo)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

/// Risponde a una richiesta di copertina.
pub fn servi(
    ctx: UriSchemeContext<'_, tauri::Wry>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let percorso = request.uri().path().trim_start_matches('/').to_owned();
    let (impronta, miniatura) = match percorso.strip_suffix(".t") {
        Some(resto) => (resto.to_owned(), true),
        None => (percorso, false),
    };

    if !e_un_impronta(&impronta) {
        responder.respond(risposta(400, "text/plain", b"impronta non valida".to_vec()));
        return;
    }

    let app = ctx.app_handle();
    let Some(stato) = app.try_state::<Stato>() else {
        responder.respond(risposta(503, "text/plain", b"libreria non aperta".to_vec()));
        return;
    };

    let file = {
        let guardia = stato
            .libreria
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match guardia.as_ref() {
            Ok(libreria) => {
                if miniatura {
                    libreria.covers.thumbnail_path_for(&impronta)
                } else {
                    libreria.covers.path_for(&impronta)
                }
            }
            Err(_) => {
                responder.respond(risposta(503, "text/plain", b"libreria non aperta".to_vec()));
                return;
            }
        }
    };

    // Il lucchetto è già rilasciato, e la lettura va su un altro thread: non
    // deve tenere fermo né lo stato né chi ha mandato la richiesta.
    tauri::async_runtime::spawn_blocking(move || {
        match std::fs::read(&file) {
            Ok(bytes) => responder.respond(risposta(200, "image/jpeg", bytes)),
            // Una copertina che manca non è un guasto: è un album senza
            // immagine, e l'interfaccia ha già un ripiego da mostrare.
            Err(_) => responder.respond(risposta(404, "text/plain", Vec::new())),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_le_impronte_passano() {
        assert!(e_un_impronta("a3f9"));
        assert!(e_un_impronta(&"0".repeat(64)));
        // Tutto il resto è una richiesta che non può essere legittima.
        assert!(!e_un_impronta(""));
        assert!(!e_un_impronta("../../etc/passwd"));
        assert!(!e_un_impronta("a3f9/../x"));
        assert!(!e_un_impronta("a3f9.jpg"));
        assert!(!e_un_impronta("C:\\Windows"));
        assert!(!e_un_impronta(&"a".repeat(129)));
    }
}
