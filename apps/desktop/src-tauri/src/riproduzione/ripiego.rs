//! Il ripiego all'apertura: se la copia **in rete** di un brano non si apre,
//! si prova la copia **di qui** del gruppo.
//!
//! La scelta della copia (`rymembra_app::specchio::scelta`) decide con quel che
//! sa: la presenza di un dispositivo, che può essere vecchia — il PC si è
//! spento un momento fa e il filo non se n'è ancora accorto. Il rappresentante
//! del gruppo è remoto, la presenza dice «risponde», l'apertura non ci riesce:
//! senza questo il brano resta muto con una copia di qui nel gruppo, e la guida
//! (§4.6) dice il contrario. Non si ripete la scelta — sceglierebbe di nuovo la
//! copia in rete —: la copia di qui si cerca da sé
//! (`scelta::copia_di_qui`).

use rymembra_domain::errors::AppError;

/// L'esito di aprire una riga, con il ripiego se era in rete e non si è aperta.
///
/// - `prima`: l'esito dell'apertura della riga scelta;
/// - `in_rete`: la riga scelta è una riga specchio (di una di qui un errore è
///   un errore, e non c'è un altro posto da cui prenderla);
/// - `ripiego`: prova la copia di qui, e dice `None` se il gruppo non ne ha.
///   Si chiama solo quando serve.
///
/// Se il ripiego non c'è, o non riesce a sua volta, l'errore è quello della copia
/// in rete: è quello che racconta cosa è andato storto davvero («<PC> non
/// raggiungibile»), e la copia di qui è un tentativo, non la scelta.
///
/// # Errori
///
/// Quelli dell'apertura.
pub(super) fn con_ripiego<T>(
    prima: Result<T, AppError>,
    in_rete: bool,
    ripiego: impl FnOnce() -> Option<Result<T, AppError>>,
) -> Result<T, AppError> {
    match prima {
        Ok(aperto) => Ok(aperto),
        Err(errore) if in_rete => match ripiego() {
            Some(Ok(aperto)) => Ok(aperto),
            Some(Err(_)) | None => Err(errore),
        },
        Err(errore) => Err(errore),
    }
}

#[cfg(test)]
mod prove {
    use std::cell::Cell;

    use rymembra_domain::errors::{ErrorCode, ErrorCodeKind};

    use super::*;

    fn guasto(codice: ErrorCode) -> AppError {
        AppError::new(codice)
    }

    #[test]
    fn una_copia_che_si_apre_non_ha_bisogno_del_ripiego() {
        let chiamato = Cell::new(false);
        let aperto = con_ripiego(Ok(1), true, || {
            chiamato.set(true);
            Some(Ok(2))
        });
        assert_eq!(aperto.ok(), Some(1));
        assert!(!chiamato.get(), "il ripiego non si cerca se non serve");
    }

    #[test]
    fn la_copia_in_rete_che_non_si_apre_ripiega_sulla_copia_di_qui() {
        // Il caso che il revisore non ha trovato nel codice: rappresentante
        // remoto, presenza vecchia, apertura che fallisce.
        let aperto: Result<i32, AppError> =
            con_ripiego(Err(guasto(ErrorCode::LontanoIrraggiungibile)), true, || {
                Some(Ok(2))
            });
        assert_eq!(aperto.ok(), Some(2));
    }

    #[test]
    fn senza_una_copia_di_qui_l_errore_e_quello_della_copia_in_rete() {
        let aperto: Result<i32, AppError> =
            con_ripiego(Err(guasto(ErrorCode::LontanoIrraggiungibile)), true, || {
                None
            });
        assert_eq!(
            aperto.err().map(|e| e.code().kind()),
            Some(ErrorCodeKind::LontanoIrraggiungibile)
        );
        // E se anche la copia di qui non si apre, il racconto resta quello di prima.
        let aperto: Result<i32, AppError> =
            con_ripiego(Err(guasto(ErrorCode::LontanoIrraggiungibile)), true, || {
                Some(Err(guasto(ErrorCode::LontanoNonAssociato)))
            });
        assert_eq!(
            aperto.err().map(|e| e.code().kind()),
            Some(ErrorCodeKind::LontanoIrraggiungibile)
        );
    }

    #[test]
    fn una_copia_di_qui_che_non_si_apre_e_un_errore_e_basta() {
        let chiamato = Cell::new(false);
        let aperto: Result<i32, AppError> = con_ripiego(
            Err(guasto(ErrorCode::LontanoIrraggiungibile)),
            false,
            || {
                chiamato.set(true);
                Some(Ok(2))
            },
        );
        assert!(aperto.is_err());
        assert!(!chiamato.get(), "non c'è un altro posto da cui prenderla");
    }
}
