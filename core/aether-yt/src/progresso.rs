//! Leggere l'avanzamento di yt-dlp senza leggere le sue frasi.
//!
//! Porto di `legacy/Aeter/electron/modules/download/progress.ts`, parte yt-dlp.
//!
//! # Perché sentinelle e non il testo normale
//!
//! `[download]  42.3% of 4.21MiB` è pensato per una persona: cambia con la
//! versione di yt-dlp, cambia con la lingua, e allinea le colonne con spazi che
//! variano. Analizzarlo vuol dire che un aggiornamento di yt-dlp fa sparire la
//! barra di avanzamento senza rompere niente di visibile in prova.
//!
//! Con `--progress-template` è yt-dlp a scrivere il formato che diciamo noi:
//! `AETHER_P:<scaricati>/<totale>`. Le sentinelle sono un contratto fra due righe
//! del nostro codice — quella che le chiede in [`crate::scarica`] e questa che le
//! legge.
//!
//! Le forme vecchie restano riconosciute come ripiego: se una versione di yt-dlp
//! non onorasse il modello, l'avanzamento si degrada invece di sparire.
//!
//! # `AETHER_D` non è un dettaglio
//!
//! È l'unica riga che porta il **percorso finale completo**, stampata da
//! `after_move` cioè dopo che il file è al suo posto. `Destination:` dà solo il
//! nome, e il nome non basta a dire dove: con `-P home/temp` il file passa da una
//! cartella all'altra, e indovinare il percorso vorrebbe dire ricostruire le
//! regole di yt-dlp. Se questa riga si perde, lo scaricamento è riuscito e noi
//! non sappiamo dire quale file sia — che vale come fallito.

/// Un fatto che yt-dlp racconta di sé.
#[derive(Debug, Clone, PartialEq)]
pub enum Evento {
    /// Byte scaricati sul totale. `frazione` è `None` quando il totale non si sa
    /// ancora — succede all'inizio, e mostrare 0% sarebbe un'informazione falsa
    /// invece che assente.
    Avanzamento {
        /// Byte già presi.
        scaricati: u64,
        /// Byte totali, 0 quando yt-dlp non li ha ancora stimati.
        totale: u64,
        /// Da 0 a 1, quando è calcolabile.
        frazione: Option<f32>,
    },
    /// Comincia un file dentro una playlist.
    FileIniziato {
        /// La posizione, da 1.
        indice: u32,
        /// Quanti in tutto.
        totale: u32,
        /// Il titolo secondo YouTube.
        titolo: String,
    },
    /// Un file è finito e sta al suo posto definitivo.
    FileFinito {
        /// Il percorso completo.
        percorso: String,
    },
    /// La percentuale letta dal testo normale, quando le sentinelle mancano.
    PercentualeVecchia {
        /// Da 0 a 1.
        frazione: f32,
    },
    /// Il nome del file di destinazione, dal testo normale. Solo il nome.
    Destinazione {
        /// Il nome del file, senza cartella.
        file: String,
    },
    /// Comincia l'elemento *n* di *m*, dal testo normale.
    Elemento {
        /// La posizione, da 1.
        indice: u32,
        /// Quanti in tutto.
        totale: u32,
    },
}

/// Legge una riga di yt-dlp. `None` quando non dice niente di utile.
///
/// La maggior parte delle righe sono `None`, ed è normale: yt-dlp parla molto.
#[must_use]
pub fn analizza(riga_grezza: &str) -> Option<Evento> {
    let riga = riga_grezza.trim();
    if riga.is_empty() {
        return None;
    }

    if let Some(resto) = riga.strip_prefix("AETHER_P:") {
        return avanzamento(resto);
    }
    if let Some(resto) = riga.strip_prefix("AETHER_F:") {
        return file_iniziato(resto);
    }
    if let Some(resto) = riga.strip_prefix("AETHER_D:") {
        let percorso = resto.trim();
        return (!percorso.is_empty()).then(|| Evento::FileFinito {
            percorso: percorso.to_owned(),
        });
    }

    vecchie_forme(riga)
}

/// `<scaricati>/<totale>`, dove ognuno dei due può essere `NA`.
fn avanzamento(resto: &str) -> Option<Evento> {
    let (sinistra, destra) = resto.split_once('/')?;
    let scaricati = numero_o_zero(sinistra)?;
    let totale = numero_o_zero(destra)?;
    let frazione = (totale > 0).then(|| {
        // `min(1.0)`: yt-dlp stima il totale, e una stima per difetto darebbe
        // frazioni sopra 1 che l'interfaccia mostrerebbe come 103%.
        #[expect(
            clippy::cast_precision_loss,
            reason = "una frazione di avanzamento non ha bisogno di più di 24 bit di mantissa"
        )]
        let grezza = scaricati as f32 / totale as f32;
        grezza.min(1.0)
    });
    Some(Evento::Avanzamento {
        scaricati,
        totale,
        frazione,
    })
}

/// `<indice>/<totale>:<titolo>`, dove indice e totale possono essere `NA`.
fn file_iniziato(resto: &str) -> Option<Evento> {
    let (indice, resto) = resto.split_once('/')?;
    let (totale, titolo) = resto.split_once(':')?;
    Some(Evento::FileIniziato {
        indice: numero_o_uno(indice)?,
        totale: numero_o_uno(totale)?,
        titolo: titolo.trim().to_owned(),
    })
}

/// Un numero, o 0 se yt-dlp ha scritto `NA`.
fn numero_o_zero(campo: &str) -> Option<u64> {
    if campo == "NA" {
        return Some(0);
    }
    campo.parse().ok()
}

