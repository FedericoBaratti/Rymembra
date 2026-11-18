//! Prove della revisione indipendente di F2 (ramo `m18-rev2`).
//!
//! Ogni prova qui descrive un difetto trovato leggendo il codice di F2 e **fallisce**
//! sul codice com'è: la riga dell'`assert!` dice cosa dovrebbe succedere. Le prove
//! `#[ignore]` sono misure (tempi, piani di query): si lanciano a mano con
//! `--ignored --nocapture` e stampano i numeri.
//!
//! Non correggono niente: stanno qui perché chi corregge abbia un rosso da far
//! diventare verde.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use rymembra_sync::{Contenuto, Documento, fondi};

use crate::sincronia;
use crate::specchio::archivio::{anteprima_dimentica, dimentica_dispositivo};
use crate::specchio::comuni::{ADESSO, chiave, giro, libreria, riga};

/// Il dispositivo dell'altro lato: il PC.
const PC: char = 'a';

fn n(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).expect(sql)
}

/// Un brano di questa macchina.
fn locale(c: &Connection, id: i64, track_key: &str, content_key: Option<&str>) {
    c.execute(
        "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, file_size,
                             date_added, date_modified)
         VALUES (?1, ?2, ?3, ?4, 'Titolo', 'Artista', 'Album', 1000, 1, 1)",
        params![id, format!("/musica/{id}.mp3"), track_key, content_key],
    )
    .expect("brano di qui");
}

fn playlist_con(c: &Connection, nome: &str, brani: &[i64]) {
    c.execute(
        "INSERT INTO playlists (playlist_key, name, created_at, updated_at)
         VALUES (?1, ?1, 100, 100)",
        [nome],
    )
    .expect("playlist");
    let id: i64 = c
        .query_row(
            "SELECT id FROM playlists WHERE playlist_key = ?1",
            [nome],
            |r| r.get(0),
        )
        .expect("id");
    for (posizione, brano) in brani.iter().enumerate() {
        c.execute(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
            params![id, brano, i64::try_from(posizione).expect("posizione")],
        )
        .expect("membro");
    }
}

/// Le chiavi dei brani di una playlist, nell'ordine, come stanno in `playlist_tracks`.
fn in_playlist(c: &Connection, nome: &str) -> Vec<String> {
    let mut st = c
        .prepare(
            "SELECT t.track_key FROM playlist_tracks pt
               JOIN playlists p ON p.id = pt.playlist_id
               JOIN tracks t ON t.id = pt.track_id
              WHERE p.playlist_key = ?1 ORDER BY pt.position",
        )
        .expect("ordine");
    st.query_map([nome], |r| r.get::<_, String>(0))
        .expect("ordine")
        .map(|r| r.expect("riga"))
        .collect()
}

fn documento(c: &Connection, chi: &str) -> Contenuto {
    sincronia::contenuto(c, chi, BTreeMap::new(), BTreeMap::new()).expect("contenuto")
}

/// Una passata vera di sincronia di `qui` con il documento di un altro: allinea,
/// legge, fonde, applica.
fn passata(qui: &mut Connection, io: &str, altro: (&str, Contenuto), adesso_ms: i64) {
    sincronia::allinea(qui, io, adesso_ms).expect("allineamento");
    let mio = documento(qui, io);
    let fuso = fondi(&[
        Documento::nuovo(io, mio.clone(), adesso_ms),
        Documento::nuovo(altro.0, altro.1, adesso_ms),
    ]);
    sincronia::applica_se_ferma(qui, &mio, &fuso, adesso_ms)
        .expect("applicazione")
        .expect("la libreria era ferma");
}

// ── 1. Il tag corretto qui toglie il brano dalla playlist del PC ────────────────

