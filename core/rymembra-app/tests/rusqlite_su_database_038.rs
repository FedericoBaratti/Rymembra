//! Una libreria vera della 3.2.0 (schema 38), aperta con la versione di SQLite
//! e di `rusqlite` che c'è adesso: migrata fino all'ultimo schema, controllata,
//! e poi riportata com'era con «Torna alla copia di prima».
//!
//! # Perché esiste
//!
//! Il file del database è la libreria degli utenti, e `rusqlite` porta con sé
//! la versione di SQLite che lo scrive (`libsqlite3-sys`, compilato dentro il
//! programma). Cambiare l'una o l'altra è l'unico aggiornamento di dipendenze
//! che può far leggere male, o scrivere male, il dato che non si rifà. Le prove
//! del nucleo usano librerie costruite dalle migrazioni stesse; questa parte da
//! un file **scritto dalla 3.2.0**, con le sue pagine, il suo `-wal` e la sua
//! ricerca testuale, e dice che cosa ne resta dopo l'andata e dopo il ritorno.
//!
//! # Come si lancia
//!
//! Non gira da sola (`#[ignore]`): serve un database vero, e i database
//! dell'utente non si toccano mai. Il file indicato **non si apre**: se ne fa
//! una copia, con il suo `-wal` e il suo `-shm`, in una cartella temporanea, e
//! si lavora lì.
//!
//! ```text
//! $env:RYMEMBRA_PROVA_DB_038 = 'D:\copia\di\un\rymembra.db'   # schema 38
//! cargo test -p rymembra-app --locked --test rusqlite_su_database_038 -- --ignored --nocapture
//! ```
//!
//! `RYMEMBRA_PROVA_SQLITE_MINIMA`, se c'è, è la versione di SQLite sotto cui la
//! prova fallisce (per esempio `3.54.0` dopo il salto a `rusqlite` 0.41): è
//! il controllo che il programma stia davvero usando quella e non una di prima.

// Un test dichiara le sue aspettative con `expect` e con l'indice: i lint del
// workspace li vietano per il codice di produzione, e `clippy.toml` li concede ai
// soli `#[test]` e ai moduli `cfg(test)`, non ai suoi aiutanti di una crate di
// test a sé.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use rymembra_app::db::{self, LATEST_VERSION, copia_di_prima, torna_alla_copia};

/// Il database da copiare: una variabile d'ambiente, mai un percorso scritto
/// qui dentro.
const VARIABILE: &str = "RYMEMBRA_PROVA_DB_038";
/// La versione di SQLite sotto cui la prova fallisce, se c'è.
const VARIABILE_MINIMA: &str = "RYMEMBRA_PROVA_SQLITE_MINIMA";

/// Copia `rymembra.db` e i suoi file di giornale, se ci sono, in `cartella`.
fn copia_con_il_giornale(sorgente: &Path, cartella: &Path) -> PathBuf {
    let destinazione = cartella.join("rymembra.db");
    for suffisso in ["", "-wal", "-shm"] {
        let da = PathBuf::from(format!("{}{suffisso}", sorgente.display()));
        if da.exists() {
            let a = PathBuf::from(format!("{}{suffisso}", destinazione.display()));
            std::fs::copy(&da, &a).expect("copia del database di prova");
        }
    }
    destinazione
}

/// `PRAGMA integrity_check`, tutte le righe: `["ok"]` se va bene.
fn integrita(connessione: &Connection) -> Vec<String> {
    let mut istruzione = connessione
        .prepare("PRAGMA integrity_check")
        .expect("integrity_check");
    istruzione
        .query_map([], |riga| riga.get::<_, String>(0))
        .expect("righe")
        .collect::<Result<_, _>>()
        .expect("lettura")
}

/// Righe e un'impronta del contenuto di ogni tabella (anche le tabelle ombra
/// della ricerca testuale), senza guardare l'ordine delle righe.
///
/// L'impronta è la somma, senza riporto, di quella di ogni riga: due database
/// con le stesse righe in un altro ordine hanno la stessa. Si calcola leggendo
/// ogni valore con `rusqlite`, quindi dice anche che la lettura è intera.
fn impronte(connessione: &Connection) -> BTreeMap<String, (i64, u64)> {
    let nomi: Vec<String> = {
        let mut istruzione = connessione
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .expect("elenco delle tabelle");
        istruzione
            .query_map([], |riga| riga.get::<_, String>(0))
            .expect("righe")
            .collect::<Result<_, _>>()
            .expect("nomi")
    };
    let mut fuori = BTreeMap::new();
    for nome in nomi {
        let mut istruzione = connessione
            .prepare(&format!("SELECT * FROM \"{nome}\""))
            .expect("lettura della tabella");
        let colonne = istruzione.column_count();
        let mut righe = istruzione.query([]).expect("righe");
        let (mut quante, mut somma) = (0_i64, 0_u64);
        while let Some(riga) = righe.next().expect("riga") {
            let mut h = DefaultHasher::new();
            for i in 0..colonne {
                match riga.get_ref(i).expect("valore") {
                    ValueRef::Null => 0_u8.hash(&mut h),
                    ValueRef::Integer(n) => n.hash(&mut h),
                    ValueRef::Real(x) => x.to_bits().hash(&mut h),
                    ValueRef::Text(t) => t.hash(&mut h),
                    ValueRef::Blob(b) => b.hash(&mut h),
                }
            }
            somma = somma.wrapping_add(h.finish());
            quante += 1;
        }
        fuori.insert(nome, (quante, somma));
    }
    fuori
}

