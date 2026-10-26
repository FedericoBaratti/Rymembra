//! Di quanto B suona dopo A, misurato su una registrazione.
//!
//! ```text
//! cargo run --release -p rymembra-play --example misura_sfasamento -- registrazione.wav
//! cargo run --release -p rymembra-play --example misura_sfasamento -- reg.flac --csv reg.csv
//! cargo run --release -p rymembra-play --example misura_sfasamento -- mic.wav --mono 4000 7000
//! cargo run --release -p rymembra-play --example misura_sfasamento -- reg.wav --mono 4000 7000 --canale 1
//! ```
//!
//! È la prova che conta: le ancore dicono quando ciascun motore *crede* di
//! suonare, la registrazione dice quando il suono arriva davvero. Si legge con
//! la strada di decodifica del motore (symphonia: WAV, FLAC e il resto),
//! aperta alla frequenza e coi canali del file, così che non ci sia nessun
//! ricampionatore in mezzo.
//!
//! # Le due misure
//!
//! - **Stereo** (senza opzioni): A nel canale sinistro e B nel destro di un
//!   ingresso di linea, tutti e due con il file `clic` di `genera_clic`. Su
//!   ogni finestra si chiama `correlazione::sfasamento(sinistro, destro)`.
//! - **Mono** (`--mono <Hz_A> <Hz_B>`): un microfono solo, con A che suona
//!   `raffiche-<Hz_A>hz` e B `raffiche-<Hz_B>hz` (`genera_clic --banda`). Su
//!   ogni finestra si chiama `correlazione::sfasamento_fra_bande`, che separa
//!   le due bande e ne correla gli inviluppi. Di un file stereo si usa un
//!   canale, il sinistro o quello di `--canale`.
//!
//! In tutte e due **positivo vuol dire B in ritardo** su A.
//!
//! # Le finestre
//!
//! Un secondo, con passo mezzo secondo: con raffiche ogni 400–600 ms ogni
//! finestra ne contiene una o due, e ogni raffica cade in due finestre. Si
//! scartano le finestre **senza raffiche** — il silenzio, il seno, un chirp
//! che occupa più di mezza finestra — e quelle che la correlazione rifiuta
//! (ambigue, o troppo diverse fra A e B). Una raffica c'è se, a blocchi di
//! 5 ms, il blocco più forte supera di 20 dB la mediana dei blocchi e sta
//! sopra una soglia assoluta: −60 dBFS sul segnale, nella misura stereo;
//! −80 sull'inviluppo di ciascuna banda, nella mono. L'inviluppo di una banda
//! stretta sta molto sotto il picco della raffica (il filtro la allarga, il
//! raddrizzamento ne prende la media), e il microfono di un portatile a un
//! paio di metri dalle casse registra piano: con −60 anche lì, raffiche sotto
//! i −45 dBFS di picco sparirebbero. A tenere fuori il fondo basta la regola
//! dei 20 dB, e la guardia della diafonia di `sfasamento_fra_bande`.
//!
//! # Cosa stampa
//!
//! Mediana, 95° percentile e massimo del valore assoluto, la tendenza (la
//! retta dei minimi quadrati sul tempo, in ms al minuto: una deriva che le
//! ancore non vedono si vede qui) e quante finestre valgono. Sono le misure
//! della tabella GO/NO-GO della Fase 0. E un CSV, una riga per finestra
//! (`inizio_s,ms,qualita,esito`), accanto alla registrazione o dove dice
//! `--csv`.
//!
//! # In release
//!
//! Ogni finestra sono duecento milioni di moltiplicazioni (vedi
//! `correlazione`): in release qualche centesimo di secondo, in una build di
//! prova qualche secondo. Per mezz'ora di registrazione, `--release`.

use std::io::Write as _;
use std::path::PathBuf;

use rymembra_play::Sorgente;
use rymembra_play::decodifica::Decodificatore;
use rymembra_play::sincronia::correlazione;

