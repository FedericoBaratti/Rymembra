//! Dove la sincronia mette i suoi file, qualunque cosa sia quel «dove».
//!
//! # Cinque operazioni, e non venti
//!
//! Elencare, leggere, scrivere, rinominare, cancellare. Non è un minimo comune
//! denominatore scelto per prudenza: è esattamente ciò che `aether_cloud::servizio`
//! usa già oggi su Drive, dopo che qualcuno ha scritto quel codice guardando il
//! problema vero. Un tratto più largo — cartelle, permessi, ricerche — sarebbe un
//! tratto che S3 e una cartella locale devono fingere di avere.
//!
//! # Perché [`Cartella`] non dichiara le impronte
//!
//! L'impronta serve a una cosa sola: sapere se vale la pena **scaricare** un file.
//! Su una cartella non c'è niente da scaricare — leggere è già l'operazione
//! economica — e calcolarla in `elenca` vorrebbe dire aprire e decomprimere tutti
//! i documenti per decidere se aprirli. Restituire `None` vuol dire «non si sa», e
//! chi legge legge: è la direzione che non salta mai un aggiornamento, ed è la
//! stessa che `FileRemoto::impronta` documenta già per i file scritti da versioni
//! che non la scrivevano.
//!
//! # Perché si scrive su un file di passaggio
//!
//! Perché la cartella la sta guardando qualcun altro. Syncthing, il client di
//! Drive o quello di Dropbox si accorgono di un file **mentre** lo si sta
//! scrivendo, e spediscono a un altro dispositivo mezzo documento — che è un gzip
//! troncato, cioè un `sync.remoteCorrupt` sul computer di qualcun altro, per una
//! scrittura che qui era andata benissimo. Si scrive accanto e si rinomina: la
//! rinomina è atomica, e chi guarda vede il file o non lo vede.

use std::fs;
use std::path::{Path, PathBuf};

use aether_domain::errors::{AppError, ErrorCode};

/// Un file nel magazzino.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRemoto {
    /// Come si nomina questo file per riaverlo. Su Drive è l'identificativo, su
    /// una cartella è il nome.
    pub id: String,
    /// Il nome.
    pub nome: String,
    /// Quanto pesa.
    pub byte: u64,
    /// L'impronta del contenuto, quando il magazzino la sa tenere.
    ///
    /// `None` vale «non si sa», e chi legge legge: è la direzione che non salta
    /// un aggiornamento.
    pub impronta: Option<String>,
}

/// Dove la sincronia mette i suoi file.
pub trait Magazzino: Send + Sync {
    /// Tutto quel che c'è, con le impronte che si conoscono.
    ///
    /// # Errori
    ///
    /// Quelli del trasporto: `fs.*` per una cartella, `net.*` per una nuvola.
    fn elenca(&self) -> Result<Vec<FileRemoto>, AppError>;

    /// I byte di un file.
    ///
    /// # Errori
    ///
    /// Quelli del trasporto, e `fs.notFound` o `net.http` con 404 se il file non
    /// c'è più — cosa che capita davvero, quando qualcuno ha svuotato la cartella
    /// mentre Aether era chiuso.
    fn leggi(&self, id: &str) -> Result<Vec<u8>, AppError>;

    /// Scrive dei byte, creando il file o sostituendo quello che c'era.
    ///
    /// `esistente` è l'identificativo del file da sostituire quando lo si conosce
    /// già dall'elenco: senza, un magazzino che ammette due file con lo stesso
    /// nome — Drive lo ammette — ne accumulerebbe una copia a ogni passata.
    ///
    /// # Errori
    ///
    /// Quelli del trasporto.
    fn scrivi(
        &self,
        nome: &str,
        esistente: Option<&str>,
        tipo: &str,
        dati: &[u8],
        impronta: &str,
    ) -> Result<FileRemoto, AppError>;

    /// Cambia nome a un file.
    ///
    /// Serve a mettere da parte un documento illeggibile invece di sovrascriverlo:
    /// quel che è già successo una volta lo si conserva finché qualcuno non decide.
    ///
    /// # Errori
    ///
    /// Quelli del trasporto.
    fn rinomina(&self, id: &str, nome: &str) -> Result<(), AppError>;

