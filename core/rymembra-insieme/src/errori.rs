//! Da codice del catalogo a errore, e ritorno: la sola forma in cui un perché
//! attraversa la rete.
//!
//! Sul filo un rifiuto è `ErrorCodeKind::code()` e basta (`Chiudo`,
//! `LetturaRifiutata`): mai una frase, perché chi lo riceve lo traduce nella
//! sua lingua, e un PC estraneo non deve poter scrivere sullo schermo di un
//! altro. Di ritorno si accettano **solo** i codici del dominio `Insieme`: un
//! `fs.notFound` arrivato da fuori sarebbe un PC che racconta i fatti del
//! proprio disco, e non è affar nostro.

use std::sync::{Mutex, MutexGuard, PoisonError};

use rymembra_domain::errors::{AppError, Domain, ErrorCode, ErrorCodeKind};

use crate::messaggi::{MASSIMO_PERSONE, MASSIMO_VOCI, VERSIONE};

/// L'errore di un codice del dominio `Insieme`, coi parametri di ripiego.
///
/// I parametri che qui non si conoscono — le due versioni, la porta, il gesto
/// — restano a zero o vuoti: chi ha quelle informazioni costruisce l'errore
/// da sé (vedi `Rete::ospita`), e chi riceve un codice dalla rete non le ha.
pub(crate) fn errore(kind: ErrorCodeKind) -> AppError {
    AppError::new(codice_di(kind))
}

/// Il codice con i parametri di ripiego. Vedi [`errore`].
fn codice_di(kind: ErrorCodeKind) -> ErrorCode {
    match kind {
        ErrorCodeKind::InsiemeIrraggiungibile => ErrorCode::InsiemeIrraggiungibile,
        ErrorCodeKind::InsiemeNonAssociato => ErrorCode::InsiemeNonAssociato,
        ErrorCodeKind::InsiemeCodiceRifiutato => ErrorCode::InsiemeCodiceRifiutato,
        ErrorCodeKind::InsiemeTroppiTentativi => ErrorCode::InsiemeTroppiTentativi {
            riprova_fra_ms: None,
        },
        ErrorCodeKind::InsiemeAssociazioneScaduta => ErrorCode::InsiemeAssociazioneScaduta,
        ErrorCodeKind::InsiemeAssociazioneOccupata => ErrorCode::InsiemeAssociazioneOccupata,
        ErrorCodeKind::InsiemeEstraneoScaduto => ErrorCode::InsiemeEstraneoScaduto,
        ErrorCodeKind::InsiemeSuperato => ErrorCode::InsiemeSuperato,
        ErrorCodeKind::InsiemeSessionePiena => ErrorCode::InsiemeSessionePiena {
            massimo: u32::try_from(MASSIMO_PERSONE).unwrap_or(u32::MAX),
        },
        ErrorCodeKind::InsiemeVersione => ErrorCode::InsiemeVersione {
            loro: 0,
            nostra: VERSIONE,
        },
        ErrorCodeKind::InsiemePortaOccupata => ErrorCode::InsiemePortaOccupata { porta: 0 },
        ErrorCodeKind::InsiemeFileCambiato => ErrorCode::InsiemeFileCambiato,
        ErrorCodeKind::InsiemeGettoneScaduto => ErrorCode::InsiemeGettoneScaduto,
        ErrorCodeKind::InsiemeConnessioneCaduta => ErrorCode::InsiemeConnessioneCaduta,
        ErrorCodeKind::InsiemeIdentitaPersa => ErrorCode::InsiemeIdentitaPersa,
        ErrorCodeKind::InsiemeGestoNonDisponibile => ErrorCode::InsiemeGestoNonDisponibile {
            gesto: String::new(),
        },
        ErrorCodeKind::InsiemeNonConcesso => ErrorCode::InsiemeNonConcesso,
        ErrorCodeKind::InsiemeRifiutato => ErrorCode::InsiemeRifiutato,
        ErrorCodeKind::InsiemeCodaPiena => ErrorCode::InsiemeCodaPiena {
            massimo: u32::try_from(MASSIMO_VOCI).unwrap_or(u32::MAX),
            fuori: 0,
        },
        ErrorCodeKind::InsiemeGiaInSessione => ErrorCode::InsiemeGiaInSessione,
        ErrorCodeKind::InsiemeSpenta => ErrorCode::InsiemeSpenta,
        // `ErrorCodeKind` è `non_exhaustive`, e da qui passano solo codici del
        // dominio `Insieme` (vedi `da_testo`): un altro codice è un errore di
        // chi chiama, e il più onesto da dare è «protocollo».
        _ => ErrorCode::InsiemeProtocollo,
    }
}

/// Il codice ricevuto dalla rete, se è uno del dominio `Insieme`.
///
/// `None` per tutto il resto: un codice di un'altra area, o uno di una
/// versione più nuova che questa non conosce. Chi chiama decide il ripiego.
pub(crate) fn da_testo(codice: &str) -> Option<ErrorCodeKind> {
    ErrorCodeKind::ALL
        .iter()
        .copied()
        .find(|kind| kind.domain() == Domain::Insieme && kind.code() == codice)
}

/// La lettura annullata da chi la aspettava: `internal.aborted`, che è `Info`.
pub(crate) fn annullata() -> AppError {
    AppError::new(ErrorCode::InternalAborted {
        what: Some("lettura insieme".to_owned()),
    })
}

/// Un guasto interno che non ha un codice suo: un filo che non parte, il
/// generatore del sistema che non risponde.
pub(crate) fn inatteso(causa: &str) -> AppError {
    AppError::new(ErrorCode::InternalUnexpected { detail: None }).with_cause(causa.to_owned())
}

/// Il lucchetto, anche se un altro filo è caduto tenendolo.
///
/// Un lucchetto avvelenato qui non è un guasto da propagare: lo stato che
/// protegge (elenchi, contatori, code) resta coerente fra un'istruzione e
/// l'altra, e far cadere tutta la rete perché un filo è caduto vorrebbe dire
/// fermare la musica di tutti per il guasto di uno.
pub(crate) fn chiave<T>(lucchetto: &Mutex<T>) -> MutexGuard<'_, T> {
    lucchetto.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn ogni_codice_del_dominio_torna_indietro_uguale() {
        for kind in ErrorCodeKind::ALL
            .iter()
            .filter(|k| k.domain() == Domain::Insieme)
        {
            assert_eq!(da_testo(kind.code()), Some(*kind));
            assert_eq!(
                errore(*kind).code().kind(),
                *kind,
                "{}: un codice nuovo del dominio va aggiunto a `codice_di`",
                kind.code()
            );
        }
    }

    #[test]
    fn un_codice_di_un_altra_area_non_passa() {
        assert_eq!(da_testo("fs.notFound"), None);
        assert_eq!(da_testo("internal.unexpected"), None);
        assert_eq!(da_testo("insieme.inventato"), None);
        assert_eq!(da_testo(""), None);
    }
}
