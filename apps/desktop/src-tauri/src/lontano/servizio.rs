//! Le domande degli altri dispositivi sulla libreria di questo: gli elenchi,
//! le copertine, i byte dei brani, gli ascolti da contare qui, i voti, il testo
//! della canzone e le sue correzioni.
//!
//! # I testi
//!
//! [`Domanda::Testo`] dà il testo di un brano **condiviso** già composto con la
//! catena intera di qui, senza scrivere e fuori dal lucchetto della libreria;
//! [`Domanda::TestoScrivi`] applica una correzione con le funzioni dei comandi
//! di sempre, solo nel database (nessun file accanto ai brani). Per un brano del
//! cloud, di catalogo, di un terzo dispositivo o che non c'è, tutte e due
//! rispondono `lontano.fileCambiato`. Vedi `rymembra_app::testi::condiviso`.
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
//!
//! Quel che manda l'altro non si prende sulla parola: l'inizio non è mai dopo
//! adesso, se l'ascolto conta lo dice la durata del brano di qui, e da ogni
//! dispositivo se ne contano al più [`ASCOLTI_ALL_ORA`] all'ora.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::time::{Duration, UNIX_EPOCH};

use crate::nucleo::Nucleo;
use rymembra_app::dispositivi;
use rymembra_app::library::TrackOrder;
use rymembra_app::library::condivisa::{self, FileCondiviso};
use rymembra_app::playback;
use rymembra_app::testi::{self, condiviso};
use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_domain::listen::{Listen, counts_as_play};
use rymembra_lontano::protocollo::{MASSIMA_PAGINA, MASSIMO_PEZZO};
use rymembra_lontano::{Domanda, Risposta};

use crate::nota;
use crate::stato::{Stato, adesso_ms};

/// Quanto una domanda aspetta la libreria: l'altro ha la sua scadenza.
///
/// Le letture in WAL non aspettano il lucchetto ([`leggi_da`]); questa attesa
/// vale per gli ascolti, che scrivono, per le letture con il giornale ripiegato,
/// e per SQLite stesso nelle rare volte in cui anche un lettore deve aspettare.
const ATTESA_LETTURA: Duration = Duration::from_secs(5);

/// Risponde a una domanda di `da`, un dispositivo associato.
///
/// # Errori
///
/// `lontano.fileCambiato` per un brano che non c'è più o è cambiato durante
/// la lettura; `lontano.irraggiungibile` con la libreria occupata;
/// `lontano.protocollo` per una domanda che qui non si serve.
pub fn rispondi(app: &Nucleo, da: &str, domanda: Domanda) -> Result<Risposta, AppError> {
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
        // Il catalogo per la libreria unica: sempre dal confine di fiducia di
        // `condivisa`, che non porta mai le righe specchio né il cloud.
        Domanda::CatalogoRiassunto => dati(&*leggi_la_libreria(app, |c| {
            condivisa::riassunto_catalogo(c)
        })?),
        Domanda::CatalogoSecchio { da_id, a_id } => dati(&leggi_la_libreria(app, |c| {
            condivisa::pagina_catalogo(c, da_id, a_id)
        })?),
        Domanda::Valuta {
            brano,
            liked,
            rating,
            quando_ms,
            liked_ms,
            rating_ms,
            chiave: _,
        } => valuta(
            app,
            brano,
            liked,
            rating,
            condivisa::OrologiValutazione {
                quando_ms,
                liked_ms,
                rating_ms,
            },
        ),
        // Il testo della canzone, e le sue correzioni: sempre da un brano
        // condiviso (`condiviso::brano_condiviso`), mai da un brano del cloud,
        // di catalogo o di un terzo dispositivo.
        Domanda::Testo { brano } => dai_il_testo(app, brano),
        Domanda::TestoScrivi {
            brano,
            cosa,
            quando_ms,
        } => scrivi_il_testo(app, da, brano, cosa, quando_ms),
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
    app: &Nucleo,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })?;
    leggi_da(&stato, azione)
}

/// Il cuore di [`leggi_la_libreria`], sullo stato: la cucitura delle prove.
///
/// Non chiede il lucchetto della libreria: legge da una connessione di sola
/// lettura sua ([`crate::stato::leggi_a_parte`]). Prima lo chiedeva, con
/// [`ATTESA_LETTURA`], e dopo una scansione lo perdeva per tutto il tempo dei
/// lavori di sottofondo, che lo prendono a turno: ogni «Sfoglia» da un altro
/// dispositivo rispondeva «non raggiungibile», per minuti — e su una libreria
/// vera, con ore di prima analisi, per ore (reperto 00-9).
fn leggi_da<T>(
    stato: &Stato,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    crate::stato::leggi_a_parte(stato, ATTESA_LETTURA, azione)
        .unwrap_or_else(|| {
            Err(AppError::new(ErrorCode::DbLocked)
                .with_cause("la libreria è occupata: il collegamento non l'aspetta"))
        })
        .map_err(|err| {
            if err.code().kind() == ErrorCodeKind::DbLocked {
                occupata(&err)
            } else {
                err
            }
        })
}

