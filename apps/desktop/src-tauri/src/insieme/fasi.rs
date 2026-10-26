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

use std::collections::{BTreeMap, BTreeSet};

use rymembra_domain::errors::{AppError, Domain, ErrorCode, ErrorCodeKind};
use rymembra_insieme::messaggi::Azione;
use rymembra_insieme::messaggi::{
    AncoraSulFilo, Diritti, MASSIMO_PERSONE, Messaggio, Persona, StatoSessione,
};
use rymembra_insieme::{ChiavePubblica, IdConnessione};

use super::gesti::Secchiello;
use crate::riproduzione::insieme::UID_CALIBRAZIONE;

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

// ── i pezzi ─────────────────────────────────────────────────────────────────

fn manda(a: IdConnessione, messaggio: Messaggio) -> Effetto {
    Effetto::Manda { a, messaggio }
}

fn avviso(chiave: &str, nome: Option<String>) -> Effetto {
    Effetto::Avviso {
        chiave: chiave.to_owned(),
        nome,
    }
}

fn errore(codice: ErrorCode) -> Effetto {
    Effetto::Errore(AppError::new(codice))
}

fn rifiutato(codice: ErrorCodeKind) -> Messaggio {
    Messaggio::Rifiutato {
        codice: codice.code().to_owned(),
    }
}

fn nome_di(s: &Situazione, id: IdConnessione) -> Option<String> {
    s.connessioni.get(&id).map(|conn| conn.nome.clone())
}

/// Il tetto: chi ospita più i membri, contro `MASSIMO_PERSONE`.
fn piena(o: &Ospitata) -> bool {
    o.membri.len().saturating_add(1) >= MASSIMO_PERSONE
}

fn sessione_piena() -> ErrorCode {
    ErrorCode::InsiemeSessionePiena {
        massimo: u32::try_from(MASSIMO_PERSONE).unwrap_or(u32::MAX),
    }
}

fn non_disponibile(gesto: &str) -> ErrorCode {
    ErrorCode::InsiemeGestoNonDisponibile {
        gesto: gesto.to_owned(),
    }
}

/// Chi ospita: fa entrare `conn` come membro con quei `diritti`, se c'è posto.
fn ammetti(
    s: &mut Situazione,
    conn: IdConnessione,
    diritti: Diritti,
    fx: &mut Vec<Effetto>,
) -> bool {
    let nome = nome_di(s, conn);
    let Fase::Ospita(o) = &mut s.fase else {
        return false;
    };
    // Già dentro: lo stesso `Ammesso` di prima, col livello con cui è entrato.
    // Succede quando la risposta a un invito e una richiesta dello stesso PC
    // si incrociano: per cambiarlo c'è «In sessione», non un secondo sì.
    if let Some(gia) = o.membri.iter().find(|m| m.conn == conn) {
        fx.push(manda(
            conn,
            Messaggio::Ammesso {
                membro: gia.membro,
                diritti: gia.diritti,
            },
        ));
        return true;
    }
    if piena(o) {
        fx.push(manda(conn, rifiutato(ErrorCodeKind::InsiemeSessionePiena)));
        return false;
    }
    let membro = o.prossimo;
    o.prossimo = o.prossimo.saturating_add(1);
    o.membri.push(MembroDiQui {
        membro,
        conn,
        secchiello: Secchiello::default(),
        diritti,
    });
    // Un PC dentro non ha più niente da chiedere né da rispondere.
    o.inviti.retain(|i| i.a != conn);
    o.richieste.retain(|r| r.da != conn);
    fx.push(manda(conn, Messaggio::Ammesso { membro, diritti }));
    fx.push(Effetto::PersoneCambiate);
    fx.push(avviso(AVVISO_ENTRATO, nome));
    true
}

/// Chi ospita: toglie `conn` dai membri. Il suo numero, se c'era.
fn togli_membro(o: &mut Ospitata, conn: IdConnessione) -> Option<u32> {
    let membro = o.membri.iter().find(|m| m.conn == conn)?.membro;
    o.membri.retain(|m| m.conn != conn);
    Some(membro)
}

fn spegni(s: &mut Situazione, fx: &mut Vec<Effetto>) {
    match &s.fase {
        Fase::Spenta => return,
        Fase::Ospita(o) => {
            for m in &o.membri {
                fx.push(manda(m.conn, Messaggio::Fine { codice: None }));
            }
            fx.push(Effetto::EsciDalLettore);
            fx.push(Effetto::SmettiDiOspitare);
            fx.push(avviso(AVVISO_CHIUSA, None));
        }
        Fase::Membro(seguita) => {
            fx.push(manda(seguita.host, Messaggio::Esco));
            fx.push(Effetto::EsciDalLettore);
            fx.push(avviso(AVVISO_SEI_USCITO, None));
        }
        Fase::Entrando { host, via, .. } => {
            fx.push(manda(*host, ritiro(*via)));
        }
        Fase::Libera { inviti } => {
            for invito in inviti {
                fx.push(manda(
                    invito.da,
                    Messaggio::RispostaInvito {
                        invito: invito.invito,
                        accetta: false,
                    },
                ));
            }
        }
        // A metà di un'uscita: il lettore sta già tornando, resta solo da
        // spegnere la rete alla fine.
        Fase::Uscendo { .. } => {}
    }
    fx.push(Effetto::FermaRete);
    s.fase = match s.fase {
        // L'uscita del lettore è in corso, o appena chiesta qui sopra: la
        // fase si chiude quando torna `Uscito`.
        Fase::Ospita(_) | Fase::Membro(_) | Fase::Uscendo { .. } => Fase::Uscendo { spegni: true },
        _ => Fase::Spenta,
    };
    s.connessioni.clear();
    s.associazione = None;
}

/// Il messaggio con cui chi sta entrando ci ripensa.
///
/// Dopo un invito accettato è `Esco` e non una risposta negativa: l'host può
/// averlo già ammesso, e `Esco` vale in tutti e due i casi. Dopo una richiesta
/// è `NonChiedoPiu`; se l'host avesse già ammesso, l'`Ammesso` arriverà a un
/// PC libero, che risponde `Esco` da sé.
fn ritiro(via: Via) -> Messaggio {
    match via {
        Via::Invito(_) => Messaggio::Esco,
        Via::Richiesta => Messaggio::NonChiedoPiu,
    }
}

fn chiusa(
    s: &mut Situazione,
    id: IdConnessione,
    codice: Option<ErrorCodeKind>,
    fx: &mut Vec<Effetto>,
) {
    let nome = nome_di(s, id);
    let pari = s.connessioni.remove(&id).and_then(|c| c.pari);
    // Per questo PC l'altro è associato, per l'altro no: l'ha dimenticato, o
    // ha cambiato identità. Senza dirlo, questo PC riprovava a collegarsi per
    // sempre e la persona vedeva un PC associato che non risponde.
    if codice == Some(ErrorCodeKind::InsiemeNonAssociato)
        && let Some(pari) = pari
        && s.non_riconoscono.insert(pari)
    {
        fx.push(avviso(AVVISO_NON_TI_RICONOSCE, nome.clone()));
    }
    if s.associazione.as_ref().is_some_and(|a| a.conn == id) {
        s.associazione = None;
        // Il codice diverso è un guasto da leggere; una connessione caduta a
        // metà è un'associazione che non c'è stata, e basta dirlo.
        let chiave = match codice {
            None
            | Some(
                ErrorCodeKind::InsiemeConnessioneCaduta | ErrorCodeKind::InsiemeIrraggiungibile,
            ) => AVVISO_ASSOCIAZIONE_ANNULLATA,
            Some(kind) if kind.domain() == Domain::Insieme => kind.i18n_key(),
            Some(_) => AVVISO_ASSOCIAZIONE_ANNULLATA,
        };
        fx.push(avviso(chiave, nome.clone()));
    }
    match &mut s.fase {
        Fase::Libera { inviti } => inviti.retain(|i| i.da != id),
        Fase::Entrando { host, .. } if *host == id => {
            fx.push(avviso(chiave_della_caduta(codice), nome));
            s.fase = Fase::Libera { inviti: Vec::new() };
        }
        Fase::Ospita(o) => {
            o.inviti.retain(|i| i.a != id);
            o.richieste.retain(|r| r.da != id);
            if let Some(membro) = togli_membro(o, id) {
                fx.push(Effetto::MembroUscito { membro });
                fx.push(Effetto::PersoneCambiate);
                fx.push(avviso(AVVISO_USCITO, nome));
            }
        }
        // L'host è sparito: connessione caduta o quindici secondi di
        // silenzio, che il trasporto già traduce in una chiusura. Niente
        // passaggio di ruolo: si esce e lo si dice.
        Fase::Membro(seguita) if seguita.host == id => {
            fx.push(Effetto::EsciDalLettore);
            fx.push(avviso(chiave_della_caduta(codice), nome));
            s.fase = Fase::Uscendo { spegni: false };
        }
        _ => {}
    }
}

/// L'avviso per l'host che si chiude sotto a chi segue o entra: una notizia
/// se la connessione è solo caduta (o tace da quindici secondi), il guasto
/// col suo codice se l'altro capo ha detto perché — una versione diversa, un
/// messaggio fuori regola.
fn chiave_della_caduta(codice: Option<ErrorCodeKind>) -> &'static str {
    match codice {
        Some(kind)
            if kind.domain() == Domain::Insieme
                && !matches!(
                    kind,
                    ErrorCodeKind::InsiemeConnessioneCaduta | ErrorCodeKind::InsiemeIrraggiungibile
                ) =>
        {
            kind.i18n_key()
        }
        _ => AVVISO_HOST_SPARITO,
    }
}

