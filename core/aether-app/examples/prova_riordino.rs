//! Prova il riordino su una COPIA di file veri, e poi lo annulla.
//!
//! I test su file costruiti provano la logica. Questo prova qualcos'altro: che
//! nomi veri — accenti, parentesi, punti interrogativi, virgole, apostrofi
//! ricurvi — sopravvivono a spostamento, giornale e ritorno.
//!
//! Il criterio non è «i file ci sono»: è che l'impronta di ogni file alla fine
//! sia identica a quella di partenza. Un riordino che sposta tutto e corrompe
//! un byte è peggio di uno che fallisce.
//!
//!     cargo run -p aether-app --example prova_riordino -- "C:/Users/.../Music" 60

// Questi esempi sono strumenti diagnostici, eseguiti a mano da chi sviluppa.
// Qui un guasto DEVE fermare tutto rumorosamente: un esempio che prosegue su un
// errore riporta numeri sbagliati, e i numeri sono l unica cosa che produce.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;

use aether_app::files::{LocalFiles, MusicFiles};
use aether_app::metadata::read_tags;
use aether_app::organize::{execute, read_journal, undo};
use aether_domain::organize::{TrackToOrganize, plan_organize};
use aether_domain::paths::{PathRules, is_supported_audio_path};

/// Impronta del contenuto, per nome di file. Il nome non cambia mai, quindi è
/// la chiave giusta per confrontare prima e dopo.
fn impronte(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for entry in walkdir_files(root) {
        let Ok(bytes) = std::fs::read(&entry) else {
            continue;
        };
        let name = entry
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.insert(name, blake3::hash(&bytes).to_hex().to_string());
    }
    out
}

fn walkdir_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(sorgente) = args.next() else {
        eprintln!("uso: prova_riordino <cartella> [quanti]");
        std::process::exit(2);
    };
    let quanti: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(60);

    let files = LocalFiles;
    let Ok(trovati) = files.walk(&sorgente) else {
        eprintln!("camminata fallita");
        std::process::exit(1);
    };

    // ── 1. copia in una cartella temporanea ──
    let banco = std::env::temp_dir().join(format!("aether-prova-{}", std::process::id()));
    std::fs::create_dir_all(&banco).expect("cartella di prova");
    let mut copiati = 0usize;
    for file in trovati.iter().filter(|f| is_supported_audio_path(&f.path)) {
        if copiati >= quanti {
            break;
        }
        let nome = file.path.rsplit(['/', '\\']).next().unwrap_or("x");
        if std::fs::copy(&file.path, banco.join(nome)).is_ok() {
            copiati += 1;
        }
    }
    let root = banco.to_string_lossy().replace('\\', "/");
    println!("Copiati {copiati} file veri in {root}\n");

    let prima = impronte(&banco);

    // ── 2. leggi i tag e pianifica ──
    let mut tracks = Vec::new();
    for (index, file) in files
        .walk(&root)
        .unwrap_or_default()
        .iter()
        .filter(|f| is_supported_audio_path(&f.path))
        .enumerate()
    {
        if let Ok(tags) = read_tags(&files, &file.path) {
            tracks.push(TrackToOrganize {
                id: i64::try_from(index).unwrap_or(0),
                path: file.path.clone(),
                album: tags.album.unwrap_or_default(),
                album_artist: tags.album_artist,
                artist: tags.artist,
            });
        }
    }
    let plan = plan_organize(&tracks, &root, PathRules::for_current_platform());
    println!(
        "Piano: {} spostamenti, {} fermi, {} gruppi da rivedere",
        plan.moves.len(),
        plan.skipped.len(),
        plan.needs_review.len()
    );

    // ── 3. esegui ──
    let giornale = banco.join("riordino.jsonl");
    let esito = execute(&plan, &giornale, |_, _| {}).expect("esecuzione");
    println!(
        "Eseguito: {} spostati, {} falliti",
        esito.moved.len(),
        esito.failed.len()
    );
    for f in esito.failed.iter().take(3) {
        println!("   fallito {} → {}: {}", f.mov.from, f.mov.to, f.error);
    }

    let dopo_spostamento = impronte(&banco);
    let integri = prima
        .iter()
        .filter(|(nome, hash)| dopo_spostamento.get(*nome) == Some(hash))
        .count();
    println!(
        "Dopo lo spostamento: {integri}/{} file con impronta identica",
        prima.len()
    );

    // ── 4. annulla ──
    let entries = read_journal(&giornale).expect("giornale");
    println!("\nGiornale: {} righe rilette", entries.len());
    let indietro = undo(&entries, |_, _| {});
    println!(
        "Annullato: {} rimessi, {} falliti",
        indietro.moved.len(),
        indietro.failed.len()
    );

    // ── 5. verdetto ──
    let dopo = impronte(&banco);
    let tornati = walkdir_files(&banco)
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e != "jsonl"))
        .filter(|p| p.parent() == Some(banco.as_path()))
        .count();
    let identici = prima
        .iter()
        .filter(|(nome, hash)| dopo.get(*nome) == Some(hash))
        .count();

    println!(
        "\nVERDETTO: {identici}/{} impronte identiche a prima, {} file di nuovo nella cartella di partenza",
        prima.len(),
        tornati
    );
    if identici == prima.len() && esito.failed.is_empty() && indietro.failed.is_empty() {
        println!("Andata e ritorno senza perdite.");
    } else {
        println!("QUALCOSA NON TORNA — non eseguire sui file veri.");
    }

    println!("\nBanco di prova lasciato in {root} (cancellalo quando vuoi)");
}
