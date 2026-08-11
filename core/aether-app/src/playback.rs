//! Quel che la riproduzione lascia sul database.
//!
//! Il motore in `aether-play` non sa cos'è una libreria: riceve dei byte e li fa
//! uscire dalle casse. Questo modulo è il ponte fra le due cose — trova i byte
//! di un brano, e scrive quel che è successo dopo che è stato ascoltato.
//!
//! # Le tre scritture, e perché stanno in una transazione sola
//!
//! Un ascolto finito tocca `play_count`, `last_played_at` e `play_history`. Sono
//! tre fatti sullo stesso evento, e mezzo evento è peggio di nessun evento: una
//! riga di cronologia senza il conteggio corrispondente rende i due numeri
//! discordi per sempre, e non c'è nessun posto da cui riconciliarli — la
//! cronologia d'ascolto è, con voti e preferiti, l'unica cosa in tutta la
//! libreria che una scansione non può ricostruire.

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::listen::Listen;
use aether_domain::queue::{QueueSnapshot, RepeatMode};
use aether_play::{BANDE, LIMITE_DB, Sorgente};
use rusqlite::Connection;

use crate::files::MusicFiles;

/// La chiave con cui la coda sta in `settings`.
pub const CHIAVE_CODA: &str = "player.queue";

/// La chiave con cui volume e silenziamento stanno in `settings`.
pub const CHIAVE_VOLUME: &str = "player.volume";

/// La chiave con cui la curva dell'equalizzatore sta in `settings`.
pub const CHIAVE_EQ: &str = "player.eq";

/// La chiave con cui stanno le curve salvate dall'utente.
///
/// Separata da [`CHIAVE_EQ`] perché sono due cose con due vite diverse: la curva
/// corrente cambia a ogni trascinamento di un cursore, l'elenco dei preset una
/// volta ogni tanto. Tenerle nello stesso valore vorrebbe dire riscrivere
/// l'elenco intero sedici volte al secondo.
pub const CHIAVE_EQ_PRESET: &str = "player.eq.presets";

/// La chiave con cui la normalizzazione ReplayGain sta in `settings`.
pub const CHIAVE_REPLAYGAIN: &str = "player.replaygain";

fn db_error(cosa: &str, err: &rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::DbQueryFailed {
        detail: Some(cosa.to_owned()),
    })
    .with_cause(err.to_string())
}

/// Prepara un brano per il motore.
///
/// Legge dal database quel che il motore non può sapere — il percorso, la durata
/// dichiarata, il guadagno ReplayGain — e apre il file attraverso
/// [`MusicFiles`], non con `std::fs`. Quella indirezione è ciò che permetterà
/// alla stessa funzione di girare su Android, dove non c'è un percorso da
/// aprire ma una concessione del sistema.
pub fn sorgente(
    connection: &Connection,
    files: &dyn MusicFiles,
    track_id: i64,
) -> Result<Sorgente, AppError> {
    let (path, durata_ms, replaygain_db): (String, i64, Option<f64>) = connection
        .query_row(
            "SELECT path, duration_ms, replaygain_track_db FROM tracks WHERE id = ?1",
            [track_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|err| match err {
            // Un brano che non c'è più non è un guasto del database: è una riga
            // cancellata mentre la coda la teneva ancora. Il codice giusto è
            // quello della sorgente, così la finestra sa che deve saltarlo.
            rusqlite::Error::QueryReturnedNoRows => {
                AppError::new(ErrorCode::PlaybackSourceUnavailable {
                    track_id: Some(track_id),
                    path: None,
                })
            }
            altro => db_error("lettura del brano da suonare", &altro),
        })?;

    let media = files.open(&path).map_err(|err| {
        AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: Some(track_id),
            path: Some(path.clone()),
        })
        .with_cause(err.cause().unwrap_or(err.code().kind().code()).to_owned())
    })?;

    Ok(Sorgente {
        track_id,
        media: Box::new(Adattatore::nuovo(media)),
        estensione: estensione_di(&path),
        durata_ms: u64::try_from(durata_ms).unwrap_or(0),
        // `f64` nel database perché SQLite non ha i float a 32 bit; il motore
        // lavora in `f32`, che per dei decibel è largamente sufficiente — la
        // correzione viene poi tagliata fra −24 e +12 dB comunque.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "sono decibel: la precisione di un f32 è un milionesimo di dB"
        )]
        replaygain_db: replaygain_db.map(|db| db as f32),
    })
}

