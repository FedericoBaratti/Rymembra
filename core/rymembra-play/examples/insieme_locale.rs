//! Due motori nello stesso processo, su due uscite: A suona per conto suo, B lo
//! segue col regolatore. È il banco della Fase 0 di «Ascolta insieme in LAN»:
//! le primitive di tempo del motore e il regolatore, misurati senza rete.
//!
//! # Come si lancia
//!
//! Prima le uscite che ci sono, per copiarne il nome esatto:
//!
//! ```text
//! cargo run -p rymembra-play --release --example insieme_locale -- --elenco
//! ```
//!
//! Poi il banco. Su questo PC, A sugli altoparlanti e B sulle cuffie, con le
//! due tracce di clic di `genera_clic` — a 4 kHz per A, la copia a 7 kHz
//! (`--banda 7000`) per B — così un microfono solo, mono, le sente tutte e due
//! e la misura le separa per banda:
//!
//! ```text
//! cargo run -p rymembra-play --release --example insieme_locale -- \
//!     --a "Altoparlanti (Senary Audio)" --b "Cuffie (WH-CH720N)" \
//!     --csv insieme.csv \
//!     clic-4k.wav --file-b clic-7k.wav
//! ```
//!
//! Con più brani, una coda: i file di A in fila, e per B un `--file-b` per
//! ciascuno, nello stesso ordine.
//!
//! ```text
//! … -- --a "Altoparlanti (Senary Audio)" --b "Cuffie (WH-CH720N)" \
//!     uno-4k.wav due-4k.wav --file-b uno-7k.wav --file-b due-7k.wav
//! ```
//!
//! Senza `--file-b` B suona gli stessi file di A. Senza `--a` o `--b` quel
//! motore apre l'uscita predefinita; un nome che non c'è **ferma** la misura,
//! invece di ripiegare in silenzio sulla predefinita come fa l'applicazione.
//!
//! `--latenza-a <ms>` e `--latenza-b <ms>` danno a ciascun motore la sua
//! latenza a mano (`Motore::latenza`): è la taratura di un'uscita Bluetooth,
//! che la scheda non dichiara. `--giri <n>` recita il copione n volte di fila:
//! sei giri fanno la mezz'ora delle soglie della Fase 0.
//!
//! # Cosa fa
//!
//! A suona la coda, e la coda gira su sé stessa finché il copione non finisce.
//! B entra in sessione come entrerebbe un PC vero — il regolatore vede che non
//! suona niente, chiede di riaprire, e l'ingresso è `apri_fermo` più
//! `riprendi_a` — e da lì lo segue. Il copione muove soltanto A: cinque pause
//! e riprese, cinque salti, gradini forzati di ±30 ms, una deriva forzata di
//! +200 e poi −200 ppm per sessanta secondi l'una, e tre cambi di brano senza
//! buco (se i file sono almeno due). Ogni disturbo sta su A, e B lo deve
//! inseguire da solo: è il regolatore che si misura, non il disturbo.
//!
//! Le due ancore si confrontano sullo stesso orologio — il processo è uno — e
//! l'**etichetta** è la posizione della voce nel copione, non il file: A e B
//! suonano byte diversi della stessa linea del tempo, e il regolatore
//! confronta le ancore lo stesso.
//!
//! # Il CSV
//!
//! Venti righe al secondo:
//!
//! ```text
//! t_ns, etichetta_a, pos_a_us, udito_a_ns, etichetta_b, pos_b_us, udito_b_ns,
//! errore_us, mossa, ppm, gradini, vuoti_a, vuoti_b, xrun_a, xrun_b,
//! controprova_a_ns, controprova_b_ns
//! ```
//!
//! `errore_us` è B meno A nello stesso istante d'ascolto, positivo se B è
//! avanti; vuoto quando non c'è un'ancora per tutti e due o i brani non
//! coincidono. `mossa` è quel che il regolatore ha chiesto a B in quella riga,
//! `ppm` e `gradini` le correzioni di B. Le controprove sono gli stessi istanti
//! d'ascolto ricavati con l'orologio del processo invece che col ponte:
//! diagnostica del ponte, non della sincronia.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rymembra_play::decodifica::Decodificatore;
use rymembra_play::sincronia::regolatore::{DaMe, DallOspite, Mossa, Regolatore, Taratura};
use rymembra_play::{Ancora, BranoAperto, Evento, FormatoUscita, Motore, Sorgente, tempo};

