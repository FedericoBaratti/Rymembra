//! Dalla riga del database ai byte per il motore: la scheda di un brano, il
//! livello con cui suonarlo, e l'apertura del file, che non tocca il database.

use rusqlite::Connection;
use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_domain::esterno::Fonte;
use rymembra_play::{Segmento, Sorgente};

use super::load_replaygain;
use crate::files::MusicFiles;
use crate::library::db_error;

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
/// la stessa `album_key` senza essere un album. E il segnaposto che la
/// scansione scrive al posto del tag mancante non è un titolo: vedi
/// [`crate::enrich::album_con_nome_sql`].
pub(super) fn stesso_disco(
    connection: &Connection,
    track_id: i64,
    intorno: Intorno,
) -> Result<bool, AppError> {
    let vicini = [intorno.prima, intorno.dopo];
    let sql = format!(
        "SELECT EXISTS (
           SELECT 1 FROM tracks a JOIN tracks b ON b.album_key = a.album_key
            WHERE a.id = ?1 AND b.id = ?2
              AND {})",
        crate::enrich::album_con_nome_sql("a.album")
    );
    for vicino in vicini.into_iter().flatten() {
        if vicino == track_id {
            continue;
        }
        let uguale: bool = connection
            .query_row(&sql, rusqlite::params![track_id, vicino], |row| row.get(0))
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
/// perché è ritentabile; così anche `net.rateLimited` e i `cloud.*` (vedi il
/// corpo); `playback.sourceUnavailable` per ogni altro modo in cui
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
        //
        // Lo stesso per i guasti del cloud — un conto da ricollegare, una
        // quota finita, una copia che la cache non ha più — e per un servizio
        // che chiede di rallentare: dicono qualcosa del servizio, non del
        // brano, e chi li riceve (l'analisi, la finestra) li tratta per quel
        // che sono invece che come un file rotto.
        let tipo = err.code().kind();
        if tipo == ErrorCodeKind::FsNetworkUnavailable
            || tipo == ErrorCodeKind::NetRateLimited
            || tipo.domain() == rymembra_domain::errors::Domain::Cloud
        {
            return err;
        }
        AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: Some(scheda.track_id),
            path: Some(percorso.to_owned()),
        })
        .with_cause(err.cause().unwrap_or(err.code().kind().code()).to_owned())
    })?;

    Ok(sorgente_con(scheda, Box::new(Adattatore::nuovo(media))))
}

/// La sorgente di un brano i cui byte sono già aperti.
///
/// La metà di [`sorgente_da_scheda`] che non dipende da **dove** stanno i
/// byte: il pezzo di un brano d'immagine, il suggerimento di formato, la
/// durata, il ReplayGain. La usa anche il desktop per un brano del cloud, che
/// arriva come flusso e non come file — e un brano d'immagine CD su OneDrive
/// deve suonare il suo pezzo esattamente come sul disco.
#[must_use]
pub fn sorgente_con(scheda: &SchedaSorgente, media: Box<dyn rymembra_play::Flusso>) -> Sorgente {
    let percorso = scheda.collocazione.file_audio().unwrap_or_default();
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

    Sorgente {
        track_id: scheda.track_id,
        media,
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
    }
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
pub(super) fn estensione_di(path: &str) -> Option<String> {
    let ultimo = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (_, ext) = ultimo.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 5 {
        return None;
    }
    Some(ext.to_ascii_lowercase())
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
