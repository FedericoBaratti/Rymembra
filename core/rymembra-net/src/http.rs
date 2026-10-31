//! Le richieste HTTP, e la traduzione di un guasto di rete in un codice Rymembra.
//!
//! # Perché uno stato non è un errore, qui
//!
//! `ureq` di serie trasforma un 4xx o un 5xx in un `Err`, e con lui butta via il
//! **corpo** della risposta. Per Google quel corpo è l'informazione: un 400 dal
//! punto di scambio dei token può essere `invalid_grant` — l'utente deve
//! riautenticarsi — oppure `invalid_client` — l'app registrata dall'utente non
//! va (identificativo o segreto sbagliati), e nessun numero di tentativi la
//! aggiusterà. Sono
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
//! mentre un 429 e un 503 sì. Il tempo che il servizio ha chiesto
//! (`Retry-After`) lo aspetta soltanto il 429: [`Rete::stato_a_errore`] lo mette
//! solo in `net.rateLimited`. Un 503 diventa `net.http`, che di attese non ne
//! porta, e si ritenta con l'attesa che raddoppia da un secondo anche quando il
//! servizio aveva chiesto di più. Portarlo anche lì vorrebbe dire un campo nuovo
//! in `net.http`, che attraversa l'IPC, e la stessa regola nelle pause del
//! caricamento di Drive.
//!
//! # Di chi ci si fida, e dove si va quando qualcuno dice «vai là»
//!
//! Due domande che fino alla 2.4.0 avevano la risposta di serie di `ureq`, e
//! nessuno l'aveva scelta.
//!
//! La prima è **quali certificati** valgono. `ureq` di serie porta con sé
//! l'elenco di Mozilla e ignora quello del sistema: su un computer d'ufficio,
//! dove un proxy apre e richiude il TLS con un certificato che l'azienda ha
//! messo nell'archivio di Windows, ogni richiesta di Rymembra falliva mentre il
//! browser accanto funzionava — e l'aggiornamento, che usa `reqwest` con
//! l'archivio del sistema, funzionava anche lui. Adesso decide l'archivio di
//! Windows per tutti e due: vedi `tls`, più sotto.
//!
//! La seconda è **quali rimandi** si seguono. Ogni client ne seguiva dieci, da
//! sé, verso qualunque indirizzo `https://`. Adesso ogni client dice la sua
//! politica, e quella di chi porta un segreto è «nessuno»: vedi [`Rimandi`].
//!
//! # Da dove si passa: il proxy di Windows
//!
//! Una terza domanda ha tenuto più a lungo la risposta di serie. `ureq` cerca
//! un proxy solo nelle variabili d'ambiente (`HTTPS_PROXY` e compagnia), che su
//! Windows non imposta nessuno: su un portatile d'ufficio con il proxy scritto
//! in Impostazioni e nessuna uscita diretta, Drive, i cataloghi e lo
//! scrobbling cadevano con «sei senza rete» mentre l'aggiornamento, che usa
//! `reqwest` e il registro lo legge, funzionava. Adesso `ureq` legge anche il
//! proxy di Windows — la funzione `win-system-proxy`, accesa alla radice del
//! workspace — e una regola di qui sceglie, richiesta per richiesta, chi va
//! diretto: vedi `senza_proxy`, più sotto. Quel che resta fuori, e va saputo prima di
//! cercare il guasto su un computer d'ufficio:
//!
//! - nessuna configurazione automatica: uno script PAC (`AutoConfigURL`) o la
//!   ricerca WPAD («Rileva automaticamente le impostazioni») non si leggono, e
//!   chi ha solo quelli va diretto come prima;
//! - `ProxyServer` si legge solo nella forma `ospite:porta`. Quella per
//!   protocollo (`http=…;https=…`) per `ureq` non è un indirizzo, e si va
//!   diretti come prima;
//! - le variabili d'ambiente vincono sul registro, e `NO_PROXY` su
//!   `ProxyOverride`, com'è in `ureq`;
//! - il registro si legge quando nasce un client: un proxy cambiato con
//!   Rymembra aperta vale per i client che nascono dopo;
//! - chi va diretto si decide sull'indirizzo chiesto. I rimandi che segue
//!   `ureq` ([`Rimandi::Liberi`]) tengono la scelta del primo indirizzo;
//!   quelli seguiti a mano ([`Rimandi::Sorvegliati`]) la rifanno a ogni salto.