    /// Toglie un file.
    ///
    /// # Errori
    ///
    /// Quelli del trasporto.
    fn cancella(&self, id: &str) -> Result<(), AppError>;
}

/// Una cartella sul disco.
///
/// È il magazzino che copre più casi di tutti gli altri messi insieme, senza una
/// riga di rete: Syncthing, iCloud Drive, OneDrive, Dropbox e il client desktop
/// di Google Drive espongono tutti una cartella sincronizzata, e per Aether sono
/// indistinguibili. È anche il magazzino con cui si provano tutti gli altri,
/// perché è l'unico che si può riempire dentro un test.
#[derive(Debug, Clone)]
pub struct Cartella {
    radice: PathBuf,
}

impl Cartella {
    /// Il magazzino dentro questa cartella.
    ///
    /// La cartella non deve esistere: la si crea alla prima scrittura. Chiederla
    /// esistente vorrebbe dire che scegliere una cartella nuova in un pannello
    /// delle impostazioni è un errore.
    #[must_use]
    pub fn nuova(radice: impl Into<PathBuf>) -> Self {
        Self {
            radice: radice.into(),
        }
    }

    /// Dove sta, per chi lo deve mostrare.
    #[must_use]
    pub fn radice(&self) -> &Path {
        &self.radice
    }

    /// Il percorso di un file, rifiutando tutto ciò che uscirebbe dalla cartella.
    ///
    /// Un nome che contiene un separatore o un `..` non è un nostro file: viene da
    /// un elenco che qualcuno ha manomesso, e seguirlo vorrebbe dire leggere o
    /// scrivere dove non ci compete.
    fn percorso(&self, nome: &str) -> Result<PathBuf, AppError> {
        let pulito = !nome.is_empty()
            && nome != "."
            && nome != ".."
            && !nome.contains('/')
            && !nome.contains('\\')
            && !nome.contains('\0');
        if !pulito {
            return Err(AppError::new(ErrorCode::FsPathInvalid {
                path: nome.to_owned(),
            }));
        }
        Ok(self.radice.join(nome))
    }

    /// Il testo di un percorso, per i messaggi d'errore.
    fn testo(percorso: &Path) -> String {
        percorso.to_string_lossy().into_owned()
    }

    /// Traduce un guasto del filesystem in un codice del catalogo.
    fn guasto(percorso: &Path, err: &std::io::Error, scrivendo: bool) -> AppError {
        let path = Self::testo(percorso);
        let detail = Some(err.to_string());
        match err.kind() {
            std::io::ErrorKind::NotFound => AppError::new(ErrorCode::FsNotFound { path }),
            std::io::ErrorKind::PermissionDenied => {
                AppError::new(ErrorCode::FsPermissionDenied { path })
            }
            // `StorageFull` è stabile da 1.83, e distinguerlo conta: un disco pieno
            // non è un guasto da ritentare, è un guasto da dire.
            std::io::ErrorKind::StorageFull => {
                AppError::new(ErrorCode::FsDiskFull { path: Some(path) })
            }
            _ if scrivendo => AppError::new(ErrorCode::FsWriteFailed { path, detail }),
            _ => AppError::new(ErrorCode::FsReadFailed { path, detail }),
        }
    }
}

impl Magazzino for Cartella {
    fn elenca(&self) -> Result<Vec<FileRemoto>, AppError> {
        let voci = match fs::read_dir(&self.radice) {
            Ok(voci) => voci,
            // Una cartella che non c'è ancora è una cartella vuota, non un guasto:
            // è lo stato in cui si trova chiunque abbia appena scelto dove
            // sincronizzare e non abbia ancora scritto niente.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(Self::guasto(&self.radice, &err, false)),
        };

        let mut trovati = Vec::new();
        for voce in voci {
            let voce = match voce {
                Ok(voce) => voce,
                // Una singola voce illeggibile non porta via l'elenco: è la stessa
                // indulgenza per record che `documento::interpreta` applica dentro
                // un file.
                Err(_) => continue,
            };
            let Ok(dati) = voce.metadata() else { continue };
            if !dati.is_file() {
                continue;
            }
            let nome = voce.file_name().to_string_lossy().into_owned();
            // I file di passaggio di una scrittura interrotta: si saltano, o si
            // proverebbe a interpretare mezzo documento.
            if nome.ends_with(PASSAGGIO) {
                continue;
            }
            trovati.push(FileRemoto {
                id: nome.clone(),
                nome,
                byte: dati.len(),
                impronta: None,
            });
        }
        trovati.sort_by(|uno, due| uno.nome.cmp(&due.nome));
        Ok(trovati)
    }

