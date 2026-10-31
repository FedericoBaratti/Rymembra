//! Le prove dei testi.
//!
//! Tutte in un file, come prima in fondo a `testi.rs`: condividono `libreria`, `inserisci` e `LRC`.

use super::catalogo::da_chiedere_al_catalogo;
use super::disco::{CHIAVE_SINCRONIZZATA, da_vorbis, lrc_da_sylt};
use super::*;
use crate::db;
use rymembra_meta::lrclib::Voce;

/// Le righe di prova non sono di nessuna canzone: quel che si prova qui è
/// da dove arriva un testo, non quale sia.
const LRC: &str = "[00:01.00]prima riga\n[00:02.00]seconda riga\n";

fn libreria() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let aperta = db::open(&dir.path().join("prova.db")).expect("apertura");
    (dir, aperta.connection)
}

fn inserisci(connection: &Connection, percorso: &Path, lyrics: Option<&str>) -> i64 {
    connection
        .execute(
            "INSERT INTO tracks
                   (path, track_key, title, artist, album, duration_ms, file_size,
                    date_added, date_modified, lyrics)
                 VALUES (?1, 'chiave', 'Titolo', 'Artista', 'Album', 120000, 1, 0, 0, ?2)",
            rusqlite::params![percorso.display().to_string(), lyrics],
        )
        .expect("inserimento");
    connection.last_insert_rowid()
}

#[test]
fn il_sidecar_vince_su_tutto() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    std::fs::write(brano.with_extension("lrc"), LRC).expect("sidecar");
    let id = inserisci(&connection, &brano, Some("un testo nel tag"));

    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Sidecar);
    assert_eq!(esito.testo.righe.len(), 2);
    // …e si è ricordato in tabella, così l'altro file dello stesso brano lo
    // trova senza avere il sidecar accanto.
    let riga = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(riga.fonte, Fonte::Sidecar);
    assert_eq!(riga.synced.as_deref(), Some(LRC));
}

#[test]
fn lo_scarto_non_spegne_le_parole_di_una_sincronia_a_mano() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");

    // Com'è dopo l'editor: in riga il gemello semplice, accanto al brano
    // l'`.a2.lrc` con i tempi delle parole.
    registra_a_mano(&connection, &quale, "[00:10.00]prima riga\n").expect("salvato");
    let esteso = brano.with_extension("a2.lrc");
    std::fs::write(&esteso, "[00:10.00]<00:10.00>prima <00:10.40>riga\n").expect("esteso");
    // Un colpo sullo scarto dopo: `updated_at` passa davanti al file.
    std::fs::File::options()
        .write(true)
        .open(&esteso)
        .and_then(|file| {
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000))
        })
        .expect("data del file");
    imposta_scarto(&connection, "chiave", -100).expect("scarto");

    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Sidecar, "l'esteso resta la fonte");
    assert_eq!(
        esito.testo.righe.first().map(|r| r.parole.len()),
        Some(2),
        "e le parole restano accese"
    );

    // Un sidecar vecchio che dice **altre** righe resta invece superato: è
    // il file che il salvataggio non è riuscito a riscrivere.
    std::fs::write(&esteso, "[00:05.00]<00:05.00>vecchia\n").expect("vecchio");
    std::fs::File::options()
        .write(true)
        .open(&esteso)
        .and_then(|file| {
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000))
        })
        .expect("data del file");
    let superato = per_brano(&connection, id).expect("testo");
    assert_eq!(superato.fonte, Fonte::Mano);
}

#[test]
fn senza_sidecar_si_ripiega_sul_tag() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, Some("prima riga\nseconda riga"));

    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Tag);
    assert!(!esito.testo.sincronizzato());
    assert_eq!(
        esito.testo.piatto.as_deref(),
        Some("prima riga\nseconda riga")
    );
}

#[test]
fn un_lrc_dentro_il_tag_si_riconosce_da_se() {
    // Capita davvero: c'è chi mette un LRC intero dentro `USLT`. Non serve
    // un ramo apposta — il lettore del dominio vede i tempi e li usa.
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, Some(LRC));

    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Tag);
    assert!(esito.testo.sincronizzato());
}

#[test]
fn un_sidecar_senza_tempi_non_copre_i_tempi_che_si_hanno() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    std::fs::write(brano.with_extension("lrc"), "prima riga\nseconda riga").expect("sidecar");
    let id = inserisci(&connection, &brano, None);
    connection
        .execute(
            "INSERT INTO lyrics (track_key, synced, source, checked_at, updated_at)
                 VALUES ('chiave', ?1, 'lrclib', 1, 1)",
            rusqlite::params![LRC],
        )
        .expect("riga con i tempi");

    // Il primato del sidecar cede davanti all'unica cosa che il sidecar non
    // ha: i tempi.
    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Lrclib);
    assert_eq!(esito.testo.righe.len(), 2);
    // E soprattutto: non ci ha scritto sopra.
    let riga = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(riga.synced.as_deref(), Some(LRC));
}

#[test]
fn un_testo_senza_tempi_resta_da_chiedere() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, Some("prima riga\nseconda riga"));

    // Le parole ci sono, i tempi no, e al catalogo non gli si è ancora
    // chiesto niente: è il caso in cui il pannello deve andare a vedere.
    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Tag);
    assert!(esito.da_chiedere);

    // Quel che scorre invece non si chiede più.
    connection
        .execute(
            "UPDATE tracks SET lyrics = ?1 WHERE id = ?2",
            rusqlite::params![LRC, id],
        )
        .expect("tag con i tempi");
    connection
        .execute("DELETE FROM lyrics WHERE track_key = 'chiave'", [])
        .expect("riga via");
    let esito = per_brano(&connection, id).expect("testo");
    assert!(esito.testo.sincronizzato());
    assert!(!esito.da_chiedere);
}

