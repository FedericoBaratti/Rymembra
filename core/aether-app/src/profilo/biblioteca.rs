//! Quel che il documento di sincronia **non** copre.
//!
//! # Perché esiste un secondo documento
//!
//! Perché il primo non è nostro. `aether_sync::Contenuto` è la forma su cui due
//! dispositivi si mettono d'accordo, e ogni campo che ci si aggiunge è un campo
//! che ogni Aether in circolazione deve saper fondere: allargarlo per portarsi
//! dietro i testi vorrebbe dire cambiare il protocollo della sincronia per una
//! funzione che con la sincronia non c'entra.
//!
//! Quindi due documenti dentro lo stesso archivio. Il primo è **letteralmente**
//! quello della sincronia, byte per byte come `documento::serializza` lo
//! produce; il secondo è questo, e contiene il complemento: cronologia,
//! correzioni, testi, desiderati, copertine. Tutto indicizzato su `track_key`,
//! come il primo, perché l'abbinamento fra macchine è uno solo e il manifesto
//! lo dichiara.
//!
//! # Cosa resta fuori, e non è una dimenticanza
//!
//! **Affinità** (`brano_vicino`, `track_impronta`, `impronta_scala`) e
//! **settimana** (`settimana_raccolta`, `settimana_brano`). Sono derivati: si
//! ricalcolano dai brani che ci sono, e portarli vorrebbe dire garantirne la
//! coerenza con una libreria che *non è quella da cui vengono*. Un'affinità
//! calcolata su diciottomila brani e riversata su una libreria che ne ha
//! duemila non è un'approssimazione: è un elenco di vicini che rimandano a
//! canzoni che di qua non esistono, e il pannello «simili» che non risponde
//! mai. Il costo di lasciarli fuori è una passata di analisi; il costo di
//! portarli è un difetto che nessuno collega al profilo importato il mese
//! prima.
//!
//! Fuori anche `enrich_undo` e `track_meta_arricchita`. Il primo è la via di
//! ritorno per i tag che le versioni fino alla 2.3.0 hanno riscritto **in
//! questi file**, e su un altro computer non riporterebbe indietro niente. Il
//! secondo è ri-derivabile da una passata di arricchimento, e reimportarlo su
//! un'altra macchina riattaccherebbe annotazioni a righe i cui file possono
//! avere tag diversi — cioè scriverebbe nella libreria una conclusione tratta
//! guardando file che non sono questi.
//!
//! # Le regole della fusione, tutte in una riga ciascuna
//!
//! - **Cronologia**: `INSERT OR IGNORE`. L'indice unico della migrazione `020`
//!   fa il resto, e senza di lui reimportare lo stesso archivio raddoppierebbe
//!   la storia d'ascolto.
//! - **Correzioni**: vince la più recente, e **mai** su niente — una correzione
//!   non cancella l'altra, la sostituisce solo se è stata decisa dopo.
//! - **Testi**: vince il più recente, e **mai** su un `source = 'mano'` locale.
//!   Un testo che qualcuno ha sincronizzato a mano è lavoro, non un dato.
//! - **Desiderati**: `INSERT OR IGNORE` sulla coppia che già li identifica.
//! - **Copertine**: la riga entra se manca, e si attacca al brano **solo se il
//!   brano non ne ha già una**.
//!
//! E la regola sopra tutte: **niente cancella niente**. Nessuna lapide del
//! profilo si applica, nessuna `DELETE` sta in questo modulo. Un profilo è un
//! dono, non un'autorità: chi lo importa aggiunge quel che gli manca, e non
//! perde quel che aveva.

use std::collections::{BTreeMap, BTreeSet};

use aether_domain::errors::{AppError, ErrorCode};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::library::db_error;

/// La versione del documento.
///
/// Come per la sincronia, sta anche nel **nome** del file dentro l'archivio: il
/// giorno in cui il formato cambiasse in modo non leggibile all'indietro, una
/// versione vecchia continuerebbe a trovare il suo `.v1` invece di litigare con
/// un file che non capisce.
pub const VERSIONE: u32 = 1;

/// Un ascolto, come sta nella cronologia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ascoltato {
    /// Quale brano.
    pub chiave: String,
    /// Quando è cominciato, in millisecondi.
    pub quando_ms: i64,
    /// Quanto se ne è sentito, in millisecondi.
    pub ms: i64,
    /// Da dove viene la riga: `local` o `spotify`.
    pub fonte: String,
}

