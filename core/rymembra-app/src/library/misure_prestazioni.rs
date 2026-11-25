//! Le misure di R33-prestazioni: quanto costano, su una libreria vera di
//! ventinovemila brani, le query che la finestra fa quando si cambia
//! l'ordinamento dei Brani, si apre un artista e si apre l'applicazione.
//!
//! Non sono prove di un comportamento: **scrivono i tempi** (`println!`). Per
//! questo sono `#[ignore]` e si lanciano a mano, su una copia del database che
//! la prova stessa fa in una cartella temporanea (`db::open` scrive: ottimizza,
//! rifà le chiavi d'ordine, e non si apre mai l'originale due volte):
//!
//! ```text
//! $env:RYMEMBRA_MISURA_DB = 'D:\...\head-043.db'
//! cargo test -p rymembra-app --locked --lib misura_prestazioni -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Senza la variabile le prove escono subito: nessun database vero è nel
//! repository, e un'esecuzione di `--ignored` generica non deve cadere.
//!
//! # Come si confronta
//!
//! «Prima» e «dopo» si misurano **nella stessa esecuzione**, alternati: ogni giro
//! fa una volta la query nuova e una volta la vecchia, nell'ordine che cambia a
//! ogni giro, e di ogni coppia si scrivono mediana, minimo e massimo. Il PC è
//! condiviso e il profilo è quello di sviluppo — SQLite è compilato senza
//! ottimizzazioni —, quindi i millisecondi assoluti contano meno dei rapporti.
//!
//! Il testo «vecchio» delle query è quello di `8ea12b2`, il commit da cui è
//! uscito il collaudo R21 dell'11/11 (`R21-prestazioni.md`).

use std::time::{Duration, Instant};

use rusqlite::Connection;

use super::{
    ArtistSummary, OrdineBraniArtista, Partecipazione, TrackOrder, albums_by_artist,
    ascoltati_di_recente, brani_artista, counts, list_artists, list_tracks, partecipazioni,
    partecipazioni_conta, recently_added_albums, search, search_count,
};
use crate::specchio::Origine;

/// Quante volte si ripete ogni misura.
const GIRI: usize = 9;

/// La pagina che la finestra chiede: duecento righe.
const PAGINA: i64 = 200;

/// Le colonne di un brano come le leggeva R21 (17, senza l'artista del disco e
/// senza la sottoquery delle copie).
const COLONNE_DI_R21: &str = "t.id, t.path, t.title, t.artist, t.album, t.album_key,
     t.track_number, t.disc_number, t.duration_ms, t.year, t.cover_art_hash,
     t.play_count, t.liked, t.rating, t.source_service, t.fonte_pagina, t.licenza";

/// Una copia del database da misurare, aperta come l'apre l'applicazione.
fn copia_aperta() -> Option<(tempfile::TempDir, Connection)> {
    let origine = std::env::var_os("RYMEMBRA_MISURA_DB")?;
    let cartella = tempfile::tempdir().expect("cartella temporanea");
    let percorso = cartella.path().join("rymembra.db");
    std::fs::copy(&origine, &percorso).expect("copia del database");
    let aperto = crate::db::open(&percorso).expect("il database si apre");
    Some((cartella, aperto.connection))
}

/// Mediana, minimo e massimo di una serie, in millisecondi.
fn riassunto(tempi: &[Duration]) -> (f64, f64, f64) {
    let mut ms: Vec<f64> = tempi.iter().map(|t| t.as_secs_f64() * 1000.0).collect();
    ms.sort_by(f64::total_cmp);
    let mediana = ms.get(ms.len().div_euclid(2)).copied().unwrap_or(0.0);
    (
        mediana,
        ms.first().copied().unwrap_or(0.0),
        ms.last().copied().unwrap_or(0.0),
    )
}

/// Misura `azione` `GIRI` volte, dopo un giro a vuoto che riscalda la cache.
fn misura(nome: &str, mut azione: impl FnMut()) -> f64 {
    azione();
    let tempi: Vec<Duration> = (0..GIRI)
        .map(|_| {
            let inizio = Instant::now();
            azione();
            inizio.elapsed()
        })
        .collect();
    let (mediana, minimo, massimo) = riassunto(&tempi);
    println!("MISURA {nome}: mediana {mediana:.2} ms (min {minimo:.2}, max {massimo:.2})");
    mediana
}