#[test]
fn la_coda_rimette_in_fila_chi_ha_solo_le_parole() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    inserisci(&connection, &brano, Some("prima riga"));
    // La riga che la catena scrive leggendo il tag: parole sì, tempi no, e
    // `checked_at` vuoto perché al catalogo non gli si è chiesto niente.
    connection
        .execute(
            "INSERT INTO lyrics (track_key, plain, source, updated_at)
                 VALUES ('chiave', 'prima riga', 'tag', 1)",
            [],
        )
        .expect("riga piatta");
    assert_eq!(
        da_cercare(&connection, 1_000, 10).expect("coda").len(),
        1,
        "le parole senza tempi non sono una domanda già fatta"
    );

    // Con i tempi non c'è più niente da chiedere, e non c'è scadenza che
    // lo rimetta in coda.
    connection
        .execute(
            "UPDATE lyrics SET synced = ?1 WHERE track_key = 'chiave'",
            rusqlite::params![LRC],
        )
        .expect("tempi");
    assert!(
        da_cercare(&connection, 1_000 + RIPROVA_MS + 1, 10)
            .expect("coda")
            .is_empty()
    );

    // Un «senza tempi» che il catalogo ha già confermato aspetta invece il
    // suo turno: `RIPROVA_MS`, non la prossima passata.
    connection
        .execute(
            "UPDATE lyrics SET synced = NULL, source = 'lrclib', checked_at = 1000
                 WHERE track_key = 'chiave'",
            [],
        )
        .expect("già chiesto");
    assert!(da_cercare(&connection, 1_000, 10).expect("coda").is_empty());
    assert_eq!(
        da_cercare(&connection, 1_000 + RIPROVA_MS + 1, 10)
            .expect("coda")
            .len(),
        1
    );
}

#[test]
fn niente_da_nessuna_parte_e_una_risposta() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);

    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Nessuna);
    assert!(esito.testo.vuoto());
    // Non è ancora stato chiesto a nessuno: è questo che permette a chi
    // chiama di andare in rete.
    assert!(!esito.cercato);
}

#[test]
fn quel_che_ha_scritto_l_utente_non_si_sovrascrive() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    std::fs::write(brano.with_extension("lrc"), LRC).expect("sidecar");
    let id = inserisci(&connection, &brano, None);
    connection
        .execute(
            "INSERT INTO lyrics (track_key, synced, source, updated_at)
                 VALUES ('chiave', '[00:09.00]a mano', 'mano', 1)",
            [],
        )
        .expect("riga a mano");

    // Il sidecar si mostra — è il file su disco, ed è la verità —
    // ma la riga di `mano` resta quella che era.
    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Sidecar);
    let riga = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(riga.fonte, Fonte::Mano);
    assert_eq!(riga.synced.as_deref(), Some("[00:09.00]a mano"));
}

#[test]
fn uno_strumentale_non_manda_a_cercare_altro() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, Some("un testo nel tag"));
    connection
        .execute(
            "INSERT INTO lyrics (track_key, source, instrumental, checked_at, updated_at)
                 VALUES ('chiave', 'lrclib', 1, 1, 1)",
            [],
        )
        .expect("riga strumentale");

    let esito = per_brano(&connection, id).expect("testo");
    assert!(esito.testo.strumentale);
    assert!(esito.cercato);
}

#[test]
fn l_aderenza_si_misura_sulla_durata_di_questo_file() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    // Il brano dura due minuti; questo testo finisce al secondo due.
    std::fs::write(brano.with_extension("lrc"), LRC).expect("sidecar");
    let id = inserisci(&connection, &brano, None);

    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.aderenza, Aderenza::Sospetta);
}

#[test]
fn lo_scarto_si_salva_e_si_rilegge() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    std::fs::write(brano.with_extension("lrc"), LRC).expect("sidecar");
    let id = inserisci(&connection, &brano, None);

    imposta_scarto(&connection, "chiave", -250).expect("scarto");
    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.scarto_ms, -250);
}

#[test]
fn uno_scarto_messo_prima_del_testo_non_blocca_il_catalogo() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");

    // Il cursore della correzione si tocca mentre il catalogo sta ancora
    // rispondendo: la riga nasce qui, e non ha ancora nessun testo.
    imposta_scarto(&connection, "chiave", -300).expect("scarto");
    let appena_nata = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(
        appena_nata.fonte,
        Fonte::Nessuna,
        "una riga senza testo non è stata scritta da nessuno"
    );

    // E la risposta del catalogo, quando arriva, si scrive: era il difetto
    // — con `source = 'mano'` la `WHERE` dell'`UPSERT` non lasciava
    // scrivere più niente, e quel brano restava senza testo per sempre.
    ricorda_esito(
        &connection,
        &quale,
        Some(&Voce {
            candidato: testo::Candidato {
                id: 7,
                titolo: "Titolo".to_owned(),
                artista: "Artista".to_owned(),
                album: Some("Album".to_owned()),
                durata_ms: Some(120_000),
                sincronizzato: true,
                strumentale: false,
            },
            piatto: Some("prima riga\nseconda riga".to_owned()),
            sincronizzato: Some(LRC.to_owned()),
        }),
    )
    .expect("esito");

    let dopo = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(dopo.fonte, Fonte::Lrclib);
    assert_eq!(dopo.synced.as_deref(), Some(LRC));
    // E lo scarto che si era messo è ancora quello: `source` cambia, la
    // correzione di chi ascolta no.
    assert_eq!(dopo.scarto_ms, -300);

    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Lrclib);
    assert_eq!(esito.scarto_ms, -300);
}

#[test]
fn quel_che_si_sincronizza_finisce_accanto_al_brano() {
    let (dir, connection) = libreria();
    // Chi lo ha chiesto: di serie il `.lrc` non si scrive.
    crate::settings::write(&connection, CHIAVE_ACCANTO, "1").expect("interruttore");
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let riga = super::brano(&connection, id).expect("riga");

    salva_a_mano(&connection, &riga, LRC).expect("salvataggio");

    // Il file, che è la verità…
    let sidecar = brano.with_extension("lrc");
    assert_eq!(std::fs::read_to_string(&sidecar).expect("sidecar"), LRC);
    // …e la riga, che ne è la copia e dice di chi è.
    let salvata = leggi_riga(&connection, "chiave").expect("riga salvata");
    assert_eq!(salvata.fonte, Fonte::Mano);
    assert!(salvata.cercato, "non lo si cerca più in rete");

    // E si rilegge come sincronizzato.
    let esito = per_brano(&connection, id).expect("testo");
    assert!(esito.testo.sincronizzato());
    assert_eq!(esito.fonte, Fonte::Sidecar);
}

