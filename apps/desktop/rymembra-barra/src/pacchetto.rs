//! Dove sta la cartella privata del pacchetto MSIX, se Rymembra gira in uno.
//!
//! # Perché sta qui
//!
//! Dentro un pacchetto MSIX Windows manda nella cartella privata del pacchetto,
//! `%LOCALAPPDATA%\Packages\<PackageFamilyName>\LocalCache`, i file che
//! l'applicazione **crea** in `%APPDATA%` (`LocalCache\Roaming`) e in
//! `%LOCALAPPDATA%` (`LocalCache\Local`). Dal di dentro il percorso resta
//! quello di sempre; Esplora risorse e ogni altro programma, che girano fuori
//! dal pacchetto, quella cartella non la vedono: «Apri la cartella dei dati»
//! finiva in «Percorso non disponibile» (collaudo in Windows Sandbox, R20-4).
//!
//! Il percorso vero lo sa solo il sistema. Il `PackageFamilyName` non è una
//! costante — quello che il Partner Center assegna a questa app sarà diverso da
//! quello delle prove — e ricavarlo dal nome dell'eseguibile non funziona con
//! un pacchetto registrato da una cartella (`Add-AppxPackage -Register`), dove
//! l'eseguibile non sta in `WindowsApps`. Lo dice l'API WinRT,
//! `ApplicationData::LocalCacheFolder`: sicura, senza `unsafe`, e che fuori da
//! un pacchetto risponde con un errore — cioè «non sono in un pacchetto», che è
//! tutto quel che serve sapere.
//!
//! Il crate si occupa solo di chiederlo. Quale percorso tradurre, e quando, lo
//! decide chi chiama (`pacchetto.rs` dell'applicazione).

use std::path::PathBuf;

/// La cartella `LocalCache` del pacchetto in cui gira il processo, o `None`.
///
/// `None` fuori da un pacchetto (la copia installata dal sito, le prove), su
/// un Windows che non offre `LocalCacheFolder` e fuori da Windows. Si chiama
/// una volta e si ricorda: il pacchetto di un processo non cambia.
///
/// Costa una chiamata WinRT: chi la usa guarda prima, a buon mercato, se la
/// copia è quella del Store, e la copia installata dal sito non la fa mai.
#[must_use]
pub fn cartella_cache_del_pacchetto() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        da_windows()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// La domanda vera, a WinRT.
///
/// Il percorso si costruisce dai caratteri larghi così come sono
/// (`HSTRING::to_os_string`), e non da una `String`: un nome utente con una
/// lettera che non è UTF-16 valido non si perde per strada.
#[cfg(windows)]
fn da_windows() -> Option<PathBuf> {
    use windows::Storage::ApplicationData;

    let cartella = ApplicationData::Current().ok()?.LocalCacheFolder().ok()?;
    let percorso = cartella.Path().ok()?.to_os_string();
    (!percorso.is_empty()).then(|| PathBuf::from(percorso))
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Il processo di prova non è in un pacchetto: la risposta è «no», senza
    /// errori né panico. È la strada di tutta la copia installata dal sito.
    #[test]
    fn fuori_da_un_pacchetto_non_c_e_nessuna_cartella() {
        assert_eq!(cartella_cache_del_pacchetto(), None);
    }
}
