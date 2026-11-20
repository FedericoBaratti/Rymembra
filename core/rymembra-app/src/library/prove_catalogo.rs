//! Le prove del catalogo che questo dispositivo serve agli altri
//! (`condivisa::riassunto_catalogo`, `pagina_catalogo`, `valuta`): a secchi di
//! id, con l'impronta che cambia solo dove cambia qualcosa, ricordato per
//! generazione; e mai le righe specchio, che sono di un altro dispositivo.

use std::sync::Mutex;

use rusqlite::Connection;

use super::condivisa::{self, ID_PER_SECCHIO, Ricordate};
use crate::specchio::comuni::{ADESSO, chiave};

/// Una libreria vuota che sorveglia `C:/M`: i brani di prova stanno lì sotto.
/// Senza una cartella sorvegliata niente si condivide (S1-8).
fn libreria() -> Connection {
    let c = crate::db::open_in_memory().expect("database").connection;
    radici(&c, &["C:/M"]);
    c
}

/// Le cartelle sorvegliate di adesso, come le scrive `imposta_cartelle`.
fn radici(c: &Connection, elenco: &[&str]) {
    crate::settings::write_json(c, crate::settings::CHIAVE_CARTELLE, &elenco.to_vec())
        .expect("cartelle sorvegliate");
}

fn brano(c: &Connection, id: i64, path: &str) {
    c.execute(
        "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key, duration_ms,
                             file_size, date_added, date_modified, codec, bitrate)
         VALUES (?1, ?2, ?3, ?4, 'A', 'Al', 'al', 180000, 1000, 1, 1, 'Flac', 900)",
        rusqlite::params![id, path, format!("k{id}"), format!("T{id}")],
    )
    .expect("brano");
}

fn locali(c: &Connection, ids: impl IntoIterator<Item = i64>) {
    for id in ids {
        brano(c, id, &format!("C:/M/{id}.flac"));
    }
}

