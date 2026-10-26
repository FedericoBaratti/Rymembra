//! [`Rete`]: quel che il desktop vede del trasporto.
//!
//! # La forma
//!
//! Comandi che non bloccano, ed [`Evento`] su un canale. Il desktop ha un filo
//! suo che riceve gli eventi e decide; qui dentro ci sono i fili che leggono le
//! connessioni, quello che accetta e quello che risponde alla scoperta. Nessuno
//! di questi chiama mai il desktop: gli manda un evento e va avanti.
//!
//! Ogni connessione ha un [`IdConnessione`], che non si riusa nella vita di
//! una [`Rete`]: un comando con l'id di una connessione chiusa non fa niente,
//! invece di finire su quella nuova che ha preso il suo posto.
//!
//! # Chi può dire cosa
//!
//! Dopo la stretta il trasporto conosce la chiave dell'altro capo, e sa dal
//! desktop quali chiavi sono fidate e quali bloccate ([`Rete::pari`]):
//!
//! - una chiave **bloccata** si chiude subito dopo la stretta, senza evento;
//! - una chiave **sconosciuta** può solo salutare e associarsi: ogni
//!   [`Messaggio`] che manda si scarta e la connessione si chiude con
//!   `insieme.nonAssociato`. Sull'host apre solo connessioni di controllo,
//!   al più otto in tutto e una per indirizzo, che vivono al più
//!   [`Tempi::vita_estraneo`] se non si sta associando; i suoi messaggi JSON
//!   hanno un tetto di quattro kibibyte, controllato prima di leggerli;
//! - una chiave **fidata** può tutto quel che il protocollo ammette. Che cosa
//!   abbia senso fare — un membro che manda uno stato di sessione, per dire —
//!   lo decide il desktop.
//!
//! E i gettoni di chi ospita li legge solo chi è fidato **e** ammesso
//! ([`Rete::ammessi`]): i membri della sessione, e nessun altro. Gli ammessi
//! partono vuoti.
//!
//! # La revisione del 26 ottobre
//!
//! Una revisione indipendente ha trovato dieci difetti, con le prove di
//! concetto; ciascuno ha una prova in `prove_revisione` che falliva prima
//! della correzione. Le scelte che ne sono uscite stanno nei moduli che le
//! fanno: i tentativi d'associazione in `codice`, le strette e i tetti
//! dell'ascolto in `ascolto` e `Condiviso::registra_entrante`, la coda
//! d'uscita e la revoca dei dati in coda in `collegamento`, l'eco della
//! scoperta in `scoperta`.

use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};

use crate::ascolto;
use crate::cliente::{self, Vivaio};
use crate::collegamento::{self, Atteso, Modo, Verso, json, prepara, vivi};
use crate::condiviso::{Condiviso, Ospitata};
use crate::cornice::{self, MASSIMO_MESSAGGIO};
use crate::errori::{chiave, errore, inatteso};
use crate::identita::{ChiavePubblica, Identita, Impronta};
use crate::messaggi::{Gettone, Messaggio};
use crate::scoperta;
use crate::servente::{COPERTINA_MASSIMA, Chiesto, Risposta, Servito};
use crate::tempi::Tempi;
use crate::trasporto::{AppDaMandare, Uso};

/// Come parte una [`Rete`].
pub struct Impostazioni {
    /// La chiave di questo PC.
    pub identita: Identita,
    /// Il nome di questo PC, come lo vedranno gli altri. Si cambia con
    /// [`Rete::rinomina`].
    pub nome: String,
    /// L'indirizzo da legare: `0.0.0.0` nell'applicazione, `127.0.0.1` nelle
    /// prove. Vale per tutto: l'ascolto di chi ospita e il socket della
    /// scoperta di chi cerca.
    pub lega: IpAddr,
    /// La prima porta da provare per ospitare; di solito [`crate::PORTA`].
    pub porta: u16,
    /// L'orologio: nanosecondi su una base unica di processo. Il desktop passa
    /// `rymembra_play::tempo::adesso_ns`.
    pub adesso_ns: fn() -> u64,
}

/// Senza il nome del PC: le stampe di prova finiscono nei diari.
impl std::fmt::Debug for Impostazioni {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Impostazioni")
            .field("identita", &self.identita)
            .field("lega", &self.lega)
            .field("porta", &self.porta)
            .finish_non_exhaustive()
    }
}

/// Il numero di una connessione, unico nella vita di una [`Rete`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IdConnessione(pub u64);

/// Uno scambio d'orologio completo, sulle basi dei tempi dei due PC.
///
/// `t1` e `t4` sono di chi chiede (il membro), `t2` e `t3` di chi risponde
/// (l'host): `t2` preso appena il ping è arrivato, `t3` subito prima di
/// scrivere il pong. È la forma che vuole `rymembra_play::sincronia::orologio`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampioneOrologio {
    /// Partenza del ping, sull'orologio di chi chiede.
    pub t1: u64,
    /// Arrivo del ping, sull'orologio di chi risponde.
    pub t2: u64,
    /// Partenza del pong, sull'orologio di chi risponde.
    pub t3: u64,
    /// Arrivo del pong, sull'orologio di chi chiede.
    pub t4: u64,
}

