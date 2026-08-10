//! Arricchisce una libreria vera, o dice soltanto cosa farebbe.
//!
//! È la via per provare un pezzo che tocca la rete senza aprire l'applicazione
//! e senza aspettare novanta secondi che il filo di sottofondo si svegli:
//!
//!     cargo run -q -p aether-app --example arricchisci -- <cartella dati> --prova
//!     cargo run -q -p aether-app --example arricchisci -- <cartella dati>
//!
//! La cartella dati è quella che contiene `aether.db` e `copertine/` — su
//! Windows `%APPDATA%\<identificatore>`; la riga `[avvio]` della console la
//! stampa a ogni apertura.
//!
//! # `--prova` prima, sempre
//!
//! Con `--prova` si decide e si stampa, e **non si scrive niente**: né i tag,
//! né le righe, né `enrich_undo`. È il modo di guardare la taratura prima di
//! affidarle millequattrocento file, e quel che conta guardare non sono gli
//! applicati — sono gli **astenuti**: se un disco che riconosceresti a occhio
//! finisce lì, la soglia è troppo stretta; se ce ne finisce dentro uno che non
//! c'entra niente, è troppo larga.
//!
//! Con l'applicazione **chiusa**. Due processi che scrivono sullo stesso
//! database vanno d'accordo — sta in WAL — ma non sui file: una passata di qui
//! e una di là riscriverebbero gli stessi tag, e i tag di prima finirebbero in
//! `enrich_undo` una volta sola, cioè per uno dei due soltanto.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stdout)]

use std::path::PathBuf;

use aether_app::covers::CoverStore;
use aether_app::enrich::{self, DepositoSqlite, Gruppo};
use aether_domain::enrich::Verdetto;
use aether_meta::Fornitori;

/// L'orologio, come lo porta l'applicazione: `aether-app` non ne espone uno.
fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

/// Quanti gruppi si guardano per volta.
///
/// Molti più dei dodici della passata automatica: quella deve potersi fermare
/// senza lasciare niente a metà, questa gira apposta e chi l'ha lanciata sa
/// aspettare.
const QUANTI: usize = 60;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dati) = args.next().map(PathBuf::from) else {
        eprintln!("uso: arricchisci <cartella dati> [--prova]");
        std::process::exit(2);
    };
    let prova = args.any(|a| a == "--prova");

    let aperto = aether_app::db::open(&dati.join("aether.db")).expect("apertura del database");
    let mut connection = aperto.connection;
    let covers = CoverStore::open(dati.join("copertine")).expect("store delle copertine");

    let adesso = now_ms();
    let mancano = enrich::quanti_mancano(&connection, adesso).expect("conteggio");
    let gruppi = enrich::candidati(&connection, adesso, QUANTI).expect("candidati");
    println!(
        "{} brani da arricchire, {} gruppi in questo lotto{}\n",
        mancano,
        gruppi.len(),
        if prova {
            " — a vuoto, non si scrive"
        } else {
            ""
        }
    );

    let deposito = DepositoSqlite::apri(&dati.join("aether.db"), adesso).expect("deposito");
    let fornitori = Fornitori::nuovo(Box::new(deposito));

    let mut applicati = 0usize;
    let mut astenuti = 0usize;
    let mut nessuno = 0usize;

    for gruppo in &gruppi {
        let decisione = enrich::decidi(&fornitori, gruppo);
        if decisione.non_raggiungibile {
            println!("— rete assente, mi fermo qui —");
            break;
        }
        stampa(gruppo, &decisione);

        applicati = applicati.saturating_add(decisione.scritture.len());
        astenuti = astenuti.saturating_add(decisione.astensioni.len());
        nessuno = nessuno.saturating_add(
            decisione
                .astensioni
                .iter()
                .filter(|(_, verdetto)| *verdetto == Verdetto::Nessuno)
                .count(),
        );

        if prova {
            continue;
        }

        let (esiti, guasti) = enrich::scrivi_file(&covers, &decisione.scritture, true);
        for guasto in &guasti {
            println!("   ! file non scritto: {guasto}");
        }
        let tx = connection.transaction().expect("transazione");
        enrich::registra(&tx, &decisione, &decisione.scritture, &esiti, now_ms())
            .expect("registrazione");
        for scrittura in &decisione.scritture {
            if !esiti.iter().any(|e| e.track_id == scrittura.track_id) {
                enrich::segna_errore(&tx, scrittura.track_id, now_ms()).expect("guasto annotato");
            }
        }
        tx.commit().expect("commit");
    }

    if !prova && applicati > 0 {
        // La stessa ragione del filo: l'arricchimento scrive
        // `mb_release_group_id`, ed è l'identificativo con cui i gruppi d'album
        // si fondono. Senza, i dischi appena riconosciuti come uno solo
        // resterebbero in due schede.
        let tx = connection.transaction().expect("transazione");
        enrich::ricostruisci(&tx).expect("ricostruzione");
        tx.commit().expect("commit");
    }

    println!(
        "\napplicati {applicati} · astenuti {astenuti} (di cui {nessuno} senza corrispondenza)"
    );
    if prova {
        println!("niente è stato scritto: rilancia senza --prova per applicare");
    }
}

/// Cosa si è deciso su un gruppo, in una forma che si legge di fila.
fn stampa(gruppo: &Gruppo, decisione: &enrich::Decisione) {
    println!("{} — {}", gruppo.artista, gruppo.titolo);
    for scrittura in &decisione.scritture {
        let campi = &scrittura.campi;
        // Solo i campi che cambierebbero davvero: stampare gli `None` farebbe
        // scorrere quattro righe di niente per ogni brano già a posto.
        let mut pezzi: Vec<String> = Vec::new();
        if let Some(titolo) = &campi.title {
            pezzi.push(format!("titolo «{titolo}»"));
        }
        if let Some(artista) = &campi.artist {
            pezzi.push(format!("interprete «{artista}»"));
        }
        if let Some(album) = &campi.album {
            pezzi.push(format!("album «{album}»"));
        }
        if let Some(anno) = campi.year {
            pezzi.push(format!("anno {anno}"));
        }
        if let Some(genere) = &campi.genre {
            pezzi.push(format!("genere «{genere}»"));
        }
        if scrittura.copertina.is_some() {
            pezzi.push("+ copertina".to_owned());
        }
        println!(
            "   ✓ {} [{} {:.2}] {}",
            scrittura.track_id,
            scrittura.fonte,
            scrittura.confidenza,
            if pezzi.is_empty() {
                "già a posto".to_owned()
            } else {
                pezzi.join(", ")
            }
        );
    }
    for (track_id, verdetto) in &decisione.astensioni {
        println!("   · {track_id} {}", verdetto.as_str());
    }
}
