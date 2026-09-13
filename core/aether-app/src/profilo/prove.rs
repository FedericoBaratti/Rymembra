//! Le prove del profilo.
//!
//! In un file suo e non in fondo a `mod.rs`: il modulo è già lungo, e queste
//! prove costruiscono librerie finte, archivi veri e cartelle temporanee — cioè
//! hanno un'impalcatura loro, che in mezzo alle definizioni si legge peggio.
//!
//! Due famiglie, e vale la pena distinguerle perché servono a cose diverse.
//! Le prime sono le **prove delle omissioni**: verificano che una chiave
//! *non* esca. Sono l'unica cosa che rende un elenco di inclusioni
//! controllabile — senza, un'omissione deliberata e una dimenticanza sono
//! indistinguibili. Le seconde sono le prove della fusione, e verificano che
//! importare aggiunga senza togliere.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::covers::CoverStore;

/// Una libreria vuota in memoria.
fn libreria() -> Connection {
    crate::db::open_in_memory().expect("database").connection
}

fn c_e_tutto(_: &str) -> bool {
    true
}

fn non_c_e_niente(_: &str) -> bool {
    false
}

/// Un identificativo fisso: le prove non hanno bisogno di casualità, e con uno
/// fisso il confronto fra due librerie è quello che si vuole provare.
fn identita_fissa() -> Result<String, AppError> {
    Ok("libreria-di-prova".to_owned())
}

/// Un identificativo diverso, per la prova dell'altra libreria.
fn identita_altrui() -> Result<String, AppError> {
    Ok("un-altra-libreria".to_owned())
}

/// Un brano, con quel poco che le colonne obbligatorie pretendono.
fn brano(connection: &Connection, id: i64, percorso: &str, chiave: &str) {
    connection
        .execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album,
                                 duration_ms, file_size, date_added, date_modified)
             VALUES (?1, ?2, ?3, 'Titolo', 'Artista', 'Album', 200000, 1000, 0, 0)",
            rusqlite::params![id, percorso, chiave],
        )
        .expect("brano");
}

/// L'ambiente di una prova: nessuna rimappatura, tutti i percorsi esistono.
fn ambiente<'a>(esiste: &'a dyn Fn(&str) -> bool) -> Ambiente<'a> {
    Ambiente {
        esiste,
        adesso_ms: 1_700_000_000_000,
        dispositivo: "qui",
        rimappature: &[],
        copertine: None,
    }
}

/// Esporta la libreria in un archivio, e restituisce dove l'ha messo.
fn esporta(
    connection: &mut Connection,
    dove: &Path,
    dispositivo: &str,
    identita_di: &dyn Fn() -> Result<String, AppError>,
) -> PathBuf {
    let cartella = dove.join("store");
    let store = CoverStore::open(&cartella).expect("store");
    let raccolto = raccogli(
        connection,
        dispositivo,
        BTreeMap::new(),
        BTreeMap::new(),
        1_700_000_000_000,
        identita_di,
    )
    .expect("raccolta");
    let destinazione = dove.join(format!("profilo.{}", archivio::ESTENSIONE));
    scrivi(
        &raccolto,
        &Sorgenti {
            copertine: &store,
            skin: BTreeMap::new(),
            bozze: BTreeMap::new(),
        },
        &destinazione,
        &|_| {},
    )
    .expect("scrittura");
    destinazione
}

// ── le omissioni: quel che non deve uscire ──────────────────────────────────

#[test]
fn l_identificativo_del_dispositivo_non_esce() {
    let mut c = libreria();
    settings::write(&c, "nuvola.dispositivo", "questo-computer").expect("scrittura");
    settings::write(&c, "player.volume", "0.8").expect("scrittura");

    let raccolto = raccogli(
        &mut c,
        "qui",
        BTreeMap::new(),
        BTreeMap::new(),
        0,
        &identita_fissa,
    )
    .expect("raccolta");
    assert!(
        !raccolto.preferenze.voci.contains_key("nuvola.dispositivo"),
        "due macchine con lo stesso identificativo si sovrascrivono nel backup, senza errori"
    );
    assert!(raccolto.preferenze.voci.contains_key("player.volume"));
    assert!(
        raccolto.lasciate.contains(&"nuvola.dispositivo".to_owned()),
        "e quel che resta qui va detto, o l'elenco di inclusioni sarebbe silenzioso quanto l'altro"
    );
}