/// Quel che succede, nell'ordine in cui succede.
///
/// [`Debug`] è scritto a mano: niente indirizzi, nomi, chiavi o codici di
/// associazione, che finirebbero nel diario di chi stampa un evento. Restano
/// il tipo, i numeri delle connessioni, i codici del catalogo e gli istanti
/// dell'orologio.
pub enum Evento {
    /// Qualcuno ospita a quell'indirizzo: è la risposta a [`Rete::cerca`].
    ///
    /// L'indirizzo è quello da cui è arrivata la risposta, con la porta che la
    /// risposta dichiara. Arriva a ogni giro di scoperta, anche per chi si
    /// conosce già: tocca al desktop non ripetersi.
    Trovato {
        /// Dove collegarsi.
        indirizzo: SocketAddr,
        /// Chi è, per riconoscerlo fra i PC associati.
        impronta: Impronta,
    },
    /// Una connessione di controllo è pronta: stretta fatta, saluto ricevuto.
    Aperta {
        /// Il suo numero.
        id: IdConnessione,
        /// La chiave dell'altro capo.
        pari: ChiavePubblica,
        /// Il nome che l'altro capo dichiara.
        nome: String,
        /// L'altro capo sta ospitando.
        ospita: bool,
        /// L'ha aperta l'altro (siamo noi a ospitare).
        in_entrata: bool,
        /// L'indirizzo dell'altro capo.
        indirizzo: SocketAddr,
    },
    /// Un messaggio da una connessione di controllo, già passato da
    /// [`Messaggio::valida`]. Mai da una chiave non fidata.
    Ricevuto {
        /// Da quale connessione.
        id: IdConnessione,
        /// Il messaggio.
        messaggio: Messaggio,
    },
    /// L'associazione è al punto del codice: va mostrato, e chiesto alla
    /// persona se coincide con quello sull'altro schermo.
    Codice {
        /// Su quale connessione.
        id: IdConnessione,
        /// Quattro cifre, `"0000"`–`"9999"`.
        codice: String,
    },
    /// Tutte e due le persone hanno confermato il codice: l'altro capo è
    /// associato. Il desktop lo salva e lo rimette fra i fidati con
    /// [`Rete::pari`].
    Associato {
        /// Su quale connessione.
        id: IdConnessione,
        /// La chiave da ricordare.
        pari: ChiavePubblica,
    },
    /// L'associazione chiesta con [`Rete::associa`] non è cominciata, e la
    /// connessione **resta aperta**: non arriverà nessun [`Evento::Codice`].
    /// Non conta fra i tentativi; si può richiamare `associa` più tardi.
    ///
    /// Oggi un solo perché: `insieme.associazioneOccupata`, chi ospita sta
    /// già associando un altro PC (ne fa una alla volta). Arriva solo a chi ha
    /// chiesto.
    AssociazioneRimandata {
        /// Su quale connessione.
        id: IdConnessione,
        /// Perché, da mostrare alla persona.
        codice: ErrorCodeKind,
    },
    /// Uno scambio d'orologio completo, su una connessione per cui si è
    /// chiamato [`Rete::sincronizza`].
    Orologio {
        /// Su quale connessione.
        id: IdConnessione,
        /// I quattro istanti.
        campione: CampioneOrologio,
    },
    /// Una connessione di controllo si è chiusa, o non si è mai aperta.
    ///
    /// Arriva **una volta** per ogni id restituito da [`Rete::collega`] e per
    /// ogni [`Evento::Aperta`] in entrata. `codice` è il perché, se c'è:
    /// `insieme.irraggiungibile`, `insieme.connessioneCaduta`,
    /// `insieme.versione`, `insieme.nonAssociato`, `insieme.codiceRifiutato`,
    /// `insieme.associazioneScaduta` (il codice non confermato in tempo),
    /// `insieme.troppiTentativi`, `insieme.estraneoScaduto` (chi ospita ha
    /// chiuso una connessione non associata rimasta oltre il suo tempo: ci si
    /// può ricollegare subito), `insieme.protocollo`, o il codice che l'altro
    /// capo ha mandato chiudendo. Arriva uguale sui due PC: chi chiude lo
    /// manda all'altro nel suo `Chiudo`.
    ///
    /// `None` è una chiusura voluta, senza niente da dire alla persona: chi
    /// ospita ha smesso di ospitare ([`Rete::smetti_di_ospitare`]), uno dei
    /// due sta uscendo ([`Rete::ferma`]), lo stesso PC si è ricollegato da
    /// estraneo e la connessione nuova ha preso il posto della vecchia, o il
    /// desktop l'ha chiusa con [`Rete::chiudi`] senza un perché. Nessun codice
    /// del catalogo dice queste cose meglio di niente: «connessione caduta»
    /// farebbe pensare a un guasto, «irraggiungibile» a un firewall.
    Chiusa {
        /// Quale.
        id: IdConnessione,
        /// Perché.
        codice: Option<ErrorCodeKind>,
    },
}

