//! Lo strato culturale dell'affinità: chi ascolta questo ascolta anche quello.
//!
//! # Cosa aggiunge, e cosa il suono non può sapere
//!
//! [`crate::sonora`] misura come suona un brano, e lo fa bene: due pezzi con lo
//! stesso timbro, lo stesso passo e la stessa dinamica finiscono vicini. Ma due
//! brani possono suonare identici e appartenere a mondi che non si toccano — un
//! pezzo per pianoforte solo di Satie e uno di un compositore di colonne sonore
//! del 2015 hanno la stessa impronta e non li ascolta la stessa gente. E al
//! contrario, due brani dello stesso disco possono suonare lontanissimi e
//! andare sempre insieme.
//!
//! Quel «vanno insieme» non sta nel segnale. Sta in cosa la gente ascolta di
//! fila, ed è l'unica delle tre affinità che va chiesta a qualcuno.
//!
//! # Si tiene solo quel che possiedi
//!
//! ListenBrainz risponde con identificativi MusicBrainz: brani che possono
//! esistere ovunque. Qui se ne tiene **soltanto quelli che sono già in
//! libreria**, risolti da `tracks.mb_recording_id`, e tutto il resto si butta.
//!
//! Non è un ripiego, è la tesi del progetto: Rymembra non ha un catalogo da
//! proporti, ha il tuo. Un vicino che non possiedi non è una raccomandazione, è
//! una vetrina — e una coda che propone brani che non puoi sentire è la cosa
//! peggiore che questo modulo potrebbe fare. L'effetto collaterale è che la
//! tabella resta piccola per costruzione: al più [`VICINI_TENUTI`] righe per
//! brano, le due fonti insieme.
//!
//! # Le due fonti, e perché la seconda ha un tetto più basso
//!
//! * `lb-registrazione` — i vicini di **questo** brano. Precisa, e richiede che
//!   l'arricchimento gli abbia già trovato un `mb_recording_id`. Viene prima.
//! * `lb-artista` — i brani degli artisti vicini al **suo** artista. È il
//!   ripiego: riempie i posti che la registrazione lascia vuoti — tutti, per la
//!   metà di libreria che un identificativo di registrazione non ce l'ha, o i
//!   cui vicini di registrazione non sono in casa — ed è per costruzione più
//!   larga: dice «questo genere di gente», non «questa canzone». Per questo
//!   entra con [`SCONTO_ARTISTA`] addosso: fra un vicino di registrazione e uno
//!   d'artista con lo stesso numero, il primo sa qualcosa che il secondo sta
//!   indovinando. E ogni artista vicino porta al più [`TETTO_PER_ARTISTA`] dei
//!   suoi brani, i più ascoltati: tutti, su una libreria vera, voleva dire
//!   centinaia di righe per brano.
//!
//! # Le finestre di lucchetto
//!
//! Come `enrich`: si leggono i candidati, si rilascia, si chiede alla rete, si
//! riprende per scrivere. Le tre funzioni pubbliche sono divise esattamente su
//! quelle giunture — [`candidati`] vuole una connessione, [`chiedi`] vuole i
//! fornitori e **non può** ricevere una connessione perché `rymembra-meta` non
//! saprebbe cosa farsene, [`registra`] vuole una transazione.

use std::collections::HashMap;

use rusqlite::{Connection, Transaction};
use rymembra_domain::errors::AppError;
use rymembra_meta::Fornitori;
use rymembra_meta::listenbrainz::{self, MBID_PER_RICHIESTA};

use crate::library;

/// Quanti brani si domandano in una passata.
///
/// Cento, cioè quattro richieste da venticinque: al ritmo di una al secondo sono
/// quattro secondi di rete per passata, e una libreria di millequattrocento
/// brani si copre in quattordici passate. Non c'è nessuna fretta — la risposta
/// si ricorda per tre mesi — e una passata corta è una passata che non tiene
/// occupato il filo dei metadati mentre l'utente aspetta una copertina.
pub const LOTTO: usize = 4 * MBID_PER_RICHIESTA;

