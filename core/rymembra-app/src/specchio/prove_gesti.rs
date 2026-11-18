//! I gesti dell'utente che tolgono voci da una playlist, e gli eventi della
//! vita di una riga che non lo sono.
//!
//! # L'invariante
//!
//! **Nessun evento della vita di una riga può produrre una lapide di playlist:
//! solo un gesto dell'utente.** Sono eventi una riga specchio che sparisce o
//! cambia id, un tag corretto che cambia la `track_key`, «Dimentica
//! dispositivo», la conferma delle rimozioni, un ripristino da backup, una
//! scansione, «Togli dalla libreria». Sono gesti «Togli dalla playlist», la
//! sostituzione del contenuto da un'importazione e la cancellazione della
//! playlist intera.
//!
//! Per le voci di un brano che qui c'è **solo come riga specchio** la lapide non
//! si deduce più dalla differenza fra `playlist_tracks` e la sequenza (vedi
//! `sincronia::brani_assenti`): un gesto che si appoggiasse a lei smetterebbe di
//! propagarsi in silenzio. Qui c'è la prova per ciascuno, con i dispositivi
//! veri alla fine: il PC, che il brano ce l'ha davvero.
//!
//! La scena è sempre la stessa. Il telefono ha il brano 1 di qui e i brani 2 e
//! 3 solo come righe specchio del PC; la playlist «sera» li ha tutti e tre. Il
//! PC li ha tutti e tre in locale, con la stessa playlist.

use std::collections::BTreeMap;

use rusqlite::Connection;
use rymembra_sync::Contenuto;

use crate::sincronia;
use crate::specchio::comuni::{
    ADESSO, chiave, documento, giro, id_specchio, in_playlist, libreria, locale, passata,
    playlist_con, pubblicata, riga, scambio,
};
use crate::{playlists, specchio};

/// Il dispositivo dell'altro lato: il PC.
const PC: char = 'a';

/// La chiave del brano `numero`.
fn k(numero: i64) -> String {
    riga(numero).track_key
}

/// Il telefono della scena, già allineato: la connessione, l'id della playlist
/// «sera» e gli id delle righe specchio dei brani 2 e 3.
fn telefono() -> (Connection, i64, i64, i64) {
    let mut c = libreria();
    locale(&c, 1, 1);
    giro(&c, &chiave(PC), &[riga(2), riga(3)], ADESSO);
    let (m2, m3) = (id_specchio(&c, PC, 2), id_specchio(&c, PC, 3));
    let sera = playlist_con(&c, "sera", &[1, m2, m3]);
    sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
    (c, sera, m2, m3)
}

/// Il PC della scena, già allineato.
fn pc() -> Connection {
    let mut c = libreria();
    for numero in 1..=3 {
        locale(&c, numero, numero);
    }
    playlist_con(&c, "sera", &[1, 2, 3]);
    sincronia::allinea(&mut c, "pc", 1_000).expect("allineamento");
    c
}

fn tolte_di(contenuto: &Contenuto, nome: &str) -> usize {
    contenuto
        .playlist
        .get(nome)
        .expect("la playlist viaggia")
        .sequenza
        .elementi()
        .filter(|(_, e)| e.tolto.is_some())
        .count()
}

// ── i gesti ─────────────────────────────────────────────────────────────────

#[test]
fn togliere_dalla_playlist_un_brano_solo_remoto_si_propaga_al_pc() {
    // Il gesto: sul telefono il brano 2 c'è solo come riga specchio, e la
    // differenza fra le tabelle non dice più niente. La lapide la scrive il gesto.
    let (mut tel, sera, m2, _m3) = telefono();
    let mut pc = pc();
    playlists::remove_at(&mut tel, sera, 1, m2).expect("tolto");
    assert_eq!(in_playlist(&tel, "sera"), vec![k(1), k(3)]);
    let (vive, tolte) = pubblicata(&tel, "sera");
    assert_eq!((vive, tolte), (vec![k(1), k(3)], 1), "una lapide sola, la sua");

    // E il giro dopo non lo riaggancia, né lo ricorda in nessun altro modo.
    sincronia::allinea(&mut tel, "telefono", 2_000).expect("allineamento");
    sincronia::allinea(&mut tel, "telefono", 3_000).expect("allineamento");
    assert_eq!(in_playlist(&tel, "sera"), vec![k(1), k(3)]);
    assert_eq!(pubblicata(&tel, "sera").1, 1);

    // Il PC, che il brano ce l'ha, lo toglie dalla sua playlist.
    scambio(&mut pc, &mut tel, 4_000);
    assert_eq!(in_playlist(&pc, "sera"), vec![k(1), k(3)]);
}