fn versione_dello_schema(connessione: &Connection) -> u32 {
    connessione
        .query_row("PRAGMA user_version", [], |riga| riga.get(0))
        .expect("user_version")
}

#[test]
#[ignore = "serve un database della 3.2.0: RYMEMBRA_PROVA_DB_038; vedi la documentazione del file"]
fn un_database_della_320_migra_si_controlla_e_torna_com_era() {
    let Ok(sorgente) = std::env::var(VARIABILE) else {
        panic!(
            "manca {VARIABILE}: il percorso di un `rymembra.db` di schema 38 (se ne fa una copia)"
        );
    };
    let sqlite = rusqlite::version();
    println!("SQLite {sqlite} (rusqlite, libsqlite3-sys compilato dentro il programma)");
    if let Ok(minima) = std::env::var(VARIABILE_MINIMA) {
        let numero =
            |v: &str| -> Vec<u32> { v.split('.').filter_map(|p| p.parse().ok()).collect() };
        assert!(
            numero(sqlite) >= numero(&minima),
            "SQLite {sqlite} è più vecchio di {minima}: il salto non è arrivato fin qui"
        );
    }

    let cartella = tempfile::tempdir().expect("cartella temporanea");
    let percorso = copia_con_il_giornale(Path::new(&sorgente), cartella.path());

    // 1. Com'è il file della 3.2.0, letto per intero con questa versione.
    let prima = {
        let c = Connection::open_with_flags(&percorso, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .expect("apertura della copia");
        assert_eq!(versione_dello_schema(&c), 38, "la copia non è di schema 38");
        assert_eq!(
            integrita(&c),
            ["ok"],
            "il file della 3.2.0 non è integro già prima"
        );
        impronte(&c)
    };
    let righe: i64 = prima.values().map(|(n, _)| *n).sum();
    println!(
        "schema 38: {} tabelle, {righe} righe, integrità ok",
        prima.len()
    );

    // 2. L'aggiornamento: l'apertura migra fino all'ultimo schema e fa la copia di prima.
    let aperto = db::open(&percorso).expect("apertura con migrazione");
    assert_eq!(aperto.version_before, 38);
    assert_eq!(
        aperto.applied,
        usize::try_from(LATEST_VERSION - 38).expect("poche migrazioni")
    );
    let mut connessione = aperto.connection;
    assert_eq!(versione_dello_schema(&connessione), LATEST_VERSION);
    assert_eq!(
        integrita(&connessione),
        ["ok"],
        "dopo la migrazione il file non è integro"
    );
    let dopo = impronte(&connessione);
    for (nome, (quante, _)) in &prima {
        // Ogni tabella che c'era arriva intera: la migrazione può aggiungere colonne
        // e righe di impostazioni, ma non perde un brano.
        if [
            "tracks",
            "albums",
            "artists",
            "cover_art",
            "playlists",
            "playlist_tracks",
            "play_history",
        ]
        .contains(&nome.as_str())
        {
            assert_eq!(
                dopo.get(nome).map(|(n, _)| *n),
                Some(*quante),
                "dopo la migrazione la tabella {nome} ha un numero di righe diverso"
            );
        }
    }
    println!(
        "schema {LATEST_VERSION}: {} tabelle, integrità ok, {} migrazioni applicate",
        dopo.len(),
        aperto.applied
    );

    // 3. «Torna alla copia di prima».
    let copia = copia_di_prima(&percorso).expect("dopo l'aggiornamento c'è la copia di prima");
    assert_eq!(copia.versione, 38);
    let ritorno =
        torna_alla_copia(&mut connessione, &percorso, &copia).expect("ritorno alla copia");
    assert_eq!(
        versione_dello_schema(&connessione),
        38,
        "il ritorno non è tornato a 38"
    );
    assert_eq!(
        integrita(&connessione),
        ["ok"],
        "dopo il ritorno il file non è integro"
    );
    let tornato = impronte(&connessione);
    assert_eq!(
        tornato, prima,
        "dopo il ritorno una tabella ha righe diverse da quelle di prima dell'aggiornamento"
    );
    println!(
        "ritorno: schema 38, integrità ok, {} tabelle con le stesse righe e lo stesso contenuto di prima; \
         la libreria dell'aggiornamento è in {}",
        tornato.len(),
        ritorno.messa_da_parte.display()
    );
    drop(connessione);

    // 4. E la libreria messa da parte è quella migrata, intera.
    let messa =
        Connection::open_with_flags(&ritorno.messa_da_parte, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("apertura della libreria messa da parte");
    assert_eq!(versione_dello_schema(&messa), LATEST_VERSION);
    assert_eq!(integrita(&messa), ["ok"]);
    println!("libreria messa da parte: schema {LATEST_VERSION}, integrità ok");
}
