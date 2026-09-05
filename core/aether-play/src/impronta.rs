//! Come suona un brano, in una cinquantina di numeri.
//!
//! Serve a rispondere a «un altro brano che somigli a questo», che è la domanda
//! a cui i tag non rispondono: due pezzi dello stesso artista e dello stesso
//! anno possono essere uno acustico e uno distorto, e nel database sono due
//! righe quasi identiche. Il confronto vero lo fa
//! [`aether_domain::affinita`], che è puro; qui si producono i numeri su cui
//! quel confronto lavora.
//!
//! # Perché sta qui
//!
//! Per la stessa ragione di [`crate::attacchi`], e la sua intestazione la
//! argomenta per esteso: i due pezzi che servono sono già in questo crate e
//! sono già provati — il decodificatore, che apre tutti i formati che l'app sa
//! suonare, e la trasformata di [`crate::spettro`], scritta a mano per non
//! portarsi dietro un albero di dipendenze. Un banco di filtri mel in un crate
//! nuovo vorrebbe dire una seconda copia di entrambi.
//!
//! # Trenta secondi dal centro, e non il file intero
//!
//! Un FLAC da quattro minuti pesa quaranta megabyte; trenta secondi ne pesano
//! tre e mezzo. La differenza non si vede su un SSD e si vede tutta su una
//! cartella di rete, che è il terreno su cui questo programma ha passato
//! l'ultima versione a lavorare — e questa passata gira in sottofondo su tutta
//! la libreria, cioè è esattamente il genere di lavoro che non deve mettersi di
//! traverso.
//!
//! **Dal centro** e non dalla testa: i primi trenta secondi di un disco sono
//! un'introduzione — una dissolvenza, una frase parlata, uno strumento solo — e
//! gli ultimi sono una coda o silenzio. Il mezzo è la parte che somiglia alla
//! canzone.
//!
//! # 22 050 Hz, e perché il taglio è a 10 kHz e non a Nyquist
//!
//! Sotto gli 11 kHz un MP3 a 256 kbps è indistinguibile dal suo FLAC: è sopra
//! che i codec con perdita tagliano. Analizzare lì significa che lo stesso brano
//! in due formati cade nello stesso punto — che in una libreria mista è la
//! differenza fra un motore che funziona e uno che si sdoppia.
//!
//! L'ultimo migliaio di hertz però va lasciato fuori lo stesso, e non per i
//! codec: è la banda di transizione del ricampionatore. Un master a 44,1 kHz e
//! uno a 48 kHz decadono lì in modi diversi, e due edizioni dello stesso disco
//! si allontanerebbero per una ragione che con la musica non c'entra. Da qui
//! [`MEL_ALTA_HZ`] a 10 000. In basso, per il motivo speculare, si parte da 40:
//! sotto ci sono il rombo e la continua, non un disco.
//!
//! # Il decodificatore a un canale prende il sinistro, non la media
//!
//! Va detto perché è la trappola di questo modulo. `decodifica::adatta_canali`,
//! chiesto un canale su una sorgente stereo, restituisce il piano 0 — e altret-
//! tanto fa il ramo del ricampionatore. Per [`crate::attacchi`] è innocuo:
//! cerca sillabe, e le sillabe stanno al centro. Per un'impronta no: un disco
//! con gli strumenti aperti a destra e a sinistra darebbe l'impronta di metà di
//! sé.
//!
//! Si apre quindi a **due** canali e la media la fa questo modulo. Una sorgente
//! mono viene duplicata da entrambi i rami, quindi la media la restituisce
//! identica: corretto in tutti e tre i casi.
//!
//! # Cosa è invariante al guadagno, e cosa no di proposito
//!
//! Quasi tutti i descrittori non cambiano se lo stesso file è masterizzato più
//! forte: le varianze dei coefficienti cepstrali lo sono perché sono varianze di
//! un logaritmo, il flusso perché è diviso per l'energia della finestra, il
//! centroide e il rolloff perché sono rapporti.
//!
//! Il livello medio ([`indice::RMS`]) invece **no**, e resta così apposta: due copie dello
//! stesso CD hanno lo stesso livello, una rimasterizzazione a volume doppio è
//! un'altra cosa, e nel confronto quella differenza deve pesare.

use aether_domain::affinita::Gruppo;
use aether_domain::errors::AppError;

use crate::decodifica::{Decodificatore, Sorgente};
use crate::spettro::{C, rovescia, trasforma};

/// La frequenza a cui si analizza. La stessa di [`crate::attacchi`].
pub const FREQUENZA: u32 = 22_050;

/// Quanti campioni entrano in una finestra: 46 ms, 21,5 Hz per bin.
pub const FINESTRA: usize = 1024;

/// Di quanto si avanza fra una finestra e la successiva: 23 ms.
pub const PASSO: usize = 512;

/// Quanti bin porta informazione: oltre la metà c'è lo specchio.
#[expect(
    clippy::integer_division,
    reason = "FINESTRA è una potenza di due: la metà è esatta"
)]
pub const META_FINESTRA: usize = FINESTRA / 2;

/// Quanto audio si guarda.
pub const DURATA_MS: u64 = 30_000;

/// Sotto quanto audio i numeri non vogliono dire niente.
///
/// Dieci secondi, cioè poco più di quattrocento finestre. Sotto, le varianze
/// descrivono il campione invece del brano, e un'impronta che descrive il
/// proprio campione è peggio di nessuna impronta: non si annuncia.
pub const MINIMO_MS: u64 = 10_000;

/// Quante bande del banco di filtri mel.
pub const MEL_BANDE: usize = 26;

/// Dove comincia il banco: sotto ci sono il rombo e la continua.
pub const MEL_BASSA_HZ: f32 = 40.0;

/// Dove finisce: sopra c'è la banda di transizione del ricampionatore.
pub const MEL_ALTA_HZ: f32 = 10_000.0;

/// Quanti coefficienti cepstrali si tengono, da `c1`.
///
/// `c0` si butta: è il livello, e il livello ha già i suoi quattro descrittori
/// con un peso loro. Tenerlo vorrebbe dire farlo pesare due volte.
pub const CEPSTRI: usize = 13;

/// Quanti numeri ha un'impronta.
pub const DIMENSIONI: usize = 47;

/// Quale versione dell'estrattore produce questi numeri.
///
/// Il database la registra per riga. Cambiare i descrittori — una banda in più,
/// un altro modo di stimare il tempo — rende le impronte vecchie incomparabili
/// con le nuove, e mescolarle darebbe distanze senza senso invece di un errore:
/// alzare questo numero è ciò che fa rianalizzare, a poco a poco, senza
/// cancellare niente in blocco.
pub const VERSIONE_ESTRATTORE: u32 = 1;

