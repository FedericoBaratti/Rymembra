//! Il trasloco di una libreria vera: dalla cartella della versione di prima a
//! quella di Rymembra, con un database vero alla versione 30 e il suo giornale
//! copiato **a caldo**.
//!
//! # Perché è un test d'integrazione
//!
//! Le prove del modulo `trasloco` muovono file con dentro due lettere. Qui il
//! file è un database SQLite di quelli che esistono sui dischi degli utenti: fatto
//! con le migrazioni fino alla 30, con le righe che la 31 deve riscrivere, e con
//! parte dei dati ancora **nel giornale**, non nel file principale. È la forma in
//! cui lo trova chi ha chiuso la versione di prima male, o chi ha una copia
//! fatta mentre girava: il database da solo non basta, e se il trasloco
//! lasciasse indietro il `-wal` la libreria si aprirebbe vuota e senza errori.
//!
//! Tutto in una cartella temporanea. Il vero `%APPDATA%` non si guarda mai.

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

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use rymembra_app::db::{self, Istantanea, LATEST_VERSION, MIGRATIONS};
use rymembra_app::trasloco::{Percorsi, Rapporto, percorsi_per, scegli_cartella, trasloca};
use rymembra_domain::eredita;

/// Un identificativo vero di playlist di Spotify: non porta nessun marchio, e la
/// migrazione non deve toccarlo.
const PLAYLIST_SPOTIFY: &str = "37i9dQZF1DXcBWIGoYBM5M";

/// Le due cartelle di una macchina di prova, calcolate dal codice vero: la nuova
/// per Rymembra e, accanto, quella che la versione di prima aveva.
struct Macchina {
    /// La radice temporanea, che sparisce con la prova.
    radice: tempfile::TempDir,
    /// I percorsi del trasloco.
    percorsi: Percorsi,
}

impl Macchina {
    fn nuova() -> Self {
        let radice = tempfile::tempdir().expect("cartella temporanea");
        let nuova = radice.path().join("io.github.federicobaratti.rymembra");
        let percorsi = percorsi_per(&scegli_cartella(None, None, nuova));
        Self { radice, percorsi }
    }

    fn precedente(&self) -> &Path {
        self.percorsi.precedente.as_deref().expect("la precedente")
    }

    fn nuova_cartella(&self) -> &Path {
        &self.percorsi.nuova
    }

    /// Dove la versione di prima tiene il database mentre lo si costruisce: una
    /// terza cartella, da cui poi si **copia** dentro quella di prima.
    fn origine(&self) -> PathBuf {
        self.radice.path().join("origine")
    }
}

/// Esegue una query e restituisce la prima colonna come testo, riga per riga.
fn colonna(connessione: &Connection, sql: &str) -> Vec<String> {
    let mut query = connessione.prepare(sql).expect(sql);
    query
        .query_map([], |riga| riga.get::<_, String>(0))
        .expect("lettura")
        .map(|riga| riga.expect("riga"))
        .collect()
}

fn conta(connessione: &Connection, tabella: &str) -> i64 {
    connessione
        .query_row(&format!("SELECT COUNT(*) FROM {tabella}"), [], |riga| {
            riga.get(0)
        })
        .expect("conteggio")
}