/// L'estensione di un percorso, in minuscolo e senza il punto.
fn estensione_di(path: &str) -> Option<String> {
    let ultimo = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (_, ext) = ultimo.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 5 {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Registra un ascolto finito.
///
/// **Non scrive niente se l'ascolto non conta.** La decisione l'ha già presa
/// `aether_domain::listen::counts_as_play`, e questa funzione la rispetta invece
/// di riprenderla: nel vecchio albero il conteggio si incrementava all'avvio del
/// brano, e scorrere cinquanta canzoni saltandole dopo un secondo ne
/// incrementava cinquanta.
///
/// Restituisce `true` se ha scritto.
pub fn record_play(connection: &mut Connection, ascolto: &Listen) -> Result<bool, AppError> {
    if !ascolto.counts {
        return Ok(false);
    }
    let ms_played = i64::try_from(ascolto.listened_ms).unwrap_or(i64::MAX);
    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione d'ascolto", &err))?;

    // `stats_updated_at` insieme agli altri: è ciò su cui `merge_stats` decide
    // chi vince quando due dispositivi si allineano. Scrivere il conteggio senza
    // di lui vorrebbe dire che l'ascolto di stasera perde contro quello di ieri
    // fatto sul telefono.
    tx.execute(
        "UPDATE tracks
            SET play_count = play_count + 1,
                last_played_at = ?2,
                stats_updated_at = ?2
          WHERE id = ?1",
        rusqlite::params![ascolto.track_id, ascolto.started_at],
    )
    .map_err(|err| db_error("conteggio d'ascolto", &err))?;

    tx.execute(
        "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, ?2, ?3)",
        rusqlite::params![ascolto.track_id, ascolto.started_at, ms_played],
    )
    .map_err(|err| db_error("cronologia d'ascolto", &err))?;

    tx.commit()
        .map_err(|err| db_error("chiusura della transazione d'ascolto", &err))?;
    Ok(true)
}

/// La coda, nella forma che va sul disco.
///
/// Un tipo a parte invece di serializzare `QueueSnapshot` direttamente: il
/// dominio non ha serde fra le dipendenze, e non deve averlo. Il formato con cui
/// una cosa si scrive è una decisione di chi la scrive, non della cosa.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CodaSuDisco {
    tracks: Vec<i64>,
    order: Vec<usize>,
    order_pos: Option<usize>,
    shuffle: bool,
    repeat: String,
}

const fn nome_ripetizione(repeat: RepeatMode) -> &'static str {
    match repeat {
        RepeatMode::Off => "off",
        RepeatMode::One => "one",
        RepeatMode::All => "all",
    }
}

fn ripetizione_da_nome(nome: &str) -> RepeatMode {
    match nome {
        "one" => RepeatMode::One,
        "all" => RepeatMode::All,
        _ => RepeatMode::Off,
    }
}

/// Conserva la coda perché sopravviva alla chiusura.
pub fn save_queue(connection: &Connection, coda: &QueueSnapshot) -> Result<(), AppError> {
    let su_disco = CodaSuDisco {
        tracks: coda.tracks.clone(),
        order: coda.order.clone(),
        order_pos: coda.order_pos,
        shuffle: coda.shuffle,
        repeat: nome_ripetizione(coda.repeat).to_owned(),
    };
    let json = serde_json::to_string(&su_disco).unwrap_or_else(|_| "{}".to_owned());
    crate::settings::write(connection, CHIAVE_CODA, &json)
}