#[test]
fn un_brano_d_immagine_tiene_i_testi_solo_nel_database() {
    let (dir, connection) = libreria();
    crate::settings::write(&connection, CHIAVE_ACCANTO, "1").expect("interruttore");
    let cue = dir.path().join("Album.cue").display().to_string();
    let immagine = dir.path().join("Album.flac");
    std::fs::write(&immagine, b"finto").expect("immagine");
    // Il `.lrc` che il percorso virtuale troverebbe da solo: `Album.cue|03`
    // con l'estensione cambiata è `Album.lrc`, che è del disco intero.
    let del_disco = dir.path().join("Album.lrc");
    std::fs::write(&del_disco, "[00:01.00]del disco\n").expect("sidecar del disco");
    connection
        .execute(
            "INSERT INTO tracks
                   (path, track_key, title, artist, album, duration_ms, file_size,
                    date_added, date_modified, immagine, inizio_cd, fine_cd)
                 VALUES (?1, 'chiave', 'Titolo', 'Artista', 'Album', 120000, 1, 0, 0,
                         ?2, 0, 9000)",
            rusqlite::params![
                rymembra_domain::cue::percorso_del_brano(&cue, 3),
                immagine.display().to_string()
            ],
        )
        .expect("brano d'immagine");
    let id = connection.last_insert_rowid();

    let riga = super::brano(&connection, id).expect("riga");
    assert_eq!(
        riga.path, None,
        "come un brano di catalogo: nessun file accanto"
    );
    let esito = per_brano(&connection, id).expect("testo");
    assert_ne!(esito.fonte, Fonte::Sidecar, "il `.lrc` del disco non è suo");

    salva_a_mano(&connection, &riga, LRC).expect("salvataggio");
    let salvata = leggi_riga(&connection, "chiave").expect("riga salvata");
    assert_eq!(
        salvata.synced.as_deref(),
        Some(LRC),
        "il testo sta nel database"
    );
    assert_eq!(
        std::fs::read_to_string(&del_disco).expect("sidecar del disco"),
        "[00:01.00]del disco\n",
        "accanto all'immagine non si scrive niente"
    );
    assert!(!dir.path().join("Album.lrc.bak").exists());
}

#[test]
fn il_lrc_di_qualcun_altro_resta_in_un_bak() {
    let (dir, connection) = libreria();
    // Chi lo ha chiesto: di serie il `.lrc` non si scrive.
    crate::settings::write(&connection, CHIAVE_ACCANTO, "1").expect("interruttore");
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let sidecar = brano.with_extension("lrc");
    let copia = brano.with_extension("lrc.bak");
    std::fs::write(&sidecar, "[00:05.00]il testo di un altro programma").expect("altrui");
    let id = inserisci(&connection, &brano, None);
    let riga = super::brano(&connection, id).expect("riga");

    salva_a_mano(&connection, &riga, LRC).expect("salvataggio");
    assert_eq!(std::fs::read_to_string(&sidecar).expect("nuovo"), LRC);
    assert_eq!(
        std::fs::read_to_string(&copia).expect("la copia"),
        "[00:05.00]il testo di un altro programma"
    );

    // La seconda volta il `.bak` resta quello di prima: è l'originale.
    salva_a_mano(&connection, &riga, "[00:09.00]rifatto").expect("secondo");
    assert_eq!(
        std::fs::read_to_string(&copia).expect("la copia"),
        "[00:05.00]il testo di un altro programma"
    );

    // E togliere un esteso non lo cancella: lo mette da parte.
    let esteso = brano.with_extension("a2.lrc");
    std::fs::write(&esteso, "[00:01.00]<00:01.00>parola").expect("esteso altrui");
    scrivi_sidecar(&brano, "a2.lrc", None).expect("rimozione");
    assert!(!esteso.exists());
    assert!(brano.with_extension("a2.lrc.bak").exists());
}

/// Un sidecar che c'è ma non si lascia leggere non si toglie alla cieca.
///
/// Il caso vero è un altro programma che lo tiene aperto senza condividerne
/// la lettura — un editor di testi, un sincronizzatore — o una share che
/// risponde a metà. Prima l'errore di lettura valeva «non c'è niente»:
/// nessun `.bak`, e la rimozione — che Windows concede a chi ha condiviso
/// la cancellazione — portava via il file dell'utente senza copia.
#[cfg(windows)]
#[test]
fn un_sidecar_che_non_si_legge_non_si_toglie_senza_copia() {
    use std::os::windows::fs::OpenOptionsExt as _;
    /// `FILE_SHARE_DELETE`: si condivide la cancellazione e nient'altro.
    const SOLO_CANCELLAZIONE: u32 = 0x0000_0004;

    let dir = tempfile::tempdir().expect("cartella");
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let sidecar = brano.with_extension("lrc");
    std::fs::write(&sidecar, "[00:05.00]il testo di qualcun altro").expect("altrui");

    let tenuto = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SOLO_CANCELLAZIONE)
        .open(&sidecar)
        .expect("un altro programma lo tiene aperto");
    let esito = scrivi_sidecar(&brano, "lrc", None);
    drop(tenuto);

    assert!(esito.is_err(), "un file illeggibile non è un file assente");
    assert_eq!(
        std::fs::read_to_string(&sidecar).expect("il file c'è ancora"),
        "[00:05.00]il testo di qualcun altro"
    );
    assert!(!brano.with_extension("lrc.bak").exists());
}

#[test]
fn un_file_che_non_si_scrive_non_perde_il_lavoro() {
    let (dir, connection) = libreria();
    // Chi lo ha chiesto: di serie il `.lrc` non si scrive.
    crate::settings::write(&connection, CHIAVE_ACCANTO, "1").expect("interruttore");
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let sidecar = brano.with_extension("lrc");
    // Un `.bak` c'è già, e il sidecar di prima è di sola lettura: la
    // scrittura del nuovo non può riuscire.
    std::fs::write(brano.with_extension("lrc.bak"), "originale").expect("bak");
    std::fs::write(&sidecar, "[00:05.00]il testo di prima").expect("vecchio");
    let mut permessi = std::fs::metadata(&sidecar).expect("permessi").permissions();
    permessi.set_readonly(true);
    std::fs::set_permissions(&sidecar, permessi.clone()).expect("sola lettura");
    std::thread::sleep(std::time::Duration::from_millis(20));

    let id = inserisci(&connection, &brano, None);
    let riga = super::brano(&connection, id).expect("riga");
    let esito = salva_a_mano(&connection, &riga, LRC);
    assert_eq!(
        esito
            .expect_err("il file non si scrive")
            .code()
            .kind()
            .code(),
        "fs.writeFailed"
    );

    // La riga però è salva, e il sidecar vecchio non la scavalca.
    let testo = per_brano(&connection, id).expect("testo");
    assert_eq!(testo.fonte, Fonte::Mano);
    assert_eq!(
        testo.testo.righe.first().map(|r| r.testo.as_str()),
        Some("prima riga")
    );

    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "si toglie la sola lettura messa dalla prova, perché la cartella temporanea si possa cancellare"
    )]
    permessi.set_readonly(false);
    let _ = std::fs::set_permissions(&sidecar, permessi);
}

