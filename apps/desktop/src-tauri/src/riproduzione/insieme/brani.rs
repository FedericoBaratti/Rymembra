//! I brani di questo PC come voci di sessione: le schede dalla libreria, e i file da mandare
//! in rete letti dal database e dal disco, con una scadenza sola. Il lettore qui non si tocca.

use std::path::{Path, PathBuf};

use rymembra_app::library::TrackSummary;
use rymembra_app::playback::{Collocazione, SchedaSorgente};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_insieme::messaggi::MASSIMO_VOCI;

use crate::nota;
use crate::riproduzione::ospiti::{self, Apertura, SchedaVoce, Voce};
use crate::stato::con_libreria;

/// Un brano di **questo** PC, con quel che serve per mandarlo in rete come
/// voce di sessione: chi ospita per i suoi, chi segue (fase 2) per quelli che
/// aggiunge.
#[derive(Debug, Clone)]
pub struct BranoDiQuesto {
    /// La riga della libreria.
    pub track_id: i64,
    /// Quel che serve a disegnarla.
    pub scheda: SchedaVoce,
    /// Il file, se è un file. `None` per un brano di catalogo.
    pub file: Option<FileDellaVoce>,
    /// Il riferimento, se è un brano di catalogo.
    ///
    /// La forma è `<fonte>:<indirizzo>`, con il nome stabile della fonte
    /// (`internet-archive:https://archive.org/download/…`): la scrive e la
    /// rilegge `riproduzione::flusso`, e chi la riceve la ripassa dalla propria
    /// lista dei cataloghi ammessi. Per la sessione è opaca: va sul filo così
    /// com'è.
    pub riferimento: Option<String>,
    /// L'impronta della copertina nella libreria, se ce n'è una.
    pub copertina: Option<String>,
}

/// Perché una voce non si può mandare: la risposta di [`voci_di_questo`].
///
/// Quattro casi perché sono quattro decisioni diverse per chi la manda: le
/// prime tre non cambiano riprovando, l'ultima sì.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mancanza {
    /// La voce non è un brano della libreria di questo PC: è di un altro, o
    /// è un brano di catalogo di qualcun altro, o il registro non la conosce.
    NonDiQuesto,
    /// La riga non c'è più in libreria: tolta o cancellata da quando è
    /// entrata in coda.
    RigaSparita,
    /// Il file non c'è più dove la riga dice (`NotFound`, che comprende la
    /// cartella sparita): riprovare non serve finché qualcuno non lo rimette.
    FileAssente,
    /// Il disco non ha risposto come si deve — un altro errore, la cartella di
    /// rete che non risponde — o la scadenza del blocco è arrivata prima di
    /// questa voce: riprovare più tardi può bastare.
    Irraggiungibile,
}

/// Il file di un [`BranoDiQuesto`].
#[derive(Debug, Clone)]
pub struct FileDellaVoce {
    /// Il percorso **del file da aprire**: per un brano d'immagine CD
    /// l'immagine, non il `…\Album.cue|03` che lo nomina in tabella.
    pub percorso: PathBuf,
    /// Quanti byte.
    pub dimensione: u64,
    /// L'ultima modifica, ms dall'epoca Unix.
    pub modificato_ms: i64,
    /// L'estensione.
    pub estensione: Option<String>,
    /// Il tratto, per un brano d'immagine CD: inizio e fine in ms.
    pub segmento: Option<(u64, Option<u64>)>,
}

/// I brani di libreria di questo PC diventano voci di sessione: restituisce le
/// loro maniglie, nell'ordine. È la strada di `Instradato::ConVoci`.
///
/// Al più [`MASSIMO_VOCI`]: oltre, la sessione non le potrebbe mandare.
/// Quelli che la libreria non ha più si saltano.
///
/// # Errori
///
/// Quelli della libreria.
pub fn voci_da_libreria(app: &tauri::AppHandle, ids: &[i64]) -> Result<Vec<i64>, AppError> {
    Ok(voci_allineate(app, ids)?.into_iter().flatten().collect())
}

