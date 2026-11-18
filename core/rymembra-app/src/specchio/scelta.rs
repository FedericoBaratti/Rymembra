//! Quale copia di un brano si suona, e come si apre una riga specchio.
//!
//! # La copia
//!
//! Un brano che c'è più volte è **una voce sola** (`copia_di`), e si suona la
//! copia migliore — locale o in rete — **sempre**: il gruppo di una riga è il
//! suo rappresentante e le righe che gli stanno dietro. Fra quelle che si
//! possono suonare adesso vince la migliore ([`Qualita`]), a parità la locale.
//! Una copia in rete si può suonare se il dispositivo risponde **e** l'impostazione
//! «Dati mobili» lo consente (`ammessa`, che decide chi chiama: da qui la rete
//! non si vede). Il ripiego sulla copia locale vale quando la copia migliore è
//! di un dispositivo che non risponde, o su dati mobili con «No».
//!
//! Se non c'è niente da suonare, il messaggio è «<PC> non raggiungibile»:
//! [`Scelta::NonRaggiungibile`] dice di quale dispositivo.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension as _};
use rymembra_domain::errors::{AppError, ErrorCode};

use super::doppioni::Qualita;
use super::parti;
use crate::dispositivi::DatiMobili;
use crate::library::db_error;

/// Quel che serve ad aprire una riga specchio dalla rete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigaRemota {
    /// Il dispositivo a cui chiedere i byte.
    pub dispositivo: String,
    /// L'id nella sua libreria.
    pub id_remoto: i64,
    /// L'estensione, per il decodificatore.
    pub estensione: String,
    /// La versione del file, com'era all'ultimo catalogo.
    pub versione: String,
    /// L'impronta d'album del proprietario: due pezzi dello stesso disco
    /// d'immagine la portano uguale.
    pub album_impronta: Option<String>,
    /// Per un brano d'immagine CD, dove comincia (fotogrammi da 1/75 s).
    pub inizio_cd: Option<i64>,
    /// E dove finisce.
    pub fine_cd: Option<i64>,
}

