//! Il formato del backup, e il SQL che lo riempie e lo rimette a posto.
//!
//! Una libreria di Aether si ricostruisce scansionando il disco: i file sono il
//! dato, il database è un indice. Quel che una scansione **non** sa ricostruire
//! è tutto qui dentro — conteggi d'ascolto, voti, preferiti, playlist, cartelle
//! sorvegliate, skin — ed è l'unica parte che, se il computer si formatta, non
//! torna da nessuna parte.
//!
//! Questo modulo è la metà impura del ripristino: i tipi che vanno e vengono da
//! Drive, il SQL che li legge e li riscrive. Le **decisioni** stanno in
//! [`aether_domain::restore`], che non ha né serde né database. La linea di
//! divisione è che uno schema non è una decisione: cambiare il nome di un campo
//! JSON non può far perdere una riga di libreria, cambiare la regola di fusione
//! sì.
//!
//! # Cosa non si salva, e perché
//!
//! - **I file audio.** Sono il dato; sono anche decine di gigabyte, e nessuno
//!   vuole scoprire che l'applicazione musicale gli ha riempito il Drive.
//! - **Le copertine.** Si ricavano dai tag e dalla rete, e occupano più di tutto
//!   il resto messo insieme.
//! - **Le istantanee dello Studio** (`skin\bozze\<id>\istantanee\*.json`). Sono
//!   una cronologia di annullamento locale, riscritta a ogni salvataggio: la
//!   cosa più rumorosa del progetto, anche sulla rete. Della bozza si salva il
//!   solo `skin.json`.
//!
//! Scritto qui perché la tentazione di «aggiungerci anche» è forte e arriva
//! sempre da qualcuno che non ha visto la bolletta.
//!
//! # Il contenuto e la busta
//!
//! [`Salvataggio`] è il file; [`Contenuto`] è ciò che il file dice. La data e il
//! nome del dispositivo stanno sulla busta e **non** dentro, così due computer
//! con la stessa libreria calcolano la stessa [`impronta`] — che è tutto ciò su
//! cui si regge la scorciatoia «non è cambiato niente, non ricaricare».
//!
//! # Indulgenza in lettura
//!
//! [`interpreta`] butta via il **singolo record** malformato e tiene il file. Un
//! backup è quello che si legge nel giorno peggiore: rifiutare millequattrocento
//! brani perché uno ha un campo storto è il modo di trasformare un guasto
//! piccolo in una perdita totale. Il file si rifiuta solo quando non è un
//! oggetto — cioè quando non c'è niente da leggere.

use std::collections::{BTreeMap, BTreeSet};

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::keys::{PlaylistKey, TrackKey};
use aether_domain::merge::{TrackStats, merge_stats};
use aether_domain::restore::{PlaylistChange, PlaylistState, RestorePlan, RootToAdd, TrackChange};
use rusqlite::{Connection, Transaction};

use crate::library::db_error;
use crate::settings::{self, CHIAVE_CARTELLE, CHIAVE_SKIN};

/// La versione del formato scritta nei file nuovi.
///
/// Si legge anche un file di versione **minore** senza dire niente: i campi che
/// non c'erano prendono il loro valore di serie. Un file di versione
/// **maggiore** invece si rifiuta — vedi [`interpreta`].
pub const VERSIONE: u32 = 1;

/// Il nome del servizio nei codici d'errore di rete.
const SERVIZIO: &str = "drive";

// ── i tipi del file ─────────────────────────────────────────────────────────

/// Le statistiche di un brano, come stanno nel file.
///
/// L'identità è [`Self::key`], cioè `artista|titolo|album` normalizzato: né il
/// percorso né l'identificativo di riga attraversano una reinstallazione.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranoSalvato {
    /// L'identità fra dispositivi.
    pub key: String,
    /// Quante volte è stato ascoltato.
    #[serde(default, skip_serializing_if = "e_zero_i64")]
    pub play_count: i64,
    /// L'ultimo ascolto, in millisecondi.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played_at: Option<i64>,
    /// Il voto, da 0 (nessuno) a 5.
    #[serde(default, skip_serializing_if = "e_zero_u8")]
    pub rating: u8,
    /// È fra i preferiti.
    #[serde(default, skip_serializing_if = "e_falso")]
    pub liked: bool,
    /// Quando la preferenza è stata espressa — anche quando è stata **tolta**.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liked_at: Option<i64>,
    /// Quando voto o preferito sono stati toccati l'ultima volta.
    #[serde(default, skip_serializing_if = "e_zero_i64")]
    pub stats_updated_at: i64,
}

/// Una playlist, come sta nel file.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSalvata {
    /// L'identità: il nome normalizzato.
    pub key: String,
    /// Il nome come si scrive.
    pub name: String,
    /// La descrizione, se c'è.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Quando è stata creata, in millisecondi.
    #[serde(default, skip_serializing_if = "e_zero_i64")]
    pub created_at: i64,
    /// Quando è stata toccata l'ultima volta, in millisecondi.
    ///
    /// È l'orologio che decide chi vince fra due versioni della stessa
    /// playlist.
    #[serde(default, skip_serializing_if = "e_zero_i64")]
    pub updated_at: i64,
    /// È automatica: l'appartenenza la decidono le regole.
    #[serde(default, skip_serializing_if = "e_falso")]
    pub is_smart: bool,
    /// Le regole, per una playlist automatica.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<String>,
    /// I membri, nell'ordine, come chiavi di brano.
    ///
    /// Vuoto per una playlist automatica: ogni dispositivo la ricalcola, ed è
    /// per questo che resta vera anche sui brani che l'altro non ha.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
}

/// Le cancellazioni, perché possano viaggiare.
///
/// Senza, l'altro dispositivo vedrebbe soltanto «a me manca una playlist» e la
/// rimanderebbe indietro: la cosa cancellata tornerebbe da sola, che è il modo
/// più rapido di far perdere fiducia in un backup.
///
/// Chiave → millisecondo della cancellazione.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lapidi {
    /// I brani tolti dalla libreria.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<String, i64>,
    /// Le playlist cancellate.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub playlists: BTreeMap<String, i64>,
}

/// Quello che il file dice, senza la busta.
///
/// Ogni elenco è **ordinato per chiave** e ogni mappa è una [`BTreeMap`]: non è
/// pignoleria, è ciò che rende `serde_json` deterministico per costruzione e
/// quindi [`impronta`] confrontabile fra due dispositivi. Un passaggio a
/// `HashMap` romperebbe in silenzio la scorciatoia che evita di ricaricare 70 KB
/// ogni quarto d'ora — un test lo sorveglia.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contenuto {
    /// I brani con qualcosa da dire, uno per chiave.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<BranoSalvato>,
    /// Le playlist.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub playlists: Vec<PlaylistSalvata>,
    /// Le cancellazioni.
    #[serde(default)]
    pub tombstones: Lapidi,
    /// Le cartelle sorvegliate, nell'ordine in cui le ha messe chi le ha scelte.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
    /// La skin attiva.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_skin: Option<String>,
    /// Le skin installate: identificativo → impronta del pacchetto.
    ///
    /// I byte stanno in un file a parte su Drive — un `.aeskin` pesa fino a
    /// venti megabyte e non cambia quasi mai, mentre questo file cambia a ogni
    /// cuoricino. Qui c'è l'inventario, che è quel che serve a sapere cosa
    /// scaricare.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skins: BTreeMap<String, String>,
    /// Le bozze dello Studio: identificativo → impronta del `skin.json`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub drafts: BTreeMap<String, String>,
}

/// Il file intero: la busta e il contenuto.
///
/// La data e il nome del dispositivo stanno **qui** e non in [`Contenuto`]: se
/// stessero dentro, due computer con esattamente la stessa libreria
/// produrrebbero impronte diverse e si ricaricherebbero il file a vicenda per
/// sempre.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Salvataggio {
    /// La versione del formato. Vedi [`VERSIONE`].
    pub version: u32,
    /// Quando è stato scritto, in millisecondi.
    pub generated_at: i64,
    /// Da quale dispositivo. Serve a capire se ha scritto qualcun altro.
    pub generated_by: String,
    /// Il contenuto.
    pub content: Contenuto,
}

