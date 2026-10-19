//! Genera il contesto di Tauri (configurazione, icone, permessi) a build time,
//! e passa al compilatore le credenziali del client Google.
//!
//! # Perché le credenziali passano di qui
//!
//! `option_env!` legge l'ambiente del crate che si sta compilando, e vuole
//! trovare **una strada sola**. Senza questo file ce ne sarebbero due — le
//! variabili d'ambiente e un file di configurazione — e chi compila dovrebbe
//! ricordarsi quale ha usato l'ultima volta. Qui il file viene riemesso come
//! variabile, così a valle esiste solo `option_env!`.
//!
//! # Perché non in `tauri.conf.json`
//!
//! Perché il bundler ne copia i valori nelle risorse dell'applicazione, in
//! chiaro e in un file di testo che chiunque apre. Dentro il binario non è un
//! nascondiglio — `strings` lo trova — ma non è nemmeno un invito.
//!
//! # Cosa succede senza credenziali
//!
//! Niente: la compilazione riesce, e a runtime il backup dice di non essere
//! configurato. È voluto. Con `env!` al posto di `option_env!`, clonare il repo
//! e lanciare `cargo test` sarebbe impossibile per chiunque non abbia un
//! progetto Google, e la prima cosa che farebbe è commentare la riga.
//!
//! # Il runtime di Visual C++, e perché dipende dal profilo
//!
//! L'installer porta dentro l'eseguibile il runtime di Visual C++ collegato
//! staticamente: chi installa Rymembra non deve avere già il pacchetto
//! ridistribuibile di Microsoft, che su un Windows appena messo spesso manca.
//! Fino a `tauri 2.11` lo decideva la CLI, e solo in `tauri build`; dalla
//! `2.12` lo decide `tauri-build`, e di serie lo fa **sempre** — anche in un
//! `cargo test`.
//!
//! Lì fa danni. Per collegarlo così `tauri-build` mette nella propria cartella
//! di uscita un `msvcrt.lib` finto e la aggiunge ai percorsi del linker; e
//! `cargo`, quando prova gli esempi della documentazione, passa quei percorsi a
//! **tutti** i crate del workspace. Gli esempi di `rymembra-domain` trovavano il
//! `msvcrt.lib` finto al posto di quello vero, e non si collegavano più.
//!
//! Quindi si torna a com'era: statico nel profilo `release`, che è quello di
//! `tauri build` e dell'installer; il runtime di sistema in sviluppo e nelle
//! prove, dove Visual C++ c'è di sicuro perché serve a compilare.

use std::path::Path;

/// Le due variabili che il codice legge con `option_env!`.
const VARIABILI: [&str; 2] = ["RYMEMBRA_GOOGLE_CLIENT_ID", "RYMEMBRA_GOOGLE_CLIENT_SECRET"];

/// Il file locale da cui leggerle quando non stanno nell'ambiente.
///
/// **Gitignorato.** Se un giorno comparisse in un commit, il rimedio non è
/// toglierlo: è eliminare il client in Google Cloud Console e farne uno nuovo —
/// togliere le righe non le de-pubblica dalla storia di git.
const FILE_LOCALE: &str = "oauth.local.toml";

fn main() {
    for variabile in VARIABILI {
        println!("cargo:rerun-if-env-changed={variabile}");
    }
    println!("cargo:rerun-if-changed={FILE_LOCALE}");

    // L'ambiente vince sul file: è quel che una macchina di build automatica
    // può fornire senza scrivere niente su disco.
    let mancanti: Vec<&str> = VARIABILI
        .into_iter()
        .filter(|variabile| std::env::var_os(variabile).is_none())
        .collect();
    if !mancanti.is_empty() {
        for (chiave, valore) in dal_file(Path::new(FILE_LOCALE)) {
            if mancanti.contains(&chiave.as_str()) {
                println!("cargo:rustc-env={chiave}={valore}");
            }
        }
    }

    // `PROFILE` vale `release` anche per un profilo che eredita da lì: è la
    // domanda giusta, «questa è la build che va nell'installer?».
    let di_rilascio = std::env::var("PROFILE").is_ok_and(|profilo| profilo == "release");
    let attributi = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new().static_vc_runtime(di_rilascio));
    // Quel che fa `tauri_build::build`, con un attributo in più: il messaggio
    // intero sull'uscita, e la compilazione che si ferma.
    if let Err(errore) = tauri_build::try_build(attributi) {
        println!("{errore:#}");
        std::process::exit(1);
    }
}

/// Le coppie `chiave = "valore"` di un file `.toml` minimale.
///
/// Scritto a mano invece di aggiungere un lettore TOML fra le dipendenze di
/// build: il file ha due righe, e nessuna delle due ha bisogno di tabelle,
/// vettori o stringhe multilinea. Un file assente non è un guasto — è la
/// condizione di chi compila senza credenziali.
fn dal_file(percorso: &Path) -> Vec<(String, String)> {
    let Ok(testo) = std::fs::read_to_string(percorso) else {
        return Vec::new();
    };
    testo
        .lines()
        .map(str::trim)
        .filter(|riga| !riga.is_empty() && !riga.starts_with('#'))
        .filter_map(|riga| {
            let (chiave, valore) = riga.split_once('=')?;
            let valore = valore.trim().trim_matches(['"', '\'']).to_owned();
            // Un valore vuoto vale come assente: è quel che resta in un file
            // di esempio con le righe da riempire, e trattarlo come una
            // credenziale vera darebbe un `invalid_client` invece di un
            // «non configurato».
            (!valore.is_empty()).then(|| (chiave.trim().to_owned(), valore))
        })
        .collect()
}
