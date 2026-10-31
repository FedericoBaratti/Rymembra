//! I brani da guardare, e la parola dell'utente su di loro.
//!
//! # Cosa risolve
//!
//! Fin qui un brano con i metadati messi male non si distingueva da uno a
//! posto. «Artista sconosciuto» si vedeva, certo, ma un titolo mojibake, un
//! `Unknown Artist` scritto dal ripper e un album dedotto dalla cartella
//! avevano tutti l'aria di dati veri — e i file che non si leggevano proprio
//! non c'erano nemmeno, ridotti a un numero in fondo alla scansione.
//!
//! Con `tracks.meta_salute` la domanda «cosa devo guardare» ha una risposta che
//! costa un indice, e con `tracks.meta_origine` ha anche un **perché** per ogni
//! campo. Questo modulo li legge e li lascia correggere.
//!
//! # Quel che oggi non succede, e va detto
//!
//! Le due colonne le popola **solo questo modulo**, quando qualcuno risponde:
//! nessuna scansione le scrive. `library::read_track` calcola i ripieghi — il
//! titolo dal nome del file, «Artista sconosciuto», l'album dalla cartella — ma
//! non lascia detto da nessuna parte di averlo fatto, e `meta_salute` resta
//! `NULL` su ogni riga che nessuno ha ancora corretto.
//!
//! Vuol dire che l'elenco dei brani da guardare, oggi, è **vuoto per
//! costruzione**: l'infrastruttura c'è tutta — le colonne, l'indice, i comandi,
//! la vista — e le manca il solo lato che la riempirebbe. Sta scritto qui e non
//! altrove perché il resto di questo `//!` descrive un lavoro che le funzioni
//! qui sotto sanno fare per davvero, e senza questa riga si leggerebbe come la
//! descrizione di qualcosa che l'utente dovrebbe già vedere.
//!
//! # Perché la correzione non tocca i file
//!
//! Perché è reversibile a costo zero e perché non cambia la data di modifica —
//! che, cambiata, farebbe rileggere e riscrivere quel file a ogni scansione
//! successiva. Il file resta la fonte; la correzione si rimette sopra a ogni
//! passata, da [`crate::provenienza::Correzioni`] salvate in `track_overrides`.
//! A rimettercela è [`riapplica`], chiamata subito dopo ogni riscrittura della
//! riga che non sa niente di quella tabella: la scansione e l'arricchimento.
//!
//! Dalla 2.3.1 questa non è più l'eccezione: è la regola di tutta Rymembra.
//! L'arricchimento riscriveva i tag **dentro** i file dell'utente, ed era la
//! sola parte del programma a farlo da sola, ogni mezz'ora, senza che nessuno
//! guardasse prima. Adesso scrive in database anche lui, e annota in
//! `track_meta_arricchita` quali campi vengano di lì.
//!
//! # L'ordine di risoluzione, scritto una volta sola
//!
//! Tre strati, e questo è l'ordine in cui si coprono:
//!
//! ```text
//! tag grezzi   →   track_meta_arricchita   →   track_overrides
//!  quel che          quel che ha trovato          quel che ha
//!  dice il file      un catalogo                  deciso l'utente
//! ```
//!
//! Chi scrive ciascuno strato:
//!
//! * i **tag grezzi** li mette `library::update_track` dai valori che
//!   `read_track` ha letto dal file, a ogni scansione che vede quel file
//!   cambiato;
//! * i **metadati arricchiti** li mette `enrich::registra`, che nella stessa
//!   transazione annota in `track_meta_arricchita` quali campi vengono da lui
//!   e cosa diceva il file sotto ciascuno; dopo una rilettura del file li
//!   rimette [`arricchimento_sopra_i_tag`], da dentro `library::update_track`;
//! * la **correzione a mano** la rimette [`riapplica`], subito dopo l'uno e
//!   l'altro. È l'ultima parola, ed è la sola dei tre che non abbia una
//!   confidenza: le altre due sono letture, questa è una decisione.
//!
//! # Lo strato di mezzo dopo una rilettura, campo per campo
//!
//! Nel caso normale non c'è niente da rimettere: l'arricchimento non apre il
//! file, quindi `tracks.date_modified` continua a coincidere con quella del
//! disco, la scansione non rilegge niente e la riga arricchita resta com'è.
//!
//! Il caso che conta è quello in cui il file lo tocca **un altro programma**.
//! Fino alla 2.4 la regola era una sola, e diceva «vince il file»: la parola più
//! recente su un file è quella del file. Giusta per chi era appena andato a
//! ritaggarlo altrove, e sbagliata per tutti gli altri — un calcolatore di
//! ReplayGain, un lettore che ci scrive il conteggio degli ascolti, un riordino
//! che lo sposta di cartella. A nessuno di quelli interessano l'album o la
//! copertina, ma bastava che passassero perché tornassero «Album sconosciuto» e
//! la copertina grigia, e l'arricchimento non ci ripassava più, perché per lui
//! il brano era già fatto.
//!
//! Adesso la domanda si fa **campo per campo**: il file dice ancora, sotto quel
//! campo, quel che diceva quando l'arricchimento l'ha coperto? Se sì, la
//! corrispondenza vale ancora e si rimette. Se no, quel campo l'ha ritaggato
//! qualcuno, e vince il file: esce dall'annotazione, e il brano torna in coda
//! all'arricchimento, che lo riguarderà con i tag nuovi. Per saper rispondere
//! l'annotazione ricorda cosa c'era sotto — vedi `enrich::Sotto` — e le regole
//! per esteso stanno su [`arricchimento_sopra_i_tag`].
//!
//! [`crate::enrich::dimentica`] resta com'era: rilegge il file, e quel che
//! trova è per costruzione quel che c'era sotto.
//!
//! # Quel che una correzione non tocca: `tracks.content_key`
//!
//! Una correzione ricalcola `track_key`, e deve: è la chiave con cui gli altri
//! dispositivi ritrovano questo brano, e lasciarla sui valori sbagliati vorrebbe
//! dire cercarlo altrove sotto il nome che l'utente ha appena corretto.
//!
//! `content_key` no, mai. È l'identità del **file** — la calcolano
//! `library::insert_track` e `library::update_track` dai tag grezzi, e nessun
//! altro — e serve a `scan_plan::match_moved_tracks` per riconoscere un file
//! che si è spostato. Ricalcolarla da qui rimetterebbe esattamente il difetto
//! che la colonna esiste per chiudere: più correzioni si accumulano, meno il
//! brano è riconoscibile quando il suo file cambia cartella, finché la riga
//! viene cancellata e reinserita — e con `ON DELETE CASCADE` la correzione se
//! ne va insieme alla riga che la teneva.
//!
//! L'`UPDATE` di [`applica_correzioni`] elenca le colonne una per una, quindi
//! oggi già non la tocca. Questo commento c'è perché resti vero domani.