#[test]
fn le_chiavi_di_questa_macchina_restano_qui() {
    // Cinque omissioni deliberate in una prova sola, ognuna con la sua
    // ragione, perché senza di questa sarebbero indistinguibili da cinque
    // dimenticanze — che è il difetto che un elenco di inclusioni si porta
    // dietro.
    //
    // - `player.output` è il nome di una scheda audio: là non nomina niente.
    // - `player.spectrum.quality` descrive questa scheda video, e il danno
    //   sarebbe **muto**: la scena continua a disegnarsi, solo peggio.
    // - `audio.latenza_ms` è il ritardo di questa catena audio: portarlo
    //   sposterebbe i testi di un decimo di secondo dalla parte sbagliata.
    // - `player.queue` sono identificativi di righe: su un'altra libreria
    //   nominano canzoni diverse.
    // - `ui.zoom` è la correzione che resta dopo la scalatura di Windows su
    //   **questo** monitor: cambiato il monitor o la scalatura, il gradino
    //   giusto è un altro. Qui il danno si vede — e `Ctrl+0` lo annulla — ma
    //   l'omissione è deliberata come le altre quattro.
    let mut c = libreria();
    for (chiave, valore) in [
        ("player.output", r#""FiiO K11""#),
        ("player.spectrum.quality", r#""alta""#),
        (crate::playback::CHIAVE_LATENZA, "-120"),
        ("player.queue", r#"{"tracks":[1,2,3]}"#),
        (crate::preferenze::CHIAVE_ZOOM, "1.5"),
    ] {
        settings::write(&c, chiave, valore).expect("scrittura");
    }

    let raccolto = raccogli(
        &mut c,
        "qui",
        BTreeMap::new(),
        BTreeMap::new(),
        0,
        &identita_fissa,
    )
    .expect("raccolta");
    for chiave in [
        "player.output",
        "player.spectrum.quality",
        crate::playback::CHIAVE_LATENZA,
        "player.queue",
        crate::preferenze::CHIAVE_ZOOM,
    ] {
        assert!(
            !raccolto.preferenze.voci.contains_key(chiave),
            "«{chiave}» è un fatto di questa macchina e non deve viaggiare"
        );
        assert!(
            raccolto.lasciate.contains(&chiave.to_owned()),
            "e va detto che è rimasta qui: «{chiave}»"
        );
    }
}

#[test]
fn le_cartelle_aperte_non_escono() {
    // La prova è scritta sul **prefisso** e non sui due nomi, e la differenza
    // non è cosmetica: un confronto sui nomi passa anche se la chiave viene
    // scritta storta, e soprattutto non dice niente sulla terza chiave che un
    // giorno qualcuno aggiungerà al pannello.
    for (chiave, _) in CATALOGO {
        assert!(
            !chiave.starts_with("ui.folders."),
            "«{chiave}» è un percorso di questa macchina e non deve viaggiare"
        );
    }
}

#[test]
fn la_chiave_di_lastfm_non_esce_dal_profilo() {
    // Non è un segreto in senso stretto — viaggia in chiaro nell'indirizzo del
    // consenso — ma è una credenziale **personale**, e un profilo è fatto per
    // essere passato in giro. Dalla 2.3.1 sta nel portachiavi; questa prova
    // tiene chiusa la porta anche per il caso in cui il travaso non sia
    // riuscito e la chiave sia rimasta in `settings`.
    for (chiave, _) in CATALOGO {
        assert!(
            !chiave.contains("api_key"),
            "«{chiave}» è una credenziale personale e non deve stare in un file che si passa in giro"
        );
    }

    let mut c = libreria();
    settings::write(&c, crate::scrobble::CHIAVE_LFM_API_KEY, "0123456789abcdef")
        .expect("scrittura");
    let raccolto = raccogli(
        &mut c,
        "qui",
        BTreeMap::new(),
        BTreeMap::new(),
        0,
        &identita_fissa,
    )
    .expect("raccolta");
    assert!(
        !raccolto
            .preferenze
            .voci
            .values()
            .any(|valore| valore.contains("0123456789abcdef")),
        "la chiave dell'applicazione non entra nel profilo"
    );
    assert!(
        raccolto
            .lasciate
            .contains(&crate::scrobble::CHIAVE_LFM_API_KEY.to_owned())
    );
}

#[test]
fn quel_che_viaggia_viaggia_davvero() {
    // L'altra metà del controllo: un elenco di inclusioni si sbaglia in due
    // versi, e questa prova guarda quello in cui una chiave che *doveva*
    // uscire non esce.
    let mut c = libreria();
    for (chiave, valore) in [
        ("player.spectrum.bands", "256"),
        ("player.spectrum.visible", "true"),
        ("player.fileFormat.visible", "false"),
        (crate::preferenze::CHIAVE_MOVIMENTO_RIDOTTO, "1"),
    ] {
        settings::write(&c, chiave, valore).expect("scrittura");
    }
    let raccolto = raccogli(
        &mut c,
        "qui",
        BTreeMap::new(),
        BTreeMap::new(),
        0,
        &identita_fissa,
    )
    .expect("raccolta");
    for chiave in [
        "player.spectrum.bands",
        "player.spectrum.visible",
        "player.fileFormat.visible",
        crate::preferenze::CHIAVE_MOVIMENTO_RIDOTTO,
    ] {
        assert!(
            raccolto.preferenze.voci.contains_key(chiave),
            "«{chiave}» è un gusto di chi ascolta, e deve viaggiare"
        );
    }
    assert!(
        raccolto
            .lasciate
            .iter()
            .all(|chiave| chiave == CHIAVE_IDENTITA),
        "e l'unica cosa rimasta qui è l'identità della libreria: {:?}",
        raccolto.lasciate
    );
}

// ── il formato ──────────────────────────────────────────────────────────────

#[test]
fn un_profilo_vecchio_si_legge_ancora() {
    // Un `.json` scritto dalla 2.2.0, su una chiavetta da prima
    // dell'aggiornamento. La cosa peggiore che una versione nuova possa fare a
    // un file vecchio è non aprirlo.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let vecchio = dir.path().join("aether-profilo.json");
    std::fs::write(
        &vecchio,
        r#"{"aether":"aether.profilo","versione":1,"creato_ms":1700000000000,
            "voci":{"player.volume":"0.42","skin.active":"sala"}}"#,
    )
    .expect("v1");

    let letto = leggi(&vecchio).expect("lettura");
    assert_eq!(letto.manifesto.versione, 1);
    assert!(!letto.ha_libreria(), "un v1 non porta libreria");

    let mut c = libreria();
    let piano = importa(&mut c, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(piano.cambi.len(), 2);
    assert_eq!(piano.versione, 1);
    assert!(
        !piano.identita_diversa,
        "un profilo che porta solo preferenze le porta a chiunque"
    );
    assert_eq!(
        settings::read(&c, "skin.active"),
        Ok(Some("sala".to_owned()))
    );
}

#[test]
fn un_json_qualunque_non_e_un_profilo() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let finto = dir.path().join("finto.json");
    std::fs::write(&finto, r#"{"tema":"chiaro"}"#).expect("scrittura");
    let err = leggi(&finto).expect_err("deve rifiutare");
    assert_eq!(err.code().kind().code(), "settings.corrupt");

    let altro = dir.path().join("altro.json");
    std::fs::write(
        &altro,
        r#"{"aether":"altro","versione":1,"creato_ms":0,"voci":{}}"#,
    )
    .expect("scrittura");
    assert!(leggi(&altro).is_err());
}

#[test]
fn quel_che_esce_rientra_uguale() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut sorgente = libreria();
    settings::write(&sorgente, "player.volume", "0.42").expect("scrittura");
    settings::write(&sorgente, "skin.active", "sala").expect("scrittura");
    settings::write(&sorgente, crate::preferenze::CHIAVE_TEMA, "chiaro").expect("scrittura");
    let archivio = esporta(&mut sorgente, dir.path(), "la", &identita_fissa);

    let letto = leggi(&archivio).expect("lettura");
    assert_eq!(letto.manifesto.versione, 2);
    assert_eq!(letto.manifesto.abbinamento, "track_key");
    assert!(
        !letto.manifesto.fuori.is_empty(),
        "il manifesto dichiara cosa ha lasciato fuori"
    );

    let mut destinazione = libreria();
    let piano = importa(&mut destinazione, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(piano.cambi.len(), 3);
    assert_eq!(piano.creato_ms, 1_700_000_000_000);
    assert_eq!(
        settings::read(&destinazione, "skin.active"),
        Ok(Some("sala".to_owned()))
    );
}

#[test]
fn reimportare_lo_stesso_archivio_non_cambia_piu_niente() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut sorgente = libreria();
    brano(&sorgente, 1, r"D:\M\a.flac", "artista|titolo|album");
    settings::write(&sorgente, "player.volume", "0.42").expect("scrittura");
    sorgente
        .execute(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 500, 200000)",
            [],
        )
        .expect("cronologia");
    sorgente
        .execute("UPDATE tracks SET play_count = 3 WHERE id = 1", [])
        .expect("ascolti");
    let archivio = esporta(&mut sorgente, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"D:\M\a.flac", "artista|titolo|album");
    let primo = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("prima");
    assert!(!primo.e_vuoto(), "la prima volta porta qualcosa");

    let secondo = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("seconda");
    assert!(
        secondo.e_vuoto(),
        "la seconda non deve cambiare niente: {secondo:?}"
    );
    assert_eq!(
        secondo.portati.cronologia, 0,
        "e soprattutto la cronologia non raddoppia: è l'indice unico della 020"
    );
    let quante: i64 = qui
        .query_row("SELECT COUNT(*) FROM play_history", [], |riga| riga.get(0))
        .expect("conteggio");
    assert_eq!(quante, 1, "una riga, non due");
}

#[test]
fn il_piano_dell_archivio_non_scrive_niente() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut sorgente = libreria();
    brano(&sorgente, 1, r"D:\M\a.flac", "chiave");
    settings::write(&sorgente, "player.volume", "0.9").expect("scrittura");
    sorgente
        .execute(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 500, 200000)",
            [],
        )
        .expect("cronologia");
    sorgente
        .execute(
            "INSERT INTO lyrics (track_key, plain, source, updated_at) VALUES ('chiave', 'la la', 'lrclib', 10)",
            [],
        )
        .expect("testo");
    let archivio = esporta(&mut sorgente, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"D:\M\a.flac", "chiave");
    let piano = piano(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("piano");
    assert_eq!(piano.cambi.len(), 1);
    assert_eq!(piano.portati.cronologia, 1);
    assert_eq!(piano.portati.testi, 1);

    // E niente di tutto ciò è successo davvero.
    assert_eq!(
        settings::read(&qui, "player.volume"),
        Ok(None),
        "il piano guarda e non tocca"
    );
    let ascolti: i64 = qui
        .query_row("SELECT COUNT(*) FROM play_history", [], |riga| riga.get(0))
        .expect("conteggio");
    assert_eq!(ascolti, 0);
    let testi: i64 = qui
        .query_row("SELECT COUNT(*) FROM lyrics", [], |riga| riga.get(0))
        .expect("conteggio");
    assert_eq!(testi, 0);
}