/// Misura due azioni alternate: a ogni giro cambia chi va per primo, così la
/// cache e il carico del PC non favoriscono sempre la stessa. Scrive le due
/// mediane e il rapporto.
fn confronta(nome: &str, mut nuova: impl FnMut(), mut vecchia: impl FnMut()) {
    nuova();
    vecchia();
    let mut tempi_nuova = Vec::new();
    let mut tempi_vecchia = Vec::new();
    for giro in 0..GIRI {
        let cronometra = |azione: &mut dyn FnMut(), dove: &mut Vec<Duration>| {
            let inizio = Instant::now();
            azione();
            dove.push(inizio.elapsed());
        };
        if giro % 2 == 0 {
            cronometra(&mut nuova, &mut tempi_nuova);
            cronometra(&mut vecchia, &mut tempi_vecchia);
        } else {
            cronometra(&mut vecchia, &mut tempi_vecchia);
            cronometra(&mut nuova, &mut tempi_nuova);
        }
    }
    let (n, n_min, n_max) = riassunto(&tempi_nuova);
    let (v, v_min, v_max) = riassunto(&tempi_vecchia);
    let rapporto = if v > 0.0 { n / v } else { 0.0 };
    println!(
        "CONFRONTO {nome}: oggi {n:.2} ms (min {n_min:.2}, max {n_max:.2}) | prima {v:.2} ms (min {v_min:.2}, max {v_max:.2}) | oggi/prima {rapporto:.2}"
    );
}

/// Esegue una query e legge ogni colonna di ogni riga, come fa un
/// `query_map` che costruisce le righe: senza leggerle SQLite potrebbe non
/// toccarle. Restituisce quante righe.
fn esegui(connection: &Connection, sql: &str, parametri: &[&dyn rusqlite::ToSql]) -> usize {
    let mut istruzione = connection.prepare_cached(sql).expect("query preparata");
    let mut righe = istruzione.query(parametri).expect("query eseguita");
    let mut quante = 0;
    while let Some(riga) = righe.next().expect("riga") {
        for colonna in 0..riga.as_ref().column_count() {
            let _ = riga.get_ref(colonna).expect("colonna");
        }
        quante += 1;
    }
    quante
}

/// Il piano di una query, una riga per passo.
fn piano(connection: &Connection, sql: &str, parametri: &[&dyn rusqlite::ToSql]) -> String {
    let mut istruzione = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("piano preparato");
    let mut righe = istruzione.query(parametri).expect("piano eseguito");
    let mut passi = Vec::new();
    while let Some(riga) = righe.next().expect("riga del piano") {
        passi.push(riga.get::<_, String>(3).expect("dettaglio"));
    }
    passi.join(" | ")
}

fn ordine_di_r21(ordine: TrackOrder) -> &'static str {
    match ordine {
        TrackOrder::Shelf => {
            "t.artist COLLATE NOCASE, t.album COLLATE NOCASE,
             t.disc_number, t.track_number, t.title COLLATE NOCASE"
        }
        TrackOrder::RecentlyAdded => "t.date_added DESC, t.id DESC",
        TrackOrder::MostPlayed => {
            "t.play_count DESC, t.last_played_at DESC, t.title COLLATE NOCASE"
        }
        TrackOrder::Title => "t.title COLLATE NOCASE, t.artist COLLATE NOCASE",
        TrackOrder::RecentlyPlayed => {
            "t.last_played_at IS NULL, t.last_played_at DESC, t.title COLLATE NOCASE"
        }
    }
}