impl Salvataggio {
    /// Mette un contenuto nella busta.
    #[must_use]
    pub fn nuovo(content: Contenuto, generated_at: i64, generated_by: String) -> Self {
        Self {
            version: VERSIONE,
            generated_at,
            generated_by,
            content,
        }
    }
}

// ── i tipi di questa parte ──────────────────────────────────────────────────

/// Quel che questo dispositivo sa, letto dal database.
///
/// Possiede i suoi dati perché [`aether_domain::restore::RestoreInput`] li
/// prende in prestito: il valore vive nella funzione che pianifica, e le fette
/// puntano qui dentro.
///
/// Non ci sono skin né bozze: quelle stanno sul disco, non nel database, e le
/// elenca chi ha il filesystem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatoLocale {
    /// I brani, **uno per chiave**: le righe che condividono un'identità sono
    /// già fuse. Ci sono tutti, anche quelli senza nessuna statistica, perché è
    /// da questo elenco che si decide quali membri di una playlist esistono qui.
    pub tracks: Vec<(TrackKey, TrackStats)>,
    /// Le playlist, con i membri nell'ordine.
    pub playlists: Vec<PlaylistState>,
    /// Le cartelle sorvegliate.
    pub roots: Vec<String>,
    /// La skin attiva, se ne è stata scelta una.
    pub active_skin: Option<String>,
    /// Le cancellazioni registrate qui.
    pub tombstones: Lapidi,
}

/// Il backup, tradotto nei tipi del dominio e già ripulito dalle lapidi.
///
/// Le fette di [`aether_domain::restore::RestoreInput`] puntano qui dentro,
/// esattamente come per [`StatoLocale`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DalBackup {
    /// I brani del backup.
    pub tracks: Vec<(TrackKey, TrackStats)>,
    /// Le playlist del backup che nessuno ha cancellato nel frattempo.
    pub playlists: Vec<PlaylistState>,
    /// Le cartelle sorvegliate del backup.
    pub roots: Vec<String>,
    /// Le skin del backup, ordinate.
    pub skins: Vec<String>,
    /// Le bozze dello Studio del backup, ordinate.
    pub drafts: Vec<String>,
    /// La skin attiva nel backup.
    pub active_skin: Option<String>,
}

/// Quel che il ripristino ha davvero scritto nel database.
///
/// Non è una copia del piano: skin e bozze le scrive chi ha il filesystem, e i
/// numeri di qui sono solo la parte che è passata dalla transazione.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applicato {
    /// Quante identità di brano hanno ricevuto nuove statistiche.
    pub tracks: usize,
    /// Quante playlist sono state create o riscritte.
    pub playlists: usize,
    /// Quante cartelle sorvegliate sono state aggiunte.
    pub roots: usize,
    /// La skin attiva è stata cambiata.
    pub active_skin: bool,
}

// ── serializzazione ─────────────────────────────────────────────────────────

/// I byte canonici del solo contenuto: è su questi che si calcola l'impronta.
///
/// # Errori
///
/// `internal.unexpected` se la serializzazione fallisce, cosa che per questi
/// tipi non può accadere — ma un'impronta sbagliata farebbe **saltare** un
/// caricamento, cioè perdere silenziosamente un backup, e non è un rischio da
/// nascondere dietro un valore di ripiego.
pub fn canonico(contenuto: &Contenuto) -> Result<Vec<u8>, AppError> {
    serde_json::to_vec(contenuto).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("serializzazione del contenuto del backup".to_owned()),
        })
        .with_cause(err.to_string())
    })
}

/// I byte del file da caricare.
///
/// # Errori
///
/// `internal.unexpected`, come [`canonico`].
pub fn serializza(salvataggio: &Salvataggio) -> Result<Vec<u8>, AppError> {
    serde_json::to_vec(salvataggio).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("serializzazione del backup".to_owned()),
        })
        .with_cause(err.to_string())
    })
}

/// L'impronta di un blocco di byte qualunque.
///
/// La stessa funzione per il file dei metadati e per un pacchetto `.aeskin`,
/// perché la domanda è la stessa: «questi byte sono già lassù?».
///
/// # Perché non l'md5 che Drive calcola da sé
///
/// Perché quello si applicherebbe ai byte **compressi**, e due gzip dello stesso
/// contenuto non sono uguali: cambiano con la versione della libreria e col
/// livello scelto. Un confronto che dipende dall'encoder direbbe «diverso» a
/// ogni aggiornamento, e ricaricherebbe tutto per niente.
#[must_use]
pub fn impronta(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Legge un file di backup, buttando via i singoli record illeggibili.
///
/// # Errori
///
/// - `sync.remoteCorrupt` se il JSON non si interpreta o se non è un oggetto con
///   un `content` dentro: non c'è niente da leggere, e chi chiama può mettere il
///   file da parte e ricrearlo dal locale.
/// - `net.badSchema` se la versione è **più alta** di [`VERSIONE`]. È una
///   distinzione che conta: un file scritto da un Aether più nuovo non è
///   corrotto, e chi chiama non deve sovrascriverlo — sarebbe la sola situazione
///   in cui questa funzione può causare una perdita di dati vera.
pub fn interpreta(raw: &[u8]) -> Result<Salvataggio, AppError> {
    let radice: serde_json::Value = serde_json::from_slice(raw).map_err(|err| {
        AppError::new(ErrorCode::SyncRemoteCorrupt).with_cause(format!("json illeggibile: {err}"))
    })?;

    let version = radice
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::from(VERSIONE));
    if version > u64::from(VERSIONE) {
        return Err(AppError::new(ErrorCode::NetBadSchema {
            service: Some(SERVIZIO.to_owned()),
            detail: Some(format!(
                "il backup è di versione {version}, questa applicazione legge la {VERSIONE}"
            )),
        }));
    }

    let contenuto = radice.get("content").filter(|v| v.is_object());
    let Some(contenuto) = contenuto else {
        return Err(AppError::new(ErrorCode::SyncRemoteCorrupt)
            .with_cause("il file non ha un contenuto leggibile"));
    };

    Ok(Salvataggio {
        version: u32::try_from(version).unwrap_or(VERSIONE),
        generated_at: radice
            .get("generatedAt")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
        generated_by: radice
            .get("generatedBy")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        content: Contenuto {
            tracks: elenco(contenuto.get("tracks")),
            playlists: elenco(contenuto.get("playlists")),
            tombstones: Lapidi {
                tracks: numeri(contenuto.pointer("/tombstones/tracks")),
                playlists: numeri(contenuto.pointer("/tombstones/playlists")),
            },
            roots: testi(contenuto.get("roots")),
            active_skin: contenuto
                .get("activeSkin")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            skins: stringhe(contenuto.get("skins")),
            drafts: stringhe(contenuto.get("drafts")),
        },
    })
}

