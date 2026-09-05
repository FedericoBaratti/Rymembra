//! Le raccolte del lunedì: la parte che tocca il database.
//!
//! La matematica sta in [`aether_domain::settimana`] e non sa niente di SQL:
//! quanto vale un ripescaggio, quando comincia il lunedì di chi guarda, come si
//! dividono dei punti in gruppi coerenti. Qui c'è il resto — da dove arrivano
//! quei numeri, dove finiscono le raccolte, e quando si buttano.
//!
//! # Le due raccolte, e la terza che manca
//!
//! **Ripescati** è quella che nessun servizio in streaming può scrivere: brani
//! che hai amato e non senti da mesi, pesati sull'affetto passato. Per farla
//! bisogna sapere cosa **possiedi** e da quanto non lo tocchi, e chi ti affitta
//! cento milioni di brani non possiede la domanda.
//!
//! **Ancora** sono gruppi coerenti trovati sulle impronte sonore, battezzati
//! con quello che contengono davvero. Dipende dall'analisi: su una libreria
//! appena scansionata non ci sono, e non è un guasto — è che non c'è ancora
//! niente da raggruppare.
//!
//! **Fuori** — musica nuova dai cataloghi liberi — non c'è, e vale la pena dire
//! perché invece di lasciarlo scoprire: `aether_catalogo` sa rispondere a «hai
//! questo preciso brano?», non a «cosa somiglia a questo». Serve una ricerca
//! per somiglianza in un catalogo esterno, che è una superficie di rete sua e
//! non un pezzo di questo modulo. Finché non c'è, il `CHECK` della migrazione
//! 18 non ne ammette nemmeno il nome.
//!
//! # Perché si scrive una volta e poi non si ricalcola
//!
//! Perché **Ripescati** guarda `last_played_at`, e `last_played_at` cambia
//! proprio ascoltando la raccolta: una raccolta ricalcolata a ogni apertura si
//! consumerebbe mentre la si usa. È scritto per esteso in cima alla migrazione
//! 18, con l'altra metà del ragionamento.
//!
//! # Perché la generazione sta in una transazione sola
//!
//! Perché due raccolte scritte a metà sono peggio di nessuna raccolta: il
//! ripiano direbbe «c'è qualcosa di nuovo» e aprirebbe una lista vuota. Si
//! legge, si calcola e si scrive in un colpo; se qualcosa va storto il lunedì
//! semplicemente non è ancora arrivato, e al prossimo avvio ci si riprova.
//!
//! # L'ordine dei lucchetti
//!
//! Questo modulo non ne prende nessuno: riceve una `Connection` o una
//! `Transaction` da chi l'ha già preso. Chi chiama [`genera`] tiene il lucchetto
//! della libreria per tutta la durata del calcolo — che su una libreria di
//! cinquantamila brani sono decine di millisecondi, perché il grosso è
//! aritmetica in memoria e non I/O — e lo fa una volta a settimana.

use std::collections::HashMap;

use aether_domain::errors::AppError;
use aether_domain::settimana::{
    GIORNO_MS, OBLIO_MINIMO_GIORNI, Punto, SETTIMANA_MS, Trascurato, raccogli, ripescaggio,
};
use aether_play::impronta::RAGGRUPPAMENTO;
use rusqlite::{Connection, Transaction};

use crate::library::db_error;
use crate::sonora;

/// Quanti brani in una raccolta.
///
/// Venti: un'ora e un quarto di musica, che è la lunghezza di un tragitto o di
/// una sessione di lavoro. Dodici — la misura dei ripiani della Home — sarebbe
/// giusta per una cosa che si **guarda**; questa si mette su e si lascia andare.
pub const QUANTI: usize = 20;

/// Sotto quanti brani una raccolta non si pubblica.
///
/// Otto. Una raccolta di tre brani non è un appuntamento, è un ripiano vuoto con
/// una decorazione sopra: meglio non annunciare niente. Vale solo per
/// **Ripescati**, che si accorcia con quel che la cronologia offre; le raccolte
/// **Ancora** hanno già la lunghezza fissa che
/// [`aether_domain::settimana::raccogli`] garantisce.
pub const MINIMO: usize = 8;

/// Quante raccolte «Ancora» si generano.
///
/// Tre, come i Daily Mix. Quattro sarebbero già una schermata da scorrere, e una
/// raccolta che non si guarda è la stessa cosa di una raccolta che non c'è.
pub const ANCORA: usize = 3;

/// Quante settimane restano prima di essere buttate.
///
/// Quattro. Servono a due cose: non ripetere la settimana appena passata, e
/// lasciare che chi apre il programma dopo dieci giorni ritrovi la raccolta di
/// cui si ricordava. Oltre il mese è archivio di qualcosa che nessuno ha chiesto
/// di archiviare.
pub const SETTIMANE_TENUTE: i64 = 4;

