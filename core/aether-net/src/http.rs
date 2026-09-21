//! Le richieste HTTP, e la traduzione di un guasto di rete in un codice Aether.
//!
//! # Perché uno stato non è un errore, qui
//!
//! `ureq` di serie trasforma un 4xx o un 5xx in un `Err`, e con lui butta via il
//! **corpo** della risposta. Per Google quel corpo è l'informazione: un 400 dal
//! punto di scambio dei token può essere `invalid_grant` — l'utente deve
//! riautenticarsi — oppure `invalid_client` — la credenziale compilata dentro
//! l'applicazione è sbagliata, e nessun numero di tentativi la aggiusterà. Sono
//! due situazioni che si dicono all'utente in due modi diversi, e senza il corpo
//! sono indistinguibili.
//!
//! Perciò [`Rete`] chiede a `ureq` di non considerare uno stato un errore, e
//! restituisce sempre una [`Risposta`]. Chi chiama decide cosa significa, con
//! [`Rete::stato_a_errore`] quando gli basta la traduzione normale.
//!
//! # Perché si ritenta solo quello che si può ritentare
//!
//! Il predicato è [`AppError::is_retryable`], che vive nel catalogo degli errori
//! insieme al codice. Una seconda tabella «questi si ritentano» in questo file
//! sarebbe la cosa che diverge: `catalog.rs` documenta che è esattamente così
//! che il vecchio albero ha fatto classificare lo stesso guasto in due modi su
//! due piattaforme.
//!
//! La conseguenza pratica è che un 401 e un 400 non si ritentano mai — la
//! risposta a «non sei autorizzato» non cambia perché la si richiede più forte —
//! mentre un 429 e un 503 sì, aspettando il tempo che il servizio ha chiesto.

use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
// L'estensione che porta `get_uri()`: senza, l'indirizzo a cui si è arrivati
// dopo i reindirizzamenti non è raggiungibile dalla risposta.
use ureq::ResponseExt as _;

/// Quanto si aspetta al massimo fra due tentativi.
///
/// Un `Retry-After` può dire ore. Aether è un'applicazione musicale con un filo
/// di sottofondo: dormire più di mezzo minuto vorrebbe dire tenere occupato quel
/// filo per niente, quando la passata successiva arriva comunque fra un quarto
/// d'ora.
const ATTESA_MASSIMA: Duration = Duration::from_secs(30);

/// Quanti byte alla volta si leggono da un prelievo.
///
/// Sessantaquattro kilobyte: abbastanza grande da non chiamare `write` mille
/// volte al secondo, abbastanza piccolo da rispondere a un annullamento entro
/// un battito di ciglia anche su una connessione lenta.
const BLOCCO_PRELIEVO: usize = 64 * 1024;

/// Quanto si aspetta un nome che non si risolve, in un prelievo.
///
/// Le tre costanti qui sotto esistono perché [`Rete::per_prelievo`] non ha una
/// scadenza complessiva, e senza di loro un indirizzo morto lascerebbe il filo
/// del prelievo fermo per il timeout del sistema operativo — su Windows,
/// decine di secondi per la connessione e nessun limite per il resto.
const PRELIEVO_RISOLUZIONE: Duration = Duration::from_secs(10);

/// Quanto si aspetta una connessione che non si apre, in un prelievo.
const PRELIEVO_CONNESSIONE: Duration = Duration::from_secs(15);

/// Quanto si aspetta un servizio che non risponde nemmeno le intestazioni.
///
/// Trenta secondi: è la stessa attesa che i cataloghi si danno per una
/// risposta, perché la domanda è la stessa — «questo servizio c'è?» — e la
/// risposta arriva prima del primo byte del corpo.
const PRELIEVO_INTESTAZIONI: Duration = Duration::from_secs(30);

/// Quanto può durare, al massimo, l'arrivo di un corpo.
///
/// Mezz'ora, e il numero è scelto contando: duecento megabyte — un concerto in
/// FLAC dell'Internet Archive — in mezz'ora vogliono novecento kilobit al
/// secondo. Sotto quella soglia non c'è connessione con cui valga la pena
/// scaricare musica, e sopra il limite non si tocca mai.
///
/// Non è una scadenza «per respiro»: `ureq` non ne ha una, e questa è la sola
/// forma di limite che resta per un corpo che smette di arrivare. Fra un blocco
/// e l'altro [`Rete::preleva`] chiede comunque se l'utente ha annullato, quindi
/// il tempo che questo numero limita è solo quello in cui nessuno sta
/// guardando.
const PRELIEVO_CORPO: Duration = Duration::from_secs(30 * 60);

/// Il metodo della richiesta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metodo {
    /// Leggere.
    Get,
    /// Creare o caricare.
    Post,
    /// Modificare.
    Patch,
    /// Sostituire per intero.
    ///
    /// Serve al caricamento ripartibile di Drive, che manda i byte con `PUT`
    /// all'indirizzo di sessione invece che al punto delle API.
    Put,
    /// Cancellare.
    Delete,
}

impl Metodo {
    /// Il nome nel protocollo.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Patch => "PATCH",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

/// Cosa si manda insieme alla richiesta.
#[derive(Debug, Clone, Copy)]
pub enum Corpo<'a> {
    /// Niente.
    Niente,
    /// Byte, con il loro tipo dichiarato.
    Byte {
        /// Il `Content-Type`.
        tipo: &'a str,
        /// I byte.
        dati: &'a [u8],
    },
    /// Un modulo `application/x-www-form-urlencoded`.
    ///
    /// È la forma che i punti OAuth di Google accettano — e l'unica: mandare
    /// JSON al punto di scambio dei token dà un `invalid_request` che non dice
    /// perché.
    Modulo(&'a [(&'a str, String)]),
}

/// Una richiesta da eseguire.
#[derive(Debug, Clone, Copy)]
pub struct Richiesta<'a> {
    /// Il metodo.
    pub metodo: Metodo,
    /// L'indirizzo completo.
    pub url: &'a str,
    /// Le intestazioni da aggiungere.
    pub intestazioni: &'a [(&'a str, &'a str)],
    /// Il corpo.
    pub corpo: Corpo<'a>,
}

/// Quel che il servizio ha risposto.
#[derive(Debug, Clone)]
pub struct Risposta {
    /// Lo stato HTTP, qualunque esso sia: qui un 404 non è un guasto.
    pub stato: u16,
    /// Il corpo, così com'è arrivato.
    pub corpo: Vec<u8>,
    /// Quanto ha chiesto di aspettare, se l'ha chiesto.
    pub riprova_fra_ms: Option<u64>,
    /// L'intestazione `Location`.
    ///
    /// Solo due intestazioni di risposta cambiano il comportamento di questo
    /// crate — questa e `Retry-After` — e stanno qui invece che in una mappa
    /// completa perché una mappa inviterebbe a leggerne altre senza pensarci.
    /// `Location` è l'indirizzo di sessione che Drive restituisce per aprire un
    /// caricamento ripartibile.
    pub posizione: Option<String>,
    /// Dove si è arrivati davvero, dopo i reindirizzamenti.
    ///
    /// Diverso da `posizione`: quella è l'intestazione di **questa** risposta —
    /// e su un reindirizzamento già seguito non c'è più — mentre questo è il
    /// capolinea. Uguale all'indirizzo chiesto quando non ci sono stati salti,
    /// che è il caso di quasi tutte le richieste di Aether.
    ///
    /// Esiste per i link corti che l'app di Spotify mette nel foglio di
    /// condivisione (`spotify.link/…`): l'unico modo di sapere che cosa
    /// nominano è guardare dove portano.
    pub url_finale: String,
}

