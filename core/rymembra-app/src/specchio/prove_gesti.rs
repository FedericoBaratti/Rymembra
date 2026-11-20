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
    ADESSO, chiave, documento, giro, giro_con, id_specchio, in_playlist, libreria, locale, passata,
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
fn un_tag_corretto_dal_proprietario_porta_la_voce_sulla_copia_con_la_chiave_di_prima() {
    // Il PC corregge un tag e il telefono lo legge: la riga specchio ha la chiave
    // nuova, il file di qui ha ancora la vecchia. La voce, che stava sulla riga
    // specchio, passa al file di qui — come quando una riga se ne va — e resta
    // la voce della chiave che la sequenza ha viva: nessuna lapide da dedurre.
    let mut c = libreria();
    locale(&c, 1, 1);
    giro(&c, &chiave(PC), &[riga(1)], ADESSO);
    let specchio = id_specchio(&c, PC, 1);
    let sera = playlist_con(&c, "sera", &[specchio]);
    sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
    let mut corretta = riga(1);
    corretta.track_key = "corretta|uno|album".to_owned();
    giro(&c, &chiave(PC), &[corretta], ADESSO + 10_000);
    let voci: Vec<i64> = c
        .prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1")
        .and_then(|mut st| st.query_map([sera], |r| r.get(0))?.collect())
        .expect("voci");
    assert_eq!(voci, vec![1], "la voce sta sul file di qui");
    for quando in [2_000, 3_000] {
        sincronia::allinea(&mut c, "telefono", quando).expect("allineamento");
        assert_eq!(in_playlist(&c, "sera"), vec![k(1)]);
        assert_eq!(tolte_di(&documento(&c, "telefono"), "sera"), 0);
    }
}