// ── la fusione ──────────────────────────────────────────────────────────────

#[test]
fn importare_un_profilo_non_perde_gli_ascolti_di_qui() {
    // Il caso che dice se la fusione è una fusione o una sostituzione: due
    // macchine hanno sentito lo stesso brano, e il totale deve essere la somma.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut la = libreria();
    brano(&la, 1, r"D:\M\a.flac", "chiave");
    la.execute("UPDATE tracks SET play_count = 7 WHERE id = 1", [])
        .expect("ascolti di là");
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"E:\M\a.flac", "chiave");
    qui.execute("UPDATE tracks SET play_count = 5 WHERE id = 1", [])
        .expect("ascolti di qui");

    importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    let quanti: i64 = qui
        .query_row("SELECT play_count FROM tracks WHERE id = 1", [], |riga| {
            riga.get(0)
        })
        .expect("conteggio");
    assert!(quanti >= 7, "gli ascolti di là sono arrivati: {quanti}");
    assert!(quanti >= 5, "e quelli di qui non si sono persi: {quanti}");
}

#[test]
fn le_playlist_si_fondono_invece_di_sostituirsi() {
    // Il verso che conta: importare un profilo **aggiunge** le playlist che
    // porta e non tocca quelle di qui. Una implementazione ingenua
    // riscriverebbe la tabella, e il sintomo sarebbe una serata di lavoro
    // sparita senza che niente lo dica.
    //
    // Il limite, scritto qui perché non si scopra sul campo: due playlist con
    // lo **stesso nome** e contenuti diversi, mai sincronizzate prima, non si
    // uniscono elemento per elemento. È la contropartita dichiarata di
    // `sincronia::ORIGINE` — due librerie battezzano i propri elementi con lo
    // stesso pseudo-dispositivo, quindi `origine:0` di qua e `origine:0` di là
    // risultano lo stesso elemento e la fusione ne tiene uno. Vale già oggi
    // fra due dispositivi che si sincronizzano, e il profilo lo eredita:
    // cambiarlo vorrebbe dire cambiare il protocollo della sincronia, non
    // questo modulo.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut la = libreria();
    brano(&la, 1, r"D:\M.flac", "a");
    brano(&la, 2, r"D:\M.flac", "b");
    playlist(&la, "Mattina", &[1, 2]);
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"E:\M.flac", "a");
    brano(&qui, 3, r"E:\M\c.flac", "c");
    playlist(&qui, "Serale", &[1, 3]);

    importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");

    let nomi: Vec<String> = qui
        .prepare("SELECT name FROM playlists ORDER BY name")
        .expect("interrogazione")
        .query_map([], |riga| riga.get(0))
        .expect("lettura")
        .collect::<Result<_, _>>()
        .expect("lettura");
    assert_eq!(
        nomi,
        vec!["Mattina".to_owned(), "Serale".to_owned()],
        "quella del profilo è arrivata e quella di qui è rimasta"
    );

    assert_eq!(
        dentro(&qui, "Serale"),
        vec!["a".to_owned(), "c".to_owned()],
        "e la playlist di qui è intatta: un profilo aggiunge, non sostituisce"
    );
    assert_eq!(
        dentro(&qui, "Mattina"),
        vec!["a".to_owned()],
        "di quella del profilo arriva ciò che qui esiste: «b» non è in questa libreria"
    );
}

