//! Decidere cosa farebbe un ripristino, prima di farlo.
//!
//! Il backup su Drive porta indietro quel che una scansione non sa ricostruire:
//! conteggi d'ascolto, voti, preferiti, playlist, cartelle sorvegliate, skin.
//! Questo modulo decide **cosa cambierebbe**; scriverlo tocca a chi ha il
//! database e il disco.
//!
//! # Perché il piano è un valore, e non un effetto
//!
//! Perché lo si mostra prima. È la stessa forma del piano di scansione
//! ([`crate::scan_plan`]): si vede l'elenco di ciò che accadrebbe, e solo dopo
//! si conferma. Un
//! ripristino che partisse subito sarebbe indistinguibile, per chi lo guarda,
//! da una perdita di dati — e non c'è modo di sapere in anticipo se la libreria
//! che sta sul computer è più aggiornata di quella che sta nella nuvola.
//!
//! E perché [`plan_restore`] è **una sola sorgente di verità per l'anteprima e
//! per l'esecuzione**: chi applica rilegge lo stato e la richiama, invece di
//! fidarsi del piano che ha mostrato. Fra il momento in cui si guarda e quello
//! in cui si conferma può essere finita una scansione.
//!
//! # La regola che tiene insieme tutto: non si distrugge
//!
//! Ogni decisione qui sotto, a parità d'informazione, sceglie la direzione che
//! **non toglie**. I conteggi salgono e non scendono ([`merge_stats`] se ne
//! occupa), le cartelle sorvegliate si uniscono e non si sostituiscono, una
//! skin già installata non viene sovrascritta, una bozza dello Studio non viene
//! toccata. Un ripristino è qualcosa che si fa quando si è già perso qualcosa:
//! non deve poter essere la seconda perdita.
//!
//! # Idempotenza
//!
//! Applicare un piano e ricalcolarlo deve dare un piano **vuoto**. Non è
//! eleganza: chi ripristina lo rifà — perché non è chiaro se sia andato a buon
//! fine, perché la connessione è caduta a metà — e un piano che dopo la prima
//! volta continua a proporre le stesse modifiche è un piano di cui non ci si
//! può fidare. Un test lo verifica.

use std::collections::{BTreeMap, BTreeSet};

use crate::keys::{PlaylistKey, TrackKey};
use crate::merge::{TrackStats, merge_stats};
use crate::omonime::{Occupato, Posto, stessa_nascita, trova_posto};

/// Come si chiama il backup nel nome di una playlist messa accanto a una di
/// qui: «Serata (Drive)». Un nome proprio, che si legge uguale in ogni lingua.
pub const ETICHETTA_ACCANTO: &str = "Drive";
use crate::paths::{PathRules, path_key};

/// Una playlist, come la conoscono sia il database sia il backup.
///
/// I membri sono [`TrackKey`] e non identificatori di riga: un id è vero solo
/// sul dispositivo che l'ha generato, e una playlist deve poter attraversare
/// una reinstallazione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistState {
    /// L'identità: il nome normalizzato.
    pub key: PlaylistKey,
    /// Il nome come si scrive.
    pub name: String,
    /// La descrizione, se c'è.
    pub description: Option<String>,
    /// Quando è stata creata, in millisecondi.
    pub created_at: i64,
    /// Quando è stata toccata l'ultima volta, in millisecondi.
    ///
    /// È l'orologio che decide chi vince fra due versioni della stessa
    /// playlist. Vedi [`plan_restore`].
    pub updated_at: i64,
    /// È automatica: l'appartenenza la decidono le regole, non l'elenco.
    pub is_smart: bool,
    /// Le regole, per una playlist automatica.
    pub rules: Option<String>,
    /// I membri, nell'ordine. Vuoto per una playlist automatica.
    pub members: Vec<TrackKey>,
}

/// Le statistiche di un brano che il ripristino cambierebbe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackChange {
    /// Quale brano.
    pub key: TrackKey,
    /// Com'è adesso, qui.
    pub before: TrackStats,
    /// Come sarebbe dopo la fusione.
    pub after: TrackStats,
}

