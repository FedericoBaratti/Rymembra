//! L'impronta MD5 del PCM che il motore ricava da un file, da confrontare con
//! quella di ffmpeg.
//!
//! ```text
//! cargo run --release -p rymembra-play --example ape_md5 -- disco.ape
//! cargo run --release -p rymembra-play --example ape_md5 -- disco.ape --da 61230
//! cargo run --release -p rymembra-play --example ape_md5 -- disco.ape --cd 4500 9120
//! cargo run --release -p rymembra-play --example ape_md5 -- disco.ape --cd 9120 -
//! ```
//!
//! # Perché esiste
//!
//! Perché l'MD5 scritto dentro un `.ape` riguarda i byte del file, non l'audio
//! decodificato: che il file sia intatto non dice niente su chi lo decodifica.
//! La prova vera è il PCM. Qui esce dalla strada intera del motore — il
//! riconoscitore, il lettore di Monkey's Audio, il decodificatore PCM di
//! symphonia, la matrice dei canali — aperta alla frequenza e coi canali del
//! file, così che non ci sia un ricampionatore in mezzo e i campioni tornino
//! interi senza perdere niente; e si confronta con quello di ffmpeg:
//!
//! ```text
//! ffmpeg -v error -i disco.ape -c:a pcm_s16le -f md5 -
//! ffmpeg -v error -i disco.ape -af atrim=start_sample=N -c:a pcm_s16le -f md5 -
//! ffmpeg -v error -i disco.ape -af atrim=start_sample=N:end_sample=M -c:a pcm_s16le -f md5 -
//! ```
//!
//! (`pcm_s24le` per un file a 24 bit, che ffmpeg altrimenti decodifica a 32;
//! `pcm_u8` a 8.)
//!
//! - `--da MS` salta prima di cominciare, come la ripresa di un brano: con
//!   `MS` multiplo di dieci, a 44,1 kHz il campione è `MS × 44,1` tondo;
//! - `--cd INIZIO FINE` suona solo il pezzo fra due punti di un `.cue`, in
//!   fotogrammi CD (`-` per «fino alla fine»): è la strada di una traccia
//!   d'immagine, e a 44,1 kHz il pezzo va dal campione `INIZIO × 588` a
//!   `FINE × 588`. Con tutti e due, il salto conta dall'inizio del pezzo.
//!
//! Tutto a flusso: un blocco alla volta dentro l'MD5, mai il file intero in
//! memoria.

use std::path::PathBuf;
use std::time::Instant;

use md5::{Digest, Md5};
use rymembra_play::decodifica::Decodificatore;
use rymembra_play::{Segmento, Sorgente};

/// Cosa chiede chi lancia l'esempio.
struct Richiesta {
    file: PathBuf,
    da_ms: Option<u64>,
    segmento: Option<(u32, Option<u32>)>,
}

fn main() {
    let Some(richiesta) = leggi_argomenti() else {
        eprintln!("uso: ape_md5 <file> [--da <ms>] [--cd <inizio> <fine|->]");
        std::process::exit(2);
    };
    if let Err(errore) = misura(&richiesta) {
        eprintln!("[errore] {errore}");
        std::process::exit(1);
    }
}

fn leggi_argomenti() -> Option<Richiesta> {
    let mut argomenti = std::env::args_os().skip(1);
    let file = PathBuf::from(argomenti.next()?);
    let mut richiesta = Richiesta {
        file,
        da_ms: None,
        segmento: None,
    };
    while let Some(opzione) = argomenti.next() {
        match opzione.to_str()? {
            "--da" => richiesta.da_ms = Some(argomenti.next()?.to_str()?.parse().ok()?),
            "--cd" => {
                let inizio = argomenti.next()?.to_str()?.parse().ok()?;
                let fine = match argomenti.next()?.to_str()? {
                    "-" => None,
                    numero => Some(numero.parse().ok()?),
                };
                richiesta.segmento = Some((inizio, fine));
            }
            _ => return None,
        }
    }
    Some(richiesta)
}

