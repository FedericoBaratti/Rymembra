//! Lo specchio dei servizi cloud: cosa c'è nelle cartelle scelte, senza
//! chiederlo alla rete.
//!
//! CONTRATTO — le firme pubbliche di questo modulo sono fissate dal piano
//! (`~/.claude/plans/studia-perfettamente-tutto-il-vectorized-sifakis.md`, § 2)
//! e le usano `rymembra-cloud` e il desktop. Le firme non si cambiano senza
//! dirlo; le funzioni in più ([`salva_radice`], [`togli_radice`],
//! [`Specchio::e_completa`], [`Specchio::dimentica_radice`], [`lega_conto`])
//! sono aggiunte. Dalla 3.2, con le app dell'utente (migrazione 037),
//! [`ContoSalvato`] porta anche `app` e `client`: annunciato, e il desktop che
//! lo costruisce è aggiornato insieme.
//!
//! # Perché uno specchio
//!
//! Per tre ragioni, tutte e tre di velocità.
//!
//! - **La camminata.** La scansione cammina «senza niente in mano»
//!   (`library::Scan::run_con`): niente lucchetto della libreria, niente rete
//!   da aspettare. La camminata di una radice del cloud legge questo specchio
//!   in memoria, e ventimila file sono millisecondi.
//! - **L'apertura.** Google Drive e MEGA aprono un file per identificativo,
//!   non per nome: dal percorso di un brano all'identificativo si passa da qui.
//! - **L'avvio.** Lo specchio si salva nel database (migrazione 036) e si
//!   ricarica con una SELECT: all'avvio la libreria del cloud c'è già, e la
//!   rete serve solo a sapere cosa è cambiato nel frattempo.
//!
//! È una cache, e come tale resta fuori dal profilo e dal backup: si ricostruisce
//! con un elenco.
//!
//! # Mai togliere su prove parziali
//!
//! Vedi [`rymembra_domain::nuvola::da_togliere`]: una voce esce dallo specchio
//! solo per una cancellazione detta dal servizio o per un elenco completo che
//! non la contiene. [`Specchio::metti`] non toglie mai niente.
//!
//! # Il percorso di una voce
//!
//! È la catena dei genitori fino alla cima del drive — la voce con `genitore =
//! None` —, coi segmenti di ciascuno: `nuvola://<conto>/Musica/Rock/01.flac`.
//! La cima **non** aggiunge un segmento suo: il suo percorso è il conto e
//! basta, e i suoi figli stanno subito sotto. È la forma di
//! [`rymembra_domain::nuvola::componi`] con i segmenti «dalla cima», e vuol
//! dire che un conto ha **una** cima: un servizio con voci senza una madre
//! visibile (le cartelle condivise con un account Google) le appende a una
//! cartella virtuale sotto la cima ([`rymembra_domain::nuvola::e_virtuale`]),
//! altrimenti i loro percorsi coinciderebbero con quello della cima.
//!
//! Una voce il cui genitore non è (ancora) nello specchio non ha percorso: non
//! è raggiungibile, la camminata non la vede e un percorso non la trova. Lo
//! diventa quando arriva il genitore, perché l'ordine in cui un servizio
//! consegna le pagine non è un contratto.
//!
//! # Il segmento si assegna una volta
//!
//! Il segmento è il nome codificato ([`rymembra_domain::nuvola::segmento_libero`]),
//! con una sigla quando il nome è già preso nella stessa cartella — Drive e MEGA
//! ammettono due `01.mp3` uno accanto all'altro, e su Windows `Song.mp3` e
//! `song.mp3` sono la stessa chiave. Si ricorda, e si salva: un segmento
//! ricalcolato a ogni avvio dipenderebbe dall'ordine di arrivo, e un brano che
//! non si è mosso cambierebbe percorso — cioè riga di libreria — perché quel
//! giorno il servizio ha elencato prima l'altro. Cambia solo quando cambiano il
//! nome o la cartella della voce.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, RwLock};

use rusqlite::Connection;

use rymembra_domain::errors::AppError;
use rymembra_domain::nuvola::{
    Cambio, Servizio, TagNoti, VoceRemota, componi, da_togliere, interessa, segmento_libero,
};
use rymembra_domain::paths::{PathRules, is_under, path_key};

use crate::library::db_error;

/// Lo specchio condiviso: lo scrive il filo di ciascun conto, lo leggono la
/// camminata della scansione e chi apre un brano.
pub type SpecchioCondiviso = Arc<RwLock<Specchio>>;

/// Un conto collegato, come sta in `nuvola_conti`.
///
/// Niente segreti qui: token, sessioni e password stanno nel portachiavi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContoSalvato {
    /// `onedrive-k3x7q2ab`.
    pub conto: String,
    /// Il servizio.
    pub servizio: Servizio,
    /// Come mostrarlo: l'email, il nome dell'account, l'indirizzo del server.
    pub nome: String,
    /// Quel che il servizio vuole ricordato e non è segreto (per WebDAV
    /// l'indirizzo e l'utente), in JSON.
    pub dettagli: Option<String>,
    /// Il cursore del flusso dei cambiamenti, se un elenco è già finito.
    pub cursore: Option<String>,
    /// Quando si è sentito il servizio l'ultima volta.
    pub ultimo_ms: Option<i64>,
    /// L'app con cui il conto è collegato (`nuvola_app.app`); `None` per
    /// WebDAV e per un conto la cui app è stata tolta.
    ///
    /// [`salva_conto`] la scrive solo quando il conto **nasce**: dopo, la
    /// cambia soltanto [`lega_conto`]. Vedi lì perché.
    pub app: Option<String>,
    /// L'identificativo che l'app aveva quando il conto ha dato il consenso.
    /// Diverso da quello di oggi vuol dire che il refresh token appartiene
    /// all'app di prima: vedi `nuvola_app::stato_del_conto`.
    pub client: Option<String>,
}

/// Una radice del cloud, come sta in `nuvola_radici`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadiceSalvata {
    /// Il percorso, quello di `library.roots`.
    pub percorso: String,
    /// Il conto.
    pub conto: String,
    /// L'identificativo della cartella sul servizio.
    pub cima: String,
    /// Il primo elenco completo è arrivato in fondo.
    pub completa: bool,
}

/// Una voce sotto una radice, per la camminata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoceSotto {
    /// Il percorso intero.
    pub percorso: String,
    /// È una cartella.
    pub cartella: bool,
    /// I byte (zero per le cartelle).
    pub dimensione: u64,
    /// L'ultima modifica.
    pub modificato_ms: i64,
}

/// Cosa è cambiato nello specchio, per chi deve salvarlo e per chi deve
/// riguardare la libreria.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toccate {
    /// Le cartelle (percorsi) in cui la scansione deve riguardare: quelle che
    /// hanno guadagnato, perso o cambiato un file, e — per un rinomina di
    /// cartella — sia il vecchio percorso sia il nuovo.
    pub cartelle: BTreeSet<String>,
    /// Le voci da scrivere in `nuvola_voci`, col segmento assegnato.
    pub messe: Vec<(String, VoceRemota, String)>,
    /// Le voci da togliere da `nuvola_voci`: `(conto, id)`.
    pub tolte: Vec<(String, String)>,
}

/// Una voce dello specchio, col posto che le si è dato.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Nodo {
    /// Quel che il servizio ha detto, l'ultima volta.
    voce: VoceRemota,
    /// Il nome nel percorso, assegnato una volta: vedi il commento in testa.
    segmento: String,
    /// Il percorso intero; `None` finché la catena dei genitori non arriva a
    /// una cima.
    percorso: Option<String>,
}

/// Lo specchio di un conto.
#[derive(Debug, Default)]
struct DelConto {
    /// Le voci, per identificativo.
    voci: HashMap<String, Nodo>,
    /// I figli di ogni cartella, per identificativo della madre (`None`: la
    /// cima). Anche quelli arrivati prima della madre: è da qui che si ritrovano
    /// quando lei arriva.
    figli: HashMap<Option<String>, BTreeSet<String>>,
    /// I segmenti presi in ogni cartella, nella forma in cui la piattaforma li
    /// confronta (`path_key`), con la voce che li tiene.
    presi: HashMap<Option<String>, HashMap<String, String>>,
    /// Le voci note che un elenco ha consegnato con un nome che non interessa
    /// più (un `.mp3` diventato `.txt`): [`Specchio::metti`] non toglie, e le
    /// lascia a [`Specchio::chiudi_elenco`], che le tratta come non elencate.
    scartate: HashSet<String>,
}

/// Lo specchio di tutti i conti, in memoria.
#[derive(Debug, Default)]
pub struct Specchio {
    /// Per conto.
    conti: HashMap<String, DelConto>,
    /// `path_key(percorso)` → `(conto, id)`: dal percorso di un brano alla
    /// voce che lo apre.
    indice: HashMap<String, (String, String)>,
    /// Le radici scelte, per `path_key(percorso)`.
    radici: BTreeMap<String, RadiceSalvata>,
}

/// Le regole con cui si confrontano i percorsi.
///
/// Una funzione e non un campo: lo specchio è `Default`, e le regole sono
/// comunque quelle della piattaforma — le stesse della scansione.
const fn regole() -> PathRules {
    PathRules::for_current_platform()
}

/// La cartella che contiene un percorso dello specchio.
///
/// Sul testo: i segmenti non contengono mai `/` (sono codificati), quindi
/// l'ultimo `/` separa la cartella dal nome. La cima non ha una cartella madre.
fn madre(percorso: &str) -> Option<&str> {
    let (testa, _) = percorso.rsplit_once('/')?;
    // `nuvola://conto` tagliato all'ultimo `/` dà `nuvola:/`: non è una
    // cartella, è lo schema.
    (testa.len() > rymembra_domain::nuvola::SCHEMA.len().saturating_sub(1)).then_some(testa)
}

impl DelConto {
    /// Il percorso che spetta a una voce, dalla madre.
    fn percorso_di(&self, conto: &str, nodo: &Nodo) -> Option<String> {
        match nodo.voce.genitore.as_deref() {
            None => Some(componi(conto, std::iter::empty())),
            // Madre di sé stessa: un servizio che dice così non ha una catena
            // che arrivi in cima, e un percorso calcolato su sé stesso
            // sarebbe un'invenzione.
            Some(genitore) if genitore == nodo.voce.id => None,
            Some(genitore) => self
                .voci
                .get(genitore)
                .and_then(|madre| madre.percorso.as_deref())
                .map(|sopra| format!("{sopra}/{}", nodo.segmento)),
        }
    }

    /// Lascia il segmento di una voce nella sua cartella.
    fn lascia_il_posto(&mut self, id: &str, genitore: Option<&String>, segmento: &str) {
        let chiave = path_key(segmento, regole());
        let madre = genitore.cloned();
        if let Some(presi) = self.presi.get_mut(&madre) {
            if presi.get(&chiave).is_some_and(|chi| chi == id) {
                presi.remove(&chiave);
            }
            if presi.is_empty() {
                self.presi.remove(&madre);
            }
        }
        if let Some(figli) = self.figli.get_mut(&madre) {
            figli.remove(id);
            if figli.is_empty() {
                self.figli.remove(&madre);
            }
        }
    }

    /// Trova un posto a una voce nella sua cartella e lo prende.
    fn prendi_il_posto(&mut self, voce: &VoceRemota) -> String {
        let madre = voce.genitore.clone();
        let presi = self.presi.get(&madre);
        let occupato = |segmento: &str| {
            presi.is_some_and(|presi| presi.contains_key(&path_key(segmento, regole())))
        };
        let segmento = segmento_libero(&voce.nome, &voce.id, &occupato);
        self.occupa(voce, &segmento);
        segmento
    }

    /// Segna un segmento come preso, e la voce fra i figli della sua madre.
    fn occupa(&mut self, voce: &VoceRemota, segmento: &str) {
        let madre = voce.genitore.clone();
        self.presi
            .entry(madre.clone())
            .or_default()
            .insert(path_key(segmento, regole()), voce.id.clone());
        self.figli.entry(madre).or_default().insert(voce.id.clone());
    }