impl std::fmt::Debug for Evento {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trovato { .. } => f.debug_struct("Trovato").finish_non_exhaustive(),
            Self::Aperta {
                id,
                ospita,
                in_entrata,
                ..
            } => f
                .debug_struct("Aperta")
                .field("id", id)
                .field("ospita", ospita)
                .field("in_entrata", in_entrata)
                .finish_non_exhaustive(),
            Self::Ricevuto { id, messaggio } => f
                .debug_struct("Ricevuto")
                .field("id", id)
                .field("messaggio", messaggio)
                .finish(),
            Self::Codice { id, .. } => f
                .debug_struct("Codice")
                .field("id", id)
                .finish_non_exhaustive(),
            Self::Associato { id, .. } => f
                .debug_struct("Associato")
                .field("id", id)
                .finish_non_exhaustive(),
            Self::AssociazioneRimandata { id, codice } => f
                .debug_struct("AssociazioneRimandata")
                .field("id", id)
                .field("codice", &codice.code())
                .finish(),
            Self::Orologio { id, campione } => f
                .debug_struct("Orologio")
                .field("id", id)
                .field("campione", campione)
                .finish(),
            Self::Chiusa { id, codice } => f
                .debug_struct("Chiusa")
                .field("id", id)
                .field("codice", &codice.map(ErrorCodeKind::code))
                .finish(),
        }
    }
}

/// Un file che questo PC lascia leggere, sotto un [`Gettone`].
///
/// Dimensione e data servono a controllare, a ogni lettura, che il file sia
/// ancora quello di quando il gettone è stato dato: se è cambiato, la lettura
/// si rifiuta con `insieme.fileCambiato` invece di mandare byte di un altro
/// file.
#[derive(Clone, PartialEq, Eq)]
pub struct FileServito {
    /// Dove sta. Viene **solo** dalla libreria di questo PC: mai da un
    /// messaggio.
    pub percorso: PathBuf,
    /// Quanti byte aveva.
    pub dimensione: u64,
    /// L'ultima modifica, in millisecondi dall'epoca Unix.
    pub modificato_ms: i64,
}

/// Senza il percorso: dentro ci sono titoli e nomi.
impl std::fmt::Debug for FileServito {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileServito")
            .field("dimensione", &self.dimensione)
            .field("modificato_ms", &self.modificato_ms)
            .finish_non_exhaustive()
    }
}

impl FileServito {
    /// Dimensione e data lette adesso dal disco, nella stessa forma in cui
    /// ogni lettura le ricontrolla.
    ///
    /// Comoda per chi dà il gettone: se dimensione e data venissero da
    /// un'altra parte — dal database, per dire — basterebbe una differenza di
    /// arrotondamento per far rifiutare ogni lettura con
    /// `insieme.fileCambiato`.
    ///
    /// # Errori
    ///
    /// `insieme.fileCambiato` se il file non si legge. Il percorso non finisce
    /// nella causa: dentro ci sono titoli e nomi, e la causa va nel diario.
    pub fn da_percorso(percorso: &Path) -> Result<Self, AppError> {
        let metadati = std::fs::metadata(percorso).map_err(|err| {
            AppError::new(ErrorCode::InsiemeFileCambiato)
                .with_cause(format!("metadati del file da servire: {}", err.kind()))
        })?;
        Ok(Self {
            percorso: percorso.to_path_buf(),
            dimensione: metadati.len(),
            modificato_ms: crate::servente::modificato_ms(&metadati),
        })
    }
}

/// Il trasporto di questo PC. Vedi il `//!` del modulo.
///
/// Si clona a poco prezzo: è un `Arc` intorno allo stato condiviso, e ogni
/// copia comanda la stessa rete. Quando se ne va l'ultima copia la rete si
/// ferma da sé, come con [`Rete::ferma`]: un `LettoreRemoto` rimasto in giro
/// non la tiene in vita.
#[derive(Clone)]
pub struct Rete {
    dentro: Arc<Condiviso>,
    _custode: Arc<Custode>,
}

impl std::fmt::Debug for Rete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rete")
            .field("dentro", &self.dentro)
            .finish_non_exhaustive()
    }
}

/// Ferma la rete quando se ne va l'ultima `Rete`.
///
/// Separato dallo stato condiviso perché i fili tengono lo stato, e uno
/// stato che si fermasse solo quando nessuno lo tiene più non si
/// fermerebbe mai: i fili aspettano proprio che si fermi per uscire.
struct Custode(Arc<Condiviso>);