impl Risposta {
    /// Lo stato è nella famiglia dei 2xx.
    #[must_use]
    pub const fn e_andata(&self) -> bool {
        self.stato >= 200 && self.stato < 300
    }

    /// Il corpo come testo, per i messaggi di errore.
    ///
    /// Con perdita, di proposito: qui non si sta interpretando un documento, si
    /// sta cercando di dire a un essere umano cosa ha risposto il servizio, e un
    /// byte storto non deve poter far fallire anche il racconto del guasto.
    #[must_use]
    pub fn testo(&self) -> String {
        String::from_utf8_lossy(&self.corpo).into_owned()
    }
}

/// Un client HTTP con le scadenze di Aether.
///
/// Si clona a costo quasi zero — `ureq::Agent` tiene il suo dentro un `Arc` — e
/// la riserva di connessioni si condivide fra i cloni: è quel che rende una
/// passata di venti richieste un solo saluto TLS invece di venti.
#[derive(Debug, Clone)]
pub struct Rete {
    agente: ureq::Agent,
    servizio: &'static str,
    /// Se questo client può parlare in chiaro con questa macchina.
    ///
    /// Non basta averlo deciso alla costruzione: `https_only(false)` vale per
    /// **ogni** indirizzo che passa da questo agente, e un client nato per
    /// `127.0.0.1:11434` uscirebbe in chiaro verso chiunque gli si desse dopo.
    /// La bandiera esiste perché [`Rete::consenti`] possa rifare la domanda a
    /// ogni richiesta, che è l'unico momento in cui la risposta conta.
    chiaro_locale: bool,
}

/// Come Aether si presenta, sempre, a chiunque.
///
/// # Perché non si può cambiare
///
/// C'era un costruttore che accettava un `User-Agent` diverso, e serviva a una
/// cosa sola: farsi passare per un browser davanti ai punti interni del lettore
/// web di Spotify, che a un nome sconosciuto rispondono male o non rispondono.
/// Quel sottosistema non c'è più — era accesso non autorizzato a un servizio —
/// e con lui è sparita l'unica ragione per cui questo valore fosse un
/// parametro.
///
/// Adesso è una costante, ed è meglio che sia difficile da cambiare: un
/// programma che può travestirsi è un programma in cui qualcuno, prima o poi,
/// lo fa. Un indirizzo dentro lo `User-Agent` è invece la cortesia che permette
/// a chi gestisce un servizio pubblico di scrivere a qualcuno invece di
/// limitarsi a bloccare — ed è la cosa che rende sostenibile interrogare
/// archivi che nessuno paga.
///
/// La versione la scrive il compilatore, non una mano: un numero tenuto a mano
/// dove nessuno lo rilegge è un numero che mente, e questo diceva «0.1» mentre
/// il programma era alla 2.3. Chi gestisce MusicBrainz una versione
/// riconoscibile la chiede, e presentarne una che non esiste è il modo di
/// finire limitati.
const AGENTE: &str = concat!(
    "Aether/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/federicobaratti/aether)"
);

/// Un pezzo di un file, con quanto è lungo il file intero.
///
/// Lo restituisce [`Rete::intervallo`], e lo consuma `crate::flusso::FlussoHttp`.
#[derive(Debug, Clone)]
pub struct Pezzo {
    /// I byte arrivati.
    ///
    /// Possono essere **meno** di quanti chiesti, e non è un guasto: è la fine
    /// del file. Possono essere zero per la stessa ragione.
    pub byte: Vec<u8>,
    /// Quanto è lungo il file in tutto, quando il servizio lo dichiara.
    ///
    /// `None` toglie il cursore invece di inventarne uno sbagliato: un flusso
    /// senza lunghezza si sente lo stesso, e `MediaSource::is_seekable` di
    /// symphonia risponde `false` proprio guardando questo.
    pub totale: Option<u64>,
    /// L'indirizzo a cui si è arrivati davvero, quando è diverso da quello
    /// chiesto.
    ///
    /// Serve a chi farà **altre** richieste sullo stesso file. L'Internet
    /// Archive risponde a `/download/…` con un rimando al nodo che quel file ce
    /// l'ha davvero, e misurato costa novecento millisecondi: pagarlo una volta
    /// per brano invece che una per intervallo è, su un brano che si scorre
    /// avanti e indietro, la differenza fra qualche secondo e qualche decina.
    ///
    /// `None` quando non c'è stato nessun rimando.
    pub url_finale: Option<String>,
}

/// Quanto è lungo il file, da `Content-Range` o da `Content-Length`.
///
/// Nell'ordine, e conta: con una risposta parziale `Content-Length` è la
/// lunghezza del **pezzo**, non del file. Leggere quella e chiamarla «totale»
/// darebbe un cursore che finisce dopo un quarto di canzone.
fn totale_da(intestazioni: &ureq::http::HeaderMap) -> Option<u64> {
    // `bytes 0-1023/45678`: quel che serve è dopo la barra. Un `*` al posto del
    // numero è legale e vuol dire «non lo so», e allora non lo si sa nemmeno noi.
    if let Some(intervallo) = intestazioni
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        && let Some(coda) = intervallo.rsplit('/').next()
        && let Ok(quanto) = coda.trim().parse::<u64>()
    {
        return Some(quanto);
    }
    intestazioni
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
}

impl Rete {
    /// Un client con una scadenza complessiva.
    ///
    /// La scadenza è **globale**, cioè vale sull'intera richiesta e non su un
    /// singolo respiro del socket: è la forma che protegge davvero da un
    /// servizio che risponde un byte al secondo, e che una scadenza di lettura
    /// non copre.
    #[must_use]
    pub fn nuova(servizio: &'static str, scadenza: Duration) -> Self {
        let configurazione = ureq::Agent::config_builder()
            .timeout_global(Some(scadenza))
            // Vedi la nota in testa al modulo: il corpo di un 4xx è
            // l'informazione, e trasformarlo in un `Err` lo butterebbe via.
            .http_status_as_error(false)
            // Nessuna richiesta di questo crate ha motivo di uscire in chiaro.
            // Non è teorico: senza, un `http://` scritto per sbaglio in una
            // costante manderebbe un token in rete leggibile da chiunque.
            .https_only(true)
            .user_agent(AGENTE)
            .build();
        Self {
            agente: configurazione.into(),
            servizio,
            chiaro_locale: false,
        }
    }

    /// Un client per portare giù un file intero.
    ///
    /// # Perché non basta [`Self::nuova`]
    ///
    /// Perché la scadenza di `nuova` è **globale**: copre la risoluzione, la
    /// connessione, le intestazioni e l'ultimo byte del corpo, tutti insieme.
    /// È la forma giusta per una risposta JSON da qualche kilobyte, ed è la
    /// forma sbagliata per un file.
    ///
    /// Il conto che lo dimostra: i cataloghi si danno trenta secondi, e la
    /// docstring di [`Self::preleva`] nomina il caso vero, «un concerto in FLAC
    /// dell'Internet Archive sono duecento megabyte». Duecento megabyte in
    /// trenta secondi vogliono cinquantatré megabit al secondo **sostenuti**.
    /// Sotto quella soglia il trasferimento moriva a metà con `net.timeout`, e
    /// siccome `net.timeout` si ritenta la coda ripartiva da zero — all'infinito,
    /// su qualunque connessione normale.
    ///
    /// # Cosa resta limitato
    ///
    /// Tutto quello in cui il silenzio vuol dire guasto: il nome che non si
    /// risolve, la connessione che non si apre, il servizio che non risponde
    /// nemmeno le intestazioni. Del corpo resta un tetto largo
    /// ([`PRELIEVO_CORPO`]), perché `ureq` non offre una scadenza per singola
    /// lettura e senza nessun limite un socket che si blocca terrebbe il filo
    /// per sempre.
    #[must_use]
    pub fn per_prelievo(servizio: &'static str) -> Self {
        let configurazione = ureq::Agent::config_builder()
            // Nessuna scadenza complessiva: è il punto di questo costruttore.
            .timeout_global(None)
            .timeout_resolve(Some(PRELIEVO_RISOLUZIONE))
            .timeout_connect(Some(PRELIEVO_CONNESSIONE))
            .timeout_recv_response(Some(PRELIEVO_INTESTAZIONI))
            .timeout_recv_body(Some(PRELIEVO_CORPO))
            // Le stesse tre regole di `nuova`, e per le stesse ragioni.
            .http_status_as_error(false)
            .https_only(true)
            .user_agent(AGENTE)
            .build();
        Self {
            agente: configurazione.into(),
            servizio,
            chiaro_locale: false,
        }
    }

