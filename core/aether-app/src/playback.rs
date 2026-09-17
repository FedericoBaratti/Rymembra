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

use aether_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use aether_domain::listen::Listen;
use aether_domain::queue::{QueueSnapshot, RepeatMode};
use aether_play::{BANDE, LIMITE_DB, Sorgente};
use rusqlite::Connection;

use crate::files::MusicFiles;
use crate::library::db_error;

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

/// La chiave con cui sta quante barre disegna lo spettro.
///
/// In `settings` e non in `localStorage`, come tutto il resto delle preferenze
/// da quando il tema si è spostato qui: `localStorage` non sopravvive a una
/// reinstallazione, e una scelta che sparisce cambiando computer è una scelta
/// che va rifatta ogni volta.
///
/// Qui c'era scritto che le preferenze «viaggiano con il backup e con la
/// sincronia»: non era vero per questa chiave, e non lo è per quasi nessuna. Il
/// backup su Drive copia **due** righe di `settings` (le cartelle sorvegliate e
/// la skin attiva) e la sincronia **tre**. L'unico meccanismo che porta una
/// preferenza da un computer a un altro è il [profilo](crate::profilo), che è
/// un elenco di inclusioni — e fino a oggi questa chiave in quell'elenco non
/// c'era. Adesso c'è, insieme a [`CHIAVE_SPETTRO_VISIBILE`].
pub const CHIAVE_SPETTRO_BANDE: &str = "player.spectrum.bands";

/// La chiave con cui sta se la scena dello spettro parte accesa.
///
/// Di serie **spenta** — chi apre «In riproduzione» è venuto a guardare la
/// copertina — ma la scelta si ricorda, e viaggia nel profilo: «voglio vedere
/// lo spettro» è un gusto di chi ascolta, non un fatto di questa macchina, e
/// resta vero su qualunque computer.
pub const CHIAVE_SPETTRO_VISIBILE: &str = "player.spectrum.visible";

/// La chiave con cui sta il tetto di qualità della scena dello spettro.
///
/// **Non** viaggia nel profilo, ed è deliberato: è la terza omissione di
/// `crate::profilo`, dopo la coda e l'uscita audio, e per la stessa ragione —
/// «alta» descrive quel che *questa* scheda video regge. La prova che tiene
/// deliberata l'omissione sta in fondo a quel file.
pub const CHIAVE_SPETTRO_QUALITA: &str = "player.spectrum.quality";

/// La chiave con cui sta se i dati tecnici del file si mostrano.
///
/// Di serie **accesa**, al contrario dello spettro: quella riga non costa né
/// una GPU né un filo — è una query su una riga sola, fatta quando il brano
/// cambia — e chi apre «In riproduzione» ha il diritto di sapere cosa sta
/// sentendo senza prima scoprire che esiste un interruttore. Assente vuol dire
/// accesa, come per `aggiornamenti.attivo`: scrivere `true` al primo avvio
/// vorrebbe dire una riga in `settings` per non dire niente di nuovo.
///
/// Viaggia nel profilo, accanto a [`CHIAVE_SPETTRO_VISIBILE`] e per la stessa
/// ragione: «voglio leggere com'è fatto il file» è un gusto di chi ascolta, non
/// un fatto di questa macchina, e resta vero su qualunque computer.
pub const CHIAVE_FORMATO_VISIBILE: &str = "player.fileFormat.visible";

/// Quel che il database sa di un brano da suonare.
///
/// # Perché è un tipo e non tre variabili dentro una funzione
///
/// Perché separa le due metà di «prepara un brano»: la prima è una query, dura
/// microsecondi e vuole il lucchetto della libreria; la seconda apre un file, su
/// una condivisione morta dura quaranta secondi e non deve tenere in mano
/// **niente**. Finché erano una funzione sola, chi apriva un brano teneva il
/// lucchetto per tutta la durata dell'apertura — e con la share giù restavano
/// fermi anche tutti i fili di sottofondo.
#[derive(Debug, Clone)]
pub struct SchedaSorgente {
    /// Quale brano.
    pub track_id: i64,
    /// Dove sta il file.
    pub path: String,
    /// La durata secondo il database, per quando il file non la dichiara.
    pub durata_ms: i64,
    /// Il guadagno ReplayGain scritto nei tag, se c'era.
    pub replaygain_db: Option<f64>,
}

/// La sola query: cosa dice il database di questo brano.
///
/// # Errori
///
/// `playback.sourceUnavailable` se la riga non c'è più, `db.queryFailed` se il
/// database non risponde.
pub fn scheda_sorgente(connection: &Connection, track_id: i64) -> Result<SchedaSorgente, AppError> {
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
    Ok(SchedaSorgente {
        track_id,
        path,
        durata_ms,
        replaygain_db,
    })
}

