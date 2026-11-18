//! La libreria unica: i brani degli altri dispositivi collegati, come righe di
//! `tracks`. La parte che non vede la rete.
//!
//! # Il modello
//!
//! Ogni brano che un dispositivo collegato condivide è una **riga specchio**,
//! con `path = 'lontano://<chiave 64hex del dispositivo>/<id nella sua
//! libreria>'` e `source = 'lontano'`: vedi la migrazione 043. Quel che una
//! riga così ha in più di una riga vera sta in `lontano_brani`. Le statistiche
//! — ascolti, voto, preferito — sono una copia in sola lettura di quelle del
//! proprietario, che le conta e fa lo scrobble: qui non si sommano mai.
//!
//! Questo modulo è la parte che non ha rete né filo: i percorsi, il filtro per
//! origine ([`Origine`]), la validazione di tutto quel che arriva da un altro
//! dispositivo ([`convalida`]), il riconciliatore ([`archivio`]) e la scelta
//! della copia da mostrare quando un brano c'è più volte ([`doppioni`]). Chi
//! chiede il catalogo all'altro, e quando, sta in
//! `src-tauri/src/lontano/specchio.rs`.
//!
//! # Dove una riga specchio NON deve entrare
//!
//! Il catalogo di un altro dispositivo è musica di un altro dispositivo.
//! L'elenco dei posti da cui sta fuori — e la decisione presa per ciascuno —
//! è in [`invarianti`]: una prova per ogni voce, perché è l'elenco che
//! manca di un elemento quando se ne aggiunge uno nuovo, e lo si scopre
//! con le statistiche contate due volte.

pub mod archivio;
#[cfg(test)]
#[path = "specchio/prove_comuni.rs"]
pub(crate) mod comuni;
pub mod convalida;
pub mod doppioni;
#[cfg(test)]
#[path = "specchio/prove_gesti.rs"]
mod gesti;
#[cfg(test)]
mod invarianti;
#[cfg(test)]
#[path = "specchio/prove_rev3.rs"]
mod rev3;
#[cfg(test)]
#[path = "specchio/prove_revisione.rs"]
mod revisione;
pub mod scarico;
pub mod scelta;
pub mod valutazioni;

use rymembra_domain::errors::{AppError, ErrorCode};

/// Il prefisso di ogni riga specchio.
pub const PREFISSO: &str = "lontano://";

/// Il valore di `tracks.source` di una riga specchio.
pub const SORGENTE: &str = "lontano";

/// Una riga specchio, per `tracks` con l'alias `t`: l'intervallo di prefisso
/// `[lontano://, lontano:/0)`, come per il cloud (`'/' + 1` è `'0'`) — che
/// l'indice di `path` sa servire.
pub const SQL_SPECCHIO: &str = "(t.path >= 'lontano://' AND t.path < 'lontano:/0')";

/// Una riga di questa macchina, per `tracks` con l'alias `t`: né una riga
/// specchio né, per chi le tiene in un'altra forma, un brano di catalogo
/// (che il suo `path` non ce l'ha).
pub const SQL_NON_SPECCHIO: &str =
    "(t.path IS NULL OR t.path < 'lontano://' OR t.path >= 'lontano:/0')";

/// Lo stesso di [`SQL_SPECCHIO`], per `tracks` senza alias.
pub const SQL_SPECCHIO_SENZA_ALIAS: &str = "(path >= 'lontano://' AND path < 'lontano:/0')";

/// Lo stesso di [`SQL_NON_SPECCHIO`], per `tracks` senza alias.
pub const SQL_NON_SPECCHIO_SENZA_ALIAS: &str =
    "(path IS NULL OR path < 'lontano://' OR path >= 'lontano:/0')";

/// È una riga specchio?
#[must_use]
pub fn e_specchio(percorso: &str) -> bool {
    percorso.starts_with(PREFISSO)
}

