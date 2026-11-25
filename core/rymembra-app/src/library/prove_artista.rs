//! Le prove della pagina di un artista: i suoi brani, le partecipazioni, l'ordine
//! e il piano delle query.
//!
//! La libreria di prova ha le tre forme che una pagina artista deve distinguere:
//! i brani di un disco suo, i brani senza disco, e le raccolte — con e senza
//! l'artista del disco — dove l'artista compare accanto ad altri.

use rusqlite::Connection;

use super::*;
use crate::library::{
    TrackOrder, list_artists, list_tracks, rebuild_aggregates, transazione_di_scrittura,
};

/// La chiave di un dispositivo collegato.
fn pc() -> String {
    "ab".repeat(32)
}

/// Un brano di prova: tutto quel che serve a distinguerlo.
struct Riga {
    id: i64,
    titolo: &'static str,
    artista: &'static str,
    album: &'static str,
    artista_album: Option<&'static str>,
    album_key: Option<&'static str>,
    anno: Option<i64>,
    traccia: Option<i64>,
    ascolti: i64,
    /// `None` per una riga di qui, `Some(n)` per la riga `n` del PC.
    del_pc: Option<i64>,
    /// La riga che la nasconde in «Tutti».
    copia_di: Option<i64>,
}

fn riga(id: i64, titolo: &'static str, artista: &'static str, album: &'static str) -> Riga {
    Riga {
        id,
        titolo,
        artista,
        album,
        artista_album: None,
        album_key: None,
        anno: None,
        traccia: None,
        ascolti: 0,
        del_pc: None,
        copia_di: None,
    }
}

fn scrivi(c: &Connection, r: &Riga) {
    let path = match r.del_pc {
        Some(remoto) => format!("lontano://{}/{remoto}", pc()),
        None => format!("C:/M/{}.flac", r.id),
    };
    c.execute(
        "INSERT INTO tracks
           (id, path, track_key, title, artist, album, album_artist, album_key, year,
            track_number, play_count, copia_di, source, duration_ms, file_size,
            date_added, date_modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 200000, 1, 1, 1)",
        rusqlite::params![
            r.id,
            path,
            format!("k{}", r.id),
            r.titolo,
            r.artista,
            r.album,
            r.artista_album,
            r.album_key,
            r.anno,
            r.traccia,
            r.ascolti,
            r.copia_di,
            if r.del_pc.is_some() {
                "lontano"
            } else {
                "locale"
            },
        ],
    )
    .expect("brano");
}

/// Le righe, scritte e poi passate dagli aggregati: è lì che si riempiono le
/// chiavi d'ordine.
fn libreria_con(righe: &[Riga]) -> Connection {
    let mut c = crate::db::open_in_memory().expect("database").connection;
    for r in righe {
        scrivi(&c, r);
    }
    let tx = transazione_di_scrittura(&mut c).expect("transazione");
    rebuild_aggregates(&tx).expect("aggregati");
    tx.commit().expect("commit");
    c
}