/// Quanti candidati al ripescaggio si leggono prima di ordinarli.
///
/// È la forma di casa: si recupera con SQL — i più fermi, che l'indice su
/// `last_played_at` serve già — e si riordina in memoria col punteggio, che
/// nessun `ORDER BY` sa esprimere. Duemila righe sono qualche centinaio di
/// microsecondi di aritmetica.
pub const CANDIDATI: usize = 2_000;

/// Quante impronte al massimo entrano nel raggruppamento.
///
/// Duecentomila, che è il limite oltre il quale la memoria comincia a contare —
/// non una taratura. Vedi [`crate::sonora::tutte`] per l'aritmetica.
pub const TETTO_PUNTI: usize = 200_000;

/// Quanto deve pesare il genere o l'artista più frequente perché dia il nome.
///
/// Metà. Sotto, il nome sarebbe la descrizione della minoranza più grande — che
/// è il modo di sbagliare a cui «Mix 3» è preferibile.
pub const MAGGIORANZA: f32 = 0.5;

/// Il nome della raccolta dei ripescati, come sta nel database.
pub const RIPESCATI: &str = "ripescati";

/// Il nome delle raccolte per somiglianza, come sta nel database.
pub const SOMIGLIANTI: &str = "ancora";

/// Una raccolta della settimana, coi suoi brani.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raccolta {
    /// La riga a cui appartiene, per marcarla aperta.
    pub id: i64,
    /// [`RIPESCATI`] o [`SOMIGLIANTI`].
    pub genere: String,
    /// Quale delle «Ancora» è. Zero per i ripescati.
    pub ordine: i64,
    /// Il materiale con cui si compone il nome: un genere, un artista, o niente.
    pub etichetta: Option<String>,
    /// Come si legge [`Raccolta::etichetta`]: `genere` o `artista`.
    pub etichetta_tipo: Option<String>,
    /// Se è già stata aperta almeno una volta.
    pub aperta: bool,
    /// I brani, nell'ordine deciso dal calcolo.
    pub brani: Vec<i64>,
}

/// Genera le raccolte di un lunedì, se non ci sono già.
///
/// Restituisce quante ne ha scritte: zero quando c'erano già, e zero quando la
/// libreria non ha ancora abbastanza da dire — che non è un errore, è il primo
/// giorno.
///
/// # Errori
///
/// `db.queryFailed` o `db.writeFailed` se il database non risponde.
pub fn genera(tx: &Transaction<'_>, lunedi: i64, adesso: i64) -> Result<usize, AppError> {
    if gia_generate(tx, lunedi)? {
        return Ok(0);
    }

    // Quel che è uscito lunedì scorso non esce di nuovo. Solo la settimana prima
    // e non tutte e quattro: su una libreria piccola escludere un mese intero
    // vorrebbe dire non avere più niente da proporre — e la promessa è «qualcosa
    // di diverso da sette giorni fa», non «mai più lo stesso brano».
    let visti = della_scorsa(tx, lunedi.saturating_sub(SETTIMANA_MS))?;

    let mut scritte: usize = 0;
    if let Some(brani) = ripescati(tx, lunedi, &visti)? {
        scrivi(tx, lunedi, RIPESCATI, 0, None, &brani, adesso)?;
        scritte = scritte.saturating_add(1);
    }
    for (ordine, brani) in somiglianti(tx, lunedi, &visti)?.into_iter().enumerate() {
        let nome = battezza(tx, &brani)?;
        let ordine = i64::try_from(ordine).unwrap_or(0);
        scrivi(tx, lunedi, SOMIGLIANTI, ordine, nome, &brani, adesso)?;
        scritte = scritte.saturating_add(1);
    }

    potatura(tx, lunedi)?;
    // Nella stessa transazione di quel che ha appena scritto: o si è generato
    // il lunedì e si è segnato, o non è successo niente delle due.
    crate::settings::write(tx, CHIAVE_ULTIMO, &lunedi.to_string())?;
    Ok(scritte)
}

