//! Il primo avvio: dove sta la musica, senza chiederlo.
//!
//! # Il problema
//!
//! Aether apriva su una stanza vuota che diceva «aggiungi una cartella dalle
//! impostazioni». Le impostazioni sono undici sezioni, la cartella la si sceglie
//! in un dialogo di sistema, e poi bisogna sapere che esiste un pulsante
//! «Scansiona». Sono quattro o cinque gesti prima della prima nota, su un
//! programma che su quella cartella non aveva mai provato a indovinare — mentre
//! `%USERPROFILE%\Music` esiste sul cento per cento delle installazioni di
//! Windows.
//!
//! Questo modulo è il tentativo di indovinare, e non è un'euristica: sono le
//! cartelle che il sistema stesso dichiara musicali, guardate una per una per
//! vedere se dentro c'è qualcosa.
//!
//! # Perché i percorsi arrivano da fuori
//!
//! Perché quali siano le cartelle musicali di un sistema lo sa il sistema, e
//! questo crate non ne conosce nessuno — è lo stesso motivo per cui i file si
//! aprono attraverso [`MusicFiles`] e non con `std::fs`. Su Windows le fornisce
//! Tauri, su Android sarebbe una concessione, in una prova sono tre stringhe.
//! Qui si decide soltanto **cosa farne**.
//!
//! # Non si conta tutto
//!
//! [`TETTO`] ferma il conteggio, e il numero che esce serve a scrivere «più di
//! duemila brani» invece di un numero esatto che nessuno verificherà. Una
//! cartella con centomila file la si attraversa comunque — la camminata non si
//! interrompe a metà — ma è quel che fa già la scansione vera, e chi chiama
//! deve metterci addosso una scadenza esattamente come fa con l'apertura di un
//! brano.

use aether_domain::paths::is_supported_audio_path;

use crate::files::MusicFiles;

/// Oltre quanti brani non si conta più.
///
/// Duemila. Il numero serve a far capire che una cartella non è vuota e quanto
/// grande sia più o meno, non a essere esatto: fra «2000+» e «2137» non c'è
/// nessuna decisione che cambia.
pub const TETTO: usize = 2_000;

/// Una cartella che sembra contenere musica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidata {
    /// Dove.
    pub percorso: String,
    /// Quanti brani ci si sono trovati, fermandosi a [`TETTO`].
    pub brani: usize,
    /// Il conteggio si è fermato al tetto: ce n'erano altri.
    pub troncato: bool,
    /// La camminata ha perso dei rami — permessi negati, una share a metà.
    ///
    /// Non impedisce di proporre la cartella: dice solo che il numero è una
    /// sottostima. Nasconderla per prudenza vorrebbe dire nascondere la cartella
    /// giusta a chi ha un disco esterno che si sta ancora svegliando.
    pub parziale: bool,
}

