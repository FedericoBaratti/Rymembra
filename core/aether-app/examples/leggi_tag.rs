//! Legge i tag dei file veri e riporta cosa ha trovato.
//!
//! Un test su file costruiti prova che il codice fa quel che dice. Questo prova
//! una cosa diversa e non sostituibile: che i file **di questa libreria**, con
//! le loro taggature accumulate da fonti diverse negli anni, si leggono.
//!
//!     cargo run -p aether-app --example leggi_tag -- "C:/Users/.../Music" 40

use aether_app::files::{LocalFiles, MusicFiles};
use aether_app::metadata::read_tags;
use aether_domain::paths::is_supported_audio_path;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("uso: leggi_tag <cartella> [quanti]");
        std::process::exit(2);
    };
    let limit: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(20);

    let files = LocalFiles;
    let found = match files.walk(&root) {
        Ok(found) => found,
        Err(err) => {
            eprintln!("camminata fallita: {err}");
            std::process::exit(1);
        }
    };
    let audio: Vec<_> = found
        .iter()
        .filter(|f| is_supported_audio_path(&f.path))
        .collect();
    println!(
        "{} file trovati, {} audio. Ne leggo {}.\n",
        found.len(),
        audio.len(),
        limit.min(audio.len())
    );

    let mut letti = 0usize;
    let mut falliti = 0usize;
    let mut senza_titolo = 0usize;
    let mut senza_copertina = 0usize;
    let mut senza_album = 0usize;
    let mut durata_zero = 0usize;

    for file in audio.iter().take(limit) {
        match read_tags(&files, &file.path) {
            Ok(tags) => {
                letti += 1;
                if tags.title.is_none() {
                    senza_titolo += 1;
                }
                if tags.album.is_none() {
                    senza_album += 1;
                }
                if tags.cover.is_none() {
                    senza_copertina += 1;
                }
                if tags.duration_ms == 0 {
                    durata_zero += 1;
                }
                if letti <= 5 {
                    println!(
                        "  {:?}\n    {} — {} [{}]  {:.0}s  {}kbps {}  copertina: {}",
                        file.path.rsplit(['/', '\\']).next().unwrap_or(""),
                        tags.artist.as_deref().unwrap_or("(nessun artista)"),
                        tags.title.as_deref().unwrap_or("(nessun titolo)"),
                        tags.album.as_deref().unwrap_or("(nessun album)"),
                        tags.duration_ms as f64 / 1000.0,
                        tags.bitrate.unwrap_or(0),
                        tags.codec.as_deref().unwrap_or("?"),
                        tags.cover.as_ref().map_or_else(
                            || "no".to_owned(),
                            |c| format!(
                                "{} byte, {}",
                                c.data.len(),
                                c.mime_type.as_deref().unwrap_or("tipo ignoto")
                            )
                        )
                    );
                }
            }
            Err(err) => {
                falliti += 1;
                if falliti <= 5 {
                    println!("  FALLITO {}: {err}", file.path);
                }
            }
        }
    }

    println!(
        "\nletti {letti}, falliti {falliti} — senza titolo {senza_titolo}, senza album {senza_album}, \
         senza copertina {senza_copertina}, durata zero {durata_zero}"
    );
}
