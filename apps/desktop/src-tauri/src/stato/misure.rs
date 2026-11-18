//! Le misure di FR2: quanto costa, sotto il lucchetto della libreria, quel che la
//! finestra rilegge quando la pagina di un artista è aperta — e quanto fuori.
//!
//! Non sono prove di un comportamento: costruiscono una libreria da cinquantamila
//! brani e **scrivono i tempi** (`println!`). Per questo sono `#[ignore]`, e si
//! lanciano a mano:
//!
//! ```text
//! cargo test --workspace --locked --lib misura -- --ignored --nocapture --test-threads=1
//! ```
//!
//! (con `--workspace` e non `-p`: `-p rymembra-desktop` ricompila tutto; con
//! `--release` per i numeri di una build vera). Il numero che interessa è la
//! **tenuta**: per quanto tempo una rilettura impedisce a un altro (una scansione
//! che scrive un lotto, un'analisi, il filo dello specchio) di prendere il
//! lucchetto. Con la pagina di un artista aperta ogni `caricaVista` rilegge gli
//! artisti e il conto delle partecipazioni, e durante la lettura di un catalogo
//! succede ogni 3 s (`RILEGGI_DURANTE_LA_LETTURA_MS`).
//!
//! # I numeri del 18/11/2026
//!
//! Profilo di sviluppo (non `--release`), PC carico da altre compilazioni,
//! cinquantamila brani. «Prima» è il codice di `bd1dc80`, senza modifiche: la via
//! `sotto_il_lucchetto` è quella di `leggi_la_libreria`. Due esecuzioni di
//! «prima», perché i tempi ballano con il carico del PC.
//!
//! | | prima (1ª / 2ª esecuzione) | dopo |
//! |---|---|---|
//! | `list_artists` | 152 / 245 ms | 156 ms |
//! | il conto (artisti + partecipazioni) | 155 / 241 ms | 160 ms |
//! | **lucchetto tenuto** per ogni rilettura del conto | 155 / 241 ms | **0 ms** |
//! | lucchetto tenuto, a una rilettura ogni 3 s | 5,2 % / 8 % del tempo | 0 % |
//! | chi vuole il lucchetto mentre si rilegge ogni 100 ms (300 richieste) | 22 hanno aspettato oltre 5 ms; p99 255 ms, massima 261 ms, media 16,8 ms | nessuna; massima 0,1 ms |
//! | una pagina da 50 righe (`brani_artista`, `list_tracks`…) | 0,3–0,5 ms | 2,8–3,5 ms |
//! | `list_albums` (50), «Tutti» | 185 ms | 135 ms |
//! | `counts` | 24 ms | 19 ms |
//!
//! La lettura costa quanto prima — è la stessa query —: quel che cambia è chi
//! aspetta. Il prezzo è una connessione aperta per ogni lettura, circa 2,5 ms in
//! più su una pagina da 50 righe (nel profilo di sviluppo): trascurabile accanto a
//! un passaggio dall'IPC, e accanto a una pagina che sotto il lucchetto può
//! trovarlo tenuto da un'altra lettura da 150–250 ms. Con meno carico le cifre
//! assolute scendono, il rapporto no. La prima versione della misura rilegge in
//! continuazione, senza pausa, e non dice niente di vero: un `Mutex` non è equo, e
//! il lettore che lo riprende appena lo lascia tiene fuori l'altro per secondi.

use std::time::{Duration, Instant};

use rymembra_app::library::{
    OrdineBraniArtista, Partecipazione, Toccati, TrackOrder, albums_by_artist, brani_artista,
    counts, list_albums, list_artists, list_tracks, partecipazioni, partecipazioni_conta,
    ricostruisci_aggregati,
};
use rymembra_app::specchio::Origine;
use rymembra_domain::errors::AppError;

use super::{Stato, con_libreria};

/// Quanti brani ha la libreria della misura.
const BRANI: i64 = 50_000;

/// L'artista di cui si apre la pagina: ha dischi suoi, brani su una raccolta (le
/// partecipazioni esatte) e un «feat.» in un brano di un altro (le euristiche).
const ARTISTA: &str = "Artista 7";

