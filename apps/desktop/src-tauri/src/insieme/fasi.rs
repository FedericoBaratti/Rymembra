//! Il riduttore della sessione: che cosa fa questo PC quando succede qualcosa.
//!
//! # Perché un riduttore puro
//!
//! Perché le domande difficili di «Ascolta insieme» non sono di rete né di
//! audio: sono di **ordine**. Un invito che scade mentre la risposta viaggia,
//! un `Ammesso` che arriva dopo che la persona ha già detto «Esci», l'host che
//! sparisce a metà dell'ingresso, un PC bloccato mentre è in sessione. Scritte
//! dentro il filo che parla con la rete, quelle decisioni si provano solo con
//! due PC e un cronometro; scritte qui, sono una funzione
//! `passo(situazione, evento, adesso) → (situazione, effetti)` che si prova a
//! tappeto con dei numeri.
//!
//! Il riduttore **non fa** niente: dice che cosa va fatto ([`Effetto`]), e il
//! filo della sessione (`super::filo`) lo esegue — rete, lettore, finestra. Il
//! tempo entra come argomento e le scadenze scattano con [`Evento::Tempo`]:
//! nessun orologio qui dentro, quindi nessuna prova che aspetta.
//!
//! # Le fasi
//!
//! `Spenta` → `Libera` (acceso, fuori sessione) → `Ospita` oppure `Entrando` →
//! `Membro`, e da lì `Uscendo` → `Libera`. Sopra le fasi, e indipendenti da
//! loro, stanno le connessioni di controllo aperte (chi è collegato, se è
//! fidato, come si chiama) e l'associazione in corso: si può associare un PC
//! mentre si ospita, e un PC fidato resta collegato qualunque cosa faccia la
//! sessione.
//!
//! # Le regole che non si vedono a colpo d'occhio
//!
//! - **Un `Ammesso` che non si aspettava si risponde con `Esco`**: è quello
//!   che arriva dopo che la persona è uscita durante l'ingresso, o dopo che la
//!   richiesta era già scaduta da questa parte. Senza, l'host terrebbe in
//!   sessione un membro che non lo sa.
//! - **Un invito per volta per PC**: il secondo invito dello stesso host
//!   sostituisce il primo, che altrimenti resterebbe nel banner con un numero
//!   che l'host non riconosce più.
//! - **Accettare un invito rifiuta gli altri**: si è in una sessione sola, e
//!   un host che aspetta una risposta che non arriverà lo saprebbe solo alla
//!   scadenza.
//! - **Il tetto si controlla quando si entra, non quando si invita soltanto**:
//!   due inviti mandati con un posto libero sono due sì possibili, e il
//!   secondo deve sentirsi dire `sessionePiena` invece di entrare decimo.
//! - **Un PC tolto dai fidati esce da tutto**: inviti, richieste, la sessione.
//!   Per un PC bloccato la rete chiude anche la connessione, e la `Chiusa` che
//!   ne segue trova già tutto in ordine.
//! - **Che cosa può fare chi entra lo dice chi ospita, ogni volta**: il livello
//!   ([`Diritti`]) viaggia con l'invito mandato o con il sì a una richiesta, e
//!   si controlla qui, all'arrivo di ogni gesto, prima del secchiello. Un
//!   invito e una richiesta che si incrociano non lo cambiano: vale quello con
//!   cui il PC è entrato.

mod pezzi;

use std::collections::{BTreeMap, BTreeSet};

use rymembra_domain::errors::{AppError, Domain, ErrorCodeKind};
use rymembra_insieme::messaggi::Azione;
use rymembra_insieme::messaggi::{AncoraSulFilo, Diritti, Messaggio, Persona, StatoSessione};
use rymembra_insieme::{ChiavePubblica, IdConnessione};

use super::gesti::Secchiello;
use pezzi::{avviso, chiusa, codice_arrivato, comando, manda, pari_tolto, ricevuto, spegni, tempo};

/// Quanto vale un invito, in ms: un minuto, per chi riceve e per chi manda.
pub const SCADENZA_INVITO_MS: u64 = 60_000;

/// Quanto vale una richiesta d'ingresso: un minuto, come un invito.
pub const SCADENZA_RICHIESTA_MS: u64 = 60_000;

