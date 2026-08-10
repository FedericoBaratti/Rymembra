//! Quando un access token è da rinfrescare.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Con quanto anticipo si rinfresca un token che sta per scadere.
///
/// Un minuto: il tempo che una passata di lavoro può metterci fra il momento in
/// cui controlla la scadenza e quello in cui fa l'ultima richiesta. Senza
/// anticipo, un token valido «ancora due secondi» supererebbe il controllo e
/// darebbe un 401 a metà caricamento.
pub const ANTICIPO: Duration = Duration::from_secs(60);

/// L'access token è ancora buono fra un minuto?
///
/// Il minuto di anticipo è il punto: vedi [`ANTICIPO`].
#[must_use]
pub fn ancora_valido(scade_ms: i64) -> bool {
    let anticipo = i64::try_from(ANTICIPO.as_millis()).unwrap_or(60_000);
    adesso_ms().saturating_add(anticipo) < scade_ms
}

/// L'orologio, in millisecondi dall'epoca.
#[must_use]
pub fn adesso_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|quanto| i64::try_from(quanto.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_token_che_scade_fra_poco_e_gia_da_rinfrescare() {
        // Il minuto di anticipo: senza, un token «valido ancora due secondi»
        // supererebbe il controllo e darebbe un 401 a metà caricamento.
        assert!(!ancora_valido(adesso_ms() + 30_000));
        assert!(ancora_valido(adesso_ms() + 120_000));
        assert!(!ancora_valido(adesso_ms() - 1));
    }
}
