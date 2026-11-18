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
//!   quelle del dispositivo, si tolgono;
//! - **oltre**, restano segnate e aspettano una conferma
//!   ([`conferma_rimozioni`]).
//!
//! # Le statistiche
//!
//! Quelle di una riga specchio sono una copia **in sola lettura** di quelle del
//! proprietario, che le conta e fa lo scrobble: qui non si sommano mai. Il
//! solo scarto è un voto o un mi piace dato qui e non ancora arrivato (c'è una
//! voce in `dispositivi_posta`): finché aspetta, il riconciliatore non lo
//! riscrive con quel che l'altro aveva prima.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};
use rymembra_domain::album::album_group_key;
use rymembra_domain::errors::AppError;
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

/// Riscrive una riga specchio che già c'è. `tieni_il_mio_voto`: c'è una
/// valutazione in posta per questo brano, e quel che si è deciso qui vale più
/// di quel che l'altro aveva prima.
fn aggiorna(
    tx: &Connection,
    track_id: i64,
    riga: &BranoCatalogo,
    impronta: &str,
    dispositivo: &str,
    tieni_il_mio_voto: bool,
    adesso_ms: i64,
) -> Result<(), AppError> {
    let cosa = "aggiornamento di una riga specchio";
    let cover = registra_la_copertina(tx, riga.cover_art_hash.as_deref(), adesso_ms)?;
    tx.execute(
        "UPDATE tracks SET
           track_key = ?2, title = ?3, artist = ?4, album = ?5, album_artist = ?6,
           album_key = ?7, year = ?8, track_number = ?9, disc_number = ?10, genre = ?11,
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
    Ok(())
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
            righe_adesso,
            conti: Conti::default(),
        }
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
        let esistenti = leggi_esistenti(&tx, &self.dispositivo, secchio.da_id, secchio.a_id)?;
        let in_posta = brani_in_posta(&tx, &self.dispositivo)?;
        let prima: Vec<i64> = esistenti.values().map(|e| e.track_id).collect();
        self.toccati.segna_brani(&tx, &prima)?;
        let mut visti = HashSet::new();
        let mut toccate = Vec::new();
        let mut nuove = Vec::new();
        let mut conti = Conti::default();
        let mut righe_adesso = self.righe_adesso;
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
                    if self.tieni_le_nuove {
                        nuove.push((id, riga));
                    }
                }
                Some(e) if e.impronta == impronta && !e.sparito => conti.invariate += 1,
                Some(e) => {
                    let mio = in_posta.contains(&riga.id);
                    aggiorna(
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
        let mut chiusura = Chiusura::default();
        for (da_id, a_id) in da_svuotare {
            let ids = segna_sparite_in(&tx, &self.dispositivo, *da_id, *a_id, adesso_ms)?;
            chiusura.segnate_sparite += ids;
            tx.execute(
                "DELETE FROM lontano_secchi WHERE dispositivo = ?1 AND da_id = ?2",
                params![self.dispositivo, da_id],
            )
            .map_err(|err| db_error("secchio svuotato", &err))?;
        }
        if completo {
            chiusura.riabbinate = self.riabbina(&tx, adesso_ms)?;
            let (tolte, in_attesa) = self.togli_le_sparite(&tx)?;
            chiusura.tolte = tolte;
            chiusura.in_attesa = in_attesa;
        } else {
            chiusura.in_attesa = sparite_in_attesa(&tx, &self.dispositivo)?;
        }
        chiusura.copie_cambiate = ricalcola_doppioni_in(&tx)?;
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
            tx.execute(
                "UPDATE lontano_brani SET id_remoto = ?2 WHERE track_id = ?1",
                params![vecchio, riga.id],
            )
            .map_err(|err| db_error("riga specchio riabbinata", &err))?;
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

    /// Toglie le righe ancora sparite, se sono poche; se no le lascia segnate e
    /// dice quante aspettano. `(tolte, in_attesa)`.
    fn togli_le_sparite(&mut self, tx: &Connection) -> Result<(usize, usize), AppError> {
        let sparite = sparite_con_percorso(tx, &self.dispositivo)?;
        if sparite.is_empty() {
            return Ok((0, 0));
        }
        let totali = usize::try_from(
            tx.query_row(
                "SELECT COUNT(*) FROM lontano_brani WHERE dispositivo = ?1",
                [&self.dispositivo],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|err| db_error("righe di un dispositivo", &err))?,
        )
        .unwrap_or(0);
        let strage = sparite.len() > SOGLIA_RIMOZIONI
            && sparite.len().saturating_mul(FRAZIONE_SOSPETTA) > totali;
        if strage {
            return Ok((0, sparite.len()));
        }
        Ok((togli(tx, &sparite, &mut self.toccati)?, 0))
    }
}

/// Com'è andato il giro.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Chiusura {
    /// Righe riabbinate a un id nuovo, sul posto.
    pub riabbinate: usize,
    /// Righe tolte.
    pub tolte: usize,
    /// Righe sparite che aspettano una conferma.
    pub in_attesa: usize,
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

/// Le righe segnate sparite di un dispositivo, col loro percorso.
fn sparite_con_percorso(
    tx: &Connection,
    dispositivo: &str,
) -> Result<Vec<(i64, String)>, AppError> {
    let cosa = "righe specchio sparite";
    let mut st = tx
        .prepare_cached(
            "SELECT t.id, t.path FROM lontano_brani l JOIN tracks t ON t.id = l.track_id
              WHERE l.dispositivo = ?1 AND l.sparito_ms IS NOT NULL ORDER BY t.id",
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

/// Toglie le righe date, con la guardia del percorso come per la scansione: se
/// la riga nel frattempo è un'altra, non si tocca.
fn togli(
    tx: &Connection,
    righe: &[(i64, String)],
    toccati: &mut Toccati,
) -> Result<usize, AppError> {
    let ids: Vec<i64> = righe.iter().map(|(id, _)| *id).collect();
    toccati.segna_brani(tx, &ids)?;
    let mut st = tx
        .prepare_cached("DELETE FROM tracks WHERE id = ?1 AND path = ?2")
        .map_err(|err| db_error("rimozione di una riga specchio", &err))?;
    let mut tolte = 0;
    for (id, path) in righe {
        tolte += st
            .execute(params![id, path])
            .map_err(|err| db_error("rimozione di una riga specchio", &err))?;
    }
    if tolte > 0 {
        // La sincronia ricorda adesso quali brani le playlist nominano e qui non
        // ci sono: se il proprietario li rimette prima del prossimo allineamento,
        // le loro voci tornano al loro posto invece di sembrare tolte dall'utente
        // (FSYNC).
        crate::sincronia::ricorda_i_brani_assenti(tx)?;
    }
    Ok(tolte)
}

/// La persona ha detto di sì: le righe segnate sparite di `dispositivo` se ne
/// vanno, e doppioni e aggregati si rifanno. Quante righe sono sparite.
///
/// # Errori
///
/// Quelli del database.
pub fn conferma_rimozioni(connection: &Connection, dispositivo: &str) -> Result<usize, AppError> {
    let tx = transazione(connection)?;
    let sparite = sparite_con_percorso(&tx, dispositivo)?;
    let mut toccati = Toccati::nessuno();
    let tolte = togli(&tx, &sparite, &mut toccati)?;
    ricalcola_doppioni_in(&tx)?;
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione dello specchio", &err))?;
    Ok(tolte)
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
    let cambiate = ricalcola_doppioni_in(&tx)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione dello specchio", &err))?;
    Ok(cambiate)
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
                        t.codec, t.sample_rate, t.bitrate, t.album, t.album_artist, t.artist,
                        t.copia_di
                   FROM tracks t LEFT JOIN lontano_brani l ON l.track_id = t.id
                  WHERE t.path IS NOT NULL AND l.sparito_ms IS NULL",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map([], |r| {
                let album: String = r.get(6)?;
                let album_artist: Option<String> = r.get(7)?;
                let artist: String = r.get(8)?;
                Ok(RigaDoppione {
                    id: r.get(0)?,
                    gruppo: r.get(1)?,
                    dispositivo: r.get(2)?,
                    qualita: Qualita::da_colonne(
                        r.get::<_, Option<String>>(3)?.as_deref(),
                        r.get(4)?,
                        r.get(5)?,
                    ),
                    album: identita_album(&album, album_artist.as_deref(), &artist),
                    copia_di: r.get(9)?,
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

/// Che cosa succederebbe alle playlist dimenticando un dispositivo.
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
}

/// Per ogni voce di playlist su una riga di `dispositivo`: la riga di un altro
/// dispositivo — questo, di preferenza — con lo stesso brano, se c'è.
fn voci_con_alternativa(
    tx: &Connection,
    dispositivo: &str,
) -> Result<Vec<(i64, i64, Option<i64>)>, AppError> {
    let cosa = "voci di playlist su un dispositivo";
    let voci: Vec<(i64, i64, String)> = {
        let mut st = tx
            .prepare(
                "SELECT pt.playlist_id, pt.track_id, COALESCE(l.content_key, t.track_key)
                   FROM playlist_tracks pt
                   JOIN lontano_brani l ON l.track_id = pt.track_id
                   JOIN tracks t ON t.id = pt.track_id
                  WHERE l.dispositivo = ?1",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map([dispositivo], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|err| db_error(cosa, &err))?;
        lette
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    // Per ogni chiave, una volta sola: la riga di un altro dispositivo, questo
    // per primo.
    let mut alternativa: HashMap<String, Option<i64>> = HashMap::new();
    let mut cerca = tx
        .prepare_cached(
            "SELECT t.id FROM tracks t LEFT JOIN lontano_brani l ON l.track_id = t.id
              WHERE (t.content_key = ?1 OR t.track_key = ?1 OR l.content_key = ?1)
                AND t.path IS NOT NULL
                AND (l.dispositivo IS NULL OR (l.dispositivo <> ?2 AND l.sparito_ms IS NULL))
              ORDER BY (l.dispositivo IS NULL) DESC, t.id LIMIT 1",
        )
        .map_err(|err| db_error(cosa, &err))?;
    let mut fuori = Vec::with_capacity(voci.len());
    for (playlist, brano, chiave) in voci {
        let altra = match alternativa.get(&chiave) {
            Some(a) => *a,
            None => {
                let trovata: Option<i64> = cerca
                    .query_row(params![chiave, dispositivo], |r| r.get(0))
                    .optional()
                    .map_err(|err| db_error(cosa, &err))?;
                alternativa.insert(chiave, trovata);
                trovata
            }
        };
        fuori.push((playlist, brano, altra));
    }
    Ok(fuori)
}

/// Cosa succederebbe alle playlist se si dimenticasse `dispositivo`: per
/// chiedere conferma dicendo quante playlist perdono brani.
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

/// Dimentica un dispositivo: le sue voci di playlist passano a un'altra copia
/// dello stesso brano, dove c'è; poi se ne vanno le sue righe, la posta che
/// aspettava di partire, lo stato dei suoi secchi, e doppioni e aggregati si
/// rifanno. Quante voci ha rimappato e quante righe ha tolto.
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
    let cosa = "dimenticare un dispositivo";
    let tx = transazione(connection)?;
    let voci = voci_con_alternativa(&tx, dispositivo)?;
    let mut rimappate = 0;
    for (playlist, brano, altra) in &voci {
        if let Some(altra) = altra {
            rimappate += tx
                .execute(
                    "UPDATE playlist_tracks SET track_id = ?3 WHERE playlist_id = ?1 AND track_id = ?2",
                    params![playlist, brano, altra],
                )
                .map_err(|err| db_error(cosa, &err))?;
        }
    }
    let righe: Vec<(i64, String)> = {
        let mut st = tx
            .prepare(
                "SELECT t.id, t.path FROM lontano_brani l JOIN tracks t ON t.id = l.track_id
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
    let mut toccati = Toccati::nessuno();
    let tolte = togli(&tx, &righe, &mut toccati)?;
    for tabella in ["lontano_secchi", "lontano_cataloghi", "dispositivi_posta"] {
        tx.execute(
            &format!("DELETE FROM {tabella} WHERE dispositivo = ?1"),
            [dispositivo],
        )
        .map_err(|err| db_error(cosa, &err))?;
    }
    ricalcola_doppioni_in(&tx)?;
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit().map_err(|err| db_error(cosa, &err))?;
    Ok((rimappate, tolte))
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
        assert_eq!(size, 5_000_000);
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
        // Il PC toglie l'ultimo brano.
        let chiusura = giro(&c, &pc, &catalogo(1, 99), ADESSO + 1000);
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
                playlist_che_perdono: 1
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