/// Rilegge la coda conservata.
///
/// Una coda illeggibile vale **coda vuota, non errore**: è la stessa scelta che
/// `comandi.rs` fa per le cartelle sorvegliate, e per la stessa ragione. Un
/// avvio che fallisce per un valore d'impostazione corrotto non lascia
/// all'utente nessun modo di correggerlo; una coda vuota sì, ne fa un'altra.
///
/// Gli invarianti — che l'ordine sia davvero una permutazione — li verifica
/// `Queue::restore`, che è dove stanno le regole della coda.
pub fn load_queue(connection: &Connection) -> Result<QueueSnapshot, AppError> {
    let Some(json) = crate::settings::read(connection, CHIAVE_CODA)? else {
        return Ok(QueueSnapshot::default());
    };
    let Ok(su_disco) = serde_json::from_str::<CodaSuDisco>(&json) else {
        return Ok(QueueSnapshot::default());
    };
    Ok(QueueSnapshot {
        tracks: su_disco.tracks,
        order: su_disco.order,
        order_pos: su_disco.order_pos,
        shuffle: su_disco.shuffle,
        repeat: ripetizione_da_nome(&su_disco.repeat),
    })
}

/// Volume e silenziamento, come stanno su disco.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Volume {
    /// Da 0 a 1.
    pub volume: f32,
    /// Silenziato.
    pub muto: bool,
}

impl Default for Volume {
    fn default() -> Self {
        // Non 1.0: un lettore che parte al massimo la prima volta è un lettore
        // che fa saltare sulla sedia.
        Self {
            volume: 0.8,
            muto: false,
        }
    }
}

/// Conserva volume e silenziamento.
pub fn save_volume(connection: &Connection, volume: Volume) -> Result<(), AppError> {
    let json = serde_json::to_string(&volume).unwrap_or_else(|_| "{}".to_owned());
    crate::settings::write(connection, CHIAVE_VOLUME, &json)
}

/// Rilegge volume e silenziamento.
pub fn load_volume(connection: &Connection) -> Result<Volume, AppError> {
    let Some(json) = crate::settings::read(connection, CHIAVE_VOLUME)? else {
        return Ok(Volume::default());
    };
    Ok(serde_json::from_str::<Volume>(&json)
        .map(|v| Volume {
            volume: v.volume.clamp(0.0, 1.0),
            ..v
        })
        .unwrap_or_default())
}

// ── la normalizzazione ──────────────────────────────────────────────────────

/// La normalizzazione ReplayGain, come sta su disco.
///
/// Assente vale **accesa**, ed è l'unico valore di serie che qui non si sceglie
/// ma si constata: il motore parte con `replaygain_attivo: true` da quando
/// esiste, e questa chiave arriva dopo. Scrivere `false` come valore di serie
/// vorrebbe dire che la prima apertura dopo l'aggiornamento spegne di nascosto
/// una cosa che era accesa — un cambiamento di volume che nessuno ha chiesto e
/// che nessuna schermata spiega.
///
/// Non c'è nessun rischio nel lasciarla accesa su una libreria senza tag: senza
/// `replaygain_db` la correzione è esattamente 1.0, cioè niente. Il valore serve
/// a chi i tag ce li ha e vuole i dischi come sono stati masterizzati.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Normalizzazione {
    /// Il guadagno dei tag si applica.
    pub attivo: bool,
    /// A quanti LUFS si porta tutto. Il riferimento dei tag è −18.
    pub bersaglio_db: f32,
}

impl Default for Normalizzazione {
    fn default() -> Self {
        Self {
            attivo: true,
            bersaglio_db: BERSAGLIO_PREDEFINITO_DB,
        }
    }
}

/// Il riferimento a cui i tag ReplayGain sono misurati.
///
/// A −18 il tag si usa com'è scritto; è il valore del riferimento originale, ed
/// è la ragione per cui è questo e non −14 (lo standard delle piattaforme di
/// streaming, che qui vorrebbe dire alzare tutto di quattro decibel).
pub const BERSAGLIO_PREDEFINITO_DB: f32 = -18.0;

/// I limiti oltre cui un bersaglio non è più una preferenza ma un guasto.
///
/// Gli stessi che `aether_play::guadagno` applica alla correzione finale: qui
/// per non scrivere su disco un valore che poi verrebbe tagliato in silenzio,
/// lasciando la finestra a mostrare un numero che non è quello che si sente.
const BERSAGLIO_MIN_DB: f32 = -30.0;
const BERSAGLIO_MAX_DB: f32 = -6.0;

/// Conserva la scelta sulla normalizzazione.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_replaygain(
    connection: &Connection,
    normalizzazione: Normalizzazione,
) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_REPLAYGAIN, &sana(normalizzazione))
}

