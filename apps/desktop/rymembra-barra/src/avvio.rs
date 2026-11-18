//! L'avvio con Windows: cosa è acceso, e come lo si accende e lo si spegne.
//!
//! # Due copie, due strade
//!
//! - **La copia installata dal sito (NSIS)** parte con Windows se, sotto
//!   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, c'è un valore col nome
//!   del programma e il comando `"<exe>" --avvio-automatico`. Il valore lo
//!   scrive e lo toglie l'interruttore delle Impostazioni, e **lo toglie anche
//!   il disinstallatore**: il modello NSIS di Tauri lo fa da sé, e solo se il
//!   nome è proprio quello del prodotto ([`NOME_VALORE`]) e se non si sta
//!   aggiornando — vedi `installer/precedente.nsh`.
//! - **La copia del Store (MSIX)** non scrive nel registro: dichiara nel
//!   manifesto un'attività di avvio ([`ID_ATTIVITA_STORE`]) e la accende e
//!   spegne con l'API WinRT `StartupTask`, che è sicura. Lo schema del
//!   manifesto (`uap5:StartupTask`) non ha un attributo per gli argomenti — solo
//!   `TaskId`, `Enabled`, `DisplayName` e `UserConfigurable` — quindi l'avvio
//!   del Store si riconosce con [`avviata_dal_login_store`], non da un
//!   argomento.
//!
//! # L'interruttore mostra ciò che dice il sistema
//!
//! Il registro è dell'utente, che può spegnere l'avvio anche da fuori: da
//! Gestione attività › Avvio, o da Impostazioni › App › Avvio. Windows non
//! toglie il valore `Run`: scrive un byte in `…\Explorer\StartupApproved\Run`.
//! Un interruttore che guardasse solo `Run` direbbe «acceso» per un avvio che
//! non parte. Qui `Run` **e** `StartupApproved` si leggono insieme, e una scelta
//! dell'utente fatta altrove **non si scavalca**: l'interruttore dice
//! [`Impedimento::Sistema`] e la correzione è dove l'ha fatta lui.
//!
//! Il formato di `StartupApproved` **non è documentato da Microsoft**. Le
//! fonti che lo descrivono sono di analisi forense (windowsir.blogspot.com,
//! «StartupApproved\Run», luglio 2022): dodici byte, il primo dice `02` o `06`
//! per «acceso» e `03` per «spento», e gli otto dopo sono l'ora dello spegnimento
//! (`FILETIME`). La lettura qui è la più prudente che le contiene: valore
//! assente = acceso, primo byte pari = acceso, primo byte dispari = spento.
//! Il suo ripiego, se Windows cambiasse il formato, è dire «acceso» di un
//! avvio che forse non parte; ma non si arriva mai a scrivere in quella chiave
//! per accenderlo, solo a toglierne il valore dopo che è stato **l'utente**, da
//! qui, a spegnere (vedi [`imposta`]).
//!
//! # Niente registro vero nelle prove
//!
//! La regola parla con [`Registro`], non col registro: le prove di questo
//! modulo usano uno in memoria. Lo stesso vale per il Store, la cui API è
//! provata solo dal vivo.

use std::path::Path;

use crate::registro::Registro;

/// Il nome del valore in `Run`: **identico** a `productName` di
/// `tauri.conf.json`, ed è l'identità che conta. Il modello NSIS toglie alla
/// disinstallazione `Run\${PRODUCTNAME}` e nient'altro: un nome diverso
/// lascerebbe l'avvio di un programma che non c'è più. Una prova in
/// `src-tauri` lo tiene uguale al file di configurazione.
pub const NOME_VALORE: &str = "Rymembra";

/// L'argomento con cui parte il programma lanciato da Windows al login. Con
/// lui la finestra si mostra ridotta a icona, e un secondo avvio con lui non
/// alza la finestra di quello che c'è già.
pub const ARGOMENTO_AVVIO_AUTOMATICO: &str = "--avvio-automatico";

/// L'identificativo dell'attività di avvio nel manifesto MSIX (`TaskId`).
/// Una prova in `strumenti/msix.test.js` lo tiene uguale a questo.
pub const ID_ATTIVITA_STORE: &str = "RymembraAvvio";

