//! Il motore audio di Aether.
//!
//! # Perché è un crate del nucleo e non codice dentro l'applicazione
//!
//! Nel vecchio albero la riproduzione era scritta due volte: Howler nel webview
//! sul desktop, ExoPlayer in Kotlin su Android. Non erano due
//! *implementazioni* della stessa decisione — erano due decisioni diverse che si
//! somigliavano, e divergevano ogni volta che una delle due veniva toccata. Il
//! `CHANGELOG.md` ne conserva un esemplare: il codice d'errore di ExoPlayer
//! veniva scartato per strada, e sul desktop Howler mostrava «2» in interfaccia.
//!
//! Qui il motore è uno. Su Android sarà lo stesso crate, dietro UniFFI, con lo
//! stesso `cpal` che là parla ad AAudio invece che a WASAPI.
//!
//! # Cosa questo crate non sa
//!
//! Non sa cos'è una libreria musicale, non apre un database, non conosce i
//! percorsi. Riceve una [`Sorgente`] — dei byte, un'estensione, una durata — e
//! li fa uscire dalle casse. Chi decide *quale* brano è la coda in
//! `aether_domain::queue`; chi trova i byte è `aether-app`. Questa divisione è
//! ciò che tiene aperta la strada per Android, dove i byte non arrivano da un
//! percorso ma da un flusso concesso dal sistema.
//!
//! # La forma, in tre fili
//!
//! 1. **Chi comanda** — chiunque, attraverso [`Motore`], che manda messaggi.
//! 2. **Chi decodifica** — un filo che possiede symphonia e riempie un anello.
//! 3. **Chi suona** — la callback di cpal, che svuota l'anello e basta.
//!
//! Fra il secondo e il terzo ci sono due anelli senza lucchetti, e la ragione è
//! in [`uscita`]: la callback ha qualche millisecondo per consegnare il blocco,
//! e non può permettersi di aspettare nessuno. Nel primo passano i campioni; nel
//! secondo le curve dell'[`equalizzatore`], che sono cinquanta numeri in virgola
//! mobile e vanno consegnati tutti insieme o per niente.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};

use aether_domain::errors::AppError;

pub mod decodifica;
pub mod equalizzatore;
pub mod motore;
pub mod spettro;
pub mod uscita;

pub use decodifica::{Flusso, Sorgente};
pub use equalizzatore::{BANDE, CENTRI_HZ, LIMITE_DB, PRESET_DI_SERIE};
pub use motore::{Evento, Motore, Posizione};
pub use spettro::Spettro;
pub use uscita::FormatoUscita;

/// Cosa sappiamo suonare.
///
/// L'elenco chiuso di `aether_domain::paths::SUPPORTED_EXTENSIONS` meno i due
/// che symphonia non decodifica. Averlo qui come tipo, invece che come confronto
/// fra stringhe sparso per l'albero, serve a una cosa sola: quando arriverà un
/// decodificatore per Opus, il compilatore indicherà i posti da toccare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Un formato che il motore sa aprire.
    Supportato,
    /// Un formato che riconosciamo e non sappiamo suonare.
    ///
    /// Opus e WMA. Non è una svista: symphonia 0.5 non ha un decodificatore né
    /// per l'uno né per l'altro, e fingere di provarci darebbe un errore di
    /// decodifica generico invece di una risposta. Sulla libreria misurata di
    /// questo progetto — 1421 brani, tutti MP3 — non ce n'è nessuno dei due.
    NonSupportato,
    /// Non è nemmeno un'estensione che consideriamo musica.
    Sconosciuto,
}

impl Codec {
    /// Cosa possiamo fare di un file con questa estensione.
    #[must_use]
    pub fn da_estensione(estensione: &str) -> Self {
        let pulita = estensione.trim_start_matches('.').to_ascii_lowercase();
        match pulita.as_str() {
            "mp3" | "flac" | "m4a" | "aac" | "ogg" | "wav" | "aiff" | "aif" => Self::Supportato,
            "opus" | "wma" => Self::NonSupportato,
            _ => Self::Sconosciuto,
        }
    }
}

/// Quel che i tre fili si dicono senza parlarsi.
///
/// Tutto atomico, perché uno dei lettori è la callback audio — che non può
/// prendere lucchetti — e un altro è il filo che decodifica, che non deve
/// aspettarla.
pub(crate) struct Condiviso {
    /// In pausa: la callback scrive silenzio senza consumare l'anello.
    pub in_pausa: AtomicBool,
    /// Il filo che decodifica chiede alla callback di scartare tutto.
    ///
    /// Lo alza solo il decodificatore, lo abbassa solo la callback: è una
    /// stretta di mano a due, e per questo non serve altro che un booleano.
    pub svuota: AtomicBool,
    /// Il guadagno voluto, nei bit di un `f32`.
    pub guadagno: AtomicU32,
    /// I fotogrammi davvero usciti dalle casse.
    pub fotogrammi: AtomicU64,
    /// I campioni serviti a vuoto perché l'anello era secco.
    pub vuoti: AtomicU64,
    /// Quanti canali ha l'uscita, per contare i fotogrammi.
    pub canali: AtomicU32,
    /// Il dispositivo è sparito.
    pub perso: AtomicBool,
    /// Perché è sparito.
    pub causa_perdita: AtomicU32,
    /// Qualcuno sta guardando lo spettro.
    ///
    /// Spento, la callback non scrive niente nel terzo anello: chi non guarda
    /// non paga. È un booleano e non un conteggio perché la schermata che lo
    /// mostra è una sola.
    pub spettro: AtomicBool,
}