/// Rilegge la scelta sulla normalizzazione.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile vale
/// [`Normalizzazione::default`], cioè accesa: la stessa regola del resto del
/// modulo.
pub fn load_replaygain(connection: &Connection) -> Result<Normalizzazione, AppError> {
    Ok(
        crate::settings::read_json::<Normalizzazione>(connection, CHIAVE_REPLAYGAIN)?
            .map(sana)
            .unwrap_or_default(),
    )
}

/// Porta un bersaglio dentro i limiti, e un valore non finito al riferimento.
fn sana(normalizzazione: Normalizzazione) -> Normalizzazione {
    Normalizzazione {
        attivo: normalizzazione.attivo,
        bersaglio_db: if normalizzazione.bersaglio_db.is_finite() {
            normalizzazione
                .bersaglio_db
                .clamp(BERSAGLIO_MIN_DB, BERSAGLIO_MAX_DB)
        } else {
            BERSAGLIO_PREDEFINITO_DB
        },
    }
}

// ── l'equalizzatore ─────────────────────────────────────────────────────────

/// La curva dell'equalizzatore, come sta su disco.
///
/// I guadagni in un `Vec` e non in un `[f32; BANDE]`: il giorno in cui le bande
/// cambiassero di numero, un valore scritto dalla versione di prima deve valere
/// «adattabile», non «illeggibile» — e un array di lunghezza fissa in serde è
/// esattamente la seconda cosa.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Equalizzazione {
    /// I filtri sono accesi.
    pub attivo: bool,
    /// Quanti decibel per banda, nell'ordine di `aether_play::CENTRI_HZ`.
    pub guadagni: Vec<f32>,
}

impl Default for Equalizzazione {
    fn default() -> Self {
        // Spento, e con la curva piatta: un lettore che alla prima apertura
        // suona già equalizzato è un lettore che suona sbagliato senza dirlo.
        Self {
            attivo: false,
            guadagni: vec![0.0; BANDE],
        }
    }
}

/// Una curva salvata dall'utente con un nome.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PresetEq {
    /// Come l'ha chiamata.
    pub nome: String,
    /// Quanti decibel per banda.
    pub guadagni: Vec<f32>,
}

/// Porta una curva alla forma giusta: [`BANDE`] valori, ognuno nei limiti.
///
/// Una curva più corta si completa con degli zeri, una più lunga si tronca, e i
/// valori non finiti valgono zero. È la stessa cautela con cui [`load_volume`]
/// taglia fra zero e uno, e per lo stesso motivo: questo valore sta in un file
/// che si può aprire e correggere a mano.
fn normalizza(guadagni: &[f32]) -> Vec<f32> {
    (0..BANDE)
        .map(|banda| {
            let grezzo = guadagni.get(banda).copied().unwrap_or(0.0);
            if grezzo.is_finite() {
                grezzo.clamp(-LIMITE_DB, LIMITE_DB)
            } else {
                0.0
            }
        })
        .collect()
}

/// Conserva la curva dell'equalizzatore.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_eq(connection: &Connection, eq: &Equalizzazione) -> Result<(), AppError> {
    crate::settings::write_json(
        connection,
        CHIAVE_EQ,
        &Equalizzazione {
            attivo: eq.attivo,
            guadagni: normalizza(&eq.guadagni),
        },
    )
}

/// Rilegge la curva conservata.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile vale
/// curva piatta e spenta, non errore.
pub fn load_eq(connection: &Connection) -> Result<Equalizzazione, AppError> {
    let Some(letto) = crate::settings::read_json::<Equalizzazione>(connection, CHIAVE_EQ)? else {
        return Ok(Equalizzazione::default());
    };
    Ok(Equalizzazione {
        attivo: letto.attivo,
        guadagni: normalizza(&letto.guadagni),
    })
}

/// Conserva l'elenco delle curve salvate.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_preset_eq(connection: &Connection, preset: &[PresetEq]) -> Result<(), AppError> {
    let puliti: Vec<PresetEq> = preset
        .iter()
        .map(|p| PresetEq {
            nome: p.nome.trim().to_owned(),
            guadagni: normalizza(&p.guadagni),
        })
        .filter(|p| !p.nome.is_empty())
        .collect();
    crate::settings::write_json(connection, CHIAVE_EQ_PRESET, &puliti)
}

