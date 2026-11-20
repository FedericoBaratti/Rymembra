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

use crate::db::RITORNO_DALLA_43;
use crate::sincronia;
use crate::specchio::archivio::{GRAZIA_MS, anteprima_dimentica, dimentica_dispositivo};
use crate::specchio::comuni::{ADESSO, chiave, giro, giro_che_puo_fallire, libreria, riga};

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
    passata(
        &mut pc,
        "pc",
        ("telefono", documento(&tel, "telefono")),
        3_000,
    );
    assert_eq!(
        in_playlist(&pc, "sera"),
        vec![k.clone()],
        "il punto di partenza"
    );

    // «Correggi i tag» sul telefono: la chiave della riga di qui cambia.
    tel.execute(
        "UPDATE tracks SET track_key = 'corretta|uno|album' WHERE id = 1",
        [],
    )
    .expect("correzione");
    passata(&mut tel, "telefono", ("pc", documento(&pc, "pc")), 4_000);
    passata(
        &mut pc,
        "pc",
        ("telefono", documento(&tel, "telefono")),
        5_000,
    );
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

    // Il PC non manda il brano 2: la riga non si toglie subito ma entra in grazia
    // (la voce deve poter ritrovare il suo posto), e per i giorni della grazia
    // non si suona e non si conta da nessuna parte.
    let senza: Vec<_> = tutte.iter().filter(|r| r.id != 2).cloned().collect();
    let chiusura = giro(&c, &chiave(PC), &senza, ADESSO + 10_000);
    assert_eq!((chiusura.tolte, chiusura.in_grazia), (0, 1), "{chiusura:?}");
    assert_eq!(
        n(
            &c,
            &format!("SELECT COUNT(*) FROM tracks WHERE id = {id2} AND copia_di = id")
        ),
        1,
        "la riga in grazia è nascosta"
    );

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
    c.execute("DELETE FROM tracks WHERE id = 2", [])
        .expect("via");
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
    assert_eq!(
        n(&c, "SELECT COUNT(*) FROM tracks WHERE copia_di IS NOT NULL"),
        1
    );
    let sql = Origine::Raggiungibili(vec![pc2])
        .sql("t.")
        .expect("origine");
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
    // Il secchio che era in volo arriva: il nucleo, che controlla `dispositivi`
    // nella transazione che scrive, lo rifiuta.
    assert!(
        giro_che_puo_fallire(&c, &pc, &[riga(1), riga(2)], ADESSO + 1_000).is_err(),
        "un secchio di un dispositivo dimenticato non si scrive"
    );
    assert_eq!(
        n(&c, "SELECT COUNT(*) FROM lontano_brani"),
        0,
        "le righe di un dispositivo dimenticato sono tornate"
    );
    assert_eq!(n(&c, "SELECT COUNT(*) FROM lontano_cataloghi"), 0);
}

// ── 6. La ricetta di ritorno 43→42 lascia album fantasma ─────────────────────────

// `RITORNO_DALLA_43` è la ricetta scritta in testa alla 043: le prove di `db`
// dicono che è quella che la migrazione porta (`le_ricette_di_ritorno_stanno_in_testa_alla_quarantatre`),
// così queste provano quel che la migrazione davvero dice.

#[test]
fn rev2_06_il_ritorno_alla_42_non_lascia_album_senza_brani() {
    // DIFETTO (Bassa/Media). La ricetta toglie le righe specchio ma non rifà
    // `albums`/`artists`, che `ricostruisci_aggregati` aveva riempito anche per
    // loro: la 042 si apre con album e artisti che non hanno più nessun brano. La
    // prova di F2 confronta lo schema e le righe di `tracks`, non gli aggregati.
    let c = libreria();
    locale(&c, 1, &riga(1).track_key, Some(&riga(1).track_key));
    giro(
        &c,
        &chiave(PC),
        &(10..=14).map(riga).collect::<Vec<_>>(),
        ADESSO,
    );
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
    c.execute_batch("PRAGMA foreign_keys = OFF")
        .expect("come sqlite3");
    c.execute_batch(RITORNO_DALLA_43).expect("ritorno");
    let nel_vuoto = n(
        &c,
        "SELECT COUNT(*) FROM playlist_tracks pt
          WHERE NOT EXISTS (SELECT 1 FROM tracks t WHERE t.id = pt.track_id)",
    );
    assert_eq!(
        nel_vuoto, 0,
        "voci di playlist che puntano a un brano che non c'è"
    );
}

// ── misure (ignorate di serie: `--ignored --nocapture`) ──────────────────────────

use std::time::Instant;

