//! Il banco di collaudo del backup su Drive, da riga di comando.
//!
//! Esiste perché OAuth si mette a punto **senza** ricompilare un'applicazione
//! Tauri a ogni tentativo. Un giro di consenso richiede un browser, un socket e
//! trenta secondi di attenzione umana: farlo dentro l'app significherebbe
//! ricostruire il frontend, riavviare la finestra e ricliccare tre schermate a
//! ogni virgola cambiata.
//!
//!     $env:AETHER_GOOGLE_CLIENT_ID     = "....apps.googleusercontent.com"
//!     $env:AETHER_GOOGLE_CLIENT_SECRET = "GOCSPX-..."
//!
//!     cargo run -p aether-cloud --example nuvola -- collega
//!     cargo run -p aether-cloud --example nuvola -- elenca
//!     cargo run -p aether-cloud --example nuvola -- scollega
//!     cargo run -p aether-cloud --example nuvola -- salva      --dati <cartella>
//!     cargo run -p aether-cloud --example nuvola -- piano      --dati <cartella>
//!     cargo run -p aether-cloud --example nuvola -- ripristina --dati <cartella> --davvero
//!
//! `--dati` è la cartella dell'applicazione, quella che la riga di avvio stampa:
//! su Windows `%APPDATA%\io.github.federicobaratti.aether`.
//!
//! `piano` stampa e non applica. `ripristina` **rifiuta** di fare qualcosa senza
//! `--davvero`: è il comando che tocca il database vero di chi lo lancia, e la
//! distanza fra «guardiamo cosa farebbe» e «fallo» dev'essere una parola
//! scritta a mano.
//!
//! Le credenziali si leggono dall'ambiente **a ogni avvio**, non compilate
//! dentro: qui si sta provando, e provare vuol dire cambiarle senza ricostruire
//! niente. Nell'applicazione vera arrivano da `build.rs`.
//!
//! Il token di aggiornamento finisce nel portachiavi di sistema, nella stessa
//! voce che userà l'applicazione. È voluto: dopo un `collega` da qui, Aether
//! trova l'account già collegato.

// Questi esempi sono strumenti diagnostici, eseguiti a mano da chi sviluppa.
// Qui un guasto DEVE fermare tutto rumorosamente: un esempio che prosegue su un
// errore riporta numeri sbagliati, e i numeri sono l'unica cosa che produce.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::Duration;

use aether_app::backup::{self, StatoLocale};
use aether_cloud::drive::Drive;
use aether_cloud::http::Rete;
use aether_cloud::oauth::{self, Credenziali};
use aether_cloud::pacchetti;
use aether_cloud::portachiavi::{self, DiSistema, Portachiavi as _};
use aether_cloud::servizio;
use aether_domain::errors::AppError;
use aether_domain::paths::PathRules;
use aether_domain::restore::{RestoreInput, RestorePlan, plan_restore};

/// La scadenza dei trasferimenti verso Drive.
///
/// Due minuti, non i quindici secondi dei punti OAuth: qui può passare una skin
/// da venti megabyte su una connessione lenta.
const SCADENZA_DRIVE: Duration = Duration::from_secs(120);

fn main() {
    let comando = std::env::args().nth(1).unwrap_or_default();
    let esito = match comando.as_str() {
        "collega" => collega(),
        "elenca" => elenca(),
        "scollega" => scollega(),
        "salva" => salva(),
        "piano" => ripristina(false),
        "ripristina" => ripristina(true),
        altro => {
            if !altro.is_empty() {
                eprintln!("comando sconosciuto: {altro}\n");
            }
            eprintln!("uso: nuvola <collega | elenca | scollega> ");
            eprintln!("     nuvola <salva | piano> --dati <cartella>");
            eprintln!("     nuvola ripristina --dati <cartella> --davvero");
            std::process::exit(2);
        }
    };
    if let Err(err) = esito {
        eprintln!("\n✗ {err}");
        std::process::exit(1);
    }
}

/// Il valore di un'opzione `--nome valore`.
fn opzione(nome: &str) -> Option<String> {
    let argomenti: Vec<String> = std::env::args().collect();
    let posizione = argomenti.iter().position(|arg| arg == nome)?;
    argomenti.get(posizione.saturating_add(1)).cloned()
}

/// C'è una bandiera `--nome`?
fn bandiera(nome: &str) -> bool {
    std::env::args().any(|arg| arg == nome)
}

