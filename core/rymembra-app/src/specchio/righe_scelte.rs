//! Quali righe di un elenco servono a un gesto, scelte dal nucleo.
//!
//! «Riproduci», «Casuale» e «Scarica qui» su un artista o su una selezione
//! grande si facevano con le righe intere in pagina: duecento per volta, in fila
//! — venticinque viaggi per cinquemila brani, e con cinquantamila selezionati
//! decine di megabyte di JSON — solo perché la finestra scartasse le righe di un
//! dispositivo spento (S9-8, S9-9). Il nucleo sa già chi risponde e quali righe
//! il proprietario non manda più: qui dà gli identificativi **nello stesso
//! ordine**, con le righe che non servono già tolte e contate.
//!
//! # Per suonare e per scaricare
//!
//! Le due domande non hanno la stessa risposta:
//!
//! - **suonare**: una riga di qui si suona; una di un dispositivo che risponde
//!   anche; una di un dispositivo che tace **si suona lo stesso** se di quel brano
//!   c'è una copia di qui (`scelta::copia_da_suonare` sceglie quella);
//! - **scaricare**: serve la riga di un altro dispositivo **che risponde** — una
//!   riga di qui non c'è da scaricare, e una di un dispositivo spento non si può,
//!   anche se ha una copia di qui.
//!
//! In tutti e due i casi una riga che il proprietario non manda più (in grazia,
//! `copia_di` uguale al proprio `id`) non serve: non si suona e non si scarica.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use rymembra_domain::errors::AppError;

use super::{SQL_SPECCHIO, parti};
use crate::library::db_error;

/// A che cosa servono le righe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Uso {
    /// Metterle in coda e suonarle.
    Suona,
    /// Scaricarle in questo dispositivo.
    Scarica,
}

/// Le righe che servono, e quelle che il nucleo ha scartato.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RigheScelte {
    /// Quelle che servono, nell'ordine chiesto. Per [`Uso::Suona`] quelle di qui
    /// e di un altro dispositivo che si può suonare; per [`Uso::Scarica`] **solo**
    /// quelle di un altro dispositivo che risponde.
    pub ids: Vec<i64>,
    /// Quante di `ids` stanno su un altro dispositivo.
    pub lontane: usize,
    /// Quante ne ha scartate: il dispositivo non risponde, o il proprietario non
    /// le manda più.
    pub spente: usize,
    /// La prima delle scartate, per dire «<PC> non raggiungibile».
    pub prima_spenta: Option<i64>,
}

/// Come sta una riga, per scegliere.
struct Riga {
    /// Il dispositivo, se è una riga specchio.
    dispositivo: Option<String>,
    /// Il proprietario non la manda più.
    in_grazia: bool,
    /// C'è anche una copia di qui, nascosta dietro di lei.
    copia_qui: bool,
}

/// Sceglie fra `ids` quelle che servono a `uso`.
///
/// `raggiungibili` sono le chiavi dei dispositivi che rispondono adesso. Gli id
/// che la libreria non ha (una riga tolta nel frattempo) restano com'erano:
/// se ne accorge chi le apre, col suo errore, e scartarle in silenzio vorrebbe
/// dire una coda più corta senza dirlo.
///
/// # Errori
///
/// Quelli del database.
pub fn scegli_le_righe(
    connection: &Connection,
    ids: &[i64],
    raggiungibili: &HashSet<String>,
    uso: Uso,
) -> Result<RigheScelte, AppError> {
    let cosa = "righe da suonare o da scaricare";
    let righe = leggi_le_righe(connection, ids, cosa)?;
    let mut scelte = RigheScelte::default();
    for &id in ids {
        let Some(riga) = righe.get(&id) else {
            // Non la conosce: la tiene chi vuole suonarla, e non chi scarica (una
            // riga che non c'è non ha niente da scaricare).
            if uso == Uso::Suona {
                scelte.ids.push(id);
            }
            continue;
        };
        let Some(dispositivo) = riga.dispositivo.as_deref() else {
            // Una riga di qui: si suona, e non c'è niente da scaricare.
            if uso == Uso::Suona {
                scelte.ids.push(id);
            }
            continue;
        };
        let serve = !riga.in_grazia
            && (raggiungibili.contains(dispositivo) || (uso == Uso::Suona && riga.copia_qui));
        if serve {
            scelte.ids.push(id);
            scelte.lontane = scelte.lontane.saturating_add(1);
        } else {
            scelte.spente = scelte.spente.saturating_add(1);
            scelte.prima_spenta.get_or_insert(id);
        }
    }
    Ok(scelte)
}