/// Il PC ha il brano nella playlist «sera». Il telefono ha la stessa canzone in
/// locale **e** la riga specchio di quella del PC. Una passata fra i due, poi si
/// corregge un tag sul telefono (la chiave della riga di qui cambia), e un'altra
/// passata.
fn tag_corretto_sul_telefono(con_la_riga_specchio: bool) -> (Connection, Connection, String) {
    let k = riga(1).track_key;
    let mut pc = libreria();
    locale(&pc, 1, &k, Some(&k));
    playlist_con(&pc, "sera", &[1]);
    sincronia::allinea(&mut pc, "pc", 1_000).expect("allineamento");

    let mut tel = libreria();
    locale(&tel, 1, &k, Some(&k));
    if con_la_riga_specchio {
        giro(&tel, &chiave(PC), &[riga(1)], ADESSO);
    }
    playlist_con(&tel, "sera", &[1]);
    passata(&mut tel, "telefono", ("pc", documento(&pc, "pc")), 2_000);
    passata(&mut pc, "pc", ("telefono", documento(&tel, "telefono")), 3_000);
    assert_eq!(in_playlist(&pc, "sera"), vec![k.clone()], "il punto di partenza");

    // «Correggi i tag» sul telefono: la chiave della riga di qui cambia.
    tel.execute(
        "UPDATE tracks SET track_key = 'corretta|uno|album' WHERE id = 1",
        [],
    )
    .expect("correzione");
    passata(&mut tel, "telefono", ("pc", documento(&pc, "pc")), 4_000);
    passata(&mut pc, "pc", ("telefono", documento(&tel, "telefono")), 5_000);
    (pc, tel, k)
}

#[test]
fn controllo_senza_riga_specchio_il_pc_tiene_il_brano_dopo_un_tag_corretto_sul_telefono() {
    // Il comportamento di prima di F2 (e la bussola per la prova sotto): la
    // vecchia chiave non c'è più sul telefono, la sua voce resta viva ma
    // invisibile, e il PC non perde niente.
    let (pc, _tel, k) = tag_corretto_sul_telefono(false);
    assert_eq!(in_playlist(&pc, "sera"), vec![k]);
}

#[test]
fn rev2_01_un_tag_corretto_sul_telefono_non_toglie_il_brano_dalla_playlist_del_pc() {
    // DIFETTO (Alta, perdita di dati su un ALTRO dispositivo). Con la riga specchio
    // la vecchia chiave «c'è ancora» per il telefono (`brani_assenti` conta le righe
    // specchio come righe), quindi la voce viva che non sta più in `playlist_tracks`
    // si legge come «l'utente l'ha tolta»: lapide, che viaggia. Sul PC la playlist
    // si rifà dalla sequenza fusa: la vecchia chiave è tolta, la nuova il PC non
    // ce l'ha, e il brano sparisce dalla sua playlist. F2 lo dichiara come
    // «limite» (invarianti.rs, `fsync_limite_dichiarato_…`) e la sua prova asserisce
    // la perdita: qui si asserisce che non deve succedere.
    let (pc, _tel, k) = tag_corretto_sul_telefono(true);
    assert_eq!(
        in_playlist(&pc, "sera"),
        vec![k],
        "il PC ha perso dalla sua playlist un brano che il suo proprietario non ha toccato"
    );
}

// ── 2. «Dimentica dispositivo» non trova la copia che c'è ────────────────────────

/// Il telefono ha il file di qui, ben taggato, e la riga specchio del PC, dove il
/// tag era sbagliato e l'utente l'ha corretto: lì `content_key` (tag grezzi) e
/// `track_key` (corretto) sono diversi. La playlist del telefono ha la riga specchio.
fn telefono_con_copia_e_riga_specchio_di_tag_corretto() -> (Connection, String) {
    let k = "beatles|help|help".to_owned();
    let mut r = riga(1);
    r.track_key = k.clone();
    r.content_key = Some("beatls|help|help".to_owned());
    let c = libreria();
    locale(&c, 1, &k, Some(&k));
    giro(&c, &chiave(PC), &[r], ADESSO);
    let specchio: i64 = c
        .query_row(
            "SELECT track_id FROM lontano_brani WHERE dispositivo = ?1",
            [chiave(PC)],
            |r| r.get(0),
        )
        .expect("riga specchio");
    playlist_con(&c, "sera", &[specchio]);
    (c, k)
}

