//! Le misure di S1-2: quanta memoria costa aprire la testa di una risposta
//! ostile.
//!
//! Non sono prove di un comportamento: costruiscono una cornice compressa di
//! pochi kilobyte che si apre in 8 MiB, la passano a [`testa_entro`] — la porta
//! da cui entra ogni testa — e **scrivono** il picco di memoria del processo
//! (`println!`). Per questo sono `#[ignore]`, e si lanciano a mano, **una per
//! volta**: il picco di un processo non scende mai, e due misure nello stesso
//! processo si confonderebbero.
//!
//! ```text
//! cargo test -p rymembra-lontano --locked --lib protocollo::misure::misura_presenza_ostile -- --ignored --nocapture --exact
//! ```
//!
//! (con `--release` per i numeri di una build vera). Il picco lo chiede a
//! Windows (`PeakWorkingSet64` e `PeakPagedMemorySize64`, via PowerShell) o a
//! Linux (`VmHWM` e `VmPeak`): senza un allocatore contatore, che sarebbe
//! `unsafe`. Il «commit» è il numero che conta, perché una memoria azzerata e
//! non ancora toccata non compare nel working set.
//!
//! # I numeri del 25/11/2026
//!
//! Profilo di sviluppo, un processo per misura, picco **in più** rispetto a
//! quando la cornice è pronta. «Prima» è `373c60d`, dove `testa_entro` era
//! `valore_entro::<Testa>`: serde teneva l'oggetto intero come `Content`.
//!
//! | | prima: working set / commit / tempo | dopo |
//! |---|---|---|
//! | presenza ostile (8 MiB di `"z":[0,…]`) | +149,6 / +201,5 MiB / 548 ms | Err: +10,2 / +17,2 MiB / 278 ms |
//! | dati ostili (8 MiB di `[0,…]`) | +261,8 / +329,0 MiB / 770 ms | Err: +10,2 / +17,2 MiB / 283 ms |
//! | dati al limite dei nodi (il peggio che passa) | — | Ok: +19,4 / +26,2 MiB / 103 ms |
//! | pagina onesta da 500 brani (497 KB) | +1,9 / +0,6 MiB / 10 ms | Ok: +1,5 / +0,3 MiB / 16 ms |
//!
//! I ~10 MiB che restano sono il JSON aperto (8 MiB, il tetto delle risposte di
//! dati) e la sua copia di lavoro: non crescono con la cornice.

use std::time::Instant;

use super::*;

/// Il picco di memoria del processo fin qui, in byte: (working set, commit).
/// `None` dove non si sa chiederlo.
fn picco() -> Option<(u64, u64)> {
    if cfg!(windows) {
        let pid = std::process::id();
        let comando = format!(
            "$p = Get-Process -Id {pid}; \"$($p.PeakWorkingSet64) $($p.PeakPagedMemorySize64)\""
        );
        let fuori = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &comando])
            .output()
            .ok()?;
        let testo = String::from_utf8_lossy(&fuori.stdout).into_owned();
        let mut numeri = testo
            .split_whitespace()
            .filter_map(|n| n.parse::<u64>().ok());
        Some((numeri.next()?, numeri.next()?))
    } else {
        let stato = std::fs::read_to_string("/proc/self/status").ok()?;
        let kb = |chiave: &str| -> Option<u64> {
            let riga = stato.lines().find(|r| r.starts_with(chiave))?;
            Some(riga.split_whitespace().nth(1)?.parse::<u64>().ok()? * 1024)
        };
        Some((kb("VmHWM:")?, kb("VmPeak:")?))
    }
}

/// Deflate di `testo`, come lo manderebbe un pari ostile.
fn comprimi(testo: &str) -> Vec<u8> {
    let mut compressore =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    compressore.write_all(testo.as_bytes()).expect("in memoria");
    compressore.finish().expect("in memoria")
}

/// Un JSON `inizio` + `[0,0,…]` + `fine`, appena sotto gli 8 MiB aperti che
/// [`MASSIMA_TESTA`] concede.
fn un_json_di_zeri(inizio: &str, fine: &str) -> String {
    let mut json = String::from(inizio);
    json.push('[');
    let zeri = (MASSIMA_TESTA - inizio.len() - fine.len() - 8).div_euclid(2);
    json.push_str(&"0,".repeat(zeri));
    json.push('0');
    json.push(']');
    json.push_str(fine);
    assert!(json.len() <= MASSIMA_TESTA, "{} byte aperti", json.len());
    json
}

fn mib(byte: u64) -> f64 {
    byte as f64 / (1024.0 * 1024.0)
}

