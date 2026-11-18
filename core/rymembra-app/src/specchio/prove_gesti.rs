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
    assert_eq!(
        (vive, tolte),
        (vec![k(1), k(3)], 1),
        "una lapide sola, la sua"
    );

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
    let (rimappate, tolte) =
        specchio::archivio::dimentica_dispositivo(&c, &chiave(PC)).expect("dimenticato");
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

// ── la grazia: righe che il proprietario non manda più ──────────────────────

/// Il telefono con un brano di qui e quattro righe del PC (2…5), e la riga del
/// brano 3 entrata in grazia: il PC non la manda più. Restituisce anche
/// l'istante del giro in cui è sparita.
fn telefono_con_una_riga_in_grazia() -> (Connection, i64, i64) {
    let c = libreria();
    locale(&c, 1, 1);
    let tutte: Vec<_> = (2..=5).map(riga).collect();
    giro(&c, &chiave(PC), &tutte, ADESSO);
    let m3 = id_specchio(&c, PC, 3);
    let senza: Vec<_> = tutte.iter().filter(|r| r.id != 3).cloned().collect();
    let quando = ADESSO + 10_000;
    let chiusura = giro(&c, &chiave(PC), &senza, quando);
    assert_eq!((chiusura.tolte, chiusura.in_grazia), (0, 1), "{chiusura:?}");
    (c, m3, quando)
}

#[test]
fn una_riga_in_grazia_non_si_suona_e_non_si_conta_da_nessuna_parte() {
    use crate::library::{TrackOrder, counts, id_dei_brani, list_albums, list_tracks, search};
    use crate::specchio::Origine;
    let (c, m3, _) = telefono_con_una_riga_in_grazia();
    let titolo_della_riga = "Titolo 3";
    let pc = chiave(PC);

    // Resta in libreria — è lì per la sua voce di playlist — ma non in nessun elenco.
    let nei_tracks: i64 = c
        .query_row("SELECT COUNT(*) FROM tracks WHERE id = ?1", [m3], |r| {
            r.get(0)
        })
        .expect("conteggio");
    assert_eq!(nei_tracks, 1, "la riga c'è ancora");

    for origine in [
        Origine::Tutti,
        Origine::Dispositivo(pc.clone()),
        Origine::Raggiungibili(vec![pc.clone()]),
    ] {
        let titoli: Vec<String> = list_tracks(&c, &origine, TrackOrder::Title, 0, 100)
            .expect("elenco")
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert!(
            !titoli.iter().any(|t| t == titolo_della_riga),
            "{origine:?}: {titoli:?}"
        );
        let ids = id_dei_brani(&c, &origine, TrackOrder::Title).expect("identificativi");
        assert!(!ids.contains(&m3), "{origine:?}");
        assert!(
            search(&c, &origine, "titolo 3", 0, 50)
                .expect("ricerca")
                .is_empty(),
            "Cerca non la trova: {origine:?}"
        );
    }
    // I conteggi: il brano di qui e tre del PC, non quattro.
    let numeri = counts(&c, &Origine::Tutti).expect("conteggi");
    assert_eq!(numeri.tracks, 4, "{numeri:?}");
    assert_eq!(
        counts(&c, &Origine::Dispositivo(pc.clone()))
            .expect("conteggi")
            .tracks,
        3
    );
    // Gli album: quello della riga in grazia è lo stesso dei suoi vicini, e resta.
    assert!(
        !list_albums(&c, &Origine::Tutti, 0, 50)
            .expect("album")
            .is_empty()
    );
    // Le playlist intelligenti guardano «Tutti» e non la trovano.
    let intelligenti =
        crate::smart::brani(&c, &rymembra_domain::regole::Insieme::default(), ADESSO)
            .expect("playlist intelligente");
    assert!(
        !intelligenti.iter().any(|t| t.title == titolo_della_riga),
        "{:?}",
        intelligenti.iter().map(|t| &t.title).collect::<Vec<_>>()
    );
}

#[test]
fn una_riga_in_grazia_ritorna_visibile_quando_il_proprietario_la_rimanda() {
    use crate::specchio::Origine;
    let (c, m3, quando) = telefono_con_una_riga_in_grazia();
    let tutte: Vec<_> = (2..=5).map(riga).collect();
    giro(&c, &chiave(PC), &tutte, quando + 1_000);
    let visibili = crate::library::counts(&c, &Origine::Tutti)
        .expect("conteggi")
        .tracks;
    assert_eq!(visibili, 5, "il brano di qui e le quattro del PC");
    let copia: Option<i64> = c
        .query_row("SELECT copia_di FROM tracks WHERE id = ?1", [m3], |r| {
            r.get(0)
        })
        .expect("copia_di");
    assert_eq!(copia, None);
}

