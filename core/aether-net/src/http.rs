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
//!
//! # Di chi ci si fida, e dove si va quando qualcuno dice «vai là»
//!
//! Due domande che fino alla 2.4.0 avevano la risposta di serie di `ureq`, e
//! nessuno l'aveva scelta.
//!
//! La prima è **quali certificati** valgono. `ureq` di serie porta con sé
//! l'elenco di Mozilla e ignora quello del sistema: su un computer d'ufficio,
//! dove un proxy apre e richiude il TLS con un certificato che l'azienda ha
//! messo nell'archivio di Windows, ogni richiesta di Aether falliva mentre il
//! browser accanto funzionava — e l'aggiornamento, che usa `reqwest` con
//! l'archivio del sistema, funzionava anche lui. Adesso decide l'archivio di
//! Windows per tutti e due: vedi `tls`, più sotto.
//!
//! La seconda è **quali rimandi** si seguono. Ogni client ne seguiva dieci, da
//! sé, verso qualunque indirizzo `https://`. Adesso ogni client dice la sua
//! politica, e quella di chi porta un segreto è «nessuno»: vedi [`Rimandi`].

use std::sync::Arc;
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
/// 2.4.0 valeva per tutti i client di Aether senza che nessuno l'avesse scelto,
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

/// Quanto tempo, al massimo, si spende a ripetere un intervallo.
///
/// Dall'altra parte c'è qualcuno che ascolta: tre tentativi da trenta secondi
/// l'uno — la scadenza delle intestazioni — farebbero un minuto e mezzo di
/// silenzio prima di dire che la rete non c'è. Passato questo bilancio il
/// guasto si racconta subito, anche se restavano tentativi.
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
/// `aether_catalogo::riconosci`, e un secondo elenco scritto qui sarebbe il
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

/// La configurazione TLS di tutti i client di Aether: l'archivio del sistema.
///
/// # Perché non l'elenco di Mozilla, che è quello di serie
///
/// Perché l'elenco di Mozilla è compilato dentro il programma e non sa niente
/// del computer su cui gira. Su un portatile aziendale il traffico passa da un
/// proxy che apre e richiude il TLS con un certificato che l'amministratore ha
/// messo nell'archivio di Windows: il browser se ne fida, l'aggiornamento di
/// Aether (che usa `reqwest` con l'archivio del sistema) se ne fida, e fino
/// alla 2.4.0 tutto il resto di Aether no — Drive, lo scrobbling, i cataloghi e
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
///
/// # Il `*`, e il ripiego che non c'è più
///
/// `bytes 0-1023/*` è legale e vuol dire «il totale non lo so». Fino alla 2.4.0
/// il numero dopo la barra non si leggeva, e la funzione **ripiegava** su
/// `Content-Length` — cioè sui 1024 byte del pezzo, chiamati lunghezza del file:
/// il brano finiva al primo intervallo, con la barra piena dopo sedici secondi.
/// Adesso una risposta che porta `Content-Range` dice il totale lì o non lo
/// dice affatto, e una risposta parziale senza `Content-Range` non lo dice
/// nemmeno lei: `Content-Length` vale come totale solo per un `200`, dove il
/// pezzo **è** il file.
fn totale_da(intestazioni: &ureq::http::HeaderMap, parziale: bool) -> Option<u64> {
    if let Some(intervallo) = intestazioni
        .get("content-range")
        .and_then(|v| v.to_str().ok())
    {
        // `bytes 0-1023/45678`: quel che serve è dopo la barra. Un `*`, o
        // qualunque cosa che non sia un numero, vuol dire «non lo so», e allora
        // non lo si sa nemmeno noi.
        return intervallo
            .rsplit_once('/')
            .and_then(|(_, coda)| coda.trim().parse::<u64>().ok());
    }
    if parziale {
        return None;
    }
    intestazioni
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
}

