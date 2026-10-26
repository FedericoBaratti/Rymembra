//! Chi è questo PC, di chi si fida, e chi ha visto in rete.
//!
//! # L'identità
//!
//! La chiave segreta sta nel portachiavi di Windows, voce
//! `insieme.chiave.<id del dispositivo>`: l'id è quello di `nuvola::dispositivo`,
//! che sta nella cartella dati. Due cartelle dati dello stesso utente — due
//! istanze di prova, o un profilo spostato — hanno così due identità, anche se
//! il portachiavi è uno.
//!
//! **Nessun ripiego.** Una voce che manca si crea, una volta. Un portachiavi
//! che non risponde, o una voce che non si legge come chiave, è
//! `insieme.identitaPersa` e basta: una chiave nuova al posto di quella
//! illeggibile annullerebbe in silenzio ogni associazione fatta, e la persona
//! se ne accorgerebbe solo quando gli altri PC non la riconoscono più. Per lo
//! stesso motivo una voce che c'è non si **sovrascrive** mai.
//!
//! # I vicini
//!
//! [`Vicinato`] tiene i PC che ospitano, visti dalla scoperta, e decide quando
//! aprire verso ciascuno la connessione di controllo: subito la prima volta,
//! poi — se cade o non si apre — con un'attesa che raddoppia da
//! [`ATTESA_MINIMA_MS`] a [`ATTESA_MASSIMA_MS`]. Per i fidati si riprova anche
//! senza rivederli (l'indirizzo è quello dell'ultima volta): è da quella
//! connessione che arrivano gli inviti.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;

use rymembra_app::insieme::Pari;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_insieme::{ChiavePubblica, IdConnessione, Identita, Impronta};
use rymembra_oauth::portachiavi::Portachiavi;

/// Al più tante connessioni di controllo in uscita.
pub const MASSIMO_COLLEGATI: usize = 16;

/// La prima attesa prima di ricollegarsi: cinque secondi.
pub const ATTESA_MINIMA_MS: u64 = 5_000;

/// L'attesa più lunga: un minuto.
pub const ATTESA_MASSIMA_MS: u64 = 60_000;

/// Un vicino non rivisto da tanto, e non collegato, sparisce dall'elenco: tre
/// giri di scoperta e mezzo.
pub const DIMENTICA_DOPO_MS: u64 = 35_000;

/// La voce del portachiavi della chiave di questo PC.
#[must_use]
pub fn voce_della_chiave(dispositivo: &str) -> String {
    format!("insieme.chiave.{dispositivo}")
}

/// La chiave di questo PC, dal portachiavi; creata e salvata se non c'è.
///
/// # Errori
///
/// `insieme.identitaPersa`: vedi il `//!`. La causa dice quale delle tre
/// strade, mai la chiave.
pub fn identita(portachiavi: &dyn Portachiavi, dispositivo: &str) -> Result<Identita, AppError> {
    let voce = voce_della_chiave(dispositivo);
    let persa =
        |perche: &str| AppError::new(ErrorCode::InsiemeIdentitaPersa).with_cause(perche.to_owned());
    match portachiavi.leggi(&voce) {
        Ok(Some(testo)) => Identita::da_testo(&testo)
            .map_err(|_| persa("la voce del portachiavi non è una chiave")),
        Ok(None) => {
            let nuova = Identita::nuova().map_err(|_| persa("nessuna chiave nuova dal sistema"))?;
            portachiavi
                .scrivi(&voce, &nuova.in_testo())
                .map_err(|_| persa("il portachiavi non salva la chiave nuova"))?;
            Ok(nuova)
        }
        Err(_) => Err(persa("il portachiavi non risponde")),
    }
}

/// I PC associati, letti dalla libreria, con gli indici per trovarli.
#[derive(Debug, Default, Clone)]
pub struct Fidati {
    elenco: Vec<Pari>,
    per_chiave: HashMap<ChiavePubblica, usize>,
    per_impronta: HashMap<Impronta, usize>,
}

impl Fidati {
    /// Dall'elenco della libreria. Le righe con una chiave che non si legge si
    /// saltano: non c'è un PC che possa presentarsi con quella.
    #[must_use]
    pub fn da(elenco: Vec<Pari>) -> Self {
        let mut per_chiave = HashMap::new();
        let mut per_impronta = HashMap::new();
        for (posto, pari) in elenco.iter().enumerate() {
            if let Some(chiave) = ChiavePubblica::da_testo(&pari.chiave) {
                per_chiave.insert(chiave, posto);
                per_impronta.insert(chiave.impronta(), posto);
            }
        }
        Self {
            elenco,
            per_chiave,
            per_impronta,
        }
    }

