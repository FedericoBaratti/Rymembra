//! Suona un file, senza aprire nessuna finestra.
//!
//! ```text
//! cargo run -p aether-play --example suona -- "C:\musica\brano.mp3"
//! cargo run -p aether-play --example suona -- brano.mp3 altro.flac
//! ```
//!
//! Serve a separare due domande che altrimenti si confondono: «il motore
//! funziona?» e «l'applicazione lo usa bene?». Con due file di fila prova anche
//! il gapless — fra il primo e il secondo non deve sentirsi niente.

use std::path::PathBuf;
use std::sync::mpsc;

use aether_play::{BranoAperto, Evento, FormatoUscita, Sorgente};

#[expect(
    clippy::integer_division,
    reason = "millisecondi in minuti e secondi, per stamparli"
)]
fn main() {
    let file: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if file.is_empty() {
        eprintln!("uso: suona <file audio> [altro file…]");
        std::process::exit(2);
    }

    let (manda, eventi) = mpsc::channel();
    let motore = match aether_play::avvia(move |evento| {
        let _ = manda.send(evento);
    }) {
        Ok(m) => m,
        Err(err) => {
            eprintln!(
                "[errore] il motore non si è avviato: {} — {}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            std::process::exit(1);
        }
    };

    let formato = motore.formato();
    println!(
        "[uscita] {} Hz, {} canali",
        formato.frequenza, formato.canali
    );

    let mut da_suonare = file.into_iter();
    let Some(primo) = da_suonare.next() else {
        return;
    };
    let Some(brano) = apri(&primo, formato) else {
        std::process::exit(1);
    };
    println!("[suona] {}", primo.display());
    motore.suona(brano);
    // Il secondo si prepara subito: è così che si attacca senza buco.
    if let Some(dopo) = da_suonare.next() {
        motore.prepara(apri(&dopo, formato));
    }

    let mut ultimo_secondo = u64::MAX;
    loop {
        while let Ok(evento) = eventi.try_recv() {
            match evento {
                Evento::Iniziato { track_id } => println!("\n[inizio] brano {track_id}"),
                Evento::Fermato => {
                    println!("\n[fine] niente altro da suonare");
                    let vuoti = motore.vuoti();
                    if vuoti > 0 {
                        println!("[attenzione] {vuoti} campioni serviti a vuoto");
                    }
                    return;
                }
                Evento::Errore(err) => {
                    println!(
                        "\n[errore] {} — {}",
                        err.code().kind().code(),
                        err.cause().unwrap_or("—")
                    );
                }
            }
        }
        let p = motore.posizione();
        let secondo = p.ms / 1000;
        if secondo != ultimo_secondo {
            ultimo_secondo = secondo;
            print!("\r[tempo] {}:{:02}   ", secondo / 60, secondo % 60);
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Apre un file, pronto per il motore. `None` se non si può.
///
/// Il motore vuole un brano già aperto, non un percorso da aprire: qui la cosa
/// non cambia niente — l'esempio è solo, e nessuno aspetta la finestra — ma
/// nell'applicazione è ciò che permette di mettere una scadenza sopra questa
/// riga. Vedi `BranoAperto`.
fn apri(percorso: &std::path::Path, formato: FormatoUscita) -> Option<BranoAperto> {
    let file = match std::fs::File::open(percorso) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("[errore] {}: {err}", percorso.display());
            return None;
        }
    };
    let sorgente = Sorgente {
        // Fuori da una libreria un identificativo vero non c'è: qui serve solo
        // a distinguere un brano dall'altro negli eventi.
        track_id: 0,
        media: Box::new(file),
        estensione: percorso
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_owned),
        durata_ms: 0,
        replaygain_db: None,
    };
    match BranoAperto::apri(sorgente, formato) {
        Ok(brano) => Some(brano),
        Err(err) => {
            eprintln!(
                "[errore] {}: {}",
                percorso.display(),
                err.code().kind().code()
            );
            None
        }
    }
}