#[test]
fn un_tag_corretto_dal_proprietario_senza_una_copia_di_qui_lascia_la_voce_dov_e() {
    // Niente file di qui con la chiave vecchia: non ci sono righe a cui passare
    // la voce, e non serve. La chiave vecchia non ha più righe, per la sincronia
    // è un brano assente, e nessuna lapide parte.
    let mut c = libreria();
    giro(&c, &chiave(PC), &[riga(1)], ADESSO);
    let specchio = id_specchio(&c, PC, 1);
    let sera = playlist_con(&c, "sera", &[specchio]);
    sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
    let mut corretta = riga(1);
    corretta.track_key = "corretta|uno|album".to_owned();
    giro(&c, &chiave(PC), &[corretta], ADESSO + 10_000);
    let voce: i64 = c
        .query_row(
            "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1",
            [sera],
            |r| r.get(0),
        )
        .expect("voce");
    assert_eq!(voce, specchio, "la voce resta sulla riga specchio");
    sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
    sincronia::allinea(&mut c, "telefono", 3_000).expect("allineamento");
    assert_eq!(
        in_playlist(&c, "sera"),
        vec!["corretta|uno|album".to_owned()]
    );
    assert_eq!(tolte_di(&documento(&c, "telefono"), "sera"), 0);
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
    use crate::specchio::archivio::togli_un_blocco_di;
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
                numero % 2,
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
    // Gli identificativi dell'intera vista sono quelli delle pagine, nello stesso
    // ordine, senza i nascosti.
    for ordine in [
        crate::library::TrackOrder::Title,
        crate::library::TrackOrder::Shelf,
    ] {
        let dalle_pagine: Vec<i64> =
            crate::library::list_tracks(&c, &Origine::Tutti, ordine, 0, 1_000)
                .expect("pagine")
                .into_iter()
                .map(|t| t.id)
                .collect();
        assert_eq!(
            crate::library::id_dei_brani(&c, &Origine::Tutti, ordine).expect("identificativi"),
            dalle_pagine,
            "{ordine:?}"
        );
    }
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

#[test]
fn una_riga_che_torna_con_un_id_nuovo_ritrova_voce_e_visibilita() {
    // Il caso di rev2_03, con la visibilità: la riga entra in grazia in un giro,
    // ritorna con un id nuovo in un altro; `riabbina` la riconosce per contenuto,
    // la voce di playlist è ancora lì, e la riga è di nuovo in «Tutti».
    use crate::specchio::Origine;
    let c = libreria();
    let tutte: Vec<_> = (1..=5).map(riga).collect();
    giro(&c, &chiave(PC), &tutte, ADESSO);
    let m2 = id_specchio(&c, PC, 2);
    playlist_con(&c, "sera", &[m2]);
    let senza: Vec<_> = tutte.iter().filter(|r| r.id != 2).cloned().collect();
    giro(&c, &chiave(PC), &senza, ADESSO + 10_000);
    assert_eq!(
        crate::library::counts(&c, &Origine::Tutti)
            .expect("conteggi")
            .tracks,
        4
    );

    let mut di_nuovo = riga(2);
    di_nuovo.id = 902;
    let mut tutte_di_nuovo = senza.clone();
    tutte_di_nuovo.push(di_nuovo);
    let chiusura = giro(&c, &chiave(PC), &tutte_di_nuovo, ADESSO + 20_000);
    assert_eq!(chiusura.riabbinate, 1, "{chiusura:?}");
    assert_eq!(
        crate::library::counts(&c, &Origine::Tutti)
            .expect("conteggi")
            .tracks,
        5,
        "la riga riabbinata è di nuovo in «Tutti»"
    );
    assert_eq!(in_playlist(&c, "sera"), vec![k(2)]);
    let su: i64 = c
        .query_row("SELECT track_id FROM playlist_tracks", [], |r| r.get(0))
        .expect("la voce");
    assert_eq!(
        su, m2,
        "la voce sta ancora sulla stessa riga, col suo id remoto nuovo"
    );
}

#[test]
fn le_righe_in_grazia_si_riconoscono_per_chi_mostra_una_playlist() {
    let (c, m3, _) = telefono_con_una_riga_in_grazia();
    let m2 = id_specchio(&c, PC, 2);
    let trovate = specchio::archivio::in_grazia(&c, &[m2, m3, 1]).expect("in grazia");
    assert_eq!(trovate, std::collections::HashSet::from([m3]));
    assert!(
        specchio::archivio::in_grazia(&c, &[])
            .expect("vuoto")
            .is_empty()
    );
}

#[test]
fn ogni_domanda_di_libreria_regge_ogni_origine_con_righe_nascoste_e_in_grazia() {
    // Le due forme nuove di `Origine::sql` (`Tutti` con `NOT IN`, `Raggiungibili`
    // con la riga di fuori nominata per tabella) non eseguono mai quando non ci
    // sono righe nascoste: qui ci sono — copie dietro una flac del PC, una riga
    // in grazia, un secondo dispositivo — e ogni domanda che prende un'origine
    // gira con tutte e cinque, con e senza alias, come le fa Android Auto.
    use crate::library::{
        TrackOrder, album_tracks, albums_by_artist, ascoltati_di_recente, counts, id_dei_brani,
        id_dei_preferiti, id_della_ricerca, list_albums, list_artists, list_liked, list_tracks,
        recently_added_albums, search, search_count,
    };
    use crate::specchio::Origine;
    let c = libreria();
    for numero in 1..=4 {
        locale(&c, numero, numero);
    }
    let pc = chiave(PC);
    let altro = chiave('b');
    let mut dal_pc: Vec<_> = (1..=2)
        .map(|numero| {
            let mut r = riga(numero);
            r.codec = Some("flac".to_owned());
            r.estensione = "flac".to_owned();
            r.sample_rate = Some(96_000);
            r.bitrate = Some(900);
            r.dimensione = 24_000_000;
            r
        })
        .collect();
    dal_pc.extend((20..=23).map(riga));
    giro(&c, &pc, &dal_pc, ADESSO);
    // Una copia dello stesso brano 3 su un secondo dispositivo, che risponde.
    giro(&c, &altro, &[riga(3), riga(30)], ADESSO);
    let senza: Vec<_> = dal_pc.iter().filter(|r| r.id != 23).cloned().collect();
    giro(&c, &pc, &senza, ADESSO + 10_000);
    let nascoste: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE copia_di IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .expect("nascoste");
    assert!(
        nascoste >= 3,
        "copie dietro la flac, e una riga in grazia: {nascoste}"
    );
    c.execute("UPDATE tracks SET liked = 1 WHERE id IN (1, 2, 3)", [])
        .expect("preferiti");

    let origini = [
        Origine::Tutti,
        Origine::Locale,
        Origine::Dispositivo(pc.clone()),
        Origine::Raggiungibili(vec![pc.clone()]),
        Origine::Raggiungibili(vec![altro.clone()]),
        Origine::Raggiungibili(Vec::new()),
    ];
    for origine in &origini {
        let chiaro = format!("{origine:?}");
        counts(&c, origine).unwrap_or_else(|e| panic!("counts {chiaro}: {e:?}"));
        for ordine in [
            TrackOrder::Title,
            TrackOrder::Shelf,
            TrackOrder::RecentlyAdded,
        ] {
            list_tracks(&c, origine, ordine, 0, 50)
                .unwrap_or_else(|e| panic!("list_tracks {chiaro}: {e:?}"));
            id_dei_brani(&c, origine, ordine)
                .unwrap_or_else(|e| panic!("id_dei_brani {chiaro}: {e:?}"));
        }
        ascoltati_di_recente(&c, origine, 12)
            .unwrap_or_else(|e| panic!("ascoltati_di_recente {chiaro}: {e:?}"));
        list_liked(&c, origine, 0, 50).unwrap_or_else(|e| panic!("list_liked {chiaro}: {e:?}"));
        id_dei_preferiti(&c, origine)
            .unwrap_or_else(|e| panic!("id_dei_preferiti {chiaro}: {e:?}"));
        album_tracks(&c, origine, &"a".repeat(32))
            .unwrap_or_else(|e| panic!("album_tracks {chiaro}: {e:?}"));
        list_albums(&c, origine, 0, 50).unwrap_or_else(|e| panic!("list_albums {chiaro}: {e:?}"));
        recently_added_albums(&c, origine, 12)
            .unwrap_or_else(|e| panic!("recently_added_albums {chiaro}: {e:?}"));
        albums_by_artist(&c, origine, "Artista", 0, 50)
            .unwrap_or_else(|e| panic!("albums_by_artist {chiaro}: {e:?}"));
        list_artists(&c, origine).unwrap_or_else(|e| panic!("list_artists {chiaro}: {e:?}"));
        search(&c, origine, "titolo", 0, 50).unwrap_or_else(|e| panic!("search {chiaro}: {e:?}"));
        search_count(&c, origine, "titolo")
            .unwrap_or_else(|e| panic!("search_count {chiaro}: {e:?}"));
        id_della_ricerca(&c, origine, "titolo")
            .unwrap_or_else(|e| panic!("id_della_ricerca {chiaro}: {e:?}"));
    }
    // E i risultati hanno il senso giusto: la riga in grazia non c'è da nessuna parte.
    let in_grazia: i64 = c
        .query_row("SELECT id FROM tracks WHERE copia_di = id", [], |r| {
            r.get(0)
        })
        .expect("la riga in grazia");
    for origine in &origini {
        let ids = id_dei_brani(&c, origine, TrackOrder::Title).expect("id");
        assert!(!ids.contains(&in_grazia), "{origine:?}");
    }
    // Raggiungibili([altro]): il PC tace, quindi le copie di qui dei brani 1 e 2
    // (nascoste dietro la flac) tornano, e il brano 3 c'è una volta sola — di qui o
    // dell'altro dispositivo, non tutte e due.
    let ids =
        id_dei_brani(&c, &Origine::Raggiungibili(vec![altro]), TrackOrder::Title).expect("id");
    let chiavi_dei_brani: Vec<String> = ids
        .iter()
        .map(|id| {
            c.query_row("SELECT track_key FROM tracks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .expect("chiave")
        })
        .collect();
    assert_eq!(
        chiavi_dei_brani
            .iter()
            .filter(|chiave_del_brano| **chiave_del_brano == k(3))
            .count(),
        1,
        "il brano 3 una volta sola: {chiavi_dei_brani:?}"
    );
}

#[test]
fn le_righe_in_grazia_restano_fuori_dalle_domande_di_artista_autoplay_smart_e_scarico() {
    // Dopo l'unione con F3a e F3c: le domande nuove prendono un'origine, e la riga
    // in grazia (`copia_di = id`) non deve entrare in nessuna — la pagina
    // dell'artista, le sue partecipazioni, la radio, le playlist intelligenti,
    // «Scarica qui». Le righe visibili del PC, invece, sì.
    use crate::autoplay::radio;
    use crate::library::{
        OrdineBraniArtista, Partecipazione, brani_artista, id_dei_brani_artista,
        id_delle_partecipazioni, partecipazioni, partecipazioni_conta,
    };
    use crate::smart::brani_da;
    use crate::specchio::Origine;
    use rymembra_domain::regole::Insieme;

    let (c, m3, _) = telefono_con_una_riga_in_grazia();
    let m4 = id_specchio(&c, PC, 4);
    // Il brano 3 (in grazia) e il 4 hanno un artista d'interprete e un altro del
    // disco: sono «partecipazioni esatte» di «Artista».
    c.execute(
        "UPDATE tracks SET album_artist = 'Altro' WHERE id IN (?1, ?2)",
        [m3, m4],
    )
    .expect("artista del disco");
    let pc = chiave(PC);
    let origini = [
        Origine::Tutti,
        Origine::Dispositivo(pc.clone()),
        Origine::Raggiungibili(vec![pc.clone()]),
    ];
    for origine in &origini {
        let chiaro = format!("{origine:?}");
        for ordine in [OrdineBraniArtista::Titolo, OrdineBraniArtista::Album] {
            let ids = id_dei_brani_artista(&c, origine, "Altro", ordine).expect("id artista");
            assert!(!ids.contains(&m3), "id dei brani dell'artista {chiaro}");
            let pagina: Vec<i64> = brani_artista(&c, origine, "Altro", ordine, 0, 100)
                .expect("brani")
                .into_iter()
                .map(|t| t.id)
                .collect();
            assert_eq!(ids, pagina, "{chiaro}");
        }
        let esatte: Vec<i64> =
            partecipazioni(&c, origine, "Artista", Partecipazione::Esatta, 0, 100)
                .expect("partecipazioni")
                .into_iter()
                .map(|t| t.id)
                .collect();
        assert!(!esatte.contains(&m3), "partecipazioni {chiaro}");
        assert_eq!(
            esatte,
            id_delle_partecipazioni(&c, origine, "Artista", Partecipazione::Esatta)
                .expect("id partecipazioni"),
            "{chiaro}"
        );
        partecipazioni_conta(&c, origine, "Artista").expect("conteggio");
        let intelligenti = brani_da(&c, &Insieme::default(), ADESSO, origine).expect("smart");
        assert!(
            !intelligenti.iter().any(|t| t.id == m3),
            "playlist intelligente {chiaro}"
        );
        let radio_di_qui = radio(&c, 1, 50, ADESSO, origine).expect("radio");
        assert!(!radio_di_qui.contains(&m3), "radio {chiaro}");
    }
    // Una riga visibile del PC, invece, c'è dove deve (Tutti e Dispositivo).
    let esatte_tutti: Vec<i64> = partecipazioni(
        &c,
        &Origine::Tutti,
        "Artista",
        Partecipazione::Esatta,
        0,
        100,
    )
    .expect("partecipazioni")
    .into_iter()
    .map(|t| t.id)
    .collect();
    assert!(esatte_tutti.contains(&m4), "{esatte_tutti:?}");
    // «Scarica qui»: la riga in grazia non ha più un file da dare.
    let gruppi = crate::specchio::scarico::righe_da_scaricare(&c, &[m3, m4]).expect("righe");
    let scaricabili: Vec<i64> = gruppi
        .iter()
        .flat_map(|(_, brani)| brani.iter().map(|b| b.id))
        .collect();
    assert_eq!(scaricabili, vec![4], "solo il brano 4, che il PC ha ancora");
}

#[test]
fn una_playlist_a_mano_dice_quali_voci_stanno_su_una_riga_in_grazia() {
    // La voce resta in playlist — nasconderla sposterebbe gli indici di «Togli» e
    // «Sposta» —, ma la playlist la dice, e la finestra la mostra grigia.
    let (c, m3, _) = telefono_con_una_riga_in_grazia();
    let m2 = id_specchio(&c, PC, 2);
    let sera = playlist_con(&c, "sera", &[1, m2, m3]);
    let brani = playlists::tracks(&c, sera).expect("brani");
    assert_eq!(
        brani
            .iter()
            .map(|b| (b.id, b.in_grazia))
            .collect::<Vec<_>>(),
        vec![(1, false), (m2, false), (m3, true)]
    );
    // E per tutte le altre righe il campo non viaggia: il formato dell'IPC non cambia.
    let json = serde_json::to_string(&brani[0]).expect("in JSON");
    assert!(!json.contains("inGrazia"), "{json}");
    let json = serde_json::to_string(&brani[2]).expect("in JSON");
    assert!(json.contains("\"inGrazia\":true"), "{json}");
}

#[test]
fn la_cronologia_conta_quello_che_mostra_senza_le_righe_in_grazia() {
    // Il totale dice quante voci ha la lista: gli ascolti di una riga in grazia
    // non sono né nell'una né nell'altro, e tornano con la riga.
    use crate::library::{count_history, list_history};
    let (c, m3, quando) = telefono_con_una_riga_in_grazia();
    let m2 = id_specchio(&c, PC, 2);
    for (id, ms) in [(m2, 1_000), (m3, 2_000), (1, 3_000)] {
        c.execute(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, ?2, 1000)",
            rusqlite::params![id, ms],
        )
        .expect("ascolto");
    }
    let ids = |c: &Connection| -> Vec<i64> {
        list_history(c, 0, 50)
            .expect("cronologia")
            .into_iter()
            .map(|v| v.brano.id)
            .collect()
    };
    assert_eq!(ids(&c), vec![1, m2]);
    assert_eq!(count_history(&c), Ok(2));
    // Il PC la rimanda: la riga torna visibile, e con lei i suoi ascolti.
    let tutte: Vec<_> = (2..=5).map(riga).collect();
    giro(&c, &chiave(PC), &tutte, quando + 10_000);
    assert_eq!(ids(&c), vec![1, m3, m2]);
    assert_eq!(count_history(&c), Ok(3));
}

#[test]
fn le_righe_sparite_si_nascondono_a_blocchi_e_alla_fine_non_resta_niente_di_visibile() {
    use crate::library::counts;
    use crate::specchio::Origine;
    use crate::specchio::archivio::{BLOCCO_RIMOZIONI, togli_un_blocco};
    // Un PC che non manda più niente: tutte le sue righe diventano sparite, e
    // sono più di un blocco. La chiusura ne nasconde un blocco e dice che ne
    // restano; il blocco dopo, in una presa sua, le altre.
    let c = libreria();
    let quante = i64::try_from(BLOCCO_RIMOZIONI).expect("blocco") + 500;
    let righe: Vec<_> = (1..=quante).map(riga).collect();
    giro(&c, &chiave(PC), &righe, ADESSO);
    // Una playlist con la voce di un brano che è anche di qui: sta nel secondo
    // blocco, e passa alla copia di qui quando la sua riga si nasconde.
    locale(&c, 900_000, quante);
    let ultima = id_specchio(&c, PC, quante);
    let sera = playlist_con(&c, "sera", &[ultima]);

    let chiusura = giro(&c, &chiave(PC), &[], ADESSO + 10_000);
    assert!(chiusura.da_nascondere, "{chiusura:?}");
    let visibili = |c: &Connection| counts(c, &Origine::Tutti).expect("conteggi").tracks;
    assert_eq!(
        visibili(&c),
        501,
        "il blocco nascosto, e quel che resta + il brano di qui"
    );
    let voce_di = |c: &Connection| -> i64 {
        c.query_row(
            "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1",
            [sera],
            |r| r.get(0),
        )
        .expect("voce")
    };
    assert_eq!(
        voce_di(&c),
        ultima,
        "la riga non è ancora nascosta, la voce sta com'era"
    );

    let blocco = togli_un_blocco(&c, &chiave(PC), ADESSO + 20_000).expect("blocco");
    assert_eq!(
        (blocco.nascoste, blocco.da_nascondere, blocco.tolte),
        (500, false, 0),
        "{blocco:?}"
    );
    assert_eq!(visibili(&c), 1, "resta il brano di qui");
    assert_eq!(voce_di(&c), 900_000, "la voce è passata alla copia di qui");

    // Niente da fare: nessuna scrittura, e nessun blocco.
    let ancora = togli_un_blocco(&c, &chiave(PC), ADESSO + 30_000).expect("blocco");
    assert_eq!(
        (ancora.nascoste, ancora.da_nascondere),
        (0, false),
        "{ancora:?}"
    );
}

// ── la cronologia d'ascolto e le voci seguono la copia di qui (S2-2, S2-1) ──────

/// Gli ascolti di una riga, in `play_history`.
fn ascolti_di(c: &Connection, brano: i64) -> i64 {
    c.query_row(
        "SELECT COUNT(*) FROM play_history WHERE track_id = ?1",
        [brano],
        |r| r.get(0),
    )
    .expect("ascolti")
}

/// Un ascolto fatto su `brano` a `quando`.
fn ascolto(c: &Connection, brano: i64, quando: i64) {
    c.execute(
        "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, ?2, 90000)",
        rusqlite::params![brano, quando],
    )
    .expect("ascolto");
}

/// Il telefono ha il brano 1 di qui, e dal PC le righe 1 (lo stesso brano) e 2
/// (solo sul PC). Il telefono ha suonato la copia migliore, quella del PC:
/// tre ascolti sulla riga specchio del brano 1 — uno nello stesso istante di un
/// ascolto di qui —, due su quella del brano 2. Restituisce la connessione e
/// gli id delle due righe specchio.
fn telefono_con_ascolti_di_un_pari() -> (Connection, i64, i64) {
    let c = libreria();
    locale(&c, 1, 1);
    giro(&c, &chiave(PC), &[riga(1), riga(2)], ADESSO);
    let (r1, r2) = (id_specchio(&c, PC, 1), id_specchio(&c, PC, 2));
    for quando in [1_700, 1_800, 1_900] {
        ascolto(&c, r1, quando);
    }
    for quando in [2_700, 2_800] {
        ascolto(&c, r2, quando);
    }
    // Lo stesso istante, già sulla copia di qui: è lo stesso ascolto, e il
    // passaggio non lo deve duplicare né fallire (indice unico della 020).
    ascolto(&c, 1, 1_800);
    (c, r1, r2)
}

#[test]
fn dimenticare_un_dispositivo_tiene_la_cronologia_sulla_copia_di_qui() {
    let (c, _r1, _r2) = telefono_con_ascolti_di_un_pari();
    specchio::archivio::dimentica_dispositivo(&c, &chiave(PC)).expect("dimenticato");
    assert_eq!(
        ascolti_di(&c, 1),
        3,
        "i tre ascolti del brano 1 stanno sulla copia di qui, quello dello stesso istante una volta sola"
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM play_history", [], |r| r
            .get::<_, i64>(0))
            .expect("totale"),
        3,
        "quelli del brano che c'è solo sul PC se ne vanno con lui"
    );
}