/// Genera se serve e restituisce le raccolte del lunedì, in un colpo solo.
///
/// È la porta che usa l'interfaccia: la transazione si apre e si chiude qui
/// dentro, così chi chiama non ha bisogno di sapere che ce n'è una — e non ha
/// bisogno di un modo suo per raccontare un guasto di SQLite.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn aggiorna(
    connection: &mut Connection,
    lunedi: i64,
    adesso: i64,
) -> Result<Vec<Raccolta>, AppError> {
    // La domanda si fa **prima** di aprire la transazione, e questa è la riga
    // che decide il costo del caso normale. Entrare nella Home succede decine
    // di volte al giorno; generare, una a settimana. Aprendo la transazione per
    // prima, le altre decine pagavano comunque una scrittura — che su SQLite in
    // WAL vuol dire prendere il lucchetto dello scrittore e tenerlo per tutto
    // il tempo della domanda, mentre chi chiama tiene già quello della libreria.
    if !gia_generate(connection, lunedi)? {
        let tx = connection
            .transaction()
            .map_err(|err| db_error("apertura della settimana", &err))?;
        genera(&tx, lunedi, adesso)?;
        tx.commit()
            .map_err(|err| db_error("chiusura della settimana", &err))?;
    }
    della_settimana(connection, lunedi)
}

/// Le raccolte di un lunedì, coi loro brani.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn della_settimana(connection: &Connection, lunedi: i64) -> Result<Vec<Raccolta>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, genere, ordine, etichetta, etichetta_tipo, aperta_at
             FROM settimana_raccolta
             WHERE lunedi = ?1
             ORDER BY genere DESC, ordine",
        )
        .map_err(|err| db_error("lettura delle raccolte della settimana", &err))?;
    let righe = statement
        .query_map([lunedi], |row| {
            Ok(Raccolta {
                id: row.get(0)?,
                genere: row.get(1)?,
                ordine: row.get(2)?,
                etichetta: row.get(3)?,
                etichetta_tipo: row.get(4)?,
                aperta: row.get::<_, Option<i64>>(5)?.is_some(),
                brani: Vec::new(),
            })
        })
        .map_err(|err| db_error("lettura delle raccolte della settimana", &err))?;

    let mut fuori = Vec::new();
    for riga in righe {
        fuori.push(riga.map_err(|err| db_error("lettura delle raccolte della settimana", &err))?);
    }
    for raccolta in &mut fuori {
        raccolta.brani = brani(connection, raccolta.id)?;
    }
    // Una raccolta i cui brani sono stati tutti cancellati dalla libreria non è
    // una raccolta vuota da mostrare: è una raccolta che non c'è più.
    fuori.retain(|raccolta| !raccolta.brani.is_empty());
    Ok(fuori)
}

/// I brani di una raccolta, nell'ordine deciso dal calcolo.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn brani(connection: &Connection, raccolta: i64) -> Result<Vec<i64>, AppError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT track_id FROM settimana_brano
             WHERE raccolta_id = ?1
             ORDER BY posizione",
        )
        .map_err(|err| db_error("lettura dei brani di una raccolta", &err))?;
    let righe = statement
        .query_map([raccolta], |row| row.get::<_, i64>(0))
        .map_err(|err| db_error("lettura dei brani di una raccolta", &err))?;
    let mut fuori = Vec::new();
    for riga in righe {
        fuori.push(riga.map_err(|err| db_error("lettura dei brani di una raccolta", &err))?);
    }
    Ok(fuori)
}

/// Segna una raccolta come aperta. La prima volta vince: riaprirla non sposta
/// la data, o «nuovo» vorrebbe dire «non aperto oggi» invece di «mai aperto».
///
/// # Errori
///
/// `db.writeFailed` se il database non risponde.
pub fn apri(connection: &Connection, raccolta: i64, adesso: i64) -> Result<(), AppError> {
    connection
        .execute(
            "UPDATE settimana_raccolta SET aperta_at = ?2
             WHERE id = ?1 AND aperta_at IS NULL",
            rusqlite::params![raccolta, adesso],
        )
        .map_err(|err| db_error("apertura di una raccolta", &err))?;
    Ok(())
}

/// La chiave in `settings` che ricorda l'ultimo lunedì generato.
///
/// # Perché non basta contare le righe
///
/// Perché generare e **scrivere** non sono la stessa cosa. Su una libreria che
/// non ha ancora abbastanza da dire — nessun brano fermo da due mesi, nessuna
/// scala sonora, o una scala che non produce gruppi — il calcolo gira per
/// intero e non scrive niente. Contando le righe, quel lunedì resta per sempre
/// «da fare»: la generazione completa si rifà a **ogni** apertura della Home,
/// e ognuna carica fino a [`TETTO_PUNTI`] vettori e ci passa sopra tre volte.
///
/// Con una chiave, «ho girato» si registra anche quando il risultato è vuoto,
/// che è precisamente il caso in cui costa di più rifarlo.
const CHIAVE_ULTIMO: &str = "settimana.ultimo_lunedi";