/// `quanti` brani di questa macchina, con le chiavi di [`riga`] a partire da `da`.
fn tanti_locali(c: &Connection, da: i64, quanti: i64) {
    let tx = c.unchecked_transaction().expect("transazione");
    {
        let mut st = tx
            .prepare(
                "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album,
                                     album_key, file_size, date_added, date_modified, codec,
                                     bitrate, sample_rate, play_count, last_played_at)
                 VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, 1000, ?1, 1, 'mp3', 320, 44100, ?8, ?9)",
            )
            .expect("preparata");
        for id in da..da + quanti {
            st.execute(params![
                id,
                format!("/musica/{}/{}.mp3", id.div_euclid(12), id),
                format!(
                    "artista {}|titolo {id}|album {}",
                    id % 900,
                    id.div_euclid(12)
                ),
                format!("Titolo {id}"),
                format!("Artista {}", id % 900),
                format!("Album {}", id.div_euclid(12)),
                format!("{:032x}", id.div_euclid(12)),
                id % 7,
                (id % 3 == 0).then_some(1_700_000_000_000 + id),
            ])
            .expect("riga");
        }
    }
    tx.commit().expect("commit");
}

#[test]
#[ignore = "misura"]
fn misura_la_043_su_un_database_alla_42_di_sessantamila_brani() {
    use crate::db::{MIGRATIONS, open};
    let dir = tempfile::tempdir().expect("cartella");
    let percorso = dir.path().join("rymembra.db");
    {
        let c = Connection::open(&percorso).expect("file");
        c.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .expect("pragma");
        for m in MIGRATIONS.iter().filter(|m| m.version <= 42) {
            c.execute_batch(m.sql).expect("migrazione");
        }
        c.execute_batch("PRAGMA user_version = 42")
            .expect("versione");
        tanti_locali(&c, 1, 60_000);
        let tx = c.unchecked_transaction().expect("tx");
        for p in 1..=300_i64 {
            tx.execute(
                "INSERT INTO playlists (id, playlist_key, name, created_at, updated_at)
                 VALUES (?1, ?2, ?2, 1, 1)",
                params![p, format!("p{p}")],
            )
            .expect("playlist");
            for pos in 0..100_i64 {
                tx.execute(
                    "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                     VALUES (?1, ?2, ?3)",
                    params![p, (p * 197 + pos * 13) % 60_000 + 1, pos],
                )
                .expect("voce");
            }
        }
        for h in 0..150_000_i64 {
            tx.execute(
                "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, ?2, 200000)",
                params![h % 60_000 + 1, 1_700_000_000_000 + h],
            )
            .expect("ascolto");
        }
        tx.commit().expect("commit");
        // Le chiavi d'ordine ci sono, come in un database vero alla 042.
        c.execute_batch("UPDATE tracks SET ordine_titolo = lower(title), ordine_artista = lower(artist), ordine_album = lower(album)").expect("chiavi");
        c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .expect("checkpoint");
    }
    let dimensione = std::fs::metadata(&percorso).map(|m| m.len()).unwrap_or(0);
    // Il solo SQL della 043, su una copia, in una transazione come fa migrate.
    let copia = dir.path().join("copia.db");
    std::fs::copy(&percorso, &copia).expect("copia");
    {
        let c = Connection::open(&copia).expect("copia aperta");
        c.execute_batch("PRAGMA foreign_keys = ON").expect("fk");
        let t = Instant::now();
        let tx = c.unchecked_transaction().expect("tx");
        tx.execute_batch(MIGRATIONS[42].sql).expect("043");
        tx.commit().expect("commit");
        eprintln!(
            "MISURA solo l'SQL della 043: {} ms",
            t.elapsed().as_millis()
        );
    }
    let t = Instant::now();
    let aperto = open(&percorso).expect("apertura con migrazione");
    eprintln!(
        "MISURA 043: {} ms (db {} MiB, applicate {}, istantanea {:?})",
        t.elapsed().as_millis(),
        dimensione.div_euclid(1 << 20),
        aperto.applied,
        aperto.istantanea.is_some()
    );
    assert_eq!(aperto.version_before, 42);
}