/// Quali fra queste cartelle contengono musica.
///
/// Restituisce solo quelle con almeno un brano, nell'ordine in cui sono
/// arrivate: chi chiama le ha già messe in ordine di probabilità, e riordinarle
/// per numero metterebbe davanti la cartella dei download di qualcun altro solo
/// perché è più piena.
///
/// I duplicati e le cartelle annidate una nell'altra **non** si tolgono qui: è
/// una decisione che riguarda le radici della libreria, e la prende
/// [`crate::library`] quando le riceve. Qui si guarda e si riferisce.
#[must_use]
pub fn esamina(files: &dyn MusicFiles, percorsi: &[String]) -> Vec<Candidata> {
    let mut fuori = Vec::new();
    for percorso in percorsi {
        // Una cartella che non c'è non è un errore da propagare: su un sistema
        // senza OneDrive, `…\OneDrive\Music` semplicemente non esiste, ed è il
        // caso normale e non il guasto.
        if !files.radice_raggiungibile(percorso) {
            continue;
        }
        let Ok(camminata) = files.walk(percorso) else {
            continue;
        };
        let mut brani = 0_usize;
        let mut troncato = false;
        for trovato in &camminata.file {
            if !is_supported_audio_path(&trovato.path) {
                continue;
            }
            brani = brani.saturating_add(1);
            if brani >= TETTO {
                troncato = true;
                break;
            }
        }
        if brani == 0 {
            continue;
        }
        fuori.push(Candidata {
            percorso: percorso.clone(),
            brani,
            troncato,
            parziale: !camminata.completa,
        });
    }
    fuori
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::files::{Camminata, ReadSeek};
    use aether_domain::errors::{AppError, ErrorCode};
    use aether_domain::scan_plan::DiscoveredFile;
    use std::collections::HashMap;

    /// Un filesystem finto: una mappa da cartella a elenco di file.
    struct Finto {
        alberi: HashMap<String, (Vec<String>, bool)>,
    }

    impl Finto {
        fn nuovo(alberi: &[(&str, &[&str], bool)]) -> Self {
            Self {
                alberi: alberi
                    .iter()
                    .map(|(radice, file, completa)| {
                        (
                            (*radice).to_owned(),
                            (file.iter().map(|f| (*f).to_owned()).collect(), *completa),
                        )
                    })
                    .collect(),
            }
        }
    }

    impl MusicFiles for Finto {
        fn walk(&self, root: &str) -> Result<Camminata, AppError> {
            let Some((file, completa)) = self.alberi.get(root) else {
                return Ok(Camminata {
                    file: Vec::new(),
                    completa: true,
                });
            };
            Ok(Camminata {
                file: file
                    .iter()
                    .map(|path| DiscoveredFile {
                        path: path.clone(),
                        size_bytes: 1,
                        modified_ms: 0,
                    })
                    .collect(),
                completa: *completa,
            })
        }

        fn radice_raggiungibile(&self, root: &str) -> bool {
            self.alberi.contains_key(root)
        }

        fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError> {
            Err(AppError::new(ErrorCode::FsReadFailed {
                path: path.to_owned(),
                detail: None,
            }))
        }
    }

    #[test]
    fn una_cartella_con_musica_si_propone_col_suo_numero() {
        let files = Finto::nuovo(&[(
            "C:/Musica",
            &["C:/Musica/a.mp3", "C:/Musica/b.flac", "C:/Musica/note.txt"],
            true,
        )]);
        let viste = esamina(&files, &["C:/Musica".to_owned()]);
        assert_eq!(
            viste,
            vec![Candidata {
                percorso: "C:/Musica".to_owned(),
                brani: 2,
                troncato: false,
                parziale: false,
            }]
        );
    }

    #[test]
    fn una_cartella_senza_musica_non_si_propone() {
        let files = Finto::nuovo(&[("C:/Documenti", &["C:/Documenti/a.txt"], true)]);
        assert!(esamina(&files, &["C:/Documenti".to_owned()]).is_empty());
    }

    #[test]
    fn una_cartella_che_non_esiste_non_e_un_guasto() {
        // Il caso di `OneDrive\Music` su un sistema senza OneDrive: capita
        // sempre, e non deve comparire da nessuna parte.
        let files = Finto::nuovo(&[]);
        assert!(esamina(&files, &["C:/Utenti/x/OneDrive/Musica".to_owned()]).is_empty());
    }

    #[test]
    fn l_ordine_e_quello_ricevuto_e_non_quello_dei_numeri() {
        let files = Finto::nuovo(&[
            ("C:/Musica", &["C:/Musica/a.mp3"], true),
            (
                "C:/Scarichi",
                &[
                    "C:/Scarichi/1.mp3",
                    "C:/Scarichi/2.mp3",
                    "C:/Scarichi/3.mp3",
                ],
                true,
            ),
        ]);
        let viste = esamina(&files, &["C:/Musica".to_owned(), "C:/Scarichi".to_owned()]);
        let percorsi: Vec<&str> = viste.iter().map(|c| c.percorso.as_str()).collect();
        assert_eq!(percorsi, vec!["C:/Musica", "C:/Scarichi"]);
    }

    #[test]
    fn il_conteggio_si_ferma_al_tetto_e_lo_dice() {
        let molti: Vec<String> = (0..(TETTO + 500))
            .map(|i| format!("C:/Musica/{i}.mp3"))
            .collect();
        let riferimenti: Vec<&str> = molti.iter().map(String::as_str).collect();
        let files = Finto::nuovo(&[("C:/Musica", &riferimenti, true)]);
        let viste = esamina(&files, &["C:/Musica".to_owned()]);
        let prima = viste.first().expect("una cartella");
        assert_eq!(prima.brani, TETTO);
        assert!(prima.troncato);
    }

    #[test]
    fn una_camminata_parziale_si_propone_lo_stesso_dicendolo() {
        // Un disco esterno che si sta svegliando, o una sottocartella a permessi
        // negati: la cartella giusta non va nascosta per prudenza.
        let files = Finto::nuovo(&[("D:/Musica", &["D:/Musica/a.mp3"], false)]);
        let viste = esamina(&files, &["D:/Musica".to_owned()]);
        let prima = viste.first().expect("una cartella");
        assert!(prima.parziale);
        assert_eq!(prima.brani, 1);
    }

    #[test]
    fn le_estensioni_che_non_sono_musica_non_contano() {
        let files = Finto::nuovo(&[(
            "C:/M",
            &[
                "C:/M/a.mp3",
                "C:/M/b.jpg",
                "C:/M/c.m3u",
                "C:/M/d.opus",
                "C:/M/e.mp3.txt",
            ],
            true,
        )]);
        let viste = esamina(&files, &["C:/M".to_owned()]);
        // `opus` è fra le estensioni riconosciute anche se il decodificatore non
        // lo suona: una cartella di opus contiene musica, e dirle vuota sarebbe
        // una bugia peggiore del brano che poi non parte.
        assert_eq!(viste.first().map(|c| c.brani), Some(2));
    }
}
