//! Quel che la riproduzione lascia sul database.
//!
//! Il motore in `rymembra-play` non sa cos'è una libreria: riceve dei byte e li fa
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

use rusqlite::Connection;
use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_domain::esterno::Fonte;
use rymembra_domain::listen::Listen;
use rymembra_domain::queue::{QueueSnapshot, RepeatMode};
use rymembra_play::{BANDE, LIMITE_DB, Segmento, Sorgente};

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
    /// Da dove escono i byte: un file sul disco, un pezzo d'immagine, o un
    /// catalogo.
    pub collocazione: Collocazione,
    /// La durata secondo il database, per quando il file non la dichiara. Per
    /// un brano d'immagine è quella del brano, non del disco.
    pub durata_ms: i64,
    /// Il guadagno che porta il brano al riferimento di −18 LUFS, in dB.
    ///
    /// Dal tag ReplayGain quando c'è, altrimenti dalla misura della passata
    /// d'analisi ([`crate::intensita`]); `None` se non si sa né l'uno né
    /// l'altro. Il nome è quello di sempre perché il motore lo tratta allo
    /// stesso modo da qualunque parte arrivi: è lo stesso numero, riferito
    /// allo stesso −18.
    pub replaygain_db: Option<f64>,
    /// Il picco vero, lineare, dalla stessa misura. Serve a non alzare un
    /// brano oltre il fondo scala: vedi `rymembra_play::correzione`.
    pub picco: Option<f64>,
}

/// Da dove escono i byte di un brano: un file intero, un pezzo di un file, o
/// un catalogo.
///
/// # Perché è un enum e non un percorso con qualche colonna accanto
///
/// Perché «questo brano è un file?» è la domanda che viene **prima** di
/// qualunque cosa si faccia con un brano, e un percorso che a volte è vuoto
/// lascia rispondere per distrazione. Le funzioni che calcolano l'impronta, che
/// agganciano i testi, che leggono e riscrivono i tag aprono tutte un percorso:
/// con un enum il compilatore le obbliga a dire cosa fanno dell'altro caso, e
/// «lo salto» diventa una riga scritta apposta invece di un caso non
/// considerato. Un caso non considerato, qui, sarebbe un indirizzo passato a
/// `File::open`.
///
/// È la stessa cosa che dice il `CHECK` della migrazione 26, e sta in due posti
/// di proposito: quel che il database rifiuta di contenere è anche quel che il
/// Rust non sa rappresentare.
///
/// # Perché le possibilità erano due e adesso sono tre
///
/// Per la stessa ragione per cui l'enum esiste. Un brano d'immagine CD — una
/// traccia di un FLAC o di un APE intero, descritta da un `.cue` — ha un
/// percorso in tabella, `…\Album.cue|03`, ma quel percorso **non è un file**:
/// non si apre, e accanto a lui non si posa un `.lrc` né si riscrivono tag. Il
/// file vero è l'immagine, che è di tutti i brani del disco insieme. Lasciarlo
/// dentro [`Collocazione::File`] avrebbe voluto dire che ogni funzione che apre
/// un percorso apriva quello sbagliato, in silenzio; una terza variante
/// obbliga di nuovo ogni chiamante a decidere — e le due domande si separano:
/// [`Collocazione::percorso`] per chi lavora **accanto** al file del brano,
/// [`Collocazione::file_audio`] per chi ne **apre** l'audio.
///
/// Il `CHECK` della 26 resta vero: una riga d'immagine ha il suo `path`, ed è
/// un brano sul disco come gli altri.
///
/// # Da non confondere con [`crate::provenienza`]
///
/// Quel modulo risponde a «da dove viene questo **titolo**» — dai tag, dal nome
/// del file, da una correzione a mano. Questo tipo risponde a «da dove vengono
/// questi **byte**». Due domande diverse sullo stesso brano, e il nome le tiene
/// separate apposta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Collocazione {
    /// Un file sul disco, col percorso così com'è scritto in tabella — mai
    /// normalizzato, perché serve ad aprirlo.
    File(String),
    /// Un brano che sta in un catalogo, e su questo disco non c'è.
    Catalogo {
        /// Quale catalogo. Decide con quale rete si va a prendere i byte, cioè
        /// con quali scadenze, quale `User-Agent` e quale riserva di
        /// connessioni.
        fonte: Fonte,
        /// L'indirizzo dei byte, così com'è in tabella. Per Audius è un
        /// percorso senza nodo davanti, e il perché sta nella migrazione 26.
        url: String,
    },
    /// Un brano d'immagine CD: il file vero e il pezzo.
    Segmento {
        /// Il percorso dell'immagine, così com'è nella colonna `immagine` —
        /// mai normalizzato, perché serve ad aprirla, e **identico** per tutti
        /// i brani dello stesso disco: è anche quel che il motore confronta
        /// per decidere se due brani sono contigui.
        immagine: String,
        /// Dove comincia il brano, in fotogrammi CD (1/75 s) dall'inizio
        /// dell'immagine.
        inizio_cd: u32,
        /// Dove finisce, escluso. `None` per l'ultimo brano, che va fino in
        /// fondo all'immagine.
        fine_cd: Option<u32>,
    },
}

impl Collocazione {
    /// Il percorso sul disco, se il brano è un file **suo**.
    ///
    /// L'unico modo di arrivare a quella stringa, e la forma è voluta: chi la
    /// chiama ha in mano un `Option` e deve deciderne il `None`.
    ///
    /// `None` anche per un brano d'immagine, ed è la risposta giusta per chi
    /// chiede questo: chi lavora **accanto** al file — un `.lrc` da posare, dei
    /// tag da riscrivere, un file da cancellare — un brano d'immagine lo tratta
    /// come un brano senza file. Il suo file è di tutto il disco, e un testo o
    /// un titolo scritti lì sarebbero di tutti i brani insieme. Chi deve
    /// **aprire** l'audio chiede [`Self::file_audio`].
    #[must_use]
    pub fn percorso(&self) -> Option<&str> {
        match self {
            Self::File(percorso) => Some(percorso),
            Self::Catalogo { .. } | Self::Segmento { .. } => None,
        }
    }

    /// Il file da cui escono i byte, se ce n'è uno sul disco: il brano stesso,
    /// o l'immagine di cui è un pezzo.
    ///
    /// Solo per chi **apre** l'audio — la riproduzione, le misure dell'analisi
    /// — che di un brano d'immagine legge il pezzo suo e nient'altro. Per tutto
    /// il resto vale [`Self::percorso`].
    #[must_use]
    pub fn file_audio(&self) -> Option<&str> {
        match self {
            Self::File(percorso) => Some(percorso),
            Self::Segmento { immagine, .. } => Some(immagine),
            Self::Catalogo { .. } => None,
        }
    }

    /// Da quale catalogo arriva, se non è sul disco.
    #[must_use]
    pub const fn fonte(&self) -> Option<Fonte> {
        match self {
            Self::File(_) | Self::Segmento { .. } => None,
            Self::Catalogo { fonte, .. } => Some(*fonte),
        }
    }
}

/// La collocazione di una riga che sta sul disco: il file intero, o il pezzo
/// d'immagine se la riga ne ha una.
///
/// Le tre colonne dell'immagine come stanno in tabella. `None` quando la riga
/// dice di essere un brano d'immagine ma non dice dove il brano comincia, o
/// dice dei confini che non stanno in un CD — un inizio negativo, una fine che
/// non viene dopo l'inizio. Non succede con le righe che scrive la scansione,
/// e chi riceve `None` lo tratta come una riga che non sa leggere: non si
/// suona, né si misura, un pezzo a caso del disco.
pub(crate) fn collocazione_su_disco(
    percorso: &str,
    immagine: Option<String>,
    inizio_cd: Option<i64>,
    fine_cd: Option<i64>,
) -> Option<Collocazione> {
    let Some(immagine) = immagine else {
        return Some(Collocazione::File(percorso.to_owned()));
    };
    let inizio_cd = u32::try_from(inizio_cd?).ok()?;
    let fine_cd = match fine_cd {
        None => None,
        Some(fine) => Some(u32::try_from(fine).ok().filter(|fine| *fine > inizio_cd)?),
    };
    Some(Collocazione::Segmento {
        immagine,
        inizio_cd,
        fine_cd,
    })
}

