//! La lettura della finestra senza il lucchetto della libreria (FR2).
//!
//! Con la pagina di un artista aperta ogni `caricaVista` rilegge gli artisti e il
//! conto delle partecipazioni — gli artisti sono un raggruppamento su tutta la
//! tabella dei brani — e durante la lettura del catalogo di un altro dispositivo
//! succede ogni 3 s. Sotto il lucchetto, per tutto il tempo della lettura, una
//! scansione, l'analisi o il filo dello specchio non scrivono: è il reperto 00-9
//! visto dall'altra parte (là era «Sfoglia» che non rispondeva). Le letture della
//! finestra passano ora dalla connessione di sola lettura che già usa il servizio
//! «lontano» (`leggi_a_parte`), con una differenza: quella aspetta il lucchetto
//! solo se il giornale non è WAL, e **non rinuncia** — [`leggi_o_aspetta`].

use std::time::{Duration, Instant};

use rymembra_app::library::{
    OrdineBraniArtista, Partecipazione, TrackOrder, album_tracks, albums_by_artist, brani_artista,
    counts, list_albums, list_artists, list_liked, list_tracks, partecipazioni,
    partecipazioni_conta, search, search_count,
};
use rymembra_app::specchio::Origine;
use rymembra_domain::errors::AppError;

use super::{Stato, con_libreria, leggi_o_aspetta};

/// Un brano di prova: l'id, il titolo, l'artista, l'artista del disco, il disco, la
/// chiave del disco, l'anno, se piace.
type Riga<'a> = (
    i64,
    &'a str,
    &'a str,
    Option<&'a str>,
    &'a str,
    Option<&'a str>,
    i64,
    bool,
);

/// La chiave del dispositivo di prova: sessantaquattro cifre esadecimali.
fn il_pc() -> String {
    "ab".repeat(32)
}

/// Un po' di libreria che dà qualcosa da leggere a ogni funzione: due artisti, una
/// raccolta (le partecipazioni esatte), un «feat.» (le euristiche), brani senza
/// album, preferiti — e un dispositivo collegato, con il caso che F2b ha
/// riscritto: una riga di qui **nascosta** da una copia migliore del PC (la 101
/// nasconde la 3), una riga del PC **in grazia** (la 102, che il PC non manda più)
/// e una riga del PC semplice (la 103). Con una riga nascosta «Tutti» non è più
/// «tutte le righe» (`ci_sono_copie_nascoste`) e le letture passano dalla sua
/// condizione `NOT IN`, dai conteggi per sottrazione e dalla tabella degli album
/// nascosti: tutto SQL che non c'è su una libreria senza dispositivi.
fn metti_della_musica(stato: &Stato) {
    con_libreria(stato, |l| {
        let c = &l.connection;
        let brani: [Riga; 8] = [
            (1, "Uno", "Tale", None, "Primo disco", Some("ak1"), 1999, true),
            (2, "Due", "Tale", None, "Primo disco", Some("ak1"), 1999, false),
            (3, "Tre", "Tale", None, "Secondo disco", Some("ak2"), 2003, false),
            (4, "Quattro", "Tale", Some("Vari artisti"), "Raccolta", Some("ak3"), 2010, true),
            (5, "Cinque", "Altro", Some("Vari artisti"), "Raccolta", Some("ak3"), 2010, false),
            (6, "Sei", "Altro feat. Tale", None, "Un disco", Some("ak4"), 2012, false),
            (7, "Sette", "Tale", None, "", None, 2015, false),
            (8, "Otto", "Altro", None, "Un disco", Some("ak4"), 2012, true),
        ];
        let guasto = |err: rusqlite::Error| rymembra_app::db::codice_da_sqlite("prova", &err);
        for (id, titolo, artista, del_disco, album, chiave, anno, piace) in brani {
            c.execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album_artist, album,
                                     album_key, year, track_number, file_size, date_added,
                                     date_modified, liked)
                 VALUES (?1, '/m/' || ?1 || '.mp3', 'k' || ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?1, 1, 0, 0, ?8)",
                rusqlite::params![id, titolo, artista, del_disco, album, chiave, anno, piace],
            )
            .map_err(guasto)?;
        }
        // Le righe del PC: (id, id nel PC, titolo, disco, chiave del disco,
        // `copia_di`, `sparito_ms`).
        let pc = il_pc();
        let del_pc: [(i64, i64, &str, &str, &str, Option<i64>, Option<i64>); 3] = [
            (101, 1, "Tre", "Secondo disco", "ak2", None, None),
            (102, 2, "Quattordici", "Terzo disco", "ak5", Some(102), Some(1_000)),
            (103, 3, "Quindici", "Primo disco", "ak1", None, None),
        ];
        for (id, remoto, titolo, album, chiave, copia_di, sparito) in del_pc {
            c.execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key, year,
                                     track_number, file_size, date_added, date_modified, source,
                                     copia_di)
                 VALUES (?1, 'lontano://' || ?2 || '/' || ?3, 'm' || ?1, ?4, 'Tale', ?5, ?6, 2003,
                         1, 1, 0, 0, 'lontano', ?7)",
                rusqlite::params![id, pc, remoto, titolo, album, chiave, copia_di],
            )
            .map_err(guasto)?;
            c.execute(
                "INSERT INTO lontano_brani (track_id, dispositivo, id_remoto, sparito_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, pc, remoto, sparito],
            )
            .map_err(guasto)?;
        }
        // La copia migliore è quella del PC: la riga di qui sparisce da «Tutti».
        c.execute("UPDATE tracks SET copia_di = 101 WHERE id = 3", [])
            .map_err(guasto)?;
        let tx = c.unchecked_transaction().map_err(guasto)?;
        rymembra_app::library::ricostruisci_aggregati(
            &tx,
            &rymembra_app::library::Toccati::tutti(),
        )?;
        tx.commit().map_err(guasto)
    })
    .expect("la musica di prova");
}