/// Quanto si aspetta l'`Ammesso` dopo aver accettato un invito.
///
/// Quindici secondi: l'host risponde appena legge la risposta, senza chiedere
/// niente a nessuno, e oltre questo tempo l'host non c'è o non ci ha sentiti.
/// Dopo una **richiesta** invece si aspetta [`SCADENZA_RICHIESTA_MS`], perché lì
/// dall'altra parte c'è una persona che deve decidere.
pub const ATTESA_AMMISSIONE_MS: u64 = 15_000;

/// Quanto può durare un'associazione: due minuti, come nel trasporto. Oltre, il
/// codice sullo schermo non vale più niente e il dialogo si chiude da sé.
pub const SCADENZA_ASSOCIAZIONE_MS: u64 = 120_000;

/// Il codice d'errore che un membro allontanato riceve dentro `Fine`. Dalla
/// sua parte diventa l'avviso [`AVVISO_TI_HA_ALLONTANATO`].
const CODICE_ALLONTANATO: ErrorCodeKind = ErrorCodeKind::InsiemeRifiutato;

// ── le chiavi degli avvisi (`insieme:avviso`) ────────────────────────────────
//
// Sono quelle che la finestra ha in `lingue/*.json` (vedi
// `richieste-interfaccia.md` §5), e **solo** quelle: una chiave che la finestra
// non conosce diventa una frase generica. Ognuna prende al più un nome
// (`{nome}`), quello del PC di cui parla. Le `together.notice.*` sono notizie
// che spariscono da sole; le `errors.insieme.*` sono guasti, in una fascia
// rossa che resta: si mandano solo per guasti veri.

/// Chi ospita: si è appena cominciato.
pub const AVVISO_SI_OSPITA: &str = "together.notice.hosting";
/// Chi segue: «sei nella sessione di {host}».
pub const AVVISO_SEI_DENTRO: &str = "together.notice.joinedSession";
/// Tutti: «{membro} è entrato».
pub const AVVISO_ENTRATO: &str = "together.notice.joined";
/// Tutti: «{membro} è uscito».
pub const AVVISO_USCITO: &str = "together.notice.left";
/// Chi ospita: «hai allontanato {membro}».
pub const AVVISO_ALLONTANATO: &str = "together.notice.removed";
/// Chi segue: «{host} ti ha allontanato».
pub const AVVISO_TI_HA_ALLONTANATO: &str = "together.notice.youWereRemoved";
/// Chi segue: sei uscito tu, e ritrovi la tua coda.
pub const AVVISO_SEI_USCITO: &str = "together.notice.youLeft";
/// Chi segue: «{host} ha terminato la sessione».
pub const AVVISO_FINITA: &str = "together.notice.ended";
/// Chi ospita: hai terminato la sessione.
pub const AVVISO_CHIUSA: &str = "together.notice.closed";
/// Chi segue: «{host} non si sente più». Al posto di
/// `errors.insieme.connessioneCaduta`: per chi segue è una notizia, non un
/// guasto da chiudere a mano.
pub const AVVISO_HOST_SPARITO: &str = "together.notice.hostGone";
/// Chi ospita: «invito mandato a {invitato}».
pub const AVVISO_INVITO_MANDATO: &str = "together.notice.inviteSent";
/// Chi ospita: «{invitato} ha detto di no».
pub const AVVISO_INVITO_RIFIUTATO: &str = "together.notice.inviteDeclined";
/// Chi ospita: «{invitato} non ha risposto».
pub const AVVISO_INVITO_SENZA_RISPOSTA: &str = "together.notice.inviteUnanswered";
/// Chi riceve: «l'invito di {host} è scaduto».
pub const AVVISO_INVITO_SCADUTO: &str = "together.notice.inviteExpired";
/// Chi chiede: «richiesta mandata a {host}».
pub const AVVISO_RICHIESTA_MANDATA: &str = "together.notice.requestSent";
/// Chi chiede: «{host} ha detto di no».
pub const AVVISO_RICHIESTA_RIFIUTATA: &str = "together.notice.requestDeclined";
/// Chi chiede: «{host} non ha risposto».
pub const AVVISO_RICHIESTA_SENZA_RISPOSTA: &str = "together.notice.requestUnanswered";
/// Chi ospita: «la richiesta di {richiedente} è scaduta».
pub const AVVISO_RICHIESTA_SCADUTA: &str = "together.notice.requestExpired";
/// Tutti e due: «associato con {altro}».
pub const AVVISO_ASSOCIATO: &str = "together.notice.paired";
/// Tutti e due: «associazione con {altro} annullata o scaduta». Per un codice
/// diverso c'è `errors.insieme.codiceRifiutato`.
pub const AVVISO_ASSOCIAZIONE_ANNULLATA: &str = "together.notice.pairingCancelled";
/// Chi segue: «{host} ha fatto partire i clic».
pub const AVVISO_CALIBRAZIONE_ACCESA: &str = "together.notice.calibrationOn";
/// Tutti: calibrazione finita.
pub const AVVISO_CALIBRAZIONE_SPENTA: &str = "together.notice.calibrationOff";
/// Chi segue: «{host} ti ha lasciato l'ascolto».
pub const AVVISO_DIRITTI_ASCOLTA: &str = "together.notice.rightsListen";
/// Chi segue: «{host} ti ha dato i comandi».
pub const AVVISO_DIRITTI_COMANDA: &str = "together.notice.rightsControl";
/// Chi segue: «{host} ti ha dato i comandi e la coda».
pub const AVVISO_DIRITTI_TUTTO: &str = "together.notice.rightsAll";
/// Un PC che questo considera associato l'ha dimenticato, e chiude ogni
/// connessione con `insieme.nonAssociato`: «{altro} non riconosce più questo
/// PC», con quel che va fatto. Una volta sola per PC: i tentativi di
/// ricollegarsi continuano, e un avviso a ogni tentativo sarebbe rumore.
pub const AVVISO_NON_TI_RICONOSCE: &str = "together.notice.forgottenBy";

