//! Quanto forte suona un brano: l'intensità percepita secondo EBU R128, e il
//! picco vero.
//!
//! # Perché misurarla, quando i tag ReplayGain esistono già
//!
//! Perché nella libreria misurata di questo progetto **nessun file li ha**. La
//! normalizzazione della 2.x leggeva il tag e, senza tag, non correggeva niente:
//! accesa o spenta suonava uguale, e un disco rimasterizzato nel 2010 arrivava
//! dieci decibel sopra uno del 1975 nella stessa coda. Il tag resta la prima
//! scelta quando c'è — l'ha scritto qualcuno con un programma apposta, spesso
//! col modo album — e questa misura è quel che si usa quando non c'è.
//!
//! # Perché l'intero file, e non trenta secondi come l'impronta
//!
//! Perché l'intensità integrata è una media su **tutto** il brano con due
//! soglie che buttano i silenzi e i passaggi molto più piani del resto: trenta
//! secondi presi dal centro di una canzone che comincia piano e finisce forte
//! darebbero un numero sbagliato di qualche decibel, cioè esattamente la
//! differenza che la normalizzazione deve togliere. Per lo stesso motivo il
//! picco va cercato ovunque: quello di un brano sta spesso nell'ultimo
//! ritornello.
//!
//! Costa una decodifica completa — qualche decimo di secondo per un FLAC locale
//! su un processore di oggi — ed è la ragione per cui la passata che la fa si
//! ferma quando la musica suona, come quella delle impronte. Vedi `analisi.rs`
//! nell'applicazione.
//!
//! # Cosa si misura, esattamente
//!
//! Il segnale **che si sentirà**: decodificato, portato a 48 kHz e alla
//! disposizione stereo con la stessa catena del motore. Un 5.1 si misura dopo
//! il passaggio a due canali, un mono dopo essere stato messo su tutti e due —
//! ed è giusto così, perché la correzione si applica a quello che esce dalle
//! casse, non al file. Il picco vero è quello di BS.1770: il segnale
//! sovracampionato quattro volte, che trova i picchi che cadono **fra** due
//! campioni e che una conversione analogica produce davvero.

use ebur128::{EbuR128, Mode};
use rymembra_domain::errors::{AppError, ErrorCode};

use crate::decodifica::{Decodificatore, Sorgente};

/// La frequenza a cui si misura.
///
/// Quella dell'uscita più comune, e quella a cui il picco vero di BS.1770 si
/// calcola sovracampionando quattro volte: sopra i 96 kHz la libreria non
/// sovracampiona più, e un file a 192 kHz misurato così com'è darebbe un picco
/// meno severo di uno a 44,1. Misurando tutti alla stessa frequenza i numeri
/// sono confrontabili fra loro.
pub const FREQUENZA: u32 = 48_000;

/// La versione della misura, come la ricorda il database.
///
/// Si alza quando cambia qualcosa che sposta i numeri — la frequenza, la
/// disposizione dei canali, la libreria — e allora la passata rimisura tutto.
pub const VERSIONE: u32 = 1;

/// Il riferimento a cui i guadagni ReplayGain sono espressi, in LUFS.
///
/// È quello di ReplayGain 2.0: un brano misurato a −18 LUFS ha guadagno zero.
/// Da qui [`guadagno_db`] ricava, da una misura, lo stesso numero che un tag
/// avrebbe scritto.
pub const RIFERIMENTO_LUFS: f64 = -18.0;

/// Quel che si è misurato di un brano.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Intensita {
    /// L'intensità integrata, in LUFS.
    pub lufs: f64,
    /// Il picco vero, lineare: `1.0` è il fondo scala, e può superarlo.
    pub picco: f64,
    /// Quanto audio si è misurato, in millisecondi.
    ///
    /// Serve a mettere insieme un album: la media dei brani pesa di più quelli
    /// più lunghi, come farebbe una misura fatta sull'album intero. Vedi
    /// [`dell_album`].
    pub durata_ms: u64,
}

/// Com'è andata la misura.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Esito {
    /// I numeri.
    Misurata(Intensita),
    /// Il brano non ha niente sopra la soglia assoluta di −70 LUFS: silenzio,
    /// o meno di quattrocento millisecondi di audio. Terminale: una traccia
    /// fantasma resta tale, e un guadagno calcolato su di lei la porterebbe a
    /// +12 dB di fruscio.
    Muto,
}