/// Venti righe al secondo: il passo del CSV e del regolatore.
const PASSO: Duration = Duration::from_millis(50);

/// Quanto prima della fine un salto «verso la fine» porta A: abbastanza per
/// vedere B allineato prima del cambio di brano.
const PRIMA_DELLA_FINE_MS: u64 = 8_000;

/// Un passo del copione: cosa si fa ad A, e quando.
#[derive(Debug, Clone, Copy)]
enum Azione {
    Pausa,
    Riprendi,
    /// Un salto di tanti millisecondi rispetto a dove A si trova.
    Salta(i64),
    /// Un salto a [`PRIMA_DELLA_FINE_MS`] dalla fine del brano, perché il
    /// cambio di brano arrivi da solo, senza buco.
    VersoLaFine,
    /// Un gradino forzato su A, in millisecondi: positivo silenzio, negativo
    /// salto.
    Gradino(i64),
    /// Una deriva forzata su A, in parti per milione.
    Deriva(i32),
    Fine,
}

/// Il copione: secondi dall'inizio e azione.
const COPIONE: &[(u64, Azione)] = &[
    (12, Azione::Pausa),
    (15, Azione::Riprendi),
    (22, Azione::Pausa),
    (26, Azione::Riprendi),
    (32, Azione::Pausa),
    (33, Azione::Riprendi),
    (40, Azione::Pausa),
    (48, Azione::Riprendi),
    (55, Azione::Pausa),
    (57, Azione::Riprendi),
    (65, Azione::Salta(20_000)),
    (75, Azione::Salta(-10_000)),
    (85, Azione::Salta(5_000)),
    (95, Azione::Salta(-30_000)),
    (105, Azione::Salta(40_000)),
    (115, Azione::Gradino(30)),
    (125, Azione::Gradino(-30)),
    (135, Azione::Gradino(30)),
    (145, Azione::Gradino(-30)),
    (155, Azione::Deriva(200)),
    (215, Azione::Deriva(-200)),
    (275, Azione::Deriva(0)),
    (280, Azione::VersoLaFine),
    (300, Azione::VersoLaFine),
    (320, Azione::VersoLaFine),
    (340, Azione::Fine),
];

/// Un file della coda, con la sua durata se il contenitore la dichiara.
struct Voce {
    percorso: PathBuf,
    durata_ms: u64,
}

/// Uno dei due motori, con la sua coda e i suoi annunci.
struct Lato {
    nome: &'static str,
    motore: Motore,
    eventi: mpsc::Receiver<Evento>,
    coda: Vec<Voce>,
}

impl Lato {
    /// Apre la voce `etichetta` della coda — che gira su sé stessa — per questo
    /// motore.
    fn apri(&self, etichetta: u64) -> Option<BranoAperto> {
        let quante = u64::try_from(self.coda.len()).ok()?;
        let posto = usize::try_from(etichetta.checked_rem(quante)?).ok()?;
        let voce = self.coda.get(posto)?;
        let sorgente = sorgente(&voce.percorso, etichetta, voce.durata_ms)?;
        match BranoAperto::apri(sorgente, self.motore.formato()) {
            Ok(brano) => Some(brano.con_etichetta(etichetta)),
            Err(err) => {
                eprintln!(
                    "[{}] {}: {}",
                    self.nome,
                    voce.percorso.display(),
                    err.code().kind().code()
                );
                None
            }
        }
    }

    /// La durata della voce `etichetta`, se si sa.
    fn durata_ms(&self, etichetta: u64) -> Option<u64> {
        let quante = u64::try_from(self.coda.len()).ok()?;
        let posto = usize::try_from(etichetta.checked_rem(quante)?).ok()?;
        self.coda.get(posto).map(|v| v.durata_ms).filter(|d| *d > 0)
    }

