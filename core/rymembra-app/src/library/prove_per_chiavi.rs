//! Le prove per chiavi: librerie sintetiche scritte direttamente in tabella.

use super::aggregati::{INTERE_IN_QUESTO_FILO, TETTO_PER_CHIAVI, ricostruisci_gli_artisti};
use super::interrogazioni::sql_dei_recenti;
use super::*;

/// Un generatore a caso con il seme in mano: una prova che fallisce deve
/// fallire di nuovo uguale, e il seme sta scritto nel messaggio.
///
/// Il secondo campo decide se gli identificativi esterni si ripetono fra album
/// diversi: tanto nelle prove di parità, dove le fusioni sono il caso da
/// mettere alla frusta, e come in una libreria vera nelle misure, dove un
/// gruppo di pubblicazione è di un disco e delle sue edizioni.
struct Dado(u64, bool);

impl Dado {
    fn nuovo(seme: u64) -> Self {
        Self(seme.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, true)
    }

    /// Gli identificativi come in una libreria taggata con Picard: uno per
    /// disco, condiviso solo dalle sue edizioni.
    fn come_una_libreria_vera(mut self) -> Self {
        self.1 = false;
        self
    }

    /// xorshift64*: abbastanza a caso per scegliere una modifica.
    fn tira(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn fino_a(&mut self, n: usize) -> usize {
        let n = u64::try_from(n.max(1)).expect("un numero piccolo");
        usize::try_from(self.tira() % n).expect("un numero piccolo")
    }

    fn moneta(&mut self, su_cento: u64) -> bool {
        self.tira() % 100 < su_cento
    }
}

/// Le copertine che le righe sintetiche possono nominare, con provenienze e
/// misure diverse: la scelta della copertina d'album le guarda tutte.
const COPERTINE: &[(&str, &str, i64)] = &[
    ("c0", "tag", 600),
    ("c1", "tag", 300),
    ("c2", "provider", 1200),
    ("c3", "caa", 500),
    ("c4", "spotify", 640),
];

fn database() -> Connection {
    let connection = crate::db::open_in_memory().expect("database").connection;
    for (hash, fonte, lato) in COPERTINE {
        connection
                .execute(
                    "INSERT INTO cover_art (hash, mime_type, width, height, byte_size, source, created_at)
                     VALUES (?1, 'image/jpeg', ?2, ?2, 1000, ?3, 0)",
                    rusqlite::params![hash, lato, fonte],
                )
                .expect("copertina");
    }
    connection
}

/// Una riga di brano, con tutto quel che gli aggregati guardano.
#[derive(Debug, Clone)]
struct Riga {
    path: String,
    title: String,
    artist: String,
    album: String,
    album_artist: Option<String>,
    year: Option<i64>,
    genre: Option<String>,
    cover: Option<&'static str>,
    mbrg: Option<String>,
    mbr: Option<String>,
    sp: Option<String>,
}

impl Riga {
    /// La chiave di base, come la scrive la scansione.
    fn chiave(&self) -> String {
        album_group_key(&self.album, &self.path)
    }
}

fn inserisci(connection: &Connection, riga: &Riga) -> i64 {
    connection
        .execute(
            "INSERT INTO tracks (path, track_key, title, artist, album, album_artist,
                                     album_key, year, genre, duration_ms, cover_art_hash,
                                     file_size, date_added, date_modified, content_key,
                                     mb_release_group_id, mb_release_id, spotify_album_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 200000, ?10,
                         1000000, 1, 1, ?2, ?11, ?12, ?13)",
            rusqlite::params![
                riga.path,
                format!("{}|{}", riga.artist, riga.title),
                riga.title,
                riga.artist,
                riga.album,
                riga.album_artist,
                riga.chiave(),
                riga.year,
                riga.genre,
                riga.cover,
                riga.mbrg,
                riga.mbr,
                riga.sp,
            ],
        )
        .expect("brano sintetico");
    connection.last_insert_rowid()
}

/// Riscrive una riga come la riscrive la scansione: i campi dal «file» e la
/// chiave d'album di base, che la ricostruzione poi rimappa.
fn riscrivi(connection: &Connection, id: i64, riga: &Riga) {
    connection
        .execute(
            "UPDATE tracks SET path = ?2, title = ?3, artist = ?4, album = ?5,
                        album_artist = ?6, album_key = ?7, year = ?8, genre = ?9,
                        cover_art_hash = ?10, mb_release_group_id = ?11,
                        mb_release_id = ?12, spotify_album_id = ?13
                  WHERE id = ?1",
            rusqlite::params![
                id,
                riga.path,
                riga.title,
                riga.artist,
                riga.album,
                riga.album_artist,
                riga.chiave(),
                riga.year,
                riga.genre,
                riga.cover,
                riga.mbrg,
                riga.mbr,
                riga.sp,
            ],
        )
        .expect("riscrittura");
}

fn leggi(connection: &Connection, id: i64) -> Riga {
    connection
        .query_row(
            "SELECT path, title, artist, album, album_artist, year, genre, cover_art_hash,
                        mb_release_group_id, mb_release_id, spotify_album_id
                   FROM tracks WHERE id = ?1",
            [id],
            |r| {
                let cover: Option<String> = r.get(7)?;
                Ok(Riga {
                    path: r.get(0)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    album_artist: r.get(4)?,
                    year: r.get(5)?,
                    genre: r.get(6)?,
                    cover: cover.and_then(|c| {
                        COPERTINE
                            .iter()
                            .find(|(h, _, _)| *h == c)
                            .map(|(h, _, _)| *h)
                    }),
                    mbrg: r.get(8)?,
                    mbr: r.get(9)?,
                    sp: r.get(10)?,
                })
            },
        )
        .expect("riga")
}

const GENERI: &[&str] = &["Rock", "Jazz", "Alternative Rock", "Elettronica"];