#[test]
#[ignore = "misura"]
fn misura_i_piani_e_i_tempi_con_e_senza_dispositivi() {
    use crate::library::{
        TrackOrder, counts, id_dei_brani, list_albums, list_artists, list_tracks, search,
    };
    use crate::specchio::Origine;
    let c = libreria();
    tanti_locali(&c, 1, 60_000);
    let tempo = |nome: &str, f: &dyn Fn()| {
        let t = Instant::now();
        f();
        eprintln!("MISURA {nome}: {} ms", t.elapsed().as_millis());
    };
    let esegui = |c: &Connection| {
        for ordine in [
            TrackOrder::Title,
            TrackOrder::Shelf,
            TrackOrder::RecentlyAdded,
        ] {
            tempo(&format!("id_dei_brani {ordine:?}"), &|| {
                let v = id_dei_brani(c, &Origine::Tutti, ordine).expect("id");
                assert!(v.len() > 1000);
            });
            tempo(&format!("list_tracks 50 {ordine:?}"), &|| {
                list_tracks(c, &Origine::Tutti, ordine, 0, 50).expect("pagina");
            });
        }
        tempo("counts", &|| {
            counts(c, &Origine::Tutti).expect("counts");
        });
        tempo("list_albums 50", &|| {
            list_albums(c, &Origine::Tutti, 0, 50).expect("album");
        });
        tempo("list_artists", &|| {
            list_artists(c, &Origine::Tutti).expect("artisti");
        });
        tempo("search", &|| {
            search(c, &Origine::Tutti, "titolo 1", 0, 50).expect("cerca");
        });
    };
    eprintln!("--- senza dispositivi ---");
    esegui(&c);
    // Un dispositivo con 20.000 brani: 2.000 dei quali doppioni di brani di qui.
    let pc = chiave(PC);
    let righe: Vec<_> = (1..=20_000_i64)
        .map(|i| {
            let mut r = riga(100_000 + i);
            if i <= 2_000 {
                r.track_key = format!("artista {}|titolo {i}|album {}", i % 900, i.div_euclid(12));
                r.content_key = Some(r.track_key.clone());
                r.codec = Some("flac".to_owned());
                r.estensione = "flac".to_owned();
            }
            r
        })
        .collect();
    let t = Instant::now();
    giro(&c, &pc, &righe, ADESSO);
    eprintln!("MISURA giro 20.000 righe: {} ms", t.elapsed().as_millis());
    eprintln!(
        "nascoste: {}",
        n(&c, "SELECT COUNT(*) FROM tracks WHERE copia_di IS NOT NULL")
    );
    eprintln!("--- con un dispositivo e righe nascoste ---");
    esegui(&c);
    for sql in [
        "SELECT t.id FROM tracks t WHERE t.copia_di IS NULL ORDER BY t.ordine_titolo",
        "SELECT t.id FROM tracks t WHERE t.id NOT IN (SELECT id FROM tracks WHERE copia_di IS NOT NULL) ORDER BY t.ordine_titolo, t.ordine_artista, t.id",
        "SELECT t.id FROM tracks t ORDER BY t.ordine_titolo",
    ] {
        let mut st = c
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("piano");
        let piano: Vec<String> = st
            .query_map([], |r| r.get::<_, String>(3))
            .expect("piano")
            .map(|r| r.expect("riga"))
            .collect();
        eprintln!("PIANO {sql}\n   {piano:?}");
    }
    // Le due forme del filtro a confronto, cinque volte ciascuna (il tempo della
    // prima include il riscaldamento): la mediana.
    for (nome, condizione) in [
        ("copia_di IS NULL", "t.copia_di IS NULL"),
        (
            "id NOT IN (nascoste)",
            "t.id NOT IN (SELECT id FROM tracks WHERE copia_di IS NOT NULL)",
        ),
    ] {
        let sql = format!(
            "SELECT t.id FROM tracks t WHERE {condizione} ORDER BY t.ordine_titolo, t.ordine_artista, t.id"
        );
        let mut tempi: Vec<u128> = (0..5)
            .map(|_| {
                let t = Instant::now();
                let mut st = c.prepare(&sql).expect("query");
                let quante = st
                    .query_map([], |r| r.get::<_, i64>(0))
                    .expect("righe")
                    .count();
                assert!(quante > 1000);
                t.elapsed().as_millis()
            })
            .collect();
        tempi.sort_unstable();
        eprintln!("MISURA filtro {nome}: mediana {} ms su {tempi:?}", tempi[2]);
    }
    // I cinque numeri di `counts`, uno per uno (mediana di tre): dove sta il costo.
    for (nome, sql) in [
        (
            "tracks",
            "SELECT (SELECT COUNT(*) FROM tracks) - (SELECT COUNT(*) FROM tracks WHERE copia_di IS NOT NULL)",
        ),
        (
            "albums",
            "SELECT (SELECT COUNT(*) FROM albums) - (SELECT COUNT(*) FROM (SELECT h.album_key FROM (SELECT DISTINCT album_key FROM tracks WHERE id IN (SELECT id FROM tracks WHERE copia_di IS NOT NULL) AND album_key IS NOT NULL) h WHERE NOT EXISTS (SELECT 1 FROM tracks v WHERE v.album_key = h.album_key AND v.copia_di IS NULL)))",
        ),
        (
            "artists",
            "SELECT (SELECT COUNT(*) FROM artists) - (SELECT COUNT(*) FROM (SELECT DISTINCT TRIM(COALESCE(NULLIF(TRIM(album_artist), ''), artist)) AS nome FROM tracks WHERE id IN (SELECT id FROM tracks WHERE copia_di IS NOT NULL)) h WHERE h.nome <> '' AND NOT EXISTS (SELECT 1 FROM tracks v WHERE TRIM(COALESCE(NULLIF(TRIM(album_artist), ''), artist)) = h.nome AND v.copia_di IS NULL))",
        ),
        (
            "liked",
            "SELECT (SELECT COUNT(*) FROM tracks WHERE liked = 1) - (SELECT COUNT(*) FROM tracks WHERE liked = 1 AND copia_di IS NOT NULL)",
        ),
        (
            "durata",
            "SELECT (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks) - (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks WHERE copia_di IS NOT NULL)",
        ),
    ] {
        let mut tempi: Vec<u128> = (0..3)
            .map(|_| {
                let t = Instant::now();
                let _: i64 = c.query_row(sql, [], |r| r.get(0)).expect("numero");
                t.elapsed().as_millis()
            })
            .collect();
        tempi.sort_unstable();
        let mut st = c
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("piano");
        let piano: Vec<String> = st
            .query_map([], |r| r.get::<_, String>(3))
            .expect("piano")
            .map(|r| r.expect("riga"))
            .collect();
        eprintln!("MISURA counts/{nome}: {} ms  piano {piano:?}", tempi[1]);
    }
    // Il riferimento: lo stesso database, con gli aggregati costruiti, senza
    // nessuna riga nascosta. Quel che costa «Tutti» con righe nascoste è la
    // differenza con questo, non con il database senza dispositivi (dove le
    // tabelle degli album sono ancora vuote).
    c.execute("UPDATE tracks SET copia_di = NULL", [])
        .expect("senza righe nascoste");
    eprintln!(
        "--- riferimento: con un dispositivo, aggregati costruiti, nessuna riga nascosta ---"
    );
    esegui(&c);
}