/// Il guadagno che porta un brano misurato a [`RIFERIMENTO_LUFS`], in dB.
///
/// Lo stesso numero che un programma di ReplayGain 2.0 scriverebbe nel tag, e
/// il motore lo tratta allo stesso modo: il bersaglio scelto da chi ascolta lo
/// sposta, e i tagli di [`crate::correzione`] lo tengono in riga.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "sono decibel, e il motore li vuole in f32: la precisione di un f32 è \
              un milionesimo di decibel"
)]
pub fn guadagno_db(lufs: f64) -> f32 {
    (RIFERIMENTO_LUFS - lufs) as f32
}

/// Misura un brano, dall'inizio alla fine.
///
/// # Errori
///
/// Quelli del decodificatore, **inalterati**, come per
/// [`crate::impronta::calcola`] e per la stessa ragione: chi tiene il registro
/// deve poter distinguere un file rotto — che si riprova fra un mese — da una
/// cartella di rete caduta, che non è colpa del brano. Un guasto della libreria
/// di misura, che su dati validi non capita, diventa `playback.decodeFailed`.
pub fn misura(sorgente: Sorgente) -> Result<Esito, AppError> {
    let track_id = sorgente.track_id;
    let mut decodificatore = Decodificatore::apri(sorgente, FREQUENZA, 2)?;
    let guasto = |cosa: &str, err: ebur128::Error| {
        AppError::new(ErrorCode::PlaybackDecodeFailed {
            track_id: Some(track_id),
            format: None,
        })
        .with_cause(format!("misura dell'intensità, {cosa}: {err}"))
    };
    let mut misuratore = EbuR128::new(2, FREQUENZA, Mode::I | Mode::TRUE_PEAK)
        .map_err(|err| guasto("preparazione", err))?;

    let mut blocco: Vec<f32> = Vec::with_capacity(8192);
    let mut campioni: u64 = 0;
    while decodificatore.prossimo(&mut blocco)? {
        misuratore
            .add_frames_f32(&blocco)
            .map_err(|err| guasto("un blocco", err))?;
        campioni = campioni.saturating_add(u64::try_from(blocco.len()).unwrap_or(u64::MAX));
    }

    let lufs = misuratore
        .loudness_global()
        .map_err(|err| guasto("intensità integrata", err))?;
    if !lufs.is_finite() {
        return Ok(Esito::Muto);
    }
    let mut picco = 0.0_f64;
    for canale in 0..2 {
        let suo = misuratore
            .true_peak(canale)
            .map_err(|err| guasto("picco vero", err))?;
        picco = picco.max(suo);
    }
    // Due canali per fotogramma, e mille millisecondi per FREQUENZA fotogrammi.
    #[expect(
        clippy::integer_division,
        reason = "millisecondi interi: quel che si perde è meno di un millisecondo"
    )]
    let durata_ms = campioni.saturating_mul(1000) / (2 * u64::from(FREQUENZA));
    Ok(Esito::Misurata(Intensita {
        lufs,
        picco,
        durata_ms,
    }))
}

