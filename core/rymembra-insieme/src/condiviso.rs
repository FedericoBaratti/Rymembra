//! Lo stato che i fili della rete condividono.
//!
//! # I lucchetti, e in che ordine
//!
//! Ogni campo ha il suo, e nessuno si tiene mentre si fa rete o disco: un
//! lucchetto preso intorno a una `write` su un socket lento fermerebbe tutti
//! gli altri fili dietro a un PC che non legge.
//!
//! L'ordine, quando se ne prendono due: [`Condiviso::in_apertura`] prima di
//! [`Condiviso::connessioni`] (solo `Rete::chiudi`, e il perché è scritto
//! lì); [`Condiviso::ospitata`] prima di [`Condiviso::connessioni`] (solo
//! `Rete::ospita`); l'associazione di un `Collegamento` prima di
//! [`Condiviso::tentativi`] e prima di quello della fiducia (chi conferma
//! l'ultimo codice fida la chiave prima di lasciare lo stato del codice: vedi
//! `collegamento::riuscita`); [`Condiviso::connessioni`] prima dei lucchetti
//! di un `Collegamento` e di quello della fiducia (le scelte di
//! [`Condiviso::registra_entrante`], che guardano le connessioni mentre
//! decidono); la coda d'uscita di un `Collegamento` prima della sua chiusura.
//! Nessun altro ne tiene due. Chi deve fare qualcosa
//! su più connessioni copia prima l'elenco e lo lascia andare
//! ([`Condiviso::connessioni_dove`]).

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use rymembra_domain::errors::ErrorCodeKind;

use crate::codice::Tentativi;
use crate::collegamento::Collegamento;
use crate::errori::chiave;
use crate::identita::{ChiavePubblica, Identita};
use crate::messaggi::MASSIMO_NOME;
use crate::rete::{Evento, IdConnessione};
use crate::scoperta::Cercatore;
use crate::servente::Servente;
use crate::tempi::Tempi;
use crate::trasporto::Uso;

/// Quanto dorme un filo che aspetta qualcosa senza un modo per farsi
/// svegliare: l'accettazione non bloccante, i socket della scoperta.
///
/// Cinquanta millisecondi: è il ritardo con cui si accorgono di dover uscire,
/// e quello che aggiungono a un'accettazione. Su gesti umani non si sente, e
/// venti risvegli al secondo di un filo che non fa niente non si vedono.
pub(crate) const RESPIRO: Duration = Duration::from_millis(50);

/// Quante connessioni di controllo non fidate, in tutto. Vedi
/// [`Condiviso::registra_entrante`].
///
/// Otto: una sessione ha al più otto membri, e i PC che si affacciano per
/// sapere chi ospita prima di associarsi sono, in una casa o in un ufficio,
/// pochi. Chi ne apre di più non è uno di loro.
pub(crate) const ESTRANEI: usize = 8;

/// Quante connessioni di lettura per chiave fidata: le due del vivaio di
/// `cliente`.
pub(crate) const LETTURE_PER_CHIAVE: usize = 2;

/// Chi è fidato e chi è bloccato, come l'ha detto il desktop l'ultima volta.
#[derive(Debug, Default)]
struct Fiducia {
    fidati: HashSet<ChiavePubblica>,
    bloccati: HashSet<ChiavePubblica>,
}

/// L'ascolto di chi ospita.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ospitata {
    /// La porta, TCP e UDP.
    pub(crate) porta: u16,
}