    /// La scadenza complessiva di questo client, se ne ha una.
    ///
    /// Esiste perché la differenza fra il client delle domande e quello dei
    /// prelievi è invisibile da fuori — sono lo stesso tipo — e una differenza
    /// invisibile è una differenza che prima o poi qualcuno annulla
    /// riassegnando un campo. Con questo, chi tiene le due reti può **provare**
    /// di averle tenute distinte.
    #[must_use]
    pub fn scadenza_complessiva(&self) -> Option<Duration> {
        self.agente.config().timeouts().global
    }

    /// Un client verso un indirizzo preciso, che sa se quell'indirizzo può
    /// essere in chiaro.
    ///
    /// # Perché esiste accanto a [`Self::nuova`]
    ///
    /// Perché `nuova` impone `https_only`, e ci sono due servizi con cui si
    /// parla in chiaro senza che un solo byte esca da questo computer: un
    /// modello di linguaggio servito da Ollama su `127.0.0.1:11434`, e lo stesso
    /// servito dal server locale di LM Studio su `127.0.0.1:1234`. Nessuno dei
    /// due ha un certificato, e nessuno dei due potrebbe averne uno.
    ///
    /// Le due strade sbagliate erano togliere `https_only` a tutti — buttare via
    /// la protezione per riparare un caso — e chiamare quei servizi dalla
    /// finestra, che oltre a rompere la politica dei contenuti farebbe della
    /// finestra l'unico posto dell'applicazione che parla da solo con la rete.
    ///
    /// Qui l'apertura è **una sola**, verificata prima che la richiesta parta, e
    /// nominata. La promessa che resta vera è che nessuna richiesta in chiaro
    /// **esce da questo computer**, ed è più debole di quella di prima: sta
    /// scritta così anche in `PRIVACY.md`, che è il posto dove conta.
    ///
    /// # Errori
    ///
    /// `net.badSchema` per qualunque indirizzo che non sia `https://`, oppure
    /// `http://` verso questa macchina.
    pub fn verso(servizio: &'static str, scadenza: Duration, url: &str) -> Result<Self, AppError> {
        if url.starts_with("https://") {
            return Ok(Self::nuova(servizio, scadenza));
        }
        if !in_chiaro_ammesso(url) {
            return Err(non_in_casa(servizio, url));
        }
        let configurazione = ureq::Agent::config_builder()
            .timeout_global(Some(scadenza))
            .http_status_as_error(false)
            // L'unica riga di tutto l'albero che apre il chiaro, e sopra c'è la
            // sola condizione in cui è lecita: l'indirizzo è questa macchina.
            .https_only(false)
            // Zero salti, e non è prudenza generica: senza, un server locale
            // che risponde «302 http://esterno/» porterebbe fuori di casa, in
            // chiaro, una richiesta partita per la macchina di chi la manda —
            // cioè esattamente la cosa che il controllo sull'ospite impedisce
            // di scrivere e che un reindirizzamento rimetterebbe in piedi.
            .max_redirects(0)
            .user_agent(AGENTE)
            .build();
        Ok(Self {
            agente: configurazione.into(),
            servizio,
            chiaro_locale: true,
        })
    }

    /// La domanda che si rifà a ogni richiesta: questo indirizzo può uscire?
    ///
    /// Su un client normale non c'è niente da chiedere — `https_only` risponde
    /// da sé, e in chiaro non parte niente. Su quello di [`Self::verso`] la
    /// risposta è «solo verso questa macchina», e va data **qui** e non alla
    /// costruzione: il permesso è dell'indirizzo, non del client, e un client
    /// nato per `127.0.0.1` a cui più tardi si passi un altro indirizzo non
    /// deve poterlo raggiungere leggibile.
    ///
    /// # Errori
    ///
    /// `net.badSchema`, nominando l'indirizzo che non si è potuto chiamare.
    fn consenti(&self, url: &str) -> Result<(), AppError> {
        if !self.chiaro_locale || url.starts_with("https://") || in_chiaro_ammesso(url) {
            return Ok(());
        }
        Err(non_in_casa(self.servizio, url))
    }

    /// Esegue una richiesta.
    ///
    /// # Errori
    ///
    /// `net.offline`, `net.timeout` o `net.http` per i guasti di **trasporto**.
    /// Uno stato di errore non produce un `Err`: arriva dentro la [`Risposta`].
    pub fn esegui(&self, richiesta: Richiesta<'_>) -> Result<Risposta, AppError> {
        self.consenti(richiesta.url)?;
        // I due rami non si possono unire: `ureq` distingue nel **tipo** un
        // costruttore che può portare un corpo da uno che non può, e ci si
        // guadagna che `GET` con un corpo non è una cosa che si possa scrivere
        // per sbaglio.
        let esito = match richiesta.metodo {
            Metodo::Get | Metodo::Delete => {
                let mut costruttore = if richiesta.metodo == Metodo::Get {
                    self.agente.get(richiesta.url)
                } else {
                    self.agente.delete(richiesta.url)
                };
                for (nome, valore) in richiesta.intestazioni {
                    costruttore = costruttore.header(*nome, *valore);
                }
                costruttore.call()
            }
            Metodo::Post | Metodo::Patch | Metodo::Put => {
                let mut costruttore = match richiesta.metodo {
                    Metodo::Patch => self.agente.patch(richiesta.url),
                    Metodo::Put => self.agente.put(richiesta.url),
                    _ => self.agente.post(richiesta.url),
                };
                for (nome, valore) in richiesta.intestazioni {
                    costruttore = costruttore.header(*nome, *valore);
                }
                match richiesta.corpo {
                    Corpo::Niente => costruttore.send_empty(),
                    Corpo::Byte { tipo, dati } => costruttore.content_type(tipo).send(dati),
                    Corpo::Modulo(campi) => costruttore
                        .content_type("application/x-www-form-urlencoded")
                        .send(modulo_urlencoded(campi).as_bytes()),
                }
            }
        };

        let mut risposta = esito.map_err(|err| self.trasporto(&err, richiesta.url))?;
        let stato = risposta.status().as_u16();
        // `ureq` segue i reindirizzamenti da sé: qui si legge soltanto dove è
        // finita. Va preso **prima** del corpo, che consuma la risposta.
        let url_finale = risposta.get_uri().to_string();
        let riprova_fra_ms = quanto_ha_chiesto(risposta.headers());
        let posizione = risposta
            .headers()
            .get("location")
            .and_then(|valore| valore.to_str().ok())
            .map(ToOwned::to_owned);
        let corpo = risposta
            .body_mut()
            .read_to_vec()
            .map_err(|err| self.trasporto(&err, richiesta.url))?;

        Ok(Risposta {
            stato,
            corpo,
            riprova_fra_ms,
            posizione,
            url_finale,
        })
    }