#[test]
fn rev2_02a_dimenticare_il_pc_rimappa_la_voce_sulla_copia_di_qui_anche_se_la_chiave_di_contenuto_del_pc_e_un_altra()
 {
    // DIFETTO (Media→Alta, vedi 02b). `voci_con_alternativa` cerca l'alternativa con
    // la chiave di CONTENUTO del proprietario (`l.content_key`), contro
    // `t.content_key`/`t.track_key` di qui; ma `applica_playlist`, la sincronia e
    // `remove_tracks` abbinano per `track_key`. Quando il proprietario ha corretto
    // un tag le due chiavi divergono, e la copia di qui — stessa `track_key` — non
    // si trova: l'anteprima dice «perde» e la voce si butta.
    let (c, _k) = telefono_con_copia_e_riga_specchio_di_tag_corretto();
    let anteprima = anteprima_dimentica(&c, &chiave(PC)).expect("anteprima");
    assert_eq!(
        anteprima.rimappabili, 1,
        "la copia di qui (stessa track_key) c'è, la voce non andava persa: {anteprima:?}"
    );
}

#[test]
fn rev2_02b_dimenticare_il_pc_non_fa_partire_una_lapide_per_la_playlist_degli_altri() {
    // La conseguenza: la voce persa nella cascata, ma la chiave c'è ancora in
    // `tracks` (la copia di qui), quindi per la sincronia non è un brano «che
    // manca»: è un brano che l'utente ha tolto dalla playlist. Lapide, che viaggia
    // fino al PC che il brano ce l'ha.
    let (mut c, k) = telefono_con_copia_e_riga_specchio_di_tag_corretto();
    sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
    dimentica_dispositivo(&c, &chiave(PC)).expect("dimenticato");
    sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
    let doc = documento(&c, "telefono");
    let tolte = doc
        .playlist
        .get("sera")
        .expect("la playlist viaggia")
        .sequenza
        .elementi()
        .filter(|(_, e)| e.tolto.is_some())
        .count();
    assert_eq!(
        tolte, 0,
        "dimenticare il PC ha fatto partire la lapide di {k} per le playlist degli altri"
    );
}

// ── 3. Una riga sparita sotto la soglia si toglie subito e porta via la voce ─────

#[test]
fn rev2_03_una_riga_che_il_pc_toglie_e_rimette_ritrova_il_suo_posto_nella_playlist() {
    // DIFETTO (Alta con la sincronia spenta — che è il default). Il PC non manda
    // più due brani per un giro (un disco esterno staccato, una libreria rifatta) e
    // poi li rimanda, con id nuovi. Sotto la soglia (50 righe / un quinto) la riga
    // specchio si toglie al primo giro (`togli_le_sparite`) e la cascata svuota la
    // voce di playlist. Per un file di qui il trigger della 028
    // (`statistiche_restano_orfane`/`statistiche_tornano`) la rimetterebbe al suo
    // posto; per una riga specchio `content_key` resta NULL **di proposito**, quindi
    // niente la ricorda. Con la sincronia accesa la sequenza la tiene e la riaggancia
    // (FSYNC); senza — il default, `sync_sequenza` vuota — la voce è persa per sempre.
    let k2 = riga(2).track_key;
    let c = libreria();
    // Cinque brani del PC: toglierne uno è sotto la soglia di cinquanta.
    let tutte: Vec<_> = (1..=5).map(riga).collect();
    giro(&c, &chiave(PC), &tutte, ADESSO);
    let id2: i64 = c
        .query_row(
            "SELECT track_id FROM lontano_brani WHERE id_remoto = 2",
            [],
            |r| r.get(0),
        )
        .expect("riga 2");
    playlist_con(&c, "sera", &[id2]);

    // Il PC non manda il brano 2: la riga se ne va subito.
    let senza: Vec<_> = tutte.iter().filter(|r| r.id != 2).cloned().collect();
    let chiusura = giro(&c, &chiave(PC), &senza, ADESSO + 10_000);
    assert_eq!(chiusura.tolte, 1, "{chiusura:?}");

    // E poi lo rimanda, con un id nuovo (la libreria del PC ricostruita).
    let mut di_nuovo = riga(2);
    di_nuovo.id = 902;
    let mut tutte_di_nuovo = senza.clone();
    tutte_di_nuovo.push(di_nuovo);
    giro(&c, &chiave(PC), &tutte_di_nuovo, ADESSO + 20_000);
    assert_eq!(
        in_playlist(&c, "sera"),
        vec![k2],
        "la voce della playlist non è tornata col brano"
    );
}