#[test]
fn la_grazia_scaduta_tiene_la_cronologia_sulla_copia_di_qui() {
    let (c, _r1, _r2) = telefono_con_ascolti_di_un_pari();
    // Il PC non manda più il brano 1: in grazia, poi scaduta.
    giro(&c, &chiave(PC), &[riga(2)], ADESSO + 10_000);
    // In grazia la riga è nascosta con i suoi ascolti (se ritorna li ritrova):
    // è alla rimozione che passano alla copia di qui.
    let chiusura = giro(
        &c,
        &chiave(PC),
        &[riga(2)],
        ADESSO + 10_000 + specchio::archivio::GRAZIA_MS + 1_000,
    );
    assert_eq!(chiusura.tolte, 1, "{chiusura:?}");
    assert_eq!(
        ascolti_di(&c, 1),
        3,
        "la riga se n'è andata, gli ascolti no"
    );
}

#[test]
fn la_conferma_di_massa_tiene_la_cronologia_sulla_copia_di_qui() {
    let c = libreria();
    locale(&c, 1, 1);
    let righe: Vec<_> = (1..=60).map(riga).collect();
    giro(&c, &chiave(PC), &righe, ADESSO);
    let r1 = id_specchio(&c, PC, 1);
    for quando in [1_700, 1_900] {
        ascolto(&c, r1, quando);
    }
    // Il PC non manda più niente: più di cinquanta righe, in attesa di conferma.
    let chiusura = giro(&c, &chiave(PC), &[riga(1000)], ADESSO + 10_000);
    assert!(chiusura.in_attesa >= 60, "{chiusura:?}");
    specchio::archivio::conferma_rimozioni(&c, &chiave(PC)).expect("conferma");
    assert_eq!(
        ascolti_di(&c, 1),
        2,
        "gli ascolti stanno sulla copia di qui"
    );
}