#[test]
fn una_copia_che_arriva_durante_la_grazia_riceve_le_voci_alla_scadenza() {
    // Il brano 3 sta solo sul PC, e la sua voce di playlist sulla riga in
    // grazia (nessuna alternativa: la grazia protegge proprio lei). Il file
    // arriva qui — una scansione, «Scarica qui» — durante la grazia: alla
    // scadenza la riga se ne va, e la voce passa alla copia di qui invece di
    // sparire con la cascata (il che, con la chiave presente, sarebbe una
    // lapide).
    let mut c = libreria();
    locale(&c, 1, 1);
    let tutte: Vec<_> = (2..=5).map(riga).collect();
    giro(&c, &chiave(PC), &tutte, ADESSO);
    let m3 = id_specchio(&c, PC, 3);
    playlist_con(&c, "sera", &[m3]);
    sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
    let senza: Vec<_> = tutte.iter().filter(|r| r.id != 3).cloned().collect();
    giro(&c, &chiave(PC), &senza, ADESSO + 10_000);
    assert_eq!(
        in_playlist(&c, "sera"),
        vec![k(3)],
        "la voce aspetta, nessuna alternativa"
    );

    // Il file arriva.
    locale(&c, 900, 3);
    let chiusura = giro(
        &c,
        &chiave(PC),
        &senza,
        ADESSO + 10_000 + specchio::archivio::GRAZIA_MS + 1_000,
    );
    assert_eq!(chiusura.tolte, 1, "{chiusura:?}");
    assert_eq!(in_playlist(&c, "sera"), vec![k(3)], "la voce ha una riga");
    let su: i64 = c
        .query_row("SELECT track_id FROM playlist_tracks", [], |r| r.get(0))
        .expect("la voce");
    assert_eq!(su, 900, "sulla copia di qui");
    sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
    assert_eq!(pubblicata(&c, "sera"), (vec![k(3)], 0), "nessuna lapide");
}

#[test]
fn le_righe_scadute_si_tolgono_anche_se_il_catalogo_non_e_cambiato() {
    // Un giro che non ha niente da leggere non chiama `chiudi`: la scadenza non
    // può dipendere da lui. `togli_un_blocco` la fa da sola.
    let (c, m3, quando) = telefono_con_una_riga_in_grazia();
    let adesso = quando + specchio::archivio::GRAZIA_MS + 1_000;
    let presto =
        specchio::archivio::togli_un_blocco(&c, &chiave(PC), quando + 1_000).expect("blocco");
    assert_eq!((presto.tolte, presto.in_grazia), (0, 1), "ancora in grazia");
    let blocco = specchio::archivio::togli_un_blocco(&c, &chiave(PC), adesso).expect("blocco");
    assert_eq!((blocco.tolte, blocco.restano), (1, 0), "{blocco:?}");
    let ancora: i64 = c
        .query_row("SELECT COUNT(*) FROM tracks WHERE id = ?1", [m3], |r| {
            r.get(0)
        })
        .expect("conteggio");
    assert_eq!(ancora, 0);
}

#[test]
fn le_rimozioni_si_fanno_a_blocchi_e_una_strage_aspetta_la_conferma() {
    use crate::specchio::archivio::{BLOCCO_RIMOZIONI, togli_un_blocco_di};
    // Cinquecento righe, quarantacinque non più mandate: sotto la soglia di
    // cinquanta, quindi non è una strage. Scadute, si tolgono a blocchi, non in
    // una transazione sola.
    let c = libreria();
    let tutte: Vec<_> = (1..=500).map(riga).collect();
    giro(&c, &chiave(PC), &tutte, ADESSO);
    let restano: Vec<_> = tutte.iter().filter(|r| r.id > 45).cloned().collect();
    giro(&c, &chiave(PC), &restano, ADESSO + 10_000);
    let dopo = ADESSO + 10_000 + specchio::archivio::GRAZIA_MS + 1_000;
    let primo = togli_un_blocco_di(&c, &chiave(PC), dopo, 20).expect("blocco");
    assert_eq!((primo.tolte, primo.restano), (20, 25), "{primo:?}");
    let secondo = togli_un_blocco_di(&c, &chiave(PC), dopo, 20).expect("blocco");
    assert_eq!((secondo.tolte, secondo.restano), (20, 5), "{secondo:?}");
    let terzo = togli_un_blocco_di(&c, &chiave(PC), dopo, 20).expect("blocco");
    assert_eq!((terzo.tolte, terzo.restano), (5, 0), "{terzo:?}");
    assert!(
        BLOCCO_RIMOZIONI >= 1_000,
        "il blocco vero è di migliaia di righe"
    );

    // Una strage — più di cinquanta righe e più di un quinto — non si toglie,
    // nemmeno scaduta: aspetta la conferma, e la conferma è a blocchi.
    let d = libreria();
    let tutte: Vec<_> = (1..=200).map(riga).collect();
    giro(&d, &chiave(PC), &tutte, ADESSO);
    giro(&d, &chiave(PC), &[riga(200)], ADESSO + 10_000);
    let blocco = togli_un_blocco_di(&d, &chiave(PC), dopo, 20).expect("blocco");
    assert_eq!((blocco.tolte, blocco.in_attesa), (0, 199), "{blocco:?}");
    let conferma =
        specchio::archivio::conferma_un_blocco_di(&d, &chiave(PC), 100).expect("conferma");
    assert_eq!(
        (conferma.tolte, conferma.restano),
        (100, 99),
        "{conferma:?}"
    );
    assert_eq!(
        specchio::archivio::conferma_rimozioni(&d, &chiave(PC)).expect("conferma"),
        99
    );
}