    /// Prende un file e lo scrive dove gli si dice, un pezzo per volta.
    ///
    /// # Perché non basta [`Self::esegui`]
    ///
    /// Perché quella raccoglie tutto il corpo in un `Vec<u8>` prima di
    /// restituirlo, e i corpi di cui si occupa sono risposte JSON da qualche
    /// kilobyte. Un concerto in FLAC dell'Internet Archive sono duecento
    /// megabyte: tenerli in memoria per poi riscriverli su disco è il doppio
    /// della memoria per niente, e soprattutto rende impossibile dire a chi
    /// guarda a che punto è — la barra salterebbe da 0 a 1 quando è già finito.
    ///
    /// # L'annullamento è una domanda, non un segnale
    ///
    /// Come in tutto il resto dell'albero: una chiusura che chi chiama collega
    /// al proprio `AtomicBool`, interrogata fra un blocco e l'altro. Non c'è
    /// niente da interrompere dall'esterno, e un prelievo annullato smette entro
    /// un blocco invece che entro una richiesta.
    ///
    /// Restituisce quanti byte sono stati scritti.
    ///
    /// # Errori
    ///
    /// `net.*` per i guasti di trasporto, `download.network` per una lettura che
    /// si interrompe a metà, `internal.aborted` per l'annullamento. Uno stato di
    /// errore diventa un `Err` — qui, a differenza di [`Self::esegui`], un corpo
    /// di errore non è un'informazione: è un file che non c'è.
    pub fn preleva(
        &self,
        url: &str,
        intestazioni: &[(&str, &str)],
        destinazione: &mut dyn std::io::Write,
        annullato: &dyn Fn() -> bool,
        avanzamento: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64, AppError> {
        self.consenti(url)?;
        let mut costruttore = self.agente.get(url);
        for (nome, valore) in intestazioni {
            costruttore = costruttore.header(*nome, *valore);
        }
        let mut risposta = costruttore
            .call()
            .map_err(|err| self.trasporto(&err, url))?;

        let stato = risposta.status().as_u16();
        if !(200..300).contains(&stato) {
            let riprova_fra_ms = quanto_ha_chiesto(risposta.headers());
            return Err(self.stato_a_errore(
                &Risposta {
                    stato,
                    corpo: Vec::new(),
                    riprova_fra_ms,
                    posizione: None,
                    url_finale: url.to_owned(),
                },
                url,
            ));
        }

        // Quanto sarà in tutto, se il servizio lo dichiara. `None` è il permesso
        // di disegnare una barra indeterminata, come per le pagine di una
        // lettura: uno zero sarebbe un numero, e un numero è una promessa.
        let totale = risposta
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        let mut lettore = risposta.body_mut().as_reader();
        let mut blocco = vec![0_u8; BLOCCO_PRELIEVO];
        let mut scritti = 0_u64;

        loop {
            if annullato() {
                return Err(AppError::new(ErrorCode::InternalAborted {
                    what: Some("prelievo".to_owned()),
                }));
            }
            let quanti = match std::io::Read::read(&mut lettore, &mut blocco) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => {
                    return Err(AppError::new(ErrorCode::DownloadNetwork)
                        .with_cause(format!("lettura interrotta dopo {scritti} byte: {err}")));
                }
            };
            let pezzo = blocco.get(..quanti).unwrap_or(&[]);
            destinazione.write_all(pezzo).map_err(|err| {
                AppError::new(ErrorCode::DownloadInvalidFiles)
                    .with_cause(format!("scrittura fallita: {err}"))
            })?;
            scritti = scritti.saturating_add(quanti as u64);
            avanzamento(scritti, totale);
        }

        // Un corpo troncato è il guasto che passerebbe in silenzio: il file c'è,
        // si apre, e finisce a metà canzone. Se il servizio ha dichiarato una
        // lunghezza, quella lunghezza è un contratto.
        if let Some(atteso) = totale
            && scritti < atteso
        {
            return Err(
                AppError::new(ErrorCode::DownloadNetwork).with_cause(format!(
                    "corpo troncato: {scritti} byte su {atteso} dichiarati"
                )),
            );
        }

        Ok(scritti)
    }