#[test]
#[ignore = "misura: serve RYMEMBRA_MISURA_DB, vedi il commento del modulo"]
fn misura_prestazioni_brani_per_ordine() {
    let Some((_cartella, c)) = copia_aperta() else {
        println!("RYMEMBRA_MISURA_DB non è impostata: niente da misurare");
        return;
    };
    let ties: i64 = c
        .query_row("SELECT COUNT(DISTINCT date_added) FROM tracks", [], |r| {
            r.get(0)
        })
        .expect("date distinte");
    let brani: i64 = c
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("brani");
    println!("LIBRERIA brani={brani} date_added_distinte={ties}");

    let tutti = Origine::Tutti;
    for (nome, ordine) in [
        ("scaffale", TrackOrder::Shelf),
        ("recenti", TrackOrder::RecentlyAdded),
        ("piu-ascoltati", TrackOrder::MostPlayed),
        ("titolo", TrackOrder::Title),
    ] {
        let vecchio = format!(
            "SELECT {COLONNE_DI_R21} FROM tracks t ORDER BY {} LIMIT ?1 OFFSET ?2",
            ordine_di_r21(ordine)
        );
        println!(
            "PIANO {nome} oggi: {}",
            piano(
                &c,
                &format!(
                    "SELECT {} FROM tracks t WHERE 1 ORDER BY {} LIMIT ?1 OFFSET ?2",
                    super::COLONNE_BRANO,
                    ordine.sql()
                ),
                &[&PAGINA, &0_i64]
            )
        );
        println!(
            "PIANO {nome} R21: {}",
            piano(&c, &vecchio, &[&PAGINA, &0_i64])
        );
        for offset in [0_i64, 14_600, 29_000] {
            confronta(
                &format!("brani {nome} offset {offset}"),
                || {
                    let righe = list_tracks(&c, &tutti, ordine, offset, PAGINA).expect("pagina");
                    assert!(!righe.is_empty() || offset >= brani);
                },
                || {
                    let _ = esegui(&c, &vecchio, &[&PAGINA, &offset]);
                },
            );
        }
    }

    // «Recenti»: l'ordine di oggi e quello di R21 sulle **stesse colonne di
    // oggi**, per separare quanto pesa l'ordine da quanto pesano le colonne.
    let con_le_colonne_di_oggi = |ordine: &str| {
        format!(
            "SELECT {} FROM tracks t WHERE 1 ORDER BY {ordine} LIMIT ?1 OFFSET ?2",
            super::COLONNE_BRANO
        )
    };
    let oggi = con_le_colonne_di_oggi(TrackOrder::RecentlyAdded.sql());
    let vecchio_ordine = con_le_colonne_di_oggi("t.date_added DESC, t.id DESC");
    println!(
        "PIANO recenti, ordine di oggi: {}",
        piano(&c, &oggi, &[&PAGINA, &0_i64])
    );
    println!(
        "PIANO recenti, ordine di R21: {}",
        piano(&c, &vecchio_ordine, &[&PAGINA, &0_i64])
    );
    for offset in [0_i64, 14_600, 29_000] {
        confronta(
            &format!("recenti, solo l'ordine, offset {offset}"),
            || {
                let _ = esegui(&c, &oggi, &[&PAGINA, &offset]);
            },
            || {
                let _ = esegui(&c, &vecchio_ordine, &[&PAGINA, &offset]);
            },
        );
    }
    // Quel che costa chiedere **tutti** gli id nell'ordine di oggi («Suona da
    // qui», «Seleziona tutto»): è dove l'ordine senza indice si paga per intero.
    let id_oggi = format!(
        "SELECT t.id FROM tracks t WHERE 1 ORDER BY {}",
        TrackOrder::RecentlyAdded.sql()
    );
    let id_vecchio = "SELECT t.id FROM tracks t WHERE 1 ORDER BY t.date_added DESC, t.id DESC";
    println!("PIANO id recenti oggi: {}", piano(&c, &id_oggi, &[]));
    println!("PIANO id recenti R21: {}", piano(&c, id_vecchio, &[]));
    confronta(
        "recenti, tutti gli id",
        || {
            let _ = esegui(&c, &id_oggi, &[]);
        },
        || {
            let _ = esegui(&c, id_vecchio, &[]);
        },
    );
}