/// Un brano a caso dentro l'album `album` dell'artista `artista`.
fn brano_a_caso(dado: &mut Dado, artista: usize, album: usize, traccia: usize) -> Riga {
    let edizione = dado.moneta(15);
    let cartella = if edizione {
        format!(r"C:\Musica\Artista {artista}\Album {album} (Deluxe)")
    } else {
        format!(r"C:\Musica\Artista {artista}\Album {album}")
    };
    // Un disco su cinque è su due dischi, in sottocartelle `CD1`/`CD2`.
    let disco = if album.is_multiple_of(5) {
        format!(r"\CD{}", 1 + dado.fino_a(2))
    } else {
        String::new()
    };
    Riga {
        path: format!(
            r"{cartella}{disco}\{traccia:02} - Brano {}.flac",
            dado.tira()
        ),
        title: format!("Brano {traccia}"),
        artist: if dado.moneta(10) {
            format!("Artista {artista} feat. Ospite {}", dado.fino_a(5))
        } else {
            format!("Artista {artista}")
        },
        album: format!("Album {album}"),
        album_artist: match dado.fino_a(6) {
            0 => Some(format!("Artista {artista}")),
            1 => Some(String::new()),
            2 => Some("  ".to_owned()),
            3 => Some(format!(" Artista {artista} ")),
            _ => None,
        },
        year: dado
            .moneta(80)
            .then(|| 1970 + i64::try_from(album % 50).unwrap_or(0)),
        genre: dado
            .moneta(70)
            .then(|| GENERI[dado.fino_a(GENERI.len())].to_owned()),
        cover: dado
            .moneta(75)
            .then(|| COPERTINE[dado.fino_a(COPERTINE.len())].0),
        // Gli identificativi per album e non per brano, come li scrive
        // Picard: due edizioni nella stessa cartella e due cartelle con lo
        // stesso gruppo di pubblicazione si devono fondere.
        mbrg: dado
            .moneta(40)
            .then(|| format!("rg-{}", if dado.1 { album % 37 } else { album })),
        mbr: dado.moneta(20).then(|| format!("r-{album}")),
        sp: dado
            .moneta(10)
            .then(|| format!("sp-{}", if dado.1 { album % 23 } else { album })),
    }
}

/// Le tre tabelle che gli aggregati scrivono, in una forma confrontabile.
#[derive(Debug, PartialEq, Eq)]
struct Fotografia {
    albums: Vec<String>,
    artists: Vec<String>,
    chiavi: Vec<(i64, Option<String>)>,
}

fn fotografia(connection: &Connection) -> Fotografia {
    let albums = connection
        .prepare(
            "SELECT album_key || '¦' || title || '¦' || artist || '¦' || COALESCE(year, '-')
                        || '¦' || COALESCE(genre, '-') || '¦' || total_tracks || '¦'
                        || COALESCE(cover_art_hash, '-') || '¦' || COALESCE(mb_album_id, '-')
                        || '¦' || COALESCE(spotify_id, '-')
                   FROM albums ORDER BY album_key",
        )
        .expect("album")
        .query_map([], |r| r.get(0))
        .expect("album")
        .collect::<Result<Vec<String>, _>>()
        .expect("album");
    let artists = connection
        .prepare("SELECT name FROM artists ORDER BY name")
        .expect("artisti")
        .query_map([], |r| r.get(0))
        .expect("artisti")
        .collect::<Result<Vec<String>, _>>()
        .expect("artisti");
    let chiavi = connection
        .prepare("SELECT id, album_key FROM tracks ORDER BY id")
        .expect("chiavi")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("chiavi")
        .collect::<Result<Vec<_>, _>>()
        .expect("chiavi");
    Fotografia {
        albums,
        artists,
        chiavi,
    }
}

/// Le righe che stanno in una fotografia e non nell'altra, per un messaggio
/// che si legga invece di due elenchi da mille righe.
fn differenze(per_chiavi: &Fotografia, intera: &Fotografia) -> String {
    fn solo<T: PartialEq + std::fmt::Debug>(a: &[T], b: &[T]) -> Vec<String> {
        a.iter()
            .filter(|x| !b.contains(x))
            .map(|x| format!("{x:?}"))
            .collect()
    }
    format!(
        "album solo per chiavi: {:?}\nalbum solo per intero: {:?}\n\
             artisti solo per chiavi: {:?}\nartisti solo per intero: {:?}\n\
             chiavi solo per chiavi: {:?}\nchiavi solo per intero: {:?}",
        solo(&per_chiavi.albums, &intera.albums),
        solo(&intera.albums, &per_chiavi.albums),
        solo(&per_chiavi.artists, &intera.artists),
        solo(&intera.artists, &per_chiavi.artists),
        solo(&per_chiavi.chiavi, &intera.chiavi),
        solo(&intera.chiavi, &per_chiavi.chiavi),
    )
}

/// Quel che darebbe la ricostruzione intera **adesso**, senza tenerla: la
/// transazione cade alla fine e il database resta com'era.
fn come_la_intera(connection: &mut Connection) -> Fotografia {
    let tx = connection.transaction().expect("transazione");
    rebuild_aggregates(&tx).expect("ricostruzione intera");
    let foto = fotografia(&tx);
    drop(tx);
    foto
}

/// Una libreria sintetica di circa `brani` righe, già ricostruita per
/// intero: è lo stato da cui parte ogni scansione vera.
fn libreria(connection: &mut Connection, dado: &mut Dado, brani: usize) -> Vec<i64> {
    let tx = connection.transaction().expect("transazione");
    let mut ids = Vec::with_capacity(brani);
    let mut album = 0;
    while ids.len() < brani {
        album += 1;
        let artista = album % 97;
        for traccia in 1..=(6 + dado.fino_a(10)) {
            ids.push(inserisci(&tx, &brano_a_caso(dado, artista, album, traccia)));
        }
    }
    rebuild_aggregates(&tx).expect("ricostruzione");
    tx.commit().expect("commit");
    ids
}

