//! La decisione della scansione, separata dal suo lavoro.
//!
//! Una scansione fa due cose molto diverse: legge il disco (lento, asincrono,
//! con code e pressione termica) e **decide** cosa inserire, cosa rileggere,
//! cosa togliere. Nel vecchio albero erano un'unica funzione, e la conseguenza
//! non è estetica: la parte che decide è quella che può cancellare righe, ed era
//! provabile solo costruendo un albero di file veri e un database vero. Infatti
//! il suo test erano 56 righe contro le 368 del modulo — e i casi che costano
//! dati non erano fra quelle 56.
//!
//! Qui la decisione è una funzione pura da tre elenchi a un piano. Ogni caso che
//! costa dati — la cartella con il nome che comincia come quella sorvegliata, il
//! percorso che differisce per una barra, il file troncato che i lettori di
//! metadati accettano lo stesso — si prova come una chiamata di funzione.
//!
//! E il piano si può **mostrare prima di eseguirlo**. «Sto per togliere 340
//! brani dalla libreria» è una frase che l'utente deve poter leggere quando ha
//! staccato il disco esterno per sbaglio, invece di scoprirlo dopo.

use std::collections::{HashMap, HashSet};

use crate::paths::{
    MIN_TRACK_BYTES, PathRules, is_in_trash, is_supported_audio_path, is_under, path_key,
};

/// Un file trovato dalla camminata sul disco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFile {
    /// Il percorso così com'è sul disco. Non normalizzato: serve ad aprirlo.
    pub path: String,
    /// La dimensione in byte.
    pub size_bytes: u64,
    /// Data di modifica in millisecondi, **già troncata all'intero**.
    ///
    /// Il troncamento sta a monte perché è la forma che il database persiste: se
    /// il confronto avvenisse fra un valore troncato e uno con i decimali, ogni
    /// scansione vedrebbe cambiato ogni file, e una riscansione da centomila
    /// brani rileggerebbe tutti i metadati invece di nessuno.
    pub modified_ms: i64,
}

/// Una riga già in libreria, come serve a decidere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownTrack {
    /// L'identificativo della riga.
    pub id: i64,
    /// Il percorso salvato.
    pub path: String,
    /// La data di modifica registrata all'ultima lettura.
    pub modified_ms: i64,
}

/// Perché un file trovato non entra in libreria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Estensione fuori dall'elenco.
    NonAudio,
    /// Dentro la cartella dei doppioni scartati.
    InTrash,
    /// Sotto la soglia: avanzo troncato, non un brano.
    TooSmall,
    /// Un altro file trovato in questa stessa passata occupa già il suo posto.
    Duplicate,
}

/// Perché una riga va tolta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveReason {
    /// Il file non c'è più, e la sua riga stava in una cartella scansionata.
    Disappeared,
    /// Due righe per lo stesso file: su un filesystem che ignora le maiuscole
    /// `A.mp3` e `a.mp3` passano il vincolo di unicità ma nominano un file solo.
    DuplicateRow,
}

impl SkipReason {
    /// Il nome che attraversa FFI, database e interfaccia. Invariato dal vecchio
    /// albero: sta già dentro dati salvati e diagnostiche.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NonAudio => "nonAudio",
            Self::InTrash => "inTrash",
            Self::TooSmall => "tooSmall",
            Self::Duplicate => "duplicate",
        }
    }
}

impl RemoveReason {
    /// Vedi [`SkipReason::as_str`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disappeared => "disappeared",
            Self::DuplicateRow => "duplicateRow",
        }
    }
}

/// Un file già in libreria la cui data di modifica è cambiata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingUpdate {
    /// Il file sul disco.
    pub file: DiscoveredFile,
    /// La riga da aggiornare.
    pub track_id: i64,
}

/// Una riga da togliere, col motivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRemoval {
    /// La riga.
    pub track: KnownTrack,
    /// Perché.
    pub reason: RemoveReason,
}

/// Un file visto e scartato, col motivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    /// Il file.
    pub file: DiscoveredFile,
    /// Perché.
    pub reason: SkipReason,
}

/// Cosa la scansione ha deciso di fare.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanPlan {
    /// Da leggere e inserire: non c'era.
    pub to_insert: Vec<DiscoveredFile>,
    /// Da rileggere e aggiornare: la data di modifica è cambiata.
    pub to_update: Vec<PendingUpdate>,
    /// Righe da togliere. È la parte del piano che va mostrata prima di eseguirla.
    pub to_remove: Vec<PendingRemoval>,
    /// File visti e scartati, con il motivo.
    pub skipped: Vec<SkippedFile>,
    /// Quante righe erano già a posto: il numero che rende veloce una riscansione.
    pub unchanged: usize,
    /// Righe fuori dalle cartelle scansionate, lasciate stare.
    ///
    /// Non è un dettaglio contabile: una scansione della sola cartella dei
    /// download non deve toccare i brani che stanno altrove. Contarle rende
    /// visibile che sono state deliberatamente ignorate, invece che dimenticate.
    pub untouched: usize,
}