/// Un database alla versione 30, in WAL, con parte dei dati **solo** nel giornale.
///
/// Si applicano le migrazioni vere fino alla 30 — le stesse che ha eseguito la
/// versione di prima — e si inseriscono le righe che la 31 deve riscrivere,
/// accanto a quelle che non deve toccare. Il `wal_autocheckpoint` a zero impedisce
/// a SQLite di riportare le pagine nel file principale: tutto quel che segue la
/// creazione vive nel `-wal`, e un trasloco che non lo porta dietro perde la
/// libreria.
fn libreria_di_prima(cartella: &Path) -> Connection {
    fs::create_dir_all(cartella).expect("cartella");
    let connessione = Connection::open(cartella.join(eredita::NOME_DATABASE)).expect("database");
    let giornale: String = connessione
        .query_row("PRAGMA journal_mode = WAL", [], |riga| riga.get(0))
        .expect("giornale");
    assert_eq!(giornale, "wal");
    connessione
        .execute_batch("PRAGMA wal_autocheckpoint = 0;")
        .expect("autocheckpoint");

    for migrazione in MIGRATIONS.iter().filter(|m| m.version <= 30) {
        connessione
            .execute_batch(migrazione.sql)
            .unwrap_or_else(|err| panic!("migrazione {}: {err}", migrazione.version));
    }
    connessione
        .execute_batch("PRAGMA user_version = 30")
        .expect("versione");

    connessione
        .execute_batch(&format!(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                 date_added, date_modified, rating, play_count)
             VALUES (1, 'C:/Musica/a.flac', 'a|b|c', 'Song 2', 'Blur', 'Blur', 5000000, 1, 1, 4, 7);
             INSERT INTO playlists (id, playlist_key, name, created_at, updated_at,
                                    source_playlist_id, source_service)
             VALUES (1, 'corsa', 'Corsa', 10, 20, 'aether:archivio:playlist:corsa', 'spotify');
             INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
             VALUES (2, 'mia', 'Mia', 10, 20);
             INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, 1, 0);
             INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 1700, 240000);
             INSERT INTO settings (key, value) VALUES ('ui.tema', 'scuro');
             INSERT INTO desiderati (track_key, title, source_kind, source_id,
                                     source_title, added_at, playlist_id, position)
             VALUES ('k1', 'Girl', 'playlist', 'aether:archivio:playlist:corsa', 'Corsa', 1, 1, 3);
             INSERT INTO desiderati (track_key, title, source_kind, source_id,
                                     source_title, added_at)
             VALUES ('k2', 'Piano', 'preferiti', 'aether:spotify:preferiti',
                     'Brani che ti piacciono', 2);
             INSERT INTO desiderati (track_key, title, source_kind, source_id,
                                     source_title, added_at)
             VALUES ('k3', 'Bolero', 'playlist', '{PLAYLIST_SPOTIFY}', 'Una vera', 3);
             INSERT INTO import_reports (source_id, saved_at, payload)
             VALUES ('aether:spotify:preferiti', 5,
                     '{{\"kind\":\"preferiti\",\"sourceId\":\"aether:spotify:preferiti\"}}');"
        ))
        .expect("righe di prima");
    connessione
}

/// Copia un file così com'è, anche se qualcun altro lo tiene aperto.
///
/// `fs::copy` su Windows apre l'origine senza condividerla in scrittura e
/// fallirebbe sul database vivo; `File::open` condivide con tutti, come fa una
/// copia di sicurezza fatta da un programma qualunque.
fn copia_a_caldo(da: &Path, a: &Path) -> io::Result<u64> {
    let mut origine = fs::File::open(da)?;
    let mut destinazione = fs::File::create(a)?;
    io::copy(&mut origine, &mut destinazione)
}

/// La cartella di prima, com'è dopo che la versione di prima ha lavorato: il
/// database con il suo giornale (copiati **mentre la connessione è aperta**) e
/// tutto il resto che il trasloco deve rinominare o lasciare stare.
///
/// Restituisce i byte del giornale copiato, perché la prova deve poter dire che
/// non era vuoto.
fn cartella_di_prima(macchina: &Macchina) -> u64 {
    let origine = macchina.origine();
    let connessione = libreria_di_prima(&origine);

    let precedente = macchina.precedente();
    fs::create_dir_all(precedente).expect("cartella di prima");
    let vecchio = eredita::NOME_DATABASE;
    for nome in [
        vecchio.to_owned(),
        format!("{vecchio}-wal"),
        format!("{vecchio}-shm"),
    ] {
        copia_a_caldo(&origine.join(&nome), &precedente.join(&nome))
            .unwrap_or_else(|err| panic!("copia di {nome}: {err}"));
    }
    let giornale = fs::metadata(precedente.join(format!("{vecchio}-wal")))
        .expect("giornale copiato")
        .len();
    // La connessione si chiude **dopo** la copia: è lei che la rende «a caldo».
    drop(connessione);

    let scrivi = |relativo: &str, contenuto: &[u8]| {
        let percorso = precedente.join(relativo);
        fs::create_dir_all(percorso.parent().expect("un padre")).expect("cartella");
        fs::write(percorso, contenuto).expect("file");
    };
    scrivi("copertine/ab/abcdef.jpg", b"\xff\xd8\xff\xe0 una copertina");
    scrivi(
        &format!("skin/notte.{}", eredita::ESTENSIONE_SKIN),
        b"PK\x03\x04 un pacchetto",
    );
    scrivi("skin/bozze/b1.json", b"{\"bozza\":1}");
    scrivi(
        &format!(
            "profilo/prima-1700000000000.{}",
            eredita::ESTENSIONE_PROFILO
        ),
        b"PK\x03\x04 un profilo",
    );
    scrivi(
        &format!("diario/{}.log", eredita::STEM_DIARIO),
        b"riga di diario\n",
    );
    giornale
}