/// L'artista «Tale»: un disco suo, un brano senza disco, due raccolte (una con
/// l'artista del disco, una senza), due «feat.», e le righe di un PC.
fn libreria() -> Connection {
    libreria_con(&[
        // Il disco suo.
        Riga {
            artista_album: Some("Tale"),
            album_key: Some("primo"),
            anno: Some(2001),
            traccia: Some(1),
            ascolti: 5,
            ..riga(1, "Zeta", "Tale", "Primo")
        },
        Riga {
            artista_album: Some("Tale"),
            album_key: Some("primo"),
            anno: Some(2001),
            traccia: Some(2),
            ascolti: 9,
            ..riga(2, "Beta", "Tale", "Primo")
        },
        // Un brano senza disco: niente `album_key`, non sta in nessun album.
        Riga {
            ascolti: 20,
            ..riga(3, "Singolo", "Tale", "Album sconosciuto")
        },
        // Una raccolta CON l'artista del disco: i brani di Tale lì sono
        // partecipazioni esatte, e quelli degli altri non sono suoi.
        Riga {
            artista_album: Some("Vari Artisti"),
            album_key: Some("raccolta"),
            anno: Some(2005),
            ascolti: 1,
            ..riga(4, "Raccolta uno", "Tale", "Raccolta")
        },
        Riga {
            artista_album: Some("Vari Artisti"),
            album_key: Some("raccolta"),
            anno: Some(2005),
            ..riga(5, "Raccolta due", "Altro", "Raccolta")
        },
        Riga {
            artista_album: Some("Vari Artisti"),
            album_key: Some("raccolta"),
            anno: Some(2005),
            ..riga(6, "Raccolta tre", "Terzo", "Raccolta")
        },
        // Una raccolta SENZA l'artista del disco: l'artista di ogni brano è il
        // suo. `albumArtista` darebbe l'album intero, e con lui i brani degli
        // altri due.
        Riga {
            album_key: Some("mista"),
            anno: Some(2003),
            ascolti: 2,
            ..riga(7, "Mista uno", "Tale", "Mista")
        },
        Riga {
            album_key: Some("mista"),
            anno: Some(2003),
            ..riga(8, "Mista due", "Altro", "Mista")
        },
        Riga {
            album_key: Some("mista"),
            anno: Some(2003),
            ..riga(9, "Mista tre", "Terzo", "Mista")
        },
        // I «feat.»: euristiche. Uno sotto l'artista del disco di un altro,
        // l'altro sotto un artista che si chiama diversamente.
        Riga {
            artista_album: Some("Altro"),
            album_key: Some("loro"),
            anno: Some(2012),
            ..riga(10, "Duetto", "Altro feat. Tale", "Loro")
        },
        Riga {
            album_key: Some("gruppo"),
            anno: Some(2012),
            ..riga(11, "Insieme", "Tale & Co", "Gruppo")
        },
        // Le righe del PC: un brano solo suo, la copia nascosta di quello di
        // qui, una raccolta con Tale, un «feat.».
        Riga {
            artista_album: Some("Tale"),
            album_key: Some("primo-pc"),
            anno: Some(2015),
            traccia: Some(1),
            ascolti: 3,
            del_pc: Some(1),
            ..riga(20, "Remoto", "Tale", "Primo del PC")
        },
        Riga {
            artista_album: Some("Tale"),
            album_key: Some("primo-pc2"),
            anno: Some(2001),
            del_pc: Some(2),
            copia_di: Some(1),
            ..riga(21, "Zeta", "Tale", "Primo")
        },
        Riga {
            artista_album: Some("Altro"),
            album_key: Some("loro-pc"),
            del_pc: Some(3),
            ..riga(22, "Duetto del PC", "Altro feat. Tale", "Loro del PC")
        },
        Riga {
            artista_album: Some("Vari Artisti"),
            album_key: Some("raccolta-pc"),
            del_pc: Some(4),
            ..riga(23, "Raccolta del PC", "Tale", "Raccolta del PC")
        },
    ])
}

fn ids(brani: &[TrackSummary]) -> Vec<i64> {
    brani.iter().map(|b| b.id).collect()
}

/// Gli identificativi, ordinati: per i confronti che non vogliono un ordine.
fn insieme(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v
}

const TUTTI: Origine = Origine::Tutti;

#[test]
fn la_scheda_del_brano_porta_l_artista_dell_album() {
    // «In riproduzione» apre la pagina dell'artista **effettivo**: l'artista del
    // disco se c'è. Senza `album_artist` sulla scheda, una compilation porterebbe
    // alla pagina dell'interprete del brano, che nell'elenco degli artisti non c'è.
    let c = libreria();
    let brani =
        brani_artista(&c, &TUTTI, "Tale", OrdineBraniArtista::Titolo, 0, 100).expect("brani");
    let per_id = |id: i64| brani.iter().find(|b| b.id == id).expect("brano");
    assert_eq!(per_id(1).album_artist.as_deref(), Some("Tale"));
    assert_eq!(per_id(7).album_artist, None, "nessun artista del disco");
    let r = list_tracks(&c, &TUTTI, TrackOrder::Shelf, 0, 100).expect("brani");
    let compilation = r.iter().find(|b| b.id == 4).expect("raccolta");
    assert_eq!(compilation.artist, "Tale");
    assert_eq!(compilation.album_artist.as_deref(), Some("Vari Artisti"));
}