/// La sola apertura: dalla scheda ai byte pronti per il motore.
///
/// Il file si apre attraverso [`MusicFiles`], non con `std::fs`. Quella
/// indirezione è ciò che permetterà alla stessa funzione di girare su Android,
/// dove non c'è un percorso da aprire ma una concessione del sistema.
///
/// **Non tocca il database**, ed è tutto il punto: questa è la metà lenta, la
/// sola che può restare appesa su una share morta, e chi la chiama può quindi
/// mandarla su un altro filo con una scadenza addosso (vedi
/// [`crate::scadenza::con_scadenza`]) senza avere in mano nessun lucchetto.
///
/// # Errori
///
/// `fs.networkUnavailable` se il guasto viene dalla rete — e passa **intero**,
/// perché è ritentabile; `playback.sourceUnavailable` per ogni altro modo in cui
/// il file non si apre.
pub fn sorgente_da_scheda(
    files: &dyn MusicFiles,
    scheda: &SchedaSorgente,
) -> Result<Sorgente, AppError> {
    let media = files.open(&scheda.path).map_err(|err| {
        // La rete giù si propaga **così com'è**: è ritentabile, e la finestra
        // ci mette accanto il tasto «Riprova». Impacchettarla in
        // `playback.sourceUnavailable` — che il catalogo dichiara mai
        // ritentabile, perché rileggere un file rotto non cambia esito — vorrebbe
        // dire dire a chi ha staccato il cavo che il brano è irrecuperabile.
        if err.code().kind() == ErrorCodeKind::FsNetworkUnavailable {
            return err;
        }
        AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: Some(scheda.track_id),
            path: Some(scheda.path.clone()),
        })
        .with_cause(err.cause().unwrap_or(err.code().kind().code()).to_owned())
    })?;

    Ok(Sorgente {
        track_id: scheda.track_id,
        media: Box::new(Adattatore::nuovo(media)),
        estensione: estensione_di(&scheda.path),
        durata_ms: u64::try_from(scheda.durata_ms).unwrap_or(0),
        // `f64` nel database perché SQLite non ha i float a 32 bit; il motore
        // lavora in `f32`, che per dei decibel è largamente sufficiente — la
        // correzione viene poi tagliata fra −24 e +12 dB comunque.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "sono decibel: la precisione di un f32 è un milionesimo di dB"
        )]
        replaygain_db: scheda.replaygain_db.map(|db| db as f32),
    })
}

/// Prepara un brano per il motore: la scheda e poi l'apertura, di fila.
///
/// L'involucro delle due metà, per chi non ha lucchetti da mollare in mezzo —
/// gli esempi, le prove, e in generale chi chiama da un filo suo.
pub fn sorgente(
    connection: &Connection,
    files: &dyn MusicFiles,
    track_id: i64,
) -> Result<Sorgente, AppError> {
    sorgente_da_scheda(files, &scheda_sorgente(connection, track_id)?)
}

/// L'estensione di un percorso, in minuscolo e senza il punto.
///
/// La sorella su indirizzi è `aether_domain::indirizzo::estensione_da_url`,
/// e le due regole si somigliano senza coincidere: quella butta prima query
/// e schema e scarta le code non alfanumeriche, questa spezza anche sulla
/// barra rovesciata perché i percorsi che le arrivano vengono dal disco di
/// Windows. Restano due funzioni perché sono due domande diverse.
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
///
/// # A nome di chi
///
/// `dispositivo` è l'identificativo di **questa** macchina, e non è un dettaglio
/// contabile: il conteggio non è più un numero in una colonna ma una somma di
/// numeri, uno per dispositivo, e chi non dice il proprio nome non può
/// contribuire. Vedi [`crate::sincronia::conta_ascolto`] sul perché la somma
/// è l'unica forma che regge fra due macchine — con un numero solo, fondere può
/// solo raddoppiare la storia o perderne metà.
pub fn record_play(
    connection: &mut Connection,
    ascolto: &Listen,
    dispositivo: &str,
) -> Result<bool, AppError> {
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
    //
    // `play_count` non si incrementa più qui: lo ricalcola `conta_ascolto` come
    // somma dei contatori, subito sotto e nella stessa transazione. Farlo in due
    // posti darebbe due volte lo stesso ascolto.
    tx.execute(
        "UPDATE tracks
            SET last_played_at = ?2,
                stats_updated_at = ?2
          WHERE id = ?1",
        rusqlite::params![ascolto.track_id, ascolto.started_at],
    )
    .map_err(|err| db_error("conteggio d'ascolto", &err))?;

    crate::sincronia::conta_ascolto(&tx, ascolto.track_id, dispositivo)?;

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

/// Il bersaglio «basso», per chi ascolta di notte o in cuffia.
///
/// Cinque decibel sotto il riferimento. Non è «più silenzioso» nel senso del
/// volume — quello ha già il suo cursore — ma un punto di arrivo più basso a
/// cui *tutti* i brani vengono portati: la differenza si sente su una
/// scaletta che mescola un disco degli anni Ottanta e una rimasterizzazione
/// recente, dove la seconda arriva molto più forte del primo.
pub const BERSAGLIO_BASSO_DB: f32 = -23.0;

/// Il bersaglio «alto», quello delle piattaforme di streaming.
///
/// È il valore a cui normalizzano Spotify e gli altri, ed è il motivo per cui
/// una libreria locale suona più piano di loro a parità di cursore. Alzando di
/// quattro decibel sopra il riferimento dei tag, i brani già forti arrivano
/// oltre lo zero: la correzione finale in `aether_play::guadagno` è tagliata a
/// +12 dB e la conversione di `aether_play::uscita` satura invece di
/// avvolgere, quindi il caso peggiore è una compressione udibile, non un
/// rumore. Chi ha una libreria di dischi rumorosi tiene «normale».
pub const BERSAGLIO_ALTO_DB: f32 = -14.0;

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

// ── dov'eri rimasto ─────────────────────────────────────────────────────────

/// A che punto del brano corrente si era arrivati.
pub const CHIAVE_POSIZIONE: &str = "player.position";

/// Conserva la posizione dentro il brano.
///
/// # Perché una chiave sua e non un campo di `QueueSnapshot`
///
/// Perché `QueueSnapshot` è un tipo del **dominio**, e descrive la struttura
/// di una coda: quali brani, in che ordine, dove siamo dentro l'ordine.
/// Quanti millisecondi sono passati dentro una traccia non è una proprietà
/// della coda — è una proprietà del motore, che il dominio non conosce e non
/// deve conoscere. Infilarcelo dentro vorrebbe dire che
/// `aether_domain::queue`, che oggi non sa nemmeno cosa sia il tempo, si
/// ritrova un campo che solo `aether-play` sa produrre.
///
/// # Perché con il brano accanto
///
/// Perché il segno si scrive ogni cinque secondi e la coda a ogni cambio di
/// brano, e fra le due scritture i conti non tornano: per i primi cinque secondi
/// di un brano nuovo il numero sul database era ancora quello del brano di
/// prima. La Home diceva «Riprendi» su un brano di 0:22 «a 0:29», e il clic
/// saltava lì. Il numero da solo non sa di chi è; con l'identificativo accanto
/// chi lo legge può chiedere se parla del brano che ha in mano.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_posizione(connection: &Connection, track_id: i64, ms: u64) -> Result<(), AppError> {
    crate::settings::write_json(
        connection,
        CHIAVE_POSIZIONE,
        &SegnoPosizione::DelBrano {
            brano: track_id,
            ms,
        },
    )
}

