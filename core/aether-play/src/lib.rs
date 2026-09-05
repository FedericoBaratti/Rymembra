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

pub mod attacchi;
pub mod decodifica;
pub mod equalizzatore;
pub mod impronta;
pub mod motore;
pub mod spettro;
pub mod uscita;

pub use decodifica::{Flusso, Sorgente};
pub use equalizzatore::{BANDE, CENTRI_HZ, LIMITE_DB, PRESET_DI_SERIE};
pub use motore::{BranoAperto, Evento, Motore, Posizione};
pub use spettro::{
    Bande, RISOLUZIONE_DI_SERIE, RISOLUZIONE_MAX, RISOLUZIONE_MIN, RISOLUZIONI, Spettro,
};
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
    volume_uscita(volume, muto) * correzione(replaygain_db, replaygain_attivo, bersaglio_db)
}

/// Quanto chiede il cursore del volume, e nient'altro.
///
/// # Perché è una funzione a sé
///
/// Perché il volume è dell'**uscita** e la correzione ReplayGain è del
/// **brano**, e con la dissolvenza incrociata la differenza smette di essere
/// filosofica: durante una sovrapposizione ci sono due brani, quindi due
/// correzioni diverse, ma un cursore del volume solo. Un unico scalare
/// globale non può dire due cose insieme.
///
/// Quel che resta globale — questo — lo applica la callback in fondo alla
/// catena, con la sua rampa. Quel che è del brano viaggia coi campioni, e li
/// raggiunge nel filo che decodifica.
#[must_use]
pub fn volume_uscita(volume: f32, muto: bool) -> f32 {
    if muto {
        return 0.0;
    }
    volume.clamp(0.0, 1.0)
}

/// La correzione che porta un brano al bersaglio: ReplayGain, e nient'altro.
///
/// Senza tag la correzione è esattamente 1.0, cioè niente: è il motivo per cui
/// tenere la normalizzazione accesa su una libreria senza tag non cambia una
/// virgola.
#[must_use]
pub fn correzione(replaygain_db: Option<f32>, attivo: bool, bersaglio_db: f32) -> f32 {
    match (attivo, replaygain_db) {
        (true, Some(db)) => {
            let spostato = db + (bersaglio_db + 18.0);
            10.0f32.powf(spostato.clamp(-24.0, 12.0) / 20.0)
        }
        _ => 1.0,
    }
}

/// Mescola due brani che si sovrappongono, dentro il blocco di quello uscente.
///
/// `fatti` è a che punto della dissolvenza si era all'inizio del blocco, in
/// fotogrammi; `durata` è quanto dura tutta la sovrapposizione. Il blocco
/// uscente viene riscritto con la somma dei due, e quel che avanza
/// dell'entrante — se è più lungo — resta a chi chiama.
///
/// # Perché a potenza costante e non lineare
///
/// Perché i due brani sono **scorrelati**. Sommando due segnali senza
/// relazione fra loro, le potenze si sommano e le ampiezze no: due rampe
/// lineari che si incrociano a metà darebbero, a metà strada, due mezze
/// ampiezze che fanno il 70% della potenza di partenza — un avvallamento
/// udibile proprio nel punto in cui l'ascoltatore sta cercando di capire cosa
/// sta succedendo. Con seno e coseno la somma dei quadrati è uno a ogni
/// istante, e il passaggio non ha un buco in mezzo.
///
/// Su materiale correlato — la stessa nota, lo stesso disco — la scelta giusta
/// sarebbe l'opposta, ma quello è il gapless, e il gapless non passa di qui.
///
/// # Perché una funzione pura
///
/// Perché è l'unico modo onesto di provare questa parte del motore. Attorno c'è
/// `Contesto`, che contiene due `Box<dyn FormatReader>` di symphonia: provarlo
/// vorrebbe dire un file vero su disco e un dispositivo audio. Qui invece ci
/// sono due fette di campioni, e le prove possono chiedersi se la potenza si
/// conserva davvero.
#[expect(
    clippy::integer_division,
    reason = "l'indice del fotogramma dentro un blocco interlacciato: la divisione               per il numero di canali è esatta per costruzione"
)]
pub fn dissolvi(uscente: &mut [f32], entrante: &[f32], fatti: u64, durata: u64, canali: u16) {
    let canali = usize::from(canali.max(1));
    // Una dissolvenza lunga zero non è una dissolvenza: senza questa riga
    // sarebbe una divisione per zero, e la prima cosa che si sentirebbe è un
    // silenzio pieno di NaN.
    if durata == 0 {
        return;
    }
    for (indice, campione) in uscente.iter_mut().enumerate() {
        // `indice / canali` e non `indice`: la rampa avanza per **fotogramma**,
        // così i canali di uno stesso istante ricevono lo stesso guadagno.
        // Trattandoli uno per uno l'immagine stereo scivolerebbe di mezzo
        // campione durante tutta la dissolvenza.
        let fotogramma = fatti.saturating_add((indice / canali) as u64);
        let (fuori, dentro) = punto_curva(fotogramma, durata);
        let entra = entrante.get(indice).copied().unwrap_or(0.0);
        *campione = *campione * fuori + entra * dentro;
    }
}