/// Una correzione che l'utente ha deciso.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correzione {
    /// I campi corretti, come JSON. La forma è quella di `track_overrides`.
    pub campi: String,
    /// Quando è stata decisa.
    pub set_at: i64,
}

/// Un testo, come sta in `lyrics`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Testo {
    /// Il testo senza tempi.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plain: Option<String>,
    /// L'LRC come sta nel file, byte per byte. Porta con sé i tempi delle
    /// parole quando il file li aveva.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced: Option<String>,
    /// Da dove viene: `''`, `sidecar`, `tag`, `lrclib`, `mano`.
    pub source: String,
    /// L'identificativo nel catalogo remoto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lrclib_id: Option<i64>,
    /// La correzione dell'utente, nel verso dello standard LRC.
    #[serde(default)]
    pub offset_ms: i64,
    /// Il brano non ha parole.
    #[serde(default)]
    pub instrumental: bool,
    /// La durata a cui questi tempi si riferiscono.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// Quando la riga è stata scritta l'ultima volta.
    pub updated_at: i64,
}

/// Un brano che manca, e da dove lo si è saputo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Desiderato {
    /// L'identità del brano, la stessa dei brani veri.
    pub chiave: String,
    /// Come si chiama.
    pub title: String,
    /// Chi lo suona.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// Da quale disco.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// L'artista del disco.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    /// Quanto dura.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// Il codice internazionale della registrazione.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    /// Che genere di contenitore lo portava.
    pub source_kind: String,
    /// Quale contenitore.
    pub source_id: String,
    /// Come si chiamava.
    pub source_title: String,
    /// Da quale servizio.
    pub source_service: String,
    /// Dove lo si può sentire o comprare.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fonte_url: Option<String>,
    /// Con quale licenza.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licenza: Option<String>,
    /// Se si può scaricare o solo comprare.
    pub disponibilita: String,
    /// Quando è entrato nell'elenco.
    pub added_at: i64,
}

/// La copertina di un brano, e la riga che la descrive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Copertina {
    /// L'impronta dei byte originali: è il nome del file nell'archivio.
    pub hash: String,
    /// Il tipo dell'immagine salvata.
    pub mime_type: String,
    /// Larghezza.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    /// Altezza.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    /// Quanto occupa su disco la versione piena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_size: Option<i64>,
    /// Da dove viene.
    pub source: String,
    /// Quando è entrata nello store.
    pub created_at: i64,
}

/// Il documento: tutto ciò che la sincronia non copre.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Biblioteca {
    /// La versione del formato.
    pub versione: u32,
    /// La cronologia d'ascolto, riga per riga.
    #[serde(default)]
    pub cronologia: Vec<Ascoltato>,
    /// Le correzioni dell'utente, per brano.
    #[serde(default)]
    pub correzioni: BTreeMap<String, Correzione>,
    /// I testi, per brano.
    #[serde(default)]
    pub testi: BTreeMap<String, Testo>,
    /// I brani che mancano.
    #[serde(default)]
    pub desiderati: Vec<Desiderato>,
    /// Le copertine, per brano.
    #[serde(default)]
    pub copertine: BTreeMap<String, Copertina>,
}

impl Biblioteca {
    /// Le impronte delle copertine, senza ripetizioni.
    ///
    /// Una sola copertina serve a tutto un disco: su una libreria vera i brani
    /// sono dieci volte le copertine, ed è la ragione per cui l'archivio le
    /// indirizza dal contenuto invece di metterne una per riga.
    #[must_use]
    pub fn impronte(&self) -> BTreeSet<String> {
        self.copertine
            .values()
            .map(|copertina| copertina.hash.clone())
            .collect()
    }
}

/// Quante cose una fusione ha portato.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Portato {
    /// Righe di cronologia entrate.
    pub cronologia: usize,
    /// Correzioni scritte o aggiornate.
    pub correzioni: usize,
    /// Testi scritti o aggiornati.
    pub testi: usize,
    /// Brani aggiunti all'elenco di quelli che mancano.
    pub desiderati: usize,
    /// Brani che hanno guadagnato una copertina.
    pub copertine: usize,
}

impl Portato {
    /// Non ha portato niente.
    #[must_use]
    pub const fn e_vuoto(&self) -> bool {
        self.cronologia == 0
            && self.correzioni == 0
            && self.testi == 0
            && self.desiderati == 0
            && self.copertine == 0
    }
}

// ── dal database al documento ───────────────────────────────────────────────