fn ricevuto(
    s: &mut Situazione,
    id: IdConnessione,
    messaggio: Messaggio,
    adesso: u64,
    fx: &mut Vec<Effetto>,
) {
    // Il trasporto non consegna messaggi di chiavi non fidate; qui si guarda
    // lo stesso, perché un PC dimenticato un istante fa può avere ancora un
    // messaggio in volo.
    let Some(conn) = s.connessioni.get(&id) else {
        return;
    };
    if conn.pari.is_none() {
        return;
    }
    let nome = Some(conn.nome.clone());
    let da_un_host = conn.ospita && !conn.in_entrata;
    match messaggio {
        Messaggio::Invito {
            invito,
            scade_fra_ms,
        } => {
            match &mut s.fase {
                Fase::Libera { inviti } if da_un_host => {
                    inviti.retain(|i| i.da != id);
                    let numero = s.numeratore;
                    inviti.push(InvitoRicevuto {
                        numero,
                        invito,
                        da: id,
                        // La durata la dice chi invita, ma il tetto è nostro: un
                        // invito che resta nel banner per un'ora non è un invito.
                        scade: adesso.saturating_add(scade_fra_ms.min(SCADENZA_INVITO_MS)),
                    });
                    s.numeratore = s.numeratore.saturating_add(1);
                    fx.push(Effetto::MostraFinestra);
                }
                // In sessione, entrando o mentre si ospita: no, subito.
                _ => fx.push(manda(
                    id,
                    Messaggio::RispostaInvito {
                        invito,
                        accetta: false,
                    },
                )),
            }
        }
        Messaggio::RispostaInvito { invito, accetta } => {
            let Fase::Ospita(o) = &mut s.fase else {
                return;
            };
            // Il livello scelto con l'invito, se l'invito vale ancora.
            let atteso = o
                .inviti
                .iter()
                .find(|i| i.invito == invito && i.a == id && i.scade > adesso)
                .map(|i| i.diritti);
            o.inviti.retain(|i| !(i.invito == invito && i.a == id));
            if !accetta {
                if atteso.is_some() {
                    fx.push(avviso(AVVISO_INVITO_RIFIUTATO, nome));
                }
                return;
            }
            if let Some(diritti) = atteso {
                ammetti(s, id, diritti, fx);
            } else {
                // Scaduto, sostituito da uno più nuovo, o mai mandato: un sì
                // fuori tempo non fa entrare nessuno.
                fx.push(manda(id, rifiutato(ErrorCodeKind::InsiemeRifiutato)));
            }
        }
        Messaggio::Chiedi => match &mut s.fase {
            Fase::Ospita(o) if conn.in_entrata => {
                if o.membri.iter().any(|m| m.conn == id) {
                    return;
                }
                if piena(o) {
                    fx.push(manda(id, rifiutato(ErrorCodeKind::InsiemeSessionePiena)));
                    return;
                }
                o.richieste.retain(|r| r.da != id);
                let numero = s.numeratore;
                o.richieste.push(RichiestaRicevuta {
                    numero,
                    da: id,
                    scade: adesso.saturating_add(SCADENZA_RICHIESTA_MS),
                });
                s.numeratore = s.numeratore.saturating_add(1);
                // Nessun avviso: la richiesta compare nel pannello, con i suoi
                // due tasti, che è il posto dove si risponde.
            }
            _ => fx.push(manda(id, rifiutato(ErrorCodeKind::InsiemeRifiutato))),
        },
        Messaggio::NonChiedoPiu => {
            if let Fase::Ospita(o) = &mut s.fase {
                o.richieste.retain(|r| r.da != id);
            }
        }
        Messaggio::Ammesso { membro, diritti } => match &s.fase {
            Fase::Entrando { host, .. } if *host == id => {
                s.fase = Fase::Membro(Seguita {
                    host: id,
                    io: membro,
                    diritti,
                    persone: Vec::new(),
                    versione: None,
                    calibrazione: false,
                });
                // L'entrata del lettore per **ultima**: se non riesce torna
                // come `LettoreRifiuta`, e quel che ne segue (i ping spenti,
                // l'uscita) deve venire dopo gli effetti di qui, non prima.
                fx.push(Effetto::Sincronizza { id, attivo: true });
                fx.push(avviso(AVVISO_SEI_DENTRO, nome));
                fx.push(Effetto::EntraComeMembro { host: id });
            }
            // Già dentro con questo host: un doppione, niente da fare.
            Fase::Membro(seguita) if seguita.host == id => {}
            // Non lo si aspettava: vedi il `//!`.
            _ => fx.push(manda(id, Messaggio::Esco)),
        },
        Messaggio::Rifiutato { codice } => {
            if let Fase::Entrando { host, via, .. } = &s.fase
                && *host == id
            {
                // Un no semplice è una notizia: alla richiesta ha detto no la
                // persona, all'invito accettato il tempo (era già scaduto
                // dall'altra parte). Ogni altro perché — la sessione piena —
                // è il codice col suo testo.
                let chiave = match (codice == ErrorCodeKind::InsiemeRifiutato.code(), via) {
                    (true, Via::Richiesta) => AVVISO_RICHIESTA_RIFIUTATA,
                    (true, Via::Invito(_)) => AVVISO_INVITO_SCADUTO,
                    (false, _) => chiave_del_codice(&codice),
                };
                fx.push(avviso(chiave, nome));
                s.fase = Fase::Libera { inviti: Vec::new() };
            }
        }
        Messaggio::Esco => {
            if let Fase::Ospita(o) = &mut s.fase
                && let Some(membro) = togli_membro(o, id)
            {
                fx.push(Effetto::MembroUscito { membro });
                fx.push(Effetto::PersoneCambiate);
                fx.push(avviso(AVVISO_USCITO, nome));
            }
        }
        Messaggio::Fine { codice } => {
            let chiave = match codice.as_deref() {
                None => AVVISO_FINITA,
                Some(c) if c == CODICE_ALLONTANATO.code() => AVVISO_TI_HA_ALLONTANATO,
                Some(c) => chiave_del_codice(c),
            };
            match &s.fase {
                Fase::Membro(seguita) if seguita.host == id => {
                    fx.push(Effetto::Sincronizza { id, attivo: false });
                    fx.push(Effetto::EsciDalLettore);
                    fx.push(avviso(chiave, nome));
                    s.fase = Fase::Uscendo { spegni: false };
                }
                Fase::Entrando { host, .. } if *host == id => {
                    fx.push(avviso(chiave, nome));
                    s.fase = Fase::Libera { inviti: Vec::new() };
                }
                _ => {}
            }
        }
        Messaggio::Stato(stato) => {
            if let Fase::Membro(seguita) = &mut s.fase
                && seguita.host == id
                && seguita.versione.is_none_or(|v| stato.versione > v)
            {
                // Chi entra ed esce, visto da chi segue: dal secondo stato in
                // poi, perché il primo è la sessione com'era, non un ingresso.
                if seguita.versione.is_some() {
                    let altri = |persone: &[Persona]| -> Vec<Persona> {
                        persone
                            .iter()
                            .filter(|p| p.membro != 0 && p.membro != seguita.io)
                            .cloned()
                            .collect()
                    };
                    let prima = altri(&seguita.persone);
                    let dopo = altri(&stato.persone);
                    for p in dopo
                        .iter()
                        .filter(|p| !prima.iter().any(|q| q.membro == p.membro))
                    {
                        fx.push(avviso(AVVISO_ENTRATO, Some(p.nome.clone())));
                    }
                    for p in prima
                        .iter()
                        .filter(|p| !dopo.iter().any(|q| q.membro == p.membro))
                    {
                        fx.push(avviso(AVVISO_USCITO, Some(p.nome.clone())));
                    }
                }
                // Che cosa può fare questo PC: dalla sua `Persona`. Uno stato
                // che non lo nomina (un host a metà di un cambio) lo lascia
                // com'era.
                let diritti = stato
                    .persone
                    .iter()
                    .find(|p| p.membro == seguita.io)
                    .map_or(seguita.diritti, |p| p.diritti);
                if diritti != seguita.diritti {
                    seguita.diritti = diritti;
                    fx.push(Effetto::DirittiCambiati(diritti));
                    fx.push(avviso(avviso_dei_diritti(diritti), nome));
                }
                seguita.versione = Some(stato.versione);
                seguita.persone.clone_from(&stato.persone);
                fx.push(Effetto::Specchia(Box::new(stato)));
            }
        }
        Messaggio::Ancora(ancora) => {
            if let Fase::Membro(seguita) = &mut s.fase
                && seguita.host == id
            {
                // I clic della calibrazione si riconoscono dall'etichetta: è
                // l'unica voce che non sta in coda.
                let calibrazione = ancora.etichetta == UID_CALIBRAZIONE;
                if calibrazione != seguita.calibrazione {
                    seguita.calibrazione = calibrazione;
                    fx.push(if calibrazione {
                        avviso(AVVISO_CALIBRAZIONE_ACCESA, nome)
                    } else {
                        avviso(AVVISO_CALIBRAZIONE_SPENTA, None)
                    });
                }
                fx.push(Effetto::Segui(ancora));
            }
        }
        // Fase 2: il gesto di un membro, se il secchiello lo lascia passare.
        // Lo esegue il filo, negli stessi percorsi dei gesti di chi ospita, e
        // risponde lui con l'`Esito`.
        Messaggio::Comando { id: numero, azione } => {
            let Fase::Ospita(o) = &mut s.fase else {
                return;
            };
            let Some(m) = o.membri.iter_mut().find(|m| m.conn == id) else {
                return;
            };
            // Un gesto di una versione più nuova, che questa non sa fare: un
            // no detto con il suo perché, e la sessione va avanti. Prima del
            // secchiello, che è per i gesti veri.
            if !azione.conosciuta() {
                fx.push(manda(
                    id,
                    Messaggio::Esito {
                        id: numero,
                        codice: Some(ErrorCodeKind::InsiemeGestoNonDisponibile.code().to_owned()),
                    },
                ));
                return;
            }
            // Quel che chi ospita non gli ha concesso: un no col suo perché,
            // anche questo prima del secchiello. È la sola porta che conta —
            // il membro si ferma da sé, ma un membro di un'altra versione no —
            // e sta prima di `Esegui` anche per `Aggiungi`, che là
            // registrerebbe le voci e ne inoltrerebbe i gettoni.
            if !azione
                .ambito()
                .is_some_and(|ambito| m.diritti.concede(ambito))
            {
                fx.push(manda(
                    id,
                    Messaggio::Esito {
                        id: numero,
                        codice: Some(ErrorCodeKind::InsiemeNonConcesso.code().to_owned()),
                    },
                ));
                return;
            }
            if m.secchiello.prendi(adesso) {
                fx.push(Effetto::Esegui {
                    da: id,
                    membro: m.membro,
                    comando: numero,
                    azione: Box::new(azione),
                });
            } else {
                fx.push(manda(
                    id,
                    Messaggio::Esito {
                        id: numero,
                        codice: Some(ErrorCodeKind::InsiemeTroppiTentativi.code().to_owned()),
                    },
                ));
            }
        }
        // Un gesto di questo PC che l'host non ha fatto: il perché, come
        // guasto. Tranne il secchiello: chi tiene premuto un tasto lo sa già,
        // e una fascia rossa per ogni colpo in più sarebbe solo rumore.
        Messaggio::Esito {
            codice: Some(codice),
            ..
        } => {
            if matches!(&s.fase, Fase::Membro(seguita) if seguita.host == id)
                && codice != ErrorCodeKind::InsiemeTroppiTentativi.code()
            {
                fx.push(avviso(chiave_del_codice(&codice), None));
            }
        }
        Messaggio::Esito { codice: None, .. } | Messaggio::Sconosciuto => {}
    }
}