#[test]
fn i_brani_di_un_artista_sono_i_suoi_anche_senza_disco() {
    let c = libreria();
    let tutti =
        brani_artista(&c, &TUTTI, "Tale", OrdineBraniArtista::Album, 0, 100).expect("brani");
    // 1, 2: il suo disco. 3: senza disco. 7: sua dentro una raccolta senza artista
    // del disco. 20: la riga del PC. Non la 21, che «Tutti» nasconde.
    assert_eq!(insieme(ids(&tutti)), vec![1, 2, 3, 7, 20]);
    // Il numero che la griglia degli artisti dice è lo stesso.
    let artisti = list_artists(&c, &TUTTI).expect("artisti");
    let tale = artisti.iter().find(|a| a.name == "Tale").expect("Tale");
    assert_eq!(tale.tracks, 5);
}

#[test]
fn riproduci_un_artista_non_porta_i_brani_degli_altri() {
    // Il difetto di `suonaArtista`: prendeva gli album dell'artista e i brani di
    // ciascuno, cioè le raccolte intere, con i brani di chiunque.
    let c = libreria();
    let suoi = id_dei_brani_artista(&c, &TUTTI, "Tale", OrdineBraniArtista::Album).expect("id");
    for degli_altri in [4, 5, 6, 8, 9, 10, 11, 22, 23] {
        assert!(
            !suoi.contains(&degli_altri),
            "il brano {degli_altri} non è di Tale: {suoi:?}"
        );
    }
    // E dell'album misto prende solo il suo.
    assert!(suoi.contains(&7));
}

#[test]
fn un_brano_senza_disco_sta_in_fondo_per_album() {
    let c = libreria();
    let v = brani_artista(&c, &TUTTI, "Tale", OrdineBraniArtista::Album, 0, 100).expect("brani");
    // Mista, Primo (per traccia), Primo del PC, e in fondo quello senza disco.
    assert_eq!(ids(&v), vec![7, 1, 2, 20, 3]);
}

#[test]
fn l_ordine_per_anno_titolo_e_ascolti() {
    let c = libreria();
    let ordina = |o| ids(&brani_artista(&c, &TUTTI, "Tale", o, 0, 100).expect("brani"));
    // 2001 (due, per traccia), 2003, 2015, e senza anno in fondo.
    assert_eq!(ordina(OrdineBraniArtista::Anno), vec![1, 2, 7, 20, 3]);
    assert_eq!(ordina(OrdineBraniArtista::Titolo), vec![2, 7, 20, 3, 1]);
    assert_eq!(
        ordina(OrdineBraniArtista::PiuAscoltati),
        vec![3, 2, 1, 20, 7]
    );
}

#[test]
fn la_vista_intera_ha_l_ordine_delle_pagine() {
    // «Seleziona tutto» e il tocco su una riga chiedono gli identificativi, e
    // devono avere lo stesso ordine delle pagine che si vedono.
    let c = libreria();
    for o in [
        OrdineBraniArtista::Album,
        OrdineBraniArtista::Anno,
        OrdineBraniArtista::Titolo,
        OrdineBraniArtista::PiuAscoltati,
    ] {
        let mut pagine = Vec::new();
        let mut da = 0;
        loop {
            let p = brani_artista(&c, &TUTTI, "Tale", o, da, 2).expect("pagina");
            if p.is_empty() {
                break;
            }
            da += 2;
            pagine.extend(ids(&p));
        }
        assert_eq!(
            id_dei_brani_artista(&c, &TUTTI, "Tale", o).expect("id"),
            pagine,
            "{o:?}"
        );
        assert_eq!(pagine.len(), 5, "{o:?}");
    }
}

#[test]
fn il_filtro_per_dispositivo_vale_nella_pagina_dell_artista() {
    let c = libreria();
    let dei = |o: &Origine| {
        insieme(ids(&brani_artista(
            &c,
            o,
            "Tale",
            OrdineBraniArtista::Album,
            0,
            100,
        )
        .expect("brani")))
    };
    assert_eq!(dei(&TUTTI), vec![1, 2, 3, 7, 20]);
    assert_eq!(dei(&Origine::Locale), vec![1, 2, 3, 7]);
    // Tutte le righe del PC, anche la copia che «Tutti» nasconde.
    assert_eq!(dei(&Origine::Dispositivo(pc())), vec![20, 21]);
}

#[test]
fn un_artista_che_non_c_e_non_ha_brani() {
    let c = libreria();
    assert!(
        brani_artista(&c, &TUTTI, "Nessuno", OrdineBraniArtista::Album, 0, 100)
            .expect("brani")
            .is_empty()
    );
    assert!(
        id_dei_brani_artista(&c, &TUTTI, "", OrdineBraniArtista::Album)
            .expect("id")
            .is_empty()
    );
}