#[test]
#[ignore = "misura"]
fn misura_dimenticare_un_dispositivo_con_tante_voci_di_playlist() {
    let c = libreria();
    tanti_locali(&c, 1, 60_000);
    let pc = chiave(PC);
    let righe: Vec<_> = (1..=20_000_i64).map(|i| riga(100_000 + i)).collect();
    giro(&c, &pc, &righe, ADESSO);
    // 3.000 voci di playlist su righe specchio diverse.
    let ids: Vec<i64> = {
        let mut st = c
            .prepare("SELECT track_id FROM lontano_brani ORDER BY track_id LIMIT 3000")
            .expect("ids");
        st.query_map([], |r| r.get(0))
            .expect("ids")
            .map(|r| r.expect("id"))
            .collect()
    };
    for p in 0..30 {
        playlist_con(&c, &format!("p{p}"), &ids[p * 100..(p + 1) * 100]);
    }
    let t = Instant::now();
    let a = anteprima_dimentica(&c, &pc).expect("anteprima");
    eprintln!(
        "MISURA anteprima_dimentica (3000 voci): {} ms {a:?}",
        t.elapsed().as_millis()
    );
    let t = Instant::now();
    let fatto = dimentica_dispositivo(&c, &pc).expect("dimenticato");
    eprintln!(
        "MISURA dimentica_dispositivo (3000 voci, 20.000 righe): {} ms {fatto:?}",
        t.elapsed().as_millis()
    );
    let mut st = c
        .prepare(
            "EXPLAIN QUERY PLAN SELECT t.id FROM tracks t LEFT JOIN lontano_brani l ON l.track_id = t.id
              WHERE (t.content_key = ?1 OR t.track_key = ?1 OR l.content_key = ?1)
                AND t.path IS NOT NULL
                AND (l.dispositivo IS NULL OR (l.dispositivo <> ?2 AND l.sparito_ms IS NULL))
              ORDER BY (l.dispositivo IS NULL) DESC, t.id LIMIT 1",
        )
        .expect("piano");
    let piano: Vec<String> = st
        .query_map(params!["x", "y"], |r| r.get::<_, String>(3))
        .expect("piano")
        .map(|r| r.expect("riga"))
        .collect();
    eprintln!("PIANO della ricerca di un'alternativa: {piano:?}");
}