impl Drop for Custode {
    fn drop(&mut self) {
        ferma_tutto(&self.0);
    }
}

impl Rete {
    /// Parte: nessuna porta aperta, nessun pacchetto mandato. Restituisce il
    /// canale degli eventi.
    ///
    /// # Errori
    ///
    /// `internal.unexpected` se non parte un filo.
    pub fn avvia(impostazioni: Impostazioni) -> Result<(Self, Receiver<Evento>), AppError> {
        Self::avvia_con(impostazioni, Tempi::default())
    }

    /// Come [`Rete::avvia`], con tempi diversi da quelli del protocollo: solo
    /// per le prove. Vedi [`Tempi`].
    ///
    /// # Errori
    ///
    /// Come [`Rete::avvia`].
    pub fn avvia_con(
        impostazioni: Impostazioni,
        tempi: Tempi,
    ) -> Result<(Self, Receiver<Evento>), AppError> {
        let Impostazioni {
            identita,
            nome,
            lega,
            porta,
            adesso_ns,
        } = impostazioni;
        let (eventi, ricevi) = std::sync::mpsc::channel();
        let dentro = Arc::new(Condiviso::nuovo(
            identita, &nome, lega, porta, adesso_ns, tempi, eventi,
        ));
        Ok((
            Self {
                _custode: Arc::new(Custode(Arc::clone(&dentro))),
                dentro,
            },
            ricevi,
        ))
    }

    /// La chiave pubblica di questo PC.
    #[must_use]
    pub fn pubblica(&self) -> ChiavePubblica {
        self.dentro.identita.pubblica()
    }

    /// Cambia il nome con cui questo PC si presenta alle connessioni nuove.
    ///
    /// Il nome si ripulisce come lo controllerà l'altro capo: senza caratteri
    /// di controllo, entro `MASSIMO_NOME` caratteri.
    pub fn rinomina(&self, nome: &str) {
        self.dentro.rinomina(nome);
    }

    /// Chi è fidato e chi è bloccato. Sostituisce gli elenchi di prima, e vale
    /// subito anche per le connessioni aperte: una chiave appena bloccata
    /// viene chiusa.
    pub fn pari(&self, fidati: Vec<ChiavePubblica>, bloccati: Vec<ChiavePubblica>) {
        let rete = &self.dentro;
        rete.imposta_fiducia(fidati, bloccati);
        for collegamento in rete.connessioni_dove(|c| rete.bloccato(&c.pari)) {
            // `nonAssociato` anche all'altro capo: un PC bloccato non deve
            // sapere di esserlo, e per lui è lo stesso che essere stato
            // dimenticato.
            collegamento.chiudi(Some(ErrorCodeKind::InsiemeNonAssociato), Modo::Cortese);
        }
    }

    /// Un giro di scoperta: una domanda in broadcast su ogni sottorete IPv4 di
    /// ogni scheda, più una per ciascuno degli indirizzi scritti a mano. Le
    /// risposte arrivano come [`Evento::Trovato`] nei secondi successivi.
    ///
    /// Non apre porte in ascolto: il socket è in uscita, legato a `lega:0`.
    pub fn cerca(&self, a_mano: &[SocketAddr]) {
        scoperta::cerca(&self.dentro, a_mano);
    }

    /// Comincia a ospitare: ascolto TCP e UDP. Restituisce la porta.
    /// Chiamarla mentre si ospita già restituisce la stessa porta.
    ///
    /// # Errori
    ///
    /// `insieme.portaOccupata` se nessuna porta si lega.
    pub fn ospita(&self) -> Result<u16, AppError> {
        let rete = &self.dentro;
        let mut ospitata = chiave(&rete.ospitata);
        if let Some(Ospitata { porta }) = *ospitata {
            return Ok(porta);
        }
        if rete.fermo() {
            return Err(AppError::new(ErrorCode::InsiemeSpenta).with_cause("rete già fermata"));
        }
        let (tcp, udp, porta) = ascolto::lega(rete.lega, rete.porta)?;
        let generazione = rete.nuova_generazione();
        let partiti = rete.filo("ascolto", move |rete| {
            ascolto::accogli(&rete, &tcp, generazione);
        }) && rete.filo("scoperta", move |rete| {
            scoperta::rispondi(&rete, &udp, porta, generazione);
        });
        if !partiti {
            // Quello partito esce vedendo la generazione cambiata.
            rete.nuova_generazione();
            return Err(inatteso("il filo dell'ascolto non parte"));
        }
        *ospitata = Some(Ospitata { porta });
        Ok(porta)
    }

