//! Dove sta yt-dlp, e cosa dire quando non c'è.
//!
//! Porto di `legacy/Aeter/electron/modules/binaries.ts`, ridotto al solo binario
//! che serve: senza `--extract-audio` non c'è ffmpeg da risolvere, e spotdl e
//! fpcalc appartengono a mestieri che qui non si fanno.
//!
//! # Il messaggio che manca è metà del lavoro
//!
//! [`ErrorCode::DownloadBinaryMissing`] porta `dir` e `url` proprio per questo:
//! «manca yt-dlp» è una constatazione, «manca yt-dlp, mettilo in *questa*
//! cartella, prendilo *qui*» è un'istruzione. Il vecchio albero lo aveva capito e
//! lo faceva viaggiare come stringa (`BINARY_MISSING:<nome>:<cartella>`,
//! `binaries.ts:63`); qui sono campi tipizzati, quindi non si possono dimenticare
//! componendo il messaggio.

use std::path::{Path, PathBuf};

use aether_domain::{AppError, ErrorCode};

/// Il nome del binario, senza estensione.
pub const NOME_YTDLP: &str = "yt-dlp";

/// Dove si va a prenderlo quando manca.
pub const INDIRIZZO_YTDLP: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest";

/// I binari esterni, risolti una volta sola.
///
/// Si costruisce con [`Binari::risolvi`] e si tiene: `exists()` su ogni
/// scaricamento sarebbe una syscall per brano per niente, dato che la cartella
/// delle risorse non cambia mentre l'applicazione gira.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binari {
    cartella: PathBuf,
    ytdlp: Option<PathBuf>,
}

impl Binari {
    /// Cerca i binari nella cartella data.
    ///
    /// Su Windows il nome ha `.exe`, altrove no. Non si prova entrambe le forme:
    /// un `yt-dlp` senza estensione su Windows non sarebbe eseguibile comunque,
    /// e accettarlo qui sposterebbe il guasto a metà scaricamento.
    #[must_use]
    pub fn risolvi(cartella: impl Into<PathBuf>) -> Self {
        let cartella = cartella.into();
        let candidato = cartella.join(nome_file(NOME_YTDLP));
        let ytdlp = candidato.is_file().then_some(candidato);
        Self { cartella, ytdlp }
    }

    /// La cartella in cui sono stati cercati.
    #[must_use]
    pub fn cartella(&self) -> &Path {
        &self.cartella
    }

    /// yt-dlp, se c'è.
    #[must_use]
    pub fn ytdlp(&self) -> Option<&Path> {
        self.ytdlp.as_deref()
    }

    /// yt-dlp, o l'errore che dice dove metterlo e dove prenderlo.
    ///
    /// # Errori
    ///
    /// [`ErrorCode::DownloadBinaryMissing`] quando il file non esiste.
    pub fn richiedi_ytdlp(&self) -> Result<&Path, AppError> {
        self.ytdlp.as_deref().ok_or_else(|| {
            AppError::new(ErrorCode::DownloadBinaryMissing {
                name: NOME_YTDLP.to_owned(),
                dir: Some(self.cartella.display().to_string()),
                url: Some(INDIRIZZO_YTDLP.to_owned()),
            })
        })
    }
}

/// Il nome del file eseguibile per questa piattaforma.
#[must_use]
fn nome_file(nome: &str) -> String {
    if cfg!(windows) {
        format!("{nome}.exe")
    } else {
        nome.to_owned()
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn senza_il_binario_lerrore_dice_dove_e_dove() {
        let binari = Binari::risolvi(Path::new("C:/non/esiste"));
        assert!(binari.ytdlp().is_none());
        let errore = binari
            .richiedi_ytdlp()
            .expect_err("una cartella inesistente non può contenere yt-dlp");
        // Non basta che fallisca: deve fallire *dicendo le due cose*. È la
        // differenza fra un utente che risolve e uno che scrive un'issue.
        match errore.code() {
            ErrorCode::DownloadBinaryMissing { name, dir, url } => {
                assert_eq!(name, NOME_YTDLP);
                assert!(dir.as_deref().is_some_and(|d| d.contains("non")));
                assert_eq!(url.as_deref(), Some(INDIRIZZO_YTDLP));
            }
            altro => panic!("codice inatteso: {altro:?}"),
        }
    }

    #[test]
    fn lo_trova_quando_ce() {
        let temporanea = std::env::temp_dir().join("aether-prova-binari");
        std::fs::create_dir_all(&temporanea).expect("la cartella temporanea si crea");
        let percorso = temporanea.join(nome_file(NOME_YTDLP));
        std::fs::write(&percorso, b"non e' un vero yt-dlp").expect("il file finto si scrive");

        let binari = Binari::risolvi(&temporanea);
        assert_eq!(binari.ytdlp(), Some(percorso.as_path()));
        assert!(binari.richiedi_ytdlp().is_ok());

        std::fs::remove_file(&percorso).expect("il file finto si toglie");
    }

    #[test]
    fn una_cartella_non_e_un_binario() {
        // `exists()` direbbe di sì a una cartella chiamata `yt-dlp.exe`, e il
        // guasto si vedrebbe solo allo spawn, con un messaggio del sistema
        // operativo invece che con il codice del catalogo.
        let temporanea = std::env::temp_dir().join("aether-prova-binari-cartella");
        let finta = temporanea.join(nome_file(NOME_YTDLP));
        std::fs::create_dir_all(&finta).expect("la cartella finta si crea");

        assert!(Binari::risolvi(&temporanea).ytdlp().is_none());

        std::fs::remove_dir_all(&temporanea).expect("la cartella finta si toglie");
    }
}
