//! Eseguire un riordino, e poterlo disfare.
//!
//! La decisione sta in [`aether_domain::organize`]. Qui si sposta, e l'unica
//! cosa che conta davvero è che si possa **tornare indietro**.
//!
//! # Il giornale
//!
//! Ogni spostamento riuscito viene scritto su disco **subito**, una riga per
//! volta, e sincronizzato. Non un elenco costruito in memoria e salvato alla
//! fine: se la corrente va via a metà dei millesettecento file, un elenco in
//! memoria sparisce e restano dei file spostati che nessuno sa più rimettere a
//! posto. Una riga già scritta invece resta.
//!
//! Il formato è una riga JSON per spostamento. Un file troncato a metà riga si
//! legge fino alla penultima e si usa lo stesso — che è precisamente il caso di
//! un'interruzione improvvisa.
//!
//! # Non si sovrascrive mai
//!
//! `rename` su Windows sovrascrive la destinazione senza dire niente. Qui la
//! destinazione si controlla prima, e se è occupata lo spostamento non avviene.
//! Fra un controllo e un rinomina c'è una finestra in cui il mondo può cambiare;
//! è accettabile per un'operazione che l'utente ha appena chiesto guardandola, e
//! non lo sarebbe per qualcosa che gira da solo in sottofondo.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::organize::{Move, OrganizePlan, TrackToOrganize};
use rusqlite::Connection;

use crate::files::io_error;
use crate::library::db_error;

/// I brani della libreria, nella forma che il piano di riordino accetta.
///
/// # Perché dal database e non dai file
///
/// Gli esempi a riga di comando camminano sul disco e rileggono i tag di ogni
/// file. Va bene per uno strumento diagnostico; dentro l'applicazione sarebbero
/// venti secondi di attesa — la durata di una scansione intera — per mostrare
/// un'anteprima, e su dati che la scansione ha già letto e messo in tabella.
///
/// L'`id` che ne esce è quello vero della riga, non un indice: il giornale lo
/// conserva, e questo lascia aperta la strada per aggiornare i percorsi senza
/// una riscansione, il giorno in cui servisse.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub fn tracks_to_organize(connection: &Connection) -> Result<Vec<TrackToOrganize>, AppError> {
    let mut statement = connection
        .prepare("SELECT id, path, album, album_artist, artist FROM tracks ORDER BY id")
        .map_err(|err| db_error("brani da riordinare", &err))?;
    let rows = statement
        .query_map([], |row| {
            Ok(TrackToOrganize {
                id: row.get(0)?,
                path: row.get(1)?,
                album: row.get(2)?,
                album_artist: row.get(3)?,
                artist: row.get(4)?,
            })
        })
        .map_err(|err| db_error("brani da riordinare", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("brani da riordinare", &err))
}

/// Una riga del giornale: uno spostamento che è avvenuto per davvero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    /// La riga di libreria coinvolta.
    pub track_id: i64,
    /// Dove stava.
    pub from: String,
    /// Dove sta ora.
    pub to: String,
}

/// Perché uno spostamento previsto non è avvenuto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// Lo spostamento che non è riuscito.
    pub mov: Move,
    /// L'errore.
    pub error: AppError,
}

/// L'esito di un riordino.
#[derive(Debug, Default)]
pub struct OrganizeOutcome {
    /// Gli spostamenti avvenuti, nell'ordine in cui sono avvenuti.
    pub moved: Vec<JournalEntry>,
    /// Quelli non avvenuti, col motivo.
    pub failed: Vec<Failure>,
    /// Le cartelle rimaste vuote e rimosse.
    pub removed_dirs: usize,
}

/// Serializza una riga di giornale.
///
/// JSON scritto a mano invece che con una libreria: le tre righe che servono
/// non giustificano una dipendenza in un modulo che deve poter girare anche
/// quando tutto il resto è rotto — ed è esattamente allora che serve.
fn encode(entry: &JournalEntry) -> String {
    fn esc(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out
    }
    format!(
        r#"{{"track_id":{},"from":"{}","to":"{}"}}"#,
        entry.track_id,
        esc(&entry.from),
        esc(&entry.to)
    )
}

/// Rilegge una riga di giornale. Una riga malformata si salta.
fn decode(line: &str) -> Option<JournalEntry> {
    fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
        let at = line.find(&format!("\"{name}\":\""))? + name.len() + 4;
        let rest = line.get(at..)?;
        let mut end = 0;
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            if c == '\\' {
                chars.next();
                continue;
            }
            if c == '"' {
                end = i;
                break;
            }
        }
        rest.get(..end)
    }
    fn unesc(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(c);
                    }
                }
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    }

    let id_at = line.find("\"track_id\":")? + 11;
    let id_rest = line.get(id_at..)?;
    let id_end = id_rest.find(|c: char| !c.is_ascii_digit() && c != '-')?;
    let track_id = id_rest.get(..id_end)?.parse().ok()?;

    Some(JournalEntry {
        track_id,
        from: unesc(field(line, "from")?),
        to: unesc(field(line, "to")?),
    })
}