#[test]
fn di_serie_accanto_al_brano_non_si_scrive_niente() {
    // Il salvataggio è tutto nella riga: nessun `.lrc`, nessun `.bak`,
    // nemmeno quando accanto c'è il file di un altro programma — che resta
    // com'era, e non scavalca il testo appena salvato perché è più vecchio.
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let altrui = brano.with_extension("lrc");
    std::fs::write(&altrui, "[00:05.00]il testo di un altro programma").expect("altrui");
    std::fs::File::options()
        .write(true)
        .open(&altrui)
        .and_then(|file| {
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000))
        })
        .expect("data del file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");
    assert!(!accanto_ai_file(&connection), "spento di serie");

    salva_a_mano(&connection, &quale, LRC).expect("salvataggio");

    assert_eq!(
        std::fs::read_to_string(&altrui).expect("il file altrui"),
        "[00:05.00]il testo di un altro programma",
        "il file di un altro non si tocca"
    );
    assert!(!brano.with_extension("lrc.bak").exists());
    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Mano, "la riga è la fonte");
    assert_eq!(
        esito.testo.righe.first().map(|r| r.testo.as_str()),
        Some("prima riga")
    );
}

#[test]
fn le_parole_ribattute_non_le_scavalca_il_vecchio_a2() {
    // Con i file accanto spenti, l'`.a2.lrc` di una sincronia di ieri resta
    // lì. Stesse righe, parole diverse: la riga, più recente, porta le
    // parole nuove e vince lei.
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let esteso = brano.with_extension("a2.lrc");
    std::fs::write(&esteso, "[00:10.00]<00:10.00>prima <00:10.40>riga\n").expect("vecchio");
    std::fs::File::options()
        .write(true)
        .open(&esteso)
        .and_then(|file| {
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000))
        })
        .expect("data del file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");

    registra_a_mano(
        &connection,
        &quale,
        "[00:10.00]<00:10.00>prima <00:10.90>riga\n",
    )
    .expect("salvato");

    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Mano);
    assert_eq!(
        esito
            .testo
            .righe
            .first()
            .and_then(|r| r.parole.get(1))
            .map(|p| p.ms),
        Some(10_900),
        "le parole di oggi, non quelle di ieri"
    );
}

#[test]
fn al_catalogo_non_vanno_i_tempi_delle_parole() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");
    registra_a_mano(
        &connection,
        &quale,
        "[00:01.00]<00:01.00>prima <00:01.50>riga\n[00:02.00]seconda riga\n",
    )
    .expect("salvato");

    let cosa = da_restituire(&connection, id).expect("pronto");
    assert!(
        !cosa.sincronizzato.contains('<'),
        "i tempi delle parole restano a casa: {}",
        cosa.sincronizzato
    );
    let letto = testo::leggi(&cosa.sincronizzato);
    assert_eq!(letto.righe.len(), 2);
    assert_eq!(cosa.piatto, "prima riga\nseconda riga");
}

#[test]
fn risincronizzare_sovrascrive_quel_che_c_era() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let riga = super::brano(&connection, id).expect("riga");

    salva_a_mano(&connection, &riga, LRC).expect("primo");
    salva_a_mano(&connection, &riga, "[00:09.00]rifatto").expect("secondo");

    let salvata = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(salvata.synced.as_deref(), Some("[00:09.00]rifatto"));
    assert_eq!(salvata.plain, None);
}

/// Una voce del catalogo con i tempi, per le prove che scrivono un esito.
fn voce_con_i_tempi() -> Voce {
    Voce {
        candidato: testo::Candidato {
            id: 7,
            titolo: "Titolo".to_owned(),
            artista: "Artista".to_owned(),
            album: Some("Album".to_owned()),
            durata_ms: Some(120_000),
            sincronizzato: true,
            strumentale: false,
        },
        piatto: Some("prima riga\nseconda riga".to_owned()),
        sincronizzato: Some(LRC.to_owned()),
    }
}

#[test]
fn un_no_del_catalogo_non_cancella_il_testo_che_si_aveva() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");

    ricorda_esito(&connection, &quale, Some(&voce_con_i_tempi())).expect("il sì");
    dimentica_esito(&connection, "chiave").expect("cerca di nuovo");
    // «Cerca di nuovo», e questa volta il catalogo non risponde niente.
    ricorda_esito(&connection, &quale, None).expect("il no");

    let riga = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(riga.synced.as_deref(), Some(LRC), "il testo di ieri resta");
    assert!(riga.cercato, "e si sa di aver chiesto");

    // Su un brano che una riga non l'aveva, il «no» la crea vuota: è la
    // memoria di un brano che nessuno conosce.
    connection
        .execute("DELETE FROM lyrics", [])
        .expect("tabella vuota");
    ricorda_esito(&connection, &quale, None).expect("il primo no");
    let vuota = leggi_riga(&connection, "chiave").expect("riga vuota");
    assert!(vuota.grezzo().is_none());
    assert!(vuota.cercato);
}

#[test]
fn rifiutare_butta_solo_quel_che_viene_dal_catalogo() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");

    ricorda_esito(&connection, &quale, Some(&voce_con_i_tempi())).expect("esito");
    imposta_scarto(&connection, "chiave", 200).expect("scarto");
    rifiuta(&connection, "chiave").expect("non è questo");
    let riga = leggi_riga(&connection, "chiave").expect("riga");
    assert!(riga.grezzo().is_none(), "il testo sbagliato se n'è andato");
    assert!(riga.cercato, "e non si va a riprenderlo da soli");
    assert_eq!(riga.scarto_ms, 200, "la correzione di chi ascolta resta");
    assert!(per_brano(&connection, id).is_ok_and(|t| !t.da_chiedere));

    // Quel che ha sincronizzato chi ascolta non si butta da qui.
    salva_a_mano(&connection, &quale, LRC).expect("a mano");
    rifiuta(&connection, "chiave").expect("niente da fare");
    let mano = leggi_riga(&connection, "chiave").expect("riga di mano");
    assert_eq!(mano.synced.as_deref(), Some(LRC));
}

/// Una voce senza tempi, diversa da [`voce_con_i_tempi`].
fn voce_piatta(id: i64, parole: &str) -> Voce {
    Voce {
        candidato: testo::Candidato {
            id,
            titolo: "Titolo".to_owned(),
            artista: "Artista".to_owned(),
            album: None,
            durata_ms: Some(120_000),
            sincronizzato: false,
            strumentale: false,
        },
        piatto: Some(parole.to_owned()),
        sincronizzato: None,
    }
}