/// Se il lunedì è già stato generato.
///
/// Le righe si contano lo stesso, e non per ridondanza: i database generati
/// prima che questa chiave esistesse hanno le raccolte e non hanno la chiave, e
/// senza il conteggio se le rifarebbero tutte una volta. Costa una `COUNT` su
/// un indice, una volta per apertura della Home.
fn gia_generate(connection: &Connection, lunedi: i64) -> Result<bool, AppError> {
    if crate::settings::read(connection, CHIAVE_ULTIMO)?
        .and_then(|v| v.parse::<i64>().ok())
        .is_some_and(|ultimo| ultimo == lunedi)
    {
        return Ok(true);
    }
    let quante: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM settimana_raccolta WHERE lunedi = ?1",
            [lunedi],
            |row| row.get(0),
        )
        .map_err(|err| db_error("conteggio delle raccolte della settimana", &err))?;
    Ok(quante > 0)
}

/// I brani già usciti da un certo lunedì in poi.
fn della_scorsa(tx: &Transaction<'_>, da: i64) -> Result<Vec<i64>, AppError> {
    let mut statement = tx
        .prepare(
            "SELECT b.track_id FROM settimana_brano b
             JOIN settimana_raccolta r ON r.id = b.raccolta_id
             WHERE r.lunedi >= ?1",
        )
        .map_err(|err| db_error("lettura delle raccolte passate", &err))?;
    let righe = statement
        .query_map([da], |row| row.get::<_, i64>(0))
        .map_err(|err| db_error("lettura delle raccolte passate", &err))?;
    let mut fuori = Vec::new();
    for riga in righe {
        fuori.push(riga.map_err(|err| db_error("lettura delle raccolte passate", &err))?);
    }
    Ok(fuori)
}

/// I ripescati di questa settimana, o `None` se non ce ne sono abbastanza.
fn ripescati(
    tx: &Transaction<'_>,
    lunedi: i64,
    visti: &[i64],
) -> Result<Option<Vec<i64>>, AppError> {
    // Il taglio in SQL è la sola parte della domanda che un indice sa servire:
    // «fermi da almeno due mesi», dal più fermo. Il punteggio — che pesa
    // l'affetto contro l'oblio — non è un `ORDER BY`, e si applica dopo.
    // Sessanta giorni, che è una costante scritta a mano in virgola mobile
    // perché il dominio la usa per dividere. Qui serve come intero, e il
    // troncamento è esattamente ciò che si vuole: la soglia SQL taglia un po'
    // più larga della curva dell'oblio, e un candidato in più che poi prende
    // punteggio zero è preferibile a un candidato in meno che non si vede.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "troncamento voluto e discusso qui sopra"
    )]
    let giorni = OBLIO_MINIMO_GIORNI.trunc() as i64;
    let soglia = lunedi.saturating_sub(giorni.saturating_mul(GIORNO_MS));
    let mut statement = tx
        .prepare(
            "SELECT id, play_count, last_played_at, liked FROM tracks
             WHERE play_count > 0 AND last_played_at IS NOT NULL AND last_played_at < ?1
             ORDER BY last_played_at ASC
             LIMIT ?2",
        )
        .map_err(|err| db_error("candidati al ripescaggio", &err))?;
    let righe = statement
        .query_map(
            rusqlite::params![soglia, i64::try_from(CANDIDATI).unwrap_or(i64::MAX)],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .map_err(|err| db_error("candidati al ripescaggio", &err))?;

    let mut punteggi: Vec<(i64, f32)> = Vec::new();
    for riga in righe {
        let (id, ascolti, ultimo, cuore) =
            riga.map_err(|err| db_error("candidati al ripescaggio", &err))?;
        if visti.contains(&id) {
            continue;
        }
        let brano = Trascurato {
            ascolti: u32::try_from(ascolti).unwrap_or(u32::MAX),
            da_quanto_ms: Some(lunedi.saturating_sub(ultimo)),
            preferito: cuore != 0,
        };
        let punteggio = ripescaggio(&brano, lunedi);
        if punteggio > 0.0 {
            punteggi.push((id, punteggio));
        }
    }
    if punteggi.len() < MINIMO {
        return Ok(None);
    }
    // A pari punteggio decide l'identificativo, non l'ordine in cui SQLite ha
    // restituito le righe: due generazioni della stessa settimana devono dare la
    // stessa raccolta.
    punteggi.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    punteggi.truncate(QUANTI);
    Ok(Some(punteggi.into_iter().map(|(id, _)| id).collect()))
}