#[test]
fn la_voce_su_una_riga_che_sparisce_segue_la_chiave_di_contenuto() {
    // S2-1. Il PC ha corretto un tag a mano: la sua `track_key` è nuova e la sua
    // chiave di contenuto (i tag grezzi) è quella di prima, la stessa della copia
    // di qui, che ha la `track_key` vecchia. Per `track_key` non si trovano; per
    // contenuto sì, ed è la regola di «Dimentica» (rev2-02): vale anche quando è
    // il PC a non mandare più il brano.
    let c = libreria();
    let mut dal_pc = riga(7);
    dal_pc.track_key = "pc|corretta|album".to_owned();
    dal_pc.content_key = Some("grezza|chiave".to_owned());
    giro(&c, &chiave(PC), &[dal_pc, riga(8)], ADESSO);
    c.execute(
        "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, file_size,
                             date_added, date_modified)
         VALUES (900, '/musica/7.mp3', 'qui|vecchia|album', 'grezza|chiave', 'T', 'A', 'B', 1, 1, 1)",
        [],
    )
    .expect("copia di qui");
    let m7 = id_specchio(&c, PC, 7);
    playlist_con(&c, "sera", &[m7]);
    ascolto(&c, m7, 1_700);

    // Il brano sparisce: la voce e l'ascolto passano subito, in grazia.
    giro(&c, &chiave(PC), &[riga(8)], ADESSO + 10_000);
    let voce = |c: &Connection| -> Vec<i64> {
        let mut st = c
            .prepare("SELECT track_id FROM playlist_tracks ORDER BY position")
            .expect("voci");
        st.query_map([], |r| r.get(0))
            .expect("voci")
            .map(|r| r.expect("voce"))
            .collect()
    };
    assert_eq!(
        voce(&c),
        vec![900],
        "in grazia la voce sta già sulla copia di qui"
    );
    // Passata la grazia la riga se ne va, e la voce no.
    giro(
        &c,
        &chiave(PC),
        &[riga(8)],
        ADESSO + 10_000 + specchio::archivio::GRAZIA_MS + 1_000,
    );
    assert_eq!(voce(&c), vec![900], "la voce è ancora sulla copia di qui");
    assert_eq!(ascolti_di(&c, 900), 1, "e l'ascolto con lei");
}