/// Come [`voci_da_libreria`], ma allineate agli id chiesti: `None` dove la
/// riga non c'è. Serve a chi deve ritrovare la voce di un posto preciso — il
/// brano corrente, entrando.
pub(super) fn voci_allineate(
    app: &tauri::AppHandle,
    ids: &[i64],
) -> Result<Vec<Option<i64>>, AppError> {
    let ids = ids.get(..ids.len().min(MASSIMO_VOCI)).unwrap_or_default();
    let righe: Vec<i64> = ids
        .iter()
        .copied()
        .filter(|id| ospiti::e_di_libreria(*id))
        .collect();
    let schede = if righe.is_empty() {
        std::collections::HashMap::new()
    } else {
        let stato = super::libreria_di(app, "voci di sessione")?;
        con_libreria(&stato, |libreria| {
            leggi_schede(&libreria.connection, &righe)
        })?
    };
    ospiti::con_registro(app, |registro| {
        ids.iter()
            .map(|id| {
                let scheda = schede.get(id)?.clone();
                let uid = registro.nuovo_uid();
                Some(registro.registra(Voce {
                    uid,
                    proprietario: None,
                    scheda,
                    apertura: Apertura::Libreria { track_id: *id },
                    da_contare: Some(*id),
                }))
            })
            .collect()
    })
    .ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("voci di sessione".to_owned()),
        })
    })
}

/// Le schede di alcune righe, dal database, con un'occhiata sola per tutte
/// le righe e una per brano per il livello.
///
/// Il livello è quello del brano (`scheda_sorgente`), non quello scelto dalla
/// normalizzazione di questo PC: viaggia agli altri, e ciascuno lo usa con la
/// sua normalizzazione. Il livello d'album non c'è perché dipende dai vicini
/// in coda, cioè da una scelta di chi ascolta.
fn leggi_schede(
    connection: &rusqlite::Connection,
    righe: &[i64],
) -> Result<std::collections::HashMap<i64, SchedaVoce>, AppError> {
    let lette = rymembra_app::library::summaries_by_id(connection, righe)?;
    Ok(lette
        .into_iter()
        .map(|riga| {
            let livello = rymembra_app::playback::scheda_sorgente(connection, riga.id).ok();
            (riga.id, scheda_da_riga(&riga, livello.as_ref()))
        })
        .collect())
}

/// La scheda di una voce, da una riga della libreria e dal suo livello.
#[expect(
    clippy::cast_possible_truncation,
    reason = "decibel e un picco lineare: la precisione di un f32 basta e avanza, ed è quella \
              che il motore usa"
)]
pub(super) fn scheda_da_riga(riga: &TrackSummary, livello: Option<&SchedaSorgente>) -> SchedaVoce {
    SchedaVoce {
        titolo: riga.title.clone(),
        artista: riga.artist.clone(),
        album: riga.album.clone(),
        durata_ms: u64::try_from(riga.duration_ms).unwrap_or(0),
        numero: riga.track_number.and_then(|n| u32::try_from(n).ok()),
        anno: riga.year.and_then(|anno| i32::try_from(anno).ok()),
        copertina: riga.cover_art_hash.clone(),
        replaygain_db: livello.and_then(|l| l.replaygain_db).map(|db| db as f32),
        picco: livello.and_then(|l| l.picco).map(|picco| picco as f32),
    }
}

/// I dettagli delle voci che sono brani di questo PC, per mandarle in rete, in
/// blocco e col perché: per ogni `uid`, nello stesso ordine e uno per uno, il
/// brano da mandare o la [`Mancanza`]. Legge il database e il disco, come
/// [`brani_di_questo`]: fuori dal lucchetto del lettore.
///
/// # Perché in blocco
///
/// Perché una voce per volta voleva dire un filo, una scadenza e una lettura
/// del disco per voce: con la coda di un NAS dove trentotto file non c'erano
/// più, trentotto attese, e alla fine un `None` che non diceva se riprovare o
/// rinunciare. Qui il registro si guarda una volta, il database una volta, e il
/// disco su un filo solo con **una** scadenza per tutto il blocco — quella dei
/// flussi. I brani di catalogo il disco non lo toccano.
///
/// # Come si classifica
///
/// - nel registro la voce non apre una riga di questo PC: [`Mancanza::NonDiQuesto`];
/// - la riga non c'è più: [`Mancanza::RigaSparita`];
/// - il file risponde `NotFound`: [`Mancanza::FileAssente`];
/// - ogni altro errore del disco, o la scadenza arrivata prima di quella voce:
///   [`Mancanza::Irraggiungibile`]. Anche il database che non risponde dà
///   questo, per tutte: è un «non adesso», non un «no».
#[must_use]
pub fn voci_di_questo(
    app: &tauri::AppHandle,
    uids: &[u64],
) -> Vec<(u64, Result<BranoDiQuesto, Mancanza>)> {
    let righe = ospiti::con_registro(app, |registro| righe_dei_uid(registro, uids))
        .unwrap_or_else(|| uids.iter().map(|uid| (*uid, None)).collect());
    let da_leggere: Vec<i64> = righe.iter().filter_map(|(_, riga)| *riga).collect();
    let lette = leggi_brani(app, &da_leggere).unwrap_or_else(|err| {
        nota!(
            "[insieme] voci da mandare non lette: {}",
            err.code().kind().code()
        );
        vec![Err(Mancanza::Irraggiungibile); da_leggere.len()]
    });
    let mut lette = lette.into_iter();
    righe
        .into_iter()
        .map(|(uid, riga)| {
            let esito = match riga {
                None => Err(Mancanza::NonDiQuesto),
                Some(_) => lette.next().unwrap_or(Err(Mancanza::Irraggiungibile)),
            };
            (uid, esito)
        })
        .collect()
}