/// Una connessione di controllo, come la vede la sessione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connessione {
    /// La chiave dell'altro capo.
    pub chiave: ChiavePubblica,
    /// Il nome che questo PC mostra: quello scelto qui per un fidato, se
    /// c'è, altrimenti quello che l'altro dichiara.
    pub nome: String,
    /// Il nome che l'altro PC dichiara: è quello che viaggia nelle `Persona`
    /// dello stato della sessione, uguale per tutti.
    pub dichiarato: String,
    /// L'altro capo ospita.
    pub ospita: bool,
    /// L'ha aperta l'altro: questo PC è l'host della connessione.
    pub in_entrata: bool,
    /// L'id in `insieme_pari`, se è fidato.
    pub pari: Option<i64>,
}

/// Un invito ricevuto, in attesa della persona.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitoRicevuto {
    /// Il numero locale, quello che la finestra rimanda in `insieme_rispondi`.
    /// Non il numero dell'host: due host possono usare lo stesso.
    pub numero: u64,
    /// Il numero dell'host, per la risposta.
    pub invito: u64,
    /// Da quale connessione.
    pub da: IdConnessione,
    /// Quando scade, ms sull'orologio monotono di questo PC.
    pub scade: u64,
}

/// Come si sta entrando.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// Un invito accettato, col numero dell'host.
    Invito(u64),
    /// Una richiesta mandata.
    Richiesta,
}

/// Un invito mandato da chi ospita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitoMandato {
    /// Il numero, che torna nella risposta.
    pub invito: u64,
    /// A quale connessione.
    pub a: IdConnessione,
    /// Quando scade.
    pub scade: u64,
    /// Che cosa potrà fare, se accetta: scelto da chi ospita con l'invito.
    pub diritti: Diritti,
}

/// Una richiesta d'ingresso arrivata a chi ospita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichiestaRicevuta {
    /// Il numero locale, per `insieme_ammetti`.
    pub numero: u64,
    /// Da quale connessione.
    pub da: IdConnessione,
    /// Quando scade.
    pub scade: u64,
}

/// Un membro della sessione di questo PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MembroDiQui {
    /// Il suo numero nella sessione (da 1; lo 0 è l'host).
    pub membro: u32,
    /// La sua connessione.
    pub conn: IdConnessione,
    /// Quanti gesti può ancora mandare di fila: vedi [`Secchiello`].
    pub secchiello: Secchiello,
    /// Che cosa può fare: vedi [`Diritti`].
    pub diritti: Diritti,
}