/// Dove Windows tiene le voci d'avvio dell'utente.
const PERCORSO_RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Dove Windows scrive che l'utente ha spento una voce d'avvio, senza toglierla.
const PERCORSO_APPROVATO: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Perché l'interruttore non si può toccare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impedimento {
    /// L'utente l'ha spento da Windows (Gestione attività, Impostazioni): la
    /// scelta è sua e si rimette lì.
    Sistema,
    /// Un criterio del computer lo accende o lo spegne: non lo cambia
    /// nessuno da qui.
    Criterio,
}

/// Com'è l'avvio con Windows, secondo il sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stato {
    /// Parte con Windows.
    pub acceso: bool,
    /// Se l'interruttore non si può toccare, e perché.
    pub impedito: Option<Impedimento>,
}

impl Stato {
    /// Acceso, e si può spegnere.
    pub const ACCESO: Self = Self {
        acceso: true,
        impedito: None,
    };

    /// Spento, e si può accendere.
    pub const SPENTO: Self = Self {
        acceso: false,
        impedito: None,
    };

    /// Spento da una scelta dell'utente fatta in Windows.
    pub const SPENTO_DAL_SISTEMA: Self = Self {
        acceso: false,
        impedito: Some(Impedimento::Sistema),
    };
}

/// Il comando che Windows lancia al login: l'eseguibile fra virgolette,
/// perché un percorso con spazi (`C:\Program Files\…`) altrimenti si spezzerebbe,
/// e l'argomento.
///
/// Il prefisso `\\?\` dei percorsi lunghi si toglie: non tutti i lettori della
/// chiave `Run` lo capiscono.
///
/// # Errors
///
/// Una frase, se il percorso non è un testo (UTF-8): il registro ne vorrebbe
/// uno, e un percorso storpiato lancerebbe un programma che non c'è.
pub fn comando_di_avvio(esegui: &Path) -> Result<String, String> {
    let testo = esegui
        .to_str()
        .ok_or_else(|| "il percorso dell'eseguibile non è un testo".to_owned())?;
    let testo = testo.strip_prefix(r"\\?\").unwrap_or(testo);
    if testo.is_empty() || testo.contains('"') {
        return Err("il percorso dell'eseguibile non è valido".to_owned());
    }
    Ok(format!("\"{testo}\" {ARGOMENTO_AVVIO_AUTOMATICO}"))
}

/// `StartupApproved` dice che l'utente l'ha spento?
///
/// Il primo byte dispari: `03` è lo spento che si trova scritto (vedi il
/// preambolo del modulo, sul formato non documentato). Un valore vuoto non dice
/// niente e vale «acceso».
#[must_use]
pub fn e_spento_dal_sistema(byte: &[u8]) -> bool {
    byte.first().is_some_and(|primo| primo % 2 == 1)
}

/// Lo stato dell'avvio nella copia NSIS: `Run` e `StartupApproved`, letti
/// insieme.
///
/// Un valore `Run` che non è il nostro comando — un'installazione in un'altra
/// cartella, una cartella spostata — non è **questo** programma: vale «spento»,
/// e [`imposta`] lo riscrive quando lo si accende.
///
/// # Errors
///
/// Quello del registro, o del percorso dell'eseguibile.
pub fn leggi<R: Registro>(registro: &R, esegui: &Path) -> Result<Stato, String> {
    let atteso = comando_di_avvio(esegui)?;
    let nostro = registro
        .leggi_testo(PERCORSO_RUN, NOME_VALORE)?
        .is_some_and(|scritto| scritto.trim().to_lowercase() == atteso.to_lowercase());
    if !nostro {
        return Ok(Stato::SPENTO);
    }
    let approvato = registro.leggi_byte(PERCORSO_APPROVATO, NOME_VALORE)?;
    if approvato.is_some_and(|byte| e_spento_dal_sistema(&byte)) {
        return Ok(Stato::SPENTO_DAL_SISTEMA);
    }
    Ok(Stato::ACCESO)
}

/// Accende o spegne l'avvio nella copia NSIS, e dice **com'è rimasto**.
///
/// Il ritorno è lo stato **riletto**, non quello chiesto: è la disciplina
/// dell'interruttore pessimista, e vale qui più che altrove, perché la scrittura
/// può riuscire e non servire a niente (un criterio, un antivirus che toglie
/// la voce).
///
/// - Se l'utente ha spento l'avvio da Windows non si tocca niente: lo stato
///   torna com'era, con [`Impedimento::Sistema`].
/// - Accendere scrive `Run`, e toglie un `StartupApproved` rimasto dall'ultima
///   volta: Windows non lo cancella quando una voce `Run` sparisce, e un
///   «spento» di anni fa farebbe sembrare impedita una voce nuova. Succede solo
///   se la voce, prima, **non era** nostra e accesa: con la voce spenta
///   dall'utente non si arriva fin qui.
/// - Spegnere toglie `Run` **e** `StartupApproved`, per lo stesso motivo.
///
/// # Errors
///
/// Quello del registro, o del percorso dell'eseguibile.
pub fn imposta<R: Registro>(registro: &R, esegui: &Path, acceso: bool) -> Result<Stato, String> {
    let prima = leggi(registro, esegui)?;
    if prima.impedito.is_some() {
        return Ok(prima);
    }
    if acceso {
        if !prima.acceso {
            registro.scrivi_testo(PERCORSO_RUN, NOME_VALORE, &comando_di_avvio(esegui)?)?;
            registro.cancella(PERCORSO_APPROVATO, NOME_VALORE)?;
        }
    } else {
        registro.cancella(PERCORSO_RUN, NOME_VALORE)?;
        registro.cancella(PERCORSO_APPROVATO, NOME_VALORE)?;
    }
    leggi(registro, esegui)
}

/// Come Windows chiama lo stato di un'attività di avvio del Store.
///
/// Uno specchio di `StartupTaskState`, perché la traduzione in [`Stato`] si
/// provi senza WinRT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatoAttivita {
    /// Spenta, e si può accendere.
    Disabilitata,
    /// Spenta dall'utente (Gestione attività, Impostazioni): da lì si riaccende.
    DisabilitataDallUtente,
    /// Accesa.
    Abilitata,
    /// Spenta da un criterio.
    DisabilitataDaCriterio,
    /// Accesa da un criterio.
    AbilitataDaCriterio,
}