#[test]
fn le_partecipazioni_esatte_sono_le_compilation() {
    // `artist` uguale al nome, ma l'artista del disco è un altro.
    let c = libreria();
    let esatte = |o: &Origine| {
        insieme(ids(&partecipazioni(
            &c,
            o,
            "Tale",
            Partecipazione::Esatta,
            0,
            100,
        )
        .expect("esatte")))
    };
    assert_eq!(esatte(&TUTTI), vec![4, 23]);
    assert_eq!(esatte(&Origine::Locale), vec![4]);
    assert_eq!(esatte(&Origine::Dispositivo(pc())), vec![23]);
}

#[test]
fn le_partecipazioni_euristiche_sono_i_feat_e_non_toccano_il_resto() {
    // Il nome fra i crediti di un brano di un altro: un indizio, non un fatto.
    let c = libreria();
    let euristiche = |o: &Origine| {
        insieme(ids(&partecipazioni(
            &c,
            o,
            "Tale",
            Partecipazione::Euristica,
            0,
            100,
        )
        .expect("euristiche")))
    };
    assert_eq!(euristiche(&TUTTI), vec![10, 11, 22]);
    assert_eq!(euristiche(&Origine::Locale), vec![10, 11]);
    assert_eq!(euristiche(&Origine::Dispositivo(pc())), vec![22]);
    // Né i brani suoi né le esatte tornano fra le euristiche: tre elenchi, tre
    // insiemi senza niente in comune.
    let suoi = id_dei_brani_artista(&c, &TUTTI, "Tale", OrdineBraniArtista::Album).expect("id");
    let esatte =
        ids(&partecipazioni(&c, &TUTTI, "Tale", Partecipazione::Esatta, 0, 100).expect("esatte"));
    for e in euristiche(&TUTTI) {
        assert!(
            !suoi.contains(&e) && !esatte.contains(&e),
            "{e} è già altrove"
        );
    }
}

#[test]
fn i_conti_dicono_quanto_e_lungo_ogni_elenco() {
    let c = libreria();
    for (o, esatte, euristiche) in [
        (TUTTI, 2, 3),
        (Origine::Locale, 1, 2),
        (Origine::Dispositivo(pc()), 1, 1),
    ] {
        let conto = partecipazioni_conta(&c, &o, "Tale").expect("conto");
        assert_eq!(conto, ContoPartecipazioni { esatte, euristiche }, "{o:?}");
        let pagina = |tipo| {
            partecipazioni(&c, &o, "Tale", tipo, 0, 100)
                .expect("pagina")
                .len()
        };
        assert_eq!(
            (
                pagina(Partecipazione::Esatta),
                pagina(Partecipazione::Euristica)
            ),
            (
                usize::try_from(esatte).expect("pochi"),
                usize::try_from(euristiche).expect("pochi")
            )
        );
    }
}

#[test]
fn le_partecipazioni_si_chiedono_a_pagine() {
    let c = libreria();
    let prima =
        partecipazioni(&c, &TUTTI, "Tale", Partecipazione::Euristica, 0, 2).expect("pagina");
    let seconda =
        partecipazioni(&c, &TUTTI, "Tale", Partecipazione::Euristica, 2, 2).expect("pagina");
    assert_eq!(prima.len(), 2);
    assert_eq!(seconda.len(), 1);
    let mut tutte = ids(&prima);
    tutte.extend(ids(&seconda));
    assert_eq!(insieme(tutte), vec![10, 11, 22]);
}

