//! Esegue il riordino sui file veri. Richiede `--conferma`.
//!
//! La guardia non è burocrazia: questo è l'unico strumento del progetto che
//! modifica i file dell'utente, e deve essere impossibile lanciarlo per sbaglio
//! sbagliando un comando in cronologia.
//!
//! Prima di toccare qualunque cosa scrive due file nella cartella dei dati:
//! l'istantanea del prima (percorso, dimensione, data) e il giornale. Il primo
//! serve a *verificare*, il secondo a *tornare indietro*. Sono due bisogni
//! diversi e due file diversi.
//!
//!     cargo run -p aether-app --example esegui_riordino -- "C:/…/Music" --conferma
//!     cargo run -p aether-app --example esegui_riordino -- --annulla <giornale>

// Strumento diagnostico eseguito a mano: un guasto deve fermare tutto
// rumorosamente invece di produrre numeri sbagliati.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aether_app::files::{LocalFiles, MusicFiles};
use aether_app::metadata::read_tags;
use aether_app::organize::{execute, read_journal, undo};
use aether_domain::organize::{TrackToOrganize, plan_organize};
use aether_domain::paths::{PathRules, is_supported_audio_path};

/// Dove finiscono giornale e istantanea: fuori dalla cartella che si riordina.
fn cartella_dati() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join("aether-rust");
    std::fs::create_dir_all(&dir).expect("cartella dati");
    dir
}

fn ora() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Nome del file → (dimensione, data di modifica).
///
/// Basta a scoprire un file perso, duplicato o troncato. L'impronta del
/// contenuto sarebbe più forte ma richiede di rileggere l'intera libreria due
/// volte, e uno spostamento sullo stesso volume non tocca i byte: è
/// un'operazione sui soli metadati del filesystem.
fn istantanea(root: &str) -> BTreeMap<String, (u64, i64)> {
    let mut out = BTreeMap::new();
    for file in LocalFiles.walk(root).unwrap_or_default() {
        if !is_supported_audio_path(&file.path) {
            continue;
        }
        let nome = file
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .to_owned();
        out.insert(nome, (file.size_bytes, file.modified_ms));
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("--annulla") {
        let Some(giornale) = args.get(1) else {
            eprintln!("uso: --annulla <percorso del giornale>");
            std::process::exit(2);
        };
        let entries = read_journal(Path::new(giornale)).expect("giornale illeggibile");
        println!("Annullo {} spostamenti…", entries.len());
        let esito = undo(&entries, |fatti, totale| {
            if fatti % 200 == 0 || fatti == totale {
                println!("  {fatti}/{totale}");
            }
        });
        println!(
            "Rimessi {}, falliti {}, cartelle rimosse {}",
            esito.moved.len(),
            esito.failed.len(),
            esito.removed_dirs
        );
        for f in esito.failed.iter().take(10) {
            println!("  FALLITO {} → {}: {}", f.mov.from, f.mov.to, f.error);
        }
        return;
    }

    let Some(root) = args.first().cloned() else {
        eprintln!("uso: esegui_riordino <cartella> --conferma");
        std::process::exit(2);
    };
    let confermato = args.iter().any(|a| a == "--conferma");

    // ── il piano ──
    let files = LocalFiles;
    let trovati = files.walk(&root).expect("camminata");
    let mut tracks = Vec::new();
    let mut illeggibili = 0usize;
    for (index, file) in trovati
        .iter()
        .filter(|f| is_supported_audio_path(&f.path))
        .enumerate()
    {
        match read_tags(&files, &file.path) {
            Ok(tags) => tracks.push(TrackToOrganize {
                id: i64::try_from(index).unwrap_or(0),
                path: file.path.clone(),
                album: tags.album.unwrap_or_default(),
                album_artist: tags.album_artist,
                artist: tags.artist,
            }),
            Err(_) => illeggibili += 1,
        }
    }
    let plan = plan_organize(&tracks, &root, PathRules::for_current_platform());

    println!("Cartella: {root}");
    println!("Brani letti: {} ({illeggibili} illeggibili)", tracks.len());
    println!("Da spostare: {}", plan.moves.len());
    println!("Fermi: {}", plan.skipped.len());
    println!("Gruppi da rivedere: {}", plan.needs_review.len());

    if !confermato {
        println!("\nNessuna conferma: non ho toccato niente.");
        println!("Per eseguire davvero, aggiungi --conferma");
        return;
    }

    // ── l'istantanea del prima, PRIMA di toccare qualsiasi cosa ──
    let dati = cartella_dati();
    let stamp = ora();
    let percorso_istantanea = dati.join(format!("prima-{stamp}.tsv"));
    let prima = istantanea(&root);
    let righe: String = prima
        .iter()
        .map(|(nome, (size, mtime))| format!("{nome}\t{size}\t{mtime}\n"))
        .collect();
    std::fs::write(&percorso_istantanea, righe).expect("istantanea");
    println!("\nIstantanea del prima: {}", percorso_istantanea.display());
    println!("  {} file censiti", prima.len());

    // ── esecuzione ──
    let percorso_giornale = dati.join(format!("riordino-{stamp}.jsonl"));
    println!("Giornale: {}\n", percorso_giornale.display());

    let esito = execute(&plan, &percorso_giornale, |fatti, totale| {
        if fatti % 200 == 0 || fatti == totale {
            println!("  {fatti}/{totale}");
        }
    })
    .expect("esecuzione");

    println!(
        "\nSpostati {}, falliti {}, cartelle vuote rimosse {}",
        esito.moved.len(),
        esito.failed.len(),
        esito.removed_dirs
    );
    for f in esito.failed.iter().take(10) {
        println!("  FALLITO {} → {}: {}", f.mov.from, f.mov.to, f.error);
    }

    // ── verifica ──
    let dopo = istantanea(&root);
    let mancanti: Vec<_> = prima.keys().filter(|n| !dopo.contains_key(*n)).collect();
    let alterati: Vec<_> = prima
        .iter()
        .filter(|(nome, prima_val)| dopo.get(*nome).is_some_and(|d| d != *prima_val))
        .map(|(nome, _)| nome)
        .collect();
    let nuovi = dopo.len().saturating_sub(prima.len());

    println!("\nVERIFICA");
    println!("  prima {} file, dopo {} file", prima.len(), dopo.len());
    println!("  spariti: {}", mancanti.len());
    println!("  con dimensione o data cambiata: {}", alterati.len());
    println!("  comparsi dal nulla: {nuovi}");
    for nome in mancanti.iter().take(10) {
        println!("    SPARITO: {nome}");
    }
    for nome in alterati.iter().take(10) {
        println!("    ALTERATO: {nome}");
    }

    if mancanti.is_empty() && alterati.is_empty() && prima.len() == dopo.len() {
        println!("\nTutti i file sono ancora lì, identici, in cartelle nuove.");
        println!(
            "Per tornare indietro:\n  cargo run -p aether-app --example esegui_riordino -- --annulla \"{}\"",
            percorso_giornale.display()
        );
    } else {
        println!("\nQUALCOSA NON TORNA. Annulla subito con:");
        println!(
            "  cargo run -p aether-app --example esegui_riordino -- --annulla \"{}\"",
            percorso_giornale.display()
        );
    }
}
