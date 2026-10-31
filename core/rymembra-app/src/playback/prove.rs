//! Le prove di quel che la riproduzione lascia sul database.

use super::*;
use rymembra_domain::listen::Listen;

use super::impostazioni::BERSAGLIO_MAX_DB;
use super::sorgente::{estensione_di, stesso_disco};
use crate::files::MusicFiles;
use rymembra_domain::errors::ErrorCodeKind;
use rymembra_play::{BANDE, LIMITE_DB, Segmento};

fn db() -> Connection {
    crate::db::open_in_memory()
        .expect("database in memoria")
        .connection
}

fn brano(connection: &Connection, id: i64, durata_ms: i64) {
    connection
        .execute(
            "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, 'T', 'A', 'Al', ?4, 1000, 0, 0)",
            rusqlite::params![id, format!("C:/m/{id}.mp3"), format!("k{id}"), durata_ms],
        )
        .expect("brano inserito");
}

fn conteggio(connection: &Connection, id: i64) -> (i64, Option<i64>, i64) {
    connection
        .query_row(
            "SELECT t.play_count, t.last_played_at,
                        (SELECT COUNT(*) FROM play_history WHERE track_id = t.id)
                   FROM tracks t WHERE t.id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("conteggio")
}

#[test]
fn i_file_sciolti_suonati_in_fila_non_fanno_un_disco() {
    // Una cartella di singoli senza tag album: la scansione scrive il
    // segnaposto, e la chiave d'album (titolo più cartella) è la stessa
    // per tutti. Suonati in fila non sono un disco: ognuno tiene il suo
    // livello.
    let c = db();
    let disco = |id: i64, album: &str, cartella: &str| {
        let percorso = format!("C:/m/{cartella}/{id}.mp3");
        c.execute(
            "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, album_key, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, 'T', 'A', ?4, ?5, 180000, 1000, 0, 0)",
            rusqlite::params![
                id,
                percorso,
                format!("k{id}"),
                album,
                rymembra_domain::album::album_group_key(album, &percorso),
            ],
        )
        .expect("brano inserito");
    };
    let segnaposto = rymembra_domain::album::UNKNOWN_ALBUM;
    disco(1, segnaposto, "Scaricati");
    disco(2, segnaposto, "Scaricati");
    disco(3, "Disco", "Disco");
    disco(4, "Disco", "Disco");
    let accanto = |vicino: i64| Intorno {
        mescolato: false,
        prima: Some(vicino),
        dopo: None,
    };
    assert_eq!(stesso_disco(&c, 1, accanto(2)), Ok(false));
    assert_eq!(
        stesso_disco(&c, 3, accanto(4)),
        Ok(true),
        "un disco vero sì"
    );
}

#[test]
fn la_durata_si_ricorda_solo_dove_mancava() {
    let c = db();
    brano(&c, 1, 0);
    brano(&c, 2, 180_000);
    let durata = |id: i64| -> i64 {
        c.query_row("SELECT duration_ms FROM tracks WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .expect("durata")
    };
    // Una riga provvisoria: zero, e il primo ascolto la dice.
    assert_eq!(ricorda_durata(&c, 1, 241_500), Ok(true));
    assert_eq!(durata(1), 241_500);
    // La seconda volta non si riscrive: c'è.
    assert_eq!(ricorda_durata(&c, 1, 100_000), Ok(false));
    assert_eq!(durata(1), 241_500);
    // Una durata che viene dal file non la tocca nessuno.
    assert_eq!(ricorda_durata(&c, 2, 90_000), Ok(false));
    assert_eq!(durata(2), 180_000);
    // Zero non dice niente; un brano che non c'è non è un guasto; una
    // maniglia di «Ascolta insieme» non è una riga.
    brano(&c, 3, 0);
    assert_eq!(ricorda_durata(&c, 3, 0), Ok(false));
    assert_eq!(durata(3), 0);
    assert_eq!(ricorda_durata(&c, 99, 1_000), Ok(false));
    assert!(ricorda_durata(&c, -4, 1_000).is_err());
}

#[test]
fn un_ascolto_che_conta_scrive_tutte_e_tre_le_cose() {
    let mut c = db();
    brano(&c, 1, 180_000);
    let scritto = record_play(
        &mut c,
        &Listen {
            track_id: 1,
            started_at: 1_700_000_000_000,
            listened_ms: 120_000,
            counts: true,
        },
        "prova",
    )
    .expect("registrato");
    assert!(scritto);
    assert_eq!(conteggio(&c, 1), (1, Some(1_700_000_000_000), 1));
}

#[test]
fn la_posizione_vale_solo_per_il_brano_di_cui_parla() {
    let c = db();
    assert_eq!(load_posizione(&c, 1).expect("letta"), 0);
    save_posizione(&c, 1, 95_000).expect("scritta");
    assert_eq!(load_posizione(&c, 1).expect("letta"), 95_000);
    // Il brano dopo, nei cinque secondi prima del segno nuovo.
    assert_eq!(load_posizione(&c, 2).expect("letta"), 0);
    // Il numero nudo delle versioni di prima vale per chi lo chiede.
    crate::settings::write_json(&c, CHIAVE_POSIZIONE, &29_000u64).expect("vecchio");
    assert_eq!(load_posizione(&c, 7).expect("letta"), 29_000);
}

#[test]
fn un_brano_saltato_non_lascia_traccia() {
    // È la correzione al difetto del vecchio albero, e questa prova è
    // l'unica cosa che la tiene in piedi.
    let mut c = db();
    brano(&c, 1, 180_000);
    let scritto = record_play(
        &mut c,
        &Listen {
            track_id: 1,
            started_at: 1_700_000_000_000,
            listened_ms: 2_000,
            counts: false,
        },
        "prova",
    )
    .expect("registrato");
    assert!(!scritto);
    assert_eq!(conteggio(&c, 1), (0, None, 0));
}

#[test]
fn due_ascolti_contano_due_volte() {
    let mut c = db();
    brano(&c, 1, 180_000);
    for istante in [1_000i64, 2_000] {
        record_play(
            &mut c,
            &Listen {
                track_id: 1,
                started_at: istante,
                listened_ms: 120_000,
                counts: true,
            },
            "prova",
        )
        .expect("registrato");
    }
    assert_eq!(conteggio(&c, 1), (2, Some(2_000), 2));
}

#[test]
fn l_ascolto_aggiorna_anche_l_orologio_delle_statistiche() {
    let mut c = db();
    brano(&c, 1, 180_000);
    record_play(
        &mut c,
        &Listen {
            track_id: 1,
            started_at: 555,
            listened_ms: 120_000,
            counts: true,
        },
        "prova",
    )
    .expect("registrato");
    let stats: i64 = c
        .query_row(
            "SELECT stats_updated_at FROM tracks WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .expect("stats");
    assert_eq!(stats, 555, "senza, la sincronia perderebbe questo ascolto");
}

#[test]
fn la_coda_torna_com_era() {
    let c = db();
    let coda = QueueSnapshot {
        tracks: vec![3, 1, 2],
        order: vec![2, 0, 1],
        order_pos: Some(1),
        shuffle: true,
        repeat: RepeatMode::All,
    };
    save_queue(&c, &coda).expect("salvata");
    assert_eq!(load_queue(&c).expect("riletta"), coda);
}

#[test]
fn nessuna_coda_conservata_da_una_coda_vuota() {
    let c = db();
    assert_eq!(load_queue(&c).expect("riletta"), QueueSnapshot::default());
}

#[test]
fn una_coda_illeggibile_non_impedisce_l_avvio() {
    let c = db();
    crate::settings::write(&c, CHIAVE_CODA, "{non è json").expect("scritta");
    assert_eq!(load_queue(&c).expect("riletta"), QueueSnapshot::default());
}

#[test]
fn il_volume_torna_com_era_e_si_taglia() {
    let c = db();
    save_volume(
        &c,
        Volume {
            volume: 0.35,
            muto: true,
        },
    )
    .expect("salvato");
    let riletto = load_volume(&c).expect("riletto");
    assert!((riletto.volume - 0.35).abs() < f32::EPSILON);
    assert!(riletto.muto);

    // Un valore assurdo scritto a mano non deve poter spaccare le orecchie.
    crate::settings::write(&c, CHIAVE_VOLUME, r#"{"volume":9.0,"muto":false}"#).expect("scritto");
    assert!((load_volume(&c).expect("riletto").volume - 1.0).abs() < f32::EPSILON);
}

#[test]
fn il_volume_della_2_4_suona_uguale_con_la_curva_nuova() {
    // Ieri 0,512 d'ampiezza; oggi la posizione che, al cubo, dà la stessa.
    let c = db();
    crate::settings::write(&c, CHIAVE_VOLUME, r#"{"volume":0.512,"muto":false}"#).expect("scritto");
    let letto = load_volume(&c).expect("riletto");
    assert!(
        (letto.volume - 0.8).abs() < 1e-5,
        "posizione {}",
        letto.volume
    );
    assert!(
        (rymembra_play::volume_uscita(letto.volume, false) - 0.512).abs() < 1e-5,
        "non suona come prima"
    );
    // Salvato una volta, resta dov'è: la conversione non si ripete.
    save_volume(&c, letto).expect("salvato");
    let riletto = load_volume(&c).expect("riletto");
    assert!((riletto.volume - 0.8).abs() < 1e-5);
}

#[test]
fn un_volume_mai_impostato_non_e_al_massimo() {
    let c = db();
    assert!(load_volume(&c).expect("riletto").volume < 1.0);
}

#[test]
fn la_curva_torna_com_era() {
    let c = db();
    let curva = Equalizzazione {
        attivo: true,
        guadagni: vec![3.0, -2.0, 0.0, 1.5, 0.0, 0.0, -4.0, 0.0, 6.0, 0.0],
    };
    save_eq(&c, &curva).expect("salvata");
    assert_eq!(load_eq(&c).expect("riletta"), curva);
}

#[test]
fn una_curva_mai_impostata_e_spenta_e_piatta() {
    let c = db();
    let curva = load_eq(&c).expect("riletta");
    assert!(
        !curva.attivo,
        "non deve equalizzare senza che glielo si chieda"
    );
    assert_eq!(curva.guadagni, vec![0.0; BANDE]);
}

#[test]
fn una_curva_assurda_scritta_a_mano_si_taglia() {
    // La stessa cautela del volume: il file si può aprire e correggere, e un
    // +90 dB su una banda non deve poter spaccare le orecchie.
    let c = db();
    crate::settings::write(
        &c,
        CHIAVE_EQ,
        r#"{"attivo":true,"guadagni":[90.0,-90.0,0,0,0,0,0,0,0,0]}"#,
    )
    .expect("scritta");
    let letta = load_eq(&c).expect("riletta");
    assert_eq!(letta.guadagni.first().copied(), Some(LIMITE_DB));
    assert_eq!(letta.guadagni.get(1).copied(), Some(-LIMITE_DB));
}

#[test]
fn una_curva_di_lunghezza_sbagliata_si_normalizza() {
    // Il caso vero: un `player.eq` scritto da una versione con un numero di
    // bande diverso. Deve valere adattabile, non illeggibile.
    let c = db();
    crate::settings::write(&c, CHIAVE_EQ, r#"{"attivo":true,"guadagni":[4.0,5.0]}"#)
        .expect("scritta");
    let letta = load_eq(&c).expect("riletta");
    assert!(letta.attivo, "la curva era leggibile: non va buttata");
    assert_eq!(letta.guadagni.len(), BANDE);
    assert_eq!(letta.guadagni.first().copied(), Some(4.0));
    assert_eq!(letta.guadagni.last().copied(), Some(0.0));
}

#[test]
fn una_curva_illeggibile_non_impedisce_l_avvio() {
    let c = db();
    crate::settings::write(&c, CHIAVE_EQ, "{non è json").expect("scritta");
    assert_eq!(load_eq(&c).expect("riletta"), Equalizzazione::default());
}

#[test]
fn la_normalizzazione_mai_impostata_e_accesa() {
    // Il valore di serie non è una preferenza: è quel che il motore fa da
    // sempre. Se questo diventasse `false`, il primo avvio dopo
    // l'aggiornamento cambierebbe il volume di chi ha i tag senza dirglielo.
    let c = db();
    let letta = load_replaygain(&c).expect("riletta");
    assert!(letta.attivo);
    assert_eq!(letta.bersaglio_db, BERSAGLIO_PREDEFINITO_DB);
}

#[test]
fn l_uscita_mai_scelta_e_quella_di_sistema() {
    let c = db();
    assert_eq!(load_uscita(&c).expect("riletta"), None);
}

#[test]
fn l_uscita_scelta_torna_com_era() {
    let c = db();
    save_uscita(&c, Some("FiiO K11")).expect("scritta");
    assert_eq!(
        load_uscita(&c).expect("riletta"),
        Some("FiiO K11".to_owned())
    );
    // E si torna indietro: «predefinito di sistema» è una scelta, non
    // l'assenza di una scelta, e deve poter cancellare quella di prima.
    save_uscita(&c, None).expect("scritta");
    assert_eq!(load_uscita(&c).expect("riletta"), None);
}

#[test]
fn un_uscita_illeggibile_non_impedisce_l_avvio() {
    // Il caso vero: un database ripristinato da un'altra macchina, o una
    // versione futura che scrive qui dentro qualcos'altro. Muti si resta
    // solo se non c'è nessuna scheda, mai per una preferenza storta.
    let c = db();
    crate::settings::write(&c, CHIAVE_USCITA, "{non è json").expect("scritta");
    assert_eq!(load_uscita(&c).expect("riletta"), None);
}

#[test]
fn le_barre_dello_spettro_mai_scelte_sono_quelle_di_serie() {
    let c = db();
    assert_eq!(
        load_spettro_bande(&c).expect("riletta"),
        rymembra_play::RISOLUZIONE_DI_SERIE
    );
}

#[test]
fn le_barre_dello_spettro_restano_scritte() {
    let c = db();
    save_spettro_bande(&c, 256).expect("scritta");
    assert_eq!(load_spettro_bande(&c).expect("riletta"), 256);
}

#[test]
fn un_numero_di_barre_che_non_esiste_diventa_il_piu_vicino() {
    // Il caso vero: una preferenza scritta da una versione con un altro
    // elenco di risoluzioni. Deve valere adattabile, non illeggibile — la
    // stessa regola della curva dell'equalizzatore.
    let c = db();
    crate::settings::write(&c, CHIAVE_SPETTRO_BANDE, "100").expect("scritta");
    assert_eq!(load_spettro_bande(&c).expect("riletta"), 128);
    crate::settings::write(&c, CHIAVE_SPETTRO_BANDE, "3").expect("scritta");
    assert_eq!(load_spettro_bande(&c).expect("riletta"), 8);
    crate::settings::write(&c, CHIAVE_SPETTRO_BANDE, "60000").expect("scritta");
    assert_eq!(load_spettro_bande(&c).expect("riletta"), 1024);
}

#[test]
fn barre_illeggibili_non_impediscono_l_avvio() {
    let c = db();
    crate::settings::write(&c, CHIAVE_SPETTRO_BANDE, "{non è json").expect("scritta");
    assert_eq!(
        load_spettro_bande(&c).expect("riletta"),
        rymembra_play::RISOLUZIONE_DI_SERIE
    );
}

#[test]
fn uno_spettro_mai_scelto_e_spento() {
    // Chi apre «In riproduzione» per la prima volta è venuto a guardare la
    // copertina, e una scena WebGL accesa di serie è una ventola accesa di
    // serie.
    let c = db();
    assert!(!load_spettro_visibile(&c).expect("riletta"));
}

#[test]
fn uno_spettro_acceso_resta_acceso() {
    // Il difetto vero che questa prova impedisce: un `unwrap_or(false)`
    // messo sul ramo sbagliato si mangia un `true` scritto davvero, e la
    // preferenza sembra non salvarsi mai — senza nessun errore, perché
    // «spento» è anche il valore di serie.
    let c = db();
    save_spettro_visibile(&c, true).expect("scritta");
    assert!(load_spettro_visibile(&c).expect("riletta"));
    save_spettro_visibile(&c, false).expect("scritta");
    assert!(!load_spettro_visibile(&c).expect("riletta"));
}

#[test]
fn una_visibilita_illeggibile_lascia_la_scena_com_era() {
    // Un database ripristinato da un'altra macchina, o una versione futura
    // che scrive qui dentro qualcos'altro: si resta spenti, che è il caso
    // conservativo — accendere una GPU su un dato storto è il difetto.
    let c = db();
    crate::settings::write(&c, CHIAVE_SPETTRO_VISIBILE, "{non è json").expect("scritta");
    assert!(!load_spettro_visibile(&c).expect("riletta"));
}

#[test]
fn il_formato_parte_acceso() {
    // L'opposto dello spettro, e la prova esiste per tenere l'asimmetria
    // deliberata invece che accidentale: due chiavi vicine con due valori
    // di serie diversi sono esattamente il posto in cui un
    // copia-e-incolla mette `unwrap_or(false)` anche qui.
    let c = db();
    assert!(load_formato_visibile(&c).expect("riletta"));
}

#[test]
fn il_formato_scelto_si_rilegge() {
    // Spento **e** riacceso: con il valore di serie acceso, una prova che
    // scrive solo `true` passerebbe anche se la scrittura non funzionasse
    // affatto. Lo spegnimento è l'unico che distingue le due cose, e la
    // riaccensione verifica che non sia un viaggio senza ritorno.
    let c = db();
    save_formato_visibile(&c, false).expect("scritta");
    assert!(!load_formato_visibile(&c).expect("riletta"));
    save_formato_visibile(&c, true).expect("scritta");
    assert!(load_formato_visibile(&c).expect("riletta"));
}

#[test]
fn un_formato_illeggibile_resta_visibile() {
    // Un valore che non si interpreta vale come assente, e assente qui è
    // acceso: è la regola del modulo, e il caso conservativo è l'opposto di
    // quello dello spettro perché il costo è una riga di testo, non una GPU.
    let c = db();
    crate::settings::write(&c, CHIAVE_FORMATO_VISIBILE, "{non è json").expect("scritta");
    assert!(load_formato_visibile(&c).expect("riletta"));
}

#[test]
fn una_qualita_mai_scelta_e_automatica() {
    let c = db();
    assert_eq!(load_spettro_qualita(&c).expect("riletta"), Qualita::Auto);
}

#[test]
fn una_qualita_che_non_esiste_diventa_automatica() {
    // È il ritaglio che non abbiamo scritto: il `rename_all` fa fallire la
    // deserializzazione di `"ultra"`, `read_json` tratta il malformato come
    // assente, e assente è «lo decide la scena». Se qualcuno togliesse il
    // `rename_all`, o accettasse la stringa grezza, questa prova cade.
    let c = db();
    crate::settings::write(&c, CHIAVE_SPETTRO_QUALITA, r#""ultra""#).expect("scritta");
    assert_eq!(load_spettro_qualita(&c).expect("riletta"), Qualita::Auto);
    crate::settings::write(&c, CHIAVE_SPETTRO_QUALITA, "{non è json").expect("scritta");
    assert_eq!(load_spettro_qualita(&c).expect("riletta"), Qualita::Auto);
    // E il nome giusto con la maiuscola sbagliata è un nome sbagliato.
    crate::settings::write(&c, CHIAVE_SPETTRO_QUALITA, r#""Alta""#).expect("scritta");
    assert_eq!(load_spettro_qualita(&c).expect("riletta"), Qualita::Auto);
}

#[test]
fn i_tre_livelli_fanno_andata_e_ritorno() {
    let c = db();
    for livello in [Qualita::Auto, Qualita::Alta, Qualita::Bassa] {
        save_spettro_qualita(&c, livello).expect("scritta");
        assert_eq!(load_spettro_qualita(&c).expect("riletta"), livello);
    }
}

#[test]
fn il_nome_sul_filo_e_il_nome_nel_database() {
    // Due elenchi di nomi per la stessa enumerazione: quello di serde, che
    // finisce nel database, e quello di `da_nome`, che arriva dalla
    // finestra. Il giorno in cui divergono, la preferenza di ieri diventa
    // illeggibile oggi — e in silenzio, perché il ripiego è `Auto`.
    for livello in [Qualita::Auto, Qualita::Alta, Qualita::Bassa] {
        let scritto = serde_json::to_string(&livello).expect("serializzata");
        let nome = scritto.trim_matches('"');
        assert_eq!(
            Qualita::da_nome(nome),
            livello,
            "serde scrive «{nome}» e `da_nome` non lo riconosce"
        );
    }
    assert_eq!(Qualita::da_nome("ultra"), Qualita::Auto);
    assert_eq!(Qualita::da_nome(""), Qualita::Auto);
}

#[test]
fn una_dissolvenza_mai_scelta_e_spenta() {
    let c = db();
    assert_eq!(load_crossfade(&c).expect("riletta"), 0);
}

#[test]
fn la_dissolvenza_resta_scritta() {
    let c = db();
    save_crossfade(&c, 8).expect("scritta");
    assert_eq!(load_crossfade(&c).expect("riletta"), 8);
}

#[test]
fn una_dissolvenza_piu_lunga_del_massimo_si_taglia() {
    let c = db();
    save_crossfade(&c, 60).expect("scritta");
    assert_eq!(load_crossfade(&c).expect("riletta"), CROSSFADE_MASSIMO_S);
}

#[test]
fn una_dissolvenza_scritta_a_mano_fuori_scala_si_taglia_in_lettura() {
    let c = db();
    crate::settings::write(&c, CHIAVE_CROSSFADE, "600").expect("scritta");
    assert_eq!(load_crossfade(&c).expect("riletta"), CROSSFADE_MASSIMO_S);
}

#[test]
fn una_dissolvenza_illeggibile_non_impedisce_l_avvio() {
    let c = db();
    crate::settings::write(&c, CHIAVE_CROSSFADE, "{non è json").expect("scritta");
    assert_eq!(load_crossfade(&c).expect("riletta"), 0);
}

#[test]
fn una_latenza_mai_scelta_e_zero() {
    let c = db();
    assert_eq!(load_latenza(&c).expect("riletta"), 0);
}

#[test]
fn la_latenza_resta_scritta_nei_due_versi() {
    let c = db();
    save_latenza(&c, 120).expect("scritta");
    assert_eq!(load_latenza(&c).expect("riletta"), 120);
    // Il verso negativo non è un caso limite: è la correzione di chi trova
    // che i testi arrivino tardi comunque.
    save_latenza(&c, -80).expect("scritta");
    assert_eq!(load_latenza(&c).expect("riletta"), -80);
}

#[test]
fn una_latenza_fuori_scala_si_taglia_dai_due_lati() {
    let c = db();
    save_latenza(&c, 5_000).expect("scritta");
    assert_eq!(load_latenza(&c).expect("riletta"), LATENZA_MASSIMA_MS);
    save_latenza(&c, -5_000).expect("scritta");
    assert_eq!(load_latenza(&c).expect("riletta"), -LATENZA_MASSIMA_MS);
}

#[test]
fn una_latenza_scritta_a_mano_fuori_scala_si_taglia_in_lettura() {
    // Questa riga può arrivare da un backup o da un database ritoccato a
    // mano: un'ora di compensazione non deve poter entrare nel motore.
    let c = db();
    crate::settings::write(&c, CHIAVE_LATENZA, "3600000").expect("scritta");
    assert_eq!(load_latenza(&c).expect("riletta"), LATENZA_MASSIMA_MS);
}

#[test]
fn una_latenza_illeggibile_non_impedisce_l_avvio() {
    let c = db();
    crate::settings::write(&c, CHIAVE_LATENZA, "{non è json").expect("scritta");
    assert_eq!(load_latenza(&c).expect("riletta"), 0);
}

// ── la latenza per uscita ──

fn voce(ms: i64, fonte: FonteLatenza) -> LatenzaUscita {
    LatenzaUscita {
        ms,
        fonte,
        verificata: true,
        quando_ms: 1_700_000_000_000,
    }
}

#[test]
fn un_uscita_senza_numero_suo_usa_la_globale() {
    let mut mappa = LatenzePerUscita::new();
    mappa.insert("Cuffie BT".to_owned(), voce(180, FonteLatenza::Microfono));
    // L'uscita con la sua voce usa la sua, anche quando vale zero.
    assert_eq!(latenza_effettiva(40, &mappa, Some("Cuffie BT")), 180);
    mappa.insert("DAC".to_owned(), voce(0, FonteLatenza::Mano));
    assert_eq!(latenza_effettiva(40, &mappa, Some("DAC")), 0);
    // Le altre, e il motore che non c'è, ripiegano sulla globale.
    assert_eq!(latenza_effettiva(40, &mappa, Some("Altoparlanti")), 40);
    assert_eq!(latenza_effettiva(-30, &mappa, None), -30);
    assert_eq!(
        latenza_effettiva(40, &LatenzePerUscita::new(), Some("DAC")),
        40
    );
    // E il limite vale anche qui: un numero arrivato da fuori non passa.
    assert_eq!(
        latenza_effettiva(9_000, &LatenzePerUscita::new(), None),
        LATENZA_MASSIMA_MS
    );
}

#[test]
fn la_mappa_delle_uscite_torna_identica() {
    let c = db();
    assert_eq!(
        load_latenze_per_uscita(&c).expect("riletta"),
        LatenzeLette::default(),
        "mai scritta vale nessuna voce, senza niente da annotare"
    );
    save_latenza_per_uscita(&c, "Cuffie BT", voce(180, FonteLatenza::Microfono)).expect("scritta");
    save_latenza_per_uscita(&c, "Altoparlanti", voce(-20, FonteLatenza::Orecchio))
        .expect("scritta");
    // Riscrivere una voce non tocca l'altra.
    save_latenza_per_uscita(&c, "Cuffie BT", voce(150, FonteLatenza::Mano)).expect("riscritta");

    let mut attesa = LatenzePerUscita::new();
    attesa.insert("Altoparlanti".to_owned(), voce(-20, FonteLatenza::Orecchio));
    attesa.insert("Cuffie BT".to_owned(), voce(150, FonteLatenza::Mano));
    assert_eq!(
        load_latenze_per_uscita(&c).expect("riletta"),
        LatenzeLette {
            voci: attesa,
            illeggibile: false,
            scartate: 0,
        }
    );
    // La forma sul disco è quella promessa: la leggerà anche la finestra.
    let grezzo = crate::settings::read(&c, CHIAVE_LATENZA_PER_USCITA)
        .expect("letta")
        .expect("c'è");
    assert!(grezzo.contains(r#""fonte":"orecchio""#), "{grezzo}");
    assert!(grezzo.contains(r#""quando_ms":1700000000000"#), "{grezzo}");
}

#[test]
fn la_latenza_di_un_uscita_si_taglia_come_la_globale() {
    let c = db();
    save_latenza_per_uscita(&c, "DAC", voce(5_000, FonteLatenza::Mano)).expect("scritta");
    assert_eq!(
        load_latenze_per_uscita(&c)
            .expect("riletta")
            .voci
            .get("DAC")
            .map(|v| v.ms),
        Some(LATENZA_MASSIMA_MS),
        "si taglia in scrittura"
    );
    // E in lettura: la riga può arrivare da un database ritoccato a mano.
    crate::settings::write(
        &c,
        CHIAVE_LATENZA_PER_USCITA,
        r#"{"DAC":{"ms":5000,"fonte":"mano","verificata":false,"quando_ms":0},
               "BT":{"ms":-3600000,"fonte":"microfono","verificata":true,"quando_ms":0}}"#,
    )
    .expect("scritta a mano");
    let lette = load_latenze_per_uscita(&c).expect("riletta");
    assert_eq!(
        lette.voci.get("DAC").map(|v| v.ms),
        Some(LATENZA_MASSIMA_MS)
    );
    assert_eq!(
        lette.voci.get("BT").map(|v| v.ms),
        Some(-LATENZA_MASSIMA_MS)
    );
}

#[test]
fn una_mappa_illeggibile_vale_vuota_e_lo_dice() {
    let c = db();
    crate::settings::write(&c, CHIAVE_LATENZA_PER_USCITA, "{non è json").expect("scritta");
    let lette = load_latenze_per_uscita(&c).expect("non è un errore");
    assert!(lette.voci.is_empty());
    assert!(
        lette.illeggibile,
        "la finestra deve poterlo scrivere nel diario"
    );

    // Un JSON valido ma di un'altra forma è illeggibile allo stesso modo.
    crate::settings::write(&c, CHIAVE_LATENZA_PER_USCITA, "[1,2,3]").expect("scritta");
    assert!(load_latenze_per_uscita(&c).expect("letta").illeggibile);

    // E la prima taratura la ripara invece di lasciarla rotta per sempre.
    save_latenza_per_uscita(&c, "DAC", voce(30, FonteLatenza::Mano)).expect("scritta");
    let riparata = load_latenze_per_uscita(&c).expect("riletta");
    assert!(!riparata.illeggibile);
    assert_eq!(riparata.voci.get("DAC").map(|v| v.ms), Some(30));
}

#[test]
fn una_voce_rotta_non_porta_via_le_altre() {
    // Il caso vero: una `fonte` che questa versione non conosce, scritta
    // da una più nuova. Le tarature delle altre uscite restano.
    let c = db();
    crate::settings::write(
        &c,
        CHIAVE_LATENZA_PER_USCITA,
        r#"{"DAC":{"ms":25,"fonte":"mano","verificata":false,"quando_ms":0},
               "BT":{"ms":200,"fonte":"radar","verificata":true,"quando_ms":0}}"#,
    )
    .expect("scritta");
    let lette = load_latenze_per_uscita(&c).expect("letta");
    assert!(!lette.illeggibile);
    assert_eq!(lette.scartate, 1);
    assert_eq!(lette.voci.len(), 1);
    assert_eq!(lette.voci.get("DAC").map(|v| v.ms), Some(25));
}

#[test]
fn il_cursore_scrive_l_uscita_corrente_e_la_globale() {
    let c = db();
    save_latenza(&c, 40).expect("globale di ieri");
    save_latenza_per_uscita(&c, "Cuffie BT", voce(180, FonteLatenza::Microfono))
        .expect("taratura delle cuffie");

    let (globale, voci) =
        dichiara_latenza(&c, Some("DAC"), 75, 1_800_000_000_000).expect("dichiarata");
    // L'uscita che suona ha il suo numero, scritto come gesto a mano.
    assert_eq!(
        voci.get("DAC"),
        Some(&LatenzaUscita {
            ms: 75,
            fonte: FonteLatenza::Mano,
            verificata: false,
            quando_ms: 1_800_000_000_000,
        })
    );
    assert_eq!(latenza_effettiva(globale, &voci, Some("DAC")), 75);
    // La globale segue il cursore: è il ripiego di chi non ha un numero suo.
    assert_eq!(globale, 75);
    assert_eq!(load_latenza(&c).expect("riletta"), 75);
    assert_eq!(latenza_effettiva(globale, &voci, Some("Altoparlanti")), 75);
    // Le cuffie tarate col microfono non le tocca nessuno.
    assert_eq!(latenza_effettiva(globale, &voci, Some("Cuffie BT")), 180);
    assert_eq!(
        voci.get("Cuffie BT").map(|v| v.fonte),
        Some(FonteLatenza::Microfono)
    );

    // Un valore fuori scala torna tagliato, ed è quel che il cursore mostra.
    let (globale, voci) = dichiara_latenza(&c, Some("DAC"), -9_000, 0).expect("dichiarata");
    assert_eq!(globale, -LATENZA_MASSIMA_MS);
    assert_eq!(
        latenza_effettiva(globale, &voci, Some("DAC")),
        -LATENZA_MASSIMA_MS
    );
}

#[test]
fn senza_motore_il_cursore_scrive_solo_la_globale() {
    let c = db();
    let (globale, voci) = dichiara_latenza(&c, None, 60, 0).expect("dichiarata");
    assert_eq!(globale, 60);
    assert!(voci.is_empty(), "nessun nome da inventare");
}

// ── le guardie: nessun id ≤ 0 sul database ──

#[test]
fn un_ascolto_di_una_voce_di_sessione_non_si_scrive() {
    // Le righe ci sono davvero, con id 0 e negativo: SQLite le accetta se
    // gliele si dà a mano. Così la prova cade se cade la guardia, e non
    // passa per caso perché l'UPDATE non trova niente.
    let mut c = db();
    for id in [0, -5] {
        brano(&c, id, 180_000);
        let esito = record_play(
            &mut c,
            &Listen {
                track_id: id,
                started_at: 1_700_000_000_000,
                listened_ms: 120_000,
                counts: true,
            },
            "prova",
        );
        assert_eq!(
            esito.map_err(|err| err.code().kind()),
            Err(ErrorCodeKind::LibraryTrackNotFound),
            "id {id}"
        );
        assert_eq!(conteggio(&c, id), (0, None, 0), "id {id}");
    }
    let cronologia: i64 = c
        .query_row("SELECT COUNT(*) FROM play_history", [], |r| r.get(0))
        .expect("cronologia");
    assert_eq!(cronologia, 0);
}

#[test]
fn la_posizione_di_una_voce_di_sessione_non_si_scrive() {
    let c = db();
    save_posizione(&c, 7, 42_000).expect("scritta");
    for id in [0, -3] {
        assert_eq!(
            save_posizione(&c, id, 99_000).map_err(|err| err.code().kind()),
            Err(ErrorCodeKind::LibraryTrackNotFound),
            "id {id}"
        );
    }
    // Il segno di prima è ancora lì, intatto.
    assert_eq!(load_posizione(&c, 7).expect("letta"), 42_000);
}

#[test]
fn il_segno_di_prima_non_vale_per_una_voce_di_sessione() {
    // Il numero nudo che scriveva la versione di prima vale per qualunque
    // brano si chieda: è il motivo per cui esiste, ed è il buco. Una voce
    // di sessione partirebbe lì.
    let c = db();
    crate::settings::write(&c, CHIAVE_POSIZIONE, "42000").expect("segno di prima");
    assert_eq!(
        load_posizione(&c, 7).expect("letta"),
        42_000,
        "per una riga vale"
    );
    for id in [0, -3] {
        assert_eq!(load_posizione(&c, id).expect("letta"), 0, "id {id}");
    }
}

#[test]
fn una_coda_sul_disco_con_una_voce_di_sessione_vale_vuota() {
    // Scritta a mano, perché `save_queue` non la scriverebbe: è il caso del
    // database toccato da fuori, e all'avvio quelle voci entrerebbero nel
    // lettore.
    let c = db();
    crate::settings::write(
        &c,
        CHIAVE_CODA,
        r#"{"tracks":[3,-1],"order":[0,1],"order_pos":0,"shuffle":false,"repeat":"off"}"#,
    )
    .expect("coda a mano");
    assert_eq!(load_queue(&c).expect("letta"), QueueSnapshot::default());

    // La stessa forma con sole righe vere si rilegge com'è.
    crate::settings::write(
        &c,
        CHIAVE_CODA,
        r#"{"tracks":[3,1],"order":[0,1],"order_pos":0,"shuffle":false,"repeat":"off"}"#,
    )
    .expect("coda a mano");
    assert_eq!(load_queue(&c).expect("letta").tracks, [3, 1]);
}

#[test]
fn una_coda_con_una_voce_di_sessione_non_si_scrive_affatto() {
    let c = db();
    let mia = QueueSnapshot {
        tracks: vec![3, 1, 2],
        order: vec![0, 1, 2],
        order_pos: Some(1),
        shuffle: false,
        repeat: RepeatMode::Off,
    };
    save_queue(&c, &mia).expect("salvata");

    let rifiutate = [
        // Il corrente è una maniglia.
        QueueSnapshot {
            tracks: vec![-1, -2],
            order: vec![0, 1],
            order_pos: Some(0),
            ..QueueSnapshot::default()
        },
        // Una coda di sessione finita: nessun corrente. È il buco della
        // guardia sul solo corrente — filtrata, diventerebbe una coda vuota
        // scritta sopra quella dell'utente.
        QueueSnapshot {
            tracks: vec![-1, -2, -3],
            order: vec![0, 1, 2],
            order_pos: None,
            ..QueueSnapshot::default()
        },
        // Mista, con il corrente che è una riga vera.
        QueueSnapshot {
            tracks: vec![4, -9],
            order: vec![0, 1],
            order_pos: Some(0),
            ..QueueSnapshot::default()
        },
        // Lo zero non è di nessuno, e si rifiuta con i negativi.
        QueueSnapshot {
            tracks: vec![5, 0],
            order: vec![0, 1],
            order_pos: Some(0),
            ..QueueSnapshot::default()
        },
    ];
    for coda in &rifiutate {
        assert_eq!(
            save_queue(&c, coda).map_err(|err| err.code().kind()),
            Err(ErrorCodeKind::LibraryTrackNotFound),
            "{coda:?}"
        );
        assert_eq!(
            load_queue(&c).expect("riletta"),
            mia,
            "sul disco deve restare la coda dell'utente"
        );
    }
}

#[test]
fn spegnere_la_normalizzazione_resta_scritto() {
    let c = db();
    save_replaygain(
        &c,
        Normalizzazione {
            attivo: false,
            bersaglio_db: -14.0,
            modo: ModoLivello::Album,
        },
    )
    .expect("salvata");
    let letta = load_replaygain(&c).expect("riletta");
    assert!(!letta.attivo);
    assert_eq!(letta.bersaglio_db, -14.0);
    assert_eq!(letta.modo, ModoLivello::Album);
}

#[test]
fn una_normalizzazione_della_2_4_prende_il_modo_di_serie() {
    let c = db();
    crate::settings::write(
        &c,
        CHIAVE_REPLAYGAIN,
        r#"{"attivo":true,"bersaglio_db":-23.0}"#,
    )
    .expect("scritta");
    let letta = load_replaygain(&c).expect("riletta");
    assert_eq!(letta.bersaglio_db, -23.0);
    assert_eq!(letta.modo, ModoLivello::Automatico);
}

#[test]
fn un_bersaglio_assurdo_si_taglia() {
    // Come per la curva: il valore si può scrivere a mano nel database, e
    // `rymembra_play::guadagno` taglierebbe comunque la correzione a ±24 dB —
    // ma in silenzio, lasciando la finestra a mostrare un numero che non è
    // quello che si sente.
    let c = db();
    crate::settings::write(
        &c,
        CHIAVE_REPLAYGAIN,
        r#"{"attivo":true,"bersaglio_db":40.0}"#,
    )
    .expect("scritta");
    assert_eq!(
        load_replaygain(&c).expect("riletta").bersaglio_db,
        BERSAGLIO_MAX_DB
    );
}

#[test]
fn una_normalizzazione_illeggibile_non_impedisce_l_avvio() {
    let c = db();
    crate::settings::write(&c, CHIAVE_REPLAYGAIN, "{non è json").expect("scritta");
    assert_eq!(
        load_replaygain(&c).expect("riletta"),
        Normalizzazione::default()
    );
}

#[test]
fn i_preset_fanno_andata_e_ritorno() {
    let c = db();
    assert!(salva_preset(&c, "Sera", &[2.0; BANDE]).expect("salvato"));
    assert!(salva_preset(&c, "Cuffie", &[-1.0; BANDE]).expect("salvato"));
    let elenco = load_preset_eq(&c).expect("riletti");
    assert_eq!(elenco.len(), 2);
    assert_eq!(elenco.first().map(|p| p.nome.as_str()), Some("Sera"));
}

#[test]
fn salvare_con_un_nome_che_c_e_gia_sostituisce() {
    // Due voci con lo stesso nome sono due voci che l'utente non sa
    // distinguere, e la seconda è quasi sempre la correzione della prima.
    let c = db();
    salva_preset(&c, "Sera", &[2.0; BANDE]).expect("salvato");
    salva_preset(&c, "  sera ", &[5.0; BANDE]).expect("risalvato");
    let elenco = load_preset_eq(&c).expect("riletti");
    assert_eq!(elenco.len(), 1, "il nome era lo stesso a meno di spazi");
    assert_eq!(
        elenco.first().map(|p| p.guadagni.first().copied()),
        Some(Some(5.0))
    );
    // Il nome resta quello scritto per ultimo, ripulito.
    assert_eq!(elenco.first().map(|p| p.nome.as_str()), Some("sera"));
}

#[test]
fn un_preset_senza_nome_non_si_salva() {
    let c = db();
    assert!(!salva_preset(&c, "   ", &[2.0; BANDE]).expect("provato"));
    assert!(load_preset_eq(&c).expect("riletti").is_empty());
}

#[test]
fn cancellare_un_preset_lo_toglie_e_dice_se_c_era() {
    let c = db();
    salva_preset(&c, "Sera", &[2.0; BANDE]).expect("salvato");
    assert!(
        !cancella_preset(&c, "Mattina").expect("provato"),
        "non c'era"
    );
    assert!(cancella_preset(&c, "SERA").expect("cancellato"));
    assert!(load_preset_eq(&c).expect("riletti").is_empty());
}

#[test]
fn un_elenco_di_preset_illeggibile_vale_elenco_vuoto() {
    let c = db();
    crate::settings::write(&c, CHIAVE_EQ_PRESET, "{non è json").expect("scritta");
    assert!(load_preset_eq(&c).expect("riletti").is_empty());
}

#[test]
fn le_estensioni_si_leggono_da_percorsi_di_entrambi_i_sistemi() {
    assert_eq!(estensione_di("C:\\m\\a.MP3").as_deref(), Some("mp3"));
    assert_eq!(estensione_di("/home/a/b.flac").as_deref(), Some("flac"));
    assert_eq!(estensione_di("senza"), None);
    // Una cartella con un punto nel nome non deve diventare un'estensione.
    assert_eq!(estensione_di("C:\\v1.2\\brano"), None);
}

#[test]
fn un_brano_cancellato_da_un_errore_di_sorgente_non_di_database() {
    use rymembra_domain::errors::ErrorCodeKind;
    let c = db();
    let err = sorgente(&c, &crate::files::LocalFiles, 999).expect_err("deve fallire");
    assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackSourceUnavailable);
}

#[test]
fn un_file_che_non_c_e_piu_da_lo_stesso_errore() {
    use rymembra_domain::errors::ErrorCodeKind;
    let c = db();
    brano(&c, 1, 1000);
    let err = sorgente(&c, &crate::files::LocalFiles, 1).expect_err("deve fallire");
    assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackSourceUnavailable);
}

/// Un `MusicFiles` che non apre niente e fallisce sempre allo stesso modo.
///
/// Serve a provare la **traduzione** del guasto, che è tutta la decisione
/// presa qui: con `LocalFiles` non c'è modo di far arrivare un errore di
/// rete senza una share vera.
struct FintiFile(ErrorCode);

impl MusicFiles for FintiFile {
    fn walk(&self, _root: &str) -> Result<crate::files::Camminata, AppError> {
        // Vuota e intera: questo doppio non ha un disco da perdere pezzi.
        Ok(crate::files::Camminata {
            file: Vec::new(),
            completa: true,
        })
    }

    fn open(&self, _path: &str) -> Result<Box<dyn crate::files::ReadSeek + Send + Sync>, AppError> {
        Err(AppError::new(self.0.clone()))
    }
}

#[test]
fn la_rete_giu_attraversa_intera_e_resta_ritentabile() {
    // Il punto: `playback.sourceUnavailable` non è mai ritentabile, quindi
    // impacchettarci dentro una share caduta vorrebbe dire dire a chi ha
    // staccato il cavo che il brano è perso. Deve passare com'è.
    let c = db();
    brano(&c, 1, 1000);
    let files = FintiFile(ErrorCode::FsNetworkUnavailable {
        path: Some("//srv/musica/1.mp3".to_owned()),
    });
    let err = sorgente(&c, &files, 1).expect_err("deve fallire");
    assert_eq!(err.code().kind(), ErrorCodeKind::FsNetworkUnavailable);
    assert!(
        err.code().is_retryable(),
        "la finestra deve offrire «Riprova»"
    );
}

#[test]
fn l_apertura_da_scheda_non_tocca_il_database_e_propaga_la_rete_giu() {
    // La metà lenta, provata da sola: è quella che chi sta sopra manda su un
    // filo con la scadenza addosso, senza nessun lucchetto in mano.
    let files = FintiFile(ErrorCode::FsNetworkUnavailable {
        path: Some("//srv/musica/1.mp3".to_owned()),
    });
    let scheda = SchedaSorgente {
        track_id: 1,
        collocazione: Collocazione::File("//srv/musica/1.mp3".to_owned()),
        durata_ms: 1_000,
        replaygain_db: None,
        picco: None,
    };
    let err = sorgente_da_scheda(&files, &scheda).expect_err("deve fallire");
    assert_eq!(err.code().kind(), ErrorCodeKind::FsNetworkUnavailable);
    assert!(err.code().is_retryable());
}

#[test]
fn ogni_altro_guasto_di_apertura_resta_sorgente_non_disponibile() {
    let c = db();
    brano(&c, 1, 1000);
    let files = FintiFile(ErrorCode::FsNotFound {
        path: "C:/m/1.mp3".to_owned(),
    });
    let err = sorgente(&c, &files, 1).expect_err("deve fallire");
    assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackSourceUnavailable);
}

// ── i brani d'immagine CD ───────────────────────────────────────────────

/// L'immagine di prova: lo stesso file per tutti i brani del disco.
const IMMAGINE: &str = "C:/m/Disco/Album.flac";

/// Il brano `numero` del disco, scritto a mano come lo scrive la scansione:
/// percorso virtuale, immagine, confini in fotogrammi CD, durata del brano.
fn brano_d_immagine(
    connection: &Connection,
    id: i64,
    numero: u8,
    inizio_cd: Option<i64>,
    fine_cd: Option<i64>,
) {
    connection
        .execute(
            "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified,
                      immagine, inizio_cd, fine_cd)
                 VALUES (?1, ?2, ?3, 'T', 'A', 'Al', 200000, 300000000, 0, 0,
                         ?4, ?5, ?6)",
            rusqlite::params![
                id,
                rymembra_domain::cue::percorso_del_brano("C:/m/Disco/Album.cue", numero),
                format!("k{id}"),
                IMMAGINE,
                inizio_cd,
                fine_cd
            ],
        )
        .expect("brano d'immagine inserito");
}

/// Un `MusicFiles` che apre un flusso vuoto e si ricorda che cosa ha aperto.
#[derive(Default)]
struct FileAperti(std::sync::Mutex<Vec<String>>);

impl MusicFiles for FileAperti {
    fn walk(&self, _root: &str) -> Result<crate::files::Camminata, AppError> {
        Ok(crate::files::Camminata {
            file: Vec::new(),
            completa: true,
        })
    }

    fn open(&self, path: &str) -> Result<Box<dyn crate::files::ReadSeek + Send + Sync>, AppError> {
        self.0.lock().expect("elenco").push(path.to_owned());
        Ok(Box::new(std::io::Cursor::new(Vec::<u8>::new())))
    }
}

#[test]
fn un_brano_d_immagine_apre_l_immagine_e_porta_il_suo_pezzo() {
    let c = db();
    brano_d_immagine(&c, 1, 2, Some(15_000), Some(30_000));
    let scheda = scheda_sorgente(&c, 1).expect("scheda");
    assert_eq!(
        scheda.collocazione,
        Collocazione::Segmento {
            immagine: IMMAGINE.to_owned(),
            inizio_cd: 15_000,
            fine_cd: Some(30_000),
        }
    );
    // Accanto al brano non c'è un file suo; l'audio sta nell'immagine.
    assert_eq!(scheda.collocazione.percorso(), None);
    assert_eq!(scheda.collocazione.file_audio(), Some(IMMAGINE));
    assert_eq!(scheda.collocazione.fonte(), None);

    let files = FileAperti::default();
    let sorgente = sorgente_da_scheda(&files, &scheda).expect("sorgente");
    assert_eq!(
        *files.0.lock().expect("elenco"),
        vec![IMMAGINE.to_owned()],
        "si apre l'immagine, mai il percorso virtuale"
    );
    assert_eq!(
        sorgente.segmento,
        Some(Segmento {
            immagine: IMMAGINE.to_owned(),
            inizio_cd: 15_000,
            fine_cd: Some(30_000),
        })
    );
    assert_eq!(sorgente.estensione.as_deref(), Some("flac"));
    assert_eq!(sorgente.durata_ms, 200_000, "la durata del brano");
}

#[test]
fn l_ultimo_brano_d_immagine_va_fino_in_fondo() {
    let c = db();
    brano_d_immagine(&c, 1, 9, Some(150_000), None);
    let files = FileAperti::default();
    let sorgente = sorgente(&c, &files, 1).expect("sorgente");
    assert_eq!(
        sorgente.segmento.map(|s| (s.inizio_cd, s.fine_cd)),
        Some((150_000, None))
    );
}

#[test]
fn un_file_intero_non_porta_un_segmento() {
    let c = db();
    brano(&c, 1, 1000);
    let files = FileAperti::default();
    let sorgente = sorgente(&c, &files, 1).expect("sorgente");
    assert_eq!(sorgente.segmento, None);
    assert_eq!(
        *files.0.lock().expect("elenco"),
        vec!["C:/m/1.mp3".to_owned()]
    );
}

#[test]
fn un_brano_d_immagine_senza_confini_non_si_suona() {
    // Senza inizio si suonerebbe il disco dall'inizio, cioè un altro brano;
    // con una fine prima dell'inizio, niente di sensato.
    let c = db();
    brano_d_immagine(&c, 1, 1, None, Some(30_000));
    brano_d_immagine(&c, 2, 2, Some(30_000), Some(30_000));
    for id in [1, 2] {
        let err = scheda_sorgente(&c, id).expect_err("riga d'immagine rotta");
        assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackSourceUnavailable);
    }
}

#[test]
fn i_confini_d_immagine_si_leggono_solo_se_stanno_in_un_cd() {
    let immagine = || Some(IMMAGINE.to_owned());
    assert_eq!(
        collocazione_su_disco("C:/m/a.flac", None, Some(5), Some(1)),
        Some(Collocazione::File("C:/m/a.flac".to_owned())),
        "senza immagine le colonne dei confini non contano"
    );
    assert_eq!(collocazione_su_disco("x", immagine(), Some(-1), None), None);
    assert_eq!(
        collocazione_su_disco("x", immagine(), Some(10), Some(10)),
        None
    );
    assert_eq!(
        collocazione_su_disco("x", immagine(), Some(0), Some(i64::from(u32::MAX) + 1)),
        None
    );
    assert_eq!(
        collocazione_su_disco("x", immagine(), Some(0), Some(75)),
        Some(Collocazione::Segmento {
            immagine: IMMAGINE.to_owned(),
            inizio_cd: 0,
            fine_cd: Some(75),
        })
    );
}
