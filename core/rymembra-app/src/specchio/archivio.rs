//! Il riconciliatore: da un secchio del catalogo di un altro dispositivo alle
//! righe specchio di `tracks`.
//!
//! La rete sta fuori: chi chiama (`src-tauri/src/lontano/specchio.rs`) chiede
//! il riassunto e i secchi **senza il lucchetto della libreria** e qui porta
//! solo quel che ha già in mano, in tre tempi come `sincronia` e `nuvola`:
//!
//! 1. si legge lo stato del dispositivo ([`stato`]);
//! 2. fuori dal lucchetto si chiede il riassunto ([`lavoro`] dice cosa
//!    rileggere) e i secchi che sono cambiati, e li si convalida
//!    ([`super::convalida`]);
//! 3. un secchio alla volta si scrive ([`Giro::applica_secchio`]), e alla fine
//!    si chiude il giro ([`Giro::chiudi`]).
//!
//! # Mai cancellare subito
//!
//! `playlist_tracks` ha `ON DELETE CASCADE`: togliere una riga svuoterebbe le
//! playlist in silenzio — e per la sincronia, fra due `allinea`, sembrerebbe
//! un brano tolto dall'utente. Una riga che il proprietario non manda più
//! prima si segna (`lontano_brani.sparito_ms`), poi segue la strada delle
//! rimozioni della scansione:
//!
//! - **si riabbina** per chiave di contenuto
//!   ([`rymembra_domain::scan_plan::match_moved_tracks`]) a una riga nuova dello
//!   stesso dispositivo: un id che cambia — la libreria del proprietario
//!   ricostruita — è un brano spostato, e si aggiorna **sul posto**, con le sue
//!   playlist;
//! - le altre, **sotto la soglia** di `SOGLIA_RIMOZIONI` righe e di un quinto di
//!   quelle del dispositivo, si tolgono — ma solo dopo la grazia;
//! - **oltre**, restano segnate e aspettano una conferma
//!   ([`conferma_rimozioni`]), che non aspetta la grazia.
//!
//! # La grazia, e la riga nascosta
//!
//! Una riga che il proprietario non manda più resta per sette giorni
//! ([`GRAZIA_MS`]) prima di essere tolta: un disco esterno staccato un weekend,
//! una libreria rifatta con id nuovi. Per una riga specchio nessun trigger
//! ricorda il suo posto nelle playlist (`content_key` resta NULL), quindi è la
//! grazia che permette a `Giro::riabbina`, anche fra giri diversi, di
//! agganciarla a una riga nuova. In quei giorni la riga è **nascosta**
//! (`nascondi_le_sparite`: `copia_di` uguale al proprio `id`) e **non si
//! suona e non si conta da nessuna parte** — «Tutti», autoplay, playlist
//! intelligenti, Android Auto, Cerca, conteggi —: la grazia protegge la voce di
//! playlist, non un brano fantasma. E la voce passa subito a un'altra copia del
//! brano, se c'è (`sposta_le_voci`), prima di ogni rimozione e quando la riga
//! entra in grazia.
//!
//! # A blocchi, con un tetto
//!
//! Le righe che scadono si tolgono a blocchi di [`BLOCCO_RIMOZIONI`], ognuno in
//! una transazione sua e — per chi tiene il lucchetto — in una presa sua
//! ([`togli_un_blocco`], [`conferma_un_blocco`]), e non oltre
//! [`TETTO_RIMOZIONI_PER_GIRO`] per giro: un pari che ritira un quinto di un
//! catalogo da centomila righe non tiene fermo il lettore per venti secondi.
//! Anche **nascondere** le righe che il pari non manda più è a blocchi della
//! stessa grandezza: la chiusura del giro ne nasconde un blocco, e
//! [`togli_un_blocco`] i blocchi dopo, ognuno in una presa sua. Nascondere non
//! costringe a rifare né i doppioni né gli aggregati: la scelta dei doppioni
//! lascia fuori le righe segnate sparite (lo sono già dal giro che le ha
//! segnate) e gli aggregati contano le righe di `tracks` senza guardare
//! `copia_di`. Si fanno una volta sola, nella chiusura del giro.
//! Le righe nuove che un giro tiene per riabbinarle sono al più
//! [`TETTO_RIABBINAMENTO`]. E ogni scrittura controlla, nella sua transazione,
//! che il dispositivo sia ancora collegato: un secchio che arriva dopo
//! «Dimentica» non riporta le sue righe.
//!
//! # Le statistiche
//!
//! Quelle di una riga specchio sono una copia **in sola lettura** di quelle del
//! proprietario, che le conta e fa lo scrobble: qui non si sommano mai. Il
//! solo scarto è un voto o un mi piace dato qui e non ancora arrivato (c'è una
//! voce in `dispositivi_posta`): finché aspetta, il riconciliatore non lo
//! riscrive con quel che l'altro aveva prima.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};
use rymembra_domain::album::album_group_key;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::scan_plan::{RemovedIdentity, match_moved_tracks};

use super::convalida::MAX_RIGHE_PER_DISPOSITIVO;
use super::doppioni::{Qualita, RigaDoppione, identita_album, scegli};
use super::{PREFISSO, SORGENTE, percorso};
use crate::library::condivisa::{
    BranoCatalogo, ID_PER_SECCHIO, RiassuntoCatalogo, SecchioCatalogo,
};
use crate::library::{Toccati, db_error, ricostruisci_aggregati};

/// Sotto quante righe sparite una rimozione non è mai «di massa»: la soglia
/// della scansione (`library::SOGLIA_RIMOZIONI`).
pub const SOGLIA_RIMOZIONI: usize = 50;

/// Oltre quale frazione delle righe di un dispositivo una rimozione è sospetta:
/// un quinto, come per la scansione.
pub const FRAZIONE_SOSPETTA: usize = 5;

/// Quanto resta, prima di essere tolta, una riga che il proprietario non manda
/// più: sette giorni.
///
/// È la grazia di [`Giro`]: una riga tolta sotto soglia e tornata con un id
/// nuovo — un disco esterno staccato un weekend, la libreria del proprietario
/// rifatta — deve ritrovare il suo posto nelle playlist, e per una riga
/// specchio nessun trigger lo fa (`content_key` resta NULL, vedi la 043). In
/// quei giorni la riga è **nascosta e non si suona da nessuna parte**: vedi
/// [`nascondi_le_sparite`].
pub const GRAZIA_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Quante righe si tolgono in una transazione sola. Il lucchetto della libreria
/// è quello del lettore: un pari ostile che ritira un quinto di un catalogo di
/// centomila righe lo teneva per venti secondi.
pub const BLOCCO_RIMOZIONI: usize = 2_000;

/// Quante righe di un dispositivo si tolgono, al massimo, in un giro: il resto
/// aspetta il giro dopo.
pub const TETTO_RIMOZIONI_PER_GIRO: usize = 10_000;

/// Quante righe nuove di un giro si tengono in memoria per riabbinare un id
/// cambiato. Oltre, le righe entrano lo stesso e non si riabbinano: un pari che
/// manda duecentomila righe al tetto dei testi faceva tenere mezzo gigabyte
/// (su Android è un'uccisione).
pub const TETTO_RIABBINAMENTO: usize = 20_000;

/// Una transazione di scrittura su una connessione che si ha in prestito:
/// `Immediate`, perché legge e poi scrive.
fn transazione(connection: &Connection) -> Result<Transaction<'_>, AppError> {
    Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .map_err(|err| db_error("apertura della transazione dello specchio", &err))
}

/// Quel che il riconciliatore sa, in libreria, di un dispositivo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatoDispositivo {
    /// La generazione della sua libreria all'ultimo riassunto letto.
    pub generazione: i64,
    /// Tutti i secchi sono stati letti, senza errori, almeno una volta.
    pub completo: bool,
    /// L'ultimo giro finito, ms.
    pub ultimo_ms: i64,
    /// I secchi com'erano all'ultima lettura: `da_id → (a_id, impronta)`.
    pub secchi: BTreeMap<i64, (i64, String)>,
    /// Quante righe specchio ha adesso.
    pub righe: usize,
}

/// Lo stato di un dispositivo.
///
/// # Errori
///
/// Quelli del database.
pub fn stato(connection: &Connection, dispositivo: &str) -> Result<StatoDispositivo, AppError> {
    let cosa = "stato di un catalogo lontano";
    let mut stato = connection
        .query_row(
            "SELECT generazione, completo, ultimo_ms FROM lontano_cataloghi WHERE dispositivo = ?1",
            [dispositivo],
            |riga| {
                Ok(StatoDispositivo {
                    generazione: riga.get(0)?,
                    completo: riga.get::<_, i64>(1)? != 0,
                    ultimo_ms: riga.get(2)?,
                    ..StatoDispositivo::default()
                })
            },
        )
        .optional()
        .map_err(|err| db_error(cosa, &err))?
        .unwrap_or_default();
    let mut secchi = connection
        .prepare("SELECT da_id, a_id, impronta FROM lontano_secchi WHERE dispositivo = ?1")
        .map_err(|err| db_error(cosa, &err))?;
    let righe = secchi
        .query_map([dispositivo], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                (r.get::<_, i64>(1)?, r.get::<_, String>(2)?),
            ))
        })
        .map_err(|err| db_error(cosa, &err))?;
    for riga in righe {
        let (da_id, resto) = riga.map_err(|err| db_error(cosa, &err))?;
        stato.secchi.insert(da_id, resto);
    }
    stato.righe = usize::try_from(
        connection
            .query_row(
                "SELECT COUNT(*) FROM lontano_brani WHERE dispositivo = ?1",
                [dispositivo],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|err| db_error(cosa, &err))?,
    )
    .unwrap_or(0);
    Ok(stato)
}

/// Il riassunto di un altro dispositivo, nei tetti: i secchi malformati —
/// fuori dalla griglia da [`ID_PER_SECCHIO`] id, con una impronta che non è di
/// trentadue cifre, senza righe o con più di quante ne stanno, ripetuti — si
/// scartano, e quando le righe dichiarate superano
/// [`MAX_RIGHE_PER_DISPOSITIVO`] si smette. Restituisce il riassunto buono e
/// quanti secchi ha scartato.
#[must_use]
pub fn convalida_riassunto(grezzo: RiassuntoCatalogo) -> (RiassuntoCatalogo, usize) {
    let mut scartati = 0_usize;
    let mut righe_totali = 0_usize;
    let mut visti = HashSet::new();
    let mut secchi: Vec<SecchioCatalogo> = Vec::new();
    for secchio in grezzo.secchi {
        let griglia = secchio.da_id >= 0
            && secchio.da_id % ID_PER_SECCHIO == 0
            && secchio.a_id == secchio.da_id.saturating_add(ID_PER_SECCHIO - 1);
        let impronta = secchio.impronta.len() == 32
            && secchio
                .impronta
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
        let righe = usize::try_from(secchio.righe).unwrap_or(0);
        let capienza = usize::try_from(ID_PER_SECCHIO).unwrap_or(usize::MAX);
        if !griglia || !impronta || righe == 0 || righe > capienza || !visti.insert(secchio.da_id) {
            scartati += 1;
            continue;
        }
        if righe_totali.saturating_add(righe) > MAX_RIGHE_PER_DISPOSITIVO {
            scartati += 1;
            continue;
        }
        righe_totali += righe;
        secchi.push(secchio);
    }
    secchi.sort_by_key(|s| s.da_id);
    (
        RiassuntoCatalogo {
            generazione: grezzo.generazione,
            secchi,
        },
        scartati,
    )
}

/// Cosa rileggere, dal riassunto e da quel che si sapeva.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lavoro {
    /// I secchi nuovi o con un'impronta diversa da quella di prima.
    pub da_leggere: Vec<SecchioCatalogo>,
    /// I secchi che c'erano e adesso non ci sono più: `(da_id, a_id)`.
    pub da_svuotare: Vec<(i64, i64)>,
    /// Quante righe dichiara il riassunto, in tutto: per l'avanzamento.
    pub righe_totali: i64,
    /// Non c'è niente da fare: stessa generazione, già letto per intero.
    pub niente: bool,
}

/// Cosa rileggere. Puro: «il client chiede solo i secchi la cui impronta è
/// cambiata».
#[must_use]
pub fn lavoro(riassunto: &RiassuntoCatalogo, stato: &StatoDispositivo) -> Lavoro {
    let da_leggere: Vec<SecchioCatalogo> = riassunto
        .secchi
        .iter()
        .filter(|s| {
            stato
                .secchi
                .get(&s.da_id)
                .is_none_or(|(_, impronta)| *impronta != s.impronta)
        })
        .cloned()
        .collect();
    let presenti: HashSet<i64> = riassunto.secchi.iter().map(|s| s.da_id).collect();
    let da_svuotare: Vec<(i64, i64)> = stato
        .secchi
        .iter()
        .filter(|(da, _)| !presenti.contains(da))
        .map(|(da, (a, _))| (*da, *a))
        .collect();
    let righe_totali = riassunto.secchi.iter().map(|s| s.righe).sum();
    let niente = da_leggere.is_empty()
        && da_svuotare.is_empty()
        && stato.completo
        && stato.generazione == riassunto.generazione;
    Lavoro {
        da_leggere,
        da_svuotare,
        righe_totali,
        niente,
    }
}

/// L'impronta di una riga com'è arrivata: se cambia una cifra, cambia.
fn impronta_della_riga(riga: &BranoCatalogo) -> String {
    let json = serde_json::to_vec(riga).unwrap_or_default();
    blake3::hash(&json).to_hex().chars().take(32).collect()
}

/// La chiave d'album di una riga specchio: titolo normalizzato più una
/// «cartella» fatta del dispositivo e dell'impronta d'album del proprietario.
/// Lo stesso album dello stesso dispositivo è lo stesso gruppo; due dischi che
/// si chiamano uguale ma sono di artisti diversi no; e quando le righe portano
/// un identificativo MusicBrainz gli aggregati li fondono con i dischi di
/// questa macchina, come per ogni altro.
fn chiave_d_album(dispositivo: &str, riga: &BranoCatalogo) -> String {
    let impronta = riga.album_key.as_deref().unwrap_or("-");
    album_group_key(
        &riga.album,
        &format!("{PREFISSO}{dispositivo}/album-{impronta}/{}", riga.id),
    )
}

/// `date_modified` dalla versione che manda il proprietario (`<byte>-<ms>`):
/// mai negativo, che a `-1` la libreria dice «da rileggere».
fn data_dalla_versione(versione: &str) -> i64 {
    versione
        .split_once('-')
        .and_then(|(_, ms)| ms.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0)
}

/// L'impronta di copertina da scrivere in `tracks`, e la sua riga in
/// `cover_art` (la chiave esterna la vuole) se non c'è già.
///
/// La riga nasce con `source = 'lontano'` e senza misure, **prima** del file:
/// così la finestra ne conosce l'impronta e la chiede, e lo store la prende
/// dall'altro dispositivo alla prima richiesta (`lontano::specchio`). Se qui
/// c'è già — la stessa copertina di un disco locale — resta quella, col suo
/// file. Quando il file di questa macchina arriva con una scansione, prende il
/// posto di quello preso da fuori: vedi `library::righe::insert_cover`.
fn registra_la_copertina(
    tx: &Connection,
    remota: Option<&str>,
    adesso_ms: i64,
) -> Result<Option<String>, AppError> {
    let Some(impronta) = remota else {
        return Ok(None);
    };
    tx.execute(
        "INSERT OR IGNORE INTO cover_art (hash, mime_type, width, height, byte_size, source, created_at)
         VALUES (?1, 'image/jpeg', NULL, NULL, NULL, ?2, ?3)",
        params![impronta, SORGENTE, adesso_ms],
    )
    .map_err(|err| db_error("copertina di una riga specchio", &err))?;
    Ok(Some(impronta.to_owned()))
}

/// Quel che di una riga già in libreria serve a decidere se riscriverla.
#[derive(Debug, Clone)]
struct Esistente {
    track_id: i64,
    impronta: String,
    sparito: bool,
}

/// Le righe specchio di un dispositivo con un id remoto in `[da, a]`.
fn leggi_esistenti(
    tx: &Connection,
    dispositivo: &str,
    da_id: i64,
    a_id: i64,
) -> Result<HashMap<i64, Esistente>, AppError> {
    let cosa = "righe specchio di un secchio";
    let mut st = tx
        .prepare_cached(
            "SELECT id_remoto, track_id, impronta, sparito_ms IS NOT NULL
               FROM lontano_brani
              WHERE dispositivo = ?1 AND id_remoto BETWEEN ?2 AND ?3",
        )
        .map_err(|err| db_error(cosa, &err))?;
    let righe = st
        .query_map(params![dispositivo, da_id, a_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                Esistente {
                    track_id: r.get(1)?,
                    impronta: r.get(2)?,
                    sparito: r.get::<_, i64>(3)? != 0,
                },
            ))
        })
        .map_err(|err| db_error(cosa, &err))?;
    righe
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(|err| db_error(cosa, &err))
}

/// I brani di `dispositivo` che hanno una valutazione in posta: il loro voto e
/// il loro mi piace di qui sono più nuovi di quelli del proprietario.
fn brani_in_posta(tx: &Connection, dispositivo: &str) -> Result<HashSet<i64>, AppError> {
    let cosa = "valutazioni in posta";
    let mut st = tx
        .prepare_cached(
            "SELECT chiave FROM dispositivi_posta WHERE dispositivo = ?1 AND tipo = 'valuta'",
        )
        .map_err(|err| db_error(cosa, &err))?;
    let righe = st
        .query_map([dispositivo], |r| r.get::<_, String>(0))
        .map_err(|err| db_error(cosa, &err))?;
    let mut brani = HashSet::new();
    for chiave in righe {
        if let Ok(id) = chiave.map_err(|err| db_error(cosa, &err))?.parse::<i64>() {
            brani.insert(id);
        }
    }
    Ok(brani)
}

