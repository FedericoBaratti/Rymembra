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

/// Un po' di libreria che dà qualcosa da leggere a ogni funzione: due artisti, una
/// raccolta (le partecipazioni esatte), un «feat.» (le euristiche), brani senza
/// album, preferiti.
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
        for (id, titolo, artista, del_disco, album, chiave, anno, piace) in brani {
            c.execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album_artist, album,
                                     album_key, year, track_number, file_size, date_added,
                                     date_modified, liked)
                 VALUES (?1, '/m/' || ?1 || '.mp3', 'k' || ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?1, 1, 0, 0, ?8)",
                rusqlite::params![id, titolo, artista, del_disco, album, chiave, anno, piace],
            )
            .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
        }
        let tx = c
            .unchecked_transaction()
            .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
        rymembra_app::library::ricostruisci_aggregati(&tx, &rymembra_app::library::Toccati::tutti())?;
        tx.commit()
            .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))
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
/// la finestra fa passare di qui dà lo stesso su tutte e due, per ogni ordine.
#[test]
fn le_letture_della_finestra_danno_lo_stesso_su_una_connessione_nuova() {
    let dir = tempfile::tempdir().expect("cartella");
    let stato = Stato::apri(dir.path().to_path_buf());
    let stato = &stato;
    metti_della_musica(stato);

    /// La stessa lettura sotto il lucchetto e su una connessione nuova.
    fn uguali<T: serde::Serialize>(
        stato: &Stato,
        cosa: &str,
        lettura: impl Fn(&rusqlite::Connection) -> Result<T, AppError>,
    ) {
        let sotto = con_libreria(stato, |l| lettura(&l.connection)).expect(cosa);
        let fuori = leggi_o_aspetta(stato, |c| lettura(c)).expect(cosa);
        let sotto = serde_json::to_value(sotto).expect("json");
        let fuori = serde_json::to_value(fuori).expect("json");
        assert_eq!(sotto, fuori, "{cosa}");
        assert!(
            !sotto.is_null() && sotto != serde_json::json!([]),
            "{cosa}: la prova non legge niente"
        );
    }

    let tutti = Origine::Tutti;
    uguali(stato, "artisti", |c| list_artists(c, &tutti));
    uguali(stato, "conteggi", |c| counts(c, &tutti));
    uguali(stato, "album", |c| list_albums(c, &tutti, 0, 50));
    uguali(stato, "album di un artista", |c| {
        albums_by_artist(c, &tutti, "Tale", 0, 50)
    });
    for ordine in [
        TrackOrder::Shelf,
        TrackOrder::Title,
        TrackOrder::RecentlyAdded,
        TrackOrder::MostPlayed,
    ] {
        uguali(stato, "brani", |c| list_tracks(c, &tutti, ordine, 0, 50));
    }
    for ordine in [
        OrdineBraniArtista::Album,
        OrdineBraniArtista::Anno,
        OrdineBraniArtista::Titolo,
        OrdineBraniArtista::PiuAscoltati,
    ] {
        uguali(stato, "brani di un artista", |c| {
            brani_artista(c, &tutti, "Tale", ordine, 0, 50)
        });
    }
    uguali(stato, "partecipazioni esatte", |c| {
        partecipazioni(c, &tutti, "Tale", Partecipazione::Esatta, 0, 50)
    });
    uguali(stato, "partecipazioni euristiche", |c| {
        partecipazioni(c, &tutti, "Tale", Partecipazione::Euristica, 0, 50)
    });
    uguali(stato, "conto delle partecipazioni", |c| {
        partecipazioni_conta(c, &tutti, "Tale")
    });
    uguali(stato, "brani di un album", |c| {
        album_tracks(c, &tutti, "ak1")
    });
    uguali(stato, "preferiti", |c| list_liked(c, &tutti, 0, 50));
    uguali(stato, "ricerca", |c| search(c, &tutti, "Tale", 0, 50));
    uguali(stato, "conteggio della ricerca", |c| {
        search_count(c, &tutti, "Tale")
    });
    // E dal punto di vista di un dispositivo: lo stesso filtro, un altro SQL.
    let locale = Origine::Locale;
    uguali(stato, "artisti di qui", |c| list_artists(c, &locale));
}