/// La cartella dati, e il nome di questo dispositivo.
fn cartella_dati() -> PathBuf {
    match opzione("--dati") {
        Some(percorso) if !percorso.trim().is_empty() => PathBuf::from(percorso),
        _ => {
            eprintln!("manca --dati <cartella>.");
            eprintln!("È la cartella che la riga di avvio di Aether stampa,");
            eprintln!("su Windows di solito %APPDATA%\\io.github.federicobaratti.aether");
            std::process::exit(2);
        }
    }
}

/// Il nome con cui questo computer firma i backup.
///
/// Nell'applicazione vera è un identificativo casuale scritto una volta in
/// `settings`, perché due computer con lo stesso nome host non si scambino per
/// lo stesso dispositivo. Qui basta il nome della macchina: serve solo a far
/// scattare la fusione quando si prova con due cartelle dati diverse.
fn dispositivo() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "banco-di-prova".to_owned())
}

/// L'orologio in millisecondi.
fn adesso_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|quanto| i64::try_from(quanto.as_millis()).ok())
        .unwrap_or(0)
}

/// Una passata di salvataggio sulla cartella dati vera.
fn salva() -> Result<(), AppError> {
    let dati = cartella_dati();
    let aperto = aether_app::db::open(&dati.join("aether.db"))?;
    let stato = backup::stato_locale(&aperto.connection)?;
    // La connessione si chiude **prima** della rete: è l'invariante che tiene il
    // lettore libero mentre il backup carica, ed è la stessa che nell'app è
    // garantita dal fatto che `aether-cloud` non riceve mai una connessione.
    drop(aperto.connection);

    println!(
        "Da salvare: {} brani con statistiche, {} playlist, {} skin, {} bozze.",
        stato.tracks.len(),
        stato.playlists.len(),
        pacchetti::elenca_skin(&dati).len(),
        pacchetti::elenca_bozze(&dati).len()
    );

    let drive = Drive::nuovo(Rete::nuova("drive", SCADENZA_DRIVE), &accesso()?);
    let passata = servizio::salva(
        &drive,
        &dati,
        &stato,
        &dispositivo(),
        adesso_ms(),
        |passo| {
            println!("  {} {}/{}", passo.cosa.nome(), passo.fatti, passo.totale);
        },
    )?;

    println!(
        "\n✓ {} caricati, {} già lassù identici.",
        passata.caricati, passata.saltati
    );
    println!("  impronta: {}", passata.impronta);
    if passata.caricati == 0 {
        println!("  (nessun caricamento: è la scorciatoia dell'impronta che funziona)");
    }
    Ok(())
}

/// Il piano di ripristino, e — solo con `--davvero` — la sua applicazione.
fn ripristina(applica: bool) -> Result<(), AppError> {
    if applica && !bandiera("--davvero") {
        eprintln!("«ripristina» tocca il database vero: aggiungi --davvero.");
        eprintln!("Per vedere solo cosa farebbe: «piano».");
        std::process::exit(2);
    }

    let dati = cartella_dati();
    let mut aperto = aether_app::db::open(&dati.join("aether.db"))?;
    let qui = backup::stato_locale(&aperto.connection)?;
    drop(aperto.connection);

    let drive = Drive::nuovo(Rete::nuova("drive", SCADENZA_DRIVE), &accesso()?);
    let Some(da) = servizio::scarica_salvataggio(&drive)? else {
        println!("Su Drive non c'è ancora nessun backup.");
        return Ok(());
    };
    println!("Backup del {} da «{}».\n", da.generato_ms, da.generato_da);

    let piano = pianifica(&dati, &qui, &da);
    stampa_piano(&piano);
    if piano.is_empty() {
        println!("\nNiente da ripristinare.");
        return Ok(());
    }
    if !applica {
        println!("\n(solo un'anteprima: «ripristina --davvero» per applicarlo)");
        return Ok(());
    }

    // Prima i file, poi il database: se si committasse la transazione e poi un
    // download fallisse, resterebbe scritto uno `skin.active` che punta al nulla.
    let scritti = servizio::scarica_pacchetti(&drive, &dati, &da, &piano, |passo| {
        println!("  {} {}/{}", passo.cosa.nome(), passo.fatti, passo.totale);
    })?;

    // Si **ricalcola** il piano invece di applicare quello mostrato: fra
    // l'anteprima e la conferma può essere finita una scansione, e le skin
    // appena scritte cambiano cosa c'è da fare.
    aperto = aether_app::db::open(&dati.join("aether.db"))?;
    let qui = backup::stato_locale(&aperto.connection)?;
    let definitivo = pianifica(&dati, &qui, &da);
    let mut connessione = aperto.connection;
    let applicato = backup::applica(&mut connessione, &definitivo)?;

    println!(
        "\n✓ {} brani, {} playlist, {} cartelle, {} skin, {} bozze.",
        applicato.tracks, applicato.playlists, applicato.roots, scritti.skin, scritti.bozze
    );
    if applicato.active_skin {
        println!("  skin attiva cambiata.");
    }
    for mancante in &scritti.mancanti {
        println!("  ⚠ mancava su Drive: {mancante}");
    }
    Ok(())
}

