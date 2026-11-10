//! Gli errori del collegamento, e come viaggiano.
//!
//! Come per «Ascolta insieme», fra due dispositivi viaggia soltanto il codice
//! di un rifiuto (`lontano.rifiutato`), mai una frase: chi lo riceve lo dice
//! nella propria lingua, e un dispositivo non può far comparire un testo suo
//! sullo schermo dell'altro.

use std::sync::{Mutex, MutexGuard, PoisonError};

use rymembra_domain::errors::{AppError, Domain, ErrorCode, ErrorCodeKind};

/// L'errore di un codice `lontano.*`.
#[must_use]
pub fn errore(kind: ErrorCodeKind) -> AppError {
    AppError::new(codice_di(kind))
}

/// Il codice con i suoi parametri: i codici di questo dominio non ne hanno.
fn codice_di(kind: ErrorCodeKind) -> ErrorCode {
    match kind {
        ErrorCodeKind::LontanoIrraggiungibile => ErrorCode::LontanoIrraggiungibile,
        ErrorCodeKind::LontanoNonAssociato => ErrorCode::LontanoNonAssociato,
        ErrorCodeKind::LontanoGettoneScaduto => ErrorCode::LontanoGettoneScaduto,
        ErrorCodeKind::LontanoLinkNonValido => ErrorCode::LontanoLinkNonValido,
        ErrorCodeKind::LontanoRifiutato => ErrorCode::LontanoRifiutato,
        ErrorCodeKind::LontanoFileCambiato => ErrorCode::LontanoFileCambiato,
        ErrorCodeKind::LontanoReteAConsumo => ErrorCode::LontanoReteAConsumo,
        ErrorCodeKind::LontanoSpento => ErrorCode::LontanoSpento,
        ErrorCodeKind::LontanoIdentitaPersa => ErrorCode::LontanoIdentitaPersa,
        // `ErrorCodeKind` è `non_exhaustive`, e qui passano solo codici del
        // dominio (vedi `da_testo`): un altro è un errore di chi chiama.
        _ => ErrorCode::LontanoProtocollo,
    }
}

/// Il codice arrivato dalla rete, se è uno del dominio `Lontano`. Un codice di
/// un'altra area, o di una versione più nuova, è `None`.
#[must_use]
pub fn da_testo(codice: &str) -> Option<ErrorCodeKind> {
    ErrorCodeKind::ALL
        .iter()
        .copied()
        .find(|kind| kind.domain() == Domain::Lontano && kind.code() == codice)
}

/// L'errore che un rifiuto arrivato dalla rete vuol dire: il suo codice, se
/// lo si conosce, altrimenti `lontano.protocollo`.
#[must_use]
pub fn dal_rifiuto(codice: &str) -> AppError {
    da_testo(codice).map_or_else(
        || {
            AppError::new(ErrorCode::LontanoProtocollo)
                .with_cause(format!("rifiuto con un codice sconosciuto: {codice}"))
        },
        errore,
    )
}

/// L'altro non si raggiunge: con la causa per il diario, mai per lo schermo.
pub(crate) fn irraggiungibile(causa: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::LontanoIrraggiungibile).with_cause(causa.to_string())
}

/// L'altro ha mandato qualcosa che non si capisce.
pub(crate) fn protocollo(causa: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::LontanoProtocollo).with_cause(causa.to_string())
}

/// Un guasto interno che non ha un codice suo.
pub(crate) fn inatteso(causa: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::InternalUnexpected { detail: None }).with_cause(causa.to_string())
}

/// Il lucchetto, anche se un altro filo è caduto tenendolo: lo stato che
/// protegge resta coerente fra un'istruzione e l'altra.
pub(crate) fn chiave<T>(lucchetto: &Mutex<T>) -> MutexGuard<'_, T> {
    lucchetto.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn ogni_codice_del_dominio_torna_indietro_uguale() {
        let mut quanti = 0;
        for kind in ErrorCodeKind::ALL
            .iter()
            .filter(|k| k.domain() == Domain::Lontano)
        {
            quanti += 1;
            assert_eq!(da_testo(kind.code()), Some(*kind));
            assert_eq!(
                errore(*kind).code().kind(),
                *kind,
                "{}: un codice nuovo del dominio va aggiunto a `codice_di`",
                kind.code()
            );
        }
        assert!(quanti >= 9);
        assert_eq!(da_testo("insieme.rifiutato"), None);
        assert_eq!(
            dal_rifiuto("lontano.domani").code().kind(),
            ErrorCodeKind::LontanoProtocollo
        );
    }
}
