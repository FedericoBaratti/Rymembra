//! Stampa la tabella dei contrasti di una skin.
//!
//! Serve a chi scrive il documento di disegno e a chi ritocca una superficie: i
//! numeri di `disegno-ux.md` vengono da qui, non dalla memoria di qualcuno.
//!
//! ```text
//! cargo run -p aether-skin --example contrasti
//! cargo run -p aether-skin --example contrasti -- percorso/di/skin.json
//! ```
//!
//! Senza argomenti misura la skin di serie.

fn main() {
    let sorgente = match std::env::args().nth(1) {
        Some(percorso) => std::fs::read_to_string(&percorso).unwrap_or_else(|err| {
            eprintln!("non riesco a leggere «{percorso}»: {err}");
            std::process::exit(1);
        }),
        None => aether_skin::PLAIN_SOURCE.to_owned(),
    };

    let skin = match aether_skin::parse_skin_json(&sorgente) {
        Ok(skin) => skin,
        Err(err) => {
            eprintln!(
                "documento non valido: {}",
                err.message().unwrap_or_default()
            );
            std::process::exit(1);
        }
    };

    println!(
        "{} {} — soglia {}:1\n",
        skin.meta.name,
        skin.meta.version,
        aether_skin::CONTRASTO_MINIMO
    );
    println!(
        "{:<16} {:<16} {:>7} {:>7}",
        "davanti", "dietro", "scuro", "chiaro"
    );

    let mut sotto = 0;
    for coppia in aether_skin::contrast_pairs(&skin) {
        let chiaro = coppia
            .light
            .map_or_else(|| "—".to_owned(), |l| format!("{l:.2}"));
        // Il segno sta in fondo, dove l'occhio arriva dopo aver letto i numeri.
        let segno = if coppia.passa() {
            ""
        } else {
            sotto += 1;
            "  <- sotto soglia"
        };
        println!(
            "{:<16} {:<16} {:>7.2} {:>7}{segno}",
            coppia.foreground.trim_start_matches("color."),
            coppia.background.trim_start_matches("color."),
            coppia.dark,
            chiaro
        );
    }

    println!();
    if sotto == 0 {
        println!("tutte le coppie si leggono.");
    } else {
        println!("{sotto} coppie sotto la soglia.");
    }
}