/// Legge dal database tutto ciò che il documento di sincronia non copre.
///
/// Sola lettura, e da chiamare **sotto il lucchetto** insieme a
/// `sincronia::{allinea, contenuto}`: sono cinque interrogazioni, cioè
/// millisecondi, e stare nella stessa finestra è quel che garantisce che le due
/// metà del profilo descrivano la stessa libreria e non due istanti diversi.
///
/// # Errori
///
/// `db.queryFailed` se una lettura fallisce.
pub fn raccogli(connection: &Connection) -> Result<Biblioteca, AppError> {
    Ok(Biblioteca {
        versione: VERSIONE,
        cronologia: cronologia(connection)?,
        correzioni: correzioni(connection)?,
        testi: testi(connection)?,
        desiderati: desiderati(connection)?,
        copertine: copertine(connection)?,
    })
}

/// La cronologia, sulla chiave del brano invece che sul suo identificativo.
fn cronologia(connection: &Connection) -> Result<Vec<Ascoltato>, AppError> {
    let mut istruzione = connection
        .prepare(
            "SELECT t.track_key, h.played_at, h.ms_played, h.source
               FROM play_history h JOIN tracks t ON t.id = h.track_id
              ORDER BY h.played_at",
        )
        .map_err(|err| db_error("cronologia del profilo", &err))?;
    let righe = istruzione
        .query_map([], |riga| {
            Ok(Ascoltato {
                chiave: riga.get(0)?,
                quando_ms: riga.get(1)?,
                ms: riga.get(2)?,
                fonte: riga.get(3)?,
            })
        })
        .map_err(|err| db_error("cronologia del profilo", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("cronologia del profilo", &err))
}

/// Le correzioni, sulla chiave del brano.
fn correzioni(connection: &Connection) -> Result<BTreeMap<String, Correzione>, AppError> {
    let mut istruzione = connection
        .prepare(
            "SELECT t.track_key, o.campi, o.set_at
               FROM track_overrides o JOIN tracks t ON t.id = o.track_id
              ORDER BY o.set_at",
        )
        .map_err(|err| db_error("correzioni del profilo", &err))?;
    let righe = istruzione
        .query_map([], |riga| {
            Ok((
                riga.get::<_, String>(0)?,
                Correzione {
                    campi: riga.get(1)?,
                    set_at: riga.get(2)?,
                },
            ))
        })
        .map_err(|err| db_error("correzioni del profilo", &err))?;
    let mut mappa = BTreeMap::new();
    for riga in righe {
        let (chiave, correzione) = riga.map_err(|err| db_error("correzioni del profilo", &err))?;
        // Due file dello stesso brano possono avere due correzioni diverse.
        // Vince la più recente, che è la stessa regola con cui la fusione
        // deciderà dall'altra parte: dire qui una cosa e là un'altra vorrebbe
        // dire che esportare e reimportare cambia il risultato.
        mappa
            .entry(chiave)
            .and_modify(|dentro: &mut Correzione| {
                if correzione.set_at > dentro.set_at {
                    *dentro = correzione.clone();
                }
            })
            .or_insert(correzione);
    }
    Ok(mappa)
}

/// I testi. Sono già indicizzati sulla chiave del brano.
fn testi(connection: &Connection) -> Result<BTreeMap<String, Testo>, AppError> {
    let mut istruzione = connection
        .prepare(
            "SELECT track_key, plain, synced, source, lrclib_id, offset_ms,
                    instrumental, duration_ms, updated_at
               FROM lyrics",
        )
        .map_err(|err| db_error("testi del profilo", &err))?;
    let righe = istruzione
        .query_map([], |riga| {
            Ok((
                riga.get::<_, String>(0)?,
                Testo {
                    plain: riga.get(1)?,
                    synced: riga.get(2)?,
                    source: riga.get(3)?,
                    lrclib_id: riga.get(4)?,
                    offset_ms: riga.get(5)?,
                    instrumental: riga.get::<_, i64>(6)? != 0,
                    duration_ms: riga.get(7)?,
                    updated_at: riga.get(8)?,
                },
            ))
        })
        .map_err(|err| db_error("testi del profilo", &err))?;
    righe
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|err| db_error("testi del profilo", &err))
}