/// Costruisce il piano dai due lati.
fn pianifica(
    dati: &std::path::Path,
    qui: &StatoLocale,
    da: &servizio::DaRipristinare,
) -> RestorePlan {
    let dal_backup = backup::dal_salvataggio(&da.contenuto, &qui.tombstones);
    // Il dominio non guarda il disco: chi ha il filesystem decora ogni cartella
    // con la sua esistenza, e chi decide la usa.
    let esistono: Vec<bool> = dal_backup
        .roots
        .iter()
        .map(|radice| std::path::Path::new(radice).is_dir())
        .collect();
    let skin_locali: Vec<String> = pacchetti::elenca_skin(dati)
        .into_keys()
        .chain(std::iter::once(pacchetti::DI_SERIE.to_owned()))
        .collect();
    let bozze_locali: Vec<String> = pacchetti::elenca_bozze(dati).into_keys().collect();

    plan_restore(&RestoreInput {
        local_tracks: &qui.tracks,
        backup_tracks: &dal_backup.tracks,
        local_playlists: &qui.playlists,
        backup_playlists: &dal_backup.playlists,
        local_roots: &qui.roots,
        backup_roots: &dal_backup.roots,
        backup_roots_exist: &esistono,
        local_skins: &skin_locali,
        backup_skins: &dal_backup.skins,
        local_drafts: &bozze_locali,
        backup_drafts: &dal_backup.drafts,
        local_active_skin: qui.active_skin.as_deref(),
        backup_active_skin: dal_backup.active_skin.as_deref(),
        path_rules: PathRules::for_current_platform(),
    })
}

/// Stampa il piano nella stessa forma che la finestra dovrà mostrare.
fn stampa_piano(piano: &RestorePlan) {
    println!(
        "BRANI       {} da aggiornare, {} invariati, {} senza file qui",
        piano.tracks.len(),
        piano.tracks_unchanged,
        piano.tracks_absent.len()
    );
    for cambio in piano.tracks.iter().take(10) {
        println!(
            "   {} · ascolti {} → {} · voto {} → {}",
            cambio.key,
            cambio.before.play_count,
            cambio.after.play_count,
            cambio.before.rating,
            cambio.after.rating
        );
    }
    if piano.tracks.len() > 10 {
        println!("   … e altri {}", piano.tracks.len() - 10);
    }

    println!(
        "PLAYLIST    {} da scrivere, {} invariate",
        piano.playlists.len(),
        piano.playlists_unchanged
    );
    for cambio in &piano.playlists {
        println!(
            "   {} · {} · {} brani su {} presenti qui",
            cambio.playlist.name,
            if cambio.is_new {
                "da creare"
            } else {
                "da aggiornare"
            },
            cambio.playlist.members.len(),
            cambio.members_in_backup
        );
    }

    println!("CARTELLE    {} da aggiungere", piano.roots_to_add.len());
    for radice in &piano.roots_to_add {
        println!(
            "   {} {}",
            radice.path,
            if radice.exists {
                ""
            } else {
                "(non esiste più)"
            }
        );
    }

    println!(
        "SKIN        {} da installare, {} già presenti",
        piano.skins_to_install.len(),
        piano.skins_present
    );
    println!(
        "BOZZE       {} da scrivere, {} già presenti",
        piano.drafts_to_write.len(),
        piano.drafts_present
    );
    if let Some(skin) = &piano.active_skin {
        println!("SKIN ATTIVA {skin}");
    }
}

/// Le credenziali del client, dall'ambiente.
fn credenziali() -> Credenziali {
    let leggi = |nome: &str| match std::env::var(nome) {
        Ok(valore) if !valore.trim().is_empty() => valore,
        _ => {
            eprintln!("manca la variabile d'ambiente {nome}.");
            eprintln!("Si prendono dalla Console di Google, sezione Credentials,");
            eprintln!("da un client OAuth di tipo «Desktop app».");
            std::process::exit(2);
        }
    };
    Credenziali {
        client_id: leggi("AETHER_GOOGLE_CLIENT_ID"),
        client_secret: leggi("AETHER_GOOGLE_CLIENT_SECRET"),
    }
}

