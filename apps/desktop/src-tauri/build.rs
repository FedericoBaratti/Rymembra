//! Genera il contesto di Tauri (configurazione, icone, permessi) a build time.
//!
//! # Nessuna credenziale
//!
//! Fino alla 3.1 questo file passava al compilatore il client Google del
//! backup, preso dall'ambiente o da un `oauth.local.toml`, e per poco anche gli
//! identificativi delle app Microsoft e Dropbox. Dalla 3.2 ogni app è
//! dell'utente, che la registra a suo nome e ne scrive l'identificativo nelle
//! Impostazioni (vedi `rymembra_domain::nuvola_app`): nel binario non c'è
//! nessun client, e non c'è niente da passare. Le build pubblicate non ne
//! avevano mai avuto uno.
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

fn main() {
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