/// La riga di [`scheda_sorgente`]: percorso, servizio, indirizzo, durata,
/// guadagno del tag, intensità e picco misurati, e le tre colonne
/// dell'immagine — il file, l'inizio e la fine del brano.
type RigaDaSuonare = (
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

/// La sola query: cosa dice il database di questo brano.
///
/// Per un brano d'immagine la durata è quella **del brano**, non del disco: è
/// quel che decide la dissolvenza, e la colonna la porta già così.
///
/// # Errori
///
/// `playback.sourceUnavailable` se la riga non c'è più, `db.queryFailed` se il
/// database non risponde.
pub fn scheda_sorgente(connection: &Connection, track_id: i64) -> Result<SchedaSorgente, AppError> {
    let riga: RigaDaSuonare = connection
        .query_row(
            "SELECT t.path, t.source_service, t.fonte_url, t.duration_ms,
                    t.replaygain_track_db, m.lufs, m.picco,
                    t.immagine, t.inizio_cd, t.fine_cd
               FROM tracks t
               LEFT JOIN track_intensita m
                      ON m.track_id = t.id AND m.versione = ?2
              WHERE t.id = ?1",
            rusqlite::params![track_id, i64::from(rymembra_play::intensita::VERSIONE)],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
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
    let (path, servizio, fonte_url, durata_ms, dal_tag, lufs, picco, immagine, inizio_cd, fine_cd) =
        riga;

    // Il `CHECK` della migrazione 26 garantisce che una delle due ci sia, e che
    // non ci siano tutte e due. Qui si legge comunque come se non lo
    // garantisse, perché questo è il punto in cui un database toccato a mano —
    // o scritto da una versione futura che avesse allentato quel vincolo —
    // entra nel programma. La risposta a «non so cosa sia questa riga» è
    // saltare il brano, non suonare qualcosa a caso.
    let collocazione = match (path, servizio.zip(fonte_url)) {
        (Some(percorso), None) => {
            // Lo stesso vale per un brano d'immagine che non dice dove
            // comincia: suonarlo dall'inizio del file vorrebbe dire suonare
            // il primo brano del disco al posto del suo.
            let Some(collocazione) = collocazione_su_disco(&percorso, immagine, inizio_cd, fine_cd)
            else {
                return Err(AppError::new(ErrorCode::PlaybackSourceUnavailable {
                    track_id: Some(track_id),
                    path: Some(percorso),
                })
                .with_cause(
                    "la riga d'immagine non dice dove il brano comincia e finisce".to_owned(),
                ));
            };
            collocazione
        }
        // Un catalogo con un'immagine accanto è una riga che dice due cose:
        // cade nell'ultimo ramo, come quella che dice file e catalogo insieme.
        (None, Some((servizio, url))) if immagine.is_none() => Collocazione::Catalogo {
            fonte: Fonte::da_testo(&servizio),
            url,
        },
        (percorso, _) => {
            return Err(AppError::new(ErrorCode::PlaybackSourceUnavailable {
                track_id: Some(track_id),
                path: percorso,
            })
            .with_cause(
                "la riga non dice né un file né un catalogo, o li dice tutti e due".to_owned(),
            ));
        }
    };

    // Il tag prima della misura: l'ha scritto qualcuno apposta. La misura
    // copre tutto il resto, che nelle librerie vere è quasi tutto.
    let replaygain_db = dal_tag.or_else(|| lufs.map(guadagno_misurato));
    Ok(SchedaSorgente {
        track_id,
        collocazione,
        durata_ms,
        replaygain_db,
        picco,
    })
}

/// Il guadagno che porta al riferimento un brano misurato a `lufs`.
///
/// `rymembra_play::intensita::guadagno_db` in `f64`, perché è quel che il
/// database e la scheda portano.
fn guadagno_misurato(lufs: f64) -> f64 {
    rymembra_play::intensita::RIFERIMENTO_LUFS - lufs
}

/// Come si sceglie il livello di un brano: il suo, o quello del suo disco.
///
/// Il livello del brano porta ogni brano allo stesso volume, ed è quel che si
/// vuole in una scaletta mescolata. Quello del disco sposta tutti i brani di
/// un album della stessa quantità, e tiene la distanza che chi l'ha
/// masterizzato ha voluto fra un'introduzione in punta di piedi e il pezzo
/// forte che la segue: normalizzati uno per uno, i due arriverebbero uguali.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModoLivello {
    /// Sempre quello del brano.
    Brano,
    /// Sempre quello del disco, quando il brano ne ha uno.
    Album,
    /// Quello del disco quando si sta ascoltando un disco — nessuno shuffle, e
    /// accanto nella coda un brano dello stesso album — altrimenti quello del
    /// brano. Di serie: è quel che fanno foobar2000 e gli altri lettori che
    /// lo offrono, ed è la sola scelta che non chiede di ricordarsi di
    /// cambiare impostazione passando da un disco a una scaletta.
    #[default]
    Automatico,
}

/// Quel che c'è intorno al brano da aprire, nella coda.
///
/// Serve a [`ModoLivello::Automatico`] per capire se si sta ascoltando un
/// disco. Lo compila chi ha in mano la coda; qui si leggono solo gli
/// identificativi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Intorno {
    /// Lo shuffle è acceso: i vicini sono un caso, non un disco.
    pub mescolato: bool,
    /// Il brano che suona subito prima, nell'ordine d'ascolto.
    pub prima: Option<i64>,
    /// Il brano che suona subito dopo.
    pub dopo: Option<i64>,
}

/// La scheda di un brano **da suonare**: come [`scheda_sorgente`], con il
/// livello scelto secondo la normalizzazione conservata e secondo la coda.
///
/// # Errori
///
/// Gli stessi di [`scheda_sorgente`].
pub fn scheda_da_suonare(
    connection: &Connection,
    track_id: i64,
    intorno: Intorno,
) -> Result<SchedaSorgente, AppError> {
    let mut scheda = scheda_sorgente(connection, track_id)?;
    let modo = load_replaygain(connection)?.modo;
    let del_disco = match modo {
        ModoLivello::Brano => false,
        ModoLivello::Album => true,
        ModoLivello::Automatico => {
            !intorno.mescolato && stesso_disco(connection, track_id, intorno)?
        }
    };
    if del_disco && let Some((guadagno, picco)) = livello_del_disco(connection, track_id)? {
        scheda.replaygain_db = Some(guadagno);
        scheda.picco = picco.or(scheda.picco);
    }
    Ok(scheda)
}