/// Apre l'indirizzo nel browser di sistema.
///
/// E lo **stampa comunque**: su una macchina senza browser predefinito, o
/// dentro una sessione remota, l'apertura automatica non funziona e l'unico modo
/// di andare avanti è copiare l'indirizzo a mano.
fn apri_browser(url: &str) -> Result<(), AppError> {
    println!("\nSe il browser non si apre da solo, apri questo indirizzo:\n\n{url}\n");

    #[cfg(target_os = "windows")]
    let esito = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let esito = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let esito = std::process::Command::new("xdg-open").arg(url).spawn();

    if esito.is_err() {
        println!("(non sono riuscito ad aprirlo: copialo a mano)");
    }
    Ok(())
}

/// Il giro di consenso completo, e il token nel portachiavi.
fn collega() -> Result<(), AppError> {
    // Le credenziali per prime: annunciare il browser e poi accorgersi che
    // manca una variabile d'ambiente lascia chi guarda a chiedersi se il
    // consenso sia partito davvero.
    let credenziali = credenziali();
    let rete = Rete::nuova("google", oauth::SCADENZA);
    println!("Apro il consenso di Google. Hai tre minuti.");

    let token = oauth::collega(&rete, &credenziali, apri_browser)?;

    let Some(refresh) = token.refresh_token.as_deref() else {
        // Senza `prompt=consent` Google non lo rimanda al secondo consenso, e
        // il backup automatico smetterebbe di funzionare dopo un'ora senza dire
        // niente. Se compare questo messaggio, è quel parametro a mancare.
        eprintln!("Google non ha mandato un token di aggiornamento.");
        eprintln!("Senza, il backup automatico smette dopo un'ora.");
        std::process::exit(1);
    };
    DiSistema.scrivi(portachiavi::GOOGLE_REFRESH_TOKEN, refresh)?;

    println!("\n✓ Collegato.");
    if let Some(email) = &token.email {
        println!("  account: {email}");
    }
    println!(
        "  il token di aggiornamento è nel portachiavi, sotto «{}».",
        portachiavi::SERVIZIO
    );
    Ok(())
}

/// Quel che c'è nella cartella privata dell'applicazione.
fn elenca() -> Result<(), AppError> {
    let drive = Drive::nuovo(Rete::nuova("drive", SCADENZA_DRIVE), &accesso()?);
    let file = drive.elenca()?;

    if file.is_empty() {
        println!("La cartella privata dell'app è vuota: non è ancora stato salvato niente.");
        return Ok(());
    }

    println!("{} file:\n", file.len());
    let mut totale = 0u64;
    for voce in &file {
        totale = totale.saturating_add(voce.byte);
        println!(
            "  {:<40} {:>10}  {}",
            voce.nome,
            leggibile(voce.byte),
            voce.impronta.as_deref().unwrap_or("(senza impronta)")
        );
    }
    println!("\n  {:<40} {:>10}", "totale", leggibile(totale));
    Ok(())
}

/// Revoca l'accesso e pulisce il portachiavi.
fn scollega() -> Result<(), AppError> {
    let rete = Rete::nuova("google", oauth::SCADENZA);
    let refresh = DiSistema.leggi(portachiavi::GOOGLE_REFRESH_TOKEN)?;

    // La revoca è best-effort: chi si scollega deve vedersi scollegato anche
    // senza rete, e la voce del portachiavi va pulita comunque.
    if let Some(token) = &refresh {
        match oauth::revoca(&rete, token) {
            Ok(()) => println!("Accesso revocato presso Google."),
            Err(err) => println!("Revoca non riuscita ({err}); pulisco lo stesso."),
        }
    }
    DiSistema.cancella(portachiavi::GOOGLE_REFRESH_TOKEN)?;
    println!("✓ Scollegato.");
    Ok(())
}

/// Un access token fresco, dal token di aggiornamento nel portachiavi.
fn accesso() -> Result<String, AppError> {
    let Some(refresh) = DiSistema.leggi(portachiavi::GOOGLE_REFRESH_TOKEN)? else {
        eprintln!("Nessun account collegato: lancia prima «collega».");
        std::process::exit(2);
    };
    let rete = Rete::nuova("google", oauth::SCADENZA);
    Ok(oauth::rinfresca(&rete, &credenziali(), &refresh)?.access_token)
}

/// Byte in una forma che si legge a colpo d'occhio.
fn leggibile(byte: u64) -> String {
    const SOGLIA: u64 = 1024;
    if byte < SOGLIA {
        return format!("{byte} B");
    }
    let kib = byte as f64 / 1024.0;
    if kib < 1024.0 {
        return format!("{kib:.1} KiB");
    }
    format!("{:.1} MiB", kib / 1024.0)
}
