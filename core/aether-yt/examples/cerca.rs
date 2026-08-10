//! Provare dal vivo la scelta del video, senza aprire l'applicazione.
//!
//! ```text
//! cargo run -q -p aether-yt --example cerca -- "Cesare Cremonini" "Poetica"
//! ```
//!
//! Stampa i candidati con canale, durata e ufficialità, e segna quale
//! vincerebbe. È il controllo che la preferenza per i canali ufficiali funzioni
//! *davvero* e non solo nei vettori di prova: i vettori dicono che la regola è
//! applicata bene, questo dice che la regola è quella giusta contro le risposte
//! vere di YouTube.
//!
//! Un esempio e non una prova d'integrazione perché tocca la rete e un binario
//! esterno: in `cargo test` sarebbe una prova che fallisce in aereo.

use std::path::PathBuf;

use aether_domain::SpotifyTrack;
use aether_domain::yt_match::{query_larga, query_stretta, scegli_candidato, ufficialita};
use aether_yt::Binari;

fn main() {
    let argomenti: Vec<String> = std::env::args().skip(1).collect();
    let (artista, titolo, durata) = match argomenti.as_slice() {
        [artista, titolo] => (artista.clone(), titolo.clone(), None),
        [artista, titolo, secondi] => (
            artista.clone(),
            titolo.clone(),
            secondi.parse::<u64>().ok().map(|s| s.saturating_mul(1000)),
        ),
        _ => {
            eprintln!("uso: cerca <artista> <titolo> [durata in secondi]");
            std::process::exit(2);
        }
    };

    let binari = Binari::risolvi(cartella_binari());
    println!("binari in {}", binari.cartella().display());
    match binari.ytdlp() {
        Some(percorso) => println!("yt-dlp: {}", percorso.display()),
        None => {
            eprintln!("yt-dlp non c'è: mettilo lì e riprova");
            std::process::exit(1);
        }
    }

    let brano = SpotifyTrack {
        title: titolo,
        artist: Some(artista),
        duration_ms: durata,
        ..SpotifyTrack::default()
    };

    println!("\nquery stretta: {}", query_stretta(&brano));
    if let Some(larga) = query_larga(&brano) {
        println!("query larga:   {larga}");
    }
    match brano.duration_ms {
        Some(ms) => println!("durata attesa: {}s", ms.div_euclid(1000)),
        None => println!("durata attesa: sconosciuta (decide solo il canale)"),
    }

    let candidati = match aether_yt::cerca(&binari, &brano, &|| false) {
        Ok(candidati) => candidati,
        Err(e) => {
            eprintln!("\nla ricerca è fallita: {e}");
            eprintln!("ritentabile: {}", e.is_retryable());
            std::process::exit(1);
        }
    };

    if candidati.is_empty() {
        println!("\nnessun risultato: il brano andrebbe segnato introvabile");
        return;
    }

    let vincitore = scegli_candidato(&candidati, &brano).map(|c| c.url.clone());
    println!("\n{} candidati:", candidati.len());
    for candidato in &candidati {
        let scelto = if Some(&candidato.url) == vincitore.as_ref() {
            "→"
        } else {
            " "
        };
        let quanto = ufficialita(
            candidato.canale.as_deref(),
            candidato.canale_verificato,
            brano.artist.as_deref(),
        );
        let durata = candidato
            .durata_sec
            .map_or_else(|| "?".to_owned(), |s| format!("{s}s"));
        println!(
            "{scelto} [{quanto:?}] {durata:>6}  {}  ({})",
            candidato.titolo,
            candidato.canale.as_deref().unwrap_or("canale ignoto")
        );
    }

    if vincitore.is_none() {
        println!("\nnessun candidato accettabile: tutti fuori dal cancello della durata");
    }
}

/// Dove cercare i binari eseguendo dal repo.
///
/// Prima le risorse dell'app desktop, che è dove staranno impacchettati; poi il
/// vecchio albero, che li ha già scaricati.
fn cartella_binari() -> PathBuf {
    if let Ok(dalla_variabile) = std::env::var("AETHER_BIN_DIR") {
        return PathBuf::from(dalla_variabile);
    }
    let radice = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf);

    let nuova = radice.join("apps/desktop/src-tauri/resources/bin");
    if nuova.is_dir() {
        return nuova;
    }
    radice.join("legacy/Aeter/resources/bin")
}