    /// Manda qualcosa e legge la risposta **mentre arriva**, un pezzo per volta.
    ///
    /// # Perché non basta né [`Self::esegui`] né [`Self::preleva`]
    ///
    /// `esegui` raccoglie tutto il corpo prima di restituirlo. Con una risposta
    /// che si scrive parola per parola vorrebbe dire aspettare in silenzio la
    /// fine e poi mostrarla tutta insieme, cioè buttare via l'unica cosa che
    /// rende sopportabile l'attesa di un modello lento.
    ///
    /// `preleva` legge a pezzi, ma è un `GET` e soprattutto **butta via il corpo
    /// di un errore**. È giusto per un file che non c'è; è sbagliato qui, dove
    /// un 400 porta con sé la frase che dice quale campo non andava — e senza
    /// quella frase all'utente resta il numero.
    ///
    /// # Errori
    ///
    /// `net.*` per i guasti di trasporto e per uno stato fuori dal 2xx, con il
    /// corpo dell'errore nella causa; `internal.aborted` se chi chiama ha smesso
    /// di volerla; e quel che restituisce `pezzo`, che ferma la lettura al primo
    /// pezzo rifiutato invece di continuare a leggere per nessuno.
    pub fn flusso(
        &self,
        url: &str,
        intestazioni: &[(&str, &str)],
        corpo: Corpo<'_>,
        annullato: &dyn Fn() -> bool,
        pezzo: &mut dyn FnMut(&[u8]) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        self.consenti(url)?;
        let mut costruttore = self.agente.post(url);
        for (nome, valore) in intestazioni {
            costruttore = costruttore.header(*nome, *valore);
        }
        let esito = match corpo {
            Corpo::Niente => costruttore.send_empty(),
            Corpo::Byte { tipo, dati } => costruttore.content_type(tipo).send(dati),
            Corpo::Modulo(campi) => costruttore
                .content_type("application/x-www-form-urlencoded")
                .send(modulo_urlencoded(campi).as_bytes()),
        };
        let mut risposta = esito.map_err(|err| self.trasporto(&err, url))?;

        let stato = risposta.status().as_u16();
        if !(200..300).contains(&stato) {
            let riprova_fra_ms = quanto_ha_chiesto(risposta.headers());
            // Il corpo si legge **prima** di tradurre lo stato: è lì che il
            // servizio scrive che quel modello non esiste, e senza resterebbe
            // un «400» buono per qualunque cosa.
            let corpo = risposta.body_mut().read_to_vec().unwrap_or_default();
            return Err(self.stato_a_errore(
                &Risposta {
                    stato,
                    corpo,
                    riprova_fra_ms,
                    posizione: None,
                    url_finale: url.to_owned(),
                },
                url,
            ));
        }

        let mut lettore = risposta.body_mut().as_reader();
        let mut blocco = vec![0_u8; BLOCCO_PRELIEVO];
        loop {
            if annullato() {
                return Err(AppError::new(ErrorCode::InternalAborted {
                    what: Some("flusso".to_owned()),
                }));
            }
            let quanti = match std::io::Read::read(&mut lettore, &mut blocco) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => {
                    return Err(AppError::new(ErrorCode::NetHttp {
                        status: stato,
                        url: Some(url.to_owned()),
                    })
                    .with_cause(format!("lettura interrotta: {err}")));
                }
            };
            pezzo(blocco.get(..quanti).unwrap_or(&[]))?;
        }
        Ok(())
    }

    /// Un pezzo di un file, chiesto per intervallo.
    ///
    /// # Perché esiste accanto a [`Self::preleva`]
    ///
    /// Perché le due domande sono diverse. `preleva` chiede **tutto**, dall'inizio
    /// alla fine, e lo scrive su un disco: è la forma di chi si tiene una copia.
    /// Questa chiede *quei* byte e basta, e chi la usa non ha il permesso di
    /// tenersi niente — serve a `Flusso`, cioè a suonare qualcosa mentre arriva.
    ///
    /// Il decodificatore non legge un file dall'inizio alla fine: cerca i tag in
    /// testa, poi in coda, poi torna al primo fotogramma, e quando qualcuno
    /// sposta il cursore salta a metà. Senza gli intervalli, ognuno di quei
    /// gesti sarebbe un file intero scaricato per leggerne quattro kilobyte.
    ///
    /// # Errori
    ///
    /// `download.badResponse` se il servizio **ignora** l'intervallo e risponde
    /// col file intero: succede, ed è un caso che va detto invece che tacere,
    /// perché vuol dire che quel flusso non si può posizionare. `net.*` per i
    /// guasti di trasporto, `download.*` per gli stati di errore.
    pub fn intervallo(&self, url: &str, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.consenti(url)?;
        let fino = da.saturating_add(quanti.max(1)).saturating_sub(1);
        let mut risposta = self
            .agente
            .get(url)
            .header("range", &format!("bytes={da}-{fino}"))
            .call()
            .map_err(|err| self.trasporto(&err, url))?;

        let stato = risposta.status().as_u16();
        if !(200..300).contains(&stato) {
            let riprova_fra_ms = quanto_ha_chiesto(risposta.headers());
            return Err(self.stato_a_errore(
                &Risposta {
                    stato,
                    corpo: Vec::new(),
                    riprova_fra_ms,
                    posizione: None,
                    url_finale: url.to_owned(),
                },
                url,
            ));
        }

        // `206 Partial Content` è la risposta che si è chiesta. Un `200` vuol
        // dire che il servizio ha ignorato l'intestazione e sta mandando tutto:
        // dal principio va bene — sono gli stessi byte — ma da un punto in mezzo
        // no, e fingere di non essersene accorti vorrebbe dire consegnare al
        // decodificatore dei byte presi dal posto sbagliato.
        if stato != 206 && da > 0 {
            return Err(
                AppError::new(ErrorCode::DownloadBadResponse).with_cause(format!(
                    "l'intervallo è stato ignorato: chiesto da {da}, risposto {stato}"
                )),
            );
        }

        let totale = totale_da(risposta.headers());
        // Prima di consumare il corpo: `get_uri` dice dove si è finiti dopo i
        // rimandi, e il corpo se lo porta via.
        let arrivato = risposta.get_uri().to_string();
        let byte = risposta.body_mut().read_to_vec().map_err(|err| {
            AppError::new(ErrorCode::DownloadNetwork)
                .with_cause(format!("lettura dell'intervallo interrotta: {err}"))
        })?;
        // L'indirizzo nuovo passa **dallo stesso cancello** di quello chiesto.
        // Un rimando è pur sempre qualcuno che dice «vai là», e seguirlo senza
        // ricontrollare vorrebbe dire che l'allowlist vale per il primo
        // indirizzo e non per il secondo — cioè non vale.
        let url_finale = (arrivato != url && self.consenti(&arrivato).is_ok()).then_some(arrivato);
        Ok(Pezzo {
            byte,
            totale,
            url_finale,
        })
    }

    /// Traduce un guasto di trasporto di `ureq` in un codice Aether.
    ///
    /// La distinzione fra «non c'è rete» e «la rete c'è ma è lenta» non è
    /// pignoleria: la prima si dice all'utente («sei senza connessione»), la
    /// seconda si ritenta da sola.
    fn trasporto(&self, err: &ureq::Error, url: &str) -> AppError {
        let host = ospite(url).map(ToOwned::to_owned);
        let url = Some(url.to_owned());
        match err {
            ureq::Error::Timeout(_) => AppError::new(ErrorCode::NetTimeout {
                url,
                timeout_ms: None,
            }),
            // Il nome prima della connessione: se non si risolve, non si è
            // nemmeno provato a connettersi, e dire «sei senza rete» manda a
            // controllare la cosa sbagliata.
            ureq::Error::HostNotFound => AppError::new(ErrorCode::NetHostUnknown { host }),
            ureq::Error::Io(io) if dns_fallito(io) => {
                AppError::new(ErrorCode::NetHostUnknown { host })
            }
            ureq::Error::ConnectionFailed => AppError::new(ErrorCode::NetOffline { url }),
            ureq::Error::Io(io) if senza_rete(io.kind()) => {
                AppError::new(ErrorCode::NetOffline { url })
            }
            ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
                AppError::new(ErrorCode::NetTimeout {
                    url,
                    timeout_ms: None,
                })
            }
            // Un guasto TLS, un'intestazione mostruosa, un corpo oltre il
            // limite: non sono «offline», e chiamarli così manderebbe l'utente a
            // controllare il cavo di rete per un certificato scaduto.
            altro => AppError::new(ErrorCode::NetBadSchema {
                service: Some(self.servizio.to_owned()),
                detail: Some(altro.to_string()),
            }),
        }
        .with_cause(err.to_string())
    }

    /// Il codice che corrisponde a uno stato di errore.
    ///
    /// Non si chiama da sola: è chi conosce la richiesta a sapere se un 404 sia
    /// un guasto o la risposta normale a «ce l'hai già questo file?».
    #[must_use]
    pub fn stato_a_errore(&self, risposta: &Risposta, url: &str) -> AppError {
        let errore = match risposta.stato {
            429 => AppError::new(ErrorCode::NetRateLimited {
                service: Some(self.servizio.to_owned()),
                retry_after_ms: risposta.riprova_fra_ms,
            }),
            408 => AppError::new(ErrorCode::NetTimeout {
                url: Some(url.to_owned()),
                timeout_ms: None,
            }),
            stato => AppError::new(ErrorCode::NetHttp {
                status: stato,
                url: Some(url.to_owned()),
            }),
        };
        // Il corpo di una risposta di errore di Google è un JSON con un
        // messaggio leggibile. Ci si fa stare i primi cinquecento caratteri: chi
        // legge un registro vuole sapere *cosa* si è lamentato, non ricevere una
        // pagina di HTML.
        errore.with_cause(accorcia(&risposta.testo(), 500))
    }

    /// Ripete un'operazione finché ha senso ripeterla.
    ///
    /// `tentativi` conta quelli **totali**: `3` vuol dire una richiesta e due
    /// ripetizioni. Fra un tentativo e l'altro si aspetta quel che il servizio
    /// ha chiesto, e se non ha chiesto niente si raddoppia partendo da un
    /// secondo.
    ///
    /// # Errori
    ///
    /// L'ultimo errore ottenuto. Se non era ritentabile, è anche il primo.
    pub fn ritenta<T>(
        &self,
        tentativi: u32,
        mut operazione: impl FnMut() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut attesa = Duration::from_secs(1);
        for rimasti in (1..tentativi.max(1)).rev() {
            match operazione() {
                Ok(valore) => return Ok(valore),
                Err(err) if err.is_retryable() => {
                    std::thread::sleep(quanto_aspettare(&err, attesa));
                    attesa = (attesa * 2).min(ATTESA_MASSIMA);
                    let _ = rimasti;
                }
                Err(err) => return Err(err),
            }
        }
        operazione()
    }
}

