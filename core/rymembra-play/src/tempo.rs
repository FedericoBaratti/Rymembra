//! L'orologio del processo, in nanosecondi.
//!
//! # Perché un orologio nostro
//!
//! Perché per stare a tempo con un altro computer servono degli istanti che si
//! possano **scrivere**: mandare in rete, confrontare, sottrarre, conservare in
//! un atomico che la callback audio legge senza lucchetti. `Instant` non è
//! niente di tutto questo — è opaco, non ha un'epoca, e la sua sola aritmetica è
//! fra due `Instant`. Qui ogni istante diventa un `u64` di nanosecondi contati
//! da [`base`], il primo istante che qualcuno ha chiesto in questo processo.
//!
//! Tutte le ancore del motore e tutti gli istanti dell'orologio fra due PC
//! parlano questa lingua: un numero solo, sempre sulla stessa base. Il
//! trasporto di rete non dipende da questo crate — riceverà [`adesso_ns`] come
//! una `fn() -> u64` da chi lo monta — e proprio per questo la base deve essere
//! **una** per processo.
//!
//! # Perché la callback la può leggere
//!
//! Perché dopo la prima chiamata [`base`] è la lettura di un `OnceLock` già
//! pieno — un atomico, nessun lucchetto — e `Instant::now` su Windows è
//! `QueryPerformanceCounter`, che non alloca e non aspetta nessuno. La prima
//! chiamata invece **inizializza**, e un'inizializzazione può aspettare un
//! altro filo: per questo `Motore::avvia_con_annunci` la fa prima di aprire
//! l'uscita, e la callback non può mai essere la prima.
//!
//! Un `u64` di nanosecondi arriva a cinquecentottanta anni dalla base: il tetto
//! non è un problema di nessuna sessione d'ascolto.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// L'istante zero di questo processo.
static BASE: OnceLock<Instant> = OnceLock::new();

/// L'istante da cui si contano tutti i nanosecondi di questo modulo.
///
/// Si prende alla prima chiamata e non cambia più.
#[must_use]
pub fn base() -> Instant {
    *BASE.get_or_init(Instant::now)
}

/// I nanosecondi passati da [`base`] a adesso.
#[must_use]
pub fn adesso_ns() -> u64 {
    ns_di(Instant::now())
}

/// I nanosecondi di un istante, contati da [`base`].
///
/// Un istante **prima** della base vale zero invece di traboccare: succede solo
/// a chi ha preso un `Instant` prima che qualcuno chiedesse la base, e per lui
/// «all'inizio» è la risposta meno sbagliata.
#[must_use]
pub fn ns_di(istante: Instant) -> u64 {
    let passati = istante.saturating_duration_since(base());
    u64::try_from(passati.as_nanos()).unwrap_or(u64::MAX)
}

/// L'istante che corrisponde a tanti nanosecondi dalla [`base`].
///
/// Oltre quel che `Instant` sa rappresentare — mai, in pratica — si resta
/// sulla base invece di cadere: qui un panico sarebbe peggio di un istante
/// sbagliato.
#[must_use]
pub fn istante_di(ns: u64) -> Instant {
    let base = base();
    base.checked_add(Duration::from_nanos(ns)).unwrap_or(base)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_istante_va_e_torna_senza_perdere_niente() {
        let adesso = Instant::now();
        let ns = ns_di(adesso);
        assert_eq!(istante_di(ns), adesso);
        assert_eq!(ns_di(istante_di(123_456_789)), 123_456_789);
    }

    #[test]
    fn prima_della_base_si_conta_zero() {
        let base = base();
        if let Some(prima) = base.checked_sub(Duration::from_millis(5)) {
            assert_eq!(ns_di(prima), 0);
        }
        assert_eq!(ns_di(base), 0);
    }

    #[test]
    fn l_orologio_non_torna_indietro() {
        let mut prima = adesso_ns();
        for _ in 0..1_000 {
            let dopo = adesso_ns();
            assert!(dopo >= prima, "{dopo} dopo {prima}");
            prima = dopo;
        }
    }
}