/// I brani che mancano.
fn desiderati(connection: &Connection) -> Result<Vec<Desiderato>, AppError> {
    let mut istruzione = connection
        .prepare(
            "SELECT track_key, title, artist, album, album_artist, duration_ms, isrc,
                    source_kind, source_id, source_title, source_service,
                    fonte_url, licenza, disponibilita, added_at
               FROM desiderati ORDER BY added_at",
        )
        .map_err(|err| db_error("desiderati del profilo", &err))?;
    let righe = istruzione
        .query_map([], |riga| {
            Ok(Desiderato {
                chiave: riga.get(0)?,
                title: riga.get(1)?,
                artist: riga.get(2)?,
                album: riga.get(3)?,
                album_artist: riga.get(4)?,
                duration_ms: riga.get(5)?,
                isrc: riga.get(6)?,
                source_kind: riga.get(7)?,
                source_id: riga.get(8)?,
                source_title: riga.get(9)?,
                source_service: riga.get(10)?,
                fonte_url: riga.get(11)?,
                licenza: riga.get(12)?,
                disponibilita: riga.get(13)?,
                added_at: riga.get(14)?,
            })
        })
        .map_err(|err| db_error("desiderati del profilo", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("desiderati del profilo", &err))
}

/// Le copertine attaccate a un brano, con la riga che le descrive.
fn copertine(connection: &Connection) -> Result<BTreeMap<String, Copertina>, AppError> {
    let mut istruzione = connection
        .prepare(
            "SELECT t.track_key, c.hash, c.mime_type, c.width, c.height, c.byte_size,
                    c.source, c.created_at
               FROM tracks t JOIN cover_art c ON c.hash = t.cover_art_hash
              ORDER BY t.track_key",
        )
        .map_err(|err| db_error("copertine del profilo", &err))?;
    let righe = istruzione
        .query_map([], |riga| {
            Ok((
                riga.get::<_, String>(0)?,
                Copertina {
                    hash: riga.get(1)?,
                    mime_type: riga.get(2)?,
                    width: riga.get(3)?,
                    height: riga.get(4)?,
                    byte_size: riga.get(5)?,
                    source: riga.get(6)?,
                    created_at: riga.get(7)?,
                },
            ))
        })
        .map_err(|err| db_error("copertine del profilo", &err))?;
    righe
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|err| db_error("copertine del profilo", &err))
}

// ── il documento sul disco ──────────────────────────────────────────────────

/// Il documento come testo, pronto da mettere nell'archivio.
///
/// Testo e non byte compressi: la compressione la fa lo zip, che per questa
/// voce usa `deflate`. Comprimerlo qui e poi metterlo dentro `Stored`
/// darebbe lo stesso risultato in byte e un formato con due strati invece di
/// uno; comprimerlo qui e poi ricomprimerlo pagherebbe CPU per niente.
///
/// # Errori
///
/// `internal.unexpected` se la serializzazione fallisce, cosa che per questi
/// tipi non può accadere — ma un profilo scritto a metà è una perdita
/// silenziosa, e non è un rischio da nascondere dietro un valore di ripiego.
pub fn serializza(biblioteca: &Biblioteca) -> Result<String, AppError> {
    serde_json::to_string(biblioteca).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("serializzazione della biblioteca del profilo".to_owned()),
        })
        .with_cause(err.to_string())
    })
}

/// Rilegge il documento.
///
/// # Errori
///
/// `settings.corrupt` se non è JSON leggibile o se dichiara una versione più
/// alta di [`VERSIONE`]. Intransigente sulla versione e per la stessa ragione di
/// `aether_sync::documento::interpreta`: è l'unico caso in cui leggere male
/// porterebbe a scrivere nella libreria una cosa che si è capita a metà.
pub fn interpreta(byte: &[u8]) -> Result<Biblioteca, AppError> {
    let biblioteca: Biblioteca = serde_json::from_slice(byte).map_err(|err| {
        AppError::new(ErrorCode::SettingsCorrupt {
            quarantined_as: None,
        })
        .with_cause(format!("la biblioteca del profilo non si legge: {err}"))
    })?;
    if biblioteca.versione > VERSIONE {
        return Err(AppError::new(ErrorCode::SettingsCorrupt {
            quarantined_as: None,
        })
        .with_cause(format!(
            "biblioteca di formato {} scritta da una versione più recente: questa legge fino alla {VERSIONE}",
            biblioteca.versione
        )));
    }
    Ok(biblioteca)
}

// ── dal documento al database ───────────────────────────────────────────────