use std::sync::Arc;
use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCode};

mod richieste;

/// Quanto si aspetta al massimo fra due tentativi.
///
/// Un `Retry-After` può dire ore. Rymembra è un'applicazione musicale con un filo
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

/// Quanto può durare, al massimo, l'arrivo di una finestra di un flusso.
///
/// Novanta secondi, e il conto è quello della finestra più grande
/// (`flusso::FINESTRA_MASSIMA`, due megabyte): in novanta secondi arrivano con
/// ventitré kilobyte al secondo, cioè centottanta kilobit — meno di quanti ne
/// chieda un MP3 dell'Internet Archive per suonare senza fermarsi. Sotto quella
/// soglia il brano non si ascolta comunque, e dirlo è meglio che restare zitti.
///
/// È la scadenza che mancava. Il flusso leggeva con la rete delle domande, che
/// ha trenta secondi **complessivi**: due megabyte su una linea da cinquanta
/// kilobyte al secondo ne vogliono quaranta, e la finestra moriva a metà — a
/// metà canzone, con un errore che parlava di una cartella di rete.
const FLUSSO_CORPO: Duration = Duration::from_secs(90);

/// Quanto può pesare, di serie, un corpo che [`Rete::esegui`] raccoglie intero.
///
/// Dieci mebibyte, ed è lo stesso numero che `ureq` si dava da sé: fino alla
/// 2.4.0 valeva per tutti i client di Rymembra senza che nessuno l'avesse scelto,
/// e nessuno se n'era accorto perché le risposte JSON dei cataloghi, dei modelli
/// e dello scrobbling stanno in qualche kilobyte. Se n'è accorto il ripristino
/// di Drive: una skin da quindici megabyte scendeva fino al decimo e poi
/// l'intero ripristino falliva con «la risposta non ha la forma attesa».
///
/// Resta dieci per chi non dice altro, perché per una risposta di un servizio
/// il tetto è una protezione — un servizio che manda un gigabyte al posto di un
/// JSON riempirebbe la memoria — e chi ha bisogno di più lo dichiara con
/// [`Rete::con_tetto_corpo`], dove si vede.
pub const TETTO_CORPO_DI_SERIE: u64 = 10 * 1024 * 1024;

/// Quante volte si chiede un intervallo, in tutto, prima di arrendersi.
///
/// Tre: una richiesta e due ripetizioni, con l'attesa che raddoppia da un
/// secondo. È lo stesso numero che il backup e lo scrobbling danno alle loro
/// chiamate, per la stessa ragione — il guasto che passa da sé passa in pochi
/// secondi, e quello che non passa va detto invece che nascosto.
const TENTATIVI_INTERVALLO: u32 = 3;

/// Fino a quando, dalla prima richiesta, può **cominciare** un nuovo tentativo
/// di un intervallo.
///
/// Dall'altra parte c'è qualcuno che ascolta: tre tentativi da trenta secondi
/// l'uno — la scadenza delle intestazioni — farebbero un minuto e mezzo di
/// silenzio prima di dire che la rete non c'è. Un guasto si ritenta solo se il
/// tentativo dopo, pausa compresa, partirebbe entro questo bilancio; altrimenti
/// si racconta subito, anche se restavano tentativi. La pausa conta perché un
/// `429` può chiedere trenta secondi: guardato solo nel momento del guasto, il
/// bilancio lasciava partire il terzo tentativo a un minuto.
///
/// Non è un tetto al tempo totale: un tentativo partito dentro il bilancio ha
/// le scadenze di [`Rete::per_flusso`] e non viene interrotto. Fermarlo al
/// residuo vorrebbe dire uccidere a metà una finestra lenta ma viva — la
/// trappola da cui [`FLUSSO_CORPO`] esiste per tirare fuori.
const BILANCIO_INTERVALLO: Duration = Duration::from_secs(45);

/// Ogni quanto, durante la pausa fra due tentativi, si guarda se chi aspetta ha
/// smesso di aspettare.
///
/// Cinquanta millisecondi: un salto altrove nel brano non deve restare fermo
/// dietro il secondo di attesa di una richiesta che nessuno vuole più.
const PASSO_ATTESA: Duration = Duration::from_millis(50);