    /// Raccoglie gli annunci: a ogni brano che comincia si prepara il dopo,
    /// che è tutto il gapless.
    fn ascolta(&self) {
        while let Ok(evento) = self.eventi.try_recv() {
            match evento {
                Evento::Iniziato { track_id } => {
                    let Ok(etichetta) = u64::try_from(track_id) else {
                        continue;
                    };
                    self.motore.prepara(self.apri(etichetta.saturating_add(1)));
                }
                Evento::Fermato => println!("[{}] fermo", self.nome),
                Evento::Errore(err) => eprintln!(
                    "[{}] errore {} — {}",
                    self.nome,
                    err.code().kind().code(),
                    err.cause().unwrap_or("—")
                ),
            }
        }
    }

    /// L'ancora da dare al regolatore: quella della musica in corso, o in
    /// pausa il punto da cui si ripartirà.
    ///
    /// `None` quando la musica è voluta ma l'ultima ancora non è «in corso»:
    /// è un blocco che sfumava verso un salto, e detta al regolatore
    /// sembrerebbe una pausa.
    fn stato(&self) -> Option<(Option<u64>, u64, u64, bool)> {
        if self.motore.posizione().in_pausa {
            let (etichetta, posizione_us) = self.motore.in_attesa()?;
            return Some((etichetta, posizione_us, 0, false));
        }
        let ancora = self.motore.ancora().filter(|a| a.in_corso)?;
        Some((ancora.etichetta, ancora.posizione_us, ancora.udito_ns, true))
    }
}

/// Una sorgente da un file, con l'etichetta come identificativo: gli annunci
/// dicono così quale voce della coda è cominciata.
fn sorgente(percorso: &Path, etichetta: u64, durata_ms: u64) -> Option<Sorgente> {
    let file = match std::fs::File::open(percorso) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("[errore] {}: {err}", percorso.display());
            return None;
        }
    };
    Some(Sorgente {
        track_id: i64::try_from(etichetta).unwrap_or(i64::MAX),
        media: Box::new(file),
        estensione: percorso
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_owned),
        durata_ms,
        replaygain_db: None,
        picco: None,
        segmento: None,
    })
}

/// La durata di un file, se il contenitore la dichiara: serve ai salti verso
/// la fine. Si apre un decodificatore e si chiede, senza decodificare niente.
fn durata_di(percorso: &Path) -> u64 {
    sorgente(percorso, 0, 0)
        .and_then(|s| Decodificatore::apri(s, 48_000, 2).ok())
        .and_then(|d| d.fine_flusso_ms())
        .unwrap_or(0)
}

/// Gli argomenti, già divisi.
struct Argomenti {
    uscita_a: Option<String>,
    uscita_b: Option<String>,
    csv: PathBuf,
    file_a: Vec<PathBuf>,
    file_b: Vec<PathBuf>,
    /// La latenza a mano di ciascun lato, in ms: `Motore::latenza`.
    latenza_a: i64,
    latenza_b: i64,
    /// Quante volte si recita il copione di fila.
    giri: u32,
}