#[test]
fn un_nome_con_virgolette_o_operatori_non_rompe_la_ricerca() {
    // Il nome finisce dentro un'espressione FTS5: virgolette, `AC/DC`, parole che
    // per FTS5 sono operatori. Una frase fra virgolette, con le virgolette interne
    // raddoppiate, li legge tutti come parole.
    let c = libreria_con(&[
        Riga {
            artista_album: Some("Altro"),
            album_key: Some("a"),
            ..riga(1, "Uno", "Altro feat. AC/DC", "A")
        },
        Riga {
            artista_album: Some("Altro"),
            album_key: Some("a"),
            ..riga(2, "Due", "Altro feat. Il \"Tale\" Show", "A")
        },
        Riga {
            artista_album: Some("Altro"),
            album_key: Some("a"),
            ..riga(3, "Tre", "Altro feat. NOT", "A")
        },
        Riga {
            artista_album: Some("Altro"),
            album_key: Some("a"),
            ..riga(4, "Quattro", "Altro feat. a*b", "A")
        },
    ]);
    let euristiche = |nome: &str| {
        ids(
            &partecipazioni(&c, &TUTTI, nome, Partecipazione::Euristica, 0, 10)
                .unwrap_or_else(|e| panic!("«{nome}»: {e:?}")),
        )
    };
    assert_eq!(euristiche("AC/DC"), vec![1]);
    assert_eq!(euristiche("Il \"Tale\" Show"), vec![2]);
    assert_eq!(euristiche("NOT"), vec![3]);
    assert_eq!(euristiche("a*b"), vec![4]);
    // Un nome senza una parola — solo punteggiatura — non ha niente da cercare, e
    // non è un errore.
    assert!(euristiche("!!!").is_empty());
    assert!(euristiche("").is_empty());
    assert!(euristiche("   ").is_empty());
    // E il conto dice lo stesso.
    assert_eq!(
        partecipazioni_conta(&c, &TUTTI, "!!!").expect("conto"),
        ContoPartecipazioni::default()
    );
}

/// Il piano di una query, una riga per passo.
fn piano(c: &Connection, sql: &str, parametri: &[&dyn rusqlite::ToSql]) -> Vec<String> {
    let mut s = c
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("piano");
    let righe = s
        .query_map(parametri, |r| r.get::<_, String>(3))
        .expect("piano");
    righe.collect::<Result<_, _>>().expect("righe del piano")
}

#[test]
fn il_piano_dei_brani_passa_dall_indice_dell_artista_effettivo() {
    let c = libreria();
    for o in [
        OrdineBraniArtista::Album,
        OrdineBraniArtista::Anno,
        OrdineBraniArtista::Titolo,
        OrdineBraniArtista::PiuAscoltati,
    ] {
        for origine in [TUTTI, Origine::Locale, Origine::Dispositivo(pc())] {
            let sql = sql_dei_brani(&c, &origine, o).expect("sql");
            let p = piano(&c, &sql, &[&"Tale", &10_i64, &0_i64]);
            assert!(
                p.iter().any(|r| r.contains("idx_tracks_artista_effettivo")),
                "{o:?} {origine:?}: {p:?}"
            );
            assert!(
                !p.iter().any(|r| r.starts_with("SCAN")),
                "{o:?} {origine:?}: {p:?}"
            );
        }
    }
}

#[test]
fn il_piano_delle_esatte_passa_dall_indice_dell_artista() {
    let c = libreria();
    for origine in [TUTTI, Origine::Locale, Origine::Dispositivo(pc())] {
        let sql = sql_delle_esatte(&c, &origine).expect("sql");
        let p = piano(&c, &sql, &[&"Tale", &10_i64, &0_i64]);
        assert!(
            p.iter().any(|r| r.contains("idx_tracks_artist ")),
            "{origine:?}: {p:?}"
        );
        assert!(
            !p.iter().any(|r| r.starts_with("SCAN")),
            "{origine:?}: {p:?}"
        );
    }
}

#[test]
fn il_piano_delle_euristiche_parte_dall_indice_di_testo() {
    let c = libreria();
    for origine in [TUTTI, Origine::Locale, Origine::Dispositivo(pc())] {
        let sql = sql_delle_euristiche(&c, &origine).expect("sql");
        let p = piano(&c, &sql, &[&"artist : \"tale\"", &"Tale", &10_i64, &0_i64]);
        assert!(
            p.iter()
                .any(|r| r.contains("tracks_fts VIRTUAL TABLE INDEX")),
            "{origine:?}: {p:?}"
        );
        // Nessuna lettura intera di `tracks`: ci si arriva per identificativo.
        assert!(
            !p.iter()
                .any(|r| r.starts_with("SCAN tracks") && !r.contains("tracks_fts")),
            "{origine:?}: {p:?}"
        );
    }
}