/// Rilegge le curve salvate.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un elenco illeggibile vale
/// elenco vuoto: l'utente al massimo risalva una curva, mentre un avvio che
/// fallisce non gli lascia nessun modo di correggerla.
pub fn load_preset_eq(connection: &Connection) -> Result<Vec<PresetEq>, AppError> {
    let letti: Vec<PresetEq> =
        crate::settings::read_json(connection, CHIAVE_EQ_PRESET)?.unwrap_or_default();
    Ok(letti
        .into_iter()
        .map(|p| PresetEq {
            nome: p.nome.trim().to_owned(),
            guadagni: normalizza(&p.guadagni),
        })
        .filter(|p| !p.nome.is_empty())
        .collect())
}

/// Aggiunge una curva all'elenco, o sostituisce quella che aveva già quel nome.
///
/// Sostituire e non affiancare: due voci con lo stesso nome nell'elenco sono
/// due voci che l'utente non sa distinguere, e la seconda è quasi sempre la
/// correzione della prima. Il confronto ignora maiuscole e spazi ai bordi,
/// perché «Sera» e «sera » sono lo stesso nome per chi li scrive.
///
/// Un nome vuoto non salva niente e restituisce `false`.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn salva_preset(
    connection: &Connection,
    nome: &str,
    guadagni: &[f32],
) -> Result<bool, AppError> {
    let nome = nome.trim();
    if nome.is_empty() {
        return Ok(false);
    }
    let mut elenco = load_preset_eq(connection)?;
    let nuovo = PresetEq {
        nome: nome.to_owned(),
        guadagni: normalizza(guadagni),
    };
    match elenco
        .iter_mut()
        .find(|p| p.nome.eq_ignore_ascii_case(nome))
    {
        Some(esistente) => *esistente = nuovo,
        None => elenco.push(nuovo),
    }
    save_preset_eq(connection, &elenco)?;
    Ok(true)
}

/// Toglie una curva salvata. `false` se non ce n'era una con quel nome.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn cancella_preset(connection: &Connection, nome: &str) -> Result<bool, AppError> {
    let nome = nome.trim();
    let mut elenco = load_preset_eq(connection)?;
    let prima = elenco.len();
    elenco.retain(|p| !p.nome.eq_ignore_ascii_case(nome));
    if elenco.len() == prima {
        return Ok(false);
    }
    save_preset_eq(connection, &elenco)?;
    Ok(true)
}

/// Da un flusso di [`MusicFiles`] a un [`Flusso`] per il motore.
///
/// L'unica cosa che aggiunge è la lunghezza, che il decodificatore usa per
/// decidere se può saltare. Si misura una volta all'apertura, riposizionandosi
/// in fondo e tornando indietro: `MusicFiles` non la dichiara, perché per
/// leggere dei tag non serve, e non vale la pena allargare quel tratto per
/// questo.
struct Adattatore {
    interno: Box<dyn crate::files::ReadSeek + Send + Sync>,
    byte: Option<u64>,
}

impl Adattatore {
    fn nuovo(mut interno: Box<dyn crate::files::ReadSeek + Send + Sync>) -> Self {
        use std::io::{Seek as _, SeekFrom};
        // Se il ritorno all'inizio fallisce, la lunghezza vale `None`: un flusso
        // lasciato in fondo e dichiarato saltabile darebbe zero campioni e
        // nessuna spiegazione.
        let byte = interno
            .seek(SeekFrom::End(0))
            .ok()
            .filter(|_| interno.seek(SeekFrom::Start(0)).is_ok());
        Self { interno, byte }
    }
}

impl std::io::Read for Adattatore {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.interno.read(buf)
    }
}

impl std::io::Seek for Adattatore {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.interno.seek(pos)
    }
}

impl aether_play::decodifica::Flusso for Adattatore {
    fn lunghezza(&self) -> Option<u64> {
        self.byte
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::listen::Listen;

    fn db() -> Connection {
        crate::db::open_in_memory()
            .expect("database in memoria")
            .connection
    }

    fn brano(connection: &Connection, id: i64, durata_ms: i64) {
        connection
            .execute(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, 'T', 'A', 'Al', ?4, 1000, 0, 0)",
                rusqlite::params![id, format!("C:/m/{id}.mp3"), format!("k{id}"), durata_ms],
            )
            .expect("brano inserito");
    }