/// Una playlist che il ripristino creerebbe o riscriverebbe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistChange {
    /// La playlist come sarebbe dopo.
    ///
    /// `members` è **già risolto**: contiene solo le chiavi che qui esistono,
    /// nell'ordine del backup. Chi applica lo scrive così com'è, dando le
    /// posizioni `0..n`.
    pub playlist: PlaylistState,
    /// Non esiste ancora su questo dispositivo.
    pub is_new: bool,
    /// Quanti membri aveva nel backup, compresi quelli che qui mancano.
    ///
    /// Serve a dire «22 brani su 30 presenti qui» invece di far sparire otto
    /// righe senza spiegazione.
    pub members_in_backup: usize,
    /// Qui c'è già una playlist con il nome di quella del backup, ma è
    /// un'altra (D-11): resta com'è, e questa le entra accanto col nome in
    /// `playlist.name`. È il nome di quella lasciata stare.
    pub beside_of: Option<String>,
}

/// Una cartella sorvegliata che il ripristino aggiungerebbe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootToAdd {
    /// Il percorso, come stava nel backup.
    pub path: String,
    /// La cartella esiste ancora su questo disco.
    ///
    /// Deciso da chi ha il filesystem e passato qui dentro: il dominio non
    /// guarda il disco. Una cartella che non c'è si aggiunge **lo stesso** —
    /// la scansione la salta senza lamentarsi, ed è il modo di ricordarsi dove
    /// stava la musica prima della reinstallazione.
    pub exists: bool,
}

/// Quel che le due parti sanno, nel momento in cui si decide.
///
/// Tutto per riferimento: costruire questo valore non deve costare una copia
/// della libreria, perché lo si costruisce due volte per ogni ripristino.
#[derive(Debug, Clone, Copy)]
pub struct RestoreInput<'a> {
    /// I brani di qui. Più righe possono condividere una chiave: vedi
    /// [`plan_restore`].
    pub local_tracks: &'a [(TrackKey, TrackStats)],
    /// I brani del backup, uno per chiave.
    pub backup_tracks: &'a [(TrackKey, TrackStats)],
    /// Le playlist di qui.
    pub local_playlists: &'a [PlaylistState],
    /// Le playlist del backup.
    pub backup_playlists: &'a [PlaylistState],
    /// Le cartelle sorvegliate di qui.
    pub local_roots: &'a [String],
    /// Le cartelle sorvegliate del backup.
    pub backup_roots: &'a [String],
    /// Per ogni cartella del backup, se esiste ancora su questo disco.
    ///
    /// Nello stesso ordine di `backup_roots`. Una voce mancante vale «non si
    /// sa», e si tratta come «non esiste»: è la direzione che non promette.
    pub backup_roots_exist: &'a [bool],
    /// Le skin **disponibili** qui: quelle installate più quelle di serie.
    pub local_skins: &'a [String],
    /// Le skin nel backup.
    pub backup_skins: &'a [String],
    /// Le bozze dello Studio già presenti qui.
    pub local_drafts: &'a [String],
    /// Le bozze dello Studio nel backup.
    pub backup_drafts: &'a [String],
    /// Come si confrontano i percorsi su questo sistema.
    pub path_rules: PathRules,
}

/// Dove va una playlist del backup, e accanto a quale playlist di qui.
///
/// La playlist stessa, se qui non ce n'è una con quel nome o se quella di qui
/// è la stessa ([`stessa_nascita`]). Altrimenti la stessa playlist con il
/// nome e la chiave del posto che [`trova_posto`] sceglie: il primo nome
/// libero — né di una playlist di qui né di un'altra del backup — o la copia
/// accanto di un ripristino di prima, di cui si prendono nome e chiave così
/// come sono, perché il confronto dell'idempotenza la ritrovi uguale.
///
/// `None` oltre cento nomi «(Drive n)» occupati: la playlist si salta.
fn posto_nel_ripristino(
    dal_backup: &PlaylistState,
    qui: &BTreeMap<&PlaylistKey, &PlaylistState>,
    chiavi_dal_backup: &BTreeSet<&PlaylistKey>,
) -> Option<(PlaylistState, Option<String>)> {
    match qui.get(&dal_backup.key) {
        Some(locale) if !stessa_nascita(locale.created_at, dal_backup.created_at) => {}
        _ => return Some((dal_backup.clone(), None)),
    }
    let scelto = trova_posto::<&PlaylistState, std::convert::Infallible>(
        &dal_backup.name,
        ETICHETTA_ACCANTO,
        |nome| {
            let chiave = PlaylistKey::compute(Some(nome));
            // Il primo nome è quello di là: la sua chiave è quella del
            // backup, che qui porta l'altra playlist.
            let chiave = if nome == dal_backup.name.trim() {
                dal_backup.key.clone()
            } else {
                chiave
            };
            if let Some(locale) = qui.get(&chiave) {
                return Ok(
                    if chiave != dal_backup.key
                        && stessa_nascita(locale.created_at, dal_backup.created_at)
                    {
                        Occupato::Suo(*locale)
                    } else {
                        Occupato::Altrui(locale.name.clone())
                    },
                );
            }
            if chiave != dal_backup.key && chiavi_dal_backup.contains(&chiave) {
                return Ok(Occupato::Altrui(nome.to_owned()));
            }
            Ok(Occupato::Libero)
        },
    );
    match scelto {
        Ok(Some(Posto::Suo(locale))) => Some((
            PlaylistState {
                key: locale.key.clone(),
                name: locale.name.clone(),
                ..dal_backup.clone()
            },
            None,
        )),
        Ok(Some(Posto::Nuovo { nome, accanto_a })) => Some((
            PlaylistState {
                key: PlaylistKey::compute(Some(&nome)),
                name: nome,
                ..dal_backup.clone()
            },
            accanto_a,
        )),
        Ok(None) => None,
        Err(mai) => match mai {},
    }
}