/// Lo stato condiviso di una `Rete`.
pub(crate) struct Condiviso {
    /// La chiave di questo PC.
    pub(crate) identita: Identita,
    /// Il nome con cui si presenta.
    nome: Mutex<String>,
    /// L'indirizzo da legare.
    pub(crate) lega: IpAddr,
    /// La prima porta da provare per ospitare.
    pub(crate) porta: u16,
    /// L'orologio di processo.
    pub(crate) adesso_ns: fn() -> u64,
    /// I tempi del protocollo.
    pub(crate) tempi: Tempi,
    /// Dove vanno gli eventi.
    eventi: Sender<Evento>,
    /// `Rete::ferma` è stata chiamata: non si apre più niente.
    fermo: AtomicBool,
    /// Il prossimo numero di connessione.
    prossimo_id: AtomicU64,
    /// Il prossimo numero di lettura, per le letture che questo PC chiede.
    pub(crate) prossima_lettura: AtomicU32,
    /// Chi è fidato e chi no.
    fiducia: Mutex<Fiducia>,
    /// Le connessioni aperte, stretta e saluto fatti.
    pub(crate) connessioni: Mutex<HashMap<IdConnessione, Arc<Collegamento>>>,
    /// Le connessioni di `Rete::collega` non ancora aperte, col perché se il
    /// desktop le ha chiuse prima che si aprissero.
    pub(crate) in_apertura: Mutex<HashMap<IdConnessione, Option<Option<ErrorCodeKind>>>>,
    /// I gettoni che questo PC serve.
    pub(crate) servente: Mutex<Servente>,
    /// L'ascolto, se si ospita.
    pub(crate) ospitata: Mutex<Option<Ospitata>>,
    /// Cresce a ogni `ospita` e a ogni `smetti_di_ospitare`: i fili
    /// dell'ascolto escono quando non è più la loro, e una connessione in
    /// entrata accettata sotto un ascolto chiuso non si registra.
    pub(crate) generazione: AtomicU64,
    /// Il socket della scoperta, se si sta cercando.
    pub(crate) cercatore: Mutex<Option<Arc<Cercatore>>>,
    /// Le associazioni fallite.
    pub(crate) tentativi: Mutex<Tentativi>,
    /// Le connessioni in entrata, per indirizzo: i tetti dell'ascolto.
    pub(crate) posti: Mutex<HashMap<IpAddr, usize>>,
    /// Le strette in entrata in corso, per indirizzo. Vedi `ascolto`.
    pub(crate) strette: Mutex<HashMap<IpAddr, usize>>,
    /// Suona quando una stretta finisce e libera un posto.
    pub(crate) strette_libere: Condvar,
    /// Le chiavi che possono leggere i gettoni di questo PC: i membri della
    /// sessione. Vedi `Rete::ammessi`.
    ammessi: Mutex<HashSet<ChiavePubblica>>,
    /// Le connessioni di servizio offerte (fase 2), per chiave dell'host.
    pub(crate) offerte: Mutex<HashSet<ChiavePubblica>>,
    /// I fili vivi.
    fili: AtomicUsize,
}

impl std::fmt::Debug for Condiviso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Condiviso")
            .field("fermo", &self.fermo())
            .field("fili", &self.fili())
            .finish_non_exhaustive()
    }
}

impl Condiviso {
    /// Lo stato di una rete appena partita.
    pub(crate) fn nuovo(
        identita: Identita,
        nome: &str,
        lega: IpAddr,
        porta: u16,
        adesso_ns: fn() -> u64,
        tempi: Tempi,
        eventi: Sender<Evento>,
    ) -> Self {
        Self {
            identita,
            nome: Mutex::new(pulisci_nome(nome)),
            lega,
            porta,
            adesso_ns,
            tempi,
            eventi,
            fermo: AtomicBool::new(false),
            prossimo_id: AtomicU64::new(1),
            prossima_lettura: AtomicU32::new(1),
            fiducia: Mutex::new(Fiducia::default()),
            connessioni: Mutex::new(HashMap::new()),
            in_apertura: Mutex::new(HashMap::new()),
            servente: Mutex::new(Servente::default()),
            ospitata: Mutex::new(None),
            generazione: AtomicU64::new(0),
            cercatore: Mutex::new(None),
            tentativi: Mutex::new(Tentativi::default()),
            posti: Mutex::new(HashMap::new()),
            strette: Mutex::new(HashMap::new()),
            strette_libere: Condvar::new(),
            ammessi: Mutex::new(HashSet::new()),
            offerte: Mutex::new(HashSet::new()),
            fili: AtomicUsize::new(0),
        }
    }

    /// `Rete::ferma` è stata chiamata.
    pub(crate) fn fermo(&self) -> bool {
        self.fermo.load(Ordering::SeqCst)
    }

    /// Segna la rete come fermata. Vedi [`Condiviso::registra`] per perché si
    /// fa sotto il lucchetto delle connessioni.
    pub(crate) fn ferma(&self) {
        let _connessioni = chiave(&self.connessioni);
        self.fermo.store(true, Ordering::SeqCst);
    }

