//! Una scansione vera, dal disco alla libreria interrogabile.
//!
//! Mostra il piano prima di eseguirlo, esegue, ricostruisce gli aggregati e
//! interroga il risultato. Il database e lo store delle copertine restano dove
//! si dice, così una seconda esecuzione misura una **riscansione** — che è il
//! numero che conta davvero, perché è quello che l'utente paga a ogni avvio.
//!
//!     cargo run --release -p aether-app --example scansione -- "C:/…/Music" [cartella-dati] [ricerca]

// La divisione fra interi qui è deliberata: millisecondi in ore, dove il resto
// non interessa a chi legge il numero.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::integer_division
)]

use std::io::Write as _;

use aether_app::covers::CoverStore;
use aether_app::files::LocalFiles;
use aether_app::library::{Scan, search};
use aether_domain::paths::PathRules;
use aether_domain::scan_plan::{RemoveReason, SkipReason};

fn secondi(ms: u128) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = ms as f64 / 1000.0;
    format!("{value:.1}s")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("uso: scansione <cartella musica> [cartella dati] [ricerca]");
        std::process::exit(2);
    };
    let dati = args.next().unwrap_or_else(|| {
        std::env::temp_dir()
            .join("aether-libreria")
            .to_string_lossy()
            .into_owned()
    });
    let ricerca = args.next();

    let cartella = std::path::Path::new(&dati);
    std::fs::create_dir_all(cartella).expect("cartella dati");
    let store = CoverStore::open(cartella.join("copertine")).expect("store");
    let aperto = aether_app::db::open(&cartella.join("aether.db")).expect("database");
    let mut connection = aperto.connection;
    println!(
        "database    versione {} → {} ({} migrazioni), FTS5 {}",
        aperto.version_before,
        aether_app::db::LATEST_VERSION,
        aperto.applied,
        if aperto.fts5 { "sì" } else { "no" }
    );

    let roots = vec![root.clone()];
    let scan = Scan {
        files: &LocalFiles,
        covers: &store,
        roots: &roots,
        rules: PathRules::for_current_platform(),
    };

    // ── il piano, prima ──
    let piano = aether_app::library::plan(&scan, &connection).expect("piano");
    println!("\n── piano ──");
    println!("da inserire   {}", piano.to_insert.len());
    println!("da aggiornare {}", piano.to_update.len());
    println!("da togliere   {}", piano.to_remove.len());
    println!("già a posto   {}", piano.unchanged);
    println!("fuori radice  {}", piano.untouched);
    for reason in [
        SkipReason::NonAudio,
        SkipReason::InTrash,
        SkipReason::TooSmall,
        SkipReason::Duplicate,
    ] {
        let quanti = piano.skipped.iter().filter(|s| s.reason == reason).count();
        if quanti > 0 {
            println!("scartati ({})  {quanti}", reason.as_str());
        }
    }
    for reason in [RemoveReason::Disappeared, RemoveReason::DuplicateRow] {
        let quanti = piano
            .to_remove
            .iter()
            .filter(|r| r.reason == reason)
            .count();
        if quanti > 0 {
            println!("da togliere ({})  {quanti}", reason.as_str());
        }
    }

    // ── l'esecuzione ──
    println!("\n── esecuzione ──");
    let esito = scan
        .run(&mut connection, |fatti, totale| {
            if totale > 0 && (fatti % 50 == 0 || fatti == totale) {
                print!("\rletti {fatti}/{totale}");
                let _ = std::io::stdout().flush();
            }
            // Da riga di comando non c'è chi prema Annulla: si va fino in fondo.
            std::ops::ControlFlow::Continue(())
        })
        .expect("scansione");
    if !esito.plan.to_insert.is_empty() || !esito.plan.to_update.is_empty() {
        println!();
    }

    println!("inseriti      {}", esito.inserted);
    println!("aggiornati    {}", esito.updated);
    println!("spostati      {}", esito.moved);
    println!("tolti         {}", esito.removed);
    println!("illeggibili   {}", esito.unreadable.len());
    println!(
        "copertine     {} ricodificate, {} già nello store",
        esito.covers_stored, esito.covers_reused
    );
    println!("album         {}", esito.aggregates.albums);
    println!("artisti       {}", esito.aggregates.artists);
    println!("durata        {}", secondi(esito.elapsed_ms));

    for illeggibile in esito.unreadable.iter().take(10) {
        println!(
            "  illeggibile: {} — {}",
            illeggibile.path, illeggibile.error
        );
    }

    // ── la libreria, interrogata ──
    let brani: i64 = connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("conteggio");
    let durata: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(duration_ms), 0) FROM tracks",
            [],
            |r| r.get(0),
        )
        .expect("durata");
    println!("\n── libreria ──");
    println!("brani         {brani}");
    println!("ascolto       {} ore", durata / 3_600_000);

    let mut statement = connection
        .prepare(
            "SELECT a.title, a.artist, a.total_tracks
             FROM albums a ORDER BY a.total_tracks DESC LIMIT 5",
        )
        .expect("album");
    println!("album più lunghi:");
    let righe = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .expect("album");
    for riga in righe.flatten() {
        println!("  {:>3} — {} · {}", riga.2, riga.1, riga.0);
    }

    if let Some(query) = ricerca {
        println!("\n── ricerca «{query}» ──");
        let trovati = search(&connection, &query, 0, 10).expect("ricerca");
        println!("{} risultati", trovati.len());
        for hit in trovati.iter().take(10) {
            println!("  {} · {} — {}", hit.artist, hit.album, hit.title);
        }
    }

    println!("\ndati in {dati}");
}