/// Una modifica a caso, come la farebbe una scansione o un'importazione,
/// con le chiavi segnate prima e dopo. Restituisce le chiavi toccate.
fn modifica_a_caso(
    tx: &Transaction<'_>,
    dado: &mut Dado,
    vivi: &mut Vec<i64>,
    prossimo_album: &mut usize,
) -> Toccati {
    let mut toccati = Toccati::nessuno();
    let quale = |dado: &mut Dado, vivi: &[i64]| vivi[dado.fino_a(vivi.len())];
    match dado.fino_a(9) {
        // Un brano nuovo in un album che c'è.
        0 if !vivi.is_empty() => {
            let accanto = leggi(tx, quale(dado, vivi));
            let mut riga = brano_a_caso(dado, 1, 1, 99);
            riga.path = format!(
                r"{}\99 - Nuovo {}.flac",
                &accanto.path[..accanto.path.rfind('\\').unwrap_or(0)],
                dado.tira()
            );
            riga.album = accanto.album.clone();
            riga.artist = accanto.artist.clone();
            riga.mbrg = accanto.mbrg.clone();
            let id = inserisci(tx, &riga);
            toccati.segna_brani(tx, &[id]).expect("dopo");
            vivi.push(id);
        }
        // Un album nuovo che porta l'identificativo di un altro: le due
        // cartelle si devono fondere.
        1 => {
            *prossimo_album += 1;
            let mut gemello = None;
            if !vivi.is_empty() {
                gemello = leggi(tx, quale(dado, vivi)).mbrg;
            }
            for traccia in 1..=(1 + dado.fino_a(4)) {
                let mut riga = brano_a_caso(dado, 7, *prossimo_album, traccia);
                if gemello.is_some() {
                    riga.mbrg.clone_from(&gemello);
                }
                let id = inserisci(tx, &riga);
                toccati.segna_brani(tx, &[id]).expect("dopo");
                vivi.push(id);
            }
        }
        // Ritaggato: cambia l'album.
        2 if !vivi.is_empty() => {
            let id = quale(dado, vivi);
            toccati.segna_brani(tx, &[id]).expect("prima");
            let mut riga = leggi(tx, id);
            riga.album = format!("Album {}", dado.fino_a(400));
            riga.mbrg = dado.moneta(50).then(|| format!("rg-{}", dado.fino_a(37)));
            riscrivi(tx, id, &riga);
            toccati.segna_brani(tx, &[id]).expect("dopo");
        }
        // Spostato in un'altra cartella.
        3 if !vivi.is_empty() => {
            let id = quale(dado, vivi);
            toccati.segna_brani(tx, &[id]).expect("prima");
            let mut riga = leggi(tx, id);
            riga.path = format!(
                r"C:\Musica\Spostati {}\{} {}.flac",
                dado.fino_a(30),
                riga.title,
                dado.tira()
            );
            riscrivi(tx, id, &riga);
            toccati.segna_brani(tx, &[id]).expect("dopo");
        }
        // Cambia l'artista, o l'artista dell'album.
        4 if !vivi.is_empty() => {
            let id = quale(dado, vivi);
            toccati.segna_brani(tx, &[id]).expect("prima");
            let mut riga = leggi(tx, id);
            if dado.moneta(50) {
                riga.artist = format!("Artista {}", dado.fino_a(120));
            } else {
                riga.album_artist = match dado.fino_a(3) {
                    0 => None,
                    1 => Some(String::new()),
                    _ => Some(format!("Artista {}", dado.fino_a(120))),
                };
            }
            riscrivi(tx, id, &riga);
            toccati.segna_brani(tx, &[id]).expect("dopo");
        }
        // Cambiano gli identificativi, **senza** rimettere la chiave di
        // base: è quel che lascerebbe un'importazione che scrive solo le
        // colonne esterne. La chiusura deve accorgersene lo stesso.
        5 if !vivi.is_empty() => {
            let id = quale(dado, vivi);
            toccati.segna_brani(tx, &[id]).expect("prima");
            tx.execute(
                "UPDATE tracks SET mb_release_group_id = ?2, spotify_album_id = ?3
                      WHERE id = ?1",
                rusqlite::params![
                    id,
                    dado.moneta(70).then(|| format!("rg-{}", dado.fino_a(37))),
                    dado.moneta(20).then(|| format!("sp-{}", dado.fino_a(23))),
                ],
            )
            .expect("identificativi");
            toccati.segna_brani(tx, &[id]).expect("dopo");
        }
        // Cambia la copertina, o il genere, o l'anno.
        6 if !vivi.is_empty() => {
            let id = quale(dado, vivi);
            toccati.segna_brani(tx, &[id]).expect("prima");
            let mut riga = leggi(tx, id);
            riga.cover = dado
                .moneta(70)
                .then(|| COPERTINE[dado.fino_a(COPERTINE.len())].0);
            riga.genre = dado
                .moneta(70)
                .then(|| GENERI[dado.fino_a(GENERI.len())].to_owned());
            riga.year = dado
                .moneta(70)
                .then(|| 1960 + i64::try_from(dado.fino_a(60)).unwrap_or(0));
            riscrivi(tx, id, &riga);
            toccati.segna_brani(tx, &[id]).expect("dopo");
        }
        // Un album intero se ne va.
        7 if !vivi.is_empty() => {
            let chiave: Option<String> = tx
                .query_row(
                    "SELECT album_key FROM tracks WHERE id = ?1",
                    [quale(dado, vivi)],
                    |r| r.get(0),
                )
                .expect("chiave");
            let ids: Vec<i64> = tx
                .prepare("SELECT id FROM tracks WHERE album_key IS ?1")
                .expect("album")
                .query_map([chiave], |r| r.get(0))
                .expect("album")
                .collect::<Result<_, _>>()
                .expect("album");
            toccati.segna_brani(tx, &ids).expect("prima");
            for id in &ids {
                tx.execute("DELETE FROM tracks WHERE id = ?1", [id])
                    .expect("cancellazione");
            }
            vivi.retain(|id| !ids.contains(id));
        }
        // Un brano solo se ne va.
        _ if !vivi.is_empty() => {
            let posto = dado.fino_a(vivi.len());
            let id = vivi.swap_remove(posto);
            toccati.segna_brani(tx, &[id]).expect("prima");
            tx.execute("DELETE FROM tracks WHERE id = ?1", [id])
                .expect("cancellazione");
        }
        _ => {}
    }
    toccati
}

#[test]
fn la_ricostruzione_intera_rifatta_non_cambia_niente() {
    // Il presupposto di tutto il resto: dopo una ricostruzione intera, una
    // seconda dà le stesse tabelle. Se non fosse vero, «per chiavi» non
    // avrebbe un punto fermo con cui confrontarsi.
    let mut connection = database();
    let mut dado = Dado::nuovo(7);
    libreria(&mut connection, &mut dado, 400);
    let prima = fotografia(&connection);
    assert_eq!(come_la_intera(&mut connection), prima);
}