#[test]
fn un_pari_che_manda_troppe_righe_nuove_non_riempie_la_memoria_del_giro() {
    // Il giro tiene in memoria le righe nuove per riabbinare un id cambiato, e
    // non oltre il tetto: le righe entrano lo stesso, ma non si riabbinano.
    let c = libreria();
    c.execute(
        "INSERT INTO dispositivi (chiave, nome, associato_ms) VALUES (?1, 'PC', 1)",
        [chiave(PC)],
    )
    .expect("dispositivo");
    let mut un_giro = specchio::archivio::Giro::nuovo(&chiave(PC), 1).con_tetto_di_riabbinamento(3);
    let righe: Vec<_> = (1..=10).map(riga).collect();
    let secchio = crate::library::condivisa::SecchioCatalogo {
        da_id: 0,
        a_id: 499,
        impronta: "a".repeat(32),
        righe: 10,
    };
    un_giro
        .applica_secchio(&c, &secchio, righe, ADESSO)
        .expect("secchio");
    assert_eq!(un_giro.conti.inserite, 10, "entrano tutte");
    assert_eq!(un_giro.da_riabbinare(), 3, "ma se ne tengono tre");
}

#[test]
fn un_dispositivo_dimenticato_non_riceve_nemmeno_la_chiusura_di_un_giro_in_volo() {
    let c = libreria();
    let pc = chiave(PC);
    c.execute(
        "INSERT INTO dispositivi (chiave, nome, associato_ms) VALUES (?1, 'PC', 1)",
        [&pc],
    )
    .expect("dispositivo");
    giro(&c, &pc, &[riga(1)], ADESSO);
    let in_volo = specchio::archivio::Giro::nuovo(&pc, 1);
    crate::dispositivi::dimentica(&c, &pc).expect("dimentica");
    specchio::archivio::dimentica_dispositivo(&c, &pc).expect("specchio dimenticato");
    let esito = in_volo.chiudi(&c, &[], true, 1, ADESSO + 5_000);
    assert!(
        esito.is_err(),
        "la chiusura non riscrive lo stato del catalogo"
    );
    let cataloghi: i64 = c
        .query_row("SELECT COUNT(*) FROM lontano_cataloghi", [], |r| r.get(0))
        .expect("conteggio");
    assert_eq!(cataloghi, 0);
}

#[test]
fn dimenticare_cerca_la_copia_per_track_key_e_poi_per_content_key() {
    // Il PC ha corretto un tag: la sua `track_key` è nuova e la sua chiave di
    // contenuto (i tag grezzi) è quella di prima. Qui la stessa canzone ha la
    // chiave di contenuto uguale e una `track_key` diversa — la copia di qui
    // si trova per contenuto, se per `track_key` non c'è niente.
    let c = libreria();
    let mut dal_pc = riga(7);
    dal_pc.track_key = "pc|corretta|album".to_owned();
    dal_pc.content_key = Some("grezza|chiave".to_owned());
    giro(&c, &chiave(PC), &[dal_pc], ADESSO);
    c.execute(
        "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, file_size,
                             date_added, date_modified)
         VALUES (900, '/musica/7.mp3', 'qui|vecchia|album', 'grezza|chiave', 'T', 'A', 'B', 1, 1, 1)",
        [],
    )
    .expect("copia di qui");
    let m7 = id_specchio(&c, PC, 7);
    playlist_con(&c, "sera", &[m7]);
    let anteprima = specchio::archivio::anteprima_dimentica(&c, &chiave(PC)).expect("anteprima");
    assert_eq!(
        (anteprima.voci, anteprima.rimappabili, anteprima.perse),
        (1, 1, 0)
    );
    specchio::archivio::dimentica_dispositivo(&c, &chiave(PC)).expect("dimenticato");
    let su: i64 = c
        .query_row("SELECT track_id FROM playlist_tracks", [], |r| r.get(0))
        .expect("la voce");
    assert_eq!(su, 900, "sulla copia di qui");
}