/// Chi ospita: la sessione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ospitata {
    /// La porta su cui si ascolta.
    pub porta: u16,
    /// I membri, nell'ordine in cui sono entrati.
    pub membri: Vec<MembroDiQui>,
    /// Il numero del prossimo membro. Non si riusa: un membro uscito e
    /// rientrato è un'altra persona per le voci che porterà (fase 2).
    pub prossimo: u32,
    /// Gli inviti mandati e non ancora risposti.
    pub inviti: Vec<InvitoMandato>,
    /// Le richieste arrivate e non ancora risposte.
    pub richieste: Vec<RichiestaRicevuta>,
}

/// Chi segue: la sessione dell'altro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seguita {
    /// La connessione con l'host.
    pub host: IdConnessione,
    /// Il numero di questo PC nella sessione.
    pub io: u32,
    /// Che cosa può fare questo PC: dall'`Ammesso`, poi dalla sua `Persona`
    /// in ogni stato.
    pub diritti: Diritti,
    /// Chi c'è, dall'ultimo stato dell'host.
    pub persone: Vec<Persona>,
    /// La versione dell'ultimo stato applicato: uno più vecchio si scarta.
    pub versione: Option<u64>,
    /// L'host sta suonando la voce di calibrazione: l'ultima ancora aveva
    /// la sua etichetta.
    pub calibrazione: bool,
}

/// La fase di questo PC. Vedi il `//!`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fase {
    /// L'interruttore è spento: nessuna rete.
    Spenta,
    /// Acceso, fuori sessione. Gli inviti ricevuti aspettano qui.
    Libera {
        /// Gli inviti in attesa della persona.
        inviti: Vec<InvitoRicevuto>,
    },
    /// Invito accettato o richiesta mandata: si aspetta l'`Ammesso`.
    Entrando {
        /// La connessione con l'host.
        host: IdConnessione,
        /// Come.
        via: Via,
        /// Fin quando si aspetta.
        scade: u64,
    },
    /// Si ospita.
    Ospita(Ospitata),
    /// Si segue.
    Membro(Seguita),
    /// Si sta uscendo: il lettore sta tornando alla coda di prima. Finisce
    /// con [`Evento::Uscito`].
    Uscendo {
        /// Dopo, si spegne tutto invece di tornare liberi.
        spegni: bool,
    },
}

/// L'associazione in corso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Associazione {
    /// Su quale connessione. Il suo numero è anche quello che la finestra
    /// rimanda in `insieme_codice`.
    pub conn: IdConnessione,
    /// Le quattro cifre, quando ci sono.
    pub codice: Option<String>,
    /// Questo PC ha detto che coincide.
    pub confermato: bool,
    /// Quando scade.
    pub scade: u64,
}

/// Tutto quel che il riduttore sa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Situazione {
    /// La fase.
    pub fase: Fase,
    /// Le connessioni di controllo aperte.
    pub connessioni: BTreeMap<IdConnessione, Connessione>,
    /// L'associazione in corso.
    pub associazione: Option<Associazione>,
    /// Il prossimo numero per inviti e richieste: cresce e basta.
    pub numeratore: u64,
    /// I PC associati che hanno detto di non riconoscere più questo, e a cui
    /// lo si è già detto: vedi [`AVVISO_NON_TI_RICONOSCE`].
    pub non_riconoscono: BTreeSet<i64>,
}

impl Default for Situazione {
    fn default() -> Self {
        Self {
            fase: Fase::Spenta,
            connessioni: BTreeMap::new(),
            associazione: None,
            numeratore: 1,
            non_riconoscono: BTreeSet::new(),
        }
    }
}

