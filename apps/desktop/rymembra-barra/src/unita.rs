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
//! Il resto del modulo è puro — quali percorsi si possono chiedere, il numero da
//! leggere — e si prova senza Windows. Questo crate non sa niente del Cestino:
//! risponde con il genere, e quel che se ne fa lo decide chi chiama.
//!
//! # Il volume, non la lettera
//!
//! Il genere è di un **volume**, e la lettera all'inizio di un percorso non
//! sempre è quella del suo volume: una scheda SD si può montare in una
//! cartella vuota di `C:` (Gestione disco, «Monta in cartella NTFS»), e allora
//! `C:\Musica\SD\a.flac` sta sulla scheda. Chiedere a `C:\` diceva «disco
//! fisso». Si chiede quindi prima a Windows il punto di montaggio del volume
//! su cui il percorso finisce (`GetVolumePathNameW`: `C:\Musica\SD\`, o `E:\`),
//! e il genere di quello. Le giunzioni e i collegamenti simbolici non li vede
//! nemmeno lui: per quelli chi chiama passa il nome vero, già risolto.

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

/// La radice dell'unità di un percorso con la lettera: `E:\`.
///
/// Anche dalla forma verbatim (`\\?\E:\…`), che è quella che `canonicalize`
/// restituisce. `None` per tutto il resto — un UNC, un percorso relativo — che
/// una lettera non ce l'ha. È il cancello di [`unita_di`]: solo un percorso
/// con la lettera arriva a Windows, perché per trovarne il volume Windows
/// cammina il percorso, e su una share vorrebbe dire svegliare la rete.
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "è il cancello della domanda a Windows, che altrove non si fa"
    )
)]
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

/// Il genere del volume su cui sta `percorso`: vedi «Il volume, non la
/// lettera».
///
/// Su Windows risponde sempre, e [`Unita::Sconosciuta`] vuol dire «chiesto, e
/// non saputo»: un percorso senza lettera — che non arriva nemmeno a Windows —,
/// un volume che Windows non ha saputo trovare. Chi chiama deve poter
/// distinguere quel «non so» da «non si chiede», che è `None`, fuori da
/// Windows.
///
/// Trovare il volume vuol dire guardare le cartelle del percorso, quindi il
/// disco: chi chiama la fa solo per un percorso che sa già locale, e su un filo
/// con la sua scadenza (`rymembra_app::volumi::collocazioni_e_generi`). Il
/// genere poi lo dà il gestore delle unità, senza toccare il disco.
#[must_use]
pub fn unita_di(percorso: &Path) -> Option<Unita> {
    #[cfg(windows)]
    {
        if radice(percorso).is_none() {
            return Some(Unita::Sconosciuta);
        }
        Some(Unita::dal_codice(chiedi_a_windows(percorso)))
    }
    #[cfg(not(windows))]
    {
        let _ = percorso;
        None
    }
}

