//! L'account Spotify, dal consenso alla libreria.
//!
//! La via A, come `archivio.rs` è la via B — e finiscono nello stesso posto:
//! tutte e due producono un `AccountSnapshot` e lo passano a
//! `aether_app::import_account`, che non sa da quale delle due sia arrivato.
//!
//! **Senza `--esegui` non scrive niente**: legge l'account, mostra il piano e se
//! ne va.
//!
//!     cargo run --release -p aether-app --example spotify_account -- <client-id> [cartella-dati] [--esegui]
//!
//! Il `client id` si prende anche da `AETHER_SPOTIFY_CLIENT_ID`.
//!
//! # Come si ottiene un client id
//!
//! Su <https://developer.spotify.com/dashboard>: «Create app», e come **Redirect
//! URI** si scrive `http://127.0.0.1` — senza porta. Non è un'omissione: la
//! RFC 8252 § 7.3 permette a un'applicazione installata di scegliere una porta
//! libera a ogni collegamento, e Spotify lo ammette esplicitamente. `localhost`
//! invece lo rifiuta.
//!
//! Serve che il proprietario dell'applicazione abbia **Spotify Premium attivo**:
//! dal febbraio 2026 è un requisito delle applicazioni in Development Mode, e
//! quando scade smettono di funzionare senza nessun avviso. È il motivo per cui
//! l'altra via esiste.
//!
//! # Il refresh token, qui, non si salva
//!
//! E il consenso si rifà a ogni esecuzione. Non è una semplificazione da
//! copiare: un token scritto in chiaro accanto al database sarebbe esattamente
//! quel che `aether_oauth::portachiavi` esiste per non fare. La finestra lo
//! metterà nel portachiavi di sistema; un esempio da riga di comando non ha
//! nessun bisogno di tenerselo.

#![allow(clippy::expect_used, clippy::print_stdout, clippy::print_stderr)]

use std::time::Duration;

use aether_app::import_account::AccountImportReport;
use aether_domain::spotify_account::Scelte;
use aether_spotify::account;

fn main() {
    let mut client_id = std::env::var("AETHER_SPOTIFY_CLIENT_ID").ok();
    let mut dati = None;
    let mut esegui = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--esegui" => esegui = true,
            _ if client_id.is_none() => client_id = Some(arg),
            _ if dati.is_none() => dati = Some(arg),
            _ => {}
        }
    }
    let Some(client_id) = client_id.filter(|id| !id.trim().is_empty()) else {
        eprintln!("uso: spotify_account <client id> [cartella dati] [--esegui]");
        eprintln!("     oppure AETHER_SPOTIFY_CLIENT_ID=… cargo run …");
        std::process::exit(2);
    };
    let dati = dati.unwrap_or_else(|| {
        std::env::temp_dir()
            .join("aether-libreria")
            .to_string_lossy()
            .into_owned()
    });

    let rete = aether_net::Rete::nuova("spotify", account::SCADENZA);

    // ── il consenso ──
    println!("Apro il consenso nel browser. Hai tre minuti.");
    let token = match account::oauth::collega(&rete, &client_id, apri) {
        Ok(token) => token,
        Err(err) => {
            eprintln!("collegamento fallito: {}", err.code().kind().code());
            if let Some(causa) = err.cause() {
                eprintln!("  {causa}");
            }
            std::process::exit(1);
        }
    };
    println!("Collegato.");

    // ── la lettura ──
    let lettura = match account::leggi(&rete, &token.access_token, mostra) {
        Ok(lettura) => lettura,
        Err(err) => {
            eprintln!("\nlettura fallita: {}", err.code().kind().code());
            std::process::exit(1);
        }
    };

    let snapshot = lettura.snapshot;
    println!("\n-- account --");
    println!(
        "profilo     {}",
        snapshot.profilo.as_deref().unwrap_or("(sconosciuto)")
    );
    match lettura.premium {
        Some(true) => println!("abbonamento Premium"),
        // Il proprietario dell'applicazione deve averlo, e per chi la usa da
        // solo sono la stessa persona. Dirlo prima è la differenza fra un
        // avviso e un guasto senza spiegazione, fra un mese.
        Some(false) => println!(
            "abbonamento NON Premium — se questa è anche l'app che hai registrato,\n\
             smetterà di funzionare: dal febbraio 2026 il Development Mode lo richiede"
        ),
        None => println!("abbonamento (non dichiarato)"),
    }
    println!("playlist    {}", snapshot.playlist.len());
    println!("preferiti   {}", snapshot.preferiti.len());
    println!("album       {}", snapshot.album.len());
    println!("artisti     {}", snapshot.artisti.len());
    println!(
        "cronologia  {} righe (l'API ne dà 50)",
        snapshot.cronologia.len()
    );
    if lettura.podcast > 0 {
        println!("podcast     {} (fuori: non è musica)", lettura.podcast);
    }
    if !lettura.senza_contenuto.is_empty() {
        // Dal marzo 2026 i brani si leggono solo delle playlist possedute o
        // collaborative. Una playlist vuota senza spiegazione sembra un guasto
        // dell'abbinamento, e non lo è.
        println!(
            "\nsenza contenuto ({}): sono playlist che segui e non possiedi,\n\
             e Spotify non ne dà più i brani.",
            lettura.senza_contenuto.len()
        );
        for nome in lettura.senza_contenuto.iter().take(10) {
            println!("  {nome}");
        }
    }
    if snapshot.e_vuoto() {
        println!("\nNiente da importare.");
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

    let piano = aether_app::import_account::plan(&mut connection, &snapshot, &Scelte::default())
        .expect("piano");
    println!("\n-- piano --");
    stampa(&piano);

    if !esegui {
        println!("\nSenza `--esegui` non ho scritto niente.");
        return;
    }

    let fatto = aether_app::import_account::import(&mut connection, &snapshot, &Scelte::default())
        .expect("importazione");
    println!("\n-- fatto --");
    stampa(&fatto);
}

/// Mostra a che punto è la lettura.
///
/// Una riga che si riscrive invece di scorrere: un account da duecento playlist
/// sono duecento righe, e chi guarda vuole sapere a che punto è, non leggere
/// l'elenco.
fn mostra(passo: &account::Avanzamento) {
    match (passo.totali, &passo.nome) {
        (Some(totali), Some(nome)) if totali > 1 => {
            println!("  {} {}/{} — {nome}", passo.fase, passo.fatti + 1, totali);
        }
        _ => println!("  {}…", passo.fase),
    }
}

/// Apre l'indirizzo del consenso nel browser di sistema.
///
/// Ci prova, e comunque lo stampa: un esempio che non riesce ad aprire una
/// finestra deve poter essere finito a mano, non fallire. Nella finestra vera lo
/// fa l'opener di Tauri.
fn apri(url: &str) -> Result<(), aether_domain::errors::AppError> {
    println!("\n{url}\n");
    let esito = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if esito.is_err() {
        println!("(non sono riuscito ad aprirlo: incollalo nel browser)");
    }
    // Tre minuti di attesa cominciano adesso.
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
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
    // Il gradino zero, che da questa via arriva davvero: il lettore keyless
    // l'ISRC non lo vede più da nessun livello.
    let per_isrc: usize = rapporto
        .playlists
        .iter()
        .chain(rapporto.albums.iter())
        .chain(std::iter::once(&rapporto.liked))
        .map(|r| r.matched_isrc)
        .sum();
    println!("  per ISRC  {per_isrc}");
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
        println!("  (la cronologia completa sta solo nell'archivio: usa `--example archivio`)");
    }
}