#[test]
fn le_righe_specchio_non_si_ricondividono_in_nessuna_domanda() {
    // §4.7: un terzo dispositivo che le vedesse attraverso questo avrebbe
    // catene, doppioni e ascolti contati al proprietario sbagliato.
    let c = libreria();
    locali(&c, [1, 2]);
    brano(&c, 3, &format!("lontano://{}/77", chiave('a')));
    brano(&c, 4, "nuvola://onedrive-abcdefgh/Musica/c.flac");
    // Il resto dei campi di una riga specchio, perché sia una riga vera di libreria.
    c.execute("UPDATE tracks SET source = 'lontano' WHERE id = 3", [])
        .unwrap();
    let id = |v: &[condivisa::BranoRemoto]| v.iter().map(|b| b.id).collect::<Vec<_>>();
    assert_eq!(
        id(&condivisa::brani(&c, super::TrackOrder::Title, 0, 50).unwrap()),
        vec![1, 2]
    );
    assert_eq!(
        id(&condivisa::brani_diretti(&c, super::TrackOrder::Shelf, 0, 50).unwrap()),
        vec![1, 2]
    );
    assert!(condivisa::brano(&c, 3).unwrap().is_none());
    assert!(condivisa::file_del_brano(&c, 3).unwrap().is_none());
    assert_eq!(
        id(&condivisa::cerca(&c, "T3", 0, 50).unwrap()),
        Vec::<i64>::new()
    );
    assert_eq!(condivisa::cerca_conteggio(&c, "T3").unwrap(), 0);
    assert_eq!(id(&condivisa::cerca(&c, "T2", 0, 50).unwrap()), vec![2]);
    // Né il catalogo.
    let riassunto = condivisa::riassunto_catalogo(&c).unwrap();
    assert_eq!(riassunto.secchi.iter().map(|s| s.righe).sum::<i64>(), 2);
    let pagina = condivisa::pagina_catalogo(&c, 0, ID_PER_SECCHIO - 1).unwrap();
    assert_eq!(
        pagina.brani.iter().map(|b| b.id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    // E una valutazione per una riga specchio è «non più condivisa».
    let err = condivisa::valuta(&c, 3, Some(true), None, ADESSO - 5, ADESSO).unwrap_err();
    assert_eq!(
        err.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::LontanoFileCambiato
    );
}

#[test]
fn i_secchi_sono_intervalli_di_500_id_stabili() {
    let c = libreria();
    // Id sparsi: 1..=3 (secchio 0), 499 (0), 500 (1), 1200 (2).
    locali(&c, [1, 2, 3, 499, 500, 1200]);
    let r = condivisa::riassunto_catalogo(&c).unwrap();
    let forma: Vec<(i64, i64, i64)> = r
        .secchi
        .iter()
        .map(|s| (s.da_id, s.a_id, s.righe))
        .collect();
    assert_eq!(forma, vec![(0, 499, 4), (500, 999, 1), (1000, 1499, 1)]);
    assert!(r.secchi.iter().all(|s| s.impronta.len() == 32));
    // Togliere un brano in mezzo non sposta gli altri secchi: le loro impronte
    // restano le stesse.
    let prima: Vec<String> = r.secchi.iter().map(|s| s.impronta.clone()).collect();
    c.execute("DELETE FROM tracks WHERE id = 2", []).unwrap();
    let dopo = condivisa::riassunto_catalogo(&c).unwrap();
    assert_eq!(dopo.secchi.len(), 3);
    assert_ne!(dopo.secchi[0].impronta, prima[0], "il suo secchio cambia");
    assert_eq!(dopo.secchi[1].impronta, prima[1]);
    assert_eq!(dopo.secchi[2].impronta, prima[2]);
    assert_eq!(dopo.secchi[0].righe, 3);
}

#[test]
fn un_brano_ascoltato_cambia_un_secchio_solo() {
    let c = libreria();
    locali(&c, [1, 2, 600, 601, 1300]);
    let prima = condivisa::riassunto_catalogo(&c).unwrap();
    c.execute(
        "UPDATE tracks SET play_count = 5, last_played_at = 1000, stats_updated_at = 1000 WHERE id = 601",
        [],
    )
    .unwrap();
    let dopo = condivisa::riassunto_catalogo(&c).unwrap();
    assert_ne!(dopo.generazione, prima.generazione);
    let diversi: Vec<i64> = prima
        .secchi
        .iter()
        .zip(&dopo.secchi)
        .filter(|(a, b)| a.impronta != b.impronta)
        .map(|(a, _)| a.da_id)
        .collect();
    assert_eq!(diversi, vec![500], "solo quello del brano 601");
    // Anche un voto, un mi piace e un titolo.
    for sql in [
        "UPDATE tracks SET liked = 1, liked_at = 5 WHERE id = 1",
        "UPDATE tracks SET rating = 3, rating_at = 5 WHERE id = 1",
        "UPDATE tracks SET title = 'Altro' WHERE id = 1",
    ] {
        let p = condivisa::riassunto_catalogo(&c).unwrap();
        c.execute(sql, []).unwrap();
        let d = condivisa::riassunto_catalogo(&c).unwrap();
        assert_ne!(p.secchi[0].impronta, d.secchi[0].impronta, "{sql}");
        assert_eq!(p.secchi[1].impronta, d.secchi[1].impronta, "{sql}");
    }
}

#[test]
fn il_riassunto_si_ricorda_per_generazione() {
    let c = libreria();
    locali(&c, [1, 2, 3]);
    let cache = Mutex::new(Ricordate::default());
    let a = condivisa::riassunto_con(&c, &cache).unwrap();
    let b = condivisa::riassunto_con(&c, &cache).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        cache.lock().unwrap().costruiti(),
        1,
        "la seconda volta dalla memoria"
    );
    // Una scrittura su tracks cambia la generazione: si ricalcola.
    c.execute("UPDATE tracks SET play_count = 9 WHERE id = 1", [])
        .unwrap();
    let d = condivisa::riassunto_con(&c, &cache).unwrap();
    assert_ne!(d, a);
    assert_eq!(cache.lock().unwrap().costruiti(), 2);
    // E si tengono al più due generazioni.
    for n in 0..5 {
        c.execute("UPDATE tracks SET play_count = ?1 WHERE id = 1", [n + 20])
            .unwrap();
        condivisa::riassunto_con(&c, &cache).unwrap();
    }
    assert!(cache.lock().unwrap().catalogo_ricordati() <= 2);
}

#[test]
fn una_pagina_porta_le_righe_del_secchio_e_nei_tetti() {
    let c = libreria();
    locali(&c, [1, 2, 499, 500]);
    c.execute(
        "UPDATE tracks SET title = ?1, album_artist = 'AA', genre = 'G', content_key = 'a|b|c',
                           mb_recording_id = '5b11f4ce-a62d-471e-81fc-a69a8278c7da',
                           liked = 1, liked_at = 77, rating = 4, rating_at = 78
         WHERE id = 2",
        [format!("é{}", "x".repeat(900))],
    )
    .unwrap();
    let p = condivisa::pagina_catalogo(&c, 0, 499).unwrap();
    assert_eq!((p.da_id, p.a_id), (0, 499));
    assert_eq!(
        p.brani.iter().map(|b| b.id).collect::<Vec<_>>(),
        vec![1, 2, 499]
    );
    let due = &p.brani[1];
    assert!(
        due.title.len() <= crate::specchio::convalida::MAX_NOME,
        "{}",
        due.title.len()
    );
    assert!(due.title.starts_with('é'));
    assert_eq!(due.album_artist.as_deref(), Some("AA"));
    assert_eq!(due.content_key.as_deref(), Some("a|b|c"));
    assert_eq!(due.track_key, "k2");
    assert_eq!(
        due.mb_recording_id.as_deref(),
        Some("5b11f4ce-a62d-471e-81fc-a69a8278c7da")
    );
    assert_eq!(
        (due.liked, due.liked_at, due.rating, due.rating_at),
        (true, Some(77), 4, Some(78))
    );
    assert_eq!(due.codec.as_deref(), Some("Flac"));
    assert_eq!(due.bitrate, Some(900));
    assert_eq!(due.estensione, "flac");
    // Un secchio più largo di quanto si serve, o al contrario, no.
    for (da, a) in [(0, 500), (10, 5), (-1, 10), (0, 100_000)] {
        let err = condivisa::pagina_catalogo(&c, da, a).unwrap_err();
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoProtocollo,
            "{da}..{a}"
        );
    }
}