impl ScanPlan {
    /// Il piano non cambia niente? Utile per non aprire una transazione per nulla.
    #[must_use]
    pub fn is_no_op(&self) -> bool {
        self.to_insert.is_empty() && self.to_update.is_empty() && self.to_remove.is_empty()
    }
}

/// Gli ingressi della decisione.
#[derive(Debug, Clone, Copy)]
pub struct ScanInput<'a> {
    /// Le cartelle che questa passata sta scansionando.
    pub roots: &'a [String],
    /// Quel che la camminata ha trovato.
    pub found: &'a [DiscoveredFile],
    /// Le righe locali già in libreria.
    pub known: &'a [KnownTrack],
}

/// Confronta disco e database e produce il piano.
///
/// Puro: nessuna lettura, nessuna scrittura, nessun orologio. Chi esegue prende
/// il piano e lo applica; chi mostra lo mostra.
///
/// L'ordine delle voci è deterministico — le righe conosciute si scorrono
/// nell'ordine in cui sono arrivate, non in quello di una tabella hash — così
/// due esecuzioni sugli stessi ingressi producono lo stesso piano, e mostrarlo
/// all'utente due volte non dà due elenchi diversi.
#[must_use]
pub fn plan_scan(input: ScanInput<'_>, rules: PathRules) -> ScanPlan {
    let mut plan = ScanPlan::default();

    // Le righe conosciute, indicizzate per forma canonica ma tenute in ordine di
    // arrivo. `order` dà la sequenza, `index` la ricerca.
    let mut order: Vec<(String, KnownTrack)> = Vec::with_capacity(input.known.len());
    let mut index: HashMap<String, usize> = HashMap::with_capacity(input.known.len());

    for track in input.known {
        let key = path_key(&track.path, rules);
        match index.get(&key) {
            None => {
                index.insert(key.clone(), order.len());
                order.push((key, track.clone()));
            }
            Some(&pos) => {
                let Some(slot) = order.get_mut(pos) else {
                    continue;
                };
                // Si tiene la riga con l'id più basso: è la più vecchia, quindi
                // quella a cui playlist, preferiti e conteggi di ascolto puntano
                // più probabilmente.
                let (keep, drop) = if slot.1.id <= track.id {
                    (slot.1.clone(), track.clone())
                } else {
                    (track.clone(), slot.1.clone())
                };
                slot.1 = keep;
                plan.to_remove.push(PendingRemoval {
                    track: drop,
                    reason: RemoveReason::DuplicateRow,
                });
            }
        }
    }

    // Le chiavi viste sul disco in questa passata, comprese quelle scartate.
    let mut seen: HashSet<String> = HashSet::with_capacity(input.found.len());

    for file in input.found {
        let key = path_key(&file.path, rules);

        if !seen.insert(key.clone()) {
            plan.skipped.push(SkippedFile {
                file: file.clone(),
                reason: SkipReason::Duplicate,
            });
            continue;
        }

        // L'ordine dei controlli è quello del costo crescente, ma soprattutto:
        // `InTrash` e `NonAudio` vengono prima di `TooSmall` perché un file nel
        // cestino o non audio va riportato per quel che è, non come «troppo
        // piccolo» — il motivo finisce nella diagnostica, e uno sbagliato manda
        // a cercare nel posto sbagliato.
        let skip = if is_in_trash(&file.path) {
            Some(SkipReason::InTrash)
        } else if !is_supported_audio_path(&file.path) {
            Some(SkipReason::NonAudio)
        } else if file.size_bytes < MIN_TRACK_BYTES {
            Some(SkipReason::TooSmall)
        } else {
            None
        };
        if let Some(reason) = skip {
            plan.skipped.push(SkippedFile {
                file: file.clone(),
                reason,
            });
            continue;
        }

        match index.get(&key).and_then(|&pos| order.get(pos)) {
            None => plan.to_insert.push(file.clone()),
            Some((_, existing)) if existing.modified_ms == file.modified_ms => plan.unchanged += 1,
            Some((_, existing)) => plan.to_update.push(PendingUpdate {
                file: file.clone(),
                track_id: existing.id,
            }),
        }
    }

    for (key, track) in &order {
        if seen.contains(key) {
            continue;
        }
        // Sparita, ma solo se stava in una cartella che questa passata ha
        // davvero guardato. Una riga fuori dalle radici non è «non trovata»: è
        // fuori competenza, e toglierla svuoterebbe la libreria alla prima
        // scansione della sola cartella dei download.
        let watched = input
            .roots
            .iter()
            .any(|root| is_under(&track.path, root, rules));
        if watched {
            plan.to_remove.push(PendingRemoval {
                track: track.clone(),
                reason: RemoveReason::Disappeared,
            });
        } else {
            plan.untouched += 1;
        }
    }

    plan
}

