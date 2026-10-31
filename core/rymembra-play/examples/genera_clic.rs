//! I file di prova della misura acustica di «Ascolta insieme».
//!
//! ```text
//! cargo run -p rymembra-play --example genera_clic -- <cartella> <minuti>
//! cargo run -p rymembra-play --example genera_clic -- <cartella> <minuti> --banda 7000
//! cargo run -p rymembra-play --example genera_clic -- <cartella> <minuti> --ritardo 3.7
//! cargo run -p rymembra-play --example genera_clic -- <cartella> <minuti> --mono 7000 2.5
//! ```
//!
//! Tutti i file sono WAV PCM a 16 bit, uno a 44,1 kHz e uno a 48 kHz, scritti a
//! mano (l'intestazione sono 44 byte, e una dipendenza per quelli non vale la
//! pena). Le due frequenze ci sono perché i PC della misura non aprono tutti
//! l'uscita alla stessa: si suona quello che il dispositivo vuole, così il
//! ricampionatore resta fuori dalla misura.
//!
//! # Senza opzioni: i file della misura stereo
//!
//! - `clic-<frequenza>.wav`, stereo con i due canali uguali:
//!   - **raffiche** di 2 ms a 4 kHz con inviluppo di Hann, a intervalli
//!     pseudo-casuali fra 400 e 600 ms. Brevi perché il picco della
//!     correlazione sia stretto; a 4 kHz perché passano da qualunque cassa e
//!     da qualunque microfono; a intervalli irregolari perché una fila
//!     regolare è ambigua (ogni raffica somiglia alla successiva, e la
//!     correlazione non saprebbe quale appaiare: vedi
//!     `rymembra_play::sincronia::correlazione`);
//!   - un **chirp** da 200 Hz a 8 kHz in mezzo secondo ogni 10 s, per chi
//!     vuole guardare la registrazione a occhio o misurare la risposta delle
//!     casse;
//! - `seno-1khz-<frequenza>.wav`: un seno a 1 kHz, per i livelli.
//!
//! Le raffiche che cadrebbero sopra un chirp si saltano. Il generatore ha un
//! seme fisso: due esecuzioni danno gli stessi byte.
//!
//! # `--banda <Hz>`: le raffiche su un'altra portante
//!
//! Per la misura con **un microfono solo**: A suona `raffiche-4000hz`, B
//! `raffiche-7000hz`, il microfono registra la somma, e `misura_sfasamento
//! --mono 4000 7000` separa le due bande. Stesso seme, stessi istanti, stesse
//! raffiche saltate: la linea del tempo è identica, cambia solo la portante.
//!
//! Con `--banda` il chirp e il seno **non ci sono**, e il file si chiama
//! `raffiche-<Hz>hz-<frequenza>.wav`. Il chirp passa da 200 Hz a 8 kHz: in una
//! registrazione mono attraversa tutte e due le bande, e l'inviluppo di A e
//! quello di B lo vedrebbero tutti e due — sporcando proprio le finestre
//! che si vogliono pulite. Il seno non serve a quella misura. Un'opzione sola
//! che vuol dire «file per le bande» è più difficile da sbagliare di due da
//! combinare; e anche A va generato con `--banda 4000`, non con il file senza
//! opzioni, che il chirp ce l'ha.
//!
//! # `--ritardo <ms>`: la prova a circuito chiuso stereo
//!
//! Come sopra (con o senza `--banda`), ma il canale **destro** arriva `<ms>`
//! dopo, anche una frazione di campione: le raffiche sono calcolate come
//! funzioni continue del tempo, non spostate di campioni interi.
//! `misura_sfasamento` deve restituire esattamente quel numero. Il nome del
//! file porta il ritardo.
//!
//! # `--mono <Hz_B> <ms>`: la prova a circuito chiuso con un microfono solo
//!
//! Un file **mono** con la somma delle raffiche di A (alla portante di
//! `--banda`, 4 kHz se manca) e di quelle di B a `<Hz_B>`, in ritardo di
//! `<ms>`, più un fondo di rumore a −50 dBFS: quello che un microfono
//! registrerebbe se B suonasse `<ms>` dopo A. `misura_sfasamento --mono` deve
//! restituire `<ms>`.