/// Quanto è lunga una finestra, in secondi.
const FINESTRA_S: f64 = 1.0;
/// Di quanto si sposta, in secondi.
const PASSO_S: f64 = 0.5;
/// Il blocco su cui si cercano le raffiche, in secondi.
const BLOCCO_S: f64 = 0.005;
/// Sotto questo valore efficace un blocco del segnale non è una raffica:
/// −60 dBFS.
const SOGLIA_SEGNALE: f64 = 1e-3;
/// Lo stesso per un blocco dell'inviluppo di una banda: −80 dBFS. Vedi la
/// documentazione del modulo.
const SOGLIA_INVILUPPO: f64 = 1e-4;

/// Cosa chiede chi lancia l'esempio.
struct Richiesta {
    file: PathBuf,
    csv: Option<PathBuf>,
    mono: Option<(f64, f64)>,
    canale: usize,
    massimo_ms: f64,
}

/// Cosa è stato di una finestra.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Esito {
    Valida(correlazione::Misura),
    SenzaRaffiche,
    Rifiutata,
}

fn main() {
    let Some(richiesta) = leggi_argomenti() else {
        eprintln!(
            "uso: misura_sfasamento <registrazione> [--mono <Hz_A> <Hz_B>] [--canale <n>] \
             [--csv <file>] [--massimo <ms>]"
        );
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
        csv: None,
        mono: None,
        canale: 0,
        massimo_ms: 50.0,
    };
    let numero = |a: Option<std::ffi::OsString>| -> Option<f64> {
        a?.to_str()?.parse().ok().filter(|v: &f64| v.is_finite())
    };
    while let Some(opzione) = argomenti.next() {
        match opzione.to_str()? {
            "--csv" => richiesta.csv = Some(PathBuf::from(argomenti.next()?)),
            "--mono" => {
                let a = numero(argomenti.next())?;
                let b = numero(argomenti.next())?;
                richiesta.mono = Some((a, b));
            }
            "--canale" => richiesta.canale = argomenti.next()?.to_str()?.parse().ok()?,
            "--massimo" => richiesta.massimo_ms = numero(argomenti.next())?,
            _ => return None,
        }
    }
    Some(richiesta)
}

/// Apre il file per il motore.
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
        segmento: None,
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
    // Una prima apertura per sapere com'è fatto il file, la seconda alla sua
    // frequenza e coi suoi canali: niente ricampionatore, niente matrice.
    let formato = Decodificatore::apri(sorgente(richiesta)?, 48_000, 2)
        .map_err(guasto)?
        .formato_suonato();
    let canali = usize::from(formato.canali.max(1));
    let frequenza = formato.frequenza;
    match richiesta.mono {
        None if canali < 2 => {
            return Err("la misura stereo vuole due canali: per un microfono solo, --mono".into());
        }
        Some(_) if richiesta.canale >= canali => {
            return Err(format!(
                "il file ha {canali} canali, non c'è il {}",
                richiesta.canale
            ));
        }
        _ => {}
    }
    println!(
        "[registrazione] {} Hz, {canali} canali — misura {}",
        frequenza,
        match richiesta.mono {
            Some((a, b)) => format!(
                "mono, bande {a} Hz (A) e {b} Hz (B), canale {}",
                richiesta.canale
            ),
            None => "stereo, A a sinistra e B a destra".to_owned(),
        }
    );
    let mut d =
        Decodificatore::apri(sorgente(richiesta)?, frequenza, formato.canali).map_err(guasto)?;

    let f = f64::from(frequenza);
    let finestra = campioni(FINESTRA_S * f);
    let passo = campioni(PASSO_S * f).max(1);
    let mut primo = Vec::new();
    let mut secondo = Vec::new();
    let mut inizio = 0usize;
    let mut finestre: Vec<(f64, Esito)> = Vec::new();
    let mut blocco = Vec::new();
    let mut ancora = true;
    while ancora {
        ancora = d.prossimo(&mut blocco).map_err(guasto)?;
        for fotogramma in blocco.chunks_exact(canali) {
            match richiesta.mono {
                Some(_) => primo.push(fotogramma.get(richiesta.canale).copied().unwrap_or(0.0)),
                None => {
                    primo.push(fotogramma.first().copied().unwrap_or(0.0));
                    secondo.push(fotogramma.get(1).copied().unwrap_or(0.0));
                }
            }
        }
        while primo.len() >= finestra {
            let a = primo.get(..finestra).unwrap_or_default();
            let b = secondo.get(..finestra).unwrap_or_default();
            let esito = valuta(a, b, frequenza, richiesta);
            finestre.push((conta(inizio) / f, esito));
            primo.drain(..passo.min(primo.len()));
            secondo.drain(..passo.min(secondo.len()));
            inizio += passo;
        }
    }

    scrivi_csv(richiesta, &finestre)?;
    riassumi(&finestre);
    Ok(())
}