/// Legge un corpo fino a `quanti` byte, un blocco per volta.
///
/// Il tetto non è una prudenza: un servizio che ignora `Range` e risponde `200`
/// col file intero — dall'inizio lo si accetta, vedi [`Rete::intervallo`] —
/// manderebbe altrimenti in memoria un concerto intero, cioè la copia del brano
/// che `flusso` promette di non tenere. Arrivati al tetto si smette di leggere
/// e la connessione si chiude.
///
/// # Errori
///
/// `download.network` se la lettura si interrompe, `internal.aborted` se
/// `avanti` ha detto di no.
fn leggi_fino_a(
    lettore: &mut dyn std::io::Read,
    quanti: u64,
    avanti: &dyn Fn() -> bool,
) -> Result<Vec<u8>, AppError> {
    let tetto = usize::try_from(quanti).unwrap_or(usize::MAX);
    // La capacità no: `quanti` lo decide chi chiama, e allocare prima di sapere
    // quanto arriverà vorrebbe dire fidarsi di un numero che nessuno ha
    // controllato. Quattro megabyte bastano a qualunque finestra del flusso.
    let mut byte = Vec::with_capacity(tetto.min(4 * 1024 * 1024));
    let mut blocco = vec![0_u8; BLOCCO_PRELIEVO];
    while byte.len() < tetto {
        if !avanti() {
            return Err(AppError::new(ErrorCode::InternalAborted {
                what: Some("lettura di un intervallo".to_owned()),
            }));
        }
        let resta = tetto.saturating_sub(byte.len()).min(blocco.len());
        let Some(dove) = blocco.get_mut(..resta) else {
            break;
        };
        match lettore.read(dove) {
            Ok(0) => break,
            Ok(n) => byte.extend_from_slice(dove.get(..n).unwrap_or(&[])),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) => {
                return Err(AppError::new(ErrorCode::DownloadNetwork)
                    .with_cause(format!("lettura dell'intervallo interrotta: {err}")));
            }
        }
    }
    Ok(byte)
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

    /// Quanti rimandi `ureq` può seguire da sé in una richiesta di questo
    /// client.
    ///
    /// Zero per [`Rimandi::Nessuno`] e anche per [`Rimandi::Sorvegliati`]:
    /// quelli li segue [`Self::prendi`], a mano, uno per volta.
    const fn salti_automatici(&self) -> u32 {
        match self.rimandi {
            Rimandi::Liberi(quanti) => quanti,
            Rimandi::Nessuno | Rimandi::Sorvegliati { .. } => 0,
        }
    }

    /// Se verso questo indirizzo una richiesta può partire, per schema.
    ///
    /// `https://` sempre; `http://` solo su un client di [`Self::verso`] e solo
    /// verso questa macchina. È la stessa regola di `https_only` più
    /// [`Self::consenti`], detta prima di aprire un socket invece che dopo —
    /// serve ai rimandi seguiti a mano, dove il bersaglio lo sceglie il
    /// servizio e non chi chiama.
    fn schema_ammesso(&self, url: &str) -> bool {
        url.starts_with("https://") || (self.chiaro_locale && in_chiaro_ammesso(url))
    }

    /// Un `GET`, con i rimandi trattati come dice [`Rimandi`].
    ///
    /// Restituisce la risposta e l'indirizzo da cui è arrivata davvero.
    ///
    /// # Come si segue un rimando a mano
    ///
    /// Con [`Rimandi::Sorvegliati`] `ureq` non segue niente: un 3xx torna qui,
    /// il suo `Location` si risolve contro l'indirizzo che l'ha dato, e il
    /// bersaglio passa tre controlli **prima** che gli si chieda un byte — lo
    /// schema ([`Self::schema_ammesso`]), il cancello, e il conto dei salti. È
    /// la differenza fra un'allowlist che vale e una che vale solo per il primo
    /// indirizzo: seguendo da sé, `ureq` avrebbe già bussato al secondo quando
    /// qualcuno avesse potuto chiedersi se era il caso.
    ///
    /// Dal secondo salto in poi `Authorization` e `Cookie` non viaggiano più,
    /// come quando i rimandi li segue `ureq` (`RedirectAuthHeaders::Never`, il
    /// suo valore di serie): chi riceve il salto non è chi ha ricevuto la
    /// chiave, e gli indirizzi a cui Google e l'Internet Archive rimandano
    /// portano già dentro il permesso che serve.
    ///
    /// # Errori
    ///
    /// `net.*` per i guasti di trasporto; `net.badSchema` per un rimando che il
    /// cancello o lo schema rifiutano, e per una catena più lunga del conto. Un
    /// errore nomina l'**ospite** del bersaglio e non l'indirizzo intero: quelli
    /// a cui si viene rimandati portano spesso una firma nella query, e una
    /// firma non va nel diario.
    fn prendi(
        &self,
        url: &str,
        intestazioni: &[(&str, &str)],
    ) -> Result<(ureq::http::Response<ureq::Body>, String), AppError> {
        self.consenti(url)?;
        let (quanti, cancello) = match &self.rimandi {
            Rimandi::Sorvegliati { quanti, cancello } => (*quanti, cancello),
            Rimandi::Nessuno | Rimandi::Liberi(_) => {
                let risposta = self.chiama_get(url, intestazioni, self.salti_automatici(), url)?;
                // `ureq` segue i rimandi da sé: qui si legge soltanto dove è
                // finita. Va preso **prima** del corpo, che consuma la risposta.
                let arrivato = risposta.get_uri().to_string();
                return Ok((risposta, arrivato));
            }
        };

        let mut dove = url.to_owned();
        let mut salti: u32 = 0;
        loop {
            // Al primo giro le intestazioni di chi chiama, dal secondo quelle
            // che un rimando può portare con sé: vedi sopra.
            let portabili: Vec<(&str, &str)> = intestazioni
                .iter()
                .filter(|(nome, _)| {
                    salti == 0
                        || !(nome.eq_ignore_ascii_case("authorization")
                            || nome.eq_ignore_ascii_case("cookie"))
                })
                .copied()
                .collect();
            let risposta = self.chiama_get(&dove, &portabili, 0, url)?;
            let stato = risposta.status().as_u16();
            let posizione = risposta
                .headers()
                .get("location")
                .and_then(|valore| valore.to_str().ok());
            // Un 3xx senza un bersaglio leggibile non è un rimando che si possa
            // seguire: torna com'è, e chi chiama lo tratta da stato di errore.
            let (true, Some(posizione)) = (e_un_rimando(stato), posizione) else {
                return Ok((risposta, dove));
            };
            let Some(bersaglio) = risolvi_rimando(&dove, posizione) else {
                return Ok((risposta, dove));
            };
            if salti >= quanti {
                return Err(rimando_rifiutato(
                    self.servizio,
                    &bersaglio,
                    &format!("più di {quanti} rimandi di fila"),
                ));
            }
            if !self.schema_ammesso(&bersaglio) {
                return Err(rimando_rifiutato(
                    self.servizio,
                    &bersaglio,
                    "il rimando porta fuori da https",
                ));
            }
            if !cancello.ammette(&bersaglio) {
                return Err(rimando_rifiutato(
                    self.servizio,
                    &bersaglio,
                    "il rimando porta dove questa richiesta non è invitata",
                ));
            }
            dove = bersaglio;
            salti = salti.saturating_add(1);
        }
    }

    /// Un `GET` solo, con un tetto di salti per `ureq`.
    ///
    /// `per_errore` è l'indirizzo da nominare se il trasporto cade: quello che
    /// chi chiama ha chiesto, non quello di un salto — vedi [`Self::prendi`]
    /// sulle firme nelle query.
    fn chiama_get(
        &self,
        url: &str,
        intestazioni: &[(&str, &str)],
        salti: u32,
        per_errore: &str,
    ) -> Result<ureq::http::Response<ureq::Body>, AppError> {
        let mut costruttore = self.agente.get(url).config().max_redirects(salti).build();
        for (nome, valore) in intestazioni {
            costruttore = costruttore.header(*nome, *valore);
        }
        costruttore
            .call()
            .map_err(|err| self.trasporto(&err, per_errore))
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
        // I rami non si possono unire: `ureq` distingue nel **tipo** un
        // costruttore che può portare un corpo da uno che non può, e ci si
        // guadagna che `GET` con un corpo non è una cosa che si possa scrivere
        // per sbaglio.
        let salti = self.salti_automatici();
        let (mut risposta, url_finale) = match richiesta.metodo {
            // Il solo metodo che può seguire rimandi sorvegliati: vedi
            // [`Rimandi::Sorvegliati`].
            Metodo::Get => self.prendi(richiesta.url, richiesta.intestazioni)?,
            Metodo::Delete => {
                let mut costruttore = self
                    .agente
                    .delete(richiesta.url)
                    .config()
                    .max_redirects(salti)
                    .build();
                for (nome, valore) in richiesta.intestazioni {
                    costruttore = costruttore.header(*nome, *valore);
                }
                let risposta = costruttore
                    .call()
                    .map_err(|err| self.trasporto(&err, richiesta.url))?;
                let arrivato = risposta.get_uri().to_string();
                (risposta, arrivato)
            }
            Metodo::Post | Metodo::Patch | Metodo::Put => {
                let costruttore = match richiesta.metodo {
                    Metodo::Patch => self.agente.patch(richiesta.url),
                    Metodo::Put => self.agente.put(richiesta.url),
                    _ => self.agente.post(richiesta.url),
                };
                let mut costruttore = costruttore.config().max_redirects(salti).build();
                for (nome, valore) in richiesta.intestazioni {
                    costruttore = costruttore.header(*nome, *valore);
                }
                let esito = match richiesta.corpo {
                    Corpo::Niente => costruttore.send_empty(),
                    Corpo::Byte { tipo, dati } => costruttore.content_type(tipo).send(dati),
                    Corpo::Modulo(campi) => costruttore
                        .content_type("application/x-www-form-urlencoded")
                        .send(modulo_urlencoded(campi).as_bytes()),
                };
                let risposta = esito.map_err(|err| self.trasporto(&err, richiesta.url))?;
                let arrivato = risposta.get_uri().to_string();
                (risposta, arrivato)
            }
        };

        let stato = risposta.status().as_u16();
        let riprova_fra_ms = quanto_ha_chiesto(risposta.headers());
        let posizione = risposta
            .headers()
            .get("location")
            .and_then(|valore| valore.to_str().ok())
            .map(ToOwned::to_owned);
        let intervallo = intestazione(risposta.headers(), "range");
        // Il tetto di questo client e non quello di `ureq`, che è di dieci
        // megabyte e non lo aveva scelto nessuno: vedi `TETTO_CORPO_DI_SERIE`.
        let corpo = risposta
            .body_mut()
            .with_config()
            .limit(self.tetto_corpo)
            .read_to_vec()
            .map_err(|err| self.trasporto(&err, richiesta.url))?;

        Ok(Risposta {
            stato,
            corpo,
            riprova_fra_ms,
            posizione,
            url_finale,
            intervallo,
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
        let (mut risposta, _) = self.prendi(url, intestazioni)?;

        let stato = risposta.status().as_u16();
        if !(200..300).contains(&stato) {
            return Err(self.stato_a_errore(&senza_corpo(&risposta, stato, url), url));
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
        // Un `POST` non segue rimandi sorvegliati — il corpo non si rimanda
        // altrove — e con `Rimandi::Nessuno`, che è quel che ha il client dei
        // modelli, non ne segue nessuno: un 3xx arriva qui sotto come stato.
        let mut costruttore = self
            .agente
            .post(url)
            .config()
            .max_redirects(self.salti_automatici())
            .build();
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
            let mut guasto = senza_corpo(&risposta, stato, url);
            // Il corpo si legge **prima** di tradurre lo stato: è lì che il
            // servizio scrive che quel modello non esiste, e senza resterebbe
            // un «400» buono per qualunque cosa.
            guasto.corpo = risposta.body_mut().read_to_vec().unwrap_or_default();
            return Err(self.stato_a_errore(&guasto, url));
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
        self.intervallo_finche(url, da, quanti, &|| true)
    }

    /// Come [`Self::intervallo`], ma si lascia a metà quando `avanti` dice di no.
    ///
    /// Serve alla lettura anticipata di `flusso`: la finestra dopo si chiede
    /// mentre si suona quella di adesso, e se chi ascolta salta altrove quella
    /// richiesta non la aspetta più nessuno. Lasciarla arrivare in fondo
    /// vorrebbe dire due megabyte che si contendono la linea con la finestra
    /// che invece serve — su una linea lenta, decine di secondi di salto in più.
    ///
    /// La domanda si fa fra un blocco e l'altro, come in [`Self::preleva`], e
    /// anche durante la pausa fra due tentativi.
    ///
    /// # Tentativi, e fino a quando
    ///
    /// Un intervallo che cade per un guasto che passa da sé — la connessione
    /// rifiutata un istante, un `503` del nodo, una lettura interrotta a metà —
    /// si richiede, con l'attesa che raddoppia da un secondo: è
    /// [`Self::ritenta`], con la sola differenza che la pausa si interrompe se
    /// chi aspetta se ne va. Fino alla 2.4.0 non si ritentava niente, e il primo
    /// `ECONNRESET` di un proxy fermava il brano.
    ///
    /// Tre tentativi in tutto ([`TENTATIVI_INTERVALLO`]) e non oltre
    /// [`BILANCIO_INTERVALLO`] di tempo speso: dall'altra parte c'è qualcuno che
    /// ascolta, e un guasto che non passa va detto. Non si ripete un intervallo
    /// **ignorato** (`download.badResponse`): il servizio che risponde `200` a
    /// una domanda da metà file risponderà `200` anche la seconda volta.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::intervallo`], e `internal.aborted` quando `avanti` ha
    /// detto di no.
    pub fn intervallo_finche(
        &self,
        url: &str,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        let partenza = std::time::Instant::now();
        self.ripeti(
            TENTATIVI_INTERVALLO,
            avanti,
            &|err: &AppError| {
                err.is_retryable()
                    && !matches!(err.code(), ErrorCode::DownloadBadResponse)
                    && partenza.elapsed() < BILANCIO_INTERVALLO
            },
            || self.intervallo_una_volta(url, da, quanti, avanti),
        )
    }

    /// Una richiesta di intervallo, senza tentativi: il corpo di
    /// [`Self::intervallo_finche`].
    fn intervallo_una_volta(
        &self,
        url: &str,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        let fino = da.saturating_add(quanti.max(1)).saturating_sub(1);
        let intervallo = format!("bytes={da}-{fino}");
        // I rimandi li decide la politica di questo client: per un flusso
        // `DaRete` la mette a `Sorvegliati`, col cancello del catalogo, e
        // `prendi` fa passare ogni bersaglio da lì prima di chiedergli un byte.
        let (mut risposta, arrivato) = self.prendi(url, &[("range", &intervallo)])?;

        let stato = risposta.status().as_u16();
        if !(200..300).contains(&stato) {
            return Err(self.stato_a_errore(&senza_corpo(&risposta, stato, url), url));
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

        let totale = totale_da(risposta.headers(), stato == 206);
        let byte = leggi_fino_a(&mut risposta.body_mut().as_reader(), quanti, avanti)?;
        // L'indirizzo nuovo è passato **dallo stesso cancello** di quello
        // chiesto, e prima che gli si chiedesse un byte: un rimando è pur
        // sempre qualcuno che dice «vai là», e seguirlo senza ricontrollare
        // vorrebbe dire che l'allowlist vale per il primo indirizzo e non per
        // il secondo — cioè non vale. Fino alla 2.4.0 questo commento lo
        // prometteva e il codice guardava solo lo schema, **dopo** che `ureq`
        // aveva già seguito il rimando da sé; adesso lo fa `prendi`, un salto
        // alla volta. Qui si ricontrolla soltanto lo schema, per il caso di un
        // client con `Rimandi::Liberi` in cui a seguire è stato `ureq`: chi
        // tiene l'indirizzo per le richieste dopo non deve potersene tenere uno
        // che non potrebbe chiamare.
        let url_finale = (arrivato != url && self.schema_ammesso(&arrivato)).then_some(arrivato);
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
            // Un corpo più grande del tetto di questo client. Non è «la forma
            // non è quella attesa», che è la frase del ripiego qui sotto: la
            // forma andava benissimo, era il peso. Detto in megabyte, che è
            // l'unità in cui chi legge un diario pensa a un file.
            ureq::Error::BodyExceedsLimit(tetto) => AppError::new(ErrorCode::NetBadSchema {
                service: Some(self.servizio.to_owned()),
                detail: Some(format!(
                    "la risposta supera il tetto di {} MB che questo client si dà",
                    tetto >> 20
                )),
            }),
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
        // Un rimando che nessuno ha seguito. Arriva qui da un client con
        // `Rimandi::Nessuno`, o da un rimando che la politica non permetteva di
        // seguire (un `POST`, un 3xx senza bersaglio), e il corpo di un 3xx di
        // solito è vuoto o una pagina HTML di cortesia: la cosa da dire è
        // **dove** il servizio voleva mandare la richiesta, perché è quel che
        // chi legge deve correggere — tipicamente l'indirizzo di base di un
        // modello, scritto senza la barra o col dominio vecchio. Ospite e
        // percorso, non la query: vedi `senza_query`.
        if (300..400).contains(&risposta.stato) {
            let verso = risposta
                .posizione
                .as_deref()
                .map_or_else(|| "nessun indirizzo".to_owned(), senza_query);
            return errore.with_cause(format!(
                "il servizio rimanda altrove ({}: «{}») e questa richiesta non segue rimandi: \
                 l'indirizzo va corretto dove è scritto",
                risposta.stato,
                accorcia(&verso, 200)
            ));
        }
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
        operazione: impl FnMut() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.ripeti(tentativi, &|| true, &AppError::is_retryable, operazione)
    }

    /// Come [`Self::ritenta`], ma la pausa fra due tentativi si lascia a metà
    /// quando `avanti` dice di no.
    ///
    /// Serve a chi ritenta mentre qualcuno aspetta e può smettere di
    /// aspettare: un caricamento che l'utente annulla, una finestra di un
    /// flusso da cui chi ascolta è saltato via. Un secondo di `sleep` in quei
    /// casi è un secondo in cui il gesto non ha effetto.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::ritenta`], e `internal.aborted` se `avanti` ha detto di
    /// no durante una pausa.
    pub fn ritenta_finche<T>(
        &self,
        tentativi: u32,
        avanti: &dyn Fn() -> bool,
        operazione: impl FnMut() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.ripeti(tentativi, avanti, &AppError::is_retryable, operazione)
    }

    /// Il ciclo di tutti i tentativi, con il predicato che decide quali errori
    /// si ripetono.
    ///
    /// Uno solo, per [`Self::ritenta`], [`Self::ritenta_finche`] e gli
    /// intervalli dei flussi: tre cicli con la stessa attesa che raddoppia sono
    /// tre cicli che divergono il giorno in cui qualcuno ne aggiusta uno.
    fn ripeti<T>(
        &self,
        tentativi: u32,
        avanti: &dyn Fn() -> bool,
        ritentabile: &dyn Fn(&AppError) -> bool,
        mut operazione: impl FnMut() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut attesa = Duration::from_secs(1);
        for _ in 1..tentativi.max(1) {
            match operazione() {
                Ok(valore) => return Ok(valore),
                Err(err) if ritentabile(&err) => {
                    if !dormi_finche(quanto_aspettare(&err, attesa), avanti) {
                        return Err(AppError::new(ErrorCode::InternalAborted {
                            what: Some(format!("{}: attesa fra due tentativi", self.servizio)),
                        })
                        .with_cause(format!("lasciato dopo {}", err.code().kind().code())));
                    }
                    attesa = (attesa * 2).min(ATTESA_MASSIMA);
                }
                Err(err) => return Err(err),
            }
        }
        operazione()
    }
}

/// Dorme `quanto`, un [`PASSO_ATTESA`] per volta, finché `avanti` dice di sì.
///
/// `false` se `avanti` ha detto di no prima della fine: chi chiama smette.
/// Un `sleep` solo sarebbe più corto da scrivere, e sordo per tutta la sua
/// durata.
fn dormi_finche(quanto: Duration, avanti: &dyn Fn() -> bool) -> bool {
    let fine = std::time::Instant::now() + quanto;
    loop {
        if !avanti() {
            return false;
        }
        let adesso = std::time::Instant::now();
        if adesso >= fine {
            return true;
        }
        std::thread::sleep(fine.saturating_duration_since(adesso).min(PASSO_ATTESA));
    }
}

/// Un'intestazione di risposta come testo, se c'è e si legge.
fn intestazione(intestazioni: &ureq::http::HeaderMap, nome: &str) -> Option<String> {
    intestazioni
        .get(nome)
        .and_then(|valore| valore.to_str().ok())
        .map(ToOwned::to_owned)
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

/// Una [`Risposta`] senza corpo, per tradurre in errore uno stato fuori dal 2xx.
///
/// Serve ai tre metodi che leggono il corpo a pezzi e non lo tengono: a loro
/// resta da dire lo stato, il tempo chiesto e — se era un rimando — dove
/// portava. Senza `posizione` un 3xx diventerebbe un «errore 302» muto.
fn senza_corpo(risposta: &ureq::http::Response<ureq::Body>, stato: u16, url: &str) -> Risposta {
    Risposta {
        stato,
        corpo: Vec::new(),
        riprova_fra_ms: quanto_ha_chiesto(risposta.headers()),
        posizione: risposta
            .headers()
            .get("location")
            .and_then(|valore| valore.to_str().ok())
            .map(ToOwned::to_owned),
        url_finale: url.to_owned(),
        intervallo: intestazione(risposta.headers(), "range"),
    }
}

/// Il rifiuto di un rimando.
///
/// `net.badSchema`, non ritentabile: un rimando che porta dove non si deve
/// andare porterà lì anche la prossima volta. Nomina l'ospite e non
/// l'indirizzo: vedi [`Rete::prendi`].
fn rimando_rifiutato(servizio: &str, bersaglio: &str, perche: &str) -> AppError {
    AppError::new(ErrorCode::NetBadSchema {
        service: Some(servizio.to_owned()),
        detail: Some(format!(
            "{perche}: «{}»",
            ospite(bersaglio).unwrap_or("un indirizzo senza ospite")
        )),
    })
}

/// Un indirizzo senza query e senza frammento.
///
/// Per scriverlo in un errore: gli indirizzi a cui un servizio rimanda portano
/// spesso nella query una firma a tempo — quelli di Google hanno `qk=`, quelli
/// dei nodi di Audius `signature=` — e una firma finita nel diario è una firma
/// che chi legge il diario può usare finché vale.
fn senza_query(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or_default().to_owned()
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
            intervallo: None,
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
    fn la_rete_dei_flussi_non_ha_una_scadenza_complessiva() {
        // Il guasto trovato ascoltando: la finestra da due megabyte chiesta con
        // i trenta secondi complessivi delle domande, su una linea da
        // cinquanta kilobyte al secondo, moriva a metà canzone.
        let rete = Rete::per_flusso("prova");
        assert_eq!(rete.scadenza_complessiva(), None);
        let scadenze = rete.agente.config().timeouts();
        assert_eq!(scadenze.connect, Some(PRELIEVO_CONNESSIONE));
        assert_eq!(scadenze.recv_body, Some(FLUSSO_CORPO));
        // E la finestra più grande passa entro il tetto sotto i duecento
        // kilobit al secondo, che è meno di quel che un MP3 chiede per suonare.
        let finestra = crate::flusso::FINESTRA_MASSIMA as f64;
        let kilobit_al_secondo = finestra * 8.0 / 1000.0 / FLUSSO_CORPO.as_secs() as f64;
        assert!(
            kilobit_al_secondo < 200.0,
            "due megabyte in {} secondi vogliono {kilobit_al_secondo:.0} kbit/s",
            FLUSSO_CORPO.as_secs()
        );
    }

    #[test]
    fn un_corpo_si_legge_fino_al_tetto_e_non_oltre() {
        // Un servizio che ignora `Range` manda il file intero: se ne prende
        // quel che si è chiesto, non un concerto in memoria.
        let dati = vec![5_u8; 300_000];
        let letti = leggi_fino_a(&mut dati.as_slice(), 100_000, &|| true);
        assert_eq!(letti.map(|b| b.len()), Ok(100_000));
        // Un corpo più corto del tetto è la fine del file, non un guasto.
        let letti = leggi_fino_a(&mut dati.as_slice(), 1_000_000, &|| true);
        assert_eq!(letti.map(|b| b.len()), Ok(300_000));
    }

    #[test]
    fn una_lettura_che_nessuno_aspetta_si_lascia_a_meta() {
        let dati = vec![5_u8; 300_000];
        let esito = leggi_fino_a(&mut dati.as_slice(), 300_000, &|| false);
        assert!(
            matches!(
                esito.as_ref().map_err(AppError::code),
                Err(ErrorCode::InternalAborted { .. })
            ),
            "doveva lasciarla: {:?}",
            esito.map(|b| b.len())
        );
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

    /// Un servitore HTTP finto su questa macchina, una connessione per
    /// richiesta.
    ///
    /// Le prove dei rimandi non possono girare contro un servizio vero — non
    /// devono uscire da questo computer — e la cosa da provare è proprio chi
    /// riceve che cosa: quante richieste, a quale percorso, con quali
    /// intestazioni. Per questo ogni testa di richiesta ricevuta si conserva.
    struct Servitore {
        base: String,
        visti: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Servitore {
        fn avvia(risponde: fn(&str) -> String) -> Self {
            use std::io::{Read as _, Write as _};
            let ascolto =
                std::net::TcpListener::bind("127.0.0.1:0").expect("una porta su questa macchina");
            let porta = ascolto.local_addr().expect("la porta scelta").port();
            let visti = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let registro = std::sync::Arc::clone(&visti);
            std::thread::spawn(move || {
                for connessione in ascolto.incoming() {
                    let Ok(mut connessione) = connessione else {
                        continue;
                    };
                    let mut testa = Vec::new();
                    let mut byte = [0_u8; 1];
                    while !testa.ends_with(b"\r\n\r\n") {
                        match connessione.read(&mut byte) {
                            Ok(1) => testa.push(byte[0]),
                            _ => break,
                        }
                    }
                    let testa = String::from_utf8_lossy(&testa).into_owned();
                    let percorso = testa.split(' ').nth(1).unwrap_or("/").to_owned();
                    registro.lock().unwrap().push(testa);
                    let _ = connessione.write_all(risponde(&percorso).as_bytes());
                }
            });
            Self {
                base: format!("http://127.0.0.1:{porta}"),
                visti,
            }
        }

        fn richieste(&self) -> Vec<String> {
            self.visti.lock().unwrap().clone()
        }

        /// Un client verso questo servitore, con questa politica.
        ///
        /// Costruito a mano apposta: `con_rimandi` su un client in chiaro
        /// rimette `Nessuno` — ed è giusto, vedi lì — ma qui serve provare il
        /// giro dei rimandi sorvegliati senza uscire da questo computer.
        fn cliente(&self, rimandi: Rimandi) -> Rete {
            let base = Rete::verso("prova", Duration::from_secs(5), &self.base)
                .expect("un indirizzo di casa costruisce");
            Rete { rimandi, ..base }
        }
    }

    fn rimando(verso: &str) -> String {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {verso}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
    }

    fn pezzo(corpo: &str) -> String {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-{}/{}\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{corpo}",
            corpo.len().saturating_sub(1),
            corpo.len(),
            corpo.len()
        )
    }

    fn risponde_con_un_salto(percorso: &str) -> String {
        match percorso {
            "/uno" => rimando("/due?firma=segreta"),
            "/due?firma=segreta" => pezzo("ciao"),
            _ => rimando("/uno"),
        }
    }

    #[test]
    fn ogni_client_ha_la_sua_politica_dei_rimandi() {
        let scadenza = Duration::from_secs(1);
        assert!(matches!(
            Rete::nuova("catalogo", scadenza).rimandi(),
            Rimandi::Liberi(RIMANDI_DI_SERIE)
        ));
        assert!(matches!(
            Rete::per_prelievo("catalogo").rimandi(),
            Rimandi::Liberi(RIMANDI_DI_SERIE)
        ));
        assert!(
            matches!(Rete::per_flusso("catalogo").rimandi(), Rimandi::Nessuno),
            "un flusso segue rimandi solo col cancello che gli dà `DaRete`"
        );
        let remoto = Rete::verso("ia", scadenza, "https://openrouter.ai/api/v1").unwrap();
        assert!(
            matches!(remoto.rimandi(), Rimandi::Nessuno),
            "il client dei modelli porta la chiave e la conversazione: nessun rimando"
        );
        let locale = Rete::verso("ia", scadenza, "http://127.0.0.1:11434/v1").unwrap();
        assert!(matches!(
            locale.con_rimandi(Rimandi::Liberi(3)).rimandi(),
            Rimandi::Nessuno
        ));
    }

    #[test]
    fn senza_rimandi_un_3xx_arriva_come_risposta_e_diventa_un_errore_chiaro() {
        let servitore = Servitore::avvia(risponde_con_un_salto);
        let rete = servitore.cliente(Rimandi::Nessuno);
        let url = format!("{}/uno", servitore.base);
        let risposta = rete
            .esegui(Richiesta {
                metodo: Metodo::Get,
                url: &url,
                intestazioni: &[],
                corpo: Corpo::Niente,
            })
            .expect("un 302 è una risposta, non un guasto di trasporto");
        assert_eq!(risposta.stato, 302);
        assert!(!risposta.e_andata(), "un rimando non è una riuscita");
        assert_eq!(
            servitore.richieste().len(),
            1,
            "il bersaglio non deve aver ricevuto niente"
        );

        let errore = rete.stato_a_errore(&risposta, &url);
        assert_eq!(errore.code().kind().code(), "net.http");
        assert!(
            !errore.is_retryable(),
            "un rimando non si aggiusta ritentando"
        );
        let causa = errore.cause().unwrap_or_default();
        assert!(
            causa.contains("/due"),
            "la causa dice dove si voleva andare: {causa}"
        );
        assert!(
            !causa.contains("segreta"),
            "la query di un rimando può portare una firma, e non va nel diario: {causa}"
        );
    }

    #[test]
    fn un_rimando_sorvegliato_si_segue_se_il_cancello_dice_di_si() {
        let servitore = Servitore::avvia(risponde_con_un_salto);
        let rete = servitore.cliente(Rimandi::Sorvegliati {
            quanti: 3,
            cancello: Cancello::nuovo(|_| true),
        });
        let url = format!("{}/uno", servitore.base);
        let pezzo = rete.intervallo(&url, 0, 4).expect("il salto è ammesso");
        assert_eq!(pezzo.byte, b"ciao");
        assert_eq!(
            pezzo.url_finale.as_deref(),
            Some(format!("{}/due?firma=segreta", servitore.base).as_str()),
            "il bersaglio si ricorda, così la prossima finestra non ripaga il salto"
        );
        let richieste = servitore.richieste();
        assert_eq!(richieste.len(), 2);
        assert!(
            richieste
                .iter()
                .all(|r| r.to_lowercase().contains("range: bytes=0-3")),
            "l'intervallo viaggia con ogni salto"
        );
    }

    #[test]
    fn un_rimando_che_il_cancello_rifiuta_non_riceve_nemmeno_la_richiesta() {
        let servitore = Servitore::avvia(risponde_con_un_salto);
        let rete = servitore.cliente(Rimandi::Sorvegliati {
            quanti: 3,
            cancello: Cancello::nuovo(|bersaglio| !bersaglio.contains("/due")),
        });
        let url = format!("{}/uno", servitore.base);
        let errore = rete.intervallo(&url, 0, 4).unwrap_err();
        assert_eq!(errore.code().kind().code(), "net.badSchema");
        assert!(!errore.is_retryable());
        assert_eq!(
            servitore.richieste().len(),
            1,
            "il cancello si interroga **prima** del salto: fino alla 2.4.0 `ureq` \
             aveva già bussato al bersaglio quando si guardava dove fosse"
        );
    }

    #[test]
    fn un_rimando_fuori_da_casa_in_chiaro_non_si_segue() {
        fn fuori(_: &str) -> String {
            rimando("http://esempio.com/brano.mp3")
        }
        let servitore = Servitore::avvia(fuori);
        let rete = servitore.cliente(Rimandi::Sorvegliati {
            quanti: 3,
            cancello: Cancello::nuovo(|_| true),
        });
        let errore = rete
            .intervallo(&format!("{}/uno", servitore.base), 0, 4)
            .unwrap_err();
        assert_eq!(
            errore.code().kind().code(),
            "net.badSchema",
            "anche con un cancello che ammette tutto, il chiaro fuori di casa non passa"
        );
    }

    #[test]
    fn dal_secondo_salto_la_chiave_non_viaggia_piu() {
        let servitore = Servitore::avvia(risponde_con_un_salto);
        let rete = servitore.cliente(Rimandi::Sorvegliati {
            quanti: 3,
            cancello: Cancello::nuovo(|_| true),
        });
        let url = format!("{}/uno", servitore.base);
        let risposta = rete
            .esegui(Richiesta {
                metodo: Metodo::Get,
                url: &url,
                intestazioni: &[("Authorization", "Bearer segreto"), ("Accept", "*/*")],
                corpo: Corpo::Niente,
            })
            .expect("il salto è ammesso");
        assert_eq!(risposta.stato, 206);
        let richieste = servitore.richieste();
        assert_eq!(richieste.len(), 2);
        assert!(richieste[0].contains("Bearer segreto"), "{}", richieste[0]);
        assert!(
            !richieste[1].contains("Bearer segreto"),
            "chi riceve il salto non è chi ha ricevuto la chiave: {}",
            richieste[1]
        );
    }

    #[test]
    fn una_catena_di_rimandi_troppo_lunga_si_ferma() {
        fn giro(_: &str) -> String {
            rimando("/giro")
        }
        let servitore = Servitore::avvia(giro);
        let rete = servitore.cliente(Rimandi::Sorvegliati {
            quanti: 2,
            cancello: Cancello::nuovo(|_| true),
        });
        let errore = rete
            .intervallo(&format!("{}/giro", servitore.base), 0, 4)
            .unwrap_err();
        assert_eq!(errore.code().kind().code(), "net.badSchema");
        assert_eq!(
            servitore.richieste().len(),
            3,
            "la prima richiesta e due salti, e poi basta"
        );
    }

    #[test]
    fn un_bersaglio_si_risolve_contro_chi_lo_ha_dato() {
        let base = "https://archive.org/download/gd77/t01.mp3?x=1";
        assert_eq!(
            risolvi_rimando(
                base,
                "https://ia800305.us.archive.org/27/items/gd77/t01.mp3"
            )
            .as_deref(),
            Some("https://ia800305.us.archive.org/27/items/gd77/t01.mp3")
        );
        assert_eq!(
            risolvi_rimando(base, "HTTPS://Nodo.esempio/x").as_deref(),
            Some("https://Nodo.esempio/x"),
            "lo schema non bada alle maiuscole"
        );
        assert_eq!(
            risolvi_rimando(base, "//nodo.esempio/x").as_deref(),
            Some("https://nodo.esempio/x")
        );
        assert_eq!(
            risolvi_rimando(base, "/27/items/gd77/t01.mp3").as_deref(),
            Some("https://archive.org/27/items/gd77/t01.mp3")
        );
        assert_eq!(
            risolvi_rimando(base, "t02.mp3").as_deref(),
            Some("https://archive.org/download/gd77/t02.mp3")
        );
        assert_eq!(
            risolvi_rimando(base, "/a?u=https://esempio.com").as_deref(),
            Some("https://archive.org/a?u=https://esempio.com"),
            "un indirizzo dentro una query non è lo schema del bersaglio"
        );
    }

    #[test]
    fn lorigine_e_schema_ospite_e_porta() {
        let stessa = |a: &str, b: &str| origine(a).is_some() && origine(a) == origine(b);
        assert_eq!(
            origine("https://openrouter.ai/api/v1").as_deref(),
            Some("https://openrouter.ai:443")
        );
        assert!(stessa(
            "https://openrouter.ai/api/v1",
            "https://openrouter.ai/api/v2"
        ));
        assert!(stessa(
            "https://OpenRouter.AI/api/v1",
            "HTTPS://openrouter.ai:443/x"
        ));
        assert!(stessa(
            "http://localhost:11434/v1",
            "http://LOCALHOST:11434"
        ));
        assert!(stessa("http://[::1]:1234/v1", "http://[::1]:1234/"));

        assert!(!stessa(
            "https://openrouter.ai/api/v1",
            "https://esempio.com/api/v1"
        ));
        assert!(!stessa(
            "https://openrouter.ai/api/v1",
            "http://openrouter.ai/api/v1"
        ));
        assert!(!stessa(
            "http://localhost:11434/v1",
            "http://localhost:1234/v1"
        ));
        assert!(
            !stessa(
                "https://openrouter.ai/v1",
                "https://openrouter.ai@esempio.com/v1"
            ),
            "conta l'ospite dopo l'ultima chiocciola"
        );
        assert!(!stessa(
            "https://openrouter.ai/v1",
            "https://openrouter.ai.esempio.com/v1"
        ));

        for illeggibile in [
            "",
            "openrouter.ai",
            "ftp://openrouter.ai",
            "https://",
            "https://x:porta/",
        ] {
            assert_eq!(origine(illeggibile), None, "«{illeggibile}»");
        }
    }

    /// Senza la funzione `platform-verifier` di `ureq`, `RootCerts::PlatformVerifier`
    /// va in **panico** alla prima stretta di mano: niente lo dice prima, né il
    /// compilatore né una richiesta in chiaro. Questa prova la stretta di mano
    /// la fa davvero, contro un socket di questa macchina che chiude subito: se
    /// la funzione c'è, esce un errore di trasporto; se manca, la prova cade.
    #[test]
    fn la_verifica_e_quella_del_sistema_e_non_va_in_panico() {
        let ascolto =
            std::net::TcpListener::bind("127.0.0.1:0").expect("una porta su questa macchina");
        let porta = ascolto.local_addr().expect("la porta scelta").port();
        std::thread::spawn(move || {
            for connessione in ascolto.incoming() {
                drop(connessione);
            }
        });
        let rete = Rete::nuova("prova", Duration::from_secs(5));
        let esito = rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url: &format!("https://127.0.0.1:{porta}/"),
            intestazioni: &[],
            corpo: Corpo::Niente,
        });
        assert!(
            esito.is_err(),
            "nessun servizio TLS risponde su quella porta"
        );
        assert!(matches!(
            rete.agente.config().tls_config().root_certs(),
            ureq::tls::RootCerts::PlatformVerifier
        ));
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

    /// Un insieme di intestazioni scritto a mano.
    fn intestazioni(coppie: &[(&str, &str)]) -> ureq::http::HeaderMap {
        let mut mappa = ureq::http::HeaderMap::new();
        for (nome, valore) in coppie {
            if let (Ok(n), Ok(v)) = (
                ureq::http::HeaderName::try_from(*nome),
                ureq::http::HeaderValue::try_from(*valore),
            ) {
                mappa.insert(n, v);
            }
        }
        mappa
    }

    /// Il difetto di WP17: `bytes 0-1023/*` ripiegava su `Content-Length`, cioè
    /// chiamava «lunghezza del file» i 1024 byte del pezzo, e il brano finiva al
    /// primo intervallo.
    #[test]
    fn un_totale_ignoto_non_diventa_la_lunghezza_del_pezzo() {
        assert_eq!(
            totale_da(
                &intestazioni(&[
                    ("content-range", "bytes 0-1023/*"),
                    ("content-length", "1024")
                ]),
                true
            ),
            None,
            "un `*` vuol dire «non lo so», non «1024»"
        );
        assert_eq!(
            totale_da(
                &intestazioni(&[
                    ("content-range", "bytes 0-1023/45678"),
                    ("content-length", "1024")
                ]),
                true
            ),
            Some(45678)
        );
        assert_eq!(
            totale_da(&intestazioni(&[("content-length", "1024")]), true),
            None,
            "una risposta parziale senza `Content-Range` non dice il totale"
        );
        assert_eq!(
            totale_da(&intestazioni(&[("content-length", "5000")]), false),
            Some(5000),
            "un `200` è il file intero, e lì `Content-Length` è il totale"
        );
        assert_eq!(
            totale_da(
                &intestazioni(&[("content-range", "bytes 0-9/boh"), ("content-length", "10")]),
                true
            ),
            None
        );
    }

    /// Quante richieste ha visto il servitore che cade una volta.
    static CADUTE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn cade_una_volta(_: &str) -> String {
        if CADUTE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            return "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_owned();
        }
        pezzo("ciao")
    }

    #[test]
    fn un_intervallo_che_cade_si_richiede() {
        let servitore = Servitore::avvia(cade_una_volta);
        let rete = servitore.cliente(Rimandi::Nessuno);
        let pezzo = rete
            .intervallo(&format!("{}/brano.mp3", servitore.base), 0, 4)
            .expect("il secondo tentativo riesce");
        assert_eq!(pezzo.byte, b"ciao");
        assert_eq!(pezzo.totale, Some(4));
        assert_eq!(
            servitore.richieste().len(),
            2,
            "un `503` passa da sé: fino alla 2.4.0 fermava il brano al primo"
        );
    }

    fn ignora_l_intervallo(_: &str) -> String {
        "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nciao".to_owned()
    }

    #[test]
    fn un_intervallo_ignorato_non_si_ripete() {
        let servitore = Servitore::avvia(ignora_l_intervallo);
        let rete = servitore.cliente(Rimandi::Nessuno);
        let errore = rete
            .intervallo(&format!("{}/brano.mp3", servitore.base), 100, 4)
            .expect_err("da metà file un `200` è il posto sbagliato");
        assert_eq!(errore.code().kind().code(), "download.badResponse");
        assert_eq!(
            servitore.richieste().len(),
            1,
            "chi ignora `Range` lo ignorerà anche la seconda volta"
        );
    }

    fn sempre_giu(_: &str) -> String {
        "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_owned()
    }

    #[test]
    fn chi_non_aspetta_piu_non_aspetta_la_pausa() {
        let servitore = Servitore::avvia(sempre_giu);
        let rete = servitore.cliente(Rimandi::Nessuno);
        let chiamate = std::sync::atomic::AtomicUsize::new(0);
        let inizio = std::time::Instant::now();
        let errore = rete
            .intervallo_finche(&format!("{}/brano.mp3", servitore.base), 0, 4, &|| {
                // Sì durante la prima richiesta, no appena comincia la pausa.
                chiamate.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 1
            })
            .expect_err("chi ascolta è saltato altrove");
        assert_eq!(errore.code().kind().code(), "internal.aborted");
        assert!(
            inizio.elapsed() < Duration::from_millis(900),
            "la pausa di un secondo doveva interrompersi: {:?}",
            inizio.elapsed()
        );
    }

    fn un_corpo_grosso(_: &str) -> String {
        let quanti = 11 * 1024 * 1024;
        let mut risposta =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {quanti}\r\nConnection: close\r\n\r\n");
        risposta.push_str(&"x".repeat(quanti));
        risposta
    }

    /// Il ripristino di Drive con una skin da undici megabyte: col tetto di
    /// `ureq` falliva per intero, col tetto dichiarato dal client passa.
    #[test]
    fn il_tetto_del_corpo_e_quello_del_client() {
        let servitore = Servitore::avvia(un_corpo_grosso);
        let rete = servitore.cliente(Rimandi::Nessuno);
        assert_eq!(rete.tetto_corpo(), TETTO_CORPO_DI_SERIE);
        let url = format!("{}/skin", servitore.base);
        let richiesta = Richiesta {
            metodo: Metodo::Get,
            url: &url,
            intestazioni: &[],
            corpo: Corpo::Niente,
        };
        let errore = rete
            .esegui(richiesta)
            .expect_err("oltre dieci megabyte, di serie, no");
        assert_eq!(errore.code().kind().code(), "net.badSchema");
        assert!(
            matches!(errore.code(), ErrorCode::NetBadSchema { detail: Some(d), .. } if d.contains("tetto")),
            "la frase dice che era il peso, non la forma: {errore}"
        );

        let largo = rete.con_tetto_corpo(25 * 1024 * 1024);
        assert_eq!(largo.tetto_corpo(), 25 * 1024 * 1024);
        let risposta = largo
            .esegui(richiesta)
            .expect("sotto il tetto di chi l'ha alzato");
        assert_eq!(risposta.corpo.len(), 11 * 1024 * 1024);
    }
}