    /// Gli identificativi sotto una cartella, lei esclusa, in profondità.
    ///
    /// Con un registro dei visitati: un servizio che consegnasse un anello di
    /// genitori non deve far girare lo specchio per sempre.
    fn discendenti(&self, id: &str) -> Vec<String> {
        let mut visti: HashSet<String> = HashSet::from([id.to_owned()]);
        let mut fuori = Vec::new();
        let mut pila: Vec<String> = vec![id.to_owned()];
        while let Some(adesso) = pila.pop() {
            let Some(figli) = self.figli.get(&Some(adesso)) else {
                continue;
            };
            for figlio in figli.iter().rev() {
                if visti.insert(figlio.clone()) {
                    fuori.push(figlio.clone());
                    pila.push(figlio.clone());
                }
            }
        }
        fuori
    }
}

impl Specchio {
    /// Ricarica tutto dal database.
    ///
    /// I segmenti si rileggono **come sono stati salvati**, non si ricalcolano:
    /// vedi il commento in testa.
    ///
    /// # Errori
    ///
    /// Quelli del database.
    pub fn carica(conn: &Connection) -> Result<Self, AppError> {
        let mut specchio = Self::default();
        for (conto, nodo) in leggi_voci(conn, None)? {
            let del_conto = specchio.conti.entry(conto).or_default();
            del_conto.occupa(&nodo.voce, &nodo.segmento);
            del_conto.voci.insert(nodo.voce.id.clone(), nodo);
        }
        let nomi: Vec<String> = specchio.conti.keys().cloned().collect();
        for conto in nomi {
            specchio.ricalcola_dalle_cime(&conto);
        }
        for radice in leggi_radici(conn, None)? {
            specchio.aggiungi_radice(radice);
        }
        Ok(specchio)
    }

    /// Rimette un conto com'è nel database: le sue voci, i loro posti, e se
    /// le sue radici sono complete.
    ///
    /// Per quando una scrittura è fallita: le voci di quella chiamata sono già
    /// cambiate in memoria, e riapplicare gli stessi cambiamenti non
    /// scriverebbe niente — la memoria li ha già, e una voce uguale non si
    /// riscrive. Rimessa la memoria come il database, i cambiamenti tornano a
    /// essere cambiamenti.
    ///
    /// Le radici restano quelle della memoria: una radice appena scelta, la
    /// cui scrittura è caduta, si ritrova incompleta e si rielenca. Gli altri
    /// conti non si toccano.
    pub fn rimpiazza_conto(&mut self, conto: &str, letto: ContoLetto) {
        self.conti.remove(conto);
        self.indice.retain(|_, (di, _)| di != conto);
        let mut del_conto = DelConto::default();
        for nodo in letto.voci {
            del_conto.occupa(&nodo.voce, &nodo.segmento);
            del_conto.voci.insert(nodo.voce.id.clone(), nodo);
        }
        if !del_conto.voci.is_empty() {
            self.conti.insert(conto.to_owned(), del_conto);
            self.ricalcola_dalle_cime(conto);
        }
        for radice in self.radici.values_mut().filter(|r| r.conto == conto) {
            radice.completa = letto
                .complete
                .contains(&path_key(&radice.percorso, regole()));
        }
    }

    /// I percorsi di un conto, dalla cima in giù: è l'unico ordine in cui
    /// ognuno trova già quello della madre.
    fn ricalcola_dalle_cime(&mut self, conto: &str) {
        let cime: Vec<String> = self
            .conti
            .get(conto)
            .and_then(|c| c.figli.get(&None))
            .map(|cime| cime.iter().cloned().collect())
            .unwrap_or_default();
        let mut ignorate = Toccate::default();
        for cima in cime {
            self.ricalcola(conto, &cima, &mut ignorate);
        }
    }