#[test]
#[ignore = "misura"]
fn misura_togliere_un_quinto_di_un_catalogo_grande_in_una_transazione_sola() {
    let c = libreria();
    tanti_locali(&c, 1, 30_000);
    let pc = chiave(PC);
    let righe: Vec<_> = (1..=100_000_i64).map(|i| riga(100_000 + i)).collect();
    let t = Instant::now();
    giro(&c, &pc, &righe, ADESSO);
    eprintln!(
        "MISURA primo giro 100.000 righe: {} ms",
        t.elapsed().as_millis()
    );
    // Il PC non manda più il 19% delle righe: sotto «un quinto», quindi nessuna conferma.
    let restano: Vec<_> = righe.iter().filter(|r| r.id % 100 >= 19).cloned().collect();
    let t = Instant::now();
    let chiusura = giro(&c, &pc, &restano, ADESSO + 10_000);
    eprintln!(
        "MISURA secondo giro (tolte {}, in grazia {}, in attesa {}): {} ms, di cui la sola chiusura (una transazione, col lucchetto) {} ms",
        chiusura.tolte,
        chiusura.in_grazia,
        chiusura.in_attesa,
        t.elapsed().as_millis(),
        crate::specchio::comuni::ULTIMA_CHIUSURA_MS.with(std::cell::Cell::get)
    );
    // Dopo la grazia le righe si tolgono a blocchi, ognuno in una presa sua del
    // lucchetto: quel che conta è il blocco più lungo, non il totale.
    let dopo = ADESSO + 10_000 + crate::specchio::archivio::GRAZIA_MS + 1_000;
    let (mut blocchi, mut piu_lungo_ms, mut totale_ms, mut tolte) = (0, 0, 0, 0);
    loop {
        let t = Instant::now();
        let blocco = crate::specchio::archivio::togli_un_blocco(&c, &pc, dopo).expect("blocco");
        let ms = t.elapsed().as_millis();
        blocchi += 1;
        piu_lungo_ms = piu_lungo_ms.max(ms);
        totale_ms += ms;
        tolte += blocco.tolte;
        if blocco.restano == 0 || blocco.tolte == 0 {
            break;
        }
    }
    eprintln!(
        "MISURA rimozioni a blocchi: {tolte} righe in {blocchi} blocchi, il piu' lungo {piu_lungo_ms} ms (totale {totale_ms} ms) — col lucchetto preso solo il blocco"
    );
}

// ── 8. Il caso comune: la copia di qui c'è, la riga specchio se ne va ────────────

/// Il telefono ha il file di qui (id 1) e la riga specchio dello stesso brano, con
/// la **stessa** `track_key` (il caso di tutti i giorni: lo stesso brano su due
/// dispositivi, senza correzioni di tag). La playlist «sera» nomina la riga
/// specchio — il rappresentante del gruppo, perché la copia del PC è migliore.
/// Restituisce la connessione e il numero di righe del PC.
fn telefono_con_la_stessa_canzone_in_due_posti(righe_del_pc: i64) -> Connection {
    let k = riga(1).track_key;
    let c = libreria();
    locale(&c, 1, &k, Some(&k));
    let tutte: Vec<_> = (1..=righe_del_pc).map(riga).collect();
    giro(&c, &chiave(PC), &tutte, ADESSO);
    let specchio: i64 = c
        .query_row(
            "SELECT track_id FROM lontano_brani WHERE id_remoto = 1",
            [],
            |r| r.get(0),
        )
        .expect("riga specchio");
    // Il brano è un gruppo: la copia del PC (mp3 320, 44100) batte quella di qui,
    // che non ha nessun dato di qualità.
    assert_eq!(
        n(
            &c,
            "SELECT COUNT(*) FROM tracks WHERE id = 1 AND copia_di IS NOT NULL"
        ),
        1,
        "il punto di partenza: la copia di qui sta dietro quella del PC"
    );
    playlist_con(&c, "sera", &[specchio]);
    c
}

#[test]
fn rev2_08a_se_il_pc_toglie_il_brano_la_voce_passa_alla_copia_di_qui() {
    // DIFETTO (Alta). `remove_tracks` (la scansione) sposta le voci di playlist di
    // una copia di qui sulla riga specchio che sta ancora lì; il verso opposto —
    // la riga specchio se ne va, la copia di qui resta — non esiste: `togli` fa la
    // `DELETE` e la cascata svuota la voce. `dimentica_dispositivo` almeno ci prova
    // (`voci_con_alternativa`); la strada normale, il PC che non manda più il brano,
    // no. La playlist perde il brano pur avendo, qui, il file.
    let c = telefono_con_la_stessa_canzone_in_due_posti(5);
    let k = riga(1).track_key;
    let senza: Vec<_> = (2..=5).map(riga).collect();
    // La riga entra in grazia e la voce passa subito alla copia di qui; passata
    // la grazia la riga se ne va, e non c'è più una voce da perdere.
    giro(&c, &chiave(PC), &senza, ADESSO + 10_000);
    let chiusura = giro(&c, &chiave(PC), &senza, ADESSO + 10_000 + GRAZIA_MS + 1_000);
    assert_eq!(chiusura.tolte, 1, "{chiusura:?}");
    assert_eq!(
        in_playlist(&c, "sera"),
        vec![k],
        "la playlist ha perso un brano di cui qui c'è il file"
    );
}

#[test]
fn rev2_08b_se_il_pc_toglie_il_brano_non_parte_una_lapide_per_la_playlist_degli_altri() {
    // La conseguenza sulla sincronia (con la sincronia accesa): la chiave del brano
    // c'è ancora in `tracks` (la copia di qui), quindi non è «assente»; la voce viva
    // non sta più in `playlist_tracks`; per `allinea_sequenze` l'ha tolta l'utente.
    // Lapide: il brano esce dalla playlist su tutti i dispositivi che lo hanno.
    let mut c = telefono_con_la_stessa_canzone_in_due_posti(5);
    sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
    let senza: Vec<_> = (2..=5).map(riga).collect();
    giro(&c, &chiave(PC), &senza, ADESSO + 10_000);
    giro(&c, &chiave(PC), &senza, ADESSO + 10_000 + GRAZIA_MS + 1_000);
    sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
    let tolte = documento(&c, "telefono")
        .playlist
        .get("sera")
        .expect("la playlist viaggia")
        .sequenza
        .elementi()
        .filter(|(_, e)| e.tolto.is_some())
        .count();
    assert_eq!(
        tolte, 0,
        "una lapide per un brano che l'utente non ha toccato"
    );
}

