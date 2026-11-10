//! Le domande degli altri dispositivi sulla libreria di questo: gli elenchi,
//! le copertine, i byte dei brani, gli ascolti da contare qui.
//!
//! Si condividono solo i file delle cartelle sorvegliate, senza percorsi:
//! vedi `rymembra_app::library::condivisa`.
//!
//! # Gli ascolti
//!
//! Un brano di questo dispositivo ascoltato su un altro si conta **qui**: è
//! qui che stanno le sue statistiche, ed è da qui che parte lo scrobble — una
//! volta sola, anche se l'ascolto arriva due volte (la sua chiave lo rende
//! innocuo). Si intesta a questo dispositivo, come un ascolto fatto qui: per
//! la sincronia è un ascolto della sua libreria.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::time::{Duration, UNIX_EPOCH};

use rymembra_app::dispositivi;
use rymembra_app::library::TrackOrder;
use rymembra_app::library::condivisa::{self, FileCondiviso};
use rymembra_app::playback;
use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_domain::listen::Listen;
use rymembra_lontano::protocollo::{MASSIMA_PAGINA, MASSIMO_PEZZO};
use rymembra_lontano::{Domanda, Risposta};
use tauri::{AppHandle, Manager as _};

use super::filo::in_libreria;
use crate::nota;
use crate::stato::{Stato, adesso_ms};

/// Quanto una domanda aspetta la libreria: l'altro ha la sua scadenza, e una
/// libreria occupata da una scansione di solito si libera prima.
const ATTESA_LETTURA: Duration = Duration::from_secs(5);

/// Risponde a una domanda di `da`, un dispositivo associato.
///
/// # Errori
///
/// `lontano.fileCambiato` per un brano che non c'è più o è cambiato durante
/// la lettura; `lontano.irraggiungibile` con la libreria occupata;
/// `lontano.protocollo` per una domanda che qui non si serve.
pub fn rispondi(app: &AppHandle, da: &str, domanda: Domanda) -> Result<Risposta, AppError> {
    let pagina = |quanti: u32| i64::from(quanti.clamp(1, MASSIMA_PAGINA));
    let esito = match domanda {
        Domanda::Album { da, quanti } => dati(&leggi_la_libreria(app, |c| {
            condivisa::album(c, i64::from(da), pagina(quanti))
        })?),
        Domanda::AlbumArtista { artista } => dati(&leggi_la_libreria(app, |c| {
            condivisa::album_artista(c, &artista, i64::from(MASSIMA_PAGINA))
        })?),
        Domanda::Artisti { da, quanti } => dati(&leggi_la_libreria(app, |c| {
            condivisa::artisti(
                c,
                usize::try_from(da).unwrap_or(usize::MAX),
                usize::try_from(pagina(quanti)).unwrap_or(0),
            )
        })?),
        Domanda::Brani { ordine, da, quanti } => dati(&leggi_la_libreria(app, |c| {
            condivisa::brani(c, ordine_da(&ordine), i64::from(da), pagina(quanti))
        })?),
        Domanda::BraniAlbum { album } => dati(&leggi_la_libreria(app, |c| {
            condivisa::brani_album(c, &album)
        })?),
        Domanda::Cerca { testo, da, quanti } => dati(&leggi_la_libreria(app, |c| {
            condivisa::cerca(c, &testo, i64::from(da), pagina(quanti))
        })?),
        Domanda::CercaConteggio { testo } => dati(&leggi_la_libreria(app, |c| {
            condivisa::cerca_conteggio(c, &testo)
        })?),
        Domanda::Copertina { impronta } => copertina(app, &impronta),
        Domanda::Leggi { brano, da, quanti } => leggi(app, brano, da, quanti),
        Domanda::Ascoltato {
            brano,
            quando_ms,
            ascoltato_ms,
            chiave,
        } => ascoltato(app, da, brano, quando_ms, ascoltato_ms, &chiave),
        Domanda::Presenza | Domanda::Annuncio(_) | Domanda::Dimenticami | Domanda::Sconosciuta => {
            Err(AppError::new(ErrorCode::LontanoProtocollo)
                .with_cause("una domanda a cui risponde il nodo"))
        }
    };
    esito.inspect_err(|err| {
        // La causa resta qui: all'altro arriva il codice.
        if err.code().kind().domain() != rymembra_domain::errors::Domain::Lontano {
            nota!(
                "[lontano] una domanda non servita codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
        }
    })
}

/// I nomi degli ordinamenti, come li usa la finestra (`comandi::brani`).
fn ordine_da(nome: &str) -> TrackOrder {
    match nome {
        "recenti" => TrackOrder::RecentlyAdded,
        "ascoltati" => TrackOrder::MostPlayed,
        "titolo" => TrackOrder::Title,
        _ => TrackOrder::Shelf,
    }
}

fn dati<T: serde::Serialize>(valore: &T) -> Result<Risposta, AppError> {
    serde_json::to_value(valore)
        .map(Risposta::Dati)
        .map_err(|err| {
            AppError::new(ErrorCode::InternalUnexpected { detail: None })
                .with_cause(err.to_string())
        })
}

/// Una lettura della libreria: occupata oltre [`ATTESA_LETTURA`], l'altro
/// sente che questo dispositivo non risponde, e riprova.
fn leggi_la_libreria<T>(
    app: &AppHandle,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    in_libreria(app, ATTESA_LETTURA, azione).map_err(|err| {
        if err.code().kind() == ErrorCodeKind::DbLocked {
            AppError::new(ErrorCode::LontanoIrraggiungibile).with_cause("la libreria è occupata")
        } else {
            err
        }
    })
}

/// Una copertina, nella forma media (al più qualche centinaio di KB): quella
/// piena può pesare megabyte, e su un telefono non serve. `Fatto` se non
/// c'è.
fn copertina(app: &AppHandle, impronta: &str) -> Result<Risposta, AppError> {
    // È un nome di file nella cartella delle copertine: solo cifre.
    let buona = (16..=128).contains(&impronta.len())
        && impronta
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
    if !buona {
        return Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("un'impronta di copertina che non lo è"));
    }
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })?;
    match stato.copertine()?.media(impronta)? {
        Some(byte) => Ok(Risposta::Byte {
            totale: u64::try_from(byte.len()).unwrap_or(u64::MAX),
            versione: impronta.to_owned(),
            byte,
        }),
        None => Ok(Risposta::Fatto),
    }
}