/// Il giornale aperto in scrittura.
struct Journal {
    file: fs::File,
    path: PathBuf,
}

impl Journal {
    fn create(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| io_error(&parent.display().to_string(), &err))?;
        }
        let file =
            fs::File::create(path).map_err(|err| io_error(&path.display().to_string(), &err))?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    /// Scrive una riga e la manda sul disco **prima** di proseguire.
    ///
    /// Senza il `sync_data`, la riga resterebbe nella cache del sistema e una
    /// caduta improvvisa la perderebbe pur avendo già spostato il file. Costa,
    /// ed è il costo di poter tornare indietro.
    fn append(&mut self, entry: &JournalEntry) -> Result<(), AppError> {
        let line = format!("{}\n", encode(entry));
        let fail = |err: &std::io::Error| io_error(&self.path.display().to_string(), err);
        self.file.write_all(line.as_bytes()).map_err(|e| fail(&e))?;
        self.file.flush().map_err(|e| fail(&e))?;
        self.file.sync_data().map_err(|e| fail(&e))?;
        Ok(())
    }
}

/// Legge un giornale, saltando le righe illeggibili.
///
/// Una riga troncata a metà — il caso di un'interruzione improvvisa — non
/// invalida quelle prima di lei.
pub fn read_journal(path: &Path) -> Result<Vec<JournalEntry>, AppError> {
    let text =
        fs::read_to_string(path).map_err(|err| io_error(&path.display().to_string(), &err))?;
    Ok(text.lines().filter_map(decode).collect())
}

/// Sposta un file senza mai sovrascrivere, creando le cartelle mancanti.
fn move_file(from: &str, to: &str) -> Result<(), AppError> {
    let destination = Path::new(to);
    if destination.exists() {
        return Err(AppError::new(ErrorCode::FsInUse {
            path: to.to_owned(),
        })
        .with_message("la destinazione esiste già: non si sovrascrive"));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|err| io_error(&parent.display().to_string(), &err))?;
    }

    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Fra volumi diversi `rename` non funziona: si copia e si cancella
            // l'originale **solo dopo** che la copia è riuscita. L'ordine è
            // tutto: al contrario, un guasto a metà lascerebbe un brano perso.
            let cross_device = err.kind() == std::io::ErrorKind::CrossesDevices;
            if !cross_device {
                return Err(io_error(from, &err));
            }
            fs::copy(from, to).map_err(|e| io_error(from, &e))?;
            fs::remove_file(from).map_err(|e| io_error(from, &e))?;
            Ok(())
        }
    }
}