#[test]
fn controllo_un_file_locale_che_torna_ritrova_il_suo_posto_nella_playlist() {
    // La bussola: per un file di qui i trigger delle statistiche orfane rimettono
    // la voce al suo posto. (Passa: è quel che la riga specchio non fa.)
    let c = libreria();
    let k = riga(2).track_key;
    locale(&c, 2, &k, Some(&k));
    playlist_con(&c, "sera", &[2]);
    c.execute("DELETE FROM tracks WHERE id = 2", []).expect("via");
    assert!(in_playlist(&c, "sera").is_empty());
    locale(&c, 3, &k, Some(&k));
    assert_eq!(in_playlist(&c, "sera"), vec![k]);
}

// ── 4. Android Auto: un brano con una copia raggiungibile sparisce ───────────────

#[test]
fn rev2_04_in_auto_un_brano_resta_se_un_dispositivo_che_risponde_ne_ha_una_copia() {
    // DIFETTO (Bassa/Media, Android Auto). Il brano sta su due PC, nessuna copia di
    // qui. Il PC1 (flac) è il rappresentante e non risponde; il PC2 (mp3) risponde.
    // La riga del PC2 ha `copia_di = riga del PC1`, e `Origine::Raggiungibili` la
    // mostra solo se `copia_di IS NULL`: il brano sparisce dall'auto, mentre
    // `copia_da_suonare` lo suonerebbe dal PC2. Il ripiego c'è solo per le righe
    // di questa macchina.
    use crate::specchio::Origine;
    let c = libreria();
    let (pc1, pc2) = (chiave('a'), chiave('b'));
    let mut flac = riga(1);
    flac.codec = Some("flac".to_owned());
    flac.estensione = "flac".to_owned();
    flac.sample_rate = Some(96_000);
    giro(&c, &pc1, &[flac], ADESSO);
    giro(&c, &pc2, &[riga(1)], ADESSO);
    assert_eq!(n(&c, "SELECT COUNT(*) FROM tracks WHERE copia_di IS NOT NULL"), 1);
    let sql = Origine::Raggiungibili(vec![pc2]).sql("t.").expect("origine");
    let in_auto = n(&c, &format!("SELECT COUNT(*) FROM tracks t WHERE {sql}"));
    assert_eq!(
        in_auto, 1,
        "il brano ha una copia sul PC2 che risponde, e in auto non c'è"
    );
}

// ── 5. Un secchio in ritardo riporta le righe di un dispositivo dimenticato ──────

#[test]
fn rev2_05_un_secchio_arrivato_dopo_il_dimentica_non_riporta_le_righe() {
    // DIFETTO (Media). `lontano::specchio::giro` controlla `associato` PRIMA della
    // domanda (che può durare fino a `ATTESA_SECCHIO` = 30 s) e poi scrive il
    // secchio senza ricontrollare: se in quei secondi la persona dimentica il
    // dispositivo (`dimentica_dispositivo`), `applica_secchio` e `chiudi` rimettono
    // le sue righe e la sua riga di `lontano_cataloghi`. Restano in libreria, in
    // ricerca e nei doppioni, di un dispositivo che non esiste più, finché non si
    // ricollega. Il nucleo non ha nessuna guardia: la riga di `dispositivi` non
    // c'è più e lui scrive lo stesso. (Il codice del guscio: specchio.rs, righe
    // `if !dati.associato(chiave)` e `dati.scrivi(|c| in_corso.applica_secchio`.)
    let c = libreria();
    let pc = chiave(PC);
    c.execute(
        "INSERT INTO dispositivi (chiave, nome, associato_ms) VALUES (?1, 'PC', 1)",
        [&pc],
    )
    .expect("dispositivo");
    giro(&c, &pc, &[riga(1), riga(2)], ADESSO);
    // Il gesto «Dimentica» com'è nel guscio: prima la riga di `dispositivi`, poi lo specchio.
    crate::dispositivi::dimentica(&c, &pc).expect("dimentica");
    dimentica_dispositivo(&c, &pc).expect("specchio dimenticato");
    assert_eq!(n(&c, "SELECT COUNT(*) FROM lontano_brani"), 0);
    // Il secchio che era in volo arriva.
    giro(&c, &pc, &[riga(1), riga(2)], ADESSO + 1_000);
    assert_eq!(
        n(&c, "SELECT COUNT(*) FROM lontano_brani"),
        0,
        "le righe di un dispositivo dimenticato sono tornate"
    );
    assert_eq!(n(&c, "SELECT COUNT(*) FROM lontano_cataloghi"), 0);
}

