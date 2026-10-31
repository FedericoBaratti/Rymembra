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

use rymembra_app::import_playlist::{self, Dischi};
use rymembra_domain::eredita;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::{PathRules, extension_of, is_supported_audio_path};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use crate::disparte::in_disparte;
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
    /// Pacchetti di skin da installare: `.ryskin`, e anche quelli con
    /// l'estensione di prima, che l'installazione riscrive col nome nuovo.
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
    /// Tutto il resto: né cartelle né file che Rymembra sappia usare.
    pub ignorati: usize,
}

/// Divide i percorsi, senza il database.
///
/// Separata dal comando per provarla senza una libreria: `e_cartella` è la
/// domanda al filesystem, e nelle prove è una chiusura. I file audio e i fogli
/// `.cue` tornano come percorsi, nell'ordine in cui sono arrivati, e a
/// tradurli in brani ci pensa il comando.
fn smista(percorsi: Vec<String>, e_cartella: impl Fn(&str) -> bool) -> (Trascinati, Vec<String>) {
    let mut smistati = Trascinati::default();
    let mut da_cercare = Vec::new();
    for percorso in percorsi {
        if e_cartella(&percorso) {
            smistati.cartelle.push(percorso);
            continue;
        }
        let estensione = extension_of(&percorso);
        // Il pacchetto di prima si legge ancora: chi ha una skin scaricata o
        // salvata a suo tempo la lascia cadere sulla finestra come sempre, e
        // `skin::installa` non guarda l'estensione, solo il contenuto.
        if estensione == "ryskin" || estensione == eredita::ESTENSIONE_SKIN {
            smistati.skin.push(percorso);
        } else if ESTENSIONI_PLAYLIST.contains(&estensione.as_str()) {
            smistati.playlist.push(percorso);
        } else if estensione == ESTENSIONE_FOGLIO || is_supported_audio_path(&percorso) {
            // Un `.cue` non è un file audio — la scansione lo tiene fuori — ma
            // chi lo lascia cadere vuole il disco che descrive: lo si cerca
            // insieme ai file audio, e diventa i suoi brani. Un `.ape` passa
            // di qui come ogni altro formato che si sa suonare.
            da_cercare.push(percorso);
        } else {
            smistati.ignorati = smistati.ignorati.saturating_add(1);
        }
    }
    (smistati, da_cercare)
}

/// L'estensione dei fogli che tagliano un'immagine CD nei suoi brani.
const ESTENSIONE_FOGLIO: &str = "cue";