/// Le raccolte per somiglianza, o niente se non c'è ancora da raggruppare.
fn somiglianti(
    tx: &Transaction<'_>,
    lunedi: i64,
    visti: &[i64],
) -> Result<Vec<Vec<i64>>, AppError> {
    let Some(scala) = sonora::scala(tx)? else {
        // Nessuna scala vuol dire che l'analisi non ha ancora coperto abbastanza
        // libreria. Non è un guasto: è che manca il materiale.
        return Ok(Vec::new());
    };
    let punti: Vec<Punto> = sonora::tutte(tx, TETTO_PUNTI)?
        .into_iter()
        .filter(|(id, _)| !visti.contains(id))
        .filter_map(|(id, vettore)| {
            scala
                .normalizza(&vettore)
                .map(|vettore| Punto { id, vettore })
        })
        .collect();

    // Il seme di partenza viene dal lunedì e da nient'altro: è tutto ciò che
    // serve perché la settimana prossima le raccolte siano altre, senza tenere
    // da nessuna parte quelle di questa. Vedi `raccogli`.
    let da_dove = usize::try_from(lunedi.div_euclid(SETTIMANA_MS).unsigned_abs()).unwrap_or(0);
    Ok(raccogli(&punti, &RAGGRUPPAMENTO, ANCORA, QUANTI, da_dove))
}

/// Il genere o l'artista che descrive un gruppo, se ce n'è uno.
fn battezza(
    tx: &Transaction<'_>,
    brani: &[i64],
) -> Result<Option<(String, &'static str)>, AppError> {
    let mut statement = tx
        .prepare_cached("SELECT genre, artist FROM tracks WHERE id = ?1")
        .map_err(|err| db_error("lettura del genere di un brano", &err))?;

    let mut generi: HashMap<String, usize> = HashMap::new();
    let mut artisti: HashMap<String, usize> = HashMap::new();
    for id in brani {
        let letto: Option<(Option<String>, String)> = statement
            .query_row([id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map(Some)
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                altro => Err(db_error("lettura del genere di un brano", &altro)),
            })?;
        let Some((genere, artista)) = letto else {
            continue;
        };
        if let Some(genere) = genere.filter(|testo| !testo.trim().is_empty()) {
            *generi.entry(genere).or_insert(0) += 1;
        }
        if !artista.trim().is_empty() {
            *artisti.entry(artista).or_insert(0) += 1;
        }
    }

    // Il genere prima dell'artista: «Jazz» descrive una raccolta, «Miles Davis»
    // descrive un disco. L'artista è il ripiego di quando i tag di genere non ci
    // sono — che su una libreria vera è metà delle volte.
    let quanti = brani.len();
    if let Some(nome) = maggioranza(&generi, quanti) {
        return Ok(Some((nome, "genere")));
    }
    Ok(maggioranza(&artisti, quanti).map(|nome| (nome, "artista")))
}

/// Il nome più frequente, se copre almeno [`MAGGIORANZA`] del gruppo.
fn maggioranza(conteggi: &HashMap<String, usize>, quanti: usize) -> Option<String> {
    if quanti == 0 {
        return None;
    }
    // A pari conteggio decide il nome in ordine alfabetico e non l'ordine della
    // mappa, che fra due esecuzioni non è lo stesso.
    let (nome, volte) = conteggi
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))?;
    let quota = *volte as f32 / quanti as f32;
    (quota >= MAGGIORANZA).then(|| nome.clone())
}