    /// Smette di ospitare: chiude l'ascolto e le connessioni in entrata.
    /// Ognuna arriva come [`Evento::Chiusa`].
    pub fn smetti_di_ospitare(&self) {
        let rete = &self.dentro;
        if chiave(&rete.ospitata).take().is_none() {
            return;
        }
        // I fili dell'ascolto e della scoperta escono al prossimo giro, e una
        // connessione accettata un istante prima non si registra più.
        rete.nuova_generazione();
        // Senza un perché: è una chiusura voluta. Vedi `None` in
        // `Evento::Chiusa`.
        for collegamento in rete.connessioni_dove(|c| c.in_entrata) {
            collegamento.chiudi(None, Modo::Cortese);
        }
    }

    /// Apre una connessione di controllo verso chi ospita. Non blocca: l'esito
    /// arriva come [`Evento::Aperta`] o [`Evento::Chiusa`] con lo stesso id.
    ///
    /// Per gli host trovati dalla scoperta c'è [`Rete::collega_atteso`], che
    /// controlla anche che dall'altra parte ci sia chi ha detto di esserci.
    pub fn collega(&self, indirizzo: SocketAddr) -> IdConnessione {
        self.apri_controllo(indirizzo, None)
    }

    /// Come [`Rete::collega`], verso un host trovato dalla scoperta: dopo la
    /// stretta la chiave dell'altro capo deve avere quell'impronta, o la
    /// connessione si chiude con `insieme.irraggiungibile`.
    ///
    /// La risposta alla scoperta viaggia in chiaro e chiunque sulla rete la
    /// può scrivere, con l'impronta e la porta che vuole (vedi `scoperta`).
    /// Senza questo controllo un PC che risponde al posto di un host associato
    /// riceverebbe la connessione destinata a lui; con il controllo, la
    /// stretta dimostra la chiave, e una chiave che non ha quell'impronta è
    /// un altro PC. `irraggiungibile` e non `nonAssociato`: per chi cercava
    /// quell'host, quell'host a quell'indirizzo non c'è.
    pub fn collega_atteso(&self, indirizzo: SocketAddr, impronta: Impronta) -> IdConnessione {
        self.apri_controllo(indirizzo, Some(Atteso::Impronta(impronta)))
    }

    /// Il corpo di [`Rete::collega`] e [`Rete::collega_atteso`].
    fn apri_controllo(&self, indirizzo: SocketAddr, atteso: Option<Atteso>) -> IdConnessione {
        let rete = &self.dentro;
        let id = rete.nuovo_id();
        if rete.fermo() {
            rete.emetti(Evento::Chiusa { id, codice: None });
            return id;
        }
        chiave(&rete.in_apertura).insert(id, None);
        let partito = rete.filo("collega", move |rete| {
            let verso = Verso::Uscente {
                id,
                uso: Uso::Controllo,
                atteso,
            };
            let esito = TcpStream::connect_timeout(&indirizzo, rete.tempi.collegamento)
                .map_err(|_| Some(ErrorCodeKind::InsiemeIrraggiungibile))
                .and_then(|presa| prepara(&rete, presa, indirizzo, verso));
            match esito {
                Ok(pronto) => vivi(&rete, pronto),
                Err(codice) => {
                    // Chiusa dal desktop mentre si apriva: vale il suo perché.
                    let del_desktop = chiave(&rete.in_apertura).remove(&id).flatten();
                    rete.emetti(Evento::Chiusa {
                        id,
                        codice: del_desktop.unwrap_or(codice),
                    });
                }
            }
        });
        if !partito {
            chiave(&rete.in_apertura).remove(&id);
            rete.emetti(Evento::Chiusa {
                id,
                codice: Some(ErrorCodeKind::InsiemeConnessioneCaduta),
            });
        }
        id
    }

    /// Manda un messaggio su una connessione di controllo. Non blocca.
    ///
    /// Il messaggio passa da [`Messaggio::valida`] anche qui, prima di
    /// partire: l'altro capo chiuderebbe la connessione per un messaggio fuori
    /// dai tetti, ed è meglio un errore a chi lo manda.
    ///
    /// Oltre gli otto kibibyte di JSON parte compresso — è lo stato della
    /// sessione — e la compressione si fa qui, nel filo che chiama: qualche
    /// millisecondo per mezzo mega, contro le centinaia che quel mezzo mega
    /// costa su un Wi-Fi. Per l'altro capo non cambia niente: lo riceve come
    /// sempre. Uno [`Messaggio::Stato`] messo in coda prende il posto di
    /// quelli prima di lui che non sono ancora partiti: un membro applica solo
    /// il più nuovo.
    ///
    /// # Errori
    ///
    /// `insieme.connessioneCaduta` se la connessione non c'è più;
    /// `insieme.protocollo` per un messaggio fuori dai tetti.
    pub fn manda(&self, a: IdConnessione, messaggio: &Messaggio) -> Result<(), AppError> {
        if let Err(fuori) = messaggio.valida() {
            return Err(errore(ErrorCodeKind::InsiemeProtocollo)
                .with_cause(format!("messaggio fuori dai tetti: {}", fuori.0)));
        }
        let carico = json(&AppDaMandare { messaggio });
        if carico.len() > MASSIMO_MESSAGGIO {
            return Err(errore(ErrorCodeKind::InsiemeProtocollo)
                .with_cause(format!("messaggio di {} byte", carico.len())));
        }
        let collegamento = self
            .dentro
            .connessione(a)
            .filter(|c| c.uso == Uso::Controllo)
            .ok_or_else(|| errore(ErrorCodeKind::InsiemeConnessioneCaduta))?;
        // Compresso qui, nel filo di chi manda, e non in quello che scrive: chi
        // scrive deve restare libero per i pong e gli esiti. Vedi «La
        // compressione» in `cornice`.
        let (byte, compresso) = match cornice::comprimi(&carico) {
            Some(compresso) => (compresso, true),
            None => (carico, false),
        };
        let stato = matches!(messaggio, Messaggio::Stato(_));
        if collegamento.metti_app(byte, compresso, stato) {
            Ok(())
        } else {
            Err(errore(ErrorCodeKind::InsiemeConnessioneCaduta))
        }
    }