    /// Tutti, come li dà la libreria.
    #[must_use]
    pub fn elenco(&self) -> &[Pari] {
        &self.elenco
    }

    /// Il PC con questa chiave.
    #[must_use]
    pub fn per_chiave(&self, chiave: &ChiavePubblica) -> Option<&Pari> {
        self.elenco.get(*self.per_chiave.get(chiave)?)
    }

    /// Il PC con questa impronta.
    #[must_use]
    pub fn per_impronta(&self, impronta: &Impronta) -> Option<&Pari> {
        self.elenco.get(*self.per_impronta.get(impronta)?)
    }

    /// Il PC con questo id.
    #[must_use]
    pub fn per_id(&self, id: i64) -> Option<&Pari> {
        self.elenco.iter().find(|p| p.id == id)
    }

    /// I due elenchi per `Rete::pari`: le chiavi fidate e quelle bloccate.
    #[must_use]
    pub fn per_la_rete(&self) -> (Vec<ChiavePubblica>, Vec<ChiavePubblica>) {
        let mut fidate = Vec::new();
        let mut bloccate = Vec::new();
        for pari in &self.elenco {
            if let Some(chiave) = ChiavePubblica::da_testo(&pari.chiave) {
                if pari.bloccato {
                    bloccate.push(chiave);
                } else {
                    fidate.push(chiave);
                }
            }
        }
        (fidate, bloccate)
    }
}

/// Un PC che ospita, visto in rete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vicino {
    /// Dove collegarsi.
    pub indirizzo: SocketAddr,
    /// L'ultima volta che la scoperta l'ha visto, ms monotoni. `None` per un
    /// fidato che si ricorda ma che non si è visto in questo giro.
    pub visto: Option<u64>,
    /// La connessione di controllo, aperta o in apertura.
    pub conn: Option<IdConnessione>,
    /// È aperta (la stretta è fatta).
    pub aperta: bool,
    /// Il nome che ha dichiarato, quando la connessione si è aperta.
    pub nome: Option<String>,
    /// Quante volte di fila non si è riusciti a tenere la connessione.
    pub cadute: u32,
    /// Non prima di quando riprovare.
    pub non_prima: u64,
    /// L'indirizzo della connessione in corso, com'era quando la si è
    /// aperta: se nel frattempo il vicino è stato visto altrove, la caduta di
    /// quella vecchia non conta.
    pub tentato: Option<SocketAddr>,
}

/// I vicini: vedi il `//!`.
#[derive(Debug, Default)]
pub struct Vicinato {
    vicini: BTreeMap<ImprontaOrdinata, Vicino>,
    per_conn: HashMap<IdConnessione, ImprontaOrdinata>,
}

/// Un'[`Impronta`] come chiave ordinata: l'ordine dei vicini nel pannello
/// non salta da un giro all'altro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ImprontaOrdinata([u8; 8]);

impl Vicinato {
    /// La scoperta l'ha visto a quell'indirizzo.
    pub fn trovato(&mut self, indirizzo: SocketAddr, impronta: Impronta, adesso: u64) {
        let vicino = self
            .vicini
            .entry(ImprontaOrdinata(impronta.0))
            .or_insert(Vicino {
                indirizzo,
                visto: None,
                conn: None,
                aperta: false,
                nome: None,
                cadute: 0,
                non_prima: 0,
                tentato: None,
            });
        // Rivisto dopo un'assenza, o visto a un indirizzo nuovo (scritto a
        // mano adesso, o il PC ha cambiato rete): si riprova subito,
        // l'attesa crescente era per un PC che a quell'indirizzo non c'era.
        if vicino.conn.is_none() && (vicino.visto.is_none() || vicino.indirizzo != indirizzo) {
            vicino.non_prima = adesso;
        }
        vicino.indirizzo = indirizzo;
        vicino.visto = Some(adesso);
    }