    fn leggi(&self, id: &str) -> Result<Vec<u8>, AppError> {
        let percorso = self.percorso(id)?;
        fs::read(&percorso).map_err(|err| Self::guasto(&percorso, &err, false))
    }

    fn scrivi(
        &self,
        nome: &str,
        _esistente: Option<&str>,
        _tipo: &str,
        dati: &[u8],
        impronta: &str,
    ) -> Result<FileRemoto, AppError> {
        let percorso = self.percorso(nome)?;
        fs::create_dir_all(&self.radice).map_err(|err| Self::guasto(&self.radice, &err, true))?;

        // Il file di passaggio sta **nella stessa cartella** e non in quella
        // temporanea di sistema: una rinomina è atomica solo dentro lo stesso
        // volume, e fra `%TEMP%` e la cartella scelta dall'utente non c'è nessuna
        // garanzia che il volume sia quello.
        let passaggio = self.radice.join(format!("{nome}{PASSAGGIO}"));
        fs::write(&passaggio, dati).map_err(|err| Self::guasto(&passaggio, &err, true))?;
        fs::rename(&passaggio, &percorso).map_err(|err| {
            // Se la rinomina fallisce il file di passaggio resta lì, e la prossima
            // passata lo salta perché `elenca` non lo guarda. Toglierlo comunque è
            // pulizia, non correttezza: se anche questo fallisce non c'è niente da
            // dire a nessuno.
            let _ = fs::remove_file(&passaggio);
            Self::guasto(&percorso, &err, true)
        })?;

        Ok(FileRemoto {
            id: nome.to_owned(),
            nome: nome.to_owned(),
            byte: u64::try_from(dati.len()).unwrap_or(u64::MAX),
            impronta: Some(impronta.to_owned()),
        })
    }

    fn rinomina(&self, id: &str, nome: &str) -> Result<(), AppError> {
        let da = self.percorso(id)?;
        let a = self.percorso(nome)?;
        fs::rename(&da, &a).map_err(|err| Self::guasto(&da, &err, true))
    }

    fn cancella(&self, id: &str) -> Result<(), AppError> {
        let percorso = self.percorso(id)?;
        match fs::remove_file(&percorso) {
            Ok(()) => Ok(()),
            // Cancellare qualcosa che non c'è è già cancellato: rendere idempotente
            // questa operazione è ciò che permette di ripetere una passata andata a
            // metà senza dover sapere fin dove era arrivata.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(Self::guasto(&percorso, &err, true)),
        }
    }
}

/// Il suffisso di un file mentre lo si sta scrivendo.
const PASSAGGIO: &str = ".in-corso";

#[cfg(test)]
mod prove {
    use super::*;

    fn cartella() -> (tempfile::TempDir, Cartella) {
        let dove = tempfile::tempdir().expect("cartella temporanea");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        (dove, magazzino)
    }

    #[test]
    fn una_cartella_che_non_esiste_e_vuota_non_rotta() {
        // È lo stato di chi ha appena scelto dove sincronizzare.
        let (_dove, magazzino) = cartella();
        assert_eq!(magazzino.elenca().expect("elenca"), Vec::new());
    }

    #[test]
    fn quel_che_si_scrive_si_rilegge() {
        let (_dove, magazzino) = cartella();
        let scritto = magazzino
            .scrivi(
                "aether-uno.v1.json.gz",
                None,
                "application/gzip",
                b"ciao",
                "abc",
            )
            .expect("scrive");
        assert_eq!(scritto.byte, 4);
        assert_eq!(magazzino.leggi(&scritto.id).expect("legge"), b"ciao");

        let elenco = magazzino.elenca().expect("elenca");
        assert_eq!(elenco.len(), 1);
        assert_eq!(
            elenco.first().map(|f| f.nome.as_str()),
            Some("aether-uno.v1.json.gz")
        );
        assert_eq!(
            elenco.first().and_then(|f| f.impronta.as_ref()),
            None,
            "una cartella non tiene le impronte: chi legge legge"
        );
    }