/// Legge dove si era arrivati **dentro `track_id`**. Assente, o di un altro
/// brano, vale l'inizio.
///
/// Il numero nudo di prima di questa versione vale per il brano che si chiede:
/// è l'unica lettura che quel formato ha sempre avuto, ed è giusta nel caso di
/// gran lunga più comune — chi aggiorna a metà di una canzone e riapre.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_posizione(connection: &Connection, track_id: i64) -> Result<u64, AppError> {
    Ok(
        match crate::settings::read_json::<SegnoPosizione>(connection, CHIAVE_POSIZIONE)? {
            Some(SegnoPosizione::DelBrano { brano, ms }) if brano == track_id => ms,
            Some(SegnoPosizione::Vecchio(ms)) => ms,
            _ => 0,
        },
    )
}

/// Come sta sul database il segno di [`save_posizione`].
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum SegnoPosizione {
    /// Brano e millisecondo.
    DelBrano { brano: i64, ms: u64 },
    /// Il numero nudo che si scriveva prima.
    Vecchio(u64),
}

// ── la coda che non finisce ─────────────────────────────────────────────────

/// Se a coda esaurita si continua da soli.
pub const CHIAVE_AUTOPLAY: &str = "player.autoplay";

/// Conserva la scelta sull'autoplay.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_autoplay(connection: &Connection, attivo: bool) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_AUTOPLAY, &attivo)
}

/// Legge la scelta sull'autoplay.
///
/// Assente vale **spento**, ed è il contrario della normalizzazione qui sopra
/// per una ragione precisa: la normalizzazione descriveva quel che il motore
/// già faceva, questo aggiunge un comportamento che prima non c'era. Un
/// aggiornamento che accendesse l'autoplay da sé farebbe partire musica che
/// nessuno ha chiesto, magari a notte fonda, in una casa in cui l'ultimo album
/// era finito apposta.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_autoplay(connection: &Connection) -> Result<bool, AppError> {
    Ok(crate::settings::read_json::<bool>(connection, CHIAVE_AUTOPLAY)?.unwrap_or(false))
}

// ── un brano dentro l'altro ─────────────────────────────────────────────────

/// Quanti secondi dura la sovrapposizione fra un brano e il successivo.
pub const CHIAVE_CROSSFADE: &str = "player.crossfade";

/// Il massimo che si può chiedere, in secondi.
///
/// Dodici come Spotify, e non per imitazione: oltre i dodici secondi la
/// sovrapposizione dura più della coda di quasi ogni brano, e quel che si
/// sente non è più un passaggio ma due canzoni suonate insieme. Il limite è
/// anche una difesa del motore — la finestra di dissolvenza va tenuta in
/// memoria e confrontata con la durata del brano, e un valore assurdo
/// significherebbe una dissolvenza che comincia prima della metà.
pub const CROSSFADE_MASSIMO_S: u64 = 12;

/// Riporta i secondi dentro il consentito.
fn sano_crossfade(secondi: u64) -> u64 {
    secondi.min(CROSSFADE_MASSIMO_S)
}

/// Conserva la durata della dissolvenza incrociata, in secondi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_crossfade(connection: &Connection, secondi: u64) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_CROSSFADE, &sano_crossfade(secondi))
}

/// Legge la durata della dissolvenza. Assente vale **zero**, cioè spenta.
///
/// Spenta di serie per la stessa ragione dell'autoplay qui sopra: fino a ieri
/// il passaggio fra due brani era esatto al campione, ed è una qualità che chi
/// ascolta un album ha scelto Aether per avere. Un aggiornamento che
/// accendesse la dissolvenza da sé sovrapporrebbe le tracce di un disco
/// pensato per non averne — cioè romperebbe il gapless senza che nessuno
/// l'abbia chiesto.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_crossfade(connection: &Connection) -> Result<u64, AppError> {
    Ok(
        crate::settings::read_json::<u64>(connection, CHIAVE_CROSSFADE)?
            .map(sano_crossfade)
            .unwrap_or(0),
    )
}

// ── quanto il suono esce dopo ───────────────────────────────────────────────