#[test]
fn un_testo_scritto_a_mano_non_si_sovrascrive() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut la = libreria();
    brano(&la, 1, r"D:\M\a.flac", "chiave");
    la.execute(
        "INSERT INTO lyrics (track_key, plain, source, updated_at)
         VALUES ('chiave', 'quello di là', 'lrclib', 9999)",
        [],
    )
    .expect("testo di là");
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"E:\M\a.flac", "chiave");
    qui.execute(
        "INSERT INTO lyrics (track_key, plain, source, updated_at)
         VALUES ('chiave', 'quello che ho sincronizzato io', 'mano', 1)",
        [],
    )
    .expect("testo di qui");

    importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    let testo: String = qui
        .query_row(
            "SELECT plain FROM lyrics WHERE track_key = 'chiave'",
            [],
            |riga| riga.get(0),
        )
        .expect("rilettura");
    assert_eq!(
        testo, "quello che ho sincronizzato io",
        "un testo fatto a mano è lavoro, e non lo si perde nemmeno per uno più recente"
    );
}

#[test]
fn un_profilo_con_un_altra_identita_porta_solo_le_preferenze() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut altrui = libreria();
    brano(&altrui, 1, r"D:\M\a.flac", "chiave");
    settings::write(&altrui, "skin.active", "sala").expect("scrittura");
    altrui
        .execute(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 500, 200000)",
            [],
        )
        .expect("cronologia");
    let archivio = esporta(&mut altrui, dir.path(), "altrui", &identita_altrui);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"E:\M\a.flac", "chiave");
    // Questa libreria ha già un'identità sua, e non è quella del profilo.
    identita(&qui, &identita_fissa).expect("identità");

    let piano = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert!(piano.identita_diversa, "e lo deve dire");
    assert_eq!(
        settings::read(&qui, "skin.active"),
        Ok(Some("sala".to_owned())),
        "le preferenze passano: non parlano di brani"
    );
    let ascolti: i64 = qui
        .query_row("SELECT COUNT(*) FROM play_history", [], |riga| riga.get(0))
        .expect("conteggio");
    assert_eq!(
        ascolti, 0,
        "la cronologia di un'altra libreria non si attacca ai brani di questa"
    );
    assert_eq!(piano.portati.cronologia, 0);
}

