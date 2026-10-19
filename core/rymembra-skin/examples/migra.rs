//! Stampa una skin portata al formato di oggi, in forma canonica.
//!
//! È quel che lo Studio fa quando apre una skin vecchia, senza aprire una
//! finestra: serve a guardare una migrazione, e a riscrivere per esteso una
//! skin di serie quando il formato cambia.
//!
//! ```text
//! cargo run -p rymembra-skin --example migra vecchia.json
//! ```

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(percorso) = std::env::args().nth(1) else {
        eprintln!("uso: migra <skin.json>");
        return ExitCode::FAILURE;
    };
    let sorgente = match std::fs::read_to_string(&percorso) {
        Ok(testo) => testo,
        Err(err) => {
            eprintln!("non si legge {percorso}: {err}");
            return ExitCode::FAILURE;
        }
    };
    match rymembra_skin::migra_testo(&sorgente) {
        Ok(migrato) => {
            eprintln!("dal formato {}", migrato.da);
            print!("{}", migrato.sorgente);
            ExitCode::SUCCESS
        }
        Err(problemi) => {
            for guasto in problemi {
                eprintln!("{}: {}", guasto.path, guasto.message);
            }
            ExitCode::FAILURE
        }
    }
}