/// Di quanti millisecondi la catena d'uscita ritarda il suono, dichiarati a mano.
///
/// # Perché `audio.` e non `player.`
///
/// Perché non descrive il lettore: descrive **questa uscita su questo computer**.
/// Le chiavi `player.*` sono scelte di chi ascolta — il volume, la curva, se
/// l'autoplay continua — e viaggiano nel profilo; questa è una proprietà del
/// cavo, del driver e del DAC che ci sono attaccati, e cambia quando cambia
/// l'hardware, non quando cambia il gusto.
pub const CHIAVE_LATENZA: &str = "audio.latenza_ms";

/// Il massimo che si può dichiarare, in valore assoluto.
///
/// Mezzo secondo. Non è un limite tecnico — una catena Bluetooth scadente ci
/// arriva — è il limite oltre il quale la compensazione smette di correggere e
/// diventa un'altra cosa: a mezzo secondo il cursore sta già mezzo pollice dietro
/// la musica, e chi continuasse ad alzare sarebbe uno che sta cercando di
/// risolvere un problema diverso.
///
/// Si applica **ai due versi**: negativo anticipa la posizione raccontata, ed è
/// il verso che serve a chi trova che i testi arrivino tardi comunque.
pub const LATENZA_MASSIMA_MS: i64 = 500;

/// Riporta i millisecondi dentro il consentito.
fn sana_latenza(ms: i64) -> i64 {
    ms.clamp(-LATENZA_MASSIMA_MS, LATENZA_MASSIMA_MS)
}

/// Conserva la latenza d'uscita dichiarata a mano, in millisecondi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_latenza(connection: &Connection, ms: i64) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_LATENZA, &sana_latenza(ms))
}

/// Legge la latenza dichiarata. Assente vale **zero**.
///
/// Zero di serie, e non una stima: una correzione che nessuno ha chiesto
/// sposterebbe il cursore di tutti per sistemare l'uscita di qualcuno. Il pezzo
/// di catena che si può misurare lo misura già il motore
/// (`aether_play::uscita::annota_latenza`); questo numero esiste per il pezzo che
/// nessuno misura, e chi lo tocca lo fa guardando l'effetto.
///
/// Un valore fuori scala si stringe **in lettura** oltre che in scrittura: questa
/// riga può arrivare da un database scritto a mano o da un backup, e un numero
/// assurdo non deve poter spostare la posizione di mezz'ora.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_latenza(connection: &Connection) -> Result<i64, AppError> {
    Ok(
        crate::settings::read_json::<i64>(connection, CHIAVE_LATENZA)?
            .map(sana_latenza)
            .unwrap_or(0),
    )
}

// ── da quale scheda esce il suono ───────────────────────────────────────────

/// Il nome dell'uscita audio scelta a mano. Assente: quella di sistema.
pub const CHIAVE_USCITA: &str = "player.output";

/// Conserva su quale uscita si vuole sentire Aether.
///
/// `None` non è «non lo so»: è la scelta esplicita «quella che usa il sistema»,
/// e va scritta come le altre — cancellare la chiave e riscriverla sono la
/// stessa cosa per chi legge, ma un `null` in `settings` dice che qualcuno c'è
/// passato, e nel dubbio è quel che si vuole trovare.
///
/// # Perché il nome e non un identificativo
///
/// Perché cpal non ne ha uno: `Device` sa dire soltanto `name()`. Il prezzo è
/// scritto in testa a `aether_play::dispositivi` — nomi tradotti, ripetuti, che
/// un aggiornamento del driver può riscrivere — e si paga con il ripiego:
/// `dispositivi::scegli` torna al predefinito quando il nome non corrisponde a
/// niente, senza chiamarlo errore.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_uscita(connection: &Connection, id: Option<&str>) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_USCITA, &id)
}

/// Rilegge l'uscita scelta. Assente, o illeggibile, vale «quella di sistema».
///
/// Illeggibile **non** è un errore, ed è voluto: questa chiave arriva anche da
/// un database ripristinato da un backup fatto su un altro computer, dove il
/// nome scritto qui non nomina niente. Rifiutare di partire per una preferenza
/// che non si può onorare vorrebbe dire un'applicazione muta al posto di
/// un'applicazione che suona dagli altoparlanti.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_uscita(connection: &Connection) -> Result<Option<String>, AppError> {
    Ok(crate::settings::read_json::<Option<String>>(connection, CHIAVE_USCITA)?.flatten())
}

/// Conserva quante barre disegna lo spettro.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_spettro_bande(connection: &Connection, quante: u16) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_SPETTRO_BANDE, &sane(quante))
}

/// Rilegge quante barre disegna lo spettro.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile o fuori
/// scala vale [`aether_play::RISOLUZIONE_DI_SERIE`]: la stessa regola del resto
/// del modulo, e qui conta doppio perché la preferenza è disegno, non suono —
/// rifiutare di partire per una barra in più sarebbe sproporzionato.
pub fn load_spettro_bande(connection: &Connection) -> Result<u16, AppError> {
    Ok(
        crate::settings::read_json::<u16>(connection, CHIAVE_SPETTRO_BANDE)?
            .map(sane)
            .unwrap_or(aether_play::RISOLUZIONE_DI_SERIE),
    )
}