/// Quanto aspettare prima di riprovare.
///
/// Il tempo che il servizio ha chiesto vince sul nostro, perché è l'unico dei
/// due che sa quando smetterà di dire di no. Il nostro serve quando non l'ha
/// detto.
///
/// A sé, e non dentro [`Rete::ritenta`], perché è la sola parte di `ritenta`
/// che decide un numero: tutto il resto lì è chiamare l'operazione e dormire.
/// Estratta, la si mette alla prova con un errore costruito a mano — senza una
/// richiesta vera e senza aspettare davvero i secondi che risponde.
#[must_use]
fn quanto_aspettare(err: &AppError, predefinita: Duration) -> Duration {
    let chiesto = match err.code() {
        ErrorCode::NetRateLimited {
            retry_after_ms: Some(ms),
            ..
        }
        | ErrorCode::NetCircuitOpen {
            retry_after_ms: Some(ms),
            ..
        } => Some(Duration::from_millis(*ms)),
        _ => None,
    };
    chiesto.unwrap_or(predefinita).min(ATTESA_MASSIMA)
}

/// Quanto il servizio ha chiesto di aspettare, in millisecondi.
///
/// Due intestazioni, e la seconda non è un capriccio: `Retry-After` è quella
/// standard e la mandano Google e Spotify, ma **ListenBrainz non la manda**. Al
/// suo posto dichiara `X-RateLimit-Reset-In`, i secondi che mancano alla
/// finestra successiva, ed è documentata insieme all'endpoint — non è
/// un'estensione inventata da noi.
///
/// Senza questa riga un `429` di ListenBrainz ricadrebbe sull'attesa che
/// raddoppia: corretta, ma cieca. Con una coda di scrobble che si svuota mille
/// ascolti alla volta significa aspettare trenta secondi quando ne bastavano
/// due, moltiplicato per ogni blocco.
///
/// L'ordine conta: se un giorno ListenBrainz mandasse tutte e due, quella
/// standard vince. È l'unica delle due che tutti i proxy di mezzo capiscono.
fn quanto_ha_chiesto(intestazioni: &ureq::http::HeaderMap) -> Option<u64> {
    let legge = |nome: &str| {
        intestazioni
            .get(nome)
            .and_then(|valore| valore.to_str().ok())
            .and_then(secondi_in_ms)
    };
    legge("retry-after").or_else(|| legge("x-ratelimit-reset-in"))
}

/// Un `Retry-After` in secondi, tradotto in millisecondi.
///
/// La forma con la data HTTP (`Retry-After: Wed, 21 Oct 2015 07:28:00 GMT`) non
/// si interpreta: Google manda i secondi, e scrivere qui un lettore di date
/// RFC 7231 vorrebbe dire scrivere un lettore di date. Quando non si capisce,
/// resta l'attesa che raddoppia — che è comunque una risposta corretta.
fn secondi_in_ms(valore: &str) -> Option<u64> {
    valore
        .trim()
        .parse::<u64>()
        .ok()
        .map(|secondi| secondi.saturating_mul(1000))
}

/// I guasti di I/O che significano «quel nome non si risolve».
///
/// # Perché si guarda il numero e non il genere
///
/// Perché un fallimento di risoluzione non ha un `ErrorKind` suo. Su Windows
/// arriva come `Uncategorized` — che è instabile e non si può nemmeno nominare
/// in un `match` — con dentro il codice di Winsock; il genere quindi non dice
/// niente, e il numero dice tutto. È il difetto che si leggeva nel diario:
///
/// ```text
/// [errore] net.badSchema gravita=error ritentabile=false
///          causa=io: Host sconosciuto. (os error 11001)
/// ```
///
/// cioè un indirizzo sbagliato raccontato come «la risposta del servizio non ha
/// la forma attesa», per un servizio che non aveva risposto.
///
/// I quattro numeri sono `WSAHOST_NOT_FOUND`, `WSATRY_AGAIN`, `WSANO_RECOVERY`
/// e `WSANO_DATA`: le quattro risposte possibili di un resolver che non ce l'ha
/// fatta. Fuori da Windows la stessa cosa arriva come `NotFound`, oppure come un
/// `Other` il cui testo nomina la ricerca — l'ultima è una lettura di stringhe,
/// e sta per ultima apposta.
fn dns_fallito(err: &std::io::Error) -> bool {
    if matches!(err.raw_os_error(), Some(11001..=11004)) {
        return true;
    }
    if err.kind() == std::io::ErrorKind::NotFound {
        return true;
    }
    let testo = err.to_string().to_lowercase();
    testo.contains("failed to lookup")
        || testo.contains("name or service not known")
        || testo.contains("nodename nor servname")
}

/// I guasti di I/O che significano «la rete non c'è».
fn senza_rete(genere: std::io::ErrorKind) -> bool {
    matches!(
        genere,
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::HostUnreachable
            | std::io::ErrorKind::NetworkUnreachable
            | std::io::ErrorKind::NetworkDown
            | std::io::ErrorKind::BrokenPipe
    )
}

/// Taglia un testo a una lunghezza massima, senza spaccare un carattere.
fn accorcia(testo: &str, quanti: usize) -> String {
    match testo.char_indices().nth(quanti) {
        None => testo.to_owned(),
        Some((taglio, _)) => {
            let mut corto = testo.get(..taglio).unwrap_or_default().to_owned();
            corto.push('…');
            corto
        }
    }
}

/// Compone un corpo `application/x-www-form-urlencoded`.
fn modulo_urlencoded(campi: &[(&str, String)]) -> String {
    let mut fuori = String::new();
    for (nome, valore) in campi {
        if !fuori.is_empty() {
            fuori.push('&');
        }
        fuori.push_str(&percento(nome));
        fuori.push('=');
        fuori.push_str(&percento(valore));
    }
    fuori
}

/// Il rifiuto di un indirizzo che uscirebbe leggibile da questo computer.
fn non_in_casa(servizio: &str, url: &str) -> AppError {
    AppError::new(ErrorCode::NetBadSchema {
        service: Some(servizio.to_owned()),
        detail: Some(format!(
            "in chiaro si parla solo con questo computer, non con «{}»",
            accorcia(url, 120)
        )),
    })
}

/// Se con questo indirizzo si può parlare in chiaro: cioè se è questo computer.
///
/// # Perché è pubblica, e non un dettaglio di [`Rete::verso`]
///
/// Perché la stessa domanda se la fa anche chi **salva** un indirizzo, per dire
/// di no mentre lo si scrive invece che al primo messaggio. Due implementazioni
/// della stessa domanda sono due risposte diverse alla stessa domanda: qui ce
/// n'è una sola, e `Rete::verso` è soltanto il punto in cui è obbligatoria.
///
/// `http://localhost@esempio.com/` **non** è questo computer. Quel che conta è
/// l'ultima chiocciola, non la prima, e senza guardarla si aprirebbe il chiaro
/// verso chiunque sappia scrivere un indirizzo.
#[must_use]
pub fn in_chiaro_ammesso(url: &str) -> bool {
    ospite_in_chiaro(url).is_some_and(|ospite| {
        ospite.eq_ignore_ascii_case("localhost") || ospite == "127.0.0.1" || ospite == "::1"
    })
}

/// L'ospite di un `http://`, e soltanto di un `http://`.
///
/// Il filtro sullo schema è il punto: `in_chiaro_ammesso` risponde «sì» a
/// `http://localhost` e deve rispondere «no» a `ftp://localhost`, che non è un
/// indirizzo per Aether.
fn ospite_in_chiaro(url: &str) -> Option<&str> {
    url.starts_with("http://").then(|| ospite(url)).flatten()
}