#[test]
fn per_chiavi_e_per_intero_danno_le_stesse_tabelle() {
    // La prova di parità: una libreria sintetica, centinaia di modifiche a
    // caso — brani nuovi, ritaggati, spostati, album fusi per
    // identificativo, album interi che se ne vanno — e dopo **ognuna** la
    // ricostruzione per chiavi deve lasciare esattamente le tabelle che
    // lascerebbe quella intera.
    for seme in [1_u64, 2, 3, 4] {
        let mut connection = database();
        let mut dado = Dado::nuovo(seme);
        let mut vivi = libreria(&mut connection, &mut dado, 300);
        let mut prossimo_album = 10_000;
        let mut per_chiavi_davvero = 0;
        for passo in 0..150 {
            let tx = transazione_di_scrittura(&mut connection).expect("transazione");
            let toccati = modifica_a_caso(&tx, &mut dado, &mut vivi, &mut prossimo_album);
            assert!(!toccati.e_tutto(), "poche modifiche, niente ripiego");
            let intere_prima = INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get);
            ricostruisci_aggregati(&tx, &toccati).expect("per chiavi");
            if INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get) == intere_prima {
                per_chiavi_davvero += 1;
            }
            tx.commit().expect("commit");
            let per_chiavi = fotografia(&connection);
            let intera = come_la_intera(&mut connection);
            assert!(
                per_chiavi == intera,
                "seme {seme}, passo {passo}: le due ricostruzioni divergono\n{}",
                differenze(&per_chiavi, &intera)
            );
        }
        // E la strada provata dev'essere quella per chiavi: una prova di
        // parità che ripiegasse ogni volta sull'intera confronterebbe
        // l'intera con sé stessa.
        assert!(
            per_chiavi_davvero >= 100,
            "seme {seme}: solo {per_chiavi_davvero} passi su 150 per chiavi"
        );
    }
}

#[test]
fn il_rimappaggio_non_si_incatena() {
    // Il guasto che la prova di parità ha trovato, scritto per esteso. Un
    // disco rimappato la volta prima su `mbrg:A`, i cui brani adesso
    // dicono soprattutto B: diventa `mbrg:B`. Nella stessa passata un
    // brano nuovo, con chiave di base alfabeticamente prima, porta A e
    // diventa `mbrg:A`. Con il rimappaggio coppia per coppia il secondo
    // `UPDATE` ripescava anche lui, e lo portava in `mbrg:B`.
    let mut connection = database();
    let base = |album: &str, path: &str, mbrg: &str| Riga {
        path: path.to_owned(),
        title: "T".to_owned(),
        artist: "X".to_owned(),
        album: album.to_owned(),
        album_artist: None,
        year: None,
        genre: None,
        cover: None,
        mbrg: Some(mbrg.to_owned()),
        mbr: None,
        sp: None,
    };
    let mut disco = Vec::new();
    for (n, mbrg) in ["B", "B", "A"].into_iter().enumerate() {
        let id = inserisci(
            &connection,
            &base("Disco", &format!(r"C:\m\Disco\{n}.flac"), mbrg),
        );
        disco.push(id);
    }
    connection
        .execute("UPDATE tracks SET album_key = 'mbrg:A'", [])
        .expect("rimappati la volta prima");
    let solo = inserisci(&connection, &base("Aaa", r"C:\m\Aaa\1.flac", "A"));
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    rebuild_aggregates(&tx).expect("ricostruzione");
    tx.commit().expect("commit");
    let chiave = |id: i64| -> String {
        connection
            .query_row("SELECT album_key FROM tracks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .expect("chiave")
    };
    assert_eq!(
        chiave(solo),
        "mbrg:A",
        "il brano nuovo è finito nel disco sbagliato"
    );
    for id in disco {
        assert_eq!(chiave(id), "mbrg:B");
    }
}

/// Un brano del disco con cui la libreria vera ha trovato il guasto: tredici
/// brani in una cartella, e solo i primi tre identificati.
fn brano_del_disco(n: usize) -> Riga {
    Riga {
        path: format!(r"\\nas\Musica\Fugees\Greatest Hits\{n:02}.flac"),
        title: format!("Brano {n}"),
        artist: "Fugees".to_owned(),
        album: "Greatest Hits".to_owned(),
        album_artist: None,
        year: None,
        genre: None,
        cover: None,
        mbrg: (n < 3).then(|| "X".to_owned()),
        mbr: None,
        sp: None,
    }
}

/// Il disco in mezzo a una libreria, ricostruito per intero: i tredici brani
/// stanno tutti sotto `mbrg:X`, ed è lo stato da cui la libreria vera è
/// partita. La libreria intorno serve alla strada per chiavi, che rinuncia
/// quando la chiusura supera metà degli album.
fn il_disco(connection: &mut Connection) -> Vec<i64> {
    libreria(connection, &mut Dado::nuovo(31), 200);
    let ids: Vec<i64> = (0..13)
        .map(|n| inserisci(connection, &brano_del_disco(n)))
        .collect();
    let tx = transazione_di_scrittura(connection).expect("transazione");
    rebuild_aggregates(&tx).expect("ricostruzione");
    tx.commit().expect("commit");
    ids
}

/// La scansione rilegge questi brani — un taggatore ne ha riscritto i file
/// — e ognuno torna alla chiave di base. Restituisce le chiavi toccate,
/// segnate prima e dopo come le segna la scansione; gli aggregati non li
/// rifà.
fn riletti(connection: &mut Connection, ids: &[i64]) -> Toccati {
    let tx = transazione_di_scrittura(connection).expect("transazione");
    let mut toccati = Toccati::nessuno();
    toccati.segna_brani(&tx, ids).expect("prima");
    for id in ids {
        let riga = leggi(&tx, *id);
        riscrivi(&tx, *id, &riga);
    }
    toccati.segna_brani(&tx, ids).expect("dopo");
    tx.commit().expect("commit");
    toccati
}

#[test]
fn un_disco_riletto_a_meta_resta_un_disco_solo() {
    // Il guasto della libreria vera, centosettanta volte nel diario. I tre
    // brani identificati tornano alla chiave di base, i dieci senza
    // identificativo restano sotto `mbrg:X`: due gruppi con la stessa
    // chiave canonica, e `INSERT INTO albums` falliva su
    // `UNIQUE constraint failed: albums.album_key` — per intero e per
    // chiavi, a ogni passata, portando con sé tutta la transazione.
    let mut connection = database();
    let ids = il_disco(&mut connection);
    let toccati = riletti(&mut connection, &ids[..3]);

    let intera = come_la_intera(&mut connection);
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    let intere_prima = INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get);
    ricostruisci_aggregati(&tx, &toccati).expect("per chiavi");
    assert_eq!(
        INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get),
        intere_prima,
        "doveva essere per chiavi"
    );
    tx.commit().expect("commit");
    let per_chiavi = fotografia(&connection);
    assert!(
        per_chiavi == intera,
        "le due ricostruzioni divergono\n{}",
        differenze(&per_chiavi, &intera)
    );
    let del_disco: Vec<&String> = per_chiavi
        .albums
        .iter()
        .filter(|album| album.contains("¦Greatest Hits¦"))
        .collect();
    assert_eq!(del_disco.len(), 1, "{del_disco:?}");
    assert!(
        del_disco[0].starts_with("mbrg:X¦Greatest Hits¦Fugees¦-¦-¦13¦"),
        "{del_disco:?}"
    );
    for (id, chiave) in &per_chiavi.chiavi {
        if ids.contains(id) {
            assert_eq!(chiave.as_deref(), Some("mbrg:X"), "brano {id}");
        }
    }
}