#[test]
fn una_voce_scartata_non_torna_con_il_catalogo() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");

    ricorda_esito(&connection, &quale, Some(&voce_con_i_tempi())).expect("esito");
    rifiuta(&connection, "chiave").expect("nessuno di questi");
    // «Cerca di nuovo», o la passata fra quattordici giorni: il catalogo
    // risponde la stessa voce di prima.
    dimentica_esito(&connection, "chiave").expect("cerca di nuovo");
    ricorda_esito(&connection, &quale, Some(&voce_con_i_tempi())).expect("di nuovo lei");
    let riga = leggi_riga(&connection, "chiave").expect("riga");
    assert!(riga.grezzo().is_none(), "la voce scartata non torna");
    assert!(riga.cercato, "ma si sa di aver chiesto");

    // Una voce diversa invece sì.
    ricorda_esito(&connection, &quale, Some(&voce_piatta(8, "un'altra"))).expect("altra");
    let altra = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(altra.plain.as_deref(), Some("un'altra"));

    // E la voce scartata, scelta a mano dall'elenco, è un ripensamento.
    ricorda_scelta(&connection, &quale, &voce_con_i_tempi()).expect("ripensamento");
    let scelta = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(scelta.synced.as_deref(), Some(LRC));
    let scartati: Option<String> = connection
        .query_row(
            "SELECT scartati FROM lyrics WHERE track_key = 'chiave'",
            [],
            |r| r.get(0),
        )
        .expect("scartati");
    assert_eq!(scartati.as_deref(), Some("[]"));
}

#[test]
fn una_voce_scelta_a_mano_resta_sua() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");
    let adesso = now_ms();

    ricorda_scelta(&connection, &quale, &voce_piatta(8, "quella giusta")).expect("scelta");
    // Senza tempi, e fra quattordici giorni: prima tornava in coda.
    let fra_un_mese = adesso.saturating_add(RIPROVA_MS).saturating_add(60_000);
    assert!(
        da_cercare(&connection, fra_un_mese, LOTTO)
            .expect("coda")
            .is_empty(),
        "la passata non la richiede"
    );
    // E se il catalogo risponde lo stesso, non le passa sopra.
    ricorda_esito(&connection, &quale, Some(&voce_con_i_tempi())).expect("il catalogo");
    let riga = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(riga.plain.as_deref(), Some("quella giusta"));
    assert_eq!(riga.synced, None);

    // Scartarla la rimette nelle mani del catalogo.
    rifiuta(&connection, "chiave").expect("nessuno di questi");
    ricorda_esito(&connection, &quale, Some(&voce_con_i_tempi())).expect("il catalogo");
    let dopo = leggi_riga(&connection, "chiave").expect("riga");
    assert_eq!(dopo.synced.as_deref(), Some(LRC));
}

#[test]
fn un_guasto_toglie_il_brano_dalla_coda_per_un_giorno_solo() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");
    let adesso = now_ms();
    assert_eq!(
        da_cercare(&connection, adesso, LOTTO).expect("coda").len(),
        1
    );

    rimanda(&connection, &quale).expect("rinvio");
    assert!(
        da_cercare(&connection, adesso, LOTTO)
            .expect("coda")
            .is_empty(),
        "la passata dopo non ricomincia da lui"
    );
    let domani = adesso
        .saturating_add(RIPROVA_DOPO_UN_GUASTO_MS)
        .saturating_add(60_000);
    assert_eq!(
        da_cercare(&connection, domani, LOTTO).expect("coda").len(),
        1,
        "e fra un giorno ci si riprova"
    );

    // Una risposta vera, arrivata dopo, non si fa abbassare da un guasto.
    ricorda_esito(&connection, &quale, None).expect("il no");
    rimanda(&connection, &quale).expect("un altro guasto");
    assert!(
        da_cercare(&connection, domani, LOTTO)
            .expect("coda")
            .is_empty()
    );
}

#[test]
fn il_testo_segue_il_brano_quando_la_chiave_cambia() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");
    ricorda_esito(&connection, &quale, Some(&voce_con_i_tempi())).expect("esito");
    imposta_scarto(&connection, "chiave", -150).expect("scarto");

    // Un secondo file dello stesso brano: finché lui porta la chiave
    // vecchia, correggere il primo non gli porta via il testo.
    let gemello = dir.path().join("brano.flac");
    connection
        .execute(
            "INSERT INTO tracks
                   (path, track_key, title, artist, album, duration_ms, file_size,
                    date_added, date_modified)
                 VALUES (?1, 'chiave', 'Titolo', 'Artista', 'Album', 120000, 1, 0, 0)",
            [gemello.display().to_string()],
        )
        .expect("gemello");
    let gemello_id = connection.last_insert_rowid();
    let corregge = |quale: i64| {
        connection
            .execute(
                "UPDATE tracks SET track_key = 'chiave-corretta' WHERE id = ?1",
                [quale],
            )
            .expect("correzione dei tag")
    };
    corregge(id);
    assert!(
        leggi_riga(&connection, "chiave").is_some(),
        "il gemello lo tiene"
    );
    assert!(leggi_riga(&connection, "chiave-corretta").is_none());

    // Corretti tutti e due, il testo è passato alla chiave nuova, con lo
    // scarto di chi ascolta.
    corregge(gemello_id);
    assert!(leggi_riga(&connection, "chiave").is_none());
    let seguita = leggi_riga(&connection, "chiave-corretta").expect("il testo ha seguito");
    assert_eq!(seguita.synced.as_deref(), Some(LRC));
    assert_eq!(seguita.scarto_ms, -150);

    // La chiave nuova ha già una riga vuota — «il catalogo non conosceva
    // quel nome» — e la riga piena prende il suo posto.
    connection
        .execute(
            "INSERT INTO lyrics (track_key, source, checked_at, updated_at)
                 VALUES ('chiave-vuota', 'lrclib', 1, 1)",
            [],
        )
        .expect("riga vuota");
    connection
        .execute(
            "UPDATE tracks SET track_key = 'chiave-vuota' WHERE id IN (?1, ?2)",
            [id, gemello_id],
        )
        .expect("verso una chiave con una riga vuota");
    let dopo = leggi_riga(&connection, "chiave-vuota").expect("la riga piena ha preso il posto");
    assert_eq!(dopo.synced.as_deref(), Some(LRC));
}