/// Quel che il ripristino farebbe, senza averlo fatto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestorePlan {
    /// I brani da aggiornare, ordinati per chiave.
    pub tracks: Vec<TrackChange>,
    /// Quanti brani del backup ci sono già, identici.
    ///
    /// È il numero che dice se il ripristino serve. Senza, l'anteprima direbbe
    /// «1 401 brani» e nessuno saprebbe se sono da toccare tutti o nessuno.
    pub tracks_unchanged: usize,
    /// I brani del backup di cui qui non esiste nessun file.
    ///
    /// Il ripristino **non li crea**: una riga senza file non si può aprire, la
    /// scansione successiva la toglierebbe, e i conteggi della libreria
    /// mentirebbero. Sono qui perché chi guarda sappia cosa gli manca.
    pub tracks_absent: Vec<TrackKey>,
    /// Le playlist da creare o riscrivere.
    pub playlists: Vec<PlaylistChange>,
    /// Quante playlist del backup sono già a posto.
    pub playlists_unchanged: usize,
    /// Le cartelle sorvegliate da aggiungere, nell'ordine del backup.
    pub roots_to_add: Vec<RootToAdd>,
    /// Le skin da scaricare e installare, ordinate.
    pub skins_to_install: Vec<String>,
    /// Quante skin del backup sono già installate.
    pub skins_present: usize,
    /// Le bozze dello Studio da scrivere, ordinate.
    pub drafts_to_write: Vec<String>,
    /// Quante bozze del backup ci sono già.
    pub drafts_present: usize,
}

impl RestorePlan {
    /// Non c'è niente da fare.
    ///
    /// `tracks_absent` **non** conta: è un elenco da leggere, non un lavoro da
    /// svolgere, e un ripristino che si rifiutasse di dirsi concluso finché
    /// tutti i file non tornano al loro posto non si concluderebbe mai.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
            && self.playlists.is_empty()
            && self.roots_to_add.is_empty()
            && self.skins_to_install.is_empty()
            && self.drafts_to_write.is_empty()
    }
}

/// I brani locali per chiave, con i doppioni fusi.
///
/// # Perché [`merge_stats`] e non [`crate::merge::collapse_duplicates`]
///
/// Sono due domande diverse, e qui sbagliarla si paga in dati gonfiati.
/// `collapse_duplicates` **somma** i conteggi, ed è giusto quando ci si chiede
/// «quante volte in tutto è stato ascoltato questo brano, in tutte le sue
/// copie»: è la domanda dell'importatore del vecchio database.
///
/// Qui la domanda è «quale valore unico va scritto in tutte le righe che sono
/// questo brano», e la somma darebbe un ciclo che non si ferma: due righe da 3
/// e 4 diventerebbero 7, scritto in entrambe; il salvataggio dopo leggerebbe 7
/// e 7 e ne farebbe 14, e così a ogni passata, senza che nessuno se ne accorga
/// finché i numeri non diventano assurdi. Il massimo invece è stabile: 4 resta
/// 4 per sempre.
fn fold_local(local_tracks: &[(TrackKey, TrackStats)]) -> BTreeMap<&TrackKey, TrackStats> {
    let mut out: BTreeMap<&TrackKey, TrackStats> = BTreeMap::new();
    for (key, stats) in local_tracks {
        out.entry(key)
            .and_modify(|gia| *gia = merge_stats(gia, stats))
            .or_insert(*stats);
    }
    out
}