/// Scrive nella libreria quel che il profilo porta, senza togliere niente.
///
/// Dentro una transazione già aperta, come [`crate::sincronia::applica_in`] e
/// per lo stesso motivo: il piano è l'esecuzione annullata, e per esserlo deve
/// essere davvero l'esecuzione.
///
/// # Errori
///
/// `db.queryFailed` se una scrittura fallisce.
pub fn applica_in(
    tx: &Transaction<'_>,
    biblioteca: &Biblioteca,
    adesso_ms: i64,
) -> Result<Portato, AppError> {
    let _ = adesso_ms;
    Ok(Portato {
        cronologia: applica_cronologia(tx, &biblioteca.cronologia)?,
        correzioni: applica_correzioni(tx, &biblioteca.correzioni)?,
        testi: applica_testi(tx, &biblioteca.testi)?,
        desiderati: applica_desiderati(tx, &biblioteca.desiderati)?,
        copertine: applica_copertine(tx, &biblioteca.copertine)?,
    })
}

/// La cronologia: `INSERT OR IGNORE`, e l'indice unico fa il resto.
fn applica_cronologia(tx: &Transaction<'_>, righe: &[Ascoltato]) -> Result<usize, AppError> {
    if righe.is_empty() {
        return Ok(0);
    }
    // `LIMIT 1` e non tutte le righe con quella chiave: due file dello stesso
    // brano sono lo stesso brano, e scrivere l'ascolto su tutti e due
    // raddoppierebbe la storia proprio nella funzione che esiste per non
    // raddoppiarla.
    let mut scrivi = tx
        .prepare(
            "INSERT OR IGNORE INTO play_history (track_id, played_at, ms_played, source)
             SELECT id, ?2, ?3, ?4 FROM tracks WHERE track_key = ?1 ORDER BY id LIMIT 1",
        )
        .map_err(|err| db_error("cronologia dal profilo", &err))?;
    let mut entrate = 0;
    for riga in righe {
        // La provenienza si conserva, ma vincolata a quel che lo schema ammette:
        // il `CHECK` di `play_history.source` conosce due valori, e un archivio
        // costruito a mano potrebbe scriverne un terzo — che farebbe fallire
        // l'intera transazione invece di perdere una riga.
        let fonte = if riga.fonte == "spotify" {
            "spotify"
        } else {
            "local"
        };
        entrate += scrivi
            .execute(rusqlite::params![
                riga.chiave,
                riga.quando_ms,
                riga.ms.max(0),
                fonte
            ])
            .map_err(|err| db_error("cronologia dal profilo", &err))?;
    }
    Ok(entrate)
}

/// Le correzioni: vince la più recente, e la riga di `tracks` si rifà subito.
fn applica_correzioni(
    tx: &Transaction<'_>,
    correzioni: &BTreeMap<String, Correzione>,
) -> Result<usize, AppError> {
    if correzioni.is_empty() {
        return Ok(0);
    }
    let mut toccati: BTreeSet<i64> = BTreeSet::new();
    {
        let mut quali = tx
            .prepare("SELECT id FROM tracks WHERE track_key = ?1")
            .map_err(|err| db_error("correzioni dal profilo", &err))?;
        let mut scrivi = tx
            .prepare(
                "INSERT INTO track_overrides (track_id, campi, set_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(track_id) DO UPDATE SET
                     campi = excluded.campi, set_at = excluded.set_at
                   WHERE excluded.set_at > track_overrides.set_at",
            )
            .map_err(|err| db_error("correzioni dal profilo", &err))?;

        for (chiave, correzione) in correzioni {
            let identificativi: Vec<i64> = quali
                .query_map([chiave], |riga| riga.get(0))
                .map_err(|err| db_error("correzioni dal profilo", &err))?
                .collect::<Result<_, _>>()
                .map_err(|err| db_error("correzioni dal profilo", &err))?;
            for track_id in identificativi {
                let scritte = scrivi
                    .execute(rusqlite::params![
                        track_id,
                        correzione.campi,
                        correzione.set_at
                    ])
                    .map_err(|err| db_error("correzioni dal profilo", &err))?;
                if scritte > 0 {
                    toccati.insert(track_id);
                }
            }
        }
    }

    // E la riga torna a mostrare quel che l'utente aveva deciso. Senza questa
    // chiamata la correzione starebbe nella sua tabella e `tracks` continuerebbe
    // a dire i tag: è esattamente il guasto che la 2.3.1 ha corretto nella
    // scansione, e ripeterlo qui vorrebbe dire reintrodurlo da un'altra porta.
    for track_id in &toccati {
        crate::incerti::riapplica(tx, *track_id)?;
    }
    if !toccati.is_empty() {
        crate::library::rebuild_aggregates(tx)?;
    }
    Ok(toccati.len())
}