/// La riga della libreria di questo PC dietro ogni `uid`, nell'ordine: solo
/// le voci di questo PC che si aprono da una riga. Una voce di un altro PC,
/// un `uid` che il registro non conosce, la calibrazione: `None`.
pub(super) fn righe_dei_uid(registro: &ospiti::Registro, uids: &[u64]) -> Vec<(u64, Option<i64>)> {
    uids.iter()
        .map(|uid| {
            let riga = registro
                .maniglia(*uid)
                .and_then(|maniglia| registro.voce(maniglia))
                .filter(|voce| voce.proprietario.is_none())
                .and_then(|voce| voce.apertura_di_libreria())
                .filter(|riga| ospiti::e_di_libreria(*riga));
            (*uid, riga)
        })
        .collect()
}

/// I dettagli di brani della libreria di questo PC, nell'ordine; quelli che
/// non ci sono più si saltano. Legge il database e il disco (dimensione e
/// data del file): fuori dal lucchetto del lettore.
///
/// # Il disco, con una scadenza
///
/// Dimensione e data si chiedono al file system, e un file su una cartella di
/// rete che dorme può farsi aspettare quanto Windows vuole. Si chiedono tutte
/// su un filo solo con la scadenza dei flussi: se passa, quelli già letti si
/// tengono e quelli non letti si saltano, come quelli che non ci sono — la
/// sessione non li può servire, e il servente rileggerebbe comunque dimensione
/// e data a ogni lettura. I brani di catalogo il disco non lo toccano, e non
/// aspettano.
///
/// # Errori
///
/// Quelli della libreria.
pub fn brani_di_questo(
    app: &tauri::AppHandle,
    ids: &[i64],
) -> Result<Vec<BranoDiQuesto>, AppError> {
    let righe: Vec<i64> = ids
        .iter()
        .copied()
        .filter(|id| ospiti::e_di_libreria(*id))
        .collect();
    Ok(leggi_brani(app, &righe)?
        .into_iter()
        .filter_map(Result::ok)
        .collect())
}

/// Una riga letta dal database, pronta per il disco.
type Letta = (TrackSummary, SchedaSorgente);

/// Il cuore di [`brani_di_questo`] e [`voci_di_questo`]: per ogni riga,
/// nell'ordine e una per una, il brano o il perché no. Il database con una
/// presa sola, il disco con una scadenza sola.
///
/// # Errori
///
/// Quelli della libreria.
fn leggi_brani(
    app: &tauri::AppHandle,
    righe: &[i64],
) -> Result<Vec<Result<BranoDiQuesto, Mancanza>>, AppError> {
    if righe.is_empty() {
        return Ok(Vec::new());
    }
    let stato = super::libreria_di(app, "brani da mandare in sessione")?;
    let lette = con_libreria(&stato, |libreria| leggi_righe(&libreria.connection, righe))?;
    // I brani del cloud non si chiedono al disco: dimensione e data vengono
    // dallo specchio, e i byte li darà il lettore del cloud (vedi
    // `insieme::filo::ospita`). Gli altri passano da `classifica` come sempre, e
    // l'ordine si ricompone alla fine.
    let archivio = crate::nuvola_musica::archivio(app);
    let quante = lette.len();
    let mut esiti: Vec<Option<Result<BranoDiQuesto, Mancanza>>> = vec![None; quante];
    let mut posti = Vec::new();
    let mut del_disco = Vec::new();
    for (posto, letta) in lette.into_iter().enumerate() {
        match letta {
            Some((riga, sorgente)) if del_cloud(&sorgente).is_some() => {
                if let Some(casella) = esiti.get_mut(posto) {
                    *casella = Some(brano_del_cloud(&riga, &sorgente, archivio.as_deref()));
                }
            }
            altra => {
                posti.push(posto);
                del_disco.push(altra);
            }
        }
    }
    let classificati = classifica(del_disco, super::APERTURA_FLUSSO, |percorso: &Path| {
        std::fs::metadata(percorso)
    });
    for (posto, esito) in posti.into_iter().zip(classificati) {
        if let Some(casella) = esiti.get_mut(posto) {
            *casella = Some(esito);
        }
    }
    Ok(esiti
        .into_iter()
        .map(|esito| esito.unwrap_or(Err(Mancanza::Irraggiungibile)))
        .collect())
}