#[test]
fn una_lettura_della_finestra_non_aspetta_chi_tiene_il_lucchetto() {
    // Il lavoro lento è simulato: tiene il lucchetto e una scrittura aperta
    // finché la lettura non ha risposto, o ben oltre quel che la lettura
    // aspetterebbe. Come `una_scrittura_lunga_qui_non_ferma_chi_sfoglia_da_lontano`
    // per il servizio «lontano», ma dalla parte della finestra.
    let dir = tempfile::tempdir().expect("cartella");
    let stato = Stato::apri(dir.path().to_path_buf());
    let stato = &stato;
    let giornale = con_libreria(stato, |l| Ok(l.giornale.clone())).expect("libreria aperta");
    assert_eq!(giornale, "wal", "senza WAL questa prova non prova niente");
    metti_della_musica(stato);

    let (preso, lavoro_partito) = std::sync::mpsc::channel::<()>();
    let (risposto, lettura_finita) = std::sync::mpsc::channel::<()>();
    std::thread::scope(|ambito| {
        let lavoro = ambito.spawn(move || {
            con_libreria(stato, |l| {
                let tx = l
                    .connection
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                    .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
                tx.execute("UPDATE tracks SET title = 'Cambiato' WHERE id = 1", [])
                    .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
                let _ = preso.send(());
                let _ = lettura_finita.recv_timeout(Duration::from_secs(5));
                tx.commit()
                    .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))
            })
        });
        lavoro_partito
            .recv()
            .expect("il lavoro ha preso la libreria");

        let inizio = Instant::now();
        let artisti = leggi_o_aspetta(stato, |c| list_artists(c, &Origine::Tutti));
        let tempo = inizio.elapsed();
        let _ = risposto.send(());
        lavoro
            .join()
            .expect("il lavoro non cade")
            .expect("il lavoro scrive");

        let artisti = artisti.expect("la lettura ha risposto");
        assert!(
            artisti.iter().any(|a| a.name == "Tale"),
            "la lettura vede quel che era scritto prima: {artisti:?}"
        );
        assert!(
            tempo < Duration::from_secs(2),
            "la lettura ha aspettato il lavoro: {tempo:?}"
        );
    });
    // Finito il lavoro, la lettura dopo vede anche la sua scrittura.
    let dopo = leggi_o_aspetta(stato, |c| {
        list_tracks(c, &Origine::Tutti, TrackOrder::Title, 0, 20)
    })
    .expect("la lettura dopo");
    assert!(dopo.iter().any(|b| b.title == "Cambiato"));
}

