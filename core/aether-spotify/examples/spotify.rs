//! Il banco di collaudo del lettore keyless, da riga di comando.
//!
//! Esiste perché l'unica parte di questo crate che **non** si può provare in
//! automatico è quella che conta di più: se Spotify oggi risponda ancora. Le
//! prove di unità coprono il TOTP, l'interpretazione delle pagine, il
//! riconoscimento dei link e l'abbinamento; nessuna di loro si accorgerebbe di
//! una rotazione dei cifrari, perché nessuna tocca la rete — ed è giusto così,
//! una suite che fallisce quando cambia qualcosa a Stoccolma non è una suite.
//!
//! ```text
//! cargo run -p aether-spotify --example spotify -- diagnostica
//! cargo run -p aether-spotify --example spotify -- <link> [--brani] [--grezzo]
//! ```
//!
//! `diagnostica` prova solo la stretta di mano: è il primo comando da dare
//! quando «non funziona», perché distingue un cifrario scaduto da un computer
//! offline.
//!
//! Con un link, stampa cosa c'è dietro e da quale livello è arrivato. `--brani`
//! aggiunge l'elenco completo — utile per controllare la paginazione su una
//! playlist lunga, dove il numero da guardare è quello fra parentesi accanto al
//! totale dichiarato.
//!
//! `--grezzo` stampa la risposta di Pathfinder senza interpretarla. È lo
//! strumento da usare quando un campo del rapporto è vuoto: dice se è sparito o
//! se si è solo spostato, che è una domanda a cui nessun messaggio d'errore
//! sa rispondere.
//!
//! Con `AETHER_SPOTIFY_CONFIG` si punta a un `spotify.json` alternativo: è il
//! modo di provare dei cifrari o delle impronte nuove senza toccare la cartella
//! dell'applicazione.

// Questi esempi sono strumenti diagnostici, eseguiti a mano da chi sviluppa.
// Qui un guasto DEVE fermare tutto rumorosamente: un esempio che prosegue su un
// errore riporta numeri sbagliati, e i numeri sono l'unica cosa che produce.
#![allow(clippy::expect_used, clippy::print_stdout, clippy::unwrap_used)]

use std::path::PathBuf;

use aether_domain::spotify::SpotifyContent;
use aether_spotify::Lettore;
use aether_spotify::config::{EsitoFile, NOME_FILE};

