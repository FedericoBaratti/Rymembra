//! Le richieste di [`Rete`]: come partono, come seguono i rimandi, come si legge
//! quel che torna, come un guasto diventa un codice e quando si ritenta. «La nota in
//! testa al modulo», nei commenti di qui, è quella di `http.rs`.

use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCode};
// L'estensione che porta `get_uri()`: senza, l'indirizzo a cui si è arrivati
// dopo i reindirizzamenti non è raggiungibile dalla risposta.
use ureq::ResponseExt as _;

use super::{
    ATTESA_MASSIMA, BILANCIO_INTERVALLO, BLOCCO_PRELIEVO, Corpo, Metodo, PASSO_ATTESA, Pezzo, Rete,
    Richiesta, Rimandi, Risposta, TENTATIVI_INTERVALLO, accorcia, e_un_rimando, in_chiaro_ammesso,
    modulo_urlencoded, non_in_casa, ospite, risolvi_rimando, senza_proxy,
};

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
pub(super) fn totale_da(intestazioni: &ureq::http::HeaderMap, parziale: bool) -> Option<u64> {
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
pub(super) fn leggi_fino_a(
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

    /// Il proxy di una richiesta verso `url`: quello dell'agente, tranne verso
    /// un ospite che va diretto (vedi [`senza_proxy`]).
    ///
    /// Si decide **per richiesta** e non con due agenti: `ureq` accetta il
    /// proxy anche nella configurazione di una richiesta sola, e la riserva di
    /// connessioni tiene distinte da sé quelle con e senza proxy — il proxy fa
    /// parte della sua chiave. Un agente solo vuol dire una riserva sola, e
    /// nessun costruttore da tenere allineato a un gemello.
    ///
    /// Ogni richiesta che parte da questo tipo passa da qui: una che se ne
    /// dimenticasse manderebbe al proxy d'ufficio la cartella WebDAV del NAS
    /// di casa, o il modello che gira su `127.0.0.1`.
    fn proxy_per(&self, url: &str) -> Option<ureq::Proxy> {
        if senza_proxy(url) {
            return None;
        }
        self.agente.config().proxy().cloned()
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
        // Il proxy si decide su `url`, che è il salto di adesso e non
        // l'indirizzo di partenza: vedi `proxy_per`.
        let mut costruttore = self
            .agente
            .get(url)
            .config()
            .max_redirects(salti)
            .proxy(self.proxy_per(url))
            .build();
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
                    .proxy(self.proxy_per(richiesta.url))
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
            // Un metodo che `ureq` non ha fra i suoi: si costruisce la
            // richiesta per nome e la si manda con l'agente, che porta con sé
            // TLS, scadenze e il divieto dei rimandi.
            Metodo::Propfind => {
                let mut costruttore = ureq::http::Request::builder()
                    .method(Metodo::Propfind.nome())
                    .uri(richiesta.url);
                for (nome, valore) in richiesta.intestazioni {
                    costruttore = costruttore.header(*nome, *valore);
                }
                let corpo = match richiesta.corpo {
                    Corpo::Byte { tipo, dati } => {
                        costruttore = costruttore.header("content-type", tipo);
                        dati.to_vec()
                    }
                    Corpo::Niente | Corpo::Modulo(_) => Vec::new(),
                };
                let costruita = costruttore.body(corpo).map_err(|err| {
                    AppError::new(ErrorCode::NetBadSchema {
                        service: Some(self.servizio.to_owned()),
                        detail: Some(format!("richiesta PROPFIND non valida: {err}")),
                    })
                })?;
                // La configurazione di una richiesta sola, come per gli altri
                // metodi: qui serve solo a decidere il proxy.
                let costruita = self
                    .agente
                    .configure_request(costruita)
                    .proxy(self.proxy_per(richiesta.url))
                    .build();
                let risposta = self
                    .agente
                    .run(costruita)
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
                let mut costruttore = costruttore
                    .config()
                    .max_redirects(salti)
                    .proxy(self.proxy_per(richiesta.url))
                    .build();
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
            .proxy(self.proxy_per(url))
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
    /// Tre tentativi in tutto ([`TENTATIVI_INTERVALLO`]), e uno nuovo comincia
    /// solo se, pausa compresa, parte entro [`BILANCIO_INTERVALLO`] dalla prima
    /// richiesta: dall'altra parte c'è qualcuno che ascolta, e un guasto che non
    /// passa va detto. Il bilancio decide se tentare ancora, non tronca un
    /// tentativo già partito, che ha le scadenze del client.
    ///
    /// Non si ripete un intervallo
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
        self.intervallo_con(url, da, quanti, avanti, &[])
    }

    /// Come [`Self::intervallo_finche`], con delle intestazioni in più.
    ///
    /// # Perché esiste
    ///
    /// Per i servizi che servono i byte di un file solo a chi mostra un
    /// permesso: Google Drive con `files/{id}?alt=media`, Graph quando
    /// risponde sul posto invece che con un rimando. Fino alla 3.1.0 un flusso
    /// poteva leggere soltanto indirizzi che portano già dentro il permesso —
    /// quelli dei cataloghi — e un `Authorization` non aveva dove passare.
    ///
    /// Le intestazioni viaggiano con la prima richiesta; dal secondo salto
    /// `Authorization` e `Cookie` restano indietro, come per ogni rimando
    /// seguito a mano ([`Self::prendi`]). L'intestazione `Range` la scrive
    /// questo metodo: una seconda, passata da chi chiama, sarebbe un
    /// intervallo diverso da quello che poi si controlla.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::intervallo_finche`].
    pub fn intervallo_con(
        &self,
        url: &str,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
        intestazioni: &[(&str, &str)],
    ) -> Result<Pezzo, AppError> {
        self.ripeti(
            TENTATIVI_INTERVALLO,
            avanti,
            &|err: &AppError| {
                err.is_retryable() && !matches!(err.code(), ErrorCode::DownloadBadResponse)
            },
            Some(std::time::Instant::now() + BILANCIO_INTERVALLO),
            || self.intervallo_una_volta(url, da, quanti, avanti, intestazioni),
        )
    }

    /// Dove rimanda un `GET`, **senza andarci**.
    ///
    /// # Perché esiste
    ///
    /// Per OneDrive. `/items/{id}/content` non dà i byte: risponde `302` verso
    /// un indirizzo che porta il permesso dentro di sé e vale per un po', e
    /// quell'indirizzo è la cosa da ricordare — le finestre dopo si chiedono
    /// lì, senza token e senza ripassare da Graph. Seguire il rimando e
    /// guardare dove si è finiti ([`Pezzo::url_finale`]) darebbe lo stesso
    /// indirizzo, ma confonderebbe le due domande che dopo contano: un `401`
    /// di Graph vuol dire «rinfresca il token», uno dell'indirizzo firmato
    /// «chiedi a Graph un indirizzo nuovo». Qui le due richieste restano due.
    ///
    /// Quel che c'era già quasi bastava — [`Rimandi::Nessuno`] fa arrivare un
    /// `302` come risposta — ma [`Self::esegui`] leggerebbe il corpo, e se il
    /// servizio un giorno servisse i byte sul posto quel corpo sarebbe il
    /// brano intero. Qui un `2xx` si lascia cadere senza leggerlo.
    ///
    /// Restituisce l'indirizzo intero del bersaglio (un `Location` relativo si
    /// risolve contro `url`), oppure `None` se il servizio ha risposto sul
    /// posto. I rimandi si guardano qualunque sia la politica di questo
    /// client: è la domanda stessa a non volerli seguire.
    ///
    /// # Errori
    ///
    /// `net.*` per i guasti di trasporto e per uno stato che non è né un `2xx`
    /// né un rimando, col principio del corpo nella causa; `net.badSchema` per
    /// un bersaglio che questo client non potrebbe chiamare (fuori da https).
    pub fn dove_rimanda(
        &self,
        url: &str,
        intestazioni: &[(&str, &str)],
    ) -> Result<Option<String>, AppError> {
        self.consenti(url)?;
        let mut risposta = self.chiama_get(url, intestazioni, 0, url)?;
        let stato = risposta.status().as_u16();
        if (200..300).contains(&stato) {
            return Ok(None);
        }
        let posizione = risposta
            .headers()
            .get("location")
            .and_then(|valore| valore.to_str().ok())
            .map(ToOwned::to_owned);
        if let (true, Some(posizione)) = (e_un_rimando(stato), posizione)
            && let Some(bersaglio) = risolvi_rimando(url, &posizione)
        {
            if !self.schema_ammesso(&bersaglio) {
                return Err(rimando_rifiutato(
                    self.servizio,
                    &bersaglio,
                    "il rimando porta fuori da https",
                ));
            }
            return Ok(Some(bersaglio));
        }
        let mut guasto = senza_corpo(&risposta, stato, url);
        // Il principio del corpo, come per ogni stato di errore: è lì che il
        // servizio dice *quale* cosa non va. Con un tetto, perché chi risponde
        // male può rispondere anche lungo.
        guasto.corpo = risposta
            .body_mut()
            .with_config()
            .limit(64 * 1024)
            .read_to_vec()
            .unwrap_or_default();
        Err(self.stato_a_errore(&guasto, url))
    }

    /// Una richiesta di intervallo, senza tentativi: il corpo di
    /// [`Self::intervallo_con`].
    fn intervallo_una_volta(
        &self,
        url: &str,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
        intestazioni: &[(&str, &str)],
    ) -> Result<Pezzo, AppError> {
        let fino = da.saturating_add(quanti.max(1)).saturating_sub(1);
        let intervallo = format!("bytes={da}-{fino}");
        let mut tutte: Vec<(&str, &str)> = intestazioni
            .iter()
            .filter(|(nome, _)| !nome.eq_ignore_ascii_case("range"))
            .copied()
            .collect();
        tutte.push(("range", &intervallo));
        // I rimandi li decide la politica di questo client: per un flusso
        // `DaRete` la mette a `Sorvegliati`, col cancello del catalogo, e
        // `prendi` fa passare ogni bersaglio da lì prima di chiedergli un byte.
        let (mut risposta, arrivato) = self.prendi(url, &tutte)?;

        let stato = risposta.status().as_u16();
        // Un `416` dopo l'ultimo byte è la fine del file, detta come la dice
        // la RFC 9110: un `Range` che comincia dove il file è già finito non
        // si può soddisfare. Lo chiede un flusso che non conosce la lunghezza
        // — il servizio non l'ha dichiarata — per scoprire dove finisce, e
        // trattarlo da guasto voleva dire chiudere il brano con «rete
        // interrotta». Diventa il pezzo vuoto, che è la fine che chi legge
        // già riconosce, e porta con sé il totale di `bytes */N` quando c'è.
        //
        // Solo se è davvero dopo la fine: con `N` oltre `da` il servizio sta
        // rifiutando un punto che c'è, e un `416` muto al primo byte non è la
        // fine di niente. Presi per una fine, troncherebbero il brano senza
        // una parola.
        if stato == 416 {
            let totale = totale_da(risposta.headers(), true);
            let in_fondo = totale.map_or(da > 0, |totale| totale <= da);
            if in_fondo {
                return Ok(Pezzo {
                    byte: Vec::new(),
                    totale,
                    url_finale: None,
                });
            }
        }
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

    /// Traduce un guasto di trasporto di `ureq` in un codice Rymembra.
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
        self.ripeti(
            tentativi,
            &|| true,
            &AppError::is_retryable,
            None,
            operazione,
        )
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
        self.ripeti(tentativi, avanti, &AppError::is_retryable, None, operazione)
    }

    /// Il ciclo di tutti i tentativi, con il predicato che decide quali errori
    /// si ripetono.
    ///
    /// Uno solo, per [`Self::ritenta`], [`Self::ritenta_finche`] e gli
    /// intervalli dei flussi: tre cicli con la stessa attesa che raddoppia sono
    /// tre cicli che divergono il giorno in cui qualcuno ne aggiusta uno.
    ///
    /// `scadenza`, quando c'è, è l'ultimo istante in cui un tentativo può
    /// **cominciare**: un guasto la cui pausa finirebbe oltre si racconta
    /// subito. Un tentativo già partito non lo ferma: ha le scadenze del suo
    /// client. Vedi [`BILANCIO_INTERVALLO`].
    pub(super) fn ripeti<T>(
        &self,
        tentativi: u32,
        avanti: &dyn Fn() -> bool,
        ritentabile: &dyn Fn(&AppError) -> bool,
        scadenza: Option<std::time::Instant>,
        mut operazione: impl FnMut() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut attesa = Duration::from_secs(1);
        for _ in 1..tentativi.max(1) {
            match operazione() {
                Ok(valore) => return Ok(valore),
                Err(err) if ritentabile(&err) => {
                    let pausa = quanto_aspettare(&err, attesa);
                    // La scadenza si guarda con la pausa dentro: un tentativo
                    // che potrebbe partire solo dopo la scadenza non parte, e
                    // il guasto si racconta adesso invece che dopo un'attesa
                    // che non porta a niente. La pausa non si accorcia: un
                    // `Retry-After` chiesto dal servizio, rispettato a metà,
                    // vale un altro `429`.
                    if scadenza.is_some_and(|fine| std::time::Instant::now() + pausa >= fine) {
                        return Err(err);
                    }
                    if !dormi_finche(pausa, avanti) {
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
/// detto — e quando l'ha detto con uno stato che il suo codice non porta: lo
/// portano solo `net.rateLimited` (un 429) e `net.circuitOpen`, non il
/// `net.http` di un 503. Vedi la nota in testa al modulo.
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
pub(super) fn quanto_ha_chiesto(intestazioni: &ureq::http::HeaderMap) -> Option<u64> {
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
pub(super) fn secondi_in_ms(valore: &str) -> Option<u64> {
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
pub(super) fn dns_fallito(err: &std::io::Error) -> bool {
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
pub(super) fn senza_rete(genere: std::io::ErrorKind) -> bool {
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