use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};

/// Le due frequenze dei file.
const FREQUENZE: [u32; 2] = [44_100, 48_000];

/// Quanto dura una raffica, in secondi.
const RAFFICA_S: f64 = 0.002;

/// L'ampiezza delle raffiche, del chirp e del seno: −6 dBFS di picco, così
/// due file sommati dal mixer di sistema non saturano.
const AMPIEZZA: f64 = 0.5;

/// Il primo chirp e la distanza fra due, in secondi.
const CHIRP_PRIMO_S: f64 = 5.0;
const CHIRP_OGNI_S: f64 = 10.0;
/// Quanto dura un chirp, in secondi.
const CHIRP_S: f64 = 0.5;

/// Il seme del generatore: fisso, perché A e B devono avere gli stessi
/// istanti.
const SEME: u64 = 0x5EED_C11C;

/// Cosa chiede chi lancia l'esempio.
struct Richiesta {
    cartella: PathBuf,
    minuti: f64,
    banda: Option<f64>,
    ritardo_ms: f64,
    mono: Option<(f64, f64)>,
}

fn main() {
    let Some(richiesta) = leggi_argomenti() else {
        eprintln!(
            "uso: genera_clic <cartella> <minuti> [--banda <Hz>] [--ritardo <ms>] \
             [--mono <Hz_B> <ms>]"
        );
        std::process::exit(2);
    };
    if let Err(errore) = genera(&richiesta) {
        eprintln!("[errore] {errore}");
        std::process::exit(1);
    }
}

fn leggi_argomenti() -> Option<Richiesta> {
    let mut argomenti = std::env::args_os().skip(1);
    let cartella = PathBuf::from(argomenti.next()?);
    let minuti: f64 = argomenti.next()?.to_str()?.parse().ok()?;
    let mut richiesta = Richiesta {
        cartella,
        minuti,
        banda: None,
        ritardo_ms: 0.0,
        mono: None,
    };
    let numero = |a: Option<std::ffi::OsString>| -> Option<f64> {
        a?.to_str()?.parse().ok().filter(|v: &f64| v.is_finite())
    };
    while let Some(opzione) = argomenti.next() {
        match opzione.to_str()? {
            "--banda" => richiesta.banda = Some(numero(argomenti.next())?),
            "--ritardo" => richiesta.ritardo_ms = numero(argomenti.next())?,
            "--mono" => {
                let hz = numero(argomenti.next())?;
                let ms = numero(argomenti.next())?;
                richiesta.mono = Some((hz, ms));
            }
            _ => return None,
        }
    }
    (richiesta.minuti.is_finite() && richiesta.minuti > 0.0).then_some(richiesta)
}