    /// Un evento al desktop. Se il desktop ha smesso di ascoltare non è un
    /// guasto: si sta uscendo.
    pub(crate) fn emetti(&self, evento: Evento) {
        let _ = self.eventi.send(evento);
    }

    /// Un numero di connessione mai usato.
    pub(crate) fn nuovo_id(&self) -> IdConnessione {
        IdConnessione(self.prossimo_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Il nome di questo PC, già ripulito.
    pub(crate) fn nome(&self) -> String {
        chiave(&self.nome).clone()
    }

    /// Cambia il nome.
    pub(crate) fn rinomina(&self, nome: &str) {
        *chiave(&self.nome) = pulisci_nome(nome);
    }

    /// Questo PC sta ospitando.
    pub(crate) fn ospita_ora(&self) -> bool {
        chiave(&self.ospitata).is_some()
    }

    /// La chiave è fra le fidate e non fra le bloccate.
    pub(crate) fn fidato(&self, pari: &ChiavePubblica) -> bool {
        let fiducia = chiave(&self.fiducia);
        fiducia.fidati.contains(pari) && !fiducia.bloccati.contains(pari)
    }

    /// La chiave è bloccata.
    pub(crate) fn bloccato(&self, pari: &ChiavePubblica) -> bool {
        chiave(&self.fiducia).bloccati.contains(pari)
    }

    /// Sostituisce gli elenchi.
    pub(crate) fn imposta_fiducia(
        &self,
        fidati: Vec<ChiavePubblica>,
        bloccati: Vec<ChiavePubblica>,
    ) {
        let mut fiducia = chiave(&self.fiducia);
        fiducia.fidati = fidati.into_iter().collect();
        fiducia.bloccati = bloccati.into_iter().collect();
    }

    /// La chiave può leggere i gettoni di questo PC. Vedi `Rete::ammessi`.
    pub(crate) fn ammesso(&self, pari: &ChiavePubblica) -> bool {
        chiave(&self.ammessi).contains(pari)
    }

    /// Sostituisce l'elenco di chi può leggere.
    pub(crate) fn imposta_ammessi(&self, chiavi: Vec<ChiavePubblica>) {
        *chiave(&self.ammessi) = chiavi.into_iter().collect();
    }

    /// Una chiave appena associata: fidata da subito, fino al prossimo
    /// `Rete::pari`. Vedi `Evento::Associato`.
    pub(crate) fn fida(&self, pari: ChiavePubblica) {
        let mut fiducia = chiave(&self.fiducia);
        fiducia.bloccati.remove(&pari);
        fiducia.fidati.insert(pari);
    }

    /// La connessione con quel numero, se è aperta.
    pub(crate) fn connessione(&self, id: IdConnessione) -> Option<Arc<Collegamento>> {
        chiave(&self.connessioni).get(&id).cloned()
    }

    /// Tutte le connessioni aperte che rispondono a un criterio, copiate fuori
    /// dal lucchetto.
    pub(crate) fn connessioni_dove(
        &self,
        criterio: impl Fn(&Collegamento) -> bool,
    ) -> Vec<Arc<Collegamento>> {
        chiave(&self.connessioni)
            .values()
            .filter(|c| criterio(c))
            .cloned()
            .collect()
    }

    /// C'è una connessione di controllo aperta da questo PC verso quella
    /// chiave. Serve alla connessione di servizio, che vive quanto lei.
    pub(crate) fn controllo_verso(&self, pari: &ChiavePubblica) -> bool {
        chiave(&self.connessioni)
            .values()
            .any(|c| c.uso == Uso::Controllo && !c.in_entrata && c.pari == *pari && !c.chiusa())
    }

    /// Registra una connessione appena aperta.
    ///
    /// `false` se la rete è stata fermata, o se è una connessione in entrata
    /// accettata sotto un ascolto che nel frattempo è stato chiuso: in tutti e
    /// due i casi chi la stava aprendo la deve chiudere.
    ///
    /// Il controllo si fa sotto il lucchetto delle connessioni, lo stesso che
    /// [`Condiviso::ferma`] e `smetti_di_ospitare` prendono per cambiare stato:
    /// così una connessione o la vedono loro nell'elenco, o lei vede loro.
    /// Senza, una connessione registrata un istante dopo il giro di chiusura
    /// resterebbe aperta su una rete fermata.
    pub(crate) fn registra(
        &self,
        collegamento: Arc<Collegamento>,
        generazione: Option<u64>,
    ) -> bool {
        let mut connessioni = chiave(&self.connessioni);
        if self.fermo() {
            return false;
        }
        if let Some(generazione) = generazione
            && self.generazione.load(Ordering::SeqCst) != generazione
        {
            return false;
        }
        connessioni.insert(collegamento.id, collegamento);
        true
    }

    /// Registra una connessione in entrata, coi tetti di chi ospita. Restituisce
    /// le connessioni che la nuova manda via; il chiamante le chiude, fuori
    /// dal lucchetto.
    ///
    /// # Errori
    ///
    /// Il perché del rifiuto, da dire all'altro capo al posto del saluto:
    /// `None` se la rete è ferma o l'ascolto è cambiato.
    ///
    /// # I tetti
    ///
    /// - Una chiave **non fidata** entra solo per il controllo (sapere chi
    ///   ospita, associarsi): una connessione di lettura o di servizio non le
    ///   serve a niente, e si rifiuta con `insieme.nonAssociato`.
    /// - Al più [`ESTRANEI`] connessioni non fidate in tutto, e una per
    ///   indirizzo: una nuova dallo stesso indirizzo manda via la vecchia, se
    ///   la vecchia non si sta associando (è lo stesso PC che si è
    ///   ricollegato), altrimenti si rifiuta. Oltre il tetto totale si
    ///   rifiuta con `insieme.irraggiungibile`: per chi bussa, adesso questo
    ///   host non si raggiunge, e riproverà come con un host spento. Il tetto
    ///   per indirizzo non vale su `127.0.0.1`, per la stessa ragione di quello
    ///   delle connessioni (vedi `ascolto`).
    /// - Una chiave **fidata** tiene al più [`LETTURE_PER_CHIAVE`] connessioni
    ///   di lettura e una di servizio: una in più ne manda via una dello
    ///   stesso tipo. Mandarla via e non rifiutare la nuova perché il caso
    ///   onesto è un membro che ha appena buttato una connessione (un annullo)
    ///   e ne apre un'altra prima che la vecchia sia sparita da qui.
    ///
    ///   Quale: prima una **ferma** (vedi `Collegamento::al_lavoro`), e fra
    ///   le ferme quella senza letture da più tempo; una che sta servendo
    ///   solo se lo fanno tutte. Fino alla revisione del 26 ottobre andava
    ///   via la più vecchia per apertura, anche se stava mandando i due mega
    ///   del brano che suona mentre l'altra era lì senza far niente.
    pub(crate) fn registra_entrante(
        &self,
        nuova: Arc<Collegamento>,
        generazione: u64,
    ) -> Result<Vec<Arc<Collegamento>>, Option<ErrorCodeKind>> {
        let mut connessioni = chiave(&self.connessioni);
        if self.fermo() || self.generazione.load(Ordering::SeqCst) != generazione {
            return Err(None);
        }
        let stesse = |c: &&Arc<Collegamento>| c.in_entrata && c.uso == nuova.uso && !c.chiusa();
        let mut via: Vec<Arc<Collegamento>> = Vec::new();
        if self.fidato(&nuova.pari) {
            let tetto = match nuova.uso {
                Uso::Lettura => LETTURE_PER_CHIAVE,
                Uso::Servizio => 1,
                // Il controllo resta coi tetti dell'ascolto: un membro ne ha una.
                Uso::Controllo => usize::MAX,
            };
            let mut sue: Vec<&Arc<Collegamento>> = connessioni
                .values()
                .filter(stesse)
                .filter(|c| c.pari == nuova.pari)
                .collect();
            sue.sort_by_cached_key(|c| (c.al_lavoro(), c.ultimo_lavoro(), c.aperta));
            let troppe = (sue.len().saturating_add(1)).saturating_sub(tetto);
            via.extend(sue.into_iter().take(troppe).cloned());
        } else {
            if nuova.uso != Uso::Controllo {
                return Err(Some(ErrorCodeKind::InsiemeNonAssociato));
            }
            let estranei: Vec<&Arc<Collegamento>> = connessioni
                .values()
                .filter(stesse)
                .filter(|c| !self.fidato(&c.pari))
                .collect();
            if !nuova.ip.is_loopback()
                && let Some(vecchia) = estranei.iter().find(|c| c.ip == nuova.ip)
            {
                if vecchia.associando() {
                    return Err(Some(ErrorCodeKind::InsiemeIrraggiungibile));
                }
                via.push(Arc::clone(vecchia));
            }
            if estranei.len().saturating_sub(via.len()) >= ESTRANEI {
                return Err(Some(ErrorCodeKind::InsiemeIrraggiungibile));
            }
        }
        connessioni.insert(nuova.id, nuova);
        Ok(via)
    }

    /// Toglie una connessione chiusa.
    pub(crate) fn togli(&self, id: IdConnessione) {
        chiave(&self.connessioni).remove(&id);
    }

    /// Cambia la generazione dell'ascolto sotto il lucchetto delle
    /// connessioni. Vedi [`Condiviso::registra`].
    pub(crate) fn nuova_generazione(&self) -> u64 {
        let _connessioni = chiave(&self.connessioni);
        self.generazione
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1)
    }

    /// Lancia un filo contato. `false` se il sistema non ne dà.
    ///
    /// Il conto serve a `Rete::fili_in_vita`: è il modo di sapere, uscendo,
    /// che `ferma` ha davvero fatto uscire tutti.
    pub(crate) fn filo(
        self: &Arc<Self>,
        nome: &str,
        corpo: impl FnOnce(Arc<Self>) + Send + 'static,
    ) -> bool {
        self.fili.fetch_add(1, Ordering::SeqCst);
        let rete = Arc::clone(self);
        let partito = std::thread::Builder::new()
            .name(format!("insieme-{nome}"))
            .spawn(move || {
                let _vita = Vita(Arc::clone(&rete));
                corpo(rete);
            });
        if partito.is_err() {
            // La chiusura non è mai partita, quindi nemmeno la sua `Vita`: il
            // conto si rimette a posto qui.
            self.fili.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        true
    }

    /// Quanti fili di questa rete sono vivi.
    pub(crate) fn fili(&self) -> usize {
        self.fili.load(Ordering::SeqCst)
    }

    /// Dorme `quanto`, a passi di [`RESPIRO`], e torna prima se la rete si
    /// ferma. `false` se si è fermata.
    pub(crate) fn dormi(&self, quanto: Duration) -> bool {
        let fine = std::time::Instant::now() + quanto;
        while std::time::Instant::now() < fine {
            if self.fermo() {
                return false;
            }
            std::thread::sleep(
                RESPIRO.min(fine.saturating_duration_since(std::time::Instant::now())),
            );
        }
        !self.fermo()
    }
}

/// Toglie un filo dal conto quando esce, anche cadendo.
struct Vita(Arc<Condiviso>);

impl Drop for Vita {
    fn drop(&mut self) {
        self.0.fili.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Un nome che l'altro capo accetterà: senza caratteri di controllo, senza
/// spazi in testa e in coda, entro [`MASSIMO_NOME`] caratteri.
///
/// L'altro capo lo controlla da sé e chiude con `insieme.protocollo` un
/// saluto fuori dai tetti: ripulirlo qui vuol dire che un nome scritto male
/// nelle impostazioni diventa un nome accorciato, non un PC che nessuno
/// riesce più a raggiungere.
pub(crate) fn pulisci_nome(nome: &str) -> String {
    nome.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MASSIMO_NOME)
        .collect()
}

/// Il nome che arriva nel saluto rispetta i tetti?
pub(crate) fn nome_valido(nome: &str) -> bool {
    nome.chars().count() <= MASSIMO_NOME && !nome.chars().any(char::is_control)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_nome_si_ripulisce_invece_di_far_chiudere_le_connessioni() {
        assert_eq!(pulisci_nome("  PC di Marco\n"), "PC di Marco");
        assert_eq!(pulisci_nome("a\u{7}b"), "ab");
        let lungo = "x".repeat(MASSIMO_NOME * 2);
        assert_eq!(pulisci_nome(&lungo).chars().count(), MASSIMO_NOME);
        assert!(nome_valido(&pulisci_nome(&lungo)));
        assert!(!nome_valido("a\nb"));
        assert!(!nome_valido(&lungo));
    }
}