/// Quanti vicini si tengono per brano, le due fonti insieme.
///
/// Venti. Oltre non serve: il riordino della coda guarda i primi candidati che
/// il motore delle regole gli passa, e un vicino in ventunesima posizione non
/// ha mai cambiato una scelta. Tenerne cento vorrebbe dire cinque volte le
/// righe per la stessa risposta.
pub const VICINI_TENUTI: usize = 20;

/// Quanto vale un vicino trovato per artista invece che per registrazione.
///
/// Sette decimi. Non è una taratura: è la differenza fra «chi ascolta questa
/// canzone ascolta anche quella» e «chi ascolta questo artista ascolta anche
/// quell'altro, e questo è un suo brano qualsiasi». La seconda è vera più
/// spesso e dice molto meno.
pub const SCONTO_ARTISTA: f64 = 0.7;

/// Ogni quanto si ritorna a chiedere per un brano.
///
/// Novanta giorni, cioè lo stesso di `rymembra_meta::deposito::VIVE_AFFINITA_MS`,
/// e i due numeri dicono la stessa cosa da due parti diverse: il deposito
/// risparmia la richiesta HTTP, questo risparmia di ricostruire le righe. Fosse
/// più corto qui, si pagherebbe la ricostruzione per riavere dal deposito la
/// risposta identica.
pub const RICHIEDI_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// Un brano di cui si vogliono i vicini.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chiesta {
    /// Quale brano, in casa.
    pub track_id: i64,
    /// Il suo `mb_recording_id`, se ce l'ha.
    pub registrazione: Option<String>,
    /// Il `mb_artist_id` del suo artista, se ce l'ha.
    pub artista: Option<String>,
}

/// Quel che si è saputo di un brano.
#[derive(Debug, Clone, PartialEq)]
pub struct Trovato {
    /// Quale brano.
    pub track_id: i64,
    /// I vicini, come identificativi MusicBrainz di registrazione.
    ///
    /// Ancora **non** risolti in brani di casa: la risoluzione ha bisogno del
    /// database, e questo è il tipo che attraversa la finestra senza lucchetto.
    pub vicini: Vec<(String, f64)>,
    /// I vicini che vengono dall'artista e non dalla registrazione.
    pub artisti: Vec<(String, f64)>,
}

/// Quanti brani aspettano ancora una domanda.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn quanti_mancano(connection: &Connection, adesso: i64) -> Result<i64, AppError> {
    connection
        .query_row(
            &format!("SELECT COUNT(*) FROM tracks t {}", da_chiedere()),
            [scadenza(adesso)],
            |riga| riga.get(0),
        )
        .map_err(|err| library::db_error("conteggio dei brani senza vicini", &err))
}

/// La clausola che dice chi va chiesto, in un posto solo.
///
/// Una funzione e non due copie per la ragione di `sonora::candidato_where`: due
/// copie divergono il giorno in cui qualcuno ne aggiusta una.
///
/// Un brano senza **nessuno** dei due identificativi non è un candidato: non
/// c'è niente da chiedere per lui, e tenerlo nell'elenco vorrebbe dire
/// riguardarlo a ogni passata per sempre. Ci rientra da solo appena
/// l'arricchimento gliene trova uno.
const fn da_chiedere() -> &'static str {
    "LEFT JOIN brano_vicino_stato s ON s.track_id = t.id
     LEFT JOIN artists a ON a.name = t.artist
     WHERE (t.mb_recording_id IS NOT NULL OR a.mb_artist_id IS NOT NULL)
       AND (s.track_id IS NULL OR s.chiesto_at < ?1)"
}

/// Quando diventa vecchia una domanda già fatta.
const fn scadenza(adesso: i64) -> i64 {
    adesso.saturating_sub(RICHIEDI_MS)
}

