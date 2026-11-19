//! Le correzioni del collaudo del 18/11 sulla libreria unica (FC1): la riga con
//! una copia di qui (D1) e lo stesso album su due dispositivi (D4).
//!
//! La scena: il telefono ha di qui l'album «Formati» da tre brani, con le sue
//! chiavi di cartella; il PC manda lo stesso album — titolo e artista uguali, le
//! chiavi del PC — con i brani 1 e 2 uguali a quelli di qui e un quarto solo suo.

use rusqlite::{Connection, params};
use rymembra_domain::album::album_group_key;

use crate::library::{
    TrackOrder, album_tracks, albums_by_artist, counts, list_albums, list_tracks,
    origine_dell_album, rebuild_aggregates, recently_added_albums,
};
use crate::specchio::Origine;
use crate::specchio::comuni::{ADESSO, chiave, giro, libreria, riga};

const PC: char = 'a';

/// Un brano di qui dell'album «Formati»: la chiave d'album è quella della sua
/// cartella, come la scrive la scansione.
fn locale_formati(c: &Connection, id: i64, numero: i64, bitrate: i64) {
    let percorso = format!("/musica/Formati/{numero}.mp3");
    c.execute(
        "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, album_key,
                             track_number, file_size, date_added, date_modified, codec, bitrate,
                             sample_rate, duration_ms)
         VALUES (?1, ?2, ?3, ?3, ?4, 'Artista', 'Formati', ?5, ?6, 8000000, 1, 1, 'Mpeg', ?7,
                 44100, 200000)",
        params![
            id,
            percorso,
            riga(numero).track_key,
            format!("Titolo {numero}"),
            album_group_key("Formati", &percorso),
            numero,
            bitrate,
        ],
    )
    .expect("brano di qui");
}

/// Il catalogo del PC: l'album «Formati» con i brani `numeri`.
fn formati_del_pc(numeri: &[i64]) -> Vec<crate::library::condivisa::BranoCatalogo> {
    numeri
        .iter()
        .map(|n| {
            let mut r = riga(*n);
            r.album = "Formati".to_owned();
            r.album_key = Some("f".repeat(32));
            r
        })
        .collect()
}

fn ricostruisci(c: &Connection) {
    let tx = c.unchecked_transaction().expect("transazione");
    rebuild_aggregates(&tx).expect("aggregati");
    tx.commit().expect("commit");
}

/// Il telefono della scena, con il giro del PC già fatto. Restituisce le righe
/// specchio dei brani 1, 2 e 4 del PC.
fn telefono_con_il_pc() -> (Connection, i64, i64, i64) {
    let c = libreria();
    // Di qui: tre brani a 128 kbps, quindi peggiori di quelli del PC (320).
    for n in 1..=3 {
        locale_formati(&c, n, n, 128);
    }
    ricostruisci(&c);
    giro(&c, &chiave(PC), &formati_del_pc(&[1, 2, 4]), ADESSO);
    let id = |n: i64| {
        c.query_row(
            "SELECT track_id FROM lontano_brani WHERE dispositivo = ?1 AND id_remoto = ?2",
            params![chiave(PC), n],
            |r| r.get::<_, i64>(0),
        )
        .expect("riga specchio")
    };
    let (m1, m2, m4) = (id(1), id(2), id(4));
    (c, m1, m2, m4)
}

// ── D1 ──────────────────────────────────────────────────────────────────────

/// Una riga specchio che ha una copia di qui nascosta dietro lo dice (D1): così
/// la finestra non la disegna in grigio e il tocco non dice «non raggiungibile»
/// col PC spento, e il nucleo — che ha il ripiego — suona la copia di qui.
#[test]
fn una_riga_specchio_con_una_copia_di_qui_dietro_lo_dice() {
    let (c, m1, m2, m4) = telefono_con_il_pc();
    let tutti = list_tracks(&c, &Origine::Tutti, TrackOrder::Title, 0, 50).expect("elenco");
    let per_id = |id: i64| tutti.iter().find(|b| b.id == id).expect("riga in «Tutti»");

    assert!(
        per_id(m1).copia_qui,
        "il brano 1 ha la sua copia di qui dietro"
    );
    assert!(per_id(m2).copia_qui, "e il 2");
    assert!(
        !per_id(m4).copia_qui,
        "il quarto è solo del PC: non c'è niente da suonare di qui"
    );
    // Le righe di qui dei brani 1 e 2 sono nascoste dietro quelle del PC.
    assert!(!tutti.iter().any(|b| b.id == 1 || b.id == 2), "{tutti:?}");

    // La stessa riga, vista dal filtro del PC, dice lo stesso: dipende dalla riga.
    let del_pc = list_tracks(
        &c,
        &Origine::Dispositivo(chiave(PC)),
        TrackOrder::Title,
        0,
        50,
    )
    .expect("elenco del PC");
    assert!(
        del_pc
            .iter()
            .find(|b| b.id == m1)
            .is_some_and(|b| b.copia_qui)
    );

    // Una riga di qui non lo dice mai: è già quella che suona.
    let di_qui = list_tracks(&c, &Origine::Locale, TrackOrder::Title, 0, 50).expect("di qui");
    assert_eq!(di_qui.len(), 3);
    assert!(di_qui.iter().all(|b| !b.copia_qui));
}