#[test]
fn la_chiusura_va_dalla_chiave_canonica_all_identificativo() {
    // Una passata di prima ha scritto i brani e non gli aggregati — la sua
    // ricostruzione è fallita dopo che i brani erano già confermati — e la
    // versione è rimasta quella giusta. I tre brani identificati stanno
    // sotto la chiave di base, i dieci senza identificativo sotto
    // `mbrg:X`. Adesso si tocca uno solo dei dieci: i suoi compagni non
    // portano nessun identificativo, e l'unica strada verso i tre è la
    // chiave stessa, `mbrg:X`, letta come l'identificativo `X`.
    let mut connection = database();
    let ids = il_disco(&mut connection);
    riletti(&mut connection, &ids[..3]);

    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    let mut toccati = Toccati::nessuno();
    toccati.segna_brani(&tx, &ids[5..6]).expect("prima");
    tx.execute(
        "UPDATE tracks SET genre = 'Hip hop' WHERE id = ?1",
        [ids[5]],
    )
    .expect("genere");
    toccati.segna_brani(&tx, &ids[5..6]).expect("dopo");
    assert_eq!(toccati.quanti_album(), 1);
    let intere_prima = INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get);
    ricostruisci_aggregati(&tx, &toccati).expect("per chiavi");
    assert_eq!(
        INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get),
        intere_prima,
        "doveva essere per chiavi"
    );
    tx.commit().expect("commit");
    let per_chiavi = fotografia(&connection);
    let intera = come_la_intera(&mut connection);
    assert!(
        per_chiavi == intera,
        "le due ricostruzioni divergono\n{}",
        differenze(&per_chiavi, &intera)
    );
}

/// La scansione rilegge i brani **identificati** di un album a caso, e solo
/// quelli: tornano alla chiave di base, gli altri restano sotto quella
/// canonica. È il modo in cui la libreria vera ha prodotto il guasto.
fn rilegge_gli_identificati(tx: &Transaction<'_>, dado: &mut Dado, vivi: &[i64]) -> Toccati {
    let mut toccati = Toccati::nessuno();
    if vivi.is_empty() {
        return toccati;
    }
    let chiave: String = tx
        .query_row(
            "SELECT album_key FROM tracks WHERE id = ?1",
            [vivi[dado.fino_a(vivi.len())]],
            |r| r.get(0),
        )
        .expect("chiave");
    let identificati: Vec<i64> = tx
        .prepare(
            "SELECT id FROM tracks WHERE album_key = ?1
                    AND (mb_release_group_id IS NOT NULL OR mb_release_id IS NOT NULL
                         OR spotify_album_id IS NOT NULL)",
        )
        .expect("identificati")
        .query_map([chiave], |r| r.get(0))
        .expect("identificati")
        .collect::<Result<_, _>>()
        .expect("identificati");
    toccati.segna_brani(tx, &identificati).expect("prima");
    for id in &identificati {
        let riga = leggi(tx, *id);
        riscrivi(tx, *id, &riga);
    }
    toccati.segna_brani(tx, &identificati).expect("dopo");
    toccati
}

#[test]
fn dischi_riletti_a_pezzi_chiavi_uniche_e_parita() {
    // La proprietà, a caso: dopo ogni passo — metà delle volte un album di
    // cui si rileggono i soli brani identificati, l'altra metà una delle
    // modifiche della prova di parità — la ricostruzione intera riesce, e
    // riuscire vuol dire chiavi d'album uniche, perché `albums.album_key` è
    // la chiave primaria; e quella per chiavi lascia le stesse tabelle.
    for seme in [21_u64, 22, 23, 24] {
        let mut connection = database();
        let mut dado = Dado::nuovo(seme);
        let mut vivi = libreria(&mut connection, &mut dado, 300);
        let mut prossimo_album = 10_000;
        let mut per_chiavi_davvero = 0;
        for passo in 0..80 {
            let tx = transazione_di_scrittura(&mut connection).expect("transazione");
            let toccati = if dado.moneta(50) {
                rilegge_gli_identificati(&tx, &mut dado, &vivi)
            } else {
                modifica_a_caso(&tx, &mut dado, &mut vivi, &mut prossimo_album)
            };
            let intere_prima = INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get);
            if let Err(err) = ricostruisci_aggregati(&tx, &toccati) {
                panic!("seme {seme}, passo {passo}: per chiavi non riesce: {err:?}");
            }
            if INTERE_IN_QUESTO_FILO.with(std::cell::Cell::get) == intere_prima {
                per_chiavi_davvero += 1;
            }
            tx.commit().expect("commit");
            let per_chiavi = fotografia(&connection);
            let tx = connection.transaction().expect("transazione");
            if let Err(err) = rebuild_aggregates(&tx) {
                panic!("seme {seme}, passo {passo}: per intero non riesce: {err:?}");
            }
            let intera = fotografia(&tx);
            drop(tx);
            assert!(
                per_chiavi == intera,
                "seme {seme}, passo {passo}: le due ricostruzioni divergono\n{}",
                differenze(&per_chiavi, &intera)
            );
        }
        assert!(
            per_chiavi_davvero >= 50,
            "seme {seme}: solo {per_chiavi_davvero} passi su 80 per chiavi"
        );
    }
}