#[test]
fn togliere_un_brano_di_qui_e_remoto_insieme_si_propaga_come_sempre() {
    // La stessa canzone c'è in due posti sul telefono (qui e specchio): la voce
    // sta sulla riga specchio, ma la chiave ha anche una riga di qui, e la
    // differenza tra le tabelle basta, come prima di F2.
    let mut c = libreria();
    locale(&c, 1, 1);
    giro(&c, &chiave(PC), &[riga(1)], ADESSO);
    let m1 = id_specchio(&c, PC, 1);
    let sera = playlist_con(&c, "sera", &[m1]);
    sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
    playlists::remove_at(&mut c, sera, 0, m1).expect("tolto");
    sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
    assert_eq!(pubblicata(&c, "sera"), (Vec::<String>::new(), 1));
}

#[test]
fn riordinare_una_playlist_con_voci_remote_non_perde_nessun_brano() {
    // Spostare un brano è, per la sincronia, una lapide per il vecchio posto e
    // un elemento nuovo per il nuovo: lo ha sempre fatto, per le voci di qui e
    // per le altre. Quel che conta è che nessun brano esca dalla playlist, né
    // qui né sul PC.
    let (mut tel, sera, m2, _m3) = telefono();
    let mut pc = pc();
    playlists::reorder(&mut tel, sera, 1, 0, m2).expect("riordinata");
    assert_eq!(in_playlist(&tel, "sera"), vec![k(2), k(1), k(3)]);
    sincronia::allinea(&mut tel, "telefono", 2_000).expect("allineamento");
    assert_eq!(pubblicata(&tel, "sera").0, vec![k(2), k(1), k(3)]);
    scambio(&mut pc, &mut tel, 4_000);
    assert_eq!(in_playlist(&pc, "sera"), vec![k(2), k(1), k(3)]);
}