/// Apre il file per il motore, col pezzo chiesto.
fn sorgente(richiesta: &Richiesta) -> Result<Sorgente, String> {
    let file = std::fs::File::open(&richiesta.file)
        .map_err(|err| format!("{}: {err}", richiesta.file.display()))?;
    Ok(Sorgente {
        track_id: 0,
        media: Box::new(file),
        estensione: richiesta
            .file
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_owned),
        durata_ms: 0,
        replaygain_db: None,
        picco: None,
        segmento: richiesta.segmento.map(|(inizio_cd, fine_cd)| Segmento {
            immagine: richiesta.file.display().to_string(),
            inizio_cd,
            fine_cd,
        }),
    })
}

fn misura(richiesta: &Richiesta) -> Result<(), String> {
    let guasto = |err: rymembra_domain::errors::AppError| {
        format!(
            "{} — {}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        )
    };
    // Una prima apertura per sapere com'è fatto il file, e la seconda alla
    // sua frequenza e coi suoi canali: niente ricampionatore, matrice
    // identità, e i campioni tornano interi.
    let formato = Decodificatore::apri(sorgente(richiesta)?, 48_000, 2)
        .map_err(guasto)?
        .formato_suonato();
    let bit = formato
        .bit
        .ok_or("il formato non dichiara i bit: non è senza perdita")?;
    println!(
        "[formato] {} {} Hz, {} bit, {} canali",
        formato.codec.unwrap_or("?"),
        formato.frequenza,
        bit,
        formato.canali
    );
    if bit == 32 {
        println!("[attenzione] a 32 bit il passaggio per f32 perde gli ultimi bit");
    }

    let mut d = Decodificatore::apri(sorgente(richiesta)?, formato.frequenza, formato.canali)
        .map_err(guasto)?;
    if let Some(ms) = richiesta.da_ms {
        d.cerca(ms).map_err(guasto)?;
        println!(
            "[salto] a {ms} ms, cioè al fotogramma {} del brano",
            d.consegnati()
        );
    }

    let partito = Instant::now();
    let mut impronta = Md5::new();
    let mut blocco = Vec::new();
    let mut byte = Vec::new();
    let mut fotogrammi: u64 = 0;
    while d.prossimo(&mut blocco).map_err(guasto)? {
        byte.clear();
        for campione in &blocco {
            scrivi(*campione, bit, &mut byte);
        }
        impronta.update(&byte);
        let usciti = blocco
            .chunks_exact(usize::from(formato.canali.max(1)))
            .count();
        fotogrammi = fotogrammi.saturating_add(u64::try_from(usciti).unwrap_or(0));
    }
    let fatto = impronta.finalize();
    let esadecimale: String = fatto.iter().map(|b| format!("{b:02x}")).collect();
    println!("[fotogrammi] {fotogrammi}");
    println!("[md5] {esadecimale}");
    println!("[tempo] {:.1} s", partito.elapsed().as_secs_f64());
    Ok(())
}

/// Un campione del motore, riportato all'intero che era nel file, in coda a
/// `byte` come lo scrive ffmpeg: little-endian, con segno — tranne a 8 bit,
/// dove è senza segno con lo zero a 128.
///
/// Il motore consegna `intero / 2^(bit-1)`, che per 16 e 24 bit sta esatto in
/// un `f32`: moltiplicare e arrotondare lo riporta indietro senza errori.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "il valore è arrotondato e tagliato all'intervallo dei bit del \
              file prima della conversione: per costruzione ci sta"
)]
fn scrivi(campione: f32, bit: u32, byte: &mut Vec<u8>) {
    let scala = f64::from(1_u32 << (bit.clamp(8, 32) - 1));
    let intero = (f64::from(campione) * scala)
        .round()
        .clamp(-scala, scala - 1.0) as i64;
    match bit {
        8 => byte.push((intero + 128) as u8),
        16 => byte.extend_from_slice(&(intero as i16).to_le_bytes()),
        24 => {
            let [basso, medio, alto, _] = (intero as i32).to_le_bytes();
            byte.extend_from_slice(&[basso, medio, alto]);
        }
        _ => byte.extend_from_slice(&(intero as i32).to_le_bytes()),
    }
}