/// Quel che può succedere.
#[derive(Debug, Clone, PartialEq)]
pub enum Evento {
    /// La rete è partita.
    Acceso,
    /// Si spegne tutto: l'interruttore, o l'applicazione che esce.
    Spegni,
    /// Una connessione di controllo è pronta.
    Aperta {
        /// Il suo numero.
        id: IdConnessione,
        /// La connessione.
        connessione: Connessione,
    },
    /// Una connessione si è chiusa.
    Chiusa {
        /// Quale.
        id: IdConnessione,
        /// Perché, se si sa.
        codice: Option<ErrorCodeKind>,
    },
    /// Un messaggio da una connessione.
    Ricevuto {
        /// Da quale.
        id: IdConnessione,
        /// Il messaggio.
        messaggio: Messaggio,
    },
    /// Il codice di quattro cifre di un'associazione.
    Codice {
        /// Su quale connessione.
        id: IdConnessione,
        /// Le cifre.
        codice: String,
    },
    /// L'associazione è riuscita e il PC è salvato fra i fidati.
    Associato {
        /// Su quale connessione.
        id: IdConnessione,
        /// L'id nuovo in `insieme_pari`.
        pari: i64,
    },
    /// L'associazione non è partita, o è finita senza riuscire.
    AssociazioneFinita {
        /// Su quale connessione.
        id: IdConnessione,
    },
    /// Un PC non è più fidato: dimenticato o bloccato.
    PariTolto {
        /// L'id che aveva in `insieme_pari`.
        pari: i64,
    },
    /// Un PC fidato ha un nome nuovo, scelto qui.
    Rinominato {
        /// L'id in `insieme_pari`.
        pari: i64,
        /// Il nome.
        nome: String,
    },
    /// Si sta ospitando: rete in ascolto e lettore entrato.
    Ospitato {
        /// La porta.
        porta: u16,
    },
    /// Il lettore non è riuscito a entrare come membro.
    LettoreRifiuta,
    /// Il lettore è tornato alla coda di prima.
    Uscito,
    /// Passa il tempo: scattano le scadenze.
    Tempo,
    /// Un gesto della persona.
    Comando(Comando),
}

/// I gesti della persona che toccano la sessione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comando {
    /// Comincia a ospitare.
    Ospita,
    /// Chi ospita: termina per tutti.
    Termina,
    /// Esce dalla sessione, o smette di entrarci.
    Esci,
    /// Chi ospita: invita un fidato collegato.
    Invita {
        /// L'id in `insieme_pari`.
        pari: i64,
        /// Che cosa potrà fare, se accetta.
        diritti: Diritti,
    },
    /// La risposta a un invito ricevuto.
    Rispondi {
        /// Il numero locale dell'invito.
        invito: u64,
        /// Sì o no.
        accetta: bool,
    },
    /// Chiede di entrare nella sessione di un fidato che ospita.
    Chiedi {
        /// L'id in `insieme_pari`.
        pari: i64,
    },
    /// Chi ospita: la risposta a una richiesta.
    Ammetti {
        /// Il numero locale della richiesta.
        richiesta: u64,
        /// Sì o no.
        accetta: bool,
        /// Che cosa potrà fare, se è un sì.
        diritti: Diritti,
    },
    /// Chi ospita: toglie un membro.
    Allontana {
        /// Il suo numero nella sessione.
        membro: u32,
    },
    /// Chi ospita: cambia che cosa può fare un membro.
    Concedi {
        /// Il suo numero nella sessione.
        membro: u32,
        /// Il livello nuovo.
        diritti: Diritti,
    },
    /// Comincia l'associazione su una connessione.
    Associa {
        /// La connessione.
        id: IdConnessione,
    },
    /// La risposta della persona al codice.
    ConfermaCodice {
        /// Il numero dell'associazione (quello della connessione).
        associazione: u64,
        /// Coincide.
        uguale: bool,
    },
}