fn genera(richiesta: &Richiesta) -> Result<(), String> {
    std::fs::create_dir_all(&richiesta.cartella)
        .map_err(|e| format!("{}: {e}", richiesta.cartella.display()))?;
    let durata = richiesta.minuti * 60.0;
    let raffiche = istanti_delle_raffiche(durata);
    let chirp = istanti_dei_chirp(durata);
    let portante = richiesta.banda.unwrap_or(4000.0);
    println!(
        "[linea del tempo] {} raffiche e {} chirp in {durata} s",
        raffiche.len(),
        chirp.len()
    );

    for frequenza in FREQUENZE {
        let nyquist = f64::from(frequenza) / 2.0;
        let fuori = |hz: f64| !(100.0..0.8 * nyquist).contains(&hz);
        if fuori(portante) || richiesta.mono.is_some_and(|(hz, _)| fuori(hz)) {
            return Err(format!(
                "a {frequenza} Hz le portanti vanno fra 100 Hz e {} Hz",
                0.8 * nyquist
            ));
        }
        let fotogrammi = fotogrammi_in(durata, frequenza)?;
        let cartella = &richiesta.cartella;

        if let Some((hz_b, ritardo_b)) = richiesta.mono {
            let nome = format!(
                "prova-mono-{}-{}-{}ms-{frequenza}.wav",
                hz_testo(portante),
                hz_testo(hz_b),
                ms_testo(ritardo_b)
            );
            let mut a = Linea::nuova(&raffiche);
            let mut b = Linea::nuova(&raffiche);
            let mut caso = Lcg(SEME ^ 0xFFFF);
            scrivi_wav(
                &cartella.join(nome),
                frequenza,
                1,
                fotogrammi,
                |t, fuori| {
                    let va = a.raffica(t, portante);
                    let vb = b.raffica(t - ritardo_b / 1000.0, hz_b);
                    // −50 dBFS efficaci.
                    let fondo = 3.16e-3 * caso.rumore();
                    if let Some(c) = fuori.first_mut() {
                        *c = 0.7 * (va + vb) + fondo;
                    }
                },
            )?;
            continue;
        }

        let con_chirp = richiesta.banda.is_none();
        let base = if con_chirp {
            "clic".to_owned()
        } else {
            format!("raffiche-{}hz", hz_testo(portante))
        };
        let nome = if richiesta.ritardo_ms == 0.0 {
            format!("{base}-{frequenza}.wav")
        } else {
            format!(
                "{base}-ritardo-{}ms-{frequenza}.wav",
                ms_testo(richiesta.ritardo_ms)
            )
        };
        let ritardo = richiesta.ritardo_ms / 1000.0;
        let mut sinistra = (Linea::nuova(&raffiche), Linea::nuova(&chirp));
        let mut destra = (Linea::nuova(&raffiche), Linea::nuova(&chirp));
        scrivi_wav(
            &cartella.join(nome),
            frequenza,
            2,
            fotogrammi,
            |t, fuori| {
                let voce = |linee: &mut (Linea, Linea), t: f64| {
                    let chirp = if con_chirp { linee.1.chirp(t) } else { 0.0 };
                    linee.0.raffica(t, portante) + chirp
                };
                let s = voce(&mut sinistra, t);
                let d = voce(&mut destra, t - ritardo);
                if let [cs, cd] = fuori {
                    *cs = s;
                    *cd = d;
                }
            },
        )?;

        if con_chirp && richiesta.ritardo_ms == 0.0 {
            let nome = format!("seno-1khz-{frequenza}.wav");
            scrivi_wav(
                &cartella.join(nome),
                frequenza,
                2,
                fotogrammi,
                |t, fuori| {
                    // Dieci millisecondi d'attacco e di coda: niente clic ai bordi.
                    let bordi = (t / 0.01).min((durata - t) / 0.01).clamp(0.0, 1.0);
                    let v = AMPIEZZA * bordi * (std::f64::consts::TAU * 1000.0 * t).sin();
                    fuori.fill(v);
                },
            )?;
        }
    }
    Ok(())
}

/// Gli istanti d'inizio delle raffiche: la prima a mezzo secondo, poi ogni
/// 400–600 ms, saltando quelle che cadrebbero sopra un chirp. Sempre gli
/// stessi, con o senza chirp, a qualunque frequenza.
fn istanti_delle_raffiche(durata: f64) -> Vec<f64> {
    let chirp = istanti_dei_chirp(durata);
    let mut caso = Lcg(SEME);
    let mut istanti = Vec::new();
    // In microsecondi interi: lo stesso istante è lo stesso numero a 44,1 e
    // a 48 kHz.
    let mut t_us: u64 = 500_000;
    while microsecondi(t_us) + RAFFICA_S < durata {
        let t = microsecondi(t_us);
        let sopra_un_chirp = chirp
            .iter()
            .any(|&c| t + RAFFICA_S > c - 0.02 && t < c + CHIRP_S + 0.02);
        if !sopra_un_chirp {
            istanti.push(t);
        }
        t_us += 400_000 + caso.prossimo() % 200_001;
    }
    istanti
}

/// Gli istanti d'inizio dei chirp.
fn istanti_dei_chirp(durata: f64) -> Vec<f64> {
    let mut istanti = Vec::new();
    let mut t = CHIRP_PRIMO_S;
    while t + CHIRP_S < durata {
        istanti.push(t);
        t += CHIRP_OGNI_S;
    }
    istanti
}