#[test]
fn senza_la_versione_giusta_si_rifa_tutto() {
    // Una libreria scritta da una regola di prima: un album fantasma che
    // nessun brano nomina. Le chiavi toccate non lo nominano, quindi solo
    // una ricostruzione intera lo può togliere — ed è quella che parte,
    // perché la versione degli aggregati non è quella di adesso.
    let mut connection = database();
    let mut dado = Dado::nuovo(11);
    let ids = libreria(&mut connection, &mut dado, 60);
    connection
        .execute_batch(
            "INSERT INTO albums (album_key, title, artist, total_tracks)
                 VALUES ('fantasma', 'Fantasma', 'Nessuno', 3);
                 UPDATE libreria_stato SET aggregati = 0;",
        )
        .expect("fantasma");
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    let mut toccati = Toccati::nessuno();
    toccati.segna_brani(&tx, &ids[..1]).expect("segnati");
    ricostruisci_aggregati(&tx, &toccati).expect("ricostruzione");
    tx.commit().expect("commit");
    let fantasmi: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM albums WHERE album_key = 'fantasma'",
            [],
            |r| r.get(0),
        )
        .expect("conteggio");
    assert_eq!(fantasmi, 0, "la ricostruzione doveva essere intera");
    assert!(aggregati_in_regola(&connection).expect("versione"));
}

#[test]
fn rileggere_tutti_i_tag_chiede_la_ricostruzione_intera() {
    let mut connection = database();
    let mut dado = Dado::nuovo(12);
    libreria(&mut connection, &mut dado, 30);
    assert!(aggregati_in_regola(&connection).expect("versione"));
    rileggi_tutti_i_tag(&connection).expect("rilettura");
    assert!(
        !aggregati_in_regola(&connection).expect("versione"),
        "dopo «Rileggi tutti i tag» la prossima ricostruzione dev'essere intera"
    );
}

#[test]
fn niente_toccato_niente_riscritto() {
    // Una scansione che non ha cambiato niente non scrive niente: né gli
    // album né, quindi, la generazione della libreria.
    let mut connection = database();
    let mut dado = Dado::nuovo(13);
    libreria(&mut connection, &mut dado, 50);
    let prima = generazione(&connection).expect("generazione");
    let foto = fotografia(&connection);
    let tx = transazione_di_scrittura(&mut connection).expect("transazione");
    let aggregati = ricostruisci_aggregati(&tx, &Toccati::nessuno()).expect("niente");
    tx.commit().expect("commit");
    assert_eq!(generazione(&connection).expect("generazione"), prima);
    assert_eq!(fotografia(&connection), foto);
    assert_eq!(aggregati.albums, foto.albums.len());
    assert_eq!(aggregati.artists, foto.artists.len());
}

#[test]
fn troppe_chiavi_e_si_rinuncia_a_contare() {
    let mut connection = database();
    let mut dado = Dado::nuovo(14);
    let ids = libreria(&mut connection, &mut dado, 300);
    // Si abbassa il tetto a mano non si può: è una costante. Si segnano
    // invece due insiemi uniti, e si guarda che l'unione rispetti il tetto.
    let mut tanti = Toccati::nessuno();
    tanti.segna_brani(&connection, &ids).expect("segnati");
    assert!(!tanti.e_tutto());
    assert!(tanti.quanti_album() <= TETTO_PER_CHIAVI);
    let mut tutto = Toccati::tutti();
    tutto.unisci(tanti.clone());
    assert!(tutto.e_tutto());
    let mut ancora = tanti;
    ancora.unisci(Toccati::nessuno());
    assert!(!ancora.e_tutto());
}

#[test]
fn la_generazione_cambia_a_ogni_scrittura_dei_brani() {
    let mut connection = database();
    let mut dado = Dado::nuovo(15);
    let ids = libreria(&mut connection, &mut dado, 10);
    let mut viste = vec![generazione(&connection).expect("generazione")];
    let mut dopo = |connection: &Connection| {
        let adesso = generazione(connection).expect("generazione");
        assert!(
            !viste.contains(&adesso),
            "la generazione è tornata a un valore già visto"
        );
        viste.push(adesso);
    };
    connection
        .execute(
            "UPDATE tracks SET play_count = play_count + 1 WHERE id = ?1",
            [ids[0]],
        )
        .expect("ascolto");
    dopo(&connection);
    let id = inserisci(&connection, &brano_a_caso(&mut dado, 1, 1, 1));
    dopo(&connection);
    connection
        .execute("DELETE FROM tracks WHERE id = ?1", [id])
        .expect("cancellazione");
    dopo(&connection);
    // Una scrittura che non tocca `tracks` non la cambia.
    let ferma = generazione(&connection).expect("generazione");
    connection
        .execute(
            "INSERT INTO settings (key, value) VALUES ('prova', '1')",
            [],
        )
        .expect("impostazione");
    assert_eq!(generazione(&connection).expect("generazione"), ferma);
}

/// Il piano di SQLite per una query, una riga per passo.
fn piano(connection: &Connection, sql: &str) -> String {
    let mut statement = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("piano");
    statement
        .query_map([], |r| r.get::<_, String>(3))
        .expect("righe del piano")
        .collect::<Result<Vec<_>, _>>()
        .expect("piano letto")
        .join(" | ")
}

#[test]
fn il_ripiano_dei_recenti_passa_dall_indice() {
    // Il ripiano della Home si rifà a ogni cambio di brano: ordinato per
    // un'espressione (`last_played_at IS NULL`) era una lettura e un
    // ordinamento di tutta la libreria, con l'indice parziale lì accanto.
    let connection = database();
    let piano: Vec<String> = {
        let mut s = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {}", sql_dei_recenti()))
            .expect("piano");
        let righe = s
            .query_map([12_i64], |r| r.get::<_, String>(3))
            .expect("piano");
        righe.collect::<Result<_, _>>().expect("righe del piano")
    };
    assert!(
        piano.iter().any(|r| r.contains("idx_tracks_last_played")),
        "{piano:?}"
    );
    assert!(!piano.iter().any(|r| r.starts_with("SCAN")), "{piano:?}");

    // E quel che restituisce: gli ascoltati, dal più recente; i mai
    // ascoltati no.
    for (id, quando, volte) in [
        (1_i64, Some(100_i64), 2_i64),
        (2, Some(300), 1),
        (3, None, 0),
    ] {
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, duration_ms,
                                         file_size, date_added, date_modified, last_played_at,
                                         play_count)
                     VALUES (?1, ?2, ?3, 'T', 'A', 'Al', 1000, 1, 1, 1, ?4, ?5)",
                rusqlite::params![id, format!("r{id}.mp3"), format!("kr{id}"), quando, volte],
            )
            .expect("brano");
    }
    let recenti = ascoltati_di_recente(&connection, 12).expect("recenti");
    assert_eq!(recenti.iter().map(|b| b.id).collect::<Vec<_>>(), [2, 1]);
}