/// Una libreria da [`BRANI`] brani, duemila artisti e settemila dischi, con le
/// raccolte e i «feat.» che danno alla pagina di un artista qualcosa da contare.
fn libreria_grande() -> (tempfile::TempDir, Stato) {
    let dir = tempfile::tempdir().expect("cartella");
    let stato = Stato::apri(dir.path().to_path_buf());
    con_libreria(&stato, |l| {
        assert_eq!(l.giornale, "wal", "senza WAL la misura non dice niente");
        let tx = l
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| rymembra_app::db::codice_da_sqlite("misura", &err))?;
        tx.execute(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
             INSERT INTO tracks
                 (id, path, track_key, title, artist, album_artist, album, album_key,
                  track_number, year, duration_ms, file_size, play_count,
                  date_added, date_modified)
             SELECT i,
                    '/m/' || i || '.mp3',
                    'k' || i,
                    'Titolo ' || i,
                    CASE WHEN i % 25 = 0
                         THEN 'Ospite ' || (i % 40) || ' feat. Artista ' || (i % 2000)
                         ELSE 'Artista ' || (i % 2000) END,
                    CASE WHEN i % 17 = 0 THEN 'Vari artisti' ELSE NULL END,
                    'Disco ' || (i % 7000),
                    'ak' || (i % 7000),
                    i % 12 + 1,
                    1970 + i % 50,
                    200000,
                    5000000,
                    i % 5,
                    0,
                    0
               FROM n",
            [BRANI],
        )
        .map_err(|err| rymembra_app::db::codice_da_sqlite("misura", &err))?;
        ricostruisci_aggregati(&tx, &Toccati::tutti())?;
        tx.commit()
            .map_err(|err| rymembra_app::db::codice_da_sqlite("misura", &err))
    })
    .expect("la libreria della misura");
    (dir, stato)
}

/// Quel che la pagina di un artista rilegge a ogni giro, in due pezzi: gli
/// artisti e il conto delle partecipazioni (l'effetto di `giroArtista`), che sono
/// quelli che F3c segnalò.
fn il_conto(c: &rusqlite::Connection) -> Result<(), AppError> {
    list_artists(c, &Origine::Tutti)?;
    partecipazioni_conta(c, &Origine::Tutti, ARTISTA)?;
    Ok(())
}

