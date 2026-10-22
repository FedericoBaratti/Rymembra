//! Una scansione vera, dal disco alla libreria interrogabile.
//!
//! Esegue, ricostruisce gli aggregati e interroga il risultato. Il database e
//! lo store delle copertine restano dove si dice, così una seconda esecuzione
//! misura una **riscansione** — che è il numero che conta davvero, perché è
//! quello che l'utente paga a ogni avvio.
//!
//!     cargo run --release -p rymembra-app --example scansione -- "C:/…/Music[;D:/…/Altra]" [cartella-dati] [ricerca]
//!
//! Più radici si separano con `;`, come le cartelle sorvegliate di una libreria
//! vera. Il piano si mostra prima di eseguire solo con `RYM_PIANO=1`: costa una
//! camminata in più, e su una share quella camminata è proprio la misura che si
//! vuole fare — farla due volte vorrebbe dire cronometrare la seconda, con la
//! cache delle cartelle già calda. Gli illeggibili finiscono tutti, con la loro
//! causa, in `illeggibili.txt` dentro la cartella dati; quelli che la memoria
//! dei rifiuti ricorda, in `illeggibili-noti.txt`.
//!
//! Con `RYM_AGGREGATI=1` non scansiona: misura tre volte la ricostruzione
//! intera degli album sul database così com'è — **senza migrarlo**, aperto con
//! `rusqlite` nudo — dentro una transazione che poi si annulla. Con
//! `RYM_AGGREGATI=migra` prima lo apre come l'applicazione, migrazioni
//! comprese. Serve a confrontare il prima e il dopo di una migrazione su due
//! copie dello stesso database.

// La divisione fra interi qui è deliberata: millisecondi in ore, dove il resto
// non interessa a chi legge il numero.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::integer_division
)]

use std::io::Write as _;

use std::sync::Arc;

use rymembra_app::covers::CoverStore;
use rymembra_app::files::LocalFiles;
use rymembra_app::library::{Avanzamento, Scadenze, Scan, search};
use rymembra_domain::paths::PathRules;
use rymembra_domain::scan_plan::{RemoveReason, SkipReason};

fn secondi(ms: u128) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "millisecondi di una scansione, stampati con un decimale: un f64 li tiene esatti per trecentomila anni"
    )]
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
            .join("rymembra-libreria")
            .to_string_lossy()
            .into_owned()
    });
    let ricerca = args.next();

    let cartella = std::path::Path::new(&dati);
    std::fs::create_dir_all(cartella).expect("cartella dati");
    if let Some(modo) = std::env::var_os("RYM_AGGREGATI") {
        misura_gli_aggregati(&cartella.join("rymembra.db"), modo == "migra");
        return;
    }
    let store = CoverStore::open(cartella.join("copertine")).expect("store");
    let aperto = rymembra_app::db::open(&cartella.join("rymembra.db")).expect("database");
    let mut connection = aperto.connection;
    println!(
        "database    versione {} → {} ({} migrazioni), FTS5 {}",
        aperto.version_before,
        rymembra_app::db::LATEST_VERSION,
        aperto.applied,
        if aperto.fts5 { "sì" } else { "no" }
    );

    let roots: Vec<String> = root
        .split(';')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
        .collect();
    let scan = Scan {
        files: Arc::new(LocalFiles),
        covers: store.clone(),
        roots: &roots,
        // Da riga di comando le radici date sono la libreria.
        radici_della_libreria: &roots,
        rules: PathRules::for_current_platform(),
        // Un esempio lo si lancia a mano e se ne legge l'esito: le rimozioni
        // che la guardia trattiene tornano una per una, e qui si stampano.
        prudente: false,
        scadenze: Scadenze::default(),
        // Da riga di comando si annulla con Ctrl+C, che non passa di qui.
        fermati: None,
    };

    // ── il piano, prima — solo se chiesto: è una camminata in più ──
    if std::env::var_os("RYM_PIANO").is_some() {
        mostra_il_piano(&scan, &connection);
    }

    // ── l'esecuzione ──
    println!("\n── esecuzione ──");
    let esito = scan
        .run_con(&mut connection, |avanzamento| {
            match avanzamento {
                Avanzamento::Camminata { cartelle, file } => {
                    print!("\rcammino: {cartelle} cartelle, {file} file");
                }
                Avanzamento::Lettura { fatti, totale } => {
                    if totale > 0 && (fatti % 50 == 0 || fatti == totale) {
                        print!("\rletti {fatti}/{totale}                    ");
                    }
                }
            }
            let _ = std::io::stdout().flush();
            // Da riga di comando non c'è chi prema Annulla: si va fino in fondo.
            std::ops::ControlFlow::Continue(())
        })
        .expect("scansione");
    println!();

    for saltata in &esito.radici_saltate {
        println!("radice non raggiungibile, saltata: {saltata}");
    }
    for ramo in &esito.rami_persi {
        println!("ramo non verificato: {ramo}");
    }
    println!(
        "camminata     {} cartelle, {} file",
        esito.cartelle_viste, esito.file_visti
    );
    println!("brani d'immagine {}", esito.brani_d_immagine);
    println!(
        "da leggere    {}",
        esito.plan.to_insert.len() + esito.plan.to_update.len()
    );
    println!("letti         {}", esito.letti);
    println!("illeggibili già noti {}", esito.illeggibili_noti);
    println!("già a posto   {}", esito.plan.unchanged);
    println!("inseriti      {}", esito.inserted);
    println!("aggiornati    {}", esito.updated);
    println!("spostati      {}", esito.moved);
    println!("tolti         {}", esito.removed);
    if esito.rimozioni_rinviate > 0 {
        println!(
            "trattenuti    {} (troppi in una volta: niente è stato tolto)",
            esito.rimozioni_rinviate
        );
    }
    if esito.non_verificati > 0 {
        println!(
            "non verificati {} (sotto {} cartelle illeggibili)",
            esito.non_verificati,
            esito.rami_persi.len()
        );
    }
    println!("illeggibili   {}", esito.unreadable.len());
    println!(
        "copertine     {} ricodificate, {} già nello store",
        esito.covers_stored, esito.covers_reused
    );
    println!("album         {}", esito.aggregates.albums);
    println!("artisti       {}", esito.aggregates.artists);
    println!("durata        {}", secondi(esito.elapsed_ms));
    let tempi = esito.tempi;
    println!(
        "fasi (ms)     camminata {} · fogli {} · piano {} · lettura {} · scrittura {} · aggregati {}",
        tempi.camminata_ms,
        tempi.fogli_ms,
        tempi.piano_ms,
        tempi.lettura_ms,
        tempi.scrittura_ms,
        tempi.aggregati_ms
    );

    for illeggibile in esito.unreadable.iter().take(10) {
        println!(
            "  illeggibile: {} — {}",
            illeggibile.path, illeggibile.error
        );
    }
    // Tutti, con la causa intera: è l'elenco che serve a capire **perché**.
    let elenco: String = esito
        .unreadable
        .iter()
        .map(|u| format!("{}\t{}\n", u.path, u.error))
        .collect();
    std::fs::write(cartella.join("illeggibili.txt"), elenco).expect("elenco degli illeggibili");
    // E quelli che la memoria dei rifiuti ricorda: i già noti di questa
    // passata, più quelli appena rifiutati.
    let noti: String = connection
        .prepare("SELECT chiave, causa FROM file_illeggibili ORDER BY chiave")
        .expect("memoria dei rifiuti")
        .query_map([], |r| {
            Ok(format!(
                "{}\t{}\n",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?
            ))
        })
        .expect("memoria dei rifiuti")
        .flatten()
        .collect();
    std::fs::write(cartella.join("illeggibili-noti.txt"), noti).expect("elenco dei noti");

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