/// Scrive una raccolta e i suoi brani.
fn scrivi(
    tx: &Transaction<'_>,
    lunedi: i64,
    genere: &str,
    ordine: i64,
    nome: Option<(String, &'static str)>,
    brani: &[i64],
    adesso: i64,
) -> Result<(), AppError> {
    let (etichetta, tipo) = match nome {
        Some((etichetta, tipo)) => (Some(etichetta), Some(tipo)),
        None => (None, None),
    };
    tx.execute(
        "INSERT INTO settimana_raccolta
           (lunedi, genere, ordine, etichetta, etichetta_tipo, generata_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![lunedi, genere, ordine, etichetta, tipo, adesso],
    )
    .map_err(|err| db_error("scrittura di una raccolta", &err))?;
    let raccolta = tx.last_insert_rowid();

    let mut statement = tx
        .prepare_cached(
            "INSERT INTO settimana_brano (raccolta_id, track_id, posizione)
             VALUES (?1, ?2, ?3)",
        )
        .map_err(|err| db_error("scrittura dei brani di una raccolta", &err))?;
    for (posizione, brano) in brani.iter().enumerate() {
        statement
            .execute(rusqlite::params![
                raccolta,
                brano,
                i64::try_from(posizione).unwrap_or(0)
            ])
            .map_err(|err| db_error("scrittura dei brani di una raccolta", &err))?;
    }
    Ok(())
}

/// Butta le settimane troppo vecchie.
fn potatura(tx: &Transaction<'_>, lunedi: i64) -> Result<(), AppError> {
    let limite = lunedi.saturating_sub(SETTIMANE_TENUTE.saturating_mul(SETTIMANA_MS));
    tx.execute("DELETE FROM settimana_raccolta WHERE lunedi < ?1", [limite])
        .map_err(|err| db_error("potatura delle raccolte vecchie", &err))?;
    Ok(())
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::settimana::lunedi;

    /// Il lunedì 5 gennaio 1970, che è comodo perché piccolo e vero.
    const LUNEDI: i64 = 4 * GIORNO_MS;

    /// Una libreria vuota, con lo schema aggiornato.
    fn libreria() -> Connection {
        crate::db::open_in_memory().expect("database").connection
    }

    /// Aggiunge `quanti` brani, tutti dello stesso artista e genere.
    fn brani_di(connection: &Connection, da: i64, quanti: i64, artista: &str, genere: &str) {
        for i in 0..quanti {
            let id = da + i;
            connection
                .execute(
                    "INSERT INTO tracks
                       (id, path, track_key, title, artist, album, album_key, genre,
                        duration_ms, file_size, date_added, date_modified)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'Disco', 'disco', ?6, 200000, 1, 1, 1)",
                    rusqlite::params![
                        id,
                        format!("{artista}-{id}.flac"),
                        format!("k{id}"),
                        format!("Brano {id}"),
                        artista,
                        genere,
                    ],
                )
                .expect("brano");
        }
    }

    /// Segna un brano come ascoltato `volte` volte, l'ultima `giorni_fa`.
    fn ascoltato(connection: &Connection, id: i64, volte: i64, giorni_fa: i64, cuore: bool) {
        connection
            .execute(
                "UPDATE tracks SET play_count = ?2, last_played_at = ?3, liked = ?4
                 WHERE id = ?1",
                rusqlite::params![id, volte, LUNEDI - giorni_fa * GIORNO_MS, i64::from(cuore)],
            )
            .expect("ascolto");
    }

    /// Genera le raccolte di un lunedì e restituisce quante ne ha scritte.
    fn genera_a(connection: &mut Connection, quando: i64) -> usize {
        let tx = connection.transaction().expect("transazione");
        let scritte = genera(&tx, quando, quando).expect("generazione");
        tx.commit().expect("commit");
        scritte
    }

    /// Tutti i brani usciti in un lunedì, di qualunque raccolta.
    fn usciti(connection: &Connection, quando: i64) -> Vec<i64> {
        let mut fuori: Vec<i64> = della_settimana(connection, quando)
            .expect("lettura")
            .into_iter()
            .flat_map(|raccolta| raccolta.brani)
            .collect();
        fuori.sort_unstable();
        fuori
    }

    /// Una libreria che nessuno ha mai ascoltato non produce niente.
    ///
    /// È il primo giorno, non un guasto: nessun ripescaggio perché non c'è
    /// niente da ripescare, e nessun raggruppamento perché l'analisi non è
    /// ancora passata.
    #[test]
    fn il_primo_giorno_non_c_e_niente_da_ripescare() {
        let mut connection = libreria();
        brani_di(&connection, 1, 30, "Art", "Rock");
        assert_eq!(genera_a(&mut connection, LUNEDI), 0);
        assert!(
            della_settimana(&connection, LUNEDI)
                .expect("lettura")
                .is_empty()
        );
    }

    /// Quel che hai amato e non senti da un anno torna.
    #[test]
    fn quel_che_non_senti_da_un_anno_torna() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 5, 400, false);
        }
        assert_eq!(genera_a(&mut connection, LUNEDI), 1);
        let raccolte = della_settimana(&connection, LUNEDI).expect("lettura");
        assert_eq!(raccolte.len(), 1);
        assert_eq!(raccolte.first().map(|r| r.genere.as_str()), Some(RIPESCATI));
        assert_eq!(raccolte.first().map(|r| r.brani.len()), Some(12));
    }

    /// Quel che hai sentito ieri non si ripesca, per quanto lo si sia amato.
    #[test]
    fn quel_che_hai_sentito_ieri_non_si_ripesca() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 50, 1, true);
        }
        assert_eq!(
            genera_a(&mut connection, LUNEDI),
            0,
            "ha ripescato brani ancora caldi"
        );
    }

    /// Meno di [`MINIMO`] brani non fanno una raccolta.
    #[test]
    fn una_manciata_di_brani_non_fa_una_raccolta() {
        let mut connection = libreria();
        let quanti = i64::try_from(MINIMO).unwrap_or(0) - 1;
        brani_di(&connection, 1, quanti, "Art", "Rock");
        for id in 1..=quanti {
            ascoltato(&connection, id, 5, 400, false);
        }
        assert_eq!(genera_a(&mut connection, LUNEDI), 0);
    }

    /// Il più amato e il più dimenticato esce per primo.
    #[test]
    fn il_piu_amato_esce_per_primo() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 1, 70, false);
        }
        // Il 7 è stato ascoltato molte volte, è un preferito, ed è fermo da più
        // tempo di tutti: vince su ogni asse.
        ascoltato(&connection, 7, 40, 400, true);
        genera_a(&mut connection, LUNEDI);
        let raccolte = della_settimana(&connection, LUNEDI).expect("lettura");
        assert_eq!(raccolte.first().and_then(|r| r.brani.first()), Some(&7));
    }

    /// Generare due volte lo stesso lunedì non raddoppia niente.
    ///
    /// È ciò che permette di provarci a ogni avvio senza pensarci.
    #[test]
    fn generare_due_volte_lo_stesso_lunedi_non_raddoppia() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 5, 400, false);
        }
        assert_eq!(genera_a(&mut connection, LUNEDI), 1);
        assert_eq!(genera_a(&mut connection, LUNEDI), 0);
        assert_eq!(
            della_settimana(&connection, LUNEDI).expect("lettura").len(),
            1
        );
    }

    /// Un lunedì che non ha prodotto niente resta comunque fatto.
    ///
    /// È il difetto per cui la Home rifaceva la generazione intera a ogni
    /// apertura. Contando le righe scritte, «zero raccolte» e «mai girato»
    /// erano indistinguibili — e su una libreria piccola, o su una a cui
    /// l'analisi non ha ancora dato una scala, zero raccolte è la normalità e
    /// non l'eccezione. Ogni ingresso nella Home ripagava per intero tre
    /// passate di raggruppamento su un carico che arriva a [`TETTO_PUNTI`]
    /// vettori, sul filo principale e col lucchetto della libreria in mano.
    #[test]
    fn un_lunedi_senza_raccolte_non_si_rigenera() {
        let mut connection = libreria();
        let quanti = i64::try_from(MINIMO).unwrap_or(0) - 1;
        brani_di(&connection, 1, quanti, "Art", "Rock");
        for id in 1..=quanti {
            ascoltato(&connection, id, 5, 400, false);
        }
        assert_eq!(genera_a(&mut connection, LUNEDI), 0, "niente da scrivere");
        assert!(
            gia_generate(&connection, LUNEDI).expect("controllo"),
            "girato a vuoto è girato: rifarlo costerebbe il calcolo intero"
        );
    }

    /// Un database generato prima che la chiave esistesse non si rifà tutto.
    ///
    /// Chi aggiorna arriva con le raccolte in tabella e senza
    /// `settimana.ultimo_lunedi`: il conteggio delle righe è quel che glielo
    /// riconosce, ed è l'unica ragione per cui è rimasto.
    #[test]
    fn le_raccolte_gia_scritte_valgono_senza_la_chiave() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 5, 400, false);
        }
        assert_eq!(genera_a(&mut connection, LUNEDI), 1);
        crate::settings::forget(&connection, CHIAVE_ULTIMO).expect("cancellazione");
        assert!(
            gia_generate(&connection, LUNEDI).expect("controllo"),
            "le righe ci sono: il lunedì è fatto anche senza la chiave"
        );
    }

    /// Il lunedì dopo non eredita il segno di quello prima.
    #[test]
    fn la_chiave_vale_per_un_lunedi_solo() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 5, 400, false);
        }
        genera_a(&mut connection, LUNEDI);
        assert!(
            !gia_generate(&connection, LUNEDI + SETTIMANA_MS).expect("controllo"),
            "un lunedì fatto non fa il successivo"
        );
    }

    /// La settimana dopo non ripete quella prima.
    #[test]
    fn la_settimana_dopo_e_un_altra() {
        let mut connection = libreria();
        brani_di(&connection, 1, 40, "Art", "Rock");
        for id in 1..=40 {
            ascoltato(&connection, id, 5, 400, false);
        }
        genera_a(&mut connection, LUNEDI);
        genera_a(&mut connection, LUNEDI + SETTIMANA_MS);

        let prima = usciti(&connection, LUNEDI);
        let dopo = usciti(&connection, LUNEDI + SETTIMANA_MS);
        assert_eq!(prima.len(), QUANTI);
        assert_eq!(dopo.len(), QUANTI);
        assert!(
            prima.iter().all(|id| !dopo.contains(id)),
            "la settimana nuova ripete quella vecchia"
        );
    }

    /// Le settimane troppo vecchie si buttano da sole.
    #[test]
    fn le_settimane_vecchie_si_buttano() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 5, 400, false);
        }
        genera_a(&mut connection, LUNEDI);
        assert_eq!(usciti(&connection, LUNEDI).len(), 12);

        // Cinque settimane dopo: la prima è fuori dalle quattro che si tengono.
        genera_a(&mut connection, LUNEDI + 5 * SETTIMANA_MS);
        assert!(
            usciti(&connection, LUNEDI).is_empty(),
            "la settimana vecchia è ancora lì"
        );
    }

    /// La prima apertura vince: riaprire non sposta la data.
    #[test]
    fn la_prima_apertura_vince() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 5, 400, false);
        }
        genera_a(&mut connection, LUNEDI);
        let raccolte = della_settimana(&connection, LUNEDI).expect("lettura");
        let raccolta = raccolte.first().map(|r| r.id).expect("una raccolta");

        apri(&connection, raccolta, 111).expect("apertura");
        apri(&connection, raccolta, 222).expect("riapertura");
        let quando: i64 = connection
            .query_row(
                "SELECT aperta_at FROM settimana_raccolta WHERE id = ?1",
                [raccolta],
                |row| row.get(0),
            )
            .expect("data");
        assert_eq!(quando, 111);
        assert_eq!(
            della_settimana(&connection, LUNEDI)
                .expect("lettura")
                .first()
                .map(|r| r.aperta),
            Some(true)
        );
    }

    /// Una raccolta i cui brani sono spariti dalla libreria non si mostra.
    #[test]
    fn una_raccolta_svuotata_non_si_mostra() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 5, 400, false);
        }
        genera_a(&mut connection, LUNEDI);
        connection
            .execute("DELETE FROM tracks", [])
            .expect("pulizia");
        assert!(
            della_settimana(&connection, LUNEDI)
                .expect("lettura")
                .is_empty(),
            "mostra una raccolta senza brani"
        );
    }

    /// Il gruppo prende il nome dal genere che lo domina.
    #[test]
    fn il_nome_viene_da_quel_che_c_e_dentro() {
        let mut connection = libreria();
        brani_di(&connection, 1, 8, "Art", "Jazz");
        brani_di(&connection, 9, 2, "Altro", "Rock");
        let tx = connection.transaction().expect("transazione");
        let nome = battezza(&tx, &(1..=10).collect::<Vec<_>>()).expect("nome");
        assert_eq!(nome, Some(("Jazz".to_owned(), "genere")));
    }

    /// Senza un genere, il nome è l'artista.
    #[test]
    fn senza_genere_il_nome_e_l_artista() {
        let mut connection = libreria();
        brani_di(&connection, 1, 10, "Art", "");
        let tx = connection.transaction().expect("transazione");
        let nome = battezza(&tx, &(1..=10).collect::<Vec<_>>()).expect("nome");
        assert_eq!(nome, Some(("Art".to_owned(), "artista")));
    }

    /// Senza maggioranza, il gruppo resta senza nome.
    ///
    /// Nessun nome è preferibile a un nome che descrive due brani su dieci.
    #[test]
    fn senza_maggioranza_il_gruppo_resta_senza_nome() {
        let mut connection = libreria();
        for (i, genere) in ["A", "B", "C", "D", "E"].iter().enumerate() {
            let indice = i64::try_from(i).unwrap_or(0);
            brani_di(&connection, 1 + 2 * indice, 2, &format!("Art{i}"), genere);
        }
        let tx = connection.transaction().expect("transazione");
        let nome = battezza(&tx, &(1..=10).collect::<Vec<_>>()).expect("nome");
        assert_eq!(nome, None);
    }

    /// Il lunedì che si calcola è quello a cui si scrive.
    ///
    /// Prova di giunzione fra la matematica del dominio e queste tabelle: se le
    /// due idee di «lunedì» divergessero, la Home leggerebbe per sempre una
    /// settimana che nessuno ha mai generato.
    #[test]
    fn si_legge_lo_stesso_lunedi_che_si_e_scritto() {
        let mut connection = libreria();
        brani_di(&connection, 1, 12, "Art", "Rock");
        for id in 1..=12 {
            ascoltato(&connection, id, 5, 400, false);
        }
        // Un mercoledì qualunque, e il fuso di Roma d'inverno.
        let mercoledi = LUNEDI + 2 * GIORNO_MS + 15 * 60 * 60 * 1000;
        genera_a(&mut connection, lunedi(mercoledi, 60));
        assert_eq!(
            della_settimana(&connection, lunedi(mercoledi + GIORNO_MS, 60))
                .expect("lettura")
                .len(),
            1,
            "giovedì non trova quel che si è generato mercoledì"
        );
    }
}