#[test]
fn l_anteprima_di_dimentica_conta_gli_ascolti_che_si_perdono() {
    // Il brano 1 ha una copia di qui, e i suoi tre ascolti passano; il brano 2
    // c'è solo sul PC, e i suoi due se ne vanno: sono quelli che la domanda dice.
    let (c, _r1, _r2) = telefono_con_ascolti_di_un_pari();
    let anteprima = specchio::archivio::anteprima_dimentica(&c, &chiave(PC)).expect("anteprima");
    assert_eq!(anteprima.ascolti_persi, 2, "{anteprima:?}");
    // Senza ascolti, niente da dire.
    let senza = libreria();
    giro(&senza, &chiave(PC), &[riga(1)], ADESSO);
    let anteprima =
        specchio::archivio::anteprima_dimentica(&senza, &chiave(PC)).expect("anteprima");
    assert_eq!(anteprima.ascolti_persi, 0, "{anteprima:?}");
}

// ── la coda salvata non nomina righe che non ci sono (S11-1) ─────────────────────

/// Il telefono ha il brano 1 di qui e la riga specchio `m` (brano 2 del PC), e la
/// coda di ieri li ha tutti e due: la riga specchio è quella che suonava, e il
/// segno di «dov'eri rimasto» è dentro di lei.
fn coda_con_una_riga_specchio(c: &Connection, m: i64) {
    use rymembra_domain::queue::{QueueSnapshot, RepeatMode};
    crate::playback::save_queue(
        c,
        &QueueSnapshot {
            tracks: vec![1, m],
            order: vec![0, 1],
            order_pos: Some(1),
            shuffle: false,
            repeat: RepeatMode::Off,
        },
    )
    .expect("coda salvata");
    crate::playback::save_posizione(c, m, 42_000).expect("posizione");
}

/// La coda e il segno com'è adesso: i brani, e dove si riprenderebbe `m`.
fn coda_e_segno(c: &Connection, m: i64) -> (Vec<i64>, u64) {
    (
        crate::playback::load_queue(c).expect("coda").tracks,
        crate::playback::load_posizione(c, m).expect("posizione"),
    )
}

/// `tracks.id` non è `AUTOINCREMENT`: un id tolto lo prende il brano nuovo.
fn riusa_l_id(c: &Connection, m: i64) {
    c.execute(
        "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, file_size,
                             date_added, date_modified)
         VALUES (?1, '/musica/nuovo.mp3', 'nuovo|brano|album', 'nuovo|brano|album', 'Nuovo',
                 'Artista', 'Album', 1000, 1, 1)",
        [m],
    )
    .expect("un brano nuovo prende l'id");
}

#[test]
fn dimenticare_un_dispositivo_toglie_le_sue_righe_dalla_coda_salvata() {
    let c = libreria();
    locale(&c, 1, 1);
    giro(&c, &chiave(PC), &[riga(2)], ADESSO);
    let m = id_specchio(&c, PC, 2);
    coda_con_una_riga_specchio(&c, m);
    specchio::archivio::dimentica_dispositivo(&c, &chiave(PC)).expect("dimenticato");
    // Il brano nuovo prende l'id: la coda non lo deve ritrovare, e nemmeno il segno.
    riusa_l_id(&c, m);
    assert_eq!(coda_e_segno(&c, m), (vec![1], 0));
}

#[test]
fn la_grazia_scaduta_toglie_le_righe_dalla_coda_salvata() {
    let c = libreria();
    locale(&c, 1, 1);
    giro(&c, &chiave(PC), &[riga(2), riga(3)], ADESSO);
    let m = id_specchio(&c, PC, 2);
    coda_con_una_riga_specchio(&c, m);
    giro(&c, &chiave(PC), &[riga(3)], ADESSO + 10_000);
    let chiusura = giro(
        &c,
        &chiave(PC),
        &[riga(3)],
        ADESSO + 10_000 + specchio::archivio::GRAZIA_MS + 1_000,
    );
    assert_eq!(chiusura.tolte, 1, "{chiusura:?}");
    riusa_l_id(&c, m);
    assert_eq!(coda_e_segno(&c, m), (vec![1], 0));
}

#[test]
fn la_conferma_di_massa_toglie_le_righe_dalla_coda_salvata() {
    let c = libreria();
    locale(&c, 1, 1);
    let righe: Vec<_> = (2..=61).map(riga).collect();
    giro(&c, &chiave(PC), &righe, ADESSO);
    let m = id_specchio(&c, PC, 2);
    coda_con_una_riga_specchio(&c, m);
    let chiusura = giro(&c, &chiave(PC), &[riga(1000)], ADESSO + 10_000);
    assert!(chiusura.in_attesa >= 60, "{chiusura:?}");
    specchio::archivio::conferma_rimozioni(&c, &chiave(PC)).expect("conferma");
    riusa_l_id(&c, m);
    assert_eq!(coda_e_segno(&c, m), (vec![1], 0));
}

#[test]
fn togliere_righe_non_tocca_la_coda_che_non_le_nomina() {
    // Una coda fatta solo di brani di qui, e un segno di posizione che è di un
    // altro brano: restano come sono (e il segno non si riscrive).
    let c = libreria();
    locale(&c, 1, 1);
    locale(&c, 2, 2);
    giro(&c, &chiave(PC), &[riga(3)], ADESSO);
    {
        use rymembra_domain::queue::{QueueSnapshot, RepeatMode};
        crate::playback::save_queue(
            &c,
            &QueueSnapshot {
                tracks: vec![2, 1],
                order: vec![1, 0],
                order_pos: Some(0),
                shuffle: true,
                repeat: RepeatMode::All,
            },
        )
        .expect("coda");
    }
    crate::playback::save_posizione(&c, 1, 7_000).expect("posizione");
    specchio::archivio::dimentica_dispositivo(&c, &chiave(PC)).expect("dimenticato");
    let coda = crate::playback::load_queue(&c).expect("coda");
    assert_eq!(
        (coda.tracks, coda.order, coda.shuffle),
        (vec![2, 1], vec![1, 0], true)
    );
    assert_eq!(
        crate::playback::load_posizione(&c, 1).expect("posizione"),
        7_000
    );
}

// ── «Dimentica» a blocchi, e lo spazzino dell'avvio (S2-3, S2-6) ─────────────────

/// Il telefono di «Dimentica»: il brano 1 di qui e dodici righe del PC — la 1 è
/// lo stesso brano di quello di qui, la 7 c'è solo sul PC —, una playlist con
/// tutte e due, due ascolti sulla 1 e uno sulla 7, e un voto in posta. Il PC è
/// già stato dimenticato dal registro: la riga di `dispositivi` non c'è più, e
/// questo è lo stato in cui un arresto lascia le righe.
fn telefono_che_dimentica_il_pc() -> (Connection, i64, i64) {
    let c = libreria();
    locale(&c, 1, 1);
    let righe: Vec<_> = (1..=12).map(riga).collect();
    giro(&c, &chiave(PC), &righe, ADESSO);
    let (r1, r7) = (id_specchio(&c, PC, 1), id_specchio(&c, PC, 7));
    playlist_con(&c, "sera", &[r1, r7]);
    for (brano, quando) in [(r1, 1_700), (r1, 1_800), (r7, 2_700)] {
        ascolto(&c, brano, quando);
    }
    crate::dispositivi::dimentica(&c, &chiave(PC)).expect("dimenticato dal registro");
    // La posta che si accoda dopo, fra la riga di `dispositivi` e le righe.
    crate::dispositivi::metti_in_posta(&c, &chiave(PC), "valuta", "1", "{}", 1).expect("posta");
    (c, r1, r7)
}

