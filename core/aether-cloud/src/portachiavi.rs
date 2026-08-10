//! Dove sta il token di aggiornamento: nel portachiavi del sistema, non nel
//! database.
//!
//! Lo dice lo schema — `001_baseline.sql`, sezione «impostazioni»: *«I SEGRETI
//! non stanno qui»* — e in questa funzione la regola smette di essere igiene e
//! diventa concreta. `aether.db` sta in `%APPDATA%`, leggibile da qualunque
//! processo giri come l'utente. E stiamo scrivendo una funzione di **backup**:
//! il giorno in cui a qualcuno venisse in mente di salvare anche il database,
//! un token scritto lì partirebbe per Drive dentro il backup di sé stesso.
//!
//! # Perché un tratto e non `keyring` e basta
//!
//! Il precedente diretto è `MusicFiles` in `aether_app::playback`, indirezione
//! nata per lo stesso motivo: quel che su Windows è il Credential Manager, su
//! Android è il Keystore, e su una macchina di prova non deve essere niente.
//! Con il tratto, [`crate::oauth`] si prova per intero senza toccare il
//! portachiavi vero — che chiederebbe una password all'utente nel bel mezzo di
//! `cargo test`.
//!
//! # Cosa non si fa mai
//!
//! Ripiegare sulla tabella `settings` quando il portachiavi non risponde.
//! Sarebbe la scelta comoda: il backup continuerebbe a funzionare e nessuno se
//! ne accorgerebbe. È precisamente il problema — un declassamento silenzioso di
//! una proprietà di sicurezza è peggio di una funzione che dice di non poter
//! partire. Se il portachiavi non c'è, il collegamento non parte e lo dice.

use aether_domain::errors::{AppError, ErrorCode};

/// Il servizio sotto cui stanno le voci di Aether.
///
/// Lo stesso identificativo del bundle dell'applicazione: nel Credential
/// Manager di Windows le voci compaiono con questo nome, e un utente che va a
/// guardare cosa gli è finito nel portachiavi deve poter riconoscere chi ce
/// l'ha messo.
pub const SERVIZIO: &str = "dev.aether.desktop";

/// La voce del token di aggiornamento di Google.
pub const GOOGLE_REFRESH_TOKEN: &str = "google.refresh_token";

/// La voce del segreto del client, quando è stato scritto dall'utente.
pub const GOOGLE_CLIENT_SECRET: &str = "google.client_secret";

/// Un posto dove tenere i segreti.
///
/// Tutte e tre le operazioni sono idempotenti: leggere una voce che non c'è dà
/// `Ok(None)`, cancellarne una che non c'è dà `Ok(())`. È quel che serve a chi
/// scollega un account senza sapere se era collegato.
pub trait Portachiavi: Send + Sync {
    /// Legge un segreto. `Ok(None)` se non c'è.
    ///
    /// # Errori
    ///
    /// `settings.secretUnavailable` se il portachiavi non risponde. Una voce
    /// assente **non** è un errore: è la condizione di chi non ha mai collegato
    /// niente.
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError>;

    /// Scrive un segreto, sostituendo quello che c'era.
    ///
    /// # Errori
    ///
    /// `settings.secretUnavailable` se il portachiavi non risponde.
    fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError>;

    /// Cancella un segreto. Cancellarne uno che non c'è non è un errore.
    ///
    /// # Errori
    ///
    /// `settings.secretUnavailable` se il portachiavi non risponde.
    fn cancella(&self, chiave: &str) -> Result<(), AppError>;
}

/// Il guasto del portachiavi, nominando la voce.
fn non_raggiungibile(chiave: &str, causa: &dyn std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::SettingsSecretUnavailable {
        key: chiave.to_owned(),
    })
    .with_cause(causa.to_string())
}

/// Il portachiavi del sistema operativo.
///
/// Su Windows è il Credential Manager, raggiunto da `keyring` attraverso
/// `windows-sys`: nessun demone da avviare, nessuna catena di compilazione C.
#[cfg(feature = "portachiavi")]
#[derive(Debug, Clone, Copy, Default)]
pub struct DiSistema;

#[cfg(feature = "portachiavi")]
impl Portachiavi for DiSistema {
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
        let voce =
            keyring::Entry::new(SERVIZIO, chiave).map_err(|err| non_raggiungibile(chiave, &err))?;
        match voce.get_password() {
            Ok(valore) => Ok(Some(valore)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(non_raggiungibile(chiave, &err)),
        }
    }

    fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
        let voce =
            keyring::Entry::new(SERVIZIO, chiave).map_err(|err| non_raggiungibile(chiave, &err))?;
        voce.set_password(valore)
            .map_err(|err| non_raggiungibile(chiave, &err))
    }

    fn cancella(&self, chiave: &str) -> Result<(), AppError> {
        let voce =
            keyring::Entry::new(SERVIZIO, chiave).map_err(|err| non_raggiungibile(chiave, &err))?;
        match voce.delete_credential() {
            // Cancellare quel che non c'è è già la situazione voluta. Un errore
            // qui farebbe fallire uno «scollega» su un account mai collegato,
            // cioè proprio la strada che qualcuno prende quando qualcosa è già
            // andato storto.
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(non_raggiungibile(chiave, &err)),
        }
    }
}

/// Un portachiavi in memoria, per le prove.
///
/// Non è un ripiego per la produzione — non c'entra niente con la nota in testa
/// al modulo: non tocca il disco, e sparisce con il processo. Esiste perché
/// `oauth` si possa provare senza che `cargo test` chieda una password.
#[derive(Debug, Default)]
pub struct InMemoria {
    voci: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
}

impl InMemoria {
    /// Un portachiavi vuoto.
    #[must_use]
    pub fn nuovo() -> Self {
        Self::default()
    }
}

impl Portachiavi for InMemoria {
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
        let voci = self
            .voci
            .lock()
            .map_err(|err| non_raggiungibile(chiave, &err))?;
        Ok(voci.get(chiave).cloned())
    }

    fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
        let mut voci = self
            .voci
            .lock()
            .map_err(|err| non_raggiungibile(chiave, &err))?;
        voci.insert(chiave.to_owned(), valore.to_owned());
        Ok(())
    }

    fn cancella(&self, chiave: &str) -> Result<(), AppError> {
        let mut voci = self
            .voci
            .lock()
            .map_err(|err| non_raggiungibile(chiave, &err))?;
        voci.remove(chiave);
        Ok(())
    }
}

/// Un portachiavi che non funziona, per provare cosa succede quando non c'è.
///
/// La strada che conta davvero: se il portachiavi è irraggiungibile il backup
/// deve **restare spento** e dirlo, non ripiegare da qualche altra parte.
#[derive(Debug, Clone, Copy, Default)]
pub struct Guasto;

impl Portachiavi for Guasto {
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
        Err(non_raggiungibile(chiave, &"portachiavi non disponibile"))
    }

    fn scrivi(&self, chiave: &str, _valore: &str) -> Result<(), AppError> {
        Err(non_raggiungibile(chiave, &"portachiavi non disponibile"))
    }

    fn cancella(&self, chiave: &str) -> Result<(), AppError> {
        Err(non_raggiungibile(chiave, &"portachiavi non disponibile"))
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn una_voce_mai_scritta_non_e_un_errore() {
        let portachiavi = InMemoria::nuovo();
        assert_eq!(portachiavi.leggi("mai.scritta"), Ok(None));
    }

    #[test]
    fn scrivere_due_volte_sostituisce() {
        let portachiavi = InMemoria::nuovo();
        assert_eq!(portachiavi.scrivi("t", "prima"), Ok(()));
        assert_eq!(portachiavi.scrivi("t", "dopo"), Ok(()));
        assert_eq!(portachiavi.leggi("t"), Ok(Some("dopo".to_owned())));
    }

    #[test]
    fn cancellare_quel_che_non_c_e_va_bene_lo_stesso() {
        // È la strada di chi preme «scollega» su un account mai collegato,
        // oppure di chi lo preme due volte perché la prima non sembrava aver
        // fatto niente.
        let portachiavi = InMemoria::nuovo();
        assert_eq!(portachiavi.cancella("mai.scritta"), Ok(()));
        assert_eq!(portachiavi.scrivi("t", "x"), Ok(()));
        assert_eq!(portachiavi.cancella("t"), Ok(()));
        assert_eq!(portachiavi.cancella("t"), Ok(()));
        assert_eq!(portachiavi.leggi("t"), Ok(None));
    }

    #[test]
    fn un_portachiavi_guasto_lo_dice_col_nome_della_voce() {
        // Il codice porta la chiave perché il messaggio all'utente possa dire
        // *cosa* non si è potuto salvare, invece di «errore del portachiavi».
        let err = Guasto.leggi(GOOGLE_REFRESH_TOKEN).unwrap_err();
        assert_eq!(err.code().kind().code(), "settings.secretUnavailable");
        assert_eq!(
            err.code(),
            &ErrorCode::SettingsSecretUnavailable {
                key: GOOGLE_REFRESH_TOKEN.to_owned()
            }
        );
    }
}
