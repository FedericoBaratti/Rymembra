//! Seconda lettura (rev3) delle correzioni di F2b. Come `prove_revisione`: ogni prova
//! `rev3_*` descrive un difetto e **fallisce** sul codice com'è; le `misura_*` sono
//! ignorate di serie e stampano numeri.

use std::time::Instant;

use rusqlite::Connection;

use crate::specchio::comuni::{
    ADESSO, ULTIMA_CHIUSURA_MS, chiave, documento, giro, id_specchio, in_playlist, libreria,
    locale, passata, playlist_con, riga,
};
use crate::specchio::doppioni::Qualita;

const PC: char = 'a';

fn n(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).expect(sql)
}

// ── 1. Il proprietario corregge un tag: la lapide arriva a un terzo ──────────────

#[test]
fn rev3_01_un_tag_corretto_sul_pc_non_toglie_il_brano_dalla_playlist_di_un_terzo() {
    // Tre dispositivi con la stessa playlist «sera» = [brano 1]. Il telefono ha il
    // file di qui (K) e la riga specchio del brano del PC (stessa K): la voce sta
    // sulla riga specchio, la copia migliore. Il PC corregge un tag: al giro dopo la
    // riga specchio ha la chiave nuova (`aggiorna`), e la voce — sempre su quella
    // riga — porta la chiave nuova. La vecchia chiave K ha ancora il file di qui,
    // quindi non è «solo specchio» né «assente»: per `allinea_sequenze` la voce viva
    // che non sta più in `playlist_tracks` l'ha tolta l'utente. Lapide, che arriva
    // al tablet, che ha K in locale e non ha la chiave nuova: il brano sparisce
    // dalla sua playlist, e nessuno ha toccato niente lì.
    let k = riga(1).track_key;
    let mut pc = libreria();
    locale(&pc, 1, 1);
    playlist_con(&pc, "sera", &[1]);
    let mut tablet = libreria();
    locale(&tablet, 1, 1);
    playlist_con(&tablet, "sera", &[1]);
    let mut tel = libreria();
    locale(&tel, 1, 1);
    giro(&tel, &chiave(PC), &[riga(1)], ADESSO);
    let specchio = id_specchio(&tel, PC, 1);
    playlist_con(&tel, "sera", &[specchio]);

    // Tutti si sono parlati.
    passata(
        &mut pc,
        "pc",
        ("tablet", documento(&tablet, "tablet")),
        1_000,
    );
    passata(&mut tel, "telefono", ("pc", documento(&pc, "pc")), 2_000);
    passata(
        &mut tablet,
        "tablet",
        ("telefono", documento(&tel, "telefono")),
        3_000,
    );
    assert_eq!(
        in_playlist(&tablet, "sera"),
        vec![k.clone()],
        "il punto di partenza"
    );

    // Il PC corregge il tag (la chiave di contenuto, quella dei tag grezzi, resta),
    // e il telefono lo legge al giro dopo.
    pc.execute(
        "UPDATE tracks SET track_key = 'corretta|uno|album' WHERE id = 1",
        [],
    )
    .expect("correzione sul PC");
    let mut corretta = riga(1);
    corretta.track_key = "corretta|uno|album".to_owned();
    giro(&tel, &chiave(PC), &[corretta], ADESSO + 10_000);

    // Il telefono allinea (pubblica il suo documento: la vecchia chiave tolta, la nuova viva)
    // e il tablet lo legge prima che il telefono abbia fuso quello del PC.
    crate::sincronia::allinea(&mut tel, "telefono", 4_000).expect("allineamento");
    passata(
        &mut tablet,
        "tablet",
        ("telefono", documento(&tel, "telefono")),
        5_000,
    );
    assert_eq!(
        in_playlist(&tablet, "sera"),
        vec![k],
        "il tablet ha perso dalla sua playlist un brano che ha ancora in locale"
    );
}

// ── 2. La 043 cambiata sul posto: nessun passo all'apertura ─────────────────────
//
// Il revisore aveva una prova rossa (`rev3_02`): un database già alla 043 di prima
// non ha `idx_lontano_brani_content_key`, `_cover` e `_sparite`, e nessuna strada
// glieli dà. È tolta, e non c'è un passo idempotente all'apertura (un
// `CREATE INDEX IF NOT EXISTS` fuori dalla migrazione), per questa ragione:
// nessun database vero ha mai eseguito la 043 di prima (il telefono e gli
// emulatori non hanno ancora una build con la libreria unica). Un passo
// all'apertura sarebbe codice che gira a ogni avvio per sempre, per
// rimediare a un database che esiste solo in qualche prova locale; chi ne
// avesse uno, da una build intermedia, lo rifà dal file di prima
// (`rymembra.db.schema-042.bak`) o aggiunge i tre indici a mano. Se una build con
// la 043 di prima arrivasse davvero a qualcuno, il rimedio è una 044 coi tre
// `CREATE INDEX IF NOT EXISTS`, non un passo all'apertura.