/// Le righe che «Tutti» mostra.
fn visibili_in_tutti(c: &Connection) -> i64 {
    crate::library::counts(c, &specchio::Origine::Tutti)
        .expect("conteggi")
        .tracks
}

fn righe_del_pc(c: &Connection) -> i64 {
    c.query_row(
        "SELECT COUNT(*) FROM lontano_brani WHERE dispositivo = ?1",
        [chiave(PC)],
        |r| r.get(0),
    )
    .expect("righe")
}

#[test]
fn dimenticare_a_blocchi_libera_il_lucchetto_fra_un_blocco_e_l_altro() {
    use specchio::archivio::{
        comincia_a_dimenticare, dimentica_un_blocco, dispositivi_a_meta, stato,
    };
    let (c, r1, _r7) = telefono_che_dimentica_il_pc();
    assert_eq!(
        visibili_in_tutti(&c),
        12,
        "il punto di partenza: la 1 è doppia, il resto è del PC"
    );

    // Il primo tempo: da adesso le righe non si vedono, e il brano di qui sì.
    assert!(comincia_a_dimenticare(&c, &chiave(PC), ADESSO + 1_000).expect("primo tempo"));
    assert_eq!(visibili_in_tutti(&c), 1, "resta il brano di qui");
    assert_eq!(
        righe_del_pc(&c),
        12,
        "le righe ci sono ancora: si cancellano a blocchi"
    );
    assert_eq!(
        stato(&c, &chiave(PC)).expect("stato"),
        specchio::archivio::StatoDispositivo {
            righe: 12,
            ..Default::default()
        },
        "e lo stato dei suoi secchi se n'è andato: se torna, si rilegge da capo"
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM dispositivi_posta", [], |r| r
            .get::<_, i64>(0))
            .expect("posta"),
        0,
        "la posta non parte più"
    );

    // Un blocco piccolo, poi l'arresto: l'app si chiude, Windows spegne.
    let primo = dimentica_un_blocco(&c, &chiave(PC), ADESSO + 2_000, 5).expect("blocco");
    assert_eq!(
        (primo.tolte, primo.finito, primo.tornato),
        (5, false, false),
        "{primo:?}"
    );
    assert_eq!(primo.ids_tolti.len(), 5);
    assert_eq!(righe_del_pc(&c), 7);
    assert_eq!(
        visibili_in_tutti(&c),
        1,
        "fra un blocco e l'altro non si vede niente del PC"
    );

    // All'avvio: lo spazzino lo trova, e finisce il lavoro.
    assert_eq!(dispositivi_a_meta(&c).expect("a metà"), vec![chiave(PC)]);
    let mut tolte = primo.tolte;
    loop {
        let blocco = dimentica_un_blocco(&c, &chiave(PC), ADESSO + 3_000, 5).expect("blocco");
        tolte += blocco.tolte;
        if blocco.finito || blocco.tolte == 0 {
            assert!(blocco.finito, "{blocco:?}");
            break;
        }
    }
    assert_eq!(tolte, 12);
    assert_eq!(righe_del_pc(&c), 0);
    assert!(
        dispositivi_a_meta(&c).expect("a metà").is_empty(),
        "niente è rimasto a metà"
    );
    for tabella in ["lontano_secchi", "lontano_cataloghi", "dispositivi_posta"] {
        assert_eq!(
            c.query_row(&format!("SELECT COUNT(*) FROM {tabella}"), [], |r| r
                .get::<_, i64>(0))
                .expect("tabella"),
            0,
            "{tabella}"
        );
    }
    // La voce sulla 1 è passata alla copia di qui, quella sulla 7 no; e gli
    // ascolti della 1 con lei.
    assert_eq!(
        in_playlist(&c, "sera"),
        vec![k(1)],
        "una sola voce, quella del brano che c'è anche qui"
    );
    assert_eq!(ascolti_di(&c, 1), 2);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM play_history", [], |r| r
            .get::<_, i64>(0))
            .expect("tutti"),
        2,
        "l'ascolto del brano solo del PC se n'è andato con lui"
    );
    assert_eq!(visibili_in_tutti(&c), 1);
    let _ = r1;
}

#[test]
fn un_dispositivo_che_si_ricollega_ferma_la_cancellazione() {
    use specchio::archivio::{comincia_a_dimenticare, dimentica_un_blocco};
    let (c, _r1, _r7) = telefono_che_dimentica_il_pc();
    assert!(comincia_a_dimenticare(&c, &chiave(PC), ADESSO + 1_000).expect("primo tempo"));
    // La persona rifà il collegamento prima che i blocchi finiscano.
    c.execute(
        "INSERT INTO dispositivi (chiave, nome, associato_ms) VALUES (?1, 'Di nuovo', 2)",
        [chiave(PC)],
    )
    .expect("riassociato");
    let blocco = dimentica_un_blocco(&c, &chiave(PC), ADESSO + 2_000, 5).expect("blocco");
    assert!(blocco.tornato && blocco.finito, "{blocco:?}");
    assert_eq!(blocco.tolte, 0);
    assert_eq!(righe_del_pc(&c), 12, "le righe sono di nuovo sue");
    // E non si ricomincia nemmeno il primo tempo su un dispositivo collegato.
    assert!(!comincia_a_dimenticare(&c, &chiave(PC), ADESSO + 3_000).expect("primo tempo"));
    assert!(
        specchio::archivio::dispositivi_a_meta(&c)
            .expect("a metà")
            .is_empty()
    );
}

#[test]
fn una_riga_specchio_di_un_dispositivo_non_collegato_si_toglie_all_apertura() {
    // L'arresto è arrivato subito dopo la riga di `dispositivi`: le righe sono
    // tutte lì, visibili, senza il primo tempo. Lo spazzino lo fa e finisce.
    use specchio::archivio::{dimentica_un_blocco, dispositivi_a_meta};
    let (c, _r1, _r7) = telefono_che_dimentica_il_pc();
    assert_eq!(visibili_in_tutti(&c), 12);
    assert_eq!(dispositivi_a_meta(&c).expect("a metà"), vec![chiave(PC)]);
    let blocco = dimentica_un_blocco(&c, &chiave(PC), ADESSO + 1_000, 50).expect("blocco");
    assert!(blocco.finito, "{blocco:?}");
    assert_eq!(righe_del_pc(&c), 0);
    assert_eq!(
        visibili_in_tutti(&c),
        1,
        "resta il brano di qui, e torna visibile"
    );
    assert!(dispositivi_a_meta(&c).expect("a metà").is_empty());
}

// ── la posta in uscita segue un brano riabbinato (S2-13) ─────────────────────────