/// Quanti rimandi segue, di serie, un client che non ha detto altro.
///
/// Dieci, che è il numero di `ureq`: è il comportamento che i cataloghi, le
/// copertine e i metadati hanno sempre avuto, e che per loro è giusto — la
/// Cover Art Archive risponde a ogni copertina con un rimando all'Internet
/// Archive, e l'Internet Archive a ogni `/download/…` con un rimando al nodo che
/// il file ce l'ha. Scritto qui invece che lasciato implicito perché adesso è
/// **una** delle politiche possibili, e un numero che nessuno ha scritto è un
/// numero che nessuno sa di aver scelto.
pub const RIMANDI_DI_SERIE: u32 = 10;

/// Chi decide se un rimando si può seguire.
///
/// Riceve l'indirizzo a cui il servizio vorrebbe mandare la richiesta — intero e
/// già risolto, cioè con schema e ospite anche quando il servizio ha scritto
/// solo un percorso — e risponde sì o no **prima** che verso quell'indirizzo
/// parta un solo byte.
///
/// # Perché una chiusura e non un elenco di domini
///
/// Perché l'elenco non sta qui. Questo crate non conosce i cataloghi e non deve
/// cominciare (vedi la regola di dipendenza in testa al crate): chi sa che un
/// flusso dell'Internet Archive può finire su `ia800305.us.archive.org` è
/// `rymembra_catalogo::riconosci`, e un secondo elenco scritto qui sarebbe il
/// secondo — cioè quello che un giorno dice una cosa diversa. Chi costruisce il
/// cancello lo prende da dove sta.
#[derive(Clone)]
pub struct Cancello(Arc<dyn Fn(&str) -> bool + Send + Sync>);

impl Cancello {
    /// Un cancello che ammette quel che `ammette` ammette.
    #[must_use]
    pub fn nuovo(ammette: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(ammette))
    }

    /// Questo indirizzo passa.
    #[must_use]
    pub fn ammette(&self, url: &str) -> bool {
        (self.0)(url)
    }
}

/// Scritto a mano: una chiusura non si stampa, e dire che c'è basta.
impl std::fmt::Debug for Cancello {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cancello(…)")
    }
}

/// Che cosa fa un client quando il servizio risponde «vai là».
///
/// # Perché tre forme e non un numero
///
/// Perché la domanda vera non è *quanti* rimandi, ma *verso dove*. Un numero
/// risponde solo alla prima, e per chi porta un segreto è la seconda a contare:
/// un modello di linguaggio o un punto OAuth che rispondessero «307 altrove»
/// porterebbero altrove la richiesta — con il corpo, che è la conversazione o
/// il codice di autorizzazione. `ureq` toglie l'intestazione `Authorization`
/// da un rimando, ma non il corpo di un 307; e una richiesta che finisce dove
/// nessuno l'aveva mandata è un guasto anche quando non si porta dietro niente.
///
/// Quindi:
///
/// - [`Rimandi::Nessuno`] per chi porta un segreto o un corpo che conta: i
///   modelli di linguaggio, OAuth, lo scrobbling. Un 3xx arriva come risposta,
///   e siccome non è un 2xx diventa un errore che nomina dove si voleva
///   andare — vedi [`Rete::stato_a_errore`].
/// - [`Rimandi::Liberi`] per i servizi pubblici letti senza credenziali, dove il
///   rimando è il modo normale di servire un file.
/// - [`Rimandi::Sorvegliati`] per chi deve seguirli ma solo entro un confine:
///   i flussi dei cataloghi e gli scarichi di Drive.
#[derive(Debug, Clone)]
pub enum Rimandi {
    /// Nessun rimando: un 3xx è una risposta, e chi chiama la tratta come tale.
    Nessuno,
    /// Fino a tanti, seguiti da `ureq` senza chiedere niente a nessuno.
    ///
    /// L'intestazione `Authorization` non li attraversa (`ureq` la toglie da
    /// ogni rimando), e `https_only` vale anche per loro.
    Liberi(u32),
    /// Fino a `quanti`, seguiti **a mano**, e solo verso dove `cancello` dice di
    /// sì.
    ///
    /// Solo per le richieste senza corpo (`GET`): un `POST` che riceve un 3xx lo
    /// riceve come risposta, perché rimandare altrove un corpo è proprio la cosa
    /// da cui [`Rimandi::Nessuno`] esiste per proteggere.
    Sorvegliati {
        /// Quanti salti al massimo, uno dopo l'altro.
        quanti: u32,
        /// Chi dice se un salto si può fare.
        cancello: Cancello,
    },
}