    /// Chiude una connessione, dicendo all'altro capo perché se c'è un perché.
    pub fn chiudi(&self, connessione: IdConnessione, codice: Option<ErrorCodeKind>) {
        let rete = &self.dentro;
        // Il lucchetto delle aperture si prende **prima** di guardare fra le
        // connessioni, e si tiene: `prepara` registra la connessione e poi
        // guarda le aperture, quindi o la si trova registrata qui, o lei
        // trova il perché scritto lì. Senza, una chiusura che cadesse fra la
        // registrazione e il controllo andrebbe persa.
        let mut in_apertura = chiave(&rete.in_apertura);
        if let Some(collegamento) = rete.connessione(connessione) {
            drop(in_apertura);
            collegamento.chiudi(codice, Modo::Cortese);
            return;
        }
        if let Some(posto) = in_apertura.get_mut(&connessione)
            && posto.is_none()
        {
            *posto = Some(codice);
        }
    }

    /// Comincia l'associazione su una connessione aperta verso chi ospita:
    /// impegno, nonce, rivelazione. L'esito è un [`Evento::Codice`] su tutti e
    /// due i PC; o, se chi ospita ne sta già facendo un'altra, un
    /// [`Evento::AssociazioneRimandata`] su questo, con la connessione ancora
    /// aperta.
    ///
    /// # Errori
    ///
    /// `insieme.troppiTentativi` se se ne sono tentate troppe di fila;
    /// `insieme.connessioneCaduta` se la connessione non c'è più;
    /// `insieme.codiceRifiutato` se dall'altra parte c'è questo stesso PC
    /// (due istanze con la stessa identità).
    pub fn associa(&self, connessione: IdConnessione) -> Result<(), AppError> {
        let collegamento = self
            .dentro
            .connessione(connessione)
            .ok_or_else(|| errore(ErrorCodeKind::InsiemeConnessioneCaduta))?;
        collegamento::associa(&self.dentro, &collegamento)
    }

    /// La risposta della persona al codice: coincide o no. Con un sì da tutti e
    /// due arriva [`Evento::Associato`]; con un no da uno dei due la
    /// connessione si chiude con `insieme.codiceRifiutato`.
    ///
    /// Con `Evento::Associato` la chiave è già fidata per questa `Rete`, fino
    /// al prossimo [`Rete::pari`]: il primo messaggio dell'altro capo può
    /// arrivare prima che il desktop la salvi.
    pub fn conferma_codice(&self, connessione: IdConnessione, uguale: bool) {
        if let Some(collegamento) = self.dentro.connessione(connessione) {
            collegamento::conferma(&self.dentro, &collegamento, uguale);
        }
    }

    /// Accende o spegne i ping d'orologio verso l'altro capo (lato membro).
    /// Accesi, arrivano [`Evento::Orologio`] qualche volta al secondo: quattro
    /// al secondo nei primi dieci secondi, poi uno.
    pub fn sincronizza(&self, connessione: IdConnessione, attivo: bool) {
        if let Some(collegamento) = self.dentro.connessione(connessione)
            && collegamento.uso == Uso::Controllo
        {
            collegamento.sincronizza(attivo);
        }
    }

    /// Lascia leggere un file sotto un gettone, a chi è fidato.
    pub fn servi(&self, gettone: Gettone, file: FileServito) {
        chiave(&self.dentro.servente).metti(gettone, Servito::File(file));
    }

    /// Lascia leggere una copertina, già in memoria, sotto un gettone.
    ///
    /// Oltre un mebibyte non si serve: chi la legge la rifiuterebbe, e il
    /// gettone resta senza niente dietro (`insieme.gettoneScaduto`).
    pub fn servi_copertina(&self, gettone: Gettone, byte: Arc<[u8]>) {
        let mut servente = chiave(&self.dentro.servente);
        if byte.len() <= COPERTINA_MASSIMA {
            servente.metti(gettone, Servito::Copertina(byte));
        } else {
            servente.togli(&gettone);
        }
    }