    /// I vicini a cui aprire adesso una connessione: non collegati, non
    /// bloccati, non questo PC, finita l'attesa, entro il tetto.
    #[must_use]
    pub fn da_collegare(
        &self,
        adesso: u64,
        mia: Impronta,
        fidati: &Fidati,
    ) -> Vec<(Impronta, SocketAddr)> {
        let collegati = self.vicini.values().filter(|v| v.conn.is_some()).count();
        let posti = MASSIMO_COLLEGATI.saturating_sub(collegati);
        self.vicini
            .iter()
            .filter(|(impronta, v)| {
                let impronta = Impronta(impronta.0);
                let fidato = fidati.per_impronta(&impronta);
                v.conn.is_none()
                    && impronta != mia
                    && !fidato.is_some_and(|p| p.bloccato)
                    && v.non_prima <= adesso
                    // Un estraneo non visto in questo giro non si cerca: è
                    // la scoperta che deve ritrovarlo.
                    && (v.visto.is_some() || fidato.is_some())
            })
            .take(posti)
            .map(|(impronta, v)| (Impronta(impronta.0), v.indirizzo))
            .collect()
    }

    /// Si sta aprendo una connessione verso quel vicino.
    pub fn collegando(&mut self, impronta: Impronta, conn: IdConnessione) {
        let chiave = ImprontaOrdinata(impronta.0);
        if let Some(v) = self.vicini.get_mut(&chiave) {
            v.conn = Some(conn);
            v.aperta = false;
            v.tentato = Some(v.indirizzo);
            self.per_conn.insert(conn, chiave);
        }
    }

    /// La connessione si è aperta: il nome che dichiara.
    pub fn aperta(&mut self, conn: IdConnessione, nome: &str) {
        if let Some(v) = self
            .per_conn
            .get(&conn)
            .and_then(|k| self.vicini.get_mut(k))
        {
            v.aperta = true;
            v.cadute = 0;
            v.nome = Some(nome.to_owned());
        }
    }

    /// La connessione si è chiusa: la prossima prova dopo un'attesa che
    /// raddoppia.
    pub fn chiusa(&mut self, conn: IdConnessione, adesso: u64) {
        let Some(chiave) = self.per_conn.remove(&conn) else {
            return;
        };
        if let Some(v) = self.vicini.get_mut(&chiave) {
            v.conn = None;
            v.aperta = false;
            // Una connessione verso un indirizzo che intanto è cambiato (uno
            // scritto a mano adesso, la scoperta che l'ha trovato altrove):
            // la sua caduta non dice niente del vicino, si prova subito quello
            // nuovo. Prima toccava aspettare l'attesa crescente, e chi aveva
            // appena corretto l'indirizzo non vedeva succedere niente.
            if v.tentato
                .take()
                .is_some_and(|tentato| tentato != v.indirizzo)
            {
                v.non_prima = adesso;
                return;
            }
            v.cadute = v.cadute.saturating_add(1);
            v.non_prima = adesso.saturating_add(attesa_dopo(v.cadute));
        }
    }

    /// Il vicino con quell'impronta è nell'elenco, visto o ricordato. Serve
    /// alle prove della pulizia.
    #[cfg(test)]
    #[must_use]
    pub fn conosciuto(&self, impronta: Impronta) -> bool {
        self.vicini.contains_key(&ImprontaOrdinata(impronta.0))
    }

    /// Il giro di pulizia: i vicini non rivisti da [`DIMENTICA_DOPO_MS`] e non
    /// collegati spariscono, tranne i fidati, che restano per riprovarli.
    pub fn pulisci(&mut self, adesso: u64, fidati: &Fidati) {
        for v in self.vicini.values_mut() {
            if v.visto
                .is_some_and(|visto| adesso.saturating_sub(visto) > DIMENTICA_DOPO_MS)
            {
                v.visto = None;
            }
        }
        self.vicini.retain(|impronta, v| {
            v.conn.is_some()
                || v.visto.is_some()
                || fidati.per_impronta(&Impronta(impronta.0)).is_some()
        });
    }

    /// I vicini da mostrare: quelli visti in questo giro o collegati.
    pub fn visibili(&self) -> impl Iterator<Item = (Impronta, &Vicino)> {
        self.vicini
            .iter()
            .filter(|(_, v)| v.visto.is_some() || v.aperta)
            .map(|(impronta, v)| (Impronta(impronta.0), v))
    }

    /// La connessione aperta verso il vicino con quell'impronta.
    #[must_use]
    pub fn conn_di(&self, impronta: Impronta) -> Option<IdConnessione> {
        self.vicini
            .get(&ImprontaOrdinata(impronta.0))
            .filter(|v| v.aperta)
            .and_then(|v| v.conn)
    }