#[test]
fn i_numeri_di_tutti_con_righe_nascoste_sono_quelli_del_filtro_riga_per_riga() {
    // «Tutti» con righe nascoste conta come totali meno nascosti, e gli album
    // tutti nascosti si cercano dalle sole righe nascoste: stesse risposte del
    // filtro `copia_di IS NULL` riga per riga, che è la definizione.
    use crate::library::{counts, list_albums};
    use crate::specchio::Origine;
    let c = libreria();
    // Cinque brani di qui, tre dei quali (1, 2, 3) hanno anche una copia migliore
    // sul PC: nascosti, e con loro tutto l'album «Album 9» di cui sono l'unica
    // presenza; più una riga del PC in grazia.
    for numero in 1..=5 {
        locale(&c, numero, numero);
        c.execute(
            "UPDATE tracks SET codec = 'mp3', bitrate = 128, sample_rate = 44100,
                               album = ?2, album_key = ?3, liked = ?4, duration_ms = 1000 * ?1
              WHERE id = ?1",
            rusqlite::params![
                numero,
                if numero <= 3 { "Album 9" } else { "Altro" },
                if numero <= 3 {
                    "chiave-nove"
                } else {
                    "chiave-altro"
                },
                i64::from(numero % 2),
            ],
        )
        .expect("brano di qui");
    }
    let mut dal_pc: Vec<_> = (1..=3)
        .map(|numero| {
            let mut r = riga(numero);
            r.codec = Some("flac".to_owned());
            r.estensione = "flac".to_owned();
            r.sample_rate = Some(96_000);
            r.bitrate = Some(900);
            r.dimensione = 24_000_000;
            r.album = "Album 9".to_owned();
            r
        })
        .collect();
    dal_pc.extend((20..=22).map(riga));
    giro(&c, &chiave(PC), &dal_pc, ADESSO);
    let senza: Vec<_> = dal_pc.iter().filter(|r| r.id != 22).cloned().collect();
    giro(&c, &chiave(PC), &senza, ADESSO + 10_000);

    let nascoste: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE copia_di IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .expect("nascoste");
    assert!(
        nascoste >= 4,
        "tre copie dietro la flac e una in grazia: {nascoste}"
    );

    let diretto = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).expect(sql) };
    let numeri = counts(&c, &Origine::Tutti).expect("conteggi");
    assert_eq!(
        numeri.tracks,
        diretto("SELECT COUNT(*) FROM tracks WHERE copia_di IS NULL")
    );
    assert_eq!(
        numeri.liked,
        diretto("SELECT COUNT(*) FROM tracks WHERE copia_di IS NULL AND liked = 1")
    );
    assert_eq!(
        numeri.duration_ms,
        diretto("SELECT COALESCE(SUM(duration_ms), 0) FROM tracks WHERE copia_di IS NULL")
    );
    assert_eq!(
        numeri.albums,
        diretto(
            "SELECT COUNT(*) FROM albums a WHERE EXISTS
               (SELECT 1 FROM tracks t WHERE t.album_key = a.album_key AND t.copia_di IS NULL)"
        )
    );
    assert_eq!(
        numeri.artists,
        diretto(
            "SELECT COUNT(DISTINCT TRIM(COALESCE(NULLIF(TRIM(album_artist), ''), artist)))
               FROM tracks WHERE copia_di IS NULL"
        )
    );
    let chiavi_visibili: Vec<String> = list_albums(&c, &Origine::Tutti, 0, 100)
        .expect("album")
        .into_iter()
        .map(|a| a.album_key)
        .collect();
    let mut direttamente: Vec<String> = {
        let mut st = c
            .prepare(
                "SELECT a.album_key FROM albums a WHERE EXISTS
                   (SELECT 1 FROM tracks t WHERE t.album_key = a.album_key AND t.copia_di IS NULL)",
            )
            .expect("album");
        st.query_map([], |r| r.get(0))
            .expect("album")
            .map(|r| r.expect("riga"))
            .collect()
    };
    let mut trovati = chiavi_visibili;
    trovati.sort();
    direttamente.sort();
    assert_eq!(trovati, direttamente);
}