/// Quel che il filo della sessione deve fare.
#[derive(Debug, Clone, PartialEq)]
pub enum Effetto {
    /// Manda un messaggio.
    Manda {
        /// A quale connessione.
        a: IdConnessione,
        /// Che cosa.
        messaggio: Messaggio,
    },
    /// Un avviso di passaggio alla finestra (`insieme:avviso`).
    Avviso {
        /// La chiave i18n.
        chiave: String,
        /// Il nome da metterci dentro.
        nome: Option<String>,
    },
    /// Il comando che ha portato qui non si fa: torna questo errore.
    Errore(AppError),
    /// Porta la finestra davanti, se è nel vassoio.
    MostraFinestra,
    /// Apre l'ascolto e fa entrare il lettore come host. Riuscito, torna come
    /// [`Evento::Ospitato`].
    Ospita,
    /// Smette di ascoltare e revoca i gettoni.
    SmettiDiOspitare,
    /// Fa entrare il lettore come membro e accende la sincronia. Fallito,
    /// torna come [`Evento::LettoreRifiuta`].
    EntraComeMembro {
        /// La connessione con l'host.
        host: IdConnessione,
    },
    /// Riporta il lettore alla coda di prima. Finito, torna come
    /// [`Evento::Uscito`].
    EsciDalLettore,
    /// Spegne la rete.
    FermaRete,
    /// I ping d'orologio verso l'host, accesi o spenti.
    Sincronizza {
        /// La connessione.
        id: IdConnessione,
        /// Accesi.
        attivo: bool,
    },
    /// Chi segue: uno stato nuovo dell'host da specchiare.
    Specchia(Box<StatoSessione>),
    /// Chi segue: un'ancora nuova dell'host.
    Segui(AncoraSulFilo),
    /// Chi segue: chi ospita ha cambiato che cosa può fare questo PC. Il
    /// ruolo del lettore va riscritto, e la finestra rifatta.
    DirittiCambiati(Diritti),
    /// Chi ospita: le persone sono cambiate, lo stato va rimandato a tutti.
    PersoneCambiate,
    /// Chi ospita: un membro non c'è più. Le sue voci escono dalla coda, e i
    /// suoi gettoni inoltrati si dimenticano.
    MembroUscito {
        /// Il suo numero nella sessione.
        membro: u32,
    },
    /// Chi ospita: il gesto di un membro, da eseguire e a cui rispondere con
    /// un `Esito`.
    Esegui {
        /// La connessione del membro.
        da: IdConnessione,
        /// Il suo numero.
        membro: u32,
        /// Il numero del comando, per l'`Esito`.
        comando: u32,
        /// Che cosa.
        azione: Box<Azione>,
    },
    /// Comincia l'associazione nel trasporto.
    AssociaInRete {
        /// Su quale connessione.
        id: IdConnessione,
    },
    /// Passa al trasporto la risposta al codice.
    ConfermaCodice {
        /// Su quale connessione.
        id: IdConnessione,
        /// Coincide.
        uguale: bool,
    },
}

/// Un passo: vedi il `//!`. `adesso` è in ms su un orologio monotono, lo
/// stesso delle scadenze già scritte nella situazione.
#[must_use]
pub fn passo(situazione: Situazione, evento: Evento, adesso: u64) -> (Situazione, Vec<Effetto>) {
    let mut s = situazione;
    let mut fx = Vec::new();
    match evento {
        Evento::Acceso => {
            if s.fase == Fase::Spenta {
                s.fase = Fase::Libera { inviti: Vec::new() };
            }
        }
        Evento::Spegni => spegni(&mut s, &mut fx),
        Evento::Aperta { id, connessione } => {
            s.connessioni.insert(id, connessione);
        }
        Evento::Chiusa { id, codice } => chiusa(&mut s, id, codice, &mut fx),
        Evento::Ricevuto { id, messaggio } => ricevuto(&mut s, id, messaggio, adesso, &mut fx),
        Evento::Codice { id, codice } => codice_arrivato(&mut s, id, codice, adesso, &mut fx),
        Evento::Associato { id, pari } => {
            // Riassociati: se un giorno lo dimentica di nuovo, lo si ridice.
            s.non_riconoscono.remove(&pari);
            if let Some(conn) = s.connessioni.get_mut(&id) {
                conn.pari = Some(pari);
                fx.push(avviso(AVVISO_ASSOCIATO, Some(conn.nome.clone())));
            }
            if s.associazione.as_ref().is_some_and(|a| a.conn == id) {
                s.associazione = None;
            }
        }
        Evento::AssociazioneFinita { id } => {
            if s.associazione.as_ref().is_some_and(|a| a.conn == id) {
                s.associazione = None;
            }
        }
        Evento::PariTolto { pari } => pari_tolto(&mut s, pari, &mut fx),
        Evento::Rinominato { pari, nome } => {
            for conn in s.connessioni.values_mut() {
                if conn.pari == Some(pari) {
                    conn.nome.clone_from(&nome);
                }
            }
        }
        Evento::Ospitato { porta } => {
            if let Fase::Libera { inviti } = &s.fase {
                // Chi ospita non entra nella sessione di nessun altro: gli
                // inviti che aspettavano si chiudono con un no, invece di
                // lasciare gli altri host ad aspettare la scadenza.
                for invito in inviti {
                    fx.push(manda(
                        invito.da,
                        Messaggio::RispostaInvito {
                            invito: invito.invito,
                            accetta: false,
                        },
                    ));
                }
                s.fase = Fase::Ospita(Ospitata {
                    porta,
                    membri: Vec::new(),
                    prossimo: 1,
                    inviti: Vec::new(),
                    richieste: Vec::new(),
                });
                fx.push(avviso(AVVISO_SI_OSPITA, None));
            }
        }
        Evento::LettoreRifiuta => {
            if let Fase::Membro(seguita) = &s.fase {
                let host = seguita.host;
                fx.push(manda(host, Messaggio::Esco));
                fx.push(Effetto::Sincronizza {
                    id: host,
                    attivo: false,
                });
                fx.push(Effetto::EsciDalLettore);
                s.fase = Fase::Uscendo { spegni: false };
            }
        }
        Evento::Uscito => {
            if let Fase::Uscendo { spegni } = s.fase {
                s.fase = if spegni {
                    Fase::Spenta
                } else {
                    Fase::Libera { inviti: Vec::new() }
                };
            }
        }
        Evento::Tempo => tempo(&mut s, adesso, &mut fx),
        Evento::Comando(comando) => self::comando(&mut s, comando, adesso, &mut fx),
    }
    (s, fx)
}