/// Divide i percorsi lasciati cadere sulla finestra.
///
/// In disparte, sul pool bloccante, perché chiede al filesystem se ogni
/// percorso è una cartella, e su una condivisione di rete che non risponde la
/// risposta vale i quaranta secondi di Windows. Fino alla 2.4.0 era `(async)`,
/// cioè quei secondi passati su un *worker* del runtime: dieci cartelle di un
/// NAS spento trascinate insieme bastavano a fermare ogni altro comando
/// asincrono. Il lucchetto della libreria si prende **dopo**, e solo se fra i
/// file ce n'è qualcuno audio.
#[tauri::command]
pub async fn smista_trascinati(app: AppHandle, percorsi: Vec<String>) -> Esito<Trascinati> {
    in_disparte("smistamento dei file trascinati", move || {
        let Some(stato) = app.try_state::<Stato>() else {
            return Err(AppError::new(ErrorCode::InternalAborted {
                what: Some("smistamento dei file trascinati".to_owned()),
            })
            .with_cause("la libreria non è più fra gli stati gestiti"));
        };
        smista_e_cerca(&stato, percorsi)
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Il corpo di [`smista_trascinati`], in disparte.
///
/// Un file audio diventa il suo brano; un `.cue` diventa i brani del suo
/// disco, in ordine di traccia (`import_playlist::indice_dei_fogli`).
/// Così anche l'immagine CD, il file grande che il `.cue` taglia: in libreria
/// non è una riga, e chi la lascia cadere vuole il disco, come chi lascia
/// cadere il foglio (`import_playlist::indice_delle_immagini`). Vedi
/// [`brani_dei_trascinati`].
/// Quel che la libreria non conosce — anche un `.cue` di cui non ha nessun
/// brano — conta fra i `fuori_libreria`, e la finestra suggerisce di aggiungere
/// la cartella.
fn smista_e_cerca(stato: &Stato, percorsi: Vec<String>) -> Result<Trascinati, AppError> {
    let (mut smistati, da_cercare) =
        smista(percorsi, |percorso| std::path::Path::new(percorso).is_dir());
    if da_cercare.is_empty() {
        return Ok(smistati);
    }
    let trovati = con_libreria(stato, |libreria| {
        let audio: Vec<String> = da_cercare
            .iter()
            .filter(|percorso| !e_un_foglio(percorso))
            .cloned()
            .collect();
        let per_percorso = rymembra_app::library::ids_by_path(
            &libreria.connection,
            &audio,
            PathRules::for_current_platform(),
        )?
        .into_iter();
        let connessione = &libreria.connection;
        brani_dei_trascinati(
            &da_cercare,
            per_percorso,
            || import_playlist::indice_dei_fogli(connessione),
            || import_playlist::indice_delle_immagini(connessione),
        )
    })?;
    for brani in trovati {
        if brani.is_empty() {
            smistati.fuori_libreria = smistati.fuori_libreria.saturating_add(1);
        } else {
            smistati.brani.extend(brani);
        }
    }
    Ok(smistati)
}

/// Il percorso è un foglio `.cue`.
fn e_un_foglio(percorso: &str) -> bool {
    extension_of(percorso) == ESTENSIONE_FOGLIO
}

/// I brani di ogni percorso da cercare, nell'ordine in cui sono arrivati —
/// fogli e file mescolati: è l'ordine in cui finiscono in coda. Vuoto per
/// quel che la libreria non conosce.
///
/// `per_percorso` dà, per ogni file che non è un foglio, la sua riga se ce
/// l'ha. Un foglio diventa i brani del suo disco; un file senza riga si cerca
/// fra le immagini CD — prima la riga col suo percorso, poi l'immagine: se la
/// vecchia riga del disco intero c'è ancora, è quella che la libreria mostra,
/// e risponde lei.
///
/// # Gli indici, una volta per gesto
///
/// Gli indici dei dischi d'immagine (`fogli`, `immagini`) leggono **tutti** i
/// brani d'immagine della libreria. Si costruiscono al più una volta per
/// gesto, e solo se servono: prima se ne costruiva uno per file, e chi
/// trascinava quattrocento file dalla cartella dei download — fuori libreria,
/// cioè proprio quelli senza riga — rileggeva la tabella quattrocento volte
/// col lucchetto della libreria in mano. La ricerca non consuma l'indice: lo
/// stesso disco trascinato due volte risponde due volte. Separata dal comando
/// per contare, nelle prove, quante volte si costruiscono.
fn brani_dei_trascinati(
    da_cercare: &[String],
    mut per_percorso: impl Iterator<Item = Option<i64>>,
    mut fogli: impl FnMut() -> Result<Dischi, AppError>,
    mut immagini: impl FnMut() -> Result<Dischi, AppError>,
) -> Result<Vec<Vec<i64>>, AppError> {
    let mut indice_dei_fogli: Option<Dischi> = None;
    let mut indice_delle_immagini: Option<Dischi> = None;
    let mut trovati: Vec<Vec<i64>> = Vec::with_capacity(da_cercare.len());
    for percorso in da_cercare {
        if e_un_foglio(percorso) {
            trovati.push(pigro(&mut indice_dei_fogli, &mut fogli)?.brani(percorso));
        } else if let Some(id) = per_percorso.next().flatten() {
            trovati.push(vec![id]);
        } else {
            trovati.push(pigro(&mut indice_delle_immagini, &mut immagini)?.brani(percorso));
        }
    }
    Ok(trovati)
}

/// L'indice nel suo posto, costruito alla prima richiesta.
fn pigro<'a>(
    posto: &'a mut Option<Dischi>,
    costruisci: &mut impl FnMut() -> Result<Dischi, AppError>,
) -> Result<&'a Dischi, AppError> {
    let dischi = match posto.take() {
        Some(dischi) => dischi,
        None => costruisci()?,
    };
    Ok(posto.insert(dischi))
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
                r"C:\Scaricati\notte.ryskin",
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
        assert_eq!(smistati.skin, tutti(&[r"C:\Scaricati\notte.ryskin"]));
        assert_eq!(audio, tutti(&[r"C:\Scaricati\01 - Brano.flac"]));
        assert_eq!(smistati.ignorati, 1);
    }

    #[test]
    fn la_skin_con_l_estensione_di_prima_si_installa_ancora() {
        // Compatibilità: un pacchetto salvato prima del cambio di nome è una
        // skin, non un file «ignorato». L'estensione si confronta in minuscolo
        // come tutte le altre, e quella nuova non ha smesso di valere.
        let vecchia = format!(r"C:\Scaricati\vecchia.{}", eredita::ESTENSIONE_SKIN);
        let maiuscola = format!(
            r"C:\Scaricati\GRANDE.{}",
            eredita::ESTENSIONE_SKIN.to_uppercase()
        );
        let nuova = r"C:\Scaricati\nuova.RySkin".to_owned();
        let (smistati, audio) = smista(
            vec![
                vecchia.clone(),
                maiuscola.clone(),
                nuova.clone(),
                r"C:\Scaricati\altro.zip".to_owned(),
            ],
            |_| false,
        );
        assert_eq!(smistati.skin, vec![vecchia, maiuscola, nuova]);
        assert!(audio.is_empty());
        assert_eq!(smistati.ignorati, 1);
    }

    #[test]
    fn un_foglio_e_un_ape_si_cercano_insieme_ai_file_audio() {
        // L'ordine è quello d'arrivo, fogli e file mescolati: è l'ordine in
        // cui i brani finiranno in coda.
        let (smistati, da_cercare) = smista(
            tutti(&[
                r"D:\Dischi\Album.CUE",
                r"D:\Dischi\Vecchio\CDImage.ape",
                r"D:\Dischi\01.flac",
                r"D:\Dischi\note.log",
            ]),
            |_| false,
        );
        assert_eq!(
            da_cercare,
            tutti(&[
                r"D:\Dischi\Album.CUE",
                r"D:\Dischi\Vecchio\CDImage.ape",
                r"D:\Dischi\01.flac",
            ])
        );
        assert_eq!(smistati.ignorati, 1);
    }

    #[test]
    fn un_foglio_trascinato_diventa_i_brani_del_suo_disco() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        con_libreria(&stato, |libreria| {
            libreria
                .connection
                .execute_batch(
                    "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                         duration_ms, file_size, date_added, date_modified)
                     VALUES (1, 'C:\\M\\a.flac', 'k1', 'T', 'A', 'Al', 'g', 1000, 1, 0, 0);
                     INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                         duration_ms, file_size, date_added, date_modified,
                                         immagine, inizio_cd, fine_cd)
                     VALUES (3, 'C:\\M\\Disco\\Album.cue|02', 'k3', 'Due', 'A', 'D', 'd', 1000,
                             9, 0, 0, 'C:\\M\\Disco\\Album.flac', 7500, NULL),
                            (2, 'C:\\M\\Disco\\Album.cue|01', 'k2', 'Uno', 'A', 'D', 'd', 1000,
                             9, 0, 0, 'C:\\M\\Disco\\Album.flac', 0, 7500);",
                )
                .map_err(|err| rymembra_app::db::codice_da_sqlite("banco", &err))
        })
        .expect("banco");

        let smistati = smista_e_cerca(
            &stato,
            tutti(&[
                r"C:\M\Disco\Album.cue",
                r"C:\M\a.flac",
                r"C:\M\nuovo.mp3",
                r"C:\M\Altro\Senza.cue",
            ]),
        )
        .expect("smistati");
        assert_eq!(
            smistati.brani,
            vec![2, 3, 1],
            "il disco in ordine di traccia, poi il file, come sono arrivati"
        );
        assert_eq!(
            smistati.fuori_libreria, 2,
            "il file e il foglio che la libreria non conosce"
        );
    }

    #[test]
    fn l_immagine_trascinata_diventa_i_brani_del_suo_disco() {
        // Il difetto: l'immagine non ha una riga sua, e finiva fra i
        // `fuori_libreria` — «aggiungi la cartella» per un disco che in
        // libreria c'è tutto.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        con_libreria(&stato, |libreria| {
            libreria
                .connection
                .execute_batch(
                    "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                         duration_ms, file_size, date_added, date_modified)
                     VALUES (1, 'C:\\M\\a.flac', 'k1', 'T', 'A', 'Al', 'g', 1000, 1, 0, 0);
                     INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                         duration_ms, file_size, date_added, date_modified,
                                         immagine, inizio_cd, fine_cd)
                     VALUES (3, 'C:\\M\\Disco\\Album.cue|02', 'k3', 'Due', 'A', 'D', 'd', 1000,
                             9, 0, 0, 'C:\\M\\Disco\\Album.flac', 7500, NULL),
                            (2, 'C:\\M\\Disco\\Album.cue|01', 'k2', 'Uno', 'A', 'D', 'd', 1000,
                             9, 0, 0, 'C:\\M\\Disco\\Album.flac', 0, 7500);",
                )
                .map_err(|err| rymembra_app::db::codice_da_sqlite("banco", &err))
        })
        .expect("banco");

        let smistati = smista_e_cerca(
            &stato,
            tutti(&[r"C:\M\a.flac", r"C:\M\Disco\Album.flac", r"C:\M\nuovo.mp3"]),
        )
        .expect("smistati");
        assert_eq!(
            smistati.brani,
            vec![1, 2, 3],
            "il file, poi il disco in ordine di traccia, come sono arrivati"
        );
        assert_eq!(
            smistati.fuori_libreria, 1,
            "solo il file che la libreria non conosce"
        );
    }

    /// Quaranta file che la libreria non conosce, la stessa immagine CD due
    /// volte in due grafie e tre fogli: gli indici dei dischi si leggono una
    /// volta per gesto, non una per file — prima erano quarantadue e tre
    /// letture di tutti i brani d'immagine, col lucchetto della libreria in
    /// mano — e i brani e i fuori libreria restano quelli di sempre.
    #[test]
    fn gli_indici_dei_dischi_si_leggono_una_volta_per_gesto() {
        let libreria = rymembra_app::db::open_in_memory()
            .expect("libreria")
            .connection;
        libreria
            .execute_batch(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, duration_ms,
                                     file_size, date_added, date_modified,
                                     immagine, inizio_cd, fine_cd)
                 VALUES (21, 'C:\\M\\Disco\\Album.cue|02', 'k21', 'Due', 'A', 'D', 1000,
                         9, 0, 0, 'C:\\M\\Disco\\Album.flac', 7500, NULL),
                        (20, 'C:\\M\\Disco\\Album.cue|01', 'k20', 'Uno', 'A', 'D', 1000,
                         9, 0, 0, 'C:\\M\\Disco\\Album.flac', 0, 7500);",
            )
            .expect("un disco d'immagine");
        let mut da_cercare: Vec<String> = (0..40)
            .map(|n| format!(r"C:\Scaricati\{n:02} - Brano.mp3"))
            .collect();
        da_cercare.insert(3, r"C:\M\Disco\Album.flac".to_owned());
        da_cercare.insert(10, "C:/M/Disco/Album.flac".to_owned());
        da_cercare.push(r"C:\M\Disco\Album.cue".to_owned());
        da_cercare.push(r"C:\Altrove\Ignoto.cue".to_owned());
        da_cercare.push("C:/M/Disco/Album.cue".to_owned());
        let file = da_cercare.iter().filter(|p| !e_un_foglio(p)).count();

        let (mut fogli, mut immagini) = (0, 0);
        let trovati = brani_dei_trascinati(
            &da_cercare,
            std::iter::repeat_n(None, file),
            || {
                fogli += 1;
                import_playlist::indice_dei_fogli(&libreria)
            },
            || {
                immagini += 1;
                import_playlist::indice_delle_immagini(&libreria)
            },
        )
        .expect("trovati");
        assert_eq!(
            (fogli, immagini),
            (1, 1),
            "un indice per gesto, non uno per file"
        );
        assert_eq!(trovati.len(), da_cercare.len());
        assert_eq!(trovati[3], vec![20, 21]);
        assert_eq!(
            trovati[10],
            vec![20, 21],
            "la seconda volta, anche scritta così"
        );
        assert_eq!(trovati[42], vec![20, 21]);
        assert!(trovati[43].is_empty(), "un foglio senza brani");
        assert_eq!(trovati[44], vec![20, 21]);
        assert_eq!(
            trovati.iter().filter(|brani| brani.is_empty()).count(),
            41,
            "fuori libreria: i quaranta file e il foglio senza brani"
        );

        // Tutti con la loro riga: nessun indice si costruisce.
        let (mut fogli, mut immagini) = (0, 0);
        let solo_file = tutti(&[r"C:\M\a.flac", r"C:\M\b.flac"]);
        let trovati = brani_dei_trascinati(
            &solo_file,
            [Some(1), Some(2)].into_iter(),
            || {
                fogli += 1;
                import_playlist::indice_dei_fogli(&libreria)
            },
            || {
                immagini += 1;
                import_playlist::indice_delle_immagini(&libreria)
            },
        )
        .expect("trovati");
        assert_eq!((fogli, immagini), (0, 0));
        assert_eq!(trovati, vec![vec![1], vec![2]]);
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