/// Decide cosa farebbe un ripristino.
///
/// # I brani
///
/// Per ogni brano del backup che qui esiste, il risultato è
/// [`merge_stats`]`(qui, backup)`: commutativo, idempotente, e incapace di far
/// scendere un conteggio o di cancellare un voto con uno zero. Se la fusione
/// non cambia niente, il brano finisce in [`RestorePlan::tracks_unchanged`] e
/// non nel piano.
///
/// I brani del backup di cui qui non c'è nessun file finiscono in
/// [`RestorePlan::tracks_absent`] e **basta**: vedi la nota su quel campo.
///
/// # Le playlist
///
/// Decide `updated_at`, sul record intero: se la versione del backup è più
/// recente si sostituiscono nome, descrizione, regole **e** appartenenza;
/// altrimenti non si tocca niente. Non c'è una fusione a metà di una lista
/// ordinata che sia difendibile — l'ordine è l'informazione — e a parità esatta
/// di data vince quel che c'è già, perché è la direzione che non riscrive.
///
/// I membri che qui non esistono si saltano, e le posizioni si ricompattano:
/// una playlist non può avere buchi. Una copia così, a cui manca qualcosa,
/// prende la data del backup meno uno: non è quella versione, e il ripristino
/// fatto dopo la scansione deve poterla ricompletare. Le playlist **automatiche** ricevono le
/// regole ma mai l'appartenenza: ogni dispositivo la ricalcola, ed è per questo
/// che restano vere anche sui brani che l'altro dispositivo non ha.
///
/// # Le cartelle, le skin, le bozze
///
/// Le cartelle sorvegliate si **uniscono**, con quelle di qui davanti, e non se
/// ne toglie mai nessuna. Il confronto passa da [`path_key`], così una barra o
/// una maiuscola diverse non aggiungono due volte la stessa cartella.
///
/// Skin e bozze si aggiungono solo se **mancano**: sovrascriverne una che c'è
/// significherebbe buttare via una skin modificata a mano o una bozza non
/// finita, che è esattamente il lavoro che nessun backup ha mai salvato.
#[must_use]
pub fn plan_restore(input: &RestoreInput<'_>) -> RestorePlan {
    let locali = fold_local(input.local_tracks);
    let presenti: BTreeSet<&TrackKey> = locali.keys().copied().collect();

    // ── brani ───────────────────────────────────────────────────────────────
    let mut tracks: BTreeMap<&TrackKey, TrackChange> = BTreeMap::new();
    let mut tracks_unchanged = 0usize;
    let mut tracks_absent: BTreeSet<&TrackKey> = BTreeSet::new();
    for (key, dal_backup) in input.backup_tracks {
        let Some(qui) = locali.get(key) else {
            tracks_absent.insert(key);
            continue;
        };
        let after = merge_stats(qui, dal_backup);
        if after == *qui {
            tracks_unchanged = tracks_unchanged.saturating_add(1);
            continue;
        }
        tracks.insert(
            key,
            TrackChange {
                key: key.clone(),
                before: *qui,
                after,
            },
        );
    }

    // ── playlist ────────────────────────────────────────────────────────────
    let qui_playlist: BTreeMap<&PlaylistKey, &PlaylistState> = input
        .local_playlists
        .iter()
        .map(|playlist| (&playlist.key, playlist))
        .collect();
    let chiavi_dal_backup: BTreeSet<&PlaylistKey> =
        input.backup_playlists.iter().map(|p| &p.key).collect();
    let mut playlists: BTreeMap<PlaylistKey, PlaylistChange> = BTreeMap::new();
    let mut playlists_unchanged = 0usize;
    for originale in input.backup_playlists {
        // Una playlist di qui con lo stesso nome ma nata in un altro momento
        // è un'altra playlist (D-11): non si riscrive. Quella del backup va
        // accanto, «Serata (Drive)», o nella sua copia accanto di un
        // ripristino di prima, che si ritrova dalla data di nascita. Da lì in
        // giù valgono le regole di sempre, sul posto scelto.
        let Some((posto, beside_of)) =
            posto_nel_ripristino(originale, &qui_playlist, &chiavi_dal_backup)
        else {
            continue;
        };
        let dal_backup = &posto;
        let esistente = qui_playlist.get(&dal_backup.key).copied();
        // A parità esatta di data vince quel che c'è già: è la direzione che
        // non riscrive, e rende il piano vuoto quando si rifà un ripristino.
        if esistente.is_some_and(|qui| dal_backup.updated_at <= qui.updated_at) {
            playlists_unchanged = playlists_unchanged.saturating_add(1);
            continue;
        }
        let members_in_backup = if dal_backup.is_smart {
            0
        } else {
            dal_backup.members.len()
        };
        let members: Vec<TrackKey> = if dal_backup.is_smart {
            Vec::new()
        } else {
            dal_backup
                .members
                .iter()
                .filter(|key| presenti.contains(key))
                .cloned()
                .collect()
        };
        // Una copia a cui mancano dei brani non è la versione del backup, e
        // non deve dirsi tale: prende la sua data **meno uno**. Con la data
        // del backup la copia monca di un PC appena reinstallato vinceva per
        // sempre — il ripristino dopo la scansione la diceva «invariata», e la
        // passata di backup la caricava su Drive sopra quella intera (R06-1).
        // Così il backup resta più recente: la fusione tiene la versione
        // intera, e un ripristino fatto dopo la scansione la ricompleta. Una
        // modifica a mano la porta ad adesso, e vince com'è giusto.
        let incompleta = members.len() < members_in_backup;
        let dopo = PlaylistState {
            members,
            updated_at: if incompleta {
                dal_backup.updated_at.saturating_sub(1)
            } else {
                dal_backup.updated_at
            },
            ..dal_backup.clone()
        };
        // Quel che l'applicazione scriverebbe è già quel che c'è: è la copia
        // monca di un ripristino fatto prima, e nessun file nuovo è arrivato.
        // Il piano resta vuoto, come promette l'idempotenza. La data di
        // creazione è quella che l'applicazione terrebbe, la più antica.
        if let Some(qui) = esistente {
            let scritta = PlaylistState {
                created_at: qui.created_at.min(dopo.created_at),
                ..dopo.clone()
            };
            if scritta == *qui {
                playlists_unchanged = playlists_unchanged.saturating_add(1);
                continue;
            }
        }
        playlists.insert(
            dal_backup.key.clone(),
            PlaylistChange {
                playlist: dopo,
                is_new: esistente.is_none(),
                members_in_backup,
                beside_of,
            },
        );
    }

    // ── cartelle sorvegliate ────────────────────────────────────────────────
    let mut chiavi_radici: BTreeSet<String> = input
        .local_roots
        .iter()
        .map(|root| path_key(root, input.path_rules))
        .collect();
    let mut roots_to_add = Vec::new();
    for (indice, root) in input.backup_roots.iter().enumerate() {
        // `insert` restituisce `false` se c'era già: fa da controllo di
        // presenza e da deduplica del backup con sé stesso in un colpo solo.
        if !chiavi_radici.insert(path_key(root, input.path_rules)) {
            continue;
        }
        roots_to_add.push(RootToAdd {
            path: root.clone(),
            exists: input
                .backup_roots_exist
                .get(indice)
                .copied()
                .unwrap_or(false),
        });
    }

    // ── skin e bozze ────────────────────────────────────────────────────────
    let (skins_to_install, skins_present) = da_aggiungere(input.backup_skins, input.local_skins);
    let (drafts_to_write, drafts_present) = da_aggiungere(input.backup_drafts, input.local_drafts);

    RestorePlan {
        tracks: tracks.into_values().collect(),
        tracks_unchanged,
        tracks_absent: tracks_absent.into_iter().cloned().collect(),
        playlists: playlists.into_values().collect(),
        playlists_unchanged,
        roots_to_add,
        skins_to_install,
        skins_present,
        drafts_to_write,
        drafts_present,
    }
}