/// Esegue il piano, scrivendo il giornale mentre procede.
///
/// Un singolo spostamento fallito non ferma gli altri: si annota e si prosegue.
/// Fermarsi al primo guasto lascerebbe la libreria a metà strada senza che
/// l'utente possa nemmeno vedere quanto è stato fatto.
pub fn execute(
    plan: &OrganizePlan,
    journal_path: &Path,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<OrganizeOutcome, AppError> {
    let mut journal = Journal::create(journal_path)?;
    let mut outcome = OrganizeOutcome::default();
    let total = plan.moves.len();

    for (index, mov) in plan.moves.iter().enumerate() {
        match move_file(&mov.from, &mov.to) {
            Ok(()) => {
                let entry = JournalEntry {
                    track_id: mov.track_id,
                    from: mov.from.clone(),
                    to: mov.to.clone(),
                };
                // Il giornale PRIMA di dichiarare fatto: se scrivere fallisce,
                // lo spostamento è comunque avvenuto e va riportato, ma
                // l'errore deve risalire — un giornale che non si scrive
                // significa che da qui in poi non si può più tornare indietro.
                journal.append(&entry)?;
                outcome.moved.push(entry);
            }
            Err(error) => outcome.failed.push(Failure {
                mov: mov.clone(),
                error,
            }),
        }
        on_progress(index + 1, total);
    }

    outcome.removed_dirs = prune_empty_dirs(plan);
    Ok(outcome)
}

/// Rimette tutto com'era, leggendo il giornale al contrario.
///
/// Al contrario e non in avanti: se due spostamenti si sono incrociati — A dove
/// stava B, B altrove — disfarli nell'ordine di andata rimetterebbe A sopra un
/// posto ancora occupato.
pub fn undo(
    entries: &[JournalEntry],
    mut on_progress: impl FnMut(usize, usize),
) -> OrganizeOutcome {
    let mut outcome = OrganizeOutcome::default();
    let total = entries.len();
    for (index, entry) in entries.iter().rev().enumerate() {
        match move_file(&entry.to, &entry.from) {
            Ok(()) => outcome.moved.push(JournalEntry {
                track_id: entry.track_id,
                from: entry.to.clone(),
                to: entry.from.clone(),
            }),
            Err(error) => outcome.failed.push(Failure {
                mov: Move {
                    track_id: entry.track_id,
                    from: entry.to.clone(),
                    to: entry.from.clone(),
                },
                error,
            }),
        }
        on_progress(index + 1, total);
    }
    // Le cartelle create dal riordino restano vuote: si tolgono, dalla più
    // profonda alla più esterna.
    let mut dirs: Vec<&str> = entries
        .iter()
        .filter_map(|e| e.to.rsplit_once(['/', '\\']).map(|(dir, _)| dir))
        .collect();
    dirs.sort_unstable_by_key(|d| std::cmp::Reverse(d.len()));
    dirs.dedup();
    for dir in dirs {
        outcome.removed_dirs += usize::from(fs::remove_dir(dir).is_ok());
    }
    outcome
}

/// Toglie le cartelle di partenza rimaste vuote, dalla più profonda.
///
/// Solo quelle che il piano ha svuotato: una cartella già vuota prima del
/// riordino non è affar nostro, e cancellarla sarebbe un effetto collaterale
/// che nessuno ha chiesto.
fn prune_empty_dirs(plan: &OrganizePlan) -> usize {
    let mut dirs: Vec<&str> = plan
        .moves
        .iter()
        .filter_map(|m| m.from.rsplit_once(['/', '\\']).map(|(dir, _)| dir))
        .collect();
    dirs.sort_unstable_by_key(|d| std::cmp::Reverse(d.len()));
    dirs.dedup();
    dirs.iter()
        .filter(|dir| {
            fs::read_dir(dir).is_ok_and(|mut it| it.next().is_none()) && fs::remove_dir(dir).is_ok()
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_domain::organize::{TrackToOrganize, plan_organize};
    use aether_domain::paths::PathRules;

    fn scrivi(path: &Path, contenuto: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("cartelle");
        }
        fs::write(path, contenuto).expect("scrittura");
    }

    /// Costruisce una libreria finta e ne pianifica il riordino.
    fn scenario(brani: &[(&str, &str, &str)]) -> (tempfile::TempDir, String, OrganizePlan) {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let root = dir.path().to_string_lossy().replace('\\', "/");
        let mut tracks = Vec::new();
        for (index, (nome, album, artista)) in brani.iter().enumerate() {
            let path = format!("{root}/{nome}");
            scrivi(Path::new(&path), nome);
            tracks.push(TrackToOrganize {
                id: i64::try_from(index).unwrap_or(0),
                path,
                album: (*album).to_owned(),
                album_artist: None,
                artist: Some((*artista).to_owned()),
            });
        }
        let plan = plan_organize(&tracks, &root, PathRules::for_current_platform());
        (dir, root, plan)
    }

    #[test]
    fn sposta_e_annulla_riportando_tutto_com_era() {
        let (dir, root, plan) = scenario(&[
            ("a.mp3", "Album", "Art"),
            ("b.mp3", "Album", "Art"),
            ("c.mp3", "Altro", "Art"),
        ]);
        let journal_path = dir.path().join("riordino.jsonl");

        let esito = execute(&plan, &journal_path, |_, _| {}).expect("esecuzione");
        assert_eq!(esito.moved.len(), 3);
        assert!(esito.failed.is_empty());
        assert!(Path::new(&format!("{root}/Art/Album/a.mp3")).exists());
        assert!(!Path::new(&format!("{root}/a.mp3")).exists());

        let entries = read_journal(&journal_path).expect("rilettura");
        assert_eq!(entries.len(), 3);

        let indietro = undo(&entries, |_, _| {});
        assert_eq!(indietro.moved.len(), 3);
        assert!(indietro.failed.is_empty());
        // Tutto dov'era, e le cartelle create sono sparite.
        assert!(Path::new(&format!("{root}/a.mp3")).exists());
        assert!(!Path::new(&format!("{root}/Art/Album")).exists());
    }

    #[test]
    fn il_giornale_sopravvive_a_una_riga_troncata() {
        // È il caso di un'interruzione improvvisa: l'ultima riga è a metà. Le
        // precedenti devono restare utilizzabili, altrimenti il giornale non
        // serve proprio quando serve.
        let dir = tempfile::tempdir().expect("cartella");
        let path = dir.path().join("g.jsonl");
        let buone = format!(
            "{}\n{}\n",
            encode(&JournalEntry {
                track_id: 1,
                from: "C:/a".into(),
                to: "C:/x/a".into()
            }),
            encode(&JournalEntry {
                track_id: 2,
                from: "C:/b".into(),
                to: "C:/x/b".into()
            })
        );
        fs::write(
            &path,
            format!("{buone}{{\"track_id\":3,\"from\":\"C:/c\",\"t"),
        )
        .expect("scrittura");

        let entries = read_journal(&path).expect("rilettura");
        assert_eq!(entries.len(), 2, "le due righe intere restano");
        assert_eq!(entries.first().map(|e| e.track_id), Some(1));
    }

    #[test]
    fn i_percorsi_con_caratteri_difficili_sopravvivono_al_giornale() {
        // Un backslash di Windows, le virgolette, un accento: se la codifica li
        // rompe, l'annullamento riporta il file nel posto sbagliato.
        let entry = JournalEntry {
            track_id: 7,
            from: r#"C:\Music\Caffè "live"\a.mp3"#.to_owned(),
            to: "C:/Music/Caffè/a.mp3".to_owned(),
        };
        let riletto = decode(&encode(&entry)).expect("decodifica");
        assert_eq!(riletto, entry);
    }

    #[test]
    fn non_sovrascrive_mai_la_destinazione() {
        // `rename` su Windows sovrascriverebbe senza dire niente: sarebbe un
        // brano perso, e il giornale direbbe che è andato tutto bene.
        let (dir, root, plan) = scenario(&[("a.mp3", "Album", "Art")]);
        let occupato = format!("{root}/Art/Album/a.mp3");
        scrivi(Path::new(&occupato), "sono un altro file");

        let esito = execute(&plan, &dir.path().join("g.jsonl"), |_, _| {}).expect("esecuzione");
        assert!(esito.moved.is_empty());
        assert_eq!(esito.failed.len(), 1);
        assert_eq!(
            fs::read_to_string(&occupato).expect("lettura"),
            "sono un altro file",
            "il file che c'era non è stato toccato"
        );
        assert!(
            Path::new(&format!("{root}/a.mp3")).exists(),
            "l'originale è ancora lì"
        );
    }

    #[test]
    fn un_guasto_singolo_non_ferma_gli_altri() {
        let (dir, root, plan) = scenario(&[
            ("a.mp3", "Album", "Art"),
            ("b.mp3", "Album", "Art"),
            ("c.mp3", "Album", "Art"),
        ]);
        // Si occupa la destinazione di uno solo dei tre.
        let bloccato = plan.moves.first().expect("almeno uno").to.clone();
        scrivi(Path::new(&bloccato), "occupato");

        let esito = execute(&plan, &dir.path().join("g.jsonl"), |_, _| {}).expect("esecuzione");
        assert_eq!(esito.moved.len(), 2, "gli altri due passano");
        assert_eq!(esito.failed.len(), 1);
        assert!(Path::new(&format!("{root}/Art/Album")).exists());
    }

    #[test]
    fn l_annullamento_va_al_contrario() {
        // Se due spostamenti si incrociano, disfarli in avanti rimetterebbe il
        // primo sopra un posto ancora occupato.
        let dir = tempfile::tempdir().expect("cartella");
        let root = dir.path().to_string_lossy().replace('\\', "/");
        scrivi(Path::new(&format!("{root}/a.mp3")), "A");

        let entries = vec![
            JournalEntry {
                track_id: 1,
                from: format!("{root}/a.mp3"),
                to: format!("{root}/tmp.mp3"),
            },
            JournalEntry {
                track_id: 1,
                from: format!("{root}/tmp.mp3"),
                to: format!("{root}/finale.mp3"),
            },
        ];
        fs::rename(format!("{root}/a.mp3"), format!("{root}/finale.mp3")).expect("simula");

        let esito = undo(&entries, |_, _| {});
        assert!(esito.failed.is_empty(), "{:?}", esito.failed);
        assert!(Path::new(&format!("{root}/a.mp3")).exists());
    }

    #[test]
    fn il_progresso_conta_fino_in_fondo() {
        let (dir, _root, plan) = scenario(&[("a.mp3", "Al", "Ar"), ("b.mp3", "Al", "Ar")]);
        let mut ultimo = (0, 0);
        execute(&plan, &dir.path().join("g.jsonl"), |fatti, totale| {
            ultimo = (fatti, totale);
        })
        .expect("esecuzione");
        assert_eq!(ultimo, (2, 2));
    }
}