// ── la portabilità ──────────────────────────────────────────────────────────

#[test]
fn le_radici_si_rimappano_e_i_percorsi_seguono() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let musica = dir.path().join("E").join("M");
    std::fs::create_dir_all(&musica).expect("cartella");
    let file = musica.join("a.flac");
    std::fs::write(&file, b"finto").expect("file");

    let mut la = libreria();
    settings::write(&la, "library.roots", r#"["D:\\M"]"#).expect("scrittura");
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"D:\M\a.flac", "chiave");

    // Prima lettura: il piano propone la rimappatura, con quanti brani di qui
    // stanno sotto quel prefisso.
    let ce_solo_il_file = |percorso: &str| Path::new(percorso).exists();
    let proposto = piano(&mut qui, &letto, &ambiente(&ce_solo_il_file)).expect("piano");
    assert_eq!(proposto.rimappature.len(), 1);
    assert_eq!(proposto.rimappature[0].da, r"D:\M");
    assert_eq!(
        proposto.rimappature[0].brani, 1,
        "e dice quanti brani di qui riguarda"
    );

    // Seconda: con la destinazione scelta, i percorsi seguono.
    let scelta = vec![Rimappatura {
        da: r"D:\M".to_owned(),
        a: musica.display().to_string(),
        brani: 1,
    }];
    let fatto = importa(
        &mut qui,
        &letto,
        &Ambiente {
            rimappature: &scelta,
            ..ambiente(&ce_solo_il_file)
        },
    )
    .expect("importazione");
    assert_eq!(fatto.percorsi_riscritti, 1);
    let percorso: String = qui
        .query_row("SELECT path FROM tracks WHERE id = 1", [], |riga| {
            riga.get(0)
        })
        .expect("rilettura");
    assert_eq!(percorso, file.display().to_string());
    let radici: String = settings::read(&qui, "library.roots")
        .expect("lettura")
        .expect("c'è");
    assert!(
        radici.contains(&musica.display().to_string().replace('\\', "\\\\")),
        "e anche la radice segue: {radici}"
    );
}