#[test]
fn una_valutazione_vince_per_campo_con_il_suo_orologio_ed_e_innocua_due_volte() {
    let c = libreria();
    locali(&c, [1]);
    // Il proprietario ha votato 5 stelle all'istante 100, e nessun mi piace.
    c.execute(
        "UPDATE tracks SET rating = 5, rating_at = 100, stats_updated_at = 100 WHERE id = 1",
        [],
    )
    .unwrap();
    // Un mi piace arrivato da fuori a 50 e le stelle a 60: il mi piace entra
    // (nessuno l'aveva deciso), le stelle no (là sono più nuove).
    assert!(condivisa::valuta(&c, 1, Some(true), Some(2), 60, ADESSO).unwrap());
    let stato = |c: &Connection| -> (i64, Option<i64>, i64, Option<i64>, i64) {
        c.query_row(
            "SELECT liked, liked_at, rating, rating_at, stats_updated_at FROM tracks WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
    };
    assert_eq!(stato(&c), (1, Some(60), 5, Some(100), 100));
    // Innocuo se arriva due volte: stesso orologio, non è più recente.
    assert!(!condivisa::valuta(&c, 1, Some(false), Some(0), 60, ADESSO).unwrap());
    assert_eq!(stato(&c), (1, Some(60), 5, Some(100), 100));
    // Più nuovo, vince: e `stats_updated_at` non torna mai indietro.
    assert!(condivisa::valuta(&c, 1, None, Some(1), 150, ADESSO).unwrap());
    assert_eq!(stato(&c), (1, Some(60), 1, Some(150), 150));
    assert!(condivisa::valuta(&c, 1, Some(false), None, 120, ADESSO).unwrap());
    assert_eq!(stato(&c), (0, Some(120), 1, Some(150), 150));
    // Un orologio avanti non fissa un voto nel futuro.
    assert!(condivisa::valuta(&c, 1, None, Some(4), ADESSO + 10_000_000_000, ADESSO).unwrap());
    let (_, _, voto, quando, _) = stato(&c);
    assert_eq!((voto, quando), (4, Some(ADESSO)));
}

#[test]
fn una_valutazione_storta_o_per_un_brano_che_non_c_e_si_rifiuta() {
    let c = libreria();
    locali(&c, [1]);
    let codice = |e: rymembra_domain::errors::AppError| e.code().kind();
    use rymembra_domain::errors::ErrorCodeKind as K;
    assert_eq!(
        codice(condivisa::valuta(&c, 1, None, Some(6), 10, ADESSO).unwrap_err()),
        K::LontanoProtocollo
    );
    assert_eq!(
        codice(condivisa::valuta(&c, 1, None, Some(-1), 10, ADESSO).unwrap_err()),
        K::LontanoProtocollo
    );
    assert_eq!(
        codice(condivisa::valuta(&c, 1, Some(true), None, 0, ADESSO).unwrap_err()),
        K::LontanoProtocollo
    );
    assert_eq!(
        codice(condivisa::valuta(&c, 99, Some(true), None, 10, ADESSO).unwrap_err()),
        K::LontanoFileCambiato
    );
    // Senza niente da dire non cambia niente.
    assert!(!condivisa::valuta(&c, 1, None, None, 10, ADESSO).unwrap());
}

#[test]
fn la_scansione_dello_stesso_originale_prende_il_posto_della_copertina_nata_da_fuori() {
    // Advisor, punto 4: la riga di `cover_art` nata per una riga specchio
    // (`source = 'lontano'`, le misure della media) non deve tenere il posto
    // della copertina vera di questa macchina, che `INSERT OR IGNORE` lasciava
    // com'era per sempre. Una riga di altra origine invece non si tocca.
    let c = libreria();
    let da_fuori = "ef".repeat(32);
    let locale = "ab".repeat(32);
    for (hash, sorgente, larghezza) in [(&da_fuori, "lontano", 320), (&locale, "tag", 700)] {
        c.execute(
            "INSERT INTO cover_art (hash, mime_type, width, height, byte_size, source, created_at)
             VALUES (?1, 'image/jpeg', ?2, ?2, 20000, ?3, 1)",
            rusqlite::params![hash, larghezza, sorgente],
        )
        .unwrap();
    }
    for (n, hash) in [&da_fuori, &locale].into_iter().enumerate() {
        let file = super::DiscoveredFile {
            path: format!("C:/M/{n}.flac"),
            size_bytes: 5_000_000,
            modified_ms: 1,
        };
        let mut riga = super::righe::riga_provvisoria(&file, None);
        riga.cover = Some(crate::covers::StoredCover {
            hash: hash.clone(),
            width: 640,
            height: 640,
            byte_size: 90_000,
            mime_type: "image/jpeg",
            already_present: false,
        });
        let tx = c.unchecked_transaction().unwrap();
        super::insert_track(&tx, &riga, 5).unwrap();
        tx.commit().unwrap();
    }
    let cosi = |hash: &str| -> (String, i64) {
        c.query_row(
            "SELECT source, width FROM cover_art WHERE hash = ?1",
            [hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(
        cosi(&da_fuori),
        ("tag".to_owned(), 640),
        "la vera prende il posto"
    );
    assert_eq!(
        cosi(&locale),
        ("tag".to_owned(), 700),
        "una già di qui non si tocca"
    );
}

#[test]
fn un_cuore_vecchio_non_vince_su_un_cuore_del_proprietario_piu_nuovo_se_le_stelle_sono_dopo() {
    // S2-9. Il telefono, col PC spento: a 100 mette il cuore, a 200 dà tre stelle
    // allo stesso brano. La voce di posta porta i due campi con **un** orologio, il
    // più recente (200). Nel frattempo, a 150, sul PC la persona ha tolto il cuore.
    // Con l'orologio unico il cuore del telefono (deciso a 100) vince su quello del
    // PC (150) perché viaggia con il 200 delle stelle: «vince il più recente, per
    // campo» non valeva.
    let c = libreria();
    locali(&c, [1]);
    c.execute(
        "UPDATE tracks SET liked = 0, liked_at = 150, stats_updated_at = 150 WHERE id = 1",
        [],
    )
    .unwrap();
    let stato = |c: &Connection| -> (i64, i64) {
        c.query_row("SELECT liked, rating FROM tracks WHERE id = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
    };
    // Una build di prima manda un orologio solo, per tutti e due i campi, e
    // continua a valere cosÃ¬: il cuore Â«ringiovanitoÂ» Ã¨ il suo limite.
    condivisa::valuta(&c, 1, Some(true), Some(3), 200, ADESSO).unwrap();
    assert_eq!(stato(&c), (1, 3), "con un orologio solo, com'era");
    c.execute(
        "UPDATE tracks SET liked = 0, liked_at = 150, rating = 0, rating_at = NULL WHERE id = 1",
        [],
    )
    .unwrap();
    // Una di adesso porta un orologio per campo: il cuore Ã¨ di 100, le stelle di 200.
    condivisa::valuta_con_orologi(
        &c,
        1,
        Some(true),
        Some(3),
        condivisa::OrologiValutazione {
            quando_ms: 200,
            liked_ms: Some(100),
            rating_ms: Some(200),
        },
        ADESSO,
    )
    .unwrap();
    assert_eq!(
        stato(&c),
        (0, 3),
        "il cuore tolto sul PC a 150 batte quello del telefono, deciso a 100; le stelle entrano"
    );
    // Un orologio di campo che non Ã¨ un istante si rifiuta, come quello della voce.
    assert!(
        condivisa::valuta_con_orologi(
            &c,
            1,
            Some(true),
            None,
            condivisa::OrologiValutazione {
                quando_ms: 200,
                liked_ms: Some(0),
                rating_ms: None,
            },
            ADESSO,
        )
        .is_err()
    );
}

#[test]
fn una_chiave_troppo_lunga_non_si_taglia_nel_catalogo() {
    // S2-8. Chi riceve il catalogo non taglia una chiave d'identità («tagliata
    // direbbe un altro brano»: `specchio::convalida`), e rifiuta la riga se non sta
    // nel tetto. Chi lo serve la tagliava a quel tetto lo stesso, e due brani con
    // lo stesso inizio di chiave — titoli in giapponese a tre byte a carattere,
    // due album con lo stesso inizio di nome — diventavano lo stesso brano.
    use crate::specchio::convalida::{MAX_CHIAVE, convalida_riga};
    let c = libreria();
    locali(&c, [1, 2, 3]);
    let lunga = format!("artista|{}|album", "x".repeat(MAX_CHIAVE));
    // Il brano 2 ha la `track_key` oltre il tetto; il 3 ha solo la chiave di
    // contenuto oltre il tetto.
    c.execute("UPDATE tracks SET track_key = ?1 WHERE id = 2", [&lunga])
        .unwrap();
    c.execute("UPDATE tracks SET content_key = ?1 WHERE id = 3", [&lunga])
        .unwrap();
    let pagina = condivisa::pagina_catalogo(&c, 0, ID_PER_SECCHIO - 1).unwrap();
    assert_eq!(
        pagina.brani.iter().map(|b| b.id).collect::<Vec<_>>(),
        vec![1, 3],
        "una riga la cui chiave non sta nel tetto non si manda: l'altro la rifiuterebbe, o peggio la prenderebbe tagliata"
    );
    let tre = pagina.brani.iter().find(|b| b.id == 3).unwrap();
    assert_eq!(
        (tre.track_key.as_str(), tre.content_key.as_deref()),
        ("k3", None),
        "la chiave di contenuto troppo lunga non si manda tagliata: il brano ripiega su `track_key`"
    );
    // Quel che si manda, chi riceve lo prende com'è.
    for brano in &pagina.brani {
        assert_eq!(
            convalida_riga(brano.clone(), ADESSO).as_ref(),
            Ok(brano),
            "{brano:?}"
        );
    }
    // E il riassunto conta le righe della pagina, nient'altro: se contasse anche
    // quella che la pagina non manda, chi legge i secchi non si fermerebbe.
    let riassunto = condivisa::riassunto_catalogo(&c).unwrap();
    assert_eq!(riassunto.secchi.iter().map(|s| s.righe).sum::<i64>(), 2);
}

// ── S1-8: il catalogo guarda le cartelle sorvegliate di adesso ───────────────

/// Quel che un'altra macchina vede di una libreria, domanda per domanda: gli id
/// che ogni risposta nomina. Solo quel che dipende da `CONDIVISO`.
fn cosa_si_vede(c: &Connection) -> Vec<(&'static str, Vec<i64>)> {
    let id = |v: Vec<condivisa::BranoRemoto>| v.iter().map(|b| b.id).collect::<Vec<_>>();
    let riassunto = condivisa::riassunto_catalogo(c).unwrap();
    let pagina = condivisa::pagina_catalogo(c, 0, ID_PER_SECCHIO - 1).unwrap();
    vec![
        (
            "brani",
            id(condivisa::brani(c, super::TrackOrder::Title, 0, 50).unwrap()),
        ),
        (
            "brani_diretti",
            id(condivisa::brani_diretti(c, super::TrackOrder::Shelf, 0, 50).unwrap()),
        ),
        (
            "brani_album",
            id(condivisa::brani_album(c, &condivisa::impronta_album("al")).unwrap()),
        ),
        ("cerca", id(condivisa::cerca(c, "T", 0, 50).unwrap())),
        (
            "cerca_conteggio",
            vec![condivisa::cerca_conteggio(c, "T").unwrap()],
        ),
        (
            "riassunto",
            vec![riassunto.secchi.iter().map(|s| s.righe).sum()],
        ),
        (
            "pagina_catalogo",
            pagina.brani.iter().map(|b| b.id).collect(),
        ),
        (
            "album",
            vec![
                condivisa::album(c, 0, 50)
                    .unwrap()
                    .iter()
                    .map(|a| a.total_tracks)
                    .sum(),
            ],
        ),
        (
            "artisti",
            vec![
                condivisa::artisti(c, 0, 50)
                    .unwrap()
                    .iter()
                    .map(|a| a.tracks)
                    .sum(),
            ],
        ),
    ]
}

/// Una libreria con questi brani — `(id, percorso)` — e gli aggregati rifatti.
fn con_brani(brani: &[(i64, &str)]) -> Connection {
    let mut c = libreria();
    for (id, percorso) in brani {
        brano(&c, *id, percorso);
    }
    let tx = c.transaction().unwrap();
    super::rebuild_aggregates(&tx).unwrap();
    tx.commit().unwrap();
    c
}

#[test]
fn una_cartella_tolta_dalle_sorvegliate_esce_subito_dal_catalogo_condiviso() {
    // S1-8. Le righe di una cartella tolta restano in libreria: una scansione le
    // considera fuori competenza, non sparite. Il catalogo condiviso le
    // elencava, le lasciava valutare e ne serviva il testo, contro la promessa
    // «finché sta ancora sotto una cartella sorvegliata».
    //
    // Il controllo: sotto la radice, scritta com'è o in un'altra grafia, i brani si
    // vedono. Il confine di separatore conta: `C:/Musica` non sta sotto `C:/M`.
    let sotto = [(1, "C:/M/a.flac"), (2, r"c:\m\Sotto\b.flac")];
    let fuori = [(3, "D:/Vecchia/c.flac"), (4, "C:/Musica/d.flac")];
    let c = con_brani(&[sotto[0], sotto[1], fuori[0], fuori[1]]);
    let solo_sotto = con_brani(&sotto);
    assert_eq!(
        cosa_si_vede(&c),
        cosa_si_vede(&solo_sotto),
        "si vede solo quel che sta sotto `C:/M`: il 3 e il 4 no"
    );
    assert!(
        cosa_si_vede(&c)
            .iter()
            .all(|(_, v)| v.iter().any(|n| *n > 0)),
        "il controllo: qualcosa si vede, in ogni domanda"
    );
    // File e valutazioni: lo stesso.
    for visibile in [1, 2] {
        assert!(condivisa::brano(&c, visibile).unwrap().is_some());
        assert!(condivisa::file_del_brano(&c, visibile).unwrap().is_some());
    }
    for escluso in [3, 4] {
        assert!(
            condivisa::brano(&c, escluso).unwrap().is_none(),
            "{escluso}"
        );
        assert!(condivisa::file_del_brano(&c, escluso).unwrap().is_none());
        let err = condivisa::valuta(&c, escluso, Some(true), None, ADESSO - 5, ADESSO).unwrap_err();
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoFileCambiato,
            "{escluso}"
        );
    }
    let cuore: i64 = c
        .query_row("SELECT liked FROM tracks WHERE id = 3", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        cuore, 0,
        "il voto non è entrato in una riga che non si condivide"
    );

    // Il momento che conta: la cartella `C:/M` viene tolta, **senza** che una
    // scansione tocchi le righe. Da quel momento di quella cartella non si vede più niente.
    radici(&c, &["D:/Altra"]);
    assert_eq!(
        cosa_si_vede(&c),
        cosa_si_vede(&con_brani(&[])),
        "tolta la cartella, la libreria condivisa è vuota"
    );
    assert!(condivisa::brano(&c, 1).unwrap().is_none());
    assert!(condivisa::file_del_brano(&c, 1).unwrap().is_none());
    // E una che torna, torna con le sue righe.
    radici(&c, &["C:/M", "D:/Vecchia"]);
    assert_eq!(
        cosa_si_vede(&c)
            .iter()
            .find(|(d, _)| *d == "brani")
            .unwrap()
            .1,
        vec![1, 2, 3]
    );
}

#[test]
fn senza_cartelle_sorvegliate_niente_si_condivide() {
    let c = libreria();
    locali(&c, [1, 2]);
    assert_eq!(
        condivisa::brani(&c, super::TrackOrder::Title, 0, 50)
            .unwrap()
            .len(),
        2,
        "il controllo"
    );
    crate::settings::forget(&c, crate::settings::CHIAVE_CARTELLE).unwrap();
    assert!(
        condivisa::brani(&c, super::TrackOrder::Title, 0, 50)
            .unwrap()
            .is_empty()
    );
    // Un elenco che non si legge vale come nessuna cartella: non apre niente.
    crate::settings::write(&c, crate::settings::CHIAVE_CARTELLE, "{non json").unwrap();
    assert!(
        condivisa::brani(&c, super::TrackOrder::Title, 0, 50)
            .unwrap()
            .is_empty()
    );
    // E una cartella del cloud nell'elenco non rende condiviso un file di qui.
    radici(&c, &["nuvola://onedrive-abcdefgh/Musica"]);
    assert!(
        condivisa::brani(&c, super::TrackOrder::Title, 0, 50)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn le_cose_ricordate_non_sopravvivono_a_una_cartella_tolta() {
    // Le cache (l'elenco degli id, gli artisti, il riassunto del catalogo) sono
    // per generazione della libreria, e togliere una cartella non scrive su
    // `tracks`: senza le cartelle nella chiave, restavano a servire le righe
    // di quella cartella finché qualcosa non cambiava.
    let c = libreria();
    locali(&c, [1, 2, 3]);
    let cache = Mutex::new(Ricordate::default());
    let prima = super::generazione(&c).unwrap();
    let ids = |c: &Connection, cache: &Mutex<Ricordate>| {
        condivisa::brani_con_scarto(c, cache, super::TrackOrder::Title, 0, 50, 0)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&c, &cache), vec![1, 2, 3]);
    let riassunto = condivisa::riassunto_con(&c, &cache).unwrap();
    assert_eq!(riassunto.secchi.iter().map(|s| s.righe).sum::<i64>(), 3);
    let artisti = condivisa::artisti_con(&c, &cache, 0, 50).unwrap();
    assert_eq!(artisti.iter().map(|a| a.tracks).sum::<i64>(), 3);

    radici(&c, &["D:/Altra"]);
    assert_eq!(
        super::generazione(&c).unwrap(),
        prima,
        "tracks non è stata toccata"
    );
    assert_eq!(ids(&c, &cache), Vec::<i64>::new(), "l'elenco ricordato");
    let riassunto = condivisa::riassunto_con(&c, &cache).unwrap();
    assert_eq!(riassunto.secchi.len(), 0, "il riassunto ricordato");
    let artisti = condivisa::artisti_con(&c, &cache, 0, 50).unwrap();
    assert_eq!(artisti.len(), 0, "gli artisti ricordati");
}

#[test]
#[ignore = "misura"]
fn misura_il_costo_delle_cartelle_sorvegliate_sul_catalogo_condiviso() {
    // S1-8: la funzione SQL che guarda le cartelle sorvegliate di adesso gira per
    // ogni riga che passa le condizioni economiche. 50.000 brani di qui sotto
    // tre cartelle (una scritta con un'altra grafia) e 200.000 righe specchio, che
    // le condizioni economiche tolgono prima.
    use std::time::Instant;
    let c = libreria();
    radici(&c, &["C:/M", "D:/Altra", "//nas/musica"]);
    c.execute(
        r"WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50000)
         INSERT INTO tracks (id, path, track_key, title, artist, album, album_key, duration_ms,
                             file_size, date_added, date_modified)
         SELECT i,
                CASE i % 3 WHEN 0 THEN 'C:/M/Artista ' || (i % 900) || '/Album ' || (i / 12) || '/' || i || '.flac'
                           WHEN 1 THEN 'D:\Altra\Artista ' || (i % 900) || '\' || i || '.flac'
                           ELSE '\\nas\musica\' || i || '.flac' END,
                'k' || i, 'T' || i, 'Artista ' || (i % 900), 'Album ' || (i / 12), 'al' || (i / 12),
                180000, 1000, 1, 1
           FROM n",
        [],
    )
    .unwrap();
    c.execute(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 200000)
         INSERT INTO tracks (id, path, track_key, title, artist, album, album_key, duration_ms,
                             file_size, date_added, date_modified, source)
         SELECT 100000 + i, 'lontano://' || ?1 || '/' || i, 'pc' || i, 'P' || i, 'Artista',
                'Album', 'lontano:/x', 180000, 1000, 1, 1, 'lontano'
           FROM n",
        [chiave('a')],
    )
    .unwrap();
    condivisa::assicura_le_radici(&c).unwrap();
    let senza = condivisa::CONDIVISO
        .split(" AND rym_sotto_le_radici")
        .next()
        .unwrap();
    for (nome, condizione) in [
        ("senza le cartelle", senza),
        ("con le cartelle", condivisa::CONDIVISO),
    ] {
        let t = Instant::now();
        let quanti: i64 = c
            .query_row(
                &format!("SELECT COUNT(*) FROM tracks t WHERE {condizione}"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        eprintln!(
            "MISURA COUNT(*) dei brani condivisi {nome}: {quanti} righe in {} ms",
            t.elapsed().as_millis()
        );
    }
    let t = Instant::now();
    let riassunto = condivisa::riassunto_catalogo(&c).unwrap();
    eprintln!(
        "MISURA riassunto_catalogo (50.000 brani condivisi, 200.000 righe specchio): {} secchi in {} ms",
        riassunto.secchi.len(),
        t.elapsed().as_millis()
    );
}