/// Dove sta ogni tratto dell'impronta, e quanto conta.
///
/// I pesi sono cinque decisioni e non quarantasette: vedi
/// [`aether_domain::affinita::Gruppo`]. Il timbro davanti perché è quel che
/// distingue un pianoforte da una chitarra distorta; il ritmo subito dietro
/// perché separa due dischi che il timbro confonderebbe; l'armonia per ultima
/// perché è la più fragile delle cinque misure.
pub const RAGGRUPPAMENTO: [Gruppo; 5] = [
    Gruppo {
        nome: "timbro",
        inizio: 0,
        fine: 26,
        peso: 0.40,
    },
    Gruppo {
        nome: "spettro",
        inizio: 26,
        fine: 34,
        peso: 0.15,
    },
    Gruppo {
        nome: "dinamica",
        inizio: 34,
        fine: 38,
        peso: 0.15,
    },
    Gruppo {
        nome: "ritmo",
        inizio: 38,
        fine: 41,
        peso: 0.20,
    },
    Gruppo {
        nome: "croma",
        inizio: 41,
        fine: 47,
        peso: 0.10,
    },
];

/// Gli indici dell'impronta, per chi legge le prove.
///
/// Non è un `enum` con cui si indicizza: è un promemoria di cosa c'è dove,
/// scritto una volta invece che nei commenti di ogni prova.
pub mod indice {
    /// Medie dei coefficienti cepstrali `c1..c13`.
    pub const MFCC_MEDIE: usize = 0;
    /// Varianze degli stessi. Sono la parte buona: la varianza di una quantità
    /// logaritmica non cambia se il file è più forte.
    pub const MFCC_VARIANZE: usize = 13;
    /// Centroide spettrale: media, varianza.
    pub const CENTROIDE: usize = 26;
    /// Rolloff all'85 %: media, varianza.
    pub const ROLLOFF: usize = 28;
    /// Piattezza spettrale: media, varianza.
    pub const PIATTEZZA: usize = 30;
    /// Flusso spettrale: media, varianza.
    pub const FLUSSO: usize = 32;
    /// Livello medio in dBFS. **Non** invariante al guadagno, di proposito.
    pub const RMS: usize = 34;
    /// Escursione del livello in dB, fra il 5° e il 95° percentile.
    pub const ESCURSIONE: usize = 35;
    /// Fattore di cresta in dB.
    pub const CRESTA: usize = 36;
    /// Quota di finestre quasi mute.
    pub const SILENZIO: usize = 37;
    /// Tempo, come `log2(bpm / 120)`.
    pub const TEMPO: usize = 38;
    /// Quanto è marcato il battito, in `0..=1`.
    pub const BATTITO: usize = 39;
    /// Attacchi al secondo, come `ln(1 + x)`.
    pub const ATTACCHI: usize = 40;
    /// Sei ampiezze della trasformata del croma: invarianti per trasposizione.
    pub const CROMA: usize = 41;
}

/// Com'è andata la misura di un brano.
///
/// I due esiti che non sono un'impronta **non** sono errori: il file si è
/// aperto e si è letto, e quel che si è trovato non è misurabile. La
/// distinzione conta per chi tiene il registro — un guasto si riprova, questi
/// no.
#[derive(Debug, Clone, PartialEq)]
pub enum Esito {
    /// I numeri.
    Fatta(Box<[f32; DIMENSIONI]>),
    /// Trenta secondi di silenzio. Terminale: fra un mese saranno gli stessi, e
    /// un'impronta di silenzio sarebbe una calamita che attira tutto.
    Muto,
    /// Meno di [`MINIMO_MS`] di audio leggibile.
    Corto,
}

/// Da dove comincia la finestra di analisi, in millisecondi.
///
/// Pura, così si prova senza aprire niente. Un brano più corto della finestra
/// si legge dall'inizio.
#[must_use]
#[expect(
    clippy::integer_division,
    reason = "il millisecondo dispari a metà di un brano non interessa nessuno"
)]
pub const fn inizio(durata_ms: u64) -> u64 {
    durata_ms.saturating_sub(DURATA_MS) / 2
}

/// L'impronta di un brano.
///
/// # Errori
///
/// Quelli del decodificatore, **inalterati**: `playback.formatUnsupported` per
/// un formato che non si sa aprire, `playback.decodeFailed` per un file
/// rovinato, e i guasti di rete così come li riconosce
/// [`crate::decodifica`]. La distinzione fra «il file è rotto» e «la cartella di
/// rete è caduta» è quella su cui la passata di sottofondo decide se ricordarsi
/// il fallimento o dimenticarlo, e appiattirla qui vorrebbe dire segnare
/// illeggibili tutti i brani incontrati mentre il portatile era fuori dalla
/// rete.
pub fn calcola(sorgente: Sorgente, durata_ms: u64) -> Result<Esito, AppError> {
    let campioni = leggi(sorgente, durata_ms)?;
    Ok(misura(&campioni))
}

/// Legge i campioni della finestra, in mono a [`FREQUENZA`].
fn leggi(sorgente: Sorgente, durata_ms: u64) -> Result<Vec<f32>, AppError> {
    // Due canali e non uno: vedi l'intestazione del modulo.
    let mut decodificatore = Decodificatore::apri(sorgente, FREQUENZA, 2)?;

    let da = inizio(durata_ms);
    if da > 0 {
        decodificatore.cerca(da)?;
    }

    let quanti = quanti_campioni(DURATA_MS);
    let mut mono: Vec<f32> = Vec::with_capacity(quanti);
    let mut blocco: Vec<f32> = Vec::with_capacity(4096);
    // Un blocco può finire a metà di una coppia: il campione spaiato aspetta il
    // blocco dopo invece di essere mediato con lo zero, che introdurrebbe un
    // clic ogni quattromila campioni.
    let mut sospeso: Option<f32> = None;

    while mono.len() < quanti && decodificatore.prossimo(&mut blocco)? {
        for campione in &blocco {
            match sospeso.take() {
                Some(sinistro) => mono.push((sinistro + *campione) * 0.5),
                None => sospeso = Some(*campione),
            }
            if mono.len() >= quanti {
                break;
            }
        }
    }
    Ok(mono)
}

