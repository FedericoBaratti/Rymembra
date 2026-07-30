//! Estrae e salva le copertine della libreria vera, e misura cosa costa.
//!
//! La ricodifica l'ho giustificata con un numero stimato (seicento kilobyte per
//! copertina, per millequattrocento brani). Questo la misura.
//!
//!     cargo run -p aether-app --example prova_copertine -- "C:/…/Music" <cartella-store>

#![allow(clippy::expect_used, clippy::unwrap_used)]

use aether_app::covers::{CoverSource, CoverStore};
use aether_app::files::{LocalFiles, MusicFiles};
use aether_app::metadata::read_tags;
use aether_domain::paths::is_supported_audio_path;

fn mb(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64 / 1_048_576.0;
    format!("{value:.1} MB")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("uso: prova_copertine <cartella musica> [cartella store]");
        std::process::exit(2);
    };
    let store_dir = args.next().unwrap_or_else(|| {
        std::env::temp_dir()
            .join("aether-copertine")
            .to_string_lossy()
            .into_owned()
    });

    let files = LocalFiles;
    let store = CoverStore::open(&store_dir).expect("store");
    let trovati = files.walk(&root).expect("camminata");

    let mut brani = 0usize;
    let mut con_copertina = 0usize;
    let mut byte_originali = 0u64;
    let mut byte_salvati = 0u64;
    let mut distinte = std::collections::HashSet::new();
    let mut condivise = 0usize;
    let mut illeggibili = 0usize;

    for file in trovati.iter().filter(|f| is_supported_audio_path(&f.path)) {
        let Ok(tags) = read_tags(&files, &file.path) else {
            continue;
        };
        brani += 1;
        let Some(cover) = tags.cover else { continue };
        con_copertina += 1;
        byte_originali += cover.data.len() as u64;

        match store.store(&cover.data, CoverSource::Tag) {
            Ok(salvata) => {
                if salvata.already_present {
                    condivise += 1;
                } else {
                    byte_salvati += salvata.byte_size;
                    // La miniatura pesa anche lei.
                    if let Ok(m) = std::fs::metadata(store.thumbnail_path_for(&salvata.hash)) {
                        byte_salvati += m.len();
                    }
                }
                distinte.insert(salvata.hash);
            }
            Err(_) => illeggibili += 1,
        }
    }

    println!("brani letti              {brani}");
    println!("con copertina            {con_copertina}");
    println!("copertine distinte       {}", distinte.len());
    println!("gia' viste (condivise)   {condivise}");
    println!("non decodificabili       {illeggibili}");
    println!();
    println!("byte incorporati nei file {}", mb(byte_originali));
    println!("byte nello store          {}", mb(byte_salvati));
    if byte_salvati > 0 {
        #[allow(clippy::cast_precision_loss)]
        let fattore = byte_originali as f64 / byte_salvati as f64;
        println!("risparmio                 {fattore:.1}x");
    }
    println!("\nstore in {store_dir}");
}