use rusqlite::{Connection, OptionalExtension as _, Transaction};
use rymembra_domain::album::album_group_key;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::keys::{TrackKey, TrackKeyInput};
use rymembra_domain::ricostruzione::{Origine, Salute};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization as _;

use crate::covers::CoverSource;
use crate::enrich::{TagDelFile, annotazione};
use crate::library::{
    Toccati, TrackRow, db_error, now_ms, ricostruisci_aggregati, transazione_di_scrittura,
};
use crate::provenienza::{Correzioni, Origini};

/// Un brano da guardare, con quel che serve a deciderlo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracciaIncerta {
    /// L'identificativo della riga.
    pub id: i64,
    /// Il percorso, che è l'unica cosa sempre vera di un brano messo male.
    pub path: String,
    /// Titolo.
    pub title: String,
    /// Interprete.
    pub artist: String,
    /// Album.
    pub album: String,
    /// Artista dell'album.
    pub album_artist: Option<String>,
    /// Genere.
    pub genre: Option<String>,
    /// Anno.
    pub year: Option<i64>,
    /// Numero di traccia.
    pub track_number: Option<i64>,
    /// Numero di disco.
    pub disc_number: Option<i64>,
    /// Durata in millisecondi. Zero su un brano degradato: non si è potuta leggere.
    pub duration_ms: i64,
    /// L'impronta della copertina, per mostrarla.
    pub cover_art_hash: Option<String>,
    /// `dedotto` o `degradato`.
    pub health: String,
    /// I nomi dei problemi, da `ricostruzione::Problema::as_str`.
    pub problems: Vec<String>,
    /// Da dove viene ogni campo, e com'era prima se è stato riparato.
    pub origins: Origini,
}

/// Le colonne che servono a una [`TracciaIncerta`], nell'ordine di lettura.
const COLONNE: &str = "id, path, title, artist, album, album_artist, genre, year,
     track_number, disc_number, duration_ms, cover_art_hash,
     meta_salute, meta_problemi, meta_origine";

fn traccia_da_riga(row: &rusqlite::Row<'_>) -> rusqlite::Result<TracciaIncerta> {
    let problemi: Option<String> = row.get(13)?;
    let origine: Option<String> = row.get(14)?;
    Ok(TracciaIncerta {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        album: row.get(4)?,
        album_artist: row.get(5)?,
        genre: row.get(6)?,
        year: row.get(7)?,
        track_number: row.get(8)?,
        disc_number: row.get(9)?,
        duration_ms: row.get(10)?,
        cover_art_hash: row.get(11)?,
        health: row.get(12)?,
        // Un JSON illeggibile non fa fallire l'elenco: si perde il dettaglio,
        // non la riga — e la riga è quella che dice all'utente che c'è qualcosa
        // da guardare.
        problems: problemi
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
        origins: Origini::da_json(origine.as_deref()),
    })
}

/// Quanti brani aspettano che qualcuno li guardi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn quanti(connection: &Connection) -> Result<i64, AppError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE meta_salute <> 'ok'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| db_error("conteggio dei brani da sistemare", &err))
}