/// Quanti campioni sono tanti millisecondi.
#[expect(
    clippy::integer_division,
    reason = "convertire millesimi in campioni è una moltiplicazione per la frequenza"
)]
fn quanti_campioni(ms: u64) -> usize {
    let n = ms.saturating_mul(u64::from(FREQUENZA)) / 1000;
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// Le misure di una finestra.
#[derive(Debug, Clone, Copy, Default)]
struct Finestra {
    centroide: f32,
    rolloff: f32,
    piattezza: f32,
    /// Livello in dBFS.
    rms: f32,
}

/// I numeri, dai campioni.
fn misura(campioni: &[f32]) -> Esito {
    if campioni.len() < quanti_campioni(MINIMO_MS) {
        return Esito::Corto;
    }

    let bit = FINESTRA.trailing_zeros();
    let ordine: Vec<usize> = (0..FINESTRA).map(|i| rovescia(i, bit)).collect();
    let hann = hann();
    let banco = banco_mel();
    let classi = pesi_di_altezza();

    let mut lavoro = vec![C::default(); FINESTRA];
    let mut correnti = vec![0.0_f32; META_FINESTRA];
    let mut prima = true;

    let mut finestre: Vec<Finestra> = Vec::new();
    let mut cepstri: Vec<[f32; CEPSTRI]> = Vec::new();
    let mut flussi: Vec<f32> = Vec::new();
    let mut croma = [0.0_f32; 12];
    let mut mel = vec![0.0_f32; MEL_BANDE];
    let mut mel_prima = vec![0.0_f32; MEL_BANDE];

    let mut partenza = 0_usize;
    while partenza.saturating_add(FINESTRA) <= campioni.len() {
        let Some(tratto) = campioni.get(partenza..partenza.saturating_add(FINESTRA)) else {
            break;
        };

        // Finestratura e rimescolamento dei bit in un passaggio solo, come fa
        // `attacchi`: la trasformata vuole già i dati in ordine invertito.
        for (posto, i) in lavoro.iter_mut().zip(0..FINESTRA) {
            let da = ordine.get(i).copied().unwrap_or(0);
            let campione = tratto.get(da).copied().unwrap_or(0.0);
            let peso = hann.get(da).copied().unwrap_or(0.0);
            *posto = C::nuovo(campione * peso, 0.0);
        }
        trasforma(&mut lavoro);

        // Oltre metà finestra c'è lo specchio: non porta niente di nuovo.
        for (bin, valore) in correnti.iter_mut().enumerate() {
            *valore = lavoro.get(bin).map_or(0.0, |c| c.potenza());
        }

        energie_mel(&correnti, &banco, &mut mel);

        finestre.push(descrivi(&correnti, tratto));
        cepstri.push(cepstro(&mel));
        accumula_croma(&correnti, &classi, &mut croma);

        if prima {
            prima = false;
        } else {
            flussi.push(flusso(&mel, &mel_prima));
        }
        std::mem::swap(&mut mel_prima, &mut mel);

        partenza = partenza.saturating_add(PASSO);
    }

    if finestre.len() < 2 || flussi.is_empty() {
        return Esito::Corto;
    }

    if muto(campioni) {
        return Esito::Muto;
    }

    Esito::Fatta(Box::new(componi(
        &finestre, &cepstri, &flussi, &croma, campioni,
    )))
}

/// Mette in fila i quarantasette numeri.
fn componi(
    finestre: &[Finestra],
    cepstri: &[[f32; CEPSTRI]],
    flussi: &[f32],
    croma: &[f32; 12],
    campioni: &[f32],
) -> [f32; DIMENSIONI] {
    let mut fuori = [0.0_f32; DIMENSIONI];

    // ── timbro ──
    for k in 0..CEPSTRI {
        let colonna: Vec<f32> = cepstri
            .iter()
            .map(|c| c.get(k).copied().unwrap_or(0.0))
            .collect();
        let (media, varianza) = statistiche(&colonna);
        poni(&mut fuori, indice::MFCC_MEDIE.saturating_add(k), media);
        poni(
            &mut fuori,
            indice::MFCC_VARIANZE.saturating_add(k),
            varianza,
        );
    }

    // ── spettro ──
    for (base, estrai) in [
        (
            indice::CENTROIDE,
            (|f: &Finestra| f.centroide) as fn(&Finestra) -> f32,
        ),
        (indice::ROLLOFF, |f: &Finestra| f.rolloff),
        (indice::PIATTEZZA, |f: &Finestra| f.piattezza),
    ] {
        let colonna: Vec<f32> = finestre.iter().map(estrai).collect();
        let (media, varianza) = statistiche(&colonna);
        poni(&mut fuori, base, media);
        poni(&mut fuori, base.saturating_add(1), varianza);
    }
    let (media, varianza) = statistiche(flussi);
    poni(&mut fuori, indice::FLUSSO, media);
    poni(&mut fuori, indice::FLUSSO.saturating_add(1), varianza);

    // ── dinamica ──
    let mut livelli: Vec<f32> = finestre.iter().map(|f| f.rms).collect();
    livelli.sort_by(f32::total_cmp);
    let (media, _) = statistiche(&livelli);
    poni(&mut fuori, indice::RMS, media);
    poni(
        &mut fuori,
        indice::ESCURSIONE,
        percentile(&livelli, 0.95) - percentile(&livelli, 0.05),
    );
    poni(&mut fuori, indice::CRESTA, cresta(campioni));
    poni(&mut fuori, indice::SILENZIO, quota_muta(&livelli));

    // ── ritmo ──
    let (tempo, battito) = tempo(flussi);
    poni(&mut fuori, indice::TEMPO, tempo);
    poni(&mut fuori, indice::BATTITO, battito);
    poni(&mut fuori, indice::ATTACCHI, densita_attacchi(flussi));

    // ── croma ──
    for (i, ampiezza) in croma_invariante(croma).iter().enumerate() {
        poni(&mut fuori, indice::CROMA.saturating_add(i), *ampiezza);
    }

    // Nessun numero non finito esce di qui, in nessuna circostanza: una libreria
    // con un solo NaN darebbe distanze non ordinabili, e l'ordinamento che ne
    // risulta non è sbagliato in un punto — è arbitrario dappertutto.
    for valore in &mut fuori {
        if !valore.is_finite() {
            *valore = 0.0;
        }
    }
    fuori
}

/// Scrive un valore, se l'indice esiste.
fn poni(fuori: &mut [f32; DIMENSIONI], dove: usize, valore: f32) {
    if let Some(posto) = fuori.get_mut(dove) {
        *posto = valore;
    }
}

/// Centroide, rolloff, piattezza e livello di una finestra.
fn descrivi(potenze: &[f32], campioni: &[f32]) -> Finestra {
    let ampiezze: Vec<f32> = potenze.iter().map(|p| p.max(0.0).sqrt()).collect();
    let totale: f32 = ampiezze.iter().sum();

    let centroide = if totale > 0.0 {
        let pesata: f32 = ampiezze
            .iter()
            .enumerate()
            .map(|(bin, a)| indice_normalizzato(bin) * a)
            .sum();
        pesata / totale
    } else {
        0.0
    };

    let soglia = totale * 0.85;
    let mut somma = 0.0_f32;
    let mut rolloff = 1.0_f32;
    for (bin, a) in ampiezze.iter().enumerate() {
        somma += *a;
        if somma >= soglia && totale > 0.0 {
            rolloff = indice_normalizzato(bin);
            break;
        }
    }

    // Media geometrica su media aritmetica: rumore vicino a uno, tono vicino a
    // zero. Il `1e-10` non è una difesa contro lo zero, è il pavimento sotto cui
    // un bin non è più segnale ma il rumore della trasformata.
    let quanti = potenze.len().max(1);
    let log_media: f32 = potenze
        .iter()
        .map(|p| (p.max(0.0) + 1e-10).ln())
        .sum::<f32>()
        / quanti_f32(quanti);
    let aritmetica: f32 = potenze.iter().map(|p| p.max(0.0)).sum::<f32>() / quanti_f32(quanti);
    let piattezza = if aritmetica > 0.0 {
        (log_media.exp() / aritmetica).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let quadrati: f32 = campioni.iter().map(|c| c * c).sum();
    let rms = in_db((quadrati / quanti_f32(campioni.len().max(1))).sqrt());

    Finestra {
        centroide,
        rolloff,
        piattezza,
        rms,
    }
}

/// L'energia in ciascuna banda mel, lineare.
fn energie_mel(potenze: &[f32], banco: &[Vec<(usize, f32)>], fuori: &mut [f32]) {
    for (banda, energia) in banco.iter().zip(fuori.iter_mut()) {
        *energia = banda
            .iter()
            .map(|(bin, peso)| potenze.get(*bin).copied().unwrap_or(0.0) * peso)
            .sum();
    }
}

/// I coefficienti cepstrali di una finestra, da `c1`.
fn cepstro(energie: &[f32]) -> [f32; CEPSTRI] {
    let log: Vec<f32> = energie.iter().map(|e| (e.max(0.0) + 1e-10).ln()).collect();

    // Trasformata coseno discreta di tipo II, tenendo `c1..c13`.
    let mut fuori = [0.0_f32; CEPSTRI];
    let n = log.len().max(1);
    for (k, posto) in fuori.iter_mut().enumerate() {
        let ordine = k.saturating_add(1);
        let mut somma = 0.0_f32;
        for (m, energia) in log.iter().enumerate() {
            let angolo =
                std::f32::consts::PI * quanti_f32(ordine) * (quanti_f32(m) + 0.5) / quanti_f32(n);
            somma += energia * angolo.cos();
        }
        *posto = somma / quanti_f32(n);
    }
    fuori
}

/// Il flusso spettrale fra due finestre: solo le salite, e diviso per l'energia.
///
/// A banda intera, al contrario di [`crate::attacchi`] che si restringe alla
/// voce: là si cercano le sillabe, qui quanto cambia il disco. La divisione per
/// l'energia della finestra è ciò che lo rende indipendente dal guadagno —
/// senza, lo stesso brano masterizzato più forte risulterebbe più mosso.
///
/// # Sulle bande mel, e non sui bin della trasformata
///
/// È la differenza fra un descrittore e un artefatto. Una nota tenuta la cui
/// frequenza non cade esattamente su un bin — cioè quasi ogni nota — sparge
/// energia sui bin vicini in una figura che cambia a ogni finestra, perché la
/// fase avanza di mezzo periodo di finestra per volta. Sui bin grezzi quella
/// dispersione produce un flusso che oscilla **periodicamente**, e
/// l'autocorrelazione di [`tempo`] ci trova un battito che non c'è: una nota
/// sola dava 0,85 di «battito marcato».
///
/// Le ventisei bande mettono insieme decine di bin ciascuna, e la figura di
/// dispersione si somma dentro la banda invece di spostarsi da un bin all'altro.
/// È anche il motivo per cui i rilevatori di attacchi seri lavorano tutti su uno
/// spettro compresso.
fn flusso(correnti: &[f32], precedenti: &[f32]) -> f32 {
    let mut salite = 0.0_f32;
    let mut energia = 0.0_f32;
    for (ora, prima) in correnti.iter().zip(precedenti.iter()) {
        let a = ora.max(0.0).sqrt();
        let b = prima.max(0.0).sqrt();
        if a > b {
            salite += a - b;
        }
        energia += a;
    }
    if energia > 0.0 { salite / energia } else { 0.0 }
}

/// Tempo come `log2(bpm / 120)`, e quanto è marcato il battito.
///
/// # Perché il tempo in logaritmo
///
/// Perché l'errore classico di ogni stimatore è il fattore due — prendere il
/// doppio o la metà del tempo vero — e in logaritmo quell'errore vale
/// esattamente uno, sempre. In battiti al minuto varrebbe 60 fra 60 e 120 e 90
/// fra 90 e 180: la stessa confusione, pesata tre volte tanto su un pezzo
/// veloce.
fn tempo(flussi: &[f32]) -> (f32, f32) {
    let (media, varianza) = statistiche(flussi);

    // Un tono continuo non ha attacchi, e il suo flusso è zero a meno del
    // rumore della trasformata. Quel rumore però è **periodico** — la finestra
    // scorre di un numero fisso di campioni sopra una sinusoide — e
    // l'autocorrelazione qui sotto è normalizzata sul ritardo zero, cioè cieca
    // alla scala: senza questo cancello darebbe 0,85 di «battito» su una nota
    // tenuta, che è il modo più sicuro di far sembrare ritmicamente simili due
    // cose che non hanno ritmo.
    if varianza.max(0.0).sqrt() < VARIAZIONE_MINIMA {
        return (0.0, 0.0);
    }

    let centrato: Vec<f32> = flussi.iter().map(|f| f - media).collect();
    let zero: f32 = centrato.iter().map(|x| x * x).sum();
    if zero <= 0.0 {
        return (0.0, 0.0);
    }

    // I ritardi che corrispondono a 60..180 battiti al minuto, a 23,22 ms per
    // finestra.
    let mut punteggi = vec![0.0_f32; RITARDO_MASSIMO.saturating_add(1)];
    let mut migliore = 0.0_f32;
    let mut ritardo_migliore = 0_usize;
    for ritardo in RITARDO_MINIMO..=RITARDO_MASSIMO {
        if ritardo >= centrato.len() {
            break;
        }
        let Some(spostato) = centrato.get(ritardo..) else {
            break;
        };
        let somma: f32 = centrato
            .iter()
            .zip(spostato.iter())
            .map(|(a, b)| a * b)
            .sum();
        let normalizzata = somma / zero;
        if let Some(posto) = punteggi.get_mut(ritardo) {
            *posto = normalizzata;
        }
        if normalizzata > migliore {
            migliore = normalizzata;
            ritardo_migliore = ritardo;
        }
    }

    if ritardo_migliore == 0 || migliore <= 0.0 {
        // Nessuna periodicità utile: si dichiara, invece di inventare un numero
        // che poi qualcuno confronterebbe con uno vero.
        return (0.0, 0.0);
    }

    // ── la correzione d'ottava ──
    //
    // L'errore del fattore due è il difetto classico di ogni stimatore per
    // autocorrelazione, e non è un caso raro: se un segnale si ripete ogni L,
    // si ripete anche ogni 2L, e il picco a 2L è alto quanto quello a L. Quale
    // dei due vinca lo decide il rumore, cioè niente.
    //
    // Fra i due si prende **il più corto**, che è la lettura giusta: il periodo
    // è il più piccolo che spiega il segnale, e i due che spiegano ugualmente
    // bene non sono due risposte fra cui scegliere. La soglia serve a non
    // scambiare per un'ottava un picco secondario qualunque.
    if let Some(meta) = dimezza(ritardo_migliore) {
        let punteggio_meta = punteggi.get(meta).copied().unwrap_or(0.0);
        if punteggio_meta >= migliore * SOGLIA_OTTAVA {
            ritardo_migliore = meta;
            migliore = migliore.max(punteggio_meta);
        }
    }

    let secondi_per_finestra = quanti_f32(PASSO) / quanti_f32_da_u32(FREQUENZA);
    let bpm = 60.0 / (quanti_f32(ritardo_migliore) * secondi_per_finestra);
    ((bpm / 120.0).log2(), migliore.clamp(0.0, 1.0))
}

/// Quanto deve variare il flusso perché valga la pena cercarci un battito.
///
/// Il flusso è già normalizzato sull'energia della finestra, quindi è un numero
/// puro in un intervallo noto: un millesimo di scarto è sotto il rumore della
/// trasformata e sopra qualunque musica.
const VARIAZIONE_MINIMA: f32 = 1e-3;

/// Il ritardo più corto che si guarda: 180 battiti al minuto.
const RITARDO_MINIMO: usize = 14;
/// Il più lungo: 60 battiti al minuto.
const RITARDO_MASSIMO: usize = 44;

/// Quanto deve valere il picco a metà ritardo perché si prenda quello.
///
/// Ottanta centesimi del picco vincente. Alto, perché scendere di un'ottava
/// quando non si deve è peggio che restarci: un pezzo a 150 battiti dichiarato a
/// 75 finisce vicino alle ballate.
const SOGLIA_OTTAVA: f32 = 0.80;

/// Metà di un ritardo, se la metà è ancora dentro la griglia.
fn dimezza(ritardo: usize) -> Option<usize> {
    let meta = ritardo.checked_div(2)?;
    (meta >= RITARDO_MINIMO).then_some(meta)
}

/// Quanti attacchi al secondo, in scala logaritmica.
///
/// `ln(1 + x)` e non il numero nudo: fra due e quattro attacchi al secondo c'è
/// una differenza di carattere, fra venti e ventidue no.
fn densita_attacchi(flussi: &[f32]) -> f32 {
    let (media, varianza) = statistiche(flussi);
    let soglia = media + varianza.max(0.0).sqrt();
    let quanti = flussi
        .windows(3)
        .filter(|w| {
            let Some(centro) = w.get(1) else {
                return false;
            };
            *centro > soglia
                && *centro >= w.first().copied().unwrap_or(0.0)
                && *centro >= w.get(2).copied().unwrap_or(0.0)
        })
        .count();
    let secondi = quanti_f32(flussi.len()) * quanti_f32(PASSO) / quanti_f32_da_u32(FREQUENZA);
    if secondi <= 0.0 {
        return 0.0;
    }
    (1.0 + quanti_f32(quanti) / secondi).ln()
}

/// Le sei ampiezze della trasformata del croma.
///
/// # Perché non le dodici classi
///
/// Perché le dodici classi grezze rendono la somiglianza dipendente dalla
/// tonalità: lo stesso pezzo in do e in re diventerebbe due pezzi diversi, che
/// non è quel che chiede chi vuole «un brano che somigli a questo».
///
/// La soluzione ovvia — ruotare il vettore sulla classe più forte — ha un
/// difetto che si vede solo dopo: quando due classi sono quasi pari, la
/// rotazione salta di un semitono, e un FLAC e il suo MP3 possono saltare in
/// modi diversi. Le ampiezze della trasformata a dodici punti sono invarianti
/// per costruzione **e continue**: due croma vicini danno ampiezze vicine,
/// sempre.
fn croma_invariante(croma: &[f32; 12]) -> [f32; 6] {
    let totale: f32 = croma.iter().sum();
    let normalizzato: Vec<f32> = if totale > 0.0 {
        croma.iter().map(|c| c / totale).collect()
    } else {
        vec![0.0; 12]
    };

    let mut fuori = [0.0_f32; 6];
    for (k, posto) in fuori.iter_mut().enumerate() {
        let ordine = k.saturating_add(1);
        let mut re = 0.0_f32;
        let mut im = 0.0_f32;
        for (n, valore) in normalizzato.iter().enumerate() {
            let angolo = -2.0 * std::f32::consts::PI * quanti_f32(ordine) * quanti_f32(n) / 12.0;
            re += valore * angolo.cos();
            im += valore * angolo.sin();
        }
        *posto = (re * re + im * im).sqrt();
    }
    fuori
}

/// Accumula il croma di una finestra su quello del brano.
fn accumula_croma(potenze: &[f32], pesi: &[Option<(usize, usize, f32)>], croma: &mut [f32; 12]) {
    for (bin, potenza) in potenze.iter().enumerate() {
        let Some(Some((giu, su, quota))) = pesi.get(bin) else {
            continue;
        };
        let ampiezza = potenza.max(0.0).sqrt();
        if let Some(posto) = croma.get_mut(*giu) {
            *posto += ampiezza * (1.0 - quota);
        }
        if let Some(posto) = croma.get_mut(*su) {
            *posto += ampiezza * quota;
        }
    }
}

/// La frequenza sotto la quale due semitoni vicini cadono nello stesso bin.
///
/// Non è una preferenza, è aritmetica: la finestra dà 21,5 Hz per bin, e un
/// semitone è un passo del 5,946 %. I due si pareggiano a `21,5 / 0,05946`, cioè
/// **362 Hz** — sotto, un do e un do diesis sono lo stesso bin, e assegnarli a
/// due classi diverse vuol dire assegnarli a caso.
///
/// Il costo è che la fondamentale del basso e di buona parte del canto resta
/// fuori, e il croma si regge sulle armoniche. È il compromesso normale per una
/// finestra di questa misura, e la scelta onesta fra due: una finestra da 4096
/// arriverebbe sotto i 100 Hz e costerebbe quattro volte la trasformata, per sei
/// numeri su quarantasette.
const CROMA_BASSA_HZ: f32 = 362.0;

/// Sopra questa ci sono armoniche e piatti, non note.
const CROMA_ALTA_HZ: f32 = 5000.0;

/// Come ogni bin si distribuisce fra due classi d'altezza vicine.
///
/// Restituisce, per ogni bin, la classe sotto, quella sopra e quanto della sua
/// energia va alla seconda.
///
/// # Perché ripartito e non arrotondato
///
/// Arrotondare assegna ogni bin a una classe sola, e allora il croma è una
/// funzione **a gradini** della frequenza: due bin adiacenti possono cadere in
/// classi diverse, e l'energia di una nota — che la finestra sparge sempre su
/// due o tre bin — si spacca in modo che dipende da dove la nota è caduta
/// rispetto ai gradini.
///
/// La conseguenza è quella che rovina il descrittore: trasporre un pezzo di un
/// semitono non ruota il croma di una posizione, lo **rimescola**, e le ampiezze
/// della trasformata — che sono invarianti alla rotazione e a nient'altro — non
/// tornano più. Ripartendo l'energia fra le due classi vicine il croma diventa
/// continuo nella frequenza, e la rotazione torna a essere una rotazione.
fn pesi_di_altezza() -> Vec<Option<(usize, usize, f32)>> {
    (0..META_FINESTRA)
        .map(|bin| {
            let hz = frequenza_del_bin(bin);
            if !(CROMA_BASSA_HZ..=CROMA_ALTA_HZ).contains(&hz) {
                return None;
            }
            let semitoni = 12.0 * (hz / 440.0).log2();
            if !semitoni.is_finite() {
                return None;
            }
            let giu = semitoni.floor();
            let quota = (semitoni - giu).clamp(0.0, 1.0);
            let classe_giu = usize_da(giu.rem_euclid(12.0))?;
            let classe_su = usize_da((giu + 1.0).rem_euclid(12.0))?;
            Some((classe_giu, classe_su, quota))
        })
        .collect()
}

/// Il banco di filtri triangolari su scala mel, come pesi per bin.
fn banco_mel() -> Vec<Vec<(usize, f32)>> {
    let bassa = in_mel(MEL_BASSA_HZ);
    let alta = in_mel(MEL_ALTA_HZ);
    let passi = MEL_BANDE.saturating_add(1);

    // I MEL_BANDE + 2 estremi, equispaziati in mel.
    let estremi: Vec<f32> = (0..=passi.saturating_add(1))
        .map(|i| in_hz(bassa + (alta - bassa) * quanti_f32(i) / quanti_f32(passi.max(1))))
        .collect();

    (0..MEL_BANDE)
        .map(|banda| {
            let sinistra = estremi.get(banda).copied().unwrap_or(0.0);
            let centro = estremi.get(banda.saturating_add(1)).copied().unwrap_or(0.0);
            let destra = estremi.get(banda.saturating_add(2)).copied().unwrap_or(0.0);
            (0..META_FINESTRA)
                .filter_map(|bin| {
                    let hz = frequenza_del_bin(bin);
                    if hz <= sinistra || hz >= destra {
                        return None;
                    }
                    let peso = if hz <= centro {
                        if centro > sinistra {
                            (hz - sinistra) / (centro - sinistra)
                        } else {
                            0.0
                        }
                    } else if destra > centro {
                        (destra - hz) / (destra - centro)
                    } else {
                        0.0
                    };
                    (peso > 0.0).then_some((bin, peso))
                })
                .collect()
        })
        .collect()
}

/// La finestra di Hann, calcolata una volta.
fn hann() -> Vec<f32> {
    (0..FINESTRA)
        .map(|i| {
            let x = 2.0 * std::f32::consts::PI * quanti_f32(i) / quanti_f32(FINESTRA);
            0.5 - 0.5 * x.cos()
        })
        .collect()
}

/// Media e varianza di una serie. Varianza nulla su una serie di un elemento.
fn statistiche(valori: &[f32]) -> (f32, f32) {
    if valori.is_empty() {
        return (0.0, 0.0);
    }
    let quanti = quanti_f32(valori.len());
    let media = valori.iter().sum::<f32>() / quanti;
    let varianza = valori
        .iter()
        .map(|v| {
            let d = v - media;
            d * d
        })
        .sum::<f32>()
        / quanti;
    (
        if media.is_finite() { media } else { 0.0 },
        if varianza.is_finite() { varianza } else { 0.0 },
    )
}

/// Il percentile di una serie **già ordinata**.
fn percentile(ordinati: &[f32], quota: f32) -> f32 {
    if ordinati.is_empty() {
        return 0.0;
    }
    let ultimo = ordinati.len().saturating_sub(1);
    let posto = (quota.clamp(0.0, 1.0) * quanti_f32(ultimo)).round();
    ordinati
        .get(usize_da(posto).unwrap_or(0).min(ultimo))
        .copied()
        .unwrap_or(0.0)
}

/// Il fattore di cresta in dB: quanto il picco supera il valore efficace.
fn cresta(campioni: &[f32]) -> f32 {
    let picco = campioni.iter().fold(0.0_f32, |m, c| m.max(c.abs()));
    let quadrati: f32 = campioni.iter().map(|c| c * c).sum();
    let rms = (quadrati / quanti_f32(campioni.len().max(1))).sqrt();
    if rms <= 0.0 {
        return 0.0;
    }
    in_db(picco) - in_db(rms)
}

/// Di quanto una finestra deve stare sotto la più forte per dirsi muta.
const SOTTO_IL_PICCO_DB: f32 = 45.0;

/// La quota di finestre quasi mute, su livelli **già ordinati**.
fn quota_muta(ordinati: &[f32]) -> f32 {
    let Some(massimo) = ordinati.last().copied() else {
        return 0.0;
    };
    let soglia = massimo - SOTTO_IL_PICCO_DB;
    let quante = ordinati.iter().filter(|l| **l < soglia).count();
    quanti_f32(quante) / quanti_f32(ordinati.len().max(1))
}

/// Sotto quale livello un brano si dichiara muto.
const MUTO_DB: f32 = -70.0;

/// Trenta secondi che non contengono niente.
///
/// # Sul valore efficace complessivo, e non sulla media dei livelli
///
/// La media dei decibel delle finestre sembra la stessa cosa e non lo è: i
/// decibel sono già un logaritmo, e le finestre vuote stanno tutte al pavimento
/// di [`in_db`], cioè a −140. Un brano di percussioni rade — un colpo ogni mezzo
/// secondo, forte, e silenzio in mezzo — ha il 97 % delle finestre al pavimento,
/// e la loro media lo dichiarerebbe muto mentre è pieno di suono.
///
/// Il valore efficace su tutti i campioni non ha questo difetto: somma energia,
/// e l'energia dei colpi c'è. Il silenzio vero resta a −140 e continua a essere
/// riconosciuto.
fn muto(campioni: &[f32]) -> bool {
    if campioni.is_empty() {
        return true;
    }
    let quadrati: f32 = campioni.iter().map(|c| c * c).sum();
    in_db((quadrati / quanti_f32(campioni.len())).sqrt()) < MUTO_DB
}

/// Da ampiezza a decibel, con un pavimento.
///
/// Il pavimento serve perché il logaritmo di zero è meno infinito, e un solo
/// meno infinito in una media la porta via con sé.
fn in_db(ampiezza: f32) -> f32 {
    if ampiezza <= 1e-7 {
        return -140.0;
    }
    20.0 * ampiezza.log10()
}

/// Da hertz a mel.
fn in_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

/// Da mel a hertz.
fn in_hz(mel: f32) -> f32 {
    700.0 * (10.0_f32.powf(mel / 2595.0) - 1.0)
}

/// A quale frequenza sta un bin.
fn frequenza_del_bin(bin: usize) -> f32 {
    quanti_f32(bin) * quanti_f32_da_u32(FREQUENZA) / quanti_f32(FINESTRA)
}

/// L'indice di un bin, riportato in `0..=1` su Nyquist.
///
/// Normalizzato e non in hertz: così centroide e rolloff sono numeri puri, e
/// cambiare la frequenza di analisi non sposterebbe la scala di due descrittori
/// su quarantasette.
fn indice_normalizzato(bin: usize) -> f32 {
    quanti_f32(bin) / quanti_f32(META_FINESTRA.max(1))
}

/// Da conteggio a virgola mobile.
#[expect(
    clippy::cast_precision_loss,
    reason = "conteggi di finestre e di bin: qualche migliaio, esatto in f32"
)]
fn quanti_f32(n: usize) -> f32 {
    n as f32
}