fn codice_arrivato(
    s: &mut Situazione,
    id: IdConnessione,
    codice: String,
    adesso: u64,
    fx: &mut Vec<Effetto>,
) {
    match &mut s.associazione {
        Some(a) if a.conn == id => a.codice = Some(codice),
        // Una per volta: la seconda si chiude con un no, che il trasporto
        // traduce in `codiceRifiutato` per l'altro capo.
        Some(_) => fx.push(Effetto::ConfermaCodice { id, uguale: false }),
        None => {
            s.associazione = Some(Associazione {
                conn: id,
                codice: Some(codice),
                confermato: false,
                scade: adesso.saturating_add(SCADENZA_ASSOCIAZIONE_MS),
            });
            // Il codice va letto su **tutti e due** gli schermi: chi ospita
            // può avere la finestra nel vassoio mentre l'altro preme «Associa».
            fx.push(Effetto::MostraFinestra);
        }
    }
}

fn pari_tolto(s: &mut Situazione, pari: i64, fx: &mut Vec<Effetto>) {
    let toccate: Vec<IdConnessione> = s
        .connessioni
        .iter()
        .filter(|(_, conn)| conn.pari == Some(pari))
        .map(|(id, _)| *id)
        .collect();
    for id in toccate {
        let nome = nome_di(s, id);
        if let Some(conn) = s.connessioni.get_mut(&id) {
            conn.pari = None;
        }
        match &mut s.fase {
            Fase::Libera { inviti } => inviti.retain(|i| i.da != id),
            Fase::Entrando { host, via, .. } if *host == id => {
                fx.push(manda(id, ritiro(*via)));
                s.fase = Fase::Libera { inviti: Vec::new() };
            }
            Fase::Ospita(o) => {
                o.inviti.retain(|i| i.a != id);
                o.richieste.retain(|r| r.da != id);
                if let Some(membro) = togli_membro(o, id) {
                    fx.push(manda(
                        id,
                        Messaggio::Fine {
                            codice: Some(ErrorCodeKind::InsiemeNonAssociato.code().to_owned()),
                        },
                    ));
                    fx.push(Effetto::MembroUscito { membro });
                    fx.push(Effetto::PersoneCambiate);
                    fx.push(avviso(AVVISO_ALLONTANATO, nome));
                }
            }
            Fase::Membro(seguita) if seguita.host == id => {
                fx.push(manda(id, Messaggio::Esco));
                fx.push(Effetto::Sincronizza { id, attivo: false });
                fx.push(Effetto::EsciDalLettore);
                fx.push(avviso(AVVISO_SEI_USCITO, None));
                s.fase = Fase::Uscendo { spegni: false };
            }
            _ => {}
        }
    }
}

fn tempo(s: &mut Situazione, adesso: u64, fx: &mut Vec<Effetto>) {
    if let Some(a) = &s.associazione
        && a.scade <= adesso
    {
        let id = a.conn;
        fx.push(Effetto::ConfermaCodice { id, uguale: false });
        fx.push(avviso(AVVISO_ASSOCIAZIONE_ANNULLATA, nome_di(s, id)));
        s.associazione = None;
    }
    let nomi: BTreeMap<IdConnessione, String> = s
        .connessioni
        .iter()
        .map(|(id, conn)| (*id, conn.nome.clone()))
        .collect();
    match &mut s.fase {
        Fase::Libera { inviti } => {
            for invito in inviti.iter().filter(|i| i.scade <= adesso) {
                fx.push(avviso(AVVISO_INVITO_SCADUTO, nomi.get(&invito.da).cloned()));
            }
            inviti.retain(|i| i.scade > adesso);
        }
        Fase::Entrando { host, via, scade } if *scade <= adesso => {
            let host = *host;
            fx.push(manda(host, ritiro(*via)));
            // Una richiesta senza risposta è la persona dall'altra parte che
            // non c'è; un invito accettato senza `Ammesso` è l'host che non
            // risponde più, cioè sparito.
            let chiave = match via {
                Via::Richiesta => AVVISO_RICHIESTA_SENZA_RISPOSTA,
                Via::Invito(_) => AVVISO_HOST_SPARITO,
            };
            fx.push(avviso(chiave, nomi.get(&host).cloned()));
            s.fase = Fase::Libera { inviti: Vec::new() };
        }
        Fase::Ospita(o) => {
            for invito in o.inviti.iter().filter(|i| i.scade <= adesso) {
                fx.push(avviso(
                    AVVISO_INVITO_SENZA_RISPOSTA,
                    nomi.get(&invito.a).cloned(),
                ));
            }
            o.inviti.retain(|i| i.scade > adesso);
            for richiesta in o.richieste.iter().filter(|r| r.scade <= adesso) {
                fx.push(manda(
                    richiesta.da,
                    rifiutato(ErrorCodeKind::InsiemeRifiutato),
                ));
                fx.push(avviso(
                    AVVISO_RICHIESTA_SCADUTA,
                    nomi.get(&richiesta.da).cloned(),
                ));
            }
            o.richieste.retain(|r| r.scade > adesso);
        }
        _ => {}
    }
}