/// Con il PC che non manda più il brano, la copia di qui torna visibile e la riga
/// specchio non ha più niente dietro.
#[test]
fn senza_la_copia_di_qui_la_riga_non_lo_dice() {
    let c = libreria();
    giro(&c, &chiave(PC), &formati_del_pc(&[1, 2]), ADESSO);
    let m1: i64 = c
        .query_row(
            "SELECT track_id FROM lontano_brani WHERE id_remoto = 1",
            [],
            |r| r.get(0),
        )
        .expect("riga");
    let tutti = list_tracks(&c, &Origine::Tutti, TrackOrder::Title, 0, 50).expect("elenco");
    assert!(
        tutti
            .iter()
            .find(|b| b.id == m1)
            .is_some_and(|b| !b.copia_qui)
    );
}

// ── D4 ──────────────────────────────────────────────────────────────────────

/// Lo stesso album su due dispositivi è **una** scheda in «Tutti», coi brani scelti
/// dai doppioni (D4): prima erano due, «Formati da 6 brani» e «Formati da 2».
#[test]
fn lo_stesso_album_su_due_dispositivi_e_una_scheda_sola() {
    let (c, m1, m2, m4) = telefono_con_il_pc();
    let schede = list_albums(&c, &Origine::Tutti, 0, 50).expect("album");
    let formati: Vec<_> = schede.iter().filter(|a| a.title == "Formati").collect();
    assert_eq!(formati.len(), 1, "una scheda sola: {schede:?}");
    // I brani visibili sono quelli scelti: 1 e 2 dal PC (320 batte 128), il 3 di qui
    // (solo qui) e il 4 del PC (solo là): quattro, non otto.
    assert_eq!(
        formati[0].total_tracks, 4,
        "il conto dei brani che «Tutti» mostra"
    );
    let brani = album_tracks(&c, &Origine::Tutti, &formati[0].album_key).expect("brani");
    let mut ids: Vec<i64> = brani.iter().map(|b| b.id).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![3, m1, m2, m4], "{brani:?}");

    // Il conteggio di «Tutti» e la pagina dell'artista dicono lo stesso.
    assert_eq!(counts(&c, &Origine::Tutti).expect("conti").albums, 1);
    let dell_artista = albums_by_artist(&c, &Origine::Tutti, "Artista", 0, 50).expect("artista");
    assert_eq!(
        dell_artista.len(),
        1,
        "un disco per due dispositivi: un album"
    );
    assert_eq!(dell_artista[0].total_tracks, 4);
    let recenti = recently_added_albums(&c, &Origine::Tutti, 12).expect("recenti");
    assert_eq!(recenti.iter().filter(|a| a.title == "Formati").count(), 1);

    // Con i filtri la stessa scheda conta quel che il filtro mostra.
    let di_qui = list_albums(&c, &Origine::Locale, 0, 50).expect("di qui");
    assert_eq!(
        di_qui
            .iter()
            .find(|a| a.title == "Formati")
            .map(|a| a.total_tracks),
        Some(3)
    );
    let del_pc = list_albums(&c, &Origine::Dispositivo(chiave(PC)), 0, 50).expect("del pc");
    assert_eq!(
        del_pc
            .iter()
            .find(|a| a.title == "Formati")
            .map(|a| a.total_tracks),
        Some(3),
        "i brani 1, 2 e 4 del PC"
    );

    // Un album che sta su più macchine si apre da «Tutti».
    assert_eq!(
        origine_dell_album(&c, &formati[0].album_key).expect("origine"),
        Origine::Tutti
    );
}

/// La chiave allineata è quella di qui, e una seconda chiamata non cambia
/// niente: l'allineamento è idempotente.
#[test]
fn l_allineamento_e_idempotente_e_prende_la_chiave_di_qui() {
    let (c, _, _, _) = telefono_con_il_pc();
    let di_qui = album_group_key("Formati", "/musica/Formati/1.mp3");
    let diverse: i64 = c
        .query_row(
            "SELECT COUNT(DISTINCT album_key) FROM tracks WHERE album = 'Formati'",
            [],
            |r| r.get(0),
        )
        .expect("conto");
    assert_eq!(diverse, 1, "una chiave sola per le righe di tutti");
    let chiave_scelta: String = c
        .query_row("SELECT album_key FROM tracks WHERE id = 1", [], |r| {
            r.get(0)
        })
        .expect("chiave");
    assert_eq!(chiave_scelta, di_qui, "la chiave di qui non si tocca");
    let tx = c.unchecked_transaction().expect("transazione");
    let mut toccati = crate::library::Toccati::nessuno();
    assert_eq!(
        super::archivio::riallinea_le_chiavi_d_album(&tx, &mut toccati).expect("seconda"),
        0
    );
    assert!(toccati.e_vuoto());
    tx.commit().expect("commit");
    // E in `albums` c'è una riga sola, quella di qui.
    let schede: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM albums WHERE title = 'Formati'",
            [],
            |r| r.get(0),
        )
        .expect("album");
    assert_eq!(schede, 1);
}