/// Uno dei vicini nella coda è dello stesso disco?
///
/// Un disco vuol dire un titolo d'album: i file sciolti di una cartella hanno
/// la stessa `album_key` senza essere un album.
fn stesso_disco(
    connection: &Connection,
    track_id: i64,
    intorno: Intorno,
) -> Result<bool, AppError> {
    let vicini = [intorno.prima, intorno.dopo];
    for vicino in vicini.into_iter().flatten() {
        if vicino == track_id {
            continue;
        }
        let uguale: bool = connection
            .query_row(
                "SELECT EXISTS (
                   SELECT 1 FROM tracks a JOIN tracks b ON b.album_key = a.album_key
                    WHERE a.id = ?1 AND b.id = ?2
                      AND a.album IS NOT NULL AND TRIM(a.album) <> '')",
                rusqlite::params![track_id, vicino],
                |row| row.get(0),
            )
            .map_err(|err| db_error("confronto dei dischi nella coda", &err))?;
        if uguale {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Il livello del disco di un brano: guadagno in dB e picco lineare.
///
/// Il tag d'album quando c'è — calcolato da chi l'ha scritto sull'album
/// suonato di fila, che è la misura giusta — altrimenti la media dei brani
/// misurati di [`crate::intensita::dell_album_di`]. Il picco viene sempre
/// dalla misura: è l'unico posto dove c'è. `None` se del disco non si sa
/// niente, e allora resta il livello del brano.
fn livello_del_disco(
    connection: &Connection,
    track_id: i64,
) -> Result<Option<(f64, Option<f64>)>, AppError> {
    let dal_tag: Option<f64> = connection
        .query_row(
            "SELECT replaygain_album_db FROM tracks WHERE id = ?1",
            [track_id],
            |row| row.get(0),
        )
        .map_err(|err| db_error("lettura del guadagno d'album", &err))?;
    let misurato = crate::intensita::dell_album_di(connection, track_id)?;
    let picco = misurato.map(|m| m.picco);
    Ok(match (dal_tag, misurato) {
        (Some(db), _) => Some((db, picco)),
        (None, Some(m)) => Some((guadagno_misurato(m.lufs), picco)),
        (None, None) => None,
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
/// il file non si apre, **compreso** un brano di catalogo.
///
/// # Perché un brano di catalogo qui è un errore e non un ramo
///
/// Perché questo crate non vede la rete, e non è una mancanza ma il confine su
/// cui è costruito l'albero: il posto in cui un flusso HTTP diventa qualcosa da
/// suonare è `apps/desktop/src-tauri/src/riproduzione/flusso.rs`, l'unico che
/// conosce tutti e due i lati. Chi chiama di qui — l'impronta sonora,
/// l'aggancio dei testi — un flusso non lo vuole comunque: non si misura un
/// brano che non si ha, e scaricarlo per misurarlo sarebbe esattamente quel che
/// `Disponibilita::SoloAscolto` vieta.
pub fn sorgente_da_scheda(
    files: &dyn MusicFiles,
    scheda: &SchedaSorgente,
) -> Result<Sorgente, AppError> {
    // `file_audio` e non `percorso`: di un brano d'immagine si apre l'immagine,
    // e il pezzo da suonare viaggia accanto ai byte, nel segmento.
    let Some(percorso) = scheda.collocazione.file_audio() else {
        return Err(AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: Some(scheda.track_id),
            // Nessun percorso, e non per reticenza: la finestra mostra quel
            // campo per dire quale cartella andare a ricollegare, e un brano di
            // catalogo una cartella non ce l'ha.
            path: None,
        })
        .with_cause("un brano di catalogo non ha un file da aprire".to_owned()));
    };

    let media = files.open(percorso).map_err(|err| {
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
            path: Some(percorso.to_owned()),
        })
        .with_cause(err.cause().unwrap_or(err.code().kind().code()).to_owned())
    })?;

    // Il pezzo, per un brano d'immagine. La stringa dell'immagine passa così
    // com'è nella colonna: è uguale per tutti i brani dello stesso disco, ed è
    // su quell'uguaglianza che il motore riconosce due brani contigui.
    let segmento = match &scheda.collocazione {
        Collocazione::Segmento {
            immagine,
            inizio_cd,
            fine_cd,
        } => Some(Segmento {
            immagine: immagine.clone(),
            inizio_cd: *inizio_cd,
            fine_cd: *fine_cd,
        }),
        Collocazione::File(_) | Collocazione::Catalogo { .. } => None,
    };

    Ok(Sorgente {
        track_id: scheda.track_id,
        media: Box::new(Adattatore::nuovo(media)),
        // Dall'immagine, per un brano d'immagine: `…\Album.cue|03` non ha
        // un'estensione che dica come sono fatti i byte.
        estensione: estensione_di(percorso),
        durata_ms: u64::try_from(scheda.durata_ms).unwrap_or(0),
        // `f64` nel database perché SQLite non ha i float a 32 bit; il motore
        // lavora in `f32`, che per dei decibel è largamente sufficiente — la
        // correzione viene poi tagliata fra −24 e +12 dB comunque.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "sono decibel: la precisione di un f32 è un milionesimo di dB"
        )]
        replaygain_db: scheda.replaygain_db.map(|db| db as f32),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "un picco lineare: la precisione di un f32 è più che abbondante"
        )]
        picco: scheda.picco.map(|p| p as f32),
        segmento,
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
/// La sorella su indirizzi è `rymembra_domain::indirizzo::estensione_da_url`,
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
/// `rymembra_domain::listen::counts_as_play`, e questa funzione la rispetta invece
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
///
/// # Errori
///
/// `library.trackNotFound` per un identificativo ≤ 0, senza aprire nessuna
/// transazione: vedi [`riga_della_libreria`].
pub fn record_play(
    connection: &mut Connection,
    ascolto: &Listen,
    dispositivo: &str,
) -> Result<bool, AppError> {
    // Prima di tutto, anche prima di `counts`: l'invariante è «mai un id ≤ 0
    // sul database», e una guardia che dipende da un'altra condizione è una
    // guardia che un giorno qualcuno sposta senza accorgersene.
    riga_della_libreria(ascolto.track_id)?;
    if !ascolto.counts {
        return Ok(false);
    }
    let ms_played = i64::try_from(ascolto.listened_ms).unwrap_or(i64::MAX);
    let tx = crate::library::transazione_di_scrittura(connection)
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

/// Il brano è una riga della libreria di questo computer?
///
/// # La regola, e da dove viene
///
/// **id > 0 è una riga di `tracks`, id ≤ 0 non lo è mai.** Gli identificativi
/// che SQLite assegna partono da 1, e da «Ascolta insieme» in poi i numeri
/// negativi sono le voci di una sessione: maniglie che esistono solo in
/// memoria, e che su un altro computer — o su questo, a sessione finita — non
/// nominano niente. Lo zero non è di nessuno dei due, e si rifiuta con loro.
///
/// Le tre scritture della riproduzione — [`record_play`], [`save_posizione`] e
/// [`save_queue`] — passano di qui **prima** di toccare il database. È l'ultima
/// difesa e non la prima: chi sta in una sessione non dovrebbe nemmeno
/// chiamarle. Se un giorno un comando nuovo salta quella strada, il danno si
/// ferma qui invece di arrivare a una cronologia o a una coda salvata.
///
/// # Perché un errore e non un `Ok` muto
///
/// Perché un rifiuto qui vuol dire che qualcuno, più su, ha sbagliato strada, e
/// la finestra lo deve poter scrivere nel diario. È sicuro farlo: nessuno dei
/// chiamanti ripete la scrittura su un `Err` — la coda abbassa la sua bandiera e
/// annota una riga, l'ascolto annota una riga e non fa scrobble, la posizione
/// scarta l'esito — quindi un rifiuto costa al più una riga per gesto, non un
/// ciclo. La causa porta il numero e basta: un identificativo non dice niente
/// di chi ascolta.
///
/// # Chi altro passa di qui
///
/// `pub(crate)` e non copiata: l'audit delle guardie della 3.1.0 l'ha messa in
/// testa a ogni altra strada di questo crate in cui un id ≤ 0 farebbe nascere
/// una riga o entrerebbe in una coda — l'aggiunta a una playlist
/// (`playlists::accoda`), le scritture dei testi che ricevono un
/// `BranoDaTestare` già in mano, l'autoplay e la radio seminati da un brano —
/// e nelle due letture di questo modulo che darebbero a una voce qualcosa che
/// non è suo: il segno di posizione di prima ([`load_posizione`]) e una coda
/// sul disco che non dovrebbe esserci ([`load_queue`]). Una regola sola detta
/// in un posto solo: una seconda copia sarebbe la prima a dimenticarsi dello
/// zero.
///
/// # Errori
///
/// `library.trackNotFound` se `track_id` ≤ 0.
pub(crate) fn riga_della_libreria(track_id: i64) -> Result<(), AppError> {
    if track_id > 0 {
        return Ok(());
    }
    Err(AppError::new(ErrorCode::LibraryTrackNotFound {
        track_id: Some(track_id),
    })
    .with_cause(format!(
        "id {track_id}: non è una riga della libreria, non si scrive"
    )))
}

/// Conserva la coda perché sopravviva alla chiusura.
///
/// # Una voce che non è della libreria, e la coda non si scrive affatto
///
/// Basta **un** identificativo ≤ 0, in qualunque punto, e la scrittura si
/// rifiuta tutta: sul database resta l'ultima coda fatta solo di righe della
/// libreria. Le due alternative avevano ciascuna un buco.
///
/// - **Rifiutare solo se il corrente è ≤ 0, e filtrare il resto.** Una coda di
///   sessione è fatta tutta di maniglie, e può non avere un corrente — finita,
///   o appena svuotata del brano che suonava. Lì il corrente è `None`, la
///   guardia non scatta, il filtro toglie tutto, e sul disco finisce una coda
///   **vuota** al posto di quella dell'utente: chiusa l'applicazione a metà
///   sessione, la sua coda di prima non c'è più.
/// - **Filtrare.** Togliere delle voci vuol dire rinumerare `order` e
///   spostare `order_pos`: codice nuovo che riscrive una permutazione, con
///   `Queue::restore` a scoprirne gli errori solo al prossimo avvio. E quel che
///   resta è una coda che l'utente non ha mai avuto davanti.
///
/// Rifiutare tutto non inventa niente: o si scrive la coda com'è, o resta
/// quella di prima. Una coda mista — righe e maniglie insieme — secondo il
/// piano non esiste; se esistesse, sarebbe un errore di chi l'ha costruita, e
/// il diario lo dice. Vedi [`riga_della_libreria`].
///
/// # Errori
///
/// `library.trackNotFound` se una voce ha id ≤ 0, senza scrivere niente;
/// `db.queryFailed` se il database non risponde.
pub fn save_queue(connection: &Connection, coda: &QueueSnapshot) -> Result<(), AppError> {
    for id in &coda.tracks {
        riga_della_libreria(*id)?;
    }
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
///
/// Una coda con un id ≤ 0 è illeggibile come una storta, ed è lo specchio di
/// [`save_queue`], che non la scrive mai: se c'è, l'ha messa qualcun altro, e
/// all'avvio quelle voci finirebbero nella coda del lettore come voci di
/// «Ascolta insieme» di una sessione che non c'è. Toglierle una per una non
/// si può — `order` e `order_pos` sono indici, e cambierebbero significato —
/// quindi vale la regola di tutta la funzione: coda vuota.
pub fn load_queue(connection: &Connection) -> Result<QueueSnapshot, AppError> {
    let Some(json) = crate::settings::read(connection, CHIAVE_CODA)? else {
        return Ok(QueueSnapshot::default());
    };
    let Ok(su_disco) = serde_json::from_str::<CodaSuDisco>(&json) else {
        return Ok(QueueSnapshot::default());
    };
    if su_disco
        .tracks
        .iter()
        .any(|id| riga_della_libreria(*id).is_err())
    {
        return Ok(QueueSnapshot::default());
    }
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

/// Volume e silenziamento come stanno **nella riga**, con la curva con cui il
/// numero va letto.
///
/// # Perché la curva sta scritta accanto al numero
///
/// Fino alla 2.4 il numero era un'ampiezza: 0,5 voleva dire metà ampiezza,
/// cioè −6 dB, e metà corsa del cursore suonava quasi come il massimo. Dalla
/// 3.0 il numero è la **posizione del cursore**, e il motore la porta al cubo
/// (`rymembra_play::volume_uscita`): metà corsa è −18 dB, che è quel che
/// l'orecchio si aspetta da «a metà». Lo stesso 0,8 letto con la curva nuova
/// suonerebbe sei decibel sotto quel che si sentiva ieri — un aggiornamento
/// che abbassa il volume a tutti senza dirlo. Una riga senza `curva` è della
/// curva vecchia, e si converte leggendola nella posizione che suona uguale:
/// la radice cubica dell'ampiezza.
#[derive(serde::Serialize, serde::Deserialize)]
struct VolumeSuDisco {
    volume: f32,
    muto: bool,
    /// 0 (assente) = ampiezza, come fino alla 2.4; [`CURVA_PERCEPITA`] =
    /// posizione del cursore.
    #[serde(default)]
    curva: u8,
}

/// La curva della 3.0: il numero è la posizione del cursore.
const CURVA_PERCEPITA: u8 = 1;

/// Conserva volume e silenziamento.
pub fn save_volume(connection: &Connection, volume: Volume) -> Result<(), AppError> {
    let su_disco = VolumeSuDisco {
        volume: volume.volume,
        muto: volume.muto,
        curva: CURVA_PERCEPITA,
    };
    let json = serde_json::to_string(&su_disco).unwrap_or_else(|_| "{}".to_owned());
    crate::settings::write(connection, CHIAVE_VOLUME, &json)
}

/// Rilegge volume e silenziamento.
///
/// Una riga scritta prima della 3.0 si converte alla curva nuova in lettura, e
/// non si riscrive: la conversione è la stessa ogni volta, e la prima
/// [`save_volume`] la fissa. Così anche un profilo esportato da una 2.4 e
/// importato qui arriva al volume a cui suonava.
pub fn load_volume(connection: &Connection) -> Result<Volume, AppError> {
    let Some(json) = crate::settings::read(connection, CHIAVE_VOLUME)? else {
        return Ok(Volume::default());
    };
    Ok(serde_json::from_str::<VolumeSuDisco>(&json)
        .map(|v| {
            let volume = if v.volume.is_finite() {
                v.volume.clamp(0.0, 1.0)
            } else {
                Volume::default().volume
            };
            Volume {
                volume: if v.curva == CURVA_PERCEPITA {
                    volume
                } else {
                    volume.cbrt()
                },
                muto: v.muto,
            }
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
/// Fino alla 2.4 lasciarla accesa su una libreria senza tag non cambiava
/// niente: senza `replaygain_db` la correzione era esattamente 1.0. Dalla 3.0
/// la passata d'analisi misura ogni brano ([`crate::intensita`]), e accesa
/// vuol dire davvero tutti i brani allo stesso volume. Chi vuole i dischi come
/// sono stati masterizzati la spegne.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Normalizzazione {
    /// Il guadagno si applica.
    pub attivo: bool,
    /// A quanti LUFS si porta tutto. Il riferimento dei tag è −18.
    pub bersaglio_db: f32,
    /// Livello del brano o del disco. Assente nelle righe scritte prima della
    /// 3.0, e allora vale il suo valore di serie.
    #[serde(default)]
    pub modo: ModoLivello,
}

impl Default for Normalizzazione {
    fn default() -> Self {
        Self {
            attivo: true,
            bersaglio_db: BERSAGLIO_PREDEFINITO_DB,
            modo: ModoLivello::default(),
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
/// oltre lo zero: la correzione finale in `rymembra_play::guadagno` è tagliata a
/// +12 dB e la conversione di `rymembra_play::uscita` satura invece di
/// avvolgere, quindi il caso peggiore è una compressione udibile, non un
/// rumore. Chi ha una libreria di dischi rumorosi tiene «normale».
pub const BERSAGLIO_ALTO_DB: f32 = -14.0;

/// I limiti oltre cui un bersaglio non è più una preferenza ma un guasto.
///
/// Gli stessi che `rymembra_play::guadagno` applica alla correzione finale: qui
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
/// `rymembra_domain::queue`, che oggi non sa nemmeno cosa sia il tempo, si
/// ritrova un campo che solo `rymembra-play` sa produrre.
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
/// `library.trackNotFound` se `track_id` ≤ 0, senza scrivere niente — il segno
/// che c'era resta quello di prima: vedi [`riga_della_libreria`].
/// `db.queryFailed` se il database non risponde.
pub fn save_posizione(connection: &Connection, track_id: i64, ms: u64) -> Result<(), AppError> {
    riga_della_libreria(track_id)?;
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
/// Un id ≤ 0 vale l'inizio senza nemmeno leggere. Il numero nudo vale per
/// «il brano che si chiede», e una voce di «Ascolta insieme» non è mai stata
/// quel brano: senza questa riga, la prima voce di una sessione partirebbe al
/// secondo in cui l'utente aveva lasciato una canzone sua, prima
/// dell'aggiornamento. Un segno `DelBrano` con un id ≤ 0 invece non può
/// esserci, perché [`save_posizione`] non lo scrive.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_posizione(connection: &Connection, track_id: i64) -> Result<u64, AppError> {
    if riga_della_libreria(track_id).is_err() {
        return Ok(0);
    }
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
/// ascolta un album ha scelto Rymembra per avere. Un aggiornamento che
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
/// (`rymembra_play::uscita::annota_latenza`); questo numero esiste per il pezzo che
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

/// La latenza dichiarata **per ogni uscita**, accanto a quella globale.
///
/// # Perché una per uscita
///
/// Perché [`CHIAVE_LATENZA`] è una sola e le catene sono tante: le casse col
/// cavo, le cuffie Bluetooth e il DAC sulla scrivania ritardano ciascuno del
/// suo, e chi passa dall'una all'altra ritrovava il numero tarato per quella di
/// prima. Ascoltare insieme su più computer lo rende un difetto e non più un
/// fastidio: lì il numero sposta **il suono**, non solo il cursore.
///
/// # Come sta scritta
///
/// Un oggetto JSON da nome d'uscita a [`LatenzaUscita`]. Il nome è quello che
/// `rymembra_play::Motore::dispositivo` dice di aver **davvero** aperto, non la
/// preferenza `player.output`: quando la scheda scelta non c'è l'apertura
/// ripiega su un'altra, e il numero va a chi sta suonando.
///
/// Sta sotto `audio.` come la globale, e per la stessa ragione resta fuori dal
/// profilo: descrive questa catena su questo computer — e in più dentro ci
/// sono i nomi delle sue schede.
pub const CHIAVE_LATENZA_PER_USCITA: &str = "audio.latenza_per_uscita";

/// Da dove viene il numero di un'uscita.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FonteLatenza {
    /// Il cursore delle impostazioni: un numero dichiarato, non misurato.
    Mano,
    /// Una taratura a orecchio, guardando il cursore contro la musica.
    Orecchio,
    /// Una misura col microfono.
    Microfono,
}

/// La latenza di un'uscita, con quel che serve a sapere quanto fidarsene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LatenzaUscita {
    /// I millisecondi, nel verso e dentro i limiti di [`CHIAVE_LATENZA`].
    pub ms: i64,
    /// Chi ha deciso il numero.
    pub fonte: FonteLatenza,
    /// Il numero è stato confermato e niente l'ha smentito da allora. Un
    /// evento che cambia la catena — le cuffie che si ricollegano con un altro
    /// codec — lo riporta a `false` senza toccare `ms`.
    pub verificata: bool,
    /// Quando è stato scritto, in millisecondi dall'epoca.
    pub quando_ms: i64,
}

/// Le latenze delle uscite, per nome. Un `BTreeMap` perché il JSON che ne esce
/// sia lo stesso a parità di contenuto: due scritture uguali, una riga uguale.
pub type LatenzePerUscita = std::collections::BTreeMap<String, LatenzaUscita>;

/// Quel che [`load_latenze_per_uscita`] ha trovato, e quel che ha dovuto
/// lasciare fuori.
///
/// Il nucleo non ha un diario, e una mappa rotta che diventa vuota senza dirlo
/// è un utente che ritrova tutte le uscite a zero senza sapere perché. I due
/// campi accanto alle voci sono il modo di dirlo alla finestra, che il diario
/// ce l'ha — e sono solo un `bool` e un numero: niente nomi, che sono le schede
/// di qualcuno.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LatenzeLette {
    /// Le voci lette, già riportate dentro i limiti.
    pub voci: LatenzePerUscita,
    /// La chiave c'era ma non era un oggetto JSON: vale come vuota.
    pub illeggibile: bool,
    /// Quante voci c'erano ma non si leggevano, e sono rimaste fuori.
    pub scartate: usize,
}

/// Rilegge le latenze per uscita. Assente vale **nessuna voce**, cioè la
/// globale per tutte.
///
/// # Una voce rotta non porta via le altre
///
/// Si legge voce per voce: un nome con un campo storto — una `fonte` che questa
/// versione non conosce, scritta da una più nuova — resta fuori da solo, e le
/// tarature delle altre uscite restano. Solo un valore che non è nemmeno un
/// oggetto vale tutto vuoto. In tutti e due i casi lo dice [`LatenzeLette`].
///
/// Come per la globale, i millisecondi si stringono **in lettura** oltre che in
/// scrittura: la riga può arrivare da un database ritoccato a mano.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Il JSON storto non ne produce
/// nessuno.
pub fn load_latenze_per_uscita(connection: &Connection) -> Result<LatenzeLette, AppError> {
    let mut lette = LatenzeLette::default();
    let Some(grezzo) = crate::settings::read(connection, CHIAVE_LATENZA_PER_USCITA)? else {
        return Ok(lette);
    };
    let Ok(voci) =
        serde_json::from_str::<std::collections::BTreeMap<String, serde_json::Value>>(&grezzo)
    else {
        lette.illeggibile = true;
        return Ok(lette);
    };
    for (nome, valore) in voci {
        match serde_json::from_value::<LatenzaUscita>(valore) {
            Ok(voce) => {
                lette.voci.insert(nome, sana_voce(voce));
            }
            Err(_) => lette.scartate = lette.scartate.saturating_add(1),
        }
    }
    Ok(lette)
}

/// Conserva la latenza di un'uscita, lasciando com'erano le altre.
///
/// Rilegge la mappa, cambia la voce di `nome` e la riscrive intera. Le voci
/// che la lettura ha scartato perché rotte se ne vanno con questa scrittura,
/// ed è voluto: una mappa illeggibile si ripara da sola alla prima taratura
/// invece di restare illeggibile per sempre.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_latenza_per_uscita(
    connection: &Connection,
    nome: &str,
    voce: LatenzaUscita,
) -> Result<(), AppError> {
    let mut voci = load_latenze_per_uscita(connection)?.voci;
    voci.insert(nome.to_owned(), sana_voce(voce));
    crate::settings::write_json(connection, CHIAVE_LATENZA_PER_USCITA, &voci)
}

/// La latenza da dare al motore che suona su `dispositivo`.
///
/// Il numero dell'uscita se c'è, altrimenti la globale. `None` — nessun motore
/// aperto, o un nome che non si sa — vale la globale anche lui.
///
/// La globale come ripiego, e non zero, perché è il numero che chi aggiorna ha
/// già tarato: prima di questa versione valeva per tutte le uscite, e
/// un'uscita che non ha ancora un numero suo continua a sentirsi come ieri.
#[must_use]
pub fn latenza_effettiva(globale: i64, mappa: &LatenzePerUscita, dispositivo: Option<&str>) -> i64 {
    sana_latenza(
        dispositivo
            .and_then(|nome| mappa.get(nome))
            .map_or(globale, |voce| voce.ms),
    )
}

/// Il cursore delle impostazioni: scrive il numero per l'uscita che suona e
/// anche come globale, e rilegge le due cose.
///
/// # Perché tutte e due
///
/// Quella dell'uscita perché è la ragione per cui la mappa esiste. La globale
/// perché il cursore di oggi è uno solo e deve continuare a valere: è il
/// ripiego di ogni uscita che un numero suo non ce l'ha ancora, e senza
/// aggiornarla le casse mai tarate continuerebbero a usare quello di ieri
/// mentre il cursore ne mostra un altro.
///
/// `uscita` è `None` quando nessun motore è aperto: allora si scrive la sola
/// globale. La voce porta `fonte: mano` e `verificata: false`, perché il
/// cursore dice quanto si **vuole** correggere, non quanto si è misurato; la
/// verifica la darà la taratura, quando ci sarà.
///
/// Restituisce la globale e la mappa come sono rimaste scritte: chi chiama ne
/// ricava la latenza per l'uscita che ha davanti con [`latenza_effettiva`] —
/// che è la stessa uscita di prima, o quella nuova se nel frattempo il motore è
/// stato riaperto.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn dichiara_latenza(
    connection: &Connection,
    uscita: Option<&str>,
    ms: i64,
    quando_ms: i64,
) -> Result<(i64, LatenzePerUscita), AppError> {
    if let Some(nome) = uscita {
        save_latenza_per_uscita(
            connection,
            nome,
            LatenzaUscita {
                ms,
                fonte: FonteLatenza::Mano,
                verificata: false,
                quando_ms,
            },
        )?;
    }
    save_latenza(connection, ms)?;
    Ok((
        load_latenza(connection)?,
        load_latenze_per_uscita(connection)?.voci,
    ))
}

/// Una voce con i millisecondi dentro i limiti della globale.
fn sana_voce(voce: LatenzaUscita) -> LatenzaUscita {
    LatenzaUscita {
        ms: sana_latenza(voce.ms),
        ..voce
    }
}

// ── da quale scheda esce il suono ───────────────────────────────────────────

/// Il nome dell'uscita audio scelta a mano. Assente: quella di sistema.
pub const CHIAVE_USCITA: &str = "player.output";

/// Conserva su quale uscita si vuole sentire Rymembra.
///
/// `None` non è «non lo so»: è la scelta esplicita «quella che usa il sistema»,
/// e va scritta come le altre — cancellare la chiave e riscriverla sono la
/// stessa cosa per chi legge, ma un `null` in `settings` dice che qualcuno c'è
/// passato, e nel dubbio è quel che si vuole trovare.
///
/// # Perché il nome e non un identificativo
///
/// Perché le scelte già salvate sono nomi: fino alla 2.4 cpal non dava altro, e
/// dalla 0.18 il suo `Device::id()` stabile serve a riconoscere il predefinito
/// fra due schede omonime, non ancora a ricordare la scelta — passarci vorrebbe
/// dire migrare le preferenze. Il prezzo è
/// scritto in testa a `rymembra_play::dispositivi` — nomi tradotti, ripetuti, che
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
/// scala vale [`rymembra_play::RISOLUZIONE_DI_SERIE`]: la stessa regola del resto
/// del modulo, e qui conta doppio perché la preferenza è disegno, non suono —
/// rifiutare di partire per una barra in più sarebbe sproporzionato.
pub fn load_spettro_bande(connection: &Connection) -> Result<u16, AppError> {
    Ok(
        crate::settings::read_json::<u16>(connection, CHIAVE_SPETTRO_BANDE)?
            .map(sane)
            .unwrap_or(rymembra_play::RISOLUZIONE_DI_SERIE),
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
    rymembra_play::RISOLUZIONI
        .into_iter()
        .min_by(|a, b| {
            let scarto = |v: u16| (f32::from(v) / f32::from(quante.max(1))).log2().abs();
            scarto(*a).total_cmp(&scarto(*b))
        })
        .unwrap_or(rymembra_play::RISOLUZIONE_DI_SERIE)
}

/// Porta un bersaglio dentro i limiti, e un valore non finito al riferimento.
fn sana(normalizzazione: Normalizzazione) -> Normalizzazione {
    Normalizzazione {
        attivo: normalizzazione.attivo,
        modo: normalizzazione.modo,
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
    /// Quanti decibel per banda, nell'ordine di `rymembra_play::CENTRI_HZ`.
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

impl rymembra_play::decodifica::Flusso for Adattatore {
    fn lunghezza(&self) -> Option<u64> {
        self.byte
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use rymembra_domain::listen::Listen;

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
    fn il_volume_della_2_4_suona_uguale_con_la_curva_nuova() {
        // Ieri 0,512 d'ampiezza; oggi la posizione che, al cubo, dà la stessa.
        let c = db();
        crate::settings::write(&c, CHIAVE_VOLUME, r#"{"volume":0.512,"muto":false}"#)
            .expect("scritto");
        let letto = load_volume(&c).expect("riletto");
        assert!(
            (letto.volume - 0.8).abs() < 1e-5,
            "posizione {}",
            letto.volume
        );
        assert!(
            (rymembra_play::volume_uscita(letto.volume, false) - 0.512).abs() < 1e-5,
            "non suona come prima"
        );
        // Salvato una volta, resta dov'è: la conversione non si ripete.
        save_volume(&c, letto).expect("salvato");
        let riletto = load_volume(&c).expect("riletto");
        assert!((riletto.volume - 0.8).abs() < 1e-5);
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
            rymembra_play::RISOLUZIONE_DI_SERIE
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
            rymembra_play::RISOLUZIONE_DI_SERIE
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

    // ── la latenza per uscita ──

    fn voce(ms: i64, fonte: FonteLatenza) -> LatenzaUscita {
        LatenzaUscita {
            ms,
            fonte,
            verificata: true,
            quando_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn un_uscita_senza_numero_suo_usa_la_globale() {
        let mut mappa = LatenzePerUscita::new();
        mappa.insert("Cuffie BT".to_owned(), voce(180, FonteLatenza::Microfono));
        // L'uscita con la sua voce usa la sua, anche quando vale zero.
        assert_eq!(latenza_effettiva(40, &mappa, Some("Cuffie BT")), 180);
        mappa.insert("DAC".to_owned(), voce(0, FonteLatenza::Mano));
        assert_eq!(latenza_effettiva(40, &mappa, Some("DAC")), 0);
        // Le altre, e il motore che non c'è, ripiegano sulla globale.
        assert_eq!(latenza_effettiva(40, &mappa, Some("Altoparlanti")), 40);
        assert_eq!(latenza_effettiva(-30, &mappa, None), -30);
        assert_eq!(
            latenza_effettiva(40, &LatenzePerUscita::new(), Some("DAC")),
            40
        );
        // E il limite vale anche qui: un numero arrivato da fuori non passa.
        assert_eq!(
            latenza_effettiva(9_000, &LatenzePerUscita::new(), None),
            LATENZA_MASSIMA_MS
        );
    }

    #[test]
    fn la_mappa_delle_uscite_torna_identica() {
        let c = db();
        assert_eq!(
            load_latenze_per_uscita(&c).expect("riletta"),
            LatenzeLette::default(),
            "mai scritta vale nessuna voce, senza niente da annotare"
        );
        save_latenza_per_uscita(&c, "Cuffie BT", voce(180, FonteLatenza::Microfono))
            .expect("scritta");
        save_latenza_per_uscita(&c, "Altoparlanti", voce(-20, FonteLatenza::Orecchio))
            .expect("scritta");
        // Riscrivere una voce non tocca l'altra.
        save_latenza_per_uscita(&c, "Cuffie BT", voce(150, FonteLatenza::Mano)).expect("riscritta");

        let mut attesa = LatenzePerUscita::new();
        attesa.insert("Altoparlanti".to_owned(), voce(-20, FonteLatenza::Orecchio));
        attesa.insert("Cuffie BT".to_owned(), voce(150, FonteLatenza::Mano));
        assert_eq!(
            load_latenze_per_uscita(&c).expect("riletta"),
            LatenzeLette {
                voci: attesa,
                illeggibile: false,
                scartate: 0,
            }
        );
        // La forma sul disco è quella promessa: la leggerà anche la finestra.
        let grezzo = crate::settings::read(&c, CHIAVE_LATENZA_PER_USCITA)
            .expect("letta")
            .expect("c'è");
        assert!(grezzo.contains(r#""fonte":"orecchio""#), "{grezzo}");
        assert!(grezzo.contains(r#""quando_ms":1700000000000"#), "{grezzo}");
    }

    #[test]
    fn la_latenza_di_un_uscita_si_taglia_come_la_globale() {
        let c = db();
        save_latenza_per_uscita(&c, "DAC", voce(5_000, FonteLatenza::Mano)).expect("scritta");
        assert_eq!(
            load_latenze_per_uscita(&c)
                .expect("riletta")
                .voci
                .get("DAC")
                .map(|v| v.ms),
            Some(LATENZA_MASSIMA_MS),
            "si taglia in scrittura"
        );
        // E in lettura: la riga può arrivare da un database ritoccato a mano.
        crate::settings::write(
            &c,
            CHIAVE_LATENZA_PER_USCITA,
            r#"{"DAC":{"ms":5000,"fonte":"mano","verificata":false,"quando_ms":0},
               "BT":{"ms":-3600000,"fonte":"microfono","verificata":true,"quando_ms":0}}"#,
        )
        .expect("scritta a mano");
        let lette = load_latenze_per_uscita(&c).expect("riletta");
        assert_eq!(
            lette.voci.get("DAC").map(|v| v.ms),
            Some(LATENZA_MASSIMA_MS)
        );
        assert_eq!(
            lette.voci.get("BT").map(|v| v.ms),
            Some(-LATENZA_MASSIMA_MS)
        );
    }

    #[test]
    fn una_mappa_illeggibile_vale_vuota_e_lo_dice() {
        let c = db();
        crate::settings::write(&c, CHIAVE_LATENZA_PER_USCITA, "{non è json").expect("scritta");
        let lette = load_latenze_per_uscita(&c).expect("non è un errore");
        assert!(lette.voci.is_empty());
        assert!(
            lette.illeggibile,
            "la finestra deve poterlo scrivere nel diario"
        );

        // Un JSON valido ma di un'altra forma è illeggibile allo stesso modo.
        crate::settings::write(&c, CHIAVE_LATENZA_PER_USCITA, "[1,2,3]").expect("scritta");
        assert!(load_latenze_per_uscita(&c).expect("letta").illeggibile);

        // E la prima taratura la ripara invece di lasciarla rotta per sempre.
        save_latenza_per_uscita(&c, "DAC", voce(30, FonteLatenza::Mano)).expect("scritta");
        let riparata = load_latenze_per_uscita(&c).expect("riletta");
        assert!(!riparata.illeggibile);
        assert_eq!(riparata.voci.get("DAC").map(|v| v.ms), Some(30));
    }

    #[test]
    fn una_voce_rotta_non_porta_via_le_altre() {
        // Il caso vero: una `fonte` che questa versione non conosce, scritta
        // da una più nuova. Le tarature delle altre uscite restano.
        let c = db();
        crate::settings::write(
            &c,
            CHIAVE_LATENZA_PER_USCITA,
            r#"{"DAC":{"ms":25,"fonte":"mano","verificata":false,"quando_ms":0},
               "BT":{"ms":200,"fonte":"radar","verificata":true,"quando_ms":0}}"#,
        )
        .expect("scritta");
        let lette = load_latenze_per_uscita(&c).expect("letta");
        assert!(!lette.illeggibile);
        assert_eq!(lette.scartate, 1);
        assert_eq!(lette.voci.len(), 1);
        assert_eq!(lette.voci.get("DAC").map(|v| v.ms), Some(25));
    }

    #[test]
    fn il_cursore_scrive_l_uscita_corrente_e_la_globale() {
        let c = db();
        save_latenza(&c, 40).expect("globale di ieri");
        save_latenza_per_uscita(&c, "Cuffie BT", voce(180, FonteLatenza::Microfono))
            .expect("taratura delle cuffie");

        let (globale, voci) =
            dichiara_latenza(&c, Some("DAC"), 75, 1_800_000_000_000).expect("dichiarata");
        // L'uscita che suona ha il suo numero, scritto come gesto a mano.
        assert_eq!(
            voci.get("DAC"),
            Some(&LatenzaUscita {
                ms: 75,
                fonte: FonteLatenza::Mano,
                verificata: false,
                quando_ms: 1_800_000_000_000,
            })
        );
        assert_eq!(latenza_effettiva(globale, &voci, Some("DAC")), 75);
        // La globale segue il cursore: è il ripiego di chi non ha un numero suo.
        assert_eq!(globale, 75);
        assert_eq!(load_latenza(&c).expect("riletta"), 75);
        assert_eq!(latenza_effettiva(globale, &voci, Some("Altoparlanti")), 75);
        // Le cuffie tarate col microfono non le tocca nessuno.
        assert_eq!(latenza_effettiva(globale, &voci, Some("Cuffie BT")), 180);
        assert_eq!(
            voci.get("Cuffie BT").map(|v| v.fonte),
            Some(FonteLatenza::Microfono)
        );

        // Un valore fuori scala torna tagliato, ed è quel che il cursore mostra.
        let (globale, voci) = dichiara_latenza(&c, Some("DAC"), -9_000, 0).expect("dichiarata");
        assert_eq!(globale, -LATENZA_MASSIMA_MS);
        assert_eq!(
            latenza_effettiva(globale, &voci, Some("DAC")),
            -LATENZA_MASSIMA_MS
        );
    }

    #[test]
    fn senza_motore_il_cursore_scrive_solo_la_globale() {
        let c = db();
        let (globale, voci) = dichiara_latenza(&c, None, 60, 0).expect("dichiarata");
        assert_eq!(globale, 60);
        assert!(voci.is_empty(), "nessun nome da inventare");
    }

    // ── le guardie: nessun id ≤ 0 sul database ──

    #[test]
    fn un_ascolto_di_una_voce_di_sessione_non_si_scrive() {
        // Le righe ci sono davvero, con id 0 e negativo: SQLite le accetta se
        // gliele si dà a mano. Così la prova cade se cade la guardia, e non
        // passa per caso perché l'UPDATE non trova niente.
        let mut c = db();
        for id in [0, -5] {
            brano(&c, id, 180_000);
            let esito = record_play(
                &mut c,
                &Listen {
                    track_id: id,
                    started_at: 1_700_000_000_000,
                    listened_ms: 120_000,
                    counts: true,
                },
                "prova",
            );
            assert_eq!(
                esito.map_err(|err| err.code().kind()),
                Err(ErrorCodeKind::LibraryTrackNotFound),
                "id {id}"
            );
            assert_eq!(conteggio(&c, id), (0, None, 0), "id {id}");
        }
        let cronologia: i64 = c
            .query_row("SELECT COUNT(*) FROM play_history", [], |r| r.get(0))
            .expect("cronologia");
        assert_eq!(cronologia, 0);
    }

    #[test]
    fn la_posizione_di_una_voce_di_sessione_non_si_scrive() {
        let c = db();
        save_posizione(&c, 7, 42_000).expect("scritta");
        for id in [0, -3] {
            assert_eq!(
                save_posizione(&c, id, 99_000).map_err(|err| err.code().kind()),
                Err(ErrorCodeKind::LibraryTrackNotFound),
                "id {id}"
            );
        }
        // Il segno di prima è ancora lì, intatto.
        assert_eq!(load_posizione(&c, 7).expect("letta"), 42_000);
    }

    #[test]
    fn il_segno_di_prima_non_vale_per_una_voce_di_sessione() {
        // Il numero nudo che scriveva la versione di prima vale per qualunque
        // brano si chieda: è il motivo per cui esiste, ed è il buco. Una voce
        // di sessione partirebbe lì.
        let c = db();
        crate::settings::write(&c, CHIAVE_POSIZIONE, "42000").expect("segno di prima");
        assert_eq!(
            load_posizione(&c, 7).expect("letta"),
            42_000,
            "per una riga vale"
        );
        for id in [0, -3] {
            assert_eq!(load_posizione(&c, id).expect("letta"), 0, "id {id}");
        }
    }

    #[test]
    fn una_coda_sul_disco_con_una_voce_di_sessione_vale_vuota() {
        // Scritta a mano, perché `save_queue` non la scriverebbe: è il caso del
        // database toccato da fuori, e all'avvio quelle voci entrerebbero nel
        // lettore.
        let c = db();
        crate::settings::write(
            &c,
            CHIAVE_CODA,
            r#"{"tracks":[3,-1],"order":[0,1],"order_pos":0,"shuffle":false,"repeat":"off"}"#,
        )
        .expect("coda a mano");
        assert_eq!(load_queue(&c).expect("letta"), QueueSnapshot::default());

        // La stessa forma con sole righe vere si rilegge com'è.
        crate::settings::write(
            &c,
            CHIAVE_CODA,
            r#"{"tracks":[3,1],"order":[0,1],"order_pos":0,"shuffle":false,"repeat":"off"}"#,
        )
        .expect("coda a mano");
        assert_eq!(load_queue(&c).expect("letta").tracks, [3, 1]);
    }

    #[test]
    fn una_coda_con_una_voce_di_sessione_non_si_scrive_affatto() {
        let c = db();
        let mia = QueueSnapshot {
            tracks: vec![3, 1, 2],
            order: vec![0, 1, 2],
            order_pos: Some(1),
            shuffle: false,
            repeat: RepeatMode::Off,
        };
        save_queue(&c, &mia).expect("salvata");

        let rifiutate = [
            // Il corrente è una maniglia.
            QueueSnapshot {
                tracks: vec![-1, -2],
                order: vec![0, 1],
                order_pos: Some(0),
                ..QueueSnapshot::default()
            },
            // Una coda di sessione finita: nessun corrente. È il buco della
            // guardia sul solo corrente — filtrata, diventerebbe una coda vuota
            // scritta sopra quella dell'utente.
            QueueSnapshot {
                tracks: vec![-1, -2, -3],
                order: vec![0, 1, 2],
                order_pos: None,
                ..QueueSnapshot::default()
            },
            // Mista, con il corrente che è una riga vera.
            QueueSnapshot {
                tracks: vec![4, -9],
                order: vec![0, 1],
                order_pos: Some(0),
                ..QueueSnapshot::default()
            },
            // Lo zero non è di nessuno, e si rifiuta con i negativi.
            QueueSnapshot {
                tracks: vec![5, 0],
                order: vec![0, 1],
                order_pos: Some(0),
                ..QueueSnapshot::default()
            },
        ];
        for coda in &rifiutate {
            assert_eq!(
                save_queue(&c, coda).map_err(|err| err.code().kind()),
                Err(ErrorCodeKind::LibraryTrackNotFound),
                "{coda:?}"
            );
            assert_eq!(
                load_queue(&c).expect("riletta"),
                mia,
                "sul disco deve restare la coda dell'utente"
            );
        }
    }

    #[test]
    fn spegnere_la_normalizzazione_resta_scritto() {
        let c = db();
        save_replaygain(
            &c,
            Normalizzazione {
                attivo: false,
                bersaglio_db: -14.0,
                modo: ModoLivello::Album,
            },
        )
        .expect("salvata");
        let letta = load_replaygain(&c).expect("riletta");
        assert!(!letta.attivo);
        assert_eq!(letta.bersaglio_db, -14.0);
        assert_eq!(letta.modo, ModoLivello::Album);
    }

    #[test]
    fn una_normalizzazione_della_2_4_prende_il_modo_di_serie() {
        let c = db();
        crate::settings::write(
            &c,
            CHIAVE_REPLAYGAIN,
            r#"{"attivo":true,"bersaglio_db":-23.0}"#,
        )
        .expect("scritta");
        let letta = load_replaygain(&c).expect("riletta");
        assert_eq!(letta.bersaglio_db, -23.0);
        assert_eq!(letta.modo, ModoLivello::Automatico);
    }

    #[test]
    fn un_bersaglio_assurdo_si_taglia() {
        // Come per la curva: il valore si può scrivere a mano nel database, e
        // `rymembra_play::guadagno` taglierebbe comunque la correzione a ±24 dB —
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
        use rymembra_domain::errors::ErrorCodeKind;
        let c = db();
        let err = sorgente(&c, &crate::files::LocalFiles, 999).expect_err("deve fallire");
        assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackSourceUnavailable);
    }

    #[test]
    fn un_file_che_non_c_e_piu_da_lo_stesso_errore() {
        use rymembra_domain::errors::ErrorCodeKind;
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
            collocazione: Collocazione::File("//srv/musica/1.mp3".to_owned()),
            durata_ms: 1_000,
            replaygain_db: None,
            picco: None,
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

    // ── i brani d'immagine CD ───────────────────────────────────────────────

    /// L'immagine di prova: lo stesso file per tutti i brani del disco.
    const IMMAGINE: &str = "C:/m/Disco/Album.flac";

    /// Il brano `numero` del disco, scritto a mano come lo scrive la scansione:
    /// percorso virtuale, immagine, confini in fotogrammi CD, durata del brano.
    fn brano_d_immagine(
        connection: &Connection,
        id: i64,
        numero: u8,
        inizio_cd: Option<i64>,
        fine_cd: Option<i64>,
    ) {
        connection
            .execute(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified,
                      immagine, inizio_cd, fine_cd)
                 VALUES (?1, ?2, ?3, 'T', 'A', 'Al', 200000, 300000000, 0, 0,
                         ?4, ?5, ?6)",
                rusqlite::params![
                    id,
                    rymembra_domain::cue::percorso_del_brano("C:/m/Disco/Album.cue", numero),
                    format!("k{id}"),
                    IMMAGINE,
                    inizio_cd,
                    fine_cd
                ],
            )
            .expect("brano d'immagine inserito");
    }

    /// Un `MusicFiles` che apre un flusso vuoto e si ricorda che cosa ha aperto.
    #[derive(Default)]
    struct FileAperti(std::sync::Mutex<Vec<String>>);

    impl MusicFiles for FileAperti {
        fn walk(&self, _root: &str) -> Result<crate::files::Camminata, AppError> {
            Ok(crate::files::Camminata {
                file: Vec::new(),
                completa: true,
            })
        }

        fn open(
            &self,
            path: &str,
        ) -> Result<Box<dyn crate::files::ReadSeek + Send + Sync>, AppError> {
            self.0.lock().expect("elenco").push(path.to_owned());
            Ok(Box::new(std::io::Cursor::new(Vec::<u8>::new())))
        }
    }

    #[test]
    fn un_brano_d_immagine_apre_l_immagine_e_porta_il_suo_pezzo() {
        let c = db();
        brano_d_immagine(&c, 1, 2, Some(15_000), Some(30_000));
        let scheda = scheda_sorgente(&c, 1).expect("scheda");
        assert_eq!(
            scheda.collocazione,
            Collocazione::Segmento {
                immagine: IMMAGINE.to_owned(),
                inizio_cd: 15_000,
                fine_cd: Some(30_000),
            }
        );
        // Accanto al brano non c'è un file suo; l'audio sta nell'immagine.
        assert_eq!(scheda.collocazione.percorso(), None);
        assert_eq!(scheda.collocazione.file_audio(), Some(IMMAGINE));
        assert_eq!(scheda.collocazione.fonte(), None);

        let files = FileAperti::default();
        let sorgente = sorgente_da_scheda(&files, &scheda).expect("sorgente");
        assert_eq!(
            *files.0.lock().expect("elenco"),
            vec![IMMAGINE.to_owned()],
            "si apre l'immagine, mai il percorso virtuale"
        );
        assert_eq!(
            sorgente.segmento,
            Some(Segmento {
                immagine: IMMAGINE.to_owned(),
                inizio_cd: 15_000,
                fine_cd: Some(30_000),
            })
        );
        assert_eq!(sorgente.estensione.as_deref(), Some("flac"));
        assert_eq!(sorgente.durata_ms, 200_000, "la durata del brano");
    }

    #[test]
    fn l_ultimo_brano_d_immagine_va_fino_in_fondo() {
        let c = db();
        brano_d_immagine(&c, 1, 9, Some(150_000), None);
        let files = FileAperti::default();
        let sorgente = sorgente(&c, &files, 1).expect("sorgente");
        assert_eq!(
            sorgente.segmento.map(|s| (s.inizio_cd, s.fine_cd)),
            Some((150_000, None))
        );
    }

    #[test]
    fn un_file_intero_non_porta_un_segmento() {
        let c = db();
        brano(&c, 1, 1000);
        let files = FileAperti::default();
        let sorgente = sorgente(&c, &files, 1).expect("sorgente");
        assert_eq!(sorgente.segmento, None);
        assert_eq!(
            *files.0.lock().expect("elenco"),
            vec!["C:/m/1.mp3".to_owned()]
        );
    }

    #[test]
    fn un_brano_d_immagine_senza_confini_non_si_suona() {
        // Senza inizio si suonerebbe il disco dall'inizio, cioè un altro brano;
        // con una fine prima dell'inizio, niente di sensato.
        let c = db();
        brano_d_immagine(&c, 1, 1, None, Some(30_000));
        brano_d_immagine(&c, 2, 2, Some(30_000), Some(30_000));
        for id in [1, 2] {
            let err = scheda_sorgente(&c, id).expect_err("riga d'immagine rotta");
            assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackSourceUnavailable);
        }
    }

    #[test]
    fn i_confini_d_immagine_si_leggono_solo_se_stanno_in_un_cd() {
        let immagine = || Some(IMMAGINE.to_owned());
        assert_eq!(
            collocazione_su_disco("C:/m/a.flac", None, Some(5), Some(1)),
            Some(Collocazione::File("C:/m/a.flac".to_owned())),
            "senza immagine le colonne dei confini non contano"
        );
        assert_eq!(collocazione_su_disco("x", immagine(), Some(-1), None), None);
        assert_eq!(
            collocazione_su_disco("x", immagine(), Some(10), Some(10)),
            None
        );
        assert_eq!(
            collocazione_su_disco("x", immagine(), Some(0), Some(i64::from(u32::MAX) + 1)),
            None
        );
        assert_eq!(
            collocazione_su_disco("x", immagine(), Some(0), Some(75)),
            Some(Collocazione::Segmento {
                immagine: IMMAGINE.to_owned(),
                inizio_cd: 0,
                fine_cd: Some(75),
            })
        );
    }
}