fn main() -> std::process::ExitCode {
    let argomenti: Vec<String> = std::env::args().skip(1).collect();
    let primo = argomenti.first().map(String::as_str).unwrap_or_default();
    let con_brani = argomenti.iter().any(|a| a == "--brani");
    let grezzo = argomenti.iter().any(|a| a == "--grezzo");

    let mut lettore = Lettore::dal_file(&percorso_config());

    if primo.is_empty() || primo == "aiuto" || primo == "--help" {
        println!("{AIUTO}");
        return std::process::ExitCode::SUCCESS;
    }

    if primo == "diagnostica" {
        return diagnostica(&mut lettore);
    }

    // `lettore.riferimento` e non `riconosci`: è la porta che usa
    // l'applicazione, e passa dalla rete per i link corti del telefono. Provare
    // qui la funzione pura vorrebbe dire che questo banco non sa collaudare
    // proprio la forma di link che l'utente ha davvero negli appunti.
    let Some(riferimento) = lettore.riferimento(primo) else {
        eprintln!("«{primo}» non è un link di Spotify.\n\n{AIUTO}");
        return std::process::ExitCode::FAILURE;
    };
    println!("→ {} {}", riferimento.genere.nome(), riferimento.id);

    if grezzo {
        return match lettore.grezzo(&riferimento) {
            Ok(valore) => {
                println!("{}", serde_json::to_string_pretty(&valore).unwrap());
                std::process::ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("   ✗ {err}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    match lettore.risolvi(&riferimento) {
        Ok(risultato) => {
            for fallito in &risultato.falliti {
                println!("   ✗ {:<11} {}", fallito.livello.nome(), fallito.perche);
            }
            stampa(&risultato.contenuto, con_brani);
            // La copertina si prova a parte: nell'applicazione non è
            // l'indirizzo a viaggiare ma i byte, e quel passaggio è l'unico che
            // dipende dalla politica dei contenuti della finestra. Se qui non
            // arriva, di là si vede un riquadro vuoto e nient'altro.
            if let Some(indirizzo) = &risultato.contenuto.cover_url {
                match lettore.copertina(indirizzo) {
                    Some(dati) => println!(
                        "     copertina   {} byte in `data:` ({})",
                        dati.len(),
                        dati.split(';').next().unwrap_or("?")
                    ),
                    None => println!("     copertina   NON scaricata da {indirizzo}"),
                }
            }
            std::process::ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("   ✗ {err}");
            eprintln!("\nProva `diagnostica` per sapere se è questo link o è tutto.");
            std::process::ExitCode::FAILURE
        }
    }
}

fn diagnostica(lettore: &mut Lettore) -> std::process::ExitCode {
    let d = lettore.diagnostica();
    let versioni: Vec<String> = d.versioni_cifrari.iter().map(u32::to_string).collect();
    println!("cifrari         {} (v{})", d.cifrari, versioni.join(", v"));
    match &d.file_configurazione {
        EsitoFile::Assente => println!("configurazione  quella compilata dentro"),
        EsitoFile::Letto => println!("configurazione  letta dal file"),
        EsitoFile::Illeggibile(perche) => {
            println!("configurazione  file illeggibile ({perche}) → compilata dentro");
        }
    }
    println!("file            {}", percorso_config().display());
    match d.stretta_di_mano {
        None => {
            println!("stretta di mano riuscita");
            std::process::ExitCode::SUCCESS
        }
        Some(perche) => {
            println!("stretta di mano FALLITA: {perche}");
            println!(
                "\nSe qui sopra c'è un errore di rete, è il collegamento. Se invece Spotify\n\
                 risponde ma il gettone non arriva, i cifrari sono scaduti: si correggono\n\
                 scrivendo il file indicato, senza ricompilare."
            );
            std::process::ExitCode::FAILURE
        }
    }
}

fn stampa(contenuto: &SpotifyContent, con_brani: bool) {
    println!("   ✓ {:<11} {}", contenuto.source.nome(), contenuto.title);
    if let Some(autore) = &contenuto.author {
        println!("     di          {autore}");
    }
    match contenuto.declared_total {
        Some(totale) => println!(
            "     brani       {} su {totale} dichiarati",
            contenuto.tracks.len()
        ),
        None => println!("     brani       {}", contenuto.tracks.len()),
    }
    if let Some((letti, attesi)) = contenuto.truncation() {
        println!("     ATTENZIONE  elenco monco: {letti} di {attesi}");
    }
    // Oggi è sempre zero. Si stampa lo stesso, ed è il punto: è la riga da
    // guardare per accorgersi il giorno in cui l'ISRC torna.
    let con_isrc = contenuto.tracks.iter().filter(|b| b.isrc.is_some()).count();
    println!("     con ISRC    {con_isrc}");

    if con_brani {
        for (indice, brano) in contenuto.tracks.iter().enumerate() {
            println!(
                "     {:>4}. {} — {}{}",
                indice + 1,
                brano.artist.as_deref().unwrap_or("?"),
                brano.title,
                brano
                    .duration_ms
                    .map(|ms| format!(" [{}]", durata(ms)))
                    .unwrap_or_default()
            );
        }
    }
}

/// Millisecondi in minuti e secondi, per leggerli.
#[allow(clippy::integer_division)]
fn durata(ms: u64) -> String {
    let secondi = ms / 1000;
    format!("{}:{:02}", secondi / 60, secondi % 60)
}

/// Dove sta `spotify.json`: l'ambiente, o la cartella dell'applicazione.
fn percorso_config() -> PathBuf {
    if let Ok(percorso) = std::env::var("AETHER_SPOTIFY_CONFIG") {
        return PathBuf::from(percorso);
    }
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("dev.aether.desktop").join(NOME_FILE)
}

const AIUTO: &str = "\
Il lettore keyless di Spotify.

    cargo run -p aether-spotify --example spotify -- diagnostica
    cargo run -p aether-spotify --example spotify -- <link> [--brani] [--grezzo]

Il link può essere un brano, un album, una playlist o un artista, nella forma
`https://open.spotify.com/playlist/…`, `spotify:playlist:…` o `spotify.link/…`
— quest'ultima costa una richiesta in più, perché va chiesto dove porta.";