    #[test]
    fn riscrivere_sostituisce_invece_di_accumulare() {
        let (_dove, magazzino) = cartella();
        magazzino
            .scrivi("uno.gz", None, "application/gzip", b"prima", "a")
            .expect("scrive");
        magazzino
            .scrivi("uno.gz", Some("uno.gz"), "application/gzip", b"dopo", "b")
            .expect("riscrive");
        assert_eq!(magazzino.elenca().expect("elenca").len(), 1);
        assert_eq!(magazzino.leggi("uno.gz").expect("legge"), b"dopo");
    }

    #[test]
    fn un_file_a_meta_non_compare_nell_elenco() {
        // Chi guarda la cartella — Syncthing, il client di Drive — non deve poter
        // spedire mezzo documento a un altro dispositivo.
        let (_dove, magazzino) = cartella();
        magazzino
            .scrivi("uno.gz", None, "application/gzip", b"intero", "a")
            .expect("scrive");
        fs::write(magazzino.radice().join("uno.gz.in-corso"), b"met").expect("finto file a metà");
        let elenco = magazzino.elenca().expect("elenca");
        assert_eq!(elenco.len(), 1, "il file di passaggio non va elencato");
    }

    #[test]
    fn un_nome_che_esce_dalla_cartella_si_rifiuta() {
        let (_dove, magazzino) = cartella();
        for cattivo in ["../fuori.gz", "sotto/dentro.gz", "..", "", "con\\barra"] {
            let esito = magazzino.leggi(cattivo);
            assert!(
                matches!(
                    esito.as_ref().map_err(AppError::code),
                    Err(ErrorCode::FsPathInvalid { .. })
                ),
                "«{cattivo}» doveva essere rifiutato, invece: {esito:?}"
            );
        }
    }

    #[test]
    fn leggere_un_file_che_non_c_e_lo_dice() {
        let (_dove, magazzino) = cartella();
        let errore = magazzino.leggi("mai-scritto.gz").expect_err("deve fallire");
        assert!(matches!(errore.code(), ErrorCode::FsNotFound { .. }));
    }

    #[test]
    fn cancellare_due_volte_non_e_un_guasto() {
        // È ciò che permette di ripetere una passata andata a metà senza sapere
        // fin dove era arrivata.
        let (_dove, magazzino) = cartella();
        magazzino
            .scrivi("uno.gz", None, "application/gzip", b"x", "a")
            .expect("scrive");
        magazzino.cancella("uno.gz").expect("cancella");
        magazzino
            .cancella("uno.gz")
            .expect("cancellare due volte va bene");
        assert_eq!(magazzino.elenca().expect("elenca").len(), 0);
    }

    #[test]
    fn un_documento_illeggibile_si_mette_da_parte_invece_di_sparire() {
        let (_dove, magazzino) = cartella();
        magazzino
            .scrivi("uno.gz", None, "application/gzip", b"illeggibile", "a")
            .expect("scrive");
        magazzino
            .rinomina("uno.gz", "uno.gz.guasto")
            .expect("rinomina");
        let elenco = magazzino.elenca().expect("elenca");
        assert_eq!(
            elenco.first().map(|f| f.nome.as_str()),
            Some("uno.gz.guasto")
        );
        assert_eq!(
            magazzino.leggi("uno.gz.guasto").expect("legge"),
            b"illeggibile"
        );
    }

    #[test]
    fn l_elenco_esce_in_ordine() {
        // Non per bellezza: due dispositivi che elencano nello stesso ordine
        // fondono nello stesso ordine, e un ordine che dipende dal filesystem è un
        // ordine che cambia fra Windows e Linux.
        let (_dove, magazzino) = cartella();
        for nome in ["zeta.gz", "alfa.gz", "mu.gz"] {
            magazzino
                .scrivi(nome, None, "application/gzip", b"x", "a")
                .expect("scrive");
        }
        let nomi: Vec<String> = magazzino
            .elenca()
            .expect("elenca")
            .into_iter()
            .map(|f| f.nome)
            .collect();
        assert_eq!(nomi, vec!["alfa.gz", "mu.gz", "zeta.gz"]);
    }
}
