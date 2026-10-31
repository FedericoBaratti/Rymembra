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
use rymembra_domain::eredita;

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
        unisci_comunque: false,
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
    // Sei omissioni deliberate in una prova sola, ognuna con la sua
    // ragione, perché senza di questa sarebbero indistinguibili da sei
    // dimenticanze — che è il difetto che un elenco di inclusioni si porta
    // dietro.
    //
    // - `player.output` è il nome di una scheda audio: là non nomina niente.
    // - `player.spectrum.quality` descrive questa scheda video, e il danno
    //   sarebbe **muto**: la scena continua a disegnarsi, solo peggio.
    // - `audio.latenza_ms` è il ritardo di questa catena audio: portarlo
    //   sposterebbe i testi di un decimo di secondo dalla parte sbagliata.
    // - `audio.latenza_per_uscita` è lo stesso ritardo detto per ogni scheda,
    //   con i nomi delle schede come chiavi: due ragioni invece di una.
    // - `player.queue` sono identificativi di righe: su un'altra libreria
    //   nominano canzoni diverse.
    // - `ui.zoom` è la correzione che resta dopo la scalatura di Windows su
    //   **questo** monitor: cambiato il monitor o la scalatura, il gradino
    //   giusto è un altro. Qui il danno si vede — e `Ctrl+0` lo annulla — ma
    //   l'omissione è deliberata come le altre cinque.
    let mut c = libreria();
    for (chiave, valore) in [
        ("player.output", r#""FiiO K11""#),
        ("player.spectrum.quality", r#""alta""#),
        (crate::playback::CHIAVE_LATENZA, "-120"),
        (
            crate::playback::CHIAVE_LATENZA_PER_USCITA,
            r#"{"FiiO K11":{"ms":-120,"fonte":"microfono","verificata":true,"quando_ms":0}}"#,
        ),
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
        crate::playback::CHIAVE_LATENZA_PER_USCITA,
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
    // E nessun nome di scheda esce per un'altra porta, dentro un valore.
    assert!(
        !raccolto
            .preferenze
            .voci
            .values()
            .any(|valore| valore.contains("FiiO K11")),
        "il nome di un'uscita audio non deve stare in un file che si passa in giro"
    );
}

#[test]
fn niente_di_audio_viaggia_nel_profilo() {
    // Per prefisso, come per le cartelle aperte: `audio.` è lo spazio dei
    // fatti di questa catena — la carta di `CHIAVE_LATENZA` lo dice — e una
    // terza chiave aggiunta un giorno lì sotto deve restare qui senza che
    // qualcuno si ricordi di questa prova.
    for (chiave, _) in CATALOGO {
        assert!(
            !chiave.starts_with("audio."),
            "«{chiave}» descrive l'uscita audio di questa macchina e non deve viaggiare"
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
fn niente_di_insieme_viaggia_nel_profilo() {
    // Per prefisso, come `audio.`: le chiavi di «Ascolta insieme» sono fatti di
    // questa macchina e di questa rete, e una quinta aggiunta un giorno deve
    // restare qui senza che qualcuno si ricordi di questa prova.
    for (chiave, _) in CATALOGO {
        assert!(
            !chiave.starts_with("insieme."),
            "«{chiave}» è di «Ascolta insieme» e non deve viaggiare"
        );
    }
}

/// Le cose di «Ascolta insieme» che non devono uscire, in una libreria: un PC
/// associato e le quattro impostazioni. Restituisce le stringhe da cercare.
fn libreria_che_ascolta_insieme(connection: &Connection) -> Vec<String> {
    let chiave = "5e".repeat(32);
    let nome_del_pari = "Portatile-di-Zaira-7Q";
    let nome_di_qui = "Fisso-in-mansarda-K2";
    let indirizzo = "10.20.30.40:47812";
    crate::insieme::associa(connection, &chiave, nome_del_pari, 1_000).expect("associato");
    crate::insieme::salva_attivo(connection, true).expect("acceso");
    crate::insieme::salva_nome(connection, nome_di_qui).expect("nome");
    crate::insieme::salva_indirizzi(connection, &[indirizzo.to_owned()]).expect("indirizzi");
    crate::insieme::segna_firewall_spiegato(connection).expect("firewall");
    vec![
        chiave,
        nome_del_pari.to_owned(),
        nome_di_qui.to_owned(),
        indirizzo.to_owned(),
    ]
}

/// Nessuna delle stringhe sta in `testo`.
fn non_contiene(dove: &str, testo: &str, segreti: &[String]) {
    for segreto in segreti {
        assert!(
            !testo.contains(segreto.as_str()),
            "«{segreto}» è uscito con il profilo, in {dove}"
        );
    }
}

#[test]
fn i_pc_associati_e_le_chiavi_di_insieme_restano_qui() {
    let mut c = libreria();
    brano(&c, 1, r"D:\M\a.flac", "artista|titolo|album");
    let segreti = libreria_che_ascolta_insieme(&c);

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
        crate::insieme::CHIAVE_ATTIVO,
        crate::insieme::CHIAVE_NOME,
        crate::insieme::CHIAVE_INDIRIZZI,
        crate::insieme::CHIAVE_FIREWALL,
    ] {
        assert!(!raccolto.preferenze.voci.contains_key(chiave), "{chiave}");
        assert!(
            raccolto.lasciate.contains(&chiave.to_owned()),
            "e va detto che è rimasta qui: «{chiave}»"
        );
    }

    // Poi l'archivio vero, letto come lo legge l'importazione. Si guardano i
    // documenti **decodificati**: il documento di sincronia viaggia compresso,
    // e cercare un nome dentro byte compressi passerebbe sempre.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let archivio = esporta(&mut c, dir.path(), "qui", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");
    non_contiene("preferenze", &in_json(&letto.preferenze), &segreti);
    non_contiene("biblioteca", &in_json(&letto.biblioteca), &segreti);
    non_contiene("manifesto", &in_json(&letto.manifesto), &segreti);
    let sincronia = letto.sincronia.as_ref().expect("il documento di sincronia");
    non_contiene("sincronia", &in_json(sincronia), &segreti);

    // E ogni voce dell'archivio, scompattata: anche quella che domani qualcuno
    // aggiungerà senza passare da uno dei quattro documenti qui sopra.
    let file = std::fs::File::open(&archivio).expect("archivio");
    let mut zip = zip::ZipArchive::new(file).expect("zip");
    for posto in 0..zip.len() {
        let mut voce = zip.by_index(posto).expect("voce");
        let nome = voce.name().to_owned();
        let mut byte = Vec::new();
        std::io::Read::read_to_end(&mut voce, &mut byte).expect("scompattata");
        non_contiene(&nome, &String::from_utf8_lossy(&byte), &segreti);
    }
}

#[test]
fn lo_specchio_del_cloud_resta_qui() {
    // Lo specchio è una cache (migrazione 036): la biblioteca porta solo le
    // tabelle che nomina, e questa prova lo tiene vero voce per voce
    // dell'archivio, anche quella che domani qualcuno aggiungerà.
    let mut c = libreria();
    brano(&c, 1, r"D:\M\a.flac", "artista|titolo|album");
    let segreti = crate::nuvola::specchio_da_non_portare(&c);
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let archivio = esporta(&mut c, dir.path(), "qui", &identita_fissa);
    let file = std::fs::File::open(&archivio).expect("archivio");
    let mut zip = zip::ZipArchive::new(file).expect("zip");
    for posto in 0..zip.len() {
        let mut voce = zip.by_index(posto).expect("voce");
        let nome = voce.name().to_owned();
        let mut byte = Vec::new();
        std::io::Read::read_to_end(&mut voce, &mut byte).expect("scompattata");
        // Il documento di sincronia viaggia compresso: si guarda decodificato.
        let testo = String::from_utf8_lossy(&byte).into_owned();
        non_contiene(&nome, &testo, &segreti);
    }
    let letto = leggi(&archivio).expect("lettura");
    non_contiene("preferenze", &in_json(&letto.preferenze), &segreti);
    non_contiene("biblioteca", &in_json(&letto.biblioteca), &segreti);
    let sincronia = letto.sincronia.as_ref().expect("il documento di sincronia");
    non_contiene("sincronia", &in_json(sincronia), &segreti);
}

#[test]
fn un_profilo_scritto_a_mano_non_accende_insieme() {
    // Il guasto da tenere chiuso: un profilo passato di mano in mano che,
    // importato, apre una porta in ascolto su un PC che non l'ha chiesto e lo
    // fa cercare indirizzi di un'altra rete. Un profilo si scrive anche a mano,
    // quindi non basta che l'esportazione non le scriva: l'importazione deve
    // non leggerle.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let scritto = dir.path().join("regalo.json");
    std::fs::write(
        &scritto,
        r#"{"rymembra":"rymembra.profilo","versione":1,"creato_ms":1700000000000,
            "voci":{"player.volume":"0.42",
                    "insieme.attivo":"true",
                    "insieme.nome":"\"Ufficio\"",
                    "insieme.indirizzi":"[\"6.6.6.6:47811\"]",
                    "insieme.firewall_spiegato":"true"}}"#,
    )
    .expect("profilo a mano");

    let letto = leggi(&scritto).expect("lettura");
    let mut c = libreria();
    let piano = importa(&mut c, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    for chiave in [
        crate::insieme::CHIAVE_ATTIVO,
        crate::insieme::CHIAVE_NOME,
        crate::insieme::CHIAVE_INDIRIZZI,
        crate::insieme::CHIAVE_FIREWALL,
    ] {
        assert!(
            piano.sconosciute.contains(&chiave.to_owned()),
            "«{chiave}» va detta sconosciuta: {:?}",
            piano.sconosciute
        );
        assert_eq!(settings::read(&c, chiave), Ok(None), "{chiave}");
    }
    assert_eq!(
        crate::insieme::impostazioni(&c).expect("lette"),
        crate::insieme::Impostazioni::default()
    );
    // Il resto del profilo entra lo stesso: le chiavi sconosciute si saltano,
    // non fanno fallire l'importazione.
    assert_eq!(
        settings::read(&c, "player.volume"),
        Ok(Some("0.42".to_owned()))
    );
}

/// Un documento qualunque in JSON, per cercarci dentro.
fn in_json<T: serde::Serialize>(valore: &T) -> String {
    serde_json::to_string(valore).expect("in JSON")
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
    let vecchio = dir.path().join("rymembra-profilo.json");
    std::fs::write(
        &vecchio,
        r#"{"rymembra":"rymembra.profilo","versione":1,"creato_ms":1700000000000,
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
        r#"{"rymembra":"altro","versione":1,"creato_ms":0,"voci":{}}"#,
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
fn un_profilo_porta_anche_gli_ascolti_degli_altri_dispositivi() {
    // Il portatile sincronizzato da un anno col telefono: dei cinque ascolti,
    // due sono suoi e tre del telefono. Il documento di sincronia porta solo
    // quelli di chi esporta, la cronologia solo le righe suonate qui: sul
    // computer nuovo i tre del telefono sparivano, in silenzio.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut la = libreria();
    brano(&la, 1, r"D:\M\a.flac", "k");
    la.execute_batch(
        "INSERT INTO sync_ascolti (track_key, dispositivo, quanti)
         VALUES ('k', 'la', 2), ('k', 'telefono', 3);
         UPDATE tracks SET play_count = 5 WHERE id = 1;",
    )
    .expect("ascolti");
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"E:\M\a.flac", "k");
    for _ in 0..2 {
        importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
        let (conteggio, del_telefono): (i64, i64) = qui
            .query_row(
                "SELECT t.play_count, a.quanti FROM tracks t
                   JOIN sync_ascolti a ON a.track_key = t.track_key AND a.dispositivo = 'telefono'
                  WHERE t.id = 1",
                [],
                |riga| Ok((riga.get(0)?, riga.get(1)?)),
            )
            .expect("conteggio");
        assert_eq!(
            conteggio, 5,
            "due di là più tre del telefono, anche rifacendo"
        );
        assert_eq!(
            del_telefono, 3,
            "e restano del telefono, non dell'importazione"
        );
    }
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
fn un_brano_corretto_ritrova_tutto_su_un_computer_nuovo() {
    // Il caso per cui un profilo esiste: là il brano è stato corretto a mano, e
    // tutto quel che lo riguarda — cronologia, testo, cuore — sta sotto la
    // chiave **corretta**. Qui lo stesso file è appena stato scansionato, con
    // la chiave dei suoi tag. La correzione si cercava per chiave corretta e
    // non trovava niente; il resto, applicato prima, nemmeno.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let dei_tag = "unknown artist|traccia 01|al";
    let corretta = "pink floyd|hey you|al";
    let campi = serde_json::to_string(&crate::provenienza::Correzioni {
        titolo: Some("Hey You".to_owned()),
        artista: Some("Pink Floyd".to_owned()),
        ..crate::provenienza::Correzioni::default()
    })
    .expect("correzione");

    let mut la = libreria();
    brano(&la, 1, r"D:\M\01.flac", corretta);
    la.execute(
        "UPDATE tracks SET title = 'Hey You', artist = 'Pink Floyd', album = 'Al',
                content_key = ?1, liked = 1, liked_at = 700
          WHERE id = 1",
        [dei_tag],
    )
    .expect("il brano corretto");
    la.execute(
        "INSERT INTO track_overrides (track_id, campi, set_at) VALUES (1, ?1, 600)",
        [&campi],
    )
    .expect("la correzione");
    la.execute(
        "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 500, 200000)",
        [],
    )
    .expect("cronologia");
    la.execute(
        "INSERT INTO lyrics (track_key, synced, source, updated_at)
         VALUES (?1, '[00:01.00]Out there in the cold', 'mano', 800)",
        [corretta],
    )
    .expect("testo a mano");
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    brano(&qui, 1, r"E:\M\01.flac", dei_tag);
    qui.execute(
        "UPDATE tracks SET title = 'Traccia 01', artist = 'Unknown Artist', album = 'Al',
                content_key = ?1
          WHERE id = 1",
        [dei_tag],
    )
    .expect("il brano coi suoi tag");

    let piano = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(piano.portati.correzioni, 1, "{piano:?}");

    let (titolo, chiave, cuore): (String, String, i64) = qui
        .query_row(
            "SELECT title, track_key, liked FROM tracks WHERE id = 1",
            [],
            |riga| Ok((riga.get(0)?, riga.get(1)?, riga.get(2)?)),
        )
        .expect("rilettura");
    assert_eq!(titolo, "Hey You", "la correzione è arrivata");
    assert_eq!(chiave, corretta);
    assert_eq!(cuore, 1, "e il cuore l'ha seguita");
    let ascolti: i64 = qui
        .query_row(
            "SELECT COUNT(*) FROM play_history WHERE track_id = 1",
            [],
            |riga| riga.get(0),
        )
        .expect("conteggio");
    assert_eq!(ascolti, 1, "la cronologia pure");
    let testo: Option<String> = qui
        .query_row(
            "SELECT synced FROM lyrics WHERE track_key = ?1",
            [corretta],
            |riga| riga.get(0),
        )
        .expect("il testo");
    assert!(testo.is_some());
}

/// Una libreria con un brano dei tag «Artista|Titolo|Album» corretto a mano in
/// `titolo`, alla data `set_at`.
fn libreria_corretta(titolo: &str, set_at: i64) -> Connection {
    let dei_tag = "artista|titolo|album";
    let corretta = format!("artista|{}|album", titolo.to_lowercase());
    let campi = serde_json::to_string(&crate::provenienza::Correzioni {
        titolo: Some(titolo.to_owned()),
        ..crate::provenienza::Correzioni::default()
    })
    .expect("correzione");
    let c = libreria();
    brano(&c, 1, r"D:\M\01.flac", &corretta);
    c.execute(
        "UPDATE tracks SET title = ?1, content_key = ?2 WHERE id = 1",
        rusqlite::params![titolo, dei_tag],
    )
    .expect("il brano corretto");
    c.execute(
        "INSERT INTO track_overrides (track_id, campi, set_at) VALUES (1, ?1, ?2)",
        rusqlite::params![campi, set_at],
    )
    .expect("la correzione");
    c
}

fn titolo_e_correzione(c: &Connection) -> (String, String, i64) {
    c.query_row(
        "SELECT t.title, o.campi, o.set_at
           FROM tracks t JOIN track_overrides o ON o.track_id = t.id
          WHERE t.id = 1",
        [],
        |riga| Ok((riga.get(0)?, riga.get(1)?, riga.get(2)?)),
    )
    .expect("rilettura")
}

#[test]
fn una_correzione_piu_vecchia_del_profilo_non_sostituisce_quella_di_qui() {
    // La regola del `//!` di `biblioteca`: vince la correzione più recente.
    // Reimportare il profilo di un mese fa non deve riportare indietro quel che
    // si è corretto nel frattempo — la clausola `WHERE excluded.set_at >`
    // dell'upsert è quel che lo impedisce, e nessuna prova la guardava.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut la = libreria_corretta("Vecchio", 600);
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria_corretta("Nuovo", 900);
    let prima = titolo_e_correzione(&qui);
    let piano = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(piano.portati.correzioni, 0, "{piano:?}");
    assert_eq!(titolo_e_correzione(&qui), prima);
    assert_eq!(prima.0, "Nuovo");
}

#[test]
fn una_correzione_piu_recente_del_profilo_vince() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut la = libreria_corretta("Nuovo", 900);
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria_corretta("Vecchio", 600);
    let piano = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(piano.portati.correzioni, 1, "{piano:?}");
    let (titolo, _, set_at) = titolo_e_correzione(&qui);
    assert_eq!((titolo.as_str(), set_at), ("Nuovo", 900));
}

#[test]
fn una_copertina_del_profilo_riempie_solo_i_buchi() {
    // L'altra regola del `//!`: una copertina che c'è resta quella che c'è.
    // Senza `AND cover_art_hash IS NULL` il profilo sostituirebbe le copertine
    // scelte qui.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let la_sua = "b".repeat(64);
    let la_mia = "a".repeat(64);
    let copertina = |c: &Connection, hash: &str| {
        c.execute(
            "INSERT INTO cover_art (hash, mime_type, source, created_at)
             VALUES (?1, 'image/jpeg', 'tag', 0)",
            [hash],
        )
        .expect("copertina");
    };

    let mut la = libreria();
    brano(&la, 1, r"D:\M\01.flac", "artista|uno|album");
    brano(&la, 2, r"D:\M\02.flac", "artista|due|album");
    copertina(&la, &la_sua);
    la.execute("UPDATE tracks SET cover_art_hash = ?1", [&la_sua])
        .expect("copertine di là");
    let archivio = esporta(&mut la, dir.path(), "la", &identita_fissa);
    let mut letto = leggi(&archivio).expect("lettura");
    // E un ascolto da una fonte che questa versione non conosce: ricade su
    // `local` invece di far fallire tutta la transazione sul `CHECK`.
    letto.biblioteca.cronologia.push(biblioteca::Ascoltato {
        chiave: "artista|due|album".to_owned(),
        quando_ms: 1_000,
        ms: 200_000,
        fonte: "youtube".to_owned(),
    });

    let mut qui = libreria();
    brano(&qui, 1, r"E:\M\01.flac", "artista|uno|album");
    brano(&qui, 2, r"E:\M\02.flac", "artista|due|album");
    copertina(&qui, &la_mia);
    qui.execute(
        "UPDATE tracks SET cover_art_hash = ?1 WHERE id = 1",
        [&la_mia],
    )
    .expect("la copertina di qui");

    let piano = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(piano.portati.copertine, 1, "{piano:?}");
    let copertine: Vec<Option<String>> = qui
        .prepare("SELECT cover_art_hash FROM tracks ORDER BY id")
        .expect("interrogazione")
        .query_map([], |riga| riga.get(0))
        .expect("lettura")
        .collect::<Result<_, _>>()
        .expect("lettura");
    assert_eq!(copertine, vec![Some(la_mia), Some(la_sua)]);
    let fonte: String = qui
        .query_row(
            "SELECT source FROM play_history WHERE played_at = 1000",
            [],
            |riga| riga.get(0),
        )
        .expect("l'ascolto è entrato");
    assert_eq!(fonte, "local");
}

#[test]
fn una_chiave_di_contenuto_ambigua_non_esce_con_la_correzione() {
    // Il FLAC corretto e l'mp3 con gli stessi tag, lasciato com'era: la chiave
    // di contenuto è la stessa, e non dice quale dei due file sia quello
    // corretto. Dall'altra parte correggerebbe anche l'mp3.
    let dei_tag = "unknown artist|traccia 01|al";
    let c = libreria();
    brano(&c, 1, r"D:\M\01.flac", "pink floyd|hey you|al");
    brano(&c, 2, r"D:\M\01.mp3", dei_tag);
    c.execute("UPDATE tracks SET content_key = ?1", [dei_tag])
        .expect("stessi tag");
    c.execute(
        "INSERT INTO track_overrides (track_id, campi, set_at) VALUES (1, '{}', 600)",
        [],
    )
    .expect("la correzione");

    let uscita = biblioteca::raccogli(&c).expect("raccolta");
    let correzione = uscita
        .correzioni
        .get("pink floyd|hey you|al")
        .expect("la correzione esce");
    assert!(correzione.contenuti.is_empty(), "{correzione:?}");

    // Tolto l'mp3, la chiave torna a dire un file solo.
    c.execute("DELETE FROM tracks WHERE id = 2", [])
        .expect("via l'mp3");
    let uscita = biblioteca::raccogli(&c).expect("raccolta");
    assert_eq!(
        uscita
            .correzioni
            .get("pink floyd|hey you|al")
            .map(|c| c.contenuti.clone()),
        Some(vec![dei_tag.to_owned()])
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

#[test]
fn chi_dice_che_e_la_sua_libreria_la_riavra() {
    // Il caso della via d'uscita: un computer nuovo che ha esportato un profilo
    // prima di importare quello vecchio, e ha così un'identità sua per la
    // stessa musica.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut vecchia = libreria();
    brano(&vecchia, 1, r"D:\M\a.flac", "chiave");
    vecchia
        .execute(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (1, 500, 200000)",
            [],
        )
        .expect("cronologia");
    let archivio = esporta(&mut vecchia, dir.path(), "vecchia", &identita_altrui);
    let letto = leggi(&archivio).expect("lettura");

    let mut nuova = libreria();
    brano(&nuova, 1, r"E:\M\a.flac", "chiave");
    identita(&nuova, &identita_fissa).expect("identità");

    let piano = importa(
        &mut nuova,
        &letto,
        &Ambiente {
            unisci_comunque: true,
            ..ambiente(&c_e_tutto)
        },
    )
    .expect("importazione");
    assert!(
        piano.identita_diversa,
        "il piano continua a dire che le identità sono due"
    );
    let ascolti: i64 = nuova
        .query_row("SELECT COUNT(*) FROM play_history", [], |riga| riga.get(0))
        .expect("conteggio");
    assert_eq!(ascolti, 1, "e la storia torna, perché lo si è chiesto");
    assert_eq!(
        settings::read(&nuova, CHIAVE_IDENTITA),
        Ok(Some(identita_fissa().expect("identità fissa"))),
        "l'identità di questa libreria resta la sua"
    );
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

/// Un profilo v1 scritto a mano, con le voci date.
fn profilo_v1(voci: &[(&str, &str)]) -> Letto {
    let documento = SuDisco {
        rymembra: FIRMA.to_owned(),
        versione: VERSIONE_V1,
        creato_ms: 1,
        voci: voci
            .iter()
            .map(|(chiave, valore)| ((*chiave).to_owned(), (*valore).to_owned()))
            .collect(),
    };
    leggi_v1(
        &serde_json::to_string(&documento).expect("json"),
        Path::new("a-mano.json"),
    )
    .expect("lettura")
}

/// Un elenco di percorsi come lo scrive `library.roots`.
fn elenco(percorsi: &[&str]) -> String {
    serde_json::to_string(percorsi).expect("json")
}

#[test]
fn il_piano_non_sonda_le_condivisioni_che_questo_pc_non_conosce() {
    // Toccare `\\host\share` vuol dire, su Windows, aprire una sessione SMB e
    // presentare le credenziali di chi importa all'host scritto nel file —
    // prima di qualunque «sì». Un percorso di rete che questo computer non
    // conosce già non si sonda: vale «da rimappare».
    let mut qui = libreria();
    settings::write(&qui, "library.roots", &elenco(&[r"D:\Musica"])).expect("scrittura");
    let letto = profilo_v1(&[
        (
            "library.roots",
            &elenco(&[
                r"\\192.0.2.1\m",
                "//192.0.2.1/m",
                r"\\?\UNC\192.0.2.1\m",
                r"\\192.0.2.1@SSL\DavWWWRoot\m",
                r"E:\Musica",
            ]),
        ),
        ("download.folder", r"\\192.0.2.1\scaricati"),
    ]);

    let chiesti = std::cell::RefCell::new(Vec::<String>::new());
    let registra = |percorso: &str| {
        chiesti.borrow_mut().push(percorso.to_owned());
        false
    };
    let proposto = piano(&mut qui, &letto, &ambiente(&registra)).expect("piano");

    let chiesti = chiesti.into_inner();
    assert!(
        chiesti
            .iter()
            .all(|p| !p.starts_with(r"\\") && !p.starts_with("//")),
        "nessuna condivisione sconosciuta si tocca: {chiesti:?}"
    );
    assert!(chiesti.iter().any(|p| p == r"E:\Musica"), "{chiesti:?}");
    assert!(
        proposto
            .rimappature
            .iter()
            .any(|r| r.da == r"\\192.0.2.1\m"),
        "la radice di rete si propone comunque da rimappare: {:?}",
        proposto.rimappature
    );
}

#[test]
fn una_condivisione_gia_nota_si_sonda() {
    // Se questo computer usa già quella condivisione, chiedere se c'è non
    // presenta le credenziali a nessuno di nuovo.
    let mut qui = libreria();
    settings::write(&qui, "library.roots", &elenco(&[r"\\nas\musica"])).expect("scrittura");
    let letto = profilo_v1(&[("library.roots", &elenco(&[r"\\NAS\musica\Rock"]))]);

    let chiesti = std::cell::RefCell::new(Vec::<String>::new());
    let registra = |percorso: &str| {
        chiesti.borrow_mut().push(percorso.to_owned());
        true
    };
    let proposto = piano(&mut qui, &letto, &ambiente(&registra)).expect("piano");
    assert!(
        chiesti.borrow().iter().any(|p| p == r"\\NAS\musica\Rock"),
        "{:?}",
        chiesti.borrow()
    );
    assert!(proposto.rimappature.is_empty());
}

#[test]
fn un_profilo_v1_non_toglie_le_cartelle_di_qui() {
    // Un v1 non ha documento di sincronia, e il ramo che unisce le cartelle
    // non girava: `library.roots` del file sostituiva l'elenco di qui, e la
    // sincronia propagava la rimozione agli altri dispositivi.
    let mut qui = libreria();
    settings::write(&qui, "library.roots", &elenco(&["C:/Musica"])).expect("scrittura");
    let letto = profilo_v1(&[("library.roots", &elenco(&["D:/Sua", "c:\\musica\\"]))]);

    let piano = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(
        settings::read(&qui, "library.roots"),
        Ok(Some(elenco(&["C:/Musica", "D:/Sua"]))),
        "si aggiunge, e la stessa cartella scritta in un altro modo non si ripete"
    );
    assert_eq!(piano.cambi.len(), 1, "{:?}", piano.cambi);

    // Rifatto, non cambia più niente.
    let piano = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert!(piano.cambi.is_empty(), "{:?}", piano.cambi);
}

#[test]
fn il_profilo_di_un_altra_libreria_non_tocca_le_cartelle_di_qui() {
    // Il profilo di un amico, per tema ed equalizzatore: le sue cartelle sono
    // percorsi di un'altra macchina, e unirle le farebbe propagare dalla
    // sincronia agli altri dispositivi come cartelle accese adesso.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut sua = libreria();
    settings::write(&sua, "library.roots", &elenco(&["D:/Sua"])).expect("scrittura");
    settings::write(&sua, "player.volume", "0.9").expect("scrittura");
    let archivio = esporta(&mut sua, dir.path(), "sua", &identita_altrui);
    let letto = leggi(&archivio).expect("lettura");

    let mut qui = libreria();
    identita(&qui, &identita_fissa).expect("identità");
    settings::write(&qui, "library.roots", &elenco(&["C:/Musica"])).expect("scrittura");
    let piano = importa(&mut qui, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert!(piano.identita_diversa);
    assert_eq!(
        settings::read(&qui, "library.roots"),
        Ok(Some(elenco(&["C:/Musica"])))
    );
    assert_eq!(
        settings::read(&qui, "player.volume"),
        Ok(Some("0.9".to_owned())),
        "le preferenze passano lo stesso"
    );
    assert!(piano.cambi.iter().all(|c| c.chiave != "library.roots"));
}

#[test]
fn le_sonde_del_piano_hanno_un_tetto() {
    // Il numero delle radici lo decide il file, e ogni sonda di un'unità di
    // rete che non risponde costa secondi sotto il lucchetto della libreria.
    let mut qui = libreria();
    let radici: Vec<String> = (0..60).map(|n| format!(r"Z:\Musica\{n}")).collect();
    let radici: Vec<&str> = radici.iter().map(String::as_str).collect();
    let letto = profilo_v1(&[("library.roots", &elenco(&radici))]);

    let chiesti = std::cell::RefCell::new(Vec::<String>::new());
    let registra = |percorso: &str| {
        chiesti.borrow_mut().push(percorso.to_owned());
        false
    };
    let proposto = piano(&mut qui, &letto, &ambiente(&registra)).expect("piano");
    assert!(
        chiesti.borrow().len() <= super::MAX_SONDE,
        "{} sonde",
        chiesti.borrow().len()
    );
    assert_eq!(
        proposto.rimappature.len(),
        60,
        "quelle oltre il tetto restano da rimappare, non spariscono"
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
fn annullare_toglie_le_preferenze_che_prima_non_c_erano() {
    // Su un computer nuovo skin, equalizzatore e tema non sono mai stati
    // scelti: in `settings` non ci sono. La copia di sicurezza non li porta, e
    // «Torna com'era prima» scorreva solo le chiavi della copia — quelle del
    // profilo restavano, e la finestra diceva di averle rimesse.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let dati = dir.path().join("dati");
    let mut qui = libreria();
    settings::write(&qui, "player.volume", "0.5").expect("scrittura");
    settings::write(&qui, "library.roots", &elenco(&["C:/Musica"])).expect("scrittura");
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

    let mut altro = libreria();
    settings::write(&altro, "skin.active", "sua").expect("scrittura");
    settings::write(&altro, "player.eq", "[1,2,3]").expect("scrittura");
    settings::write(&altro, "player.volume", "0.9").expect("scrittura");
    settings::write(&altro, "library.roots", &elenco(&["D:/Sua"])).expect("scrittura");
    let archivio = esporta(&mut altro, dir.path(), "altro", &identita_fissa);
    importa(
        &mut qui,
        &leggi(&archivio).expect("lettura"),
        &ambiente(&c_e_tutto),
    )
    .expect("importazione");
    assert_eq!(
        settings::read(&qui, "skin.active"),
        Ok(Some("sua".to_owned()))
    );

    let piano = annulla(&mut qui, &copia, &ambiente(&c_e_tutto)).expect("annullamento");
    assert_eq!(settings::read(&qui, "skin.active"), Ok(None));
    assert_eq!(settings::read(&qui, "player.eq"), Ok(None));
    assert_eq!(
        settings::read(&qui, "player.volume"),
        Ok(Some("0.5".to_owned()))
    );
    // Le cartelle che il profilo aveva aggiunto se ne vanno: l'annullamento
    // rimette l'elenco della copia com'è, non lo unisce.
    assert_eq!(
        settings::read(&qui, "library.roots"),
        Ok(Some(elenco(&["C:/Musica"])))
    );
    assert!(
        piano.cambi.iter().any(|c| c.chiave == "skin.active"),
        "e la chiave tolta si conta fra i cambi: {:?}",
        piano.cambi
    );
}

#[test]
fn una_copia_senza_l_elenco_delle_assenti_non_dimentica_niente() {
    // Una copia scritta da una versione di prima non dice quali chiavi
    // mancavano, e il suo catalogo poteva essere più corto: nel dubbio non si
    // toglie niente.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let copia = dir.path().join("prima.json");
    std::fs::write(
        &copia,
        r#"{"rymembra":"rymembra.profilo","versione":1,"creato_ms":1,"voci":{"player.volume":"0.5"}}"#,
    )
    .expect("copia");
    let mut qui = libreria();
    settings::write(&qui, "skin.active", "sua").expect("scrittura");
    annulla(&mut qui, &copia, &ambiente(&c_e_tutto)).expect("annullamento");
    assert_eq!(
        settings::read(&qui, "skin.active"),
        Ok(Some("sua".to_owned()))
    );
}

/// Una raccolta vuota, da gonfiare a mano oltre i limiti della lettura.
fn raccolto_vuoto() -> Raccolto {
    let mut c = libreria();
    raccogli(
        &mut c,
        "qui",
        BTreeMap::new(),
        BTreeMap::new(),
        1,
        &identita_fissa,
    )
    .expect("raccolta")
}

/// Cento e una migliaia di copertine: con le miniature, più voci di quante la
/// lettura ne ammetta.
fn troppe_copertine(raccolto: &mut Raccolto) {
    for n in 0..=archivio::limiti::MAX_VOCI.div_euclid(2) {
        raccolto.biblioteca.copertine.insert(
            format!("brano-{n}"),
            biblioteca::Copertina {
                hash: format!("{n:064x}"),
                mime_type: "image/jpeg".to_owned(),
                width: None,
                height: None,
                byte_size: None,
                source: "tag".to_owned(),
                created_at: 0,
            },
        );
    }
}

#[test]
fn l_esportazione_rifiuta_un_profilo_che_la_lettura_rifiuterebbe() {
    // `indice` rifiuta l'archivio intero oltre `MAX_VOCI` voci o con una voce
    // oltre `MAX_VOCE`; l'esportazione scriveva lo stesso, diceva «fatto», e
    // sul computer nuovo il profilo non si apriva. Adesso si ferma prima di
    // scrivere, e il profilo di prima sulla chiavetta resta com'era.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let store = CoverStore::open(dir.path().join("store")).expect("store");
    let sorgenti = Sorgenti {
        copertine: &store,
        skin: BTreeMap::new(),
        bozze: BTreeMap::new(),
    };
    let destinazione = dir.path().join("Rymembra.ryprofile");
    std::fs::write(&destinazione, b"il profilo di prima").expect("profilo vecchio");

    let mut troppe = raccolto_vuoto();
    troppe_copertine(&mut troppe);
    let err = scrivi(&troppe, &sorgenti, &destinazione, &|_| {}).expect_err("troppe voci");
    assert_eq!(err.code().kind().code(), "profile.tooLarge");
    // Quando a sforare è il numero delle voci, `voce` è vuota e il limite
    // conta voci.
    assert!(matches!(
        err.code(),
        ErrorCode::ProfileTooLarge { voce, limite, .. }
            if voce.is_empty()
                && *limite == u64::try_from(archivio::limiti::MAX_VOCI).expect("limite")
    ));

    let mut enorme = raccolto_vuoto();
    let limite = usize::try_from(archivio::limiti::MAX_VOCE).expect("limite");
    enorme.biblioteca.testi.insert(
        "k".to_owned(),
        biblioteca::Testo {
            plain: Some("a".repeat(limite + 1)),
            synced: None,
            source: "mano".to_owned(),
            lrclib_id: None,
            offset_ms: 0,
            instrumental: false,
            duration_ms: None,
            updated_at: 0,
        },
    );
    let err = scrivi(&enorme, &sorgenti, &destinazione, &|_| {}).expect_err("voce enorme");
    assert_eq!(err.code().kind().code(), "profile.tooLarge");
    assert!(matches!(
        err.code(),
        ErrorCode::ProfileTooLarge { voce, byte, limite }
            if voce == archivio::NOME_BIBLIOTECA
                && *byte > archivio::limiti::MAX_VOCE
                && *limite == archivio::limiti::MAX_VOCE
    ));

    assert_eq!(
        std::fs::read(&destinazione).expect("rilettura"),
        b"il profilo di prima"
    );
    assert!(
        !dir.path().join("Rymembra.ryprofile.parziale").exists(),
        "si ferma prima di aprire qualunque file"
    );
}

#[test]
fn la_copia_di_sicurezza_si_scrive_anche_oltre_i_limiti() {
    // La decisione: il controllo vale per l'esportazione chiesta da chi usa il
    // programma, non per la copia di sicurezza. Senza copia l'importazione non
    // parte, e chi ha una libreria oltre il limite non potrebbe più importare
    // nemmeno un profilo di sole preferenze.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let store = CoverStore::open(dir.path().join("store")).expect("store");
    let mut troppe = raccolto_vuoto();
    troppe_copertine(&mut troppe);
    let copia = copia_di_sicurezza(dir.path(), 1);
    scrivi(
        &troppe,
        &Sorgenti {
            copertine: &store,
            skin: BTreeMap::new(),
            bozze: BTreeMap::new(),
        },
        &copia,
        &|_| {},
    )
    .expect("la copia si scrive");
    assert!(copia.exists());
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

// ── i profili della versione di prima ───────────────────────────────────────
//
// Su chiavette e in cartelle di backup ci sono profili scritti con i nomi di
// allora: il campo e la firma del manifesto, le skin con la loro estensione, i
// documenti di sincronia col prefisso di allora, le sorgenti dei desiderati.
// Si leggono e si importano; quel che si scrive da qui in poi ha solo nomi
// nuovi.

/// Esporta la libreria con delle skin e delle bozze dentro.
fn esporta_con_pacchetti(
    connection: &mut Connection,
    dove: &Path,
    skin: BTreeMap<String, PathBuf>,
    bozze: BTreeMap<String, PathBuf>,
) -> PathBuf {
    let store = CoverStore::open(dove.join("store")).expect("store");
    let raccolto = raccogli(
        connection,
        "la",
        BTreeMap::new(),
        BTreeMap::new(),
        1_700_000_000_000,
        &identita_fissa,
    )
    .expect("raccolta");
    let destinazione = dove.join(format!("profilo.{}", archivio::ESTENSIONE));
    scrivi(
        &raccolto,
        &Sorgenti {
            copertine: &store,
            skin,
            bozze,
        },
        &destinazione,
        &|_| {},
    )
    .expect("scrittura");
    destinazione
}

/// Il campo e la firma di prima al posto di quelli di adesso, in un JSON.
fn con_la_firma_di_prima(json: &[u8]) -> Vec<u8> {
    let mut valore: serde_json::Value = serde_json::from_slice(json).expect("JSON");
    let oggetto = valore.as_object_mut().expect("un oggetto");
    assert!(oggetto.remove("rymembra").is_some(), "il campo di adesso");
    oggetto.insert("aether".to_owned(), eredita::FIRMA_PROFILO.into());
    serde_json::to_vec(&valore).expect("JSON")
}

/// Riscrive un profilo come lo scriveva la versione di prima: stessi byte,
/// nomi di allora.
///
/// Il formato è lo stesso e cambiano quattro cose, tutte qui: il campo e la
/// firma dei due JSON, il prefisso dei documenti di sincronia, l'estensione
/// delle skin, il prefisso delle sorgenti dentro la biblioteca. Partire da un
/// profilo vero e rinominare è più fedele di ricostruirne uno a mano.
fn come_lo_scriveva_la_versione_di_prima(nuovo: &Path, vecchio: &Path) {
    use std::io::{Read as _, Write as _};

    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(nuovo).expect("apertura")).expect("archivio");
    let mut uscita = zip::ZipWriter::new(std::fs::File::create(vecchio).expect("creazione"));
    let opzioni =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for indice in 0..zip.len() {
        let mut voce = zip.by_index(indice).expect("voce");
        let nome = voce.name().to_owned();
        let mut byte = Vec::new();
        voce.read_to_end(&mut byte).expect("lettura");

        let (nome, byte) = if nome == archivio::NOME_MANIFESTO || nome == archivio::NOME_PREFERENZE
        {
            (nome, con_la_firma_di_prima(&byte))
        } else if nome == archivio::NOME_BIBLIOTECA {
            let testo = String::from_utf8(byte).expect("UTF-8");
            let di_prima =
                testo.replace("\"rymembra:", &format!("\"{}", eredita::PREFISSO_SORGENTE));
            (nome, di_prima.into_bytes())
        } else if let Some(resto) = nome.strip_prefix("sincronia/rymembra-") {
            (
                format!("sincronia/{}{resto}", eredita::PREFISSO_DISPOSITIVO),
                byte,
            )
        } else if let Some(id) = nome
            .strip_prefix("skin/")
            .and_then(|foglia| foglia.strip_suffix(".ryskin"))
        {
            (format!("skin/{id}.{}", eredita::ESTENSIONE_SKIN), byte)
        } else {
            (nome, byte)
        };
        uscita.start_file(nome, opzioni).expect("voce nuova");
        uscita.write_all(&byte).expect("scrittura");
    }
    uscita.finish().expect("chiusura");
}

/// I nomi delle voci di un archivio.
fn nomi_nell_archivio(percorso: &Path) -> Vec<String> {
    let zip =
        zip::ZipArchive::new(std::fs::File::open(percorso).expect("apertura")).expect("archivio");
    zip.file_names().map(str::to_owned).collect()
}

/// Le sorgenti dei desiderati, per data di inserimento.
fn sorgenti_dei_desiderati(connection: &Connection) -> Vec<String> {
    connection
        .prepare("SELECT source_id FROM desiderati ORDER BY added_at, track_key")
        .expect("interrogazione")
        .query_map([], |riga| riga.get(0))
        .expect("lettura")
        .collect::<Result<_, _>>()
        .expect("lettura")
}

/// Una libreria con le due specie di desiderato: uno da un archivio di Spotify,
/// con la sorgente fabbricata, e uno da una playlist vera.
fn libreria_con_desiderati(prefisso: &str) -> Connection {
    let c = libreria();
    settings::write(&c, "player.volume", "0.42").expect("scrittura");
    brano(&c, 1, "C:/m/a.mp3", "a|b|c");
    c.execute_batch(&format!(
        "INSERT INTO desiderati (track_key, title, source_kind, source_id, source_title, added_at)
         VALUES ('k1', 'Girl', 'playlist', '{prefisso}archivio:playlist:corsa', 'Corsa', 1);
         INSERT INTO desiderati (track_key, title, source_kind, source_id, source_title, added_at)
         VALUES ('k2', 'Bolero', 'playlist', '37i9dQZF1DXcBWIGoYBM5M', 'Una vera', 2);"
    ))
    .expect("desiderati");
    c
}

#[test]
fn un_profilo_scritto_dalla_versione_di_prima_si_legge_e_si_importa() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let skin = dir.path().join("sorgente.ryskin");
    let bozza = dir.path().join("bozza.json");
    std::fs::write(&skin, b"PK pacchetto").expect("skin");
    std::fs::write(&bozza, b"{\"bozza\":1}").expect("bozza");
    let mut sorgente = libreria_con_desiderati("rymembra:");
    let nuovo = esporta_con_pacchetti(
        &mut sorgente,
        dir.path(),
        BTreeMap::from([("notte".to_owned(), skin)]),
        BTreeMap::from([("prova".to_owned(), bozza)]),
    );
    assert!(
        leggi(&nuovo).expect("lettura").sincronia.is_some(),
        "senza documento di sincronia la prova non prova il prefisso"
    );

    let vecchio = dir.path().join("di-prima.aeprofile");
    come_lo_scriveva_la_versione_di_prima(&nuovo, &vecchio);
    let nomi = nomi_nell_archivio(&vecchio);
    assert!(
        nomi.contains(&format!("skin/notte.{}", eredita::ESTENSIONE_SKIN)),
        "{nomi:?}"
    );
    assert!(
        nomi.iter()
            .any(|nome| nome.starts_with(&format!("sincronia/{}", eredita::PREFISSO_DISPOSITIVO))),
        "{nomi:?}"
    );

    // Si legge: la firma di prima, le preferenze di prima, il documento di
    // sincronia col prefisso di prima, le sorgenti di prima.
    let letto = leggi(&vecchio).expect("un profilo di prima si legge");
    assert_eq!(letto.manifesto.rymembra, eredita::FIRMA_PROFILO);
    assert_eq!(letto.preferenze.rymembra, eredita::FIRMA_PROFILO);
    assert_eq!(
        letto
            .preferenze
            .voci
            .get("player.volume")
            .map(String::as_str),
        Some("0.42")
    );
    assert!(
        letto.sincronia.is_some(),
        "il documento di sincronia col prefisso di prima si riconosce"
    );
    assert_eq!(letto.biblioteca.desiderati.len(), 2);
    assert!(
        letto
            .biblioteca
            .desiderati
            .iter()
            .any(|d| d.source_id.starts_with(eredita::PREFISSO_SORGENTE)),
        "la prova deve partire dalla sorgente di prima"
    );

    // Le skin e le bozze escono con l'identificativo e i byte giusti: chi le
    // installa le scrive col nome di adesso.
    let mut viste = Vec::new();
    let (skin_viste, bozze_viste) = per_ogni_pacchetto(&vecchio, |id, bozza, byte| {
        viste.push((id.to_owned(), bozza, byte.to_vec()));
        Ok(true)
    })
    .expect("pacchetti");
    assert_eq!((skin_viste, bozze_viste), (1, 1));
    viste.sort();
    assert_eq!(
        viste,
        [
            ("notte".to_owned(), false, b"PK pacchetto".to_vec()),
            ("prova".to_owned(), true, b"{\"bozza\":1}".to_vec()),
        ]
    );

    // Si importa, e le sorgenti entrano col prefisso di adesso.
    let mut destinazione = libreria();
    brano(&destinazione, 1, "C:/m/a.mp3", "a|b|c");
    importa(&mut destinazione, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(
        sorgenti_dei_desiderati(&destinazione),
        ["rymembra:archivio:playlist:corsa", "37i9dQZF1DXcBWIGoYBM5M"]
    );
    assert_eq!(
        settings::read(&destinazione, "player.volume"),
        Ok(Some("0.42".to_owned()))
    );

    // Reimportare non raddoppia niente.
    importa(&mut destinazione, &letto, &ambiente(&c_e_tutto)).expect("seconda importazione");
    assert_eq!(sorgenti_dei_desiderati(&destinazione).len(), 2);
}

#[test]
fn una_sorgente_di_prima_non_raddoppia_la_riga_gia_migrata() {
    // La libreria di qui è già passata dalla migrazione 31: ha la riga col
    // prefisso di adesso. Un profilo di prima porta la stessa riga col prefisso
    // di allora, e non deve aggiungerne una seconda.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let mut sorgente = libreria_con_desiderati("rymembra:");
    let nuovo = esporta_con_pacchetti(&mut sorgente, dir.path(), BTreeMap::new(), BTreeMap::new());
    let vecchio = dir.path().join("di-prima.aeprofile");
    come_lo_scriveva_la_versione_di_prima(&nuovo, &vecchio);
    let letto = leggi(&vecchio).expect("lettura");

    let mut destinazione = libreria_con_desiderati("rymembra:");
    let prima = sorgenti_dei_desiderati(&destinazione);
    importa(&mut destinazione, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(sorgenti_dei_desiderati(&destinazione), prima);
}

#[test]
fn una_scrittura_nuova_porta_solo_nomi_nuovi() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let skin = dir.path().join("sorgente.ryskin");
    std::fs::write(&skin, b"PK pacchetto").expect("skin");
    let mut sorgente = libreria_con_desiderati("rymembra:");
    let archivio = esporta_con_pacchetti(
        &mut sorgente,
        dir.path(),
        BTreeMap::from([("notte".to_owned(), skin)]),
        BTreeMap::new(),
    );
    assert!(archivio.to_string_lossy().ends_with(".ryprofile"));

    // Il nome di prima, per esteso, non compare in nessuna voce né nei testi
    // che raccontano l'archivio: è lo stesso nome che tutte le costanti di
    // eredità condividono come parte fissa.
    let di_prima = eredita::STEM_DIARIO;
    let nomi = nomi_nell_archivio(&archivio);
    assert!(
        nomi.contains(&"skin/notte.ryskin".to_owned())
            && nomi
                .iter()
                .any(|nome| nome.starts_with("sincronia/rymembra-")),
        "{nomi:?}"
    );
    for nome in &nomi {
        assert!(!nome.to_lowercase().contains(di_prima), "{nome}");
    }

    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(&archivio).expect("apertura")).expect("archivio");
    for json in [
        archivio::NOME_MANIFESTO,
        archivio::NOME_PREFERENZE,
        archivio::NOME_BIBLIOTECA,
    ] {
        let mut testo = String::new();
        std::io::Read::read_to_string(&mut zip.by_name(json).expect("voce"), &mut testo)
            .expect("lettura");
        assert!(!testo.to_lowercase().contains(di_prima), "{json}: {testo}");
        // Senza spazi: i due JSON possono essere scritti rientrati.
        testo.retain(|c| !c.is_whitespace());
        if json != archivio::NOME_BIBLIOTECA {
            assert!(testo.contains("\"rymembra\":"), "{json}: {testo}");
            assert!(testo.contains(FIRMA), "{json}: {testo}");
        } else {
            assert!(
                testo.contains("\"rymembra:archivio:playlist:corsa\""),
                "{testo}"
            );
        }
    }

    // E il file in sé si rilegge come un profilo di adesso.
    let letto = leggi(&archivio).expect("lettura");
    assert_eq!(letto.manifesto.rymembra, FIRMA);
    assert_eq!(letto.preferenze.rymembra, FIRMA);
    let ripetuto = serde_json::to_string(&letto.preferenze).expect("serializzazione");
    assert!(!ripetuto.contains("aether"), "{ripetuto}");
}

#[test]
fn un_profilo_v1_con_il_campo_di_prima_si_legge() {
    // Un `.json` scritto dalla versione di prima: il campo si chiamava
    // `aether`, e la firma era la sua.
    let dir = tempfile::tempdir().expect("cartella temporanea");
    let vecchio = dir.path().join("profilo-di-prima.json");
    std::fs::write(
        &vecchio,
        format!(
            r#"{{"aether":"{}","versione":1,"creato_ms":1700000000000,
                "voci":{{"player.volume":"0.42","skin.active":"sala"}}}}"#,
            eredita::FIRMA_PROFILO
        ),
    )
    .expect("v1");

    let letto = leggi(&vecchio).expect("un v1 di prima si legge");
    assert_eq!(letto.manifesto.versione, 1);
    assert!(!letto.ha_libreria());
    let mut c = libreria();
    let piano = importa(&mut c, &letto, &ambiente(&c_e_tutto)).expect("importazione");
    assert_eq!(piano.cambi.len(), 2);
    assert_eq!(
        settings::read(&c, "skin.active"),
        Ok(Some("sala".to_owned()))
    );

    // Il campo di adesso con la firma di prima, e il campo di prima con la
    // firma di adesso: due mescolanze che un file fatto a mano può avere, e la
    // firma è quel che decide.
    for mescolato in [
        format!(
            r#"{{"rymembra":"{}","versione":1,"creato_ms":0,"voci":{{}}}}"#,
            eredita::FIRMA_PROFILO
        ),
        format!(r#"{{"aether":"{FIRMA}","versione":1,"creato_ms":0,"voci":{{}}}}"#),
    ] {
        let file = dir.path().join("mescolato.json");
        std::fs::write(&file, mescolato).expect("v1");
        assert!(leggi(&file).is_ok());
    }
}

#[test]
fn una_firma_estranea_nel_campo_di_prima_resta_rifiutata() {
    let dir = tempfile::tempdir().expect("cartella temporanea");
    for estranea in [
        r#"{"aether":"altro","versione":1,"creato_ms":0,"voci":{}}"#,
        r#"{"aether":"","versione":1,"creato_ms":0,"voci":{}}"#,
        // Maiuscole: le firme si confrontano per intero, non per somiglianza.
        r#"{"aether":"Aether.profilo","versione":1,"creato_ms":0,"voci":{}}"#,
    ] {
        let file = dir.path().join("estraneo.json");
        std::fs::write(&file, estranea).expect("v1");
        let err = leggi(&file).expect_err("deve rifiutare");
        assert_eq!(err.code().kind().code(), "settings.corrupt", "{estranea}");
    }
}