/// L'identità di una riga che il piano toglierebbe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemovedIdentity<'a> {
    /// La riga.
    pub track_id: i64,
    /// La sua chiave di **contenuto**, come sta nel database.
    ///
    /// Non `track_key`, ed è la differenza che fa funzionare l'appaiamento: la
    /// chiave di brano si ricalcola dai valori che l'utente ha corretto, quindi
    /// dopo una correzione non combacia più con quella del file appena letto,
    /// che i tag li ha ancora sbagliati. Vedi
    /// [`crate::keys::ContentKey`].
    pub content_key: &'a str,
}

/// Una riga che sopravvive perché il suo file si è solo spostato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rematch {
    /// La riga da aggiornare invece di cancellare.
    pub track_id: i64,
    /// La posizione, dentro l'elenco degli inserimenti, del file che la riempie.
    pub insert_index: usize,
}

/// La chiave identifica davvero un brano?
///
/// Le forme di [`crate::keys::ContentKey`] sono due, e la domanda si risolve
/// per entrambe guardando il **secondo segmento**:
///
/// - `artista|titolo|album` — il secondo segmento è il titolo. Senza titolo non
///   resta identità sufficiente: appaiare su una chiave del genere
///   significherebbe dichiarare «stesso brano» due file che hanno in comune
///   soltanto di non avere un nome.
/// - `durata|dimensione` — il secondo segmento è la dimensione del file. Un
///   file senza tag che si sposta resta lungo e pesante uguale, e quelle due
///   cose insieme sono l'unica identità che gli è rimasta. Con `|` e basta non
///   se ne conosce nessuna delle due, e allora non si appaia.
fn identifies_a_track(content_key: &str) -> bool {
    content_key
        .split('|')
        .nth(1)
        .is_some_and(|secondo| !secondo.is_empty())
}