/// L'intensità di un album, dalle misure dei suoi brani.
///
/// # Un'approssimazione, dichiarata
///
/// La misura vera di un album è quella fatta sull'album **suonato di fila**:
/// le soglie di BS.1770 si applicano a tutti i blocchi insieme. Qui ci sono solo
/// i numeri di ogni brano — tenere i blocchi di ogni file vorrebbe dire
/// migliaia di valori per riga — e allora si fa la media delle **energie**,
/// pesata sulla durata. Coincide con la misura vera quando i brani non hanno
/// passaggi sotto la soglia relativa, e se ne scosta di qualche decimo di
/// decibel quando ne hanno: è la stessa strada di chi calcola ReplayGain
/// album a partire da misure già fatte, ed è largamente sotto quel che
/// l'orecchio distingue fra due dischi.
///
/// Il picco dell'album è il più alto dei suoi brani, senza approssimazioni.
///
/// `None` se non c'è niente da mettere insieme.
#[must_use]
pub fn dell_album(brani: &[Intensita]) -> Option<Intensita> {
    let mut energia = 0.0_f64;
    let mut peso = 0.0_f64;
    let mut picco = 0.0_f64;
    let mut durata_ms = 0_u64;
    for brano in brani {
        if !brano.lufs.is_finite() {
            continue;
        }
        // Un brano senza durata conta come un secondo: c'è, e la sua misura
        // non va buttata per un campo mancante.
        let quanto = brano.durata_ms.max(1_000) as f64;
        energia += quanto * 10.0_f64.powf(brano.lufs / 10.0);
        peso += quanto;
        picco = picco.max(brano.picco);
        durata_ms = durata_ms.saturating_add(brano.durata_ms);
    }
    if peso <= 0.0 || energia <= 0.0 {
        return None;
    }
    Some(Intensita {
        lufs: 10.0 * (energia / peso).log10(),
        picco,
        durata_ms,
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Byte in memoria che si comportano come un file.
    struct Byte(std::io::Cursor<Vec<u8>>);

    impl std::io::Read for Byte {
        fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.0, dove)
        }
    }

    impl std::io::Seek for Byte {
        fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
            std::io::Seek::seek(&mut self.0, da)
        }
    }

    impl crate::decodifica::Flusso for Byte {
        fn lunghezza(&self) -> Option<u64> {
            u64::try_from(self.0.get_ref().len()).ok()
        }
    }

    /// Un WAV stereo a 16 bit con un seno di ampiezza data, lungo quanto si
    /// vuole: la misura di BS.1770 su un seno a 1 kHz è nota a mano.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "i campioni di prova stanno fra -1 e 1, e la durata è positiva: per \
                  costruzione i prodotti entrano in un i16 e in un usize"
    )]
    fn seno(frequenza: u32, ampiezza: f32, secondi: f32) -> Sorgente {
        let quanti = (frequenza as f32 * secondi) as usize;
        let dati = u32::try_from(quanti * 4).unwrap_or(0);
        let mut byte = Vec::with_capacity(44 + quanti * 4);
        byte.extend_from_slice(b"RIFF");
        byte.extend_from_slice(&(36 + dati).to_le_bytes());
        byte.extend_from_slice(b"WAVEfmt ");
        byte.extend_from_slice(&16u32.to_le_bytes());
        byte.extend_from_slice(&1u16.to_le_bytes());
        byte.extend_from_slice(&2u16.to_le_bytes());
        byte.extend_from_slice(&frequenza.to_le_bytes());
        byte.extend_from_slice(&(frequenza * 4).to_le_bytes());
        byte.extend_from_slice(&4u16.to_le_bytes());
        byte.extend_from_slice(&16u16.to_le_bytes());
        byte.extend_from_slice(b"data");
        byte.extend_from_slice(&dati.to_le_bytes());
        for i in 0..quanti {
            let t = i as f32 / frequenza as f32;
            let v =
                ((ampiezza * (2.0 * std::f32::consts::PI * 1000.0 * t).sin()) * 32_767.0) as i16;
            byte.extend_from_slice(&v.to_le_bytes());
            byte.extend_from_slice(&v.to_le_bytes());
        }
        Sorgente {
            track_id: 1,
            media: Box::new(Byte(std::io::Cursor::new(byte))),
            estensione: Some("wav".to_owned()),
            durata_ms: 0,
            replaygain_db: None,
            picco: None,
            segmento: None,
        }
    }

    fn misurata(sorgente: Sorgente) -> Intensita {
        match misura(sorgente).expect("la misura riesce") {
            Esito::Misurata(intensita) => intensita,
            Esito::Muto => panic!("il seno è stato preso per silenzio"),
        }
    }

    #[test]
    fn un_seno_a_meno_venti_si_misura_come_dice_la_norma() {
        // È la prova di EBU Tech 3341: un seno a 1 kHz con il picco a −20 dBFS
        // su tutti e due i canali misura −20 LUFS. I conti tornano da soli —
        // il filtro K a 1 kHz guadagna 0,691 dB e la formula li toglie, la
        // potenza media di un seno è metà del suo picco al quadrato (−3,01 dB)
        // e la somma dei due canali la raddoppia (+3,01 dB).
        let ampiezza = 10.0_f32.powf(-20.0 / 20.0);
        let letta = misurata(seno(48_000, ampiezza, 5.0));
        let attesa = -20.0;
        assert!(
            (letta.lufs - attesa).abs() < 0.1,
            "misurati {} LUFS, attesi {attesa}",
            letta.lufs
        );
        // Il picco vero di un seno a 1 kHz è la sua ampiezza.
        assert!(
            (letta.picco - f64::from(ampiezza)).abs() < 0.01,
            "picco {}",
            letta.picco
        );
        assert!(
            (4_900..=5_100).contains(&letta.durata_ms),
            "durata {}",
            letta.durata_ms
        );
    }

    #[test]
    fn la_frequenza_del_file_non_sposta_la_misura() {
        // Lo stesso seno a 44,1 e a 48 kHz: il ricampionatore non deve
        // cambiare il numero, altrimenti due edizioni dello stesso disco
        // riceverebbero due correzioni diverse.
        let a_44 = misurata(seno(44_100, 0.25, 4.0));
        let a_48 = misurata(seno(48_000, 0.25, 4.0));
        assert!(
            (a_44.lufs - a_48.lufs).abs() < 0.05,
            "{} contro {}",
            a_44.lufs,
            a_48.lufs
        );
    }

    #[test]
    fn una_traccia_d_immagine_si_misura_da_sola() {
        // Un'immagine fatta di silenzio e poi di un seno forte: la traccia che
        // sta tutta nel silenzio deve risultare muta, e quella del seno deve
        // durare quanto il suo pezzo e non quanto il disco. Se la misura
        // guardasse il file intero, la prima avrebbe un'intensità e la seconda
        // durerebbe sei secondi.
        let mut silenzio = seno(48_000, 0.0, 2.0);
        let mut forte = seno(48_000, 0.5, 4.0);
        // Il WAV del silenzio, con i quattro secondi del seno in coda.
        let mut byte = Vec::new();
        std::io::Read::read_to_end(&mut silenzio.media, &mut byte).expect("si legge");
        let mut seno_forte = Vec::new();
        std::io::Read::read_to_end(&mut forte.media, &mut seno_forte).expect("si legge");
        let dati = seno_forte.get(44..).expect("c'è l'intestazione");
        byte.extend_from_slice(dati);
        let lunghezza = u32::try_from(byte.len() - 44).expect("sta");
        byte[40..44].copy_from_slice(&lunghezza.to_le_bytes());
        byte[4..8].copy_from_slice(&(lunghezza + 36).to_le_bytes());

        let pezzo = |inizio_cd: u32, fine_cd: Option<u32>| Sorgente {
            media: Box::new(Byte(std::io::Cursor::new(byte.clone()))),
            segmento: Some(crate::Segmento {
                immagine: "disco.wav".to_owned(),
                inizio_cd,
                fine_cd,
            }),
            ..seno(48_000, 0.0, 0.0)
        };
        assert_eq!(
            misura(pezzo(0, Some(150))).expect("si legge"),
            Esito::Muto,
            "la prima traccia è silenzio"
        );
        let seconda = misurata(pezzo(150, None));
        assert!(
            (3_990..=4_010).contains(&seconda.durata_ms),
            "durata {}",
            seconda.durata_ms
        );
    }

    #[test]
    fn il_silenzio_non_ha_un_intensita() {
        assert_eq!(
            misura(seno(48_000, 0.0, 3.0)).expect("si legge"),
            Esito::Muto
        );
    }

    #[test]
    fn dal_lufs_al_guadagno_come_replaygain() {
        assert!((guadagno_db(-18.0)).abs() < f32::EPSILON);
        assert!((guadagno_db(-9.0) + 9.0).abs() < 1e-5);
        assert!((guadagno_db(-25.5) - 7.5).abs() < 1e-5);
    }

    #[test]
    fn un_album_di_brani_uguali_misura_come_un_brano() {
        let brano = Intensita {
            lufs: -12.0,
            picco: 0.9,
            durata_ms: 200_000,
        };
        let album = dell_album(&[brano, brano, brano]).expect("c'è");
        assert!((album.lufs + 12.0).abs() < 1e-9);
        assert!((album.picco - 0.9).abs() < f64::EPSILON);
        assert_eq!(album.durata_ms, 600_000);
    }

    #[test]
    fn nell_album_conta_l_energia_e_non_la_media_dei_numeri() {
        // Due brani uguali per durata, a −10 e −20 LUFS: la media dei numeri
        // sarebbe −15, quella delle energie è −12,6 — il brano forte pesa di
        // più, perché è quello che si sente.
        let forte = Intensita {
            lufs: -10.0,
            picco: 1.1,
            durata_ms: 180_000,
        };
        let piano = Intensita {
            lufs: -20.0,
            picco: 0.3,
            durata_ms: 180_000,
        };
        let album = dell_album(&[forte, piano]).expect("c'è");
        assert!((album.lufs + 12.596).abs() < 0.01, "album a {}", album.lufs);
        assert!((album.picco - 1.1).abs() < f64::EPSILON);
    }

    #[test]
    fn un_album_vuoto_non_ha_un_intensita() {
        assert_eq!(dell_album(&[]), None);
        let assurdo = Intensita {
            lufs: f64::NEG_INFINITY,
            picco: 0.0,
            durata_ms: 1_000,
        };
        assert_eq!(dell_album(&[assurdo]), None);
    }
}