#[test]
fn un_brano_che_non_c_e_nella_nuova_radice_non_si_riscrive() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut la = libreria();
    settings::write(&la, "library.roots", r#"["D:\\M"]"#).expect("scrittura");
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"D:\M\sparito.flac", "chiave");
    let scelta = vec![Rimappatura {
        da: r"D:\M".to_owned(),
        a: dir.path().join("vuota").display().to_string(),
        brani: 1,
    }];

    let fatto = importa(
        &mut qui,
        &letto,
        &Ambiente {
            rimappature: &scelta,
            ..ambiente(&non_c_e_niente)
        },
    )
    .expect("importazione");
    assert_eq!(fatto.percorsi_riscritti, 0);
    assert_eq!(
        fatto.brani_irrintracciabili, 1,
        "si contano, e la riga resta com'era"
    );
    let percorso: String = qui
        .query_row("SELECT path FROM tracks WHERE id = 1", [], |riga| {
            riga.get(0)
        })
        .expect("rilettura");
    assert_eq!(
        percorso, r"D:\M\sparito.flac",
        "un percorso vecchio dice la verità su dov'era; uno nuovo mentirebbe su dov'è"
    );
}

// ── la copia di sicurezza ───────────────────────────────────────────────────

#[test]
fn la_copia_di_sicurezza_si_scrive_prima() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let dati = dir.path().join("dati");

    // Com'è questo computer prima di toccare niente.
    let mut qui = libreria();
    settings::write(&qui, "skin.active", "mia").expect("scrittura");
    let store = CoverStore::open(dati.join("copertine")).expect("store");
    let raccolto = raccogli(
        &mut qui,
        "qui",
        BTreeMap::new(),
        BTreeMap::new(),
        1,
        &identita_fissa,
    )
    .expect("raccolta");
    let copia = copia_di_sicurezza(&dati, 1);
    scrivi(
        &raccolto,
        &Sorgenti {
            copertine: &store,
            skin: BTreeMap::new(),
            bozze: BTreeMap::new(),
        },
        &copia,
        &|_| {},
    )
    .expect("copia");
    assert!(copia.exists(), "la copia si scrive prima di importare");
    assert_eq!(ultima_copia(&dati).as_deref(), Some(copia.as_path()));

    // Poi arriva il profilo di qualcun altro.
    let mut altro = libreria();
    settings::write(&altro, "skin.active", "sua").expect("scrittura");
    let archivio = esporta(&mut altro, dir.path(), "altro", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");
    importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(
        settings::read(&qui, "skin.active"),
        Ok(Some("sua".to_owned()))
    );

    // E l'annullamento rimette le preferenze, che è tutto quel che può fare.
    annulla(&mut qui, &copia, &ambiente(&c_e_tutto)).expect("annullamento");
    assert_eq!(
        settings::read(&qui, "skin.active"),
        Ok(Some("mia".to_owned()))
    );
}