/// Un elenco di record, saltando quelli che non si interpretano.
///
/// È la regola d'indulgenza in una riga: `filter_map` invece di `collect` su un
/// `Result`. Un campo obbligatorio mancante fa sparire **quel** record, non il
/// backup.
fn elenco<T: serde::de::DeserializeOwned>(valore: Option<&serde_json::Value>) -> Vec<T> {
    valore
        .and_then(serde_json::Value::as_array)
        .map(|righe| {
            righe
                .iter()
                .filter_map(|riga| serde_json::from_value(riga.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Una mappa da chiave a millisecondi, saltando le voci che non lo sono.
fn numeri(valore: Option<&serde_json::Value>) -> BTreeMap<String, i64> {
    valore
        .and_then(serde_json::Value::as_object)
        .map(|voci| {
            voci.iter()
                .filter_map(|(chiave, quando)| Some((chiave.clone(), quando.as_i64()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// Una mappa da chiave a testo, saltando le voci che non lo sono.
fn stringhe(valore: Option<&serde_json::Value>) -> BTreeMap<String, String> {
    valore
        .and_then(serde_json::Value::as_object)
        .map(|voci| {
            voci.iter()
                .filter_map(|(chiave, testo)| Some((chiave.clone(), testo.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Un elenco di testi, saltando quel che non lo è.
fn testi(valore: Option<&serde_json::Value>) -> Vec<String> {
    valore
        .and_then(serde_json::Value::as_array)
        .map(|righe| {
            righe
                .iter()
                .filter_map(|riga| riga.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

// ── lettura del database ────────────────────────────────────────────────────

/// Legge dal database tutto ciò che il backup riguarda.
///
/// # I doppioni si fondono con il **massimo**, non con la somma
///
/// Più righe possono condividere la stessa `track_key` — due formati dello
/// stesso brano, un doppione mai deduplicato — e qui diventano un record solo.
/// La fusione usa [`merge_stats`], che prende il massimo, e **non**
/// `collapse_duplicates`, che somma.
///
/// La ragione è che il ripristino riscrive le statistiche su **tutte** le righe
/// che condividono la chiave. Con la somma, due righe da 3 e 4 diventerebbero 7
/// scritto in entrambe; il salvataggio successivo leggerebbe 7 e 7 e ne farebbe
/// 14, e così a ogni passata, senza che nessuno se ne accorga finché i numeri
/// non diventano assurdi. Il massimo è un punto fisso: 4 resta 4 per sempre.
///
/// # Errori
///
/// `db.queryFailed` se una lettura fallisce.
pub fn stato_locale(connection: &Connection) -> Result<StatoLocale, AppError> {
    Ok(StatoLocale {
        tracks: leggi_brani(connection)?,
        playlists: leggi_playlist(connection)?,
        roots: settings::read_json(connection, CHIAVE_CARTELLE)?.unwrap_or_default(),
        active_skin: settings::read(connection, CHIAVE_SKIN)?,
        tombstones: leggi_lapidi(connection)?,
    })
}

/// I brani, uno per identità.
fn leggi_brani(connection: &Connection) -> Result<Vec<(TrackKey, TrackStats)>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT track_key, play_count, last_played_at, rating, liked, liked_at,
                    stats_updated_at
             FROM tracks",
        )
        .map_err(|err| db_error("brani da salvare", &err))?;
    let righe = statement
        .query_map([], |row| {
            let chiave: String = row.get(0)?;
            let voto: i64 = row.get(3)?;
            let piaciuto: i64 = row.get(4)?;
            Ok((
                chiave,
                TrackStats {
                    play_count: row.get(1)?,
                    last_played_at: row.get(2)?,
                    rating: u8::try_from(voto.clamp(0, 5)).unwrap_or(0),
                    liked: piaciuto != 0,
                    liked_at: row.get(5)?,
                    stats_updated_at: row.get(6)?,
                },
            ))
        })
        .map_err(|err| db_error("brani da salvare", &err))?;

    let mut per_chiave: BTreeMap<String, TrackStats> = BTreeMap::new();
    for riga in righe {
        let (chiave, stats) = riga.map_err(|err| db_error("brani da salvare", &err))?;
        per_chiave
            .entry(chiave)
            .and_modify(|gia| *gia = merge_stats(gia, &stats))
            .or_insert(stats);
    }
    Ok(per_chiave
        .into_iter()
        .map(|(chiave, stats)| (TrackKey::from_stored(chiave), stats))
        .collect())
}

/// Le playlist con i loro membri.
///
/// I membri arrivano da **una** query per tutte le playlist invece che da una
/// per ciascuna: su una libreria con quaranta playlist la differenza fra le due
/// forme è quaranta round trip, e questa funzione gira mentre il lucchetto della
/// libreria è preso.
fn leggi_playlist(connection: &Connection) -> Result<Vec<PlaylistState>, AppError> {
    let mut membri: BTreeMap<i64, Vec<TrackKey>> = BTreeMap::new();
    {
        let mut statement = connection
            .prepare(
                "SELECT pt.playlist_id, t.track_key
                 FROM playlist_tracks pt
                 JOIN tracks t ON t.id = pt.track_id
                 ORDER BY pt.playlist_id, pt.position",
            )
            .map_err(|err| db_error("membri delle playlist", &err))?;
        let righe = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| db_error("membri delle playlist", &err))?;
        for riga in righe {
            let (id, chiave) = riga.map_err(|err| db_error("membri delle playlist", &err))?;
            membri
                .entry(id)
                .or_default()
                .push(TrackKey::from_stored(chiave));
        }
    }

    let mut statement = connection
        .prepare(
            "SELECT id, playlist_key, name, description, created_at, updated_at, is_smart, rules
             FROM playlists
             ORDER BY playlist_key",
        )
        .map_err(|err| db_error("playlist da salvare", &err))?;
    let righe = statement
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let automatica: i64 = row.get(6)?;
            Ok((
                id,
                PlaylistState {
                    key: PlaylistKey::from_stored(row.get::<_, String>(1)?),
                    name: row.get(2)?,
                    description: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    is_smart: automatica != 0,
                    rules: row.get(7)?,
                    members: Vec::new(),
                },
            ))
        })
        .map_err(|err| db_error("playlist da salvare", &err))?;

    let mut playlists = Vec::new();
    for riga in righe {
        let (id, mut playlist) = riga.map_err(|err| db_error("playlist da salvare", &err))?;
        if !playlist.is_smart {
            playlist.members = membri.remove(&id).unwrap_or_default();
        }
        playlists.push(playlist);
    }
    Ok(playlists)
}

/// Le cancellazioni registrate qui.
fn leggi_lapidi(connection: &Connection) -> Result<Lapidi, AppError> {
    let mut statement = connection
        .prepare("SELECT kind, key, deleted_at FROM sync_tombstones")
        .map_err(|err| db_error("lapidi", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|err| db_error("lapidi", &err))?;
    let mut lapidi = Lapidi::default();
    for riga in righe {
        let (tipo, chiave, quando) = riga.map_err(|err| db_error("lapidi", &err))?;
        match tipo.as_str() {
            "track" => {
                lapidi.tracks.insert(chiave, quando);
            }
            "playlist" => {
                lapidi.playlists.insert(chiave, quando);
            }
            // Lo schema ha un `CHECK` che lo impedisce; se un giorno arrivasse
            // un terzo tipo da una versione più nuova, ignorarlo è più utile che
            // rifiutare l'intero backup.
            _ => {}
        }
    }
    Ok(lapidi)
}

// ── dal database al file, e ritorno ─────────────────────────────────────────

/// Costruisce il contenuto da caricare.
///
/// I brani **senza niente da dire** non finiscono nel file: un brano mai
/// ascoltato, senza voto e senza preferito non porta nessuna informazione che un
/// ripristino possa usare, e su una libreria appena scansionata sono la quasi
/// totalità. Toglierli è la differenza fra un file da settanta chilobyte e uno
/// da mezzo megabyte, ricaricato ogni quarto d'ora.
///
/// Attenzione a cosa *non* è vuoto: un preferito **tolto** ha `liked = false` ma
/// una `liked_at`, ed è esattamente il record che non si può perdere — senza,
/// l'altro dispositivo rimetterebbe il cuoricino alla prima passata.
#[must_use]
pub fn snapshot(
    stato: &StatoLocale,
    skins: BTreeMap<String, String>,
    drafts: BTreeMap<String, String>,
) -> Contenuto {
    Contenuto {
        tracks: stato
            .tracks
            .iter()
            .filter(|(_, stats)| !senza_niente_da_dire(stats))
            .map(|(chiave, stats)| BranoSalvato::nuovo(chiave.as_str(), stats))
            .collect(),
        playlists: stato.playlists.iter().map(PlaylistSalvata::nuova).collect(),
        tombstones: stato.tombstones.clone(),
        roots: stato.roots.clone(),
        active_skin: stato.active_skin.clone(),
        skins,
        drafts,
    }
}

/// Statistiche che non dicono niente a nessuno.
///
/// `stats_updated_at` è escluso apposta: da solo non è un'informazione, è
/// l'orologio di una decisione che non c'è stata.
fn senza_niente_da_dire(stats: &TrackStats) -> bool {
    *stats
        == TrackStats {
            stats_updated_at: stats.stats_updated_at,
            ..TrackStats::default()
        }
}

impl BranoSalvato {
    /// Il record di un brano.
    fn nuovo(key: &str, stats: &TrackStats) -> Self {
        Self {
            key: key.to_owned(),
            play_count: stats.play_count,
            last_played_at: stats.last_played_at,
            rating: stats.rating,
            liked: stats.liked,
            liked_at: stats.liked_at,
            stats_updated_at: stats.stats_updated_at,
        }
    }

    /// Le statistiche, **riportate entro i limiti dello schema**.
    ///
    /// Il voto si taglia a `0..=5` e il conteggio non scende sotto zero. Non è
    /// diffidenza verso i nostri stessi file: è che `tracks.rating` ha un
    /// `CHECK`, e un valore fuori scala — da una versione futura, da un file
    /// manomesso, da un byte girato — farebbe fallire l'`UPDATE` e con lui
    /// **l'intera transazione di ripristino**. Un voto assurdo costa un voto;
    /// un ripristino che non parte costa tutto il resto.
    fn statistiche(&self) -> TrackStats {
        TrackStats {
            play_count: self.play_count.max(0),
            last_played_at: self.last_played_at,
            rating: self.rating.min(5),
            liked: self.liked,
            liked_at: self.liked_at,
            stats_updated_at: self.stats_updated_at,
        }
    }
}

impl PlaylistSalvata {
    /// Il record di una playlist.
    fn nuova(playlist: &PlaylistState) -> Self {
        Self {
            key: playlist.key.as_str().to_owned(),
            name: playlist.name.clone(),
            description: playlist.description.clone(),
            created_at: playlist.created_at,
            updated_at: playlist.updated_at,
            is_smart: playlist.is_smart,
            rules: playlist.rules.clone(),
            members: if playlist.is_smart {
                Vec::new()
            } else {
                playlist
                    .members
                    .iter()
                    .map(|chiave| chiave.as_str().to_owned())
                    .collect()
            },
        }
    }

    /// La playlist nei tipi del dominio.
    fn stato(&self) -> PlaylistState {
        PlaylistState {
            key: PlaylistKey::from_stored(self.key.clone()),
            name: self.name.clone(),
            description: self.description.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            is_smart: self.is_smart,
            rules: self.rules.clone(),
            members: if self.is_smart {
                Vec::new()
            } else {
                self.members
                    .iter()
                    .map(|chiave| TrackKey::from_stored(chiave.clone()))
                    .collect()
            },
        }
    }
}

/// Traduce il backup nei tipi del dominio, togliendo ciò che è stato cancellato.
///
/// # Le lapidi valgono per le playlist e non per i brani
///
/// Una playlist cancellata qui non deve tornare dal backup: è una cosa che
/// esiste solo nel database, e ricrearla annullerebbe una decisione dell'utente.
///
/// Per i brani è diverso, e non per distrazione. Il ripristino non crea mai una
/// riga di brano — la crea la scansione, quando il file c'è. Se il file c'è, la
/// riga è legittima, e le sue statistiche vanno fuse: una lapide vecchia
/// significherebbe soltanto che quel file era stato tolto e poi rimesso, e
/// buttare via la sua storia d'ascolto sarebbe una perdita senza nessun
/// guadagno.
#[must_use]
pub fn dal_salvataggio(contenuto: &Contenuto, lapidi_locali: &Lapidi) -> DalBackup {
    let sepolta = |chiave: &str, updated_at: i64| {
        let quando = |lapidi: &BTreeMap<String, i64>| lapidi.get(chiave).copied();
        let piu_recente = quando(&contenuto.tombstones.playlists)
            .into_iter()
            .chain(quando(&lapidi_locali.playlists))
            .max();
        piu_recente.is_some_and(|quando| quando >= updated_at)
    };

    DalBackup {
        tracks: contenuto
            .tracks
            .iter()
            .map(|brano| {
                (
                    TrackKey::from_stored(brano.key.clone()),
                    brano.statistiche(),
                )
            })
            .collect(),
        playlists: contenuto
            .playlists
            .iter()
            .filter(|playlist| !sepolta(&playlist.key, playlist.updated_at))
            .map(PlaylistSalvata::stato)
            .collect(),
        roots: contenuto.roots.clone(),
        skins: contenuto.skins.keys().cloned().collect(),
        drafts: contenuto.drafts.keys().cloned().collect(),
        active_skin: contenuto.active_skin.clone(),
    }
}

/// Fonde due contenuti in memoria, senza toccare nessun database.
///
/// Serve prima di sovrascrivere un file scritto da **un altro dispositivo**: si
/// scarica il suo, lo si unisce al nostro e si carica l'unione. Il file remoto
/// non regredisce mai; il database locale non si tocca — per vedere qui gli
/// ascolti dell'altro computer bisogna comunque premere «Ripristina», ed è
/// deliberato.
///
/// A parità di informazione vince `locale`: è lo stesso verso di
/// [`aether_domain::restore::plan_restore`], ed è ciò che rende l'operazione
/// stabile quando la si ripete.
#[must_use]
pub fn fondi(locale: &Contenuto, remoto: &Contenuto) -> Contenuto {
    let mut brani: BTreeMap<&str, TrackStats> = BTreeMap::new();
    for brano in locale.tracks.iter().chain(&remoto.tracks) {
        let stats = brano.statistiche();
        brani
            .entry(&brano.key)
            .and_modify(|gia| *gia = merge_stats(gia, &stats))
            .or_insert(stats);
    }

    let mut tombstones = Lapidi::default();
    for (chiave, quando) in locale
        .tombstones
        .tracks
        .iter()
        .chain(&remoto.tombstones.tracks)
    {
        let voce = tombstones.tracks.entry(chiave.clone()).or_insert(*quando);
        *voce = (*voce).max(*quando);
    }
    for (chiave, quando) in locale
        .tombstones
        .playlists
        .iter()
        .chain(&remoto.tombstones.playlists)
    {
        let voce = tombstones
            .playlists
            .entry(chiave.clone())
            .or_insert(*quando);
        *voce = (*voce).max(*quando);
    }

    let mut playlists: BTreeMap<&str, &PlaylistSalvata> = BTreeMap::new();
    for playlist in locale.playlists.iter().chain(&remoto.playlists) {
        playlists
            .entry(&playlist.key)
            .and_modify(|gia| {
                if playlist.updated_at > gia.updated_at {
                    *gia = playlist;
                }
            })
            .or_insert(playlist);
    }

    // Le cartelle si confrontano per stringa esatta e non per `path_key`: qui i
    // due elenchi possono venire da due sistemi operativi diversi, e applicare
    // le regole di questo a un percorso dell'altro darebbe unioni diverse a
    // seconda di chi carica. La deduplica vera la fa `plan_restore`, sul
    // dispositivo che dovrà davvero usare quei percorsi.
    let mut viste: BTreeSet<&str> = BTreeSet::new();
    let roots = locale
        .roots
        .iter()
        .chain(&remoto.roots)
        .filter(|radice| viste.insert(radice.as_str()))
        .cloned()
        .collect();

    let mut skins = remoto.skins.clone();
    skins.extend(
        locale
            .skins
            .iter()
            .map(|(id, impronta)| (id.clone(), impronta.clone())),
    );
    let mut drafts = remoto.drafts.clone();
    drafts.extend(
        locale
            .drafts
            .iter()
            .map(|(id, impronta)| (id.clone(), impronta.clone())),
    );

    Contenuto {
        tracks: brani
            .into_iter()
            .map(|(chiave, stats)| BranoSalvato::nuovo(chiave, &stats))
            .collect(),
        playlists: playlists
            .into_values()
            .filter(|playlist| {
                tombstones
                    .playlists
                    .get(&playlist.key)
                    .is_none_or(|quando| *quando < playlist.updated_at)
            })
            .cloned()
            .collect(),
        tombstones,
        roots,
        active_skin: locale
            .active_skin
            .clone()
            .or_else(|| remoto.active_skin.clone()),
        skins,
        drafts,
    }
}

// ── applicazione ────────────────────────────────────────────────────────────

/// Esegue la parte di ripristino che sta nel database, in **una** transazione.
///
/// Skin e bozze non passano di qui: sono file, li scrive chi ha il filesystem, e
/// li scrive **prima**. L'ordine conta — se si committasse per prima la
/// transazione e poi un download fallisse, resterebbe scritto uno `skin.active`
/// che punta a una skin che non esiste, e l'applicazione si riaprirebbe illegibile.
///
/// # Errori
///
/// `db.queryFailed` se una scrittura fallisce. In quel caso non viene applicato
/// **niente**: è il motivo per cui è una transazione sola.
pub fn applica(connection: &mut Connection, piano: &RestorePlan) -> Result<Applicato, AppError> {
    let tx = connection
        .transaction()
        .map_err(|err| db_error("ripristino", &err))?;

    let applicato = Applicato {
        tracks: applica_brani(&tx, &piano.tracks)?,
        playlists: applica_playlist(&tx, &piano.playlists)?,
        roots: applica_cartelle(&tx, &piano.roots_to_add)?,
        active_skin: match &piano.active_skin {
            Some(id) => {
                settings::write(&tx, CHIAVE_SKIN, id)?;
                true
            }
            None => false,
        },
    };

    tx.commit().map_err(|err| db_error("ripristino", &err))?;
    Ok(applicato)
}

/// Riscrive le statistiche dei brani.
///
/// `WHERE track_key = ?` e non `WHERE id = ?`: **tutte** le righe che sono
/// quell'identità ricevono le stesse statistiche. Sembra un errore e non lo è —
/// `tracks.track_key` non è `UNIQUE` apposta, e due file dello stesso brano sono
/// due copie di una storia d'ascolto sola. È idempotente perché il valore scritto
/// è già un massimo: rileggere e rifondere dà lo stesso numero.
///
/// **Non tocca né i campi descrittivi né le chiavi**, e per questo — a differenza
/// della scansione e dell'arricchimento — non chiama
/// [`crate::incerti::riapplica`]: un ripristino rimette ascolti, voti e
/// preferiti, cioè quel che il backup sa, e il backup non porta né i titoli né
/// `track_overrides`. Non c'è niente da rimettere sopra perché niente è stato
/// scavalcato.
fn applica_brani(tx: &Transaction<'_>, cambi: &[TrackChange]) -> Result<usize, AppError> {
    if cambi.is_empty() {
        return Ok(0);
    }
    let mut aggiorna = tx
        .prepare(
            "UPDATE tracks
             SET play_count = ?2, last_played_at = ?3, rating = ?4, liked = ?5,
                 liked_at = ?6, stats_updated_at = ?7
             WHERE track_key = ?1",
        )
        .map_err(|err| db_error("statistiche ripristinate", &err))?;
    for cambio in cambi {
        aggiorna
            .execute(rusqlite::params![
                cambio.key.as_str(),
                cambio.after.play_count,
                cambio.after.last_played_at,
                i64::from(cambio.after.rating),
                i64::from(cambio.after.liked),
                cambio.after.liked_at,
                cambio.after.stats_updated_at,
            ])
            .map_err(|err| db_error("statistiche ripristinate", &err))?;
    }
    Ok(cambi.len())
}

/// Crea o riscrive le playlist del piano.
fn applica_playlist(tx: &Transaction<'_>, cambi: &[PlaylistChange]) -> Result<usize, AppError> {
    if cambi.is_empty() {
        return Ok(0);
    }
    let mut inserisci = tx
        .prepare(
            "INSERT INTO playlists
                 (playlist_key, name, description, created_at, updated_at, is_smart, rules)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(playlist_key) DO UPDATE SET
                 name        = excluded.name,
                 description = excluded.description,
                 -- La data di creazione più antica delle due: è quella vera, e
                 -- un ripristino non deve far sembrare appena nata una playlist
                 -- che esiste da tre anni.
                 created_at  = MIN(playlists.created_at, excluded.created_at),
                 updated_at  = excluded.updated_at,
                 is_smart    = excluded.is_smart,
                 rules       = excluded.rules",
        )
        .map_err(|err| db_error("playlist ripristinata", &err))?;
    let mut identifica = tx
        .prepare("SELECT id FROM playlists WHERE playlist_key = ?1")
        .map_err(|err| db_error("playlist ripristinata", &err))?;
    let mut cerca_brano = tx
        .prepare("SELECT id FROM tracks WHERE track_key = ?1 ORDER BY id LIMIT 1")
        .map_err(|err| db_error("playlist ripristinata", &err))?;
    let mut svuota = tx
        .prepare("DELETE FROM playlist_tracks WHERE playlist_id = ?1")
        .map_err(|err| db_error("playlist ripristinata", &err))?;
    let mut accoda = tx
        .prepare(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        )
        .map_err(|err| db_error("playlist ripristinata", &err))?;
    let mut dissotterra = tx
        .prepare("DELETE FROM sync_tombstones WHERE kind = 'playlist' AND key = ?1")
        .map_err(|err| db_error("playlist ripristinata", &err))?;

    for cambio in cambi {
        let playlist = &cambio.playlist;
        let chiave = playlist.key.as_str();
        inserisci
            .execute(rusqlite::params![
                chiave,
                playlist.name,
                playlist.description,
                playlist.created_at,
                playlist.updated_at,
                i64::from(playlist.is_smart),
                playlist.rules,
            ])
            .map_err(|err| db_error("playlist ripristinata", &err))?;

        // La lapide se ne va con la ricreazione: lasciarla significherebbe che
        // il salvataggio successivo spedisce insieme la playlist e la notizia
        // della sua cancellazione, e l'altro dispositivo non saprebbe a chi
        // credere.
        dissotterra
            .execute([chiave])
            .map_err(|err| db_error("playlist ripristinata", &err))?;

        // Le playlist automatiche ricevono la riga e le regole, mai
        // l'appartenenza: ogni dispositivo la ricalcola.
        if playlist.is_smart {
            continue;
        }

        let id: i64 = identifica
            .query_row([chiave], |row| row.get(0))
            .map_err(|err| db_error("playlist ripristinata", &err))?;

        let mut brani = Vec::with_capacity(playlist.members.len());
        for membro in &playlist.members {
            let trovato: Option<i64> = cerca_brano
                .query_row([membro.as_str()], |row| row.get(0))
                .or_else(|err| match err {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    altro => Err(altro),
                })
                .map_err(|err| db_error("playlist ripristinata", &err))?;
            if let Some(track_id) = trovato {
                brani.push(track_id);
            }
        }

        // Si svuota e si riscrive, invece di aggiustare: `playlist_tracks` ha
        // `PRIMARY KEY (playlist_id, position)` e il vincolo si controlla a ogni
        // istruzione, non alla COMMIT. Le posizioni escono `0..n` senza buchi,
        // che è la forma che la finestra si aspetta di poter usare come indici.
        svuota
            .execute([id])
            .map_err(|err| db_error("playlist ripristinata", &err))?;
        for (posizione, track_id) in brani.iter().enumerate() {
            let posizione = i64::try_from(posizione).unwrap_or(i64::MAX);
            accoda
                .execute(rusqlite::params![id, track_id, posizione])
                .map_err(|err| db_error("playlist ripristinata", &err))?;
        }
    }
    Ok(cambi.len())
}

/// Aggiunge le cartelle sorvegliate del backup a quelle di qui.
///
/// In coda e senza togliere niente: quelle già scelte restano prime, perché
/// sono la scelta che l'utente ha fatto su **questo** computer.
fn applica_cartelle(tx: &Transaction<'_>, da_aggiungere: &[RootToAdd]) -> Result<usize, AppError> {
    if da_aggiungere.is_empty() {
        return Ok(0);
    }
    let mut cartelle: Vec<String> = settings::read_json(tx, CHIAVE_CARTELLE)?.unwrap_or_default();
    for radice in da_aggiungere {
        cartelle.push(radice.path.clone());
    }
    settings::write_json(tx, CHIAVE_CARTELLE, &cartelle)?;
    Ok(da_aggiungere.len())
}

// ── minuzie di serializzazione ──────────────────────────────────────────────

/// Un intero che non vale la pena scrivere nel file.
fn e_zero_i64(valore: &i64) -> bool {
    *valore == 0
}

/// Un voto assente.
fn e_zero_u8(valore: &u8) -> bool {
    *valore == 0
}

/// Un vero che non c'è.
fn e_falso(valore: &bool) -> bool {
    !*valore
}

#[cfg(test)]
mod prove {
    use aether_domain::paths::PathRules;
    use aether_domain::restore::{RestoreInput, plan_restore};

    use super::*;

    /// Le regole di percorso delle prove: sempre le stesse su ogni macchina,
    /// altrimenti un test passerebbe qui e fallirebbe su Linux.
    const REGOLE: PathRules = PathRules {
        case_insensitive: true,
    };

    /// Un database con lo schema vero e qualche brano.
    ///
    /// Lo schema vero e non una tabella scritta a mano: qui si provano dei
    /// `CHECK`, delle chiavi esterne e un `ON CONFLICT`, cioè proprio le cose
    /// che una tabella semplificata non avrebbe.
    fn libreria() -> Connection {
        let connection = crate::db::open_in_memory().expect("database").connection;
        connection
            .execute_batch(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (1, 'a.mp3', 'art|uno|al',    'Uno',     'Art', 'Al', 1000, 1, 1, 1),
                        (2, 'b.mp3', 'art|due|al',    'Due',     'Art', 'Al', 2000, 1, 1, 1),
                        (3, 'c.mp3', 'art|tre|al',    'Tre',     'Art', 'Al', 3000, 1, 1, 1),
                        (4, 'd.mp3', 'art|quattro|al','Quattro', 'Art', 'Al', 4000, 1, 1, 1);",
            )
            .expect("brani");
        connection
    }

    fn statistica(connection: &Connection, id: i64, sql: &str) {
        connection
            .execute(&format!("UPDATE tracks SET {sql} WHERE id = {id}"), [])
            .expect("statistiche");
    }

    /// Il piano che un ripristino proporrebbe, fra due stati già letti.
    fn pianifica(qui: &StatoLocale, dal_backup: &DalBackup) -> RestorePlan {
        let esistono = vec![true; dal_backup.roots.len()];
        plan_restore(&RestoreInput {
            local_tracks: &qui.tracks,
            backup_tracks: &dal_backup.tracks,
            local_playlists: &qui.playlists,
            backup_playlists: &dal_backup.playlists,
            local_roots: &qui.roots,
            backup_roots: &dal_backup.roots,
            backup_roots_exist: &esistono,
            local_skins: &[],
            backup_skins: &dal_backup.skins,
            local_drafts: &[],
            backup_drafts: &dal_backup.drafts,
            local_active_skin: qui.active_skin.as_deref(),
            backup_active_skin: dal_backup.active_skin.as_deref(),
            path_rules: REGOLE,
        })
    }

    /// Legge, salva, rilegge: il giro completo che fa un backup.
    fn giro(connection: &Connection) -> (StatoLocale, DalBackup) {
        let qui = stato_locale(connection).expect("stato locale");
        let contenuto = snapshot(&qui, BTreeMap::new(), BTreeMap::new());
        let bytes = serializza(&Salvataggio::nuovo(
            contenuto,
            1_700_000_000_000,
            "prova".into(),
        ))
        .expect("serializzato");
        let riletto = interpreta(&bytes).expect("riletto");
        let dal_backup = dal_salvataggio(&riletto.content, &qui.tombstones);
        (qui, dal_backup)
    }

    #[test]
    fn salvare_e_rileggere_non_propone_nessuna_modifica() {
        // Il punto fisso: è la proprietà su cui si regge tutto il resto. Se il
        // giro completo su una libreria intoccata proponesse anche una sola
        // modifica, ogni ripristino ne proporrebbe altre all'infinito e nessuno
        // saprebbe più dire se è servito.
        let connection = libreria();
        statistica(&connection, 1, "play_count = 12, last_played_at = 900");
        statistica(&connection, 2, "rating = 4, stats_updated_at = 800");
        statistica(
            &connection,
            3,
            "liked = 1, liked_at = 700, stats_updated_at = 700",
        );
        settings::write_json(&connection, CHIAVE_CARTELLE, &vec!["C:\\Musica"]).expect("cartelle");
        settings::write(&connection, CHIAVE_SKIN, "plain").expect("skin");
        connection
            .execute_batch(
                "INSERT INTO playlists (id, playlist_key, name, created_at, updated_at, is_smart)
                 VALUES (1, 'serata', 'Serata', 10, 20, 0);
                 INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 VALUES (1, 3, 0), (1, 1, 1);",
            )
            .expect("playlist");

        let (qui, dal_backup) = giro(&connection);
        let piano = pianifica(&qui, &dal_backup);
        assert!(
            piano.is_empty(),
            "un backup appena scritto non ha niente da ripristinare: {piano:?}"
        );
        assert_eq!(piano.tracks_unchanged, 3);
        assert_eq!(piano.playlists_unchanged, 1);
    }

    #[test]
    fn l_impronta_non_dipende_dall_ordine_di_inserimento() {
        // È l'affermazione sulle `BTreeMap`. Un rifacimento che passasse a
        // `HashMap` romperebbe in silenzio la scorciatoia «non è cambiato
        // niente», e il sintomo sarebbe un caricamento inutile ogni quarto
        // d'ora — cioè niente di visibile, finché non arriva la bolletta.
        let uno = crate::db::open_in_memory().expect("database").connection;
        let due = crate::db::open_in_memory().expect("database").connection;
        let righe = [
            "(1, 'a.mp3', 'k|a|x', 'A', 'Art', 'Al', 1, 1, 1, 1, 5)",
            "(2, 'b.mp3', 'k|b|x', 'B', 'Art', 'Al', 1, 1, 1, 1, 7)",
            "(3, 'c.mp3', 'k|c|x', 'C', 'Art', 'Al', 1, 1, 1, 1, 9)",
        ];
        let colonne = "INSERT INTO tracks (id, path, track_key, title, artist, album,
                       duration_ms, file_size, date_added, date_modified, play_count) VALUES ";
        uno.execute_batch(&format!("{colonne}{};", righe.join(", ")))
            .expect("in ordine");
        due.execute_batch(&format!(
            "{colonne}{};",
            righe.iter().rev().cloned().collect::<Vec<_>>().join(", ")
        ))
        .expect("al contrario");

        let contenuto = |c: &Connection| {
            snapshot(
                &stato_locale(c).expect("stato"),
                BTreeMap::new(),
                BTreeMap::new(),
            )
        };
        let a = canonico(&contenuto(&uno)).expect("byte");
        let b = canonico(&contenuto(&due)).expect("byte");
        assert_eq!(impronta(&a), impronta(&b));
    }

    #[test]
    fn i_brani_senza_niente_da_dire_non_finiscono_nel_file() {
        // Su una libreria appena scansionata sono la quasi totalità: tenerli
        // moltiplicherebbe per sette il file che si ricarica di continuo.
        let connection = libreria();
        statistica(&connection, 1, "play_count = 3");
        let contenuto = snapshot(
            &stato_locale(&connection).expect("stato"),
            BTreeMap::new(),
            BTreeMap::new(),
        );
        assert_eq!(contenuto.tracks.len(), 1);

        // …ma un preferito TOLTO ha qualcosa da dire, e va salvato: senza,
        // l'altro dispositivo rimetterebbe il cuoricino alla prima passata.
        statistica(&connection, 2, "liked = 0, liked_at = 500");
        let contenuto = snapshot(
            &stato_locale(&connection).expect("stato"),
            BTreeMap::new(),
            BTreeMap::new(),
        );
        assert_eq!(contenuto.tracks.len(), 2);
    }

    #[test]
    fn un_record_storto_non_butta_via_il_file() {
        // L'indulgenza. Un backup è quello che si legge nel giorno peggiore.
        let raw = br#"{
            "version": 1,
            "generatedAt": 5,
            "generatedBy": "altro",
            "content": {
                "tracks": [
                    {"key": "art|uno|al", "playCount": 9},
                    {"playCount": 4},
                    "e questo cos'e",
                    {"key": "art|due|al", "rating": "quattro"}
                ]
            }
        }"#;
        let letto = interpreta(raw).expect("il file si legge lo stesso");
        assert_eq!(letto.content.tracks.len(), 1, "solo il record buono");
        assert_eq!(letto.content.tracks.first().map(|b| b.play_count), Some(9));
    }

    #[test]
    fn i_campi_sconosciuti_non_fermano_la_lettura() {
        // Un file scritto da una versione più nuova ma della stessa generazione:
        // i campi che non conosciamo si ignorano, il resto si legge.
        let raw = br#"{
            "version": 1,
            "content": {
                "tracks": [{"key": "a|b|c", "playCount": 2, "colorePreferito": "blu"}],
                "invenzioneFutura": {"chissa": true}
            }
        }"#;
        let letto = interpreta(raw).expect("letto");
        assert_eq!(letto.content.tracks.len(), 1);
    }

    #[test]
    fn un_file_illeggibile_e_corrotto() {
        assert_eq!(
            interpreta(b"non json")
                .map(|_| ())
                .unwrap_err()
                .code()
                .kind()
                .code(),
            "sync.remoteCorrupt"
        );
        assert_eq!(
            interpreta(b"[1, 2, 3]")
                .map(|_| ())
                .unwrap_err()
                .code()
                .kind()
                .code(),
            "sync.remoteCorrupt"
        );
        assert_eq!(
            interpreta(br#"{"version": 1}"#)
                .map(|_| ())
                .unwrap_err()
                .code()
                .kind()
                .code(),
            "sync.remoteCorrupt",
            "senza contenuto non c'è niente da leggere"
        );
    }

    #[test]
    fn una_versione_futura_non_e_una_corruzione() {
        // La distinzione che evita l'unica perdita di dati vera di cui questo
        // modulo è capace: `sync.remoteCorrupt` autorizza chi chiama a mettere
        // il file da parte e riscriverlo dal locale. Su un backup scritto da un
        // Aether più nuovo sarebbe una cancellazione.
        let err = interpreta(br#"{"version": 99, "content": {}}"#)
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.code().kind().code(), "net.badSchema");
    }

    #[test]
    fn ripristinare_rimette_le_statistiche_e_poi_non_propone_piu_niente() {
        // Il caso vero: il computer è stato reinstallato, i file sono tornati
        // dalla scansione, le statistiche no.
        let ricco = libreria();
        statistica(&ricco, 1, "play_count = 40, last_played_at = 900");
        statistica(&ricco, 2, "rating = 5, stats_updated_at = 800");
        statistica(
            &ricco,
            3,
            "liked = 1, liked_at = 700, stats_updated_at = 700",
        );
        let contenuto = snapshot(
            &stato_locale(&ricco).expect("stato"),
            BTreeMap::new(),
            BTreeMap::new(),
        );

        let mut vuoto = libreria();
        let qui = stato_locale(&vuoto).expect("stato");
        let dal_backup = dal_salvataggio(&contenuto, &qui.tombstones);
        let piano = pianifica(&qui, &dal_backup);
        assert_eq!(piano.tracks.len(), 3);

        let applicato = applica(&mut vuoto, &piano).expect("applicato");
        assert_eq!(applicato.tracks, 3);

        let (voti, ascolti, cuori): (i64, i64, i64) = vuoto
            .query_row(
                "SELECT SUM(rating), SUM(play_count), SUM(liked) FROM tracks",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("statistiche");
        assert_eq!((voti, ascolti, cuori), (5, 40, 1));

        // E rifarlo non propone niente: chi ripristina lo rifà.
        let qui = stato_locale(&vuoto).expect("stato");
        let secondo = pianifica(&qui, &dal_salvataggio(&contenuto, &qui.tombstones));
        assert!(secondo.is_empty(), "il secondo piano deve essere vuoto");
    }

    #[test]
    fn due_righe_dello_stesso_brano_ricevono_le_stesse_statistiche() {
        // Due file dello stesso brano, formati diversi. Il ripristino scrive su
        // entrambe le righe, ed è corretto perché il valore è un massimo: la
        // passata successiva legge lo stesso numero e non lo raddoppia.
        let mut connection = libreria();
        connection
            .execute_batch(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, duration_ms,
                                     file_size, date_added, date_modified)
                 VALUES (5, 'a.flac', 'art|uno|al', 'Uno', 'Art', 'Al', 1000, 1, 1, 1);",
            )
            .expect("doppione");

        let contenuto = Contenuto {
            tracks: vec![BranoSalvato {
                key: "art|uno|al".to_owned(),
                play_count: 11,
                ..BranoSalvato::default()
            }],
            ..Contenuto::default()
        };
        let qui = stato_locale(&connection).expect("stato");
        let piano = pianifica(&qui, &dal_salvataggio(&contenuto, &qui.tombstones));
        applica(&mut connection, &piano).expect("applicato");

        let conteggi: Vec<i64> = {
            let mut statement = connection
                .prepare("SELECT play_count FROM tracks WHERE track_key = 'art|uno|al'")
                .expect("query");
            statement
                .query_map([], |row| row.get(0))
                .expect("righe")
                .collect::<Result<Vec<i64>, _>>()
                .expect("conteggi")
        };
        assert_eq!(conteggi, [11, 11]);

        // Il punto fisso: rileggere e risalvare non gonfia niente.
        let contenuto = snapshot(
            &stato_locale(&connection).expect("stato"),
            BTreeMap::new(),
            BTreeMap::new(),
        );
        assert_eq!(
            contenuto.tracks.first().map(|b| b.play_count),
            Some(11),
            "sommare invece di prendere il massimo darebbe 22, poi 44, poi 88"
        );
    }

    #[test]
    fn una_playlist_si_ricrea_con_le_posizioni_dense() {
        // Metà dei brani non ci sono più: le posizioni devono uscire 0..n senza
        // buchi, altrimenti la finestra manderebbe indietro indici che puntano
        // al brano sbagliato.
        let mut connection = libreria();
        let contenuto = Contenuto {
            playlists: vec![PlaylistSalvata {
                key: "serata".to_owned(),
                name: "Serata".to_owned(),
                created_at: 10,
                updated_at: 20,
                members: vec![
                    "art|tre|al".to_owned(),
                    "art|mai|vista".to_owned(),
                    "art|uno|al".to_owned(),
                ],
                ..PlaylistSalvata::default()
            }],
            ..Contenuto::default()
        };
        let qui = stato_locale(&connection).expect("stato");
        let piano = pianifica(&qui, &dal_salvataggio(&contenuto, &qui.tombstones));
        let cambio = piano.playlists.first().expect("una playlist");
        assert!(cambio.is_new);
        assert_eq!(cambio.members_in_backup, 3, "per poter dire «2 su 3»");

        applica(&mut connection, &piano).expect("applicato");
        let mut statement = connection
            .prepare(
                "SELECT pt.position, t.title
                 FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
                 ORDER BY pt.position",
            )
            .expect("query");
        let righe: Vec<(i64, String)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("righe")
            .collect::<Result<_, _>>()
            .expect("posizioni");
        assert_eq!(
            righe,
            [(0, "Tre".to_owned()), (1, "Uno".to_owned())],
            "nessun buco dove stava il brano che manca"
        );
    }

    #[test]
    fn una_playlist_automatica_non_riceve_righe() {
        let mut connection = libreria();
        let contenuto = Contenuto {
            playlists: vec![PlaylistSalvata {
                key: "preferiti".to_owned(),
                name: "Preferiti".to_owned(),
                updated_at: 20,
                is_smart: true,
                rules: Some(r#"{"liked":true}"#.to_owned()),
                members: vec!["art|uno|al".to_owned()],
                ..PlaylistSalvata::default()
            }],
            ..Contenuto::default()
        };
        let qui = stato_locale(&connection).expect("stato");
        let piano = pianifica(&qui, &dal_salvataggio(&contenuto, &qui.tombstones));
        applica(&mut connection, &piano).expect("applicato");

        let righe: i64 = connection
            .query_row("SELECT COUNT(*) FROM playlist_tracks", [], |row| row.get(0))
            .expect("righe");
        assert_eq!(righe, 0, "l'appartenenza la ricalcola ogni dispositivo");
        let regole: Option<String> = connection
            .query_row(
                "SELECT rules FROM playlists WHERE playlist_key = 'preferiti'",
                [],
                |row| row.get(0),
            )
            .expect("regole");
        assert_eq!(regole.as_deref(), Some(r#"{"liked":true}"#));
    }

    #[test]
    fn una_playlist_cancellata_qui_non_torna_dal_backup() {
        // La lapide serve a questo: senza, la cancellazione verrebbe annullata
        // dal primo ripristino, e l'utente si ritroverebbe la playlist che
        // aveva buttato.
        let connection = libreria();
        connection
            .execute(
                "INSERT INTO sync_tombstones (kind, key, deleted_at)
                 VALUES ('playlist', 'serata', 50)",
                [],
            )
            .expect("lapide");
        let contenuto = Contenuto {
            playlists: vec![PlaylistSalvata {
                key: "serata".to_owned(),
                name: "Serata".to_owned(),
                updated_at: 20,
                ..PlaylistSalvata::default()
            }],
            ..Contenuto::default()
        };
        let qui = stato_locale(&connection).expect("stato");
        let dal_backup = dal_salvataggio(&contenuto, &qui.tombstones);
        assert!(dal_backup.playlists.is_empty());

        // …ma una playlist modificata DOPO la cancellazione sull'altro
        // dispositivo torna: la decisione più recente vince.
        let contenuto = Contenuto {
            playlists: vec![PlaylistSalvata {
                key: "serata".to_owned(),
                name: "Serata".to_owned(),
                updated_at: 90,
                ..PlaylistSalvata::default()
            }],
            ..Contenuto::default()
        };
        let dal_backup = dal_salvataggio(&contenuto, &qui.tombstones);
        assert_eq!(dal_backup.playlists.len(), 1);
    }

    #[test]
    fn ricreare_una_playlist_toglie_la_sua_lapide() {
        // Altrimenti il salvataggio successivo spedirebbe insieme la playlist e
        // la notizia della sua cancellazione, e l'altro dispositivo non saprebbe
        // a chi credere.
        let mut connection = libreria();
        connection
            .execute(
                "INSERT INTO sync_tombstones (kind, key, deleted_at)
                 VALUES ('playlist', 'serata', 50)",
                [],
            )
            .expect("lapide");
        let contenuto = Contenuto {
            playlists: vec![PlaylistSalvata {
                key: "serata".to_owned(),
                name: "Serata".to_owned(),
                updated_at: 90,
                ..PlaylistSalvata::default()
            }],
            ..Contenuto::default()
        };
        let qui = stato_locale(&connection).expect("stato");
        let piano = pianifica(&qui, &dal_salvataggio(&contenuto, &qui.tombstones));
        applica(&mut connection, &piano).expect("applicato");

        let lapidi: i64 = connection
            .query_row("SELECT COUNT(*) FROM sync_tombstones", [], |row| row.get(0))
            .expect("lapidi");
        assert_eq!(lapidi, 0);
    }

    #[test]
    fn le_cartelle_si_aggiungono_in_coda_senza_togliere() {
        let mut connection = libreria();
        settings::write_json(&connection, CHIAVE_CARTELLE, &vec!["C:\\Musica"]).expect("cartelle");
        let contenuto = Contenuto {
            roots: vec!["D:\\Archivio".to_owned(), "c:/musica/".to_owned()],
            ..Contenuto::default()
        };
        let qui = stato_locale(&connection).expect("stato");
        let piano = pianifica(&qui, &dal_salvataggio(&contenuto, &qui.tombstones));
        applica(&mut connection, &piano).expect("applicato");

        let cartelle: Vec<String> = settings::read_json(&connection, CHIAVE_CARTELLE)
            .expect("lette")
            .unwrap_or_default();
        assert_eq!(
            cartelle,
            ["C:\\Musica".to_owned(), "D:\\Archivio".to_owned()],
            "la scelta di qui resta prima, e la cartella già presente non si ripete"
        );
    }

    #[test]
    fn un_voto_fuori_scala_non_fa_saltare_tutto_il_ripristino() {
        // Un `CHECK` violato annullerebbe la transazione, cioè l'intero
        // ripristino. Un voto assurdo costa un voto; un ripristino che non parte
        // costa tutto il resto.
        let mut connection = libreria();
        let contenuto = Contenuto {
            tracks: vec![BranoSalvato {
                key: "art|uno|al".to_owned(),
                rating: 99,
                stats_updated_at: 10,
                ..BranoSalvato::default()
            }],
            ..Contenuto::default()
        };
        let qui = stato_locale(&connection).expect("stato");
        let piano = pianifica(&qui, &dal_salvataggio(&contenuto, &qui.tombstones));
        applica(&mut connection, &piano).expect("il ripristino non deve fallire");

        let voto: i64 = connection
            .query_row("SELECT rating FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("voto");
        assert_eq!(voto, 5);
    }

    #[test]
    fn fondere_due_contenuti_non_fa_scendere_un_conteggio() {
        let brano = |chiave: &str, ascolti: i64| BranoSalvato {
            key: chiave.to_owned(),
            play_count: ascolti,
            ..BranoSalvato::default()
        };
        let locale = Contenuto {
            tracks: vec![brano("a|b|c", 3), brano("d|e|f", 20)],
            roots: vec!["C:\\Qui".to_owned()],
            active_skin: Some("notte".to_owned()),
            ..Contenuto::default()
        };
        let remoto = Contenuto {
            tracks: vec![brano("a|b|c", 17), brano("g|h|i", 1)],
            roots: vec!["D:\\Là".to_owned(), "C:\\Qui".to_owned()],
            active_skin: Some("giorno".to_owned()),
            ..Contenuto::default()
        };
        let unione = fondi(&locale, &remoto);
        let ascolti: Vec<(String, i64)> = unione
            .tracks
            .iter()
            .map(|b| (b.key.clone(), b.play_count))
            .collect();
        assert_eq!(
            ascolti,
            [
                ("a|b|c".to_owned(), 17),
                ("d|e|f".to_owned(), 20),
                ("g|h|i".to_owned(), 1),
            ]
        );
        assert_eq!(unione.roots, ["C:\\Qui".to_owned(), "D:\\Là".to_owned()]);
        assert_eq!(
            unione.active_skin.as_deref(),
            Some("notte"),
            "vince il locale"
        );

        // Idempotente: rifonderla con lo stesso remoto non cambia niente.
        assert_eq!(fondi(&unione, &remoto), unione);
    }

    #[test]
    fn fondere_non_resuscita_una_playlist_cancellata() {
        let mut lapidi = Lapidi::default();
        lapidi.playlists.insert("serata".to_owned(), 100);
        let locale = Contenuto {
            tombstones: lapidi,
            ..Contenuto::default()
        };
        let remoto = Contenuto {
            playlists: vec![PlaylistSalvata {
                key: "serata".to_owned(),
                name: "Serata".to_owned(),
                updated_at: 40,
                ..PlaylistSalvata::default()
            }],
            ..Contenuto::default()
        };
        let unione = fondi(&locale, &remoto);
        assert!(unione.playlists.is_empty());
        assert_eq!(unione.tombstones.playlists.get("serata"), Some(&100));
    }
}