fn argomenti() -> Result<Option<Argomenti>, String> {
    let mut argomenti = Argomenti {
        uscita_a: None,
        uscita_b: None,
        csv: PathBuf::from("insieme_locale.csv"),
        file_a: Vec::new(),
        file_b: Vec::new(),
        latenza_a: 0,
        latenza_b: 0,
        giri: 1,
    };
    let mut resto = std::env::args_os().skip(1);
    while let Some(arg) = resto.next() {
        let testo = arg.to_string_lossy().into_owned();
        let mut valore = |chi: &str| resto.next().ok_or_else(|| format!("{chi} vuole un valore"));
        match testo.as_str() {
            "--elenco" => return Ok(None),
            "--a" => argomenti.uscita_a = Some(valore("--a")?.to_string_lossy().into_owned()),
            "--b" => argomenti.uscita_b = Some(valore("--b")?.to_string_lossy().into_owned()),
            "--csv" => argomenti.csv = PathBuf::from(valore("--csv")?),
            "--file-b" => argomenti.file_b.push(PathBuf::from(valore("--file-b")?)),
            "--latenza-a" => argomenti.latenza_a = numero("--latenza-a", valore("--latenza-a")?)?,
            "--latenza-b" => argomenti.latenza_b = numero("--latenza-b", valore("--latenza-b")?)?,
            "--giri" => {
                argomenti.giri = numero("--giri", valore("--giri")?)?;
                if argomenti.giri == 0 {
                    return Err("--giri vuole almeno 1".to_owned());
                }
            }
            altro if altro.starts_with("--") => {
                return Err(format!("argomento sconosciuto: {altro}"));
            }
            _ => argomenti.file_a.push(PathBuf::from(arg)),
        }
    }
    if argomenti.file_a.is_empty() {
        return Err("serve almeno un file da suonare".to_owned());
    }
    if argomenti.file_b.is_empty() {
        argomenti.file_b = argomenti.file_a.clone();
    } else if argomenti.file_b.len() != argomenti.file_a.len() {
        return Err(format!(
            "{} file per A e {} --file-b per B: ne serve uno per ciascuno",
            argomenti.file_a.len(),
            argomenti.file_b.len()
        ));
    }
    Ok(Some(argomenti))
}

/// Un numero da riga di comando, col nome dell'opzione nell'errore.
fn numero<T: std::str::FromStr>(chi: &str, testo: std::ffi::OsString) -> Result<T, String> {
    testo
        .to_string_lossy()
        .trim()
        .parse()
        .map_err(|_| format!("{chi} vuole un numero"))
}

fn elenco() {
    for uscita in rymembra_play::dispositivi::elenco() {
        let segno = if uscita.predefinito {
            "  (predefinita)"
        } else {
            ""
        };
        println!("{}{segno}", uscita.id);
    }
}