/// Gli stati che vogliono dire «la cosa che cerchi è là».
///
/// Senza il 300 («scegli tu») e il 304 («ce l'hai già»): hanno un `Location`
/// solo per caso, e seguirli vorrebbe dire inventare una risposta.
const fn e_un_rimando(stato: u16) -> bool {
    matches!(stato, 301 | 302 | 303 | 307 | 308)
}

/// La configurazione TLS di tutti i client di Rymembra: l'archivio del sistema.
///
/// # Perché non l'elenco di Mozilla, che è quello di serie
///
/// Perché l'elenco di Mozilla è compilato dentro il programma e non sa niente
/// del computer su cui gira. Su un portatile aziendale il traffico passa da un
/// proxy che apre e richiude il TLS con un certificato che l'amministratore ha
/// messo nell'archivio di Windows: il browser se ne fida, l'aggiornamento di
/// Rymembra (che usa `reqwest` con l'archivio del sistema) se ne fida, e fino
/// alla 2.4.0 tutto il resto di Rymembra no — Drive, lo scrobbling, i cataloghi e
/// i modelli fallivano con un errore di certificato che nessuno poteva
/// correggere.
///
/// `rustls-platform-verifier` chiede a Windows di verificare la catena, con le
/// stesse regole — revoca compresa — che usa il sistema. Non allarga niente:
/// un certificato di cui Windows non si fida resta rifiutato. Sposta soltanto
/// la decisione dove sta quella di tutti gli altri programmi del computer.
///
/// Ha bisogno della funzione `platform-verifier` di `ureq`, dichiarata alla
/// radice del workspace: senza, `ureq` va in panico alla prima stretta di mano.
/// La prova `la_verifica_e_quella_del_sistema_e_non_va_in_panico` esiste per
/// accorgersene prima di un utente.
fn tls() -> ureq::tls::TlsConfig {
    ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build()
}

/// Quel che tutti i client hanno in comune, prima delle scadenze.
///
/// Le regole che valevano ripetute in quattro costruttori, adesso in un posto
/// solo: una riga dimenticata in uno dei quattro era un client con una regola
/// in meno, e nessuna prova l'avrebbe notato.
///
/// Zero rimandi a livello di agente, e non è la politica: è il pavimento. Ogni
/// richiesta dichiara la sua in base a [`Rimandi`], e se un giorno un metodo
/// nuovo si dimenticasse di farlo, quel metodo non seguirebbe niente invece di
/// seguire dieci salti verso chiunque.
fn di_base() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    ureq::Agent::config_builder()
        // Vedi la nota in testa al modulo: il corpo di un 4xx è
        // l'informazione, e trasformarlo in un `Err` lo butterebbe via.
        .http_status_as_error(false)
        // Nessuna richiesta di questo crate ha motivo di uscire in chiaro.
        // Non è teorico: senza, un `http://` scritto per sbaglio in una
        // costante manderebbe un token in rete leggibile da chiunque.
        .https_only(true)
        .max_redirects(0)
        // `PROPFIND`, per i server WebDAV: `ureq` di serie accetta solo i
        // metodi di HTTP/1.1. Accenderlo qui non apre niente: i metodi che
        // questo crate manda sono quelli di [`Metodo`], e nessuno può
        // scriverne un altro.
        .allow_non_standard_methods(true)
        .user_agent(AGENTE)
        .tls_config(tls())
}

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
    /// Chiedere le proprietà di una risorsa WebDAV (RFC 4918): l'elenco di
    /// una cartella, con dimensioni, date ed etichette. Un metodo che `ureq`
    /// non nomina, e che si manda per nome.
    Propfind,
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
            Self::Propfind => "PROPFIND",
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
    /// che è il caso di quasi tutte le richieste di Rymembra.
    ///
    /// Esiste per i link corti che l'app di Spotify mette nel foglio di
    /// condivisione (`spotify.link/…`): l'unico modo di sapere che cosa
    /// nominano è guardare dove portano.
    pub url_finale: String,
    /// L'intestazione `Range` della **risposta**.
    ///
    /// La terza delle intestazioni che cambiano il comportamento di qualcuno, e
    /// per un servizio solo: un caricamento ripartibile di Drive risponde `308
    /// Resume Incomplete` con `Range: bytes=0-N`, cioè «fin qui l'ho ricevuto».
    /// È l'unico modo di sapere da dove ripartire dopo un pezzo andato perso
    /// senza rimandare da capo venti megabyte. `None` quando manca, che per un
    /// `308` vuol dire «non ho ricevuto niente».
    pub intervallo: Option<String>,
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