/// I testi: vince il più recente, e **mai** su un `source = 'mano'` locale.
fn applica_testi(tx: &Transaction<'_>, testi: &BTreeMap<String, Testo>) -> Result<usize, AppError> {
    if testi.is_empty() {
        return Ok(0);
    }
    let mut scrivi = tx
        .prepare(
            "INSERT INTO lyrics (track_key, plain, synced, source, lrclib_id, offset_ms,
                                 instrumental, duration_ms, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(track_key) DO UPDATE SET
                 plain = excluded.plain,
                 synced = excluded.synced,
                 source = excluded.source,
                 lrclib_id = excluded.lrclib_id,
                 offset_ms = excluded.offset_ms,
                 instrumental = excluded.instrumental,
                 duration_ms = excluded.duration_ms,
                 updated_at = excluded.updated_at
               -- Le due condizioni non sono intercambiabili. La seconda è la
               -- solita regola del più recente; la prima dice che un testo
               -- sincronizzato a mano su questo computer non si tocca nemmeno
               -- se quello che arriva è più recente, perché è lavoro di
               -- qualcuno e non un dato scaricato.
               WHERE lyrics.source <> 'mano' AND excluded.updated_at > lyrics.updated_at",
        )
        .map_err(|err| db_error("testi dal profilo", &err))?;
    let mut scritti = 0;
    for (chiave, testo) in testi {
        scritti += scrivi
            .execute(rusqlite::params![
                chiave,
                testo.plain,
                testo.synced,
                testo.source,
                testo.lrclib_id,
                testo.offset_ms,
                i64::from(testo.instrumental),
                testo.duration_ms,
                testo.updated_at,
            ])
            .map_err(|err| db_error("testi dal profilo", &err))?;
    }
    Ok(scritti)
}

/// I desiderati: entrano quelli che qui non ci sono.
fn applica_desiderati(tx: &Transaction<'_>, righe: &[Desiderato]) -> Result<usize, AppError> {
    if righe.is_empty() {
        return Ok(0);
    }
    let mut scrivi = tx
        .prepare(
            "INSERT OR IGNORE INTO desiderati
                 (track_key, title, artist, album, album_artist, duration_ms, isrc,
                  source_kind, source_id, source_title, source_service,
                  fonte_url, licenza, disponibilita, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )
        .map_err(|err| db_error("desiderati dal profilo", &err))?;
    let mut entrati = 0;
    for riga in righe {
        entrati += scrivi
            .execute(rusqlite::params![
                riga.chiave,
                riga.title,
                riga.artist,
                riga.album,
                riga.album_artist,
                riga.duration_ms,
                riga.isrc,
                riga.source_kind,
                riga.source_id,
                riga.source_title,
                riga.source_service,
                riga.fonte_url,
                riga.licenza,
                riga.disponibilita,
                riga.added_at,
            ])
            .map_err(|err| db_error("desiderati dal profilo", &err))?;
    }
    Ok(entrati)
}

/// Le copertine: la riga entra se manca, e si attacca solo a chi non ne ha una.
fn applica_copertine(
    tx: &Transaction<'_>,
    copertine: &BTreeMap<String, Copertina>,
) -> Result<usize, AppError> {
    if copertine.is_empty() {
        return Ok(0);
    }
    let mut descrivi = tx
        .prepare(
            "INSERT OR IGNORE INTO cover_art
                 (hash, mime_type, width, height, byte_size, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .map_err(|err| db_error("copertine dal profilo", &err))?;
    // `IS NULL` e non un `COALESCE`: una copertina che c'è resta quella che c'è.
    // Chi importa un profilo non sta chiedendo di cambiare le proprie
    // copertine, sta chiedendo di riempire i buchi.
    let mut attacca = tx
        .prepare(
            "UPDATE tracks SET cover_art_hash = ?2
              WHERE track_key = ?1 AND cover_art_hash IS NULL",
        )
        .map_err(|err| db_error("copertine dal profilo", &err))?;

    let mut attaccate = 0;
    for (chiave, copertina) in copertine {
        descrivi
            .execute(rusqlite::params![
                copertina.hash,
                copertina.mime_type,
                copertina.width,
                copertina.height,
                copertina.byte_size,
                copertina.source,
                copertina.created_at,
            ])
            .map_err(|err| db_error("copertine dal profilo", &err))?;
        attaccate += attacca
            .execute(rusqlite::params![chiave, copertina.hash])
            .map_err(|err| db_error("copertine dal profilo", &err))?;
    }
    Ok(attaccate)
}