/// Una riga che `aggiorna` riscrive col suo album_key di prima si riallinea nella
/// stessa chiusura, e le schede restano una.
#[test]
fn una_riga_riscritta_dal_proprietario_si_riallinea_nello_stesso_giro() {
    let (c, _, _, _) = telefono_con_il_pc();
    // Il PC cambia il brano 1 (un titolo corretto): `aggiorna` rimette la chiave
    // di base della riga specchio, e il giro la rimette a posto.
    let mut cambiato = formati_del_pc(&[1, 2, 4]);
    cambiato[0].title = "Titolo uno corretto".to_owned();
    giro(&c, &chiave(PC), &cambiato, ADESSO + 60_000);
    let schede = list_albums(&c, &Origine::Tutti, 0, 50).expect("album");
    assert_eq!(
        schede.iter().filter(|a| a.title == "Formati").count(),
        1,
        "{schede:?}"
    );
    let diverse: i64 = c
        .query_row(
            "SELECT COUNT(DISTINCT album_key) FROM tracks WHERE album = 'Formati'",
            [],
            |r| r.get(0),
        )
        .expect("conto");
    assert_eq!(diverse, 1);
}

/// Due dischi di qui con lo stesso nome restano due dischi, anche se il PC ne ha
/// uno: l'allineamento non fonde mai due gruppi di qui.
#[test]
fn due_dischi_di_qui_con_lo_stesso_nome_non_si_fondono() {
    let c = libreria();
    for (id, numero, cartella) in [(1, 1, "Formati"), (2, 2, "Formati bis")] {
        let percorso = format!("/musica/{cartella}/{numero}.mp3");
        c.execute(
            "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album,
                                 album_key, file_size, date_added, date_modified)
             VALUES (?1, ?2, ?3, ?3, ?4, 'Artista', 'Formati', ?5, 1000, 1, 1)",
            params![
                id,
                percorso,
                riga(numero).track_key,
                format!("Titolo {numero}"),
                album_group_key("Formati", &percorso),
            ],
        )
        .expect("brano di qui");
    }
    ricostruisci(&c);
    giro(&c, &chiave(PC), &formati_del_pc(&[3]), ADESSO);
    let di_qui: i64 = c
        .query_row(
            "SELECT COUNT(DISTINCT album_key) FROM tracks WHERE id IN (1, 2)",
            [],
            |r| r.get(0),
        )
        .expect("conto");
    assert_eq!(di_qui, 2, "le due cartelle di qui restano due dischi");
}

/// Un disco che sta solo su due dispositivi di fuori, senza righe di qui, è una
/// scheda sola: le righe del dispositivo con la chiave più alta prendono la minore.
#[test]
fn lo_stesso_album_su_due_dispositivi_di_fuori_e_una_scheda_sola() {
    let c = libreria();
    let (a, b) = (chiave('a'), chiave('b'));
    giro(&c, &a, &formati_del_pc(&[1, 2]), ADESSO);
    let mut del_b = formati_del_pc(&[1, 3]);
    for r in &mut del_b {
        r.album_key = Some("e".repeat(32));
    }
    giro(&c, &b, &del_b, ADESSO);
    let schede = list_albums(&c, &Origine::Tutti, 0, 50).expect("album");
    assert_eq!(
        schede.iter().filter(|s| s.title == "Formati").count(),
        1,
        "{schede:?}"
    );
}

/// §4.7: un disco di qui con righe specchio unite non dice a chi sfoglia di qui
/// brani che non sono suoi: il conto condiviso guarda solo le righe di qui.
#[test]
fn il_conto_di_un_disco_condiviso_non_conta_le_righe_degli_altri_dispositivi() {
    let (c, _, _, _) = telefono_con_il_pc();
    let condivisi = crate::library::condivisa::album(&c, 0, 50).expect("album condivisi");
    let formati = condivisi
        .iter()
        .find(|a| a.title == "Formati")
        .expect("il disco di qui");
    assert_eq!(
        formati.total_tracks, 3,
        "tre brani di qui, non i quattro di tutti: {condivisi:?}"
    );
}