/// La libreria occupata oltre [`ATTESA_LETTURA`]: all'altro arriva
/// `lontano.irraggiungibile`, che lo fa riprovare, e qui una riga nel diario.
///
/// La riga serve perché [`rispondi`] non scrive gli errori del dominio
/// `lontano` — sono quel che si risponde, non guasti di qui — e di una libreria
/// che non risponde ai dispositivi collegati il diario non diceva niente. Può
/// succedere ancora con il giornale ripiegato su `TRUNCATE`, quando le letture
/// tornano al lucchetto, e per gli ascolti, che scrivono. Solo il codice e la
/// causa, che dice l'operazione e cosa ha risposto SQLite: niente di chi
/// chiede né di cosa.
fn occupata(err: &AppError) -> AppError {
    nota!(
        "[lontano] una domanda non servita: la libreria è occupata codice={} causa={}",
        err.code().kind().code(),
        err.cause().unwrap_or("—")
    );
    AppError::new(ErrorCode::LontanoIrraggiungibile).with_cause("la libreria è occupata")
}

/// Una copertina, nella forma media (al più qualche centinaio di KB): quella
/// piena può pesare megabyte, e su un telefono non serve. `Fatto` se non
/// c'è.
fn copertina(app: &Nucleo, impronta: &str) -> Result<Risposta, AppError> {
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
    la_copertina_di(&stato, impronta)
}

/// [`copertina`] per uno stato: solo le copertine di un brano che questa libreria
/// condivide (S1-9).
///
/// Lo store è uno solo per tutti i brani — anche quelli non condivisi, del cloud,
/// di catalogo — e chi sa l'impronta di un'immagine sapeva se è in libreria e la
/// scaricava, anche per un brano che non gli si è mai elencato. Qui si chiede
/// **prima** alla libreria, su una connessione a parte, e solo se l'impronta è di
/// un brano condiviso si apre lo store: per ogni altra la risposta è quella di una
/// che non c'è, e nessuna decodifica è a spese di chi risponde.
fn la_copertina_di(stato: &Stato, impronta: &str) -> Result<Risposta, AppError> {
    if !leggi_da(stato, |c| condivisa::copertina_condivisa(c, impronta))? {
        return Ok(Risposta::Fatto);
    }
    match stato.copertine()?.media(impronta)? {
        Some(byte) => Ok(Risposta::Byte {
            totale: u64::try_from(byte.len()).unwrap_or(u64::MAX),
            versione: impronta.to_owned(),
            byte,
        }),
        None => Ok(Risposta::Fatto),
    }
}

/// I brani di un album non superano una pagina, come ogni altra domanda d'elenco:
/// la cornice del protocollo è dimensionata su quella.
const _: () = assert!(condivisa::MASSIMI_BRANI_ALBUM == MASSIMA_PAGINA);

/// Una copertina servita pesa quanto lo store ne accetta da un altro dispositivo:
/// chi la riceve la legge al più per quanto la domanda ne chiede
/// (`Domanda::byte_attesi`), e chi la serve non ne manda di più.
const _: () = assert!(
    rymembra_lontano::protocollo::MASSIMA_COPERTINA
        == rymembra_app::covers::MASSIMO_ESTRANEA as u64
);

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
fn leggi(app: &Nucleo, brano: i64, da: u64, quanti: u64) -> Result<Risposta, AppError> {
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

/// Un mi piace o un voto dato altrove a un brano di qui: lo scrive il
/// proprietario, con la regola dell'orologio per campo
/// ([`condivisa::valuta`]). Se qualcosa è cambiato, la sincronia lo sa.
fn valuta(
    app: &Nucleo,
    brano: i64,
    liked: Option<bool>,
    rating: Option<i64>,
    orologi: condivisa::OrologiValutazione,
) -> Result<Risposta, AppError> {
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })?;
    let cambiato = crate::stato::con_libreria_entro(&stato, ATTESA_LETTURA, |libreria| {
        condivisa::valuta_con_orologi(
            &libreria.connection,
            brano,
            liked,
            rating,
            orologi,
            adesso_ms(),
        )
    })
    .unwrap_or_else(|| {
        Err(occupata(&AppError::new(ErrorCode::DbLocked).with_cause(
            "la libreria è occupata: la valutazione ricevuta non l'aspetta",
        )))
    })?;
    if cambiato {
        crate::nuvola::sporca(app);
    }
    Ok(Risposta::Fatto)
}

/// Il testo della canzone di un brano condiviso, già composto con la catena
/// intera di qui — il `.lrc` accanto al file, i tag, la tabella —, o `Fatto` se
/// non c'è niente da dire. Non cerca in rete: chi risponde non ha nessun
/// pannello aperto, e i testi che non ha li cerca chi li chiede.
fn dai_il_testo(app: &Nucleo, brano: i64) -> Result<Risposta, AppError> {
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })?;
    il_testo_di(&stato, brano)
}