    /// Revoca un gettone: le letture in corso finiscono con
    /// `insieme.gettoneScaduto`.
    pub fn revoca(&self, gettone: &Gettone) {
        chiave(&self.dentro.servente).togli(gettone);
    }

    /// Revoca tutti i gettoni.
    pub fn revoca_tutto(&self) {
        chiave(&self.dentro.servente).svuota();
    }

    /// Chi legge i byte da chi ospita (lato membro). Apre le connessioni di
    /// lettura quando servono, al più due, e le riusa.
    #[must_use]
    pub fn lettore(&self, ospite: SocketAddr, pari: ChiavePubblica) -> LettoreRemoto {
        LettoreRemoto {
            dentro: Arc::new(Fonte::Remota(Vivaio::nuovo(
                Arc::clone(&self.dentro),
                ospite,
                pari,
            ))),
        }
    }

    /// Chi legge i gettoni di **questo** PC senza passare dalla rete: i file
    /// dal disco, le copertine dalla memoria, e (fase 2) i gettoni inoltrati
    /// dalla connessione di servizio del membro che li possiede.
    ///
    /// Serve all'host, che deve suonare anche le voci dei membri: per lui
    /// sono gettoni inoltrati, e un [`Rete::lettore`] verso sé stesso
    /// vorrebbe dire una connessione a sé stesso con la propria chiave.
    #[must_use]
    pub fn lettore_locale(&self) -> LettoreRemoto {
        LettoreRemoto {
            dentro: Arc::new(Fonte::Locale(Arc::clone(&self.dentro))),
        }
    }

    /// Fase 2, lato membro: apre la connessione di servizio verso chi ospita,
    /// da cui l'host chiederà i byte dei gettoni che questo PC serve.
    ///
    /// Vive quanto la connessione di controllo verso lo stesso host: se cade
    /// mentre quella c'è ancora, si riapre da sola, con un'attesa che cresce
    /// da uno a trenta secondi. Chiamarla di nuovo non ne apre un'altra.
    pub fn offri(&self, ospite: SocketAddr, pari: ChiavePubblica) {
        let rete = &self.dentro;
        if rete.fermo() || !chiave(&rete.offerte).insert(pari) {
            return;
        }
        let partito = rete.filo("offri", move |rete| {
            let mut pausa = Duration::from_secs(1);
            while !rete.fermo() && rete.controllo_verso(&pari) {
                let verso = Verso::Uscente {
                    id: rete.nuovo_id(),
                    uso: Uso::Servizio,
                    atteso: Some(Atteso::Chiave(pari)),
                };
                let pronto = TcpStream::connect_timeout(&ospite, rete.tempi.collegamento)
                    .ok()
                    .and_then(|presa| prepara(&rete, presa, ospite, verso).ok());
                if let Some(pronto) = pronto {
                    vivi(&rete, pronto);
                    pausa = Duration::from_secs(1);
                }
                if !rete.dormi(pausa) {
                    break;
                }
                pausa = pausa.saturating_mul(2).min(Duration::from_secs(30));
            }
            chiave(&rete.offerte).remove(&pari);
        });
        if !partito {
            chiave(&rete.offerte).remove(&pari);
        }
    }

    /// Fase 2, lato host: i byte di questo gettone li ha il membro con quella
    /// chiave, attraverso la sua connessione di servizio. Chi lo legge non se
    /// ne accorge.
    ///
    /// `false`, e niente cambia, se il gettone è già in uso: un file o una
    /// copertina di questo PC, o l'inoltro a un altro membro. Il gettone lo
    /// sceglie il membro che aggiunge la voce, e senza questo controllo
    /// potrebbe scegliere quello di un brano altrui e farsi chiedere i suoi
    /// byte. Lo stesso gettone allo stesso membro dà `true` senza cambiare
    /// niente. Il desktop rifiuta l'aggiunta quando torna `false`.
    pub fn inoltra(&self, gettone: Gettone, proprietario: ChiavePubblica) -> bool {
        chiave(&self.dentro.servente).inoltra(gettone, proprietario)
    }