    /// Scrive nel database quel che una chiamata ha cambiato.
    ///
    /// Tutto o niente: dentro un savepoint, che apre la transazione se chi
    /// chiama non ne ha una, e si appoggia alla sua se ce l'ha.
    ///
    /// Prima le voci, poi le cancellazioni: vale perché nessuna chiamata dello
    /// specchio consegna la stessa voce in tutte e due — [`Specchio::metti`]
    /// non toglie, [`Specchio::chiudi_elenco`] non mette, e
    /// [`Specchio::applica`] tiene solo l'ultima parola.
    ///
    /// # Errori
    ///
    /// Quelli del database.
    pub fn salva(conn: &Connection, toccate: &Toccate) -> Result<(), AppError> {
        if toccate.messe.is_empty() && toccate.tolte.is_empty() {
            return Ok(());
        }
        con_un_savepoint(conn, "nuvola_salva", || {
            let mut metti = conn
                .prepare_cached(
                    "INSERT INTO nuvola_voci (conto, id, genitore, nome, segmento, cartella,
                                              dimensione, modificato_ms, versione, tag)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(conto, id) DO UPDATE SET
                       genitore = excluded.genitore, nome = excluded.nome,
                       segmento = excluded.segmento, cartella = excluded.cartella,
                       dimensione = excluded.dimensione,
                       modificato_ms = excluded.modificato_ms,
                       versione = excluded.versione, tag = excluded.tag",
                )
                .map_err(|err| db_error("scrittura dello specchio del cloud", &err))?;
            for (conto, voce, segmento) in &toccate.messe {
                let tag = voce
                    .tag
                    .as_ref()
                    .and_then(|tag| serde_json::to_string(&TagSalvati::from(tag)).ok());
                metti
                    .execute(rusqlite::params![
                        conto,
                        voce.id,
                        voce.genitore,
                        voce.nome,
                        segmento,
                        i64::from(voce.cartella),
                        i64::try_from(voce.dimensione).unwrap_or(i64::MAX),
                        voce.modificato_ms,
                        voce.versione,
                        tag,
                    ])
                    .map_err(|err| db_error("scrittura dello specchio del cloud", &err))?;
            }
            let mut togli = conn
                .prepare_cached("DELETE FROM nuvola_voci WHERE conto = ?1 AND id = ?2")
                .map_err(|err| db_error("scrittura dello specchio del cloud", &err))?;
            for (conto, id) in &toccate.tolte {
                togli
                    .execute([conto, id])
                    .map_err(|err| db_error("scrittura dello specchio del cloud", &err))?;
            }
            Ok(())
        })
    }

    /// Mette (o aggiorna) delle voci di un conto: una pagina d'elenco, per
    /// esempio. Non toglie mai niente.
    ///
    /// Le voci arrivano con i loro antenati, in qualunque ordine. Si tengono le
    /// cartelle e i file che interessano ([`interessa`]); un file noto che
    /// arriva con un nome che non interessa più resta com'era, e lo toglierà
    /// il [`Specchio::chiudi_elenco`] di questo elenco — un elenco è una prova
    /// parziale finché non arriva in fondo.
    pub fn metti(&mut self, conto: &str, voci: Vec<VoceRemota>) -> Toccate {
        let mut toccate = Toccate::default();
        for voce in voci {
            self.metti_una(conto, voce, false, &mut toccate);
        }
        toccate
    }

    /// Chiude un elenco **completo** della cartella `cima`: toglie quel che
    /// stava sotto `cima` e non è fra `elencati`, e segna la radice completa.
    ///
    /// `elencati` sono gli identificativi che l'elenco ha consegnato. Quelli
    /// che [`Specchio::metti`] ha visto con un nome che non interessa contano
    /// come non elencati: l'elenco li ha nominati, ma come qualcosa che allo
    /// specchio non serve.
    pub fn chiudi_elenco(
        &mut self,
        conto: &str,
        cima: &str,
        elencati: &HashSet<String>,
    ) -> Toccate {
        let mut toccate = Toccate::default();
        let via: Vec<String> = match self.conti.get(conto) {
            Some(del_conto) => {
                let tenuti: HashSet<&str> = elencati
                    .iter()
                    .map(String::as_str)
                    .filter(|id| !del_conto.scartate.contains(*id))
                    .collect();
                let sotto = del_conto.discendenti(cima);
                da_togliere(sotto.iter().map(String::as_str), &tenuti, true)
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            }
            None => Vec::new(),
        };
        for id in via {
            self.togli_albero(conto, &id, &mut toccate);
        }
        if let Some(del_conto) = self.conti.get_mut(conto) {
            del_conto.scartate.retain(|id| !elencati.contains(id));
        }
        for radice in self.radici.values_mut() {
            if radice.conto == conto && radice.cima == cima {
                radice.completa = true;
            }
        }
        toccate
    }

    /// Applica i cambiamenti del flusso del servizio.
    ///
    /// Una cancellazione detta dal servizio toglie la voce e, per una
    /// cartella, tutto quel che le stava sotto. Un file noto che cambia nome
    /// in qualcosa che non interessa più (`a.mp3` → `a.txt`) è anche lui una
    /// prova esplicita, e si toglie.
    ///
    /// # Tolta e rimessa nello stesso giro
    ///
    /// Un file sostituito su Dropbox arriva come cancellazione e poi come voce
    /// nuova, con lo stesso identificativo; un figlio portato fuori da una
    /// cartella cancellata arriva dopo la cancellazione della madre. In
    /// memoria vince l'ultima parola, e [`Specchio::salva`] — che scrive prima
    /// tutte le voci e poi tutte le cancellazioni — deve dire lo stesso: alla
    /// fine si tiene da `messe` solo quel che in memoria c'è, e da `tolte`
    /// solo quel che non c'è più.
    pub fn applica(&mut self, conto: &str, cambi: Vec<Cambio>) -> Toccate {
        let mut toccate = Toccate::default();
        for cambio in cambi {
            match cambio {
                Cambio::Voce(voce) => self.metti_una(conto, voce, true, &mut toccate),
                Cambio::Tolta { id } => self.togli_albero(conto, &id, &mut toccate),
            }
        }
        let c_e = |id: &str| {
            self.conti
                .get(conto)
                .is_some_and(|del_conto| del_conto.voci.contains_key(id))
        };
        toccate.messe.retain(|(_, voce, _)| c_e(&voce.id));
        toccate.tolte.retain(|(_, id)| !c_e(id));
        toccate
    }

    /// Il percorso di una voce.
    #[must_use]
    pub fn percorso(&self, conto: &str, id: &str) -> Option<String> {
        self.conti.get(conto)?.voci.get(id)?.percorso.clone()
    }

    /// La voce di un percorso, con le regole di confronto della piattaforma:
    /// `(conto, voce)`.
    #[must_use]
    pub fn voce(&self, percorso: &str) -> Option<(&str, &VoceRemota)> {
        let (conto, id) = self.indice.get(&path_key(percorso, regole()))?;
        let nodo = self.conti.get(conto)?.voci.get(id)?;
        Some((conto.as_str(), &nodo.voce))
    }

    /// Le voci sotto una radice, cartelle comprese. `None` se la radice non è
    /// nello specchio.
    ///
    /// # Il terzo caso: una cartella che c'era
    ///
    /// Un percorso che non porta a nessuna voce, ma che sta sotto una radice
    /// già elencata per intero, è una cartella che **non c'è più** — rinominata
    /// o cancellata sul servizio — e vale una cartella vuota, non una
    /// sconosciuta. È quel che serve alla scansione dopo un rinomina: la
    /// cartella vecchia si giudica vuota e quella nuova piena, nella stessa
    /// passata, e i brani si riconoscono spostati invece di entrare doppi. Una
    /// sconosciuta (`None`) la scansione la salta come un disco staccato.
    ///
    /// Finché il primo elenco di una radice non è arrivato in fondo, quel che
    /// manca dallo specchio non prova niente: chi cammina chiede anche
    /// [`Specchio::e_completa`].
    ///
    /// E vale solo dentro una radice **al suo posto**: una radice rinominata,
    /// spostata o tolta sul servizio ha lasciato il suo percorso vuoto, e
    /// quel vuoto non è una cartella svuotata — è una radice che sta altrove,
    /// o da nessuna parte. Vedi [`Specchio::raggiungibile`].
    #[must_use]
    pub fn sotto(&self, radice: &str) -> Option<Vec<VoceSotto>> {
        let Some((conto, id)) = self.indice.get(&path_key(radice, regole())) else {
            return self.e_completa(radice).then(Vec::new);
        };
        let del_conto = self.conti.get(conto)?;
        if !del_conto.voci.get(id)?.voce.cartella {
            return None;
        }
        Some(
            del_conto
                .discendenti(id)
                .iter()
                .filter_map(|figlio| del_conto.voci.get(figlio))
                .filter_map(|nodo| {
                    Some(VoceSotto {
                        percorso: nodo.percorso.clone()?,
                        cartella: nodo.voce.cartella,
                        dimensione: nodo.voce.dimensione,
                        modificato_ms: nodo.voce.modificato_ms,
                    })
                })
                .collect(),
        )
    }

    /// Il percorso sta sotto una radice il cui primo elenco è arrivato in
    /// fondo?
    ///
    /// È la domanda che [`Specchio::sotto`] non può fare per chi chiama: una
    /// radice appena scelta ha nello specchio le pagine arrivate finora, e una
    /// camminata che le prendesse per tutto direbbe «spariti» i brani delle
    /// pagine che mancano. Chi cammina una radice non completa la dichiara
    /// monca, e la scansione non toglie niente.
    #[must_use]
    pub fn e_completa(&self, percorso: &str) -> bool {
        self.radici.values().any(|radice| {
            radice.completa
                && is_under(percorso, &radice.percorso, regole())
                && self.al_suo_posto(radice)
        })
    }

    /// Il percorso è una radice scelta, o le sta sotto, e quella radice è al
    /// suo posto: è quel che la scansione chiede prima di giudicare le righe
    /// di una cartella del cloud.
    ///
    /// Anche una sottocartella: i cambiamenti fanno riguardare la cartella del
    /// file che è cambiato, non la radice intera.
    ///
    /// # Una radice che non è più dove è stata scelta
    ///
    /// La cima di una radice rinominata o spostata ha un percorso nuovo, e il
    /// suo vecchio è vuoto: una camminata lì direbbe «sparito» ogni brano, con
    /// ascolti e voti. Allo stesso modo una radice la cui cima è stata tolta.
    /// Per la scansione quella radice è un disco staccato: le sue righe
    /// restano, finché il trasloco ([`Specchio::radici_traslocate`]) non le
    /// porta al nome nuovo o l'utente non la sceglie di nuovo.
    #[must_use]
    pub fn raggiungibile(&self, percorso: &str) -> bool {
        self.radici.values().any(|radice| {
            is_under(percorso, &radice.percorso, regole()) && self.al_suo_posto(radice)
        })
    }

    /// Le radici di un conto la cui cima ha oggi un percorso diverso da quello
    /// salvato: `(radice, percorso di adesso)`.
    ///
    /// Sono le cartelle scelte che l'utente ha rinominato o spostato sul
    /// servizio. Ci finiscono solo dove la cima ha un identificativo stabile
    /// (OneDrive, Google Drive): su Dropbox e WebDAV l'identificativo è il
    /// percorso, un rinomina toglie la cima e ne porta una nuova, e la radice
    /// resta solo non raggiungibile.
    #[must_use]
    pub fn radici_traslocate(&self, conto: &str) -> Vec<(RadiceSalvata, String)> {
        self.radici
            .values()
            .filter(|radice| radice.conto == conto)
            .filter_map(|radice| {
                let adesso = self.percorso(conto, &radice.cima)?;
                (path_key(&adesso, regole()) != path_key(&radice.percorso, regole()))
                    .then(|| (radice.clone(), adesso))
            })
            .collect()
    }

    /// La cima della radice c'è, e la catena le dà il percorso salvato.
    fn al_suo_posto(&self, radice: &RadiceSalvata) -> bool {
        self.percorso(&radice.conto, &radice.cima)
            .is_some_and(|adesso| {
                path_key(&adesso, regole()) == path_key(&radice.percorso, regole())
            })
    }

    /// Le radici di un conto.
    #[must_use]
    pub fn radici(&self, conto: &str) -> Vec<RadiceSalvata> {
        self.radici
            .values()
            .filter(|radice| radice.conto == conto)
            .cloned()
            .collect()
    }

    /// Registra una radice scelta (non ancora completa).
    ///
    /// Una radice con lo stesso percorso prende il posto di quella che c'era.
    pub fn aggiungi_radice(&mut self, radice: RadiceSalvata) {
        self.radici
            .insert(path_key(&radice.percorso, regole()), radice);
    }

    /// Dimentica una radice tolta dalla libreria. Le voci restano: possono
    /// stare sotto un'altra radice, e comunque si tolgono solo su prova.
    pub fn dimentica_radice(&mut self, percorso: &str) -> Option<RadiceSalvata> {
        self.radici.remove(&path_key(percorso, regole()))
    }

    /// Dimentica un conto: le voci, e le radici se non `tieni_radici`. Le
    /// righe della libreria restano.
    ///
    /// Per questo [`Toccate::cartelle`] torna vuoto: non c'è niente da far
    /// riguardare alla scansione, e anzi le righe di quel conto devono restare
    /// come sono — attenuate, «da ricollegare».
    ///
    /// # Le radici di chi tiene le cartelle
    ///
    /// Scollegando si può scegliere di tenere le cartelle nella libreria, «fino
    /// a quando lo ricolleghi». Le loro radici allora restano, **incomplete**:
    /// le voci se ne sono andate, e al ricollegamento il filo del conto le
    /// rielenca da capo. Senza la radice nessuno le avrebbe più elencate, e le
    /// cartelle sarebbero rimaste spente per sempre.
    pub fn dimentica(&mut self, conto: &str, tieni_radici: bool) -> Toccate {
        let mut toccate = Toccate::default();
        if let Some(del_conto) = self.conti.remove(conto) {
            let mut ids: Vec<String> = del_conto.voci.into_keys().collect();
            ids.sort();
            toccate.tolte = ids.into_iter().map(|id| (conto.to_owned(), id)).collect();
        }
        self.indice.retain(|_, (di, _)| di != conto);
        if tieni_radici {
            self.riapri_radici(conto);
        } else {
            self.radici.retain(|_, radice| radice.conto != conto);
        }
        toccate
    }

    /// Segna incomplete tutte le radici di un conto, e le restituisce così
    /// com'erano da salvare.
    ///
    /// Per quando il servizio non accetta più il cursore: le radici si
    /// rielencano da capo, e finché un elenco non arriva in fondo quel che
    /// manca non prova niente. Salvate incomplete **prima** del cursore nuovo,
    /// un elenco interrotto — la rete, un limite di frequenza, l'uscita — si
    /// riprende al giro dopo invece di perdersi dietro un cursore che non lo
    /// ricorda più.
    pub fn riapri_radici(&mut self, conto: &str) -> Vec<RadiceSalvata> {
        self.radici
            .values_mut()
            .filter(|radice| radice.conto == conto)
            .map(|radice| {
                radice.completa = false;
                radice.clone()
            })
            .collect()
    }

    /// Mette una voce, da un elenco (`da_cambio` falso) o dal flusso dei
    /// cambiamenti (vero). Vedi [`Specchio::metti`] e [`Specchio::applica`].
    fn metti_una(&mut self, conto: &str, voce: VoceRemota, da_cambio: bool, toccate: &mut Toccate) {
        // Un nome vuoto non è un nome: non avrebbe un segmento, e il suo
        // percorso coinciderebbe con quello della madre. Tranne la cima del
        // drive, che una madre non ce l'ha e nel percorso non entra mai: i
        // depositi la mandano senza nome (OneDrive, Dropbox), e scartarla
        // lascerebbe orfano tutto quel che le sta sotto.
        if voce.nome.is_empty() && voce.genitore.is_some() {
            return;
        }
        if !voce.cartella && !interessa(&voce.nome) {
            let nota = self
                .conti
                .get(conto)
                .is_some_and(|del_conto| del_conto.voci.contains_key(&voce.id));
            if nota && da_cambio {
                self.togli_albero(conto, &voce.id, toccate);
            } else if nota && let Some(del_conto) = self.conti.get_mut(conto) {
                del_conto.scartate.insert(voce.id);
            }
            return;
        }
        let del_conto = self.conti.entry(conto.to_owned()).or_default();
        del_conto.scartate.remove(&voce.id);

        let vecchio = del_conto.voci.get(&voce.id).cloned();
        if vecchio.as_ref().is_some_and(|nodo| nodo.voce == voce) {
            return;
        }
        let stesso_posto = vecchio
            .as_ref()
            .is_some_and(|nodo| nodo.voce.genitore == voce.genitore && nodo.voce.nome == voce.nome);
        let segmento = match vecchio.as_ref() {
            Some(nodo) if stesso_posto => nodo.segmento.clone(),
            Some(nodo) => {
                del_conto.lascia_il_posto(&voce.id, nodo.voce.genitore.as_ref(), &nodo.segmento);
                del_conto.prendi_il_posto(&voce)
            }
            None => del_conto.prendi_il_posto(&voce),
        };
        // Il contenuto di un file è cambiato senza che si muovesse: la sua
        // cartella va riguardata. Lo spostamento lo racconta `ricalcola`.
        let contenuto_cambiato = vecchio.as_ref().is_some_and(|nodo| {
            !voce.cartella
                && (nodo.voce.dimensione != voce.dimensione
                    || nodo.voce.modificato_ms != voce.modificato_ms
                    || nodo.voce.versione != voce.versione)
        });
        toccate
            .messe
            .push((conto.to_owned(), voce.clone(), segmento.clone()));
        let id = voce.id.clone();
        del_conto.voci.insert(
            id.clone(),
            Nodo {
                voce,
                segmento,
                percorso: vecchio.as_ref().and_then(|nodo| nodo.percorso.clone()),
            },
        );
        let cambiato_di_posto = self.ricalcola(conto, &id, toccate);
        if (vecchio.is_none() || contenuto_cambiato) && !cambiato_di_posto {
            // Un file nuovo, o cambiato sul posto: la sua cartella.
            if let Some(percorso) = self.percorso(conto, &id)
                && let Some(nodo) = self.conti.get(conto).and_then(|c| c.voci.get(&id))
                && !nodo.voce.cartella
                && let Some(cartella) = madre(&percorso)
            {
                toccate.cartelle.insert(cartella.to_owned());
            }
        }
    }

    /// Rifà il percorso di una voce e di tutto quel che le sta sotto, e tiene
    /// l'indice allineato.
    ///
    /// Racconta in `toccate` dove la libreria deve riguardare: per la voce di
    /// partenza, la cartella vecchia e quella nuova se è un file, il percorso
    /// vecchio e quello nuovo se è una cartella; per quel che sta sotto, solo
    /// se la partenza un percorso prima non l'aveva — altrimenti il percorso
    /// vecchio e quello nuovo della partenza dicono già tutto. Restituisce se
    /// il percorso della partenza è cambiato.
    fn ricalcola(&mut self, conto: &str, id: &str, toccate: &mut Toccate) -> bool {
        let Self { conti, indice, .. } = self;
        let Some(del_conto) = conti.get_mut(conto) else {
            return false;
        };
        let mut partenza_cambiata = false;
        let mut partenza_senza_percorso = false;
        let mut ordine = vec![id.to_owned()];
        ordine.extend(del_conto.discendenti(id));
        for (posto, adesso) in ordine.iter().enumerate() {
            let Some(nodo) = del_conto.voci.get(adesso) else {
                continue;
            };
            let nuovo = del_conto.percorso_di(conto, nodo);
            let vecchio = nodo.percorso.clone();
            let cartella = nodo.voce.cartella;
            if nuovo == vecchio {
                continue;
            }
            if let Some(vecchio) = vecchio.as_deref() {
                let chiave = path_key(vecchio, regole());
                if indice
                    .get(&chiave)
                    .is_some_and(|(di, chi)| di == conto && chi == adesso)
                {
                    indice.remove(&chiave);
                }
            }
            if let Some(nuovo) = nuovo.as_deref() {
                indice
                    .entry(path_key(nuovo, regole()))
                    .or_insert_with(|| (conto.to_owned(), adesso.clone()));
            }
            if posto == 0 {
                partenza_cambiata = true;
                partenza_senza_percorso = vecchio.is_none();
            }
            let racconta = posto == 0 || partenza_senza_percorso;
            if racconta && !cartella {
                // Un file: la cartella da cui se n'è andato e quella in cui è
                // arrivato.
                for percorso in [vecchio.as_deref(), nuovo.as_deref()].into_iter().flatten() {
                    if let Some(dove) = madre(percorso) {
                        toccate.cartelle.insert(dove.to_owned());
                    }
                }
            } else if racconta && posto == 0 && vecchio.is_some() {
                // Una cartella che aveva già un posto e l'ha cambiato: il
                // vecchio e il nuovo. Una cartella che un posto lo trova solo
                // adesso non ha niente da far riguardare da sé: lo fanno i suoi
                // file, uno per uno.
                for percorso in [vecchio.as_deref(), nuovo.as_deref()].into_iter().flatten() {
                    toccate.cartelle.insert(percorso.to_owned());
                }
            }
            if let Some(nodo) = del_conto.voci.get_mut(adesso) {
                nodo.percorso = nuovo;
            }
        }
        partenza_cambiata
    }

    /// Toglie una voce e tutto quel che le sta sotto.
    fn togli_albero(&mut self, conto: &str, id: &str, toccate: &mut Toccate) {
        let Self { conti, indice, .. } = self;
        let Some(del_conto) = conti.get_mut(conto) else {
            return;
        };
        if !del_conto.voci.contains_key(id) {
            return;
        }
        let mut via = vec![id.to_owned()];
        via.extend(del_conto.discendenti(id));
        for (posto, adesso) in via.iter().enumerate() {
            del_conto.scartate.remove(adesso);
            let Some(nodo) = del_conto.voci.remove(adesso) else {
                continue;
            };
            del_conto.lascia_il_posto(adesso, nodo.voce.genitore.as_ref(), &nodo.segmento);
            if let Some(percorso) = nodo.percorso.as_deref() {
                let chiave = path_key(percorso, regole());
                if indice
                    .get(&chiave)
                    .is_some_and(|(di, chi)| di == conto && chi == adesso)
                {
                    indice.remove(&chiave);
                }
                // Un file: la sua cartella. La cartella da cui si comincia: lei
                // stessa, che adesso è vuota — e la madre, che l'ha persa.
                if !nodo.voce.cartella {
                    if let Some(cartella) = madre(percorso) {
                        toccate.cartelle.insert(cartella.to_owned());
                    }
                } else if posto == 0 {
                    toccate.cartelle.insert(percorso.to_owned());
                }
            }
            toccate.tolte.push((conto.to_owned(), adesso.clone()));
        }
    }
}