    fn conteggio(connection: &Connection, id: i64) -> (i64, Option<i64>, i64) {
        connection
            .query_row(
                "SELECT t.play_count, t.last_played_at,
                        (SELECT COUNT(*) FROM play_history WHERE track_id = t.id)
                   FROM tracks t WHERE t.id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("conteggio")
    }

    #[test]
    fn un_ascolto_che_conta_scrive_tutte_e_tre_le_cose() {
        let mut c = db();
        brano(&c, 1, 180_000);
        let scritto = record_play(
            &mut c,
            &Listen {
                track_id: 1,
                started_at: 1_700_000_000_000,
                listened_ms: 120_000,
                counts: true,
            },
        )
        .expect("registrato");
        assert!(scritto);
        assert_eq!(conteggio(&c, 1), (1, Some(1_700_000_000_000), 1));
    }

    #[test]
    fn un_brano_saltato_non_lascia_traccia() {
        // È la correzione al difetto del vecchio albero, e questa prova è
        // l'unica cosa che la tiene in piedi.
        let mut c = db();
        brano(&c, 1, 180_000);
        let scritto = record_play(
            &mut c,
            &Listen {
                track_id: 1,
                started_at: 1_700_000_000_000,
                listened_ms: 2_000,
                counts: false,
            },
        )
        .expect("registrato");
        assert!(!scritto);
        assert_eq!(conteggio(&c, 1), (0, None, 0));
    }

    #[test]
    fn due_ascolti_contano_due_volte() {
        let mut c = db();
        brano(&c, 1, 180_000);
        for istante in [1_000i64, 2_000] {
            record_play(
                &mut c,
                &Listen {
                    track_id: 1,
                    started_at: istante,
                    listened_ms: 120_000,
                    counts: true,
                },
            )
            .expect("registrato");
        }
        assert_eq!(conteggio(&c, 1), (2, Some(2_000), 2));
    }

    #[test]
    fn l_ascolto_aggiorna_anche_l_orologio_delle_statistiche() {
        let mut c = db();
        brano(&c, 1, 180_000);
        record_play(
            &mut c,
            &Listen {
                track_id: 1,
                started_at: 555,
                listened_ms: 120_000,
                counts: true,
            },
        )
        .expect("registrato");
        let stats: i64 = c
            .query_row(
                "SELECT stats_updated_at FROM tracks WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .expect("stats");
        assert_eq!(stats, 555, "senza, la sincronia perderebbe questo ascolto");
    }

    #[test]
    fn la_coda_torna_com_era() {
        let c = db();
        let coda = QueueSnapshot {
            tracks: vec![3, 1, 2],
            order: vec![2, 0, 1],
            order_pos: Some(1),
            shuffle: true,
            repeat: RepeatMode::All,
        };
        save_queue(&c, &coda).expect("salvata");
        assert_eq!(load_queue(&c).expect("riletta"), coda);
    }

    #[test]
    fn nessuna_coda_conservata_da_una_coda_vuota() {
        let c = db();
        assert_eq!(load_queue(&c).expect("riletta"), QueueSnapshot::default());
    }

    #[test]
    fn una_coda_illeggibile_non_impedisce_l_avvio() {
        let c = db();
        crate::settings::write(&c, CHIAVE_CODA, "{non è json").expect("scritta");
        assert_eq!(load_queue(&c).expect("riletta"), QueueSnapshot::default());
    }

    #[test]
    fn il_volume_torna_com_era_e_si_taglia() {
        let c = db();
        save_volume(
            &c,
            Volume {
                volume: 0.35,
                muto: true,
            },
        )
        .expect("salvato");
        let riletto = load_volume(&c).expect("riletto");
        assert!((riletto.volume - 0.35).abs() < f32::EPSILON);
        assert!(riletto.muto);

        // Un valore assurdo scritto a mano non deve poter spaccare le orecchie.
        crate::settings::write(&c, CHIAVE_VOLUME, r#"{"volume":9.0,"muto":false}"#)
            .expect("scritto");
        assert!((load_volume(&c).expect("riletto").volume - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn un_volume_mai_impostato_non_e_al_massimo() {
        let c = db();
        assert!(load_volume(&c).expect("riletto").volume < 1.0);
    }

    #[test]
    fn la_curva_torna_com_era() {
        let c = db();
        let curva = Equalizzazione {
            attivo: true,
            guadagni: vec![3.0, -2.0, 0.0, 1.5, 0.0, 0.0, -4.0, 0.0, 6.0, 0.0],
        };
        save_eq(&c, &curva).expect("salvata");
        assert_eq!(load_eq(&c).expect("riletta"), curva);
    }

    #[test]
    fn una_curva_mai_impostata_e_spenta_e_piatta() {
        let c = db();
        let curva = load_eq(&c).expect("riletta");
        assert!(
            !curva.attivo,
            "non deve equalizzare senza che glielo si chieda"
        );
        assert_eq!(curva.guadagni, vec![0.0; BANDE]);
    }

    #[test]
    fn una_curva_assurda_scritta_a_mano_si_taglia() {
        // La stessa cautela del volume: il file si può aprire e correggere, e un
        // +90 dB su una banda non deve poter spaccare le orecchie.
        let c = db();
        crate::settings::write(
            &c,
            CHIAVE_EQ,
            r#"{"attivo":true,"guadagni":[90.0,-90.0,0,0,0,0,0,0,0,0]}"#,
        )
        .expect("scritta");
        let letta = load_eq(&c).expect("riletta");
        assert_eq!(letta.guadagni.first().copied(), Some(LIMITE_DB));
        assert_eq!(letta.guadagni.get(1).copied(), Some(-LIMITE_DB));
    }

    #[test]
    fn una_curva_di_lunghezza_sbagliata_si_normalizza() {
        // Il caso vero: un `player.eq` scritto da una versione con un numero di
        // bande diverso. Deve valere adattabile, non illeggibile.
        let c = db();
        crate::settings::write(&c, CHIAVE_EQ, r#"{"attivo":true,"guadagni":[4.0,5.0]}"#)
            .expect("scritta");
        let letta = load_eq(&c).expect("riletta");
        assert!(letta.attivo, "la curva era leggibile: non va buttata");
        assert_eq!(letta.guadagni.len(), BANDE);
        assert_eq!(letta.guadagni.first().copied(), Some(4.0));
        assert_eq!(letta.guadagni.last().copied(), Some(0.0));
    }

    #[test]
    fn una_curva_illeggibile_non_impedisce_l_avvio() {
        let c = db();
        crate::settings::write(&c, CHIAVE_EQ, "{non è json").expect("scritta");
        assert_eq!(load_eq(&c).expect("riletta"), Equalizzazione::default());
    }

    #[test]
    fn la_normalizzazione_mai_impostata_e_accesa() {
        // Il valore di serie non è una preferenza: è quel che il motore fa da
        // sempre. Se questo diventasse `false`, il primo avvio dopo
        // l'aggiornamento cambierebbe il volume di chi ha i tag senza dirglielo.
        let c = db();
        let letta = load_replaygain(&c).expect("riletta");
        assert!(letta.attivo);
        assert_eq!(letta.bersaglio_db, BERSAGLIO_PREDEFINITO_DB);
    }

    #[test]
    fn spegnere_la_normalizzazione_resta_scritto() {
        let c = db();
        save_replaygain(
            &c,
            Normalizzazione {
                attivo: false,
                bersaglio_db: -14.0,
            },
        )
        .expect("salvata");
        let letta = load_replaygain(&c).expect("riletta");
        assert!(!letta.attivo);
        assert_eq!(letta.bersaglio_db, -14.0);
    }

    #[test]
    fn un_bersaglio_assurdo_si_taglia() {
        // Come per la curva: il valore si può scrivere a mano nel database, e
        // `aether_play::guadagno` taglierebbe comunque la correzione a ±24 dB —
        // ma in silenzio, lasciando la finestra a mostrare un numero che non è
        // quello che si sente.
        let c = db();
        crate::settings::write(
            &c,
            CHIAVE_REPLAYGAIN,
            r#"{"attivo":true,"bersaglio_db":40.0}"#,
        )
        .expect("scritta");
        assert_eq!(
            load_replaygain(&c).expect("riletta").bersaglio_db,
            BERSAGLIO_MAX_DB
        );
    }

    #[test]
    fn una_normalizzazione_illeggibile_non_impedisce_l_avvio() {
        let c = db();
        crate::settings::write(&c, CHIAVE_REPLAYGAIN, "{non è json").expect("scritta");
        assert_eq!(
            load_replaygain(&c).expect("riletta"),
            Normalizzazione::default()
        );
    }

    #[test]
    fn i_preset_fanno_andata_e_ritorno() {
        let c = db();
        assert!(salva_preset(&c, "Sera", &[2.0; BANDE]).expect("salvato"));
        assert!(salva_preset(&c, "Cuffie", &[-1.0; BANDE]).expect("salvato"));
        let elenco = load_preset_eq(&c).expect("riletti");
        assert_eq!(elenco.len(), 2);
        assert_eq!(elenco.first().map(|p| p.nome.as_str()), Some("Sera"));
    }

    #[test]
    fn salvare_con_un_nome_che_c_e_gia_sostituisce() {
        // Due voci con lo stesso nome sono due voci che l'utente non sa
        // distinguere, e la seconda è quasi sempre la correzione della prima.
        let c = db();
        salva_preset(&c, "Sera", &[2.0; BANDE]).expect("salvato");
        salva_preset(&c, "  sera ", &[5.0; BANDE]).expect("risalvato");
        let elenco = load_preset_eq(&c).expect("riletti");
        assert_eq!(elenco.len(), 1, "il nome era lo stesso a meno di spazi");
        assert_eq!(
            elenco.first().map(|p| p.guadagni.first().copied()),
            Some(Some(5.0))
        );
        // Il nome resta quello scritto per ultimo, ripulito.
        assert_eq!(elenco.first().map(|p| p.nome.as_str()), Some("sera"));
    }

    #[test]
    fn un_preset_senza_nome_non_si_salva() {
        let c = db();
        assert!(!salva_preset(&c, "   ", &[2.0; BANDE]).expect("provato"));
        assert!(load_preset_eq(&c).expect("riletti").is_empty());
    }

    #[test]
    fn cancellare_un_preset_lo_toglie_e_dice_se_c_era() {
        let c = db();
        salva_preset(&c, "Sera", &[2.0; BANDE]).expect("salvato");
        assert!(
            !cancella_preset(&c, "Mattina").expect("provato"),
            "non c'era"
        );
        assert!(cancella_preset(&c, "SERA").expect("cancellato"));
        assert!(load_preset_eq(&c).expect("riletti").is_empty());
    }

    #[test]
    fn un_elenco_di_preset_illeggibile_vale_elenco_vuoto() {
        let c = db();
        crate::settings::write(&c, CHIAVE_EQ_PRESET, "{non è json").expect("scritta");
        assert!(load_preset_eq(&c).expect("riletti").is_empty());
    }

    #[test]
    fn le_estensioni_si_leggono_da_percorsi_di_entrambi_i_sistemi() {
        assert_eq!(estensione_di("C:\\m\\a.MP3").as_deref(), Some("mp3"));
        assert_eq!(estensione_di("/home/a/b.flac").as_deref(), Some("flac"));
        assert_eq!(estensione_di("senza"), None);
        // Una cartella con un punto nel nome non deve diventare un'estensione.
        assert_eq!(estensione_di("C:\\v1.2\\brano"), None);
    }

    #[test]
    fn un_brano_cancellato_da_un_errore_di_sorgente_non_di_database() {
        use aether_domain::errors::ErrorCodeKind;
        let c = db();
        let err = sorgente(&c, &crate::files::LocalFiles, 999).expect_err("deve fallire");
        assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackSourceUnavailable);
    }

    #[test]
    fn un_file_che_non_c_e_piu_da_lo_stesso_errore() {
        use aether_domain::errors::ErrorCodeKind;
        let c = db();
        brano(&c, 1, 1000);
        let err = sorgente(&c, &crate::files::LocalFiles, 1).expect_err("deve fallire");
        assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackSourceUnavailable);
    }
}