/// La prossima scadenza, se ce n'è una: il filo della sessione si sveglia
/// allora con [`Evento::Tempo`], invece di girare a vuoto.
#[must_use]
pub fn prossima_scadenza(s: &Situazione) -> Option<u64> {
    let dalla_fase = match &s.fase {
        Fase::Libera { inviti } => inviti.iter().map(|i| i.scade).min(),
        Fase::Entrando { scade, .. } => Some(*scade),
        Fase::Ospita(o) => o
            .inviti
            .iter()
            .map(|i| i.scade)
            .chain(o.richieste.iter().map(|r| r.scade))
            .min(),
        Fase::Spenta | Fase::Membro(_) | Fase::Uscendo { .. } => None,
    };
    let dall_associazione = s.associazione.as_ref().map(|a| a.scade);
    match (dalla_fase, dall_associazione) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Chi ospita: le persone della sessione, l'host per primo. `nome` è quello
/// di questo PC.
#[must_use]
pub fn persone_di_qui(s: &Situazione, nome: &str) -> Vec<Persona> {
    let mut persone = vec![Persona {
        membro: 0,
        nome: nome.to_owned(),
        diritti: Diritti::Tutto,
    }];
    if let Fase::Ospita(o) = &s.fase {
        for m in &o.membri {
            if let Some(conn) = s.connessioni.get(&m.conn) {
                persone.push(Persona {
                    membro: m.membro,
                    nome: conn.dichiarato.clone(),
                    diritti: m.diritti,
                });
            }
        }
    }
    persone
}

/// Chi segue: l'avviso per il livello nuovo che chi ospita ha dato.
#[must_use]
pub const fn avviso_dei_diritti(diritti: Diritti) -> &'static str {
    match diritti {
        Diritti::Ascolta => AVVISO_DIRITTI_ASCOLTA,
        Diritti::Comanda => AVVISO_DIRITTI_COMANDA,
        Diritti::Tutto => AVVISO_DIRITTI_TUTTO,
    }
}

/// Chi ospita: le connessioni dei membri, a cui mandare stato e ancore.
#[must_use]
pub fn connessioni_dei_membri(s: &Situazione) -> Vec<IdConnessione> {
    match &s.fase {
        Fase::Ospita(o) => o.membri.iter().map(|m| m.conn).collect(),
        _ => Vec::new(),
    }
}

/// La chiave i18n per un codice arrivato dalla rete. Solo i codici del
/// dominio `Insieme`: un PC estraneo non può far comparire qui la chiave di
/// un guasto del database. Un codice sconosciuto vale `rifiutato`.
#[must_use]
pub fn chiave_del_codice(codice: &str) -> &'static str {
    ErrorCodeKind::ALL
        .iter()
        .find(|kind| kind.domain() == Domain::Insieme && kind.code() == codice)
        .unwrap_or(&ErrorCodeKind::InsiemeRifiutato)
        .i18n_key()
}

#[cfg(test)]
mod prove;