#[test]
fn i_percorsi_piegati_passano_dall_indice() {
    // La domanda di `ids_by_path` sulle maiuscole: senza la 030 era una
    // lettura intera della tabella per ogni file trascinato.
    let connection = database();
    let letto = piano(
        &connection,
        "SELECT id FROM tracks WHERE path = 'x' COLLATE NOCASE LIMIT 1",
    );
    assert!(
        letto.contains("idx_tracks_percorso_piegato"),
        "il confronto senza maiuscole non usa l'indice: {letto}"
    );
}

#[test]
fn gli_artisti_toccati_passano_dall_indice() {
    // L'indice della 030 è scritto sulla stessa espressione di
    // `ARTISTA_EFFETTIVO`: se un giorno le due divergono, SQLite smette di
    // usarlo in silenzio e ogni ricostruzione torna a leggere tutto.
    let connection = database();
    let letto = piano(
        &connection,
        &format!(
            "SELECT DISTINCT {ARTISTA_EFFETTIVO} FROM tracks
                  WHERE {ARTISTA_EFFETTIVO} IN ('a', 'b')"
        ),
    );
    assert!(
        letto.contains("idx_tracks_artista_effettivo"),
        "la domanda sugli artisti non usa l'indice: {letto}"
    );
}

#[test]
fn i_percorsi_trascinati_si_trovano_anche_con_altre_maiuscole() {
    let mut connection = database();
    let mut dado = Dado::nuovo(16);
    let ids = libreria(&mut connection, &mut dado, 20);
    let vero = leggi(&connection, ids[3]).path;
    let gridato = vero.to_uppercase();
    let trovati = ids_by_path(
        &connection,
        &[vero.clone(), gridato.clone(), r"C:\non\c'è.flac".to_owned()],
        PathRules {
            case_insensitive: true,
        },
    )
    .expect("ricerca");
    assert_eq!(trovati, vec![Some(ids[3]), Some(ids[3]), None]);
    let esatti = ids_by_path(
        &connection,
        &[gridato],
        PathRules {
            case_insensitive: false,
        },
    )
    .expect("ricerca");
    assert_eq!(
        esatti,
        vec![None],
        "dove le maiuscole contano, non si piega"
    );
}

/// Quanto ci mette `fare`, in millisecondi, prendendo la migliore di tre:
/// la prima volta paga le pagine che SQLite non ha ancora in cache.
fn cronometra<T>(mut fare: impl FnMut() -> T) -> f64 {
    let mut migliore = f64::MAX;
    for _ in 0..3 {
        let inizio = Instant::now();
        std::hint::black_box(fare());
        migliore = migliore.min(inizio.elapsed().as_secs_f64() * 1000.0);
    }
    migliore
}