/// Porta un numero di barre su una delle risoluzioni che esistono.
///
/// Non un `clamp`: fra 64 e 128 non c'è niente, e scrivere 100 vorrebbe dire
/// conservare un valore che il motore poi stringe per conto suo — cioè una
/// preferenza che dice una cosa e ne fa un'altra. Si sceglie la potenza di due
/// più vicina *in rapporto*, che è il modo in cui queste scale si confrontano:
/// fra 64 e 128, il mezzo è 90, non 96.
fn sane(quante: u16) -> u16 {
    aether_play::RISOLUZIONI
        .into_iter()
        .min_by(|a, b| {
            let scarto = |v: u16| (f32::from(v) / f32::from(quante.max(1))).log2().abs();
            scarto(*a).total_cmp(&scarto(*b))
        })
        .unwrap_or(aether_play::RISOLUZIONE_DI_SERIE)
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

// ── lo spettro: se si vede, e quanto può costare ────────────────────────────

/// Quanto la scena dello spettro può spendere su questa macchina.
///
/// Una skin dice come la scena **appare**; questa dice quanto questo computer è
/// disposto a pagarla. Sono due domande diverse, e tenerle separate è ciò che
/// permette alla prima di viaggiare in un profilo mentre la seconda resta qui.
///
/// # Perché non c'è nessun `sane()` a mano
///
/// Perché il ritaglio esiste già, scritto una volta sola e più in alto.
/// `#[serde(rename_all = "lowercase")]` significa che nel database finiscono
/// esattamente tre parole — `"auto"`, `"alta"`, `"bassa"` — e che qualunque
/// altra cosa, `"ultra"` compreso, **non si deserializza**.
/// [`crate::settings::read_json`] dichiara già che un valore che non si
/// interpreta vale *come se non ci fosse*, e assente vale [`Qualita::Auto`].
/// Il ritaglio cade fuori da quella regola invece di essere una seconda regola
/// che qualcuno dovrà ricordarsi di tenere allineata alla prima.
///
/// Per le barre è diverso, e [`sane`] resta dov'è: là i valori validi sono otto
/// numeri fra i quali non c'è niente, e un `100` scritto da una versione con un
/// altro elenco di risoluzioni **si deserializza benissimo** — il tipo è `u16`,
/// non un'enumerazione. Là il ritaglio va scritto perché serde non lo fa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Qualita {
    /// Lo decide la scena, guardando quel che questa macchina riesce a fare.
    Auto,
    /// Il massimo che la scena sa disegnare, costi quel che costi.
    Alta,
    /// Il minimo indispensabile: ventola ferma e batteria che dura.
    Bassa,
}

impl Qualita {
    /// Il livello che porta questo nome; ogni altro nome vale [`Qualita::Auto`].
    ///
    /// È l'ingresso dal filo: la finestra manda una stringa, e una stringa può
    /// essere qualunque cosa. I nomi sono gli **stessi** che serde scrive nel
    /// database — se i due elenchi divergessero, la preferenza di ieri
    /// diventerebbe illeggibile oggi, in silenzio — e c'è una prova che lo
    /// tiene vero: `il_nome_sul_filo_e_il_nome_nel_database`.
    #[must_use]
    pub fn da_nome(nome: &str) -> Self {
        match nome {
            "alta" => Self::Alta,
            "bassa" => Self::Bassa,
            _ => Self::Auto,
        }
    }
}

/// Conserva se la scena dello spettro parte accesa.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_spettro_visibile(connection: &Connection, acceso: bool) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_SPETTRO_VISIBILE, &acceso)
}

/// Rilegge se la scena dello spettro parte accesa. Mai scelta, è spenta.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile vale
/// come assente, cioè spento: la stessa regola del resto del modulo, e qui la
/// più conservativa delle due — una scena che non si accende si accende con un
/// click, una che si accende da sola su un dato storto costa una GPU a chi non
/// l'aveva chiesta.
pub fn load_spettro_visibile(connection: &Connection) -> Result<bool, AppError> {
    Ok(crate::settings::read_json::<bool>(connection, CHIAVE_SPETTRO_VISIBILE)?.unwrap_or(false))
}

/// Conserva il tetto di qualità della scena dello spettro.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_spettro_qualita(connection: &Connection, qualita: Qualita) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_SPETTRO_QUALITA, &qualita)
}

/// Rilegge il tetto di qualità. Mai scelto, o illeggibile, è [`Qualita::Auto`].
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un nome che questa versione
/// non conosce non è un errore, e la ragione sta nella carta di [`Qualita`]:
/// serde rifiuta di leggerlo, `read_json` lo tratta come assente, e assente è
/// «lo decide la scena» — che è la risposta giusta anche quando la preferenza
/// arriva da una versione che aveva un livello in più.
pub fn load_spettro_qualita(connection: &Connection) -> Result<Qualita, AppError> {
    Ok(
        crate::settings::read_json::<Qualita>(connection, CHIAVE_SPETTRO_QUALITA)?
            .unwrap_or(Qualita::Auto),
    )
}

// ── i dati tecnici del file: se si mostrano ─────────────────────────────────

/// Conserva se i dati tecnici del file si mostrano.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_formato_visibile(connection: &Connection, acceso: bool) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_FORMATO_VISIBILE, &acceso)
}