/// Da frequenza di campionamento a virgola mobile.
#[expect(
    clippy::cast_precision_loss,
    reason = "22 050 sta in f32 senza perdere niente"
)]
fn quanti_f32_da_u32(n: u32) -> f32 {
    n as f32
}

/// Da virgola mobile a indice, se ci sta.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "il valore è già arrotondato e non negativo per costruzione; il \
              controllo esplicito qui sotto copre il caso in cui non lo fosse"
)]
fn usize_da(x: f32) -> Option<usize> {
    if !x.is_finite() || !(0.0..=1e9).contains(&x) {
        return None;
    }
    Some(x as usize)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un segnale sinusoidale di ampiezza data.
    fn seno(hz: f32, quanti: usize, ampiezza: f32) -> Vec<f32> {
        (0..quanti)
            .map(|i| {
                let t = i as f32 / FREQUENZA as f32;
                ampiezza * (2.0 * std::f32::consts::PI * hz * t).sin()
            })
            .collect()
    }

    /// Rumore bianco riproducibile, con un generatore lineare congruente.
    fn rumore(quanti: usize) -> Vec<f32> {
        let mut stato = 0x2545_F491_4F6C_DD1D_u64;
        (0..quanti)
            .map(|_| {
                stato = stato
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                let x = ((stato >> 33) as f32) / ((1_u64 << 31) as f32);
                (x - 1.0).clamp(-1.0, 1.0)
            })
            .collect()
    }

    fn quanti_di(ms: u64) -> usize {
        quanti_campioni(ms)
    }

    fn impronta_di(campioni: &[f32]) -> [f32; DIMENSIONI] {
        match misura(campioni) {
            Esito::Fatta(v) => *v,
            altro => panic!("attesa un'impronta, arrivato {altro:?}"),
        }
    }

    // ── la finestra ─────────────────────────────────────────────────────────

    #[test]
    fn la_finestra_si_prende_dal_centro() {
        // Un brano di quattro minuti: trenta secondi presi a metà.
        assert_eq!(inizio(240_000), 105_000);
        // Un brano più corto della finestra si legge dall'inizio.
        assert_eq!(inizio(20_000), 0);
        assert_eq!(inizio(0), 0);
        // Un brano lungo esattamente la finestra, idem.
        assert_eq!(inizio(DURATA_MS), 0);
    }

    // ── i casi che non sono un'impronta ─────────────────────────────────────

    #[test]
    fn troppo_poco_audio_si_dichiara_corto() {
        assert_eq!(misura(&[]), Esito::Corto);
        assert_eq!(misura(&seno(440.0, quanti_di(1_000), 0.5)), Esito::Corto);
        // Meno di una finestra intera.
        assert_eq!(misura(&[0.0; 100]), Esito::Corto);
    }

    #[test]
    fn il_silenzio_si_dichiara_muto() {
        assert_eq!(misura(&vec![0.0; quanti_di(DURATA_MS)]), Esito::Muto);
    }

    #[test]
    fn un_soffio_appena_udibile_non_e_silenzio() {
        let quasi: Vec<f32> = rumore(quanti_di(DURATA_MS))
            .iter()
            .map(|x| x * 0.01)
            .collect();
        assert!(matches!(misura(&quasi), Esito::Fatta(_)));
    }

    // ── la salute dei numeri ────────────────────────────────────────────────

    #[test]
    fn nessun_numero_non_finito_esce_mai() {
        for campioni in [
            seno(440.0, quanti_di(DURATA_MS), 0.5),
            rumore(quanti_di(DURATA_MS)),
            seno(440.0, quanti_di(DURATA_MS), 1.0),
            // Una continua pura: energia solo nel bin zero.
            vec![0.5; quanti_di(DURATA_MS)],
        ] {
            let v = impronta_di(&campioni);
            assert!(
                v.iter().all(|x| x.is_finite()),
                "un descrittore non finito: {v:?}"
            );
        }
    }

    #[test]
    fn l_impronta_ha_le_dimensioni_dichiarate() {
        let v = impronta_di(&rumore(quanti_di(DURATA_MS)));
        assert_eq!(v.len(), DIMENSIONI);
        // E il raggruppamento le copre tutte, senza buchi né sovrapposizioni.
        let mut atteso = 0;
        for gruppo in &RAGGRUPPAMENTO {
            assert_eq!(gruppo.inizio, atteso, "buco prima di {}", gruppo.nome);
            atteso = gruppo.fine;
        }
        assert_eq!(atteso, DIMENSIONI);
        // E i pesi sommano a uno, che è ciò che rende leggibile RIFERIMENTO.
        let somma: f32 = RAGGRUPPAMENTO.iter().map(|g| g.peso).sum();
        assert!((somma - 1.0).abs() < 1e-6, "i pesi sommano a {somma}");
    }

    // ── che la trasformata sia giusta, non solo plausibile ──────────────────

    #[test]
    fn il_rumore_e_piatto_e_un_tono_no() {
        let r = impronta_di(&rumore(quanti_di(DURATA_MS)));
        let t = impronta_di(&seno(440.0, quanti_di(DURATA_MS), 0.5));
        let piatto_rumore = r[indice::PIATTEZZA];
        let piatto_tono = t[indice::PIATTEZZA];
        assert!(
            piatto_rumore > piatto_tono * 10.0,
            "rumore {piatto_rumore}, tono {piatto_tono}"
        );
    }

    #[test]
    fn un_tono_acuto_ha_il_centroide_piu_in_alto_di_uno_grave() {
        let grave = impronta_di(&seno(200.0, quanti_di(DURATA_MS), 0.5));
        let acuto = impronta_di(&seno(4000.0, quanti_di(DURATA_MS), 0.5));
        assert!(
            acuto[indice::CENTROIDE] > grave[indice::CENTROIDE],
            "acuto {} non sopra grave {}",
            acuto[indice::CENTROIDE],
            grave[indice::CENTROIDE]
        );
        assert!(acuto[indice::ROLLOFF] > grave[indice::ROLLOFF]);
    }

    #[test]
    fn le_classi_di_altezza_si_ripetono_a_ogni_ottava() {
        let pesi = pesi_di_altezza();

        // Il bin `b` e il bin `2b` stanno a un'ottava esatta — la frequenza di
        // un bin è proporzionale al suo indice — e questo evita di far dipendere
        // la prova dall'arrotondamento di una frequenza al bin più vicino, che
        // è un'altra cosa da quella che si vuole misurare.
        for b in 20..40 {
            let (Some(basso), Some(alto)) = (
                pesi.get(b).copied().flatten(),
                pesi.get(b * 2).copied().flatten(),
            ) else {
                continue;
            };
            assert_eq!(basso.0, alto.0, "l'ottava ha cambiato classe al bin {b}");
            assert!(
                (basso.2 - alto.2).abs() < 1e-4,
                "l'ottava ha cambiato quota al bin {b}"
            );
        }

        // E ovunque: le due classi sono adiacenti in cerchio, e la quota è una
        // frazione vera — è ciò che rende continuo il croma.
        for p in pesi.iter().flatten() {
            assert_eq!((p.0 + 1) % 12, p.1, "classi non adiacenti: {p:?}");
            assert!((0.0..=1.0).contains(&p.2), "quota fuori scala: {p:?}");
        }
    }

    #[test]
    fn il_banco_mel_sta_dentro_la_banda_dichiarata() {
        let banco = banco_mel();
        assert_eq!(banco.len(), MEL_BANDE);
        for banda in &banco {
            for (bin, peso) in banda {
                let hz = frequenza_del_bin(*bin);
                assert!(
                    (MEL_BASSA_HZ * 0.5..=MEL_ALTA_HZ).contains(&hz),
                    "un bin a {hz} Hz fuori dalla banda"
                );
                assert!((0.0..=1.0).contains(peso));
            }
        }
        // Nessuna banda vuota: sarebbe un coefficiente cepstrale sempre uguale.
        assert!(banco.iter().all(|b| !b.is_empty()));
    }

    // ── l'invarianza al guadagno, che è la ragione della normalizzazione ────

    #[test]
    fn lo_stesso_brano_piu_forte_cambia_solo_il_livello() {
        let piano = impronta_di(&seno(440.0, quanti_di(DURATA_MS), 0.2));
        let forte = impronta_di(&seno(440.0, quanti_di(DURATA_MS), 0.8));

        // Il livello deve cambiare, ed è l'unico che deve.
        assert!(
            forte[indice::RMS] > piano[indice::RMS] + 5.0,
            "il livello non è salito"
        );

        // Timbro, spettro e croma no.
        for i in indice::MFCC_MEDIE..indice::RMS {
            let d = (forte[i] - piano[i]).abs();
            assert!(d < 0.05, "il descrittore {i} è cambiato di {d} col volume");
        }
        for i in indice::CROMA..DIMENSIONI {
            let d = (forte[i] - piano[i]).abs();
            assert!(d < 0.05, "il croma {i} è cambiato di {d} col volume");
        }
        // E nemmeno il fattore di cresta, che è un rapporto.
        let d = (forte[indice::CRESTA] - piano[indice::CRESTA]).abs();
        assert!(d < 0.5, "la cresta è cambiata di {d} col volume");
    }

    // ── l'invarianza per trasposizione ──────────────────────────────────────

    #[test]
    fn trasporre_di_un_semitono_non_muove_il_croma() {
        // Un accordo, non una nota sola: con una nota sola il croma è un
        // impulso, e la sua trasformata è piatta comunque.
        let quanti = quanti_di(DURATA_MS);
        let accordo = |base: f32| -> Vec<f32> {
            let a = seno(base, quanti, 0.3);
            let b = seno(base * 2.0_f32.powf(4.0 / 12.0), quanti, 0.3);
            let c = seno(base * 2.0_f32.powf(7.0 / 12.0), quanti, 0.3);
            a.iter()
                .zip(b.iter())
                .zip(c.iter())
                .map(|((x, y), z)| x + y + z)
                .collect()
        };
        let in_do = impronta_di(&accordo(523.25));
        let in_do_diesis = impronta_di(&accordo(523.25 * 2.0_f32.powf(1.0 / 12.0)));

        for i in indice::CROMA..DIMENSIONI {
            let d = (in_do[i] - in_do_diesis[i]).abs();
            assert!(
                d < 0.08,
                "il croma {i} si è mosso di {d} per una trasposizione"
            );
        }
    }

    // ── il ritmo ────────────────────────────────────────────────────────────

    #[test]
    fn senza_periodicita_il_tempo_si_dichiara_ignoto() {
        // Un tono continuo non ha attacchi: nessun battito da trovare.
        let v = impronta_di(&seno(440.0, quanti_di(DURATA_MS), 0.5));
        assert_eq!(v[indice::BATTITO], 0.0);
        assert_eq!(v[indice::TEMPO], 0.0);
    }

    #[test]
    fn un_impulso_periodico_da_il_suo_tempo() {
        // Un colpo ogni ventuno finestre: 123 battiti al minuto, cioè log2 di
        // poco più di zero. Un numero intero di finestre e non mezzo secondo
        // tondo, perché un periodo che cade a metà finestra mette alla prova
        // l'allineamento della griglia e non lo stimatore.
        let quanti = quanti_di(DURATA_MS);
        let periodo = 21 * PASSO;
        let mut campioni = vec![0.0_f32; quanti];
        let mut i = 0;
        while i < quanti {
            // Un colpo corto e pieno di armoniche.
            for j in 0..256 {
                if let Some(posto) = campioni.get_mut(i + j) {
                    let inviluppo = 1.0 - (j as f32 / 256.0);
                    *posto = inviluppo * if j % 2 == 0 { 0.8 } else { -0.8 };
                }
            }
            i += periodo;
        }
        let v = impronta_di(&campioni);
        assert!(
            v[indice::BATTITO] > 0.2,
            "battito debole: {}",
            v[indice::BATTITO]
        );
        // 123 bpm ⇒ log2(123/120) ≈ 0,036, con la tolleranza della griglia dei
        // ritardi, che a 120 bpm è di circa un quinto di ottava.
        assert!(
            v[indice::TEMPO].abs() < 0.25,
            "tempo {} lontano da 123 bpm",
            v[indice::TEMPO]
        );
    }

    #[test]
    fn la_griglia_dei_ritardi_copre_la_banda_dichiarata() {
        let per_finestra = PASSO as f32 / FREQUENZA as f32;
        let veloce = 60.0 / (RITARDO_MINIMO as f32 * per_finestra);
        let lento = 60.0 / (RITARDO_MASSIMO as f32 * per_finestra);
        assert!(
            (175.0..=190.0).contains(&veloce),
            "estremo veloce: {veloce}"
        );
        assert!((55.0..=65.0).contains(&lento), "estremo lento: {lento}");
    }

    // ── la dinamica ─────────────────────────────────────────────────────────

    #[test]
    #[expect(clippy::integer_division, reason = "si costruisce un segnale di prova")]
    fn un_brano_compresso_ha_meno_escursione_di_uno_dinamico() {
        let quanti = quanti_di(DURATA_MS);
        let costante = rumore(quanti);
        // Lo stesso rumore, ma con metà a volume molto basso.
        let dinamico: Vec<f32> = costante
            .iter()
            .enumerate()
            .map(|(i, x)| {
                if i % (quanti / 4) < (quanti / 8) {
                    *x
                } else {
                    x * 0.002
                }
            })
            .collect();
        let a = impronta_di(&costante);
        let b = impronta_di(&dinamico);
        assert!(
            b[indice::ESCURSIONE] > a[indice::ESCURSIONE] + 5.0,
            "escursione: costante {}, dinamico {}",
            a[indice::ESCURSIONE],
            b[indice::ESCURSIONE]
        );
        assert!(b[indice::SILENZIO] > a[indice::SILENZIO]);
    }

    // ── che due cose diverse siano diverse ──────────────────────────────────

    #[test]
    #[expect(clippy::integer_division, reason = "metà dei descrittori, contati")]
    fn due_segnali_diversi_non_danno_la_stessa_impronta() {
        let a = impronta_di(&seno(440.0, quanti_di(DURATA_MS), 0.5));
        let b = impronta_di(&rumore(quanti_di(DURATA_MS)));
        let diversi = a
            .iter()
            .zip(b.iter())
            .filter(|(x, y)| (*x - *y).abs() > 1e-3)
            .count();
        assert!(
            diversi > DIMENSIONI / 2,
            "solo {diversi} descrittori diversi"
        );
    }
}