/// Il file del cloud di una voce, se lo è: il brano, o la sua immagine CD.
fn del_cloud(sorgente: &SchedaSorgente) -> Option<&str> {
    sorgente
        .collocazione
        .file_audio()
        .filter(|file| rymembra_domain::nuvola::e_nuvola(file))
}

/// Un [`BranoDiQuesto`] per un brano del cloud, dallo specchio.
///
/// Dimensione e data quelle dello specchio, che sono quelle con cui il
/// lettore del cloud ricontrolla a ogni lettura che il file non sia cambiato.
/// Un conto non collegato su questo PC vale «irraggiungibile»: riprovare dopo
/// averlo collegato basta.
fn brano_del_cloud(
    riga: &TrackSummary,
    sorgente: &SchedaSorgente,
    archivio: Option<&rymembra_cloud::musica::ArchivioNuvola>,
) -> Result<BranoDiQuesto, Mancanza> {
    let segmento = match &sorgente.collocazione {
        Collocazione::Segmento {
            inizio_cd, fine_cd, ..
        } => Some((ms_da_cd(*inizio_cd), fine_cd.map(ms_da_cd))),
        Collocazione::File(_) | Collocazione::Catalogo { .. } => None,
    };
    let percorso = del_cloud(sorgente).ok_or(Mancanza::NonDiQuesto)?;
    let archivio = archivio.ok_or(Mancanza::Irraggiungibile)?;
    let (conto, voce) = archivio.voce_di(percorso).ok_or(Mancanza::FileAssente)?;
    if archivio.deposito(&conto).is_none() {
        return Err(Mancanza::Irraggiungibile);
    }
    let percorso = PathBuf::from(percorso);
    Ok(BranoDiQuesto {
        track_id: riga.id,
        scheda: scheda_da_riga(riga, Some(sorgente)),
        file: Some(FileDellaVoce {
            estensione: estensione_di(&percorso),
            dimensione: voce.dimensione,
            modificato_ms: voce.modificato_ms,
            segmento,
            percorso,
        }),
        riferimento: None,
        copertina: riga.cover_art_hash.clone(),
    })
}

/// Le righe dal database, nell'ordine chiesto e coi doppioni: `None` per una
/// riga che non c'è più (o che non si sa più aprire).
pub(super) fn leggi_righe(
    connection: &rusqlite::Connection,
    righe: &[i64],
) -> Result<Vec<Option<Letta>>, AppError> {
    let schede = rymembra_app::library::summaries_by_id(connection, righe)?;
    let per_id: std::collections::HashMap<i64, TrackSummary> =
        schede.into_iter().map(|riga| (riga.id, riga)).collect();
    Ok(righe
        .iter()
        .map(|id| {
            let riga = per_id.get(id)?.clone();
            let sorgente = rymembra_app::playback::scheda_sorgente(connection, *id).ok()?;
            Some((riga, sorgente))
        })
        .collect())
}