/// Le misure del rapporto di WP21, su cinquantamila brani sintetici.
///
/// `#[ignore]` perché è un cronometro e non una prova: costruire la
/// libreria vale qualche secondo, e i numeri dicono qualcosa solo letti da
/// qualcuno. Si lancia con
/// `cargo test -p rymembra-app misure_su_cinquantamila_brani -- --ignored --nocapture`.
#[test]
#[ignore = "misure per il rapporto, non una prova"]
fn misure_su_cinquantamila_brani() {
    let mut connection = database();
    let mut dado = Dado::nuovo(2026).come_una_libreria_vera();
    let inizio = Instant::now();
    let ids = libreria(&mut connection, &mut dado, 50_000);
    println!(
        "libreria sintetica: {} brani in {:.0} ms",
        ids.len(),
        inizio.elapsed().as_secs_f64() * 1000.0
    );
    let numeri = counts(&connection).expect("numeri");
    println!(
        "album {} · artisti {} · brani {}",
        numeri.albums, numeri.artists, numeri.tracks
    );

    // ── gli aggregati: prima (sempre intera) e dopo (per chiavi) ──
    let intera = cronometra(|| {
        let tx = connection.transaction().expect("transazione");
        rebuild_aggregates(&tx).expect("intera");
        drop(tx);
    });
    println!("ricostruzione intera (com'era dopo ogni scansione): {intera:.1} ms");
    for quanti in [1_usize, 12, 500] {
        let scelti: Vec<i64> = ids.iter().copied().step_by(97).take(quanti).collect();
        let per_chiavi = cronometra(|| {
            let tx = transazione_di_scrittura(&mut connection).expect("transazione");
            let mut toccati = Toccati::nessuno();
            toccati.segna_brani(&tx, &scelti).expect("prima");
            tx.execute(
                &format!(
                    "UPDATE tracks SET genre = 'Misura' WHERE id IN ({})",
                    segnaposti(scelti.len())
                ),
                rusqlite::params_from_iter(&scelti),
            )
            .expect("modifica");
            toccati.segna_brani(&tx, &scelti).expect("dopo");
            ricostruisci_aggregati(&tx, &toccati).expect("per chiavi");
            drop(tx);
        });
        println!("per chiavi, {quanti} brani toccati (modifica compresa): {per_chiavi:.1} ms");
    }
    let ferma = cronometra(|| {
        let tx = transazione_di_scrittura(&mut connection).expect("transazione");
        ricostruisci_aggregati(&tx, &Toccati::nessuno()).expect("niente");
        drop(tx);
    });
    println!("scansione che non ha cambiato niente: {ferma:.2} ms");

    // ── i file trascinati con altre maiuscole ──
    let gridati: Vec<String> = ids
        .iter()
        .step_by(500)
        .take(100)
        .map(|id| leggi(&connection, *id).path.to_uppercase())
        .collect();
    let regole = PathRules {
        case_insensitive: true,
    };
    let con_indice = cronometra(|| ids_by_path(&connection, &gridati, regole).expect("ricerca"));
    connection
        .execute_batch("DROP INDEX idx_tracks_percorso_piegato")
        .expect("via l'indice");
    let senza_indice = cronometra(|| ids_by_path(&connection, &gridati, regole).expect("ricerca"));
    connection
        .execute_batch("CREATE INDEX idx_tracks_percorso_piegato ON tracks(path COLLATE NOCASE)")
        .expect("indice rimesso");
    println!(
        "100 file trascinati con altre maiuscole: {senza_indice:.1} ms senza indice, {con_indice:.2} ms con"
    );

    // ── gli artisti toccati: con e senza l'indice sull'espressione ──
    let nomi: std::collections::BTreeSet<String> =
        (0..50).map(|n| format!("Artista {n}")).collect();
    let artisti_con = cronometra(|| {
        let tx = transazione_di_scrittura(&mut connection).expect("transazione");
        ricostruisci_gli_artisti(&tx, &nomi).expect("artisti");
        drop(tx);
    });
    connection
        .execute_batch("DROP INDEX idx_tracks_artista_effettivo")
        .expect("via l'indice");
    let artisti_senza = cronometra(|| {
        let tx = transazione_di_scrittura(&mut connection).expect("transazione");
        ricostruisci_gli_artisti(&tx, &nomi).expect("artisti");
        drop(tx);
    });
    let elenco_senza = cronometra(|| list_artists(&connection).expect("artisti"));
    connection
        .execute_batch(&format!(
            "CREATE INDEX idx_tracks_artista_effettivo ON tracks({ARTISTA_EFFETTIVO})"
        ))
        .expect("indice rimesso");
    let elenco_con = cronometra(|| list_artists(&connection).expect("artisti"));
    println!("50 artisti toccati: {artisti_senza:.1} ms senza indice, {artisti_con:.2} ms con");

    // ── le letture che la finestra chiede: chi supera i 50 ms ──
    println!("elenco degli artisti: {elenco_senza:.1} ms senza indice, {elenco_con:.1} ms con");
    let letture: Vec<(&str, f64)> = vec![
        (
            "numeri (avvio)",
            cronometra(|| counts(&connection).expect("numeri")),
        ),
        (
            "brani, prima pagina per scaffale",
            cronometra(|| list_tracks(&connection, TrackOrder::Shelf, 0, 200).expect("brani")),
        ),
        (
            "brani, pagina a metà per scaffale",
            cronometra(|| list_tracks(&connection, TrackOrder::Shelf, 25_000, 200).expect("brani")),
        ),
        (
            "brani, pagina in fondo per titolo",
            cronometra(|| list_tracks(&connection, TrackOrder::Title, 49_800, 200).expect("brani")),
        ),
        (
            "album, pagina a metà",
            cronometra(|| list_albums(&connection, 2_000, 200).expect("album")),
        ),
        (
            "dischi aggiunti di recente",
            cronometra(|| recently_added_albums(&connection, 12).expect("dischi")),
        ),
        (
            "ricerca «brano» (pagina)",
            cronometra(|| search(&connection, "brano", 0, 200).expect("ricerca")),
        ),
        (
            "ricerca «brano» (conteggio)",
            cronometra(|| search_count(&connection, "brano").expect("conteggio")),
        ),
        (
            "ricerca «art» (pagina)",
            cronometra(|| search(&connection, "art", 0, 200).expect("ricerca")),
        ),
        (
            "preferiti",
            cronometra(|| list_liked(&connection, 0, 200).expect("preferiti")),
        ),
    ];
    for (nome, ms) in letture {
        println!("{nome}: {ms:.1} ms");
    }

    // ── le playlist: l'elenco con le intelligenti, e un brano aggiunto ──
    use rymembra_domain::regole::{
        Campo, Combinazione, Insieme, Operatore, Ordinamento, Regola, Valore,
    };
    let regola = |campo, operatore, valore| Insieme {
        combinazione: Combinazione::Tutte,
        regole: vec![Regola {
            campo,
            operatore,
            valore,
        }],
        limite: None,
        ordinamento: Ordinamento::Scaffale,
    };
    let intelligenti = [
        regola(
            Campo::Artista,
            Operatore::Contiene,
            Valore::Testo("artista 1".into()),
        ),
        regola(
            Campo::Titolo,
            Operatore::Inizia,
            Valore::Testo("brano 1".into()),
        ),
        regola(
            Campo::Genere,
            Operatore::Uguale,
            Valore::Testo("jazz".into()),
        ),
        regola(
            Campo::Album,
            Operatore::NonContiene,
            Valore::Testo("9".into()),
        ),
        regola(Campo::Anno, Operatore::Maggiore, Valore::Numero(1990)),
        regola(Campo::Valutazione, Operatore::Maggiore, Valore::Numero(3)),
    ];
    for (n, insieme) in intelligenti.iter().enumerate() {
        crate::playlists::create_smart(&connection, &format!("Intelligente {n}"), insieme)
            .expect("intelligente");
    }
    let mut a_mano = 0;
    for n in 0..20 {
        a_mano = crate::playlists::create(&connection, &format!("A mano {n}"))
            .expect("playlist")
            .id;
        let brani: Vec<i64> = ids.iter().copied().skip(n * 50).take(50).collect();
        crate::playlists::add_tracks(&mut connection, a_mano, &brani).expect("brani");
    }
    let elenco_com_era = cronometra(|| {
        // Com'era: l'elenco e un conteggio vero per ogni intelligente.
        for insieme in &intelligenti {
            crate::smart::conteggi(&connection, insieme, 0).expect("conteggio");
        }
    });
    let _ = crate::playlists::list(&connection).expect("prima volta");
    let elenco_ricordato = cronometra(|| crate::playlists::list(&connection).expect("elenco"));
    println!(
        "elenco playlist con {} intelligenti: {elenco_com_era:.1} ms di soli conteggi com'era, {elenco_ricordato:.1} ms con i conteggi ricordati",
        intelligenti.len()
    );
    // Un brano aggiunto a una playlist a mano scrive `playlist_tracks`, non
    // `tracks`: la generazione non cambia, e il riepilogo è di lei sola.
    let aggiunta = cronometra(|| {
        crate::playlists::add_tracks(&mut connection, a_mano, &ids[..1]).expect("aggiunta")
    });
    println!(
        "un brano aggiunto a una playlist (riepilogo compreso): {aggiunta:.1} ms, contro {elenco_com_era:.1} ms del solo ricalcolo delle intelligenti com'era"
    );
    let aperta =
        cronometra(|| crate::smart::brani(&connection, &intelligenti[0], 0).expect("brani"));
    let anteprima =
        cronometra(|| crate::smart::quanti(&connection, &intelligenti[0], 0).expect("quanti"));
    println!(
        "playlist intelligente aperta: {aperta:.1} ms · anteprima delle regole: {anteprima:.1} ms"
    );
}
