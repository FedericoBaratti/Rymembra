//! Mostra cosa il riordino proporrebbe sulla libreria vera, senza toccarla.
//!
//! Legge i tag dai file e stampa il piano. Non sposta niente: è esattamente la
//! schermata che l'app dovrà mostrare prima di chiedere conferma.
//!
//!     cargo run -p aether-app --example piano_riordino -- "C:/Users/.../Music"

use std::collections::HashMap;

use aether_app::files::{LocalFiles, MusicFiles};
use aether_app::metadata::read_tags;
use aether_domain::organize::{SkipReason, TrackToOrganize, plan_organize};
use aether_domain::paths::{PathRules, is_supported_audio_path};

fn main() {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("uso: piano_riordino <cartella>");
        std::process::exit(2);
    };

    let files = LocalFiles;
    let Ok(found) = files.walk(&root) else {
        eprintln!("camminata fallita");
        std::process::exit(1);
    };

    let mut tracks = Vec::new();
    let mut illeggibili = 0usize;
    for (index, file) in found
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

    println!(
        "Letti {} brani ({illeggibili} illeggibili).\n",
        tracks.len()
    );
    println!("PROPONE DI SPOSTARE: {} file", plan.moves.len());

    // Quante cartelle nascerebbero, e quanto sono popolate.
    let mut per_cartella: HashMap<&str, usize> = HashMap::new();
    for m in &plan.moves {
        let dir = m.to.rsplit_once('/').map_or("", |(d, _)| d);
        *per_cartella.entry(dir).or_insert(0) += 1;
    }
    println!("  in {} cartelle\n", per_cartella.len());

    let mut ordinate: Vec<_> = per_cartella.into_iter().collect();
    ordinate.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("  le dieci piu' popolate:");
    for (dir, n) in ordinate.iter().take(10) {
        let breve = dir.strip_prefix(&root).unwrap_or(dir);
        println!("    {n:>4} brani  {breve}");
    }

    let mut per_motivo: HashMap<&str, usize> = HashMap::new();
    for s in &plan.skipped {
        *per_motivo.entry(s.reason.as_str()).or_insert(0) += 1;
    }
    println!("\nLASCIA DOV'E': {} file", plan.skipped.len());
    for (motivo, n) in &per_motivo {
        println!("    {n:>4}  {motivo}");
    }

    println!("\nDA RIVEDERE A MANO: {} gruppi", plan.needs_review.len());
    let mut review = plan.needs_review.clone();
    review.sort_by_key(|r| std::cmp::Reverse(r.tracks));
    for r in review.iter().take(12) {
        let artisti: Vec<_> = r.artists.iter().take(3).cloned().collect();
        println!(
            "    {:>3} brani  \"{}\"  — {} artisti: {}{}",
            r.tracks,
            r.album,
            r.artists.len(),
            artisti.join(", "),
            if r.artists.len() > 3 { ", …" } else { "" }
        );
    }

    let sistemati = plan.moves.len()
        + per_motivo
            .get(SkipReason::AlreadyInPlace.as_str())
            .copied()
            .unwrap_or(0);
    println!(
        "\nRiepilogo: {sistemati} brani su {} finirebbero in una cartella d'album corretta.",
        tracks.len()
    );
}