#[test]
#[ignore = "misura: serve RYMEMBRA_MISURA_DB, vedi il commento del modulo"]
fn misura_prestazioni_pagina_dell_artista() {
    let Some((_cartella, c)) = copia_aperta() else {
        println!("RYMEMBRA_MISURA_DB non è impostata: niente da misurare");
        return;
    };
    let tutti = Origine::Tutti;
    let artisti: Vec<ArtistSummary> = list_artists(&c, &tutti).expect("artisti");
    let Some(grande) = artisti.iter().max_by_key(|a| (a.albums, a.tracks)) else {
        println!("nessun artista");
        return;
    };
    // Il più grande della copia, e quello che il collaudo dal vivo apriva
    // («Città», 20 album).
    let mut nomi = vec![grande.name.clone()];
    if artisti.iter().any(|a| a.name == "Città") {
        nomi.push("Città".to_owned());
    }
    for nome in nomi {
        let riga = artisti.iter().find(|a| a.name == nome).expect("riga");
        println!(
            "ARTISTA {nome:?}: {} album, {} brani, su {} artisti",
            riga.albums,
            riga.tracks,
            artisti.len()
        );
        let conto = partecipazioni_conta(&c, &tutti, &nome).expect("conto");
        println!(
            "ARTISTA partecipazioni: {} esatte, {} euristiche",
            conto.esatte, conto.euristiche
        );

        // Le chiamate che la pagina fa quando si apre un artista (`App.tsx`,
        // «la pagina di un artista»): il conto delle partecipazioni, **tutti** gli
        // artisti per rileggere i due numeri, i suoi album, la prima pagina dei suoi
        // brani, e le partecipazioni se ce ne sono.
        let t_conto = misura("artista: partecipazioni_conta", || {
            let _ = partecipazioni_conta(&c, &tutti, &nome).expect("conto");
        });
        let t_artisti = misura(
            "artista: list_artists (tutti, per rileggere i due numeri)",
            || {
                let _ = list_artists(&c, &tutti).expect("artisti");
            },
        );
        let t_album = misura("artista: albums_by_artist (pagina)", || {
            let _ = albums_by_artist(&c, &tutti, &nome, 0, PAGINA).expect("album");
        });
        let t_brani = misura("artista: brani_artista (pagina, per album)", || {
            let _ = brani_artista(&c, &tutti, &nome, OrdineBraniArtista::Album, 0, PAGINA)
                .expect("brani");
        });
        let t_esatte = misura("artista: partecipazioni esatte (pagina)", || {
            let _ = partecipazioni(&c, &tutti, &nome, Partecipazione::Esatta, 0, PAGINA)
                .expect("esatte");
        });
        let t_euristiche = misura("artista: partecipazioni euristiche (pagina)", || {
            let _ = partecipazioni(&c, &tutti, &nome, Partecipazione::Euristica, 0, PAGINA)
                .expect("euristiche");
        });
        println!(
            "SOMMA artista: tutte le chiamate {:.2} ms, di cui list_artists {:.2} ms",
            t_conto + t_artisti + t_album + t_brani + t_esatte + t_euristiche,
            t_artisti
        );

        // I piani delle tre query nuove.
        println!(
            "PIANO artista, brani: {}",
            piano(
                &c,
                &format!(
                    "SELECT {} {} LIMIT ?2 OFFSET ?3",
                    super::COLONNE_BRANO,
                    super::artista::da_brani_artista(&c, &tutti, OrdineBraniArtista::Album)
                        .expect("da_brani_artista")
                ),
                &[&nome, &PAGINA, &0_i64]
            )
        );
        println!(
            "PIANO artista, album: {}",
            piano(
                &c,
                &super::interrogazioni::sql_degli_album_di_un_artista(&c, &tutti)
                    .expect("sql degli album"),
                &[&nome, &PAGINA, &0_i64]
            )
        );
        let frase = format!("artist : \"{}\"", nome.replace('"', "\"\""));
        println!(
            "PIANO artista, conto euristiche: {}",
            piano(
                &c,
                &format!(
                    "SELECT COUNT(*) FROM tracks t
                  WHERE t.id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1)
                    AND t.artist <> ?2 AND {} <> ?2 AND 1",
                    super::ARTISTA_EFFETTIVO
                ),
                &[&frase, &nome]
            )
        );
    }
}