#[test]
fn gli_identificativi_delle_partecipazioni_hanno_l_ordine_delle_pagine() {
    // Un tocco su una riga di «Compare anche in» suona quell'elenco dalla riga
    // premuta: gli identificativi sono quelli delle pagine, nello stesso ordine.
    let c = libreria();
    for tipo in [Partecipazione::Esatta, Partecipazione::Euristica] {
        for origine in [TUTTI, Origine::Locale, Origine::Dispositivo(pc())] {
            let mut pagine = Vec::new();
            let mut da = 0;
            loop {
                let p = partecipazioni(&c, &origine, "Tale", tipo, da, 2).expect("pagina");
                if p.is_empty() {
                    break;
                }
                da += 2;
                pagine.extend(ids(&p));
            }
            assert_eq!(
                id_delle_partecipazioni(&c, &origine, "Tale", tipo).expect("id"),
                pagine,
                "{tipo:?} {origine:?}"
            );
        }
    }
    assert!(
        id_delle_partecipazioni(&c, &TUTTI, "!!!", Partecipazione::Euristica)
            .expect("id")
            .is_empty()
    );
}

/// S9-3: l'artista da solo è la stessa riga che la griglia darebbe per lui — nome,
/// conti, copertine —, con ogni filtro per dispositivo. La pagina non legge più
/// l'elenco intero per due numeri, e i numeri restano quelli di prima.
#[test]
fn l_artista_da_solo_e_la_riga_della_griglia() {
    let c = libreria();
    for origine in [
        Origine::Tutti,
        Origine::Locale,
        Origine::Dispositivo(pc()),
        Origine::Raggiungibili(Vec::new()),
    ] {
        let griglia = list_artists(&c, &origine).expect("griglia");
        assert!(
            !griglia.is_empty(),
            "{origine:?}: la prova non guarda niente"
        );
        for attesa in &griglia {
            let sola = artista_solo(&c, &origine, &attesa.name)
                .expect("artista")
                .unwrap_or_else(|| panic!("{origine:?}: manca «{}»", attesa.name));
            assert_eq!(&sola, attesa, "{origine:?}: «{}»", attesa.name);
        }
        // Chi la griglia non ha, da solo non c'è: un nome che non esiste, uno
        // vuoto, e uno che ha brani solo sotto un altro filtro.
        assert!(
            artista_solo(&c, &origine, "Nessuno")
                .expect("artista")
                .is_none()
        );
        assert!(artista_solo(&c, &origine, "").expect("artista").is_none());
    }
    // «Tale» ha brani di qui e del PC: con «Questo dispositivo» ha solo i suoi.
    let tutti = artista_solo(&c, &Origine::Tutti, "Tale").expect("artista");
    let locale = artista_solo(&c, &Origine::Locale, "Tale").expect("artista");
    assert!(tutti.is_some() && locale.is_some());
}

/// S9-3: le copertine sono le quattro dei dischi più vecchi, una per disco: lo
/// stesso mosaico della griglia.
#[test]
fn le_copertine_dell_artista_da_solo_sono_le_quattro_dei_dischi_piu_vecchi() {
    const CHIAVI: [&str; 6] = ["a0", "a1", "a2", "a3", "a4", "a5"];
    const DISCHI: [&str; 6] = ["A0", "A1", "A2", "A3", "A4", "A5"];
    let dischi: Vec<Riga> = (0..6_i64)
        .zip(CHIAVI.into_iter().zip(DISCHI))
        .map(|(i, (chiave, disco))| Riga {
            artista_album: Some("Cover"),
            album_key: Some(chiave),
            anno: Some(2000 + i),
            ..riga(100 + i, "Brano", "Cover", disco)
        })
        .collect();
    let c = libreria_con(&dischi);
    // Le copertine di prova non hanno la loro riga in `cover_art`: qui conta quali
    // quattro si scelgono, non il file.
    c.execute_batch("PRAGMA foreign_keys = OFF")
        .expect("pragma");
    for i in 0..6_i64 {
        c.execute(
            "UPDATE tracks SET cover_art_hash = ?1 WHERE id = ?2",
            rusqlite::params![format!("h{i}"), 100 + i],
        )
        .expect("copertina");
    }
    let sola = artista_solo(&c, &TUTTI, "Cover")
        .expect("artista")
        .expect("c'è");
    assert_eq!(sola.covers, vec!["h0", "h1", "h2", "h3"]);
    assert_eq!(sola.albums, 6);
    assert_eq!(sola.tracks, 6);
    let griglia = list_artists(&c, &TUTTI).expect("griglia");
    assert_eq!(
        Some(&sola),
        griglia.iter().find(|a| a.name == "Cover"),
        "la riga è quella della griglia"
    );
}