/// Apre `compressa` come testa, dice com'è andata e quanto è costato.
fn misura(nome: &str, compressa: &[u8], aperta: usize) {
    let prima = picco();
    let inizio = Instant::now();
    let esito = testa_entro(compressa, true, MASSIMA_TESTA);
    let tempo = inizio.elapsed();
    let dopo = picco();
    let tipo = match &esito {
        Ok(Testa::Presenza(_)) => "Ok(Presenza)".to_owned(),
        Ok(Testa::Dati { .. }) => "Ok(Dati)".to_owned(),
        Ok(altra) => format!("Ok({altra:?})").chars().take(40).collect(),
        Err(err) => format!("Err({})", err.code().kind().code()),
    };
    println!(
        "MISURA {nome}: {} byte sul filo, {aperta} aperti, {tipo}, {} ms",
        compressa.len(),
        tempo.as_millis()
    );
    if let (Some((ws0, cm0)), Some((ws1, cm1))) = (prima, dopo) {
        println!(
            "MISURA {nome}: picco working set {:.1} -> {:.1} MiB (+{:.1}), picco commit {:.1} -> {:.1} MiB (+{:.1})",
            mib(ws0),
            mib(ws1),
            mib(ws1.saturating_sub(ws0)),
            mib(cm0),
            mib(cm1),
            mib(cm1.saturating_sub(cm0)),
        );
    } else {
        println!("MISURA {nome}: il picco di memoria non si sa chiedere qui");
    }
}

/// La risposta a `Presenza` di un pari ostile: una presenza vera con un campo
/// in più da 8 MiB di zeri. È la risposta che il compito `segui` aspetta a ogni
/// connessione.
#[test]
#[ignore = "misura: si lancia a mano, una per volta"]
fn misura_presenza_ostile() {
    let json = un_json_di_zeri(r#"{"tipo":"presenza","nome":"x","z":"#, "}");
    let compressa = comprimi(&json);
    assert!(compressa.len() < 16 * 1024, "{} byte", compressa.len());
    misura("presenza ostile", &compressa, json.len());
}

/// La risposta di un pari ostile a una domanda che aspetta dei `Dati`: ogni
/// zero è un `Value` di 32 byte.
#[test]
#[ignore = "misura: si lancia a mano, una per volta"]
fn misura_dati_ostili() {
    let json = un_json_di_zeri(r#"{"tipo":"dati","dati":"#, "}");
    let compressa = comprimi(&json);
    assert!(compressa.len() < 16 * 1024, "{} byte", compressa.len());
    misura("dati ostili", &compressa, json.len());
}

/// Il peggio che passa: una risposta di dati con tanti nodi quanti
/// [`MASSIMI_NODI_DATI`] ne concede, tutti zeri. Quel che resta di S1-2 dopo la
/// correzione.
#[test]
#[ignore = "misura: si lancia a mano, una per volta"]
fn misura_dati_al_limite_dei_nodi() {
    let mut json = String::from(r#"{"tipo":"dati","dati":["#);
    json.push_str(&"0,".repeat(MASSIMI_NODI_DATI - 8));
    json.push_str("0]}");
    let compressa = comprimi(&json);
    misura("dati al limite dei nodi", &compressa, json.len());
}

/// Il controllo: una pagina vera da cinquecento brani, come la manda un pari
/// onesto. Quel che si fa per i due casi sopra non deve toccarla.
#[test]
#[ignore = "misura: si lancia a mano, una per volta"]
fn misura_pagina_onesta() {
    let righe: Vec<serde_json::Value> = (0..MASSIMA_PAGINA)
        .map(|n| {
            serde_json::json!({
                "id": n, "title": "x".repeat(60), "artist": "y".repeat(40),
                "album": "z".repeat(60), "albumArtist": "w".repeat(40), "genre": "rock",
                "codec": "flac", "trackKey": "k".repeat(120), "contentKey": "c".repeat(120),
                "albumKey": "a".repeat(32), "coverArtHash": "h".repeat(64),
                "mbRecordingId": "m".repeat(36), "mbReleaseGroupId": "m".repeat(36),
                "mbReleaseId": "m".repeat(36), "versione": "v1", "durationMs": 215_000,
                "trackNo": 3, "discNo": 1, "year": 1998, "rating": 4, "liked": true,
                "playCount": 12, "lastPlayedAt": 1_700_000_000_000_i64,
            })
        })
        .collect();
    let json = serde_json::to_string(&Testa::Dati {
        dati: serde_json::json!({ "generazione": 1, "brani": righe }),
    })
    .expect("si scrive");
    let compressa = comprimi(&json);
    misura("pagina onesta", &compressa, json.len());
}