    /// Le chiavi che possono leggere i gettoni di questo PC: i membri della
    /// sessione. Sostituisce l'elenco di prima; si chiama a ogni ingresso e a
    /// ogni uscita.
    ///
    /// Di partenza è **vuoto**: nessuno legge finché il desktop non dice chi
    /// è nella sessione. Una lettura da una chiave che non c'è si rifiuta con
    /// `insieme.gettoneScaduto`, come per un gettone revocato: a chi è fuori
    /// non si dice se il gettone esiste.
    ///
    /// Vale per le letture che arrivano a chi ospita. Sul membro che serve i
    /// suoi brani con [`Rete::offri`], chi legge è l'host a cui li ha offerti
    /// — la stretta lo dimostra — e basta che sia fidato: la sessione è la
    /// sua, e il membro non tiene un elenco suo. [`Rete::lettore_locale`] è
    /// questo PC, e non passa da qui.
    pub fn ammessi(&self, chiavi: Vec<ChiavePubblica>) {
        self.dentro.imposta_ammessi(chiavi);
    }

    /// Chiude tutto: ascolto, connessioni, fili. Si chiama uscendo.
    ///
    /// Non aspetta: i fili escono da soli appena l'altro capo di ogni
    /// connessione chiude la sua metà, di solito in pochi millisecondi (vedi
    /// [`Rete::aspetta_fili`]).
    pub fn ferma(&self) {
        ferma_tutto(&self.dentro);
    }

    /// Quanti fili di questa rete sono ancora vivi.
    #[must_use]
    pub fn fili_in_vita(&self) -> usize {
        self.dentro.fili()
    }

    /// Aspetta che i fili escano, per non più di `entro`. `true` se sono
    /// usciti tutti.
    ///
    /// Dopo [`Rete::ferma`] un filo può restare vivo solo se l'altro capo di
    /// una connessione non chiude la sua metà (un PC sparito col cavo
    /// staccato): resta allora fino alla scadenza del silenzio, perché su
    /// Windows uno `shutdown` non sveglia una lettura bloccata, e non c'è un
    /// modo senza `unsafe` di farlo prima.
    #[must_use]
    pub fn aspetta_fili(&self, entro: Duration) -> bool {
        let fine = Instant::now() + entro;
        loop {
            if self.dentro.fili() == 0 {
                return true;
            }
            if Instant::now() >= fine {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Il corpo di [`Rete::ferma`], che serve anche al [`Custode`].
fn ferma_tutto(rete: &Condiviso) {
    rete.ferma();
    *chiave(&rete.ospitata) = None;
    rete.nuova_generazione();
    // Senza un perché, come `smetti_di_ospitare`: vedi `None` in
    // `Evento::Chiusa`.
    for collegamento in rete.connessioni_dove(|_| true) {
        collegamento.chiudi(None, Modo::Cortese);
    }
    chiave(&rete.servente).svuota();
}

/// Legge i byte dei gettoni da chi ospita. Vedi [`Rete::lettore`] e
/// [`Rete::lettore_locale`].
#[derive(Debug, Clone)]
pub struct LettoreRemoto {
    dentro: Arc<Fonte>,
}

/// Da dove legge un [`LettoreRemoto`].
#[derive(Debug)]
enum Fonte {
    /// Dall'host, in rete.
    Remota(Vivaio),
    /// Da questo PC.
    Locale(Arc<Condiviso>),
}

impl LettoreRemoto {
    /// Fino a `quanti` byte da `da`, e la dimensione totale del file. Meno di
    /// `quanti` vuol dire fine del file.
    ///
    /// `annullo` alzato fa tornare subito con `internal.aborted`: è la
    /// bandiera con cui il decodificatore smette di aspettare (vedi
    /// `rymembra_net::flusso::Sorgente`). La connessione su cui la lettura
    /// stava aspettando si butta.
    ///
    /// # Errori
    ///
    /// `insieme.gettoneScaduto`, `insieme.fileCambiato`,
    /// `insieme.connessioneCaduta`, `insieme.irraggiungibile`,
    /// `internal.aborted`.
    pub fn pezzo(
        &self,
        gettone: &Gettone,
        da: u64,
        quanti: u64,
        annullo: Option<&AtomicBool>,
    ) -> Result<(Vec<u8>, u64), AppError> {
        cliente::a_finestre(da, quanti, |da, quanti| {
            self.una(gettone, Chiesto::Byte { da, quanti }, annullo)
        })
    }

    /// I byte di una copertina.
    ///
    /// # Errori
    ///
    /// Come [`LettoreRemoto::pezzo`].
    pub fn copertina(&self, gettone: &Gettone) -> Result<Vec<u8>, AppError> {
        match self.una(gettone, Chiesto::Copertina, None)? {
            Risposta::Byte { byte, .. } => Ok(byte),
            Risposta::Rifiutata(codice) => Err(errore(codice)),
        }
    }

    /// Una richiesta, dalla fonte giusta.
    fn una(
        &self,
        gettone: &Gettone,
        chiesto: Chiesto,
        annullo: Option<&AtomicBool>,
    ) -> Result<Risposta, AppError> {
        match &*self.dentro {
            Fonte::Remota(vivaio) => vivaio.chiedi(gettone, chiesto, annullo),
            Fonte::Locale(rete) => cliente::qui(rete, gettone, chiesto, annullo),
        }
    }
}