/// Microsecondi in secondi.
///
/// Passava da un `u32`, e oltre i 71 min 35 s di microsecondi restituiva
/// l'infinito: le raffiche di una traccia più lunga si fermavano lì, in
/// silenzio. La conversione diretta è esatta fino a 2^53 µs — quasi tre secoli
/// — e sotto 2^32 dà gli stessi bit di prima, quindi i file già generati
/// restano riproducibili.
#[expect(
    clippy::cast_precision_loss,
    reason = "esatta sotto 2^53 µs, cioè per ogni durata che un WAV può contenere"
)]
fn microsecondi(us: u64) -> f64 {
    us as f64 / 1e6
}

/// Una fila di eventi, letta con il tempo che va solo avanti.
struct Linea<'a> {
    inizi: &'a [f64],
    prossimo: usize,
}

impl<'a> Linea<'a> {
    fn nuova(inizi: &'a [f64]) -> Self {
        Self { inizi, prossimo: 0 }
    }

    /// Da quanto è cominciato l'evento in corso all'istante `t`, se ce n'è
    /// uno lungo `durata`.
    fn dentro(&mut self, t: f64, durata: f64) -> Option<f64> {
        while let Some(&inizio) = self.inizi.get(self.prossimo) {
            if t >= inizio + durata {
                self.prossimo += 1;
            } else {
                return (t >= inizio).then_some(t - inizio);
            }
        }
        None
    }

    /// La raffica a `portante` Hz all'istante `t`.
    fn raffica(&mut self, t: f64, portante: f64) -> f64 {
        self.dentro(t, RAFFICA_S).map_or(0.0, |tau| {
            let inviluppo = (std::f64::consts::PI * tau / RAFFICA_S).sin().powi(2);
            AMPIEZZA * inviluppo * (std::f64::consts::TAU * portante * tau).sin()
        })
    }

    /// Il chirp da 200 Hz a 8 kHz all'istante `t`, con 5 ms d'attacco e di
    /// coda.
    fn chirp(&mut self, t: f64) -> f64 {
        self.dentro(t, CHIRP_S).map_or(0.0, |tau| {
            let (da, a) = (200.0, 8000.0);
            let fase = da * tau + (a - da) * tau * tau / (2.0 * CHIRP_S);
            let bordi = (tau / 0.005).min((CHIRP_S - tau) / 0.005).clamp(0.0, 1.0);
            let bordi = (std::f64::consts::FRAC_PI_2 * bordi).sin().powi(2);
            AMPIEZZA * bordi * (std::f64::consts::TAU * fase).sin()
        })
    }
}

/// Un generatore congruenziale lineare con seme fisso.
struct Lcg(u64);

impl Lcg {
    fn prossimo(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// Circa gaussiano, varianza 1: la somma di dodici uniformi.
    fn rumore(&mut self) -> f64 {
        (0..12)
            .map(|_| {
                let v = u32::try_from(self.prossimo() & 0xFFFF_FFFF).unwrap_or(0);
                f64::from(v) / 4_294_967_296.0
            })
            .sum::<f64>()
            - 6.0
    }
}

/// Quanti fotogrammi servono per `durata` secondi.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "il valore è positivo, arrotondato e controllato contro u32::MAX prima \
              della conversione"
)]
fn fotogrammi_in(durata: f64, frequenza: u32) -> Result<u32, String> {
    let n = (durata * f64::from(frequenza)).ceil();
    if (1.0..f64::from(u32::MAX)).contains(&n) {
        Ok(n as u32)
    } else {
        Err(format!("{durata} s a {frequenza} Hz non stanno in un WAV"))
    }
}

/// Un numero di Hz per un nome di file.
fn hz_testo(hz: f64) -> String {
    format!("{hz:.0}")
}

/// Un ritardo per un nome di file: la virgola diventa una «p» (`2p5`), il
/// meno una «m».
fn ms_testo(ms: f64) -> String {
    format!("{ms}").replace('.', "p").replace('-', "m")
}

