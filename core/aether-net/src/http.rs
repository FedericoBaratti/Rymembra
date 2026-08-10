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
}

/// Come Aether si presenta a un servizio che ha il diritto di sapere chi è.
///
/// Un indirizzo dentro lo `User-Agent` è la cortesia che permette a chi gestisce
/// un servizio pubblico di scrivere a qualcuno invece di limitarsi a bloccare.
const AGENTE_PREDEFINITO: &str = "Aether/0.1 (+https://github.com/federicobaratti/aether)";

impl Rete {
    /// Un client con una scadenza complessiva.
    ///
    /// La scadenza è **globale**, cioè vale sull'intera richiesta e non su un
    /// singolo respiro del socket: è la forma che protegge davvero da un
    /// servizio che risponde un byte al secondo, e che una scadenza di lettura
    /// non copre.
    #[must_use]
    pub fn nuova(servizio: &'static str, scadenza: Duration) -> Self {
        Self::nuova_con_agente(servizio, scadenza, AGENTE_PREDEFINITO)
    }

    /// Come [`Self::nuova`], ma dichiarando un altro `User-Agent`.
    ///
    /// # Perché è un parametro e non una costante
    ///
    /// Esiste per un caso solo, e vale la pena dire quale invece di lasciarlo
    /// scoprire: il lettore keyless di Spotify parla con i punti interni del
    /// lettore web, che a uno `User-Agent` sconosciuto rispondono in modo
    /// diverso — quando rispondono. Lì presentarsi come Aether non è onestà,
    /// è un guasto: non c'è nessuno a cui quel nome dica qualcosa, e l'unico
    /// effetto è che la richiesta non funziona.
    ///
    /// Resta un costruttore separato, e non un parametro di [`Self::nuova`],
    /// perché il valore normale è il nome vero: chi scrive un client nuovo deve
    /// dover **scegliere** di non dirlo, non trovarselo già scelto.
    #[must_use]
    pub fn nuova_con_agente(servizio: &'static str, scadenza: Duration, agente: &str) -> Self {
        let configurazione = ureq::Agent::config_builder()
            .timeout_global(Some(scadenza))
            // Vedi la nota in testa al modulo: il corpo di un 4xx è
            // l'informazione, e trasformarlo in un `Err` lo butterebbe via.
            .http_status_as_error(false)
            // Nessuna richiesta di questo crate ha motivo di uscire in chiaro.
            // Non è teorico: senza, un `http://` scritto per sbaglio in una
            // costante manderebbe un token in rete leggibile da chiunque.
            .https_only(true)
            .user_agent(agente)
            .build();
        Self {
            agente: configurazione.into(),
            servizio,
        }
    }

    /// Esegue una richiesta.
    ///
    /// # Errori
    ///
    /// `net.offline`, `net.timeout` o `net.http` per i guasti di **trasporto**.
    /// Uno stato di errore non produce un `Err`: arriva dentro la [`Risposta`].
    pub fn esegui(&self, richiesta: Richiesta<'_>) -> Result<Risposta, AppError> {
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
        let riprova_fra_ms = risposta
            .headers()
            .get("retry-after")
            .and_then(|valore| valore.to_str().ok())
            .and_then(secondi_in_ms);
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

    /// Traduce un guasto di trasporto di `ureq` in un codice Aether.
    ///
    /// La distinzione fra «non c'è rete» e «la rete c'è ma è lenta» non è
    /// pignoleria: la prima si dice all'utente («sei senza connessione»), la
    /// seconda si ritenta da sola.
    fn trasporto(&self, err: &ureq::Error, url: &str) -> AppError {
        let url = Some(url.to_owned());
        match err {
            ureq::Error::Timeout(_) => AppError::new(ErrorCode::NetTimeout {
                url,
                timeout_ms: None,
            }),
            ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
                AppError::new(ErrorCode::NetOffline { url })
            }
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
/// Pubblica perché non tutti i cicli di tentativi possono passare da
/// [`Rete::con_tentativi`]: quello di `aether_spotify::pathfinder` porta con sé
/// un `&mut Sessione` da invalidare sul 401, che in una chiusura `FnMut` non ci
/// sta. Quel ciclo resta scritto a mano, ma l'attesa la calcola qui — o
/// riprenderebbe un `429` all'istante, che è il modo più diretto di prenderne un
/// secondo.
#[must_use]
pub fn quanto_aspettare(err: &AppError, predefinita: Duration) -> Duration {
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
}