/// L'ospite di un indirizzo, comunque cominci.
///
/// Serve a due cose che leggono l'autorità per ragioni opposte:
/// [`in_chiaro_ammesso`], che deve sapere se è questo computer, e
/// [`Rete::trasporto`], che quando il nome non si risolve deve poterlo nominare
/// — «esempio.com non esiste» è la frase che dice cosa correggere, mentre
/// l'indirizzo intero con percorso e query è quel che si scrive in un log.
///
/// `http://localhost@esempio.com/` risponde `esempio.com`: conta l'**ultima**
/// chiocciola, e guardare la prima è il modo di farsi passare per casa da
/// chiunque sappia scrivere un indirizzo.
fn ospite(url: &str) -> Option<&str> {
    let resto = url
        .split_once("://")
        .map_or(url, |(_, dopo_schema)| dopo_schema);
    let fine = resto.find(['/', '?', '#']).unwrap_or(resto.len());
    let autorita = resto.get(..fine)?;
    let dopo_utente = autorita.rsplit('@').next()?;
    // `[::1]:1234`: le parentesi quadre tengono insieme i due punti
    // dell'indirizzo, che senza sarebbero quelli della porta.
    let nudo = if let Some(dentro) = dopo_utente.strip_prefix('[') {
        dentro.split(']').next()?
    } else {
        dopo_utente.split(':').next()?
    };
    (!nudo.is_empty()).then_some(nudo)
}

