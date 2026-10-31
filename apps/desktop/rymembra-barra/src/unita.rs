//! Che genere di unità c'è sotto una lettera: una chiavetta, un disco, un CD.
//!
//! # Perché sta qui
//!
//! «Elimina dal disco» promette il Cestino solo dove il Cestino c'è, e Windows
//! non ne ha uno sulle unità rimovibili — chiavette, schede SD —, sui CD e sui
//! RAM disk: lì la shell cancella per sempre, e lo racconta come un viaggio nel
//! Cestino. `rymembra_app::volumi` sa dire «di rete» o «locale» senza `unsafe`,
//! con `canonicalize`; il **genere** di un'unità locale invece lo dice soltanto
//! `GetDriveTypeW`, che è una chiamata `unsafe`. La radice del workspace vieta
//! `unsafe` con `forbid`, e questo crate è l'unico con l'eccezione dichiarata:
//! la chiamata sta qui, in una funzione sola, con il suo perché.
//!
//! Il resto del modulo è puro — la radice da chiedere, il numero da leggere —
//! e si prova senza Windows. Questo crate non sa niente del Cestino: risponde
//! con il genere, e quel che se ne fa lo decide chi chiama.

use std::path::{Component, Path, Prefix};

/// Il genere di un'unità, come lo dice Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unita {
    /// Un'unità rimovibile: una chiavetta USB, una scheda SD.
    Rimovibile,
    /// Un disco fisso, compresi i dischi USB che si dichiarano tali.
    Fissa,
    /// Una lettera mappata su una condivisione di rete.
    DiRete,
    /// Un lettore di CD o DVD.
    Cd,
    /// Un RAM disk.
    RamDisk,
    /// Windows non lo sa, o sotto quella radice non c'è un'unità.
    Sconosciuta,
}

impl Unita {
    /// Il genere dal numero che `GetDriveTypeW` restituisce.
    ///
    /// I numeri sono quelli di `fileapi.h` (`DRIVE_UNKNOWN` = 0 …
    /// `DRIVE_RAMDISK` = 6), scritti qui invece che importati: nel crate
    /// `windows` stanno in `Win32_System_WindowsProgramming`, una parte grande
    /// dell'API da accendere per sette costanti che non cambiano da Windows 95.
    #[must_use]
    pub const fn dal_codice(codice: u32) -> Self {
        match codice {
            2 => Self::Rimovibile,
            3 => Self::Fissa,
            4 => Self::DiRete,
            5 => Self::Cd,
            6 => Self::RamDisk,
            // 0 `DRIVE_UNKNOWN`, 1 `DRIVE_NO_ROOT_DIR`, e qualunque numero che
            // un Windows futuro inventasse.
            _ => Self::Sconosciuta,
        }
    }
}

/// La radice dell'unità di un percorso con la lettera, nella forma che
/// `GetDriveTypeW` vuole: `E:\`.
///
/// Anche dalla forma verbatim (`\\?\E:\…`), che è quella che `canonicalize`
/// restituisce. `None` per tutto il resto — un UNC, un percorso relativo — che
/// una lettera non ce l'ha, e per cui la domanda non ha senso.
#[must_use]
pub fn radice(percorso: &Path) -> Option<String> {
    let Component::Prefix(prefisso) = percorso.components().next()? else {
        return None;
    };
    match prefisso.kind() {
        Prefix::Disk(lettera) | Prefix::VerbatimDisk(lettera) => {
            Some(format!("{}:\\", char::from(lettera)))
        }
        _ => None,
    }
}

/// Il genere dell'unità su cui sta `percorso`, se ha una lettera.
///
/// La domanda va al gestore delle unità e non al disco: una chiavetta o un
/// lettore senza supporto rispondono subito, e nessuna rete si sveglia — ma
/// chi chiama la fa comunque solo per un percorso che sa già locale.
///
/// `None` per un percorso senza lettera, e fuori da Windows.
#[must_use]
pub fn unita_di(percorso: &Path) -> Option<Unita> {
    let radice = radice(percorso)?;
    #[cfg(windows)]
    {
        Some(Unita::dal_codice(chiedi_a_windows(&radice)))
    }
    #[cfg(not(windows))]
    {
        let _ = radice;
        None
    }
}

/// `GetDriveTypeW` sulla radice data.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "GetDriveTypeW non ha un'interfaccia sicura: il blocco dice perché è corretto"
)]
fn chiedi_a_windows(radice: &str) -> u32 {
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::core::PCWSTR;

    let larga: Vec<u16> = radice.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `larga` è una stringa UTF-16 terminata da zero che vive fino
    // alla fine di questa funzione, cioè oltre la chiamata. `GetDriveTypeW` la
    // legge soltanto: non la conserva e non scrive niente.
    unsafe { GetDriveTypeW(PCWSTR(larga.as_ptr())) }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_numeri_di_windows_si_leggono() {
        assert_eq!(Unita::dal_codice(0), Unita::Sconosciuta);
        assert_eq!(Unita::dal_codice(1), Unita::Sconosciuta);
        assert_eq!(Unita::dal_codice(2), Unita::Rimovibile);
        assert_eq!(Unita::dal_codice(3), Unita::Fissa);
        assert_eq!(Unita::dal_codice(4), Unita::DiRete);
        assert_eq!(Unita::dal_codice(5), Unita::Cd);
        assert_eq!(Unita::dal_codice(6), Unita::RamDisk);
        assert_eq!(Unita::dal_codice(99), Unita::Sconosciuta);
    }

    #[cfg(windows)]
    #[test]
    fn la_radice_e_la_lettera_con_la_barra() {
        assert_eq!(
            radice(Path::new(r"E:\Musica\a.flac")).as_deref(),
            Some(r"E:\")
        );
        // La lettera torna maiuscola: `Prefix::Disk` la normalizza così.
        assert_eq!(radice(Path::new(r"e:\a.flac")).as_deref(), Some(r"E:\"));
        assert_eq!(
            radice(Path::new(r"\\?\E:\Musica\a.flac")).as_deref(),
            Some(r"E:\")
        );
        assert_eq!(radice(Path::new(r"\\nas\musica\a.flac")), None);
        assert_eq!(radice(Path::new(r"\\?\UNC\nas\musica\a.flac")), None);
        assert_eq!(radice(Path::new(r"Musica\a.flac")), None);
    }

    #[cfg(windows)]
    #[test]
    fn il_disco_di_sistema_e_un_disco_fisso() {
        let sistema = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        assert_eq!(
            unita_di(&Path::new(&sistema).join("Windows")),
            Some(Unita::Fissa)
        );
        assert_eq!(unita_di(Path::new(r"\\nas\musica\a.flac")), None);
    }
}
