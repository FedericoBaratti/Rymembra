//! Le prove del ponte. Tre leggono `insieme.rs` come testo (le consegne a tre tempi, l'aggiunta
//! di un membro a coda piena, lo specchio): le funzioni che cercano devono restare nel padre.

use super::*;

use std::path::Path;

use rymembra_app::library::TrackSummary;
use rymembra_app::playback::{Collocazione, SchedaSorgente};

use super::brani::{classifica, leggi_righe, ms_da_cd, righe_dei_uid, scheda_da_riga};
use super::regole::{Partenza, autoplay_per, coda_si_scrive_per};
use super::regole::{dissolvenza_per, punto_da_mostrare};

#[test]
fn in_sessione_la_dissolvenza_e_zero_e_fuori_e_quella_scelta() {
    assert_eq!(dissolvenza_per(true, 8), 0);
    assert_eq!(dissolvenza_per(false, 8), 8000);
    assert_eq!(dissolvenza_per(false, 0), 0);
}

#[test]
fn in_sessione_niente_autoplay_e_la_coda_non_si_scrive() {
    assert!(!autoplay_per(true, true));
    assert!(autoplay_per(false, true));
    assert!(!autoplay_per(false, false));
    assert!(!coda_si_scrive_per(true), "la coda di sessione è sospesa");
    assert!(coda_si_scrive_per(false));
}

/// Le regole di sessione decidono quel che vale **adesso**, e non toccano
/// quel che è scritto: si legge la libreria prima e dopo averle applicate
/// e il contenuto di `settings` è identico, riga per riga.
#[test]
fn le_regole_di_sessione_non_scrivono_mai_nelle_impostazioni() {
    let libreria = rymembra_app::db::open_in_memory()
        .expect("database")
        .connection;
    rymembra_app::playback::save_crossfade(&libreria, 8).expect("dissolvenza");
    rymembra_app::playback::save_autoplay(&libreria, true).expect("autoplay");
    let coda = QueueSnapshot {
        tracks: vec![1, 2, 3],
        order: vec![0, 1, 2],
        order_pos: Some(1),
        shuffle: true,
        repeat: rymembra_domain::queue::RepeatMode::All,
    };
    rymembra_app::playback::save_queue(&libreria, &coda).expect("coda");
    let fotografia = |c: &rusqlite::Connection| -> Vec<(String, String)> {
        let mut sql = c
            .prepare("SELECT key, value FROM settings ORDER BY key")
            .expect("query");
        sql.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("righe")
            .collect::<Result<_, _>>()
            .expect("lettura")
    };
    let prima = fotografia(&libreria);

    // Quel che il lettore legge dalle impostazioni, e quel che la sessione
    // ne fa: la dissolvenza a zero, l'autoplay spento, la coda non scritta.
    let secondi = rymembra_app::playback::load_crossfade(&libreria).expect("letta");
    let voluto = rymembra_app::playback::load_autoplay(&libreria).expect("letto");
    assert_eq!(dissolvenza_per(true, secondi), 0);
    assert!(!autoplay_per(true, voluto));
    assert!(!coda_si_scrive_per(true));
    // La sessione mette da parte la coda in memoria, così com'è.
    let sessione = Sessione::nuova(
        Ruolo::Ospita,
        CodaDiPrima {
            istantanea: rymembra_app::playback::load_queue(&libreria).expect("coda letta"),
            curata: true,
            ms: 0,
            avviata: false,
        },
    );
    assert_eq!(sessione.prima.istantanea, coda);

    assert_eq!(
        fotografia(&libreria),
        prima,
        "le impostazioni sono cambiate"
    );
    // E all'uscita le regole tornano quelle scritte, senza rileggere niente.
    assert_eq!(dissolvenza_per(false, secondi), 8000);
    assert!(autoplay_per(false, voluto));
    // Una coda di maniglie, se mai arrivasse al disco, il nucleo la
    // rifiuterebbe: la seconda difesa dietro alla sospensione.
    let di_maniglie = QueueSnapshot {
        tracks: vec![-1, -2],
        order: vec![0, 1],
        order_pos: Some(0),
        shuffle: false,
        repeat: rymembra_domain::queue::RepeatMode::Off,
    };
    assert!(rymembra_app::playback::save_queue(&libreria, &di_maniglie).is_err());
    assert_eq!(fotografia(&libreria), prima);
}