/// Codifica una stringa per una `query` o un modulo.
///
/// L'insieme di caratteri non riservati è quello della RFC 3986. Lo spazio
/// diventa `%20` e **non** `+`: la seconda forma è ammessa solo dentro un
/// modulo, e un client che la usa dappertutto prima o poi la infila in un
/// indirizzo di autorizzazione, dove Google la legge come un più letterale.
pub fn percento(input: &str) -> String {
    let mut fuori = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                fuori.push(char::from(byte));
            }
            altro => fuori.push_str(&format!("%{altro:02X}")),
        }
    }
    fuori
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn in_chiaro_si_parla_solo_con_questo_computer() {
        assert!(in_chiaro_ammesso("http://localhost:11434/v1"));
        assert!(in_chiaro_ammesso("http://127.0.0.1:1234/v1/models"));
        assert!(in_chiaro_ammesso("http://[::1]:11434/v1"));
        assert!(
            in_chiaro_ammesso("http://LocalHost/v1"),
            "nel nome di un ospite le maiuscole non contano"
        );

        assert!(!in_chiaro_ammesso("http://esempio.com/v1"));
        assert!(
            !in_chiaro_ammesso("http://localhost@esempio.com/v1"),
            "l'ospite sta dopo l'ultima chiocciola: è il modo di travestire da locale un indirizzo che non lo è"
        );
        assert!(
            !in_chiaro_ammesso("http://127.0.0.1.esempio.com/v1"),
            "un ospite che comincia per 127.0.0.1 non è 127.0.0.1"
        );
        assert!(
            !in_chiaro_ammesso("http://"),
            "un ospite vuoto non è questo computer"
        );
        assert!(
            !in_chiaro_ammesso("https://openrouter.ai/api/v1"),
            "la domanda vale solo per il chiaro: chi cifra non passa di qui"
        );
    }

    #[test]
    fn il_permesso_del_chiaro_e_dellindirizzo_e_non_del_client() {
        // Il difetto che questa prova chiude: `verso` guardava l'indirizzo una
        // volta sola, alla costruzione. L'agente che ne usciva aveva
        // `https_only(false)` per **qualunque** indirizzo, quindi bastava
        // riusarlo con un altro per uscire in chiaro verso il mondo. Adesso la
        // domanda si rifà a ogni richiesta, e questa non tocca la rete: il
        // rifiuto arriva prima del socket.
        let locale = Rete::verso(
            "ollama",
            Duration::from_secs(1),
            "http://127.0.0.1:11434/v1",
        )
        .expect("un indirizzo di casa costruisce");

        let fuori = locale
            .esegui(Richiesta {
                metodo: Metodo::Get,
                url: "http://esempio.com/v1/models",
                intestazioni: &[],
                corpo: Corpo::Niente,
            })
            .unwrap_err();
        assert_eq!(fuori.code().kind().code(), "net.badSchema");

        let ancora_a_casa = locale.esegui(Richiesta {
            metodo: Metodo::Get,
            url: "http://127.0.0.1:11434/v1/models",
            intestazioni: &[],
            corpo: Corpo::Niente,
        });
        assert!(
            !matches!(
                &ancora_a_casa,
                Err(err) if err.code().kind().code() == "net.badSchema"
            ),
            "verso casa la guardia non deve dire niente: quel che risponde qui              è la rete, ed è un'altra domanda"
        );
    }

    #[test]
    fn un_client_verso_un_indirizzo_rifiuta_quel_che_uscirebbe_leggibile() {
        let scadenza = Duration::from_secs(1);
        assert!(Rete::verso("openrouter", scadenza, "https://openrouter.ai/api/v1").is_ok());
        assert!(
            Rete::verso("ollama", scadenza, "http://127.0.0.1:11434/v1").is_ok(),
            "l'eccezione voluta: un modello servito da questa macchina"
        );

        let fuori = Rete::verso("personalizzato", scadenza, "http://esempio.com/v1").unwrap_err();
        assert_eq!(fuori.code().kind().code(), "net.badSchema");
        assert!(
            matches!(fuori.code(), ErrorCode::NetBadSchema { detail: Some(d), .. } if d.contains("esempio.com")),
            "il messaggio nomina l'indirizzo: «schema non valido» da solo non dice cosa correggere"
        );

        let altro = Rete::verso("personalizzato", scadenza, "ftp://esempio.com").unwrap_err();
        assert_eq!(
            altro.code().kind().code(),
            "net.badSchema",
            "quel che non è né http né https non è un indirizzo per Aether"
        );
    }

    #[test]
    fn lo_spazio_non_diventa_un_piu() {
        // In un indirizzo di autorizzazione un `+` è un più, e gli scope
        // separati da `+` invece che da `%20` arrivano a Google come un unico
        // scope inesistente.
        assert_eq!(percento("openid email"), "openid%20email");
        assert_eq!(percento("a~b_c-d.e"), "a~b_c-d.e", "questi non si toccano");
        assert_eq!(percento("https://x/y"), "https%3A%2F%2Fx%2Fy");
    }

    #[test]
    fn un_modulo_ha_la_forma_che_google_accetta() {
        let campi = [
            ("grant_type", "authorization_code".to_owned()),
            ("code", "4/0Ab_c".to_owned()),
        ];
        assert_eq!(
            modulo_urlencoded(&campi),
            "grant_type=authorization_code&code=4%2F0Ab_c"
        );
    }

    #[test]
    fn un_retry_after_si_legge_solo_se_e_un_numero() {
        assert_eq!(secondi_in_ms("30"), Some(30_000));
        assert_eq!(secondi_in_ms(" 2 "), Some(2000));
        assert_eq!(
            secondi_in_ms("Wed, 21 Oct 2015 07:28:00 GMT"),
            None,
            "resta l'attesa che raddoppia, che è comunque corretta"
        );
    }

    #[test]
    fn listenbrainz_dice_quanto_aspettare_con_un_altro_nome() {
        let mappa = |coppie: &[(&str, &str)]| {
            let mut m = ureq::http::HeaderMap::new();
            for (nome, valore) in coppie {
                if let (Ok(n), Ok(v)) = (
                    ureq::http::HeaderName::try_from(*nome),
                    ureq::http::HeaderValue::try_from(*valore),
                ) {
                    m.insert(n, v);
                }
            }
            m
        };

        assert_eq!(
            quanto_ha_chiesto(&mappa(&[("retry-after", "7")])),
            Some(7000)
        );
        assert_eq!(
            quanto_ha_chiesto(&mappa(&[("x-ratelimit-reset-in", "3")])),
            Some(3000),
            "ListenBrainz non manda Retry-After: senza questa riga un 429 aspetterebbe alla cieca"
        );
        assert_eq!(
            quanto_ha_chiesto(&mappa(&[
                ("retry-after", "1"),
                ("x-ratelimit-reset-in", "60")
            ])),
            Some(1000),
            "quella standard vince: è l'unica che i proxy di mezzo capiscono"
        );
        assert_eq!(quanto_ha_chiesto(&mappa(&[])), None);
    }

    #[test]
    fn gli_stati_diventano_i_codici_giusti() {
        let rete = Rete::nuova("drive", Duration::from_secs(1));
        let risposta = |stato: u16, riprova: Option<u64>| Risposta {
            stato,
            corpo: b"{\"error\":\"qualcosa\"}".to_vec(),
            riprova_fra_ms: riprova,
            posizione: None,
            url_finale: "https://x".to_owned(),
        };

        let limite = rete.stato_a_errore(&risposta(429, Some(5000)), "https://x");
        assert_eq!(limite.code().kind().code(), "net.rateLimited");
        assert!(limite.is_retryable(), "un 429 si aspetta e si rifà");

        let vietato = rete.stato_a_errore(&risposta(401, None), "https://x");
        assert_eq!(vietato.code().kind().code(), "net.http");
        assert!(
            !vietato.is_retryable(),
            "«non sei autorizzato» non cambia se lo si richiede più forte"
        );

        let guasto = rete.stato_a_errore(&risposta(503, None), "https://x");
        assert!(guasto.is_retryable(), "un servizio giù torna su");

        // Il corpo arriva nella causa: è lì che Google dice *quale* cosa è
        // andata storta.
        assert!(guasto.cause().is_some_and(|c| c.contains("qualcosa")));
    }

    /// Il difetto che il diario mostrava per intero:
    ///
    /// ```text
    /// [errore] net.badSchema gravita=error ritentabile=false
    ///          causa=io: Host sconosciuto. (os error 11001)
    /// ```
    ///
    /// Un nome che non si risolve finiva nel ripiego di `trasporto`, che è
    /// «guasto di trasporto che non è la rete giù» — cioè un certificato
    /// scaduto o un'intestazione mostruosa — e usciva come errore grave, non
    /// ritentabile, con la frase sbagliata.
    #[test]
    fn un_nome_che_non_si_risolve_non_e_uno_schema_sbagliato() {
        for numero in [11001, 11002, 11003, 11004] {
            let io = std::io::Error::from_raw_os_error(numero);
            assert!(dns_fallito(&io), "os error {numero}");
            assert!(
                !senza_rete(io.kind()),
                "os error {numero} non è «la rete non c'è»"
            );
        }
        // Le due forme che arrivano fuori da Windows.
        assert!(dns_fallito(&std::io::Error::other(
            "failed to lookup address information: Name or service not known",
        )));
        assert!(dns_fallito(&std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such host",
        )));
        // E quel che non c'entra resta fuori: una connessione rifiutata è il
        // servizio spento, non il nome sbagliato.
        assert!(!dns_fallito(&std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "connection refused",
        )));
    }

    #[test]
    fn lospite_e_quello_dopo_lultima_chiocciola() {
        assert_eq!(
            ospite("https://openrouter.ai/api/v1"),
            Some("openrouter.ai")
        );
        assert_eq!(ospite("http://127.0.0.1:11434/v1"), Some("127.0.0.1"));
        assert_eq!(ospite("http://[::1]:1234/v1"), Some("::1"));
        assert_eq!(
            ospite("http://localhost@esempio.com/x"),
            Some("esempio.com"),
            "l'ultima chiocciola, non la prima"
        );
        assert_eq!(ospite("http:///niente"), None);
    }

    #[test]
    fn si_ritenta_solo_quel_che_si_puo_ritentare() {
        let rete = Rete::nuova("drive", Duration::from_secs(1));

        let mut volte = 0;
        let esito: Result<(), AppError> = rete.ritenta(3, || {
            volte += 1;
            Err(AppError::new(ErrorCode::NetHttp {
                status: 400,
                url: None,
            }))
        });
        assert!(esito.is_err());
        assert_eq!(volte, 1, "un 400 non si ritenta nemmeno una volta");

        let mut volte = 0;
        let esito: Result<u32, AppError> = rete.ritenta(3, || {
            volte += 1;
            if volte < 3 {
                return Err(AppError::new(ErrorCode::NetRateLimited {
                    service: None,
                    // Zero millisecondi: qui si prova la conta dei tentativi,
                    // non la pazienza di chi guarda passare i test.
                    retry_after_ms: Some(0),
                }));
            }
            Ok(volte)
        });
        assert_eq!(esito, Ok(3));
    }

    #[test]
    fn un_testo_lungo_si_accorcia_senza_spaccare_un_carattere() {
        let lungo = "à".repeat(100);
        let corto = accorcia(&lungo, 10);
        assert_eq!(corto.chars().count(), 11, "dieci più i puntini");
        assert_eq!(accorcia("corto", 10), "corto");
    }

    #[test]
    fn la_rete_delle_domande_ha_una_scadenza_complessiva() {
        let rete = Rete::nuova("prova", Duration::from_secs(30));
        assert_eq!(rete.scadenza_complessiva(), Some(Duration::from_secs(30)));
    }

    #[test]
    fn la_rete_dei_prelievi_non_ha_una_scadenza_complessiva() {
        // Il difetto che questa prova impedisce di rifare: con una scadenza
        // complessiva di trenta secondi, duecento megabyte di FLAC volevano
        // cinquantatré megabit al secondo sostenuti, e sotto quella soglia il
        // prelievo moriva a metà — per sempre, perché `net.timeout` si ritenta.
        let rete = Rete::per_prelievo("prova");
        assert_eq!(rete.scadenza_complessiva(), None);

        // Ma il silenzio resta limitato dove silenzio vuol dire guasto.
        let scadenze = rete.agente.config().timeouts();
        assert_eq!(scadenze.resolve, Some(PRELIEVO_RISOLUZIONE));
        assert_eq!(scadenze.connect, Some(PRELIEVO_CONNESSIONE));
        assert_eq!(scadenze.recv_response, Some(PRELIEVO_INTESTAZIONI));
        assert_eq!(scadenze.recv_body, Some(PRELIEVO_CORPO));
    }

    #[test]
    fn il_tetto_del_corpo_regge_un_concerto_su_una_linea_lenta() {
        // Duecento megabyte è la misura che la docstring di `preleva` nomina.
        let megabyte = 200_u64;
        let secondi = PRELIEVO_CORPO.as_secs();
        let megabit_al_secondo = (megabyte * 8) as f64 / secondi as f64;
        assert!(
            megabit_al_secondo < 1.0,
            "un concerto in FLAC deve passare anche sotto un megabit: servono {megabit_al_secondo:.2} Mbit/s"
        );
    }

    #[test]
    fn lo_user_agent_dice_il_nome_la_versione_e_dove_scrivere() {
        assert!(AGENTE.starts_with("Aether/"));
        assert!(
            AGENTE.contains(env!("CARGO_PKG_VERSION")),
            "scritta a mano era ferma alla 0.1: MusicBrainz limita chi si presenta con una versione che non esiste"
        );
        assert!(
            AGENTE.contains("+https://github.com/federicobaratti/aether"),
            "l'indirizzo è quel che permette a un servizio di scrivere invece di bloccare"
        );
    }
}
