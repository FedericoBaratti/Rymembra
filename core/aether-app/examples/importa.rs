//! Porta nella libreria nuova quel che una scansione non sa ricostruire.
//!
//! Mostra il piano e si ferma. Per scrivere davvero serve `--esegui`, perché
//! l'elenco di quel che **non** si ritrova è la cosa da leggere prima di
//! decidere, non dopo.
//!
//!     cargo run --release -p aether-app --example importa -- <vecchio.db> <cartella-dati> [--esegui]

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stdout)]

use aether_app::import_legacy::{ImportReport, import, open_legacy, plan_import};

fn stampa(titolo: &str, r: &ImportReport) {
    println!("\n── {titolo} ──");
    println!("righe nel vecchio database   {}", r.legacy_tracks);
    println!("  di cui con qualcosa da portare  {}", r.legacy_with_stats);
    println!("brani della libreria toccati {}", r.matched);
    println!("  ascolti portati               {}", r.play_count_carried);
    println!("  voti                          {}", r.ratings_carried);
    println!("  preferiti                     {}", r.liked_carried);
    println!("cronologia                   {} righe", r.history_rows);
    if r.history_orphans > 0 {
        println!("  saltate (brano assente)       {}", r.history_orphans);
    }
    println!("playlist                     {}", r.playlists);
    if r.playlist_entries > 0 || r.playlist_orphans > 0 {
        println!(
            "  voci                          {} ({} saltate)",
            r.playlist_entries, r.playlist_orphans
        );
    }
    println!("lapidi                       {}", r.tombstones);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(vecchio), Some(dati)) = (args.next(), args.next()) else {
        eprintln!("uso: importa <vecchio.db> <cartella dati> [--esegui]");
        std::process::exit(2);
    };
    let esegui = args.any(|a| a == "--esegui");

    let legacy = open_legacy(std::path::Path::new(&vecchio)).expect("vecchio database");
    let percorso = std::path::Path::new(&dati).join("aether.db");
    let mut connection = aether_app::db::open(&percorso)
        .expect("libreria nuova")
        .connection;

    let piano = plan_import(&legacy, &mut connection).expect("piano");
    stampa("piano", &piano);

    if !piano.unmatched.is_empty() {
        println!(
            "\nnon si ritrovano in libreria: {} brani del vecchio database.",
            piano.unmatched.len()
        );
        println!("sono file che non stanno più sul disco — i primi 25:");
        for voce in piano.unmatched.iter().take(25) {
            println!("  {voce}");
        }
    }

    if !esegui {
        println!("\nniente è stato scritto. Rilancia con --esegui per applicare.");
        return;
    }

    let esito = import(&legacy, &mut connection).expect("importazione");
    stampa("eseguito", &esito);

    let (ascolti, cronologia, preferiti): (i64, i64, i64) = connection
        .query_row(
            "SELECT (SELECT COALESCE(SUM(play_count),0) FROM tracks),
                    (SELECT COUNT(*) FROM play_history),
                    (SELECT COUNT(*) FROM tracks WHERE liked = 1)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("verifica");
    println!("\n── libreria dopo ──");
    println!("somma ascolti  {ascolti}");
    println!("cronologia     {cronologia} righe");
    println!("preferiti      {preferiti}");
}