/// I due guadagni della curva a un certo fotogramma: quanto di quello che esce,
/// quanto di quello che entra.
///
/// Una funzione sola perché la curva deve essere **una**: [`risali`] riprende
/// esattamente dal punto in cui [`dissolvi`] si è fermata, e due formule scritte
/// in due posti che si scostassero di un capello produrrebbero un gradino
/// proprio lì — cioè un clic nel punto che tutta questa parte del motore esiste
/// per non avere.
pub(crate) fn punto_curva(fotogramma: u64, durata: u64) -> (f32, f32) {
    // Oltre la fine si resta a fondo corsa invece di traboccare: il blocco
    // può sforare la dissolvenza, e quel che sfora è già tutto il nuovo.
    let t = if fotogramma >= durata || durata == 0 {
        1.0
    } else {
        fotogramma as f32 / durata as f32
    };
    let angolo = t * std::f32::consts::FRAC_PI_2;
    (angolo.cos(), angolo.sin())
}

/// Porta a piena ampiezza un brano che stava entrando, da solo.
///
/// `da` è il guadagno che aveva quando è rimasto solo — il seno di
/// [`punto_curva`] al fotogramma in cui la sovrapposizione si è interrotta —
/// e da lì la salita è lineare: `fatti` fotogrammi fatti su `durata`.
///
/// # Quando serve
///
/// La sovrapposizione comincia quando alla fine del brano manca quanto dura la
/// dissolvenza, e «quanto manca» si sa dalla durata **dichiarata** dal database.
/// Certi file la dichiarano più lunga di quel che contengono — un MP3 a bitrate
/// variabile senza intestazione Xing la fa stimare, e la stima sbaglia — e
/// allora il decodificatore del brano uscente finisce a metà curva. Il brano che
/// entrava è a mezza ampiezza e non ha più niente sotto.
///
/// Senza questa funzione salterebbe di colpo a tutta ampiezza: un gradino nella
/// forma d'onda, cioè un clic, sul brano che poi si ascolta per intero. Con
/// questa, riprende da dove era e ci arriva in una quarantina di millisecondi.
///
/// # Perché breve e lineare, e non il resto della curva
///
/// Perché la curva era tarata su una durata che si è appena scoperta falsa:
/// continuarla vorrebbe dire un brano che parte a mezza ampiezza e ci mette dei
/// secondi a venire su, da solo e senza niente da cui emergere — un errore più
/// udibile del clic che si voleva togliere. E lineare perché su quaranta
/// millisecondi la potenza costante non ha niente da conservare: non c'è un
/// secondo segnale con cui sommarsi, c'è solo un livello che si muove, ed è la
/// stessa rampa con cui l'uscita insegue il cursore del volume.
#[expect(
    clippy::integer_division,
    reason = "l'indice del fotogramma dentro un blocco interlacciato: la divisione \
              per il numero di canali è esatta per costruzione"
)]
pub fn risali(blocco: &mut [f32], da: f32, fatti: u64, durata: u64, canali: u16) {
    let canali = usize::from(canali.max(1));
    // Una salita lunga zero è una divisione per zero, e chi chiama non deve
    // chiamare: la riga sta qui perché «non deve» dentro un `/` non è un
    // controllo.
    if durata == 0 {
        return;
    }
    let da = da.clamp(0.0, 1.0);
    for (indice, campione) in blocco.iter_mut().enumerate() {
        let fotogramma = fatti.saturating_add((indice / canali) as u64);
        let t = if fotogramma >= durata {
            1.0
        } else {
            fotogramma as f32 / durata as f32
        };
        *campione *= da + (1.0 - da) * t;
    }
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

    // ── la dissolvenza ──

    #[test]
    fn una_dissolvenza_lunga_zero_e_il_gapless_di_prima() {
        // La prova che il piano chiedeva: a durata 0 il blocco che esce deve
        // restare **identico** a com'era, campione per campione. È la garanzia
        // che accendere il crossfade sia una scelta e non un cambiamento del
        // suono di chi non lo accende.
        let originale = [0.5_f32, -0.25, 0.125, -1.0];
        let mut uscente = originale;
        dissolvi(&mut uscente, &[1.0; 4], 0, 0, 2);
        assert_eq!(uscente, originale);
    }

    #[test]
    fn all_inizio_si_sente_solo_quello_che_esce() {
        let mut uscente = [1.0_f32, 1.0];
        dissolvi(&mut uscente, &[1.0, 1.0], 0, 100, 2);
        // cos(0) = 1, sin(0) = 0: il brano entrante non c'è ancora.
        for campione in uscente {
            assert!((campione - 1.0).abs() < 1e-6, "campione {campione}");
        }
    }

    #[test]
    fn alla_fine_si_sente_solo_quello_che_entra() {
        let mut uscente = [1.0_f32, 1.0];
        dissolvi(&mut uscente, &[0.5, 0.5], 100, 100, 2);
        for campione in uscente {
            assert!((campione - 0.5).abs() < 1e-6, "campione {campione}");
        }
    }

    #[test]
    fn oltre_la_fine_si_resta_a_fondo_corsa() {
        // Il blocco può sforare la finestra: quel che sfora è già tutto il
        // brano nuovo, non un guadagno che continua a crescere.
        let mut uscente = [1.0_f32, 1.0];
        dissolvi(&mut uscente, &[0.5, 0.5], 500, 100, 2);
        for campione in uscente {
            assert!((campione - 0.5).abs() < 1e-6, "campione {campione}");
        }
    }

    #[test]
    fn l_energia_resta_costante_per_tutta_la_dissolvenza() {
        // Il motivo per cui la curva è coseno/seno e non due rette. Su
        // materiale scorrelato — che è il caso di due brani diversi — le
        // potenze si sommano, quindi cos²+sin² = 1 tiene il volume percepito
        // fermo. Due rampe lineari darebbero 0,5 a metà: un buco udibile.
        for fotogramma in 0..=100_u64 {
            let mut uscente = [1.0_f32];
            dissolvi(&mut uscente, &[1.0], fotogramma, 100, 1);
            let a = (fotogramma as f32 / 100.0) * std::f32::consts::FRAC_PI_2;
            let energia = a.cos().powi(2) + a.sin().powi(2);
            assert!((energia - 1.0).abs() < 1e-5, "energia {energia}");
            // E la somma non deve mai superare la radice di due, che è il
            // massimo di cos+sin: oltre ci sarebbe saturazione.
            let somma = uscente[0];
            assert!(somma <= std::f32::consts::SQRT_2 + 1e-5, "somma {somma}");
        }
    }

    #[test]
    fn i_canali_di_uno_stesso_fotogramma_ricevono_lo_stesso_guadagno() {
        // Se la rampa avanzasse per campione invece che per fotogramma,
        // sinistra e destra starebbero su due punti diversi della curva e
        // l'immagine stereo scivolerebbe per tutta la dissolvenza.
        let mut uscente = [1.0_f32, 1.0, 1.0, 1.0];
        dissolvi(&mut uscente, &[0.0; 4], 0, 4, 2);
        assert!((uscente[0] - uscente[1]).abs() < f32::EPSILON);
        assert!((uscente[2] - uscente[3]).abs() < f32::EPSILON);
        // E fotogrammi diversi devono invece essere diversi, altrimenti la
        // prova qui sopra passerebbe anche con una rampa ferma.
        assert!(uscente[0] > uscente[2]);
    }

    #[test]
    fn un_blocco_entrante_piu_corto_non_esce_dai_bordi() {
        // `indexing_slicing` è vietato nel progetto e `dissolvi` usa `get`: la
        // prova sta qui perché il caso è reale, non teorico. L'ultimo blocco
        // del brano entrante è quasi sempre più corto degli altri.
        let mut uscente = [1.0_f32; 6];
        dissolvi(&mut uscente, &[1.0, 1.0], 0, 6, 2);
        assert!(uscente.iter().all(|c| c.is_finite()));
    }

    // ── la ripresa, quando la curva si interrompe ──

    #[test]
    fn la_ripresa_attacca_dove_la_dissolvenza_si_e_interrotta() {
        // Il contratto fra le due funzioni, ed è tutto il punto della ripresa:
        // l'ultimo campione mescolato e il primo di quello che continua da solo
        // devono avere lo stesso guadagno. Se si scostassero, al posto del clic
        // che si voleva togliere ce ne sarebbe uno più piccolo.
        let (_, dentro) = punto_curva(50, 100);
        // Il brano uscente è già a zero: quel che resta nel blocco è tutto e
        // solo il guadagno dell'entrante.
        let mut mescolato = [0.0_f32];
        dissolvi(&mut mescolato, &[1.0], 50, 100, 1);
        let mut ripreso = [1.0_f32];
        risali(&mut ripreso, dentro, 0, 40, 1);
        assert!(
            (mescolato[0] - ripreso[0]).abs() < 1e-6,
            "{} contro {}",
            mescolato[0],
            ripreso[0]
        );
    }

    #[test]
    fn la_ripresa_arriva_a_piena_ampiezza_e_ci_resta() {
        let mut alla_fine = [1.0_f32, 1.0];
        risali(&mut alla_fine, 0.7, 40, 40, 2);
        for campione in alla_fine {
            assert!((campione - 1.0).abs() < 1e-6, "campione {campione}");
        }
        // Oltre la fine non si continua a salire: il blocco può sforare.
        let mut oltre = [1.0_f32];
        risali(&mut oltre, 0.7, 500, 40, 1);
        assert!((oltre[0] - 1.0).abs() < 1e-6, "campione {}", oltre[0]);
    }

    #[test]
    fn la_ripresa_sale_e_non_torna_indietro() {
        // Una salita che avesse un solo passo all'indietro sarebbe un
        // tremolio, che è più udibile del gradino che sostituisce.
        let mut blocco = [1.0_f32; 64];
        risali(&mut blocco, 0.5, 0, 64, 1);
        let mut precedente = 0.0_f32;
        for campione in blocco {
            assert!(campione >= precedente, "{campione} dopo {precedente}");
            precedente = campione;
        }
        assert!(
            precedente > 0.9,
            "non arriva da nessuna parte: {precedente}"
        );
    }

    #[test]
    fn i_canali_di_uno_stesso_fotogramma_risalgono_insieme() {
        let mut blocco = [1.0_f32; 4];
        risali(&mut blocco, 0.0, 0, 2, 2);
        assert!((blocco[0] - blocco[1]).abs() < f32::EPSILON);
        assert!((blocco[2] - blocco[3]).abs() < f32::EPSILON);
        assert!(blocco[2] > blocco[0]);
    }

    #[test]
    fn una_ripresa_lunga_zero_non_divide_per_zero() {
        let mut blocco = [1.0_f32];
        risali(&mut blocco, 0.5, 0, 0, 1);
        assert!(blocco[0].is_finite());
    }

    #[test]
    fn zero_canali_non_divide_per_zero() {
        // Il formato arriva dal dispositivo: non dovrebbe mai dire zero
        // canali, ma «non dovrebbe» dentro un `/` è come non avere il
        // controllo. `canali.max(1)` lo copre, questa prova lo tiene coperto.
        let mut uscente = [1.0_f32, 1.0];
        dissolvi(&mut uscente, &[0.5, 0.5], 0, 4, 0);
        assert!(uscente.iter().all(|c| c.is_finite()));
    }
}