/// Un client HTTP con le scadenze di Rymembra.
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
    /// Quali rimandi si seguono, e verso dove. Vedi [`Rimandi`].
    ///
    /// Sta qui e non nell'agente perché si cambia senza rifare l'agente:
    /// [`Rete::con_rimandi`] dà una copia con un'altra politica e **la stessa
    /// riserva di connessioni**. È quel che permette a `Drive::nuovo` di imporre
    /// la sua qualunque sia la rete che gli si passa.
    rimandi: Rimandi,
    /// Quanto può pesare un corpo raccolto intero da [`Rete::esegui`].
    ///
    /// Sta accanto ai rimandi e per la stessa ragione: si cambia senza rifare
    /// l'agente, con [`Rete::con_tetto_corpo`], e la riserva di connessioni
    /// resta quella. Vedi [`TETTO_CORPO_DI_SERIE`].
    tetto_corpo: u64,
}

/// Come Rymembra si presenta, sempre, a chiunque.
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
    "Rymembra/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/federicobaratti/rymembra)"
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

impl Rete {
    /// Un client con una scadenza complessiva.
    ///
    /// La scadenza è **globale**, cioè vale sull'intera richiesta e non su un
    /// singolo respiro del socket: è la forma che protegge davvero da un
    /// servizio che risponde un byte al secondo, e che una scadenza di lettura
    /// non copre.
    ///
    /// Segue i rimandi come ha sempre fatto ([`RIMANDI_DI_SERIE`]): chi porta
    /// un segreto se ne toglie il permesso con [`Self::con_rimandi`].
    #[must_use]
    pub fn nuova(servizio: &'static str, scadenza: Duration) -> Self {
        let configurazione = di_base().timeout_global(Some(scadenza)).build();
        Self {
            agente: configurazione.into(),
            servizio,
            chiaro_locale: false,
            rimandi: Rimandi::Liberi(RIMANDI_DI_SERIE),
            tetto_corpo: TETTO_CORPO_DI_SERIE,
        }
    }

    /// Una copia di questo client con un'altra politica dei rimandi.
    ///
    /// La riserva di connessioni resta **condivisa**: `ureq::Agent` si clona
    /// tenendo il suo dentro un `Arc`, e i rimandi si decidono richiesta per
    /// richiesta, non nell'agente. Costa quanto un clone.
    ///
    /// Un client di [`Self::verso`] verso questa macchina resta a zero
    /// qualunque cosa gli si chieda: lì il divieto non è una preferenza, è
    /// quel che impedisce a un server locale di mandare fuori di casa, in
    /// chiaro, una richiesta partita per `127.0.0.1`.
    #[must_use]
    pub fn con_rimandi(&self, rimandi: Rimandi) -> Self {
        let mut copia = self.clone();
        copia.rimandi = if self.chiaro_locale {
            Rimandi::Nessuno
        } else {
            rimandi
        };
        copia
    }

    /// Una copia di questo client che raccoglie corpi fino a `byte`.
    ///
    /// Come [`Self::con_rimandi`]: la riserva di connessioni resta condivisa, e
    /// costa quanto un clone. Serve a chi scarica file veri con
    /// [`Self::esegui`] — Drive, che riporta indietro skin da venti megabyte —
    /// e va detto dove lo si fa, non alzato per tutti: vedi
    /// [`TETTO_CORPO_DI_SERIE`].
    #[must_use]
    pub fn con_tetto_corpo(&self, byte: u64) -> Self {
        let mut copia = self.clone();
        copia.tetto_corpo = byte;
        copia
    }

    /// Quanto può pesare un corpo raccolto intero da questo client.
    ///
    /// Esiste per la stessa ragione di [`Self::rimandi`]: la differenza è
    /// invisibile da fuori, e chi ne alza uno deve poterlo provare.
    #[must_use]
    pub const fn tetto_corpo(&self) -> u64 {
        self.tetto_corpo
    }