#[test]
fn i_segnaposto_non_arrivano_al_catalogo() {
    let brano = |percorso: &str, titolo: &str, artista: &str, album: &str| BranoDaTestare {
        id: 1,
        path: Some(percorso.to_owned()),
        track_key: "chiave".to_owned(),
        title: titolo.to_owned(),
        artist: artista.to_owned(),
        album: album.to_owned(),
        duration_ms: 200_000,
        lyrics: None,
    };

    // Il nome del file porta l'artista: si prende da lì, e il titolo senza
    // numero e senza artista davanti.
    let domanda = da_chiedere_al_catalogo(&brano(
        r"D:\Scaricati\03 - Verdena - Luna.mp3",
        "03 - Verdena - Luna",
        "Artista sconosciuto",
        "Album sconosciuto",
    ));
    assert_eq!(domanda.artista, "Verdena");
    assert_eq!(domanda.titolo, "Luna");
    assert_eq!(domanda.album, None, "l'album segnaposto si omette");

    // Il nome del file non lo porta: l'artista resta vuoto, e il catalogo
    // si interroga per titolo.
    let domanda = da_chiedere_al_catalogo(&brano(
        r"D:\Musica\Luna.mp3",
        "Luna",
        "Unknown Artist",
        "Mondo",
    ));
    assert_eq!(domanda.artista, "");
    assert_eq!(domanda.titolo, "Luna");
    assert_eq!(domanda.album.as_deref(), Some("Mondo"));

    // Con i tag giusti non cambia niente.
    let domanda = da_chiedere_al_catalogo(&brano(
        r"D:\Musica\x.mp3",
        "Poetica (Official Video)",
        "Cesare Cremonini",
        "Possibili scenari",
    ));
    assert_eq!(domanda.artista, "Cesare Cremonini");
    assert_eq!(domanda.titolo, "Poetica");
}

#[test]
fn un_brano_che_non_c_e_lo_dice() {
    let (_dir, connection) = libreria();
    let errore = per_brano(&connection, 999).expect_err("nessun brano");
    assert_eq!(errore.code().kind().code(), "library.trackNotFound");
}

#[test]
fn un_sidecar_illeggibile_e_una_fonte_in_meno_non_un_errore() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    // Byte che non sono UTF-8: si legge lo stesso quel che si può.
    std::fs::write(brano.with_extension("lrc"), b"[00:01.00]prima \xff riga").expect("sidecar");
    let id = inserisci(&connection, &brano, None);

    let esito = per_brano(&connection, id).expect("testo");
    assert_eq!(esito.fonte, Fonte::Sidecar);
    assert_eq!(esito.testo.righe.len(), 1);
}

#[test]
fn si_restituisce_solo_quel_che_si_e_sincronizzato_a_mano() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);
    let quale = super::brano(&connection, id).expect("brano");

    // Quel che è arrivato dal catalogo torna indietro da dove è venuto:
    // niente.
    ricorda_esito(
        &connection,
        &quale,
        Some(&Voce {
            candidato: testo::Candidato {
                id: 7,
                titolo: "Titolo".to_owned(),
                artista: "Artista".to_owned(),
                album: Some("Album".to_owned()),
                durata_ms: Some(120_000),
                sincronizzato: true,
                strumentale: false,
            },
            piatto: Some("prima riga\nseconda riga".to_owned()),
            sincronizzato: Some(LRC.to_owned()),
        }),
    )
    .expect("esito");

    let errore = da_restituire(&connection, id).expect_err("non è roba nostra");
    assert_eq!(errore.code().kind().code(), "metadata.lyricsPublishRefused");
}

#[test]
fn il_piatto_che_si_manda_esce_dai_tempi_che_si_mandano() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    // Un testo nel tag che dice tutt'altro: se finisse nel `plainLyrics`
    // il catalogo riceverebbe due testi diversi nello stesso invio.
    let id = inserisci(&connection, &brano, Some("tutt'altre parole"));
    let quale = super::brano(&connection, id).expect("brano");
    salva_a_mano(&connection, &quale, LRC).expect("salvataggio");

    let cosa = da_restituire(&connection, id).expect("pronto");
    assert_eq!(cosa.piatto, "prima riga\nseconda riga");
    assert_eq!(cosa.sincronizzato, LRC);
    assert_eq!(cosa.durata_ms, 120_000);
}

/// Un brano sincronizzato a mano e pronto da pubblicare, così le prove che
/// seguono possono guastare **un** campo alla volta.
fn pronto_da_pubblicare(dir: &tempfile::TempDir, connection: &Connection) -> i64 {
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(connection, &brano, None);
    let quale = super::brano(connection, id).expect("brano");
    salva_a_mano(connection, &quale, LRC).expect("salvataggio");
    id
}

#[test]
fn non_si_restituisce_un_brano_senza_artista() {
    let (dir, connection) = libreria();
    let id = pronto_da_pubblicare(&dir, &connection);
    // Così com'è, si pubblica: è il termine di paragone di tutto il resto.
    da_restituire(&connection, id).expect("pronto");

    // Quel che la scansione scrive quando il tag `artist` non c'è.
    for assente in [
        rymembra_domain::album::UNKNOWN_ARTIST,
        // E le stesse assenze scritte da un taggatore altrui: il confronto
        // è sul valore, non su una traduzione.
        "Unknown Artist",
        "Various Artists",
        "<unknown>",
        "   ",
    ] {
        connection
            .execute(
                "UPDATE tracks SET artist = ?1 WHERE id = ?2",
                rusqlite::params![assente, id],
            )
            .expect("artista assente");
        let errore = da_restituire(&connection, id).err().unwrap_or_else(|| {
            panic!("«{assente}» non è un artista da mandare in un catalogo pubblico")
        });
        assert_eq!(errore.code().kind().code(), "metadata.lyricsPublishRefused");
    }
}

#[test]
fn non_si_restituisce_un_brano_senza_titolo() {
    let (dir, connection) = libreria();
    let id = pronto_da_pubblicare(&dir, &connection);

    // `Traccia 01` è il ripiego dal nome del file, non un titolo.
    for assente in ["Traccia 01", "Unknown Title", "Untitled", ""] {
        connection
            .execute(
                "UPDATE tracks SET title = ?1 WHERE id = ?2",
                rusqlite::params![assente, id],
            )
            .expect("titolo assente");
        let errore = da_restituire(&connection, id)
            .err()
            .unwrap_or_else(|| panic!("«{assente}» non è un titolo"));
        assert_eq!(errore.code().kind().code(), "metadata.lyricsPublishRefused");
    }
}