#[test]
fn una_riga_riabbinata_porta_con_se_la_sua_posta() {
    use crate::dispositivi::{POSTA_TESTO, POSTA_VALUTA, in_posta, metti_in_posta};
    // Il PC ricostruisce la libreria: il brano 2 torna con un id nuovo, e la riga
    // specchio si riabbina sul posto (`riabbina`). Un cuore dato qui e ancora in
    // posta nomina il brano con l'id di prima: il PC risponderebbe «quel brano non
    // c'è», e il cuore si butterebbe.
    let c = libreria();
    let tutte: Vec<_> = (1..=5).map(riga).collect();
    giro(&c, &chiave(PC), &tutte, ADESSO);
    let id2 = id_specchio(&c, PC, 2);
    specchio::valutazioni::dai_al_gruppo(&c, id2, Some(true), None, ADESSO + 1_000).expect("cuore");
    // E un testo corretto, che aspetta in posta con la stessa chiave.
    metti_in_posta(
        &c,
        &chiave(PC),
        POSTA_TESTO,
        "2",
        r#"{"scartoMs":120}"#,
        ADESSO + 2_000,
    )
    .expect("testo");
    assert!(
        in_posta(&c, &chiave(PC), POSTA_VALUTA, "2")
            .expect("posta")
            .is_some()
    );

    // Il PC non manda più il brano 2, e poi lo rimanda con l'id 902.
    let senza: Vec<_> = tutte.iter().filter(|r| r.id != 2).cloned().collect();
    giro(&c, &chiave(PC), &senza, ADESSO + 10_000);
    let mut di_nuovo = riga(2);
    di_nuovo.id = 902;
    let mut tutte_di_nuovo = senza.clone();
    tutte_di_nuovo.push(di_nuovo);
    let chiusura = giro(&c, &chiave(PC), &tutte_di_nuovo, ADESSO + 20_000);
    assert_eq!(chiusura.riabbinate, 1, "{chiusura:?}");
    assert_eq!(
        id_specchio(&c, PC, 902),
        id2,
        "la riga è la stessa, col suo id nuovo"
    );

    assert!(
        in_posta(&c, &chiave(PC), POSTA_VALUTA, "2")
            .expect("posta")
            .is_none(),
        "il cuore non resta all'id di prima"
    );
    let voto = in_posta(&c, &chiave(PC), POSTA_VALUTA, "902")
        .expect("posta")
        .expect("il cuore segue il brano");
    let valutazione = specchio::valutazioni::Valutazione::da_corpo(&voto.corpo).expect("corpo");
    assert_eq!(
        (valutazione.brano, valutazione.liked),
        (902, Some(true)),
        "e nel corpo c'è l'id di adesso: è quello che il PC legge"
    );
    assert_eq!(voto.quando_ms, ADESSO + 1_000, "l'orologio non cambia");
    assert!(
        in_posta(&c, &chiave(PC), POSTA_TESTO, "2")
            .expect("posta")
            .is_none()
    );
    assert!(
        in_posta(&c, &chiave(PC), POSTA_TESTO, "902")
            .expect("posta")
            .is_some(),
        "anche il testo corretto"
    );
}

// ── S2-5: doppioni e chiavi d'album si rifanno solo quando serve ─────────────

/// Quante volte, in questo filo, si sono rifatti doppioni e chiavi d'album.
fn rifatti() -> usize {
    specchio::archivio::PASSATE_DI_DOPPIONI.with(std::cell::Cell::get)
}

/// Un telefono con il brano 1 di qui, che dichiara la sua chiave d'album, e i
/// brani 1 e 2 del PC dello stesso disco: le righe del PC prendono la chiave
/// d'album di qui. Dopo il primo giro.
fn telefono_con_il_disco_del_pc() -> Connection {
    let c = libreria();
    locale(&c, 1, 1);
    c.execute(
        "UPDATE tracks SET album_key = 'storage/musica|album' WHERE id = 1",
        [],
    )
    .expect("chiave d'album di qui");
    giro(&c, &chiave(PC), &[riga(1), riga(2)], ADESSO);
    c
}

fn chiavi_d_album(c: &Connection) -> Vec<String> {
    c.prepare("SELECT album_key FROM tracks ORDER BY id")
        .expect("chiavi")
        .query_map([], |r| r.get::<_, String>(0))
        .expect("chiavi")
        .map(|r| r.expect("chiave"))
        .collect()
}

#[test]
fn un_giro_che_non_porta_niente_non_rifa_doppioni_e_album() {
    // La chiusura di un giro rifaceva doppioni e chiavi d'album su tutta la
    // libreria anche quando il catalogo era quello di prima: 9 secondi di
    // lucchetto con 250.000 righe (misura, profilo di sviluppo).
    let c = telefono_con_il_disco_del_pc();
    assert_eq!(
        chiavi_d_album(&c),
        vec!["storage/musica|album"; 3],
        "il controllo: le righe del PC hanno preso la chiave d'album di qui"
    );
    let prima = rifatti();
    let (_, conti) = giro_con(&c, &chiave(PC), &[riga(1), riga(2)], ADESSO + 1);
    assert_eq!(conti.inserite + conti.aggiornate + conti.segnate_sparite, 0);
    assert_eq!(
        rifatti(),
        prima,
        "un giro che non porta niente non rifà niente"
    );
}

#[test]
fn un_giro_con_solo_ascolti_non_rifa_doppioni_e_album_e_tiene_il_disco_unito() {
    // Il PC suona musica: ogni ascolto cambia il suo catalogo, e la riga
    // dell'ascolto si riscrive. Cambia il conteggio, non chi è un doppione né a
    // quale disco appartiene: e la riga riscritta non deve perdere la chiave
    // d'album di qui, che solo il riallineamento — che qui non gira — le ridarebbe.
    let c = telefono_con_il_disco_del_pc();
    let prima = rifatti();
    let mut con_ascolti = riga(2);
    con_ascolti.play_count = 9;
    con_ascolti.liked = true;
    con_ascolti.rating = 4;
    con_ascolti.liked_at = Some(1_700_000_000_000);
    con_ascolti.rating_at = Some(1_700_000_000_000);
    con_ascolti.last_played_at = Some(1_700_000_000_000);
    con_ascolti.stats_updated_at = 1_700_000_000_000;
    let (_, conti) = giro_con(&c, &chiave(PC), &[riga(1), con_ascolti], ADESSO + 1);
    assert_eq!(conti.aggiornate, 1, "la riga si riscrive");
    assert_eq!(rifatti(), prima, "ma i doppioni e il disco non si rifanno");
    let ascolti: i64 = c
        .query_row(
            "SELECT play_count FROM tracks WHERE id = ?1",
            [id_specchio(&c, PC, 2)],
            |r| r.get(0),
        )
        .expect("ascolti");
    assert_eq!(ascolti, 9);
    assert_eq!(
        chiavi_d_album(&c),
        vec!["storage/musica|album"; 3],
        "il disco resta uno solo"
    );
}