#[test]
fn rev2_08c_confermare_la_rimozione_di_massa_porta_le_voci_sulla_copia_di_qui() {
    // Lo stesso per l'altra strada: oltre la soglia le righe aspettano, la persona
    // conferma (`conferma_rimozioni`) e `togli` cancella senza spostare niente.
    let c = telefono_con_la_stessa_canzone_in_due_posti(60);
    let k = riga(1).track_key;
    // Il PC non manda più niente: 60 righe sparite, più di cinquanta e più di un
    // quinto del totale, quindi in attesa.
    let chiusura = giro(&c, &chiave(PC), &[riga(1000)], ADESSO + 10_000);
    assert_eq!(chiusura.tolte, 0, "{chiusura:?}");
    assert!(chiusura.in_attesa >= 60, "{chiusura:?}");
    crate::specchio::archivio::conferma_rimozioni(&c, &chiave(PC)).expect("conferma");
    assert_eq!(
        in_playlist(&c, "sera"),
        vec![k],
        "confermare ha tolto dalla playlist un brano di cui qui c'è il file"
    );
}
#[test]
fn rev2_09_la_voce_di_una_copia_che_se_ne_va_non_passa_a_una_riga_specchio_gia_sparita() {
    // Nota (Bassa). `remove_tracks` sposta le voci di una copia di qui sulla prima
    // riga specchio con la stessa `track_key` (`ORDER BY m.id LIMIT 1`) senza
    // guardare `lontano_brani.sparito_ms`: una riga già segnata sparita, in attesa di
    // conferma, riceve la voce, e alla conferma la cascata la porta via.
    let mut c = telefono_con_la_stessa_canzone_in_due_posti(60);
    // Il PC non manda più niente: tutte in attesa (più di cinquanta e di un quinto).
    giro(&c, &chiave(PC), &[riga(1000)], ADESSO + 10_000);
    assert!(
        n(
            &c,
            "SELECT COUNT(*) FROM lontano_brani WHERE sparito_ms IS NOT NULL"
        ) >= 60,
        "tutte in attesa"
    );
    // La voce sta sulla copia di qui, che poi se ne va (un file tolto, una scansione).
    c.execute("UPDATE playlist_tracks SET track_id = 1", [])
        .expect("voce su L");
    crate::library::applica_rimozioni(
        &mut c,
        &[crate::library::RimozioneInAttesa {
            id: 1,
            path: "/musica/1.mp3".to_owned(),
        }],
    )
    .expect("rimozione");
    let su_una_riga_sparita = n(
        &c,
        "SELECT COUNT(*) FROM playlist_tracks pt JOIN lontano_brani l ON l.track_id = pt.track_id
          WHERE l.sparito_ms IS NOT NULL",
    );
    assert_eq!(
        su_una_riga_sparita, 0,
        "la voce è finita su una riga che il proprietario ha già tolto"
    );
}