/// Inserisce una riga specchio e la sua riga di `lontano_brani`. Restituisce
/// l'id di `tracks`.
fn inserisci(
    tx: &Connection,
    dispositivo: &str,
    riga: &BranoCatalogo,
    impronta: &str,
    adesso_ms: i64,
) -> Result<i64, AppError> {
    let cosa = "inserimento di una riga specchio";
    let cover = registra_la_copertina(tx, riga.cover_art_hash.as_deref(), adesso_ms)?;
    // `content_key` resta NULL: i trigger delle statistiche orfane scattano solo
    // per le righe che ce l'hanno. Vedi la migrazione 043.
    tx.execute(
        "INSERT INTO tracks
           (path, track_key, content_key, title, artist, album, album_artist, album_key,
            year, track_number, disc_number, genre, duration_ms, cover_art_hash,
            bitrate, sample_rate, channels, codec, file_size, date_added, date_modified,
            play_count, last_played_at, rating, liked, liked_at, rating_at, stats_updated_at,
            mb_recording_id, mb_release_group_id, mb_release_id, source)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31)",
        params![
            percorso(dispositivo, riga.id),
            riga.track_key,
            riga.title,
            riga.artist,
            riga.album,
            riga.album_artist,
            chiave_d_album(dispositivo, riga),
            riga.year,
            riga.track_number,
            riga.disc_number,
            riga.genre,
            riga.duration_ms,
            cover,
            riga.bitrate,
            riga.sample_rate,
            riga.channels,
            riga.codec,
            riga.dimensione,
            riga.date_added,
            data_dalla_versione(&riga.versione),
            riga.play_count,
            riga.last_played_at,
            riga.rating,
            i64::from(riga.liked),
            riga.liked_at,
            riga.rating_at,
            riga.stats_updated_at,
            riga.mb_recording_id,
            riga.mb_release_group_id,
            riga.mb_release_id,
            SORGENTE,
        ],
    )
    .map_err(|err| db_error(cosa, &err))?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO lontano_brani
           (track_id, dispositivo, id_remoto, estensione, versione, album_impronta,
            content_key, cover_remota, inizio_cd, fine_cd, impronta, sparito_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL)",
        params![
            id,
            dispositivo,
            riga.id,
            riga.estensione,
            riga.versione,
            riga.album_key,
            riga.content_key,
            riga.cover_art_hash,
            riga.segmento.map(|s| s.inizio_cd),
            riga.segmento.and_then(|s| s.fine_cd),
            impronta,
        ],
    )
    .map_err(|err| db_error(cosa, &err))?;
    Ok(id)
}

/// La riga di `track_id` come la manda adesso il proprietario decide una scelta di
/// doppioni o un disco **diversi** da quelli di prima?
///
/// Quel che [`ricalcola_doppioni_in`] e [`riallinea_le_chiavi_d_album`] leggono
/// di una riga specchio: la chiave del brano e quella di contenuto, la qualità
/// (codec, campionamento, bitrate, dimensione, durata), titolo e artista del
/// disco e la sua impronta d'album; e se la riga era sparita e adesso torna. Gli
/// ascolti, i voti, i mi piace, la copertina, l'anno non decidono niente di
/// questo: un PC che suona musica cambia il suo catalogo a ogni ascolto, e
/// rifare per ognuno doppioni e dischi su tutta la libreria era la maggior parte
/// del lucchetto di una chiusura (S2-5).
///
/// Una riga che non c'è vale «cambia»: nel dubbio si rifà.
fn cambia_doppioni_o_disco(
    tx: &Connection,
    track_id: i64,
    riga: &BranoCatalogo,
) -> Result<bool, AppError> {
    let cosa = "aggiornamento di una riga specchio";
    type Prima = (
        String,
        Option<String>,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        i64,
        i64,
        Option<String>,
        bool,
    );
    let prima: Option<Prima> = tx
        .query_row(
            "SELECT t.track_key, l.content_key, t.album, t.album_artist, t.artist, t.codec,
                    t.sample_rate, t.bitrate, t.file_size, t.duration_ms, l.album_impronta,
                    l.sparito_ms IS NOT NULL
               FROM tracks t JOIN lontano_brani l ON l.track_id = t.id
              WHERE t.id = ?1",
            [track_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                ))
            },
        )
        .optional()
        .map_err(|err| db_error(cosa, &err))?;
    let Some(prima) = prima else {
        return Ok(true);
    };
    let uguale = prima.0 == riga.track_key
        && prima.1 == riga.content_key
        && prima.2 == riga.album
        && prima.3 == riga.album_artist
        && prima.4 == riga.artist
        && prima.5 == riga.codec
        && prima.6 == riga.sample_rate
        && prima.7 == riga.bitrate
        && prima.8 == riga.dimensione
        && prima.9 == riga.duration_ms
        && prima.10 == riga.album_key
        && !prima.11;
    Ok(!uguale)
}

/// Riscrive una riga specchio che già c'è. `tieni_il_mio_voto`: c'è una
/// valutazione in posta per questo brano, e quel che si è deciso qui vale più
/// di quel che l'altro aveva prima.
///
/// Dice se la riga ha cambiato quel che decide doppioni e disco
/// ([`cambia_doppioni_o_disco`]). Se no, **tiene la sua chiave d'album**: quella
/// di qui che [`riallinea_le_chiavi_d_album`] le ha dato, e che riscriverla con
/// quella del proprietario lascerebbe il disco diviso in due finché il
/// riallineamento — che per una riga con soli ascolti nuovi non gira — non
/// ricomincia.
fn aggiorna(
    tx: &Connection,
    track_id: i64,
    riga: &BranoCatalogo,
    impronta: &str,
    dispositivo: &str,
    tieni_il_mio_voto: bool,
    adesso_ms: i64,
) -> Result<bool, AppError> {
    let cosa = "aggiornamento di una riga specchio";
    let cambia = cambia_doppioni_o_disco(tx, track_id, riga)?;
    // Il proprietario ha corretto un tag e la chiave del brano è cambiata. Le voci
    // di playlist di questa riga portavano la chiave vecchia, e se la chiave
    // vecchia c'è ancora su un'altra riga — il file di qui, il caso comune —
    // restano lì: per la sincronia la voce che avesse seguito la riga alla
    // chiave nuova sarebbe una voce **tolta** dalla playlist (la chiave vecchia
    // ha un file, non è assente e non è solo specchio), e la lapide partirebbe
    // verso chi quel brano ce l'ha, senza che nessuno abbia toccato niente.
    // Senza un'altra riga con la chiave vecchia non si sposta niente: la chiave
    // vecchia non ha più righe, e per la sincronia è un brano assente,
    // invisibile e non tolto. È la regola di quando una riga se ne va.
    if cambia_la_chiave_di_una_riga_con_voci(tx, track_id, &riga.track_key)? {
        sposta_le_voci(tx, &[track_id], false)?;
    }
    let cover = registra_la_copertina(tx, riga.cover_art_hash.as_deref(), adesso_ms)?;
    tx.execute(
        "UPDATE tracks SET
           track_key = ?2, title = ?3, artist = ?4, album = ?5, album_artist = ?6,
           album_key = CASE WHEN ?33 THEN album_key ELSE ?7 END, year = ?8, track_number = ?9, disc_number = ?10, genre = ?11,
           duration_ms = ?12, cover_art_hash = ?13, bitrate = ?14, sample_rate = ?15,
           channels = ?16, codec = ?17, file_size = ?18, date_added = ?19, date_modified = ?20,
           play_count = ?21, last_played_at = ?22,
           rating = CASE WHEN ?32 THEN rating ELSE ?23 END,
           liked = CASE WHEN ?32 THEN liked ELSE ?24 END,
           liked_at = CASE WHEN ?32 THEN liked_at ELSE ?25 END,
           rating_at = CASE WHEN ?32 THEN rating_at ELSE ?26 END,
           stats_updated_at = CASE WHEN ?32 THEN stats_updated_at ELSE ?27 END,
           mb_recording_id = ?28, mb_release_group_id = ?29, mb_release_id = ?30
         WHERE id = ?1 AND ?31 IS NOT NULL",
        params![
            track_id,
            riga.track_key,
            riga.title,
            riga.artist,
            riga.album,
            riga.album_artist,
            chiave_d_album(dispositivo, riga),
            riga.year,
            riga.track_number,
            riga.disc_number,
            riga.genre,
            riga.duration_ms,
            cover,
            riga.bitrate,
            riga.sample_rate,
            riga.channels,
            riga.codec,
            riga.dimensione,
            riga.date_added,
            data_dalla_versione(&riga.versione),
            riga.play_count,
            riga.last_played_at,
            riga.rating,
            i64::from(riga.liked),
            riga.liked_at,
            riga.rating_at,
            riga.stats_updated_at,
            riga.mb_recording_id,
            riga.mb_release_group_id,
            riga.mb_release_id,
            impronta,
            tieni_il_mio_voto,
            !cambia,
        ],
    )
    .map_err(|err| db_error(cosa, &err))?;
    tx.execute(
        "UPDATE lontano_brani SET
           estensione = ?2, versione = ?3, album_impronta = ?4, content_key = ?5,
           cover_remota = ?6, inizio_cd = ?7, fine_cd = ?8, impronta = ?9, sparito_ms = NULL
         WHERE track_id = ?1",
        params![
            track_id,
            riga.estensione,
            riga.versione,
            riga.album_key,
            riga.content_key,
            riga.cover_art_hash,
            riga.segmento.map(|s| s.inizio_cd),
            riga.segmento.and_then(|s| s.fine_cd),
            // Un voto di qui che aspetta in posta non è stato preso dalla riga:
            // la riga non è «uguale a prima» finché non arriva, e al giro dopo
            // si guarda di nuovo.
            if tieni_il_mio_voto { "" } else { impronta },
        ],
    )
    .map_err(|err| db_error(cosa, &err))?;
    Ok(cambia)
}

/// Quel che un giro ha fatto, finora.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Conti {
    /// Righe nuove.
    pub inserite: usize,
    /// Righe riscritte perché è cambiato qualcosa.
    pub aggiornate: usize,
    /// Righe uguali a prima: non si toccano.
    pub invariate: usize,
    /// Righe che non sono entrate (oltre il tetto): le scarti di
    /// convalida li conta chi convalida.
    pub oltre_il_tetto: usize,
    /// Righe che il proprietario non manda più, segnate.
    pub segnate_sparite: usize,
}

/// Un giro di riconciliazione con un dispositivo: i secchi che si scrivono uno
/// alla volta, e la chiusura.
#[derive(Debug)]
pub struct Giro {
    dispositivo: String,
    toccati: Toccati,
    /// Le righe messe in questo giro, per riabbinare un id cambiato: solo se il
    /// dispositivo aveva già righe (il primo giro non ha niente da riabbinare, e
    /// cinquantamila righe in memoria non servono a nessuno).
    nuove: Vec<(i64, BranoCatalogo)>,
    tieni_le_nuove: bool,
    /// Quante righe nuove si tengono, al massimo: [`TETTO_RIABBINAMENTO`].
    tetto_riabbinamento: usize,
    righe_adesso: usize,
    /// Quel che si è fatto.
    pub conti: Conti,
}

impl Giro {
    /// Un giro nuovo per `dispositivo`, che adesso ha `righe_adesso` righe.
    #[must_use]
    pub fn nuovo(dispositivo: &str, righe_adesso: usize) -> Self {
        Self {
            dispositivo: dispositivo.to_owned(),
            toccati: Toccati::nessuno(),
            nuove: Vec::new(),
            tieni_le_nuove: righe_adesso > 0,
            tetto_riabbinamento: TETTO_RIABBINAMENTO,
            righe_adesso,
            conti: Conti::default(),
        }
    }

    /// Un tetto più basso per le righe da riabbinare: per le prove, che non
    /// vogliono scrivere ventimila righe per vedere che il tetto c'è.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn con_tetto_di_riabbinamento(mut self, tetto: usize) -> Self {
        self.tetto_riabbinamento = tetto;
        self
    }

    /// Quante righe nuove tiene per riabbinarle.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn da_riabbinare(&self) -> usize {
        self.nuove.len()
    }

    /// Quante righe ha adesso il dispositivo, con quelle di questo giro.
    #[must_use]
    pub const fn righe_adesso(&self) -> usize {
        self.righe_adesso
    }

    /// Scrive un secchio, in una transazione sua: le righe nuove entrano, quelle
    /// che sono cambiate si riscrivono, quelle uguali non si toccano, quelle che
    /// il proprietario non manda più si segnano. `righe` sono già convalidate
    /// ([`super::convalida::convalida_secchio`]).
    ///
    /// # Errori
    ///
    /// Quelli del database.
    pub fn applica_secchio(
        &mut self,
        connection: &Connection,
        secchio: &SecchioCatalogo,
        righe: Vec<BranoCatalogo>,
        adesso_ms: i64,
    ) -> Result<(), AppError> {
        let tx = transazione(connection)?;
        // Il cancello nella transazione, non prima della domanda: fra la
        // domanda (fino a trenta secondi) e questa scrittura la persona può
        // aver dimenticato il dispositivo, e un secchio in volo non deve
        // riportarne le righe.
        controlla_che_sia_associato(&tx, &self.dispositivo)?;
        let esistenti = leggi_esistenti(&tx, &self.dispositivo, secchio.da_id, secchio.a_id)?;
        let in_posta = brani_in_posta(&tx, &self.dispositivo)?;
        let prima: Vec<i64> = esistenti.values().map(|e| e.track_id).collect();
        self.toccati.segna_brani(&tx, &prima)?;
        let mut visti = HashSet::new();
        let mut toccate = Vec::new();
        let mut nuove = Vec::new();
        let mut conti = Conti::default();
        let mut righe_adesso = self.righe_adesso;
        // Qualcosa di quel che scrive decide doppioni o dischi?
        let mut da_rifare = false;
        for riga in righe {
            visti.insert(riga.id);
            let impronta = impronta_della_riga(&riga);
            match esistenti.get(&riga.id) {
                None => {
                    if righe_adesso >= MAX_RIGHE_PER_DISPOSITIVO {
                        conti.oltre_il_tetto += 1;
                        continue;
                    }
                    let id = inserisci(&tx, &self.dispositivo, &riga, &impronta, adesso_ms)?;
                    righe_adesso += 1;
                    toccate.push(id);
                    conti.inserite += 1;
                    // Con un tetto: oltre non si riabbina, vedi `TETTO_RIABBINAMENTO`.
                    if self.tieni_le_nuove
                        && self.nuove.len().saturating_add(nuove.len()) < self.tetto_riabbinamento
                    {
                        nuove.push((id, riga));
                    }
                }
                Some(e) if e.impronta == impronta && !e.sparito => conti.invariate += 1,
                Some(e) => {
                    let mio = in_posta.contains(&riga.id);
                    da_rifare |= aggiorna(
                        &tx,
                        e.track_id,
                        &riga,
                        &impronta,
                        &self.dispositivo,
                        mio,
                        adesso_ms,
                    )?;
                    toccate.push(e.track_id);
                    conti.aggiornate += 1;
                }
            }
        }
        for (id_remoto, e) in &esistenti {
            if !visti.contains(id_remoto) && !e.sparito {
                tx.execute(
                    "UPDATE lontano_brani SET sparito_ms = ?2 WHERE track_id = ?1",
                    params![e.track_id, adesso_ms],
                )
                .map_err(|err| db_error("riga specchio sparita", &err))?;
                conti.segnate_sparite += 1;
            }
        }
        // Una riga nuova o sparita cambia chi partecipa; una riscritta, solo se è
        // cambiato quel che decide ([`cambia_doppioni_o_disco`]). Il segno sta in
        // `settings`, nella stessa transazione: un arresto prima della chiusura
        // non lo perde, e la chiusura che viene dopo rifà quel che manca.
        if da_rifare || conti.inserite > 0 || conti.segnate_sparite > 0 {
            segna_da_rifare(&tx)?;
        }
        self.toccati.segna_brani(&tx, &toccate)?;
        tx.execute(
            "INSERT INTO lontano_secchi (dispositivo, da_id, a_id, impronta)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(dispositivo, da_id) DO UPDATE SET
               a_id = excluded.a_id, impronta = excluded.impronta",
            params![
                self.dispositivo,
                secchio.da_id,
                secchio.a_id,
                secchio.impronta
            ],
        )
        .map_err(|err| db_error("secchio del catalogo", &err))?;
        tx.commit()
            .map_err(|err| db_error("chiusura della transazione dello specchio", &err))?;
        self.righe_adesso = righe_adesso;
        self.nuove.extend(nuove);
        self.conti.inserite += conti.inserite;
        self.conti.aggiornate += conti.aggiornate;
        self.conti.invariate += conti.invariate;
        self.conti.oltre_il_tetto += conti.oltre_il_tetto;
        self.conti.segnate_sparite += conti.segnate_sparite;
        Ok(())
    }

    /// Chiude il giro: svuota i secchi che il proprietario non ha più, riabbina
    /// le righe il cui id è cambiato, toglie quelle sparite (se non sono troppe),
    /// rifà la scelta dei doppioni e gli aggregati, e scrive lo stato.
    ///
    /// `completo`: tutti i secchi da leggere sono stati letti. Un giro rotto a
    /// metà non decide delle rimozioni (non sa cosa manca davvero), ma rifà
    /// comunque doppioni e aggregati di quel che ha scritto.
    ///
    /// # Errori
    ///
    /// Quelli del database.
    pub fn chiudi(
        mut self,
        connection: &Connection,
        da_svuotare: &[(i64, i64)],
        completo: bool,
        generazione: i64,
        adesso_ms: i64,
    ) -> Result<Chiusura, AppError> {
        let tx = transazione(connection)?;
        controlla_che_sia_associato(&tx, &self.dispositivo)?;
        let mut chiusura = Chiusura::default();
        for (da_id, a_id) in da_svuotare {
            let ids = segna_sparite_in(&tx, &self.dispositivo, *da_id, *a_id, adesso_ms)?;
            chiusura.segnate_sparite += ids;
            if ids > 0 {
                segna_da_rifare(&tx)?;
            }
            tx.execute(
                "DELETE FROM lontano_secchi WHERE dispositivo = ?1 AND da_id = ?2",
                params![self.dispositivo, da_id],
            )
            .map_err(|err| db_error("secchio svuotato", &err))?;
        }
        if completo {
            chiusura.riabbinate = self.riabbina(&tx, adesso_ms)?;
            if chiusura.riabbinate > 0 {
                segna_da_rifare(&tx)?;
            }
            // Un blocco solo: gli altri, se ce ne sono, li chiede chi chiama
            // (`togli_un_blocco`), ognuno nella sua presa del lucchetto.
            let blocco = togli_un_blocco_in(
                &tx,
                &self.dispositivo,
                adesso_ms,
                &mut self.toccati,
                BLOCCO_RIMOZIONI,
            )?;
            if blocco.doppioni_da_rifare {
                segna_da_rifare(&tx)?;
            }
            chiusura.tolte = blocco.tolte;
            chiusura.ids_tolti = blocco.ids_tolti;
            chiusura.in_attesa = blocco.in_attesa;
            chiusura.in_grazia = blocco.in_grazia;
            chiusura.da_togliere = blocco.restano;
        } else {
            chiusura.in_attesa = sparite_in_attesa(&tx, &self.dispositivo)?;
        }
        // Le righe che il proprietario non manda più, e che restano per la
        // grazia o per la conferma, non si suonano né si contano da nessuna
        // parte: vedi `nascondi_le_sparite`. Un blocco solo: se ce ne sono di
        // più, `da_nascondere` lo dice e li chiede chi chiama, ognuno in una
        // presa sua del lucchetto (`togli_un_blocco`).
        let nascoste = nascondi_le_sparite(&tx, &self.dispositivo, BLOCCO_RIMOZIONI)?;
        chiusura.spostate = nascoste.spostate;
        chiusura.da_nascondere = nascoste.restano;
        if nascoste.nascoste > 0 || nascoste.spostate > 0 {
            segna_da_rifare(&tx)?;
        }
        // Doppioni e dischi si rifanno **se serve**, e non a ogni chiusura (S2-5):
        // un PC che suona musica cambia il suo catalogo a ogni ascolto, e rifarli
        // su tutta la libreria per ognuno era 9 secondi di lucchetto con
        // 250.000 righe (misura, profilo di sviluppo).
        if e_da_rifare(&tx)? {
            chiusura.copie_cambiate = rifai_doppioni_e_album(&tx, &mut self.toccati)?;
        }
        ricostruisci_aggregati(&tx, &self.toccati)?;
        tx.execute(
            "INSERT INTO lontano_cataloghi (dispositivo, generazione, completo, ultimo_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(dispositivo) DO UPDATE SET
               generazione = excluded.generazione, completo = excluded.completo,
               ultimo_ms = excluded.ultimo_ms",
            params![
                self.dispositivo,
                generazione,
                i64::from(completo),
                adesso_ms
            ],
        )
        .map_err(|err| db_error("stato del catalogo", &err))?;
        tx.commit()
            .map_err(|err| db_error("chiusura della transazione dello specchio", &err))?;
        Ok(chiusura)
    }

    /// Le righe sparite il cui contenuto è comparso con un altro id di questo
    /// giro sono lo stesso brano: si aggiornano **sul posto**.
    fn riabbina(&mut self, tx: &Connection, adesso_ms: i64) -> Result<usize, AppError> {
        if self.nuove.is_empty() {
            return Ok(0);
        }
        let sparite = sparite_con_chiave(tx, &self.dispositivo)?;
        if sparite.is_empty() {
            return Ok(0);
        }
        let chiavi: Vec<String> = self
            .nuove
            .iter()
            .map(|(_, r)| r.content_key.clone().unwrap_or_else(|| r.track_key.clone()))
            .collect();
        let rimosse: Vec<RemovedIdentity<'_>> = sparite
            .iter()
            .map(|(track_id, chiave)| RemovedIdentity {
                track_id: *track_id,
                content_key: chiave.as_str(),
            })
            .collect();
        let inserite: Vec<&str> = chiavi.iter().map(String::as_str).collect();
        let abbinate = match_moved_tracks(&rimosse, &inserite);
        let mut fatte = 0;
        for abbinata in abbinate {
            let Some((nuovo_id, riga)) = self.nuove.get(abbinata.insert_index) else {
                continue;
            };
            let vecchio = abbinata.track_id;
            self.toccati.segna_brani(tx, &[vecchio, *nuovo_id])?;
            // Prima via la riga nuova, che occupa il percorso e l'id remoto.
            tx.execute("DELETE FROM tracks WHERE id = ?1", [nuovo_id])
                .map_err(|err| db_error("riga specchio riabbinata", &err))?;
            tx.execute(
                "UPDATE tracks SET path = ?2 WHERE id = ?1",
                params![vecchio, percorso(&self.dispositivo, riga.id)],
            )
            .map_err(|err| db_error("riga specchio riabbinata", &err))?;
            let id_di_prima: i64 = tx
                .query_row(
                    "SELECT id_remoto FROM lontano_brani WHERE track_id = ?1",
                    [vecchio],
                    |r| r.get(0),
                )
                .map_err(|err| db_error("riga specchio riabbinata", &err))?;
            tx.execute(
                "UPDATE lontano_brani SET id_remoto = ?2 WHERE track_id = ?1",
                params![vecchio, riga.id],
            )
            .map_err(|err| db_error("riga specchio riabbinata", &err))?;
            sposta_la_posta(tx, &self.dispositivo, id_di_prima, riga.id)?;
            let impronta = impronta_della_riga(riga);
            aggiorna(
                tx,
                vecchio,
                riga,
                &impronta,
                &self.dispositivo,
                false,
                adesso_ms,
            )?;
            self.toccati.segna_brani(tx, &[vecchio])?;
            fatte += 1;
        }
        Ok(fatte)
    }
}