/// Esegue `azione` dentro un savepoint: tutto o niente.
///
/// Savepoint e non `transaction()`, che vorrebbe una connessione mutabile; e
/// guardando **prima** se una transazione c'era già, per sapere chi la deve
/// chiudere se qualcosa va storto — vedi `playlists::disfa_savepoint`, da cui
/// la regola viene.
pub(crate) fn con_un_savepoint(
    conn: &Connection,
    nome: &str,
    azione: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    let nostra = conn.is_autocommit();
    conn.execute_batch(&format!("SAVEPOINT {nome}"))
        .map_err(|err| db_error("apertura di una scrittura dello specchio", &err))?;
    let esito = azione().and_then(|()| {
        conn.execute_batch(&format!("RELEASE {nome}"))
            .map_err(|err| db_error("chiusura di una scrittura dello specchio", &err))
    });
    if esito.is_err() {
        let _ = conn.execute_batch(&format!("ROLLBACK TO {nome}; RELEASE {nome}"));
        if nostra && !conn.is_autocommit() {
            let _ = conn.execute_batch("ROLLBACK");
        }
    }
    esito
}

/// [`TagNoti`] nella forma della colonna `nuvola_voci.tag`: JSON.
///
/// Una copia dei campi con i nomi del dominio, e non un `derive` sul dominio,
/// che non serializza niente da sé — come `FoglioSalvato` per i fogli `.cue`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct TagSalvati {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    titolo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artista: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artista_album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    genere: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    anno: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    traccia: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    disco: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    durata_ms: Option<u64>,
}

impl From<&TagNoti> for TagSalvati {
    fn from(tag: &TagNoti) -> Self {
        Self {
            titolo: tag.titolo.clone(),
            artista: tag.artista.clone(),
            album: tag.album.clone(),
            artista_album: tag.artista_album.clone(),
            genere: tag.genere.clone(),
            anno: tag.anno,
            traccia: tag.traccia,
            disco: tag.disco,
            durata_ms: tag.durata_ms,
        }
    }
}

impl From<TagSalvati> for TagNoti {
    fn from(tag: TagSalvati) -> Self {
        Self {
            titolo: tag.titolo,
            artista: tag.artista,
            album: tag.album,
            artista_album: tag.artista_album,
            genere: tag.genere,
            anno: tag.anno,
            traccia: tag.traccia,
            disco: tag.disco,
            durata_ms: tag.durata_ms,
        }
    }
}

/// I conti salvati.
///
/// Un conto di un servizio che questa versione non conosce — scritto da una
/// versione più nuova — si salta invece di far fallire tutti gli altri.
///
/// # Errori
///
/// Quelli del database.
pub fn conti(conn: &Connection) -> Result<Vec<ContoSalvato>, AppError> {
    let mut statement = conn
        .prepare(
            "SELECT conto, servizio, nome, dettagli, cursore, ultimo_ms, app, client
               FROM nuvola_conti ORDER BY conto",
        )
        .map_err(|err| db_error("lettura dei conti del cloud", &err))?;
    let righe = statement
        .query_map([], |riga| {
            Ok((
                riga.get::<_, String>(0)?,
                riga.get::<_, String>(1)?,
                riga.get::<_, String>(2)?,
                riga.get::<_, Option<String>>(3)?,
                riga.get::<_, Option<String>>(4)?,
                riga.get::<_, Option<i64>>(5)?,
                riga.get::<_, Option<String>>(6)?,
                riga.get::<_, Option<String>>(7)?,
            ))
        })
        .map_err(|err| db_error("lettura dei conti del cloud", &err))?;
    let mut fuori = Vec::new();
    for riga in righe {
        let (conto, servizio, nome, dettagli, cursore, ultimo_ms, app, client) =
            riga.map_err(|err| db_error("lettura dei conti del cloud", &err))?;
        let Some(servizio) = Servizio::da_chiave(&servizio) else {
            continue;
        };
        fuori.push(ContoSalvato {
            conto,
            servizio,
            nome,
            dettagli,
            cursore,
            ultimo_ms,
            app,
            client,
        });
    }
    Ok(fuori)
}

/// Scrive (o riscrive) un conto.
///
/// `app` e `client` si scrivono solo se il conto non c'era: vedi
/// [`lega_conto`].
///
/// # Errori
///
/// Quelli del database.
pub fn salva_conto(conn: &Connection, conto: &ContoSalvato) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO nuvola_conti (conto, servizio, nome, dettagli, cursore, ultimo_ms, app, client)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(conto) DO UPDATE SET
           servizio = excluded.servizio, nome = excluded.nome,
           dettagli = excluded.dettagli, cursore = excluded.cursore,
           ultimo_ms = excluded.ultimo_ms",
        rusqlite::params![
            conto.conto,
            conto.servizio.chiave(),
            conto.nome,
            conto.dettagli,
            conto.cursore,
            conto.ultimo_ms,
            conto.app,
            conto.client,
        ],
    )
    .map(|_| ())
    .map_err(|err| db_error("scrittura di un conto del cloud", &err))
}

/// Scrive il cursore di un conto **che c'è ancora**: `false` se la riga non
/// c'è più.
///
/// È la scrittura del filo del conto, al posto di [`salva_conto`]: un conto
/// scollegato mentre il filo era in rete non deve rinascere quando il filo
/// torna a salvare il cursore. `salva_conto` scrive anche se manca, ed è per
/// quando un conto si collega.
///
/// # Errori
///
/// Quelli del database.
pub fn salva_cursore(
    conn: &Connection,
    conto: &str,
    cursore: Option<&str>,
    ultimo_ms: Option<i64>,
) -> Result<bool, AppError> {
    conn.execute(
        "UPDATE nuvola_conti SET cursore = ?2, ultimo_ms = ?3 WHERE conto = ?1",
        rusqlite::params![conto, cursore, ultimo_ms],
    )
    .map(|righe| righe > 0)
    .map_err(|err| db_error("scrittura del cursore di un conto del cloud", &err))
}

/// Lega un conto a un'app, col client che l'app ha adesso.
///
/// # Perché una scrittura a parte
///
/// Il filo di un conto legge la sua riga, elenca per minuti, e la riscrive
/// con [`salva_conto`] per salvare il cursore. Se `salva_conto` riscrivesse
/// anche il legame, un'app cambiata o tolta in quei minuti tornerebbe com'era
/// alla fine dell'elenco, in silenzio. Il legame cambia solo qui: al
/// collegamento, e quando un'app si toglie (`nuvola_app::togli`).
///
/// # Errori
///
/// Quelli del database.
pub fn lega_conto(
    conn: &Connection,
    conto: &str,
    app: Option<&str>,
    client: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE nuvola_conti SET app = ?2, client = ?3 WHERE conto = ?1",
        rusqlite::params![conto, app, client],
    )
    .map(|_| ())
    .map_err(|err| db_error("legame di un conto del cloud alla sua app", &err))
}

/// Toglie un conto, le sue voci e — se non `tieni_radici` — le sue radici
/// dal database. Le radici tenute restano incomplete.
///
/// Le righe della libreria non si toccano: vedi [`Specchio::dimentica`].
///
/// # Errori
///
/// Quelli del database.
pub fn togli_conto(conn: &Connection, conto: &str, tieni_radici: bool) -> Result<(), AppError> {
    let radici = if tieni_radici {
        "UPDATE nuvola_radici SET completa = 0 WHERE conto = ?1"
    } else {
        "DELETE FROM nuvola_radici WHERE conto = ?1"
    };
    con_un_savepoint(conn, "nuvola_togli_conto", || {
        for sql in [
            "DELETE FROM nuvola_voci WHERE conto = ?1",
            radici,
            "DELETE FROM nuvola_conti WHERE conto = ?1",
        ] {
            conn.execute(sql, [conto])
                .map_err(|err| db_error("cancellazione di un conto del cloud", &err))?;
        }
        Ok(())
    })
}

/// Scrive (o riscrive) una radice in `nuvola_radici`.
///
/// # Errori
///
/// Quelli del database.
pub fn salva_radice(conn: &Connection, radice: &RadiceSalvata) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO nuvola_radici (percorso, conto, cima, completa)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(percorso) DO UPDATE SET
           conto = excluded.conto, cima = excluded.cima, completa = excluded.completa",
        rusqlite::params![
            radice.percorso,
            radice.conto,
            radice.cima,
            i64::from(radice.completa),
        ],
    )
    .map(|_| ())
    .map_err(|err| db_error("scrittura di una radice del cloud", &err))
}

/// Toglie una radice da `nuvola_radici`. Le voci dello specchio restano.
///
/// # Errori
///
/// Quelli del database.
pub fn togli_radice(conn: &Connection, percorso: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM nuvola_radici WHERE percorso = ?1", [percorso])
        .map(|_| ())
        .map_err(|err| db_error("cancellazione di una radice del cloud", &err))
}

