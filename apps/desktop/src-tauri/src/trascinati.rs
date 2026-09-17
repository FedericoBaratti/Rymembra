//! Quel che arriva trascinando dei file sulla finestra.
//!
//! # Perché il nucleo, e non la finestra
//!
//! Il gestore stava tutto nella finestra, e mandava ogni percorso che non fosse
//! una skin fra le cartelle sorvegliate: la ragione scritta accanto era che
//! distinguere una cartella da un file vorrebbe dire chiedere al filesystem
//! dall'interfaccia. La ragione è giusta, e la conclusione no — un `.m3u8`
//! lasciato cadere finiva fra le **cartelle sorvegliate**, e con lui ogni MP3.
//! La domanda al filesystem va fatta, e va fatta qui.
//!
//! La finestra riceve i percorsi già divisi per quel che se ne fa, e decide solo
//! dove portare chi guarda.

use aether_domain::paths::{PathRules, extension_of, is_supported_audio_path};
use serde::Serialize;
use tauri::State;

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Le estensioni che l'importazione di playlist sa leggere.
///
/// Le stesse del filtro del dialogo in `App.tsx`: un file che si può scegliere
/// da lì si può anche lasciar cadere, e il contrario.
const ESTENSIONI_PLAYLIST: [&str; 4] = ["m3u", "m3u8", "pls", "xspf"];

/// I percorsi trascinati, divisi per destinazione.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trascinati {
    /// Da aggiungere alle cartelle sorvegliate.
    pub cartelle: Vec<String>,
    /// Pacchetti `.aeskin` da installare.
    pub skin: Vec<String>,
    /// File di playlist da importare.
    pub playlist: Vec<String>,
    /// I brani in libreria corrispondenti ai file audio, nell'ordine in cui
    /// sono arrivati: da accodare.
    pub brani: Vec<i64>,
    /// File audio che la libreria non conosce.
    ///
    /// Un numero e non un elenco: la finestra ne fa una frase sola — «aggiungi
    /// la cartella che li contiene» — e non ha niente da fare con i nomi.
    pub fuori_libreria: usize,
    /// Tutto il resto: né cartelle né file che Aether sappia usare.
    pub ignorati: usize,
}

/// Divide i percorsi, senza il database.
///
/// Separata dal comando per provarla senza una libreria: `e_cartella` è la
/// domanda al filesystem, e nelle prove è una chiusura. I file audio tornano
/// come percorsi, e a tradurli in brani ci pensa il comando.
fn smista(percorsi: Vec<String>, e_cartella: impl Fn(&str) -> bool) -> (Trascinati, Vec<String>) {
    let mut smistati = Trascinati::default();
    let mut audio = Vec::new();
    for percorso in percorsi {
        if e_cartella(&percorso) {
            smistati.cartelle.push(percorso);
            continue;
        }
        let estensione = extension_of(&percorso);
        if estensione == "aeskin" {
            smistati.skin.push(percorso);
        } else if ESTENSIONI_PLAYLIST.contains(&estensione.as_str()) {
            smistati.playlist.push(percorso);
        } else if is_supported_audio_path(&percorso) {
            audio.push(percorso);
        } else {
            smistati.ignorati = smistati.ignorati.saturating_add(1);
        }
    }
    (smistati, audio)
}

/// Divide i percorsi lasciati cadere sulla finestra.
///
/// `async` perché chiede al filesystem se ogni percorso è una cartella, e su una
/// condivisione di rete la risposta può farsi aspettare: non sul filo che
/// disegna la finestra. Il lucchetto della libreria si prende **dopo**, e solo
/// se fra i file ce n'è qualcuno audio.
#[tauri::command(async)]
pub fn smista_trascinati(stato: State<'_, Stato>, percorsi: Vec<String>) -> Esito<Trascinati> {
    let (mut smistati, audio) =
        smista(percorsi, |percorso| std::path::Path::new(percorso).is_dir());
    if audio.is_empty() {
        return Ok(smistati);
    }
    let trovati = con_libreria(&stato, |libreria| {
        aether_app::library::ids_by_path(
            &libreria.connection,
            &audio,
            PathRules::for_current_platform(),
        )
    })
    .map_err(errore)?;
    for trovato in trovati {
        match trovato {
            Some(id) => smistati.brani.push(id),
            None => smistati.fuori_libreria = smistati.fuori_libreria.saturating_add(1),
        }
    }
    Ok(smistati)
}

#[cfg(test)]
mod prove {
    use super::*;

    fn tutti(percorsi: &[&str]) -> Vec<String> {
        percorsi.iter().map(|p| (*p).to_owned()).collect()
    }

    #[test]
    fn ogni_file_va_dove_serve() {
        let (smistati, audio) = smista(
            tutti(&[
                r"C:\Musica",
                r"C:\Scaricati\Serata.M3U8",
                r"C:\Scaricati\vecchia.pls",
                r"C:\Scaricati\notte.aeskin",
                r"C:\Scaricati\01 - Brano.flac",
                r"C:\Scaricati\note.txt",
            ]),
            |percorso| percorso == r"C:\Musica",
        );
        assert_eq!(smistati.cartelle, tutti(&[r"C:\Musica"]));
        assert_eq!(
            smistati.playlist,
            tutti(&[r"C:\Scaricati\Serata.M3U8", r"C:\Scaricati\vecchia.pls"]),
            "l'estensione si confronta in minuscolo"
        );
        assert_eq!(smistati.skin, tutti(&[r"C:\Scaricati\notte.aeskin"]));
        assert_eq!(audio, tutti(&[r"C:\Scaricati\01 - Brano.flac"]));
        assert_eq!(smistati.ignorati, 1);
    }

    #[test]
    fn una_cartella_col_punto_nel_nome_resta_una_cartella() {
        // «Album.m3u» è un nome di cartella possibile, e l'estensione non deve
        // vincere sulla risposta del filesystem.
        let (smistati, audio) = smista(tutti(&[r"D:\Raccolte\Album.m3u"]), |_| true);
        assert_eq!(smistati.cartelle, tutti(&[r"D:\Raccolte\Album.m3u"]));
        assert!(smistati.playlist.is_empty());
        assert!(audio.is_empty());
    }
}