/// `GetVolumePathNameW` sul percorso, e `GetDriveTypeW` sul punto di
/// montaggio che restituisce.
///
/// Zero, cioè `DRIVE_UNKNOWN`, se il volume non si trova: il buffer non basta,
/// il percorso porta uno zero dentro, Windows rifiuta.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "GetVolumePathNameW e GetDriveTypeW non hanno un'interfaccia sicura: \
              ogni blocco dice perché è corretto"
)]
fn chiedi_a_windows(percorso: &Path) -> u32 {
    use std::os::windows::ffi::OsStrExt as _;

    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
    use windows::core::PCWSTR;

    /// `MAX_PATH`, il minimo che si dà al punto di montaggio.
    const LUNGHEZZA_MINIMA: usize = 260;

    let largo: Vec<u16> = percorso.as_os_str().encode_wide().collect();
    // Uno zero dentro il percorso lo taglierebbe, e Windows cercherebbe il
    // volume di un altro percorso: meglio non sapere.
    if largo.contains(&0) {
        return 0;
    }
    let largo: Vec<u16> = largo.into_iter().chain(std::iter::once(0)).collect();
    // Il punto di montaggio è un pezzo iniziale del percorso, al più con una
    // barra in più in fondo: la lunghezza del percorso più due basta sempre, e
    // `MAX_PATH` copre le forme corte che Windows può dare al posto del pezzo.
    let mut volume = vec![0_u16; largo.len().saturating_add(1).max(LUNGHEZZA_MINIMA)];
    // SAFETY: `largo` è una stringa UTF-16 terminata da zero che vive fino alla
    // fine di questa funzione, cioè oltre la chiamata, e Windows la legge
    // soltanto. `volume` è un buffer di cui il crate passa puntatore e
    // lunghezza veri: Windows ci scrive al più quella lunghezza, zero finale
    // compreso, e non conserva né l'uno né l'altro.
    let trovato = unsafe { GetVolumePathNameW(PCWSTR(largo.as_ptr()), &mut volume) };
    if trovato.is_err() {
        return 0;
    }
    // Andato a buon fine, il punto di montaggio è terminato da zero dentro il
    // buffer; se lo zero mancasse, la stringa non andrebbe passata a nessuno.
    if !volume.contains(&0) {
        return 0;
    }
    // SAFETY: `volume` è una stringa UTF-16 terminata da zero dentro il suo
    // buffer (appena controllato), che vive oltre la chiamata. `GetDriveTypeW`
    // la legge soltanto: non la conserva e non scrive niente.
    unsafe { GetDriveTypeW(PCWSTR(volume.as_ptr())) }
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
        let windows = Path::new(&sistema).join("Windows");
        assert_eq!(unita_di(&windows), Some(Unita::Fissa));
        // E nella forma che `canonicalize` restituisce, che è quella su cui
        // la domanda si fa davvero: se Windows rifiutasse il prefisso verbatim,
        // ogni disco fisso diventerebbe «non si sa», cioè senza Cestino.
        let vero = std::fs::canonicalize(&windows).expect("la cartella di Windows c'è");
        assert!(
            vero.to_string_lossy().starts_with(r"\\?\"),
            "{}",
            vero.display()
        );
        assert_eq!(unita_di(&vero), Some(Unita::Fissa));
    }

    #[cfg(windows)]
    #[test]
    fn una_cartella_e_un_file_che_non_c_e_stanno_sul_loro_volume() {
        // Le prove girano nella cartella della cassa, su un disco fisso: il
        // volume si trova dalla cartella risolta come da un file che non
        // esiste, perché il punto di montaggio è quello della loro cartella.
        let qui = std::fs::canonicalize(".").expect("la cartella di lavoro c'è");
        assert_eq!(unita_di(&qui), Some(Unita::Fissa));
        assert_eq!(
            unita_di(&qui.join("non-esiste").join("a.flac")),
            Some(Unita::Fissa)
        );
    }

    #[cfg(windows)]
    #[test]
    fn senza_lettera_non_si_chiede_e_non_si_sa() {
        // Un UNC non arriva a Windows: per trovarne il volume Windows andrebbe
        // sulla rete. E «non si sa» non è «non si chiede»: chi chiama ne fa un
        // percorso senza Cestino.
        let prima = std::time::Instant::now();
        assert_eq!(
            unita_di(Path::new(
                r"\\rymembra-server-che-non-esiste.invalid\m\a.flac"
            )),
            Some(Unita::Sconosciuta)
        );
        assert_eq!(
            unita_di(Path::new(
                r"\\?\UNC\rymembra-server-che-non-esiste.invalid\m\a.flac"
            )),
            Some(Unita::Sconosciuta)
        );
        assert_eq!(
            unita_di(Path::new(r"Musica\a.flac")),
            Some(Unita::Sconosciuta)
        );
        assert!(
            prima.elapsed() < std::time::Duration::from_secs(1),
            "la forma basta: nessuna rete"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn fuori_da_windows_il_genere_non_si_chiede() {
        assert_eq!(unita_di(Path::new("/media/chiavetta/a.flac")), None);
    }
}