/// Gli identificatori del backup che qui mancano, e quanti invece ci sono già.
///
/// Ordinati, perché il piano si mostra e si confronta: un elenco che cambia
/// ordine a ogni chiamata renderebbe illeggibile un'anteprima e instabile un
/// test.
fn da_aggiungere(dal_backup: &[String], qui: &[String]) -> (Vec<String>, usize) {
    let presenti: BTreeSet<&str> = qui.iter().map(String::as_str).collect();
    let mancanti: BTreeSet<&str> = dal_backup
        .iter()
        .map(String::as_str)
        .filter(|id| !presenti.contains(id))
        .collect();
    let gia_qui = dal_backup
        .iter()
        .filter(|id| presenti.contains(id.as_str()))
        .count();
    (
        mancanti.into_iter().map(ToOwned::to_owned).collect(),
        gia_qui,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chiave(nome: &str) -> TrackKey {
        TrackKey::from_stored(nome)
    }

    fn statistiche(play_count: i64, rating: u8) -> TrackStats {
        TrackStats {
            play_count,
            rating,
            stats_updated_at: play_count,
            ..TrackStats::default()
        }
    }

    fn playlist(nome: &str, updated_at: i64, membri: &[&str]) -> PlaylistState {
        PlaylistState {
            key: PlaylistKey::from_stored(nome),
            name: nome.to_owned(),
            description: None,
            created_at: 0,
            updated_at,
            is_smart: false,
            rules: None,
            members: membri.iter().map(|m| chiave(m)).collect(),
        }
    }

    /// Un ingresso vuoto, da riempire per campi con `..`.
    fn vuoto<'a>() -> RestoreInput<'a> {
        RestoreInput {
            local_tracks: &[],
            backup_tracks: &[],
            local_playlists: &[],
            backup_playlists: &[],
            local_roots: &[],
            backup_roots: &[],
            backup_roots_exist: &[],
            local_skins: &[],
            backup_skins: &[],
            local_drafts: &[],
            backup_drafts: &[],
            path_rules: PathRules {
                case_insensitive: true,
            },
        }
    }

    #[test]
    fn niente_da_fare_da_due_parti_uguali() {
        let brani = [(chiave("a|b|c"), statistiche(5, 3))];
        let piano = plan_restore(&RestoreInput {
            local_tracks: &brani,
            backup_tracks: &brani,
            ..vuoto()
        });
        assert!(piano.is_empty());
        assert_eq!(piano.tracks_unchanged, 1, "va contato, non ignorato");
    }

    #[test]
    fn ripristinare_due_volte_non_propone_niente_la_seconda() {
        // La proprietà più importante del modulo: chi ripristina lo rifà.
        let locali = [(chiave("a|b|c"), statistiche(3, 0))];
        let dal_backup = [(chiave("a|b|c"), statistiche(17, 4))];
        let primo = plan_restore(&RestoreInput {
            local_tracks: &locali,
            backup_tracks: &dal_backup,
            ..vuoto()
        });
        assert_eq!(primo.tracks.len(), 1);

        // Si applica: le statistiche locali diventano quelle decise dal piano.
        let dopo: Vec<(TrackKey, TrackStats)> = primo
            .tracks
            .iter()
            .map(|cambio| (cambio.key.clone(), cambio.after))
            .collect();
        let secondo = plan_restore(&RestoreInput {
            local_tracks: &dopo,
            backup_tracks: &dal_backup,
            ..vuoto()
        });
        assert!(secondo.is_empty(), "il secondo piano deve essere vuoto");
        assert_eq!(secondo.tracks_unchanged, 1);
    }

    #[test]
    fn un_conteggio_non_scende_mai() {
        let locali = [(chiave("a|b|c"), statistiche(40, 0))];
        let dal_backup = [(chiave("a|b|c"), statistiche(2, 0))];
        let piano = plan_restore(&RestoreInput {
            local_tracks: &locali,
            backup_tracks: &dal_backup,
            ..vuoto()
        });
        // Il backup è più povero: non c'è niente da riportare indietro.
        assert!(piano.tracks.is_empty());
        assert_eq!(piano.tracks_unchanged, 1);
    }

    #[test]
    fn due_righe_dello_stesso_brano_non_gonfiano_i_conteggi() {
        // Due file dello stesso brano (formati diversi). Fondere con la somma
        // farebbe crescere il numero a ogni passata; il massimo lo tiene fermo.
        let locali = [
            (chiave("a|b|c"), statistiche(3, 0)),
            (chiave("a|b|c"), statistiche(4, 0)),
        ];
        let dal_backup = [(chiave("a|b|c"), statistiche(4, 0))];
        let piano = plan_restore(&RestoreInput {
            local_tracks: &locali,
            backup_tracks: &dal_backup,
            ..vuoto()
        });
        assert!(
            piano.tracks.is_empty(),
            "4 e 4 sono la stessa cosa: niente da fare"
        );
    }

    #[test]
    fn un_brano_senza_file_qui_si_elenca_e_non_si_crea() {
        let dal_backup = [(chiave("perduto|x|y"), statistiche(9, 5))];
        let piano = plan_restore(&RestoreInput {
            backup_tracks: &dal_backup,
            ..vuoto()
        });
        assert!(piano.tracks.is_empty(), "nessuna riga fantasma");
        assert_eq!(piano.tracks_absent, vec![chiave("perduto|x|y")]);
        assert!(
            piano.is_empty(),
            "un brano che manca non è un lavoro da svolgere"
        );
    }

    #[test]
    fn una_playlist_tiene_solo_i_membri_che_esistono_qui() {
        let locali = [
            (chiave("a"), statistiche(0, 0)),
            (chiave("c"), statistiche(0, 0)),
        ];
        let dal_backup = [playlist("Serata", 10, &["a", "b", "c"])];
        let piano = plan_restore(&RestoreInput {
            local_tracks: &locali,
            backup_playlists: &dal_backup,
            ..vuoto()
        });
        let cambio = piano.playlists.first().expect("una playlist");
        assert_eq!(cambio.playlist.members, vec![chiave("a"), chiave("c")]);
        assert_eq!(cambio.members_in_backup, 3, "per poter dire «2 su 3»");
        assert!(cambio.is_new);
    }

    fn nata(nome: &str, created_at: i64, updated_at: i64, membri: &[&str]) -> PlaylistState {
        PlaylistState {
            key: PlaylistKey::compute(Some(nome)),
            created_at,
            ..playlist(nome, updated_at, membri)
        }
    }

    #[test]
    fn una_playlist_omonima_diversa_entra_accanto() {
        // D-11: «Serata» di qui è nata a 500, quella del backup a 10: sono due
        // playlist con lo stesso nome. Quella di qui non si riscrive; quella del
        // backup si crea accanto, intera.
        let brani = [
            (chiave("a"), statistiche(1, 0)),
            (chiave("b"), statistiche(1, 0)),
        ];
        let locali = [nata("serata", 500, 600, &["a"])];
        let dal_backup = [nata("serata", 10, 900, &["b", "a"])];
        let piano = plan_restore(&RestoreInput {
            local_tracks: &brani,
            backup_tracks: &brani,
            local_playlists: &locali,
            backup_playlists: &dal_backup,
            ..vuoto()
        });
        assert_eq!(piano.playlists.len(), 1, "{:?}", piano.playlists);
        let cambio = &piano.playlists[0];
        assert_eq!(cambio.playlist.name, "serata (Drive)");
        assert!(cambio.is_new);
        assert_eq!(cambio.playlist.members, vec![chiave("b"), chiave("a")]);
    }

    #[test]
    fn la_stessa_playlist_si_riconosce_dalla_nascita() {
        // Nata nello stesso istante (o senza una data, che vale «non si sa»):
        // è la stessa, e valgono le regole di sempre — il backup più recente
        // la riscrive.
        for nascita in [10, 0] {
            let brani = [
                (chiave("a"), statistiche(1, 0)),
                (chiave("b"), statistiche(1, 0)),
            ];
            let locali = [nata("serata", nascita, 600, &["a"])];
            let dal_backup = [nata("serata", 10, 900, &["b", "a"])];
            let piano = plan_restore(&RestoreInput {
                local_tracks: &brani,
                backup_tracks: &brani,
                local_playlists: &locali,
                backup_playlists: &dal_backup,
                ..vuoto()
            });
            assert_eq!(piano.playlists.len(), 1);
            assert_eq!(
                piano.playlists[0].playlist.name, "serata",
                "nascita {nascita}"
            );
            assert!(!piano.playlists[0].is_new);
        }
    }

    #[test]
    fn la_copia_accanto_non_si_scontra_con_un_altra_del_backup() {
        // Il backup porta già una «serata (Drive)»: quella che va accanto
        // prende il nome dopo, e le due non si confondono.
        let brani = [(chiave("a"), statistiche(1, 0))];
        let locali = [nata("serata", 500, 600, &["a"])];
        let dal_backup = [
            nata("serata", 10, 900, &["a"]),
            nata("serata (Drive)", 20, 900, &["a"]),
        ];
        let piano = plan_restore(&RestoreInput {
            local_tracks: &brani,
            backup_tracks: &brani,
            local_playlists: &locali,
            backup_playlists: &dal_backup,
            ..vuoto()
        });
        let nomi: Vec<&str> = piano
            .playlists
            .iter()
            .map(|c| c.playlist.name.as_str())
            .collect();
        assert_eq!(nomi.len(), 2, "{nomi:?}");
        assert!(nomi.contains(&"serata (Drive 2)"), "{nomi:?}");
        assert!(nomi.contains(&"serata (Drive)"), "{nomi:?}");
    }

    #[test]
    fn una_playlist_locale_piu_recente_non_si_tocca() {
        let qui = [playlist("Serata", 100, &["a"])];
        let dal_backup = [playlist("Serata", 50, &["a", "b"])];
        let piano = plan_restore(&RestoreInput {
            local_playlists: &qui,
            backup_playlists: &dal_backup,
            ..vuoto()
        });
        assert!(piano.playlists.is_empty());
        assert_eq!(piano.playlists_unchanged, 1);
    }

    #[test]
    fn a_parita_di_data_vince_quella_che_c_e_gia() {
        let qui = [playlist("Serata", 100, &["a"])];
        let dal_backup = [playlist("Serata", 100, &["a", "b"])];
        let piano = plan_restore(&RestoreInput {
            local_playlists: &qui,
            backup_playlists: &dal_backup,
            ..vuoto()
        });
        assert!(piano.playlists.is_empty(), "altrimenti non è idempotente");
    }

    #[test]
    fn una_playlist_automatica_non_riceve_mai_l_appartenenza() {
        // Ogni dispositivo la ricalcola dalle regole: copiarne i membri la
        // renderebbe falsa sui brani che questo dispositivo non ha.
        let locali = [(chiave("a"), statistiche(0, 0))];
        let mut automatica = playlist("Preferiti", 10, &["a"]);
        automatica.is_smart = true;
        automatica.rules = Some("{\"liked\":true}".to_owned());
        let dal_backup = [automatica];
        let piano = plan_restore(&RestoreInput {
            local_tracks: &locali,
            backup_playlists: &dal_backup,
            ..vuoto()
        });
        let cambio = piano.playlists.first().expect("una playlist");
        assert!(cambio.playlist.members.is_empty());
        assert_eq!(cambio.playlist.rules.as_deref(), Some("{\"liked\":true}"));
    }

    #[test]
    fn le_cartelle_si_uniscono_e_non_si_tolgono() {
        let qui = ["C:\\Musica".to_owned()];
        let dal_backup = ["D:\\Altro".to_owned(), "C:\\Musica".to_owned()];
        let piano = plan_restore(&RestoreInput {
            local_roots: &qui,
            backup_roots: &dal_backup,
            backup_roots_exist: &[false, true],
            ..vuoto()
        });
        assert_eq!(piano.roots_to_add.len(), 1, "la seconda c'è già");
        let aggiunta = piano.roots_to_add.first().expect("una cartella");
        assert_eq!(aggiunta.path, "D:\\Altro");
        assert!(!aggiunta.exists, "si aggiunge lo stesso, ma si dice");
    }

    #[test]
    fn una_cartella_scritta_diversa_non_si_aggiunge_due_volte() {
        // Stessa cartella, barre e maiuscole diverse: su Windows è lo stesso
        // posto, e aggiungerla due volte farebbe scansionare tutto in doppio.
        let qui = ["C:\\Musica".to_owned()];
        let dal_backup = ["c:/musica/".to_owned()];
        let piano = plan_restore(&RestoreInput {
            local_roots: &qui,
            backup_roots: &dal_backup,
            backup_roots_exist: &[true],
            ..vuoto()
        });
        assert!(piano.roots_to_add.is_empty());
    }

    #[test]
    fn una_skin_gia_installata_non_si_sovrascrive() {
        let qui = ["notte".to_owned()];
        let dal_backup = ["notte".to_owned(), "giorno".to_owned()];
        let piano = plan_restore(&RestoreInput {
            local_skins: &qui,
            backup_skins: &dal_backup,
            ..vuoto()
        });
        assert_eq!(piano.skins_to_install, vec!["giorno".to_owned()]);
        assert_eq!(piano.skins_present, 1);
    }

    #[test]
    fn una_bozza_gia_qui_non_si_tocca() {
        // Una bozza è lavoro non salvato: un ripristino non se lo mangia.
        let qui = ["in-corso".to_owned()];
        let dal_backup = ["in-corso".to_owned()];
        let piano = plan_restore(&RestoreInput {
            local_drafts: &qui,
            backup_drafts: &dal_backup,
            ..vuoto()
        });
        assert!(piano.drafts_to_write.is_empty());
        assert_eq!(piano.drafts_present, 1);
    }

    #[test]
    fn il_piano_non_dipende_dall_ordine_degli_ingressi() {
        // Il piano si mostra e si confronta: due letture dello stesso database
        // in ordine diverso devono darne uno identico.
        let locali = [
            (chiave("b"), statistiche(1, 0)),
            (chiave("a"), statistiche(1, 0)),
        ];
        let girati = [
            (chiave("a"), statistiche(1, 0)),
            (chiave("b"), statistiche(1, 0)),
        ];
        let dal_backup = [
            (chiave("a"), statistiche(9, 0)),
            (chiave("b"), statistiche(9, 0)),
        ];
        let uno = plan_restore(&RestoreInput {
            local_tracks: &locali,
            backup_tracks: &dal_backup,
            ..vuoto()
        });
        let due = plan_restore(&RestoreInput {
            local_tracks: &girati,
            backup_tracks: &dal_backup,
            ..vuoto()
        });
        assert_eq!(uno, due);
    }
}