/// Scrive un WAV PCM a 16 bit. `genera` riceve l'istante in secondi e i
/// campioni del fotogramma da riempire, fra −1 e 1.
fn scrivi_wav(
    percorso: &Path,
    frequenza: u32,
    canali: u16,
    fotogrammi: u32,
    mut genera: impl FnMut(f64, &mut [f64]),
) -> Result<(), String> {
    let guasto = |e: std::io::Error| format!("{}: {e}", percorso.display());
    let blocco = u32::from(canali) * 2;
    let dati = fotogrammi
        .checked_mul(blocco)
        .filter(|d| *d <= u32::MAX - 36)
        .ok_or_else(|| format!("{}: troppo lungo per un WAV", percorso.display()))?;
    let mut fuori = BufWriter::new(File::create(percorso).map_err(guasto)?);

    let mut testa = Vec::with_capacity(44);
    testa.extend_from_slice(b"RIFF");
    testa.extend_from_slice(&(36 + dati).to_le_bytes());
    testa.extend_from_slice(b"WAVE");
    testa.extend_from_slice(b"fmt ");
    testa.extend_from_slice(&16u32.to_le_bytes());
    testa.extend_from_slice(&1u16.to_le_bytes()); // PCM intero
    testa.extend_from_slice(&canali.to_le_bytes());
    testa.extend_from_slice(&frequenza.to_le_bytes());
    testa.extend_from_slice(&(frequenza * blocco).to_le_bytes());
    testa.extend_from_slice(&u16::try_from(blocco).unwrap_or(4).to_le_bytes());
    testa.extend_from_slice(&16u16.to_le_bytes());
    testa.extend_from_slice(b"data");
    testa.extend_from_slice(&dati.to_le_bytes());
    fuori.write_all(&testa).map_err(guasto)?;

    let f = f64::from(frequenza);
    let mut fotogramma = vec![0.0f64; usize::from(canali)];
    let mut saturati = 0u64;
    for n in 0..fotogrammi {
        genera(f64::from(n) / f, &mut fotogramma);
        for v in &fotogramma {
            let (intero, saturato) = a_16_bit(*v);
            saturati += u64::from(saturato);
            fuori.write_all(&intero.to_le_bytes()).map_err(guasto)?;
        }
    }
    fuori.flush().map_err(guasto)?;
    println!(
        "[scritto] {} ({frequenza} Hz, {canali} canali, {fotogrammi} fotogrammi)",
        percorso.display()
    );
    if saturati > 0 {
        println!("[attenzione] {saturati} campioni saturati");
    }
    Ok(())
}

/// Un campione fra −1 e 1 come intero a 16 bit, e se è stato tagliato.
#[expect(
    clippy::cast_possible_truncation,
    reason = "il valore è arrotondato e tagliato all'intervallo di un i16 prima \
              della conversione"
)]
fn a_16_bit(v: f64) -> (i16, bool) {
    let scalato = (v * 32767.0).round();
    let tagliato = scalato.clamp(-32768.0, 32767.0);
    (tagliato as i16, tagliato != scalato)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn le_raffiche_arrivano_fino_in_fondo() {
        // Novanta minuti: oltre i 71 min 35 s i microsecondi non stanno più in
        // un `u32`, e la conversione restituiva l'infinito — il ciclo si
        // fermava, e gli ultimi diciotto minuti del file restavano senza
        // raffiche, senza un avviso.
        let durata = 90.0 * 60.0;
        let ultima = istanti_delle_raffiche(durata).last().copied();
        assert!(
            ultima.is_some_and(|t| t > 5_390.0),
            "l'ultima raffica cade a {ultima:?} s su {durata} s"
        );
    }

    #[test]
    fn sotto_i_settantun_minuti_gli_istanti_sono_quelli_di_prima() {
        // A e B devono avere gli stessi istanti, e due esecuzioni gli stessi
        // byte: sotto 2^32 µs la conversione nuova è identica al bit a quella
        // di prima.
        for us in [0u64, 1, 500_000, 1_234_567, 4_294_967_295] {
            let prima = u32::try_from(us).map_or(f64::INFINITY, |v| f64::from(v) / 1e6);
            assert_eq!(microsecondi(us).to_bits(), prima.to_bits(), "{us} µs");
        }
    }
}