#[test]
fn le_copie_si_tengono_in_tre() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let dati = dir.path().to_owned();
    std::fs::create_dir_all(dati.join(CARTELLA_COPIE)).expect("cartella");
    for quando in [1_i64, 2, 3, 4, 5] {
        std::fs::write(copia_di_sicurezza(&dati, quando), b"finto").expect("copia");
    }
    assert_eq!(ruota_le_copie(&dati), 2);
    assert!(!copia_di_sicurezza(&dati, 1).exists());
    assert!(copia_di_sicurezza(&dati, 5).exists());
    assert_eq!(
        ultima_copia(&dati).as_deref(),
        Some(copia_di_sicurezza(&dati, 5).as_path())
    );
}

/// Una playlist con dentro dei brani, con il minimo che le colonne pretendono.
fn playlist(connection: &Connection, nome: &str, brani: &[i64]) {
    connection
        .execute(
            "INSERT INTO playlists (name, playlist_key, created_at, updated_at)
             VALUES (?1, ?1, 0, 0)",
            rusqlite::params![nome],
        )
        .expect("playlist");
    let id = connection.last_insert_rowid();
    for (posizione, track_id) in brani.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![id, track_id, i64::try_from(posizione).unwrap_or(0)],
            )
            .expect("membro");
    }
}

/// Le chiavi dei brani dentro una playlist, in ordine.
fn dentro(connection: &Connection, nome: &str) -> Vec<String> {
    connection
        .prepare(
            "SELECT t.track_key FROM playlist_tracks pt
               JOIN tracks t ON t.id = pt.track_id
               JOIN playlists p ON p.id = pt.playlist_id
              WHERE p.name = ?1 ORDER BY t.track_key",
        )
        .expect("interrogazione")
        .query_map([nome], |riga| riga.get(0))
        .expect("lettura")
        .collect::<Result<_, _>>()
        .expect("lettura")
}