/// Una finestra: se ha raffiche, e quanto vale.
fn valuta(a: &[f32], b: &[f32], frequenza: u32, richiesta: &Richiesta) -> Esito {
    let misura = match richiesta.mono {
        Some((hz_a, hz_b)) => {
            let bande = (
                correlazione::inviluppo(a, frequenza, hz_a),
                correlazione::inviluppo(a, frequenza, hz_b),
            );
            let (Some(banda_a), Some(banda_b)) = bande else {
                return Esito::Rifiutata;
            };
            if !ha_raffiche(&banda_a, frequenza, SOGLIA_INVILUPPO)
                || !ha_raffiche(&banda_b, frequenza, SOGLIA_INVILUPPO)
            {
                return Esito::SenzaRaffiche;
            }
            correlazione::sfasamento_fra_bande(a, frequenza, hz_a, hz_b, richiesta.massimo_ms)
        }
        None => {
            if !ha_raffiche(a, frequenza, SOGLIA_SEGNALE)
                || !ha_raffiche(b, frequenza, SOGLIA_SEGNALE)
            {
                return Esito::SenzaRaffiche;
            }
            correlazione::sfasamento(a, b, frequenza, richiesta.massimo_ms)
        }
    };
    misura.map_or(Esito::Rifiutata, Esito::Valida)
}

/// Se nella finestra c'è almeno una raffica: a blocchi di 5 ms, il blocco più
/// forte supera di 20 dB la mediana e sta sopra `soglia` (valore efficace).
fn ha_raffiche(x: &[f32], frequenza: u32, soglia: f64) -> bool {
    let lungo = campioni(BLOCCO_S * f64::from(frequenza)).max(1);
    let mut livelli: Vec<f64> = x
        .chunks(lungo)
        .map(|b| {
            let energia: f64 = b.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            (energia / conta(b.len().max(1))).sqrt()
        })
        .collect();
    livelli.sort_by(f64::total_cmp);
    let (Some(&massimo), Some(&mediana)) = (livelli.last(), livelli.get(livelli.len() >> 1)) else {
        return false;
    };
    massimo > soglia && massimo > 10.0 * mediana
}

fn scrivi_csv(richiesta: &Richiesta, finestre: &[(f64, Esito)]) -> Result<(), String> {
    let percorso = richiesta.csv.clone().unwrap_or_else(|| {
        let mut nome = richiesta.file.clone().into_os_string();
        nome.push(".sfasamento.csv");
        PathBuf::from(nome)
    });
    let guasto = |e: std::io::Error| format!("{}: {e}", percorso.display());
    let mut fuori = std::io::BufWriter::new(std::fs::File::create(&percorso).map_err(guasto)?);
    writeln!(fuori, "inizio_s,ms,qualita,esito").map_err(guasto)?;
    for (inizio, esito) in finestre {
        match esito {
            Esito::Valida(m) => writeln!(fuori, "{inizio:.3},{:.4},{:.4},valida", m.ms, m.qualita),
            Esito::SenzaRaffiche => writeln!(fuori, "{inizio:.3},,,senza_raffiche"),
            Esito::Rifiutata => writeln!(fuori, "{inizio:.3},,,rifiutata"),
        }
        .map_err(guasto)?;
    }
    fuori.flush().map_err(guasto)?;
    println!("[csv] {}", percorso.display());
    Ok(())
}