/// Il prossimo lotto di brani da chiedere.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn candidati(
    connection: &Connection,
    adesso: i64,
    quanti: usize,
) -> Result<Vec<Chiesta>, AppError> {
    let sql = format!(
        "SELECT t.id, t.mb_recording_id, a.mb_artist_id FROM tracks t {}
         ORDER BY t.last_played_at DESC NULLS LAST, t.date_added DESC
         LIMIT ?2",
        da_chiedere()
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| library::db_error("candidati ai vicini", &err))?;
    let righe = statement
        .query_map(
            rusqlite::params![scadenza(adesso), i64::try_from(quanti).unwrap_or(i64::MAX)],
            |riga| {
                Ok(Chiesta {
                    track_id: riga.get(0)?,
                    registrazione: riga.get(1)?,
                    artista: riga.get(2)?,
                })
            },
        )
        .map_err(|err| library::db_error("candidati ai vicini", &err))?;
    let mut fuori = Vec::new();
    for riga in righe {
        fuori.push(riga.map_err(|err| library::db_error("candidati ai vicini", &err))?);
    }
    Ok(fuori)
}

/// Chiede a ListenBrainz i vicini di questo lotto. **Nessun lucchetto.**
///
/// Due richieste per lotto, non due per brano: gli endpoint accettano
/// venticinque identificativi per volta, ed è ciò che rende sopportabile una
/// libreria intera.
///
/// # Errori
///
/// L'errore di rete così com'è, e solo se **entrambe** le domande sono fallite
/// senza restituire niente: se una delle due ha risposto, quel che si è saputo
/// vale più dell'errore sull'altra, e chi chiama scriverà la metà che ha.
pub fn chiedi(fornitori: &Fornitori, lotto: &[Chiesta]) -> Result<Vec<Trovato>, AppError> {
    let registrazioni: Vec<&str> = lotto
        .iter()
        .filter_map(|c| c.registrazione.as_deref())
        .collect();
    let artisti: Vec<&str> = lotto.iter().filter_map(|c| c.artista.as_deref()).collect();

    let per_brano = listenbrainz::brani_affini(fornitori, &registrazioni);
    let per_artista = listenbrainz::artisti_affini(fornitori, &artisti);

    // Un guasto su tutte e due vuol dire che la rete non c'è: si propaga, e la
    // passata si ferma senza segnare niente come «chiesto». Con una sola delle
    // due caduta si va avanti: la mappa vuota è indistinguibile da «questo
    // brano non ha vicini di quel tipo», che è un esito vero e frequente.
    let (per_brano, per_artista) = match (per_brano, per_artista) {
        (Err(err), Err(_)) => return Err(err),
        (brani, artisti) => (brani.unwrap_or_default(), artisti.unwrap_or_default()),
    };

    Ok(lotto
        .iter()
        .map(|chiesta| Trovato {
            track_id: chiesta.track_id,
            vicini: chiesta
                .registrazione
                .as_deref()
                .and_then(|mbid| per_brano.get(mbid))
                .map(|vicini| {
                    vicini
                        .iter()
                        .take(VICINI_TENUTI)
                        .map(|v| (v.mbid.clone(), v.affinita))
                        .collect()
                })
                .unwrap_or_default(),
            artisti: chiesta
                .artista
                .as_deref()
                .and_then(|mbid| per_artista.get(mbid))
                .map(|vicini| {
                    vicini
                        .iter()
                        .take(VICINI_TENUTI)
                        .map(|v| (v.mbid.clone(), v.affinita * SCONTO_ARTISTA))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect())
}

/// Scrive quel che si è saputo, e segna che si è chiesto.
///
/// Il valore di ritorno è quante righe di vicinanza sono entrate — che è quasi
/// sempre molto meno dei vicini ricevuti, perché si tiene solo quel che è già in
/// libreria.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn registra(tx: &Transaction<'_>, trovati: &[Trovato], adesso: i64) -> Result<usize, AppError> {
    let mut scritte = 0_usize;
    for trovato in trovati {
        // Le righe vecchie di **questo** brano se ne vanno prima: una risposta
        // nuova sostituisce la precedente, non ci si somma. Senza, un vicino
        // che ListenBrainz ha smesso di considerare tale resterebbe in tabella
        // per sempre, perché un `INSERT OR REPLACE` non cancella quel che non
        // ricompare. È anche quel che rende [`VICINI_TENUTI`] un tetto per
        // brano e non per passata.
        tx.execute(
            "DELETE FROM brano_vicino WHERE track_id = ?1",
            [trovato.track_id],
        )
        .map_err(|err| library::db_error("pulizia dei vicini di un brano", &err))?;

        // La registrazione prima, l'artista nei posti che lascia: vedi le due
        // fonti in testa al modulo.
        let da_registrazione = scrivi_fonte(
            tx,
            trovato.track_id,
            &trovato.vicini,
            "lb-registrazione",
            adesso,
            VICINI_TENUTI,
        )?;
        let da_artista = scrivi_fonte(
            tx,
            trovato.track_id,
            &trovato.artisti,
            "lb-artista",
            adesso,
            VICINI_TENUTI.saturating_sub(da_registrazione),
        )?;
        scritte = scritte
            .saturating_add(da_registrazione)
            .saturating_add(da_artista);

        // Si segna **sempre**, anche a zero vicini: è tutta la ragione per cui
        // `brano_vicino_stato` è una tabella a parte. Senza questa riga, un
        // brano che ListenBrainz non conosce tornerebbe candidato alla passata
        // dopo, e a quella dopo ancora, per sempre.
        tx.execute(
            "INSERT INTO brano_vicino_stato (track_id, chiesto_at, quanti)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(track_id) DO UPDATE SET chiesto_at = ?2, quanti = ?3",
            rusqlite::params![
                trovato.track_id,
                adesso,
                i64::try_from(trovato.vicini.len().saturating_add(trovato.artisti.len()))
                    .unwrap_or(i64::MAX)
            ],
        )
        .map_err(|err| library::db_error("stato dei vicini di un brano", &err))?;
    }
    Ok(scritte)
}

/// Quanti brani porta, al più, un artista vicino.
///
/// Tre: i suoi più ascoltati. Un artista vicino dice «questo genere di gente»,
/// e tre brani bastano a dirlo; tutti i suoi brani in libreria voleva dire, su
/// una libreria vera, centinaia di righe per brano e milioni in tutto — la
/// tabella che ogni brano tolto doveva poi scorrere per la cascata.
pub const TETTO_PER_ARTISTA: usize = 3;

/// Le righe di una fonte sola, risolte da identificativo a brano di casa.
///
/// La risoluzione avviene **dentro** l'inserimento, con una sottoquery, e non
/// leggendo prima gli identificativi in una mappa: sono al più venti righe per
/// brano e SQLite ha già l'indice giusto: portarle fuori e rimetterle dentro
/// costerebbe più della query che si vuole evitare.
///
/// `posti` è quante righe può ancora scrivere per questo brano: i vicini si
/// prendono nell'ordine in cui arrivano, dal più forte, finché ce n'è.
fn scrivi_fonte(
    tx: &Transaction<'_>,
    track_id: i64,
    vicini: &[(String, f64)],
    fonte: &str,
    adesso: i64,
    posti: usize,
) -> Result<usize, AppError> {
    if vicini.is_empty() || posti == 0 {
        return Ok(0);
    }
    let sql = match fonte {
        // Un vicino d'artista è un artista: si scrivono i suoi brani più
        // ascoltati, al più [`TETTO_PER_ARTISTA`] e mai più dei posti rimasti
        // (`?6`). La `SELECT` interna e il `WHERE true` non sono pignoleria:
        // un `INSERT … SELECT` con `ON CONFLICT` ha bisogno di una `WHERE`
        // perché SQLite non legga `ON` come la condizione di una giunzione.
        "lb-artista" => {
            "INSERT INTO brano_vicino (track_id, vicino_id, punteggio, fonte, raccolto_at)
             SELECT ?1, scelti.id, ?3, ?4, ?5 FROM (
               SELECT t.id FROM tracks t
               JOIN artists a ON a.name = t.artist
               WHERE a.mb_artist_id = ?2 AND t.id <> ?1
               ORDER BY t.play_count DESC, t.id
               LIMIT ?6
             ) scelti WHERE true
             ON CONFLICT(track_id, vicino_id, fonte)
               DO UPDATE SET punteggio = MAX(punteggio, ?3), raccolto_at = ?5"
        }
        _ => {
            "INSERT INTO brano_vicino (track_id, vicino_id, punteggio, fonte, raccolto_at)
             SELECT ?1, scelti.id, ?3, ?4, ?5 FROM (
               SELECT t.id FROM tracks t
               WHERE t.mb_recording_id = ?2 AND t.id <> ?1
               ORDER BY t.id
               LIMIT ?6
             ) scelti WHERE true
             ON CONFLICT(track_id, vicino_id, fonte)
               DO UPDATE SET punteggio = MAX(punteggio, ?3), raccolto_at = ?5"
        }
    };
    let mut statement = tx
        .prepare_cached(sql)
        .map_err(|err| library::db_error("inserimento di un vicino", &err))?;
    let mut scritte = 0_usize;
    for (mbid, punteggio) in vicini {
        let rimasti = posti.saturating_sub(scritte);
        if rimasti == 0 {
            break;
        }
        let tetto = if fonte == "lb-artista" {
            rimasti.min(TETTO_PER_ARTISTA)
        } else {
            rimasti
        };
        // Il `CHECK (punteggio BETWEEN 0 AND 1)` dello schema è una rete, non un
        // filtro: un punteggio fuori scala qui vorrebbe dire che il servizio ha
        // cambiato normalizzazione, e serrarlo è preferibile a far fallire una
        // transazione intera per una riga.
        let punteggio = punteggio.clamp(0.0, 1.0);
        scritte = scritte.saturating_add(
            statement
                .execute(rusqlite::params![
                    track_id,
                    mbid,
                    punteggio,
                    fonte,
                    adesso,
                    i64::try_from(tetto).unwrap_or(i64::MAX)
                ])
                .map_err(|err| library::db_error("inserimento di un vicino", &err))?,
        );
    }
    Ok(scritte)
}

/// Quanti vicini conosce, per brano. Serve alle prove e alla diagnostica.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn quanti_vicini(connection: &Connection) -> Result<HashMap<i64, usize>, AppError> {
    let mut statement = connection
        .prepare("SELECT track_id, COUNT(*) FROM brano_vicino GROUP BY track_id")
        .map_err(|err| library::db_error("conteggio dei vicini", &err))?;
    let righe = statement
        .query_map([], |riga| {
            Ok((riga.get::<_, i64>(0)?, riga.get::<_, i64>(1)?))
        })
        .map_err(|err| library::db_error("conteggio dei vicini", &err))?;
    let mut fuori = HashMap::new();
    for riga in righe {
        let (id, quanti) = riga.map_err(|err| library::db_error("conteggio dei vicini", &err))?;
        fuori.insert(id, usize::try_from(quanti).unwrap_or(0));
    }
    Ok(fuori)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Una libreria con due artisti, identificativi MusicBrainz e tutto.
    fn libreria() -> Connection {
        let connection = crate::db::open_in_memory().expect("database").connection;
        connection
            .execute_batch(
                "INSERT INTO artists (name, mb_artist_id) VALUES
                   ('Art',   'aaaaaaaa-0000-0000-0000-000000000001'),
                   ('Altro', 'aaaaaaaa-0000-0000-0000-000000000002');
                 INSERT INTO tracks
                     (id, path, track_key, title, artist, album, album_key,
                      duration_ms, file_size, date_added, date_modified, mb_recording_id)
                 VALUES
                   (1, 'a.mp3', 'k1', 'Uno', 'Art',   'P', 'art|p',   1000, 1, 1, 1,
                    'bbbbbbbb-0000-0000-0000-000000000001'),
                   (2, 'b.mp3', 'k2', 'Due', 'Art',   'P', 'art|p',   1000, 1, 1, 1,
                    'bbbbbbbb-0000-0000-0000-000000000002'),
                   (3, 'c.mp3', 'k3', 'Tre', 'Altro', 'T', 'altro|t', 1000, 1, 1, 1,
                    'bbbbbbbb-0000-0000-0000-000000000003'),
                   (4, 'd.mp3', 'k4', 'Qua', 'Altro', 'T', 'altro|t', 1000, 1, 1, 1, NULL);",
            )
            .expect("libreria");
        connection
    }

    /// Un vicino che è in libreria si scrive; uno che non c'è si butta.
    ///
    /// È la prova della tesi del modulo: quel che non possiedi non è una
    /// raccomandazione.
    #[test]
    fn si_tiene_solo_quel_che_si_ha_in_casa() {
        let mut connection = libreria();
        let trovati = vec![Trovato {
            track_id: 1,
            vicini: vec![
                // Questo è il brano 2, che c'è.
                ("bbbbbbbb-0000-0000-0000-000000000002".to_owned(), 1.0),
                // Questo non è di nessuno.
                ("cccccccc-0000-0000-0000-000000000009".to_owned(), 0.9),
            ],
            artisti: Vec::new(),
        }];
        let tx = connection.transaction().expect("transazione");
        let scritte = registra(&tx, &trovati, 1_000).expect("registrazione");
        tx.commit().expect("commit");
        assert_eq!(scritte, 1, "ha scritto un vicino che non è in libreria");
        let conto = quanti_vicini(&connection).expect("conteggio");
        assert_eq!(conto.get(&1), Some(&1));
    }

    /// Un vicino d'artista diventa tutti i brani di quell'artista, scontati.
    #[test]
    fn un_artista_vicino_porta_i_suoi_brani() {
        let mut connection = libreria();
        let trovati = vec![Trovato {
            track_id: 1,
            vicini: Vec::new(),
            artisti: vec![("aaaaaaaa-0000-0000-0000-000000000002".to_owned(), 0.7)],
        }];
        let tx = connection.transaction().expect("transazione");
        registra(&tx, &trovati, 1_000).expect("registrazione");
        tx.commit().expect("commit");
        // I brani 3 e 4 sono di «Altro».
        let conto = quanti_vicini(&connection).expect("conteggio");
        assert_eq!(
            conto.get(&1),
            Some(&2),
            "non ha preso tutti i brani dell'artista"
        );
    }

    /// Aggiunge un artista con il suo identificativo e `quanti` brani suoi.
    fn artista_con_brani(connection: &Connection, numero: i64, quanti: i64) -> String {
        let mbid = format!("dddddddd-0000-0000-0000-{numero:012}");
        let nome = format!("Vicino {numero}");
        connection
            .execute(
                "INSERT INTO artists (name, mb_artist_id) VALUES (?1, ?2)",
                rusqlite::params![nome, mbid],
            )
            .expect("artista");
        for n in 0..quanti {
            let id = 1_000 * numero + n;
            connection
                .execute(
                    "INSERT INTO tracks
                         (id, path, track_key, title, artist, album, album_key,
                          duration_ms, file_size, date_added, date_modified, play_count)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'X', 'x', 1000, 1, 1, 1, ?6)",
                    rusqlite::params![
                        id,
                        format!("{id}.mp3"),
                        format!("k{id}"),
                        format!("T{id}"),
                        nome,
                        n % 5
                    ],
                )
                .expect("brano");
        }
        mbid
    }

    /// Un artista vicino con cento brani in libreria non ne porta cento.
    ///
    /// Prima ogni brano dell'artista vicino diventava una riga, per ognuno dei
    /// venti artisti tenuti e per ogni brano dello stesso artista: su una
    /// libreria vera milioni di righe, contro i venti per brano che
    /// [`VICINI_TENUTI`] dichiara.
    #[test]
    fn un_artista_vicino_non_porta_piu_del_tetto() {
        let mut connection = libreria();
        let altro = artista_con_brani(&connection, 7, 100);
        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Trovato {
                track_id: 1,
                vicini: Vec::new(),
                artisti: vec![(altro, 0.7)],
            }],
            1_000,
        )
        .expect("registrazione");
        tx.commit().expect("commit");
        let conto = quanti_vicini(&connection).expect("conteggio");
        assert!(
            conto.get(&1).copied().unwrap_or(0) <= VICINI_TENUTI,
            "{conto:?}"
        );
    }

    /// Le due fonti insieme non passano il tetto per brano, e la registrazione
    /// viene prima: l'artista riempie i posti che lascia.
    #[test]
    fn le_due_fonti_insieme_stanno_nel_tetto() {
        let mut connection = libreria();
        let artisti: Vec<(String, f64)> = (10..20)
            .map(|numero| (artista_con_brani(&connection, numero, 10), 0.5))
            .collect();
        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Trovato {
                track_id: 1,
                vicini: vec![
                    ("bbbbbbbb-0000-0000-0000-000000000002".to_owned(), 1.0),
                    ("bbbbbbbb-0000-0000-0000-000000000003".to_owned(), 0.9),
                ],
                artisti,
            }],
            1_000,
        )
        .expect("registrazione");
        tx.commit().expect("commit");
        let per_fonte = |fonte: &str| -> usize {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM brano_vicino WHERE track_id = 1 AND fonte = ?1",
                    [fonte],
                    |r| r.get::<_, i64>(0),
                )
                .map(|n| usize::try_from(n).unwrap_or(0))
                .expect("conteggio")
        };
        assert_eq!(per_fonte("lb-registrazione"), 2);
        assert_eq!(
            per_fonte("lb-registrazione") + per_fonte("lb-artista"),
            VICINI_TENUTI
        );
    }

    /// Un brano non si propone mai come vicino di se stesso.
    #[test]
    fn nessuno_e_vicino_di_se_stesso() {
        let mut connection = libreria();
        let trovati = vec![Trovato {
            track_id: 1,
            vicini: vec![("bbbbbbbb-0000-0000-0000-000000000001".to_owned(), 1.0)],
            artisti: vec![("aaaaaaaa-0000-0000-0000-000000000001".to_owned(), 1.0)],
        }];
        let tx = connection.transaction().expect("transazione");
        registra(&tx, &trovati, 1_000).expect("registrazione");
        tx.commit().expect("commit");
        let conto = quanti_vicini(&connection).expect("conteggio");
        // L'artista di sé porta il brano 2 (stesso artista), mai il brano 1.
        assert_eq!(conto.get(&1), Some(&1));
    }

    /// Chiedere e non trovare niente si ricorda, o si richiede per sempre.
    #[test]
    fn un_brano_senza_vicini_non_si_richiede_domani() {
        let mut connection = libreria();
        let prima = candidati(&connection, 10_000, LOTTO).expect("candidati");
        assert_eq!(prima.len(), 4, "tutti e quattro hanno un identificativo");

        let trovati = vec![Trovato {
            track_id: 1,
            vicini: Vec::new(),
            artisti: Vec::new(),
        }];
        let tx = connection.transaction().expect("transazione");
        let scritte = registra(&tx, &trovati, 10_000).expect("registrazione");
        tx.commit().expect("commit");
        assert_eq!(scritte, 0);

        let dopo = candidati(&connection, 10_000, LOTTO).expect("candidati");
        assert!(
            !dopo.iter().any(|c| c.track_id == 1),
            "ha richiesto un brano su cui aveva già avuto una risposta vuota"
        );
        assert_eq!(dopo.len(), 3);
    }

    /// Passati i tre mesi si torna a chiedere.
    #[test]
    fn dopo_il_tempo_si_richiede() {
        let mut connection = libreria();
        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Trovato {
                track_id: 1,
                vicini: Vec::new(),
                artisti: Vec::new(),
            }],
            10_000,
        )
        .expect("registrazione");
        tx.commit().expect("commit");

        let tardi = 10_000 + RICHIEDI_MS + 1;
        let dopo = candidati(&connection, tardi, LOTTO).expect("candidati");
        assert!(
            dopo.iter().any(|c| c.track_id == 1),
            "non è mai tornato a chiedere"
        );
    }

    /// Una risposta nuova sostituisce la vecchia invece di sommarcisi.
    #[test]
    fn una_risposta_nuova_prende_il_posto_di_quella_di_prima() {
        let mut connection = libreria();
        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Trovato {
                track_id: 1,
                vicini: vec![("bbbbbbbb-0000-0000-0000-000000000002".to_owned(), 1.0)],
                artisti: Vec::new(),
            }],
            1_000,
        )
        .expect("prima");
        tx.commit().expect("commit");

        let tx = connection.transaction().expect("transazione");
        registra(
            &tx,
            &[Trovato {
                track_id: 1,
                vicini: vec![("bbbbbbbb-0000-0000-0000-000000000003".to_owned(), 1.0)],
                artisti: Vec::new(),
            }],
            2_000,
        )
        .expect("seconda");
        tx.commit().expect("commit");

        let conto = quanti_vicini(&connection).expect("conteggio");
        assert_eq!(
            conto.get(&1),
            Some(&1),
            "i vicini di ieri si sono sommati a quelli di oggi"
        );
        let vicino: i64 = connection
            .query_row(
                "SELECT vicino_id FROM brano_vicino WHERE track_id = 1",
                [],
                |r| r.get(0),
            )
            .expect("il vicino");
        assert_eq!(vicino, 3, "è rimasto il vicino di ieri");
    }

    /// Un brano senza nessuno dei due identificativi non è un candidato.
    #[test]
    fn senza_identificativi_non_si_chiede_niente() {
        let connection = crate::db::open_in_memory().expect("database").connection;
        connection
            .execute_batch(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, album_key,
                      duration_ms, file_size, date_added, date_modified)
                 VALUES (1, 'a.mp3', 'k1', 'Uno', 'Art', 'P', 'art|p', 1000, 1, 1, 1);",
            )
            .expect("brano");
        assert!(
            candidati(&connection, 10_000, LOTTO)
                .expect("candidati")
                .is_empty()
        );
        assert_eq!(quanti_mancano(&connection, 10_000).expect("conto"), 0);
    }

    /// Un punteggio fuori scala si serra invece di far fallire la transazione.
    #[test]
    fn un_punteggio_fuori_scala_non_fa_cadere_il_lotto() {
        let mut connection = libreria();
        let tx = connection.transaction().expect("transazione");
        let esito = registra(
            &tx,
            &[Trovato {
                track_id: 1,
                vicini: vec![("bbbbbbbb-0000-0000-0000-000000000002".to_owned(), 42.0)],
                artisti: Vec::new(),
            }],
            1_000,
        );
        assert!(
            esito.is_ok(),
            "il CHECK dello schema ha fatto cadere il lotto"
        );
        tx.commit().expect("commit");
        let punteggio: f64 = connection
            .query_row(
                "SELECT punteggio FROM brano_vicino WHERE track_id = 1",
                [],
                |r| r.get(0),
            )
            .expect("punteggio");
        assert!((punteggio - 1.0).abs() < 1e-9);
    }
}