/// Un conto com'è nel database, per [`Specchio::rimpiazza_conto`].
///
/// Si legge con la libreria in mano e si consegna allo specchio dopo averla
/// lasciata: i due lucchetti non si prendono mai uno dentro l'altro.
#[derive(Debug, Default)]
pub struct ContoLetto {
    voci: Vec<Nodo>,
    /// Le radici complete del conto, per `path_key`.
    complete: HashSet<String>,
}

/// Legge un conto per [`Specchio::rimpiazza_conto`].
///
/// # Errori
///
/// Quelli del database.
pub fn leggi_conto(conn: &Connection, conto: &str) -> Result<ContoLetto, AppError> {
    Ok(ContoLetto {
        voci: leggi_voci(conn, Some(conto))?
            .into_iter()
            .map(|(_, nodo)| nodo)
            .collect(),
        complete: leggi_radici(conn, Some(conto))?
            .into_iter()
            .filter(|radice| radice.completa)
            .map(|radice| path_key(&radice.percorso, regole()))
            .collect(),
    })
}

/// Le voci di `nuvola_voci`, di un conto o di tutti, col segmento salvato e
/// senza percorso: lo dà [`Specchio::ricalcola_dalle_cime`].
fn leggi_voci(conn: &Connection, conto: Option<&str>) -> Result<Vec<(String, Nodo)>, AppError> {
    let mut statement = conn
        .prepare(
            "SELECT conto, id, genitore, nome, segmento, cartella, dimensione,
                    modificato_ms, versione, tag
               FROM nuvola_voci WHERE ?1 IS NULL OR conto = ?1 ORDER BY conto, id",
        )
        .map_err(|err| db_error("lettura dello specchio del cloud", &err))?;
    let righe = statement
        .query_map([conto], |riga| {
            let tag: Option<String> = riga.get(9)?;
            Ok((
                riga.get::<_, String>(0)?,
                Nodo {
                    voce: VoceRemota {
                        id: riga.get(1)?,
                        genitore: riga.get(2)?,
                        nome: riga.get(3)?,
                        cartella: riga.get::<_, i64>(5)? != 0,
                        dimensione: u64::try_from(riga.get::<_, i64>(6)?).unwrap_or(0),
                        modificato_ms: riga.get(7)?,
                        versione: riga.get(8)?,
                        // Un JSON che non si rilegge vale «nessun tag
                        // noto»: è un'aggiunta, la lettura vera arriva
                        // comunque.
                        tag: tag
                            .and_then(|testo| serde_json::from_str::<TagSalvati>(&testo).ok())
                            .map(TagNoti::from),
                    },
                    segmento: riga.get(4)?,
                    percorso: None,
                },
            ))
        })
        .map_err(|err| db_error("lettura dello specchio del cloud", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("lettura dello specchio del cloud", &err))
}

/// Le radici di `nuvola_radici`, di un conto o di tutti.
fn leggi_radici(conn: &Connection, conto: Option<&str>) -> Result<Vec<RadiceSalvata>, AppError> {
    let mut statement = conn
        .prepare(
            "SELECT percorso, conto, cima, completa FROM nuvola_radici
              WHERE ?1 IS NULL OR conto = ?1",
        )
        .map_err(|err| db_error("lettura delle radici del cloud", &err))?;
    let righe = statement
        .query_map([conto], |riga| {
            Ok(RadiceSalvata {
                percorso: riga.get(0)?,
                conto: riga.get(1)?,
                cima: riga.get(2)?,
                completa: riga.get::<_, i64>(3)? != 0,
            })
        })
        .map_err(|err| db_error("lettura delle radici del cloud", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("lettura delle radici del cloud", &err))
}

/// Il frammento SQL che tiene fuori le righe del cloud: per le passate che
/// leggono i file per intero (impronta, volume, rilettura dei tag).
///
/// Un intervallo sull'indice binario di `path` e non un `LIKE`: `LIKE` non usa
/// quell'indice e piega le maiuscole. `'/' + 1` è `'0'`, quindi ogni percorso
/// che comincia con `nuvola://` sta fra i due estremi.
pub const FUORI_DALLA_NUVOLA: &str = "NOT (t.path >= 'nuvola://' AND t.path < 'nuvola:/0')";

/// Uno specchio con dentro nomi riconoscibili, per le prove che lo vogliono
/// fuori dal profilo, dal backup e dalla sincronia: restituisce le stringhe da
/// cercare in quel che esce.
#[cfg(test)]
pub(crate) fn specchio_da_non_portare(conn: &Connection) -> Vec<String> {
    let conto = "onedrive-zqzqzqzq";
    salva_conto(
        conn,
        &ContoSalvato {
            conto: conto.to_owned(),
            servizio: Servizio::OneDrive,
            nome: "zaira.specchio@outlook.com".to_owned(),
            dettagli: Some(r#"{"drive":"Drive-Segreto-Q7"}"#.to_owned()),
            cursore: Some("cursore-delta-QX9".to_owned()),
            ultimo_ms: Some(1),
            app: Some("microsoft-zqzqzqzq".to_owned()),
            client: Some("App-Segreta-Q7".to_owned()),
        },
    )
    .expect("conto");
    conn.execute(
        "INSERT INTO nuvola_app (app, piattaforma, nome, client_id, creata_ms)
         VALUES ('microsoft-zqzqzqzq', 'microsoft', 'Nome-App-Segreta-Q7', 'App-Segreta-Q7', 1)",
        [],
    )
    .expect("app");
    let mut specchio = Specchio::default();
    let toccate = specchio.metti(
        conto,
        vec![
            VoceRemota {
                id: "cima-77".to_owned(),
                genitore: None,
                nome: "root".to_owned(),
                cartella: true,
                dimensione: 0,
                modificato_ms: 0,
                versione: None,
                tag: None,
            },
            VoceRemota {
                id: "voce-segreta-77".to_owned(),
                genitore: Some("cima-77".to_owned()),
                nome: "Specchio-Segreto-77.flac".to_owned(),
                cartella: false,
                dimensione: 1,
                modificato_ms: 1,
                versione: Some("etag-segreto-77".to_owned()),
                tag: None,
            },
        ],
    );
    Specchio::salva(conn, &toccate).expect("voci");
    salva_radice(
        conn,
        &RadiceSalvata {
            percorso: format!("nuvola://{conto}/Radice-Segreta-77"),
            conto: conto.to_owned(),
            cima: "cima-77".to_owned(),
            completa: true,
        },
    )
    .expect("radice");
    [
        conto,
        "zaira.specchio",
        "Drive-Segreto-Q7",
        "cursore-delta-QX9",
        "Specchio-Segreto-77",
        "voce-segreta-77",
        "etag-segreto-77",
        "Radice-Segreta-77",
        "App-Segreta-Q7",
        "Nome-App-Segreta-Q7",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[cfg(test)]
mod prove {
    use super::*;

    const CONTO: &str = "onedrive-abcdefgh";

    /// Una cartella.
    fn cartella(id: &str, genitore: Option<&str>, nome: &str) -> VoceRemota {
        VoceRemota {
            id: id.to_owned(),
            genitore: genitore.map(str::to_owned),
            nome: nome.to_owned(),
            cartella: true,
            dimensione: 0,
            modificato_ms: 0,
            versione: None,
            tag: None,
        }
    }

    /// Un file, con dimensione e data.
    fn file(id: &str, genitore: &str, nome: &str) -> VoceRemota {
        VoceRemota {
            id: id.to_owned(),
            genitore: Some(genitore.to_owned()),
            nome: nome.to_owned(),
            cartella: false,
            dimensione: 1_000,
            modificato_ms: 10,
            versione: Some("v1".to_owned()),
            tag: None,
        }
    }

    /// La cima, `Musica` e `Rock` sotto, e due brani in `Rock`.
    fn albero() -> Vec<VoceRemota> {
        vec![
            cartella("R", None, "root"),
            cartella("M", Some("R"), "Musica"),
            cartella("K", Some("M"), "Rock"),
            file("1", "K", "01 Intro.flac"),
            file("2", "K", "02 Dopo.flac"),
        ]
    }

    fn radice(percorso: &str, cima: &str) -> RadiceSalvata {
        RadiceSalvata {
            percorso: percorso.to_owned(),
            conto: CONTO.to_owned(),
            cima: cima.to_owned(),
            completa: false,
        }
    }

    fn insieme(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    fn percorsi(sotto: Option<Vec<VoceSotto>>) -> Vec<String> {
        let mut tutti: Vec<String> = sotto
            .unwrap_or_default()
            .into_iter()
            .map(|voce| voce.percorso)
            .collect();
        tutti.sort();
        tutti
    }

    #[test]
    fn il_percorso_e_la_catena_dei_genitori_senza_la_cima() {
        let mut specchio = Specchio::default();
        specchio.metti(CONTO, albero());
        assert_eq!(
            specchio.percorso(CONTO, "1").as_deref(),
            Some("nuvola://onedrive-abcdefgh/Musica/Rock/01 Intro.flac")
        );
        assert_eq!(
            specchio.percorso(CONTO, "R").as_deref(),
            Some("nuvola://onedrive-abcdefgh")
        );
        let (conto, voce) = specchio
            .voce("NUVOLA://onedrive-abcdefgh/musica/rock/01 intro.flac")
            .expect("si trova con le regole della piattaforma");
        assert_eq!((conto, voce.id.as_str()), (CONTO, "1"));
    }

    #[test]
    fn la_cima_senza_nome_tiene_insieme_l_albero() {
        // Così la mandano i depositi: la cima del drive non ha un nome da
        // mostrare, e non entra nel percorso.
        let mut specchio = Specchio::default();
        let mut voci = albero();
        if let Some(cima) = voci.first_mut() {
            cima.nome = String::new();
        }
        specchio.metti(CONTO, voci);
        assert_eq!(
            specchio.percorso(CONTO, "1").as_deref(),
            Some("nuvola://onedrive-abcdefgh/Musica/Rock/01 Intro.flac")
        );
    }

    #[test]
    fn i_figli_arrivati_prima_della_madre_si_ritrovano() {
        // L'ordine delle pagine non è un contratto: il brano prima della sua
        // cartella, la cartella prima della cima.
        let mut specchio = Specchio::default();
        let mut voci = albero();
        voci.reverse();
        let toccate = specchio.metti(CONTO, voci);
        assert_eq!(
            specchio.percorso(CONTO, "2").as_deref(),
            Some("nuvola://onedrive-abcdefgh/Musica/Rock/02 Dopo.flac")
        );
        assert!(
            toccate
                .cartelle
                .contains("nuvola://onedrive-abcdefgh/Musica/Rock"),
            "{toccate:?}"
        );
        // Un figlio senza madre non ha percorso, e nessuno lo trova.
        let mut orfano = Specchio::default();
        orfano.metti(CONTO, vec![file("9", "Z", "solo.flac")]);
        assert_eq!(orfano.percorso(CONTO, "9"), None);
    }

    #[test]
    fn una_seconda_cima_messa_sotto_la_cartella_virtuale_ritrova_i_suoi_percorsi() {
        // Prima della 3.2.0 finita, i cambiamenti di Drive portavano una
        // cartella condivisa come una seconda cima: col percorso del conto, e
        // coi figli in concorrenza con quelli della cima vera. Quando arriva
        // al suo posto, sotto la cartella virtuale, tutto si rimette in fila.
        let mut specchio = Specchio::default();
        specchio.metti(CONTO, albero());
        specchio.applica(
            CONTO,
            vec![
                Cambio::Voce(cartella("S", None, "Musica di Marco")),
                Cambio::Voce(cartella("SM", Some("S"), "Musica")),
                Cambio::Voce(file("3", "SM", "03.flac")),
            ],
        );
        specchio.applica(
            CONTO,
            vec![
                Cambio::Voce(cartella("~condivisi", Some("R"), "Shared with me")),
                Cambio::Voce(cartella("S", Some("~condivisi"), "Musica di Marco")),
            ],
        );
        let sotto = "nuvola://onedrive-abcdefgh/Shared with me/Musica di Marco";
        assert_eq!(specchio.percorso(CONTO, "S").as_deref(), Some(sotto));
        assert_eq!(
            specchio.percorso(CONTO, "3"),
            Some(format!("{sotto}/Musica/03.flac"))
        );
        let di = |percorso: &str| specchio.voce(percorso).map(|(_, v)| v.id.clone());
        assert_eq!(di("nuvola://onedrive-abcdefgh").as_deref(), Some("R"));
        assert_eq!(
            di("nuvola://onedrive-abcdefgh/Musica").as_deref(),
            Some("M")
        );
        assert_eq!(di(&format!("{sotto}/Musica")).as_deref(), Some("SM"));
        assert_eq!(di(&format!("{sotto}/Musica/03.flac")).as_deref(), Some("3"));
        assert_eq!(di("nuvola://onedrive-abcdefgh/Musica/03.flac"), None);
    }

    #[test]
    fn si_tengono_le_cartelle_e_i_file_che_interessano() {
        let mut specchio = Specchio::default();
        let mut voci = albero();
        voci.push(file("3", "K", "copertina.jpg"));
        voci.push(file("4", "K", "disco.cue"));
        voci.push(cartella("F", Some("K"), "Foto"));
        let toccate = specchio.metti(CONTO, voci);
        assert_eq!(specchio.percorso(CONTO, "3"), None, "una foto non serve");
        assert!(specchio.percorso(CONTO, "4").is_some());
        assert!(specchio.percorso(CONTO, "F").is_some(), "le cartelle sì");
        assert!(toccate.messe.iter().all(|(_, voce, _)| voce.id != "3"));
    }

    #[test]
    fn i_nomi_doppi_prendono_una_sigla_e_il_primo_tiene_il_suo() {
        let mut specchio = Specchio::default();
        specchio.metti(CONTO, albero());
        specchio.metti(
            CONTO,
            vec![
                file("AAAAAA111111", "K", "01 Intro.flac"),
                file("BBBBBB222222", "K", "01 INTRO.flac"),
            ],
        );
        let rock = "nuvola://onedrive-abcdefgh/Musica/Rock";
        assert_eq!(
            specchio.percorso(CONTO, "1"),
            Some(format!("{rock}/01 Intro.flac")),
            "il primo arrivato non perde il nome"
        );
        assert_eq!(
            specchio.percorso(CONTO, "AAAAAA111111"),
            Some(format!("{rock}/01 Intro~111111.flac"))
        );
        // Su Windows le maiuscole non distinguono: anche questo è un doppio.
        let terzo = specchio.percorso(CONTO, "BBBBBB222222").expect("percorso");
        if regole().case_insensitive {
            assert_eq!(terzo, format!("{rock}/01 INTRO~222222.flac"));
        }
        // Tre voci, tre percorsi diversi, e ognuno porta alla sua.
        for id in ["1", "AAAAAA111111", "BBBBBB222222"] {
            let percorso = specchio.percorso(CONTO, id).expect("percorso");
            assert_eq!(
                specchio.voce(&percorso).map(|(_, v)| v.id.as_str()),
                Some(id)
            );
        }
        // Il primo se ne va: gli altri **non** si rinominano, perché il loro
        // percorso è la loro riga di libreria.
        specchio.applica(CONTO, vec![Cambio::Tolta { id: "1".to_owned() }]);
        assert_eq!(
            specchio.percorso(CONTO, "AAAAAA111111"),
            Some(format!("{rock}/01 Intro~111111.flac"))
        );
    }

    #[test]
    fn la_barra_e_il_separatore_dei_brani_si_codificano() {
        let mut specchio = Specchio::default();
        specchio.metti(
            CONTO,
            vec![
                cartella("R", None, "root"),
                cartella("A", Some("R"), "AC/DC"),
                file("1", "A", "Live | Unplugged.flac"),
            ],
        );
        let percorso = specchio.percorso(CONTO, "1").expect("percorso");
        assert_eq!(
            percorso,
            "nuvola://onedrive-abcdefgh/AC%2FDC/Live %7C Unplugged.flac"
        );
        assert_eq!(
            rymembra_domain::nuvola::segmenti(&percorso),
            ["AC/DC", "Live | Unplugged.flac"]
        );
        assert_eq!(rymembra_domain::cue::brano_d_immagine(&percorso), None);
        assert_eq!(
            specchio.voce(&percorso).map(|(_, v)| v.id.as_str()),
            Some("1")
        );
    }

    #[test]
    fn metti_non_toglie_mai_e_l_elenco_completo_si() {
        let mut specchio = Specchio::default();
        specchio.metti(CONTO, albero());
        specchio.aggiungi_radice(radice("nuvola://onedrive-abcdefgh/Musica", "M"));
        // Una seconda pagina che non nomina il brano 2, e che dice il brano 1
        // diventato un `.txt`: niente se ne va.
        let mut uno = file("1", "K", "01 Intro.txt");
        uno.dimensione = 5;
        let toccate = specchio.metti(CONTO, vec![uno]);
        assert!(toccate.tolte.is_empty());
        assert!(specchio.percorso(CONTO, "1").is_some());
        assert!(specchio.percorso(CONTO, "2").is_some());
        assert!(!specchio.e_completa("nuvola://onedrive-abcdefgh/Musica"));

        // L'elenco arriva in fondo: elencati la cima, le cartelle, e l'1
        // (come `.txt`). Se ne vanno il 2, che non c'era, e l'1, che non
        // interessa più. Sopra la cima non si tocca niente.
        let toccate = specchio.chiudi_elenco(CONTO, "M", &insieme(&["M", "K", "1"]));
        let mut tolte: Vec<&str> = toccate.tolte.iter().map(|(_, id)| id.as_str()).collect();
        tolte.sort_unstable();
        assert_eq!(tolte, ["1", "2"]);
        assert!(
            toccate
                .cartelle
                .contains("nuvola://onedrive-abcdefgh/Musica/Rock")
        );
        assert!(specchio.percorso(CONTO, "R").is_some());
        assert!(specchio.e_completa("nuvola://onedrive-abcdefgh/Musica/Rock"));
        assert!(!specchio.e_completa("nuvola://onedrive-abcdefgh/Altro"));
        assert!(specchio.radici(CONTO).iter().all(|radice| radice.completa));
    }

    #[test]
    fn un_elenco_chiuso_non_tocca_le_altre_cartelle() {
        let mut specchio = Specchio::default();
        let mut voci = albero();
        voci.push(cartella("J", Some("M"), "Jazz"));
        voci.push(file("5", "J", "Blue.flac"));
        specchio.metti(CONTO, voci);
        // L'elenco di `Rock` non ha visto `Jazz`: non è affar suo.
        let toccate = specchio.chiudi_elenco(CONTO, "K", &insieme(&["K", "1", "2"]));
        assert!(toccate.tolte.is_empty(), "{toccate:?}");
        assert!(specchio.percorso(CONTO, "5").is_some());
    }

    #[test]
    fn un_cambio_verso_un_nome_che_non_interessa_e_una_rimozione() {
        let mut specchio = Specchio::default();
        specchio.metti(CONTO, albero());
        let toccate = specchio.applica(
            CONTO,
            vec![
                Cambio::Voce(file("1", "K", "01 Intro.txt")),
                // Uno sconosciuto che non interessa: niente.
                Cambio::Voce(file("8", "K", "nota.txt")),
            ],
        );
        assert_eq!(toccate.tolte, [(CONTO.to_owned(), "1".to_owned())]);
        assert_eq!(
            toccate.cartelle.iter().collect::<Vec<_>>(),
            ["nuvola://onedrive-abcdefgh/Musica/Rock"]
        );
        assert_eq!(specchio.percorso(CONTO, "1"), None);
    }

    #[test]
    fn le_cartelle_toccate_sono_quelle_dei_file_cambiati() {
        let mut specchio = Specchio::default();
        let mut voci = albero();
        voci.push(cartella("J", Some("M"), "Jazz"));
        specchio.metti(CONTO, voci);
        let rock = "nuvola://onedrive-abcdefgh/Musica/Rock";
        let jazz = "nuvola://onedrive-abcdefgh/Musica/Jazz";

        // Uguale a prima: niente.
        let toccate = specchio.applica(CONTO, vec![Cambio::Voce(file("1", "K", "01 Intro.flac"))]);
        assert_eq!(toccate, Toccate::default());

        // Dimensione, data, versione: la sua cartella.
        let cambi: [fn(&mut VoceRemota); 3] = [
            |v| v.dimensione = 2_000,
            |v| v.modificato_ms = 20,
            |v| v.versione = Some("v2".to_owned()),
        ];
        for cambia in cambi {
            let mut voce = specchio
                .conti
                .get(CONTO)
                .and_then(|c| c.voci.get("1"))
                .map(|n| n.voce.clone())
                .expect("voce");
            cambia(&mut voce);
            let toccate = specchio.applica(CONTO, vec![Cambio::Voce(voce)]);
            assert_eq!(toccate.cartelle.iter().collect::<Vec<_>>(), [rock]);
            assert_eq!(toccate.messe.len(), 1);
        }

        // Il nome: la sua cartella, una volta.
        let toccate = specchio.applica(CONTO, vec![Cambio::Voce(file("2", "K", "02 Altro.flac"))]);
        assert_eq!(toccate.cartelle.iter().collect::<Vec<_>>(), [rock]);
        assert_eq!(
            specchio.percorso(CONTO, "2"),
            Some(format!("{rock}/02 Altro.flac"))
        );
        assert!(specchio.voce(&format!("{rock}/02 Dopo.flac")).is_none());

        // Spostato in un'altra cartella: tutte e due.
        let toccate = specchio.applica(CONTO, vec![Cambio::Voce(file("2", "J", "02 Altro.flac"))]);
        assert_eq!(toccate.cartelle.iter().collect::<Vec<_>>(), [jazz, rock]);

        // Nuovo: la sua cartella. Tolto: la sua cartella.
        let toccate = specchio.applica(CONTO, vec![Cambio::Voce(file("6", "J", "nuovo.flac"))]);
        assert_eq!(toccate.cartelle.iter().collect::<Vec<_>>(), [jazz]);
        let toccate = specchio.applica(CONTO, vec![Cambio::Tolta { id: "6".to_owned() }]);
        assert_eq!(toccate.cartelle.iter().collect::<Vec<_>>(), [jazz]);
        assert_eq!(toccate.tolte, [(CONTO.to_owned(), "6".to_owned())]);
    }

    #[test]
    fn una_cartella_rinominata_porta_con_se_il_sottoalbero() {
        let mut specchio = Specchio::default();
        let mut voci = albero();
        voci.push(cartella("D", Some("K"), "Disco 2"));
        voci.push(file("7", "D", "a.flac"));
        specchio.metti(CONTO, voci);
        specchio.aggiungi_radice(radice("nuvola://onedrive-abcdefgh/Musica", "M"));
        specchio.chiudi_elenco(CONTO, "M", &insieme(&["K", "1", "2", "D", "7"]));

        let toccate = specchio.applica(
            CONTO,
            vec![Cambio::Voce(cartella("K", Some("M"), "Rock 70"))],
        );
        let vecchia = "nuvola://onedrive-abcdefgh/Musica/Rock";
        let nuova = "nuvola://onedrive-abcdefgh/Musica/Rock 70";
        assert_eq!(
            toccate.cartelle.iter().collect::<Vec<_>>(),
            [vecchia, nuova]
        );
        assert_eq!(
            specchio.percorso(CONTO, "7"),
            Some(format!("{nuova}/Disco 2/a.flac"))
        );
        assert_eq!(
            specchio
                .voce(&format!("{nuova}/Disco 2/a.flac"))
                .map(|(_, v)| v.id.as_str()),
            Some("7")
        );
        assert!(
            specchio
                .voce(&format!("{vecchia}/Disco 2/a.flac"))
                .is_none()
        );
        // La cartella vecchia, sotto una radice completa, è vuota e non
        // sconosciuta: è così che la scansione riconosce lo spostamento.
        assert_eq!(specchio.sotto(vecchia), Some(Vec::new()));
        // La radice è rimasta al suo posto: una sottocartella che cambia nome
        // non è un trasloco, e si raggiunge come prima.
        assert!(specchio.radici_traslocate(CONTO).is_empty());
        assert!(specchio.raggiungibile(vecchia) && specchio.raggiungibile(nuova));
        assert_eq!(
            percorsi(specchio.sotto(nuova)),
            [
                format!("{nuova}/01 Intro.flac"),
                format!("{nuova}/02 Dopo.flac"),
                format!("{nuova}/Disco 2"),
                format!("{nuova}/Disco 2/a.flac"),
            ]
        );
    }

    /// `Musica` scelta come radice ed elencata per intero.
    fn con_la_radice_musica() -> Specchio {
        let mut specchio = Specchio::default();
        specchio.metti(CONTO, albero());
        specchio.aggiungi_radice(radice("nuvola://onedrive-abcdefgh/Musica", "M"));
        specchio.chiudi_elenco(CONTO, "M", &insieme(&["K", "1", "2"]));
        specchio
    }

    #[test]
    fn una_radice_spostata_non_e_ne_completa_ne_vuota() {
        // La cartella scelta cambia nome sul servizio: il suo percorso di
        // prima non porta più a niente, e quel niente non è una cartella
        // svuotata. Prima della 3.2.0 finita era «completa e vuota», e la
        // scansione toglieva ogni brano della radice.
        let mut specchio = con_la_radice_musica();
        let vecchia = "nuvola://onedrive-abcdefgh/Musica";
        let rock = format!("{vecchia}/Rock");
        assert!(specchio.raggiungibile(vecchia) && specchio.raggiungibile(&rock));
        assert!(specchio.e_completa(&rock));

        specchio.applica(
            CONTO,
            vec![Cambio::Voce(cartella("M", Some("R"), "Musica 2"))],
        );
        assert_eq!(specchio.sotto(vecchia), None);
        assert_eq!(specchio.sotto(&rock), None);
        assert!(!specchio.e_completa(&rock));
        assert!(!specchio.raggiungibile(vecchia));
        assert!(!specchio.raggiungibile(&rock));
        // Il trasloco la riconosce, col percorso di adesso.
        let mut scelta = radice(vecchia, "M");
        scelta.completa = true;
        assert_eq!(
            specchio.radici_traslocate(CONTO),
            [(scelta, "nuvola://onedrive-abcdefgh/Musica 2".to_owned())]
        );
        // E rimessa a posto torna com'era.
        specchio.applica(
            CONTO,
            vec![Cambio::Voce(cartella("M", Some("R"), "Musica"))],
        );
        assert!(specchio.raggiungibile(&rock) && specchio.e_completa(&rock));
        assert!(specchio.radici_traslocate(CONTO).is_empty());
    }

    #[test]
    fn una_radice_cancellata_non_e_vuota() {
        let mut specchio = con_la_radice_musica();
        let vecchia = "nuvola://onedrive-abcdefgh/Musica";
        specchio.applica(CONTO, vec![Cambio::Tolta { id: "M".to_owned() }]);
        assert_eq!(specchio.sotto(vecchia), None);
        assert!(!specchio.e_completa(vecchia));
        assert!(!specchio.raggiungibile(vecchia));
        // Non è un trasloco: la cima non c'è più da nessuna parte.
        assert!(specchio.radici_traslocate(CONTO).is_empty());
    }

    #[test]
    fn una_cartella_cancellata_si_porta_via_quel_che_contiene() {
        let mut specchio = Specchio::default();
        specchio.metti(CONTO, albero());
        let toccate = specchio.applica(CONTO, vec![Cambio::Tolta { id: "K".to_owned() }]);
        let mut tolte: Vec<&str> = toccate.tolte.iter().map(|(_, id)| id.as_str()).collect();
        tolte.sort_unstable();
        assert_eq!(tolte, ["1", "2", "K"]);
        assert!(
            toccate
                .cartelle
                .contains("nuvola://onedrive-abcdefgh/Musica/Rock")
        );
        assert_eq!(specchio.percorso(CONTO, "1"), None);
        // E una cancellazione di qualcosa che non c'è non fa niente.
        let niente = specchio.applica(
            CONTO,
            vec![Cambio::Tolta {
                id: "ZZ".to_owned(),
            }],
        );
        assert_eq!(niente, Toccate::default());
    }

    #[test]
    fn una_radice_sconosciuta_non_ha_niente_sotto() {
        let mut specchio = Specchio::default();
        assert_eq!(specchio.sotto("nuvola://onedrive-abcdefgh/Musica"), None);
        specchio.metti(CONTO, albero());
        // Nota, ma non elencata per intero: quel che manca non prova niente.
        assert_eq!(specchio.sotto("nuvola://onedrive-abcdefgh/Altro"), None);
        assert_eq!(specchio.sotto("nuvola://dropbox-abcdefgh/Musica"), None);
        // Un file non è una radice.
        assert_eq!(
            specchio.sotto("nuvola://onedrive-abcdefgh/Musica/Rock/01 Intro.flac"),
            None
        );
        let sotto = specchio
            .sotto("nuvola://onedrive-abcdefgh/Musica")
            .expect("nota");
        assert_eq!(sotto.len(), 3, "Rock e i suoi due brani: {sotto:?}");
        let brano = sotto
            .iter()
            .find(|voce| voce.percorso.ends_with("01 Intro.flac"))
            .expect("il brano");
        assert_eq!(
            (brano.cartella, brano.dimensione, brano.modificato_ms),
            (false, 1_000, 10)
        );
    }

    #[test]
    fn dimenticare_un_conto_non_fa_riguardare_niente() {
        let mut specchio = Specchio::default();
        specchio.metti(CONTO, albero());
        specchio.metti("dropbox-abcdefgh", vec![cartella("r", None, "")]);
        specchio.aggiungi_radice(radice("nuvola://onedrive-abcdefgh/Musica", "M"));
        let toccate = specchio.dimentica(CONTO, false);
        assert_eq!(toccate.tolte.len(), 5);
        assert!(toccate.cartelle.is_empty());
        assert!(specchio.radici(CONTO).is_empty());
        assert!(specchio.voce("nuvola://onedrive-abcdefgh/Musica").is_none());
    }

    /// Lo stato intero, in una forma che si confronta.
    fn fotografia(specchio: &Specchio) -> Vec<(String, Nodo)> {
        let mut tutto: Vec<(String, Nodo)> = specchio
            .conti
            .iter()
            .flat_map(|(conto, del_conto)| {
                del_conto
                    .voci
                    .values()
                    .map(move |nodo| (conto.clone(), nodo.clone()))
            })
            .collect();
        tutto.sort_by(|a, b| (&a.0, &a.1.voce.id).cmp(&(&b.0, &b.1.voce.id)));
        tutto
    }

    #[test]
    fn ricaricato_dopo_il_salvataggio_e_lo_stesso_specchio() {
        let connection = crate::db::open_in_memory().expect("database").connection;
        let mut specchio = Specchio::default();
        let mut voci = albero();
        voci.push(file("AAAAAA111111", "K", "01 Intro.flac"));
        let mut con_tag = file("3", "K", "03 Tag.flac");
        con_tag.tag = Some(TagNoti {
            titolo: Some("Titolo".to_owned()),
            durata_ms: Some(180_000),
            traccia: Some(3),
            ..TagNoti::default()
        });
        voci.push(con_tag);
        let prima = specchio.metti(CONTO, voci);
        Specchio::salva(&connection, &prima).expect("salvato");
        let tolte = specchio.applica(CONTO, vec![Cambio::Tolta { id: "2".to_owned() }]);
        Specchio::salva(&connection, &tolte).expect("salvato");
        let mut la_radice = radice("nuvola://onedrive-abcdefgh/Musica", "M");
        la_radice.completa = true;
        specchio.aggiungi_radice(la_radice.clone());
        salva_radice(&connection, &la_radice).expect("radice");

        let ricaricato = Specchio::carica(&connection).expect("caricato");
        assert_eq!(fotografia(&ricaricato), fotografia(&specchio));
        assert_eq!(ricaricato.radici(CONTO), [la_radice]);
        assert_eq!(ricaricato.indice, specchio.indice);
        // Il segmento si è riletto, non ricalcolato: un terzo «01 Intro»
        // trova presi tutti e due i nomi.
        let mut ricaricato = ricaricato;
        ricaricato.metti(CONTO, vec![file("CCCCCC333333", "K", "01 intro.flac")]);
        assert_eq!(
            ricaricato.percorso(CONTO, "1").as_deref(),
            Some("nuvola://onedrive-abcdefgh/Musica/Rock/01 Intro.flac")
        );
        if regole().case_insensitive {
            assert_eq!(
                ricaricato.percorso(CONTO, "CCCCCC333333").as_deref(),
                Some("nuvola://onedrive-abcdefgh/Musica/Rock/01 intro~333333.flac")
            );
        }

        // E togliere la radice dal database la toglie dal ricaricato.
        togli_radice(&connection, "nuvola://onedrive-abcdefgh/Musica").expect("via");
        assert!(
            Specchio::carica(&connection)
                .expect("caricato")
                .radici(CONTO)
                .is_empty()
        );
    }

    /// L'albero salvato, i cambiamenti applicati e salvati, e lo specchio
    /// ricaricato dal database.
    fn dopo_i_cambiamenti(cambi: Vec<Cambio>) -> (Specchio, Specchio) {
        let connection = crate::db::open_in_memory().expect("database").connection;
        let mut specchio = Specchio::default();
        let prima = specchio.metti(CONTO, albero());
        Specchio::salva(&connection, &prima).expect("salvato");
        let toccate = specchio.applica(CONTO, cambi);
        Specchio::salva(&connection, &toccate).expect("salvato");
        let ricaricato = Specchio::carica(&connection).expect("caricato");
        (specchio, ricaricato)
    }

    #[test]
    fn tolta_e_rimessa_nello_stesso_giro_resta_nel_database() {
        // Un file sostituito: la cancellazione e poi la voce nuova, con lo
        // stesso identificativo. Prima le voci si scrivevano tutte e le
        // cancellazioni dopo, e la riga se ne andava: al riavvio il brano
        // mancava, e la scansione lo toglieva con ascolti e voti.
        let mut nuovo = file("1", "K", "01 Intro.flac");
        nuovo.dimensione = 2_000;
        let (specchio, ricaricato) = dopo_i_cambiamenti(vec![
            Cambio::Tolta { id: "1".to_owned() },
            Cambio::Voce(nuovo),
        ]);
        assert_eq!(fotografia(&ricaricato), fotografia(&specchio));
        assert_eq!(
            ricaricato.percorso(CONTO, "1").as_deref(),
            Some("nuvola://onedrive-abcdefgh/Musica/Rock/01 Intro.flac")
        );
    }

    #[test]
    fn un_figlio_portato_fuori_da_una_cartella_tolta_resta() {
        // La cartella cancellata prima, il figlio spostato altrove dopo: nel
        // flusso di Graph l'ordine è questo.
        let (specchio, ricaricato) = dopo_i_cambiamenti(vec![
            Cambio::Tolta { id: "K".to_owned() },
            Cambio::Voce(file("2", "M", "02 Dopo.flac")),
        ]);
        assert_eq!(fotografia(&ricaricato), fotografia(&specchio));
        assert_eq!(
            ricaricato.percorso(CONTO, "2").as_deref(),
            Some("nuvola://onedrive-abcdefgh/Musica/02 Dopo.flac")
        );
        assert_eq!(ricaricato.percorso(CONTO, "1"), None);
        assert_eq!(ricaricato.percorso(CONTO, "K"), None);
    }

    #[test]
    fn messa_e_poi_tolta_nello_stesso_giro_se_ne_va() {
        let (specchio, ricaricato) = dopo_i_cambiamenti(vec![
            Cambio::Voce(file("6", "K", "06 Breve.flac")),
            Cambio::Tolta { id: "6".to_owned() },
        ]);
        assert_eq!(fotografia(&ricaricato), fotografia(&specchio));
        assert_eq!(ricaricato.percorso(CONTO, "6"), None);
    }

    #[test]
    fn salvare_dentro_una_transazione_altrui_si_appoggia_a_quella() {
        let mut connection = crate::db::open_in_memory().expect("database").connection;
        let mut specchio = Specchio::default();
        let toccate = specchio.metti(CONTO, albero());
        let tx = connection.transaction().expect("transazione");
        Specchio::salva(&tx, &toccate).expect("salvato dentro");
        tx.rollback().expect("disfatta");
        // Chi aveva la transazione l'ha disfatta: non resta niente.
        let righe: i64 = connection
            .query_row("SELECT COUNT(*) FROM nuvola_voci", [], |r| r.get(0))
            .expect("conto");
        assert_eq!(righe, 0);
    }

    #[test]
    fn i_conti_si_salvano_e_si_tolgono_con_tutto_il_loro() {
        let connection = crate::db::open_in_memory().expect("database").connection;
        let mut conto = ContoSalvato {
            conto: CONTO.to_owned(),
            servizio: Servizio::OneDrive,
            nome: "mario@outlook.com".to_owned(),
            dettagli: Some(r#"{"drive":"x"}"#.to_owned()),
            cursore: None,
            ultimo_ms: None,
            app: Some("microsoft-abcdefgh".to_owned()),
            client: Some("client-1".to_owned()),
        };
        salva_conto(&connection, &conto).expect("salvato");
        conto.cursore = Some("delta-1".to_owned());
        conto.ultimo_ms = Some(42);
        salva_conto(&connection, &conto).expect("riscritto");
        // Un servizio che questa versione non conosce si salta.
        connection
            .execute(
                "INSERT INTO nuvola_conti (conto, servizio, nome) VALUES ('ftp-abcdefgh', 'ftp', 'x')",
                [],
            )
            .expect("conto del futuro");
        assert_eq!(conti(&connection).expect("conti"), [conto]);

        let mut specchio = Specchio::default();
        let toccate = specchio.metti(CONTO, albero());
        Specchio::salva(&connection, &toccate).expect("voci");
        salva_radice(
            &connection,
            &radice("nuvola://onedrive-abcdefgh/Musica", "M"),
        )
        .expect("radice");
        togli_conto(&connection, CONTO, false).expect("tolto");
        for tabella in ["nuvola_voci", "nuvola_radici"] {
            let righe: i64 = connection
                .query_row(&format!("SELECT COUNT(*) FROM {tabella}"), [], |r| r.get(0))
                .expect("conto");
            assert_eq!(righe, 0, "{tabella}");
        }
        assert!(conti(&connection).expect("conti").is_empty());
    }

    #[test]
    fn riscrivere_un_conto_non_disfa_il_suo_legame() {
        // Il filo del conto ha letto la riga prima che l'app cambiasse, e la
        // riscrive alla fine dell'elenco: il legame nuovo deve restare.
        let connection = crate::db::open_in_memory().expect("database").connection;
        let mut letto = ContoSalvato {
            conto: CONTO.to_owned(),
            servizio: Servizio::OneDrive,
            nome: "mario@outlook.com".to_owned(),
            dettagli: None,
            cursore: None,
            ultimo_ms: None,
            app: Some("microsoft-aaaaaaaa".to_owned()),
            client: Some("vecchio".to_owned()),
        };
        salva_conto(&connection, &letto).expect("nato");
        lega_conto(
            &connection,
            CONTO,
            Some("microsoft-bbbbbbbb"),
            Some("nuovo"),
        )
        .expect("legato");
        letto.cursore = Some("delta-9".to_owned());
        salva_conto(&connection, &letto).expect("riscritto dal filo");
        let dopo = conti(&connection).expect("conti").remove(0);
        assert_eq!(dopo.cursore.as_deref(), Some("delta-9"));
        assert_eq!(dopo.app.as_deref(), Some("microsoft-bbbbbbbb"));
        assert_eq!(dopo.client.as_deref(), Some("nuovo"));
        lega_conto(&connection, CONTO, None, None).expect("slegato");
        let slegato = conti(&connection).expect("conti").remove(0);
        assert_eq!((slegato.app, slegato.client), (None, None));
    }

    fn un_conto() -> ContoSalvato {
        ContoSalvato {
            conto: CONTO.to_owned(),
            servizio: Servizio::OneDrive,
            nome: "mario@outlook.com".to_owned(),
            dettagli: None,
            cursore: None,
            ultimo_ms: None,
            app: None,
            client: None,
        }
    }

    #[test]
    fn un_cursore_dopo_lo_scollegamento_non_fa_rinascere_il_conto() {
        // Il filo del conto torna dal longpoll di Dropbox — che non chiede
        // l'accesso — dopo che il conto è stato scollegato: il suo cursore non
        // deve rimettere al mondo la riga.
        let connection = crate::db::open_in_memory().expect("database").connection;
        salva_conto(&connection, &un_conto()).expect("nato");
        assert!(
            salva_cursore(&connection, CONTO, Some("delta-2"), Some(7)).expect("scritto"),
            "un conto che c'è prende il cursore"
        );
        let letto = conti(&connection).expect("conti").remove(0);
        assert_eq!(
            (letto.cursore.as_deref(), letto.ultimo_ms),
            (Some("delta-2"), Some(7))
        );

        togli_conto(&connection, CONTO, false).expect("tolto");
        assert!(!salva_cursore(&connection, CONTO, Some("delta-3"), Some(8)).expect("niente"));
        assert!(conti(&connection).expect("conti").is_empty());
    }

    #[test]
    fn rileggere_un_conto_lo_riporta_al_database() {
        // Una scrittura caduta lascia la memoria avanti al database: riletto il
        // conto, gli stessi cambiamenti tornano a essere cambiamenti.
        let connection = crate::db::open_in_memory().expect("database").connection;
        let mut specchio = Specchio::default();
        let toccate = specchio.metti(CONTO, albero());
        Specchio::salva(&connection, &toccate).expect("salvato");
        let toccate = specchio.metti("dropbox-abcdefgh", vec![cartella("r", None, "")]);
        Specchio::salva(&connection, &toccate).expect("salvato");
        let mut la_radice = radice("nuvola://onedrive-abcdefgh/Musica", "M");
        la_radice.completa = true;
        salva_radice(&connection, &la_radice).expect("radice");
        specchio.aggiungi_radice(la_radice);
        let atteso = Specchio::carica(&connection).expect("caricato");

        let cambi = || {
            vec![
                Cambio::Voce(file("6", "K", "06 Breve.flac")),
                Cambio::Tolta { id: "2".to_owned() },
            ]
        };
        // Applicati e mai salvati: la scrittura è caduta.
        let _ = specchio.applica(CONTO, cambi());
        assert!(
            specchio.applica(CONTO, cambi()).messe.is_empty(),
            "niente da scrivere"
        );

        specchio.rimpiazza_conto(CONTO, leggi_conto(&connection, CONTO).expect("letto"));
        assert_eq!(fotografia(&specchio), fotografia(&atteso));
        assert_eq!(specchio.indice, atteso.indice);
        assert_eq!(specchio.radici(CONTO), atteso.radici(CONTO));
        let di_nuovo = specchio.applica(CONTO, cambi());
        assert_eq!(
            (di_nuovo.messe.len(), di_nuovo.tolte.len()),
            (1, 1),
            "{di_nuovo:?}"
        );
    }

    #[test]
    fn riaprire_le_radici_le_rende_incomplete() {
        let connection = crate::db::open_in_memory().expect("database").connection;
        let mut specchio = con_la_radice_musica();
        let mut altra = radice("nuvola://dropbox-abcdefgh/Musica", "/musica");
        altra.conto = "dropbox-abcdefgh".to_owned();
        altra.completa = true;
        specchio.aggiungi_radice(altra.clone());
        assert!(specchio.e_completa("nuvola://onedrive-abcdefgh/Musica/Rock"));

        let riaperte = specchio.riapri_radici(CONTO);
        assert_eq!(riaperte, [radice("nuvola://onedrive-abcdefgh/Musica", "M")]);
        assert!(!specchio.e_completa("nuvola://onedrive-abcdefgh/Musica/Rock"));
        // L'altro conto non si tocca.
        assert_eq!(specchio.radici("dropbox-abcdefgh"), [altra]);
        // Salvate così, si ricaricano incomplete.
        for radice in &riaperte {
            salva_radice(&connection, radice).expect("salvata");
        }
        assert_eq!(
            Specchio::carica(&connection)
                .expect("caricato")
                .radici(CONTO),
            riaperte
        );
    }

    #[test]
    fn scollegare_tenendo_le_cartelle_tiene_le_radici_da_rielencare() {
        // «Tieni le cartelle finché non lo ricolleghi»: la radice resta, e al
        // ricollegamento si rielenca. Prima se ne andava, e le cartelle
        // restavano spente per sempre.
        let connection = crate::db::open_in_memory().expect("database").connection;
        salva_conto(&connection, &un_conto()).expect("conto");
        let mut specchio = Specchio::default();
        let toccate = specchio.metti(CONTO, albero());
        Specchio::salva(&connection, &toccate).expect("voci");
        specchio.aggiungi_radice(radice("nuvola://onedrive-abcdefgh/Musica", "M"));
        specchio.chiudi_elenco(CONTO, "M", &insieme(&["K", "1", "2"]));
        for radice in specchio.radici(CONTO) {
            salva_radice(&connection, &radice).expect("radice");
        }

        let tolte = specchio.dimentica(CONTO, true);
        assert_eq!(tolte.tolte.len(), 5);
        togli_conto(&connection, CONTO, true).expect("tolto");
        let attesa = [radice("nuvola://onedrive-abcdefgh/Musica", "M")];
        assert_eq!(specchio.radici(CONTO), attesa);
        let ricaricato = Specchio::carica(&connection).expect("caricato");
        assert_eq!(ricaricato.radici(CONTO), attesa);
        assert!(fotografia(&ricaricato).is_empty());
        assert!(conti(&connection).expect("conti").is_empty());
    }

    #[test]
    fn fuori_dalla_nuvola_tiene_fuori_solo_il_cloud() {
        let connection = crate::db::open_in_memory().expect("database").connection;
        connection
            .execute_batch(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                     date_added, date_modified)
                 VALUES (1, 'C:\\M\\a.flac', 'k1', 'A', 'B', 'C', 1, 1, 1),
                        (2, 'nuvola://onedrive-abcdefgh/a.flac', 'k2', 'A', 'B', 'C', 1, 1, 1),
                        (3, 'nuvola:/0', 'k3', 'A', 'B', 'C', 1, 1, 1),
                        (4, 'NUVOLA://onedrive-abcdefgh/b.flac', 'k4', 'A', 'B', 'C', 1, 1, 1),
                        (5, '\\\\nas\\nuvola\\c.flac', 'k5', 'A', 'B', 'C', 1, 1, 1);",
            )
            .expect("righe");
        let mut statement = connection
            .prepare(&format!(
                "SELECT t.id FROM tracks t WHERE {FUORI_DALLA_NUVOLA} ORDER BY t.id"
            ))
            .expect("query");
        let ids: Vec<i64> = statement
            .query_map([], |r| r.get(0))
            .expect("righe")
            .collect::<Result<_, _>>()
            .expect("ids");
        // Il `nuvola://` in maiuscolo non lo scrive nessuno — `e_nuvola`
        // confronta esatto — e quindi non è cloud nemmeno qui.
        assert_eq!(ids, [1, 3, 4, 5]);
    }
}