#[test]
fn un_album_sconosciuto_si_manda_vuoto_invece_di_far_cadere_tutto() {
    let (dir, connection) = libreria();
    let id = pronto_da_pubblicare(&dir, &connection);
    connection
        .execute(
            "UPDATE tracks SET album = ?1 WHERE id = ?2",
            rusqlite::params![rymembra_domain::album::UNKNOWN_ALBUM, id],
        )
        .expect("album assente");

    // Un singolo fuori da ogni disco si contribuisce comunque: l'album è
    // facoltativo per il catalogo. Quel che non deve uscire è la sentinella.
    let cosa = da_restituire(&connection, id).expect("si pubblica comunque");
    assert_eq!(cosa.album, "");
    assert_eq!(cosa.artista, "Artista");
    assert_eq!(cosa.titolo, "Titolo");
}

#[test]
fn una_durata_diversa_rende_sospetto_un_testo_che_ci_sta_dentro() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);

    // Il file dura due minuti; questo testo finisce a un minuto e
    // cinquanta, cioè comodamente dentro: `verifica_durata` non ha niente
    // da ridire, ed è appunto il caso che non prendeva nessuno.
    let dentro = "[00:10.00]prima riga\n[01:50.00]ultima riga\n";
    connection
        .execute(
            "INSERT INTO lyrics (track_key, synced, source, duration_ms, checked_at, updated_at)
                 VALUES ('chiave', ?1, 'lrclib', 120000, 1, 1)",
            rusqlite::params![dentro],
        )
        .expect("riga della stessa edizione");
    assert_eq!(
        per_brano(&connection, id).expect("testo").aderenza,
        Aderenza::Buona,
        "stessa durata: non c'è niente da segnalare"
    );

    // La stessa riga, ma battuta su un'incisione che dura venticinque
    // secondi in più. I tempi ci stanno dentro lo stesso, e il testo
    // scorrerà sempre più in ritardo: è quel che la colonna `duration_ms`
    // di `011_testi.sql` prometteva di far notare.
    connection
        .execute(
            "UPDATE lyrics SET duration_ms = 145000 WHERE track_key = 'chiave'",
            [],
        )
        .expect("altra edizione");
    assert_eq!(
        per_brano(&connection, id).expect("testo").aderenza,
        Aderenza::Sospetta
    );

    // Mezzo secondo di differenza invece no: è quel che separa due
    // codifiche dello stesso master, e segnalarlo tingerebbe di giallo
    // mezza libreria.
    connection
        .execute(
            "UPDATE lyrics SET duration_ms = 120500 WHERE track_key = 'chiave'",
            [],
        )
        .expect("stessa incisione");
    assert_eq!(
        per_brano(&connection, id).expect("testo").aderenza,
        Aderenza::Buona
    );

    // E una riga vecchia, scritta prima che qualcuno leggesse quella
    // colonna, non diventa sospetta per il fatto di non saperlo.
    connection
        .execute(
            "UPDATE lyrics SET duration_ms = NULL WHERE track_key = 'chiave'",
            [],
        )
        .expect("durata ignota");
    assert_eq!(
        per_brano(&connection, id).expect("testo").aderenza,
        Aderenza::Buona,
        "«non si sa» non è «è sbagliato»"
    );
}

#[test]
fn il_titolo_si_normalizza_prima_di_chiedere() {
    let (dir, connection) = libreria();
    let percorso = dir.path().join("brano.mp3");
    std::fs::write(&percorso, b"finto").expect("file");
    let id = inserisci(&connection, &percorso, None);
    let mut quale = super::brano(&connection, id).expect("brano");

    quale.title = "Poetica (Official Video)".to_owned();
    let domanda = da_chiedere_al_catalogo(&quale);
    assert_eq!(
        domanda.titolo, "Poetica",
        "il rumore del caricamento non si cerca"
    );
    assert_eq!(domanda.album.as_deref(), Some("Album"));
    assert_eq!(domanda.durata_ms, Some(120_000));

    // Quel che dice del brano invece resta: cercare la versione dal vivo
    // di un file dal vivo è la cosa giusta.
    quale.title = "Poetica (Live)".to_owned();
    assert_eq!(da_chiedere_al_catalogo(&quale).titolo, "Poetica (Live)");

    // Un titolo fatto di solo rumore torna com'era: cercare la stringa
    // vuota non è cercare.
    quale.title = "(Official Video)".to_owned();
    assert_eq!(da_chiedere_al_catalogo(&quale).titolo, "(Official Video)");

    // Un album vuoto non diventa una stringa vuota da mandare: diventa
    // «non c'è», ed è `lrclib::url_esatta` a saltarlo.
    quale.album = "   ".to_owned();
    assert_eq!(da_chiedere_al_catalogo(&quale).album, None);
}

#[test]
fn un_sylt_diventa_un_lrc() {
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{FrameFlags, SyncTextContentType, SynchronizedTextFrame, TimestampFormat};

    // Il caso comune: una riga intera per voce, nessun a capo in testa.
    let per_righe = SynchronizedTextFrame::new(
        lofty::TextEncoding::UTF8,
        *b"ita",
        TimestampFormat::MS,
        SyncTextContentType::Lyrics,
        None,
        vec![
            (1_000, "prima riga".to_owned()),
            (2_000, "seconda".to_owned()),
        ],
    );
    // Si passa dai byte, che è la forma in cui il frame arriva davvero:
    // in lofty `SYLT` non entra nel `Tag` unificato, sta in un
    // `Frame::Binary` e va interpretato a mano.
    let byte = per_righe.as_bytes(WriteOptions::default()).expect("byte");
    let riletto = SynchronizedTextFrame::parse(&byte, FrameFlags::default()).expect("frame");
    assert_eq!(
        lrc_da_sylt(&riletto).as_deref(),
        Some("[00:01.00]prima riga\n[00:02.00]seconda\n")
    );

    // Il caso dello standard: sillabe, e l'a capo che dice dove comincia
    // una riga. Le sillabe di una riga si ricompongono in una riga sola,
    // col tempo della prima.
    let sillabato = SynchronizedTextFrame::new(
        lofty::TextEncoding::UTF8,
        *b"eng",
        TimestampFormat::MS,
        SyncTextContentType::Lyrics,
        None,
        vec![
            (1_000, "\nHel".to_owned()),
            (1_200, "lo".to_owned()),
            (2_000, "\nworld".to_owned()),
        ],
    );
    assert_eq!(
        lrc_da_sylt(&sillabato).as_deref(),
        Some("[00:01.00]Hello\n[00:02.00]world\n")
    );

    // I tempi in fotogrammi MPEG non si convertono a occhio: senza la
    // durata del fotogramma sarebbe un testo che scorre a una velocità
    // inventata, e nessun testo è meglio di un testo storto.
    let a_fotogrammi = SynchronizedTextFrame::new(
        lofty::TextEncoding::UTF8,
        *b"eng",
        TimestampFormat::MPEG,
        SyncTextContentType::Lyrics,
        None,
        vec![(1, "prima".to_owned())],
    );
    assert_eq!(lrc_da_sylt(&a_fotogrammi), None);

    // E un `SYLT` che porta accordi non è il testo della canzone.
    let accordi = SynchronizedTextFrame::new(
        lofty::TextEncoding::UTF8,
        *b"eng",
        TimestampFormat::MS,
        SyncTextContentType::Chord,
        None,
        vec![(1_000, "Bb F Fsus".to_owned())],
    );
    assert_eq!(lrc_da_sylt(&accordi), None);
}