/// È la chiave pubblica di un dispositivo: sessantaquattro cifre esadecimali
/// minuscole, come la scrive iroh e come la vuole `dispositivi.chiave`?
#[must_use]
pub fn e_una_chiave(chiave: &str) -> bool {
    chiave.len() == 64
        && chiave
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Il percorso di una riga specchio.
#[must_use]
pub fn percorso(dispositivo: &str, id_remoto: i64) -> String {
    format!("{PREFISSO}{dispositivo}/{id_remoto}")
}

/// Il dispositivo e l'id remoto di un percorso specchio, se ha la forma che
/// [`percorso`] scrive: una chiave valida, e un id positivo senza segno né zeri
/// in testa. Qualunque altra cosa — un percorso `lontano://` scritto da un
/// profilo costruito a mano — non è una riga che il riconciliatore conosca.
#[must_use]
pub fn parti(percorso: &str) -> Option<(&str, i64)> {
    let resto = percorso.strip_prefix(PREFISSO)?;
    let (dispositivo, id) = resto.split_once('/')?;
    if !e_una_chiave(dispositivo) || id.is_empty() || id.starts_with('0') {
        return None;
    }
    if !id.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let id: i64 = id.parse().ok()?;
    (id > 0).then_some((dispositivo, id))
}

/// Da dove si guarda la libreria: il filtro per dispositivo.
///
/// - **Tutti**: una voce per brano, le copie nascoste (`copia_di IS NULL`);
/// - **Locale**: tutte le righe di questa macchina, anche quelle che in «Tutti»
///   stanno dietro una copia migliore di un altro dispositivo;
/// - **Dispositivo**: tutte le righe di quel dispositivo.
///
/// `copia_di IS NULL` vale **ovunque** si mostra «Tutti»: altrimenti i
/// doppioni rientrano dalla porta di servizio.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "tipo", content = "chiave", rename_all = "camelCase")]
pub enum Origine {
    /// Tutta la libreria unica.
    #[default]
    Tutti,
    /// Solo questa macchina.
    Locale,
    /// Solo questo dispositivo, dalla sua chiave.
    Dispositivo(String),
    /// «Tutti», senza le righe dei dispositivi che adesso non rispondono: per
    /// chi non può mostrare una riga grigia — Android Auto, dove un brano che
    /// non parte al volante è un guasto. Le chiavi sono quelle dei dispositivi
    /// **raggiungibili**.
    ///
    /// Una copia di qui che «Tutti» nasconde perché un altro dispositivo ne ha
    /// una migliore resta visibile quando quel dispositivo non risponde: il
    /// brano non sparisce dall'auto perché il PC è spento. Lo stesso per la
    /// copia di un **altro** dispositivo che risponde — se il rappresentante
    /// tace e non c'è una copia di qui — e una sola per brano. Non viaggia
    /// sull'IPC: la finestra non sa chi è raggiungibile, lo sa chi risponde
    /// all'auto.
    #[serde(skip)]
    Raggiungibili(Vec<String>),
}

/// Da dove viene una riga: per leggere **il suo album** senza che «Tutti» ne
/// nasconda dei pezzi. Una riga di questa macchina vale [`Origine::Locale`], una
/// riga specchio il suo dispositivo.
///
/// Un album ha un'origine sola: le righe locali hanno la chiave di una cartella
/// di qui, quelle specchio la chiave dell'album di un dispositivo.
#[must_use]
pub fn origine_della_riga(percorso: Option<&str>) -> Origine {
    percorso
        .and_then(parti)
        .map_or(Origine::Locale, |(dispositivo, _)| {
            Origine::Dispositivo(dispositivo.to_owned())
        })
}

/// Le righe di questa macchina, per `tracks` con l'alias dato (`"t."`, `"r."`
/// o vuoto).
fn di_questa_macchina(alias: &str) -> String {
    format!("({alias}path IS NULL OR {alias}path < 'lontano://' OR {alias}path >= 'lontano:/0')")
}

/// Le righe di un dispositivo: un intervallo di prefisso, che l'indice di `path`
/// sa servire.
///
/// **Meno quelle in grazia**: una riga che il proprietario non manda più resta
/// per qualche giorno in libreria (`archivio::GRAZIA_MS`) con `copia_di` uguale
/// al proprio `id`, e non si suona né si conta da nessuna parte — nemmeno nel
/// filtro del dispositivo. `IS NOT` e non `<>`: per le righe senza copia
/// (`copia_di` NULL) il confronto è vero.
///
/// # Errori
///
/// `ipc.payloadInvalid` per una chiave che non è di 64 cifre esadecimali:
/// finisce dentro l'SQL, e per un intervallo di prefisso non c'è un'altra
/// strada.
fn del_dispositivo(chiave: &str, alias: &str) -> Result<String, AppError> {
    if !e_una_chiave(chiave) {
        return Err(AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: "libreria".to_owned(),
            detail: Some("l'origine non è la chiave di un dispositivo".to_owned()),
        }));
    }
    Ok(format!(
        "({alias}path >= 'lontano://{chiave}/' AND {alias}path < 'lontano://{chiave}0'
          AND {alias}copia_di IS NOT {alias}id)"
    ))
}