#[test]
#[ignore = "misura"]
fn misura_quanto_pesa_in_memoria_un_giro_di_un_pari_ostile_al_tetto() {
    // `Giro::nuove` tiene in memoria **ogni** riga inserita nel giro (per
    // riabbinare un id cambiato) se il dispositivo aveva già almeno una riga, e non
    // ha un tetto sotto `MAX_RIGHE_PER_DISPOSITIVO` (200.000); `riabbina` ne clona
    // anche le chiavi in un secondo vettore. Un pari che prima manda una riga e poi
    // duecentomila righe al tetto dei testi fa tenere tutto in RAM.
    use crate::specchio::convalida::{MAX_CHIAVE, MAX_CODEC, MAX_GENERE, MAX_ID_MB, MAX_NOME};
    let r = crate::library::condivisa::BranoCatalogo {
        title: "t".repeat(MAX_NOME),
        artist: "a".repeat(MAX_NOME),
        album: "b".repeat(MAX_NOME),
        album_artist: Some("c".repeat(MAX_NOME)),
        genre: Some("g".repeat(MAX_GENERE)),
        codec: Some("x".repeat(MAX_CODEC)),
        track_key: "k".repeat(MAX_CHIAVE),
        content_key: Some("k".repeat(MAX_CHIAVE)),
        mb_recording_id: Some("m".repeat(MAX_ID_MB)),
        mb_release_group_id: Some("m".repeat(MAX_ID_MB)),
        mb_release_id: Some("m".repeat(MAX_ID_MB)),
        cover_art_hash: Some("a".repeat(128)),
        ..riga(1)
    };
    let heap = r.title.len()
        + r.artist.len()
        + r.album.len()
        + r.album_artist.as_deref().map_or(0, str::len)
        + r.genre.as_deref().map_or(0, str::len)
        + r.codec.as_deref().map_or(0, str::len)
        + r.track_key.len()
        + r.content_key.as_deref().map_or(0, str::len)
        + r.mb_recording_id.as_deref().map_or(0, str::len)
        + r.mb_release_group_id.as_deref().map_or(0, str::len)
        + r.mb_release_id.as_deref().map_or(0, str::len)
        + r.cover_art_hash.as_deref().map_or(0, str::len)
        + r.album_key.as_deref().map_or(0, str::len)
        + r.estensione.len()
        + r.versione.len();
    let per_riga = heap + std::mem::size_of_val(&r);
    let chiavi_clonate = MAX_CHIAVE; // `riabbina` clona `content_key`/`track_key` in un `Vec<String>`
    // Il tetto di `Giro::nuove` (`TETTO_RIABBINAMENTO`): le righe oltre entrano lo
    // stesso in libreria, ma non si tengono in memoria per riabbinarle.
    let totale = (per_riga + chiavi_clonate) * crate::specchio::archivio::TETTO_RIABBINAMENTO;
    eprintln!(
        "MISURA riga al tetto: {per_riga} byte; {} righe in `Giro::nuove` (il tetto) + le chiavi clonate: {} MiB",
        crate::specchio::archivio::TETTO_RIABBINAMENTO,
        totale.div_euclid(1 << 20)
    );
}

// ── 10. La «copia migliore» fra due copie con perdita ────────────────────────────

#[test]
fn rev2_10_fra_due_copie_con_perdita_vince_il_bitrate_non_la_frequenza_di_campionamento() {
    // DIFETTO (Bassa/Media). `Qualita` ordina `(senza_perdita, campionamento,
    // bitrate)`: è la lettera della guida, ma per un codec con perdita la frequenza
    // di campionamento non dice quanto suona bene: un AAC a 128 kbps e 48 kHz (una
    // registrazione del telefono, un file preso da un servizio) batte un MP3 a
    // 320 kbps e 44,1 kHz, e «si suona sempre la migliore» suona la peggiore.
    use crate::specchio::doppioni::Qualita;
    let aac_48 = Qualita::da_colonne(Some("aac"), Some(48_000), Some(128));
    let mp3_320 = Qualita::da_colonne(Some("mp3"), Some(44_100), Some(320));
    assert!(
        mp3_320 > aac_48,
        "un MP3 a 320 kbps/44,1 kHz deve battere un AAC a 128 kbps/48 kHz: {mp3_320:?} contro {aac_48:?}"
    );
}

// ── 11. `Origine::Raggiungibili` senza alias ─────────────────────────────────────

#[test]
fn rev2_11_raggiungibili_senza_alias_ripiega_sulla_copia_di_qui_come_con_l_alias() {
    // DIFETTO (Bassa, Android Auto). Con `prefisso` vuoto — come lo chiamano
    // `list_artists`, `albums_by_artist` e `recently_added_albums` — la sottoquery
    // `EXISTS (SELECT 1 FROM tracks r WHERE r.id = copia_di AND …)` risolve
    // `copia_di` sulla **sua** tabella `r` (la colonna più interna vince), quindi
    // `r.id = r.copia_di` non è mai vero e la copia di qui nascosta dietro un
    // dispositivo che tace non torna. Con l'alias `t.` torna: la prova di F2
    // (`raggiungibili_nasconde_…`) prova solo quel caso.
    use crate::specchio::Origine;
    let c = libreria();
    let (pc, portatile) = (chiave('a'), chiave('b'));
    for (id, path, copia) in [
        (1_i64, "C:/M/a.flac".to_owned(), None),
        // La copia di qui del brano 11 sta dietro la flac del PC, che non risponde.
        (2, "C:/M/b.mp3".to_owned(), Some(10_i64)),
        (10, crate::specchio::percorso(&pc, 11), None),
        (20, crate::specchio::percorso(&portatile, 21), None),
    ] {
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size, date_added,
                                 date_modified, copia_di)
             VALUES (?1, ?2, ?2, 'T', 'A', 'B', 1, 1, 1, ?3)",
            params![id, path, copia],
        )
        .expect("riga");
    }
    let origine = Origine::Raggiungibili(vec![portatile]);
    let con_alias = n(
        &c,
        &format!(
            "SELECT COUNT(*) FROM tracks t WHERE {}",
            origine.sql("t.").unwrap()
        ),
    );
    let senza_alias = n(
        &c,
        &format!(
            "SELECT COUNT(*) FROM tracks WHERE {}",
            origine.sql("").unwrap()
        ),
    );
    assert_eq!(con_alias, 3, "la 1, la 2 (ripiego) e la 20");
    assert_eq!(
        senza_alias, con_alias,
        "senza alias la copia di qui non torna"
    );
}

