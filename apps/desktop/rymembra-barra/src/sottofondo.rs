//! Il lavoro che nessuno ha chiesto, a priorità di sottofondo; e la domanda «si
//! va a batteria?».
//!
//! # Perché sta qui
//!
//! Dopo una scansione l'applicazione misura impronte e volume di tutta la
//! libreria: ore di decodifica che nessuno ha chiesto, su due o tre fili. La
//! passata cede già alla musica — si ferma mentre suona — ma col resto del
//! sistema girava a priorità normale, e a batteria come a corrente: misurato,
//! due processori occupati per minuti, e su una libreria vera per ore (R21-5).
//!
//! Windows ha una modalità fatta apposta, `THREAD_MODE_BACKGROUND_BEGIN`: il
//! filo che la chiede scende di priorità per il processore **e** per il disco e
//! la memoria, e cede a qualunque programma in primo piano. Dire se si va a
//! batteria lo sa `GetSystemPowerStatus`. Sono due chiamate `unsafe`, e la radice
//! del workspace vieta `unsafe` con `forbid`: stanno qui, nell'unico crate con
//! l'eccezione dichiarata, dietro un'interfaccia che `unsafe` non lo chiede a
//! nessuno. Quel che se ne fa — quali fili, quanti — lo decide chi chiama.
//!
//! # Solo chi lo chiede
//!
//! La modalità vale per **il filo che chiama**, e solo finché vive la guardia
//! [`Sottofondo`]: non si attacca a un processo intero, e non arriva mai da sola
//! al filo della callback audio o a quello della decodifica — che non la
//! chiedono, e non devono. La guardia non attraversa i fili (non è `Send`): la
//! modalità si toglie sul filo che l'ha messa, come Windows pretende.
//!
//! Fuori da Windows non fa niente, e la batteria non si sa: sul telefono la
//! passata non c'è, e altrove non c'è una piattaforma che la usi.

use std::marker::PhantomData;

/// Il filo che l'ha chiesta lavora a priorità di sottofondo finché vive.
///
/// Si prende all'inizio del lavoro e si lascia cadere alla fine, anche in un
/// panico: la modalità si toglie nel `Drop`, e il filo — che può servire ad
/// altro dopo — torna com'era.
///
/// Una guardia presa mentre il filo è già in sottofondo non fa niente, né
/// prendendola né lasciandola: Windows rifiuta di entrarci due volte, e toglierla
/// alla guardia di dentro la toglierebbe anche a quella di fuori.
#[derive(Debug)]
#[must_use = "la modalità dura quanto la guardia: lasciata cadere subito, non resta niente"]
pub struct Sottofondo {
    /// La modalità l'ha messa questa guardia, e la toglie lei.
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "fuori da Windows non si mette niente")
    )]
    entrata: bool,
    /// Né `Send` né `Sync`: la modalità è del filo, e si toglie su quello.
    _del_filo: PhantomData<*const ()>,
}

impl Sottofondo {
    /// Mette il filo che chiama a priorità di sottofondo: processore, disco e
    /// memoria. Se Windows rifiuta — il filo c'è già — la guardia è vuota.
    pub fn entra() -> Self {
        Self {
            entrata: entra_nel_sottofondo(),
            _del_filo: PhantomData,
        }
    }
}

impl Drop for Sottofondo {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.entrata {
            esci_dal_sottofondo();
        }
    }
}

/// Si va a batteria adesso? `None` quando non si sa: fuori da Windows, o se
/// Windows non risponde, o se lui stesso non lo sa.
///
/// Una lettura di una struttura del sistema: si può chiedere a ogni lotto.
#[must_use]
pub fn a_batteria() -> Option<bool> {
    #[cfg(windows)]
    {
        stato_della_corrente().and_then(a_batteria_da)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Il campo `ACLineStatus` di `SYSTEM_POWER_STATUS`, tradotto.
///
/// Zero è «scollegato dalla corrente», cioè a batteria; uno è «collegato»; 255
/// è «non si sa». Un PC fisso senza batteria risponde uno. Ogni altro numero
/// non è documentato, e vale «non si sa».
#[cfg_attr(
    not(any(windows, test)),
    allow(dead_code, reason = "la legge solo la chiamata di Windows")
)]
const fn a_batteria_da(ac_line_status: u8) -> Option<bool> {
    match ac_line_status {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

/// `SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN)`.
/// `true` se la modalità l'ha messa questa chiamata.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "GetCurrentThread e SetThreadPriority non hanno un'interfaccia sicura: \
              ogni blocco dice perché è corretto"
)]
fn entra_nel_sottofondo() -> bool {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
    };
    // SAFETY: `GetCurrentThread` non ha precondizioni: restituisce la
    // pseudo-maniglia del filo che chiama, che non va chiusa e vale finché il
    // filo vive — cioè per tutta la chiamata qui sotto. `SetThreadPriority`
    // riceve quella maniglia e una costante documentata: non legge né scrive
    // memoria nostra. Se il filo è già in sottofondo risponde con un errore,
    // e allora questa guardia non ha messo niente.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN).is_ok() }
}

#[cfg(not(windows))]
const fn entra_nel_sottofondo() -> bool {
    false
}

/// `SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END)`.
///
/// Un errore qui non ha nessuno a cui dirlo: arriva dentro un `Drop`, e il
/// peggio che succede è un filo di sottofondo che resta tale.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "GetCurrentThread e SetThreadPriority non hanno un'interfaccia sicura: \
              ogni blocco dice perché è corretto"
)]
fn esci_dal_sottofondo() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_END,
    };
    // SAFETY: come in `entra_nel_sottofondo`: la pseudo-maniglia del filo che
    // chiama — lo stesso che è entrato, perché la guardia non è `Send` — e una
    // costante documentata. Nessuna memoria nostra passa al sistema.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END) };
}

/// `GetSystemPowerStatus`, e il suo `ACLineStatus`. `None` se Windows non
/// risponde.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "GetSystemPowerStatus non ha un'interfaccia sicura: il blocco dice \
              perché è corretto"
)]
fn stato_della_corrente() -> Option<u8> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut stato = SYSTEM_POWER_STATUS::default();
    // SAFETY: `stato` è una `SYSTEM_POWER_STATUS` nostra, viva per tutta la
    // chiamata, e il puntatore che si passa è il suo: Windows ci scrive i suoi
    // campi, una struttura di dimensione fissa, e non la conserva.
    let letto = unsafe { GetSystemPowerStatus(&raw mut stato) };
    letto.is_ok().then_some(stato.ACLineStatus)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_corrente_di_windows_si_legge() {
        assert_eq!(a_batteria_da(0), Some(true));
        assert_eq!(a_batteria_da(1), Some(false));
        assert_eq!(a_batteria_da(255), None);
        assert_eq!(a_batteria_da(7), None);
    }

    /// Entrare, rientrare e uscire non cadono, e la guardia di dentro non
    /// toglie la modalità a quella di fuori.
    #[test]
    fn la_guardia_si_prende_e_si_lascia() {
        let fuori = Sottofondo::entra();
        let dentro = Sottofondo::entra();
        assert!(
            !dentro.entrata,
            "una seconda guardia sullo stesso filo non entra"
        );
        drop(dentro);
        drop(fuori);
        // E dopo si può rientrare.
        let ancora = Sottofondo::entra();
        assert_eq!(ancora.entrata, cfg!(windows));
    }

    /// La domanda sulla batteria non cade, e fuori da Windows non sa.
    #[test]
    fn la_batteria_si_chiede() {
        let risposta = a_batteria();
        if !cfg!(windows) {
            assert_eq!(risposta, None);
        }
    }
}