#[test]
fn una_libreria_vera_passa_da_aether_a_rymembra() {
    let macchina = Macchina::nuova();
    let giornale = cartella_di_prima(&macchina);
    assert!(
        giornale > 0,
        "senza un giornale pieno la prova non prova niente"
    );

    // Il trasloco: la cartella di prima diventa quella nuova.
    let rapporto = trasloca(&macchina.percorsi).expect("trasloco");
    assert!(rapporto.spostata);
    assert!(rapporto.conflitti.is_empty(), "{:?}", rapporto.conflitti);
    assert!(rapporto.saltati.is_empty(), "{:?}", rapporto.saltati);
    assert!(!macchina.precedente().exists());
    let nuova = macchina.nuova_cartella();
    assert!(!nuova.join(eredita::NOME_DATABASE).exists());
    assert!(nuova.join("rymembra.db").is_file());
    // Il giornale ha viaggiato intero, col suo nome nuovo.
    assert_eq!(
        fs::metadata(nuova.join("rymembra.db-wal"))
            .expect("giornale")
            .len(),
        giornale
    );

    // Il resto: le copertine come stavano, le bozze intatte, il resto col nome
    // nuovo e lo stesso contenuto.
    assert_eq!(
        fs::read(nuova.join("copertine/ab/abcdef.jpg")).expect("copertina"),
        b"\xff\xd8\xff\xe0 una copertina"
    );
    assert_eq!(
        fs::read(nuova.join("skin/notte.ryskin")).expect("skin"),
        b"PK\x03\x04 un pacchetto"
    );
    assert!(!nuova.join("skin/notte.aeskin").exists());
    assert_eq!(
        fs::read(nuova.join("skin/bozze/b1.json")).expect("bozza"),
        b"{\"bozza\":1}"
    );
    assert_eq!(
        fs::read(nuova.join("profilo/prima-1700000000000.ryprofile")).expect("profilo"),
        b"PK\x03\x04 un profilo"
    );
    assert_eq!(
        fs::read_to_string(nuova.join("diario/rymembra.log")).expect("diario"),
        "riga di diario\n"
    );

    // Si apre il database come lo apre l'applicazione: migra dalla 30 in su.
    let aperto = db::open(&nuova.join("rymembra.db")).expect("apertura");
    assert_eq!(aperto.version_before, 30, "il giornale ha portato la 30");
    // Quante ne mancano dalla 30, e non «una»: il numero scritto a mano
    // diventava rosso alla prima migrazione aggiunta dopo la 31.
    assert_eq!(
        u32::try_from(aperto.applied).expect("il conto delle migrazioni sta in un u32"),
        LATEST_VERSION - 30,
        "dalla 30 mancano la 31 e tutte quelle dopo"
    );
    let connessione = &aperto.connection;
    let versione: u32 = connessione
        .query_row("PRAGMA user_version", [], |riga| riga.get(0))
        .expect("versione");
    assert_eq!(versione, LATEST_VERSION);

    // Le righe sono tutte lì — comprese quelle che stavano solo nel giornale — e
    // gli identificativi delle sorgenti hanno il nome di adesso.
    assert_eq!(conta(connessione, "tracks"), 1);
    assert_eq!(conta(connessione, "playlists"), 2);
    assert_eq!(conta(connessione, "playlist_tracks"), 1);
    assert_eq!(conta(connessione, "play_history"), 1);
    assert_eq!(conta(connessione, "desiderati"), 3);
    assert_eq!(conta(connessione, "import_reports"), 1);
    assert_eq!(
        colonna(
            connessione,
            "SELECT value FROM settings WHERE key = 'ui.tema'"
        ),
        ["scuro"]
    );
    assert_eq!(
        colonna(
            connessione,
            "SELECT source_id FROM desiderati ORDER BY added_at"
        ),
        [
            "rymembra:archivio:playlist:corsa",
            "rymembra:spotify:preferiti",
            PLAYLIST_SPOTIFY
        ]
    );
    assert_eq!(
        colonna(
            connessione,
            "SELECT COALESCE(source_playlist_id, '-') FROM playlists ORDER BY id"
        ),
        ["rymembra:archivio:playlist:corsa", "-"]
    );
    assert_eq!(
        colonna(connessione, "SELECT source_id FROM import_reports"),
        ["rymembra:spotify:preferiti"]
    );
    assert!(
        colonna(connessione, "SELECT payload FROM import_reports")[0]
            .contains("\"sourceId\":\"rymembra:spotify:preferiti\"")
    );

    // La copia di sicurezza della 30 c'è, col nome di adesso, ed è la libreria
    // **prima** della 31: ci si torna rinominandola.
    let attesa = db::nome_istantanea(&nuova.join("rymembra.db"), 30);
    assert_eq!(
        attesa.file_name().and_then(|nome| nome.to_str()),
        Some("rymembra.db.schema-030.bak")
    );
    assert!(
        matches!(&aperto.istantanea, Some(Istantanea::Fatta { percorso, .. }) if *percorso == attesa),
        "{:?}",
        aperto.istantanea
    );
    {
        let copia = Connection::open(&attesa).expect("la copia si apre");
        let versione_copia: u32 = copia
            .query_row("PRAGMA user_version", [], |riga| riga.get(0))
            .expect("versione");
        assert_eq!(versione_copia, 30);
        assert_eq!(
            colonna(&copia, "SELECT source_id FROM desiderati ORDER BY added_at")[0],
            "aether:archivio:playlist:corsa"
        );
    }
    drop(aperto);

    // Secondo avvio: il trasloco non ha niente da fare, e il database si riapre
    // senza migrare.
    let secondo = trasloca(&macchina.percorsi).expect("secondo trasloco");
    assert_eq!(secondo, Rapporto::default());
    let riaperto = db::open(&nuova.join("rymembra.db")).expect("riapertura");
    assert_eq!(riaperto.version_before, LATEST_VERSION);
    assert_eq!(riaperto.applied, 0);
    assert_eq!(conta(&riaperto.connection, "desiderati"), 3);
}