#[test]
fn un_giro_che_cambia_una_chiave_la_qualita_o_il_disco_rifa_doppioni_e_album() {
    // Il telefono ha i brani 1 e 2 di qui, dello stesso disco; il PC ha il brano 1,
    // nascosto dietro quello di qui.
    let c = libreria();
    locale(&c, 1, 1);
    locale(&c, 2, 2);
    // La stessa qualità delle righe del PC (`riga`): a pari qualità vince questo
    // dispositivo, e la riga del PC sta dietro.
    c.execute(
        "UPDATE tracks SET album_key = 'storage/musica|album', codec = 'mp3', bitrate = 320,
                           sample_rate = 44100, file_size = 8000000, duration_ms = 200000",
        [],
    )
    .expect("chiave d'album e qualità di qui");
    giro(&c, &chiave(PC), &[riga(1)], ADESSO);
    let remoto = id_specchio(&c, PC, 1);
    let copia_di = |c: &Connection, id: i64| -> Option<i64> {
        c.query_row("SELECT copia_di FROM tracks WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .expect("copia_di")
    };
    assert_eq!(
        copia_di(&c, remoto),
        Some(1),
        "il controllo: dietro il brano 1 di qui"
    );
    let mut ora = ADESSO;
    let mut cambia = |c: &Connection, r: crate::library::condivisa::BranoCatalogo, cosa: &str| {
        let prima = rifatti();
        ora += 1;
        giro_con(c, &chiave(PC), &[r], ora);
        assert_eq!(rifatti(), prima + 1, "{cosa}: si rifanno");
    };

    // Un'altra chiave: il brano del PC diventa lo stesso brano del 2.
    let mut stessa = riga(1);
    stessa.track_key = riga(2).track_key;
    stessa.content_key = Some(riga(2).track_key);
    cambia(&c, stessa.clone(), "una chiave");
    assert_eq!(copia_di(&c, remoto), Some(2), "ora sta dietro il brano 2");

    // La qualità: stessa chiave, un file migliore di quello di qui.
    let mut migliore = stessa;
    migliore.bitrate = Some(900);
    migliore.codec = Some("flac".to_owned());
    migliore.dimensione = 24_000_000;
    cambia(&c, migliore.clone(), "la qualità");
    assert_eq!(copia_di(&c, remoto), None, "adesso è lui il migliore");
    assert_eq!(copia_di(&c, 2), Some(remoto), "e il brano 2 gli sta dietro");

    // Il disco: un altro album, che non è più quello di qui.
    let mut altro = migliore;
    altro.album = "Un altro disco".to_owned();
    altro.album_key = Some("b".repeat(32));
    cambia(&c, altro, "il disco");
    let chiavi = chiavi_d_album(&c);
    assert_eq!(chiavi[0], "storage/musica|album");
    assert_ne!(
        chiavi[2], chiavi[0],
        "il brano del PC non è più nel disco di qui"
    );
}

#[test]
fn dimenticare_un_dispositivo_rifa_i_doppioni_solo_se_dietro_di_lui_c_e_qualcosa() {
    use crate::specchio::archivio::comincia_a_dimenticare;
    // Nessuna riga sta dietro quelle del PC: niente da riportare in vista.
    let c = libreria();
    locale(&c, 1, 1);
    giro(&c, &chiave(PC), &[riga(5), riga(6)], ADESSO);
    crate::dispositivi::dimentica(&c, &chiave(PC)).expect("dimenticato dal registro");
    let prima = rifatti();
    assert!(comincia_a_dimenticare(&c, &chiave(PC), ADESSO).expect("primo tempo"));
    assert_eq!(rifatti(), prima, "niente stava dietro il PC");

    // Il brano di qui sta nascosto dietro la riga migliore del PC: dimenticando
    // il PC torna a vedersi, e per questo si rifanno.
    let c = libreria();
    locale(&c, 1, 1);
    let mut migliore = riga(1);
    migliore.bitrate = Some(900);
    migliore.codec = Some("flac".to_owned());
    migliore.dimensione = 24_000_000;
    giro(&c, &chiave(PC), &[migliore], ADESSO);
    let nascosto: Option<i64> = c
        .query_row("SELECT copia_di FROM tracks WHERE id = 1", [], |r| r.get(0))
        .expect("copia_di");
    assert!(
        nascosto.is_some(),
        "il controllo: il brano di qui è nascosto"
    );
    crate::dispositivi::dimentica(&c, &chiave(PC)).expect("dimenticato dal registro");
    let prima = rifatti();
    assert!(comincia_a_dimenticare(&c, &chiave(PC), ADESSO).expect("primo tempo"));
    assert_eq!(rifatti(), prima + 1, "qualcosa stava dietro il PC");
    let dopo: Option<i64> = c
        .query_row("SELECT copia_di FROM tracks WHERE id = 1", [], |r| r.get(0))
        .expect("copia_di");
    assert_eq!(dopo, None, "il brano di qui è tornato in vista");
}

// ── S2-1, la direzione opposta: una copia di qui che se ne va ───────────────

#[test]
fn la_voce_di_una_copia_di_qui_che_se_ne_va_segue_la_chiave_di_contenuto() {
    // Il PC ha corretto un tag: la sua `track_key` non è più quella di qui, ma la
    // chiave di contenuto sì (`lontano_brani.content_key`). Quando la copia di qui
    // se ne va — «Togli dalla libreria», un file sparito — le sue voci di
    // playlist passano alla riga del PC, con la stessa regola di quando se ne va
    // una riga specchio (`altre_copie`); cercandola per `track_key` soltanto non
    // la trovavano, e la cascata le portava via: per la sincronia, un brano che
    // c'è ancora e non sta più nella playlist è un brano che l'utente ha tolto.
    let mut c = libreria();
    c.execute(
        "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, file_size,
                             date_added, date_modified)
         VALUES (1, '/musica/1.mp3', 'artista|vecchio titolo|album', 'artista|vecchio titolo|album',
                 'Vecchio titolo', 'Artista', 'Album', 1000, 1, 1)",
        [],
    )
    .expect("brano di qui");
    let mut corretta = riga(2);
    corretta.track_key = "artista|titolo corretto|album".to_owned();
    corretta.content_key = Some("artista|vecchio titolo|album".to_owned());
    giro(&c, &chiave(PC), &[corretta], ADESSO);
    let remoto = id_specchio(&c, PC, 2);
    let sera = playlist_con(&c, "sera", &[1]);

    let regole = rymembra_domain::paths::PathRules::for_current_platform();
    let fatto = crate::library::cancella_brani(&mut c, &[1], &[], regole).expect("cancellazione");
    assert_eq!(fatto.tolti, 1);
    let voci: Vec<i64> = c
        .prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position")
        .expect("voci")
        .query_map([sera], |r| r.get(0))
        .expect("voci")
        .map(|r| r.expect("voce"))
        .collect();
    assert_eq!(
        voci,
        vec![remoto],
        "la voce è passata alla riga del PC, con la chiave di contenuto"
    );
}