#[test]
#[ignore = "misura: serve RYMEMBRA_MISURA_DB, vedi il commento del modulo"]
fn misura_prestazioni_avvio_e_ricerca() {
    let Some((_cartella, c)) = copia_aperta() else {
        println!("RYMEMBRA_MISURA_DB non è impostata: niente da misurare");
        return;
    };
    let tutti = Origine::Tutti;
    // Quel che la finestra chiede per disegnare la Home (`avvio` e i ripiani).
    misura("avvio: counts", || {
        let _ = counts(&c, &tutti).expect("numeri");
    });
    // I cinque numeri di `counts` uno per uno, coi loro piani: quel che pesa a
    // freddo non è il tempo a cache calda ma quante pagine del database si
    // leggono, e una `SUM` su una colonna senza indice le legge tutte.
    for (nome, sql) in [
        ("brani", "SELECT COUNT(*) FROM tracks"),
        ("album", "SELECT COUNT(*) FROM albums"),
        ("artisti", "SELECT COUNT(*) FROM artists"),
        ("preferiti", "SELECT COUNT(*) FROM tracks WHERE liked = 1"),
        (
            "durata totale",
            "SELECT COALESCE(SUM(duration_ms), 0) FROM tracks",
        ),
    ] {
        println!("PIANO counts, {nome}: {}", piano(&c, sql, &[]));
        misura(&format!("avvio: counts, {nome}"), || {
            let _ = esegui(&c, sql, &[]);
        });
    }
    // Quanto pesano sul disco la tabella e i suoi indici: a freddo, una
    // scansione di `tracks` legge tutto quel che c'è qui sotto.
    let page_size: i64 = c
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .expect("page_size");
    let pagine: i64 = c
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .expect("page_count");
    println!(
        "DATABASE {} pagine da {page_size} byte = {:.1} MiB",
        pagine,
        (pagine * page_size) as f64 / 1_048_576.0
    );
    if let Ok(mut istruzione) = c.prepare(
        "SELECT name, SUM(pgsize), COUNT(*) FROM dbstat GROUP BY name ORDER BY 2 DESC LIMIT 14",
    ) {
        let righe = istruzione
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .expect("dbstat");
        for riga in righe.flatten() {
            println!(
                "DATABASE {:<34} {:>7.2} MiB {:>6} pagine",
                riga.0,
                riga.1 as f64 / 1_048_576.0,
                riga.2
            );
        }
    } else {
        println!("DATABASE dbstat non c'è in questo SQLite");
    }
    misura("avvio: ascoltati_di_recente(12)", || {
        let _ = ascoltati_di_recente(&c, &tutti, 12).expect("recenti");
    });
    misura("avvio: recently_added_albums(12)", || {
        let _ = recently_added_albums(&c, &tutti, 12).expect("dischi");
    });
    misura("avvio: trascurati(12)", || {
        let _ = super::trascurati(&c, 1_795_000_000_000, 12).expect("trascurati");
    });
    misura("avvio: list_artists (la vista Artisti)", || {
        let _ = list_artists(&c, &tutti).expect("artisti");
    });
    for testo in ["a", "notte"] {
        misura(&format!("ricerca {testo:?}: search (pagina)"), || {
            let _ = search(&c, &tutti, testo, 0, PAGINA).expect("ricerca");
        });
        misura(&format!("ricerca {testo:?}: search_count"), || {
            let _ = search_count(&c, &tutti, testo).expect("conteggio");
        });
    }
    // L'apertura del database: quel che il filo `rymembra-apertura` fa prima di
    // dire «pronta». Ogni giro parte da una copia nuova, e la copia non si conta.
    let origine = std::env::var_os("RYMEMBRA_MISURA_DB").expect("variabile");
    let mut tempi = Vec::new();
    for _ in 0..5 {
        let cartella = tempfile::tempdir().expect("cartella");
        let percorso = cartella.path().join("rymembra.db");
        std::fs::copy(&origine, &percorso).expect("copia");
        let inizio = Instant::now();
        let aperto = crate::db::open(&percorso).expect("apertura");
        tempi.push(inizio.elapsed());
        drop(aperto);
    }
    let (mediana, minimo, massimo) = riassunto(&tempi);
    println!(
        "MISURA apertura: db::open su una copia (mediana {mediana:.2} ms, min {minimo:.2}, max {massimo:.2})"
    );
    println!(
        "PIANO controllo delle chiavi d'ordine all'apertura: {}",
        piano(
            &c,
            "SELECT EXISTS (SELECT 1 FROM tracks WHERE ordine_titolo IS NULL)",
            &[]
        )
    );
}