/// La mediana e il massimo di `n` esecuzioni.
fn mediana_e_massimo(n: usize, mut una: impl FnMut() -> Duration) -> (Duration, Duration) {
    let mut tempi: Vec<Duration> = (0..n).map(|_| una()).collect();
    tempi.sort();
    (
        tempi[tempi.len().div_euclid(2)],
        *tempi.last().expect("almeno una"),
    )
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

/// Una lettura del nucleo, da fare in un modo o nell'altro.
type Lettura<'a> = &'a (dyn Fn(&rusqlite::Connection) -> Result<(), AppError> + Sync);

/// Come la finestra legge: `sotto_il_lucchetto` è com'era (`leggi_la_libreria`:
/// `con_libreria` più la funzione del nucleo), l'altra è la connessione di sola
/// lettura (`leggi_o_aspetta`).
type Via = fn(&Stato, Lettura) -> Result<(), AppError>;

fn sotto_il_lucchetto(stato: &Stato, lettura: Lettura) -> Result<(), AppError> {
    con_libreria(stato, |l| lettura(&l.connection))
}

/// Quanto aspetta il lucchetto chi lo vuole mentre un altro filo rilegge il conto
/// a ritmo di `pausa` fra una lettura e l'altra: `n` richieste a 7 ms l'una
/// dall'altra, ordinate.
///
/// Con la pausa, e non in continuazione: un `Mutex` non è equo, e un lettore che
/// lo riprende appena lo lascia può tenere fuori l'altro per secondi — è
/// successo alla prima versione di questa misura, e non è il caso della
/// finestra, che rilegge ogni 3 s.
fn attese_del_lucchetto(stato: &Stato, via: Via, pausa: Duration, n: usize) -> Vec<Duration> {
    let finito = std::sync::atomic::AtomicBool::new(false);
    let mut attese = Vec::with_capacity(n);
    std::thread::scope(|ambito| {
        ambito.spawn(|| {
            while !finito.load(std::sync::atomic::Ordering::Acquire) {
                via(stato, &il_conto).expect("il conto");
                std::thread::sleep(pausa);
            }
        });
        for _ in 0..n {
            let inizio = Instant::now();
            con_libreria(stato, |_| Ok(())).expect("il lucchetto");
            attese.push(inizio.elapsed());
            std::thread::sleep(Duration::from_millis(7));
        }
        finito.store(true, std::sync::atomic::Ordering::Release);
    });
    attese.sort();
    attese
}

fn racconta_le_attese(nome: &str, attese: &[Duration]) {
    let somma: Duration = attese.iter().sum();
    let oltre = attese
        .iter()
        .filter(|a| **a > Duration::from_millis(5))
        .count();
    println!(
        "MISURA {nome}: attesa del lucchetto su {} richieste — mediana {}, p90 {}, p99 {}, massima {}, media {}; {oltre} oltre i 5 ms",
        attese.len(),
        ms(attese[attese.len().div_euclid(2)]),
        ms(attese[(attese.len() * 90).div_euclid(100)]),
        ms(attese[(attese.len() * 99).div_euclid(100)]),
        ms(*attese.last().expect("almeno una")),
        ms(somma / u32::try_from(attese.len()).expect("poche"))
    );
}

/// Tutta la misura, per una via: quanto dura ogni lettura, e quanto fa aspettare
/// chi vuole il lucchetto.
fn misura(nome: &str, via: Via) {
    let (_dir, stato) = libreria_grande();
    let _ = via(&stato, &il_conto);
    let (mediana, massimo) = mediana_e_massimo(15, || {
        let inizio = Instant::now();
        via(&stato, &il_conto).expect("il conto");
        inizio.elapsed()
    });
    println!(
        "MISURA {nome} — il conto (artisti + partecipazioni): mediana {} (massimo {}) per giro",
        ms(mediana),
        ms(massimo)
    );
    let singole: [(&str, Lettura); 8] = [
        ("list_artists", &|c| {
            list_artists(c, &Origine::Tutti).map(|_| ())
        }),
        ("partecipazioni_conta", &|c| {
            partecipazioni_conta(c, &Origine::Tutti, ARTISTA).map(|_| ())
        }),
        ("brani_artista (50)", &|c| {
            brani_artista(
                c,
                &Origine::Tutti,
                ARTISTA,
                OrdineBraniArtista::Album,
                0,
                50,
            )
            .map(|_| ())
        }),
        ("partecipazioni esatte (50)", &|c| {
            partecipazioni(c, &Origine::Tutti, ARTISTA, Partecipazione::Esatta, 0, 50).map(|_| ())
        }),
        ("partecipazioni euristiche (50)", &|c| {
            partecipazioni(
                c,
                &Origine::Tutti,
                ARTISTA,
                Partecipazione::Euristica,
                0,
                50,
            )
            .map(|_| ())
        }),
        ("albums_by_artist (50)", &|c| {
            albums_by_artist(c, &Origine::Tutti, ARTISTA, 0, 50).map(|_| ())
        }),
        ("list_albums (50)", &|c| {
            list_albums(c, &Origine::Tutti, 0, 50).map(|_| ())
        }),
        ("list_tracks per titolo (50)", &|c| {
            list_tracks(c, &Origine::Tutti, TrackOrder::Title, 0, 50).map(|_| ())
        }),
    ];
    for (cosa, lettura) in singole {
        let (mediana, massimo) = mediana_e_massimo(25, || {
            let inizio = Instant::now();
            via(&stato, lettura).expect("la lettura");
            inizio.elapsed()
        });
        println!(
            "MISURA {nome} — {cosa}: mediana {} (massimo {})",
            ms(mediana),
            ms(massimo)
        );
    }
    let (mediana, _) = mediana_e_massimo(25, || {
        let inizio = Instant::now();
        via(&stato, &|c| counts(c, &Origine::Tutti).map(|_| ())).expect("counts");
        inizio.elapsed()
    });
    println!("MISURA {nome} — counts: mediana {}", ms(mediana));
    let attese = attese_del_lucchetto(&stato, via, Duration::from_millis(100), 300);
    racconta_le_attese(
        &format!("{nome}, il conto rifatto ogni 100 ms (30 volte più spesso di quanto succede)"),
        &attese,
    );
}

/// PRIMA: com'era, la rilettura sotto il lucchetto.
#[test]
#[ignore = "costruisce una libreria da 50.000 brani: si lancia a mano"]
fn misura_prima_sotto_il_lucchetto() {
    misura("prima (sotto il lucchetto)", sotto_il_lucchetto);
}

/// DOPO: la rilettura su una connessione di sola lettura, senza il lucchetto.
#[test]
#[ignore = "costruisce una libreria da 50.000 brani: si lancia a mano"]
fn misura_dopo_senza_il_lucchetto() {
    misura("dopo (connessione a parte)", fuori_dal_lucchetto);
}

fn fuori_dal_lucchetto(stato: &Stato, lettura: Lettura) -> Result<(), AppError> {
    super::leggi_o_aspetta(stato, |c| lettura(c))
}