/// Le misure della tabella GO/NO-GO.
fn riassumi(finestre: &[(f64, Esito)]) {
    let valide: Vec<(f64, f64, f32)> = finestre
        .iter()
        .filter_map(|(t, e)| match e {
            Esito::Valida(m) => Some((*t + FINESTRA_S / 2.0, m.ms, m.qualita)),
            _ => None,
        })
        .collect();
    let senza = finestre
        .iter()
        .filter(|(_, e)| *e == Esito::SenzaRaffiche)
        .count();
    let rifiutate = finestre
        .iter()
        .filter(|(_, e)| *e == Esito::Rifiutata)
        .count();
    println!(
        "[finestre] {} valide su {} ({senza} senza raffiche, {rifiutate} rifiutate)",
        valide.len(),
        finestre.len()
    );
    if valide.is_empty() {
        println!("[risultato] nessuna misura");
        return;
    }
    let mut ms: Vec<f64> = valide.iter().map(|v| v.1).collect();
    ms.sort_by(f64::total_cmp);
    let mut assoluti: Vec<f64> = ms.iter().map(|v| v.abs()).collect();
    assoluti.sort_by(f64::total_cmp);
    let mediana = quantile(&ms, 0.5);
    let p95 = quantile(&assoluti, 0.95);
    let massimo = assoluti.last().copied().unwrap_or(0.0);
    let qualita = quantile(
        &{
            let mut q: Vec<f64> = valide.iter().map(|v| f64::from(v.2)).collect();
            q.sort_by(f64::total_cmp);
            q
        },
        0.5,
    );
    println!("[mediana] {mediana:+.3} ms (positivo: B in ritardo su A)");
    println!("[p95 |·|] {p95:.3} ms");
    println!("[massimo |·|] {massimo:.3} ms");
    match tendenza(&valide) {
        Some(pendenza) => println!("[tendenza] {pendenza:+.4} ms/min"),
        None => println!("[tendenza] —"),
    }
    println!("[qualità mediana] {qualita:.3}");
}

/// Il quantile `q` di valori già ordinati, con interpolazione lineare.
fn quantile(ordinati: &[f64], q: f64) -> f64 {
    let ultimo = conta(ordinati.len().saturating_sub(1));
    let posto = (q.clamp(0.0, 1.0) * ultimo).max(0.0);
    let sotto = campioni(posto.floor());
    let sopra = campioni(posto.ceil());
    let (Some(&a), Some(&b)) = (ordinati.get(sotto), ordinati.get(sopra)) else {
        return 0.0;
    };
    a + (b - a) * (posto - posto.floor())
}

/// La pendenza della retta dei minimi quadrati di ms sul tempo, in ms al
/// minuto. `None` con meno di due punti o tutti nello stesso istante.
fn tendenza(valide: &[(f64, f64, f32)]) -> Option<f64> {
    let n = conta(valide.len());
    if n < 2.0 {
        return None;
    }
    let tm = valide.iter().map(|v| v.0).sum::<f64>() / n;
    let ym = valide.iter().map(|v| v.1).sum::<f64>() / n;
    let sxx: f64 = valide.iter().map(|v| (v.0 - tm) * (v.0 - tm)).sum();
    let sxy: f64 = valide.iter().map(|v| (v.0 - tm) * (v.1 - ym)).sum();
    (sxx > 0.0).then(|| sxy / sxx * 60.0)
}

/// Un conteggio come numero con la virgola.
fn conta(n: usize) -> f64 {
    u32::try_from(n).map_or(f64::from(u32::MAX), f64::from)
}

/// Un numero di campioni, arrotondato.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "il valore è arrotondato e limitato a [0, u32::MAX] prima della conversione"
)]
fn campioni(x: f64) -> usize {
    let limitato = x.round().clamp(0.0, f64::from(u32::MAX));
    usize::try_from(limitato as u32).unwrap_or(0)
}