/// Il cuore di [`dai_il_testo`], sullo stato: la cucitura delle prove.
///
/// Nei tre tempi di `testo_brano`, e **tutti fuori dal lucchetto della
/// libreria**: la riga si legge a parte (come ogni altra domanda, vedi
/// [`leggi_da`]), il disco — che può essere una cartella di rete — non tiene
/// niente, e la composizione non scrive la cache. Dopo una scansione che tiene
/// il lucchetto per ore (reperto 00-9) il testo risponde lo stesso.
fn il_testo_di(stato: &Stato, brano: i64) -> Result<Risposta, AppError> {
    let Some((dato, riga)) = leggi_da(stato, |c| {
        Ok(condiviso::brano_condiviso(c, brano)?.map(|dato| {
            let riga = testi::leggi_riga(c, &dato.track_key);
            (dato, riga)
        }))
    })?
    else {
        return Err(AppError::new(ErrorCode::LontanoFileCambiato)
            .with_cause("il brano non è più condiviso"));
    };
    let disco = testi::leggi_dal_disco(&dato, riga.as_ref());
    match condiviso::testo_da_dare(&dato, riga, disco) {
        Some(testo) => dati(&testo),
        None => Ok(Risposta::Fatto),
    }
}

/// Una correzione al testo di un brano condiviso, arrivata da un altro
/// dispositivo `da`: la scrive qui con la logica dei comandi di sempre. Solo la
/// libreria, mai un file accanto al brano. `quando_ms` è l'orologio di chi
/// l'ha decisa, zero se una build di prima non lo manda: vedi
/// [`condiviso::scrivi_dal_dispositivo`].
fn scrivi_il_testo(
    app: &Nucleo,
    da: &str,
    brano: i64,
    cosa: rymembra_lontano::protocollo::ScritturaTesto,
    quando_ms: i64,
) -> Result<Risposta, AppError> {
    let Some(scrittura) = super::testo::dal_protocollo(cosa) else {
        return Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("una correzione di una versione più nuova"));
    };
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })?;
    let scrivente = condiviso::Scrivente {
        dispositivo: da,
        quando_ms,
        adesso_ms: adesso_ms(),
    };
    scrivi_un_testo(&stato, brano, &scrittura, &scrivente)?;
    // Il testo sta nel profilo di chi lo possiede: come ogni suo gesto.
    crate::nuvola::sporca(app);
    Ok(Risposta::Fatto)
}

/// Il cuore di [`scrivi_il_testo`], sullo stato: la cucitura delle prove.
fn scrivi_un_testo(
    stato: &Stato,
    brano: i64,
    scrittura: &condiviso::Scrittura,
    scrivente: &condiviso::Scrivente<'_>,
) -> Result<(), AppError> {
    crate::stato::con_libreria_entro(stato, ATTESA_LETTURA, |libreria| {
        condiviso::scrivi_dal_dispositivo(&libreria.connection, brano, scrittura, scrivente)
    })
    .unwrap_or_else(|| {
        Err(occupata(&AppError::new(ErrorCode::DbLocked).with_cause(
            "la libreria è occupata: la correzione ricevuta non l'aspetta",
        )))
    })
}

/// Al più tanti ascolti ricevuti all'ora da ogni dispositivo: contro un
/// dispositivo collegato che ne inventa (ogni chiave nuova varrebbe un
/// ascolto, e uno scrobble sull'account di chi ascolta qui).
///
/// Un'ora di musica vera ne fa al più qualche decina; il resto del margine è
/// per chi torna dopo giorni senza rete con gli ascolti in fila. Oltre, chi
/// li manda sente `lontano.irraggiungibile`, li tiene e riprova al giro dopo:
/// una fila lunga arriva in qualche ora, e non si perde.
const ASCOLTI_ALL_ORA: usize = 120;

/// L'ora di [`ASCOLTI_ALL_ORA`].
const UN_ORA_MS: i64 = 60 * 60 * 1000;

/// Un ascolto fatto altrove di un brano di qui, come arriva.
#[derive(Debug, Clone, Copy)]
struct Arrivato<'a> {
    brano: i64,
    quando_ms: i64,
    ascoltato_ms: i64,
    chiave: &'a str,
}

/// Un ascolto fatto altrove di un brano di qui: si conta qui, una volta.
fn ascoltato(
    app: &Nucleo,
    da: &str,
    brano: i64,
    quando_ms: i64,
    ascoltato_ms: i64,
    chiave: &str,
) -> Result<Risposta, AppError> {
    let arrivato = Arrivato {
        brano,
        quando_ms,
        ascoltato_ms,
        chiave,
    };
    let stato = app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("lontano".to_owned()),
        })
    })?;
    let contato = crate::stato::con_libreria_entro(&stato, ATTESA_LETTURA, |libreria| {
        let c = &mut libreria.connection;
        let dispositivo = crate::nuvola::dispositivo(c)?;
        conta_l_ascolto(c, da, &arrivato, &dispositivo, adesso_ms())
    })
    .unwrap_or_else(|| {
        Err(occupata(&AppError::new(ErrorCode::DbLocked).with_cause(
            "la libreria è occupata: l'ascolto ricevuto non l'aspetta",
        )))
    })?;
    if let Some(ascolto) = contato {
        crate::nuvola::sporca(app);
        crate::scrobble::dopo_un_ascolto(app, &ascolto);
    }
    Ok(Risposta::Fatto)
}

