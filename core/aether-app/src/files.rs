//! L'accesso ai file, dietro un tratto.
//!
//! Sul desktop un brano è un percorso e `std::fs` lo apre. Su Android un brano è
//! un URI di MediaStore o una concessione dello Storage Access Framework, e per
//! aprirlo serve passare da Java. Le due cose non si assomigliano abbastanza da
//! poter essere la stessa funzione, e non si assomigliano abbastanza poco da
//! giustificare due scansioni diverse.
//!
//! Quindi: la scansione parla con questo tratto, e chi la esegue gli passa
//! l'implementazione della sua piattaforma. La **decisione** su cosa inserire e
//! cosa togliere resta una funzione pura in `aether-domain`, uguale ovunque; qui
//! cambia solo il modo di ottenere l'elenco e i byte.
//!
//! È anche ciò che rende la scansione provabile senza toccare il disco: un
//! doppio in memoria implementa il tratto e i test girano su alberi di file
//! finti, compresi quelli che un filesystem vero non lascerebbe costruire.

use std::io::{Read, Seek};
use std::path::Path;

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::scan_plan::DiscoveredFile;

/// Ciò che serve a un lettore di metadati: leggere e riposizionarsi.
///
/// I formati audio hanno i tag in testa (ID3v2) o in coda (ID3v1, APE), quindi
/// non basta uno stream in avanti.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Da dove arrivano i file musicali.
pub trait MusicFiles: Send + Sync {
    /// Elenca ricorsivamente i file sotto una radice.
    ///
    /// **Non filtra**: restituisce quel che trova, comprese le estensioni non
    /// audio e i file troppo piccoli. Il filtro è una decisione, e le decisioni
    /// stanno nel dominio, dove si provano. Una camminata che filtrasse da sé
    /// renderebbe invisibile al piano il motivo per cui un file è stato
    /// scartato — che è l'informazione che serve quando un brano «non compare».
    ///
    /// Una radice illeggibile non è un errore fatale: si riporta vuota, perché
    /// un disco esterno staccato non deve impedire la scansione degli altri.
    fn walk(&self, root: &str) -> Result<Vec<DiscoveredFile>, AppError>;

    /// Apre un file in lettura.
    ///
    /// `Sync` oltre a `Send` perché lo stesso flusso serve al motore audio, e
    /// symphonia lo pretende: il suo `MediaSource` è `Send + Sync`. Non è un
    /// requisito gratuito — significa che un'implementazione non può nascondere
    /// uno stato mutabile senza lucchetto dietro un `&self` — ma è quello che
    /// permette a lettura dei tag e riproduzione di passare dallo stesso tratto
    /// invece che da due.
    fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError>;
}

/// Traduce un errore di I/O nel catalogo, distinguendo i casi che cambiano cosa
/// deve fare il chiamante.
///
/// La distinzione non è cosmetica: `NotFound` durante una scansione significa
/// «il file è appena sparito, salta e prosegui», mentre `PermissionDenied`
/// significa «tutta questa cartella è inaccessibile, dirlo all'utente». Il
/// vecchio albero li appiattiva entrambi in un avviso nel log.
pub fn io_error(path: &str, err: &std::io::Error) -> AppError {
    use std::io::ErrorKind as K;
    let code = match err.kind() {
        K::NotFound => ErrorCode::FsNotFound {
            path: path.to_owned(),
        },
        K::PermissionDenied => ErrorCode::FsPermissionDenied {
            path: path.to_owned(),
        },
        K::StorageFull | K::QuotaExceeded => ErrorCode::FsDiskFull {
            path: Some(path.to_owned()),
        },
        K::InvalidFilename => ErrorCode::FsPathInvalid {
            path: path.to_owned(),
        },
        _ => ErrorCode::FsReadFailed {
            path: path.to_owned(),
            detail: Some(err.kind().to_string()),
        },
    };
    AppError::new(code).with_cause(err.to_string())
}

/// I file musicali sul filesystem locale.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFiles;