#[test]
#[ignore = "misura: serve RYMEMBRA_MISURA_DB, vedi il commento del modulo"]
fn misura_prestazioni_con_copie_nascoste() {
    // Il collaudo dal vivo non aveva dispositivi collegati: nessuna riga
    // specchio, nessuna copia nascosta, e `filtro` rispondeva «niente da
    // filtrare». Chi ha un telefono collegato sì: «Tutti» toglie le righe
    // dietro una copia migliore. Qui si nasconde una riga su cinque (20 %, più
    // di quanto un telefono con la stessa musica ne nasconda) e si guarda quanto
    // costa il filtro rispetto alla stessa libreria senza niente di nascosto.
    let Some((_cartella_a, senza)) = copia_aperta() else {
        println!("RYMEMBRA_MISURA_DB non è impostata: niente da misurare");
        return;
    };
    let (_cartella_b, con) = copia_aperta().expect("seconda copia");
    let nascoste = con
        .execute(
            "UPDATE tracks SET copia_di = id + 1
              WHERE id % 5 = 0 AND id < (SELECT MAX(id) FROM tracks)",
            [],
        )
        .expect("copie nascoste");
    println!("NASCOSTE {nascoste} righe nascoste dietro una copia migliore");
    let tutti = Origine::Tutti;
    for (nome, ordine) in [
        ("scaffale", TrackOrder::Shelf),
        ("recenti", TrackOrder::RecentlyAdded),
        ("titolo", TrackOrder::Title),
    ] {
        for offset in [0_i64, 14_600] {
            confronta(
                &format!("con nascoste / senza, brani {nome} offset {offset}"),
                || {
                    let _ = list_tracks(&con, &tutti, ordine, offset, PAGINA).expect("pagina");
                },
                || {
                    let _ = list_tracks(&senza, &tutti, ordine, offset, PAGINA).expect("pagina");
                },
            );
        }
    }
    confronta(
        "con nascoste / senza, counts",
        || {
            let _ = counts(&con, &tutti).expect("numeri");
        },
        || {
            let _ = counts(&senza, &tutti).expect("numeri");
        },
    );
    confronta(
        "con nascoste / senza, list_artists",
        || {
            let _ = list_artists(&con, &tutti).expect("artisti");
        },
        || {
            let _ = list_artists(&senza, &tutti).expect("artisti");
        },
    );
    let artisti = list_artists(&senza, &tutti).expect("artisti");
    if let Some(grande) = artisti.iter().max_by_key(|a| (a.albums, a.tracks)) {
        let nome = grande.name.clone();
        confronta(
            "con nascoste / senza, albums_by_artist",
            || {
                let _ = albums_by_artist(&con, &tutti, &nome, 0, PAGINA).expect("album");
            },
            || {
                let _ = albums_by_artist(&senza, &tutti, &nome, 0, PAGINA).expect("album");
            },
        );
        confronta(
            "con nascoste / senza, brani_artista",
            || {
                let _ = brani_artista(&con, &tutti, &nome, OrdineBraniArtista::Album, 0, PAGINA)
                    .expect("brani");
            },
            || {
                let _ = brani_artista(&senza, &tutti, &nome, OrdineBraniArtista::Album, 0, PAGINA)
                    .expect("brani");
            },
        );
    }
    confronta(
        "con nascoste / senza, search 'notte'",
        || {
            let _ = search(&con, &tutti, "notte", 0, PAGINA).expect("ricerca");
        },
        || {
            let _ = search(&senza, &tutti, "notte", 0, PAGINA).expect("ricerca");
        },
    );
}