#[test]
fn le_copie_di_sicurezza_di_prima_seguono_il_database() {
    // Chi ha migrato con la versione di prima ha una copia `<db>.schema-NNN.bak`
    // accanto al database. Dopo il trasloco la copia ha il nome che la potatura
    // di adesso riconosce, quindi le due copie più recenti si tengono e le altre
    // se ne vanno come sempre.
    let macchina = Macchina::nuova();
    cartella_di_prima(&macchina);
    let precedente = macchina.precedente();
    for versione in [27, 28, 29] {
        let copia = db::nome_istantanea(&precedente.join(eredita::NOME_DATABASE), versione);
        fs::write(&copia, format!("copia {versione}")).expect("copia di prima");
    }

    trasloca(&macchina.percorsi).expect("trasloco");

    let nuova = macchina.nuova_cartella();
    for versione in [27, 28, 29] {
        let attesa = db::nome_istantanea(&nuova.join("rymembra.db"), versione);
        assert_eq!(
            fs::read_to_string(&attesa).expect("copia rinominata"),
            format!("copia {versione}")
        );
    }

    // Aprendo, la 30 si aggiunge e la potatura toglie le più vecchie: il nome
    // delle rinominate è lo stesso di quelle che fa lei.
    let aperto = db::open(&nuova.join("rymembra.db")).expect("apertura");
    drop(aperto);
    let rimaste: Vec<u32> = (0..=40)
        .filter(|versione| db::nome_istantanea(&nuova.join("rymembra.db"), *versione).exists())
        .collect();
    assert_eq!(rimaste.len(), db::ISTANTANEE_TENUTE, "{rimaste:?}");
    assert!(rimaste.contains(&30), "la copia appena fatta resta sempre");
}

/// Aether vero, aperto: il trasloco si rifiuta e non tocca niente.
///
/// Solo su Windows: è l'unico sistema dove un database aperto non si sposta, ed
/// è quello della sonda. La connessione è quella di SQLite, non un file tenuto
/// aperto a mano: è esattamente come lo tiene la versione di prima.
#[cfg(windows)]
#[test]
fn con_aether_aperto_non_si_tocca_niente() {
    let macchina = Macchina::nuova();
    cartella_di_prima(&macchina);
    let aperta = Connection::open(macchina.precedente().join(eredita::NOME_DATABASE))
        .expect("Aether apre il suo database");
    let giornale: String = aperta
        .query_row("PRAGMA journal_mode", [], |riga| riga.get(0))
        .expect("giornale");
    assert_eq!(giornale, "wal");
    assert_eq!(conta(&aperta, "desiderati"), 3);

    let errore = trasloca(&macchina.percorsi).expect_err("Aether è aperto");
    assert!(
        matches!(errore, rymembra_app::trasloco::Errore::InUso { .. }),
        "{errore:?}"
    );
    // Né la cartella nuova né un file di quella di prima sono cambiati.
    assert!(!macchina.nuova_cartella().exists());
    assert!(macchina.precedente().join(eredita::NOME_DATABASE).is_file());
    assert!(
        macchina
            .precedente()
            .join(format!("{}-wal", eredita::NOME_DATABASE))
            .is_file()
    );

    // Chiuso Aether, il trasloco riesce.
    drop(aperta);
    let rapporto = trasloca(&macchina.percorsi).expect("trasloco");
    assert!(rapporto.spostata);
    let aperto = db::open(&macchina.nuova_cartella().join("rymembra.db")).expect("apertura");
    assert_eq!(conta(&aperto.connection, "desiderati"), 3);
}