/// La posizione vecchia sul brano nuovo, vista nella prova: dopo
/// «prossimo» il motore dice ancora 63 798 ms finché la voce nuova non
/// parte, e la finestra deve mostrare l'inizio — o il punto del salto.
#[test]
fn fino_alla_partenza_si_mostra_il_punto_promesso() {
    let promessa = Programmata {
        maniglia: Some(-7),
        etichetta: Some(7),
        posizione_us: 42_000_000,
        quando_ns: 1_000,
    };
    // La voce nuova è in coda e il motore suona ancora la vecchia: il
    // punto promesso, anche dopo l'istante (il primo campione è in ritardo).
    assert_eq!(
        punto_da_mostrare(Some(&promessa), Some(-7), Some(-6), 63_798, 500),
        Some(42_000)
    );
    assert_eq!(
        punto_da_mostrare(Some(&promessa), Some(-7), Some(-6), 63_798, 5_000),
        Some(42_000)
    );
    // Un salto nella stessa voce: il punto d'arrivo fino all'istante, poi
    // quello vero del motore — ma solo quando il motore c'è arrivato.
    assert_eq!(
        punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 63_798, 500),
        Some(42_000)
    );
    assert_eq!(
        punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 42_004, 5_000),
        None
    );
    // L'istante è passato ma il motore dice ancora il punto di prima (il
    // primo campione del salto non è ancora all'orecchio): il promesso,
    // non il vecchio — la barra non torna indietro.
    assert_eq!(
        punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 63_798, 400_000_000),
        Some(42_000)
    );
    // Arrivato, e andato avanti di 400 ms come il tempo: il motore.
    assert_eq!(
        punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 42_390, 400_001_000),
        None
    );
    // Scaduta, anche se il motore non c'è: la promessa non vale più, e la
    // barra dice quel che il motore dice — non resta ferma per sempre.
    assert_eq!(
        punto_da_mostrare(Some(&promessa), Some(-7), Some(-7), 81_394, 600_000_000_000),
        None
    );
    // Lo specchio di chi segue già sulla voce dopo, senza promessa: zero.
    assert_eq!(punto_da_mostrare(None, Some(-8), Some(-7), 5, 0), Some(0));
    // Una promessa di un'altra voce non conta; motore e coda d'accordo, il
    // motore.
    assert_eq!(
        punto_da_mostrare(Some(&promessa), Some(-9), Some(-9), 5, 0),
        None
    );
    assert_eq!(punto_da_mostrare(None, Some(-9), None, 5, 0), None);
}

/// Una riga di `tracks` per le prove delle voci da mandare: un file, o un
/// brano di catalogo se `percorso` è `None`.
fn riga_in_tabella(c: &rusqlite::Connection, id: i64, percorso: Option<&str>) {
    // Un file ha percorso, dimensione e data; un brano di catalogo ha
    // servizio, indirizzo, licenza e disponibilità, e niente del file: è
    // il `CHECK` della migrazione 26.
    let esito = match percorso {
        Some(percorso) => c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, duration_ms,
                                     file_size, date_added, date_modified)
                 VALUES (?1, ?2, 'k' || ?1, 'T', 'A', 'B', 200000, 3, 0, 0)",
            rusqlite::params![id, percorso],
        ),
        None => c.execute(
            "INSERT INTO tracks (id, track_key, title, artist, album, duration_ms,
                                     date_added, source_service, fonte_url, licenza,
                                     disponibilita)
                 VALUES (?1, 'k' || ?1, 'T', 'A', 'B', 200000, 0, 'internet-archive',
                         'https://archive.org/download/gd77/t01.mp3', 'cc-by',
                         'scaricabile')",
            rusqlite::params![id],
        ),
    };
    esito.expect("riga");
}