// ── 6. La ricetta di ritorno 43→42 lascia album fantasma ─────────────────────────

/// La ricetta scritta in testa alla 043 (e provata da `db::tests`).
const RITORNO_DALLA_43: &str = "DROP TABLE dispositivi_posta; DROP TABLE lontano_brani;
     DROP TABLE lontano_secchi; DROP TABLE lontano_cataloghi;
     DELETE FROM tracks WHERE path >= 'lontano://' AND path < 'lontano:/0';
     DROP INDEX idx_tracks_copia_di; ALTER TABLE tracks DROP COLUMN copia_di;
     PRAGMA user_version = 42;";

#[test]
fn rev2_06_il_ritorno_alla_42_non_lascia_album_senza_brani() {
    // DIFETTO (Bassa/Media). La ricetta toglie le righe specchio ma non rifà
    // `albums`/`artists`, che `ricostruisci_aggregati` aveva riempito anche per
    // loro: la 042 si apre con album e artisti che non hanno più nessun brano. La
    // prova di F2 confronta lo schema e le righe di `tracks`, non gli aggregati.
    let c = libreria();
    locale(&c, 1, &riga(1).track_key, Some(&riga(1).track_key));
    giro(&c, &chiave(PC), &(10..=14).map(riga).collect::<Vec<_>>(), ADESSO);
    c.execute_batch(RITORNO_DALLA_43).expect("ritorno");
    let fantasmi = n(
        &c,
        "SELECT COUNT(*) FROM albums a
          WHERE NOT EXISTS (SELECT 1 FROM tracks t WHERE t.album_key = a.album_key)",
    );
    assert_eq!(fantasmi, 0, "album senza nessun brano dopo il ritorno");
}

#[test]
fn rev2_07_la_ricetta_di_ritorno_non_lascia_voci_che_puntano_nel_vuoto_a_chiavi_spente() {
    // DIFETTO (Bassa, istruzione mancante). La ricetta si affida alla cascata e non
    // dice di accendere `PRAGMA foreign_keys = ON`: dal programma `sqlite3` (che le
    // tiene spente di serie) le voci di playlist e la cronologia delle righe
    // cancellate restano a puntare nel vuoto, e `tracks.id` (senza AUTOINCREMENT) può
    // riusare quegli id per brani nuovi.
    let c = libreria();
    giro(&c, &chiave(PC), &[riga(10)], ADESSO);
    let specchio: i64 = c
        .query_row("SELECT track_id FROM lontano_brani", [], |r| r.get(0))
        .expect("riga");
    playlist_con(&c, "sera", &[specchio]);
    c.execute_batch("PRAGMA foreign_keys = OFF").expect("come sqlite3");
    c.execute_batch(RITORNO_DALLA_43).expect("ritorno");
    let nel_vuoto = n(
        &c,
        "SELECT COUNT(*) FROM playlist_tracks pt
          WHERE NOT EXISTS (SELECT 1 FROM tracks t WHERE t.id = pt.track_id)",
    );
    assert_eq!(nel_vuoto, 0, "voci di playlist che puntano a un brano che non c'è");
}