/// Riconosce, fra le righe che sparirebbero, quelle il cui file è semplicemente
/// altrove: si aggiorna il percorso invece di cancellare e reinserire.
///
/// # Perché non è un'ottimizzazione
///
/// L'identificativo di una riga è il perno a cui puntano playlist, cronologia
/// d'ascolto, conteggi, valutazioni e preferiti. Cancellare e reinserire ne
/// crea uno nuovo: il brano ricompare in libreria **senza niente di tutto
/// quello**, e sparisce dalle playlist in cui stava.
///
/// Non è un caso raro. È esattamente ciò che succede a **ogni file** quando una
/// libreria viene riordinata in cartelle — da Picard, da un altro lettore, o
/// dal riordino che Aether stessa offriva fino alla 2.3.0: senza questo
/// passaggio, un riordino solo azzera l'intera storia d'ascolto.
///
/// L'appaiamento è sulla sola chiave — non sulla dimensione a parte — perché un
/// file può essersi spostato *e* essere stato ritaggato nello stesso giro. A
/// pari chiave si appaia in ordine: se due copie dello stesso brano si scambiano
/// le statistiche fra loro il danno è nullo, mentre un appaiamento rifiutato
/// costa la storia di entrambe.
///
/// # Perché la chiave è quella di contenuto
///
/// Perché è l'unica che le due parti calcolano allo stesso modo. Le righe che
/// sparirebbero portano la chiave che sta nel database; i file appena letti
/// quella calcolata adesso dai loro tag. Con `track_key` le due divergono
/// appena l'utente corregge qualcosa — la riga ha la chiave dei valori
/// corretti, il file quella dei tag grezzi — e l'appaiamento fallisce proprio
/// sui brani a cui qualcuno ha dedicato attenzione: il brano si duplica, e la
/// riga vecchia se ne va con ascolti, voto e playlist. [`crate::keys::ContentKey`]
/// non cambia mai, e le due parti tornano a parlare la stessa lingua.
#[must_use]
pub fn match_moved_tracks(removed: &[RemovedIdentity<'_>], inserted_keys: &[&str]) -> Vec<Rematch> {
    let mut candidates: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, key) in inserted_keys.iter().enumerate() {
        if identifies_a_track(key) {
            candidates.entry(key).or_default().push(index);
        }
    }

    let mut taken: HashMap<&str, usize> = HashMap::new();
    let mut matched = Vec::new();
    for row in removed {
        if !identifies_a_track(row.content_key) {
            continue;
        }
        let Some(free) = candidates.get(row.content_key) else {
            continue;
        };
        let next = taken.entry(row.content_key).or_insert(0);
        let Some(&index) = free.get(*next) else {
            continue;
        };
        *next += 1;
        matched.push(Rematch {
            track_id: row.track_id,
            insert_index: index,
        });
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: PathRules = PathRules {
        case_insensitive: true,
    };

    fn file(path: &str, size_bytes: u64, modified_ms: i64) -> DiscoveredFile {
        DiscoveredFile {
            path: path.to_owned(),
            size_bytes,
            modified_ms,
        }
    }

    fn track(id: i64, path: &str, modified_ms: i64) -> KnownTrack {
        KnownTrack {
            id,
            path: path.to_owned(),
            modified_ms,
        }
    }

    fn plan(roots: &[&str], found: Vec<DiscoveredFile>, known: Vec<KnownTrack>) -> ScanPlan {
        let roots: Vec<String> = roots.iter().map(|r| (*r).to_owned()).collect();
        plan_scan(
            ScanInput {
                roots: &roots,
                found: &found,
                known: &known,
            },
            WIN,
        )
    }

    #[test]
    fn la_cartella_accanto_non_viene_svuotata() {
        // Il difetto storico, come test: scansionando C:\Music, le righe di
        // C:\Musica non sono «sparite», sono fuori competenza.
        let p = plan(
            &[r"C:\Music"],
            vec![file(r"C:\Music\a.mp3", 5_000_000, 1000)],
            vec![
                track(1, r"C:\Music\a.mp3", 1000),
                track(2, r"C:\Musica\b.mp3", 1000),
            ],
        );
        assert!(p.to_remove.is_empty(), "nessuna riga va tolta");
        assert_eq!(p.untouched, 1);
        assert_eq!(p.unchanged, 1);
    }

    #[test]
    fn una_barra_diversa_non_fa_sparire_il_file() {
        let p = plan(
            &[r"C:\Music"],
            vec![file("C:/Music/a.mp3", 5_000_000, 1000)],
            vec![track(1, r"C:\Music\a.mp3", 1000)],
        );
        assert!(p.to_remove.is_empty());
        assert_eq!(p.unchanged, 1);
    }

    #[test]
    fn il_disco_staccato_produce_un_piano_da_mostrare() {
        // Non è un caso da evitare: è il caso in cui il piano serve. L'utente
        // deve poter leggere «sto per togliere 2 brani» e dire di no.
        let p = plan(
            &[r"D:\Music"],
            vec![],
            vec![
                track(1, r"D:\Music\a.mp3", 1),
                track(2, r"D:\Music\b.mp3", 1),
            ],
        );
        assert_eq!(p.to_remove.len(), 2);
        assert!(
            p.to_remove
                .iter()
                .all(|r| r.reason == RemoveReason::Disappeared)
        );
        assert!(!p.is_no_op());
    }

    #[test]
    fn fra_due_righe_per_lo_stesso_file_sopravvive_la_piu_vecchia() {
        let p = plan(
            &[r"C:\Music"],
            vec![file(r"C:\Music\a.mp3", 5_000_000, 1000)],
            vec![
                track(7, r"C:\Music\A.MP3", 1000),
                track(3, r"C:\Music\a.mp3", 1000),
            ],
        );
        assert_eq!(p.to_remove.len(), 1);
        assert_eq!(p.to_remove.first().map(|r| r.track.id), Some(7));
        assert_eq!(
            p.to_remove.first().map(|r| r.reason),
            Some(RemoveReason::DuplicateRow)
        );
    }

    #[test]
    fn la_soglia_e_inclusiva() {
        let sotto = plan(
            &[r"C:\M"],
            vec![file(r"C:\M\a.mp3", MIN_TRACK_BYTES - 1, 1)],
            vec![],
        );
        let esatta = plan(
            &[r"C:\M"],
            vec![file(r"C:\M\a.mp3", MIN_TRACK_BYTES, 1)],
            vec![],
        );
        assert_eq!(sotto.skipped.len(), 1);
        assert_eq!(esatta.to_insert.len(), 1);
    }

    #[test]
    fn il_motivo_dello_scarto_manda_a_cercare_nel_posto_giusto() {
        // Un file nel cestino è scartato PER QUELLO, anche se è pure troncato.
        let p = plan(&[r"C:\M"], vec![file(r"C:\M\.trash\a.mp3", 10, 1)], vec![]);
        assert_eq!(
            p.skipped.first().map(|s| s.reason),
            Some(SkipReason::InTrash)
        );
    }

    #[test]
    fn nessuna_radice_significa_non_toccare_niente() {
        let p = plan(&[], vec![], vec![track(1, r"C:\Music\a.mp3", 1)]);
        assert!(p.to_remove.is_empty());
        assert_eq!(p.untouched, 1);
        assert!(p.is_no_op());
    }

    #[test]
    fn un_riordino_non_azzera_la_storia_d_ascolto() {
        // Il caso vero: il riordino ha spostato tutti i file, quindi il piano
        // vede due sparizioni e due file nuovi. Sono le stesse due righe.
        let rematch = match_moved_tracks(
            &[
                RemovedIdentity {
                    track_id: 1,
                    content_key: "art|uno|alb",
                },
                RemovedIdentity {
                    track_id: 2,
                    content_key: "art|due|alb",
                },
            ],
            &["art|due|alb", "art|uno|alb"],
        );
        assert_eq!(
            rematch,
            vec![
                Rematch {
                    track_id: 1,
                    insert_index: 1
                },
                Rematch {
                    track_id: 2,
                    insert_index: 0
                },
            ]
        );
    }

    #[test]
    fn i_file_senza_titolo_non_si_appaiano_fra_loro() {
        // `art||` non identifica un brano: appaiarli significherebbe dire
        // «stesso brano» a due file che hanno in comune solo di non avere nome.
        let rematch = match_moved_tracks(
            &[RemovedIdentity {
                track_id: 1,
                content_key: "art||",
            }],
            &["art||"],
        );
        assert!(rematch.is_empty());
    }

    #[test]
    fn un_file_senza_tag_si_appaia_su_durata_e_dimensione() {
        // La forma di ripiego di `ContentKey`: due segmenti invece di tre. Un
        // file che i tag non ce li ha resta lungo e pesante uguale quando lo si
        // sposta, ed è l'unica identità che gli è rimasta.
        let rematch = match_moved_tracks(
            &[RemovedIdentity {
                track_id: 1,
                content_key: "283000|9000000",
            }],
            &["283000|9000000"],
        );
        assert_eq!(
            rematch,
            vec![Rematch {
                track_id: 1,
                insert_index: 0
            }]
        );

        // Ma se non si conosce nemmeno quello, non si appaia niente.
        let ignoto = match_moved_tracks(
            &[RemovedIdentity {
                track_id: 1,
                content_key: "|",
            }],
            &["|"],
        );
        assert!(ignoto.is_empty());
    }

    #[test]
    fn una_riga_sola_non_si_appaia_a_due_file() {
        // Due copie sparite, tre arrivate: se ne appaiano due, la terza è nuova.
        let rematch = match_moved_tracks(
            &[
                RemovedIdentity {
                    track_id: 1,
                    content_key: "a|t|b",
                },
                RemovedIdentity {
                    track_id: 2,
                    content_key: "a|t|b",
                },
            ],
            &["a|t|b", "a|t|b", "a|t|b"],
        );
        assert_eq!(rematch.len(), 2);
        assert_eq!(
            rematch.iter().map(|r| r.insert_index).collect::<Vec<_>>(),
            vec![0, 1],
            "ogni inserimento va appaiato una volta sola"
        );
    }

    #[test]
    fn quel_che_non_torna_resta_un_inserimento() {
        let rematch = match_moved_tracks(
            &[RemovedIdentity {
                track_id: 1,
                content_key: "a|sparito|b",
            }],
            &["a|altro|b"],
        );
        assert!(rematch.is_empty());
    }

    #[test]
    fn il_piano_e_deterministico() {
        let found: Vec<_> = (0..50)
            .map(|i| file(&format!(r"C:\M\{i}.mp3"), 5_000_000, 1))
            .collect();
        let known: Vec<_> = (0..50)
            .map(|i| track(i, &format!(r"C:\M\vecchio{i}.mp3"), 1))
            .collect();
        let roots = vec![r"C:\M".to_owned()];
        let a = plan_scan(
            ScanInput {
                roots: &roots,
                found: &found,
                known: &known,
            },
            WIN,
        );
        let b = plan_scan(
            ScanInput {
                roots: &roots,
                found: &found,
                known: &known,
            },
            WIN,
        );
        // Con una tabella hash a dettare l'ordine questo cadrebbe a intermittenza,
        // e l'elenco mostrato all'utente cambierebbe fra un'apertura e l'altra.
        assert_eq!(a, b);
    }
}