// ── 3. La plausibilità della qualità: falsi negativi su file veri ────────────────

/// Un file di `minuti` minuti che pesa `kbps` kbps di media.
fn peso(kbps: i64, minuti: i64) -> (Option<i64>, Option<i64>) {
    let ms = minuti * 60_000;
    (Some((kbps * ms).div_euclid(8)), Some(ms))
}

#[test]
fn rev3_03a_un_wav_a_otto_bit_mono_resta_senza_perdita() {
    // Il PCM non compresso è senza perdita per definizione, qualunque peso abbia:
    // un WAV a 8 bit, mono, 22,05 kHz pesa 176 kbps (un messaggio vocale, una
    // registrazione vecchia). La soglia dei 224 kbps reali lo declassa a «con
    // perdita» e lo fa perdere contro un MP3 a 192 kbps.
    let (dimensione, durata) = peso(176, 3);
    let wav = Qualita::da_riga(Some("wav"), Some(22_050), Some(176), dimensione, durata);
    assert!(
        wav.senza_perdita,
        "un WAV non compresso non ha perdita: {wav:?}"
    );
}

#[test]
fn rev3_03b_un_flac_mono_di_parlato_resta_senza_perdita() {
    // Un audiolibro o una registrazione mono a 22,05 kHz in FLAC pesa 80–150 kbps
    // reali: è un FLAC vero, e per il controllo non lo è.
    let (dimensione, durata) = peso(110, 30);
    let flac = Qualita::da_riga(Some("flac"), Some(22_050), Some(110), dimensione, durata);
    assert!(flac.senza_perdita, "un FLAC mono di parlato: {flac:?}");
}

#[test]
fn rev3_03c_un_alac_sotto_la_soglia_resta_senza_perdita() {
    // ALAC mono a 44,1 kHz di un disco di musica da camera: 190 kbps reali.
    let (dimensione, durata) = peso(190, 5);
    let alac = Qualita::da_riga(Some("alac"), Some(44_100), Some(190), dimensione, durata);
    assert!(alac.senza_perdita, "un ALAC dichiarato tale: {alac:?}");
}

// ── 4. `chiudi` in una transazione sola: quanto cresce ───────────────────────────

#[test]
#[ignore = "misura"]
fn misura_la_chiusura_quando_il_pc_non_manda_piu_niente() {
    // Il caso peggiore di un pari: un riassunto vuoto. Tutte le sue righe diventano
    // sparite, e `nascondi_le_sparite` le nasconde (una `UPDATE` per riga, con la
    // sua ricerca di un'altra copia per le voci di playlist) nella transazione di
    // `chiudi`, sotto un solo giro del lucchetto. I blocchi da 2.000 valgono solo per
    // le rimozioni.
    for quante in [10_000_i64, 20_000, 40_000] {
        let c = libreria();
        let righe: Vec<_> = (1..=quante).map(riga).collect();
        giro(&c, &chiave(PC), &righe, ADESSO);
        let t = Instant::now();
        giro(&c, &chiave(PC), &[], ADESSO + 10_000);
        let nascoste = n(&c, "SELECT COUNT(*) FROM tracks WHERE copia_di = id");
        eprintln!(
            "MISURA chiusura con {quante} righe sparite: chiudi {} ms (giro intero {} ms), nascoste {nascoste}",
            ULTIMA_CHIUSURA_MS.with(std::cell::Cell::get),
            t.elapsed().as_millis()
        );
        // I blocchi dopo, ognuno in una presa sua: quanti sono e il più lungo.
        let (mut blocchi, mut il_piu_lungo, mut totale) = (0, 0, 0);
        loop {
            let t = Instant::now();
            let blocco =
                crate::specchio::archivio::togli_un_blocco(&c, &chiave(PC), ADESSO + 20_000)
                    .expect("blocco");
            let ms = t.elapsed().as_millis();
            (blocchi, il_piu_lungo, totale) = (blocchi + 1, il_piu_lungo.max(ms), totale + ms);
            if !blocco.da_nascondere {
                break;
            }
        }
        eprintln!(
            "MISURA   e poi {blocchi} blocchi: il più lungo {il_piu_lungo} ms, in tutto {totale} ms, nascoste {}",
            n(&c, "SELECT COUNT(*) FROM tracks WHERE copia_di = id")
        );
    }
}

// ── 5. La grazia: dove può comparire una riga nascosta ──────────────────────────