/// Le righe fra `ids`, con quel che serve a scegliere, in una lettura sola.
///
/// `json_each` e non un `IN (?, ?, …)` per ciascuno: con duecentomila id i
/// segnaposto non bastano, e un elenco di numeri in una stringa sì.
fn leggi_le_righe(
    connection: &Connection,
    ids: &[i64],
    cosa: &str,
) -> Result<HashMap<i64, Riga>, AppError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let insieme = serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_owned());
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT t.id, t.path, t.copia_di IS t.id,
                    CASE WHEN {SQL_SPECCHIO}
                         THEN EXISTS (SELECT 1 FROM tracks h
                                       WHERE h.copia_di = t.id AND h.id <> t.id
                                         AND h.path IS NOT NULL
                                         AND NOT (h.path >= 'lontano://' AND h.path < 'lontano:/0'))
                         ELSE 0 END
               FROM tracks t
              WHERE t.id IN (SELECT value FROM json_each(?1))"
        ))
        .map_err(|err| db_error(cosa, &err))?;
    let lette = statement
        .query_map([&insieme], |r| {
            let id: i64 = r.get(0)?;
            let percorso: Option<String> = r.get(1)?;
            Ok((
                id,
                Riga {
                    dispositivo: percorso
                        .as_deref()
                        .and_then(parti)
                        .map(|(dispositivo, _)| dispositivo.to_owned()),
                    in_grazia: r.get::<_, i64>(2)? != 0,
                    copia_qui: r.get::<_, i64>(3)? != 0,
                },
            ))
        })
        .map_err(|err| db_error(cosa, &err))?;
    lette
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(|err| db_error(cosa, &err))
}

#[cfg(test)]
mod prove {
    use super::*;

    fn pc() -> String {
        "a".repeat(64)
    }

    fn spento() -> String {
        "b".repeat(64)
    }

    /// 1 e 2 di qui; 10 sul PC acceso; 20 sul PC spento; 21 sul PC spento ma con
    /// una copia di qui (22) dietro; 30 sul PC acceso ma che non la manda più.
    fn libreria() -> Connection {
        let c = crate::db::open_in_memory().expect("database").connection;
        let riga = |id: i64, percorso: String, copia_di: Option<i64>| {
            c.execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                     date_added, date_modified, copia_di)
                 VALUES (?1, ?2, ?3, 'T', 'A', 'B', 1, 1, 1, ?4)",
                rusqlite::params![id, percorso, format!("k{id}"), copia_di],
            )
            .expect("riga");
        };
        riga(1, "C:/Musica/uno.flac".to_owned(), None);
        riga(2, "C:/Musica/due.flac".to_owned(), None);
        riga(10, crate::specchio::percorso(&pc(), 10), None);
        riga(20, crate::specchio::percorso(&spento(), 20), None);
        riga(21, crate::specchio::percorso(&spento(), 21), None);
        riga(22, "C:/Musica/ventuno.mp3".to_owned(), Some(21));
        // In grazia: `copia_di` uguale al proprio `id`.
        riga(30, crate::specchio::percorso(&pc(), 30), Some(30));
        c
    }

    fn raggiungibili() -> HashSet<String> {
        HashSet::from([pc()])
    }

    #[test]
    fn per_suonare_restano_quelle_di_qui_quelle_che_rispondono_e_quelle_con_la_copia_di_qui() {
        let c = libreria();
        let ids = [1, 20, 10, 21, 30, 2];
        let scelte = scegli_le_righe(&c, &ids, &raggiungibili(), Uso::Suona).expect("scelte");
        assert_eq!(scelte.ids, vec![1, 10, 21, 2], "nell'ordine chiesto");
        assert_eq!(scelte.lontane, 2, "10 risponde, 21 ha la copia di qui");
        assert_eq!(scelte.spente, 2, "20 non risponde, 30 non c'è più per lui");
        assert_eq!(scelte.prima_spenta, Some(20));
    }

    #[test]
    fn per_scaricare_servono_solo_quelle_di_chi_risponde() {
        let c = libreria();
        let ids = [1, 20, 10, 21, 30, 2];
        let scelte = scegli_le_righe(&c, &ids, &raggiungibili(), Uso::Scarica).expect("scelte");
        assert_eq!(scelte.ids, vec![10], "una di qui non c'è da scaricare");
        assert_eq!(scelte.lontane, 1);
        assert_eq!(
            scelte.spente, 3,
            "20 e 21 stanno su un PC che tace — la copia di qui non le rende scaricabili —, 30 non c'è più"
        );
        assert_eq!(scelte.prima_spenta, Some(20));
    }

    #[test]
    fn niente_di_lontano_vuol_dire_niente_da_scaricare_e_niente_di_spento() {
        let c = libreria();
        let scelte = scegli_le_righe(&c, &[1, 2], &raggiungibili(), Uso::Scarica).expect("scelte");
        assert_eq!(scelte, RigheScelte::default());
        let scelte = scegli_le_righe(&c, &[], &raggiungibili(), Uso::Suona).expect("scelte");
        assert_eq!(scelte, RigheScelte::default());
    }

    #[test]
    fn una_riga_che_la_libreria_non_ha_si_lascia_a_chi_la_suona() {
        let c = libreria();
        let per_suonare =
            scegli_le_righe(&c, &[1, 999], &raggiungibili(), Uso::Suona).expect("scelte");
        assert_eq!(per_suonare.ids, vec![1, 999]);
        let per_scaricare =
            scegli_le_righe(&c, &[1, 999], &raggiungibili(), Uso::Scarica).expect("scelte");
        assert!(per_scaricare.ids.is_empty());
        assert_eq!(per_scaricare.spente, 0);
    }

    #[test]
    fn senza_dispositivi_che_rispondono_restano_solo_le_righe_di_qui() {
        let c = libreria();
        let scelte =
            scegli_le_righe(&c, &[1, 10, 20, 21, 2], &HashSet::new(), Uso::Suona).expect("scelte");
        assert_eq!(scelte.ids, vec![1, 21, 2]);
        assert_eq!(scelte.spente, 2);
    }
}