#[test]
fn cancellare_una_playlist_con_voci_remote_la_seppellisce_e_annulla_la_riporta() {
    let (mut tel, sera, _m2, _m3) = telefono();
    let mut pc = pc();
    let cancellata = playlists::cancella(&mut tel, sera).expect("cancellata");
    // Cancellata: la lapide della playlist, e tutte le voci sepolte con lei —
    // anche quelle dei brani che qui sono solo righe specchio. Non c'è più una
    // playlist da pubblicare: la sua sequenza sta nelle lapidi del database.
    let sepolte: i64 = tel
        .query_row(
            "SELECT COUNT(*) FROM sync_sequenza
              WHERE playlist_key = 'sera' AND tolto_ms IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .expect("conteggio");
    assert_eq!(sepolte, 3, "tre voci, tre lapidi");

    // «Annulla»: torna com'era, senza una lapide.
    playlists::ripristina(&mut tel, &cancellata).expect("ripristinata");
    assert_eq!(in_playlist(&tel, "sera"), vec![k(1), k(2), k(3)]);
    assert_eq!(pubblicata(&tel, "sera"), (vec![k(1), k(2), k(3)], 0));
    scambio(&mut pc, &mut tel, 4_000);
    assert_eq!(in_playlist(&pc, "sera"), vec![k(1), k(2), k(3)]);
}

#[test]
fn rinominare_una_playlist_con_voci_remote_non_lascia_lapidi_di_voci() {
    let (mut tel, sera, _m2, _m3) = telefono();
    playlists::rename(&tel, sera, "notte").expect("rinominata");
    sincronia::allinea(&mut tel, "telefono", 2_000).expect("allineamento");
    assert_eq!(in_playlist(&tel, "notte"), vec![k(1), k(2), k(3)]);
    assert_eq!(
        pubblicata(&tel, "notte"),
        (vec![k(1), k(2), k(3)], 0),
        "la sequenza segue il nome nuovo con le sue voci vive"
    );
}

#[test]
fn ripristinare_un_backup_non_fa_partire_lapidi_per_le_voci_remote() {
    // Il backup è di prima: la playlist aveva solo il brano di qui. Il
    // ripristino la riscrive — e la voce del brano remoto, che nel backup non
    // c'è, se ne va dalla playlist. Un ripristino è un evento: nessuna lapide.
    use rymembra_domain::paths::PathRules;
    use rymembra_domain::restore::{RestoreInput, plan_restore};

    let (mut tel, _sera, _m2, _m3) = telefono();
    let prima = libreria();
    locale(&prima, 1, 1);
    playlist_con(&prima, "sera", &[1]);
    let stato_prima = crate::backup::stato_locale(&prima).expect("stato di prima");
    let salvato = crate::backup::snapshot(&stato_prima, BTreeMap::new(), BTreeMap::new());
    let dal_backup = crate::backup::dal_salvataggio(&salvato, &stato_prima.tombstones);

    let qui = crate::backup::stato_locale(&tel).expect("stato locale");
    let esistono = vec![true; dal_backup.roots.len()];
    let piano = plan_restore(&RestoreInput {
        local_tracks: &qui.tracks,
        backup_tracks: &dal_backup.tracks,
        local_playlists: &qui.playlists,
        backup_playlists: &dal_backup.playlists,
        local_roots: &qui.roots,
        backup_roots: &dal_backup.roots,
        backup_roots_exist: &esistono,
        local_skins: &[],
        backup_skins: &dal_backup.skins,
        local_drafts: &[],
        backup_drafts: &dal_backup.drafts,
        path_rules: PathRules {
            case_insensitive: true,
        },
    });
    crate::backup::applica(&mut tel, &piano).expect("ripristinato");

    sincronia::allinea(&mut tel, "telefono", 2_000).expect("allineamento");
    let (_vive, tolte) = pubblicata(&tel, "sera");
    assert_eq!(
        tolte, 0,
        "una lapide per un brano che il ripristino ha solo lasciato fuori"
    );
}

// ── gli eventi ──────────────────────────────────────────────────────────────

#[test]
fn una_scansione_che_toglie_la_copia_di_qui_non_fa_partire_lapidi() {
    // Il brano 1 c'è qui e sul PC; la voce sta sulla riga di qui. Il file se ne
    // va (una scansione): la voce passa alla riga specchio, e nessuna lapide.
    let mut c = libreria();
    locale(&c, 1, 1);
    giro(&c, &chiave(PC), &[riga(1)], ADESSO);
    playlist_con(&c, "sera", &[1]);
    sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
    crate::library::applica_rimozioni(
        &mut c,
        &[crate::library::RimozioneInAttesa {
            id: 1,
            path: "/musica/1.mp3".to_owned(),
        }],
    )
    .expect("rimozione");
    sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
    assert_eq!(pubblicata(&c, "sera"), (vec![k(1)], 0));
    assert_eq!(in_playlist(&c, "sera"), vec![k(1)]);
}

#[test]
fn il_brano_remoto_tolto_e_rimesso_resta_dopo_due_passate() {
    // Tolto con la lapide, poi rimesso a mano: è una voce nuova, e nessun
    // allineamento la prende per «il ritorno di una voce tolta».
    let (mut tel, sera, m2, _m3) = telefono();
    playlists::remove_at(&mut tel, sera, 1, m2).expect("tolto");
    playlists::add_tracks(&mut tel, sera, &[m2]).expect("rimesso");
    sincronia::allinea(&mut tel, "telefono", 2_000).expect("allineamento");
    sincronia::allinea(&mut tel, "telefono", 3_000).expect("allineamento");
    assert_eq!(in_playlist(&tel, "sera"), vec![k(1), k(3), k(2)]);
    let (vive, _tolte) = pubblicata(&tel, "sera");
    assert_eq!(vive, vec![k(1), k(3), k(2)]);
}

#[test]
fn un_tag_corretto_qui_con_la_riga_specchio_ancora_viva_non_lascia_lapidi() {
    // Il caso di rev2_01 visto dal telefono: la vecchia chiave resta sulla
    // riga specchio, la voce viva non è «tolta», e nessun allineamento la
    // seppellisce — né ora né dopo che il PC ha ricevuto il documento.
    let (mut tel, _sera, _m2, _m3) = telefono();
    tel.execute(
        "UPDATE tracks SET track_key = 'corretta|uno|album' WHERE id = 1",
        [],
    )
    .expect("correzione");
    sincronia::allinea(&mut tel, "telefono", 2_000).expect("allineamento");
    sincronia::allinea(&mut tel, "telefono", 3_000).expect("allineamento");
    assert_eq!(tolte_di(&documento(&tel, "telefono"), "sera"), 0);
}

#[test]
fn dimenticare_un_dispositivo_non_lascia_lapidi_neanche_con_una_copia_di_qui() {
    let mut c = libreria();
    locale(&c, 1, 1);
    giro(&c, &chiave(PC), &[riga(1), riga(2)], ADESSO);
    let (m1, m2) = (id_specchio(&c, PC, 1), id_specchio(&c, PC, 2));
    playlist_con(&c, "sera", &[m1, m2]);
    sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
    let (rimappate, tolte) = specchio::archivio::dimentica_dispositivo(&c, &chiave(PC))
        .expect("dimenticato");
    assert_eq!((rimappate, tolte), (1, 2));
    sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
    // La voce del brano 1 è passata alla copia di qui; quella del brano 2 non
    // ha un'altra riga, ed è solo invisibile.
    assert_eq!(in_playlist(&c, "sera"), vec![k(1)]);
    assert_eq!(pubblicata(&c, "sera"), (vec![k(1), k(2)], 0));
}

// ── il formato di oggi ──────────────────────────────────────────────────────

/// Il documento che il PC con la 3.2.1 pubblica per la playlist «sera»: tre
/// voci, e la terza tolta alle 5000. Scritto a mano con le chiavi di oggi — non
/// prodotto dal codice che c'è — perché è proprio quel che un pari che non si è
/// aggiornato continua a mandare.
const PARI_DI_OGGI: &str = r#"{
    "playlist": {
        "sera": {
            "nome": "sera", "creata_at": 100, "at": 100,
            "sequenza": {
                "origine:0": {"brano": "artista|titolo 1|album"},
                "origine:1": {"dopo": "origine:0", "brano": "artista|titolo 2|album"},
                "origine:2": {"dopo": "origine:1", "brano": "artista|titolo 3|album", "tolto": 5000}
            }
        }
    },
    "lapidi": {}
}"#;