/// Apre un motore su un'uscita, con gli annunci su un canale.
fn lato(
    nome: &'static str,
    uscita: Option<String>,
    latenza_ms: i64,
    file: &[PathBuf],
) -> Option<Lato> {
    let (manda, eventi) = mpsc::channel();
    let voluta = uscita.clone();
    let motore = match rymembra_play::avvia(
        move |evento| {
            let _ = manda.send(evento);
        },
        uscita,
    ) {
        Ok(m) => m,
        Err(err) => {
            eprintln!(
                "[{nome}] il motore non si è avviato: {} — {}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            return None;
        }
    };
    // Un'uscita chiesta per nome e non trovata ripiega in silenzio sulla
    // predefinita — è quel che vuole l'applicazione, ed è il guasto peggiore
    // per una misura: con le cuffie spente A e B finirebbero sulla stessa
    // scheda, e due motori su un orologio solo vanno d'accordo per forza. Qui
    // ci si ferma.
    if let Some(voluta) = voluta.filter(|v| v != motore.dispositivo()) {
        eprintln!(
            "[{nome}] chiesta «{voluta}», aperta «{}»: l'uscita chiesta non c'è (spenta,              scollegata?). Misura annullata.",
            motore.dispositivo()
        );
        return None;
    }
    motore.latenza(latenza_ms);
    let FormatoUscita { frequenza, canali } = motore.formato();
    println!(
        "[{nome}] {} — {frequenza} Hz, {canali} canali, latenza a mano {latenza_ms} ms, ponte {:?}",
        motore.dispositivo(),
        motore.ponte()
    );
    // La sessione accesa: il flusso non va mai a riposo, ed è la condizione
    // di `riprendi_a`.
    motore.tieni_sveglio(true);
    let coda = file
        .iter()
        .map(|percorso| Voce {
            percorso: percorso.clone(),
            durata_ms: durata_di(percorso),
        })
        .collect();
    Some(Lato {
        nome,
        motore,
        eventi,
        coda,
    })
}

/// B meno A nello stesso istante d'ascolto, in microsecondi: positivo se B è
/// avanti. Solo se tutti e due suonano lo stesso brano.
fn errore_us(a: Option<&Ancora>, b: Option<&Ancora>) -> Option<i64> {
    let (a, b) = (a?, b?);
    if !a.in_corso || !b.in_corso || a.etichetta != b.etichetta {
        return None;
    }
    let posizione_a = i128::from(a.posizione_us) * 1_000;
    let posizione_b = i128::from(b.posizione_us) * 1_000;
    let errore_ns = posizione_b - (posizione_a + i128::from(b.udito_ns) - i128::from(a.udito_ns));
    i64::try_from(errore_ns.checked_div(1_000)?).ok()
}

/// Esegue su B quel che il regolatore chiede, e lo dice in una parola per il
/// CSV.
fn esegui(b: &Lato, mossa: Mossa) -> String {
    let frequenza = b.motore.formato().frequenza;
    match mossa {
        Mossa::Niente => String::new(),
        Mossa::Riapri {
            etichetta,
            posizione_us,
            quando_ns,
        } => {
            riapri(b, etichetta, posizione_us, quando_ns);
            format!("riapri:{etichetta}:{posizione_us}")
        }
        // Le convenzioni di segno del regolatore sono già quelle del motore:
        // vedi `Mossa::Gradino` e `Mossa::Deriva`.
        Mossa::Gradino(fotogrammi) => {
            b.motore.gradino(fotogrammi);
            format!("gradino:{fotogrammi}")
        }
        Mossa::Deriva(ppm) => {
            b.motore.deriva(ppm);
            format!("deriva:{ppm}")
        }
        Mossa::Pausa => {
            b.motore.pausa();
            "pausa".to_owned()
        }
        // Il banco suona sempre la stessa voce: mettersi al punto, fermi.
        Mossa::Posiziona { posizione_us, .. } => {
            b.motore.vai_a(posizione_us.checked_div(1000).unwrap_or(0));
            format!("posiziona:{posizione_us}")
        }
        Mossa::AvviaA { quando_ns, salta } => {
            // Come lo vuole `Mossa::AvviaA`: il fotogramma in attesa va
            // sentito `salta` fotogrammi prima di `quando`, e se quell'istante
            // è passato il motore salta da sé.
            let anticipo = (i128::from(salta) * 1_000_000_000)
                .checked_div(i128::from(frequenza))
                .unwrap_or(0);
            let avvio = u64::try_from((i128::from(quando_ns) - anticipo).max(1)).unwrap_or(1);
            if let Err(rifiuto) = b.motore.riprendi_a(avvio) {
                // Più di un secondo da recuperare: non è più una ripresa, è
                // una riapertura dove l'ospite sarà a `quando`.
                eprintln!("[B] riprendi_a rifiutato ({rifiuto:?}): si riapre");
                if let Some((Some(etichetta), posizione_us)) = b.motore.in_attesa() {
                    let salto_us = (i128::from(salta) * 1_000_000)
                        .checked_div(i128::from(frequenza))
                        .unwrap_or(0);
                    let punto = u64::try_from(i128::from(posizione_us) + salto_us).unwrap_or(0);
                    riapri(b, etichetta, punto, quando_ns);
                }
            }
            format!("avvia:{quando_ns}:{salta}")
        }
    }
}

/// `apri_fermo` e `riprendi_a`, coi microsecondi che `apri_fermo` non prende
/// recuperati sull'istante: vedi `Mossa::Riapri`.
fn riapri(b: &Lato, etichetta: u64, posizione_us: u64, quando_ns: u64) {
    let Some(brano) = b.apri(etichetta) else {
        return;
    };
    let da_ms = posizione_us.checked_div(1_000).unwrap_or(0);
    let avanzo_ns = posizione_us
        .saturating_sub(da_ms.saturating_mul(1_000))
        .saturating_mul(1_000);
    b.motore.apri_fermo(brano, da_ms);
    if let Err(rifiuto) = b.motore.riprendi_a(quando_ns.saturating_sub(avanzo_ns)) {
        eprintln!("[B] riprendi_a rifiutato ({rifiuto:?})");
    }
}

/// Una riga del copione, eseguita su A.
fn recita(a: &Lato, azione: Azione) {
    let frequenza = i64::from(a.motore.formato().frequenza);
    match azione {
        Azione::Pausa => a.motore.pausa(),
        Azione::Riprendi => a.motore.riprendi(),
        Azione::Salta(di_ms) => {
            let qui = i64::try_from(a.motore.posizione().ms).unwrap_or(0);
            let dove = u64::try_from(qui.saturating_add(di_ms).max(0)).unwrap_or(0);
            a.motore.vai_a(dove);
        }
        Azione::VersoLaFine => {
            if a.coda.len() < 2 {
                println!("[copione] un file solo: niente cambio di brano");
                return;
            }
            let etichetta = a.motore.ancora().and_then(|x| x.etichetta).unwrap_or(0);
            match a.durata_ms(etichetta) {
                Some(durata) => a.motore.vai_a(durata.saturating_sub(PRIMA_DELLA_FINE_MS)),
                None => println!("[copione] durata ignota: niente salto verso la fine"),
            }
        }
        Azione::Gradino(ms) => {
            a.motore
                .gradino(ms.saturating_mul(frequenza).checked_div(1_000).unwrap_or(0));
        }
        Azione::Deriva(ppm) => a.motore.deriva(ppm),
        Azione::Fine => {}
    }
}

/// Un campo del CSV che può mancare.
fn campo<T: std::fmt::Display>(valore: Option<T>) -> String {
    valore.map(|v| v.to_string()).unwrap_or_default()
}

fn main() {
    let argomenti = match argomenti() {
        Ok(Some(a)) => a,
        Ok(None) => {
            elenco();
            return;
        }
        Err(errore) => {
            eprintln!("{errore}");
            eprintln!(
                "uso: insieme_locale [--elenco] [--a <uscita>] [--b <uscita>] [--csv <file>] \
                 [--latenza-a <ms>] [--latenza-b <ms>] [--giri <n>] <file A>… [--file-b <file B>]…"
            );
            std::process::exit(2);
        }
    };
    let Some(a) = lato(
        "A",
        argomenti.uscita_a,
        argomenti.latenza_a,
        &argomenti.file_a,
    ) else {
        std::process::exit(1);
    };
    let Some(b) = lato(
        "B",
        argomenti.uscita_b,
        argomenti.latenza_b,
        &argomenti.file_b,
    ) else {
        std::process::exit(1);
    };
    let mut csv = match std::fs::File::create(&argomenti.csv) {
        Ok(f) => std::io::BufWriter::new(f),
        Err(err) => {
            eprintln!("[errore] {}: {err}", argomenti.csv.display());
            std::process::exit(1);
        }
    };
    let _ = writeln!(
        csv,
        "t_ns,etichetta_a,pos_a_us,udito_a_ns,etichetta_b,pos_b_us,udito_b_ns,errore_us,mossa,\
         ppm,gradini,vuoti_a,vuoti_b,xrun_a,xrun_b,controprova_a_ns,controprova_b_ns"
    );

    // A suona libero; B ancora niente: lo farà entrare il regolatore.
    let Some(primo) = a.apri(0) else {
        std::process::exit(1);
    };
    a.motore.suona(primo);
    let mut regolatore = Regolatore::nuovo(Taratura::default());
    let mut inizio = Instant::now();
    let mut copione = COPIONE.iter().peekable();
    let mut giro = 1u32;
    let mut errori: Vec<i64> = Vec::new();
    let mut ultimo_secondo = u64::MAX;

    for passo in 0u32.. {
        let secondi = inizio.elapsed().as_secs();
        while let Some(&&(quando, azione)) = copione.peek() {
            if quando > secondi {
                break;
            }
            copione.next();
            println!("[copione] {quando:>3} s  {azione:?}");
            if matches!(azione, Azione::Fine) && giro < argomenti.giri {
                // Un altro giro, da capo: le soglie della Fase 0 sono su
                // mezz'ora, e il copione dura meno di sei minuti.
                giro += 1;
                println!("[copione] giro {giro} di {}", argomenti.giri);
                inizio = Instant::now();
                copione = COPIONE.iter().peekable();
                break;
            }
            if matches!(azione, Azione::Fine) {
                riassunto(&errori, &a, &b);
                let _ = csv.flush();
                return;
            }
            recita(&a, azione);
        }
        a.ascolta();
        b.ascolta();

        let adesso = tempo::adesso_ns();
        let ospite = a
            .stato()
            .and_then(|(etichetta, posizione_us, udito_ns, in_corso)| {
                Some(DallOspite {
                    etichetta: etichetta?,
                    posizione_us,
                    udito_ospite_ns: udito_ns,
                    in_corso,
                    programmata: false,
                })
            });
        let mia = b
            .stato()
            .map(|(etichetta, posizione_us, udito_ns, in_corso)| DaMe {
                etichetta,
                posizione_us,
                udito_ns,
                in_corso,
                in_correzione: b.motore.in_correzione(),
                riserva_us: b.motore.riserva_us(),
            });
        // Orologio `None`: lo stesso processo, la stessa base dei tempi.
        let mossa = regolatore.passo(
            adesso,
            ospite.as_ref(),
            mia.as_ref(),
            None,
            b.motore.formato().frequenza,
        );
        let fatta = esegui(&b, mossa);

        let ancora_a = a.motore.ancora();
        let ancora_b = b.motore.ancora();
        let errore = errore_us(ancora_a.as_ref(), ancora_b.as_ref());
        if let Some(e) = errore {
            errori.push(e);
        }
        let (conti_a, conti_b) = (a.motore.contatori(), b.motore.contatori());
        let _ = writeln!(
            csv,
            "{adesso},{},{},{},{},{},{},{},{fatta},{},{},{},{},{},{},{},{}",
            campo(ancora_a.and_then(|x| x.etichetta)),
            campo(ancora_a.map(|x| x.posizione_us)),
            campo(ancora_a.map(|x| x.udito_ns)),
            campo(ancora_b.and_then(|x| x.etichetta)),
            campo(ancora_b.map(|x| x.posizione_us)),
            campo(ancora_b.map(|x| x.udito_ns)),
            campo(errore),
            regolatore.diagnosi().ppm,
            conti_b.gradini_fatti,
            conti_a.vuoti,
            conti_b.vuoti,
            conti_a.xrun,
            conti_b.xrun,
            campo(ancora_a.map(|x| x.controprova_ns)),
            campo(ancora_b.map(|x| x.controprova_ns)),
        );
        if secondi != ultimo_secondo {
            ultimo_secondo = secondi;
            print!(
                "\r[{secondi:>3} s] errore {:>8} µs  ppm {:>4}  gradini {}   ",
                campo(errore),
                regolatore.diagnosi().ppm,
                conti_b.gradini_fatti
            );
            let _ = std::io::stdout().flush();
        }

        // Il passo successivo all'istante giusto, non dopo un sonno fisso: il
        // lavoro del giro non deve far scivolare il ritmo delle righe.
        let prossimo = inizio + PASSO * passo.saturating_add(1);
        std::thread::sleep(prossimo.saturating_duration_since(Instant::now()));
    }
}

/// Le cifre da guardare a fine copione: quanto B è rimasto vicino ad A.
fn riassunto(errori: &[i64], a: &Lato, b: &Lato) {
    println!();
    let mut assoluti: Vec<u64> = errori.iter().map(|e| e.unsigned_abs()).collect();
    assoluti.sort_unstable();
    let quanti = assoluti.len();
    let centile = |p: usize| {
        let posto = quanti.saturating_mul(p).checked_div(100).unwrap_or(0);
        assoluti
            .get(posto.min(quanti.saturating_sub(1)))
            .copied()
            .unwrap_or(0)
    };
    println!(
        "[riassunto] {quanti} righe allineate: |errore| mediano {} µs, p95 {} µs, massimo {} µs",
        centile(50),
        centile(95),
        assoluti.last().copied().unwrap_or(0)
    );
    println!("[riassunto] A {:?}", a.motore.contatori());
    println!("[riassunto] B {:?}", b.motore.contatori());
}