/// Il cuore di [`ascoltato`], sulla libreria: l'ascolto contato (da mandare
/// allo scrobble), o `None` se non c'era niente da contare. `dispositivo` è
/// l'identificativo di questa macchina, a cui l'ascolto s'intesta.
fn conta_l_ascolto(
    c: &mut rusqlite::Connection,
    da: &str,
    arrivato: &Arrivato<'_>,
    dispositivo: &str,
    adesso: i64,
) -> Result<Option<Listen>, AppError> {
    // La chiave è di chi ascolta: con la sua chiave davanti, due dispositivi
    // non si pestano i piedi.
    let chiave = format!(
        "{da}:{}",
        arrivato.chiave.chars().take(64).collect::<String>()
    );
    // Un brano che non c'è più: l'ascolto non ha dove contarsi, e chi
    // l'ha mandato lo può dimenticare.
    let Some(brano) = condivisa::brano(c, arrivato.brano)? else {
        return Ok(None);
    };
    // Il tetto prima di segnare la chiave: un ascolto fermato qui resta in
    // fila dall'altra parte e torna, e non deve trovarsi già «ricevuto».
    if dispositivi::ricevuti_da(c, da, adesso.saturating_sub(UN_ORA_MS))? >= ASCOLTI_ALL_ORA {
        return Err(AppError::new(ErrorCode::LontanoIrraggiungibile)
            .with_cause("troppi ascolti da questo dispositivo nell'ultima ora"));
    }
    if !dispositivi::ascolto_nuovo(c, &chiave, adesso)? {
        return Ok(None);
    }
    let ascoltato = u64::try_from(arrivato.ascoltato_ms).unwrap_or(0);
    // Un ascolto non comincia dopo adesso: un orologio avanti lo metterebbe nel
    // futuro, e `record_play` non lascia tornare indietro «ultimo ascolto» e
    // l'orologio delle statistiche.
    let inizio = arrivato.quando_ms.min(adesso);
    // Lo stesso brano con lo stesso inizio è già in cronologia: lo ha contato un
    // altro invio, con un'altra chiave. Non è un guasto, e **non** si riscrive:
    // `record_play` fallirebbe sull'indice unico dopo aver mosso `tracks` e i
    // contatori, e la chiave si toglierebbe — il conto orario resterebbe a uno, e
    // un dispositivo che ripete lo stesso ascolto con chiavi nuove avrebbe tre
    // scritture e una transazione annullata a ogni tentativo, sotto il lucchetto
    // della libreria, senza freno (reperto S1-6). La chiave resta segnata: conta.
    if playback::gia_in_cronologia(c, arrivato.brano, inizio)? {
        return Ok(None);
    }
    let ascolto = Listen {
        track_id: arrivato.brano,
        started_at: inizio,
        listened_ms: ascoltato,
        // Conta con la regola e la durata di qui, non sulla parola dell'altro.
        counts: counts_as_play(ascoltato, u64::try_from(brano.duration_ms).unwrap_or(0)),
    };
    match playback::record_play(c, &ascolto, dispositivo) {
        Ok(true) => Ok(Some(ascolto)),
        Ok(false) => Ok(None),
        Err(err) => {
            // Non contato: la chiave non lo deve dire contato.
            let _ = dispositivi::ascolto_da_ricontare(c, &chiave);
            Err(err)
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Martedì 10 novembre 2026, più o meno.
    const ADESSO: i64 = 1_794_312_000_000;

    fn da() -> String {
        "ab".repeat(32)
    }

    /// Chi scrive le correzioni di prova: `da`, senza orologio, adesso.
    fn scrivente() -> condiviso::Scrivente<'static> {
        condiviso::Scrivente {
            dispositivo: "abababababababababababababababababababababababababababababababab",
            quando_ms: 0,
            adesso_ms: ADESSO,
        }
    }

    /// Una libreria con un brano condiviso di tre minuti, il numero 1.
    fn libreria() -> rusqlite::Connection {
        let c = rymembra_app::db::open_in_memory()
            .expect("database in memoria")
            .connection;
        // Il brano sta sotto una cartella sorvegliata: senza, niente si condivide.
        rymembra_app::settings::write_json(&c, rymembra_app::settings::CHIAVE_CARTELLE, &["C:/m"])
            .expect("cartelle sorvegliate");
        c.execute(
            "INSERT INTO tracks
                 (id, path, track_key, title, artist, album, duration_ms,
                  file_size, date_added, date_modified)
             VALUES (1, 'C:/m/1.mp3', 'k1', 'T', 'A', 'Al', 180000, 1000, 0, 0)",
            [],
        )
        .expect("brano");
        c
    }

    fn arrivato(chiave: &str, quando_ms: i64) -> Arrivato<'_> {
        Arrivato {
            brano: 1,
            quando_ms,
            ascoltato_ms: 120_000,
            chiave,
        }
    }

    fn orologi(c: &rusqlite::Connection) -> (Option<i64>, Option<i64>, i64) {
        c.query_row(
            "SELECT last_played_at, stats_updated_at, play_count FROM tracks WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("orologi")
    }

    #[test]
    fn un_ascolto_dal_futuro_si_conta_adesso() {
        // Un telefono con l'orologio avanti di un anno: l'ascolto conta, ma
        // non fissa «ultimo ascolto» e l'orologio delle statistiche nel
        // futuro, dove nessun ascolto vero li raggiungerebbe più.
        let mut c = libreria();
        let fra_un_anno = ADESSO + 365 * 24 * 3600 * 1000;
        let contato = conta_l_ascolto(&mut c, &da(), &arrivato("k", fra_un_anno), "qui", ADESSO)
            .expect("contato")
            .expect("un ascolto");
        assert_eq!(contato.started_at, ADESSO);
        assert_eq!(orologi(&c), (Some(ADESSO), Some(ADESSO), 1));
    }

    #[test]
    fn un_ascolto_troppo_breve_non_conta() {
        // Chi lo manda dice «conta»; qui si guarda la durata del brano di qui.
        let mut c = libreria();
        let mut breve = arrivato("k", ADESSO - 1000);
        breve.ascoltato_ms = 1;
        assert_eq!(
            conta_l_ascolto(&mut c, &da(), &breve, "qui", ADESSO).expect("risposto"),
            None
        );
        assert_eq!(orologi(&c).2, 0);
    }

    #[test]
    fn un_dispositivo_non_conta_piu_di_tanti_ascolti_all_ora() {
        let mut c = libreria();
        let chiavi: Vec<String> = (0..=ASCOLTI_ALL_ORA).map(|n| format!("k{n}")).collect();
        // Ognuno col suo inizio: la cronologia non ne vuole due nello stesso
        // millisecondo per lo stesso brano.
        let inizio = |n: usize| ADESSO - 1000 * (i64::try_from(n).unwrap() + 1);
        for (n, chiave) in chiavi.iter().take(ASCOLTI_ALL_ORA).enumerate() {
            conta_l_ascolto(&mut c, &da(), &arrivato(chiave, inizio(n)), "qui", ADESSO)
                .expect("contato")
                .expect("un ascolto");
        }
        let oltre = arrivato(&chiavi[ASCOLTI_ALL_ORA], inizio(ASCOLTI_ALL_ORA));
        let err = conta_l_ascolto(&mut c, &da(), &oltre, "qui", ADESSO).unwrap_err();
        assert_eq!(err.code().kind(), ErrorCodeKind::LontanoIrraggiungibile);
        assert_eq!(orologi(&c).2, i64::try_from(ASCOLTI_ALL_ORA).unwrap());
        // Un altro dispositivo ha il suo conto.
        let altro = "cd".repeat(32);
        assert!(
            conta_l_ascolto(&mut c, &altro, &arrivato("k0", ADESSO - 500), "qui", ADESSO)
                .expect("contato")
                .is_some()
        );
        // Chi è stato fermato lo tiene in fila e lo rimanda: passata l'ora
        // conta, perché il rifiuto non l'aveva segnato come ricevuto.
        let dopo = ADESSO + UN_ORA_MS + 1;
        assert!(
            conta_l_ascolto(&mut c, &da(), &oltre, "qui", dopo)
                .expect("contato")
                .is_some()
        );
    }

    #[test]
    fn un_ascolto_con_lo_stesso_inizio_conta_contro_il_tetto_e_non_riscrive() {
        // Reperto S1-6: un ascolto per lo stesso brano e lo stesso inizio ma con una
        // chiave nuova superava `ascolto_nuovo`, apriva una transazione, falliva sull'indice
        // unico della cronologia e la sua chiave si toglieva: il conto
        // orario restava a uno, e ogni tentativo costava tre scritture e una
        // transazione annullata, sotto il lucchetto della libreria, senza freno.
        let mut c = libreria();
        let inizio = ADESSO - 5000;
        conta_l_ascolto(&mut c, &da(), &arrivato("k0", inizio), "qui", ADESSO)
            .expect("contato")
            .expect("un ascolto");
        // Gli altri invii dello stesso ascolto non sono un guasto: sono già
        // contati. E contano ognuno contro il tetto.
        for n in 1..ASCOLTI_ALL_ORA {
            let chiave = format!("k{n}");
            let esito = conta_l_ascolto(&mut c, &da(), &arrivato(&chiave, inizio), "qui", ADESSO);
            assert!(matches!(esito, Ok(None)), "il duplicato n. {n}: {esito:?}");
        }
        assert_eq!(orologi(&c).2, 1, "contato una volta sola");
        // Al tetto, e senza scrivere niente: né la chiave, né una transazione.
        let scritture = c.total_changes();
        let err =
            conta_l_ascolto(&mut c, &da(), &arrivato("troppi", inizio), "qui", ADESSO).unwrap_err();
        assert_eq!(err.code().kind(), ErrorCodeKind::LontanoIrraggiungibile);
        assert_eq!(c.total_changes(), scritture, "oltre il tetto non si scrive");
    }

    /// Una riga del catalogo con ogni testo al massimo di quel che chi riceve
    /// accetta, e quasi incomprimibile.
    fn riga_lunga(n: i64, seme: &mut u64) -> condivisa::BranoCatalogo {
        use rymembra_app::specchio::convalida::{
            MAX_CHIAVE, MAX_CODEC, MAX_GENERE, MAX_ID_MB, MAX_NOME,
        };
        let mut a_caso = |lunghezza: usize| -> String {
            (0..lunghezza)
                .map(|_| {
                    *seme ^= *seme << 13;
                    *seme ^= *seme >> 7;
                    *seme ^= *seme << 17;
                    let cifra = u8::try_from(*seme % 36).unwrap();
                    char::from(if cifra < 10 {
                        b'0' + cifra
                    } else {
                        b'a' + cifra - 10
                    })
                })
                .collect()
        };
        let uuid = |c: &mut dyn FnMut(usize) -> String| {
            let v = c(MAX_ID_MB);
            format!(
                "{}-{}-{}-{}-{}",
                &v[..8],
                &v[9..13],
                &v[14..18],
                &v[19..23],
                &v[24..]
            )
        };
        condivisa::BranoCatalogo {
            id: n,
            title: a_caso(MAX_NOME),
            artist: a_caso(MAX_NOME),
            album: a_caso(MAX_NOME),
            album_artist: Some(a_caso(MAX_NOME)),
            genre: Some(a_caso(MAX_GENERE)),
            album_key: Some(a_caso(32)),
            track_number: Some(n),
            disc_number: Some(1),
            year: Some(2001),
            duration_ms: 1_234_567,
            cover_art_hash: Some(a_caso(64)),
            play_count: 123_456,
            liked: true,
            rating: 5,
            liked_at: Some(1_794_312_000_123),
            rating_at: Some(1_794_312_000_456),
            date_added: 1_794_312_000_789,
            last_played_at: Some(1_794_312_000_999),
            stats_updated_at: 1_794_312_000_111,
            bitrate: Some(1_411),
            sample_rate: Some(192_000),
            channels: Some(2),
            codec: Some(a_caso(MAX_CODEC)),
            track_key: a_caso(MAX_CHIAVE),
            content_key: Some(a_caso(MAX_CHIAVE)),
            mb_recording_id: Some(uuid(&mut a_caso)),
            mb_release_group_id: Some(uuid(&mut a_caso)),
            mb_release_id: Some(uuid(&mut a_caso)),
            estensione: "flac".to_owned(),
            dimensione: 123_456_789,
            versione: "123456789-1794312000123".to_owned(),
            segmento: Some(condivisa::Segmento {
                inizio_cd: 1_234_567,
                fine_cd: Some(2_345_678),
            }),
        }
    }

    #[test]
    fn cinquecento_righe_lunghe_del_catalogo_stanno_nella_cornice() {
        // Con ogni testo al tetto, e quasi incomprimibili: se qui sta, sta il
        // catalogo di chiunque. (§4.3: «con 500 righe lunghe va verificato il
        // tetto della cornice».)
        let mut seme = 0x9E37_79B9_7F4A_7C15_u64;
        let brani: Vec<condivisa::BranoCatalogo> = (0..i64::from(MASSIMA_PAGINA))
            .map(|n| riga_lunga(n + 1, &mut seme))
            .collect();
        let pagina = condivisa::PaginaCatalogo {
            generazione: i64::MAX,
            da_id: 0,
            a_id: 499,
            brani,
        };
        let dati = serde_json::to_value(&pagina).expect("la pagina si scrive");
        let aperti = serde_json::to_vec(&dati).unwrap().len();
        let peso = rymembra_lontano::protocollo::peso_della_risposta(&dati)
            .expect("la pagina lunga parte");
        // Il tetto della cornice sul filo, compressa.
        assert!(
            peso <= rymembra_lontano::protocollo::MASSIMA_CORNICE + 4,
            "{peso} byte sul filo per {aperti} aperti"
        );
        eprintln!("cinquecento righe al tetto: {aperti} byte aperti, {peso} sul filo");
        // E si legge uguale.
        let riletta: condivisa::PaginaCatalogo = serde_json::from_value(dati).unwrap();
        assert_eq!(riletta, pagina);
    }

    /// Un brano condiviso, scritto nella libreria dello stato.
    fn metti_il_brano(c: &rusqlite::Connection, id: i64) -> Result<(), AppError> {
        // Sotto una cartella sorvegliata: senza, niente si condivide.
        rymembra_app::settings::write_json(c, rymembra_app::settings::CHIAVE_CARTELLE, &["C:/m"])?;
        c.execute(
            "INSERT INTO tracks
                 (id, path, track_key, title, artist, album, duration_ms,
                  file_size, date_added, date_modified)
             VALUES (?1, 'C:/m/' || ?1 || '.mp3', 'k' || ?1, 'T', 'A', 'Al', 180000,
                     1000, 0, 0)",
            [id],
        )
        .map(|_| ())
        .map_err(|err| rymembra_app::db::codice_da_sqlite("brano di prova", &err))
    }

    #[test]
    fn una_scrittura_lunga_qui_non_ferma_chi_sfoglia_da_lontano() {
        // Reperto 00-9: dopo una scansione i lavori di sottofondo prendono la
        // libreria a turno — su una libreria vera, per ore — e ogni domanda di
        // un dispositivo collegato rispondeva `lontano.irraggiungibile` dopo
        // `ATTESA_LETTURA`. Qui il lavoro lento è simulato: tiene il lucchetto
        // e una transazione di scrittura aperta finché la domanda non ha
        // risposto, o ben oltre la sua attesa.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let stato = &stato;
        let giornale =
            crate::stato::con_libreria(stato, |l| Ok(l.giornale.clone())).expect("libreria aperta");
        assert_eq!(giornale, "wal", "senza WAL questa prova non prova niente");
        crate::stato::con_libreria(stato, |l| metti_il_brano(&l.connection, 1))
            .expect("il brano di prima");

        let (preso, lavoro_partito) = std::sync::mpsc::channel::<()>();
        let (risposto, domanda_finita) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|ambito| {
            let lavoro = ambito.spawn(move || {
                crate::stato::con_libreria(stato, |l| {
                    let tx = l
                        .connection
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                        .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
                    metti_il_brano(&tx, 2)?;
                    let _ = preso.send(());
                    let _ = domanda_finita.recv_timeout(ATTESA_LETTURA * 3);
                    tx.commit()
                        .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))
                })
            });
            lavoro_partito
                .recv()
                .expect("il lavoro ha preso la libreria");

            let inizio = std::time::Instant::now();
            let esito = leggi_da(stato, |c| condivisa::brani(c, TrackOrder::Title, 0, 20));
            let tempo = inizio.elapsed();
            let _ = risposto.send(());
            lavoro
                .join()
                .expect("il lavoro non cade")
                .expect("il lavoro scrive");

            let brani = esito.expect("la domanda ha risposto");
            // Quel che era scritto prima, e non la scrittura ancora aperta.
            assert_eq!(brani.iter().map(|b| b.id).collect::<Vec<_>>(), vec![1]);
            assert!(
                tempo < ATTESA_LETTURA,
                "la domanda ha aspettato il lavoro: {tempo:?}"
            );
        });
        // Finito il lavoro, la domanda dopo vede anche il suo brano.
        let dopo = leggi_da(stato, |c| condivisa::brani(c, TrackOrder::Title, 0, 20))
            .expect("la domanda dopo");
        assert_eq!(dopo.len(), 2);
    }

    fn codice_di(esito: Result<Risposta, AppError>) -> String {
        esito
            .expect_err("un errore")
            .code()
            .kind()
            .code()
            .to_owned()
    }

    #[test]
    fn il_testo_si_da_solo_di_un_brano_condiviso() {
        // Un testo c'è per ognuno di questi brani, e solo uno è dato: quello
        // che la libreria condivide. Gli altri rispondono come un brano che non
        // c'è, e di quel che non c'è non si dice altro.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let stato = &stato;
        let chiave_dello_specchio = "ab".repeat(32);
        crate::stato::con_libreria(stato, |l| {
            metti_il_brano(&l.connection, 1)?;
            let c = &l.connection;
            c.execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                     date_added, date_modified)
                 VALUES (2, 'nuvola://drive/2.mp3', 'k2', 'T', 'A', 'B', 1, 0, 0)",
                [],
            )
            .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
            c.execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                     date_added, date_modified, source)
                 VALUES (3, ?1, 'k3', 'T', 'A', 'B', 1, 0, 0, 'lontano')",
                [rymembra_app::specchio::percorso(&chiave_dello_specchio, 9)],
            )
            .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
            for k in ["k1", "k2", "k3"] {
                c.execute(
                    "INSERT INTO lyrics (track_key, synced, source, updated_at)
                     VALUES (?1, '[00:01.00]ciao\n', 'lrclib', 1)",
                    [k],
                )
                .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
            }
            Ok(())
        })
        .expect("la libreria di prova");

        let Ok(Risposta::Dati(testo)) = il_testo_di(stato, 1) else {
            panic!("il brano condiviso dà il suo testo");
        };
        assert_eq!(testo["fonte"], "lrclib");
        assert_eq!(testo["sincronizzato"], "[00:01.00]ciao\n");
        for (id, cosa) in [(2, "del cloud"), (3, "di un terzo"), (99, "che non c'è")] {
            assert_eq!(
                codice_di(il_testo_di(stato, id)),
                "lontano.fileCambiato",
                "un brano {cosa}"
            );
            // E nemmeno si corregge.
            assert_eq!(
                scrivi_un_testo(stato, id, &condiviso::Scrittura::Scarto(50), &scrivente(),)
                    .expect_err("rifiutata")
                    .code()
                    .kind()
                    .code(),
                "lontano.fileCambiato",
                "un brano {cosa}"
            );
        }
        // Un brano condiviso, invece, si corregge, e il testo che si dà dopo è
        // quello nuovo.
        scrivi_un_testo(stato, 1, &condiviso::Scrittura::Scarto(-120), &scrivente())
            .expect("scritta");
        let Ok(Risposta::Dati(dopo)) = il_testo_di(stato, 1) else {
            panic!("il testo di dopo");
        };
        assert_eq!(dopo["scartoMs"], -120);
    }

    /// S1-9. Lo store delle copertine è uno solo: una copertina di un brano del
    /// cloud c'è, e chi ne conosce l'impronta la riceveva. Si serve solo quella
    /// di un brano condiviso; per ogni altra si risponde come per una che non c'è.
    #[test]
    fn una_copertina_si_serve_solo_se_e_di_un_brano_condiviso() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let (condivisa_h, cloud_h) = ("1".repeat(64), "2".repeat(64));
        crate::stato::con_libreria(&stato, |l| {
            let prova = |err: rusqlite::Error| rymembra_app::db::codice_da_sqlite("prova", &err);
            metti_il_brano(&l.connection, 1)?;
            for hash in [&condivisa_h, &cloud_h] {
                l.connection
                    .execute(
                        "INSERT INTO cover_art (hash, mime_type, width, height, byte_size, source,
                                                created_at)
                         VALUES (?1, 'image/jpeg', 1, 1, 1, 'provider', 1)",
                        [hash],
                    )
                    .map_err(prova)?;
            }
            l.connection
                .execute(
                    "UPDATE tracks SET cover_art_hash = ?1 WHERE id = 1",
                    [&condivisa_h],
                )
                .map_err(prova)?;
            l.connection
                .execute(
                    "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                         date_added, date_modified, cover_art_hash)
                     VALUES (2, 'nuvola://drive/2.mp3', 'k2', 'T', 'A', 'B', 1, 0, 0, ?1)",
                    [&cloud_h],
                )
                .map_err(prova)?;
            Ok(())
        })
        .expect("la libreria di prova");
        // Tutte e due le copertine stanno nello store, nella forma media.
        let store = stato.copertine().expect("lo store");
        for hash in [&condivisa_h, &cloud_h] {
            let percorso = store.percorso_media(hash);
            std::fs::create_dir_all(percorso.parent().expect("una cartella")).expect("cartella");
            std::fs::write(percorso, b"jpeg finto").expect("copertina");
        }

        let Ok(Risposta::Byte { byte, .. }) = la_copertina_di(&stato, &condivisa_h) else {
            panic!("la copertina di un brano condiviso si serve");
        };
        assert_eq!(byte, b"jpeg finto");
        assert_eq!(
            la_copertina_di(&stato, &cloud_h).expect("risposta"),
            Risposta::Fatto,
            "quella di un brano del cloud no: come se non ci fosse"
        );
        assert_eq!(
            la_copertina_di(&stato, &"3".repeat(64)).expect("risposta"),
            Risposta::Fatto,
            "e una che non c'è risponde allo stesso modo"
        );
    }

    #[test]
    fn un_brano_senza_testo_risponde_fatto() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        crate::stato::con_libreria(&stato, |l| metti_il_brano(&l.connection, 1)).expect("brano");
        assert_eq!(il_testo_di(&stato, 1).expect("risposta"), Risposta::Fatto);
    }

    #[test]
    fn il_testo_non_aspetta_il_lucchetto_che_tiene_una_scrittura_lunga() {
        // Come la domanda di Sfoglia (reperto 00-9): dopo una scansione i lavori
        // di sottofondo tengono la libreria a turno, e il testo di un brano —
        // che apre anche il disco — deve rispondere lo stesso, perché sta
        // fuori dal lucchetto in tutti e tre i tempi.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let stato = &stato;
        let giornale =
            crate::stato::con_libreria(stato, |l| Ok(l.giornale.clone())).expect("libreria aperta");
        assert_eq!(giornale, "wal", "senza WAL questa prova non prova niente");
        crate::stato::con_libreria(stato, |l| {
            metti_il_brano(&l.connection, 1)?;
            l.connection
                .execute(
                    "INSERT INTO lyrics (track_key, synced, source, updated_at)
                     VALUES ('k1', '[00:01.00]ciao\n', 'lrclib', 1)",
                    [],
                )
                .map(|_| ())
                .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))
        })
        .expect("il brano di prima");

        let (preso, lavoro_partito) = std::sync::mpsc::channel::<()>();
        let (risposto, domanda_finita) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|ambito| {
            let lavoro = ambito.spawn(move || {
                crate::stato::con_libreria(stato, |l| {
                    let tx = l
                        .connection
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                        .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))?;
                    metti_il_brano(&tx, 2)?;
                    let _ = preso.send(());
                    let _ = domanda_finita.recv_timeout(ATTESA_LETTURA * 3);
                    tx.commit()
                        .map_err(|err| rymembra_app::db::codice_da_sqlite("prova", &err))
                })
            });
            lavoro_partito
                .recv()
                .expect("il lavoro ha preso la libreria");
            let inizio = std::time::Instant::now();
            let esito = il_testo_di(stato, 1);
            let tempo = inizio.elapsed();
            let _ = risposto.send(());
            lavoro
                .join()
                .expect("il lavoro non cade")
                .expect("il lavoro scrive");
            assert!(
                matches!(esito, Ok(Risposta::Dati(_))),
                "il testo ha risposto: {esito:?}"
            );
            assert!(
                tempo < ATTESA_LETTURA,
                "il testo ha aspettato il lavoro: {tempo:?}"
            );
        });
    }

    #[test]
    fn un_pari_che_trova_la_libreria_occupata_cento_volte_non_riempie_il_diario() {
        // S1-5, il resto: `occupata` scrive una riga a ogni domanda che cade per
        // la libreria occupata, e con una scansione in corso un pari ne fa
        // cadere otto ogni cinque secondi. Il tetto sta nel diario, non qui: la
        // prova passa dal produttore vero, e conta quel che arriva nel file.
        let diario = crate::diario::a_diario_aperto(|| {
            for _ in 0..100 {
                let _ = occupata(
                    &AppError::new(ErrorCode::DbLocked)
                        .with_cause("la libreria è occupata: il collegamento non l'aspetta"),
                );
            }
        });
        let scritte = diario
            .lines()
            .filter(|riga| riga.contains("una domanda non servita"))
            .count();
        assert!(
            diario.contains("sessione aperta"),
            "la prova deve vedere il diario, o non prova niente: {diario}"
        );
        assert!(
            scritte <= 5,
            "100 domande cadute per la stessa causa in un minuto: {scritte} righe nel diario"
        );
    }
}
