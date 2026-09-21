//! La ricerca libera sui cataloghi, provata contro i servizi veri.
//!
//! # Perché un esempio e non una prova
//!
//! Perché parla con la rete. Le prove di questo albero girano senza, e ce ne
//! sono più di mille: una sola che chiami `archive.org` le renderebbe mille e
//! una che ogni tanto falliscono per colpa di qualcun altro, e il giorno in cui
//! succede nessuno guarda più quale delle mille era.
//!
//! # A cosa serve
//!
//! A rispondere a tre domande prima di costruirci sopra una schermata:
//!
//! 1. una frase digitata da una persona — non il titolo di un brano copiato da
//!    una playlist — tira fuori risultati che quella persona riconoscerebbe?
//! 2. quanto costa una ricerca, in richieste e in secondi?
//! 3. quanta parte di quel che torna si può solo ascoltare, e quanta tenere?
//!
//! # Come si lancia
//!
//! ```text
//! cargo run -p aether-catalogo --example ricerca_libera -- "grateful dead 1977"
//! ```

use std::process::ExitCode;
use std::time::{Duration, Instant};

use aether_catalogo::Cataloghi;
use aether_domain::errors::AppError;
use aether_domain::esterno::Disponibilita;
use aether_domain::scelta::Candidato;

/// Quante richieste HTTP costa al massimo una passata, per catalogo.
///
/// Non è misurato: è il tetto che i due moduli si sono dati e che i loro
/// commenti spiegano. Audius chiede una volta sola; l'Internet Archive indicizza
/// gli *item* e non i file, quindi alla ricerca avanzata seguono fino a quattro
/// schede aperte una per una — ed è il numero che decide se una ricerca possa
/// partire a ogni tasto premuto o soltanto a Invio.
const TETTO_RICHIESTE: &str = "Archive: 1 + fino a 4 schede — Audius: 1";

fn main() -> ExitCode {
    let Some(testo) = std::env::args().nth(1) else {
        println!("uso: cargo run -p aether-catalogo --example ricerca_libera -- \"<testo>\"");
        return ExitCode::FAILURE;
    };

    let cataloghi = Cataloghi::nuovi();
    let mai = || false;

    println!("╭─ «{testo}»");
    println!("│  tetto di richieste per passata — {TETTO_RICHIESTE}");
    println!("│");

    let inizio = Instant::now();
    let esatta = cataloghi.archivio().cerca_libera(&testo, true, &mai);
    let tempo_esatta = inizio.elapsed();
    passata("Internet Archive, frase esatta", &esatta, tempo_esatta);

    let inizio = Instant::now();
    let larga = cataloghi.archivio().cerca_libera(&testo, false, &mai);
    let tempo_larga = inizio.elapsed();
    passata("Internet Archive, termini sciolti", &larga, tempo_larga);

    let inizio = Instant::now();
    let audius = cataloghi.audius().cerca_libera(&testo);
    let tempo_audius = inizio.elapsed();
    passata("Audius", &audius, tempo_audius);

    println!("╰─ fine");
    ExitCode::SUCCESS
}

/// Il resoconto di una passata: i risultati, o il guasto che li ha impediti.
fn passata(nome: &str, esito: &Result<Vec<Candidato>, AppError>, quanto: Duration) {
    match esito {
        Err(err) => {
            // Un catalogo che non risponde non è un catalogo che non ha niente,
            // ed è la differenza che una schermata di ricerca dovrà dire.
            println!("├─ {nome} — {:.2} s", quanto.as_secs_f32());
            println!("│    guasto: {:?}", err.code());
            if let Some(causa) = err.cause() {
                println!("│    causa:  {causa}");
            }
            println!("│");
        }
        Ok(trovati) => {
            let (tenibili, ascoltabili, da_comprare) = conta(trovati);
            println!(
                "├─ {nome} — {} risultati in {:.2} s",
                trovati.len(),
                quanto.as_secs_f32()
            );
            println!(
                "│    scaricabili {tenibili} · solo ascolto {ascoltabili} · solo acquisto {da_comprare}"
            );
            for candidato in trovati {
                riga(candidato);
            }
            println!("│");
        }
    }
}

/// Quanti se ne potrebbero tenere, quanti solo ascoltare, quanti né l'uno né
/// l'altro.
fn conta(trovati: &[Candidato]) -> (usize, usize, usize) {
    let mut tenibili: usize = 0;
    let mut ascoltabili: usize = 0;
    let mut da_comprare: usize = 0;
    for candidato in trovati {
        match candidato.disponibilita {
            Disponibilita::Scaricabile => tenibili = tenibili.saturating_add(1),
            Disponibilita::SoloAscolto => ascoltabili = ascoltabili.saturating_add(1),
            Disponibilita::SoloAcquisto => da_comprare = da_comprare.saturating_add(1),
        }
    }
    (tenibili, ascoltabili, da_comprare)
}

/// Un risultato, come lo vedrebbe chi guarda la schermata che ancora non c'è.
///
/// Fonte, licenza e pagina pubblica stanno qui e non in fondo perché è così che
/// dovranno stare là: per certi cataloghi il rimando visibile è un obbligo dei
/// termini, non una gentilezza.
fn riga(candidato: &Candidato) {
    #[expect(
        clippy::integer_division,
        reason = "sono minuti e secondi: il resto lo stampa la riga accanto"
    )]
    let durata = match candidato.durata_sec {
        Some(secondi) => format!("{}:{:02}", secondi / 60, secondi % 60),
        None => "—:—".to_owned(),
    };
    println!(
        "│    · {} — {}",
        candidato.titolo,
        candidato.autore.as_deref().unwrap_or("senza autore")
    );
    println!(
        "│      {durata} · {} · {} · {} · {:?}",
        candidato.fonte.etichetta(),
        candidato.licenza.nome(),
        candidato.disponibilita.nome(),
        candidato.natura
    );
    println!("│      {}", candidato.url);
    if let Some(pagina) = candidato.pagina.as_deref() {
        println!("│      pagina: {pagina}");
    }
}