/// Un numero, o 1 se yt-dlp ha scritto `NA`.
fn numero_o_uno(campo: &str) -> Option<u32> {
    if campo == "NA" {
        return Some(1);
    }
    campo.parse().ok()
}

/// Le forme leggibili da una persona, tenute come ripiego.
fn vecchie_forme(riga: &str) -> Option<Evento> {
    if let Some(resto) = riga.strip_prefix("[download] Downloading item ")
        && let Some((indice, resto)) = resto.split_once(" of ")
        && let (Ok(indice), Ok(totale)) = (indice.trim().parse(), resto.trim().parse())
    {
        return Some(Evento::Elemento { indice, totale });
    }

    if let Some(resto) = riga.strip_prefix("[download]") {
        let campo = resto.trim_start();
        if let Some(percento) = campo.split('%').next()
            && campo.contains('%')
            && let Ok(valore) = percento.trim().parse::<f32>()
        {
            return Some(Evento::PercentualeVecchia {
                frazione: (valore / 100.0).min(1.0),
            });
        }
    }

    if let Some(posizione) = riga.find("Destination:") {
        let percorso = riga
            .get(posizione.saturating_add("Destination:".len())..)
            .unwrap_or("")
            .trim();
        // Solo il nome: quel che c'è prima è una cartella temporanea che non è
        // dove il file finirà.
        let file = percorso.rsplit(['/', '\\']).next().unwrap_or(percorso);
        return (!file.is_empty()).then(|| Evento::Destinazione {
            file: file.to_owned(),
        });
    }

    None
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_sentinella_di_avanzamento_da_la_frazione() {
        assert_eq!(
            analizza("AETHER_P:512/1024"),
            Some(Evento::Avanzamento {
                scaricati: 512,
                totale: 1024,
                frazione: Some(0.5)
            })
        );
    }

    #[test]
    fn senza_totale_la_frazione_e_assente_non_zero() {
        // Zero e «non lo so ancora» sono due cose diverse: la prima disegna una
        // barra ferma a sinistra, la seconda un'attesa indeterminata.
        assert_eq!(
            analizza("AETHER_P:1000/NA"),
            Some(Evento::Avanzamento {
                scaricati: 1000,
                totale: 0,
                frazione: None
            })
        );
    }

    #[test]
    fn la_frazione_non_supera_uno() {
        // Il totale di yt-dlp è una stima: `total_bytes_estimate` può stare
        // sotto ai byte davvero scaricati.
        let Some(Evento::Avanzamento { frazione, .. }) = analizza("AETHER_P:2000/1000") else {
            panic!("doveva essere un avanzamento");
        };
        assert_eq!(frazione, Some(1.0));
    }

    #[test]
    fn la_sentinella_di_fine_porta_il_percorso_intero() {
        assert_eq!(
            analizza("AETHER_D:C:\\Musica\\Artista\\Album\\01 - Titolo.m4a"),
            Some(Evento::FileFinito {
                percorso: "C:\\Musica\\Artista\\Album\\01 - Titolo.m4a".to_owned()
            })
        );
        // Una sentinella vuota non è un file: prenderla per buona vorrebbe dire
        // dichiarare riuscito uno scaricamento e poi taggare un percorso vuoto.
        assert_eq!(analizza("AETHER_D:"), None);
        assert_eq!(analizza("AETHER_D:   "), None);
    }

    #[test]
    fn linizio_di_un_file_porta_indice_e_titolo() {
        assert_eq!(
            analizza("AETHER_F:2/10:Un titolo: con i due punti"),
            Some(Evento::FileIniziato {
                indice: 2,
                totale: 10,
                titolo: "Un titolo: con i due punti".to_owned()
            })
        );
        // I due punti nel titolo non devono troncarlo: la divisione è sulla
        // prima ricorrenza dopo il totale, non su tutte.
        assert_eq!(
            analizza("AETHER_F:NA/NA:Solo"),
            Some(Evento::FileIniziato {
                indice: 1,
                totale: 1,
                titolo: "Solo".to_owned()
            })
        );
    }

    #[test]
    fn una_sentinella_storta_non_e_un_evento() {
        // Meglio nessun avanzamento che un avanzamento inventato.
        assert_eq!(analizza("AETHER_P:abc/def"), None);
        assert_eq!(analizza("AETHER_P:512"), None);
        assert_eq!(analizza("AETHER_F:1"), None);
    }

    #[test]
    fn le_forme_vecchie_restano_un_ripiego() {
        let Some(Evento::PercentualeVecchia { frazione }) =
            analizza("[download]  42.3% of 4.21MiB at 1.00MiB/s")
        else {
            panic!("doveva essere una percentuale vecchia");
        };
        // Confronto a tolleranza e non `==`: 42.3/100 in f32 non è il letterale
        // 0.423, e un test che pretendesse l'uguaglianza esatta fallirebbe per
        // una ragione che non ha niente a che vedere con l'analisi della riga.
        assert!((frazione - 0.423).abs() < 1e-6);

        assert_eq!(
            analizza("[download] Downloading item 3 of 12"),
            Some(Evento::Elemento {
                indice: 3,
                totale: 12
            })
        );
        assert_eq!(
            analizza("[download] Destination: C:\\Temp\\brano.f140.m4a"),
            Some(Evento::Destinazione {
                file: "brano.f140.m4a".to_owned()
            })
        );
    }

    #[test]
    fn le_righe_qualunque_non_dicono_niente() {
        assert_eq!(analizza(""), None);
        assert_eq!(analizza("   "), None);
        assert_eq!(
            analizza("[youtube] Extracting URL: https://y/watch?v=1"),
            None
        );
        assert_eq!(analizza("WARNING: qualcosa"), None);
    }
}