impl MusicFiles for LocalFiles {
    fn walk(&self, root: &str) -> Result<Vec<DiscoveredFile>, AppError> {
        let mut found = Vec::new();
        // `walkdir` con i link simbolici non seguiti: seguirli permetterebbe a
        // un anello di far girare la scansione all'infinito, e a un link verso
        // la cartella superiore di contare ogni brano due volte.
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                // Una sottocartella illeggibile non ferma la camminata: si perde
                // quel ramo, non l'intera libreria.
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let path = entry.path().to_string_lossy().into_owned();
            found.push(DiscoveredFile {
                path,
                size_bytes: metadata.len(),
                modified_ms: modified_ms(&metadata),
            });
        }
        Ok(found)
    }

    fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError> {
        let file = std::fs::File::open(Path::new(path)).map_err(|err| io_error(path, &err))?;
        Ok(Box::new(std::io::BufReader::new(file)))
    }
}

/// La data di modifica in millisecondi interi.
///
/// Il troncamento è deliberato e deve stare qui, non a valle: è la forma con cui
/// il database la salva e con cui la scansione confronta. Se il confronto
/// avvenisse fra un valore troncato e uno con i decimali, ogni passata vedrebbe
/// cambiato ogni file e una riscansione da centomila brani rileggerebbe tutti i
/// metadati invece di nessuno.
///
/// Un orologio che non si legge dà 0: un file «modificato all'epoca zero» verrà
/// riletto a ogni scansione, che è lento ma corretto — il contrario, dargli
/// l'ora attuale, lo farebbe sembrare aggiornato per sempre.
fn modified_ms(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn la_camminata_trova_i_file_annidati_e_non_filtra() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let nested = dir.path().join("Album").join("CD1");
        std::fs::create_dir_all(&nested).expect("cartelle");
        for name in ["a.mp3", "b.txt", "c.flac"] {
            let mut f = std::fs::File::create(nested.join(name)).expect("file");
            f.write_all(b"x").expect("scrittura");
        }

        let root = dir.path().to_string_lossy().into_owned();
        let found = LocalFiles.walk(&root).expect("camminata");

        // Anche il .txt: filtrare è una decisione, e sta nel dominio. Qui si
        // riporta quel che c'è, altrimenti il piano non potrebbe dire PERCHÉ un
        // file non è entrato in libreria.
        assert_eq!(found.len(), 3);
        assert!(found.iter().any(|f| f.path.ends_with("b.txt")));
    }

    #[test]
    fn una_radice_inesistente_non_fa_cadere_la_scansione() {
        // Il disco esterno staccato: si riporta vuoto, e le altre radici
        // vengono scansionate lo stesso.
        let found = LocalFiles.walk("Z:/non/esiste").expect("nessun errore");
        assert!(found.is_empty());
    }

    #[test]
    fn la_data_di_modifica_e_intera() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let path = dir.path().join("a.mp3");
        std::fs::write(&path, b"x").expect("scrittura");
        let root = dir.path().to_string_lossy().into_owned();
        let found = LocalFiles.walk(&root).expect("camminata");
        let file = found.first().expect("un file");
        assert!(file.modified_ms > 0, "l'orologio deve essere leggibile qui");
        // Due camminate di fila devono dare lo stesso valore: se il troncamento
        // non fosse stabile, ogni passata vedrebbe «cambiato» ogni file.
        let ancora = LocalFiles.walk(&root).expect("camminata");
        assert_eq!(
            ancora.first().map(|f| f.modified_ms),
            Some(file.modified_ms)
        );
    }

    #[test]
    fn gli_errori_di_io_si_distinguono() {
        use aether_domain::errors::ErrorCodeKind;
        let not_found = io_error("x", &std::io::Error::from(std::io::ErrorKind::NotFound));
        let denied = io_error(
            "x",
            &std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        // Durante una scansione i due vogliono reazioni diverse: il primo «salta
        // e prosegui», il secondo «dillo all'utente». Il vecchio albero li
        // appiattiva entrambi in un avviso nel log.
        assert_eq!(not_found.code().kind(), ErrorCodeKind::FsNotFound);
        assert_eq!(denied.code().kind(), ErrorCodeKind::FsPermissionDenied);
    }
}