impl Origine {
    /// La condizione su `tracks`; `prefisso` è l'alias con il punto (`"t."`) o
    /// vuoto.
    ///
    /// # Errori
    ///
    /// `ipc.payloadInvalid` per una chiave di dispositivo che non è di 64 cifre
    /// esadecimali.
    pub fn sql(&self, prefisso: &str) -> Result<String, AppError> {
        Ok(match self {
            // «Tutti»: le righe che nessuna copia migliore (o la grazia) nasconde. Come
            // `NOT IN` dei nascosti e non `copia_di IS NULL`: i nascosti sono pochi, e
            // l'indice parziale di `copia_di` li dà da solo, mentre il confronto
            // riga per riga costringeva a leggere ogni riga di `tracks` anche dove
            // l'indice d'ordine da solo bastava (id_dei_brani: tre volte più lento).
            Self::Tutti => {
                format!("{prefisso}id NOT IN (SELECT id FROM tracks WHERE copia_di IS NOT NULL)")
            }
            Self::Locale => di_questa_macchina(prefisso),
            Self::Dispositivo(chiave) => del_dispositivo(chiave, prefisso)?,
            Self::Raggiungibili(chiavi) => {
                // Dentro una sottoquery `copia_di` e `id` da soli si risolvono
                // sulla riga **più interna**: senza alias per la riga di fuori
                // (`list_artists`, `albums_by_artist`) il ripiego sulla copia di
                // qui non scattava mai. Con la tabella per nome la riga di fuori
                // si nomina lo stesso: le sottoquery danno alle loro un alias.
                let fuori = if prefisso.is_empty() {
                    "tracks."
                } else {
                    prefisso
                };
                let sue = chiavi
                    .iter()
                    .map(|chiave| del_dispositivo(chiave, prefisso))
                    .collect::<Result<Vec<_>, _>>()?;
                // Le righe dei dispositivi che rispondono, per una tabella con
                // l'alias dato.
                let risponde = |alias: &str| -> Result<String, AppError> {
                    Ok(chiavi
                        .iter()
                        .map(|chiave| del_dispositivo(chiave, alias))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(" OR "))
                };
                // Il rappresentante di una copia sta su un dispositivo che non
                // risponde.
                let risponde_r = risponde("r.")?;
                let rappresentante_tace = if risponde_r.is_empty() {
                    format!("EXISTS (SELECT 1 FROM tracks r WHERE r.id = {fuori}copia_di)")
                } else {
                    format!(
                        "EXISTS (SELECT 1 FROM tracks r
                                  WHERE r.id = {fuori}copia_di
                                    AND NOT ({risponde_r}))"
                    )
                };
                // Una riga di questa macchina: visibile se «Tutti» la mostra, o
                // se la copia che la nasconde sta su un dispositivo che non
                // risponde.
                let qui = format!(
                    "({} AND ({prefisso}copia_di IS NULL OR {rappresentante_tace}))",
                    di_questa_macchina(prefisso)
                );
                if sue.is_empty() {
                    qui
                } else {
                    // La riga di un dispositivo che risponde: visibile se «Tutti»
                    // la mostra; o, se il rappresentante tace, se è la copia che
                    // si suonerebbe — non quando ce n'è una di qui, né un'altra di
                    // un dispositivo che risponde con l'id più basso. Il brano
                    // non sparisce dall'auto perché il PC migliore è spento.
                    let copia_di_qui = format!(
                        "NOT EXISTS (SELECT 1 FROM tracks x
                                      WHERE x.copia_di = {fuori}copia_di AND {})",
                        di_questa_macchina("x.")
                    );
                    let risponde_x = risponde("x.")?;
                    let altra_raggiungibile = format!(
                        "NOT EXISTS (SELECT 1 FROM tracks x
                                      WHERE x.copia_di = {fuori}copia_di
                                        AND x.id < {fuori}id AND ({risponde_x}))"
                    );
                    format!(
                        "({qui} OR (({}) AND ({prefisso}copia_di IS NULL
                            OR ({rappresentante_tace} AND {copia_di_qui} AND {altra_raggiungibile}))))",
                        sue.join(" OR ")
                    )
                }
            }
        })
    }

    /// È «Tutti»?
    #[must_use]
    pub const fn e_tutti(&self) -> bool {
        matches!(self, Self::Tutti)
    }

    /// La riga di questo `percorso` si può suonare adesso?
    ///
    /// Per chi ha già una lista in mano — le righe di una playlist, che la
    /// nomina una per una — e non può mettere la condizione nell'SQL. Una riga
    /// di questa macchina vale sempre; una riga specchio vale se il suo
    /// dispositivo è fra le [`Origine::Raggiungibili`]. Le altre origini non
    /// parlano di rete, e ammettono tutto.
    #[must_use]
    pub fn raggiunge(&self, percorso: Option<&str>) -> bool {
        match self {
            Self::Raggiungibili(chiavi) => percorso
                .and_then(parti)
                .is_none_or(|(dispositivo, _)| chiavi.iter().any(|c| c == dispositivo)),
            _ => true,
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn chiave(c: char) -> String {
        c.to_string().repeat(64)
    }

    #[test]
    fn il_percorso_si_scrive_e_si_rilegge() {
        let k = chiave('a');
        let p = percorso(&k, 4120);
        assert_eq!(p, format!("lontano://{k}/4120"));
        assert!(e_specchio(&p));
        assert_eq!(parti(&p), Some((k.as_str(), 4120)));
        assert!(!e_specchio("C:/Musica/a.flac"));
        assert!(!e_specchio("nuvola://onedrive-abcdefgh/Musica/a.flac"));
    }

    #[test]
    fn un_percorso_che_non_e_la_forma_vera_non_e_una_riga_che_si_conosca() {
        let k = chiave('b');
        for storto in [
            "lontano://".to_owned(),
            format!("lontano://{k}"),
            format!("lontano://{k}/"),
            format!("lontano://{k}/0"),
            format!("lontano://{k}/-3"),
            format!("lontano://{k}/+3"),
            format!("lontano://{k}/007"),
            format!("lontano://{k}/12/13"),
            format!("lontano://{k}/99999999999999999999"),
            format!("lontano://{}/3", chiave('A')),
            format!("lontano://{}/3", "a".repeat(63)),
            format!("lontano://{k}\\3"),
            format!("lontano://{k}/../3"),
        ] {
            assert_eq!(parti(&storto), None, "{storto}");
        }
    }

    #[test]
    fn l_intervallo_di_prefisso_prende_le_righe_specchio_e_nient_altro() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE tracks (id INTEGER PRIMARY KEY, path TEXT, copia_di INTEGER)",
        )
        .unwrap();
        let k = chiave('c');
        for (id, p) in [
            (1, "C:/Musica/a.flac".to_owned()),
            (2, "nuvola://onedrive-abcdefgh/a.flac".to_owned()),
            (3, percorso(&k, 7)),
            (4, "lontano:/dispositivo".to_owned()),
            (5, "lontano:0".to_owned()),
            (6, "lontano".to_owned()),
            (7, "Lontano://x".to_owned()),
        ] {
            c.execute(
                "INSERT INTO tracks (id, path) VALUES (?1, ?2)",
                rusqlite::params![id, p],
            )
            .unwrap();
        }
        // Una riga di quel dispositivo in grazia (il proprietario non la manda
        // più): `copia_di` uguale al proprio id, e fuori da ogni elenco.
        c.execute(
            "INSERT INTO tracks (id, path, copia_di) VALUES (8, ?1, 8)",
            [percorso(&k, 9)],
        )
        .unwrap();
        let ids = |condizione: &str| -> Vec<i64> {
            let mut q = c
                .prepare(&format!(
                    "SELECT t.id FROM tracks t WHERE {condizione} ORDER BY t.id"
                ))
                .unwrap();
            q.query_map([], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(ids(SQL_SPECCHIO), vec![3, 8]);
        assert_eq!(ids(SQL_NON_SPECCHIO), vec![1, 2, 4, 5, 6, 7]);
        assert_eq!(
            ids(&Origine::Locale.sql("t.").unwrap()),
            vec![1, 2, 4, 5, 6, 7]
        );
        assert_eq!(
            ids(&Origine::Dispositivo(k.clone()).sql("t.").unwrap()),
            vec![3]
        );
        assert_eq!(
            ids(&Origine::Dispositivo(chiave('d')).sql("t.").unwrap()),
            Vec::<i64>::new()
        );
    }

    #[test]
    fn raggiungibili_nasconde_le_righe_di_chi_tace_e_non_nasconde_la_copia_di_qui() {
        // Tre dispositivi: il PC (rappresentante migliore del brano 11), il
        // portatile (brano 21, solo suo) e il tablet. Il PC non risponde.
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE tracks (id INTEGER PRIMARY KEY, path TEXT, copia_di INTEGER)",
        )
        .unwrap();
        let (pc, portatile, tablet) = (chiave('a'), chiave('b'), chiave('c'));
        for (id, p, copia) in [
            (1, "C:/M/a.flac".to_owned(), None),
            // Il brano 11 sta in due posti: la copia di qui è dietro quella del PC.
            (2, "C:/M/b.mp3".to_owned(), Some(10)),
            (10, percorso(&pc, 11), None),
            (20, percorso(&portatile, 21), None),
            // Il tablet ha una copia nascosta dietro quella del portatile, che
            // risponde: resta nascosta.
            (30, percorso(&tablet, 21), Some(20)),
        ] {
            c.execute(
                "INSERT INTO tracks VALUES (?1, ?2, ?3)",
                rusqlite::params![id, p, copia],
            )
            .unwrap();
        }
        let ids = |origine: &Origine| -> Vec<i64> {
            let mut q = c
                .prepare(&format!(
                    "SELECT t.id FROM tracks t WHERE {} ORDER BY t.id",
                    origine.sql("t.").unwrap()
                ))
                .unwrap();
            q.query_map([], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(ids(&Origine::Tutti), vec![1, 10, 20]);
        // Risponde il portatile, il PC no: i brani del PC spariscono, e la copia
        // di qui del brano 11 — nascosta dietro il PC — torna.
        assert_eq!(
            ids(&Origine::Raggiungibili(vec![portatile.clone()])),
            vec![1, 2, 20]
        );
        // Risponde anche il PC: è «Tutti».
        assert_eq!(
            ids(&Origine::Raggiungibili(vec![pc.clone(), portatile.clone()])),
            vec![1, 10, 20]
        );
        // Non risponde nessuno: resta quel che c'è di qui, anche se nascosto.
        assert_eq!(ids(&Origine::Raggiungibili(Vec::new())), vec![1, 2]);
        // Una chiave storta non entra nell'SQL.
        assert!(
            Origine::Raggiungibili(vec!["x'; --".to_owned()])
                .sql("t.")
                .is_err()
        );
        // E non viaggia sull'IPC.
        assert!(serde_json::to_string(&Origine::Raggiungibili(Vec::new())).is_err());
    }

    #[test]
    fn un_origine_che_non_e_una_chiave_non_finisce_nell_sql() {
        for storta in [
            "",
            "x",
            "'; DROP TABLE tracks; --",
            &"A".repeat(64),
            &"a".repeat(65),
        ] {
            let errore = Origine::Dispositivo(storta.to_owned())
                .sql("t.")
                .unwrap_err();
            assert_eq!(
                errore.code().kind(),
                rymembra_domain::errors::ErrorCodeKind::IpcPayloadInvalid,
                "{storta}"
            );
        }
        assert_eq!(
            Origine::Tutti.sql("t.").unwrap(),
            "t.id NOT IN (SELECT id FROM tracks WHERE copia_di IS NOT NULL)"
        );
        assert_eq!(
            Origine::Tutti.sql("").unwrap(),
            "id NOT IN (SELECT id FROM tracks WHERE copia_di IS NOT NULL)"
        );
    }

    #[test]
    fn l_origine_viaggia_come_json() {
        let k = chiave('e');
        for (origine, json) in [
            (Origine::Tutti, r#"{"tipo":"tutti"}"#.to_owned()),
            (Origine::Locale, r#"{"tipo":"locale"}"#.to_owned()),
            (
                Origine::Dispositivo(k.clone()),
                format!(r#"{{"tipo":"dispositivo","chiave":"{k}"}}"#),
            ),
        ] {
            assert_eq!(serde_json::to_string(&origine).unwrap(), json);
            assert_eq!(serde_json::from_str::<Origine>(&json).unwrap(), origine);
        }
    }
}