#[test]
fn un_pari_col_formato_di_oggi_si_fonde_come_prima() {
    // Sul PC e sul telefono dell'utente c'è la 3.2.1, e deve continuare a
    // fondere con quel che questo codice scrive. Il pari ha tolto il terzo
    // brano dalla playlist; il telefono lo ha solo come riga specchio, e la
    // voce se ne va dalla sua playlist come la lapide dice.
    let (mut tel, _sera, _m2, _m3) = telefono();
    let pari: Contenuto = serde_json::from_str(PARI_DI_OGGI).expect("il formato di oggi si legge");
    passata(&mut tel, "telefono", ("pc", pari), 6_000);
    assert_eq!(in_playlist(&tel, "sera"), vec![k(1), k(2)]);
    assert_eq!(pubblicata(&tel, "sera"), (vec![k(1), k(2)], 1));
}

#[test]
fn il_documento_che_parte_ha_le_chiavi_di_prima() {
    // E in uscita: la lapide di un gesto sta nelle stesse chiavi di sempre
    // (`brano`, `dopo`, `tolto`), e il documento non ha campi nuovi. Un pari che
    // si è fermato alla 3.2.1 lo legge com'è.
    let (mut tel, sera, m2, _m3) = telefono();
    playlists::remove_at(&mut tel, sera, 1, m2).expect("tolto");
    let json = serde_json::to_value(documento(&tel, "telefono")).expect("in JSON");
    let radice: Vec<&str> = json
        .as_object()
        .expect("un oggetto")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(radice, vec!["lapidi", "playlist"], "{json}");
    let playlist = json["playlist"]["sera"].as_object().expect("la playlist");
    let mut campi: Vec<&str> = playlist.keys().map(String::as_str).collect();
    campi.sort_unstable();
    assert_eq!(campi, vec!["at", "creata_at", "nome", "sequenza"]);
    let secondo = json["playlist"]["sera"]["sequenza"]["origine:1"]
        .as_object()
        .expect("l'elemento tolto");
    let mut campi: Vec<&str> = secondo.keys().map(String::as_str).collect();
    campi.sort_unstable();
    assert_eq!(campi, vec!["brano", "dopo", "tolto"], "{json}");
    // E si rilegge, come lo rilegge il pari.
    let _: Contenuto = serde_json::from_value(json).expect("si rilegge");
}