fn comando(s: &mut Situazione, comando: Comando, adesso: u64, fx: &mut Vec<Effetto>) {
    if s.fase == Fase::Spenta {
        fx.push(errore(ErrorCode::InsiemeSpenta));
        return;
    }
    match comando {
        Comando::Ospita => match &s.fase {
            Fase::Libera { .. } => fx.push(Effetto::Ospita),
            Fase::Ospita(_) => {}
            _ => fx.push(errore(ErrorCode::InsiemeGiaInSessione)),
        },
        Comando::Termina => match &s.fase {
            Fase::Ospita(o) => {
                for m in &o.membri {
                    fx.push(manda(m.conn, Messaggio::Fine { codice: None }));
                }
                fx.push(Effetto::EsciDalLettore);
                fx.push(Effetto::SmettiDiOspitare);
                fx.push(avviso(AVVISO_CHIUSA, None));
                s.fase = Fase::Uscendo { spegni: false };
            }
            Fase::Membro(_) | Fase::Entrando { .. } => {
                fx.push(errore(non_disponibile("termina")));
            }
            _ => {}
        },
        // Per chi ospita «esci» è «termina»: non c'è una sessione senza di lui.
        Comando::Esci if matches!(s.fase, Fase::Ospita(_)) => {
            self::comando(s, Comando::Termina, adesso, fx);
        }
        Comando::Esci => match &s.fase {
            Fase::Membro(seguita) => {
                let host = seguita.host;
                fx.push(manda(host, Messaggio::Esco));
                fx.push(Effetto::Sincronizza {
                    id: host,
                    attivo: false,
                });
                fx.push(Effetto::EsciDalLettore);
                fx.push(avviso(AVVISO_SEI_USCITO, None));
                s.fase = Fase::Uscendo { spegni: false };
            }
            // «Annulla» mentre si entra: è un ritiro, non un errore, e non
            // merita un avviso — l'ha appena chiesto la persona.
            Fase::Entrando { host, via, .. } => {
                fx.push(manda(*host, ritiro(*via)));
                s.fase = Fase::Libera { inviti: Vec::new() };
            }
            _ => {}
        },
        Comando::Invita { pari, diritti } => {
            let Some(id) = s
                .connessioni
                .iter()
                .find(|(_, c)| c.pari == Some(pari) && c.in_entrata)
                .map(|(id, _)| *id)
            else {
                let codice = if matches!(s.fase, Fase::Ospita(_)) {
                    ErrorCode::InsiemeIrraggiungibile
                } else {
                    non_disponibile("invita")
                };
                fx.push(errore(codice));
                return;
            };
            let invito = s.numeratore;
            let Fase::Ospita(o) = &mut s.fase else {
                fx.push(errore(non_disponibile("invita")));
                return;
            };
            if o.membri.iter().any(|m| m.conn == id) {
                return;
            }
            if piena(o) {
                fx.push(errore(sessione_piena()));
                return;
            }
            // Un invito nuovo allo stesso PC sostituisce il vecchio: la sua
            // risposta, se arriva, non trova più il numero e vale un no.
            o.inviti.retain(|i| i.a != id);
            o.inviti.push(InvitoMandato {
                invito,
                a: id,
                scade: adesso.saturating_add(SCADENZA_INVITO_MS),
                diritti,
            });
            s.numeratore = s.numeratore.saturating_add(1);
            fx.push(manda(
                id,
                Messaggio::Invito {
                    invito,
                    scade_fra_ms: SCADENZA_INVITO_MS,
                },
            ));
            fx.push(avviso(AVVISO_INVITO_MANDATO, nome_di(s, id)));
        }
        Comando::Rispondi { invito, accetta } => {
            let Fase::Libera { inviti } = &mut s.fase else {
                fx.push(errore(ErrorCode::InsiemeGiaInSessione));
                return;
            };
            let Some(posto) = inviti
                .iter()
                .position(|i| i.numero == invito && i.scade > adesso)
            else {
                inviti.retain(|i| i.numero != invito);
                fx.push(errore(ErrorCode::InsiemeRifiutato));
                return;
            };
            let scelto = inviti.remove(posto);
            if !accetta {
                fx.push(manda(
                    scelto.da,
                    Messaggio::RispostaInvito {
                        invito: scelto.invito,
                        accetta: false,
                    },
                ));
                return;
            }
            // Accettarne uno rifiuta gli altri: vedi il `//!`.
            for altro in inviti.drain(..) {
                fx.push(manda(
                    altro.da,
                    Messaggio::RispostaInvito {
                        invito: altro.invito,
                        accetta: false,
                    },
                ));
            }
            fx.push(manda(
                scelto.da,
                Messaggio::RispostaInvito {
                    invito: scelto.invito,
                    accetta: true,
                },
            ));
            s.fase = Fase::Entrando {
                host: scelto.da,
                via: Via::Invito(scelto.invito),
                scade: adesso.saturating_add(ATTESA_AMMISSIONE_MS),
            };
        }
        Comando::Chiedi { pari } => {
            let Fase::Libera { inviti } = &mut s.fase else {
                fx.push(errore(ErrorCode::InsiemeGiaInSessione));
                return;
            };
            let Some(host) = s
                .connessioni
                .iter()
                .find(|(_, c)| c.pari == Some(pari) && c.ospita && !c.in_entrata)
                .map(|(id, _)| *id)
            else {
                fx.push(errore(ErrorCode::InsiemeIrraggiungibile));
                return;
            };
            for altro in inviti.drain(..) {
                fx.push(manda(
                    altro.da,
                    Messaggio::RispostaInvito {
                        invito: altro.invito,
                        accetta: false,
                    },
                ));
            }
            fx.push(manda(host, Messaggio::Chiedi));
            fx.push(avviso(AVVISO_RICHIESTA_MANDATA, nome_di(s, host)));
            s.fase = Fase::Entrando {
                host,
                via: Via::Richiesta,
                scade: adesso.saturating_add(SCADENZA_RICHIESTA_MS),
            };
        }
        Comando::Ammetti {
            richiesta,
            accetta,
            diritti,
        } => {
            let Fase::Ospita(o) = &mut s.fase else {
                fx.push(errore(non_disponibile("ammetti")));
                return;
            };
            let Some(posto) = o
                .richieste
                .iter()
                .position(|r| r.numero == richiesta && r.scade > adesso)
            else {
                o.richieste.retain(|r| r.numero != richiesta);
                fx.push(errore(ErrorCode::InsiemeRifiutato));
                return;
            };
            let scelta = o.richieste.remove(posto);
            if !accetta {
                fx.push(manda(scelta.da, rifiutato(ErrorCodeKind::InsiemeRifiutato)));
                return;
            }
            if !ammetti(s, scelta.da, diritti, fx) {
                fx.push(errore(sessione_piena()));
            }
        }
        Comando::Concedi { membro, diritti } => {
            let Fase::Ospita(o) = &mut s.fase else {
                fx.push(errore(non_disponibile("concedi")));
                return;
            };
            // Un membro già uscito: niente da cambiare, come per «Allontana».
            let Some(m) = o.membri.iter_mut().find(|m| m.membro == membro) else {
                return;
            };
            // Lo stesso livello due volte non è un cambio: niente stato in più,
            // e niente avviso a chi segue.
            if m.diritti != diritti {
                m.diritti = diritti;
                fx.push(Effetto::PersoneCambiate);
            }
        }
        Comando::Allontana { membro } => {
            let Fase::Ospita(o) = &mut s.fase else {
                fx.push(errore(non_disponibile("allontana")));
                return;
            };
            let Some(conn) = o.membri.iter().find(|m| m.membro == membro).map(|m| m.conn) else {
                return;
            };
            togli_membro(o, conn);
            fx.push(manda(
                conn,
                Messaggio::Fine {
                    codice: Some(CODICE_ALLONTANATO.code().to_owned()),
                },
            ));
            fx.push(Effetto::MembroUscito { membro });
            fx.push(Effetto::PersoneCambiate);
            fx.push(avviso(AVVISO_ALLONTANATO, nome_di(s, conn)));
        }
        Comando::Associa { id } => {
            let Some(conn) = s.connessioni.get(&id) else {
                fx.push(errore(ErrorCode::InsiemeIrraggiungibile));
                return;
            };
            // Già fidato: l'associazione c'è, e rifarla non serve.
            if conn.pari.is_some() {
                return;
            }
            // Una sola per volta, e vince l'ultima chiesta: quella di prima si
            // chiude con un no.
            if let Some(vecchia) = s.associazione.take()
                && vecchia.conn != id
            {
                fx.push(Effetto::ConfermaCodice {
                    id: vecchia.conn,
                    uguale: false,
                });
            }
            s.associazione = Some(Associazione {
                conn: id,
                codice: None,
                confermato: false,
                scade: adesso.saturating_add(SCADENZA_ASSOCIAZIONE_MS),
            });
            fx.push(Effetto::AssociaInRete { id });
        }
        Comando::ConfermaCodice {
            associazione,
            uguale,
        } => {
            let Some(a) = s
                .associazione
                .as_mut()
                .filter(|a| a.conn.0 == associazione && a.codice.is_some())
            else {
                fx.push(errore(ErrorCode::InsiemeCodiceRifiutato));
                return;
            };
            let id = a.conn;
            fx.push(Effetto::ConfermaCodice { id, uguale });
            if uguale {
                a.confermato = true;
            } else {
                s.associazione = None;
            }
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use rymembra_insieme::messaggi::Ripeti;

    const HOST: IdConnessione = IdConnessione(1);
    const MARCO: IdConnessione = IdConnessione(2);
    const LUCA: IdConnessione = IdConnessione(3);

    fn conn(pari: Option<i64>, nome: &str, ospita: bool, in_entrata: bool) -> Connessione {
        Connessione {
            chiave: ChiavePubblica([u8::try_from(pari.unwrap_or(0)).unwrap_or(0); 32]),
            nome: nome.to_owned(),
            dichiarato: nome.to_owned(),
            ospita,
            in_entrata,
            pari,
        }
    }

    /// Un PC acceso, collegato a un host fidato.
    fn libero_con_host() -> Situazione {
        let mut s = Situazione::default();
        (s, _) = passo(s, Evento::Acceso, 0);
        (s, _) = passo(
            s,
            Evento::Aperta {
                id: HOST,
                connessione: conn(Some(10), "Sala", true, false),
            },
            0,
        );
        s
    }

    #[test]
    fn un_associato_che_non_riconosce_piu_questo_pc_lo_si_dice_una_volta() {
        let s = libero_con_host();
        let (s, fx) = passo(
            s,
            Evento::Chiusa {
                id: HOST,
                codice: Some(ErrorCodeKind::InsiemeNonAssociato),
            },
            10,
        );
        assert_eq!(
            fx,
            vec![avviso(AVVISO_NON_TI_RICONOSCE, Some("Sala".to_owned()))]
        );
        // Ci riprova, e di nuovo no: niente secondo avviso.
        let (s, _) = passo(
            s,
            Evento::Aperta {
                id: IdConnessione(9),
                connessione: conn(Some(10), "Sala", true, false),
            },
            20,
        );
        let (s, fx) = passo(
            s,
            Evento::Chiusa {
                id: IdConnessione(9),
                codice: Some(ErrorCodeKind::InsiemeNonAssociato),
            },
            30,
        );
        assert!(fx.is_empty(), "{fx:?}");
        // Una chiusura qualunque non è un «non ti riconosco».
        let (_, fx) = passo(
            libero_con_host(),
            Evento::Chiusa {
                id: HOST,
                codice: Some(ErrorCodeKind::InsiemeConnessioneCaduta),
            },
            10,
        );
        assert!(fx.is_empty(), "{fx:?}");
        assert!(s.non_riconoscono.contains(&10));
    }

    /// Un PC che ospita, con `n` fidati collegati (pari 100.., conn 100..).
    fn host_con(n: u64) -> Situazione {
        let mut s = Situazione::default();
        (s, _) = passo(s, Evento::Acceso, 0);
        (s, _) = passo(s, Evento::Ospitato { porta: 47811 }, 0);
        for i in 0..n {
            let pari = i64::try_from(100 + i).unwrap();
            (s, _) = passo(
                s,
                Evento::Aperta {
                    id: IdConnessione(100 + i),
                    connessione: conn(Some(pari), &format!("pc{i}"), false, true),
                },
                0,
            );
        }
        s
    }

    fn mandati(fx: &[Effetto]) -> Vec<(IdConnessione, Messaggio)> {
        fx.iter()
            .filter_map(|e| match e {
                Effetto::Manda { a, messaggio } => Some((*a, messaggio.clone())),
                _ => None,
            })
            .collect()
    }

    fn avvisi(fx: &[Effetto]) -> Vec<String> {
        fx.iter()
            .filter_map(|e| match e {
                Effetto::Avviso { chiave, .. } => Some(chiave.clone()),
                _ => None,
            })
            .collect()
    }

    fn errori(fx: &[Effetto]) -> Vec<&'static str> {
        fx.iter()
            .filter_map(|e| match e {
                Effetto::Errore(err) => Some(err.code().kind().code()),
                _ => None,
            })
            .collect()
    }

    fn invita_e_accetta(
        s: Situazione,
        conn: IdConnessione,
        pari: i64,
    ) -> (Situazione, Vec<Effetto>) {
        invita_e_accetta_con(s, conn, pari, Diritti::Tutto)
    }

    /// Come [`invita_e_accetta`], con il livello scelto nell'invito.
    fn invita_e_accetta_con(
        s: Situazione,
        conn: IdConnessione,
        pari: i64,
        diritti: Diritti,
    ) -> (Situazione, Vec<Effetto>) {
        let (s, fx) = passo(s, Evento::Comando(Comando::Invita { pari, diritti }), 1_000);
        let invito = mandati(&fx)
            .into_iter()
            .find_map(|(_, m)| match m {
                Messaggio::Invito { invito, .. } => Some(invito),
                _ => None,
            })
            .expect("l'invito parte");
        passo(
            s,
            Evento::Ricevuto {
                id: conn,
                messaggio: Messaggio::RispostaInvito {
                    invito,
                    accetta: true,
                },
            },
            2_000,
        )
    }

    fn invito_arrivato(s: Situazione, da: IdConnessione, invito: u64, adesso: u64) -> Situazione {
        passo(
            s,
            Evento::Ricevuto {
                id: da,
                messaggio: Messaggio::Invito {
                    invito,
                    scade_fra_ms: SCADENZA_INVITO_MS,
                },
            },
            adesso,
        )
        .0
    }

    fn numeri_degli_inviti(s: &Situazione) -> Vec<u64> {
        match &s.fase {
            Fase::Libera { inviti } => inviti.iter().map(|i| i.numero).collect(),
            altro => panic!("non libera: {altro:?}"),
        }
    }

    /// Porta un PC libero dentro la sessione dell'host, per invito.
    fn membro() -> Situazione {
        let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
        let numero = numeri_degli_inviti(&s)[0];
        let (s, _) = passo(
            s,
            Evento::Comando(Comando::Rispondi {
                invito: numero,
                accetta: true,
            }),
            100,
        );
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: Messaggio::Ammesso {
                    membro: 3,
                    diritti: Diritti::Tutto,
                },
            },
            200,
        );
        assert!(fx.contains(&Effetto::EntraComeMembro { host: HOST }));
        assert!(matches!(&s.fase, Fase::Membro(m) if m.io == 3));
        s
    }

    #[test]
    fn l_host_che_sparisce_fa_uscire_chi_segue_con_l_avviso() {
        let s = membro();
        let (s, fx) = passo(
            s,
            Evento::Chiusa {
                id: HOST,
                codice: Some(ErrorCodeKind::InsiemeConnessioneCaduta),
            },
            5_000,
        );
        assert_eq!(s.fase, Fase::Uscendo { spegni: false });
        assert!(fx.contains(&Effetto::EsciDalLettore));
        assert_eq!(avvisi(&fx), vec![AVVISO_HOST_SPARITO]);
        // Niente messaggi a una connessione che non c'è più.
        assert!(mandati(&fx).is_empty());
        let (s, _) = passo(s, Evento::Uscito, 5_100);
        assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
    }

    #[test]
    fn l_host_che_sparisce_durante_l_ingresso_riporta_liberi() {
        let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
        let numero = numeri_degli_inviti(&s)[0];
        let (s, _) = passo(
            s,
            Evento::Comando(Comando::Rispondi {
                invito: numero,
                accetta: true,
            }),
            100,
        );
        assert!(matches!(s.fase, Fase::Entrando { .. }));
        let (s, fx) = passo(
            s,
            Evento::Chiusa {
                id: HOST,
                codice: None,
            },
            200,
        );
        assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
        assert_eq!(avvisi(&fx), vec![AVVISO_HOST_SPARITO]);
        assert!(
            !fx.contains(&Effetto::EsciDalLettore),
            "il lettore non era entrato"
        );
    }

    #[test]
    fn un_invito_scaduto_sparisce_e_la_risposta_tardiva_e_un_errore() {
        let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
        let numero = numeri_degli_inviti(&s)[0];
        assert_eq!(prossima_scadenza(&s), Some(SCADENZA_INVITO_MS));
        let (s, _) = passo(s, Evento::Tempo, SCADENZA_INVITO_MS - 1);
        assert_eq!(numeri_degli_inviti(&s), vec![numero], "non ancora");
        let (s, fx) = passo(s, Evento::Tempo, SCADENZA_INVITO_MS);
        assert!(numeri_degli_inviti(&s).is_empty());
        assert_eq!(avvisi(&fx), vec![AVVISO_INVITO_SCADUTO]);
        assert!(
            mandati(&fx).is_empty(),
            "all'host non si dice niente: {fx:?}"
        );
        let (s, fx) = passo(
            s,
            Evento::Comando(Comando::Rispondi {
                invito: numero,
                accetta: true,
            }),
            SCADENZA_INVITO_MS + 10,
        );
        assert_eq!(errori(&fx), vec!["insieme.rifiutato"]);
        assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
    }

    #[test]
    fn la_durata_di_un_invito_ha_il_tetto_di_chi_lo_riceve() {
        let s = passo(
            libero_con_host(),
            Evento::Ricevuto {
                id: HOST,
                messaggio: Messaggio::Invito {
                    invito: 1,
                    scade_fra_ms: 24 * 3_600_000,
                },
            },
            1_000,
        )
        .0;
        assert_eq!(prossima_scadenza(&s), Some(1_000 + SCADENZA_INVITO_MS));
    }

    #[test]
    fn dall_host_un_invito_scaduto_avvisa_e_il_si_tardivo_non_fa_entrare() {
        let s = host_con(1);
        let (s, fx) = passo(
            s,
            Evento::Comando(Comando::Invita {
                pari: 100,
                diritti: Diritti::Tutto,
            }),
            0,
        );
        let invito = match mandati(&fx).as_slice() {
            [(_, Messaggio::Invito { invito, .. })] => *invito,
            altro => panic!("{altro:?}"),
        };
        let (s, fx) = passo(s, Evento::Tempo, SCADENZA_INVITO_MS);
        assert_eq!(avvisi(&fx), vec![AVVISO_INVITO_SENZA_RISPOSTA]);
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(100),
                messaggio: Messaggio::RispostaInvito {
                    invito,
                    accetta: true,
                },
            },
            SCADENZA_INVITO_MS + 5,
        );
        assert_eq!(
            mandati(&fx),
            vec![(
                IdConnessione(100),
                Messaggio::Rifiutato {
                    codice: "insieme.rifiutato".to_owned()
                }
            )]
        );
        assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.is_empty()));
    }

    #[test]
    fn la_sessione_piena_dice_di_no_al_nono_membro() {
        // Otto membri più l'host fanno nove: il tetto.
        let mut s = host_con(9);
        for i in 0..8 {
            let (dopo, fx) =
                invita_e_accetta(s, IdConnessione(100 + i), i64::try_from(100 + i).unwrap());
            assert!(fx.contains(&Effetto::PersoneCambiate), "membro {i}");
            s = dopo;
        }
        assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.len() == 8));
        assert_eq!(persone_di_qui(&s, "io").len(), MASSIMO_PERSONE);
        // Invitare il nono è già un errore.
        let (s, fx) = passo(
            s,
            Evento::Comando(Comando::Invita {
                pari: 108,
                diritti: Diritti::Tutto,
            }),
            3_000,
        );
        assert_eq!(errori(&fx), vec!["insieme.sessionePiena"]);
        // E una richiesta riceve il no sul filo.
        let (_, fx) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(108),
                messaggio: Messaggio::Chiedi,
            },
            3_000,
        );
        assert_eq!(
            mandati(&fx),
            vec![(
                IdConnessione(108),
                Messaggio::Rifiutato {
                    codice: "insieme.sessionePiena".to_owned()
                }
            )]
        );
    }

    #[test]
    fn due_si_con_un_posto_solo_fanno_entrare_il_primo() {
        let mut s = host_con(9);
        for i in 0..7 {
            s = invita_e_accetta(s, IdConnessione(100 + i), i64::try_from(100 + i).unwrap()).0;
        }
        // Un posto libero, due inviti in volo.
        let (s, fx1) = passo(
            s,
            Evento::Comando(Comando::Invita {
                pari: 107,
                diritti: Diritti::Tutto,
            }),
            1_000,
        );
        let (s, fx2) = passo(
            s,
            Evento::Comando(Comando::Invita {
                pari: 108,
                diritti: Diritti::Tutto,
            }),
            1_000,
        );
        let numero = |fx: &[Effetto]| {
            mandati(fx)
                .into_iter()
                .find_map(|(_, m)| match m {
                    Messaggio::Invito { invito, .. } => Some(invito),
                    _ => None,
                })
                .unwrap()
        };
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(107),
                messaggio: Messaggio::RispostaInvito {
                    invito: numero(&fx1),
                    accetta: true,
                },
            },
            2_000,
        );
        assert!(fx.contains(&Effetto::PersoneCambiate));
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(108),
                messaggio: Messaggio::RispostaInvito {
                    invito: numero(&fx2),
                    accetta: true,
                },
            },
            2_000,
        );
        assert_eq!(
            mandati(&fx),
            vec![(
                IdConnessione(108),
                Messaggio::Rifiutato {
                    codice: "insieme.sessionePiena".to_owned()
                }
            )]
        );
        assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.len() == 8));
    }

    #[test]
    fn il_doppio_invito_dello_stesso_host_sostituisce_il_primo() {
        let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
        let s = invito_arrivato(s, HOST, 8, 1_000);
        let Fase::Libera { inviti } = &s.fase else {
            panic!()
        };
        assert_eq!(inviti.len(), 1);
        assert_eq!(inviti[0].invito, 8);
        assert_eq!(inviti[0].scade, 1_000 + SCADENZA_INVITO_MS);
    }

    #[test]
    fn dall_host_il_secondo_invito_annulla_il_primo() {
        let s = host_con(1);
        let (s, fx1) = passo(
            s,
            Evento::Comando(Comando::Invita {
                pari: 100,
                diritti: Diritti::Tutto,
            }),
            0,
        );
        let (s, _) = passo(
            s,
            Evento::Comando(Comando::Invita {
                pari: 100,
                diritti: Diritti::Tutto,
            }),
            10,
        );
        let Fase::Ospita(o) = &s.fase else { panic!() };
        assert_eq!(o.inviti.len(), 1);
        // La risposta al primo non fa entrare: il numero non vale più.
        let primo = match mandati(&fx1).as_slice() {
            [(_, Messaggio::Invito { invito, .. })] => *invito,
            altro => panic!("{altro:?}"),
        };
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(100),
                messaggio: Messaggio::RispostaInvito {
                    invito: primo,
                    accetta: true,
                },
            },
            20,
        );
        assert!(!fx.contains(&Effetto::PersoneCambiate));
        assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.is_empty()));
    }

    #[test]
    fn inviti_di_due_host_accettarne_uno_rifiuta_l_altro() {
        let mut s = libero_con_host();
        (s, _) = passo(
            s,
            Evento::Aperta {
                id: LUCA,
                connessione: conn(Some(11), "Studio", true, false),
            },
            0,
        );
        let s = invito_arrivato(s, HOST, 7, 0);
        let s = invito_arrivato(s, LUCA, 7, 0);
        let numeri = numeri_degli_inviti(&s);
        assert_eq!(numeri.len(), 2, "stesso numero da due host: due inviti");
        let (s, fx) = passo(
            s,
            Evento::Comando(Comando::Rispondi {
                invito: numeri[1],
                accetta: true,
            }),
            10,
        );
        assert_eq!(
            mandati(&fx),
            vec![
                (
                    HOST,
                    Messaggio::RispostaInvito {
                        invito: 7,
                        accetta: false
                    }
                ),
                (
                    LUCA,
                    Messaggio::RispostaInvito {
                        invito: 7,
                        accetta: true
                    }
                ),
            ]
        );
        assert!(matches!(s.fase, Fase::Entrando { host: LUCA, .. }));
    }

    #[test]
    fn uscire_durante_l_ingresso_e_poi_ricevere_ammesso_risponde_esco() {
        let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
        let numero = numeri_degli_inviti(&s)[0];
        let (s, _) = passo(
            s,
            Evento::Comando(Comando::Rispondi {
                invito: numero,
                accetta: true,
            }),
            100,
        );
        let (s, fx) = passo(s, Evento::Comando(Comando::Esci), 150);
        assert_eq!(mandati(&fx), vec![(HOST, Messaggio::Esco)]);
        assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
        // L'host aveva già ammesso: l'`Ammesso` arriva a un PC libero.
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: Messaggio::Ammesso {
                    membro: 1,
                    diritti: Diritti::Tutto,
                },
            },
            200,
        );
        assert_eq!(mandati(&fx), vec![(HOST, Messaggio::Esco)]);
        assert!(!fx.contains(&Effetto::EntraComeMembro { host: HOST }));
        assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
    }

    #[test]
    fn ritirare_una_richiesta_manda_non_chiedo_piu() {
        let s = libero_con_host();
        let (s, fx) = passo(s, Evento::Comando(Comando::Chiedi { pari: 10 }), 0);
        assert_eq!(mandati(&fx), vec![(HOST, Messaggio::Chiedi)]);
        let (s, fx) = passo(s, Evento::Comando(Comando::Esci), 10);
        assert_eq!(mandati(&fx), vec![(HOST, Messaggio::NonChiedoPiu)]);
        assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
    }

    #[test]
    fn una_richiesta_senza_risposta_scade_da_tutte_e_due_le_parti() {
        // Chi chiede.
        let (s, _) = passo(
            libero_con_host(),
            Evento::Comando(Comando::Chiedi { pari: 10 }),
            0,
        );
        let (s, fx) = passo(s, Evento::Tempo, SCADENZA_RICHIESTA_MS);
        assert_eq!(mandati(&fx), vec![(HOST, Messaggio::NonChiedoPiu)]);
        assert_eq!(avvisi(&fx), vec![AVVISO_RICHIESTA_SENZA_RISPOSTA]);
        assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
        // Chi ospita.
        let s = host_con(1);
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(100),
                messaggio: Messaggio::Chiedi,
            },
            0,
        );
        assert!(avvisi(&fx).is_empty(), "la richiesta sta nel pannello");
        let (s, fx) = passo(s, Evento::Tempo, SCADENZA_RICHIESTA_MS);
        assert_eq!(
            mandati(&fx),
            vec![(
                IdConnessione(100),
                Messaggio::Rifiutato {
                    codice: "insieme.rifiutato".to_owned()
                }
            )]
        );
        assert_eq!(avvisi(&fx), vec![AVVISO_RICHIESTA_SCADUTA]);
        assert!(matches!(&s.fase, Fase::Ospita(o) if o.richieste.is_empty()));
    }

    #[test]
    fn una_richiesta_accettata_fa_entrare() {
        let s = host_con(1);
        let (s, _) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(100),
                messaggio: Messaggio::Chiedi,
            },
            0,
        );
        let numero = match &s.fase {
            Fase::Ospita(o) => o.richieste[0].numero,
            _ => panic!(),
        };
        let (s, fx) = passo(
            s,
            Evento::Comando(Comando::Ammetti {
                richiesta: numero,
                accetta: true,
                diritti: Diritti::Comanda,
            }),
            10,
        );
        assert_eq!(
            mandati(&fx),
            vec![(
                IdConnessione(100),
                Messaggio::Ammesso {
                    membro: 1,
                    diritti: Diritti::Comanda,
                }
            )]
        );
        assert_eq!(avvisi(&fx), vec![AVVISO_ENTRATO]);
        assert_eq!(
            persone_di_qui(&s, "Sala"),
            vec![
                Persona {
                    membro: 0,
                    nome: "Sala".to_owned(),
                    diritti: Diritti::Tutto,
                },
                Persona {
                    membro: 1,
                    nome: "pc0".to_owned(),
                    diritti: Diritti::Comanda,
                }
            ]
        );
    }

    #[test]
    fn il_fidato_bloccato_mentre_e_invitato_non_entra_piu() {
        let s = host_con(1);
        let (s, fx) = passo(
            s,
            Evento::Comando(Comando::Invita {
                pari: 100,
                diritti: Diritti::Tutto,
            }),
            0,
        );
        let invito = match mandati(&fx).as_slice() {
            [(_, Messaggio::Invito { invito, .. })] => *invito,
            altro => panic!("{altro:?}"),
        };
        let (s, _) = passo(s, Evento::PariTolto { pari: 100 }, 10);
        assert!(matches!(&s.fase, Fase::Ospita(o) if o.inviti.is_empty()));
        // Il suo sì era già in volo: non fa entrare, e non lo si legge
        // nemmeno, perché la chiave non è più fidata.
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(100),
                messaggio: Messaggio::RispostaInvito {
                    invito,
                    accetta: true,
                },
            },
            20,
        );
        assert!(fx.is_empty(), "{fx:?}");
        assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.is_empty()));
    }

    #[test]
    fn il_fidato_bloccato_mentre_e_in_sessione_esce_con_fine() {
        let s = invita_e_accetta(host_con(2), IdConnessione(100), 100).0;
        let s = invita_e_accetta(s, IdConnessione(101), 101).0;
        let (s, fx) = passo(s, Evento::PariTolto { pari: 100 }, 3_000);
        assert_eq!(
            mandati(&fx),
            vec![(
                IdConnessione(100),
                Messaggio::Fine {
                    codice: Some("insieme.nonAssociato".to_owned())
                }
            )]
        );
        assert!(fx.contains(&Effetto::PersoneCambiate));
        assert_eq!(connessioni_dei_membri(&s), vec![IdConnessione(101)]);
        // La rete poi chiude la connessione: niente da rifare.
        let (_, fx) = passo(
            s,
            Evento::Chiusa {
                id: IdConnessione(100),
                codice: None,
            },
            3_100,
        );
        assert!(fx.is_empty(), "{fx:?}");
    }

    #[test]
    fn chi_segue_e_blocca_l_host_esce_dalla_sessione() {
        let s = membro();
        let (s, fx) = passo(s, Evento::PariTolto { pari: 10 }, 1_000);
        assert_eq!(mandati(&fx), vec![(HOST, Messaggio::Esco)]);
        assert!(fx.contains(&Effetto::EsciDalLettore));
        assert_eq!(s.fase, Fase::Uscendo { spegni: false });
    }

    #[test]
    fn i_messaggi_di_un_pc_non_fidato_non_contano() {
        let mut s = Situazione::default();
        (s, _) = passo(s, Evento::Acceso, 0);
        (s, _) = passo(
            s,
            Evento::Aperta {
                id: MARCO,
                connessione: conn(None, "Sconosciuto", true, false),
            },
            0,
        );
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: MARCO,
                messaggio: Messaggio::Invito {
                    invito: 1,
                    scade_fra_ms: 1_000,
                },
            },
            0,
        );
        assert!(fx.is_empty());
        assert_eq!(numeri_degli_inviti(&s), Vec::<u64>::new());
    }

    #[test]
    fn lo_stato_vecchio_si_scarta_e_l_ancora_passa_solo_dall_host() {
        let s = membro();
        let stato = |versione| {
            Messaggio::Stato(StatoSessione {
                versione,
                persone: vec![Persona {
                    membro: 0,
                    nome: "Sala".to_owned(),
                    diritti: Diritti::Tutto,
                }],
                coda: vec![],
                corrente: None,
                in_pausa: true,
                ripeti: Ripeti::No,
            })
        };
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: stato(5),
            },
            0,
        );
        assert_eq!(fx.len(), 1);
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: stato(4),
            },
            0,
        );
        assert!(fx.is_empty(), "più vecchio");
        let ancora = AncoraSulFilo {
            etichetta: 1,
            posizione_us: 0,
            udito_ns: 0,
            in_corso: true,
            programmata: false,
        };
        let (_, fx) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: Messaggio::Ancora(ancora),
            },
            0,
        );
        assert_eq!(fx, vec![Effetto::Segui(ancora)]);
    }

    #[test]
    fn la_fine_dall_host_dice_perche() {
        for (codice, chiave) in [
            (None, AVVISO_FINITA),
            (Some("insieme.rifiutato"), AVVISO_TI_HA_ALLONTANATO),
            (Some("insieme.nonAssociato"), "errors.insieme.nonAssociato"),
            // Un codice d'un altro dominio non passa: vale un rifiuto.
            (Some("db.corrupt"), "errors.insieme.rifiutato"),
        ] {
            let (s, fx) = passo(
                membro(),
                Evento::Ricevuto {
                    id: HOST,
                    messaggio: Messaggio::Fine {
                        codice: codice.map(ToOwned::to_owned),
                    },
                },
                0,
            );
            assert_eq!(avvisi(&fx), vec![chiave], "{codice:?}");
            assert_eq!(s.fase, Fase::Uscendo { spegni: false });
        }
    }

    #[test]
    fn terminare_manda_fine_a_tutti_e_spegnere_in_sessione_aspetta_il_lettore() {
        let s = invita_e_accetta(host_con(2), IdConnessione(100), 100).0;
        let s = invita_e_accetta(s, IdConnessione(101), 101).0;
        let (t, fx) = passo(s.clone(), Evento::Comando(Comando::Termina), 0);
        assert_eq!(
            mandati(&fx),
            vec![
                (IdConnessione(100), Messaggio::Fine { codice: None }),
                (IdConnessione(101), Messaggio::Fine { codice: None }),
            ]
        );
        assert!(fx.contains(&Effetto::SmettiDiOspitare));
        assert_eq!(
            passo(t, Evento::Uscito, 1).0.fase,
            Fase::Libera { inviti: vec![] }
        );

        let (t, fx) = passo(s, Evento::Spegni, 0);
        assert_eq!(fx.last(), Some(&Effetto::FermaRete));
        assert_eq!(t.fase, Fase::Uscendo { spegni: true });
        assert_eq!(passo(t, Evento::Uscito, 1).0.fase, Fase::Spenta);
    }

    #[test]
    fn da_spenta_ogni_comando_risponde_spenta() {
        let (_, fx) = passo(Situazione::default(), Evento::Comando(Comando::Ospita), 0);
        assert_eq!(errori(&fx), vec!["insieme.spenta"]);
    }

    #[test]
    fn l_associazione_scade_e_una_seconda_si_rifiuta() {
        let mut s = Situazione::default();
        (s, _) = passo(s, Evento::Acceso, 0);
        (s, _) = passo(
            s,
            Evento::Aperta {
                id: MARCO,
                connessione: conn(None, "Marco", true, false),
            },
            0,
        );
        let (s, fx) = passo(s, Evento::Comando(Comando::Associa { id: MARCO }), 0);
        assert_eq!(fx, vec![Effetto::AssociaInRete { id: MARCO }]);
        let (s, fx) = passo(
            s,
            Evento::Codice {
                id: MARCO,
                codice: "4821".to_owned(),
            },
            10,
        );
        assert!(fx.is_empty());
        // Un secondo codice su un'altra connessione: no.
        let (s, fx) = passo(
            s,
            Evento::Codice {
                id: LUCA,
                codice: "1111".to_owned(),
            },
            20,
        );
        assert_eq!(
            fx,
            vec![Effetto::ConfermaCodice {
                id: LUCA,
                uguale: false
            }]
        );
        let (s, fx) = passo(s, Evento::Tempo, SCADENZA_ASSOCIAZIONE_MS);
        assert_eq!(
            fx,
            vec![
                Effetto::ConfermaCodice {
                    id: MARCO,
                    uguale: false
                },
                avviso(AVVISO_ASSOCIAZIONE_ANNULLATA, Some("Marco".to_owned())),
            ]
        );
        assert_eq!(s.associazione, None);
    }

    #[test]
    fn l_associazione_confermata_e_riuscita_rende_fidato() {
        let mut s = Situazione::default();
        (s, _) = passo(s, Evento::Acceso, 0);
        (s, _) = passo(
            s,
            Evento::Aperta {
                id: MARCO,
                connessione: conn(None, "Marco", false, true),
            },
            0,
        );
        // Chi ospita non chiama `associa`: gli arriva il codice.
        let (s, fx) = passo(
            s,
            Evento::Codice {
                id: MARCO,
                codice: "0042".to_owned(),
            },
            0,
        );
        assert_eq!(fx, vec![Effetto::MostraFinestra]);
        let (s, fx) = passo(
            s,
            Evento::Comando(Comando::ConfermaCodice {
                associazione: MARCO.0,
                uguale: true,
            }),
            5,
        );
        assert_eq!(
            fx,
            vec![Effetto::ConfermaCodice {
                id: MARCO,
                uguale: true
            }]
        );
        let (s, fx) = passo(
            s,
            Evento::Associato {
                id: MARCO,
                pari: 42,
            },
            9,
        );
        assert_eq!(avvisi(&fx), vec![AVVISO_ASSOCIATO]);
        assert_eq!(s.connessioni[&MARCO].pari, Some(42));
        assert_eq!(s.associazione, None);
    }

    #[test]
    fn il_gesto_di_un_membro_si_esegue_finche_il_secchiello_lo_lascia_passare() {
        let mut s = invita_e_accetta(host_con(1), IdConnessione(100), 100).0;
        let comando = |numero| Evento::Ricevuto {
            id: IdConnessione(100),
            messaggio: Messaggio::Comando {
                id: numero,
                azione: Azione::Pausa,
            },
        };
        for numero in 0..10 {
            let (dopo, fx) = passo(s, comando(numero), 5_000);
            assert_eq!(
                fx,
                vec![Effetto::Esegui {
                    da: IdConnessione(100),
                    membro: 1,
                    comando: numero,
                    azione: Box::new(Azione::Pausa),
                }]
            );
            s = dopo;
        }
        // L'undicesimo nello stesso istante: no, col perché.
        let (s, fx) = passo(s, comando(10), 5_000);
        assert_eq!(
            fx,
            vec![manda(
                IdConnessione(100),
                Messaggio::Esito {
                    id: 10,
                    codice: Some("insieme.troppiTentativi".to_owned())
                }
            )]
        );
        // Un passo dopo, di nuovo.
        let (_, fx) = passo(s, comando(11), 5_100);
        assert!(matches!(
            fx.as_slice(),
            [Effetto::Esegui { comando: 11, .. }]
        ));
    }

    #[test]
    fn un_gesto_di_una_versione_piu_nuova_ha_il_suo_no_e_non_tocca_il_secchiello() {
        let s = invita_e_accetta(host_con(1), IdConnessione(100), 100).0;
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(100),
                messaggio: Messaggio::Comando {
                    id: 4,
                    azione: Azione::Sconosciuta,
                },
            },
            5_000,
        );
        assert_eq!(
            fx,
            vec![manda(
                IdConnessione(100),
                Messaggio::Esito {
                    id: 4,
                    codice: Some("insieme.gestoNonDisponibile".to_owned())
                }
            )]
        );
        assert!(matches!(s.fase, Fase::Ospita(_)), "la sessione continua");
    }

    #[test]
    fn un_comando_da_chi_non_e_membro_non_si_esegue() {
        let s = host_con(1);
        let (_, fx) = passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(100),
                messaggio: Messaggio::Comando {
                    id: 1,
                    azione: Azione::Pausa,
                },
            },
            0,
        );
        assert!(fx.is_empty(), "{fx:?}");
    }

    #[test]
    fn l_esito_negativo_dall_host_e_un_avviso_tranne_il_secchiello() {
        let esito = |codice: Option<&str>| Evento::Ricevuto {
            id: HOST,
            messaggio: Messaggio::Esito {
                id: 1,
                codice: codice.map(ToOwned::to_owned),
            },
        };
        let (s, fx) = passo(membro(), esito(Some("insieme.gestoNonDisponibile")), 0);
        assert_eq!(avvisi(&fx), vec!["errors.insieme.gestoNonDisponibile"]);
        let (s, fx) = passo(s, esito(Some("insieme.troppiTentativi")), 0);
        assert!(fx.is_empty());
        let (_, fx) = passo(s, esito(None), 0);
        assert!(fx.is_empty());
    }

    #[test]
    fn le_voci_di_un_membro_escono_con_lui_in_ogni_modo_in_cui_esce() {
        let due = || {
            let s = invita_e_accetta(host_con(2), IdConnessione(100), 100).0;
            invita_e_accetta(s, IdConnessione(101), 101).0
        };
        let uscito = |fx: &[Effetto]| {
            fx.iter()
                .filter_map(|e| match e {
                    Effetto::MembroUscito { membro } => Some(*membro),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let (_, fx) = passo(
            due(),
            Evento::Ricevuto {
                id: IdConnessione(101),
                messaggio: Messaggio::Esco,
            },
            0,
        );
        assert_eq!(uscito(&fx), vec![2]);
        let (_, fx) = passo(
            due(),
            Evento::Chiusa {
                id: IdConnessione(100),
                codice: None,
            },
            0,
        );
        assert_eq!(uscito(&fx), vec![1]);
        let (_, fx) = passo(due(), Evento::Comando(Comando::Allontana { membro: 2 }), 0);
        assert_eq!(uscito(&fx), vec![2]);
        let (_, fx) = passo(due(), Evento::PariTolto { pari: 100 }, 0);
        assert_eq!(uscito(&fx), vec![1]);
    }

    #[test]
    fn allontanare_un_membro_gli_manda_fine_col_perche() {
        let s = invita_e_accetta(host_con(1), IdConnessione(100), 100).0;
        let (s, fx) = passo(s, Evento::Comando(Comando::Allontana { membro: 1 }), 0);
        assert_eq!(
            mandati(&fx),
            vec![(
                IdConnessione(100),
                Messaggio::Fine {
                    codice: Some("insieme.rifiutato".to_owned())
                }
            )]
        );
        assert!(connessioni_dei_membri(&s).is_empty());
    }

    #[test]
    fn chiave_del_codice_resta_nel_dominio_insieme() {
        assert_eq!(
            chiave_del_codice("insieme.sessionePiena"),
            "errors.insieme.sessionePiena"
        );
        assert_eq!(chiave_del_codice("db.corrupt"), "errors.insieme.rifiutato");
        assert_eq!(
            chiave_del_codice("insieme.\nqualcosa"),
            "errors.insieme.rifiutato"
        );
    }

    fn stato_con(versione: u64, membri: &[(u32, &str)]) -> Messaggio {
        let mut persone = vec![Persona {
            membro: 0,
            nome: "Sala".to_owned(),
            diritti: Diritti::Tutto,
        }];
        persone.extend(membri.iter().map(|(membro, nome)| Persona {
            membro: *membro,
            nome: (*nome).to_owned(),
            diritti: Diritti::Tutto,
        }));
        Messaggio::Stato(StatoSessione {
            versione,
            persone,
            coda: vec![],
            corrente: None,
            in_pausa: false,
            ripeti: Ripeti::No,
        })
    }

    #[test]
    fn chi_segue_sente_chi_entra_e_chi_esce_ma_non_se_stesso() {
        // Questo PC è il membro 3 (vedi `membro`).
        let s = membro();
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: stato_con(1, &[(1, "Marco"), (3, "Io")]),
            },
            0,
        );
        assert!(
            avvisi(&fx).is_empty(),
            "il primo stato è la sessione com'era"
        );
        let (_, fx) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: stato_con(2, &[(3, "Io"), (4, "Luca")]),
            },
            0,
        );
        assert_eq!(
            fx.iter()
                .filter_map(|e| match e {
                    Effetto::Avviso { chiave, nome } => Some((chiave.as_str(), nome.as_deref())),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![
                (AVVISO_ENTRATO, Some("Luca")),
                (AVVISO_USCITO, Some("Marco"))
            ]
        );
    }

    #[test]
    fn la_calibrazione_dell_host_si_annuncia_a_chi_segue() {
        let ancora = |etichetta| Evento::Ricevuto {
            id: HOST,
            messaggio: Messaggio::Ancora(AncoraSulFilo {
                etichetta,
                posizione_us: 0,
                udito_ns: 0,
                in_corso: true,
                programmata: false,
            }),
        };
        let (s, fx) = passo(membro(), ancora(UID_CALIBRAZIONE), 0);
        assert_eq!(avvisi(&fx), vec![AVVISO_CALIBRAZIONE_ACCESA]);
        let (s, fx) = passo(s, ancora(UID_CALIBRAZIONE), 0);
        assert!(avvisi(&fx).is_empty(), "una volta sola");
        let (_, fx) = passo(s, ancora(5), 0);
        assert_eq!(avvisi(&fx), vec![AVVISO_CALIBRAZIONE_SPENTA]);
    }

    #[test]
    fn esci_mentre_si_entra_e_un_annulla_senza_errori() {
        let (s, _) = passo(
            libero_con_host(),
            Evento::Comando(Comando::Chiedi { pari: 10 }),
            0,
        );
        let (s, fx) = passo(s, Evento::Comando(Comando::Esci), 5);
        assert!(errori(&fx).is_empty());
        assert!(avvisi(&fx).is_empty());
        assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
    }

    #[test]
    fn un_no_dell_host_e_una_notizia_la_sessione_piena_un_guasto() {
        for (via_richiesta, codice, chiave) in [
            (true, "insieme.rifiutato", AVVISO_RICHIESTA_RIFIUTATA),
            (false, "insieme.rifiutato", AVVISO_INVITO_SCADUTO),
            (
                true,
                "insieme.sessionePiena",
                "errors.insieme.sessionePiena",
            ),
        ] {
            let s = if via_richiesta {
                passo(
                    libero_con_host(),
                    Evento::Comando(Comando::Chiedi { pari: 10 }),
                    0,
                )
                .0
            } else {
                let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
                let numero = numeri_degli_inviti(&s)[0];
                passo(
                    s,
                    Evento::Comando(Comando::Rispondi {
                        invito: numero,
                        accetta: true,
                    }),
                    0,
                )
                .0
            };
            let (s, fx) = passo(
                s,
                Evento::Ricevuto {
                    id: HOST,
                    messaggio: Messaggio::Rifiutato {
                        codice: codice.to_owned(),
                    },
                },
                1,
            );
            assert_eq!(avvisi(&fx), vec![chiave], "{codice}");
            assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
        }
    }

    #[test]
    fn ogni_chiave_d_avviso_e_una_di_quelle_della_finestra() {
        // `richieste-interfaccia.md` §5: le sole che la finestra traduce.
        let note = [
            "hosting",
            "joinedSession",
            "joined",
            "left",
            "removed",
            "youWereRemoved",
            "youLeft",
            "ended",
            "closed",
            "hostGone",
            "inviteSent",
            "inviteDeclined",
            "inviteUnanswered",
            "inviteExpired",
            "requestSent",
            "requestDeclined",
            "requestUnanswered",
            "requestExpired",
            "paired",
            "pairingCancelled",
            "calibrationOn",
            "calibrationOff",
            "forgottenBy",
            "rightsListen",
            "rightsControl",
            "rightsAll",
        ];
        for chiave in [
            AVVISO_SI_OSPITA,
            AVVISO_SEI_DENTRO,
            AVVISO_ENTRATO,
            AVVISO_USCITO,
            AVVISO_ALLONTANATO,
            AVVISO_TI_HA_ALLONTANATO,
            AVVISO_SEI_USCITO,
            AVVISO_FINITA,
            AVVISO_CHIUSA,
            AVVISO_HOST_SPARITO,
            AVVISO_INVITO_MANDATO,
            AVVISO_INVITO_RIFIUTATO,
            AVVISO_INVITO_SENZA_RISPOSTA,
            AVVISO_INVITO_SCADUTO,
            AVVISO_RICHIESTA_MANDATA,
            AVVISO_RICHIESTA_RIFIUTATA,
            AVVISO_RICHIESTA_SENZA_RISPOSTA,
            AVVISO_RICHIESTA_SCADUTA,
            AVVISO_ASSOCIATO,
            AVVISO_ASSOCIAZIONE_ANNULLATA,
            AVVISO_CALIBRAZIONE_ACCESA,
            AVVISO_CALIBRAZIONE_SPENTA,
            AVVISO_NON_TI_RICONOSCE,
            AVVISO_DIRITTI_ASCOLTA,
            AVVISO_DIRITTI_COMANDA,
            AVVISO_DIRITTI_TUTTO,
        ] {
            let resto = chiave.strip_prefix("together.notice.").expect(chiave);
            assert!(note.contains(&resto), "{chiave}");
        }
    }

    #[test]
    fn il_lettore_che_non_entra_fa_uscire_e_spegne_i_ping_dopo_averli_accesi() {
        let s = membro();
        let (s, fx) = passo(s, Evento::LettoreRifiuta, 300);
        assert_eq!(
            fx,
            vec![
                manda(HOST, Messaggio::Esco),
                Effetto::Sincronizza {
                    id: HOST,
                    attivo: false
                },
                Effetto::EsciDalLettore,
            ]
        );
        assert_eq!(s.fase, Fase::Uscendo { spegni: false });
        // E nel passo che fa entrare, l'entrata del lettore è l'ultimo
        // effetto: i ping spenti dal rifiuto vengono dopo quelli accesi.
        let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
        let numero = numeri_degli_inviti(&s)[0];
        let (s, _) = passo(
            s,
            Evento::Comando(Comando::Rispondi {
                invito: numero,
                accetta: true,
            }),
            100,
        );
        let (_, fx) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: Messaggio::Ammesso {
                    membro: 1,
                    diritti: Diritti::Tutto,
                },
            },
            200,
        );
        assert_eq!(fx.last(), Some(&Effetto::EntraComeMembro { host: HOST }));
    }

    // ── che cosa può fare chi entra ─────────────────────────────────────────

    fn diritti_di(s: &Situazione, membro: u32) -> Option<Diritti> {
        persone_di_qui(s, "Sala")
            .into_iter()
            .find(|p| p.membro == membro)
            .map(|p| p.diritti)
    }

    fn gesto(numero: u32, azione: Azione) -> Evento {
        Evento::Ricevuto {
            id: IdConnessione(100),
            messaggio: Messaggio::Comando { id: numero, azione },
        }
    }

    fn non_concesso(numero: u32) -> Effetto {
        manda(
            IdConnessione(100),
            Messaggio::Esito {
                id: numero,
                codice: Some("insieme.nonConcesso".to_owned()),
            },
        )
    }

    fn concedi(membro: u32, diritti: Diritti) -> Evento {
        Evento::Comando(Comando::Concedi { membro, diritti })
    }

    #[test]
    fn l_invito_porta_il_suo_livello_fino_all_ammesso() {
        for diritti in [Diritti::Ascolta, Diritti::Comanda, Diritti::Tutto] {
            let (s, fx) = invita_e_accetta_con(host_con(1), IdConnessione(100), 100, diritti);
            assert_eq!(
                mandati(&fx),
                vec![(
                    IdConnessione(100),
                    Messaggio::Ammesso { membro: 1, diritti }
                )]
            );
            assert_eq!(diritti_di(&s, 1), Some(diritti));
            assert_eq!(diritti_di(&s, 0), Some(Diritti::Tutto), "chi ospita");
        }
    }

    /// Un invito e una richiesta che si incrociano non sono un secondo sì:
    /// il PC resta col livello con cui è entrato.
    #[test]
    fn rientrare_una_seconda_volta_non_cambia_il_livello() {
        let mut s = invita_e_accetta_con(host_con(1), IdConnessione(100), 100, Diritti::Ascolta).0;
        let mut fx = Vec::new();
        assert!(ammetti(&mut s, IdConnessione(100), Diritti::Tutto, &mut fx));
        assert_eq!(
            mandati(&fx),
            vec![(
                IdConnessione(100),
                Messaggio::Ammesso {
                    membro: 1,
                    diritti: Diritti::Ascolta
                }
            )]
        );
        assert_eq!(diritti_di(&s, 1), Some(Diritti::Ascolta));
    }

    #[test]
    fn concedere_cambia_il_livello_e_rimanda_lo_stato_una_volta() {
        let s = invita_e_accetta_con(host_con(1), IdConnessione(100), 100, Diritti::Ascolta).0;
        let (s, fx) = passo(s, concedi(1, Diritti::Tutto), 3_000);
        assert_eq!(fx, vec![Effetto::PersoneCambiate]);
        assert_eq!(diritti_di(&s, 1), Some(Diritti::Tutto));
        // Lo stesso livello di nuovo: niente da rimandare.
        let (s, fx) = passo(s, concedi(1, Diritti::Tutto), 3_100);
        assert!(fx.is_empty(), "{fx:?}");
        // Un membro che non c'è più: niente, come per «Allontana».
        let (_, fx) = passo(s, concedi(9, Diritti::Ascolta), 3_200);
        assert!(fx.is_empty(), "{fx:?}");
        // Fuori dalla sessione non c'è niente da concedere.
        let (_, fx) = passo(libero_con_host(), concedi(1, Diritti::Tutto), 0);
        assert_eq!(errori(&fx), vec!["insieme.gestoNonDisponibile"]);
    }

    /// La porta è qui: chi ascolta non comanda e chi comanda non tocca la
    /// coda, e i no non costano gettoni del secchiello.
    #[test]
    fn la_porta_dell_host_ferma_quel_che_non_e_concesso_prima_del_secchiello() {
        let mut s = invita_e_accetta_con(host_con(1), IdConnessione(100), 100, Diritti::Ascolta).0;
        let aggiungi = Azione::Aggiungi {
            voci: vec![],
            dove: rymembra_insieme::messaggi::Dove::InFondo,
            suona: false,
        };
        // Venti no nello stesso istante: tutti col perché, nessuno eseguito.
        for numero in 0..20 {
            let (dopo, fx) = passo(s, gesto(numero, Azione::Pausa), 5_000);
            assert_eq!(fx, vec![non_concesso(numero)]);
            s = dopo;
        }
        let (s, fx) = passo(s, gesto(20, aggiungi.clone()), 5_000);
        assert_eq!(fx, vec![non_concesso(20)], "nessuna voce registrata");
        // Comanda: la pausa sì, dieci di fila, perché il secchiello è pieno.
        let (mut s, _) = passo(s, concedi(1, Diritti::Comanda), 5_000);
        for numero in 30..40 {
            let (dopo, fx) = passo(s, gesto(numero, Azione::Pausa), 5_000);
            assert!(
                matches!(fx.as_slice(), [Effetto::Esegui { comando, .. }] if *comando == numero),
                "{fx:?}"
            );
            s = dopo;
        }
        // Ma non la coda.
        let (s, fx) = passo(s, gesto(40, Azione::Vai { uid: 7 }), 5_100);
        assert_eq!(fx, vec![non_concesso(40)]);
        let (s, fx) = passo(s, gesto(41, aggiungi), 5_100);
        assert_eq!(fx, vec![non_concesso(41)]);
        // Tutto: anche la coda.
        let (s, _) = passo(s, concedi(1, Diritti::Tutto), 5_200);
        let (_, fx) = passo(s, gesto(42, Azione::Vai { uid: 7 }), 5_200);
        assert!(
            matches!(fx.as_slice(), [Effetto::Esegui { comando: 42, .. }]),
            "{fx:?}"
        );
    }

    #[test]
    fn chi_segue_sente_il_livello_nuovo_una_volta_sola() {
        // Questo PC è il membro 3, entrato con `Tutto` (vedi `membro`).
        let stato = |versione, diritti| {
            Messaggio::Stato(StatoSessione {
                versione,
                persone: vec![
                    Persona {
                        membro: 0,
                        nome: "Sala".to_owned(),
                        diritti: Diritti::Tutto,
                    },
                    Persona {
                        membro: 3,
                        nome: "Io".to_owned(),
                        diritti,
                    },
                ],
                coda: vec![],
                corrente: None,
                in_pausa: false,
                ripeti: Ripeti::No,
            })
        };
        let arriva = |s, messaggio| {
            passo(
                s,
                Evento::Ricevuto {
                    id: HOST,
                    messaggio,
                },
                0,
            )
        };
        let (s, fx) = arriva(membro(), stato(1, Diritti::Tutto));
        assert!(!fx.iter().any(|e| matches!(e, Effetto::DirittiCambiati(_))));
        assert!(avvisi(&fx).is_empty());
        let (s, fx) = arriva(s, stato(2, Diritti::Ascolta));
        assert!(fx.contains(&Effetto::DirittiCambiati(Diritti::Ascolta)));
        assert!(fx.contains(&Effetto::Avviso {
            chiave: AVVISO_DIRITTI_ASCOLTA.to_owned(),
            nome: Some("Sala".to_owned()),
        }));
        assert!(matches!(&s.fase, Fase::Membro(m) if m.diritti == Diritti::Ascolta));
        // Lo stesso livello nello stato dopo: niente.
        let (s, fx) = arriva(s, stato(3, Diritti::Ascolta));
        assert!(avvisi(&fx).is_empty(), "{fx:?}");
        // Uno stato che non nomina questo PC lo lascia com'era.
        let (s, fx) = arriva(s, stato_con(4, &[(1, "Marco")]));
        assert!(
            !avvisi(&fx).contains(&AVVISO_DIRITTI_TUTTO.to_owned()),
            "{fx:?}"
        );
        assert!(matches!(&s.fase, Fase::Membro(m) if m.diritti == Diritti::Ascolta));
    }

    #[test]
    fn l_ammesso_dice_gia_che_cosa_si_puo_fare() {
        let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
        let numero = numeri_degli_inviti(&s)[0];
        let (s, _) = passo(
            s,
            Evento::Comando(Comando::Rispondi {
                invito: numero,
                accetta: true,
            }),
            100,
        );
        let (s, _) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: Messaggio::Ammesso {
                    membro: 2,
                    diritti: Diritti::Comanda,
                },
            },
            200,
        );
        assert!(matches!(&s.fase, Fase::Membro(m) if m.diritti == Diritti::Comanda));
    }
}