/// Il piano prima di eseguirlo: quel che la scansione sta per fare.
fn mostra_il_piano(scan: &Scan<'_>, connection: &rusqlite::Connection) {
    let esplorazione = rymembra_app::library::plan(scan, connection).expect("piano");
    let piano = &esplorazione.piano;
    for saltata in &esplorazione.radici_saltate {
        println!("radice non raggiungibile, saltata: {saltata}");
    }
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
}

/// La ricostruzione intera degli album, misurata tre volte e annullata.
///
/// Senza migrare, se non lo si chiede: è il modo di misurare il database
/// **prima** di una migrazione — una copia lasciata alla versione di prima —
/// e confrontarlo con un'altra copia migrata.
fn misura_gli_aggregati(percorso: &std::path::Path, migra: bool) {
    let mut connection = if migra {
        rymembra_app::db::open(percorso)
            .expect("database")
            .connection
    } else {
        rusqlite::Connection::open(percorso).expect("database")
    };
    let versione: i64 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("versione");
    let brani: i64 = connection
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("brani");
    println!("database alla versione {versione}, {brani} brani");
    for giro in 1..=3 {
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .expect("transazione");
        let inizio = std::time::Instant::now();
        let aggregati = rymembra_app::library::rebuild_aggregates(&tx).expect("ricostruzione");
        let ms = inizio.elapsed().as_millis();
        tx.rollback().expect("annullamento");
        println!(
            "giro {giro}: ricostruzione intera {ms} ms ({} album, {} artisti)",
            aggregati.albums, aggregati.artists
        );
    }
    // Il caso pesante: le chiavi canoniche tornate chiavi di base, come le
    // lascia «Rileggi tutti i tag» (i brani si riscrivono con la chiave dei
    // tag, e la ricostruzione le rimappa tutte). Qui le si sporca con una `x`
    // davanti, e la ricostruzione le riporta dov'erano.
    for giro in 1..=3 {
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .expect("transazione");
        let inizio = std::time::Instant::now();
        let sporcate = tx
            .execute(
                "UPDATE tracks SET album_key = 'x' || album_key
                  WHERE album_key LIKE 'mbrg:%' OR album_key LIKE 'mbr:%'
                     OR album_key LIKE 'sp:%'",
                [],
            )
            .expect("chiavi da rimappare");
        let preparazione = inizio.elapsed().as_millis();
        let inizio = std::time::Instant::now();
        rymembra_app::library::rebuild_aggregates(&tx).expect("ricostruzione");
        let ms = inizio.elapsed().as_millis();
        let rimaste: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM tracks WHERE album_key LIKE 'x%'",
                [],
                |r| r.get(0),
            )
            .expect("rimaste");
        tx.rollback().expect("annullamento");
        println!(
            "giro {giro} con {sporcate} brani da rimappare: sporcarli {preparazione} ms, \
             ricostruzione intera {ms} ms ({rimaste} rimasti da rimappare)"
        );
    }
}