    /// La politica dei rimandi di questo client.
    ///
    /// Esiste per la stessa ragione di [`Self::scadenza_complessiva`]: la
    /// differenza fra un client che segue i rimandi e uno che non li segue è
    /// invisibile da fuori, e chi ne tiene uno a zero deve poterlo **provare**.
    #[must_use]
    pub const fn rimandi(&self) -> &Rimandi {
        &self.rimandi
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
        // Le regole di `nuova` stanno in `di_base`, e valgono per le stesse
        // ragioni; qui cambiano solo le scadenze.
        let configurazione = di_base()
            // Nessuna scadenza complessiva: è il punto di questo costruttore.
            .timeout_global(None)
            .timeout_resolve(Some(PRELIEVO_RISOLUZIONE))
            .timeout_connect(Some(PRELIEVO_CONNESSIONE))
            .timeout_recv_response(Some(PRELIEVO_INTESTAZIONI))
            .timeout_recv_body(Some(PRELIEVO_CORPO))
            .build();
        Self {
            agente: configurazione.into(),
            servizio,
            chiaro_locale: false,
            // Un file dell'Internet Archive arriva **sempre** dopo un rimando:
            // `/download/…` risponde col nodo che lo tiene.
            rimandi: Rimandi::Liberi(RIMANDI_DI_SERIE),
            tetto_corpo: TETTO_CORPO_DI_SERIE,
        }
    }