// ── «Dimentica» con un catalogo al tetto: quanto tiene il lucchetto ──────────────

/// Un telefono con cinquantamila brani suoi e un PC al tetto dei duecentomila:
/// ventimila dei brani del PC sono lo stesso brano di uno di qui (la copia
/// migliore è del PC, e quella di qui sta nascosta), 3.000 voci di playlist e
/// 5.000 ascolti su righe del PC — la metà con una copia di qui, la metà no.
/// Le righe si scrivono con un'`INSERT … SELECT`: le stesse tabelle e gli stessi
/// trigger di un giro vero, senza duecentomila passate per `Giro`.
fn telefono_con_un_pc_al_tetto() -> Connection {
    const DEL_PC: i64 = 200_000;
    let c = libreria();
    tanti_locali(&c, 1, 50_000);
    let pc = chiave(PC);
    c.execute(
        "INSERT INTO dispositivi (chiave, nome, associato_ms) VALUES (?1, 'Prova', 1)",
        [&pc],
    )
    .expect("dispositivo");
    let t = Instant::now();
    c.execute(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?2)
         INSERT INTO tracks (path, track_key, title, artist, album, album_key, file_size,
                             date_added, date_modified, codec, bitrate, sample_rate,
                             duration_ms, source)
         SELECT 'lontano://' || ?1 || '/' || i,
                CASE WHEN i <= 20000
                     THEN 'artista ' || (i % 900) || '|titolo ' || i || '|album ' || (i / 12)
                     ELSE 'artista ' || (i % 900) || '|solo pc ' || i || '|album ' || (i / 12) END,
                'Titolo ' || i, 'Artista ' || (i % 900), 'Album ' || (i / 12),
                'lontano:/' || ?1 || '/album-' || (i / 12),
                24000000, i, 1, 'flac', 900, 96000, 200000, 'lontano'
           FROM n",
        params![pc, DEL_PC],
    )
    .expect("righe del PC");
    c.execute(
        "INSERT INTO lontano_brani (track_id, dispositivo, id_remoto, estensione, versione,
                                    content_key, impronta)
         SELECT id, ?1, CAST(substr(path, length(?1) + 12) AS INTEGER), 'flac', '24000000-1',
                track_key, 'x'
           FROM tracks WHERE source = 'lontano'",
        [&pc],
    )
    .expect("lontano_brani");
    eprintln!(
        "preparazione: {} righe del PC scritte in {} ms",
        DEL_PC,
        t.elapsed().as_millis()
    );
    // Gli aggregati e i doppioni, come li lascia un giro.
    let t = Instant::now();
    {
        let tx = c.unchecked_transaction().expect("transazione");
        crate::library::rebuild_aggregates(&tx).expect("aggregati");
        tx.commit().expect("commit");
    }
    crate::specchio::archivio::ricalcola_doppioni(&c).expect("doppioni");
    eprintln!(
        "preparazione: aggregati e doppioni in {} ms",
        t.elapsed().as_millis()
    );
    // 3.000 voci e 5.000 ascolti: righe con una copia di qui (id remoto 1..=1500,
    // 2.500) e senza (25.001..).
    let riga_del_pc = |remoto: i64| -> i64 {
        c.query_row(
            "SELECT track_id FROM lontano_brani WHERE id_remoto = ?1",
            [remoto],
            |r| r.get(0),
        )
        .expect("riga")
    };
    let voci: Vec<i64> = (1..=1_500)
        .chain(25_001..=26_500)
        .map(riga_del_pc)
        .collect();
    for (p, pezzo) in voci.chunks(100).enumerate() {
        playlist_con(&c, &format!("p{p}"), pezzo);
    }
    for (n, remoto) in (1..=2_500).chain(25_001..=27_500).enumerate() {
        c.execute(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, ?2, 90000)",
            params![
                riga_del_pc(remoto),
                1_700_000_000_000_i64 + i64::try_from(n).expect("n")
            ],
        )
        .expect("ascolto");
    }
    c
}

#[test]
#[ignore = "misura"]
fn misura_dimenticare_un_pc_da_duecentomila_righe() {
    let c = telefono_con_un_pc_al_tetto();
    let pc = chiave(PC);
    let t = Instant::now();
    let fatto = dimentica_dispositivo(&c, &pc).expect("dimenticato");
    eprintln!(
        "MISURA dimentica_dispositivo (200.000 righe, 3.000 voci, 5.000 ascolti, 50.000 brani di qui): {} ms {fatto:?}",
        t.elapsed().as_millis()
    );
    eprintln!(
        "righe del PC rimaste: {}",
        n(&c, "SELECT COUNT(*) FROM tracks WHERE source = 'lontano'")
    );
}
