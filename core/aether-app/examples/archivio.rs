//! L'archivio di Spotify, dallo zip alla libreria.
//!
//! Le due metà del lavoro si incontrano qui e solo qui: `aether-archivio` apre
//! il pacco e ne ricava un `AccountSnapshot`, `aether_app::import_account` lo
//! scrive. Nessuna delle due conosce l'altra — è la regola del ramo, e questo
//! esempio è la dimostrazione che si può rispettarla e avere comunque un
//! programma che funziona.
//!
//! **Senza `--esegui` non scrive niente**: mostra il piano e se ne va. Il piano
//! è l'importazione vera dentro una transazione abbandonata, quindi i numeri che
//! stampa sono quelli che si otterrebbero, non una previsione.
//!
//!     cargo run --release -p aether-app --example archivio -- <archivio.zip> [cartella-dati] [--esegui]
//!
//! Gli archivi che Spotify manda sono due e arrivano in momenti diversi — i dati
//! dell'account in qualche giorno, la cronologia estesa fino a trenta. Si
//! possono dare uno alla volta: quel che manca resta vuoto, e il rapporto lo
//! dice invece di far sembrare un guasto una metà che deve ancora arrivare.

#![allow(clippy::expect_used, clippy::print_stdout, clippy::print_stderr)]

use aether_app::import_account::AccountImportReport;
use aether_domain::spotify_account::Scelte;

fn main() {
    let mut zip = None;
    let mut dati = None;
    let mut esegui = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--esegui" => esegui = true,
            _ if zip.is_none() => zip = Some(arg),
            _ if dati.is_none() => dati = Some(arg),
            _ => {}
        }
    }
    let Some(zip) = zip else {
        eprintln!("uso: archivio <archivio.zip> [cartella dati] [--esegui]");
        std::process::exit(2);
    };
    let dati = dati.unwrap_or_else(|| {
        std::env::temp_dir()
            .join("aether-libreria")
            .to_string_lossy()
            .into_owned()
    });

    // ── il pacco ──
    let lettura = match aether_archivio::leggi(std::path::Path::new(&zip)) {
        Ok(lettura) => lettura,
        Err(err) => {
            eprintln!("l'archivio non si è aperto: {}", err.code().kind().code());
            std::process::exit(1);
        }
    };

    println!("── archivio ──");
    println!("letti       {}", lettura.letti.join(", "));
    if !lettura.ignorati.is_empty() {
        // «Ho letto 4 file e ne ho ignorati 6» è un'informazione; il silenzio no.
        println!("ignorati    {}", lettura.ignorati.join(", "));
    }
    for guasto in &lettura.illeggibili {
        println!("illeggibile {} — {}", guasto.nome, guasto.perche);
    }
    if lettura.righe_illeggibili > 0 {
        println!("righe storte {}", lettura.righe_illeggibili);
    }
    if lettura.podcast > 0 {
        println!("podcast     {} (fuori: non è musica)", lettura.podcast);
    }
    if lettura.non_musica.totale() > 0 {
        println!(
            "non musica  {} podcast seguiti, {} al bando, {} altro",
            lettura.non_musica.podcast, lettura.non_musica.al_bando, lettura.non_musica.altro
        );
    }

    let snapshot = lettura.snapshot;
    println!("\n── account ──");
    println!(
        "profilo     {}",
        snapshot.profilo.as_deref().unwrap_or("(sconosciuto)")
    );
    println!("playlist    {}", snapshot.playlist.len());
    println!("preferiti   {}", snapshot.preferiti.len());
    println!("album       {}", snapshot.album.len());
    println!("artisti     {}", snapshot.artisti.len());
    println!("cronologia  {} righe", snapshot.cronologia.len());
    if snapshot.e_vuoto() {
        println!("\nNiente da importare. È l'altro dei due archivi?");
        return;
    }

    // ── la libreria ──
    let cartella = std::path::Path::new(&dati);
    std::fs::create_dir_all(cartella).expect("cartella dati");
    let aperto = aether_app::db::open(&cartella.join("aether.db")).expect("database");
    let mut connection = aperto.connection;
    println!(
        "\ndatabase    versione {} → {} ({} migrazioni)",
        aperto.version_before,
        aether_app::db::LATEST_VERSION,
        aperto.applied
    );

    // ── il piano ──
    let piano = aether_app::import_account::plan(&mut connection, &snapshot, &Scelte::default())
        .expect("piano");
    println!("\n── piano ──");
    stampa(&piano);

    if !esegui {
        println!("\nSenza `--esegui` non ho scritto niente.");
        return;
    }

    let fatto = aether_app::import_account::import(&mut connection, &snapshot, &Scelte::default())
        .expect("importazione");
    println!("\n── fatto ──");
    stampa(&fatto);

    // I mancanti sono l'unica cosa che l'utente non può ricostruire dopo: vanno
    // detti, non contati. I primi venti bastano a capire se l'abbinamento sta
    // funzionando; l'elenco intero lo mostra la finestra.
    let mancanti: Vec<&aether_app::import_esterno::MissingTrack> = fatto
        .playlists
        .iter()
        .chain(fatto.albums.iter())
        .chain(std::iter::once(&fatto.liked))
        .flat_map(|r| r.missing_tracks.iter())
        .collect();
    if !mancanti.is_empty() {
        println!("\n── che non ho ── (primi 20 di {})", mancanti.len());
        for brano in mancanti.iter().take(20) {
            println!(
                "  {} — {}",
                brano.artist.as_deref().unwrap_or("?"),
                brano.title
            );
        }
    }
}

fn stampa(rapporto: &AccountImportReport) {
    println!("provenienza {}", rapporto.source);
    println!(
        "playlist    {} importate, {} saltate",
        rapporto.playlists.len(),
        rapporto.rejected_playlists.len()
    );
    for saltata in &rapporto.rejected_playlists {
        println!("  saltata «{}» — {}", saltata.name, saltata.code);
    }
    println!(
        "brani       {} ritrovati, {} no ({} righe in coda)",
        rapporto.matched(),
        rapporto.missing(),
        rapporto.wanted_rows()
    );
    println!("preferiti   {} segnati adesso", rapporto.liked_marked);
    println!(
        "album       {} visti, {} identificativi scritti",
        rapporto.albums_seen, rapporto.album_ids_written
    );
    println!(
        "artisti     {} visti, {} collegati",
        rapporto.artists_seen, rapporto.artists_linked
    );
    println!(
        "cronologia  {} ascolti, {} statistiche riallineate",
        rapporto.history_rows, rapporto.stats_updated
    );
    let scarti = rapporto.history_skipped;
    if scarti.total() > 0 {
        println!(
            "  fuori     {} doppioni, {} troppo brevi, {} non in libreria",
            scarti.duplicates, scarti.too_short, scarti.not_in_library
        );
    }
    if !rapporto.full_history {
        println!("  (la cronologia completa sta solo nell'archivio, non nell'API)");
    }
}