/// Quanti byte e da quando, sul disco: la versione di un file aperto.
fn come_sta(file: &File) -> Result<(u64, i64), AppError> {
    let metadati = file.metadata().map_err(cambiato)?;
    let modificato = metadati
        .modified()
        .ok()
        .and_then(|quando| quando.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Ok((metadati.len(), modificato))
}

fn cambiato(err: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::LontanoFileCambiato).with_cause(err.to_string())
}

/// Un pezzo di un brano. La dimensione e la data si guardano prima e dopo la
/// lettura, come fa «Ascolta insieme» (`rymembra_insieme::servente`): un file
/// riscritto a metà non deve arrivare cucito con quello di prima.
fn leggi(app: &AppHandle, brano: i64, da: u64, quanti: u64) -> Result<Risposta, AppError> {
    let FileCondiviso { percorso, .. } =
        leggi_la_libreria(app, |c| condivisa::file_del_brano(c, brano))?
            .ok_or_else(|| cambiato("il brano non è più condiviso"))?;
    let mut file = File::open(&percorso).map_err(cambiato)?;
    let prima = come_sta(&file)?;
    let (totale, modificato) = prima;
    let versione = condivisa::versione(i64::try_from(totale).unwrap_or(i64::MAX), modificato);
    if da >= totale {
        return Ok(Risposta::Byte {
            byte: Vec::new(),
            totale,
            versione,
        });
    }
    let quanti = quanti.min(MASSIMO_PEZZO).min(totale.saturating_sub(da));
    file.seek(SeekFrom::Start(da)).map_err(cambiato)?;
    let mut byte = Vec::with_capacity(usize::try_from(quanti).unwrap_or(0));
    (&mut file)
        .take(quanti)
        .read_to_end(&mut byte)
        .map_err(cambiato)?;
    if come_sta(&file)? != prima {
        return Err(cambiato("il file è cambiato durante la lettura"));
    }
    Ok(Risposta::Byte {
        byte,
        totale,
        versione,
    })
}

/// Un ascolto fatto altrove di un brano di qui: si conta qui, una volta.
fn ascoltato(
    app: &AppHandle,
    da: &str,
    brano: i64,
    quando_ms: i64,
    ascoltato_ms: i64,
    chiave: &str,
) -> Result<Risposta, AppError> {
    // La chiave è di chi ascolta: con la sua chiave davanti, due dispositivi
    // non si pestano i piedi.
    let chiave = format!("{da}:{}", chiave.chars().take(64).collect::<String>());
    let ascolto = Listen {
        track_id: brano,
        started_at: quando_ms,
        listened_ms: u64::try_from(ascoltato_ms).unwrap_or(0),
        counts: true,
    };
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })?;
    let scritto = crate::stato::con_libreria_entro(&stato, ATTESA_LETTURA, |libreria| {
        let c = &mut libreria.connection;
        // Un brano che non c'è più: l'ascolto non ha dove contarsi, e chi
        // l'ha mandato lo può dimenticare.
        if condivisa::brano(c, brano)?.is_none() {
            return Ok(false);
        }
        if !dispositivi::ascolto_nuovo(c, &chiave, adesso_ms())? {
            return Ok(false);
        }
        let dispositivo = crate::nuvola::dispositivo(c)?;
        match playback::record_play(c, &ascolto, &dispositivo) {
            Ok(contato) => Ok(contato),
            Err(err) => {
                // Non contato: la chiave non lo deve dire contato.
                let _ = dispositivi::ascolto_da_ricontare(c, &chiave);
                Err(err)
            }
        }
    })
    .unwrap_or_else(|| {
        Err(AppError::new(ErrorCode::LontanoIrraggiungibile).with_cause("la libreria è occupata"))
    })?;
    if scritto {
        crate::nuvola::sporca(app);
        crate::scrobble::dopo_un_ascolto(app, &ascolto);
    }
    Ok(Risposta::Fatto)
}