    /// La persona ha chiesto di cercare (o ha cambiato gli indirizzi scritti
    /// a mano): chi non è collegato si riprova subito, senza aspettare la fine
    /// dell'attesa crescente. Fino a un minuto di attesa dopo qualche prova
    /// andata male, proprio mentre si sta cercando di collegarsi, era il «non
    /// succede niente» della prima prova fra due PC.
    pub fn riprova_subito(&mut self, adesso: u64) {
        for v in self.vicini.values_mut().filter(|v| v.conn.is_none()) {
            v.non_prima = v.non_prima.min(adesso);
        }
    }

    /// Tutto da capo: la rete si è spenta.
    pub fn svuota(&mut self) {
        self.vicini.clear();
        self.per_conn.clear();
    }
}

/// L'attesa dopo `cadute` di fila: 5, 10, 20, 40, 60, 60… secondi.
#[must_use]
pub fn attesa_dopo(cadute: u32) -> u64 {
    let raddoppi = cadute.saturating_sub(1).min(8);
    ATTESA_MINIMA_MS
        .saturating_mul(1_u64 << raddoppi)
        .min(ATTESA_MASSIMA_MS)
}

/// Un'impronta in esadecimale, come la vuole `insieme_associa`.
#[must_use]
pub fn impronta_in_testo(impronta: Impronta) -> String {
    use std::fmt::Write as _;
    impronta
        .0
        .iter()
        .fold(String::with_capacity(16), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// L'impronta da una chiave di `insieme_associa`. `None` se non sono otto byte
/// in esadecimale.
#[must_use]
pub fn impronta_da_testo(testo: &str) -> Option<Impronta> {
    let testo = testo.trim();
    if testo.len() != 16 || !testo.is_ascii() {
        return None;
    }
    let mut byte = [0_u8; 8];
    for (posto, b) in byte.iter_mut().enumerate() {
        let da = posto.checked_mul(2)?;
        *b = u8::from_str_radix(testo.get(da..da.checked_add(2)?)?, 16).ok()?;
    }
    Some(Impronta(byte))
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::sync::Mutex;

    /// Un portachiavi in memoria, che può far finta di non rispondere.
    #[derive(Default)]
    struct Finto {
        voci: Mutex<HashMap<String, String>>,
        rotto: bool,
    }

    impl Portachiavi for Finto {
        fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
            if self.rotto {
                return Err(AppError::new(ErrorCode::SettingsSecretUnavailable {
                    key: chiave.to_owned(),
                }));
            }
            Ok(self.voci.lock().unwrap().get(chiave).cloned())
        }
        fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
            self.voci
                .lock()
                .unwrap()
                .insert(chiave.to_owned(), valore.to_owned());
            Ok(())
        }
        fn cancella(&self, chiave: &str) -> Result<(), AppError> {
            self.voci.lock().unwrap().remove(chiave);
            Ok(())
        }
    }

    #[test]
    fn un_portachiavi_che_non_risponde_e_identita_persa_senza_ripiego() {
        let finto = Finto {
            rotto: true,
            ..Finto::default()
        };
        let err = identita(&finto, "abc").unwrap_err();
        assert_eq!(err.code().kind().code(), "insieme.identitaPersa");
        assert!(finto.voci.lock().unwrap().is_empty(), "niente chiave nuova");
    }

    #[test]
    fn una_voce_illeggibile_non_si_sovrascrive() {
        let finto = Finto::default();
        finto
            .scrivi(&voce_della_chiave("abc"), "zzz-segreto-finto")
            .unwrap();
        let err = identita(&finto, "abc").unwrap_err();
        assert_eq!(err.code().kind().code(), "insieme.identitaPersa");
        assert_eq!(
            finto
                .voci
                .lock()
                .unwrap()
                .get("insieme.chiave.abc")
                .map(String::as_str),
            Some("zzz-segreto-finto")
        );
        // E la causa non contiene il testo della voce.
        assert!(!err.cause().unwrap_or("").contains("zzz-segreto-finto"));
    }

    #[test]
    fn una_voce_che_manca_si_crea_una_volta_e_poi_si_rilegge() {
        let finto = Finto::default();
        let prima = identita(&finto, "abc").expect("chiave nuova");
        let dopo = identita(&finto, "abc").expect("la stessa, riletta");
        assert_eq!(prima.pubblica(), dopo.pubblica());
        let altra = identita(&finto, "def").expect("un altro dispositivo");
        assert_ne!(
            prima.pubblica(),
            altra.pubblica(),
            "una cartella dati, un'identità"
        );
    }

    fn pari(id: i64, byte: u8, bloccato: bool) -> Pari {
        Pari {
            id,
            chiave: ChiavePubblica([byte; 32]).in_testo(),
            nome: format!("pc{id}"),
            bloccato,
            associato_ms: 0,
            visto_ms: None,
        }
    }

    #[test]
    fn i_fidati_si_trovano_per_chiave_e_impronta_e_si_dividono_per_la_rete() {
        let fidati = Fidati::da(vec![
            pari(1, 1, false),
            pari(2, 2, true),
            Pari {
                chiave: "rotta".to_owned(),
                ..pari(3, 3, false)
            },
        ]);
        assert_eq!(
            fidati.per_chiave(&ChiavePubblica([1; 32])).map(|p| p.id),
            Some(1)
        );
        assert_eq!(
            fidati
                .per_impronta(&ChiavePubblica([2; 32]).impronta())
                .map(|p| p.id),
            Some(2)
        );
        let (fidate, bloccate) = fidati.per_la_rete();
        assert_eq!(fidate, vec![ChiavePubblica([1; 32])]);
        assert_eq!(bloccate, vec![ChiavePubblica([2; 32])]);
    }

    fn addr(porta: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], porta))
    }

    #[test]
    fn un_vicino_si_collega_subito_e_poi_con_l_attesa_che_cresce() {
        let mia = Impronta([9; 8]);
        let fidati = Fidati::da(vec![]);
        let mut v = Vicinato::default();
        let lui = Impronta([1; 8]);
        v.trovato(addr(47811), lui, 0);
        assert_eq!(v.da_collegare(0, mia, &fidati), vec![(lui, addr(47811))]);
        v.collegando(lui, IdConnessione(1));
        assert!(
            v.da_collegare(0, mia, &fidati).is_empty(),
            "già in apertura"
        );
        v.chiusa(IdConnessione(1), 1_000);
        assert!(v.da_collegare(5_999, mia, &fidati).is_empty());
        assert_eq!(v.da_collegare(6_000, mia, &fidati).len(), 1);
        v.collegando(lui, IdConnessione(2));
        v.chiusa(IdConnessione(2), 6_000);
        assert!(
            v.da_collegare(15_999, mia, &fidati).is_empty(),
            "dieci secondi la seconda"
        );
        assert_eq!(attesa_dopo(10), ATTESA_MASSIMA_MS);
    }

    #[test]
    fn se_stessi_e_i_bloccati_non_si_collegano_e_il_tetto_vale() {
        let mia = Impronta([9; 8]);
        let bloccato = ChiavePubblica([2; 32]);
        let fidati = Fidati::da(vec![pari(2, 2, true)]);
        let mut v = Vicinato::default();
        v.trovato(addr(1), mia, 0);
        v.trovato(addr(2), bloccato.impronta(), 0);
        for i in 0..20_u8 {
            v.trovato(addr(100 + u16::from(i)), Impronta([i + 20; 8]), 0);
        }
        let da_fare = v.da_collegare(0, mia, &fidati);
        assert_eq!(da_fare.len(), MASSIMO_COLLEGATI);
        assert!(
            da_fare
                .iter()
                .all(|(imp, _)| *imp != mia && *imp != bloccato.impronta())
        );
    }

    #[test]
    fn un_estraneo_sparito_si_dimentica_un_fidato_si_riprova() {
        let fidato = ChiavePubblica([1; 32]);
        let fidati = Fidati::da(vec![pari(1, 1, false)]);
        let mut v = Vicinato::default();
        v.trovato(addr(1), fidato.impronta(), 0);
        v.trovato(addr(2), Impronta([7; 8]), 0);
        v.pulisci(DIMENTICA_DOPO_MS + 1, &fidati);
        assert!(v.conosciuto(fidato.impronta()));
        assert!(!v.conosciuto(Impronta([7; 8])));
        assert_eq!(v.visibili().count(), 0, "non visto in questo giro");
        assert_eq!(
            v.da_collegare(DIMENTICA_DOPO_MS + 1, Impronta([9; 8]), &fidati),
            vec![(fidato.impronta(), addr(1))],
            "il fidato si riprova all'ultimo indirizzo"
        );
    }

    #[test]
    fn un_indirizzo_nuovo_o_una_ricerca_chiesta_non_aspettano_l_attesa_crescente() {
        let fidato = ChiavePubblica([1; 32]);
        let fidati = Fidati::da(vec![pari(1, 1, false)]);
        let mia = Impronta([9; 8]);
        let mut v = Vicinato::default();
        v.trovato(addr(1), fidato.impronta(), 0);
        // Quattro cadute di fila: la prossima prova fra quaranta secondi.
        for n in 0..4 {
            v.collegando(fidato.impronta(), IdConnessione(n));
            v.chiusa(IdConnessione(n), 1_000);
        }
        assert!(v.da_collegare(2_000, mia, &fidati).is_empty());
        // Rivisto allo stesso indirizzo: l'attesa resta.
        v.trovato(addr(1), fidato.impronta(), 2_000);
        assert!(v.da_collegare(2_000, mia, &fidati).is_empty());
        // A un indirizzo nuovo: subito.
        v.trovato(addr(2), fidato.impronta(), 2_000);
        assert_eq!(
            v.da_collegare(2_000, mia, &fidati),
            vec![(fidato.impronta(), addr(2))]
        );
        // Una prova verso l'indirizzo vecchio ancora in corso quando arriva
        // quello nuovo: la sua caduta non fa aspettare.
        v.collegando(fidato.impronta(), IdConnessione(8));
        v.trovato(addr(3), fidato.impronta(), 2_500);
        v.chiusa(IdConnessione(8), 2_600);
        assert_eq!(
            v.da_collegare(2_600, mia, &fidati),
            vec![(fidato.impronta(), addr(3))]
        );
        // Di nuovo giù, e poi la persona preme «Cerca»: subito anche lì.
        v.collegando(fidato.impronta(), IdConnessione(9));
        v.chiusa(IdConnessione(9), 3_000);
        assert!(v.da_collegare(3_000, mia, &fidati).is_empty());
        v.riprova_subito(3_000);
        assert_eq!(v.da_collegare(3_000, mia, &fidati).len(), 1);
    }

    #[test]
    fn l_impronta_va_e_torna_in_esadecimale() {
        let i = Impronta([0x3f, 0xa2, 0x91, 0xc0, 0, 1, 2, 0xff]);
        assert_eq!(impronta_in_testo(i), "3fa291c0000102ff");
        assert_eq!(impronta_da_testo("3fa291c0000102ff"), Some(i));
        assert_eq!(impronta_da_testo("3fa2"), None);
        assert_eq!(impronta_da_testo("zzzzzzzzzzzzzzzz"), None);
        assert_eq!(impronta_da_testo("ééééééééé"), None);
    }

    #[test]
    fn un_estraneo_chiuso_dopo_due_minuti_non_sparisce_dal_pannello_e_aspetta_a_ricollegarsi() {
        // Il trasporto chiude un estraneo che non si associa dopo due minuti
        // (`Chiusa{None}`), e rifiuta prima del saluto senza `Aperta`: in
        // tutti e due i casi il vicino resta nel pannello col suo nome finché
        // la scoperta lo vede, e la prova dopo aspetta.
        let mia = Impronta([9; 8]);
        let fidati = Fidati::da(vec![]);
        let lui = Impronta([4; 8]);
        let mut v = Vicinato::default();
        v.trovato(addr(47811), lui, 0);
        v.collegando(lui, IdConnessione(1));
        v.aperta(IdConnessione(1), "Portatile");
        v.trovato(addr(47811), lui, 10_000);
        v.chiusa(IdConnessione(1), 120_000);
        v.trovato(addr(47811), lui, 120_500);
        let visti: Vec<(Impronta, Option<String>)> =
            v.visibili().map(|(i, x)| (i, x.nome.clone())).collect();
        assert_eq!(visti, vec![(lui, Some("Portatile".to_owned()))]);
        assert!(
            v.da_collegare(124_999, mia, &fidati).is_empty(),
            "rivisto non salta l'attesa"
        );
        assert_eq!(v.da_collegare(125_000, mia, &fidati).len(), 1);
        // Un rifiuto prima del saluto: nessuna `Aperta`, l'attesa raddoppia.
        v.collegando(lui, IdConnessione(2));
        v.chiusa(IdConnessione(2), 125_000);
        v.trovato(addr(47811), lui, 126_000);
        assert_eq!(v.visibili().count(), 1, "sempre lì");
        assert!(v.da_collegare(134_999, mia, &fidati).is_empty());
        assert_eq!(v.da_collegare(135_000, mia, &fidati).len(), 1);
    }
}