/// Lo stato di un'attività di avvio del Store, nei termini dell'interruttore.
#[must_use]
pub const fn stato_dell_attivita(attivita: StatoAttivita) -> Stato {
    match attivita {
        StatoAttivita::Disabilitata => Stato::SPENTO,
        StatoAttivita::DisabilitataDallUtente => Stato::SPENTO_DAL_SISTEMA,
        StatoAttivita::Abilitata => Stato::ACCESO,
        StatoAttivita::DisabilitataDaCriterio => Stato {
            acceso: false,
            impedito: Some(Impedimento::Criterio),
        },
        StatoAttivita::AbilitataDaCriterio => Stato {
            acceso: true,
            impedito: Some(Impedimento::Criterio),
        },
    }
}

/// Lo stato dell'attività di avvio del pacchetto, secondo il sistema.
///
/// # Errors
///
/// Una frase che dice cosa ha risposto WinRT, o che non si è su Windows.
pub fn leggi_dal_store() -> Result<Stato, String> {
    #[cfg(windows)]
    {
        store::leggi()
    }
    #[cfg(not(windows))]
    {
        Err("l'avvio del Store esiste solo su Windows".to_owned())
    }
}

/// Accende o spegne l'attività di avvio del pacchetto, e dice com'è rimasta.
///
/// Come [`imposta`]: lo stato è **riletto**. Un'attività spenta dall'utente o
/// da un criterio non si tocca, e `RequestEnableAsync` non si chiama: Windows
/// la rifiuterebbe comunque, e chiederla non cambierebbe la scelta di chi l'ha
/// spenta.
///
/// Bloccante, per qualche millisecondo: da un filo dei comandi `(async)`, mai
/// da quello della finestra.
///
/// # Errors
///
/// Come [`leggi_dal_store`].
pub fn imposta_dal_store(acceso: bool) -> Result<Stato, String> {
    #[cfg(windows)]
    {
        store::imposta(acceso)
    }
    #[cfg(not(windows))]
    {
        let _ = acceso;
        Err("l'avvio del Store esiste solo su Windows".to_owned())
    }
}