/// Il disco delle prove: quello vero, con due eccezioni a nome. Un file
/// che si chiama `negato` risponde «permesso negato», uno che si chiama
/// `lento` fa aspettare due secondi. Conta le letture.
fn disco_finto(
    letture: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) -> impl Fn(&Path) -> std::io::Result<std::fs::Metadata> + Send + 'static {
    move |percorso: &Path| {
        letture.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let nome = percorso.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if nome.starts_with("negato") {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        if nome.starts_with("lento") {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        std::fs::metadata(percorso)
    }
}

fn esiti_in_breve(esiti: &[Result<BranoDiQuesto, Mancanza>]) -> Vec<Result<i64, Mancanza>> {
    esiti
        .iter()
        .map(|esito| esito.as_ref().map(|brano| brano.track_id).map_err(|m| *m))
        .collect()
}

/// I casi del disco e della libreria, con file veri, nell'ordine chiesto e
/// coi doppioni: il file c'è, la riga non c'è più, il file non c'è più, il
/// disco risponde un altro errore.
#[test]
fn ogni_voce_dice_perche_non_si_puo_mandare() {
    let cartella = tempfile::tempdir().expect("cartella");
    let presente = cartella.path().join("presente.flac");
    std::fs::write(&presente, b"abc").expect("file");
    let sparito = cartella
        .path()
        .join("sottocartella-sparita")
        .join("sparito.flac");
    let negato = cartella.path().join("negato.flac");
    let c = rymembra_app::db::open_in_memory()
        .expect("database")
        .connection;
    riga_in_tabella(&c, 1, Some(&presente.to_string_lossy()));
    riga_in_tabella(&c, 2, Some(&sparito.to_string_lossy()));
    riga_in_tabella(&c, 4, Some(&negato.to_string_lossy()));

    let lette = leggi_righe(&c, &[2, 1, 3, 4, 1]).expect("lette");
    let letture = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let esiti = classifica(
        lette,
        std::time::Duration::from_secs(5),
        disco_finto(std::sync::Arc::clone(&letture)),
    );
    assert_eq!(
        esiti_in_breve(&esiti),
        [
            Err(Mancanza::FileAssente),
            Ok(1),
            Err(Mancanza::RigaSparita),
            Err(Mancanza::Irraggiungibile),
            Ok(1),
        ]
    );
    // Il brano presente porta il suo file com'è sul disco.
    let file = esiti
        .get(1)
        .and_then(|esito| esito.as_ref().ok())
        .and_then(|brano| brano.file.clone())
        .expect("file");
    assert_eq!(file.dimensione, 3);
    assert_eq!(file.estensione.as_deref(), Some("flac"));
    assert_eq!(letture.load(std::sync::atomic::Ordering::SeqCst), 4);
}

/// La scadenza del blocco taglia a metà: quel che è stato letto prima
/// resta, la voce su cui il disco è fermo e quelle dopo sono
/// irraggiungibili — riprovarle più tardi può bastare.
#[test]
fn la_scadenza_taglia_il_blocco_a_meta() {
    let cartella = tempfile::tempdir().expect("cartella");
    let mut riga = 0;
    let c = rymembra_app::db::open_in_memory()
        .expect("database")
        .connection;
    for nome in ["prima.flac", "lento.flac", "dopo.flac"] {
        let percorso = cartella.path().join(nome);
        std::fs::write(&percorso, b"abc").expect("file");
        riga += 1;
        riga_in_tabella(&c, riga, Some(&percorso.to_string_lossy()));
    }
    let lette = leggi_righe(&c, &[1, 2, 3]).expect("lette");
    let letture = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let inizio = std::time::Instant::now();
    let esiti = classifica(
        lette,
        std::time::Duration::from_millis(300),
        disco_finto(letture),
    );
    assert!(
        inizio.elapsed() < std::time::Duration::from_millis(1500),
        "la scadenza è del blocco, non della voce lenta"
    );
    assert_eq!(
        esiti_in_breve(&esiti),
        [
            Ok(1),
            Err(Mancanza::Irraggiungibile),
            Err(Mancanza::Irraggiungibile),
        ]
    );
}

/// Un brano di catalogo non tocca il disco, nemmeno quando il disco è
/// fermo: esce subito, col suo riferimento.
#[test]
fn un_brano_di_catalogo_non_tocca_il_disco() {
    let cartella = tempfile::tempdir().expect("cartella");
    let lento = cartella.path().join("lento.flac");
    std::fs::write(&lento, b"abc").expect("file");
    let c = rymembra_app::db::open_in_memory()
        .expect("database")
        .connection;
    riga_in_tabella(&c, 1, None);
    riga_in_tabella(&c, 2, Some(&lento.to_string_lossy()));
    let lette = leggi_righe(&c, &[1, 2]).expect("lette");
    let letture = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let esiti = classifica(
        lette,
        std::time::Duration::from_millis(100),
        disco_finto(std::sync::Arc::clone(&letture)),
    );
    let catalogo = esiti
        .first()
        .and_then(|esito| esito.as_ref().ok())
        .expect("il brano di catalogo");
    assert!(catalogo.file.is_none());
    assert_eq!(
        catalogo.riferimento.as_deref(),
        Some("internet-archive:https://archive.org/download/gd77/t01.mp3")
    );
    assert_eq!(
        esiti.get(1).cloned().map(|e| e.err()),
        Some(Some(Mancanza::Irraggiungibile))
    );
    assert_eq!(
        letture.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "solo il file ha letto il disco"
    );
}

/// Il registro dice quali voci sono di questo PC: solo quelle che si
/// aprono da una riga della sua libreria, e nell'ordine chiesto.
#[test]
fn solo_le_voci_di_questo_pc_si_mandano() {
    let voce = |uid: u64, proprietario: Option<&str>, apertura: Apertura| Voce {
        uid,
        proprietario: proprietario.map(str::to_owned),
        scheda: SchedaVoce {
            titolo: String::new(),
            artista: String::new(),
            album: String::new(),
            durata_ms: 1,
            numero: None,
            anno: None,
            copertina: None,
            replaygain_db: None,
            picco: None,
        },
        apertura,
        da_contare: None,
    };
    let mut registro = ospiti::Registro::default();
    registro.registra(voce(1, None, Apertura::Libreria { track_id: 40 }));
    registro.registra(voce(
        2,
        Some("PC di Marco"),
        Apertura::Catalogo {
            riferimento: String::new(),
        },
    ));
    registro.registra(voce(
        3,
        None,
        Apertura::Catalogo {
            riferimento: String::new(),
        },
    ));
    assert_eq!(
        righe_dei_uid(&registro, &[2, 1, 99, UID_CALIBRAZIONE, 3, 1]),
        [
            (2, None),
            (1, Some(40)),
            (99, None),
            (UID_CALIBRAZIONE, None),
            (3, None),
            (1, Some(40)),
        ]
    );
}

/// La gara d'ingresso: la strada ha già creato le voci, ma il lettore non
/// ospita ancora. Le maniglie non entrano nella coda dell'utente: entrano i
/// brani chiesti, e le voci si dimenticano.
#[test]
fn le_maniglie_non_entrano_in_una_coda_che_non_e_di_sessione() {
    let righe = vec![10, 11];
    let voci = Some(vec![-1, -2]);
    assert_eq!(
        brani_da_mettere(false, righe.clone(), voci.clone()),
        (vec![10, 11], vec![-1, -2])
    );
    assert_eq!(
        brani_da_mettere(true, righe.clone(), voci),
        (vec![-1, -2], vec![])
    );
    assert_eq!(
        brani_da_mettere(false, righe.clone(), None),
        (righe, vec![])
    );
}

/// `suona` di chi ospita: l'indice si ricalcola sulla voce del brano
/// scelto, e un brano che non è diventato una voce non tocca la coda.
#[test]
fn suona_ritrova_il_brano_scelto_fra_le_voci() {
    let ok = |esito: (Partenza, Vec<i64>)| (esito.0.ok(), esito.1);
    // La riga 11 non c'è più: le voci sono una di meno, e il brano scelto
    // — la riga 12 — è la seconda voce, non la terza.
    let voci = (vec![-1, -3], vec![Some(10), Some(12)]);
    assert_eq!(
        ok(lista_da_suonare(true, vec![10, 11, 12], 2, Some(voci))),
        (Some((vec![-1, -3], 1)), vec![])
    );
    // Lo stesso brano due volte: la stessa occorrenza.
    let voci = (vec![-1, -2, -3], vec![Some(10), Some(11), Some(10)]);
    assert_eq!(
        ok(lista_da_suonare(true, vec![10, 11, 10], 2, Some(voci))),
        (Some((vec![-1, -2, -3], 2)), vec![])
    );
    // Il brano scelto non è diventato una voce: rifiuto, e le voci create
    // si dimenticano.
    let voci = (vec![-1], vec![Some(10)]);
    let (esito, da_dimenticare) = lista_da_suonare(true, vec![10, 11], 1, Some(voci));
    assert_eq!(
        esito.err().map(|e| e.code().kind()),
        Some(rymembra_domain::errors::ErrorCodeKind::LibraryTrackNotFound)
    );
    assert_eq!(da_dimenticare, vec![-1]);
    // Nessuna voce: rifiuto, non una coda vuota.
    let (esito, _) = lista_da_suonare(true, vec![10], 0, Some((vec![], vec![])));
    assert!(esito.is_err());
    // Il lettore non ospita (la gara d'ingresso): le righe, e l'indice
    // chiesto.
    let voci = (vec![-1], vec![Some(10)]);
    assert_eq!(
        ok(lista_da_suonare(false, vec![10, 11], 1, Some(voci))),
        (Some((vec![10, 11], 1)), vec![-1])
    );
}

/// La coda dell'utente messa da parte entrando non porta maniglie: quelle
/// di una gara d'ingresso tornerebbero all'uscita come voci sparite, e la
/// coda non si salverebbe più.
#[test]
fn la_coda_di_prima_si_ripulisce_dalle_maniglie() {
    let sporca = QueueSnapshot {
        tracks: vec![10, -3, 11, -4],
        order: vec![0, 1, 2, 3],
        order_pos: Some(2),
        shuffle: false,
        repeat: rymembra_domain::queue::RepeatMode::All,
    };
    let pulita = ripulita(sporca);
    assert_eq!(pulita.tracks, vec![10, 11]);
    assert_eq!(
        Queue::restore(pulita.clone()).current(),
        Some(11),
        "il cursore resta sul brano dell'utente"
    );
    assert_eq!(pulita.repeat, rymembra_domain::queue::RepeatMode::All);
    // Una coda pulita resta com'è.
    let gia = QueueSnapshot {
        tracks: vec![1, 2],
        order: vec![1, 0],
        order_pos: Some(0),
        shuffle: true,
        repeat: rymembra_domain::queue::RepeatMode::Off,
    };
    assert_eq!(ripulita(gia.clone()), gia);
}

/// I percorsi del ponte che aprono fuori dai lucchetti ricontrollano tutto
/// alla consegna. Le righe che lo fanno, cercate nel sorgente: senza una
/// di loro le prove delle regole passerebbero lo stesso.
#[test]
fn le_consegne_del_ponte_ricontrollano_quel_che_e_cambiato() {
    let ponte = include_str!("../insieme.rs");
    let ponte = ponte.split("\n#[cfg(test)]").next().unwrap_or(ponte);
    let corpo = |firma: &str| {
        let da = ponte.find(firma).expect(firma);
        let resto = &ponte[da..];
        &resto[..resto.find("\n}\n").unwrap_or(resto.len())]
    };
    let ingresso = corpo("pub fn entra_come_host(");
    assert!(ingresso.contains("lettore.motore.formato() != foto.formato"));
    assert!(ingresso.contains("istantanea: ripulita(foto.istantanea.clone())"));
    assert!(ingresso.contains("dimentica_voci(app, &maniglie);"));
    let riapri = corpo("fn riapri(");
    assert!(riapri.contains("registro.ancora_viva(maniglia, generazione)"));
    let apri = riapri
        .find("lettore.apri_fermo(brano, da_ms);")
        .expect("apri_fermo");
    let continua = riapri
        .find("riapertura::continua_l_ascolto(maniglia);")
        .expect("l'ascolto continua");
    assert!(apri < continua, "il segno dell'ascolto va dopo l'apertura");
    let esci = corpo("pub fn esci(");
    let se = esci.find("if uscito {").expect("solo se si era dentro");
    let segni = esci.find("azzera_i_segni(&stato);").expect("i segni");
    assert!(
        se < segni,
        "i segni dell'utente si toccano solo uscendo davvero"
    );
}

#[test]
fn la_finestra_tiene_il_brano_che_suona_e_quel_che_viene_dopo() {
    let ordine: Vec<i64> = (1..=5000).collect();
    // Al centro: un decimo prima, il resto dopo.
    let (pezzo, posto) = finestra(&ordine, Some(2000), 1000);
    assert_eq!(pezzo.len(), 1000);
    assert_eq!(posto, Some(100));
    assert_eq!(pezzo.get(100).copied(), Some(2001));
    // In fondo: la finestra si appoggia alla fine e tiene tutto il tetto.
    let (pezzo, posto) = finestra(&ordine, Some(4990), 1000);
    assert_eq!(pezzo.len(), 1000);
    assert_eq!(pezzo.last().copied(), Some(5000));
    assert_eq!(posto.and_then(|p| pezzo.get(p).copied()), Some(4991));
    // In testa, e senza un corrente.
    let (pezzo, posto) = finestra(&ordine, Some(0), 1000);
    assert_eq!((pezzo.first().copied(), posto), (Some(1), Some(0)));
    let (pezzo, posto) = finestra(&ordine, None, 1000);
    assert_eq!((pezzo.len(), posto), (1000, None));
    // Sotto il tetto non si taglia niente.
    let corta = vec![7, 8, 9];
    assert_eq!(finestra(&corta, Some(2), 1000), (corta.clone(), Some(2)));
}

#[test]
fn un_tratto_di_cd_va_e_torna_in_millisecondi() {
    for cd in [0_u32, 1, 74, 75, 13_337] {
        let ms = ms_da_cd(cd);
        // Un fotogramma CD è 13,3 ms: l'arrotondamento resta sotto il mezzo.
        let esatto = f64::from(cd) * 1000.0 / 75.0;
        assert!((ms as f64 - esatto).abs() <= 0.5, "{cd} → {ms}");
    }
}

#[test]
fn la_scheda_di_una_riga_porta_il_livello_del_brano() {
    let riga = TrackSummary {
        id: 9,
        path: Some(r"C:\Musica\a.flac".to_owned()),
        title: "Titolo".to_owned(),
        artist: "Artista".to_owned(),
        album: "Album".to_owned(),
        album_key: Some("k".to_owned()),
        track_number: Some(4),
        disc_number: Some(1),
        duration_ms: 200_000,
        year: Some(1999),
        cover_art_hash: Some("ab12".to_owned()),
        play_count: 3,
        liked: true,
        rating: 5,
        fonte: None,
        fonte_pagina: None,
        licenza: None,
    };
    let livello = SchedaSorgente {
        track_id: 9,
        collocazione: Collocazione::File(r"C:\Musica\a.flac".to_owned()),
        durata_ms: 200_000,
        replaygain_db: Some(-7.25),
        picco: Some(0.98),
    };
    let scheda = scheda_da_riga(&riga, Some(&livello));
    assert_eq!(scheda.titolo, "Titolo");
    assert_eq!(scheda.durata_ms, 200_000);
    assert_eq!(scheda.numero, Some(4));
    assert_eq!(scheda.anno, Some(1999));
    assert_eq!(scheda.copertina.as_deref(), Some("ab12"));
    assert!(
        scheda
            .replaygain_db
            .is_some_and(|db| (db + 7.25).abs() < 1e-3)
    );
    assert!(scheda_da_riga(&riga, None).replaygain_db.is_none());
}

/// Un'aggiunta di un membro a coda piena: la fascia rossa non va a chi
/// ospita, che non ha fatto niente (il «no» va al membro, vedi
/// `insieme::filo::ospita::esito_dell_aggiunta`), e le voci rimaste fuori escono
/// dal registro sotto lo stesso lucchetto che le ha lasciate fuori.
#[test]
fn l_aggiunta_di_un_membro_a_coda_piena_non_avvisa_chi_ospita() {
    let ponte = include_str!("../insieme.rs");
    let ponte = ponte.split("\n#[cfg(test)]").next().unwrap_or(ponte);
    let da = ponte
        .find("pub fn accoda_maniglie(")
        .expect("accoda_maniglie");
    let resto = &ponte[da..];
    let corpo = &resto[..resto.find("\n}\n").unwrap_or(resto.len())];
    assert!(
        !corpo.contains("entrano(app"),
        "l'avviso di coda piena andrebbe alla finestra di chi ospita"
    );
    assert!(
        corpo.contains("dimentica_voci(app"),
        "le voci rimaste fuori resterebbero nel registro"
    );
}

/// Chi segue: lo specchio sostituisce la coda a ogni stato dell'host, e
/// le voci che ne escono escono anche dal registro — dopo che la coda è
/// stata sostituita, così la coda non nomina mai una maniglia tolta.
#[test]
fn lo_specchio_toglie_dal_registro_le_voci_uscite() {
    let ponte = include_str!("../insieme.rs");
    let ponte = ponte.split("\n#[cfg(test)]").next().unwrap_or(ponte);
    let da = ponte.find("pub fn specchia(").expect("specchia");
    let resto = &ponte[da..];
    let corpo = &resto[..resto.find("\n}\n").unwrap_or(resto.len())];
    let sostituita = corpo
        .find("lettore.coda = Queue::restore(nuova);")
        .expect("la coda nuova");
    let dimenticate = corpo.find("dimentica_le_uscite(");
    assert!(
        dimenticate.is_some_and(|d| d > sostituita),
        "le voci uscite dallo specchio restano nel registro fino all'uscita"
    );
}

fn voce_finta(uid: u64) -> Voce {
    Voce {
        uid,
        proprietario: None,
        scheda: SchedaVoce {
            titolo: format!("voce {uid}"),
            artista: String::new(),
            album: String::new(),
            durata_ms: 1_000,
            numero: None,
            anno: None,
            copertina: None,
            replaygain_db: None,
            picco: None,
        },
        apertura: Apertura::Catalogo {
            riferimento: format!("catalogo:{uid}"),
        },
        da_contare: None,
    }
}

#[test]
fn due_stati_senza_voci_in_comune_lasciano_nel_registro_solo_la_seconda_coda() {
    let mut registro = ospiti::Registro::default();
    let prima: Vec<i64> = (1..=3)
        .map(|uid| registro.registra(voce_finta(uid)))
        .collect();
    // Il secondo stato arriva già registrato (lo specchio registra prima
    // di sostituire la coda); il motore ha ancora in mano la prima voce.
    let dopo: Vec<i64> = (4..=6)
        .map(|uid| registro.registra(voce_finta(uid)))
        .collect();
    let in_mano = prima.first().copied().into_iter().collect::<Vec<_>>();
    registro.togli(&uscite_dalla_coda(&prima, &dopo, &in_mano));
    assert_eq!(registro.quante(), dopo.len() + 1);
    assert!(
        prima
            .get(1..)
            .unwrap()
            .iter()
            .all(|m| registro.voce(*m).is_none())
    );
    // Una voce registrata e non ancora in coda (un'aggiunta in volo) non
    // è nella coda di prima: non si tocca.
    let in_volo = registro.registra(voce_finta(7));
    registro.togli(&uscite_dalla_coda(&dopo, &dopo, &[]));
    assert!(registro.voce(in_volo).is_some());
    // Mai una riga della libreria, mai la calibrazione, ognuna una volta.
    assert_eq!(
        uscite_dalla_coda(&[5, MANIGLIA_CALIBRAZIONE, -9, -9, -10], &[-10], &[]),
        vec![-9]
    );
}

#[test]
fn nella_coda_di_sessione_entra_solo_quel_che_sta_sotto_il_tetto() {
    assert_eq!(quante_entrano(true, MASSIMO_VOCI, 1), 0, "piena");
    assert_eq!(quante_entrano(true, 990, 15), 10, "un album a metà");
    assert_eq!(quante_entrano(true, 0, 3), 3);
    assert_eq!(quante_entrano(false, 5_000, 3), 3, "fuori sessione tutti");
    // Un'aggiunta intera di un membro entra sempre in una coda vuota.
    let aggiunta = rymembra_insieme::messaggi::MASSIMO_AGGIUNTE;
    assert_eq!(quante_entrano(true, 0, aggiunta), aggiunta);
}