/// La riga remota di una riga di `tracks`, se lo è.
///
/// # Errori
///
/// Quelli del database.
pub fn riga_remota(connection: &Connection, track_id: i64) -> Result<Option<RigaRemota>, AppError> {
    connection
        .query_row(
            "SELECT l.dispositivo, l.id_remoto, l.estensione, l.versione, l.album_impronta,
                    l.inizio_cd, l.fine_cd
               FROM lontano_brani l WHERE l.track_id = ?1",
            [track_id],
            |r| {
                Ok(RigaRemota {
                    dispositivo: r.get(0)?,
                    id_remoto: r.get(1)?,
                    estensione: r.get(2)?,
                    versione: r.get(3)?,
                    album_impronta: r.get(4)?,
                    inizio_cd: r.get(5)?,
                    fine_cd: r.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|err| db_error("riga remota", &err))
}

/// Il dispositivo e l'id remoto di una riga, dal suo percorso, se è una riga
/// specchio: per l'ascolto che va al proprietario.
///
/// # Errori
///
/// Quelli del database.
pub fn proprietario_di(
    connection: &Connection,
    track_id: i64,
) -> Result<Option<(String, i64)>, AppError> {
    let path: Option<Option<String>> = connection
        .query_row("SELECT path FROM tracks WHERE id = ?1", [track_id], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|err| db_error("percorso di una riga", &err))?;
    Ok(path
        .flatten()
        .as_deref()
        .and_then(parti)
        .map(|(dispositivo, id)| (dispositivo.to_owned(), id)))
}

/// Quale riga si suona.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scelta {
    /// Questa riga (può essere un'altra copia del brano).
    Suona(i64),
    /// Le copie del brano stanno tutte su dispositivi che adesso non si
    /// possono usare: il primo di loro, per dire «<PC> non raggiungibile».
    NonRaggiungibile(String),
}

/// Una riga del gruppo, per scegliere.
struct Candidata {
    id: i64,
    dispositivo: Option<String>,
    sparita: bool,
    qualita: Qualita,
}

/// La riga da suonare al posto di `id`: vedi il `//!`. `ammessa` dice se si
/// può usare un dispositivo, dalla sua chiave.
///
/// Chi non ha dispositivi collegati non paga niente: una `EXISTS` e basta.
///
/// # Errori
///
/// Quelli del database.
pub fn copia_da_suonare(
    connection: &Connection,
    id: i64,
    ammessa: &dyn Fn(&str) -> bool,
) -> Result<Scelta, AppError> {
    let cosa = "copia da suonare";
    let ci_sono_righe_remote: bool = connection
        .query_row("SELECT EXISTS (SELECT 1 FROM lontano_brani)", [], |r| {
            r.get(0)
        })
        .map_err(|err| db_error(cosa, &err))?;
    if !ci_sono_righe_remote {
        return Ok(Scelta::Suona(id));
    }
    let Some(copia_di) = connection
        .query_row("SELECT copia_di FROM tracks WHERE id = ?1", [id], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .optional()
        .map_err(|err| db_error(cosa, &err))?
    else {
        // La riga non c'è: se ne accorge chi la apre, col suo errore.
        return Ok(Scelta::Suona(id));
    };
    let radice = copia_di.unwrap_or(id);
    let candidate: Vec<Candidata> = {
        let mut st = connection
            .prepare_cached(
                "SELECT t.id, l.dispositivo, l.sparito_ms IS NOT NULL,
                        t.codec, t.sample_rate, t.bitrate
                   FROM tracks t LEFT JOIN lontano_brani l ON l.track_id = t.id
                  WHERE (t.id = ?1 OR t.copia_di = ?1) AND t.path IS NOT NULL",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map([radice], |r| {
                Ok(Candidata {
                    id: r.get(0)?,
                    dispositivo: r.get(1)?,
                    sparita: r.get::<_, Option<i64>>(2)?.unwrap_or(0) != 0,
                    qualita: Qualita::da_colonne(
                        r.get::<_, Option<String>>(3)?.as_deref(),
                        r.get(4)?,
                        r.get(5)?,
                    ),
                })
            })
            .map_err(|err| db_error(cosa, &err))?;
        lette
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    let usabili = candidate.iter().filter(|c| match &c.dispositivo {
        None => true,
        Some(dispositivo) => !c.sparita && ammessa(dispositivo),
    });
    // La migliore; a parità la locale, poi l'id più basso.
    let migliore =
        usabili.max_by_key(|c| (c.qualita, c.dispositivo.is_none(), std::cmp::Reverse(c.id)));
    if let Some(migliore) = migliore {
        return Ok(Scelta::Suona(migliore.id));
    }
    // Niente da suonare: tutte le copie sono di dispositivi che adesso non si
    // possono usare. Il primo di loro, per dire «<PC> non raggiungibile».
    Ok(match candidate.iter().find_map(|c| c.dispositivo.clone()) {
        Some(dispositivo) => Scelta::NonRaggiungibile(dispositivo),
        None => Scelta::Suona(id),
    })
}

/// La rete su cui si suona, per l'impostazione «Dati mobili».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rete {
    /// Wi-Fi, cavo: una copia in rete costa niente.
    Libera,
    /// Dati mobili, o una rete a consumo: vale la scelta della persona.
    AConsumo(DatiMobili),
}

/// La riga da suonare al posto di `id`, con la rete di adesso: vedi il `//!` e
/// l'impostazione «Dati mobili».
///
/// - **Sì**, o una rete libera: sempre la migliore.
/// - **No**, su rete a consumo: una copia in rete vale come irraggiungibile, e si
///   suona quella di qui; senza, è `lontano.reteAConsumo`.
/// - **Chiedi**, su rete a consumo, quando la migliore è in rete: la domanda di
///   oggi, una volta — `lontano.reteAConsumo`, che la finestra trasforma nella
///   domanda, e la risposta diventa l'impostazione. Se la migliore è di qui, si
///   suona senza chiedere niente.
///
/// `presenti` sono i dispositivi che rispondono adesso.
///
/// # Errori
///
/// `lontano.irraggiungibile` se tutte le copie sono di dispositivi che non
/// rispondono; `lontano.reteAConsumo` come sopra; quelli del database.
pub fn scegli_con_la_rete(
    connection: &Connection,
    id: i64,
    presenti: &HashSet<String>,
    rete: Rete,
) -> Result<i64, AppError> {
    let migliore = match copia_da_suonare(connection, id, &|d| presenti.contains(d))? {
        Scelta::Suona(riga) => riga,
        Scelta::NonRaggiungibile(_) => {
            return Err(AppError::new(ErrorCode::LontanoIrraggiungibile)
                .with_cause("tutte le copie del brano stanno su dispositivi che non rispondono"));
        }
    };
    let Rete::AConsumo(scelta) = rete else {
        return Ok(migliore);
    };
    if scelta == DatiMobili::Si || riga_remota(connection, migliore)?.is_none() {
        return Ok(migliore);
    }
    // Una copia in rete su dati mobili che non si possono usare.
    let a_consumo = || AppError::new(ErrorCode::LontanoReteAConsumo);
    match scelta {
        DatiMobili::Chiedi | DatiMobili::Si => Err(a_consumo()),
        DatiMobili::No => match copia_da_suonare(connection, id, &|_| false)? {
            Scelta::Suona(locale) => Ok(locale),
            Scelta::NonRaggiungibile(_) => Err(a_consumo()),
        },
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::specchio::comuni::{chiave, libreria};

    fn riga_locale(c: &Connection, id: i64, codec: &str, copia_di: Option<i64>) {
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size, date_added,
                                 date_modified, codec, bitrate, sample_rate, copia_di)
             VALUES (?1, ?2, 'k', 'T', 'A', 'B', 1, 1, 1, ?3, 320, 44100, ?4)",
            rusqlite::params![id, format!("C:/M/{id}"), codec, copia_di],
        )
        .unwrap();
    }

    fn riga_remota_di(
        c: &Connection,
        id: i64,
        dispositivo: &str,
        codec: &str,
        copia_di: Option<i64>,
        sparita: bool,
    ) {
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size, date_added,
                                 date_modified, source, codec, bitrate, sample_rate, copia_di)
             VALUES (?1, ?2, 'k', 'T', 'A', 'B', 1, 1, 1, 'lontano', ?3, 320, 44100, ?4)",
            rusqlite::params![
                id,
                crate::specchio::percorso(dispositivo, id + 1000),
                codec,
                copia_di
            ],
        )
        .unwrap();
        c.execute(
            "INSERT INTO lontano_brani (track_id, dispositivo, id_remoto, estensione, versione,
                                        album_impronta, inizio_cd, fine_cd, sparito_ms)
             VALUES (?1, ?2, ?3, 'flac', '9-9', 'ab', 750, 1500, ?4)",
            rusqlite::params![id, dispositivo, id + 1000, sparita.then_some(5)],
        )
        .unwrap();
    }

    #[test]
    fn senza_dispositivi_si_suona_la_riga_chiesta_e_basta() {
        let c = libreria();
        riga_locale(&c, 1, "Mpeg", None);
        assert_eq!(
            copia_da_suonare(&c, 1, &|_| true).unwrap(),
            Scelta::Suona(1)
        );
        // Una riga che non c'è: se ne accorge chi apre.
        assert_eq!(
            copia_da_suonare(&c, 99, &|_| true).unwrap(),
            Scelta::Suona(99)
        );
    }

    #[test]
    fn la_copia_migliore_in_rete_si_suona_sempre_quando_risponde() {
        // La locale (1) sta dietro la flac del PC (2).
        let c = libreria();
        let pc = chiave('a');
        riga_locale(&c, 1, "Mpeg", Some(2));
        riga_remota_di(&c, 2, &pc, "Flac", None, false);
        for chiesta in [1, 2] {
            assert_eq!(
                copia_da_suonare(&c, chiesta, &|_| true).unwrap(),
                Scelta::Suona(2),
                "chiesta la {chiesta}"
            );
        }
    }

    #[test]
    fn col_pc_spento_si_ripiega_sulla_copia_locale() {
        let c = libreria();
        let pc = chiave('a');
        riga_locale(&c, 1, "Mpeg", Some(2));
        riga_remota_di(&c, 2, &pc, "Flac", None, false);
        // La voce è la remota (id 2): il PC non risponde.
        assert_eq!(
            copia_da_suonare(&c, 2, &|_| false).unwrap(),
            Scelta::Suona(1)
        );
        assert_eq!(
            copia_da_suonare(&c, 1, &|_| false).unwrap(),
            Scelta::Suona(1)
        );
    }

    #[test]
    fn sui_dati_mobili_con_no_la_copia_remota_vale_come_irraggiungibile() {
        // Chi chiama dice «non ammessa» per quel dispositivo (rete a consumo,
        // impostazione «No»): si suona la locale, anche se è peggiore.
        let c = libreria();
        let pc = chiave('b');
        riga_locale(&c, 1, "Mpeg", Some(2));
        riga_remota_di(&c, 2, &pc, "Flac", None, false);
        let pc2 = pc.clone();
        assert_eq!(
            copia_da_suonare(&c, 2, &move |d| d != pc2).unwrap(),
            Scelta::Suona(1)
        );
    }

    #[test]
    fn senza_copia_locale_e_col_pc_spento_si_dice_quale_pc() {
        let c = libreria();
        let pc = chiave('c');
        riga_remota_di(&c, 2, &pc, "Flac", None, false);
        assert_eq!(
            copia_da_suonare(&c, 2, &|_| false).unwrap(),
            Scelta::NonRaggiungibile(pc)
        );
    }

    #[test]
    fn una_riga_in_attesa_di_sparire_non_si_suona() {
        let c = libreria();
        let pc = chiave('d');
        riga_locale(&c, 1, "Mpeg", Some(2));
        riga_remota_di(&c, 2, &pc, "Flac", None, true);
        assert_eq!(
            copia_da_suonare(&c, 2, &|_| true).unwrap(),
            Scelta::Suona(1)
        );
    }

    #[test]
    fn a_parita_si_suona_la_locale_e_con_la_locale_migliore_anche() {
        let c = libreria();
        let pc = chiave('e');
        riga_locale(&c, 1, "Flac", None);
        riga_remota_di(&c, 2, &pc, "Flac", Some(1), false);
        assert_eq!(
            copia_da_suonare(&c, 2, &|_| true).unwrap(),
            Scelta::Suona(1)
        );
        // Due locali dello stesso brano, non legate, sono due righe: ognuna
        // suona se stessa.
        riga_locale(&c, 3, "Mpeg", None);
        assert_eq!(
            copia_da_suonare(&c, 3, &|_| true).unwrap(),
            Scelta::Suona(3)
        );
    }

    #[test]
    fn la_riga_remota_dice_come_aprirsi_e_di_chi_e() {
        let c = libreria();
        let pc = chiave('f');
        riga_locale(&c, 1, "Mpeg", None);
        riga_remota_di(&c, 2, &pc, "Flac", None, false);
        assert_eq!(riga_remota(&c, 1).unwrap(), None);
        let r = riga_remota(&c, 2).unwrap().unwrap();
        assert_eq!((r.dispositivo.as_str(), r.id_remoto), (pc.as_str(), 1002));
        assert_eq!(
            (r.estensione.as_str(), r.inizio_cd, r.fine_cd),
            ("flac", Some(750), Some(1500))
        );
        assert_eq!(proprietario_di(&c, 2).unwrap(), Some((pc, 1002)));
        assert_eq!(proprietario_di(&c, 1).unwrap(), None);
        assert_eq!(proprietario_di(&c, 99).unwrap(), None);
    }

    fn presenti(c: char) -> HashSet<String> {
        HashSet::from([chiave(c)])
    }

    #[test]
    fn sulla_rete_libera_o_con_si_si_suona_sempre_la_migliore() {
        let c = libreria();
        let pc = chiave('a');
        riga_locale(&c, 1, "Mpeg", Some(2));
        riga_remota_di(&c, 2, &pc, "Flac", None, false);
        for rete in [Rete::Libera, Rete::AConsumo(DatiMobili::Si)] {
            assert_eq!(
                scegli_con_la_rete(&c, 1, &presenti('a'), rete).unwrap(),
                2,
                "{rete:?}"
            );
        }
    }

    #[test]
    fn sui_dati_mobili_con_no_si_suona_la_copia_del_telefono_anche_se_peggiore() {
        let c = libreria();
        let pc = chiave('a');
        riga_locale(&c, 1, "Mpeg", Some(2));
        riga_remota_di(&c, 2, &pc, "Flac", None, false);
        assert_eq!(
            scegli_con_la_rete(&c, 2, &presenti('a'), Rete::AConsumo(DatiMobili::No)).unwrap(),
            1
        );
        // E se non c'è, il messaggio dei dati mobili.
        let d = libreria();
        riga_remota_di(&d, 2, &pc, "Flac", None, false);
        let err =
            scegli_con_la_rete(&d, 2, &presenti('a'), Rete::AConsumo(DatiMobili::No)).unwrap_err();
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoReteAConsumo
        );
    }

    #[test]
    fn sui_dati_mobili_con_chiedi_si_chiede_solo_se_la_migliore_e_in_rete() {
        let c = libreria();
        let pc = chiave('a');
        riga_locale(&c, 1, "Mpeg", Some(2));
        riga_remota_di(&c, 2, &pc, "Flac", None, false);
        let err = scegli_con_la_rete(&c, 1, &presenti('a'), Rete::AConsumo(DatiMobili::Chiedi))
            .unwrap_err();
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoReteAConsumo
        );
        // Se la migliore è di qui, niente da chiedere.
        let d = libreria();
        riga_locale(&d, 1, "Flac", None);
        riga_remota_di(&d, 2, &pc, "Mpeg", Some(1), false);
        assert_eq!(
            scegli_con_la_rete(&d, 2, &presenti('a'), Rete::AConsumo(DatiMobili::Chiedi)).unwrap(),
            1
        );
    }

    #[test]
    fn con_il_pc_spento_si_ripiega_e_senza_copie_si_dice_irraggiungibile() {
        let c = libreria();
        let pc = chiave('a');
        riga_locale(&c, 1, "Mpeg", Some(2));
        riga_remota_di(&c, 2, &pc, "Flac", None, false);
        let nessuno = HashSet::new();
        assert_eq!(
            scegli_con_la_rete(&c, 2, &nessuno, Rete::Libera).unwrap(),
            1
        );
        let d = libreria();
        riga_remota_di(&d, 2, &pc, "Flac", None, false);
        let err = scegli_con_la_rete(&d, 2, &nessuno, Rete::Libera).unwrap_err();
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoIrraggiungibile
        );
    }
}