/// Windows ha lanciato questo processo al login, dall'attività di avvio del
/// pacchetto?
///
/// È l'unico modo che il Store dà: l'attività lancia l'eseguibile **senza
/// argomenti** (lo schema non ne ha), e la sola traccia è l'attivazione
/// (`AppInstance::GetActivatedEventArgs`, Windows 10 1809 o successivo: vedi la
/// documentazione Microsoft «Get activation info for packaged apps»), che
/// risponde **solo la prima volta**. Va chiamata una volta, presto, e solo se
/// la copia è quella del Store: fuori da un pacchetto risponde con un errore, e
/// qui vale `false`.
///
/// Gira su un filo suo, che finisce subito: la chiamata WinRT fa entrare il
/// filo nel suo apartment COM, e il filo principale non deve essere inizializzato
/// prima che `tao` scelga il suo.
#[must_use]
pub fn avviata_dal_login_store() -> bool {
    #[cfg(windows)]
    {
        std::thread::Builder::new()
            .name("rymembra-attivazione".to_owned())
            .spawn(store::attivata_dal_login)
            .ok()
            .and_then(|filo| filo.join().ok())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
mod store {
    use windows::ApplicationModel::Activation::ActivationKind;
    use windows::ApplicationModel::{AppInstance, StartupTask, StartupTaskState};
    use windows::core::HSTRING;

    use super::{ID_ATTIVITA_STORE, Stato, StatoAttivita, stato_dell_attivita};

    /// L'attività di avvio dichiarata nel manifesto.
    fn attivita() -> Result<StartupTask, String> {
        StartupTask::GetAsync(&HSTRING::from(ID_ATTIVITA_STORE))
            .and_then(|operazione| operazione.join())
            .map_err(|err| format!("l'attività di avvio del pacchetto non si apre: {err}"))
    }

    /// Lo stato di WinRT, nel nostro.
    fn tradotto(stato: StartupTaskState) -> Result<StatoAttivita, String> {
        match stato {
            StartupTaskState::Disabled => Ok(StatoAttivita::Disabilitata),
            StartupTaskState::DisabledByUser => Ok(StatoAttivita::DisabilitataDallUtente),
            StartupTaskState::Enabled => Ok(StatoAttivita::Abilitata),
            StartupTaskState::DisabledByPolicy => Ok(StatoAttivita::DisabilitataDaCriterio),
            StartupTaskState::EnabledByPolicy => Ok(StatoAttivita::AbilitataDaCriterio),
            altro => Err(format!(
                "stato dell'attività di avvio sconosciuto: {}",
                altro.0
            )),
        }
    }

    pub(super) fn leggi() -> Result<Stato, String> {
        let stato = attivita()?
            .State()
            .map_err(|err| format!("lo stato dell'avvio del pacchetto non si legge: {err}"))?;
        tradotto(stato).map(stato_dell_attivita)
    }

    pub(super) fn imposta(acceso: bool) -> Result<Stato, String> {
        let attivita = attivita()?;
        let prima = attivita
            .State()
            .map_err(|err| format!("lo stato dell'avvio del pacchetto non si legge: {err}"))
            .and_then(tradotto)?;
        // Una scelta dell'utente o di un criterio non si scavalca.
        if stato_dell_attivita(prima).impedito.is_none() {
            if acceso && prima == StatoAttivita::Disabilitata {
                attivita
                    .RequestEnableAsync()
                    .and_then(|operazione| operazione.join())
                    .map_err(|err| format!("Windows non accende l'avvio del pacchetto: {err}"))?;
            } else if !acceso && prima == StatoAttivita::Abilitata {
                attivita
                    .Disable()
                    .map_err(|err| format!("Windows non spegne l'avvio del pacchetto: {err}"))?;
            }
        }
        // Lo stato riletto, non quello che ha risposto la richiesta.
        leggi()
    }

    pub(super) fn attivata_dal_login() -> bool {
        AppInstance::GetActivatedEventArgs()
            .and_then(|argomenti| argomenti.Kind())
            .is_ok_and(|tipo| tipo == ActivationKind::StartupTask)
    }
}

#[cfg(test)]
mod prove {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Mutex, PoisonError};

    use super::*;

    /// Un valore del registro finto.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Voce {
        Testo(String),
        Byte(Vec<u8>),
    }

    /// Un registro in memoria: l'unico che le prove toccano.
    #[derive(Default)]
    struct Finto {
        valori: Mutex<HashMap<(String, String), Voce>>,
        /// Le scritture «riescono» e non lasciano niente: un criterio, un
        /// antivirus che toglie la voce.
        scrive_a_vuoto: bool,
        /// Ogni scrittura fallisce.
        non_scrive: bool,
    }

    impl Finto {
        fn con(&self, percorso: &str, nome: &str, voce: Voce) {
            self.valori
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert((percorso.to_owned(), nome.to_owned()), voce);
        }

        fn voce(&self, percorso: &str, nome: &str) -> Option<Voce> {
            self.valori
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&(percorso.to_owned(), nome.to_owned()))
                .cloned()
        }

        fn tutto(&self) -> HashMap<(String, String), Voce> {
            self.valori
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    impl Registro for Finto {
        fn leggi_testo(&self, percorso: &str, nome: &str) -> Result<Option<String>, String> {
            Ok(match self.voce(percorso, nome) {
                Some(Voce::Testo(testo)) => Some(testo),
                _ => None,
            })
        }

        fn leggi_byte(&self, percorso: &str, nome: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(match self.voce(percorso, nome) {
                Some(Voce::Byte(byte)) => Some(byte),
                _ => None,
            })
        }

        fn scrivi_testo(&self, percorso: &str, nome: &str, valore: &str) -> Result<(), String> {
            if self.non_scrive {
                return Err("il registro finto non scrive".to_owned());
            }
            if !self.scrive_a_vuoto {
                self.con(percorso, nome, Voce::Testo(valore.to_owned()));
            }
            Ok(())
        }

        fn cancella(&self, percorso: &str, nome: &str) -> Result<(), String> {
            self.valori
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&(percorso.to_owned(), nome.to_owned()));
            Ok(())
        }
    }

    fn esegui() -> PathBuf {
        PathBuf::from(r"C:\Users\ada\AppData\Local\Rymembra\rymembra.exe")
    }

    fn comando() -> String {
        comando_di_avvio(&esegui()).expect("un percorso valido")
    }

    #[test]
    fn il_comando_ha_l_eseguibile_fra_virgolette_e_l_argomento() {
        assert_eq!(
            comando(),
            r#""C:\Users\ada\AppData\Local\Rymembra\rymembra.exe" --avvio-automatico"#
        );
        // Con gli spazi nel percorso le virgolette sono quel che tiene unito il comando.
        let con_spazi = comando_di_avvio(&PathBuf::from(r"C:\Program Files\Rymembra\rymembra.exe"))
            .expect("un percorso valido");
        assert!(
            con_spazi.starts_with(r#""C:\Program Files\"#),
            "{con_spazi}"
        );
    }

    #[test]
    fn il_prefisso_dei_percorsi_lunghi_non_entra_nel_comando() {
        let lungo = comando_di_avvio(&PathBuf::from(r"\\?\C:\Rymembra\rymembra.exe"))
            .expect("un percorso valido");
        assert_eq!(lungo, r#""C:\Rymembra\rymembra.exe" --avvio-automatico"#);
    }

    #[test]
    fn un_percorso_senza_senso_non_diventa_un_comando() {
        assert!(comando_di_avvio(&PathBuf::new()).is_err());
        // Una virgoletta nel percorso romperebbe il quoting del comando.
        assert!(comando_di_avvio(&PathBuf::from("C:\\a\"b\\r.exe")).is_err());
    }

    #[test]
    fn l_argomento_e_una_parola_sola() {
        // Il comando dice `"<exe>" <argomento>`: uno spazio dentro l'argomento
        // lo farebbe leggere come due.
        assert!(!ARGOMENTO_AVVIO_AUTOMATICO.contains(char::is_whitespace));
        assert!(ARGOMENTO_AVVIO_AUTOMATICO.starts_with("--"));
    }

    #[test]
    fn senza_voce_l_avvio_e_spento() {
        let registro = Finto::default();
        assert_eq!(leggi(&registro, &esegui()), Ok(Stato::SPENTO));
    }

    #[test]
    fn accendere_scrive_il_comando_e_si_rilegge_acceso() {
        let registro = Finto::default();
        let stato = imposta(&registro, &esegui(), true).expect("scrittura");
        assert_eq!(stato, Stato::ACCESO);
        assert_eq!(
            registro.voce(PERCORSO_RUN, NOME_VALORE),
            Some(Voce::Testo(comando()))
        );
    }

    #[test]
    fn spegnere_toglie_la_voce_e_lo_spento_rimasto() {
        let registro = Finto::default();
        imposta(&registro, &esegui(), true).expect("accende");
        registro.con(
            PERCORSO_APPROVATO,
            NOME_VALORE,
            Voce::Byte(vec![2, 0, 0, 0]),
        );
        let stato = imposta(&registro, &esegui(), false).expect("spegne");
        assert_eq!(stato, Stato::SPENTO);
        assert!(registro.tutto().is_empty(), "{:?}", registro.tutto());
    }

    #[test]
    fn spegnere_senza_voce_non_e_un_errore() {
        let registro = Finto::default();
        assert_eq!(imposta(&registro, &esegui(), false), Ok(Stato::SPENTO));
    }

    #[test]
    fn l_utente_che_spegne_da_windows_si_vede_e_non_si_scavalca() {
        // Gestione attività › Avvio › Disabilita: `Run` resta, `StartupApproved`
        // porta `03` e l'ora.
        let registro = Finto::default();
        registro.con(PERCORSO_RUN, NOME_VALORE, Voce::Testo(comando()));
        registro.con(
            PERCORSO_APPROVATO,
            NOME_VALORE,
            Voce::Byte(vec![3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]),
        );
        assert_eq!(leggi(&registro, &esegui()), Ok(Stato::SPENTO_DAL_SISTEMA));
        let prima = registro.tutto();
        // Né accendere né spegnere cambiano niente: la scelta è sua.
        assert_eq!(
            imposta(&registro, &esegui(), true),
            Ok(Stato::SPENTO_DAL_SISTEMA)
        );
        assert_eq!(
            imposta(&registro, &esegui(), false),
            Ok(Stato::SPENTO_DAL_SISTEMA)
        );
        assert_eq!(registro.tutto(), prima);
    }

    #[test]
    fn i_valori_di_acceso_di_startup_approved_valgono_acceso() {
        for byte in [
            vec![2, 0, 0, 0],
            vec![6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            vec![],
        ] {
            let registro = Finto::default();
            registro.con(PERCORSO_RUN, NOME_VALORE, Voce::Testo(comando()));
            registro.con(PERCORSO_APPROVATO, NOME_VALORE, Voce::Byte(byte.clone()));
            assert_eq!(leggi(&registro, &esegui()), Ok(Stato::ACCESO), "{byte:?}");
        }
        // E senza `StartupApproved` il valore `Run` è acceso.
        let registro = Finto::default();
        registro.con(PERCORSO_RUN, NOME_VALORE, Voce::Testo(comando()));
        assert_eq!(leggi(&registro, &esegui()), Ok(Stato::ACCESO));
    }

    #[test]
    fn uno_spento_vecchio_non_blocca_una_voce_nuova() {
        // Windows non cancella `StartupApproved` quando una voce `Run` sparisce:
        // chi aveva spento l'avvio anni fa e poi l'ha tolto non deve trovare
        // l'interruttore impedito per un «spento» che nessuno ricorda.
        let registro = Finto::default();
        registro.con(
            PERCORSO_APPROVATO,
            NOME_VALORE,
            Voce::Byte(vec![3, 0, 0, 0]),
        );
        let stato = imposta(&registro, &esegui(), true).expect("accende");
        assert_eq!(stato, Stato::ACCESO);
        assert_eq!(registro.voce(PERCORSO_APPROVATO, NOME_VALORE), None);
    }

    #[test]
    fn una_voce_che_punta_altrove_non_e_questo_programma() {
        let registro = Finto::default();
        registro.con(
            PERCORSO_RUN,
            NOME_VALORE,
            Voce::Testo(r#""D:\Vecchia\rymembra.exe" --avvio-automatico"#.to_owned()),
        );
        assert_eq!(leggi(&registro, &esegui()), Ok(Stato::SPENTO));
        // Accendere la rimette al posto giusto.
        assert_eq!(imposta(&registro, &esegui(), true), Ok(Stato::ACCESO));
        assert_eq!(
            registro.voce(PERCORSO_RUN, NOME_VALORE),
            Some(Voce::Testo(comando()))
        );
        // E spegnere toglie anche una voce che punta altrove: ha il nostro nome.
        registro.con(
            PERCORSO_RUN,
            NOME_VALORE,
            Voce::Testo("altro.exe".to_owned()),
        );
        assert_eq!(imposta(&registro, &esegui(), false), Ok(Stato::SPENTO));
        assert_eq!(registro.voce(PERCORSO_RUN, NOME_VALORE), None);
    }

    #[test]
    fn il_maiuscolo_dei_percorsi_non_conta() {
        let registro = Finto::default();
        registro.con(
            PERCORSO_RUN,
            NOME_VALORE,
            Voce::Testo(comando().to_uppercase()),
        );
        assert_eq!(leggi(&registro, &esegui()), Ok(Stato::ACCESO));
    }

    #[test]
    fn vale_quel_che_si_rilegge() {
        // La scrittura «riesce» e non lascia niente: l'interruttore deve dire
        // spento, non quel che gli è stato chiesto.
        let registro = Finto {
            scrive_a_vuoto: true,
            ..Finto::default()
        };
        assert_eq!(imposta(&registro, &esegui(), true), Ok(Stato::SPENTO));
    }

    #[test]
    fn un_guasto_del_registro_si_dice() {
        let registro = Finto {
            non_scrive: true,
            ..Finto::default()
        };
        assert!(imposta(&registro, &esegui(), true).is_err());
        assert!(registro.tutto().is_empty());
    }

    #[test]
    fn un_valore_che_non_e_un_testo_non_e_nostro() {
        let registro = Finto::default();
        registro.con(PERCORSO_RUN, NOME_VALORE, Voce::Byte(vec![1, 2, 3]));
        assert_eq!(leggi(&registro, &esegui()), Ok(Stato::SPENTO));
    }

    #[test]
    fn gli_stati_del_store_diventano_lo_stato_dell_interruttore() {
        assert_eq!(
            stato_dell_attivita(StatoAttivita::Disabilitata),
            Stato::SPENTO
        );
        assert_eq!(stato_dell_attivita(StatoAttivita::Abilitata), Stato::ACCESO);
        // Spenta dall'utente: si rimette da Windows.
        assert_eq!(
            stato_dell_attivita(StatoAttivita::DisabilitataDallUtente),
            Stato::SPENTO_DAL_SISTEMA
        );
        // I criteri: non si cambiano da qui, in nessuno dei due versi.
        assert_eq!(
            stato_dell_attivita(StatoAttivita::DisabilitataDaCriterio),
            Stato {
                acceso: false,
                impedito: Some(Impedimento::Criterio)
            }
        );
        assert_eq!(
            stato_dell_attivita(StatoAttivita::AbilitataDaCriterio),
            Stato {
                acceso: true,
                impedito: Some(Impedimento::Criterio)
            }
        );
    }

    #[test]
    fn fuori_da_un_pacchetto_il_login_del_store_non_si_riconosce() {
        // Il processo di prova non è in un pacchetto: la risposta è «no», senza
        // errori né panico — e senza toccare il registro.
        assert!(!avviata_dal_login_store());
        assert!(leggi_dal_store().is_err());
    }
}