impl Condiviso {
    pub(crate) fn nuovo() -> Arc<Self> {
        Arc::new(Self {
            // Si comincia in pausa: il flusso si apre subito, ma finché non c'è
            // un brano non deve consumare niente.
            in_pausa: AtomicBool::new(true),
            svuota: AtomicBool::new(false),
            guadagno: AtomicU32::new(1.0f32.to_bits()),
            fotogrammi: AtomicU64::new(0),
            vuoti: AtomicU64::new(0),
            canali: AtomicU32::new(2),
            perso: AtomicBool::new(false),
            causa_perdita: AtomicU32::new(0),
            spettro: AtomicBool::new(false),
        })
    }
}

/// Il guadagno da applicare, messi insieme volume, silenziamento e ReplayGain.
///
/// # ReplayGain
///
/// Il tag dichiara di quanti decibel il brano va corretto per stare a un
/// riferimento di −18 LUFS. `bersaglio_db` sposta quel riferimento: a −18 il tag
/// si usa com'è, a −14 si aggiungono quattro decibel. La correzione si taglia
/// fra −24 e +12 dB perché un tag sbagliato — e ce ne sono — non deve poter
/// mandare in saturazione l'uscita né azzerarla.
///
/// Sulla libreria misurata di questo progetto **nessun file ha il tag**: questo
/// stadio esiste perché il volume ne ha bisogno comunque, e perché il giorno in
/// cui i tag ci saranno non si dovrà toccare la catena.
#[must_use]
pub fn guadagno(
    volume: f32,
    muto: bool,
    replaygain_db: Option<f32>,
    replaygain_attivo: bool,
    bersaglio_db: f32,
) -> f32 {
    if muto {
        return 0.0;
    }
    let volume = volume.clamp(0.0, 1.0);
    let correzione = match (replaygain_attivo, replaygain_db) {
        (true, Some(db)) => {
            let spostato = db + (bersaglio_db + 18.0);
            10.0f32.powf(spostato.clamp(-24.0, 12.0) / 20.0)
        }
        _ => 1.0,
    };
    volume * correzione
}

/// Apre il dispositivo e avvia i fili. È il punto d'ingresso del crate.
///
/// L'osservatore viene chiamato da un filo del motore, mai da quello della
/// callback audio: può prendere lucchetti, scrivere su disco e mandare eventi
/// alla finestra senza rischiare di interrompere il suono.
pub fn avvia(osservatore: impl Fn(Evento) + Send + 'static) -> Result<Motore, AppError> {
    Motore::avvia(osservatore)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_formati_che_sappiamo_e_quelli_che_no() {
        assert_eq!(Codec::da_estensione("mp3"), Codec::Supportato);
        assert_eq!(Codec::da_estensione(".FLAC"), Codec::Supportato);
        assert_eq!(Codec::da_estensione("opus"), Codec::NonSupportato);
        assert_eq!(Codec::da_estensione("wma"), Codec::NonSupportato);
        assert_eq!(Codec::da_estensione("txt"), Codec::Sconosciuto);
    }

    #[test]
    fn tutte_le_estensioni_del_dominio_hanno_una_risposta() {
        // Nessuna estensione che il dominio considera musica deve risultare
        // «sconosciuta»: o la sappiamo suonare, o lo diciamo.
        for ext in aether_domain::paths::SUPPORTED_EXTENSIONS {
            assert_ne!(
                Codec::da_estensione(ext),
                Codec::Sconosciuto,
                "estensione {ext} senza risposta"
            );
        }
    }

    #[test]
    fn il_silenziamento_vince_su_tutto() {
        assert_eq!(guadagno(1.0, true, Some(6.0), true, -18.0), 0.0);
    }

    #[test]
    fn senza_replaygain_il_guadagno_e_il_volume() {
        assert!((guadagno(0.5, false, None, true, -18.0) - 0.5).abs() < f32::EPSILON);
        // Il tag c'è ma la correzione è spenta.
        assert!((guadagno(0.5, false, Some(-6.0), false, -18.0) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn un_brano_troppo_forte_viene_abbassato() {
        let g = guadagno(1.0, false, Some(-6.0), true, -18.0);
        assert!(g < 1.0 && g > 0.4, "guadagno {g}");
    }

    #[test]
    fn il_bersaglio_sposta_il_riferimento() {
        let a_18 = guadagno(1.0, false, Some(0.0), true, -18.0);
        let a_14 = guadagno(1.0, false, Some(0.0), true, -14.0);
        assert!(a_14 > a_18, "a −14 deve suonare più forte");
    }

    #[test]
    fn un_tag_assurdo_non_manda_in_saturazione() {
        let g = guadagno(1.0, false, Some(90.0), true, -18.0);
        // +12 dB è il tetto: circa 4x, non trentamila.
        assert!(g < 4.1, "guadagno {g}");
    }

    #[test]
    fn il_volume_si_taglia_fra_zero_e_uno() {
        assert!((guadagno(5.0, false, None, false, -18.0) - 1.0).abs() < f32::EPSILON);
        assert_eq!(guadagno(-1.0, false, None, false, -18.0), 0.0);
    }
}
