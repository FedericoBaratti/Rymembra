//! Due dispositivi, una cartella, nessun server.
//!
//! È la dimostrazione che M0 funziona, e si guarda con due terminali aperti sulla
//! stessa cartella:
//!
//!     cargo run -p aether-sync --example due_dispositivi -- --cartella ./prova --io portatile
//!     cargo run -p aether-sync --example due_dispositivi -- --cartella ./prova --io telefono
//!
//! Ogni esecuzione fa un gesto e poi una passata, e stampa quel che vede. Con
//! `--ascolta <brano>` conta un ascolto, con `--voto <brano>=<0-5>` mette un voto,
//! con `--accoda <playlist>=<brano>` aggiunge alla playlist. Senza gesti fa solo
//! la passata e riferisce.
//!
//! Lo stato di ciascun dispositivo sta in un file suo dentro la cartella dati
//! locale — qui, per semplicità, un JSON accanto alla cartella condivisa. Nella
//! vera applicazione quel posto è il database, e questo esempio esiste proprio per
//! mostrare che al motore non interessa quale dei due sia.
//!
//! La cartella si può mettere dentro Syncthing, dentro iCloud Drive, dentro
//! OneDrive o dentro la cartella di Google Drive: per Aether sono la stessa cosa,
//! ed è il motivo per cui `Cartella` è il primo magazzino e non l'ultimo.

#![allow(
    clippy::expect_used,
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "un esempio da riga di comando: stampare è quel che fa, e un guasto deve fermarlo"
)]

use std::path::PathBuf;

use aether_sync::documento::Contenuto;
use aether_sync::registro::Voto;
use aether_sync::{Avanzamento, Cartella, Memoria, Motore, PlaylistSincronizzata};

fn main() {
    let mut cartella: Option<String> = None;
    let mut io: Option<String> = None;
    let mut ascolta: Option<String> = None;
    let mut voto: Option<String> = None;
    let mut accoda: Option<String> = None;

    let mut argomenti = std::env::args().skip(1);
    while let Some(arg) = argomenti.next() {
        match arg.as_str() {
            "--cartella" => cartella = argomenti.next(),
            "--io" => io = argomenti.next(),
            "--ascolta" => ascolta = argomenti.next(),
            "--voto" => voto = argomenti.next(),
            "--accoda" => accoda = argomenti.next(),
            altro => {
                eprintln!("argomento sconosciuto: {altro}");
                std::process::exit(2);
            }
        }
    }

    let (Some(cartella), Some(io)) = (cartella, io) else {
        eprintln!(
            "uso: due_dispositivi --cartella <dove> --io <nome> \
             [--ascolta <brano>] [--voto <brano>=<0-5>] [--accoda <playlist>=<brano>]"
        );
        std::process::exit(2);
    };

    let radice = PathBuf::from(&cartella);
    let mio_stato = radice.join(format!("stato-{io}.json"));
    let magazzino = Cartella::nuova(radice.join(aether_sync::documento::CARTELLA));

    // Quel che questo dispositivo sa: nella vera applicazione lo legge dal
    // database, qui da un file accanto.
    let mut mio: Contenuto = std::fs::read(&mio_stato)
        .ok()
        .and_then(|byte| serde_json::from_slice(&byte).ok())
        .unwrap_or_default();

    let adesso = adesso_ms();
    let mut fatto = Vec::new();

    if let Some(brano) = ascolta {
        *mio.ascolti.entry(brano.clone()).or_insert(0) += 1;
        mio.ultimo.insert(brano.clone(), adesso);
        fatto.push(format!("ascoltato «{brano}»"));
    }

    if let Some(coppia) = voto {
        let (brano, quanto) = coppia.split_once('=').unwrap_or((coppia.as_str(), "5"));
        let v: u8 = quanto.parse().unwrap_or(5).min(5);
        mio.voti.insert(brano.to_owned(), Voto { v, at: adesso });
        fatto.push(format!("votato «{brano}» {v} stelle"));
    }

    if let Some(coppia) = accoda {
        let (quale, brano) = coppia
            .split_once('=')
            .unwrap_or((coppia.as_str(), "senza-nome"));
        let playlist =
            mio.playlist
                .entry(quale.to_owned())
                .or_insert_with(|| PlaylistSincronizzata {
                    nome: quale.to_owned(),
                    at: adesso,
                    creata_at: adesso,
                    ..PlaylistSincronizzata::default()
                });
        playlist.sequenza.accoda(&io, brano);
        fatto.push(format!("aggiunto «{brano}» a «{quale}»"));
    }

    let mut memoria_motore = Memoria::nuova();
    let mut motore = Motore::nuovo(&magazzino, io.clone(), &mut memoria_motore);
    let esito = match motore.passata(&mio, adesso, &avanzando) {
        Ok(esito) => esito,
        Err(err) => {
            eprintln!("la passata è fallita: {err}");
            std::process::exit(1);
        }
    };

    if let Ok(byte) = serde_json::to_vec_pretty(&mio) {
        let _ = std::fs::write(&mio_stato, byte);
    }

    println!("── {io} ──────────────────────────────────────────");
    for cosa in &fatto {
        println!("  fatto: {cosa}");
    }
    println!(
        "  passata: {} letti, {} saltati, {}",
        esito.passata.letti,
        esito.passata.saltati,
        if esito.passata.scritto {
            "documento riscritto"
        } else {
            "niente da riscrivere"
        }
    );
    for guasto in &esito.passata.guasti {
        println!("  guasto su «{}»: {}", guasto.nome, guasto.perche);
    }

    let fuso = &esito.fuso;
    println!("  dispositivi visti: {}", fuso.dispositivi.join(", "));

    if !fuso.ascolti.is_empty() {
        println!("  ascolti:");
        for (brano, contatore) in &fuso.ascolti {
            let da: Vec<String> = contatore
                .dispositivi()
                .map(|(chi, quanti)| format!("{chi} {quanti}"))
                .collect();
            println!(
                "    {brano}: {} in tutto  ({})",
                contatore.totale(),
                da.join(", ")
            );
        }
    }

    if !fuso.voti.is_empty() {
        println!("  voti:");
        for (brano, voto) in &fuso.voti {
            println!("    {brano}: {} stelle", voto.v);
        }
    }

    for (chiave, playlist) in &fuso.playlist {
        println!(
            "  playlist «{chiave}»: {}",
            playlist.sequenza.ordine().join(" → ")
        );
    }
}

/// L'avanzamento, stampato solo quando c'è davvero qualcosa da guardare.
fn avanzando(passo: Avanzamento) {
    if passo.totale > 3 && passo.fatti < passo.totale {
        println!("  … {} di {}", passo.fatti, passo.totale);
    }
}

/// Adesso, in millisecondi.
///
/// Il dominio non ha un orologio — è chi orchestra a fornirlo — e in questo esempio
/// chi orchestra è questa funzione.
fn adesso_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|quanto| i64::try_from(quanto.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
