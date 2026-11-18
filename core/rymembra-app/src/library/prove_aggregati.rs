//! Le prove della regola con cui si costruiscono gli aggregati, quando cambia.

use rusqlite::Connection;

use super::*;

/// Una libreria vuota, con lo schema aggiornato.
fn libreria() -> Connection {
    crate::db::open_in_memory().expect("database").connection
}

/// Un brano su file, con la chiave d'album che gli si dà.
fn su_file(connection: &Connection, id: i64, path: &str, album: &str, album_key: &str) {
    connection
        .execute(
            "INSERT INTO tracks
               (id, path, track_key, title, artist, album, album_key,
                duration_ms, file_size, date_added, date_modified)
             VALUES (?1, ?2, ?3, 'Brano', 'Pink Floyd', ?4, ?5, 200000, 1, 1, 1)",
            rusqlite::params![id, path, format!("k{id}"), album, album_key],
        )
        .expect("brano");
}

fn versione(connection: &Connection) -> i64 {
    connection
        .query_row(
            "SELECT aggregati FROM libreria_stato WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .expect("versione")
}

fn chiavi(connection: &Connection) -> Vec<(i64, String)> {
    let mut statement = connection
        .prepare("SELECT id, album_key FROM tracks ORDER BY id")
        .expect("chiavi");
    statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("chiavi")
        .collect::<Result<Vec<_>, _>>()
        .expect("chiavi")
}

#[test]
fn una_libreria_della_regola_di_prima_si_rimette_in_riga() {
    // Una libreria costruita con la regola di prima: i due dischi di «The
    // Wall» in due cartelle «(CD1)» e «(CD2)», ognuno con la sua chiave, e un
    // brano di catalogo che il percorso non ce l'ha. Alla prima ricostruzione
    // intera le chiavi si rifanno dalla regola di adesso, e il disco è uno.
    let mut connection = libreria();
    let percorsi = [
        (1, "C:/M/Pink Floyd/The Wall (CD1)/01.flac"),
        (2, "C:/M/Pink Floyd/The Wall (CD1)/02.flac"),
        (3, "C:/M/Pink Floyd/The Wall (CD2)/01.flac"),
        (4, "C:/M/Pink Floyd/The Wall (CD2)/02.flac"),
    ];
    for (id, path) in percorsi {
        let cartella = if id <= 2 { "(cd1)" } else { "(cd2)" };
        su_file(
            &connection,
            id,
            path,
            "The Wall",
            &format!("the wall c:/m/pink floyd/the wall {cartella}"),
        );
    }
    let indirizzo = "https://archive.org/download/concerto/01.mp3";
    connection
        .execute(
            "INSERT INTO tracks
               (id, path, track_key, title, artist, album, album_key, duration_ms,
                date_added, source, source_service, fonte_url, licenza, disponibilita)
             VALUES (5, NULL, 'k5', 'Dal vivo', 'Qualcuno', 'Concerto', 'vecchia', 1000,
                     1, 'catalogo', 'archive', ?1, 'pubblicoDominio', 'soloAscolto')",
            [indirizzo],
        )
        .expect("brano di catalogo");
    connection
        .execute("UPDATE libreria_stato SET aggregati = 2 WHERE id = 1", [])
        .expect("versione di prima");

    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    rebuild_aggregates(&tx).expect("ricostruzione");
    tx.commit().expect("commit");

    let disco = album_group_key("The Wall", "C:/M/Pink Floyd/The Wall/01.flac");
    assert_eq!(
        chiavi(&connection),
        vec![
            (1, disco.clone()),
            (2, disco.clone()),
            (3, disco.clone()),
            (4, disco.clone()),
            (5, album_group_key("Concerto", indirizzo)),
        ]
    );
    let album: i64 = connection
        .query_row(
            "SELECT total_tracks FROM albums WHERE album_key = ?1",
            [&disco],
            |r| r.get(0),
        )
        .expect("un album solo");
    assert_eq!(album, 4);
    assert_eq!(versione(&connection), VERSIONE_AGGREGATI);
}

#[test]
fn una_libreria_appena_nata_non_rifa_le_chiavi() {
    // Lo zero è «non si sa»: un database nuovo, o «Rileggi tutti i tag». Le
    // chiavi le scrive chi scrive i brani, e la ricostruzione le prende come
    // sono.
    let mut connection = libreria();
    su_file(
        &connection,
        1,
        "C:/M/A/01.flac",
        "Disco",
        "una chiave qualunque",
    );
    assert_eq!(versione(&connection), 0);
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    rebuild_aggregates(&tx).expect("ricostruzione");
    tx.commit().expect("commit");
    assert_eq!(
        chiavi(&connection),
        vec![(1, "una chiave qualunque".to_owned())]
    );
}

/// Le funzioni che scrivono titolo, artista o album di un brano, e come le
/// loro chiavi d'ordine (migrazione 042) tornano in pari.
///
/// Nessuna le scrive da sé: un brano nuovo nasce con le chiavi a NULL, il
/// trigger `chiavi_d_ordine_da_rifare` le rimette a NULL quando un nome cambia,
/// e [`riempi_le_chiavi_d_ordine`] le rifà in `Toccati::segna_brani` (che ogni
/// transazione di scrittura dei brani chiama prima di chiudersi), in testa a
/// ogni ricostruzione degli aggregati e all'apertura dopo una migrazione.
/// Quel che conta, per ognuna, è che nella sua transazione passi una delle
/// due.
const CHI_SCRIVE_I_NOMI: &[(&str, &str, &str)] = &[
    (
        "src/catalogo.rs",
        "aggiungi",
        "rebuild_aggregates nella stessa transazione",
    ),
    (
        "src/enrich.rs",
        "aggiorna_brano",
        "dentro registra: la passata segna i brani (segna_brani) nella stessa transazione",
    ),
    (
        "src/enrich.rs",
        "rimetti_i_tag_del_file",
        "registra_dimenticati: segna_brani e ricostruisci_aggregati nella stessa transazione",
    ),
    (
        "src/incerti.rs",
        "applica_correzioni",
        "correggi: segna_brani e ricostruisci_aggregati; riapplica: dentro la transazione della scansione",
    ),
    (
        "src/library/righe.rs",
        "insert_track",
        "la transazione del lotto segna i brani (segna_brani) prima di chiudersi",
    ),
    (
        "src/library/righe.rs",
        "update_track",
        "la transazione del lotto segna i brani (segna_brani) prima di chiudersi",
    ),
    (
        "src/specchio/archivio.rs",
        "aggiorna",
        "applica_secchio segna i brani (segna_brani) prima e dopo, e Giro::chiudi ricostruisce gli aggregati",
    ),
    (
        "src/specchio/archivio.rs",
        "inserisci",
        "applica_secchio segna i brani (segna_brani) prima e dopo, e Giro::chiudi ricostruisce gli aggregati",
    ),
];

/// Dove cominciano le prove di un file: il primo `#[cfg(test)]` in prima
/// colonna seguito, dopo i suoi attributi e commenti, da un `mod`.
fn inizio_delle_prove(sorgente: &str) -> usize {
    sorgente
        .match_indices("\n#[cfg(test)]")
        .find(|(dove, segno)| {
            sorgente[dove + segno.len()..]
                .lines()
                .map(str::trim_start)
                .find(|riga| {
                    !riga.is_empty()
                        && !riga.starts_with("//")
                        && !riga.starts_with("#[")
                        && !riga.starts_with(')')
                        && !riga.starts_with("clippy::")
                        && !riga.starts_with("reason")
                })
                .is_some_and(|riga| riga.starts_with("mod ") || riga.starts_with("pub mod "))
        })
        .map_or(sorgente.len(), |(dove, _)| dove)
}

/// Le funzioni del codice (non delle prove) con un'istruzione che inserisce un
/// brano o ne cambia titolo, artista o album.
fn trova_chi_scrive_i_nomi() -> Vec<(String, String)> {
    fn visita(cartella: &std::path::Path, file: &mut Vec<std::path::PathBuf>) {
        for voce in std::fs::read_dir(cartella).expect("cartella dei sorgenti") {
            let percorso = voce.expect("voce").path();
            if percorso.is_dir() {
                visita(&percorso, file);
            } else if percorso.extension().is_some_and(|e| e == "rs") {
                file.push(percorso);
            }
        }
    }
    /// `nome =` come parola intera: `album_artist =` non è `artist =`.
    fn assegna(insieme: &str, nome: &str) -> bool {
        insieme
            .match_indices(&format!("{nome} ="))
            .any(|(dove, _)| {
                insieme[..dove]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
            })
    }
    let radice = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut file = Vec::new();
    visita(&radice.join("src"), &mut file);
    let mut trovati = Vec::new();
    for percorso in file {
        let nome = percorso.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if nome == "tests.rs" || nome.starts_with("prove") {
            continue;
        }
        let sorgente = std::fs::read_to_string(&percorso).expect("sorgente");
        let codice = &sorgente[..inizio_delle_prove(&sorgente)];
        for segno in [
            "INSERT INTO tracks",
            "INSERT OR IGNORE INTO tracks",
            "INSERT OR REPLACE INTO tracks",
            "UPDATE tracks",
        ] {
            for (dove, _) in codice.match_indices(segno) {
                let istruzione = &codice[dove..];
                let istruzione = &istruzione[..istruzione.find('"').unwrap_or(istruzione.len())];
                if segno == "UPDATE tracks" {
                    let insieme = istruzione.split("WHERE").next().unwrap_or("");
                    if !["title", "artist", "album"]
                        .iter()
                        .any(|campo| assegna(insieme, campo))
                    {
                        continue;
                    }
                }
                let funzione = codice[..dove]
                    .rfind("fn ")
                    .map(|inizio| {
                        codice[inizio + 3..]
                            .chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                let relativo = percorso
                    .strip_prefix(radice)
                    .expect("dentro il crate")
                    .to_string_lossy()
                    .replace('\\', "/");
                trovati.push((relativo, funzione));
            }
        }
    }
    trovati.sort();
    trovati.dedup();
    trovati
}

#[test]
fn chi_scrive_i_nomi_e_seguito_da_una_ricostruzione() {
    // Se compare una strada nuova che scrive i nomi dei brani, questa prova
    // lo dice: va verificato che dopo arrivi una ricostruzione degli aggregati
    // (o che le chiavi a NULL possano aspettare la prossima apertura), e la
    // strada va scritta qui sopra col suo perché.
    let attesi: Vec<(String, String)> = CHI_SCRIVE_I_NOMI
        .iter()
        .map(|(file, funzione, _)| ((*file).to_owned(), (*funzione).to_owned()))
        .collect();
    assert_eq!(trova_chi_scrive_i_nomi(), attesi);
}

#[test]
fn le_chiavi_d_ordine_si_rifanno_alla_ricostruzione() {
    // Un brano scritto senza chiavi — com'è ogni inserimento — le riceve dalla
    // ricostruzione; un nome cambiato da SQL le perde, e le riavrà alla
    // prossima.
    let mut connection = libreria();
    su_file(&connection, 1, "C:/M/A/01.flac", "L'Été", "chiave");
    let chiavi_d_ordine = |c: &Connection| -> (Option<String>, Option<String>, Option<String>) {
        c.query_row(
            "SELECT ordine_titolo, ordine_artista, ordine_album FROM tracks WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("chiavi")
    };
    assert_eq!(chiavi_d_ordine(&connection), (None, None, None));
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    assert_eq!(riempi_le_chiavi_d_ordine(&tx).expect("riempite"), 1);
    assert_eq!(
        riempi_le_chiavi_d_ordine(&tx).expect("niente da fare"),
        0,
        "una seconda passata non riscrive niente"
    );
    tx.commit().expect("commit");
    assert_eq!(
        chiavi_d_ordine(&connection),
        (
            Some("brano".to_owned()),
            Some("pink floyd".to_owned()),
            Some("ete".to_owned())
        )
    );
}

#[test]
fn segnare_i_brani_rifa_le_chiavi_nella_stessa_transazione() {
    // Una scansione lunga chiude un lotto alla volta e ricostruisce gli
    // aggregati alla fine: senza questo, i brani dei primi lotti stavano in
    // cima ai Brani per nome per tutta la scansione. Ogni transazione che
    // scrive brani li segna prima di chiudersi, ed è lì che le chiavi si
    // rifanno.
    let mut connection = libreria();
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    su_file(&tx, 7, "C:/M/B/01.flac", "Les Misérables", "chiave");
    let mut toccati = Toccati::nessuno();
    toccati.segna_brani(&tx, &[7]).expect("segnati");
    let chiave: Option<String> = tx
        .query_row("SELECT ordine_album FROM tracks WHERE id = 7", [], |r| {
            r.get(0)
        })
        .expect("chiave");
    assert_eq!(chiave.as_deref(), Some("miserables"));
    tx.commit().expect("commit");
}