#[test]
fn un_syncedlyrics_di_vorbis_si_legge() {
    let mut tag = lofty::ogg::tag::VorbisComments::default();
    tag.push(CHIAVE_SINCRONIZZATA.to_owned(), LRC.to_owned());
    assert_eq!(da_vorbis(Some(&tag)).as_deref(), Some(LRC));

    // Un tag che quella chiave non ce l'ha è una fonte in meno, non un
    // errore — e nemmeno una stringa vuota lo è.
    let vuoto = lofty::ogg::tag::VorbisComments::default();
    assert_eq!(da_vorbis(Some(&vuoto)), None);
    assert_eq!(da_vorbis(None), None);
    let mut spazi = lofty::ogg::tag::VorbisComments::default();
    spazi.push(CHIAVE_SINCRONIZZATA.to_owned(), "  \n ".to_owned());
    assert_eq!(da_vorbis(Some(&spazi)), None);
}

#[test]
fn un_file_che_non_e_musica_non_ha_tag_sincronizzati() {
    // La catena delle fonti chiama `tag_sincronizzato` su ogni brano senza
    // tempi, compresi quelli il cui file è sparito o illeggibile: deve
    // rispondere «niente» invece di far cadere la lettura.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let finto = dir.path().join("brano.mp3");
    std::fs::write(&finto, b"non e' un mp3").expect("file");
    assert_eq!(tag_sincronizzato(&finto), None);
    assert_eq!(tag_sincronizzato(&dir.path().join("non c'e'.mp3")), None);
}

#[test]
fn dimenticare_l_esito_rimette_il_brano_in_coda() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    inserisci(&connection, &brano, None);
    connection
        .execute(
            "INSERT INTO lyrics (track_key, source, checked_at, updated_at)
                 VALUES ('chiave', 'lrclib', 1000, 1000)",
            [],
        )
        .expect("un «non ce l'ho» ricordato");
    assert!(
        da_cercare(&connection, 1_000, 10).expect("coda").is_empty(),
        "gliel'abbiamo già chiesto"
    );

    dimentica_esito(&connection, "chiave").expect("dimenticato");
    assert_eq!(
        da_cercare(&connection, 1_000, 10).expect("coda").len(),
        1,
        "e adesso si può richiedere senza aspettare quattordici giorni"
    );
}

#[test]
fn un_brano_senza_testo_non_ha_niente_da_restituire() {
    let (dir, connection) = libreria();
    let brano = dir.path().join("brano.mp3");
    std::fs::write(&brano, b"finto").expect("file");
    let id = inserisci(&connection, &brano, None);

    let errore = da_restituire(&connection, id).expect_err("non c'è niente");
    assert_eq!(errore.code().kind().code(), "metadata.lyricsPublishRefused");
}

// ── le guardie: nessuna riga di testo per un id ≤ 0 ──

/// Un brano come lo costruirebbe chi ha in mano una voce di sessione: un
/// id ≤ 0 e una `track_key` qualunque. `lyrics` è chiavata su quella, senza
/// chiave esterna, quindi senza la guardia ogni scrittura qui sotto
/// creerebbe una riga — la prova cade se cade la guardia.
fn voce_di_sessione(id: i64, percorso: &Path) -> BranoDaTestare {
    BranoDaTestare {
        id,
        path: Some(percorso.display().to_string()),
        track_key: "chiave-di-sessione".to_owned(),
        title: "Titolo".to_owned(),
        artist: "Artista".to_owned(),
        album: "Album".to_owned(),
        duration_ms: 120_000,
        lyrics: None,
    }
}

#[test]
fn una_voce_di_sessione_non_scrive_testi_nella_libreria() {
    let (dir, connection) = libreria();
    let percorso = dir.path().join("di-un-altro.mp3");
    std::fs::write(&percorso, b"finto").expect("file");
    std::fs::write(percorso.with_extension("lrc"), LRC).expect("sidecar");

    for id in [0, -4] {
        let brano = voce_di_sessione(id, &percorso);
        let rifiuti = [
            ricorda_esito(&connection, &brano, None),
            ricorda_esito(&connection, &brano, Some(&voce_con_i_tempi())),
            ricorda_scelta(&connection, &brano, &voce_con_i_tempi()),
            rimanda(&connection, &brano),
            registra_a_mano(&connection, &brano, LRC),
            salva_a_mano(&connection, &brano, "[00:05.00]sopra il sidecar\n"),
        ];
        for esito in rifiuti {
            assert_eq!(
                esito.map_err(|err| err.code().kind().code()),
                Err("library.trackNotFound"),
                "id {id}"
            );
        }

        // La lettura invece funziona: il testo accanto al file si mostra,
        // solo non si ricorda.
        let letto = per_questo_brano(&connection, &brano);
        assert_eq!(letto.fonte, Fonte::Sidecar, "id {id}");
        assert!(
            leggi_riga(&connection, "chiave-di-sessione").is_none(),
            "id {id}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(percorso.with_extension("lrc")).expect("sidecar"),
        LRC,
        "e il sidecar non si è toccato"
    );

    // La stessa lettura con una riga vera la ricorda: è la prova che la
    // guardia, e non altro, ha tenuto vuota la tabella.
    let vero = voce_di_sessione(1, &percorso);
    let _ = per_questo_brano(&connection, &vero);
    assert!(leggi_riga(&connection, "chiave-di-sessione").is_some());
}