/// Una pagina di brani da guardare.
///
/// I degradati per primi — un file che non si legge è un problema più grosso di
/// un album dedotto dalla cartella — e a parità i più recenti, che sono quelli
/// che l'utente ha appena aggiunto e di cui si ricorda.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn elenco(
    connection: &Connection,
    offset: i64,
    limite: i64,
) -> Result<Vec<TracciaIncerta>, AppError> {
    let sql = format!(
        "SELECT {COLONNE} FROM tracks
         WHERE meta_salute <> 'ok'
         ORDER BY meta_salute = 'degradato' DESC, date_added DESC, id DESC
         LIMIT ?1 OFFSET ?2"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| db_error("elenco dei brani da sistemare", &err))?;
    let righe = statement
        .query_map([limite, offset], traccia_da_riga)
        .map_err(|err| db_error("elenco dei brani da sistemare", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei brani da sistemare", &err))
}

/// Un brano solo, da guardare.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn uno(connection: &Connection, track_id: i64) -> Result<Option<TracciaIncerta>, AppError> {
    let sql = format!("SELECT {COLONNE} FROM tracks WHERE id = ?1");
    match connection.query_row(&sql, [track_id], traccia_da_riga) {
        Ok(traccia) => Ok(Some(traccia)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(db_error("lettura di un brano da sistemare", &err)),
    }
}

/// Il brano non c'è.
///
/// `library.trackNotFound`, e non `db.queryFailed` come prima: il database ha
/// risposto benissimo, è il brano che non c'è — e la frase di `db.queryFailed`
/// dice all'utente che il difetto è quasi sempre di Rymembra.
fn non_trovato(track_id: i64) -> AppError {
    AppError::new(ErrorCode::LibraryTrackNotFound {
        track_id: Some(track_id),
    })
    .with_message(format!("il brano {track_id} non è in libreria"))
}

/// Applica una correzione dell'utente a un brano.
///
/// Fa tre cose in una transazione sola, e devono stare insieme:
///
/// 1. **riscrive la riga**, chiavi comprese, ed è [`applica_correzioni`];
/// 2. **salva la correzione** in `track_overrides`, perché ogni scrittura
///    successiva dei campi descrittivi la rimetta sopra. A rimettercela è
///    [`riapplica`], chiamata da [`crate::library`] subito dopo ogni
///    `update_track` e da [`crate::enrich::registra`] subito dopo
///    `aggiorna_brano`: senza quelle due chiamate la correzione sparirebbe
///    dalla riga alla prima riscansione del file e resterebbe orfana qui
///    dentro, senza che niente lo dica;
/// 3. **ricostruisce gli aggregati**, perché un artista o un album appena
///    corretti devono esistere come schede.
///
/// I campi corretti diventano [`Origine::Manuale`]. Non è `enrich::plan_write` a
/// proteggerli: quello non sa delle correzioni, e su un campo manuale propone
/// il valore del catalogo come su ogni altro. A tenerli è il punto 2: subito
/// dopo ogni scrittura dell'arricchimento `enrich::registra` chiama
/// [`riapplica_sopra_i_tag`], che rimette sopra la correzione — campi e chiave.
///
/// # Errori
///
/// `library.trackNotFound` se il brano non c'è, `db.queryFailed` se il database
/// non risponde.
pub fn correggi(
    connection: &mut Connection,
    track_id: i64,
    correzioni: &Correzioni,
) -> Result<(), AppError> {
    if correzioni.e_vuoto() {
        return Ok(());
    }
    // `Immediate`, come ogni transazione che legge e poi scrive: una differita
    // che arriva alla prima scrittura dopo che un'altra connessione ha scritto
    // — la cache dell'arricchimento, l'indice delle cartelle — fallisce subito
    // con `SQLITE_BUSY_SNAPSHOT`, senza nemmeno aspettare. Vedi
    // [`transazione_di_scrittura`].
    let tx = transazione_di_scrittura(connection)
        .map_err(|err| db_error("apertura della transazione", &err))?;
    // Album e artista **prima** della correzione: se cambia l'album, quello da
    // cui il brano esce va rifatto anche lui. Vedi [`Toccati`].
    let mut toccati = Toccati::nessuno();
    toccati.segna_brani(&tx, &[track_id])?;

    // Le correzioni si accumulano: chi corregge il titolo oggi e l'artista
    // domani non deve perdere la prima. È la stessa ragione per cui
    // `enrich_undo` si scrive con `INSERT OR IGNORE` — solo al contrario, qui la
    // verità è l'ultima parola e non la prima.
    let mut tutte: Correzioni = tx
        .query_row(
            "SELECT campi FROM track_overrides WHERE track_id = ?1",
            [track_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    fondi(&mut tutte, correzioni);

    applica_correzioni(&tx, track_id, &tutte)?;

    let campi = serde_json::to_string(&tutte).map_err(|err| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("correzione di un brano".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    tx.execute(
        "INSERT INTO track_overrides (track_id, campi, set_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(track_id) DO UPDATE SET campi = ?2, set_at = ?3",
        rusqlite::params![track_id, campi, now_ms()],
    )
    .map_err(|err| db_error("salvataggio della correzione", &err))?;

    // Solo gli album e gli artisti di questo brano, prima e dopo: la
    // ricostruzione intera rileggeva tutta la libreria per un titolo
    // corretto, e su centomila brani teneva la libreria più dei quattro
    // secondi che la finestra aspetta — il cuore premuto subito dopo tornava
    // `db.locked`.
    toccati.segna_brani(&tx, &[track_id])?;
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione", &err))
}

/// Scrive nella riga `tracks` i valori che l'utente ha deciso.
///
/// È il passo che [`correggi`] e [`riapplica`] hanno in comune, e sta in una
/// funzione sola perché la seconda deve fare **esattamente** quel che ha fatto
/// la prima: gli stessi campi, le stesse origini `manuale`, le stesse due chiavi
/// ricalcolate. Due copie divergerebbero, e a divergere sarebbe quella che gira
/// durante una scansione, cioè quella che nessuno sta guardando.
///
/// `track_key` e `album_key` si ricalcolano dai valori finali: non ricalcolarle
/// lascerebbe il brano agganciato all'identità che aveva quando si chiamava
/// «Artista sconosciuto», e la sincronizzazione lo cercherebbe con quel nome su
/// ogni altro dispositivo.
///
/// `tutte` sono **tutte** le correzioni salvate su quel brano, non quella appena
/// arrivata: la fusione la fa chi chiama, perché solo chi chiama sa se ne sta
/// aggiungendo una ([`correggi`]) o rimettendo sopra quelle di prima
/// ([`riapplica`]).
///
/// # Errori
///
/// `library.trackNotFound` se il brano non c'è, `db.queryFailed` se il database
/// non risponde.
pub(crate) fn applica_correzioni(
    tx: &Transaction<'_>,
    track_id: i64,
    tutte: &Correzioni,
) -> Result<(), AppError> {
    let (path, mut title, mut artist, mut album, origine): (
        String,
        String,
        String,
        String,
        Option<String>,
    ) = tx
        .query_row(
            // `COALESCE(path, fonte_url)` e non `path`: da qui esce solo la
            // cartella con cui `album_group_key` raggruppa le edizioni, e per un
            // brano di catalogo quella cartella è il pezzo di indirizzo che sta
            // prima del file — cioè l'item dell'Internet Archive, che è il
            // concerto. Per Audius è l'identificativo del brano, e un singolo
            // che sta per conto suo è la verità.
            "SELECT COALESCE(path, fonte_url), title, artist, album, meta_origine
               FROM tracks WHERE id = ?1",
            [track_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => non_trovato(track_id),
            altro => db_error("lettura del brano da correggere", &altro),
        })?;

    let mut origini = Origini::da_json(origine.as_deref());
    let manuale = || {
        Some(crate::provenienza::Voce {
            da: Origine::Manuale.as_str().to_owned(),
            prima: None,
        })
    };
    if let Some(nuovo) = tutte.titolo.clone() {
        title = nuovo;
        origini.titolo = manuale();
    }
    if let Some(nuovo) = tutte.artista.clone() {
        artist = nuovo;
        origini.artista = manuale();
    }
    if let Some(nuovo) = tutte.album.clone() {
        album = nuovo;
        origini.album = manuale();
    }
    for (valore, voce) in [
        (tutte.album_artist.is_some(), &mut origini.album_artist),
        (tutte.genere.is_some(), &mut origini.genere),
        (tutte.anno.is_some(), &mut origini.anno),
        (tutte.traccia.is_some(), &mut origini.traccia),
        (tutte.disco.is_some(), &mut origini.disco),
    ] {
        if valore {
            *voce = manuale();
        }
    }

    let track_key = TrackKey::compute(TrackKeyInput {
        artist: Some(&artist),
        title: Some(&title),
        album: Some(&album),
    })
    .into_string();
    let album_key = album_group_key(&album, &path);

    // `content_key` **non** compare in questo `UPDATE`, e non è una svista:
    // vedi il `//!` del modulo. L'elenco esplicito delle colonne è ciò che lo
    // garantisce, e va tenuto esplicito.
    tx.execute(
        "UPDATE tracks SET
           title = ?2, artist = ?3, album = ?4,
           album_artist = COALESCE(?5, album_artist),
           genre = COALESCE(?6, genre),
           year = COALESCE(?7, year),
           track_number = COALESCE(?8, track_number),
           disc_number = COALESCE(?9, disc_number),
           track_key = ?10, album_key = ?11,
           meta_origine = ?12,
           -- Una risposta dell'utente chiude la domanda: il brano esce
           -- dall'elenco anche se qualche campo che non ha toccato resta
           -- dedotto. Chiederglielo di nuovo sarebbe non aver ascoltato — e
           -- vale anche quando qui ci passa `riapplica`: una riscansione non
           -- riapre una domanda a cui si è già risposto.
           meta_salute = ?13, meta_problemi = NULL
         WHERE id = ?1",
        rusqlite::params![
            track_id,
            title,
            artist,
            album,
            tutte.album_artist,
            tutte.genere,
            tutte.anno,
            tutte.traccia,
            tutte.disco,
            track_key,
            album_key,
            serde_json::to_string(&origini).ok(),
            Salute::Ok.as_str(),
        ],
    )
    .map(|_| ())
    .map_err(|err| db_error("scrittura del brano corretto", &err))
}

/// Rimette sopra la riga la correzione che l'utente aveva salvato.
///
/// Va chiamata **subito dopo** ogni scrittura che rifà i campi descrittivi di
/// `tracks` da una fonte che non sa niente di `track_overrides`: la scansione
/// ([`crate::library`], dopo `update_track`) e l'arricchimento
/// ([`crate::enrich::registra`], dopo `aggiorna_brano`) — da quelle due per
/// tramite di [`riapplica_sopra_i_tag`], che spiega perché. Senza queste chiamate
/// bastava che cambiasse la data di modifica di un file perché la correzione
/// sparisse dalla riga restando orfana nella sua tabella — ed è stato così fino
/// a questa release, commento della migrazione 016 compreso.
///
/// **Non ricostruisce gli aggregati**: chi la chiama lo fa già, una volta per
/// passata invece di una volta per brano.
///
/// `Ok(false)` quando non c'è niente da rimettere, che è il caso della
/// stragrande maggioranza delle righe: costa una lettura sulla chiave primaria
/// di `track_overrides`, e una scansione non paga altro.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub(crate) fn riapplica(tx: &Transaction<'_>, track_id: i64) -> Result<bool, AppError> {
    let salvate: Option<String> = {
        let mut statement = tx
            .prepare_cached("SELECT campi FROM track_overrides WHERE track_id = ?1")
            .map_err(|err| db_error("lettura di una correzione salvata", &err))?;
        match statement.query_row([track_id], |row| row.get::<_, String>(0)) {
            Ok(campi) => Some(campi),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(err) => return Err(db_error("lettura di una correzione salvata", &err)),
        }
    };
    let Some(salvate) = salvate else {
        return Ok(false);
    };
    // Un JSON illeggibile — scritto da una versione futura, o rovinato — non fa
    // cadere la scansione: si perde la riapplicazione su quella riga, non la
    // passata su ventimila file. La riga di `track_overrides` resta dov'è, e la
    // prossima correzione la riscrive bene.
    let Ok(tutte) = serde_json::from_str::<Correzioni>(&salvate) else {
        return Ok(false);
    };
    if tutte.e_vuoto() {
        return Ok(false);
    }
    applica_correzioni(tx, track_id, &tutte)?;
    Ok(true)
}

/// [`riapplica`], per chi ha appena rifatto la riga dai tag del file.
///
/// # La chiave, e perché non la scrive chi scrive i tag
///
/// Chi rifà la riga dai tag — la scansione, l'arricchimento, i due ritorni
/// indietro dell'arricchimento — ne calcola anche la `track_key`, e su un brano
/// corretto quella è la chiave **sbagliata**: [`riapplica`] la rimette giusta un
/// istante dopo, nella stessa transazione. Per le colonne di `tracks` il giro
/// non si vede. Per `lyrics` sì: il trigger della migrazione 022 sposta il testo
/// a ogni cambio di chiave, quindi all'andata se lo portava dietro, e al ritorno
/// poteva non riportarlo. Se un'altra copia del brano, non corretta, porta
/// ancora la chiave dei tag, la regola «non togliere il testo a un gemello»
/// lasciava il testo a lei: quello sincronizzato a mano sul FLAC corretto
/// finiva all'mp3 con i tag sbagliati.
///
/// Quei quattro `UPDATE` scrivono quindi la chiave dei tag **solo sui brani
/// senza correzione** (`CASE WHEN EXISTS (… track_overrides …)`), e la chiave
/// di un brano corretto cambia una volta sola, qui: da quella di prima a quella
/// che la correzione dà sopra i tag nuovi. Resta la correzione che c'è ma non
/// si legge, o è vuota: [`riapplica`] non rimette niente, e allora la chiave
/// giusta è quella dei tag, che si scrive adesso — sui brani senza correzione
/// è già lì, e l'`UPDATE` non trova righe.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub(crate) fn riapplica_sopra_i_tag(
    tx: &Transaction<'_>,
    track_id: i64,
    chiave_dei_tag: &str,
) -> Result<bool, AppError> {
    if riapplica(tx, track_id)? {
        return Ok(true);
    }
    tx.prepare_cached("UPDATE tracks SET track_key = ?2 WHERE id = ?1 AND track_key <> ?2")
        .and_then(|mut statement| statement.execute(rusqlite::params![track_id, chiave_dei_tag]))
        .map_err(|err| db_error("chiave di un brano senza correzione", &err))?;
    Ok(false)
}

/// Quel che l'arricchimento rimette sopra una riga appena riletta dal file.
#[derive(Debug)]
pub(crate) struct SopraITag {
    /// La riga da scrivere: i tag del file, con sopra i campi arricchiti che il
    /// file non ha smentito, e `track_key` e `album_key` ricalcolate da quelli.
    pub(crate) riga: TrackRow,
    /// La copertina dell'arricchimento da rimettere, quando il file non ne porta
    /// una sua.
    pub(crate) copertina: Option<String>,
}

/// Che fine fa un campo arricchito quando il file sotto è stato riletto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Destino {
    /// Il file sotto dice ancora quel che diceva: il campo arricchito si rimette.
    Rimetti,
    /// Il file adesso dice proprio il valore arricchito: il campo viene dal file,
    /// e l'annotazione lo lascia andare.
    DalFile,
    /// Il file è stato ritaggato: vince lui, e il brano torna in coda.
    Cambiato,
}

/// Decide il [`Destino`] di un campo.
///
/// `sotto` è quel che l'annotazione ricorda del file — `None` quando non lo
/// ricorda, cioè su un'annotazione della 2.4 — e `file` quel che il file dice
/// adesso, ripieghi già tolti.
///
/// # Quando non si sa cosa c'era sotto
///
/// Si rimette solo se il file, sotto quel campo, **non dice niente**. È il caso
/// di gran lunga più frequente — l'arricchimento riempie buchi: «Album
/// sconosciuto», un anno che non c'era — ed è anche l'unico in cui rimettere
/// non può disfare il lavoro di nessuno: dove il file non dice niente, nessuno
/// l'ha ritaggato. Dove invece dice qualcosa di diverso dal catalogo non si sa
/// se l'abbia sempre detto — l'arricchimento l'aveva sostituito — o se qualcuno
/// l'abbia appena scritto. Nel dubbio vince il file, e il brano torna in coda:
/// se aveva ragione il catalogo, la passata dopo lo ritroverà.
fn destino<T>(
    arricchito: &T,
    sotto: Option<&Option<T>>,
    file: Option<&T>,
    uguali: fn(Option<&T>, Option<&T>) -> bool,
) -> Destino {
    if uguali(file, Some(arricchito)) {
        return Destino::DalFile;
    }
    let intatto = match sotto {
        Some(prima) => uguali(file, prima.as_ref()),
        None => file.is_none(),
    };
    if intatto {
        Destino::Rimetti
    } else {
        Destino::Cambiato
    }
}

/// Due testi di tag sono lo stesso testo.
///
/// Anche quando uno è scritto scomposto e l'altro no: dalla 3.0 la lettura dei
/// tag li normalizza in NFC (vedi `metadata::clean`), mentre quel che
/// l'annotazione ricorda può venire da una riga letta prima. «é» scritto in due
/// modi non è un ritaggo, e trattarlo come tale rimetterebbe in coda un brano
/// che nessuno ha toccato.
fn stesso_testo(a: Option<&String>, b: Option<&String>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a == b || a.nfc().eq(b.nfc()),
        (a, b) => a.is_none() && b.is_none(),
    }
}

/// Due numeri di tag sono lo stesso numero.
fn stesso_numero<T: PartialEq>(a: Option<&T>, b: Option<&T>) -> bool {
    a == b
}

/// Rimette lo strato dell'arricchimento sopra una riga appena riletta dal file.
///
/// La chiama `library::update_track` **prima** di scrivere, e restituisce la
/// riga da scrivere al posto di quella letta: così la riga passa dai tag del
/// file al risultato finale con un `UPDATE` solo, e `track_key` cambia al più
/// una volta. Non è un'eleganza — il trigger della migrazione 022 sposta il
/// testo delle canzoni a ogni cambio di chiave, e un giro «chiave dei tag, poi
/// chiave arricchita» lo porterebbe via e poi forse non lo riporterebbe. È la
/// stessa ragione scritta su [`riapplica_sopra_i_tag`].
///
/// `Ok(None)` quando il brano non ha un'annotazione — cioè quasi sempre, e costa
/// una lettura sulla chiave primaria di `track_meta_arricchita`.
///
/// # Le regole, campo per campo
///
/// Per ogni campo che l'annotazione dice arricchito, il suo [`Destino`]:
///
/// * il file **adesso dice proprio il valore arricchito** → resta quello, ma
///   viene dal file: l'annotazione lo lascia andare;
/// * il file **dice ancora quel che diceva sotto** → il campo arricchito si
///   rimette, e l'annotazione non cambia;
/// * altrimenti il file **è stato ritaggato** → vince il file, il campo esce
///   dall'annotazione, e il brano torna in coda all'arricchimento
///   (`enrich_status` a `NULL`), che lo riguarderà con i tag nuovi.
///
/// # La copertina
///
/// Se il file adesso ne porta una sua, vince quella, e l'annotazione dimentica
/// la sua. Se non ne porta, si rimette quella dell'arricchimento — purché sia
/// ancora in `cover_art`, e non ci sarebbe se qualcuno avesse ripulito la cache.
/// Un'annotazione della 2.4 la copertina non la ricordava: lì si guarda la
/// copertina che la riga aveva fino a un istante fa, e si rimette se non veniva
/// da un tag, annotandola perché la volta dopo si sappia. Se invece veniva da un
/// tag, quel file la sua copertina l'ha persa: resta senza, e torna in coda.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un'annotazione illeggibile non
/// è un errore: vale come assente, e la scansione prosegue.
pub(crate) fn arricchimento_sopra_i_tag(
    tx: &Transaction<'_>,
    track_id: i64,
    letta: &TrackRow,
) -> Result<Option<SopraITag>, AppError> {
    let Some(annotata) = annotazione(tx, track_id)? else {
        return Ok(None);
    };
    let file = TagDelFile::della_riga_letta(letta);
    let mut riga = letta.clone();
    let mut resta = annotata.clone();
    let mut in_coda = false;

    macro_rules! decidi {
        ($campo:ident, $uguali:expr, $metti:expr) => {
            if let Some(valore) = annotata.$campo.clone() {
                match destino(
                    &valore,
                    annotata.sotto.$campo.as_ref(),
                    file.$campo.as_ref(),
                    $uguali,
                ) {
                    Destino::Rimetti => ($metti)(&mut riga, valore),
                    Destino::DalFile => {
                        resta.$campo = None;
                        resta.sotto.$campo = None;
                    }
                    Destino::Cambiato => {
                        resta.$campo = None;
                        resta.sotto.$campo = None;
                        in_coda = true;
                    }
                }
            }
        };
    }
    decidi!(title, stesso_testo, |r: &mut TrackRow, v| r.title = v);
    decidi!(artist, stesso_testo, |r: &mut TrackRow, v| r.artist = v);
    decidi!(album, stesso_testo, |r: &mut TrackRow, v| r.album = v);
    decidi!(album_artist, stesso_testo, |r: &mut TrackRow, v| r
        .album_artist =
        Some(v));
    decidi!(year, stesso_numero::<i32>, |r: &mut TrackRow, v| r.year =
        Some(i64::from(v)));
    decidi!(genre, stesso_testo, |r: &mut TrackRow, v| r.genre = Some(v));
    decidi!(track_number, stesso_numero::<u32>, |r: &mut TrackRow, v| {
        r.track_number = Some(i64::from(v))
    });
    decidi!(disc_number, stesso_numero::<u32>, |r: &mut TrackRow, v| r
        .disc_number =
        Some(i64::from(v)));
    decidi!(mb_recording_id, stesso_testo, |r: &mut TrackRow, v| r
        .mb_recording_id =
        Some(v));
    decidi!(mb_release_id, stesso_testo, |r: &mut TrackRow, v| r
        .mb_release_id =
        Some(v));
    decidi!(mb_release_group_id, stesso_testo, |r: &mut TrackRow, v| r
        .mb_release_group_id =
        Some(v));

    let copertina = if letta.cover.is_some() {
        // Il file adesso ne porta una sua: vince lui, come per ogni campo.
        resta.copertina = None;
        None
    } else if let Some(impronta) = annotata.copertina.clone() {
        if copertina_registrata(tx, &impronta)? {
            Some(impronta)
        } else {
            resta.copertina = None;
            in_coda = true;
            None
        }
    } else {
        match copertina_di_prima(tx, track_id)? {
            Some((impronta, fonte)) if fonte != CoverSource::Tag.as_str() => {
                resta.copertina = Some(impronta.clone());
                Some(impronta)
            }
            Some(_) => {
                in_coda = true;
                None
            }
            None => None,
        }
    };

    // Le chiavi dai valori finali, come fa `enrich::aggiorna_brano`: un brano a
    // cui si è appena rimesso l'album del catalogo si raggruppa con quello.
    riga.track_key = TrackKey::compute(TrackKeyInput {
        artist: Some(&riga.artist),
        title: Some(&riga.title),
        album: Some(&riga.album),
    })
    .into_string();
    riga.album_key = album_group_key(&riga.album, &riga.path);

    if resta != annotata {
        riscrivi_annotazione(tx, track_id, &resta)?;
    }
    if in_coda {
        // Di nuovo candidato: `enrich::candidato_where` prende i brani con
        // `enrich_status` a `NULL`, cioè «mai provato». Lo è, con questi tag.
        tx.prepare_cached(
            "UPDATE tracks SET enrich_status = NULL, enrich_attempted_at = NULL WHERE id = ?1",
        )
        .and_then(|mut statement| statement.execute([track_id]))
        .map_err(|err| db_error("rimessa in coda di un brano ritaggato", &err))?;
    }

    Ok(Some(SopraITag { riga, copertina }))
}

/// La copertina è ancora registrata in `cover_art`.
///
/// Rimettere un'impronta che non c'è più farebbe cadere la chiave esterna di
/// `tracks.cover_art_hash`, e con lei l'intero lotto della scansione.
fn copertina_registrata(tx: &Transaction<'_>, impronta: &str) -> Result<bool, AppError> {
    tx.prepare_cached("SELECT 1 FROM cover_art WHERE hash = ?1")
        .and_then(|mut statement| statement.exists([impronta]))
        .map_err(|err| db_error("lettura di una copertina", &err))
}

/// La copertina che la riga aveva prima di questa rilettura, con la sua
/// provenienza in `cover_art.source`.
///
/// Solo se è ancora registrata: una provenienza che non si legge non dice
/// niente, e un'impronta senza la sua riga non si potrebbe rimettere.
fn copertina_di_prima(
    tx: &Transaction<'_>,
    track_id: i64,
) -> Result<Option<(String, String)>, AppError> {
    tx.prepare_cached(
        "SELECT c.hash, c.source FROM tracks t JOIN cover_art c ON c.hash = t.cover_art_hash
          WHERE t.id = ?1",
    )
    .and_then(|mut statement| {
        statement
            .query_row([track_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()
    })
    .map_err(|err| db_error("lettura di una copertina", &err))
}

/// Riscrive l'annotazione di un brano con quel che ne resta, o la toglie se non
/// resta niente.
///
/// Fonte, confidenza e data restano quelle della passata che l'ha scritta: la
/// rilettura del file non ha deciso niente di nuovo, ha solo tolto.
fn riscrivi_annotazione(
    tx: &Transaction<'_>,
    track_id: i64,
    resta: &crate::enrich::CampiArricchiti,
) -> Result<(), AppError> {
    if resta.e_vuota() {
        return tx
            .prepare_cached("DELETE FROM track_meta_arricchita WHERE track_id = ?1")
            .and_then(|mut statement| statement.execute([track_id]))
            .map(|_| ())
            .map_err(|err| db_error("annotazione di un brano ritaggato", &err));
    }
    let campi = serde_json::to_string(resta).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("serializzazione dei campi arricchiti".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    tx.prepare_cached("UPDATE track_meta_arricchita SET campi = ?2 WHERE track_id = ?1")
        .and_then(|mut statement| statement.execute(rusqlite::params![track_id, campi]))
        .map(|_| ())
        .map_err(|err| db_error("annotazione di un brano ritaggato", &err))
}

/// Fonde una correzione nuova sopra quelle già salvate.
///
/// Un campo assente nella nuova non cancella il vecchio: assente vuol dire «non
/// l'ho toccato», mai «svuotalo».
fn fondi(tutte: &mut Correzioni, nuove: &Correzioni) {
    macro_rules! prendi {
        ($campo:ident) => {
            if nuove.$campo.is_some() {
                tutte.$campo = nuove.$campo.clone();
            }
        };
    }
    prendi!(titolo);
    prendi!(artista);
    prendi!(album);
    prendi!(album_artist);
    prendi!(genere);
    prendi!(anno);
    prendi!(traccia);
    prendi!(disco);
}

/// «La deduzione è giusta»: si tiene quel che c'è e non se ne parla più.
///
/// È la risposta più frequente, e senza di essa l'elenco dei brani da guardare
/// non si svuoterebbe mai: la maggior parte delle deduzioni sono corrette, e
/// l'utente vuole poterlo dire con un gesto invece di riscrivere a mano quel che
/// già legge giusto.
///
/// Tecnicamente è una [`correggi`] con i valori che il brano ha adesso: così la
/// conferma sopravvive alla riscansione — la scansione rimetterebbe i valori
/// dedotti dal file, e questa li rimette sopra — e i campi diventano
/// [`Origine::Manuale`], che l'arricchimento non tocca.
///
/// # Errori
///
/// `library.trackNotFound` se il brano non c'è, `db.queryFailed` se il database
/// non risponde.
pub fn conferma(connection: &mut Connection, track_id: i64) -> Result<(), AppError> {
    let Some(traccia) = uno(connection, track_id)? else {
        return Err(non_trovato(track_id));
    };
    // Si confermano **solo** i campi che erano dedotti. Fissare come «manuale»
    // anche quelli letti dai tag vorrebbe dire che una riscansione di un file
    // ri-taggato dall'utente non aggiornerebbe più niente.
    let dedotto = |voce: &Option<crate::provenienza::Voce>| {
        voce.as_ref().is_some_and(|v| v.origine() != Origine::Tag)
    };
    let o = &traccia.origins;
    let correzioni = Correzioni {
        titolo: dedotto(&o.titolo).then(|| traccia.title.clone()),
        artista: dedotto(&o.artista).then(|| traccia.artist.clone()),
        album: dedotto(&o.album).then(|| traccia.album.clone()),
        album_artist: dedotto(&o.album_artist)
            .then(|| traccia.album_artist.clone())
            .flatten(),
        genere: dedotto(&o.genere).then(|| traccia.genre.clone()).flatten(),
        anno: dedotto(&o.anno).then_some(traccia.year).flatten(),
        traccia: dedotto(&o.traccia)
            .then_some(traccia.track_number)
            .flatten(),
        disco: dedotto(&o.disco).then_some(traccia.disc_number).flatten(),
    };

    if correzioni.e_vuoto() {
        // Niente da fissare — un brano degradato senza nemmeno un indizio dal
        // percorso — ma la domanda va comunque chiusa: se l'elenco lo
        // riproponesse, il pulsante «va bene così» non farebbe niente.
        connection
            .execute(
                "UPDATE tracks SET meta_salute = ?2, meta_problemi = NULL WHERE id = ?1",
                rusqlite::params![track_id, Salute::Ok.as_str()],
            )
            .map(|_| ())
            .map_err(|err| db_error("conferma di un brano", &err))
    } else {
        correggi(connection, track_id, &correzioni)
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Una riga di brano come la scriverebbe una scansione, senza scansione.
    fn brano(connection: &Connection, path: &str, salute: &str, origine: Option<&str>) -> i64 {
        connection
            .execute(
                "INSERT INTO tracks (
                   path, track_key, title, artist, album, album_key, duration_ms,
                   file_size, date_added, date_modified, meta_salute, meta_origine,
                   meta_problemi, source
                 ) VALUES (?1, 'k', 'Senza tag', 'Artista sconosciuto',
                   'Album sconosciuto', 'ak', 0, 1, 1, 1, ?2, ?3, ?4, 'scan')",
                rusqlite::params![path, salute, origine, Some(r#"["senza-tag"]"#.to_owned())],
            )
            .expect("inserimento");
        connection.last_insert_rowid()
    }

    fn apri() -> Connection {
        crate::db::open_in_memory().expect("database").connection
    }

    #[test]
    fn lelenco_mostra_solo_quel_che_ce_da_guardare() {
        let connection = apri();
        brano(&connection, "C:/M/a.mp3", "ok", None);
        brano(&connection, "C:/M/b.mp3", "dedotto", None);
        brano(&connection, "C:/M/c.mp3", "degradato", None);

        assert_eq!(quanti(&connection).expect("conteggio"), 2);
        let elenco = elenco(&connection, 0, 10).expect("elenco");
        assert_eq!(elenco.len(), 2);
        // I degradati per primi: un file che non si legge è un problema più
        // grosso di un album dedotto dalla cartella.
        assert_eq!(elenco.first().map(|t| t.health.as_str()), Some("degradato"));
    }

    #[test]
    fn una_correzione_riscrive_la_riga_e_le_sue_chiavi() {
        let mut connection = apri();
        let id = brano(
            &connection,
            "C:/M/Pink Floyd/The Wall/a.mp3",
            "dedotto",
            None,
        );

        correggi(
            &mut connection,
            id,
            &Correzioni {
                artista: Some("Pink Floyd".to_owned()),
                album: Some("The Wall".to_owned()),
                ..Correzioni::default()
            },
        )
        .expect("correzione");

        let (artista, album, chiave, salute, origine): (
            String,
            String,
            String,
            String,
            Option<String>,
        ) = connection
            .query_row(
                "SELECT artist, album, track_key, meta_salute, meta_origine
                 FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("riga");
        assert_eq!(artista, "Pink Floyd");
        assert_eq!(album, "The Wall");
        assert_eq!(salute, "ok");
        // La chiave si ricalcola: senza, il brano resterebbe agganciato
        // all'identità che aveva quando si chiamava «Artista sconosciuto», e la
        // sincronizzazione lo cercherebbe con quel nome su ogni altro
        // dispositivo.
        assert_ne!(chiave, "k");
        assert!(chiave.contains("pink floyd"), "chiave: {chiave}");
        // …e i campi corretti si dichiarano «manuale», che `plan_write` non tocca.
        let origine = origine.expect("provenienza");
        assert!(origine.contains("manuale"), "origine: {origine}");
    }

    #[test]
    fn le_correzioni_si_accumulano() {
        // Chi corregge il titolo oggi e l'artista domani non deve perdere la
        // prima: quel che sta in `track_overrides` è l'ultima parola su ogni
        // campo, non l'ultima operazione.
        let mut connection = apri();
        let id = brano(&connection, "C:/M/a.mp3", "dedotto", None);

        correggi(
            &mut connection,
            id,
            &Correzioni {
                titolo: Some("Hey You".to_owned()),
                ..Correzioni::default()
            },
        )
        .expect("prima");
        correggi(
            &mut connection,
            id,
            &Correzioni {
                artista: Some("Pink Floyd".to_owned()),
                ..Correzioni::default()
            },
        )
        .expect("seconda");

        let campi: String = connection
            .query_row(
                "SELECT campi FROM track_overrides WHERE track_id = ?1",
                [id],
                |r| r.get(0),
            )
            .expect("correzioni");
        assert!(campi.contains("Hey You"), "campi: {campi}");
        assert!(campi.contains("Pink Floyd"), "campi: {campi}");
    }

    #[test]
    fn confermare_fissa_solo_quel_che_era_dedotto() {
        let mut connection = apri();
        let origine = r#"{"titolo":{"da":"percorso"},"artista":{"da":"tag"}}"#;
        let id = brano(&connection, "C:/M/a.mp3", "dedotto", Some(origine));

        conferma(&mut connection, id).expect("conferma");

        let campi: String = connection
            .query_row(
                "SELECT campi FROM track_overrides WHERE track_id = ?1",
                [id],
                |r| r.get(0),
            )
            .expect("correzioni");
        // Il titolo era dedotto: si fissa, così la riscansione non lo rifà.
        assert!(campi.contains("Senza tag"), "campi: {campi}");
        // L'interprete veniva dai tag: fissarlo vorrebbe dire che ri-taggare il
        // file a mano non aggiornerebbe più niente.
        assert!(!campi.contains("Artista sconosciuto"), "campi: {campi}");
        assert_eq!(quanti(&connection).expect("conteggio"), 0);
    }

    #[test]
    fn confermare_un_brano_senza_niente_da_fissare_chiude_lo_stesso_la_domanda() {
        // Un file degradato di cui nemmeno il percorso dice niente: non c'è
        // nessun campo da rendere manuale, ma il pulsante «va bene così» deve
        // comunque toglierlo dall'elenco. Altrimenti non farebbe niente.
        let mut connection = apri();
        let id = brano(&connection, "C:/M/a.mp3", "degradato", None);
        conferma(&mut connection, id).expect("conferma");
        assert_eq!(quanti(&connection).expect("conteggio"), 0);
    }

    #[test]
    fn una_riga_nuova_non_ha_sovrascritture() {
        // Perché il ramo `insert_track` della scansione non chiama
        // [`riapplica`], e non è una dimenticanza: `track_overrides` è
        // agganciata all'`id` della riga, e una riga che nasce adesso non può
        // averne una. La prova fissa le due metà — `riapplica` dice `false`, e
        // non tocca niente — così chiamarla anche lì sarebbe inutile, non
        // sbagliato, e chi legge il commento sa quale delle due cose sta
        // leggendo.
        let mut connection = apri();
        let id = brano(&connection, "C:/M/a.mp3", "dedotto", None);

        let tx = connection.transaction().expect("transazione");
        assert!(!riapplica(&tx, id).expect("riapplicazione"));
        tx.commit().expect("chiusura");

        let (titolo, chiave, salute): (String, String, String) = connection
            .query_row(
                "SELECT title, track_key, meta_salute FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("riga");
        assert_eq!(titolo, "Senza tag");
        assert_eq!(
            chiave, "k",
            "nessuna chiave ricalcolata su una riga che nessuno ha corretto"
        );
        assert_eq!(salute, "dedotto", "e la domanda resta aperta");
    }

    #[test]
    fn il_destino_di_un_campo_arricchito_dopo_la_rilettura() {
        let trovato = "Homogenic".to_owned();
        let altro = "Vespertine".to_owned();
        let minuscolo = "homogenic".to_owned();
        // Sotto non c'era niente, e il file tace ancora: si rimette.
        assert_eq!(
            destino(&trovato, Some(&None), None, stesso_testo),
            Destino::Rimetti
        );
        // Il file adesso dice proprio quello: il campo viene dal file.
        assert_eq!(
            destino(&trovato, Some(&None), Some(&trovato), stesso_testo),
            Destino::DalFile
        );
        // Il file dice altro: l'ha ritaggato qualcuno.
        assert_eq!(
            destino(&trovato, Some(&None), Some(&altro), stesso_testo),
            Destino::Cambiato
        );
        // Sotto c'era un valore, e c'è ancora: il catalogo l'aveva corretto, e
        // la correzione vale ancora.
        assert_eq!(
            destino(
                &trovato,
                Some(&Some(minuscolo.clone())),
                Some(&minuscolo),
                stesso_testo
            ),
            Destino::Rimetti
        );
        // Un'annotazione della 2.4: si rimette solo dove il file tace.
        assert_eq!(
            destino(&trovato, None, None, stesso_testo),
            Destino::Rimetti
        );
        assert_eq!(
            destino(&trovato, None, Some(&minuscolo), stesso_testo),
            Destino::Cambiato
        );
        // Scritto composto sotto e scomposto adesso: non è un ritaggo.
        assert_eq!(
            destino(
                &trovato,
                Some(&Some("Bj\u{00F6}rk".to_owned())),
                Some(&"Bjo\u{0308}rk".to_owned()),
                stesso_testo
            ),
            Destino::Rimetti
        );
        // E i numeri come i testi.
        assert_eq!(
            destino(&1997, Some(&None), Some(&1996), stesso_numero::<i32>),
            Destino::Cambiato
        );
    }

    #[test]
    fn un_brano_che_non_ce_non_fa_cadere_niente() {
        let mut connection = apri();
        assert!(conferma(&mut connection, 999).is_err());
        assert!(
            correggi(
                &mut connection,
                999,
                &Correzioni {
                    titolo: Some("x".to_owned()),
                    ..Correzioni::default()
                }
            )
            .is_err()
        );
    }
}