/// La posta in uscita per il brano `da` — un voto, un testo corretto — passa al
/// brano che adesso ha l'id `a`: lo stesso brano, riabbinato dopo che il
/// proprietario ha rifatto la sua libreria con id nuovi (S2-13).
///
/// La chiave di una voce è l'id che il brano ha **dall'altra parte**, e il corpo
/// di un voto lo porta anche dentro (`brano`): è quello che il proprietario
/// legge. Rimasta all'id di prima, la voce arriverebbe a un brano che non c'è
/// (`lontano.fileCambiato`) e si butterebbe, e il cuore dato qui sparirebbe al
/// giro dopo. L'orologio della voce non cambia: è quello della decisione.
///
/// `OR IGNORE`: se per l'id nuovo c'è già una voce — non può esserci, la riga
/// con quell'id è appena entrata — non si pesta, e quella di prima si butta da
/// sola.
fn sposta_la_posta(tx: &Connection, dispositivo: &str, da: i64, a: i64) -> Result<(), AppError> {
    if da == a {
        return Ok(());
    }
    tx.execute(
        "UPDATE OR IGNORE dispositivi_posta
            SET chiave = ?4,
                corpo = CASE WHEN tipo = 'valuta' THEN json_set(corpo, '$.brano', ?3) ELSE corpo END
          WHERE dispositivo = ?1 AND chiave = ?2 AND tipo IN ('valuta', 'testo')",
        params![dispositivo, da.to_string(), a, a.to_string()],
    )
    .map(|_| ())
    .map_err(|err| db_error("posta di una riga riabbinata", &err))
}

/// Com'è andato il giro.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chiusura {
    /// Righe riabbinate a un id nuovo, sul posto.
    pub riabbinate: usize,
    /// Righe tolte (in questa presa: i blocchi dopo il primo li conta chi li
    /// chiede).
    pub tolte: usize,
    /// I loro identificativi, per sgomberare la coda del lettore ([`Blocco::ids_tolti`]).
    pub ids_tolti: Vec<i64>,
    /// Righe sparite che aspettano una conferma: una rimozione di massa, sopra
    /// la soglia.
    pub in_attesa: usize,
    /// Righe sparite che restano per la grazia ([`GRAZIA_MS`]): nascoste, e
    /// non c'è niente da confermare.
    pub in_grazia: usize,
    /// Righe scadute e da togliere che non sono entrate nel blocco di questa
    /// presa: si chiedono con [`togli_un_blocco`], finché non sono zero.
    pub da_togliere: usize,
    /// Restano righe sparite da nascondere, oltre il blocco di questa presa: si
    /// chiedono con [`togli_un_blocco`], finché non lo dice più. Intanto restano
    /// visibili com'erano, con le loro voci.
    pub da_nascondere: bool,
    /// Voci di playlist passate a un'altra copia dello stesso brano perché la
    /// riga che le aveva è entrata in grazia.
    pub spostate: usize,
    /// Righe sparite segnate chiudendo (secchi svuotati).
    pub segnate_sparite: usize,
    /// Righe il cui `copia_di` è cambiato.
    pub copie_cambiate: usize,
}

/// Le righe di `[da, a]` che il proprietario non ha più: si segnano.
fn segna_sparite_in(
    tx: &Connection,
    dispositivo: &str,
    da_id: i64,
    a_id: i64,
    adesso_ms: i64,
) -> Result<usize, AppError> {
    tx.execute(
        "UPDATE lontano_brani SET sparito_ms = ?4
          WHERE dispositivo = ?1 AND id_remoto BETWEEN ?2 AND ?3 AND sparito_ms IS NULL",
        params![dispositivo, da_id, a_id, adesso_ms],
    )
    .map_err(|err| db_error("righe specchio sparite", &err))
}

/// Le righe segnate sparite di un dispositivo, con la chiave con cui si
/// riabbinano: quella di contenuto del proprietario, o `track_key`.
fn sparite_con_chiave(tx: &Connection, dispositivo: &str) -> Result<Vec<(i64, String)>, AppError> {
    let cosa = "righe specchio sparite";
    let mut st = tx
        .prepare_cached(
            "SELECT l.track_id, COALESCE(l.content_key, t.track_key)
               FROM lontano_brani l JOIN tracks t ON t.id = l.track_id
              WHERE l.dispositivo = ?1 AND l.sparito_ms IS NOT NULL
              ORDER BY l.track_id",
        )
        .map_err(|err| db_error(cosa, &err))?;
    let righe = st
        .query_map([dispositivo], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|err| db_error(cosa, &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))
}

/// Fra questi id c'è una riga specchio? Per chi non deve toccarle: la
/// cancellazione a mano, la sessione di «Ascolta insieme».
///
/// # Errori
///
/// Quelli del database.
pub fn tra_le_righe_specchio(connection: &Connection, ids: &[i64]) -> Result<bool, AppError> {
    Ok(!righe_specchio_fra(connection, ids)?.is_empty())
}

/// Quali di questi id sono righe specchio, nell'ordine in cui arrivano.
///
/// # Errori
///
/// Quelli del database.
pub fn righe_specchio_fra(connection: &Connection, ids: &[i64]) -> Result<Vec<i64>, AppError> {
    let cosa = "righe specchio fra quelle date";
    let mut trovate = Vec::new();
    for pezzo in ids.chunks(500) {
        let segnaposti = vec!["?"; pezzo.len()].join(",");
        let mut st = connection
            .prepare(&format!(
                "SELECT t.id FROM tracks t WHERE t.id IN ({segnaposti}) AND {}",
                super::SQL_SPECCHIO
            ))
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map(rusqlite::params_from_iter(pezzo), |r| r.get::<_, i64>(0))
            .map_err(|err| db_error(cosa, &err))?;
        for id in lette {
            trovate.push(id.map_err(|err| db_error(cosa, &err))?);
        }
    }
    trovate.sort_by_key(|id| ids.iter().position(|x| x == id));
    Ok(trovate)
}

/// Quante righe di un dispositivo aspettano la conferma di sparire.
///
/// # Errori
///
/// Quelli del database.
pub fn sparite_in_attesa(connection: &Connection, dispositivo: &str) -> Result<usize, AppError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM lontano_brani WHERE dispositivo = ?1 AND sparito_ms IS NOT NULL",
            [dispositivo],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| usize::try_from(n).unwrap_or(0))
        .map_err(|err| db_error("righe specchio sparite", &err))
}

/// La riga ha delle voci di playlist e sta per cambiare `track_key`.
fn cambia_la_chiave_di_una_riga_con_voci(
    tx: &Connection,
    track_id: i64,
    nuova: &str,
) -> Result<bool, AppError> {
    tx.prepare_cached(
        "SELECT EXISTS (SELECT 1 FROM tracks WHERE id = ?1 AND track_key <> ?2)
            AND EXISTS (SELECT 1 FROM playlist_tracks WHERE track_id = ?1)",
    )
    .and_then(|mut st| st.query_row(params![track_id, nuova], |r| r.get(0)))
    .map_err(|err| db_error("chiave di una riga specchio", &err))
}

/// Un elenco di interi in JSON: `json_each` lo legge per intero in una
/// condizione sola, senza il tetto dei parametri di SQLite.
fn in_json<T: serde::Serialize + ?Sized>(elenco: &T) -> String {
    serde_json::to_string(elenco).unwrap_or_else(|_| "[]".to_owned())
}

/// Come si cerca l'altra copia di un brano ([`altre_copie`]).
#[derive(Debug, Clone, Copy)]
struct Ricerca<'a> {
    /// Righe che stanno per andarsene insieme alle cercate: mai una copia.
    in_corso: &'a HashSet<i64>,
    /// Le righe di questo dispositivo non sono copie: lo si sta dimenticando, e
    /// la domanda è cosa resterebbe.
    fuori_dal_dispositivo: Option<&'a str>,
    /// Se per `track_key` non si trova niente, si prova con la chiave di
    /// contenuto del proprietario.
    anche_per_contenuto: bool,
}