/// Dalle righe lette ai brani, col disco su un filo solo e una scadenza sola
/// per tutto il blocco.
///
/// Il filo mette da parte quel che legge man mano: alla scadenza si tiene
/// quel che c'è, e le voci non ancora lette sono [`Mancanza::Irraggiungibile`]
/// — il filo, se è ancora appeso a una cartella, finisce da sé e quel che
/// aggiunge dopo non lo legge più nessuno. I brani di catalogo si fanno prima e
/// subito: il disco non lo toccano, e non aspettano dietro a un NAS che dorme.
///
/// `stat` è la lettura del disco: `std::fs::metadata`, o una finta nelle prove.
pub(super) fn classifica<S>(
    lette: Vec<Option<Letta>>,
    scadenza: std::time::Duration,
    stat: S,
) -> Vec<Result<BranoDiQuesto, Mancanza>>
where
    S: Fn(&Path) -> std::io::Result<std::fs::Metadata> + Send + 'static,
{
    let mut esiti: Vec<Option<Result<BranoDiQuesto, Mancanza>>> = vec![None; lette.len()];
    let mut sul_disco = Vec::new();
    for (posto, letta) in lette.into_iter().enumerate() {
        let esito = match letta {
            None => Err(Mancanza::RigaSparita),
            Some((riga, sorgente)) => {
                if matches!(sorgente.collocazione, Collocazione::Catalogo { .. }) {
                    brano_di_questo(&riga, &sorgente, &stat)
                } else {
                    sul_disco.push((posto, riga, sorgente));
                    continue;
                }
            }
        };
        if let Some(casella) = esiti.get_mut(posto) {
            *casella = Some(esito);
        }
    }
    if !sul_disco.is_empty() {
        let quanti = sul_disco.len();
        let raccolti = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let nel_filo = std::sync::Arc::clone(&raccolti);
        let esito =
            rymembra_app::scadenza::con_scadenza_o_caduta("insieme-file", scadenza, move || {
                for (posto, riga, sorgente) in sul_disco {
                    let esito = brano_di_questo(&riga, &sorgente, &stat);
                    nel_filo
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((posto, esito));
                }
            });
        let letti = std::mem::take(
            &mut *raccolti
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        match esito {
            Ok(()) => {}
            Err(rymembra_app::scadenza::Mancato::Scaduto) => nota!(
                "[insieme] file da servire: {} su {quanti} letti entro {} ms, gli altri \
                 irraggiungibili",
                letti.len(),
                scadenza.as_millis()
            ),
            Err(rymembra_app::scadenza::Mancato::Caduto { .. }) => nota!(
                "[insieme] file da servire: la lettura è caduta dopo {} su {quanti}, gli altri \
                 irraggiungibili",
                letti.len()
            ),
        }
        for (posto, esito) in letti {
            if let Some(casella) = esiti.get_mut(posto) {
                *casella = Some(esito);
            }
        }
    }
    esiti
        .into_iter()
        .map(|esito| esito.unwrap_or(Err(Mancanza::Irraggiungibile)))
        .collect()
}

/// Un [`BranoDiQuesto`] da una riga e dalla sua collocazione, o il perché
/// il suo file non si può servire.
fn brano_di_questo(
    riga: &TrackSummary,
    sorgente: &SchedaSorgente,
    stat: &dyn Fn(&Path) -> std::io::Result<std::fs::Metadata>,
) -> Result<BranoDiQuesto, Mancanza> {
    let scheda = scheda_da_riga(riga, Some(sorgente));
    let (file, riferimento) = match &sorgente.collocazione {
        Collocazione::File(percorso) => (Some(file_della_voce(percorso, None, stat)?), None),
        Collocazione::Segmento {
            immagine,
            inizio_cd,
            fine_cd,
        } => {
            let tratto = (ms_da_cd(*inizio_cd), fine_cd.map(ms_da_cd));
            (Some(file_della_voce(immagine, Some(tratto), stat)?), None)
        }
        Collocazione::Catalogo { fonte, url } => {
            (None, Some(super::flusso::riferimento_in_testo(*fonte, url)))
        }
    };
    Ok(BranoDiQuesto {
        track_id: riga.id,
        scheda,
        file,
        riferimento,
        copertina: riga.cover_art_hash.clone(),
    })
}

/// Il file di una voce, letto adesso dal disco. `NotFound` — il file o la sua
/// cartella non ci sono — è [`Mancanza::FileAssente`], e lo è anche un
/// percorso che non è un file; ogni altro errore è
/// [`Mancanza::Irraggiungibile`].
fn file_della_voce(
    percorso: &str,
    segmento: Option<(u64, Option<u64>)>,
    stat: &dyn Fn(&Path) -> std::io::Result<std::fs::Metadata>,
) -> Result<FileDellaVoce, Mancanza> {
    let percorso = PathBuf::from(percorso);
    let dati = stat(&percorso).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Mancanza::FileAssente
        } else {
            Mancanza::Irraggiungibile
        }
    })?;
    if !dati.is_file() {
        return Err(Mancanza::FileAssente);
    }
    let modificato_ms = dati
        .modified()
        .ok()
        .and_then(|quando| quando.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0);
    Ok(FileDellaVoce {
        estensione: estensione_di(&percorso),
        dimensione: dati.len(),
        modificato_ms,
        segmento,
        percorso,
    })
}

/// L'estensione di un file, in minuscolo: il suggerimento per il
/// riconoscitore del formato sull'altro PC.
fn estensione_di(percorso: &Path) -> Option<String> {
    percorso
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

/// Fotogrammi del CD (1/75 s) in millisecondi, arrotondati: il contrario di
/// `ospiti::tratto`, e il giro è esatto.
pub(super) fn ms_da_cd(cd: u32) -> u64 {
    u64::from(cd)
        .saturating_mul(2000)
        .saturating_add(75)
        .checked_div(150)
        .unwrap_or(0)
}