    /// Un client per suonare un file mentre arriva.
    ///
    /// # Perché non basta né [`Self::nuova`] né [`Self::per_prelievo`]
    ///
    /// Perché un flusso sta in mezzo. Chiede pezzi di un file, non risposte da
    /// qualche kilobyte: la scadenza complessiva di `nuova` — trenta secondi
    /// per l'Internet Archive — è la stessa trappola che `per_prelievo` ha
    /// tolto ai prelievi, solo più piccola, e scattava davvero: una finestra
    /// da due megabyte su una linea lenta moriva a metà canzone. Ma non può
    /// nemmeno aspettare mezz'ora come un prelievo, perché dall'altra parte
    /// c'è qualcuno che sta ascoltando e un brano fermo per mezz'ora senza una
    /// parola è un'applicazione rotta.
    ///
    /// Quindi le stesse attese di `per_prelievo` per il nome, la connessione e
    /// le intestazioni, e per il corpo [`FLUSSO_CORPO`].
    ///
    /// # Nessun rimando, finché non arriva il cancello
    ///
    /// Un flusso segue i rimandi — l'Internet Archive non serve un file senza
    /// — ma solo verso dove il catalogo che l'ha dato è di casa, e chi lo sa è
    /// il catalogo, non questo client. Il cancello lo porta
    /// `crate::flusso::DaRete::nuova`; questa rete da sola non ne segue
    /// nessuno, così un flusso aperto senza cancello si ferma al primo «vai là»
    /// invece di andarci a occhi chiusi.
    #[must_use]
    pub fn per_flusso(servizio: &'static str) -> Self {
        let configurazione = di_base()
            .timeout_global(None)
            .timeout_resolve(Some(PRELIEVO_RISOLUZIONE))
            .timeout_connect(Some(PRELIEVO_CONNESSIONE))
            .timeout_recv_response(Some(PRELIEVO_INTESTAZIONI))
            .timeout_recv_body(Some(FLUSSO_CORPO))
            .build();
        Self {
            agente: configurazione.into(),
            servizio,
            chiaro_locale: false,
            rimandi: Rimandi::Nessuno,
            tetto_corpo: TETTO_CORPO_DI_SERIE,
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
    /// # Nessun rimando, nemmeno in `https://`
    ///
    /// Un client nato per un indirizzo preciso parla con **quell'**indirizzo.
    /// È il client dei modelli di linguaggio, e porta due cose che non devono
    /// finire altrove: la chiave, e la conversazione. `ureq` toglierebbe la
    /// prima da un rimando, ma un `307` rimanda il corpo — cioè tutto quel che
    /// l'utente ha scritto — a chiunque il servizio nomini. Un 3xx arriva quindi
    /// come risposta, e diventa un errore che dice dove si voleva andare.
    ///
    /// # Errori
    ///
    /// `net.badSchema` per qualunque indirizzo che non sia `https://`, oppure
    /// `http://` verso questa macchina.
    pub fn verso(servizio: &'static str, scadenza: Duration, url: &str) -> Result<Self, AppError> {
        if url.starts_with("https://") {
            return Ok(Self::nuova(servizio, scadenza).con_rimandi(Rimandi::Nessuno));
        }
        if !in_chiaro_ammesso(url) {
            return Err(non_in_casa(servizio, url));
        }
        let configurazione = di_base()
            .timeout_global(Some(scadenza))
            // L'unica riga di tutto l'albero che apre il chiaro, e sopra c'è la
            // sola condizione in cui è lecita: l'indirizzo è questa macchina.
            .https_only(false)
            // E nessun proxy, già nell'agente: un client che parla solo con
            // questa macchina non ha niente da chiedere a un proxy, che il
            // `127.0.0.1` di chi lo chiama non sa dove sia. `proxy_per` lo
            // direbbe comunque richiesta per richiesta; qui non resta nemmeno
            // da dirlo.
            .proxy(None)
            .build();
        Ok(Self {
            agente: configurazione.into(),
            servizio,
            chiaro_locale: true,
            // Zero salti, e non è prudenza generica: senza, un server locale
            // che risponde «302 http://esterno/» porterebbe fuori di casa, in
            // chiaro, una richiesta partita per la macchina di chi la manda —
            // cioè esattamente la cosa che il controllo sull'ospite impedisce
            // di scrivere e che un reindirizzamento rimetterebbe in piedi.
            // `con_rimandi` non può alzarlo: vedi lì.
            rimandi: Rimandi::Nessuno,
            tetto_corpo: TETTO_CORPO_DI_SERIE,
        })
    }
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

/// Dove porta un `Location`, scritto per intero.
///
/// Un servizio può scrivere un indirizzo intero, uno senza schema
/// (`//ospite/percorso`), un percorso assoluto (`/percorso`) o uno relativo
/// (`altro.mp3`): la RFC 9110 li ammette tutti, e un bersaglio lasciato a metà
/// non si potrebbe né chiamare né far giudicare al cancello, che guarda
/// l'ospite. I segmenti `..` non si riducono: non cambiano l'ospite, che è
/// l'unica cosa che qui si decide, e il servizio li capisce da sé.
///
/// `None` per una base che non ha la forma `schema://…`, che da qui non
/// dovrebbe mai passare.
fn risolvi_rimando(base: &str, posizione: &str) -> Option<String> {
    let posizione = posizione.trim();
    // Lo schema si confronta senza badare alle maiuscole, come dice la RFC
    // 3986; un `HTTPS://` non deve diventare un percorso relativo.
    let schema_esplicito = posizione.split_once("://").is_some_and(|(schema, _)| {
        !schema.is_empty()
            && schema
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    });
    if schema_esplicito {
        let (schema, resto) = posizione.split_once("://")?;
        return Some(format!("{}://{resto}", schema.to_ascii_lowercase()));
    }
    let (schema, resto) = base.split_once("://")?;
    if let Some(senza_schema) = posizione.strip_prefix("//") {
        return Some(format!("{schema}://{senza_schema}"));
    }
    let fine_autorita = resto.find(['/', '?', '#']).unwrap_or(resto.len());
    let autorita = resto.get(..fine_autorita)?;
    if posizione.starts_with('/') {
        return Some(format!("{schema}://{autorita}{posizione}"));
    }
    // Relativo: si sostituisce l'ultimo pezzo del percorso della base.
    let percorso = resto
        .get(fine_autorita..)
        .unwrap_or_default()
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let cartella = percorso
        .rfind('/')
        .map_or("/", |fine| percorso.get(..=fine).unwrap_or("/"));
    Some(format!("{schema}://{autorita}{cartella}{posizione}"))
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

/// Se una richiesta verso questo indirizzo va **diretta**, anche quando c'è un
/// proxy: la regola `<local>` di Windows, rifatta qui.
///
/// # Perché rifatta
///
/// Con «Non usare il proxy per gli indirizzi locali» Windows scrive `<local>`
/// in `ProxyOverride`, e vuol dire: diretto ogni nome **senza punto** — `nas`,
/// `diskstation`, il nome di un computer di casa o d'ufficio. Verso questa
/// macchina il proxy Windows non lo usa mai. `ureq` legge il resto di
/// `ProxyOverride` ma quella voce la butta via, e senza questa regola una
/// cartella WebDAV su `https://nas/`, che finché il proxy di Windows non si
/// leggeva andava diretta e funzionava, finirebbe al proxy — che il NAS di
/// solito non lo vede.
///
/// Vale sempre, anche quando in Windows la casella è spenta: un nome senza
/// punto si risolve solo nella rete di chi chiama, e mandarlo a un proxy
/// vorrebbe dire chiedergli un nome che non conosce.
///
/// Diretti: un nome senza punto (`localhost` compreso) e un indirizzo di
/// loopback (`127.x.x.x`, `::1`). Al proxy: un nome col punto (`nas.lan`), un
/// indirizzo IP che non è di loopback — `192.168.1.10` ci va anche per
/// Windows, a meno che `ProxyOverride` non lo nomini, e quella parte `ureq` la
/// legge — e un indirizzo senza ospite, che comunque non parte.
fn senza_proxy(url: &str) -> bool {
    let Some(nome) = ospite(url) else {
        return false;
    };
    // Un indirizzo IP si giudica da quel che è: `2001:db8::1` non ha punti, ma
    // non è un nome della rete di casa.
    if let Ok(indirizzo) = nome.parse::<std::net::IpAddr>() {
        return indirizzo.is_loopback();
    }
    !nome.contains('.')
}

/// L'ospite di un `http://`, e soltanto di un `http://`.
///
/// Il filtro sullo schema è il punto: `in_chiaro_ammesso` risponde «sì» a
/// `http://localhost` e deve rispondere «no» a `ftp://localhost`, che non è un
/// indirizzo per Rymembra.
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
///
/// Pubblica da quando la usano anche i cancelli dei rimandi, che stanno fuori
/// da qui (vedi [`Cancello`]) e decidono proprio sull'ospite: un secondo modo
/// di leggerlo, scritto altrove, sarebbe il posto in cui la chiocciola torna a
/// ingannare qualcuno. Le maiuscole restano come sono: confrontarle è compito
/// di chi confronta.
#[must_use]
pub fn ospite(url: &str) -> Option<&str> {
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

/// L'origine di un indirizzo: schema, ospite e porta, nella forma che si
/// confronta.
///
/// # A che cosa serve
///
/// A sapere se due indirizzi parlano con **lo stesso servizio**, che è la
/// domanda di chi tiene un segreto legato a un indirizzo: la chiave di un
/// modello si scrive per `https://openrouter.ai`, e se l'indirizzo di base
/// diventa un altro la chiave non deve seguirlo. Il percorso non conta —
/// `/api/v1` e `/api/v2` sono lo stesso servizio con la stessa chiave — mentre
/// lo schema, l'ospite e la porta sì: sono quel che decide chi riceve la
/// richiesta.
///
/// # La forma
///
/// `schema://ospite:porta`, sempre con la porta: `https://esempio.com` e
/// `https://esempio.com:443` sono la stessa origine, e senza scriverla
/// esplicita due forme dello stesso indirizzo sembrerebbero due servizi. Schema
/// e ospite in minuscolo, perché lì le maiuscole non contano; l'utente prima
/// dell'ultima chiocciola via, perché non fa parte di chi riceve.
///
/// `None` per quel che non è `http://` o `https://`, o non ha un ospite, o ha
/// una porta che non è un numero: un'origine che non si sa leggere non è
/// uguale a nessun'altra, ed è il verso giusto in cui sbagliare quando la
/// domanda è «posso mandare qui la chiave?».
#[must_use]
pub fn origine(url: &str) -> Option<String> {
    let url = url.trim();
    let (schema, resto) = url.split_once("://")?;
    let schema = schema.to_ascii_lowercase();
    let di_serie: u16 = match schema.as_str() {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    let fine = resto.find(['/', '?', '#']).unwrap_or(resto.len());
    let autorita = resto.get(..fine)?;
    let dopo_utente = autorita.rsplit('@').next()?;
    let (nudo, porta) = if let Some(dentro) = dopo_utente.strip_prefix('[') {
        // `[::1]:1234`: le parentesi tengono insieme i due punti
        // dell'indirizzo, e la porta sta dopo quella di chiusura.
        let (indirizzo, dopo) = dentro.split_once(']')?;
        let porta = match dopo.strip_prefix(':') {
            Some(numero) => Some(numero),
            None if dopo.is_empty() => None,
            None => return None,
        };
        (format!("[{}]", indirizzo.to_ascii_lowercase()), porta)
    } else {
        let (nome, porta) = match dopo_utente.split_once(':') {
            Some((nome, numero)) => (nome, Some(numero)),
            None => (dopo_utente, None),
        };
        (nome.to_ascii_lowercase(), porta)
    };
    if nudo.is_empty() || nudo == "[]" {
        return None;
    }
    let porta = match porta {
        None | Some("") => di_serie,
        Some(numero) => numero.parse::<u16>().ok()?,
    };
    Some(format!("{schema}://{nudo}:{porta}"))
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
mod prove;