/// Rilegge se i dati tecnici del file si mostrano. Mai scelta, è accesa.
///
/// # Perché il valore di serie è l'opposto di quello dello spettro
///
/// Perché il costo è l'opposto. Una scena WebGL che si accende da sola su un
/// valore illeggibile costa una GPU a chi non l'aveva chiesta, e lì «assente
/// vale spento» è la più conservativa delle due risposte. Qui la riga costa una
/// `SELECT` su quattro colonne di una riga sola, una volta per cambio di brano:
/// il danno di un'accensione indesiderata è una riga di testo in più, e il
/// danno di uno spegnimento indesiderato è che la funzione non esiste per chi
/// non sa di doverla cercare.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile vale
/// come assente, cioè acceso: la stessa regola del resto del modulo, applicata
/// al valore di serie che questa chiave ha.
pub fn load_formato_visibile(connection: &Connection) -> Result<bool, AppError> {
    Ok(crate::settings::read_json::<bool>(connection, CHIAVE_FORMATO_VISIBILE)?.unwrap_or(true))
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
///
/// Pubblica perché la chiama anche il comando `equalizzatore`, che ha smesso di
/// scrivere e rileggere il database a ogni pixel di cursore e ha bisogno dello
/// stesso taglio senza passarci in mezzo. Averne due sarebbe averne due diverse.
pub fn normalizza(guadagni: &[f32]) -> Vec<f32> {
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
            "prova",
        )
        .expect("registrato");
        assert!(scritto);
        assert_eq!(conteggio(&c, 1), (1, Some(1_700_000_000_000), 1));
    }

    #[test]
    fn la_posizione_vale_solo_per_il_brano_di_cui_parla() {
        let c = db();
        assert_eq!(load_posizione(&c, 1).expect("letta"), 0);
        save_posizione(&c, 1, 95_000).expect("scritta");
        assert_eq!(load_posizione(&c, 1).expect("letta"), 95_000);
        // Il brano dopo, nei cinque secondi prima del segno nuovo.
        assert_eq!(load_posizione(&c, 2).expect("letta"), 0);
        // Il numero nudo delle versioni di prima vale per chi lo chiede.
        crate::settings::write_json(&c, CHIAVE_POSIZIONE, &29_000u64).expect("vecchio");
        assert_eq!(load_posizione(&c, 7).expect("letta"), 29_000);
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
            "prova",
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
                "prova",
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
            "prova",
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
    fn l_uscita_mai_scelta_e_quella_di_sistema() {
        let c = db();
        assert_eq!(load_uscita(&c).expect("riletta"), None);
    }

    #[test]
    fn l_uscita_scelta_torna_com_era() {
        let c = db();
        save_uscita(&c, Some("FiiO K11")).expect("scritta");
        assert_eq!(
            load_uscita(&c).expect("riletta"),
            Some("FiiO K11".to_owned())
        );
        // E si torna indietro: «predefinito di sistema» è una scelta, non
        // l'assenza di una scelta, e deve poter cancellare quella di prima.
        save_uscita(&c, None).expect("scritta");
        assert_eq!(load_uscita(&c).expect("riletta"), None);
    }

    #[test]
    fn un_uscita_illeggibile_non_impedisce_l_avvio() {
        // Il caso vero: un database ripristinato da un'altra macchina, o una
        // versione futura che scrive qui dentro qualcos'altro. Muti si resta
        // solo se non c'è nessuna scheda, mai per una preferenza storta.
        let c = db();
        crate::settings::write(&c, CHIAVE_USCITA, "{non è json").expect("scritta");
        assert_eq!(load_uscita(&c).expect("riletta"), None);
    }

    #[test]
    fn le_barre_dello_spettro_mai_scelte_sono_quelle_di_serie() {
        let c = db();
        assert_eq!(
            load_spettro_bande(&c).expect("riletta"),
            aether_play::RISOLUZIONE_DI_SERIE
        );
    }

    #[test]
    fn le_barre_dello_spettro_restano_scritte() {
        let c = db();
        save_spettro_bande(&c, 256).expect("scritta");
        assert_eq!(load_spettro_bande(&c).expect("riletta"), 256);
    }

    #[test]
    fn un_numero_di_barre_che_non_esiste_diventa_il_piu_vicino() {
        // Il caso vero: una preferenza scritta da una versione con un altro
        // elenco di risoluzioni. Deve valere adattabile, non illeggibile — la
        // stessa regola della curva dell'equalizzatore.
        let c = db();
        crate::settings::write(&c, CHIAVE_SPETTRO_BANDE, "100").expect("scritta");
        assert_eq!(load_spettro_bande(&c).expect("riletta"), 128);
        crate::settings::write(&c, CHIAVE_SPETTRO_BANDE, "3").expect("scritta");
        assert_eq!(load_spettro_bande(&c).expect("riletta"), 8);
        crate::settings::write(&c, CHIAVE_SPETTRO_BANDE, "60000").expect("scritta");
        assert_eq!(load_spettro_bande(&c).expect("riletta"), 1024);
    }

    #[test]
    fn barre_illeggibili_non_impediscono_l_avvio() {
        let c = db();
        crate::settings::write(&c, CHIAVE_SPETTRO_BANDE, "{non è json").expect("scritta");
        assert_eq!(
            load_spettro_bande(&c).expect("riletta"),
            aether_play::RISOLUZIONE_DI_SERIE
        );
    }

    #[test]
    fn uno_spettro_mai_scelto_e_spento() {
        // Chi apre «In riproduzione» per la prima volta è venuto a guardare la
        // copertina, e una scena WebGL accesa di serie è una ventola accesa di
        // serie.
        let c = db();
        assert!(!load_spettro_visibile(&c).expect("riletta"));
    }

    #[test]
    fn uno_spettro_acceso_resta_acceso() {
        // Il difetto vero che questa prova impedisce: un `unwrap_or(false)`
        // messo sul ramo sbagliato si mangia un `true` scritto davvero, e la
        // preferenza sembra non salvarsi mai — senza nessun errore, perché
        // «spento» è anche il valore di serie.
        let c = db();
        save_spettro_visibile(&c, true).expect("scritta");
        assert!(load_spettro_visibile(&c).expect("riletta"));
        save_spettro_visibile(&c, false).expect("scritta");
        assert!(!load_spettro_visibile(&c).expect("riletta"));
    }

    #[test]
    fn una_visibilita_illeggibile_lascia_la_scena_com_era() {
        // Un database ripristinato da un'altra macchina, o una versione futura
        // che scrive qui dentro qualcos'altro: si resta spenti, che è il caso
        // conservativo — accendere una GPU su un dato storto è il difetto.
        let c = db();
        crate::settings::write(&c, CHIAVE_SPETTRO_VISIBILE, "{non è json").expect("scritta");
        assert!(!load_spettro_visibile(&c).expect("riletta"));
    }

    #[test]
    fn il_formato_parte_acceso() {
        // L'opposto dello spettro, e la prova esiste per tenere l'asimmetria
        // deliberata invece che accidentale: due chiavi vicine con due valori
        // di serie diversi sono esattamente il posto in cui un
        // copia-e-incolla mette `unwrap_or(false)` anche qui.
        let c = db();
        assert!(load_formato_visibile(&c).expect("riletta"));
    }

    #[test]
    fn il_formato_scelto_si_rilegge() {
        // Spento **e** riacceso: con il valore di serie acceso, una prova che
        // scrive solo `true` passerebbe anche se la scrittura non funzionasse
        // affatto. Lo spegnimento è l'unico che distingue le due cose, e la
        // riaccensione verifica che non sia un viaggio senza ritorno.
        let c = db();
        save_formato_visibile(&c, false).expect("scritta");
        assert!(!load_formato_visibile(&c).expect("riletta"));
        save_formato_visibile(&c, true).expect("scritta");
        assert!(load_formato_visibile(&c).expect("riletta"));
    }

    #[test]
    fn un_formato_illeggibile_resta_visibile() {
        // Un valore che non si interpreta vale come assente, e assente qui è
        // acceso: è la regola del modulo, e il caso conservativo è l'opposto di
        // quello dello spettro perché il costo è una riga di testo, non una GPU.
        let c = db();
        crate::settings::write(&c, CHIAVE_FORMATO_VISIBILE, "{non è json").expect("scritta");
        assert!(load_formato_visibile(&c).expect("riletta"));
    }

    #[test]
    fn una_qualita_mai_scelta_e_automatica() {
        let c = db();
        assert_eq!(load_spettro_qualita(&c).expect("riletta"), Qualita::Auto);
    }

    #[test]
    fn una_qualita_che_non_esiste_diventa_automatica() {
        // È il ritaglio che non abbiamo scritto: il `rename_all` fa fallire la
        // deserializzazione di `"ultra"`, `read_json` tratta il malformato come
        // assente, e assente è «lo decide la scena». Se qualcuno togliesse il
        // `rename_all`, o accettasse la stringa grezza, questa prova cade.
        let c = db();
        crate::settings::write(&c, CHIAVE_SPETTRO_QUALITA, r#""ultra""#).expect("scritta");
        assert_eq!(load_spettro_qualita(&c).expect("riletta"), Qualita::Auto);
        crate::settings::write(&c, CHIAVE_SPETTRO_QUALITA, "{non è json").expect("scritta");
        assert_eq!(load_spettro_qualita(&c).expect("riletta"), Qualita::Auto);
        // E il nome giusto con la maiuscola sbagliata è un nome sbagliato.
        crate::settings::write(&c, CHIAVE_SPETTRO_QUALITA, r#""Alta""#).expect("scritta");
        assert_eq!(load_spettro_qualita(&c).expect("riletta"), Qualita::Auto);
    }

    #[test]
    fn i_tre_livelli_fanno_andata_e_ritorno() {
        let c = db();
        for livello in [Qualita::Auto, Qualita::Alta, Qualita::Bassa] {
            save_spettro_qualita(&c, livello).expect("scritta");
            assert_eq!(load_spettro_qualita(&c).expect("riletta"), livello);
        }
    }

    #[test]
    fn il_nome_sul_filo_e_il_nome_nel_database() {
        // Due elenchi di nomi per la stessa enumerazione: quello di serde, che
        // finisce nel database, e quello di `da_nome`, che arriva dalla
        // finestra. Il giorno in cui divergono, la preferenza di ieri diventa
        // illeggibile oggi — e in silenzio, perché il ripiego è `Auto`.
        for livello in [Qualita::Auto, Qualita::Alta, Qualita::Bassa] {
            let scritto = serde_json::to_string(&livello).expect("serializzata");
            let nome = scritto.trim_matches('"');
            assert_eq!(
                Qualita::da_nome(nome),
                livello,
                "serde scrive «{nome}» e `da_nome` non lo riconosce"
            );
        }
        assert_eq!(Qualita::da_nome("ultra"), Qualita::Auto);
        assert_eq!(Qualita::da_nome(""), Qualita::Auto);
    }

    #[test]
    fn una_dissolvenza_mai_scelta_e_spenta() {
        let c = db();
        assert_eq!(load_crossfade(&c).expect("riletta"), 0);
    }

    #[test]
    fn la_dissolvenza_resta_scritta() {
        let c = db();
        save_crossfade(&c, 8).expect("scritta");
        assert_eq!(load_crossfade(&c).expect("riletta"), 8);
    }

    #[test]
    fn una_dissolvenza_piu_lunga_del_massimo_si_taglia() {
        let c = db();
        save_crossfade(&c, 60).expect("scritta");
        assert_eq!(load_crossfade(&c).expect("riletta"), CROSSFADE_MASSIMO_S);
    }

    #[test]
    fn una_dissolvenza_scritta_a_mano_fuori_scala_si_taglia_in_lettura() {
        let c = db();
        crate::settings::write(&c, CHIAVE_CROSSFADE, "600").expect("scritta");
        assert_eq!(load_crossfade(&c).expect("riletta"), CROSSFADE_MASSIMO_S);
    }

    #[test]
    fn una_dissolvenza_illeggibile_non_impedisce_l_avvio() {
        let c = db();
        crate::settings::write(&c, CHIAVE_CROSSFADE, "{non è json").expect("scritta");
        assert_eq!(load_crossfade(&c).expect("riletta"), 0);
    }

    #[test]
    fn una_latenza_mai_scelta_e_zero() {
        let c = db();
        assert_eq!(load_latenza(&c).expect("riletta"), 0);
    }

    #[test]
    fn la_latenza_resta_scritta_nei_due_versi() {
        let c = db();
        save_latenza(&c, 120).expect("scritta");
        assert_eq!(load_latenza(&c).expect("riletta"), 120);
        // Il verso negativo non è un caso limite: è la correzione di chi trova
        // che i testi arrivino tardi comunque.
        save_latenza(&c, -80).expect("scritta");
        assert_eq!(load_latenza(&c).expect("riletta"), -80);
    }

    #[test]
    fn una_latenza_fuori_scala_si_taglia_dai_due_lati() {
        let c = db();
        save_latenza(&c, 5_000).expect("scritta");
        assert_eq!(load_latenza(&c).expect("riletta"), LATENZA_MASSIMA_MS);
        save_latenza(&c, -5_000).expect("scritta");
        assert_eq!(load_latenza(&c).expect("riletta"), -LATENZA_MASSIMA_MS);
    }

    #[test]
    fn una_latenza_scritta_a_mano_fuori_scala_si_taglia_in_lettura() {
        // Questa riga può arrivare da un backup o da un database ritoccato a
        // mano: un'ora di compensazione non deve poter entrare nel motore.
        let c = db();
        crate::settings::write(&c, CHIAVE_LATENZA, "3600000").expect("scritta");
        assert_eq!(load_latenza(&c).expect("riletta"), LATENZA_MASSIMA_MS);
    }

    #[test]
    fn una_latenza_illeggibile_non_impedisce_l_avvio() {
        let c = db();
        crate::settings::write(&c, CHIAVE_LATENZA, "{non è json").expect("scritta");
        assert_eq!(load_latenza(&c).expect("riletta"), 0);
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

    /// Un `MusicFiles` che non apre niente e fallisce sempre allo stesso modo.
    ///
    /// Serve a provare la **traduzione** del guasto, che è tutta la decisione
    /// presa qui: con `LocalFiles` non c'è modo di far arrivare un errore di
    /// rete senza una share vera.
    struct FintiFile(ErrorCode);

    impl MusicFiles for FintiFile {
        fn walk(&self, _root: &str) -> Result<crate::files::Camminata, AppError> {
            // Vuota e intera: questo doppio non ha un disco da perdere pezzi.
            Ok(crate::files::Camminata {
                file: Vec::new(),
                completa: true,
            })
        }

        fn open(
            &self,
            _path: &str,
        ) -> Result<Box<dyn crate::files::ReadSeek + Send + Sync>, AppError> {
            Err(AppError::new(self.0.clone()))
        }
    }

    #[test]
    fn la_rete_giu_attraversa_intera_e_resta_ritentabile() {
        // Il punto: `playback.sourceUnavailable` non è mai ritentabile, quindi
        // impacchettarci dentro una share caduta vorrebbe dire dire a chi ha
        // staccato il cavo che il brano è perso. Deve passare com'è.
        let c = db();
        brano(&c, 1, 1000);
        let files = FintiFile(ErrorCode::FsNetworkUnavailable {
            path: Some("//srv/musica/1.mp3".to_owned()),
        });
        let err = sorgente(&c, &files, 1).expect_err("deve fallire");
        assert_eq!(err.code().kind(), ErrorCodeKind::FsNetworkUnavailable);
        assert!(
            err.code().is_retryable(),
            "la finestra deve offrire «Riprova»"
        );
    }

    #[test]
    fn l_apertura_da_scheda_non_tocca_il_database_e_propaga_la_rete_giu() {
        // La metà lenta, provata da sola: è quella che chi sta sopra manda su un
        // filo con la scadenza addosso, senza nessun lucchetto in mano.
        let files = FintiFile(ErrorCode::FsNetworkUnavailable {
            path: Some("//srv/musica/1.mp3".to_owned()),
        });
        let scheda = SchedaSorgente {
            track_id: 1,
            path: "//srv/musica/1.mp3".to_owned(),
            durata_ms: 1_000,
            replaygain_db: None,
        };
        let err = sorgente_da_scheda(&files, &scheda).expect_err("deve fallire");
        assert_eq!(err.code().kind(), ErrorCodeKind::FsNetworkUnavailable);
        assert!(err.code().is_retryable());
    }

    #[test]
    fn ogni_altro_guasto_di_apertura_resta_sorgente_non_disponibile() {
        let c = db();
        brano(&c, 1, 1000);
        let files = FintiFile(ErrorCode::FsNotFound {
            path: "C:/m/1.mp3".to_owned(),
        });
        let err = sorgente(&c, &files, 1).expect_err("deve fallire");
        assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackSourceUnavailable);
    }
}