/// Il difetto che una connessione nuova può avere: quella principale tiene le
/// funzioni che una query precedente ha registrato (`assicura_ordine`), una nuova
/// no. Una query che si affidasse a una funzione senza registrarla passerebbe
/// sotto il lucchetto e fallirebbe qui con «no such function». Ogni lettura che
/// la finestra fa passare di qui dà lo stesso su tutte e due, per ogni ordine e
/// per ogni origine — `Tutti` con una riga nascosta e una in grazia, la sola
/// condizione che F2b ha riscritto, `Locale` e il dispositivo.
#[test]
fn le_letture_della_finestra_danno_lo_stesso_su_una_connessione_nuova() {
    let dir = tempfile::tempdir().expect("cartella");
    let stato = Stato::apri(dir.path().to_path_buf());
    let stato = &stato;
    metti_della_musica(stato);

    /// La stessa lettura sotto il lucchetto e su una connessione nuova. `piena`:
    /// la prova pretende che la lettura trovi qualcosa, perché una lettura vuota
    /// è uguale a se stessa senza provare niente.
    fn uguali<T: serde::Serialize>(
        stato: &Stato,
        cosa: &str,
        piena: bool,
        lettura: impl Fn(&rusqlite::Connection) -> Result<T, AppError>,
    ) {
        let sotto = con_libreria(stato, |l| lettura(&l.connection)).expect(cosa);
        let fuori = leggi_o_aspetta(stato, |c| lettura(c)).expect(cosa);
        let sotto = serde_json::to_value(sotto).expect("json");
        let fuori = serde_json::to_value(fuori).expect("json");
        assert_eq!(sotto, fuori, "{cosa}");
        assert!(
            !piena || (!sotto.is_null() && sotto != serde_json::json!([])),
            "{cosa}: la prova non legge niente"
        );
    }

    // Il caso che le righe nascoste cambiano: lo si guarda prima, perché se «Tutti»
    // non nascondesse niente la prova qui sotto non toccherebbe il codice nuovo.
    let ids = con_libreria(stato, |l| {
        list_tracks(&l.connection, &Origine::Tutti, TrackOrder::Title, 0, 50)
    })
    .expect("brani")
    .iter()
    .map(|b| b.id)
    .collect::<Vec<_>>();
    assert!(
        !ids.contains(&3),
        "la riga di qui è nascosta dalla copia del PC: {ids:?}"
    );
    assert!(ids.contains(&101), "e la copia del PC si vede: {ids:?}");
    assert!(
        !ids.contains(&102),
        "la riga in grazia non si vede: {ids:?}"
    );

    for origine in [
        Origine::Tutti,
        Origine::Locale,
        Origine::Dispositivo(il_pc()),
    ] {
        // Quel che sta solo su questa macchina — i preferiti, le raccolte, i «feat.» —
        // non c'è nel filtro del dispositivo, e lì una lettura vuota è la risposta giusta.
        let di_qui = !matches!(origine, Origine::Dispositivo(_));
        let o = &origine;
        uguali(stato, "artisti", true, |c| list_artists(c, o));
        uguali(stato, "conteggi", true, |c| counts(c, o));
        uguali(stato, "album", true, |c| list_albums(c, o, 0, 50));
        uguali(stato, "album di un artista", true, |c| {
            albums_by_artist(c, o, "Tale", 0, 50)
        });
        for ordine in [
            TrackOrder::Shelf,
            TrackOrder::Title,
            TrackOrder::RecentlyAdded,
            TrackOrder::MostPlayed,
        ] {
            uguali(stato, "brani", true, |c| list_tracks(c, o, ordine, 0, 50));
        }
        for ordine in [
            OrdineBraniArtista::Album,
            OrdineBraniArtista::Anno,
            OrdineBraniArtista::Titolo,
            OrdineBraniArtista::PiuAscoltati,
        ] {
            uguali(stato, "brani di un artista", true, |c| {
                brani_artista(c, o, "Tale", ordine, 0, 50)
            });
        }
        uguali(stato, "partecipazioni esatte", di_qui, |c| {
            partecipazioni(c, o, "Tale", Partecipazione::Esatta, 0, 50)
        });
        uguali(stato, "partecipazioni euristiche", di_qui, |c| {
            partecipazioni(c, o, "Tale", Partecipazione::Euristica, 0, 50)
        });
        uguali(stato, "conto delle partecipazioni", true, |c| {
            partecipazioni_conta(c, o, "Tale")
        });
        uguali(stato, "brani di un album", true, |c| {
            album_tracks(c, o, "ak2")
        });
        uguali(stato, "preferiti", di_qui, |c| list_liked(c, o, 0, 50));
        uguali(stato, "ricerca", true, |c| search(c, o, "Tale", 0, 50));
        uguali(stato, "conteggio della ricerca", true, |c| {
            search_count(c, o, "Tale")
        });
    }
}