#[test]
fn rev3_04_una_riga_in_grazia_non_compare_da_nessuna_parte() {
    use crate::library::{
        TrackOrder, album_tracks, ascoltati_di_recente, counts, id_dei_brani, list_albums,
        list_liked, list_tracks, origine_dell_album, search,
    };
    use crate::specchio::Origine;
    let c = libreria();
    let pc = chiave(PC);
    let mut piaciuta = riga(2);
    piaciuta.liked = true;
    piaciuta.liked_at = Some(1_700_000_000_000);
    piaciuta.last_played_at = Some(1_700_000_000_000);
    piaciuta.play_count = 9;
    piaciuta.rating = 5;
    giro(&c, &pc, &[riga(1), piaciuta, riga(3), riga(4)], ADESSO);
    let remota = id_specchio(&c, PC, 2);
    playlist_con(&c, "sera", &[remota]);
    c.execute(
        "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, 1700, 1000)",
        [remota],
    )
    .expect("ascolto");
    // Il PC non manda più il brano 2: entra in grazia.
    giro(&c, &pc, &[riga(1), riga(3), riga(4)], ADESSO + 10_000);
    assert_eq!(
        n(
            &c,
            &format!("SELECT copia_di = id FROM tracks WHERE id = {remota}")
        ),
        1
    );

    let ids =
        |v: Vec<crate::library::TrackSummary>| v.into_iter().map(|b| b.id).collect::<Vec<_>>();
    let tutte = [Origine::Tutti, Origine::Dispositivo(pc.clone())];
    for origine in &tutte {
        assert!(
            !ids(list_tracks(&c, origine, TrackOrder::Title, 0, 100).unwrap()).contains(&remota),
            "list_tracks {origine:?}"
        );
        assert!(
            !ids(list_liked(&c, origine, 0, 100).unwrap()).contains(&remota),
            "list_liked {origine:?}"
        );
        assert!(
            !ids(ascoltati_di_recente(&c, origine, 20).unwrap()).contains(&remota),
            "ascoltati_di_recente {origine:?}"
        );
        assert!(
            !ids(search(&c, origine, "Titolo 2", 0, 20).unwrap()).contains(&remota),
            "search {origine:?}"
        );
    }
    assert!(
        !id_dei_brani(&c, &Origine::Tutti, TrackOrder::MostPlayed)
            .unwrap()
            .contains(&remota)
    );
    assert_eq!(counts(&c, &Origine::Tutti).unwrap().tracks, 3);
    assert_eq!(
        counts(&c, &Origine::Tutti).unwrap().liked,
        0,
        "il cuore di una riga nascosta"
    );
    // L'album: la sua pagina, aperta come fa `brani_album` senza un'origine.
    let album_key: String = c
        .query_row(
            "SELECT album_key FROM tracks WHERE id = ?1",
            [remota],
            |r| r.get(0),
        )
        .expect("album");
    let origine = origine_dell_album(&c, &album_key).unwrap();
    assert!(
        !ids(album_tracks(&c, &origine, &album_key).unwrap()).contains(&remota),
        "la pagina dell'album mostra la riga nascosta"
    );
    assert!(!list_albums(&c, &Origine::Tutti, 0, 10).unwrap().is_empty());
}

#[test]
fn rev3_05_la_cronologia_non_mostra_una_riga_in_grazia() {
    // La cronologia (`list_history`) legge `play_history` con il brano: una riga che
    // «non si suona e non si conta da nessuna parte» non dovrebbe comparire nemmeno
    // lì, con un tocco che dice «<PC> non raggiungibile».
    let c = libreria();
    let pc = chiave(PC);
    giro(&c, &pc, &[riga(1), riga(2)], ADESSO);
    let remota = id_specchio(&c, PC, 2);
    c.execute(
        "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, 1700, 1000)",
        [remota],
    )
    .expect("ascolto");
    giro(&c, &pc, &[riga(1)], ADESSO + 10_000);
    let storia = crate::library::list_history(&c, 0, 50).expect("cronologia");
    assert!(
        storia.iter().all(|voce| voce.brano.id != remota),
        "la cronologia mostra una riga in grazia"
    );
}

#[test]
fn rev3_01b_un_tag_corretto_sul_pc_non_fa_partire_una_lapide_dal_telefono() {
    // Lo stesso scenario di rev3_01, ma guardando quel che il telefono pubblica
    // (e non quel che il tablet finisce per avere): una lapide che parte e poi
    // viene neutralizzata dalla fusione è una regola che regge per caso.
    let mut tel = libreria();
    locale(&tel, 1, 1);
    giro(&tel, &chiave(PC), &[riga(1)], ADESSO);
    let specchio = id_specchio(&tel, PC, 1);
    playlist_con(&tel, "sera", &[specchio]);
    crate::sincronia::allinea(&mut tel, "telefono", 1_000).expect("allineamento");
    let mut corretta = riga(1);
    corretta.track_key = "corretta|uno|album".to_owned();
    giro(&tel, &chiave(PC), &[corretta], ADESSO + 10_000);
    crate::sincronia::allinea(&mut tel, "telefono", 2_000).expect("allineamento");
    let tolte = documento(&tel, "telefono")
        .playlist
        .get("sera")
        .expect("la playlist viaggia")
        .sequenza
        .elementi()
        .filter(|(_, e)| e.tolto.is_some())
        .count();
    assert_eq!(
        tolte, 0,
        "il telefono pubblica una lapide per la vecchia chiave"
    );
}