/// Dov'è ancora lo stesso brano di ciascuna di queste righe: `riga → altra`.
/// Le righe senza un'altra copia non ci sono.
///
/// È **l'unica** regola con cui lo si cerca — per le voci di playlist quando la
/// riga se ne va ([`sposta_le_voci`]), per la cronologia d'ascolto
/// ([`sposta_la_cronologia`]), per quel che «Dimentica» dice di perdere
/// ([`anteprima_dimentica`]) —: due regole diverse fanno mentire l'anteprima, e
/// una voce che in grazia seguiva la chiave di contenuto e alla scadenza no
/// (S2-1) è proprio una regola detta due volte.
///
/// Si cerca per **`track_key`**, come la sincronia e il riaggancio;
/// e solo per chi così non trova niente, e se `Ricerca::
/// anche_per_contenuto`, per **chiave di contenuto**: quella del proprietario
/// quando ha corretto un tag e le due chiavi divergono (`lontano_brani.
/// content_key`; per le righe di qui, `tracks.content_key`). L'altra riga è, nell'
/// ordine, una di questa macchina; poi quella di un altro dispositivo che il
/// proprietario manda ancora, la più vecchia. **Mai** una riga segnata sparita
/// (la voce, o l'ascolto, ci morirebbe alla conferma), né la riga stessa, né una
/// di quelle che si stanno togliendo.
///
/// Le chiavi si cercano tutte insieme, in un parametro JSON: su tremila righe una
/// scansione per chiave, col lucchetto preso, costava minuti (indici sulla
/// `track_key` e `idx_lontano_brani_content_key`).
fn altre_copie(
    tx: &Connection,
    righe: &[i64],
    ricerca: Ricerca<'_>,
) -> Result<HashMap<i64, i64>, AppError> {
    let cosa = "altra copia dello stesso brano";
    if righe.is_empty() {
        return Ok(HashMap::new());
    }
    // Le chiavi di ogni riga.
    let chiavi: Vec<(i64, String, Option<String>)> = {
        let mut st = tx
            .prepare_cached(
                "SELECT t.id, t.track_key, COALESCE(l.content_key, t.content_key)
                   FROM tracks t LEFT JOIN lontano_brani l ON l.track_id = t.id
                  WHERE t.id IN (SELECT value FROM json_each(?1))",
            )
            .map_err(|err| db_error(cosa, &err))?;
        st.query_map([in_json(righe)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|err| db_error(cosa, &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    // Le righe con quella chiave, di qui per prime. Una chiave può averne molte
    // (i brani senza tag): se ne tengono tutte, e si sceglie la prima che serve.
    let per_chiave: HashMap<String, Vec<i64>> = {
        let chiavi_cercate: BTreeSet<&str> = chiavi.iter().map(|(_, k, _)| k.as_str()).collect();
        let mut st = tx
            .prepare_cached(
                "SELECT t.track_key, t.id
                   FROM tracks t LEFT JOIN lontano_brani l ON l.track_id = t.id
                  WHERE t.track_key IN (SELECT value FROM json_each(?1))
                    AND (l.track_id IS NULL
                         OR (l.sparito_ms IS NULL AND (?2 IS NULL OR l.dispositivo <> ?2)))
                  ORDER BY (l.track_id IS NOT NULL), t.id",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map(
                params![in_json(&chiavi_cercate), ricerca.fuori_dal_dispositivo],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .map_err(|err| db_error(cosa, &err))?;
        let mut mappa: HashMap<String, Vec<i64>> = HashMap::new();
        for riga in lette {
            let (chiave, id) = riga.map_err(|err| db_error(cosa, &err))?;
            mappa.entry(chiave).or_default().push(id);
        }
        mappa
    };
    let la_prima = |riga: i64, candidate: Option<&Vec<i64>>| -> Option<i64> {
        candidate?
            .iter()
            .copied()
            .find(|id| *id != riga && !ricerca.in_corso.contains(id))
    };
    let mut trovate: HashMap<i64, i64> = HashMap::new();
    for (riga, chiave, _) in &chiavi {
        if let Some(altra) = la_prima(*riga, per_chiave.get(chiave)) {
            trovate.insert(*riga, altra);
        }
    }
    if !ricerca.anche_per_contenuto {
        return Ok(trovate);
    }
    // Per chiave di contenuto, solo per chi non ha trovato niente.
    let senza: BTreeSet<&str> = chiavi
        .iter()
        .filter(|(riga, _, _)| !trovate.contains_key(riga))
        .filter_map(|(_, _, contenuto)| contenuto.as_deref())
        .collect();
    if senza.is_empty() {
        return Ok(trovate);
    }
    let per_contenuto: HashMap<String, Vec<i64>> = {
        let mut st = tx
            .prepare_cached(
                "SELECT k, id FROM (
                   SELECT t.content_key AS k, t.id AS id, 0 AS fuori
                     FROM tracks t
                    WHERE t.content_key IN (SELECT value FROM json_each(?1))
                      AND (t.path IS NULL OR t.path < 'lontano://' OR t.path >= 'lontano:/0')
                   UNION ALL
                   SELECT l.content_key, l.track_id, 1
                     FROM lontano_brani l
                    WHERE l.content_key IN (SELECT value FROM json_each(?1))
                      AND l.sparito_ms IS NULL AND (?2 IS NULL OR l.dispositivo <> ?2)
                 ) ORDER BY fuori, id",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map(
                params![in_json(&senza), ricerca.fuori_dal_dispositivo],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .map_err(|err| db_error(cosa, &err))?;
        let mut mappa: HashMap<String, Vec<i64>> = HashMap::new();
        for riga in lette {
            let (chiave, id) = riga.map_err(|err| db_error(cosa, &err))?;
            mappa.entry(chiave).or_default().push(id);
        }
        mappa
    };
    for (riga, _, contenuto) in &chiavi {
        if trovate.contains_key(riga) {
            continue;
        }
        let candidate = contenuto.as_deref().and_then(|c| per_contenuto.get(c));
        if let Some(altra) = la_prima(*riga, candidate) {
            trovate.insert(*riga, altra);
        }
    }
    Ok(trovate)
}

/// Prima di togliere delle righe specchio: le loro voci di playlist passano a
/// un'altra riga dello stesso brano, se ce n'è una ([`altre_copie`]).
///
/// `playlist_tracks` ha `ON DELETE CASCADE`: togliere una riga con le sue voci
/// le svuoterebbe, e se la `track_key` c'è ancora su un'altra riga, per la
/// sincronia la voce che manca non è un brano assente ma un brano **tolto
/// dalla playlist**, e la lapide partirebbe verso il dispositivo che il brano
/// ce l'ha. Il caso comune: lo stesso brano su due dispositivi, e quello che si
/// ritira. `remove_tracks` lo fa nel verso opposto, con questa stessa funzione, per la
/// copia di qui che se ne va.
///
/// `per_contenuto`: se per `track_key` non c'è niente si prova con la chiave di
/// contenuto del proprietario. Vale quando la riga **se ne va** (grazia,
/// conferma, «Dimentica»): il brano ce l'ha ancora, qui, con un'altra chiave. Non
/// vale quando la riga resta e cambia chiave ([`aggiorna`]): lì la voce si
/// sposta solo per tenere la chiave vecchia dov'è ancora viva, e un'altra chiave
/// non è la stessa.
///
/// Se non c'è un'altra copia, la voce se ne va con la riga, e la sua chiave resta
/// nella sequenza della sincronia: invisibile, non tolta.
///
/// Quante voci ha spostato.
pub(crate) fn sposta_le_voci(
    tx: &Connection,
    da_togliere: &[i64],
    per_contenuto: bool,
) -> Result<usize, AppError> {
    let cosa = "voci di playlist verso un'altra copia";
    if da_togliere.is_empty() {
        return Ok(0);
    }
    // Solo le righe che hanno delle voci: sono poche, e per le altre non c'è
    // niente da cercare.
    let con_voci: Vec<i64> = {
        let mut st = tx
            .prepare_cached(
                "SELECT DISTINCT track_id FROM playlist_tracks
                  WHERE track_id IN (SELECT value FROM json_each(?1))",
            )
            .map_err(|err| db_error(cosa, &err))?;
        st.query_map([in_json(da_togliere)], |r| r.get(0))
            .map_err(|err| db_error(cosa, &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    if con_voci.is_empty() {
        return Ok(0);
    }
    let in_corso: HashSet<i64> = da_togliere.iter().copied().collect();
    let altre = altre_copie(
        tx,
        &con_voci,
        Ricerca {
            in_corso: &in_corso,
            fuori_dal_dispositivo: None,
            anche_per_contenuto: per_contenuto,
        },
    )?;
    let mut sposta = tx
        .prepare_cached("UPDATE playlist_tracks SET track_id = ?2 WHERE track_id = ?1")
        .map_err(|err| db_error(cosa, &err))?;
    let mut spostate = 0;
    for da in con_voci {
        if let Some(a) = altre.get(&da) {
            spostate += sposta
                .execute(params![da, a])
                .map_err(|err| db_error(cosa, &err))?;
        }
    }
    Ok(spostate)
}

/// Prima di togliere delle righe specchio: la cronologia d'ascolto fatta su di
/// loro passa all'altra copia dello stesso brano, se c'è ([`altre_copie`]).
///
/// `play_history` ha `ON DELETE CASCADE`, e `record_play_specchio` scrive gli
/// ascolti di un brano remoto sulla **riga specchio** — la copia migliore è di
/// norma quella del PC, anche quando qui c'è il file —: togliere la riga
/// («Dimentica», la grazia scaduta, la conferma di una strage) portava via mesi
/// di «Cronologia» di brani che sul telefono ci sono ancora (S2-2). Il
/// proprietario ha lo stesso ascolto per conto suo, ma è una vista di qui.
///
/// Un passo suo e non dentro [`sposta_le_voci`]: quella esce subito se nessuna
/// playlist ha voci, che è il caso comune. `UPDATE OR IGNORE`: un ascolto
/// nello stesso istante su tutte e due le righe è lo stesso ascolto (indice
/// unico della migrazione 20), e quel che resta sulla riga se ne va con lei. È
/// lo stesso gesto di `desiderati`, quando un brano scaricato prende il posto
/// del suo riferimento.
///
/// Senza un'altra copia gli ascolti se ne vanno con la riga: quanti, lo dice
/// l'anteprima di «Dimentica» ([`Anteprima::ascolti_persi`]). Quanti ascolti ha
/// spostato.
fn sposta_la_cronologia(tx: &Connection, da_togliere: &[i64]) -> Result<usize, AppError> {
    let cosa = "cronologia d'ascolto verso un'altra copia";
    if da_togliere.is_empty() {
        return Ok(0);
    }
    let con_ascolti: Vec<i64> = {
        let mut st = tx
            .prepare_cached(
                "SELECT DISTINCT track_id FROM play_history
                  WHERE track_id IN (SELECT value FROM json_each(?1))",
            )
            .map_err(|err| db_error(cosa, &err))?;
        st.query_map([in_json(da_togliere)], |r| r.get(0))
            .map_err(|err| db_error(cosa, &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    if con_ascolti.is_empty() {
        return Ok(0);
    }
    let in_corso: HashSet<i64> = da_togliere.iter().copied().collect();
    let altre = altre_copie(
        tx,
        &con_ascolti,
        Ricerca {
            in_corso: &in_corso,
            fuori_dal_dispositivo: None,
            anche_per_contenuto: true,
        },
    )?;
    let mut sposta = tx
        .prepare_cached("UPDATE OR IGNORE play_history SET track_id = ?2 WHERE track_id = ?1")
        .map_err(|err| db_error(cosa, &err))?;
    let mut spostati = 0;
    for da in con_ascolti {
        if let Some(a) = altre.get(&da) {
            spostati += sposta
                .execute(params![da, a])
                .map_err(|err| db_error(cosa, &err))?;
        }
    }
    Ok(spostati)
}

/// Toglie le righe date, con la guardia del percorso come per la scansione: se
/// la riga nel frattempo è un'altra, non si tocca. Prima, le voci di playlist
/// ([`sposta_le_voci`]) e la cronologia d'ascolto ([`sposta_la_cronologia`])
/// passano a un'altra copia del brano, se c'è.
///
/// E la coda salvata, con il segno di «dov'eri rimasto», smette di nominarle
/// ([`crate::playback::sgombra_il_salvato`]): `tracks.id` non è `AUTOINCREMENT`,
/// e l'id di una riga tolta lo prende il primo brano nuovo (S11-1). Chi tiene la
/// coda in memoria, il lettore, la sgombera dopo la scrittura con gli
/// identificativi che questa restituisce: stanno nello strato che lo possiede.
///
/// Gli identificativi delle righe tolte davvero, e quante voci di playlist sono
/// passate a un'altra copia.
fn togli(
    tx: &Connection,
    righe: &[(i64, String)],
    toccati: &mut Toccati,
) -> Result<Tolte, AppError> {
    let ids: Vec<i64> = righe.iter().map(|(id, _)| *id).collect();
    toccati.segna_brani(tx, &ids)?;
    let voci_spostate = sposta_le_voci(tx, &ids, true)?;
    sposta_la_cronologia(tx, &ids)?;
    let mut st = tx
        .prepare_cached("DELETE FROM tracks WHERE id = ?1 AND path = ?2")
        .map_err(|err| db_error("rimozione di una riga specchio", &err))?;
    let mut tolte = Vec::with_capacity(righe.len());
    for (id, path) in righe {
        let cancellate = st
            .execute(params![id, path])
            .map_err(|err| db_error("rimozione di una riga specchio", &err))?;
        if cancellate > 0 {
            tolte.push(*id);
        }
    }
    if !tolte.is_empty() {
        // La sincronia ricorda adesso quali brani le playlist nominano e qui non
        // ci sono: se il proprietario li rimette prima del prossimo allineamento,
        // le loro voci tornano al loro posto invece di sembrare tolte dall'utente
        // (FSYNC).
        crate::sincronia::ricorda_i_brani_assenti(tx)?;
        crate::playback::sgombra_il_salvato(tx, &tolte)?;
    }
    Ok(Tolte {
        ids: tolte,
        voci_spostate,
    })
}

/// Quel che [`togli`] ha fatto.
#[derive(Debug, Default)]
struct Tolte {
    /// Gli identificativi delle righe tolte davvero.
    ids: Vec<i64>,
    /// Le voci di playlist passate a un'altra copia prima di toglierle.
    voci_spostate: usize,
}

/// Il dispositivo si può ancora leggere?
///
/// Dentro la transazione che scrive, e non prima: fra la domanda e la scrittura
/// passano fino a trenta secondi, e la persona può aver «Dimenticato» il
/// dispositivo (la riga di `dispositivi` se ne va) o averlo revocato. Un secchio
/// che arriva dopo non deve riportare le sue righe in libreria, né la sua riga
/// di `lontano_cataloghi`.
///
/// # Errori
///
/// `lontano.nonAssociato` se non lo è più; quelli del database.
fn controlla_che_sia_associato(tx: &Connection, dispositivo: &str) -> Result<(), AppError> {
    if crate::dispositivi::associato(tx, dispositivo)? {
        Ok(())
    } else {
        Err(AppError::new(ErrorCode::LontanoNonAssociato)
            .with_cause("il dispositivo è stato dimenticato mentre si leggeva il suo catalogo"))
    }
}

/// Quali di queste righe sono in grazia: segnate sparite e nascoste. Per chi
/// mostra una playlist — le sue voci restano sulla riga — e non può mostrare
/// una riga che non si suona (Android Auto).
///
/// # Errori
///
/// Quelli del database.
pub fn in_grazia(connection: &Connection, ids: &[i64]) -> Result<HashSet<i64>, AppError> {
    let cosa = "righe in grazia";
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    let insieme = serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_owned());
    let mut st = connection
        .prepare_cached(
            "SELECT id FROM tracks
              WHERE copia_di = id AND id IN (SELECT value FROM json_each(?1))",
        )
        .map_err(|err| db_error(cosa, &err))?;
    st.query_map([&insieme], |r| r.get::<_, i64>(0))
        .map_err(|err| db_error(cosa, &err))?
        .collect::<Result<_, _>>()
        .map_err(|err| db_error(cosa, &err))
}

/// Nasconde le righe che il proprietario non manda più, e porta le loro voci di
/// playlist su un'altra copia del brano.
///
/// Una riga segnata sparita resta in libreria per la grazia ([`GRAZIA_MS`]) o
/// finché qualcuno non conferma: serve a tenerle le voci di playlist, se il
/// brano non ha altre copie. Ma **non si suona e non si conta da nessuna parte**
/// — «Tutti», l'autoplay, le playlist intelligenti, Android Auto, Cerca, i
/// conteggi —, altrimenti la perdita di dati diventa un brano fantasma che non
/// parte. Per tutte quelle strade vale già `copia_di IS NULL`, e non c'è un
/// rappresentante che una riga sparita possa avere: `copia_di` uguale al proprio
/// `id` vuol dire «nascosta» (il resto, `Origine::Dispositivo`, lo controlla da
/// sé). Quando la riga ritorna ([`aggiorna`] toglie il segno) il ricalcolo dei
/// doppioni le rida il suo posto.
///
/// Le voci passano subito alla copia di qui, se c'è ([`sposta_le_voci`]): così
/// la grazia protegge solo le voci che non hanno un'altra strada, e una voce
/// non resta su una riga che nessuno suona mentre il brano sta sul telefono.
///
/// **Un blocco**, di al più `grandezza` righe: chi ne ritira quarantamila non
/// tiene il lucchetto della libreria per quanto ci mette a nasconderle tutte.
/// Non rifà né i doppioni né gli aggregati — vedi il `//!` —, quindi un blocco
/// costa quel che nasconde.
fn nascondi_le_sparite(
    tx: &Connection,
    dispositivo: &str,
    grandezza: usize,
) -> Result<Nascoste, AppError> {
    let cosa = "righe specchio sparite da nascondere";
    // Solo quelle che non lo sono già: chi non ha niente da nascondere non
    // riscrive una riga. Una in più del blocco, per sapere se ne restano.
    let mut da_nascondere: Vec<i64> = {
        let mut st = tx
            .prepare_cached(
                "SELECT l.track_id FROM lontano_brani l JOIN tracks t ON t.id = l.track_id
                  WHERE l.dispositivo = ?1 AND l.sparito_ms IS NOT NULL
                    AND t.copia_di IS NOT t.id
                  ORDER BY l.track_id LIMIT ?2",
            )
            .map_err(|err| db_error(cosa, &err))?;
        st.query_map(
            params![
                dispositivo,
                i64::try_from(grandezza.saturating_add(1)).unwrap_or(i64::MAX)
            ],
            |r| r.get(0),
        )
        .map_err(|err| db_error(cosa, &err))?
        .collect::<Result<_, _>>()
        .map_err(|err| db_error(cosa, &err))?
    };
    let restano = da_nascondere.len() > grandezza;
    da_nascondere.truncate(grandezza);
    if da_nascondere.is_empty() {
        return Ok(Nascoste::default());
    }
    // Le voci prima di nasconderle: una riga sparita non è più un'alternativa
    // per nessuno, e `sposta_le_voci` la scarta già dalle candidate.
    let spostate = sposta_le_voci(tx, &da_nascondere, true)?;
    let mut nascondi = tx
        .prepare_cached("UPDATE tracks SET copia_di = id WHERE id = ?1")
        .map_err(|err| db_error(cosa, &err))?;
    for id in &da_nascondere {
        nascondi.execute([id]).map_err(|err| db_error(cosa, &err))?;
    }
    Ok(Nascoste {
        nascoste: da_nascondere.len(),
        spostate,
        restano,
    })
}

/// Quel che un blocco di [`nascondi_le_sparite`] ha fatto.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Nascoste {
    /// Righe nascoste in questo blocco.
    nascoste: usize,
    /// Voci di playlist passate a un'altra copia dello stesso brano.
    spostate: usize,
    /// Restano righe sparite da nascondere.
    restano: bool,
}

/// Quel che un blocco di rimozioni ha fatto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blocco {
    /// Righe tolte in questo blocco.
    pub tolte: usize,
    /// Gli identificativi delle righe tolte: chi tiene la coda del lettore la
    /// sgombera con questi, dopo la scrittura (S11-1).
    pub ids_tolti: Vec<i64>,
    /// Righe da togliere che restano: se non sono zero, si chiede un altro
    /// blocco, in un'altra presa del lucchetto.
    pub restano: usize,
    /// Righe sparite che aspettano una conferma.
    pub in_attesa: usize,
    /// Righe sparite che restano per la grazia.
    pub in_grazia: usize,
    /// Fra le righe tolte ce n'era una non ancora nascosta: i doppioni si
    /// rifanno. Le righe sparite sono già nascoste — e fuori dalla scelta dei
    /// doppioni — dal giro che le ha segnate, e toglierle non cambia quel che
    /// resta: rifare ogni volta la scelta su tutta la libreria costava, a ogni
    /// blocco, quanto il blocco.
    pub doppioni_da_rifare: bool,
    /// Righe sparite nascoste in questo blocco ([`togli_un_blocco`]).
    pub nascoste: usize,
    /// Restano righe sparite da nascondere: si chiede un altro blocco.
    pub da_nascondere: bool,
}

/// Toglie un blocco di righe sparite, in una transazione già aperta.
///
/// - **Rimozione di massa** (più di [`SOGLIA_RIMOZIONI`] righe, e più di un
///   quinto di quelle del dispositivo): non si toglie niente, e le righe
///   aspettano la conferma ([`Blocco::in_attesa`]). Si conta su **tutte** le
///   sparite, anche quelle ancora in grazia: la domanda non deve aspettare.
/// - **Altrimenti**: solo le righe scadute, cioè sparite da più di
///   [`GRAZIA_MS`], al massimo [`BLOCCO_RIMOZIONI`] per volta. Le altre
///   ([`Blocco::in_grazia`]) restano com'è.
///
/// Si ricalcola a ogni blocco, e non può che allontanarsi da «troppe»: con ogni
/// riga tolta calano le sparite e le righe del dispositivo, e il loro rapporto
/// scende.
fn togli_un_blocco_in(
    tx: &Connection,
    dispositivo: &str,
    adesso_ms: i64,
    toccati: &mut Toccati,
    blocco: usize,
) -> Result<Blocco, AppError> {
    let cosa = "righe specchio da togliere";
    // Le sparite dall'indice parziale (poche): se non ce n'è nessuna, e il
    // caso comune lo è, non si legge niente d'altro.
    let (sparite, scadute): (i64, i64) = tx
        .query_row(
            "SELECT COUNT(*), COUNT(*) FILTER (WHERE sparito_ms <= ?2)
               FROM lontano_brani WHERE dispositivo = ?1 AND sparito_ms IS NOT NULL",
            params![dispositivo, adesso_ms.saturating_sub(GRAZIA_MS)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|err| db_error(cosa, &err))?;
    let (sparite, scadute) = (
        usize::try_from(sparite).unwrap_or(0),
        usize::try_from(scadute).unwrap_or(0),
    );
    if sparite == 0 {
        return Ok(Blocco::default());
    }
    // Le righe del dispositivo, per il rapporto della rimozione di massa: dal
    // solo indice (dispositivo, id remoto).
    let totali = usize::try_from(
        tx.query_row(
            "SELECT COUNT(*) FROM lontano_brani WHERE dispositivo = ?1",
            [dispositivo],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|err| db_error(cosa, &err))?,
    )
    .unwrap_or(0);
    let strage = sparite > SOGLIA_RIMOZIONI && sparite.saturating_mul(FRAZIONE_SOSPETTA) > totali;
    if strage {
        return Ok(Blocco {
            in_attesa: sparite,
            ..Blocco::default()
        });
    }
    let (da_togliere, non_nascoste): (Vec<(i64, String)>, bool) = {
        let mut st = tx
            .prepare_cached(
                "SELECT t.id, t.path, t.copia_di IS NOT t.id
                   FROM lontano_brani l JOIN tracks t ON t.id = l.track_id
                  WHERE l.dispositivo = ?1 AND l.sparito_ms IS NOT NULL AND l.sparito_ms <= ?2
                  ORDER BY t.id LIMIT ?3",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let righe = st
            .query_map(
                params![
                    dispositivo,
                    adesso_ms.saturating_sub(GRAZIA_MS),
                    i64::try_from(blocco).unwrap_or(i64::MAX)
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, bool>(2)?)),
            )
            .map_err(|err| db_error(cosa, &err))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error(cosa, &err))?;
        let non_nascoste = righe.iter().any(|(_, _, visibile)| *visibile);
        (
            righe.into_iter().map(|(id, path, _)| (id, path)).collect(),
            non_nascoste,
        )
    };
    let ids_tolti = togli(tx, &da_togliere, toccati)?.ids;
    let tolte = ids_tolti.len();
    Ok(Blocco {
        tolte,
        ids_tolti,
        restano: scadute.saturating_sub(tolte),
        in_attesa: 0,
        in_grazia: sparite.saturating_sub(scadute),
        doppioni_da_rifare: non_nascoste,
        ..Blocco::default()
    })
}

/// Un blocco di rimozioni, in una presa sua del lucchetto: le righe sparite da
/// più della grazia, se non sono troppe. Doppioni e aggregati si rifanno. Con
/// lui, un blocco di righe sparite da nascondere
/// ([`Chiusura::da_nascondere`]).
///
/// È quel che chiude il giro quando restano righe da togliere
/// ([`Chiusura::da_togliere`]) o da nascondere, e quel che fa il giro che non ha
/// niente da leggere: le righe scadono anche se il catalogo non è cambiato. Chi
/// chiama ripete finché `restano` è zero e `da_nascondere` è falso, ma non oltre
/// [`TETTO_RIMOZIONI_PER_GIRO`] righe tolte per giro, e si ferma se un blocco non
/// ha tolto né nascosto niente.
///
/// # Errori
///
/// Quelli del database.
pub fn togli_un_blocco(
    connection: &Connection,
    dispositivo: &str,
    adesso_ms: i64,
) -> Result<Blocco, AppError> {
    togli_un_blocco_di(connection, dispositivo, adesso_ms, BLOCCO_RIMOZIONI)
}

/// [`togli_un_blocco`] con la grandezza del blocco scelta da chi chiama: le
/// prove la tengono piccola.
///
/// # Errori
///
/// Quelli del database.
pub fn togli_un_blocco_di(
    connection: &Connection,
    dispositivo: &str,
    adesso_ms: i64,
    grandezza: usize,
) -> Result<Blocco, AppError> {
    // Chi non ha righe sparite — il caso comune, a ogni giro — non apre una
    // transazione di scrittura per niente.
    if sparite_in_attesa(connection, dispositivo)? == 0 {
        return Ok(Blocco::default());
    }
    let tx = transazione(connection)?;
    let mut toccati = Toccati::nessuno();
    let mut blocco = togli_un_blocco_in(&tx, dispositivo, adesso_ms, &mut toccati, grandezza)?;
    // E un blocco di righe sparite da nascondere, se ne restano dalla chiusura
    // del giro: il lucchetto si libera fra un blocco e l'altro.
    let nascoste = nascondi_le_sparite(&tx, dispositivo, grandezza)?;
    blocco.nascoste = nascoste.nascoste;
    blocco.da_nascondere = nascoste.restano;
    if blocco.tolte > 0 {
        if blocco.doppioni_da_rifare {
            rifai_doppioni_e_album(&tx, &mut toccati)?;
        }
        ricostruisci_aggregati(&tx, &toccati)?;
    }
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione dello specchio", &err))?;
    Ok(blocco)
}

/// La persona ha detto di sì: un blocco delle righe segnate sparite di
/// `dispositivo` se ne va, **tutte** — la grazia non conta più —, e doppioni e
/// aggregati si rifanno. Chi chiama ripete finché `restano` è zero, ogni volta
/// in una presa sua del lucchetto.
///
/// # Errori
///
/// Quelli del database.
pub fn conferma_un_blocco(connection: &Connection, dispositivo: &str) -> Result<Blocco, AppError> {
    conferma_un_blocco_di(connection, dispositivo, BLOCCO_RIMOZIONI)
}

/// [`conferma_un_blocco`] con la grandezza del blocco scelta da chi chiama.
///
/// # Errori
///
/// Quelli del database.
pub fn conferma_un_blocco_di(
    connection: &Connection,
    dispositivo: &str,
    grandezza: usize,
) -> Result<Blocco, AppError> {
    let cosa = "righe specchio da togliere";
    let tx = transazione(connection)?;
    let (da_togliere, non_nascoste): (Vec<(i64, String)>, bool) = {
        let mut st = tx
            .prepare_cached(
                "SELECT t.id, t.path, t.copia_di IS NOT t.id
                   FROM lontano_brani l JOIN tracks t ON t.id = l.track_id
                  WHERE l.dispositivo = ?1 AND l.sparito_ms IS NOT NULL
                  ORDER BY t.id LIMIT ?2",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let righe = st
            .query_map(
                params![dispositivo, i64::try_from(grandezza).unwrap_or(i64::MAX)],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, bool>(2)?)),
            )
            .map_err(|err| db_error(cosa, &err))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error(cosa, &err))?;
        let non_nascoste = righe.iter().any(|(_, _, visibile)| *visibile);
        (
            righe.into_iter().map(|(id, path, _)| (id, path)).collect(),
            non_nascoste,
        )
    };
    let mut toccati = Toccati::nessuno();
    let ids_tolti = togli(&tx, &da_togliere, &mut toccati)?.ids;
    let tolte = ids_tolti.len();
    if tolte > 0 {
        // Le righe segnate sparite sono nascoste dal giro che le ha segnate, e
        // fuori dalla scelta dei doppioni: se lo sono tutte, non c'è niente da
        // rifare (vedi `Blocco::doppioni_da_rifare`).
        if non_nascoste {
            rifai_doppioni_e_album(&tx, &mut toccati)?;
        }
        ricostruisci_aggregati(&tx, &toccati)?;
    }
    let restano = sparite_in_attesa(&tx, dispositivo)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione dello specchio", &err))?;
    Ok(Blocco {
        tolte,
        ids_tolti,
        restano,
        doppioni_da_rifare: non_nascoste,
        ..Blocco::default()
    })
}

/// La persona ha detto di sì: le righe segnate sparite di `dispositivo` se ne
/// vanno, e doppioni e aggregati si rifanno. Quante righe sono sparite.
///
/// A blocchi ([`conferma_un_blocco`]), uno per transazione: chi tiene il
/// lucchetto fra un blocco e l'altro e vuole liberarlo chiama il blocco da sé.
///
/// # Errori
///
/// Quelli del database.
pub fn conferma_rimozioni(connection: &Connection, dispositivo: &str) -> Result<usize, AppError> {
    let mut tolte = 0;
    loop {
        let blocco = conferma_un_blocco(connection, dispositivo)?;
        tolte += blocco.tolte;
        if blocco.restano == 0 || blocco.tolte == 0 {
            return Ok(tolte);
        }
    }
}

/// Rifà la scelta dei doppioni, in una transazione sua: dopo una scansione, o
/// quando un brano scaricato entra in libreria. Quante righe hanno cambiato
/// `copia_di`.
///
/// # Errori
///
/// Quelli del database.
pub fn ricalcola_doppioni(connection: &Connection) -> Result<usize, AppError> {
    if !servono_i_doppioni(connection)? {
        return Ok(0);
    }
    let tx = transazione(connection)?;
    let mut toccati = Toccati::nessuno();
    let cambiate = rifai_doppioni_e_album(&tx, &mut toccati)?;
    // Allineare le chiavi d'album cambia gli album: dopo una scansione gli
    // aggregati sono già fatti, e queste chiavi no.
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione dello specchio", &err))?;
    Ok(cambiate)
}

/// [`ricalcola_doppioni`] **se serve**: per la fine di una scansione, che li
/// rifaceva a ogni passata anche senza una riga nuova, cambiata o tolta — sul
/// telefono con un PC da duecentomila righe, secondi di lucchetto per ogni
/// avvio, sveglia e raffica di eventi di una cartella (S2-5).
///
/// `cambiato`: la passata ha toccato righe di qui. Si segna **prima** di rifare, e
/// il rifacimento toglie il segno: un arresto in mezzo lascia il lavoro da fare per
/// la chiusura di un giro o per la scansione dopo. Con un segno già lì — lasciato
/// da un giro che ha scritto senza poter chiudere — si rifà lo stesso.
///
/// # Errori
///
/// Quelli del database.
pub fn ricalcola_doppioni_se_serve(
    connection: &Connection,
    cambiato: bool,
) -> Result<usize, AppError> {
    if !servono_i_doppioni(connection)? {
        return Ok(0);
    }
    if cambiato {
        segna_da_rifare(connection)?;
    } else if !e_da_rifare(connection)? {
        return Ok(0);
    }
    ricalcola_doppioni(connection)
}

#[cfg(test)]
thread_local! {
    /// Quante volte, in questo filo, si sono rifatti i doppioni e le chiavi
    /// d'album su tutta la libreria: lo guardano le prove (S2-5), che vogliono
    /// sapere se una chiusura o una scansione senza novità lo rifà.
    pub(crate) static PASSATE_DI_DOPPIONI: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// La chiave di `settings` che dice: doppioni e chiavi d'album sono **da
/// rifare**. Chi scrive qualcosa che li decide — una riga specchio nuova, sparita
/// o con un'altra chiave o qualità, una riga di qui entrata o uscita — la mette
/// nella sua transazione; chi li rifà la toglie nella sua. Così un arresto fra le
/// due cose non perde il lavoro, e non si rifà quando non c'è niente da rifare
/// (S2-5).
const DA_RIFARE: &str = "specchio.doppioni.da_rifare";

/// Segna che doppioni e chiavi d'album sono da rifare: vedi [`DA_RIFARE`].
fn segna_da_rifare(tx: &Connection) -> Result<(), AppError> {
    crate::settings::write(tx, DA_RIFARE, "1")
}

/// Chi riscrive **di qui**, fuori da una scansione e da un giro, quel che decide
/// doppioni e dischi di una riga — l'arricchimento che dà un album a un brano
/// senza, una correzione a mano dei tag, il ritorno ai tag del file — lo dice
/// qui, nella sua transazione. Senza, una chiusura che porta solo ascolti non
/// rifà niente (S2-5), e il disco di qui e quello del PC restano due finché non
/// arriva una novità vera.
///
/// Senza dispositivi collegati né copie nascoste non c'è niente da scegliere, e
/// il segno non si scrive: chi non ha un altro dispositivo non paga niente.
///
/// # Errori
///
/// Quelli del database.
pub(crate) fn doppioni_da_rifare(tx: &Connection) -> Result<(), AppError> {
    if servono_i_doppioni(tx)? {
        segna_da_rifare(tx)
    } else {
        Ok(())
    }
}

/// Doppioni e chiavi d'album sono da rifare?
fn e_da_rifare(tx: &Connection) -> Result<bool, AppError> {
    Ok(crate::settings::read(tx, DA_RIFARE)?.is_some())
}

/// La scelta dei doppioni e, subito dopo, la chiave d'album delle righe specchio
/// allineata a quella delle righe di qui. Chi chiama poi ricostruisce gli
/// aggregati con `toccati`. Quante righe hanno cambiato `copia_di`.
///
/// # Errori
///
/// Quelli del database.
pub(crate) fn rifai_doppioni_e_album(
    tx: &Connection,
    toccati: &mut Toccati,
) -> Result<usize, AppError> {
    #[cfg(test)]
    PASSATE_DI_DOPPIONI.with(|n| n.set(n.get().saturating_add(1)));
    let cambiate = ricalcola_doppioni_in(tx)?;
    riallinea_le_chiavi_d_album(tx, toccati)?;
    // Fatto tutto quel che c'era da fare: il segno non serve più.
    crate::settings::forget(tx, DA_RIFARE)?;
    Ok(cambiate)
}

/// Le righe di un album che sta su più dispositivi, con la stessa chiave
/// d'album: l'album è **una scheda sola** in «Tutti» (D4 del collaudo del 18/11).
///
/// # Il difetto
///
/// La chiave d'album di una riga specchio è del suo dispositivo
/// (`lontano:/<dispositivo>/album-…`), quella di una riga di qui è della sua
/// cartella (`storage/…`): lo stesso disco su due dispositivi dava due schede, una
/// con i brani di là e una con quelli di qui, con i conteggi di ognuna. Nel
/// collaudo, «Album Formati» da 6 e da 2 brani, e la pagina dell'artista con
/// «4 album» per due dischi.
///
/// # La regola
///
/// Le righe che hanno la stessa [`identita_album`] — titolo e artista
/// normalizzati — si guardano insieme. Se fra loro c'è una riga specchio:
///
/// - con righe di qui, le righe specchio prendono la chiave d'album di qui (la
///   chiave di qui con più righe, a parità la minore): il disco di qui è quello
///   che l'utente ha, e le copie degli altri gli si uniscono;
/// - senza righe di qui, ma con righe di **più** dispositivi, le righe di tutti
///   gli altri dispositivi prendono la chiave minore.
///
/// Due dischi di qui con lo stesso nome restano due dischi — lo dice la cartella,
/// e il resto della libreria li tiene separati —, e due dischi dello stesso
/// dispositivo anche (la chiave d'album del proprietario). Una riga che il
/// proprietario non manda più, in grazia, non partecipa.
///
/// Idempotente: una seconda chiamata non cambia niente. Le chiavi vecchie e le
/// nuove vanno in `toccati`, perché gli aggregati le rifacciano.
///
/// # Errori
///
/// Quelli del database.
pub(crate) fn riallinea_le_chiavi_d_album(
    tx: &Connection,
    toccati: &mut Toccati,
) -> Result<usize, AppError> {
    let cosa = "chiavi d'album";
    let ci_sono_righe_specchio: bool = tx
        .query_row("SELECT EXISTS (SELECT 1 FROM lontano_brani)", [], |r| {
            r.get(0)
        })
        .map_err(|err| db_error(cosa, &err))?;
    if !ci_sono_righe_specchio {
        return Ok(0);
    }
    /// Una riga, per l'allineamento.
    struct Riga {
        id: i64,
        chiave: String,
        dispositivo: Option<String>,
    }
    let mut per_identita: HashMap<String, Vec<Riga>> = HashMap::new();
    {
        let mut st = tx
            .prepare(
                "SELECT t.id, t.album_key, t.album, t.album_artist, t.artist, l.dispositivo
                   FROM tracks t LEFT JOIN lontano_brani l ON l.track_id = t.id
                  WHERE t.path IS NOT NULL AND t.album_key IS NOT NULL AND t.album_key <> ''
                    AND l.sparito_ms IS NULL",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map([], |r| {
                Ok((
                    Riga {
                        id: r.get(0)?,
                        chiave: r.get(1)?,
                        dispositivo: r.get(5)?,
                    },
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(|err| db_error(cosa, &err))?;
        for letta in lette {
            let (riga, album, album_artist, artist) = letta.map_err(|err| db_error(cosa, &err))?;
            if let Some(identita) = identita_album(&album, album_artist.as_deref(), &artist) {
                per_identita.entry(identita).or_default().push(riga);
            }
        }
    }
    let mut da_cambiare: Vec<(i64, String)> = Vec::new();
    for righe in per_identita.values() {
        if !righe.iter().any(|r| r.dispositivo.is_some()) {
            // Solo righe di qui: due cartelle con lo stesso disco restano due
            // dischi, come in ogni libreria.
            continue;
        }
        // La chiave a cui tutti si allineano, e il dispositivo che già ce l'ha
        // (`None` per una chiave di qui: tutte le righe specchio la prendono).
        let (canonica, di_chi): (&str, Option<&str>) = {
            let mut di_qui: BTreeMap<&str, usize> = BTreeMap::new();
            for r in righe.iter().filter(|r| r.dispositivo.is_none()) {
                *di_qui.entry(r.chiave.as_str()).or_insert(0) += 1;
            }
            if let Some((chiave, _)) = di_qui
                .iter()
                .max_by_key(|(chiave, quante)| (**quante, std::cmp::Reverse(**chiave)))
            {
                (*chiave, None)
            } else {
                let dispositivi: BTreeSet<&str> = righe
                    .iter()
                    .filter_map(|r| r.dispositivo.as_deref())
                    .collect();
                if dispositivi.len() < 2 {
                    continue;
                }
                let Some(minore) = righe.iter().min_by_key(|r| r.chiave.as_str()) else {
                    continue;
                };
                (minore.chiave.as_str(), minore.dispositivo.as_deref())
            }
        };
        for r in righe {
            let Some(suo) = r.dispositivo.as_deref() else {
                continue;
            };
            // Le righe del dispositivo che ha già la chiave canonica sono dello
            // stesso proprietario: i suoi dischi non si fondono fra loro.
            if r.chiave != canonica && di_chi != Some(suo) {
                da_cambiare.push((r.id, canonica.to_owned()));
            }
        }
    }
    if da_cambiare.is_empty() {
        return Ok(0);
    }
    let ids: Vec<i64> = da_cambiare.iter().map(|(id, _)| *id).collect();
    // Le chiavi di prima, e poi quelle di dopo: gli aggregati rifanno entrambe.
    toccati.segna_brani(tx, &ids)?;
    {
        let mut scrivi = tx
            .prepare_cached("UPDATE tracks SET album_key = ?2 WHERE id = ?1")
            .map_err(|err| db_error(cosa, &err))?;
        for (id, chiave) in &da_cambiare {
            scrivi
                .execute(params![id, chiave])
                .map_err(|err| db_error(cosa, &err))?;
        }
    }
    toccati.segna_brani(tx, &ids)?;
    Ok(da_cambiare.len())
}

/// C'è qualcosa da scegliere: righe specchio, o righe con una copia di una
/// scelta di prima. Chi non ha dispositivi collegati non paga niente.
fn servono_i_doppioni(connection: &Connection) -> Result<bool, AppError> {
    connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM lontano_brani)
                 OR EXISTS (SELECT 1 FROM tracks WHERE copia_di IS NOT NULL)",
            [],
            |r| r.get(0),
        )
        .map_err(|err| db_error("doppioni", &err))
}

/// [`ricalcola_doppioni`], dentro una transazione già aperta.
///
/// Una riga segnata sparita non partecipa: non è più un'opzione per nessuno, e
/// le righe che stavano dietro di lei tornano a scegliersi fra le altre.
pub(crate) fn ricalcola_doppioni_in(tx: &Connection) -> Result<usize, AppError> {
    if !servono_i_doppioni(tx)? {
        return Ok(0);
    }
    let cosa = "doppioni";
    let righe: Vec<RigaDoppione> = {
        let mut st = tx
            .prepare(
                "SELECT t.id, COALESCE(t.content_key, l.content_key, t.track_key), l.dispositivo,
                        t.codec, t.sample_rate, t.bitrate, t.copia_di, t.file_size, t.duration_ms
                   FROM tracks t LEFT JOIN lontano_brani l ON l.track_id = t.id
                  WHERE t.path IS NOT NULL AND l.sparito_ms IS NULL",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map([], |r| {
                Ok(RigaDoppione {
                    id: r.get(0)?,
                    gruppo: r.get(1)?,
                    dispositivo: r.get(2)?,
                    qualita: Qualita::da_riga(
                        r.get::<_, Option<String>>(3)?.as_deref(),
                        r.get(4)?,
                        r.get(5)?,
                        r.get(7)?,
                        r.get(8)?,
                    ),
                    copia_di: r.get(6)?,
                })
            })
            .map_err(|err| db_error(cosa, &err))?;
        lette
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    let cambi = scegli(&righe);
    let mut scrivi = tx
        .prepare_cached("UPDATE tracks SET copia_di = ?2 WHERE id = ?1")
        .map_err(|err| db_error(cosa, &err))?;
    for (id, nuovo) in &cambi {
        scrivi
            .execute(params![id, nuovo])
            .map_err(|err| db_error(cosa, &err))?;
    }
    // Le righe segnate sparite che stavano dietro a una copia: se la loro riga
    // di riferimento non c'è più, tornano visibili.
    tx.execute(
        "UPDATE tracks SET copia_di = NULL
          WHERE copia_di IS NOT NULL AND copia_di NOT IN (SELECT id FROM tracks)",
        [],
    )
    .map_err(|err| db_error(cosa, &err))?;
    Ok(cambi.len())
}

/// Una copertina presa da un dispositivo è arrivata nello store: la sua riga di
/// `cover_art` — nata senza misure — le prende. Solo se la riga è ancora
/// «presa da fuori»: se nel frattempo una scansione ha messo la copertina vera
/// di questa macchina, le sue misure restano. Quante righe ha aggiornato.
///
/// # Errori
///
/// Quelli del database.
pub fn copertina_arrivata(
    connection: &Connection,
    impronta: &str,
    larghezza: u32,
    altezza: u32,
    byte: u64,
) -> Result<usize, AppError> {
    connection
        .execute(
            "UPDATE cover_art SET width = ?2, height = ?3, byte_size = ?4
              WHERE hash = ?1 AND source = ?5",
            params![
                impronta,
                larghezza,
                altezza,
                i64::try_from(byte).unwrap_or(0),
                SORGENTE
            ],
        )
        .map_err(|err| db_error("copertina di una riga specchio", &err))
}

/// Il dispositivo con cui una riga specchio ha l'impronta di copertina
/// `impronta`, fra quelli che `fra` ammette (di solito quelli che rispondono).
///
/// # Errori
///
/// Quelli del database.
pub fn dispositivo_della_copertina(
    connection: &Connection,
    impronta: &str,
    fra: &dyn Fn(&str) -> bool,
) -> Result<Option<String>, AppError> {
    let cosa = "dispositivo di una copertina";
    let mut st = connection
        .prepare_cached("SELECT DISTINCT dispositivo FROM lontano_brani WHERE cover_remota = ?1")
        .map_err(|err| db_error(cosa, &err))?;
    let righe = st
        .query_map([impronta], |r| r.get::<_, String>(0))
        .map_err(|err| db_error(cosa, &err))?;
    for dispositivo in righe {
        let dispositivo = dispositivo.map_err(|err| db_error(cosa, &err))?;
        if fra(&dispositivo) {
            return Ok(Some(dispositivo));
        }
    }
    Ok(None)
}

/// Che cosa succederebbe alle playlist e alla cronologia dimenticando un
/// dispositivo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anteprima {
    /// Le righe specchio del dispositivo.
    pub righe: usize,
    /// Le voci di playlist su quelle righe.
    pub voci: usize,
    /// Quante restano, rimappate su un'altra copia dello stesso brano.
    pub rimappabili: usize,
    /// Quante si perdono.
    pub perse: usize,
    /// In quante playlist si perde qualcosa: il numero da dire a chi conferma.
    pub playlist_che_perdono: usize,
    /// Gli ascolti, in «Cronologia», dei brani che non hanno un'altra copia: si
    /// perdono con le righe. Quelli dei brani che l'hanno passano a lei
    /// ([`sposta_la_cronologia`]).
    pub ascolti_persi: usize,
}

/// Per ogni voce di playlist su una riga di `dispositivo`: la riga con lo
/// stesso brano che resterebbe, se c'è ([`altre_copie`]).
fn voci_con_alternativa(
    tx: &Connection,
    dispositivo: &str,
) -> Result<Vec<(i64, i64, Option<i64>)>, AppError> {
    let cosa = "voci di playlist su un dispositivo";
    let voci: Vec<(i64, i64)> = {
        let mut st = tx
            .prepare(
                "SELECT pt.playlist_id, pt.track_id
                   FROM playlist_tracks pt
                   JOIN lontano_brani l ON l.track_id = pt.track_id
                  WHERE l.dispositivo = ?1",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map([dispositivo], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|err| db_error(cosa, &err))?;
        lette
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    if voci.is_empty() {
        return Ok(Vec::new());
    }
    let brani: Vec<i64> = voci
        .iter()
        .map(|(_, brano)| *brano)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let altre = altre_copie(
        tx,
        &brani,
        Ricerca {
            in_corso: &HashSet::new(),
            fuori_dal_dispositivo: Some(dispositivo),
            anche_per_contenuto: true,
        },
    )?;
    Ok(voci
        .into_iter()
        .map(|(playlist, brano)| (playlist, brano, altre.get(&brano).copied()))
        .collect())
}

/// Quanti ascolti, in «Cronologia», stanno su righe di `dispositivo` che non
/// hanno un'altra copia: quelli che «Dimentica» perderebbe.
fn ascolti_senza_altra_copia(tx: &Connection, dispositivo: &str) -> Result<usize, AppError> {
    let cosa = "cronologia su un dispositivo";
    let per_riga: Vec<(i64, i64)> = {
        let mut st = tx
            .prepare(
                "SELECT h.track_id, COUNT(*)
                   FROM play_history h JOIN lontano_brani l ON l.track_id = h.track_id
                  WHERE l.dispositivo = ?1
                  GROUP BY h.track_id",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map([dispositivo], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|err| db_error(cosa, &err))?;
        lette
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    let brani: Vec<i64> = per_riga.iter().map(|(brano, _)| *brano).collect();
    let altre = altre_copie(
        tx,
        &brani,
        Ricerca {
            in_corso: &HashSet::new(),
            fuori_dal_dispositivo: Some(dispositivo),
            anche_per_contenuto: true,
        },
    )?;
    Ok(per_riga
        .iter()
        .filter(|(brano, _)| !altre.contains_key(brano))
        .map(|(_, quanti)| usize::try_from(*quanti).unwrap_or(0))
        .sum())
}

/// Cosa succederebbe alle playlist e alla cronologia se si dimenticasse
/// `dispositivo`: per chiedere conferma dicendo quante playlist perdono brani e
/// quanti ascolti se ne vanno.
///
/// # Errori
///
/// Quelli del database.
pub fn anteprima_dimentica(
    connection: &Connection,
    dispositivo: &str,
) -> Result<Anteprima, AppError> {
    let voci = voci_con_alternativa(connection, dispositivo)?;
    let righe = stato(connection, dispositivo)?.righe;
    let mut anteprima = Anteprima {
        righe,
        voci: voci.len(),
        ascolti_persi: ascolti_senza_altra_copia(connection, dispositivo)?,
        ..Anteprima::default()
    };
    let mut playlist = HashSet::new();
    for (lista, _, altra) in &voci {
        if altra.is_some() {
            anteprima.rimappabili += 1;
        } else {
            anteprima.perse += 1;
            playlist.insert(*lista);
        }
    }
    anteprima.playlist_che_perdono = playlist.len();
    Ok(anteprima)
}

// ── «Dimentica», a blocchi ───────────────────────────────────────────────────
//
// Le righe di un dispositivo con duecentomila brani, in una transazione sola,
// tenevano il lucchetto della libreria **un minuto** (S2-6, misurato: vedi
// `prove_revisione::misura_dimenticare_un_pc_da_duecentomila_righe`), e un
// arresto fra la riga di `dispositivi` e le righe specchio le lasciava in
// libreria per sempre, senza un dispositivo a cui appartenere (S2-3). Adesso in
// tre tempi:
//
// 1. **Il dispositivo si segna «in via di dimenticanza»** ([`comincia_a_dimenticare`]),
//    in una transazione sola e breve: tutte le sue righe diventano *sparite e
//    nascoste* — lo stato che la grazia e la conferma già conoscono — e quindi
//    non si vedono, non si suonano e non partecipano a niente; lo stato dei suoi
//    secchi e la sua posta se ne vanno; e i brani di qui che stavano dietro di
//    lui tornano a vedersi. La riga di `dispositivi` l'ha già tolta
//    `dispositivi::dimentica`: **è lei lo stato**. Un dispositivo che non c'è in
//    `dispositivi` e ha ancora delle righe è un «Dimentica» rimasto a metà.
// 2. **Si cancella a blocchi** ([`dimentica_un_blocco`]), ognuno in una
//    transazione sua e in una presa sua del lucchetto, con le voci e la
//    cronologia che passano alla copia di qui come per la conferma.
// 3. **All'avvio uno spazzino riprende** ([`dispositivi_a_meta`]) quel che un
//    arresto ha lasciato a metà.
//
// Se nel frattempo il dispositivo si ricollega — la riga torna in `dispositivi` —
// i blocchi si fermano: le righe sono di nuovo sue, e la prima lettura del suo
// catalogo (senza più secchi) le ritrova, una per una, o le lascia scadere.

/// Il dispositivo è ancora, o di nuovo, uno dei collegati?
///
/// La riga di `dispositivi` c'è, anche revocata: la revoca la dice l'altro, e
/// non è un «Dimentica». Dentro la transazione che scrive.
fn e_collegato(tx: &Connection, dispositivo: &str) -> Result<bool, AppError> {
    tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM dispositivi WHERE chiave = ?1)",
        [dispositivo],
        |r| r.get(0),
    )
    .map_err(|err| db_error("dispositivo da dimenticare", &err))
}

/// Il primo tempo, nella transazione di chi chiama: lo stato dei secchi e la
/// posta se ne vanno, tutte le righe diventano sparite e nascoste, e doppioni e
/// chiavi d'album si rifanno una volta. Chi chiama ricostruisce gli aggregati
/// con `toccati`. Quante voci di playlist sono passate a un'altra copia.
///
/// Idempotente: sulle righe già sparite e nascoste non fa niente.
fn comincia_in(
    tx: &Connection,
    dispositivo: &str,
    adesso_ms: i64,
    toccati: &mut Toccati,
) -> Result<usize, AppError> {
    let cosa = "dimenticare un dispositivo";
    // Niente impronte che, se il dispositivo si ricollega, farebbero saltare la
    // rilettura di secchi le cui righe sono nel frattempo sparite.
    for tabella in ["lontano_secchi", "lontano_cataloghi", "dispositivi_posta"] {
        tx.execute(
            &format!("DELETE FROM {tabella} WHERE dispositivo = ?1"),
            [dispositivo],
        )
        .map_err(|err| db_error(cosa, &err))?;
    }
    // Sparite da adesso: non sono un'alternativa per nessuna voce, non entrano
    // nei doppioni.
    tx.execute(
        "UPDATE lontano_brani SET sparito_ms = ?2 WHERE dispositivo = ?1 AND sparito_ms IS NULL",
        params![dispositivo, adesso_ms],
    )
    .map_err(|err| db_error(cosa, &err))?;
    // Le voci di playlist alla copia di qui, come fa `nascondi_le_sparite`: le
    // righe che ne hanno sono poche, e si cercano per la sola riga che le ha. E
    // poi tutte le righe nascoste con un'istruzione sola — `nascondi_le_sparite`
    // le scriverebbe una per una, e su duecentomila righe sono tre secondi di
    // lucchetto in più (misurati).
    let con_voci: Vec<i64> = {
        let mut st = tx
            .prepare_cached(
                "SELECT DISTINCT pt.track_id FROM playlist_tracks pt
                   JOIN lontano_brani l ON l.track_id = pt.track_id
                  WHERE l.dispositivo = ?1",
            )
            .map_err(|err| db_error(cosa, &err))?;
        st.query_map([dispositivo], |r| r.get(0))
            .map_err(|err| db_error(cosa, &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    // Qualcosa sta dietro una riga del dispositivo? Dalle righe **di fuori** dal
    // dispositivo, prima di nasconderle: i brani di qui che la copia migliore
    // del PC nascondeva, e quelli di un altro dispositivo. Se non c'è niente, non
    // c'è niente da riportare in vista — le righe del dispositivo non partecipano
    // più, e le altre restano come sono — e rifare doppioni e dischi su tutta la
    // libreria sono secondi di lucchetto per niente (S2-5).
    let c_e_chi_sta_dietro: bool = tx
        .query_row(
            "SELECT EXISTS (
               SELECT 1 FROM tracks h
                WHERE h.copia_di IS NOT NULL AND h.copia_di <> h.id
                  AND h.copia_di IN (SELECT track_id FROM lontano_brani WHERE dispositivo = ?1)
                  AND h.id NOT IN (SELECT track_id FROM lontano_brani WHERE dispositivo = ?1))",
            [dispositivo],
            |r| r.get(0),
        )
        .map_err(|err| db_error(cosa, &err))?;
    let spostate = sposta_le_voci(tx, &con_voci, true)?;
    tx.execute(
        "UPDATE tracks SET copia_di = id
          WHERE id IN (SELECT track_id FROM lontano_brani WHERE dispositivo = ?1)
            AND copia_di IS NOT id",
        [dispositivo],
    )
    .map_err(|err| db_error(cosa, &err))?;
    // I brani di qui che stavano dietro una riga del dispositivo tornano a
    // vedersi: senza, per tutta la cancellazione sarebbero invisibili.
    if c_e_chi_sta_dietro || e_da_rifare(tx)? {
        rifai_doppioni_e_album(tx, toccati)?;
    }
    Ok(spostate)
}

/// Il primo tempo di «Dimentica» ([`comincia_a_dimenticare`] e il blocco che lo
/// riprende lo condividono): il dispositivo non serve più niente, e le sue righe
/// non si vedono. In una transazione breve, a lucchetto preso.
///
/// `dispositivi::dimentica` ha già tolto la riga di `dispositivi`: se è tornata —
/// una nuova associazione nel frattempo — non si fa niente e dice `false`.
///
/// # Errori
///
/// Quelli del database.
pub fn comincia_a_dimenticare(
    connection: &Connection,
    dispositivo: &str,
    adesso_ms: i64,
) -> Result<bool, AppError> {
    comincia_a_dimenticare_con(connection, dispositivo, adesso_ms, true).map(|voci| voci.is_some())
}

/// [`comincia_a_dimenticare`] con la guardia scelta da chi chiama: le prove di
/// «Dimentica» hanno il dispositivo in `dispositivi`, e non la vogliono.
/// `Some(voci)` se ha fatto il primo tempo, con le voci passate alla copia di qui.
fn comincia_a_dimenticare_con(
    connection: &Connection,
    dispositivo: &str,
    adesso_ms: i64,
    con_la_guardia: bool,
) -> Result<Option<usize>, AppError> {
    let tx = transazione(connection)?;
    if con_la_guardia && e_collegato(&tx, dispositivo)? {
        return Ok(None);
    }
    let mut toccati = Toccati::nessuno();
    let voci = comincia_in(&tx, dispositivo, adesso_ms, &mut toccati)?;
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit()
        .map_err(|err| db_error("dimenticare un dispositivo", &err))?;
    Ok(Some(voci))
}

/// Quel che un blocco di «Dimentica» ha fatto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BloccoDimentica {
    /// Righe tolte in questo blocco.
    pub tolte: usize,
    /// I loro identificativi: chi tiene la coda del lettore la sgombera con
    /// questi, dopo la scrittura (S11-1).
    pub ids_tolti: Vec<i64>,
    /// Voci di playlist passate a un'altra copia in questo blocco.
    pub voci_spostate: usize,
    /// Non c'è più niente da togliere: chi chiama smette.
    pub finito: bool,
    /// Il dispositivo si è ricollegato: le sue righe non si toccano più.
    pub tornato: bool,
}

/// Un blocco di «Dimentica»: al più `grandezza` righe del dispositivo, in una
/// transazione sua. Chi chiama lo ripete, **ogni volta in una presa sua del
/// lucchetto**, finché `finito` — e si ferma anche se un blocco non ha tolto
/// niente, perché non si giri a vuoto.
///
/// Le voci di playlist e gli ascolti passano alla copia di qui, come per la
/// conferma ([`togli`]); gli album e gli artisti si rifanno per le sole chiavi
/// toccate. Se il primo tempo non c'è stato — un arresto fra la riga di
/// `dispositivi` e [`comincia_a_dimenticare`], o uno spazzino davanti a righe di
/// una versione di prima — lo fa qui.
///
/// # Errori
///
/// Quelli del database.
pub fn dimentica_un_blocco(
    connection: &Connection,
    dispositivo: &str,
    adesso_ms: i64,
    grandezza: usize,
) -> Result<BloccoDimentica, AppError> {
    dimentica_un_blocco_con(connection, dispositivo, adesso_ms, grandezza, true)
}

/// [`dimentica_un_blocco`] con la guardia scelta da chi chiama.
fn dimentica_un_blocco_con(
    connection: &Connection,
    dispositivo: &str,
    adesso_ms: i64,
    grandezza: usize,
    con_la_guardia: bool,
) -> Result<BloccoDimentica, AppError> {
    let cosa = "dimenticare un dispositivo";
    let tx = transazione(connection)?;
    if con_la_guardia && e_collegato(&tx, dispositivo)? {
        return Ok(BloccoDimentica {
            finito: true,
            tornato: true,
            ..BloccoDimentica::default()
        });
    }
    let mut toccati = Toccati::nessuno();
    let da_cominciare: bool = tx
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM lontano_brani
                             WHERE dispositivo = ?1 AND sparito_ms IS NULL)",
            [dispositivo],
            |r| r.get(0),
        )
        .map_err(|err| db_error(cosa, &err))?;
    let mut voci_spostate = if da_cominciare {
        comincia_in(&tx, dispositivo, adesso_ms, &mut toccati)?
    } else {
        0
    };
    let righe: Vec<(i64, String)> = {
        let mut st = tx
            .prepare_cached(
                "SELECT t.id, t.path FROM lontano_brani l JOIN tracks t ON t.id = l.track_id
                  WHERE l.dispositivo = ?1 ORDER BY t.id LIMIT ?2",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map(
                params![dispositivo, i64::try_from(grandezza).unwrap_or(i64::MAX)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|err| db_error(cosa, &err))?;
        lette
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    let tolte = togli(&tx, &righe, &mut toccati)?;
    voci_spostate += tolte.voci_spostate;
    let restano: bool = tx
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM lontano_brani WHERE dispositivo = ?1)",
            [dispositivo],
            |r| r.get(0),
        )
        .map_err(|err| db_error(cosa, &err))?;
    if !restano {
        // L'ultimo blocco: niente deve restare di lui. Anche lo stato dei secchi,
        // se qualcuno l'avesse riscritto nel frattempo.
        for tabella in ["lontano_secchi", "lontano_cataloghi", "dispositivi_posta"] {
            tx.execute(
                &format!("DELETE FROM {tabella} WHERE dispositivo = ?1"),
                [dispositivo],
            )
            .map_err(|err| db_error(cosa, &err))?;
        }
    }
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit().map_err(|err| db_error(cosa, &err))?;
    Ok(BloccoDimentica {
        tolte: tolte.ids.len(),
        ids_tolti: tolte.ids,
        voci_spostate,
        finito: !restano,
        tornato: false,
    })
}

/// I dispositivi che hanno ancora righe, o lo stato di un catalogo, e non sono
/// più collegati: un «Dimentica» rimasto a metà — un arresto, la chiusura dalla
/// barra, Windows che spegne —. Chi chiama, all'avvio, li finisce con
/// [`dimentica_un_blocco`].
///
/// # Errori
///
/// Quelli del database.
pub fn dispositivi_a_meta(connection: &Connection) -> Result<Vec<String>, AppError> {
    let cosa = "dispositivi dimenticati a metà";
    let mut st = connection
        .prepare(
            "SELECT dispositivo FROM (
               SELECT dispositivo FROM lontano_cataloghi
               UNION SELECT dispositivo FROM lontano_secchi
               UNION SELECT DISTINCT dispositivo FROM lontano_brani
             ) WHERE dispositivo NOT IN (SELECT chiave FROM dispositivi)
             ORDER BY dispositivo",
        )
        .map_err(|err| db_error(cosa, &err))?;
    let lette = st
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|err| db_error(cosa, &err))?;
    lette
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))
}

/// Dimentica un dispositivo, tutto in una volta: le sue voci di playlist e la
/// sua cronologia passano a un'altra copia dello stesso brano, dove c'è; poi se
/// ne vanno le sue righe, la posta che aspettava di partire, lo stato dei suoi
/// secchi, e doppioni e aggregati si rifanno. Quante voci ha rimappato e quante
/// righe ha tolto.
///
/// A blocchi come tutto il resto ([`comincia_a_dimenticare`],
/// [`dimentica_un_blocco`]), ma **senza lasciare il lucchetto** fra l'uno e
/// l'altro, perché chi chiama ce l'ha in mano: le prove, e basta. Il nucleo
/// chiama i blocchi da sé, ognuno in una presa sua.
///
/// Non chiede niente: chi chiede conferma, dicendo quante playlist perdono
/// brani, è chi chiama, con [`anteprima_dimentica`].
///
/// # Errori
///
/// Quelli del database.
pub fn dimentica_dispositivo(
    connection: &Connection,
    dispositivo: &str,
) -> Result<(usize, usize), AppError> {
    let adesso_ms = crate::library::now_ms();
    let mut rimappate =
        comincia_a_dimenticare_con(connection, dispositivo, adesso_ms, false)?.unwrap_or(0);
    let mut tolte = 0;
    loop {
        let blocco =
            dimentica_un_blocco_con(connection, dispositivo, adesso_ms, BLOCCO_RIMOZIONI, false)?;
        tolte += blocco.tolte;
        rimappate += blocco.voci_spostate;
        if blocco.finito || blocco.tolte == 0 {
            return Ok((rimappate, tolte));
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::library::condivisa::Segmento;
    use crate::library::generazione;
    use crate::specchio::comuni::{ADESSO, chiave, giro, giro_con, libreria, riga};

    const PC: char = 'a';

    /// Le colonne di una riga specchio che il primo giro deve scrivere: il
    /// percorso, la sorgente, la chiave di contenuto, il titolo, la chiave
    /// d'album, mi piace, voto, ascolti, la copia, i byte e la copertina.
    type RigaLetta = (
        String,
        String,
        Option<String>,
        String,
        String,
        i64,
        i64,
        i64,
        Option<i64>,
        i64,
        Option<String>,
    );

    fn n(c: &Connection, sql: &str) -> i64 {
        c.query_row(sql, [], |r| r.get(0)).expect(sql)
    }

    fn catalogo(da: i64, a: i64) -> Vec<BranoCatalogo> {
        (da..=a).map(riga).collect()
    }

    fn id_di(c: &Connection, dispositivo: &str, remoto: i64) -> i64 {
        c.query_row(
            "SELECT track_id FROM lontano_brani WHERE dispositivo = ?1 AND id_remoto = ?2",
            params![dispositivo, remoto],
            |r| r.get(0),
        )
        .expect("riga specchio")
    }

    #[test]
    fn il_primo_giro_scrive_le_righe_come_righe_di_libreria() {
        let c = libreria();
        let pc = chiave(PC);
        let mut brano = riga(7);
        brano.liked = true;
        brano.liked_at = Some(ADESSO - 5);
        brano.rating = 4;
        brano.rating_at = Some(ADESSO - 4);
        brano.stats_updated_at = ADESSO - 4;
        brano.play_count = 12;
        brano.last_played_at = Some(ADESSO - 1000);
        brano.cover_art_hash = Some("cd".repeat(32));
        let (chiusura, conti) = giro_con(&c, &pc, &[brano.clone(), riga(8), riga(600)], ADESSO);
        assert_eq!(
            (conti.inserite, conti.aggiornate, conti.invariate),
            (3, 0, 0)
        );
        assert_eq!(chiusura, Chiusura::default());
        assert_eq!(n(&c, "SELECT COUNT(*) FROM tracks"), 3);
        let id = id_di(&c, &pc, 7);
        let (
            path,
            sorgente,
            content_key,
            titolo,
            album_key,
            liked,
            rating,
            play_count,
            copia,
            size,
            cover,
        ): RigaLetta = c
            .query_row(
                "SELECT path, source, content_key, title, album_key, liked, rating, play_count,
                        copia_di, file_size, cover_art_hash FROM tracks WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(path, format!("lontano://{pc}/7"));
        assert_eq!(sorgente, "lontano");
        assert_eq!(
            content_key, None,
            "la chiave del proprietario non sta in tracks"
        );
        assert_eq!(titolo, "Titolo 7");
        assert!(
            album_key.contains(&format!("lontano:/{pc}/album-")),
            "{album_key}"
        );
        assert_eq!(
            (liked, rating, play_count),
            (1, 4, 12),
            "la copia delle sue statistiche"
        );
        assert_eq!(copia, None);
        assert_eq!(size, 8_000_000);
        assert_eq!(
            cover,
            Some("cd".repeat(32)),
            "l'impronta del proprietario: il file arriva alla prima richiesta"
        );
        let (estensione, ck, remota, impronta): (String, Option<String>, Option<String>, String) = c
            .query_row(
                "SELECT estensione, content_key, cover_remota, impronta FROM lontano_brani WHERE track_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(estensione, "mp3");
        assert_eq!(ck.as_deref(), Some("artista|titolo 7|album"));
        assert_eq!(remota, Some("cd".repeat(32)));
        assert_eq!(impronta.len(), 32);
        // I secchi e lo stato, scritti.
        assert_eq!(
            n(&c, "SELECT COUNT(*) FROM lontano_secchi"),
            2,
            "id 7 e 8 in 0, 600 in 1"
        );
        let s = stato(&c, &pc).unwrap();
        assert!(s.completo);
        assert_eq!(s.righe, 3);
        // Le chiavi d'ordine ci sono: nessuna riga in cima per nome.
        assert_eq!(
            n(
                &c,
                "SELECT COUNT(*) FROM tracks WHERE ordine_titolo IS NULL"
            ),
            0
        );
        // Gli aggregati: l'album c'è, con tre brani.
        assert_eq!(n(&c, "SELECT COUNT(*) FROM albums"), 1);
    }

    #[test]
    fn un_giro_uguale_al_precedente_non_riscrive_i_brani() {
        let c = libreria();
        let pc = chiave(PC);
        let righe = catalogo(1, 40);
        giro(&c, &pc, &righe, ADESSO);
        let prima = generazione(&c).unwrap();
        let (chiusura, conti) = giro_con(&c, &pc, &righe, ADESSO + 1000);
        assert_eq!(
            (conti.inserite, conti.aggiornate, conti.invariate),
            (0, 0, 0),
            "secchi uguali: non si chiedono"
        );
        assert_eq!(chiusura, Chiusura::default());
        assert_eq!(
            generazione(&c).unwrap(),
            prima,
            "nessuna scrittura su tracks"
        );
    }

    #[test]
    fn un_brano_cambiato_riscrive_solo_il_suo_secchio_e_solo_lui() {
        let c = libreria();
        let pc = chiave(PC);
        let mut righe = catalogo(1, 40);
        righe.extend(catalogo(600, 640));
        giro(&c, &pc, &righe, ADESSO);
        // Un ascolto sul PC: cambia il conteggio di un brano del secchio 1.
        righe[45].play_count += 1;
        let (_, conti) = giro_con(&c, &pc, &righe, ADESSO + 1000);
        assert_eq!(conti.aggiornate, 1);
        assert_eq!(conti.invariate, 40, "il resto del secchio non si tocca");
        assert_eq!(
            n(
                &c,
                &format!(
                    "SELECT play_count FROM tracks WHERE id = {}",
                    id_di(&c, &pc, righe[45].id)
                )
            ),
            4
        );
    }

    #[test]
    fn il_lavoro_chiede_solo_i_secchi_la_cui_impronta_e_cambiata() {
        let secchio = |k: i64, impronta: &str| SecchioCatalogo {
            da_id: k * 500,
            a_id: k * 500 + 499,
            impronta: impronta.repeat(32),
            righe: 10,
        };
        let riassunto = RiassuntoCatalogo {
            generazione: 9,
            secchi: vec![secchio(0, "a"), secchio(1, "b"), secchio(3, "c")],
        };
        let mut stato = StatoDispositivo {
            generazione: 9,
            completo: true,
            ..StatoDispositivo::default()
        };
        for (k, i) in [(0, "a"), (1, "x"), (2, "d")] {
            stato.secchi.insert(k * 500, (k * 500 + 499, i.repeat(32)));
        }
        let l = lavoro(&riassunto, &stato);
        assert_eq!(
            l.da_leggere.iter().map(|s| s.da_id).collect::<Vec<_>>(),
            vec![500, 1500],
            "il cambiato e il nuovo"
        );
        assert_eq!(l.da_svuotare, vec![(1000, 1499)], "quello che non c'è più");
        assert_eq!(l.righe_totali, 30);
        assert!(!l.niente);
        // Tutto uguale, stessa generazione, già letto: niente.
        let uguale = StatoDispositivo {
            generazione: 9,
            completo: true,
            secchi: riassunto
                .secchi
                .iter()
                .map(|s| (s.da_id, (s.a_id, s.impronta.clone())))
                .collect(),
            ..StatoDispositivo::default()
        };
        assert!(lavoro(&riassunto, &uguale).niente);
        // Mai letto per intero: si rilegge anche a parità.
        let incompleto = StatoDispositivo {
            completo: false,
            ..uguale
        };
        assert!(!lavoro(&riassunto, &incompleto).niente);
    }

    #[test]
    fn un_riassunto_storto_si_scarta_secchio_per_secchio() {
        let buono = SecchioCatalogo {
            da_id: 500,
            a_id: 999,
            impronta: "a".repeat(32),
            righe: 10,
        };
        let secchi = vec![
            buono.clone(),
            SecchioCatalogo {
                da_id: 501,
                ..buono.clone()
            },
            SecchioCatalogo {
                da_id: 1000,
                a_id: 1100,
                ..buono.clone()
            },
            SecchioCatalogo {
                da_id: 1500,
                impronta: "xyz".to_owned(),
                a_id: 1999,
                righe: 10,
            },
            SecchioCatalogo {
                da_id: 2000,
                a_id: 2499,
                righe: 0,
                impronta: "a".repeat(32),
            },
            SecchioCatalogo {
                da_id: 2500,
                a_id: 2999,
                righe: 501,
                impronta: "a".repeat(32),
            },
            SecchioCatalogo {
                da_id: -500,
                a_id: -1,
                ..buono.clone()
            },
            buono.clone(),
        ];
        let (buoni, scartati) = convalida_riassunto(RiassuntoCatalogo {
            generazione: 3,
            secchi,
        });
        assert_eq!(buoni.secchi, vec![buono]);
        assert_eq!(scartati, 7);
        // Il tetto di righe per dispositivo: oltre, non si prendono secchi.
        let tanti: Vec<SecchioCatalogo> = (0..(i64::try_from(MAX_RIGHE_PER_DISPOSITIVO)
            .unwrap()
            .div_euclid(500)
            + 5))
            .map(|k| SecchioCatalogo {
                da_id: k * 500,
                a_id: k * 500 + 499,
                impronta: "b".repeat(32),
                righe: 500,
            })
            .collect();
        let (buoni, scartati) = convalida_riassunto(RiassuntoCatalogo {
            generazione: 3,
            secchi: tanti,
        });
        assert_eq!(
            buoni.secchi.iter().map(|s| s.righe).sum::<i64>(),
            i64::try_from(MAX_RIGHE_PER_DISPOSITIVO).unwrap()
        );
        assert_eq!(scartati, 5);
    }

    #[test]
    fn oltre_il_tetto_di_righe_per_dispositivo_le_nuove_non_entrano() {
        let c = libreria();
        let pc = chiave(PC);
        c.execute(
            "INSERT INTO dispositivi (chiave, nome, associato_ms) VALUES (?1, 'PC', 1)",
            [&pc],
        )
        .unwrap();
        let mut giro = Giro::nuovo(&pc, MAX_RIGHE_PER_DISPOSITIVO);
        let secchio = SecchioCatalogo {
            da_id: 0,
            a_id: 499,
            impronta: "a".repeat(32),
            righe: 3,
        };
        giro.applica_secchio(&c, &secchio, catalogo(1, 3), ADESSO)
            .unwrap();
        assert_eq!(giro.conti.oltre_il_tetto, 3);
        assert_eq!(n(&c, "SELECT COUNT(*) FROM tracks"), 0);
    }

    #[test]
    fn una_riga_che_sparisce_si_toglie_dopo_il_giro_se_sono_poche() {
        let c = libreria();
        let pc = chiave(PC);
        giro(&c, &pc, &catalogo(1, 100), ADESSO);
        let id = id_di(&c, &pc, 100);
        c.execute("INSERT INTO playlists (id, playlist_key, name, created_at, updated_at) VALUES (1, 'p', 'P', 1, 1)", []).unwrap();
        c.execute(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, ?1, 0)",
            [id],
        )
        .unwrap();
        // Il PC toglie l'ultimo brano: la riga entra in grazia — resta, nascosta,
        // con la sua voce di playlist, e non c'è niente da confermare.
        let chiusura = giro(&c, &pc, &catalogo(1, 99), ADESSO + 1000);
        assert_eq!((chiusura.tolte, chiusura.in_attesa), (0, 0));
        assert_eq!(chiusura.in_grazia, 1);
        assert_eq!(n(&c, "SELECT COUNT(*) FROM tracks"), 100);
        assert_eq!(
            n(&c, "SELECT COUNT(*) FROM tracks WHERE copia_di IS NOT NULL"),
            1,
            "nascosta da «Tutti» e da tutto quel che lo imita"
        );
        assert_eq!(n(&c, "SELECT COUNT(*) FROM playlist_tracks"), 1);
        // Passata la grazia se ne va.
        let chiusura = giro(&c, &pc, &catalogo(1, 99), ADESSO + 1000 + GRAZIA_MS + 1000);
        assert_eq!(chiusura.tolte, 1);
        assert_eq!(chiusura.in_attesa, 0);
        assert_eq!(n(&c, "SELECT COUNT(*) FROM tracks"), 99);
        assert_eq!(
            n(&c, "SELECT COUNT(*) FROM playlist_tracks"),
            0,
            "la cascata, come per una scansione"
        );
        assert_eq!(
            n(
                &c,
                "SELECT COUNT(*) FROM lontano_brani WHERE sparito_ms IS NOT NULL"
            ),
            0
        );
    }

    #[test]
    fn una_strage_non_toglie_niente_e_aspetta_la_conferma() {
        let c = libreria();
        let pc = chiave(PC);
        giro(&c, &pc, &catalogo(1, 120), ADESSO);
        // Il PC sembra aver perso sessanta brani su centoventi: più di
        // cinquanta e più di un quinto.
        let chiusura = giro(&c, &pc, &catalogo(1, 60), ADESSO + 1000);
        assert_eq!(chiusura.tolte, 0);
        assert_eq!(chiusura.in_attesa, 60);
        assert_eq!(
            n(&c, "SELECT COUNT(*) FROM tracks"),
            120,
            "le righe restano dove sono"
        );
        assert_eq!(sparite_in_attesa(&c, &pc).unwrap(), 60);
        // Un sì: se ne vanno.
        assert_eq!(conferma_rimozioni(&c, &pc).unwrap(), 60);
        assert_eq!(n(&c, "SELECT COUNT(*) FROM tracks"), 60);
        assert_eq!(sparite_in_attesa(&c, &pc).unwrap(), 0);
        // E una riga che ricompare esce dall'attesa da sola.
        let c2 = libreria();
        giro(&c2, &pc, &catalogo(1, 120), ADESSO);
        giro(&c2, &pc, &catalogo(1, 60), ADESSO + 1000);
        let tornate = giro(&c2, &pc, &catalogo(1, 120), ADESSO + 2000);
        assert_eq!(tornate.in_attesa, 0);
        assert_eq!(n(&c2, "SELECT COUNT(*) FROM tracks"), 120);
    }

    #[test]
    fn un_giro_rotto_a_meta_non_decide_delle_rimozioni() {
        let c = libreria();
        let pc = chiave(PC);
        giro(&c, &pc, &catalogo(1, 10), ADESSO);
        // Il riassunto di dopo non ha più il secchio, ma il giro non finisce.
        let chiusura = Giro::nuovo(&pc, 10)
            .chiudi(&c, &[(0, 499)], false, 2, ADESSO + 1000)
            .expect("chiusura");
        assert_eq!(chiusura.tolte, 0);
        assert_eq!(chiusura.in_attesa, 10);
        assert_eq!(n(&c, "SELECT COUNT(*) FROM tracks"), 10);
        assert!(
            !stato(&c, &pc).unwrap().completo,
            "e si dice che non è completo"
        );
    }

    #[test]
    fn un_id_che_cambia_si_riabbina_sul_posto_e_le_playlist_restano() {
        // Il proprietario ha ricostruito la sua libreria: stessi brani, id
        // nuovi. Cancellare e reinserire svuoterebbe le playlist (e per la
        // sincronia sembrerebbe un brano tolto dall'utente).
        let c = libreria();
        let pc = chiave(PC);
        giro(&c, &pc, &catalogo(1, 5), ADESSO);
        let id2 = id_di(&c, &pc, 2);
        c.execute("INSERT INTO playlists (id, playlist_key, name, created_at, updated_at) VALUES (1, 'p', 'P', 1, 1)", []).unwrap();
        c.execute(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, ?1, 0)",
            [id2],
        )
        .unwrap();
        c.execute(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, 1700, 1000)",
            [id2],
        )
        .unwrap();
        // Stessi brani (stesse chiavi), id 1001..1005.
        let nuove: Vec<BranoCatalogo> = (1..=5)
            .map(|k| BranoCatalogo {
                id: 1000 + k,
                ..riga(k)
            })
            .collect();
        let chiusura = giro(&c, &pc, &nuove, ADESSO + 1000);
        assert_eq!(chiusura.riabbinate, 5);
        assert_eq!(chiusura.tolte, 0);
        assert_eq!(chiusura.in_attesa, 0);
        assert_eq!(
            n(&c, "SELECT COUNT(*) FROM tracks"),
            5,
            "nessuna riga in più"
        );
        assert_eq!(
            id_di(&c, &pc, 1002),
            id2,
            "la stessa riga, col suo id nuovo"
        );
        assert_eq!(
            n(
                &c,
                &format!("SELECT COUNT(*) FROM playlist_tracks WHERE track_id = {id2}")
            ),
            1,
            "la playlist ha ancora il suo brano"
        );
        assert_eq!(
            n(
                &c,
                &format!("SELECT COUNT(*) FROM play_history WHERE track_id = {id2}")
            ),
            1
        );
        let path: String = c
            .query_row("SELECT path FROM tracks WHERE id = ?1", [id2], |r| r.get(0))
            .unwrap();
        assert_eq!(path, format!("lontano://{pc}/1002"));
    }

    #[test]
    fn un_voto_dato_qui_e_in_posta_non_si_riscrive_con_quel_che_l_altro_aveva() {
        let c = libreria();
        let pc = chiave(PC);
        let mut righe = catalogo(1, 3);
        giro(&c, &pc, &righe, ADESSO);
        let id = id_di(&c, &pc, 2);
        // Qui si è messo un mi piace e si sono date quattro stelle, e aspettano
        // in posta.
        c.execute(
            "UPDATE tracks SET liked = 1, liked_at = ?2, rating = 4, rating_at = ?2, stats_updated_at = ?2 WHERE id = ?1",
            params![id, ADESSO + 500],
        )
        .unwrap();
        c.execute(
            "INSERT INTO dispositivi_posta (dispositivo, tipo, chiave, corpo, quando_ms) VALUES (?1, 'valuta', '2', '{}', ?2)",
            params![pc, ADESSO + 500],
        )
        .unwrap();
        // Il PC intanto ha contato un ascolto, e dice ancora non-mi-piace.
        righe[1].play_count = 9;
        giro(&c, &pc, &righe, ADESSO + 1000);
        let (liked, rating, play_count): (i64, i64, i64) = c
            .query_row(
                "SELECT liked, rating, play_count FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (liked, rating),
            (1, 4),
            "il voto di qui aspetta di arrivare"
        );
        assert_eq!(
            play_count, 9,
            "il conteggio è del proprietario e si aggiorna"
        );
        // Arrivata la valutazione, il PC dice la stessa cosa e tutto torna a
        // seguirlo.
        c.execute("DELETE FROM dispositivi_posta", []).unwrap();
        righe[1].liked = true;
        righe[1].liked_at = Some(ADESSO + 500);
        righe[1].rating = 4;
        righe[1].rating_at = Some(ADESSO + 500);
        righe[1].stats_updated_at = ADESSO + 500;
        giro(&c, &pc, &righe, ADESSO + 2000);
        let liked: i64 = c
            .query_row("SELECT liked FROM tracks WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(liked, 1);
    }

    #[test]
    fn un_brano_d_immagine_cd_tiene_il_suo_pezzo_fuori_da_tracks() {
        let c = libreria();
        let pc = chiave(PC);
        let mut r = riga(3);
        r.segmento = Some(Segmento {
            inizio_cd: 750,
            fine_cd: Some(1500),
        });
        giro(&c, &pc, &[r], ADESSO);
        let id = id_di(&c, &pc, 3);
        let (immagine, inizio_tracks): (Option<String>, Option<i64>) = c
            .query_row(
                "SELECT immagine, inizio_cd FROM tracks WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (immagine, inizio_tracks),
            (None, None),
            "le colonne di tracks vogliono un file locale"
        );
        let (inizio, fine): (Option<i64>, Option<i64>) = c
            .query_row(
                "SELECT inizio_cd, fine_cd FROM lontano_brani WHERE track_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((inizio, fine), (Some(750), Some(1500)));
    }

    #[test]
    fn la_copertina_di_una_riga_nasce_prima_del_file_e_poi_prende_le_misure() {
        let c = libreria();
        let pc = chiave(PC);
        let ef = "ef".repeat(32);
        let mut a = riga(1);
        a.cover_art_hash = Some(ef.clone());
        let mut b = riga(2);
        b.cover_art_hash = Some(ef.clone());
        let senza = BranoCatalogo {
            cover_art_hash: None,
            ..riga(3)
        };
        giro(&c, &pc, &[a, b, senza], ADESSO);
        // La finestra ne conosce l'impronta e la chiederà.
        assert_eq!(
            n(
                &c,
                "SELECT COUNT(*) FROM tracks WHERE cover_art_hash IS NOT NULL"
            ),
            2
        );
        let (sorgente, larghezza): (String, Option<i64>) = c
            .query_row(
                "SELECT source, width FROM cover_art WHERE hash = ?1",
                [&ef],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((sorgente.as_str(), larghezza), ("lontano", None));
        // Il dispositivo a cui chiederla: uno di quelli ammessi.
        assert_eq!(
            dispositivo_della_copertina(&c, &ef, &|_| true).unwrap(),
            Some(pc.clone())
        );
        assert_eq!(
            dispositivo_della_copertina(&c, &ef, &|_| false).unwrap(),
            None
        );
        assert_eq!(
            dispositivo_della_copertina(&c, &"00".repeat(32), &|_| true).unwrap(),
            None
        );
        // Arrivata, la riga prende le misure.
        assert_eq!(copertina_arrivata(&c, &ef, 320, 300, 20_000).unwrap(), 1);
        let (l, a, byte): (i64, i64, i64) = c
            .query_row(
                "SELECT width, height, byte_size FROM cover_art WHERE hash = ?1",
                [&ef],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((l, a, byte), (320, 300, 20_000));
        // Se una scansione ha messo quella vera, le sue misure non si toccano.
        c.execute(
            "UPDATE cover_art SET source = 'tag', width = 640 WHERE hash = ?1",
            [&ef],
        )
        .unwrap();
        assert_eq!(copertina_arrivata(&c, &ef, 320, 300, 20_000).unwrap(), 0);
        assert_eq!(n(&c, "SELECT width FROM cover_art"), 640);
        // E un giro dopo la riga la tiene.
        let mut b2 = riga(2);
        b2.cover_art_hash = Some(ef.clone());
        b2.play_count = 77;
        giro(&c, &pc, &[riga(1), b2, riga(3)], ADESSO + 3);
        let id2 = id_di(&c, &pc, 2);
        let cover: Option<String> = c
            .query_row(
                "SELECT cover_art_hash FROM tracks WHERE id = ?1",
                [id2],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cover, Some(ef));
    }

    #[test]
    fn una_copertina_gia_di_questa_macchina_resta_la_sua() {
        // Lo stesso disco c'è in locale, con la sua copertina e il suo file: la
        // riga di `cover_art` non si tocca, e nessuno la rifà «da fuori».
        let c = libreria();
        let pc = chiave(PC);
        let ef = "ef".repeat(32);
        c.execute(
            "INSERT INTO cover_art (hash, mime_type, width, height, byte_size, source, created_at)
             VALUES (?1, 'image/jpeg', 640, 640, 90000, 'tag', 1)",
            [&ef],
        )
        .unwrap();
        let mut r = riga(1);
        r.cover_art_hash = Some(ef.clone());
        giro(&c, &pc, &[r], ADESSO);
        let (sorgente, larghezza): (String, i64) = c
            .query_row(
                "SELECT source, width FROM cover_art WHERE hash = ?1",
                [&ef],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((sorgente.as_str(), larghezza), ("tag", 640));
        assert_eq!(copertina_arrivata(&c, &ef, 320, 320, 1).unwrap(), 0);
    }

    #[test]
    fn i_doppioni_si_rifanno_dopo_il_giro_e_dopo_un_dispositivo_dimenticato() {
        let c = libreria();
        let pc = chiave(PC);
        // Un brano locale in mp3, lo stesso brano sul PC in flac.
        c.execute(
            "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, file_size,
                                 date_added, date_modified, codec, bitrate, sample_rate)
             VALUES (1, 'C:/M/a.mp3', 'artista|titolo 7|album', 'artista|titolo 7|album', 'Titolo 7',
                     'Artista', 'Album', 1000, 1, 1, 'Mpeg', 320, 44100)",
            [],
        )
        .unwrap();
        let mut remota = riga(7);
        remota.codec = Some("Flac".to_owned());
        remota.bitrate = None;
        let chiusura = giro(&c, &pc, &[remota], ADESSO);
        assert_eq!(chiusura.copie_cambiate, 1);
        let remota_id = id_di(&c, &pc, 7);
        assert_eq!(
            n(&c, "SELECT copia_di FROM tracks WHERE id = 1"),
            remota_id,
            "la locale sta dietro la flac del PC"
        );
        assert_eq!(
            n(
                &c,
                &format!("SELECT COUNT(*) FROM tracks WHERE id = {remota_id} AND copia_di IS NULL")
            ),
            1
        );
        // Il PC viene dimenticato: la locale torna visibile.
        let (_, tolte) = dimentica_dispositivo(&c, &pc).unwrap();
        assert_eq!(tolte, 1);
        assert_eq!(
            n(&c, "SELECT COUNT(*) FROM tracks WHERE copia_di IS NOT NULL"),
            0
        );
        assert_eq!(n(&c, "SELECT COUNT(*) FROM tracks"), 1);
    }

    #[test]
    fn dimenticare_un_dispositivo_rimappa_le_voci_e_dice_quante_playlist_perdono() {
        let c = libreria();
        let pc = chiave(PC);
        c.execute(
            "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, file_size,
                                 date_added, date_modified)
             VALUES (1, 'C:/M/a.mp3', 'artista|titolo 7|album', 'artista|titolo 7|album', 'Titolo 7',
                     'Artista', 'Album', 1000, 1, 1)",
            [],
        )
        .unwrap();
        giro(&c, &pc, &[riga(7), riga(8)], ADESSO);
        let (r7, r8) = (id_di(&c, &pc, 7), id_di(&c, &pc, 8));
        for (id, nome) in [(1, "P1"), (2, "P2")] {
            c.execute(
                "INSERT INTO playlists (id, playlist_key, name, created_at, updated_at) VALUES (?1, ?2, ?2, 1, 1)",
                params![id, nome],
            )
            .unwrap();
        }
        // P1 ha il brano 7 (che esiste anche qui) e l'8 (che c'è solo sul PC);
        // P2 ha solo il 7.
        for (playlist, brano, posizione) in [(1, r7, 0), (1, r8, 1), (2, r7, 0)] {
            c.execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
                params![playlist, brano, posizione],
            )
            .unwrap();
        }
        let a = anteprima_dimentica(&c, &pc).unwrap();
        assert_eq!(
            a,
            Anteprima {
                righe: 2,
                voci: 3,
                rimappabili: 2,
                perse: 1,
                playlist_che_perdono: 1,
                ascolti_persi: 0
            }
        );
        let (rimappate, tolte) = dimentica_dispositivo(&c, &pc).unwrap();
        assert_eq!((rimappate, tolte), (2, 2));
        assert_eq!(
            n(
                &c,
                "SELECT COUNT(*) FROM playlist_tracks WHERE track_id = 1"
            ),
            2,
            "le voci sul 7 vanno alla locale"
        );
        assert_eq!(
            n(&c, "SELECT COUNT(*) FROM playlist_tracks"),
            2,
            "quella sull'8 si perde"
        );
        assert_eq!(stato(&c, &pc).unwrap(), StatoDispositivo::default());
        assert_eq!(n(&c, "SELECT COUNT(*) FROM lontano_brani"), 0);
    }
}
