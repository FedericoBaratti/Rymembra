//! Stampa il foglio di una skin.
//!
//! Serve a due cose: guardare cosa produce il compilatore senza aprire una
//! finestra, e ricavare il blocco di base che il guscio desktop tiene in
//! `stile.css` per non lampeggiare prima che l'IPC risponda. Quel blocco non si
//! scrive a mano — si copia da qui, e un test lo tiene allineato.
//!
//! ```text
//! cargo run -p aether-skin --example compila           # la skin di serie
//! cargo run -p aether-skin --example compila mia.json  # una qualunque
//! ```

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let percorso = std::env::args().nth(1);

    let sorgente = match percorso.as_deref() {
        None => aether_skin::PLAIN_SOURCE.to_owned(),
        Some(file) => match std::fs::read_to_string(file) {
            Ok(testo) => testo,
            Err(err) => {
                eprintln!("non si legge {file}: {err}");
                return ExitCode::FAILURE;
            }
        },
    };

    let skin = match aether_skin::parse_skin_json(&sorgente) {
        Ok(skin) => skin,
        Err(err) => {
            eprintln!("skin non valida [{}]", err.code().kind().code());
            if let Some(messaggio) = err.message() {
                eprintln!("{messaggio}");
            }
            return ExitCode::FAILURE;
        }
    };

    for avviso in aether_skin::check_skin(&skin) {
        eprintln!("avviso {} — {}", avviso.path, avviso.message);
    }

    let compilata = aether_skin::compile_skin(&skin);
    eprintln!(
        "[{}] costo {} su {}, token dinamici {}",
        compilata.id,
        compilata.cost,
        aether_skin::SURFACE_COST_BUDGET,
        compilata.dynamic_tokens.len()
    );
    print!("{}", compilata.css);
    ExitCode::SUCCESS
}
