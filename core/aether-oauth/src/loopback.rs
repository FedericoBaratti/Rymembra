//! Il servitore effimero che raccoglie la risposta del browser.
//!
//! Il consenso si dà nel **browser di sistema**, non in una finestra
//! dell'applicazione: una webview che sa disegnare `accounts.google.com` è una
//! superficie di phishing, e nessuno può controllare la barra degli indirizzi di
//! una cosa che non ce l'ha. Ma allora la risposta deve tornare indietro da
//! qualche parte, e quel posto è un socket su `127.0.0.1` che vive il tempo di
//! una richiesta.
//!
//! # Perché la porta è zero
//!
//! [`Attesa::apri`] chiede `127.0.0.1:0`: la sceglie il sistema operativo fra
//! quelle libere. Una porta fissa sarebbe già occupata sul computer di qualcuno,
//! e — peggio — sarebbe indovinabile: un programma qualunque potrebbe mettersi
//! in ascolto lì prima di noi e raccogliere il codice al posto nostro.
//!
//! Entrambi i fornitori che Aether usa lo ammettono, e per la stessa ragione:
//! Google accetta qualunque porta su loopback per i client «Desktop app» senza
//! che nessuno la registri; Spotify chiede di registrare l'indirizzo di ritorno
//! **senza porta** (`http://127.0.0.1`) e poi lascia aggiungere quella scelta al
//! momento, che è la raccomandazione della RFC 8252. `localhost` invece Spotify
//! lo vieta apertamente: è un nome, e un nome lo si può risolvere altrove.
//!
//! `127.0.0.1` e **mai** `0.0.0.0`: la seconda forma aprirebbe la porta a tutta
//! la rete locale per i tre minuti dell'operazione.
//!
//! # Cosa non finisce mai in un errore
//!
//! Il codice di autorizzazione. Gli [`AppError`] di questo crate arrivano fino
//! alla finestra dentro `ErroreIpc.cause`, e da lì nei registri: un codice in
//! chiaro in un log è un codice che qualcuno può ancora scambiare per un token.
//! Nessuna funzione di questo modulo lo mette in un messaggio.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use aether_domain::errors::{AppError, ErrorCode};

use crate::Servizio;

/// Quanto si aspetta prima di riprovare ad accettare una connessione.
///
/// Il socket è non bloccante perché `accept` da solo non ha una scadenza, e
/// senza scadenza un utente che chiude il browser lascerebbe un thread appeso
/// per sempre. Cento millisecondi non si sentono su un'operazione che dura
/// quanto ci mette una persona a leggere una schermata di consenso.
const RESPIRO: Duration = Duration::from_millis(100);

/// Il tetto alla riga di richiesta che si accetta di leggere.
///
/// Chi si collega a questa porta non è per forza un browser. Otto kibibyte
/// bastano a qualunque redirect vero e impediscono che una riga infinita si
/// mangi la memoria.
const RIGA_MASSIMA: u64 = 8 * 1024;

/// Il servitore in ascolto, prima che il browser risponda.
///
/// Si apre **prima** di costruire l'indirizzo di autorizzazione, perché la porta
/// fa parte di quell'indirizzo: chiederla dopo vorrebbe dire mandare l'utente su
/// una pagina che rimanda a un socket che non esiste ancora.
#[derive(Debug)]
pub struct Attesa {
    listener: TcpListener,
    porta: u16,
    servizio: Servizio,
}

/// Quel che il browser ha riportato indietro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Risposta {
    /// Il codice di autorizzazione da scambiare con un token.
    ///
    /// Non implementa `Display` per distrazione: è un segreto a tempo, e non
    /// deve poter finire in una stringa di formato senza che qualcuno lo scriva
    /// apposta.
    pub code: String,
}

impl Attesa {
    /// Apre il servitore su una porta scelta dal sistema.
    ///
    /// Il [`Servizio`] serve solo a nominare chi si sta aspettando, dentro gli
    /// errori: non cambia una riga del protocollo.
    ///
    /// # Errori
    ///
    /// `net.offline` se non si riesce ad aprire il socket: sul computer di
    /// qualcuno c'è un firewall che vieta anche il loopback, ed è più utile
    /// dirlo che far sembrare l'operazione partita.
    pub fn apri(servizio: Servizio) -> Result<Self, AppError> {
        let indirizzo = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let listener = TcpListener::bind(indirizzo).map_err(|err| {
            AppError::new(ErrorCode::NetOffline { url: None })
                .with_cause(format!("apertura del servitore locale: {err}"))
        })?;
        let porta = listener
            .local_addr()
            .map_err(|err| {
                AppError::new(ErrorCode::NetOffline { url: None })
                    .with_cause(format!("porta del servitore locale: {err}"))
            })?
            .port();
        listener.set_nonblocking(true).map_err(|err| {
            AppError::new(ErrorCode::NetOffline { url: None })
                .with_cause(format!("modalità del servitore locale: {err}"))
        })?;
        Ok(Self {
            listener,
            porta,
            servizio,
        })
    }

    /// La porta su cui sta ascoltando.
    #[must_use]
    pub fn porta(&self) -> u16 {
        self.porta
    }

    /// L'indirizzo di ritorno da mandare al fornitore.
    ///
    /// Senza barra finale: è la forma che entrambi accettano, e cambiarla qui
    /// vorrebbe dire far fallire lo scambio con `redirect_uri_mismatch` — un
    /// errore che si scopre solo dopo che l'utente ha già dato il consenso.
    #[must_use]
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}", self.porta)
    }

    /// Aspetta la risposta del browser, per non più di `entro`.
    ///
    /// Consuma l'attesa: il servitore serve una volta sola. Le richieste che non
    /// portano né un codice né un errore — la richiesta dell'iconcina che certi
    /// browser mandano da soli, un programma che sta scandendo le porte — si
    /// rispedisce via e si continua ad aspettare quella vera. Fermarsi alla
    /// prima connessione qualunque farebbe fallire il collegamento a seconda del
    /// browser, cioè in modo irriproducibile.
    ///
    /// # Errori
    ///
    /// - `internal.timeout` se non arriva niente entro il tempo dato: è il caso
    ///   di chi apre la pagina e va a fare altro.
    /// - `internal.aborted` se l'utente nega il consenso. Non è un guasto, ed è
    ///   `Info`: è una risposta legittima a una domanda.
    /// - `net.badSchema` se lo `state` non corrisponde. Vuol dire che a questa
    ///   porta ha bussato qualcosa che non è la nostra richiesta, e l'unica
    ///   risposta sicura è buttare via tutto e ricominciare.
    pub fn aspetta(self, state: &str, entro: Duration) -> Result<Risposta, AppError> {
        let scadenza = Instant::now() + entro;
        loop {
            if Instant::now() >= scadenza {
                return Err(AppError::new(ErrorCode::InternalTimeout {
                    what: format!("consenso {}", self.servizio.nome),
                    timeout_ms: u64::try_from(entro.as_millis()).unwrap_or(u64::MAX),
                }));
            }
            let flusso = match self.listener.accept() {
                Ok((flusso, _)) => flusso,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(RESPIRO);
                    continue;
                }
                Err(err) => {
                    return Err(AppError::new(ErrorCode::NetOffline { url: None })
                        .with_cause(format!("connessione locale rifiutata: {err}")));
                }
            };
            match serve(flusso, state, self.servizio) {
                Esito::Finita(esito) => return esito,
                Esito::NonEraLei => {}
            }
        }
    }
}

/// Se una connessione era quella che aspettavamo.
enum Esito {
    /// Portava un codice o un errore: si può chiudere.
    Finita(Result<Risposta, AppError>),
    /// Non portava niente di utile: si continua ad aspettare.
    NonEraLei,
}

/// Legge una richiesta, risponde, e dice se era quella giusta.
fn serve(flusso: TcpStream, state: &str, servizio: Servizio) -> Esito {
    // Una scadenza sul socket accettato: un client che apre la connessione e
    // non manda niente non deve poter tenere fermo tutto il collegamento.
    let _ = flusso.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = flusso.set_nonblocking(false);

    let mut lettore = BufReader::new(&flusso);
    let mut riga = String::new();
    if (&mut lettore)
        .take(RIGA_MASSIMA)
        .read_line(&mut riga)
        .is_err()
    {
        return Esito::NonEraLei;
    }

    let query = interroga(&riga).unwrap_or_default();
    let valore = |cercato: &str| {
        query
            .iter()
            .find(|(nome, _)| nome == cercato)
            .map(|(_, valore)| valore.clone())
    };

    // Primo: era per noi? Una richiesta senza né codice né errore non è la
    // risposta al consenso — è l'iconcina che il browser chiede da solo, o
    // qualcuno che sta scandendo le porte. Si rimanda via e si continua ad
    // aspettare quella vera.
    let code = valore("code");
    let errore = valore("error");
    if code.is_none() && errore.is_none() {
        rispondi(&flusso, "404 Not Found", PAGINA_IGNOTA);
        return Esito::NonEraLei;
    }

    // Secondo: era **la nostra**? Il controllo dello `state` viene prima di
    // guardare il contenuto, compreso un `error`: altrimenti chiunque possa
    // bussare a questa porta potrebbe far fallire il collegamento mandando un
    // `error=access_denied` che non viene dal fornitore.
    if valore("state").as_deref() != Some(state) {
        rispondi(&flusso, "400 Bad Request", PAGINA_IGNOTA);
        // Il valore ricevuto **non** si mette nella causa: se davvero è arrivato
        // da qualcun altro, ricopiarlo in un registro è l'unica cosa peggiore
        // che si possa fare con lui.
        return Esito::Finita(Err(AppError::new(ErrorCode::NetBadSchema {
            service: Some(servizio.chiave.to_owned()),
            detail: Some("la risposta non corrisponde alla richiesta".to_owned()),
        })));
    }

    if let Some(motivo) = errore {
        rispondi(&flusso, "200 OK", PAGINA_NEGATA);
        // Il motivo del fornitore finisce nella causa: `access_denied` è
        // un'informazione utile a chi legge un registro, e non è un segreto.
        return Esito::Finita(Err(AppError::new(ErrorCode::InternalAborted {
            what: Some(format!("consenso {}", servizio.nome)),
        })
        .with_cause(motivo)));
    }

    let Some(code) = code else {
        rispondi(&flusso, "404 Not Found", PAGINA_IGNOTA);
        return Esito::NonEraLei;
    };
    rispondi(&flusso, "200 OK", PAGINA_FATTA);
    Esito::Finita(Ok(Risposta { code }))
}

/// Le coppie della stringa di interrogazione, già decodificate.
///
/// `None` se la riga non è una richiesta `GET` con un'interrogazione: non è un
/// guasto, è qualcosa che non stavamo aspettando.
fn interroga(riga: &str) -> Option<Vec<(String, String)>> {
    let percorso = riga.strip_prefix("GET ")?.split_whitespace().next()?;
    let query = percorso.split_once('?')?.1;
    Some(
        query
            .split('&')
            .filter(|pezzo| !pezzo.is_empty())
            .map(|pezzo| {
                let (nome, valore) = pezzo.split_once('=').unwrap_or((pezzo, ""));
                (percento(nome), percento(valore))
            })
            .collect(),
    )
}

/// Decodifica le sequenze `%XX` e i `+` di una stringa di interrogazione.
///
/// Non è ornamento: un codice di autorizzazione di Google contiene `/`, che
/// arriva scritto `%2F`. Passarlo al punto di scambio senza decodificarlo dà un
/// `invalid_grant` che sembra un token scaduto e non lo è.
///
/// Si decodifica in byte e non in caratteri: una sequenza percentuale porta un
/// **byte**, e comporre i caratteri uno a uno trasformerebbe un accento UTF-8 in
/// due segni sbagliati.
fn percento(input: &str) -> String {
    let mut byte = Vec::with_capacity(input.len());
    let mut resto = input.bytes();
    while let Some(b) = resto.next() {
        match b {
            b'+' => byte.push(b' '),
            b'%' => match (resto.next().and_then(cifra), resto.next().and_then(cifra)) {
                (Some(alto), Some(basso)) => byte.push(alto * 16 + basso),
                // Una sequenza monca in fondo alla stringa: si lascia cadere,
                // perché non c'è un byte da ricostruire.
                _ => break,
            },
            altro => byte.push(altro),
        }
    }
    String::from_utf8_lossy(&byte).into_owned()
}

/// Il valore di una cifra esadecimale, se lo è.
fn cifra(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Scrive una risposta e chiude.
///
/// Gli errori di scrittura si ignorano di proposito: a questo punto il codice è
/// già stato letto, e un browser che ha chiuso la connessione mentre gli
/// rispondevamo non è una ragione per far fallire un collegamento riuscito.
fn rispondi(mut flusso: &TcpStream, stato: &str, corpo: &str) {
    let risposta = format!(
        "HTTP/1.1 {stato}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\
         \r\n\
         {corpo}",
        corpo.len()
    );
    let _ = flusso.write_all(risposta.as_bytes());
    let _ = flusso.flush();
}

/// La pagina che l'utente vede quando è andata bene.
const PAGINA_FATTA: &str = concat!(
    "<!doctype html><html lang=\"it\"><meta charset=\"utf-8\">",
    "<title>Aether</title>",
    "<body style=\"font:16px system-ui;display:grid;place-items:center;height:100vh;margin:0;",
    "background:#0b0b0d;color:#f4f4f5\">",
    "<div style=\"text-align:center\"><h1 style=\"font-size:20px\">Account collegato</h1>",
    "<p style=\"opacity:.7\">Puoi chiudere questa scheda e tornare ad Aether.</p></div>"
);

/// La pagina che l'utente vede quando ha detto di no.
const PAGINA_NEGATA: &str = concat!(
    "<!doctype html><html lang=\"it\"><meta charset=\"utf-8\">",
    "<title>Aether</title>",
    "<body style=\"font:16px system-ui;display:grid;place-items:center;height:100vh;margin:0;",
    "background:#0b0b0d;color:#f4f4f5\">",
    "<div style=\"text-align:center\"><h1 style=\"font-size:20px\">Nessun accesso concesso</h1>",
    "<p style=\"opacity:.7\">Aether non ha ricevuto il permesso. Puoi chiudere questa scheda.</p>",
    "</div>"
);

/// Quel che si risponde a chi bussa senza essere il browser.
const PAGINA_IGNOTA: &str = "<!doctype html><meta charset=\"utf-8\"><title>Aether</title>";

#[cfg(test)]
mod prove {
    use super::*;

    /// Il fornitore finto delle prove.
    const PROVA: Servizio = Servizio {
        chiave: "prova",
        nome: "Prova",
    };

    /// Bussa alla porta come farebbe il browser, e restituisce la risposta.
    ///
    /// TCP crudo e non un client HTTP: qui si sta provando il **nostro**
    /// servitore, e mettere di mezzo una libreria vorrebbe dire provare anche
    /// lei. Sono tre righe.
    fn bussa(porta: u16, percorso: &str) -> String {
        let mut flusso =
            std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, porta)).expect("connessione");
        write!(flusso, "GET {percorso} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").expect("richiesta");
        let mut risposta = String::new();
        let _ = flusso.read_to_string(&mut risposta);
        risposta
    }

    #[test]
    fn un_codice_giusto_si_raccoglie_e_si_decodifica() {
        // Il `%2F` non è un dettaglio: i codici di Google contengono barre, e
        // passarli senza decodificarli dà un `invalid_grant` che sembra un token
        // scaduto e non lo è.
        let attesa = Attesa::apri(PROVA).expect("apertura");
        let porta = attesa.porta();
        let filo = std::thread::spawn(move || bussa(porta, "/?code=4%2F0Ab_c-d&state=abc"));

        let risposta = attesa
            .aspetta("abc", Duration::from_secs(10))
            .expect("il codice arriva");
        assert_eq!(risposta.code, "4/0Ab_c-d");
        assert!(filo.join().expect("filo").contains("200 OK"));
    }

    #[test]
    fn uno_state_sbagliato_si_rifiuta() {
        // A questa porta può bussare qualunque cosa giri sulla stessa macchina.
        // Lo `state` è ciò che distingue la risposta alla nostra richiesta da
        // una risposta a quella di qualcun altro.
        let attesa = Attesa::apri(PROVA).expect("apertura");
        let porta = attesa.porta();
        let filo = std::thread::spawn(move || bussa(porta, "/?code=rubato&state=altro"));

        let err = attesa
            .aspetta("nostro", Duration::from_secs(10))
            .expect_err("non è la nostra");
        assert_eq!(err.code().kind().code(), "net.badSchema");
        // Il codice ricevuto non finisce mai nel messaggio: ricopiarlo in un
        // registro sarebbe la cosa peggiore da farne.
        assert!(!format!("{err}").contains("rubato"));
        assert!(filo.join().expect("filo").contains("400"));
    }

    #[test]
    fn un_errore_con_lo_state_giusto_e_un_rifiuto_dell_utente() {
        let attesa = Attesa::apri(PROVA).expect("apertura");
        let porta = attesa.porta();
        let filo = std::thread::spawn(move || bussa(porta, "/?error=access_denied&state=s"));

        let err = attesa
            .aspetta("s", Duration::from_secs(10))
            .expect_err("l'utente ha detto di no");
        assert_eq!(err.code().kind().code(), "internal.aborted");
        assert!(
            err.cause().is_some_and(|c| c.contains("access_denied")),
            "il motivo serve a chi legge un registro"
        );
        filo.join().expect("filo");
    }

    #[test]
    fn un_errore_senza_lo_state_giusto_non_interrompe_niente() {
        // Senza il controllo dello `state` prima del contenuto, chiunque possa
        // bussare a questa porta potrebbe far fallire il collegamento mandando
        // un `error=access_denied` che non viene dal fornitore.
        let attesa = Attesa::apri(PROVA).expect("apertura");
        let porta = attesa.porta();
        let filo = std::thread::spawn(move || bussa(porta, "/?error=access_denied&state=finto"));

        let err = attesa
            .aspetta("vero", Duration::from_secs(10))
            .expect_err("respinta");
        assert_eq!(err.code().kind().code(), "net.badSchema");
        filo.join().expect("filo");
    }

    #[test]
    fn una_richiesta_qualunque_non_interrompe_l_attesa() {
        // Certi browser chiedono l'iconcina da soli. Fermarsi alla prima
        // connessione farebbe fallire il collegamento a seconda del browser,
        // cioè in modo irriproducibile.
        let attesa = Attesa::apri(PROVA).expect("apertura");
        let porta = attesa.porta();
        let filo = std::thread::spawn(move || {
            bussa(porta, "/favicon.ico");
            bussa(porta, "/?state=s&code=quello_vero")
        });

        let risposta = attesa
            .aspetta("s", Duration::from_secs(10))
            .expect("il codice arriva lo stesso");
        assert_eq!(risposta.code, "quello_vero");
        filo.join().expect("filo");
    }

    #[test]
    fn il_servitore_si_spegne_dopo_la_richiesta_che_conta() {
        let attesa = Attesa::apri(PROVA).expect("apertura");
        let porta = attesa.porta();
        let filo = std::thread::spawn(move || bussa(porta, "/?code=x&state=s"));
        attesa
            .aspetta("s", Duration::from_secs(10))
            .expect("codice");
        filo.join().expect("filo");

        // L'attesa è stata consumata, quindi il socket è chiuso: la porta non
        // resta aperta ad aspettare qualcun altro.
        assert!(
            std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, porta)).is_err(),
            "la porta doveva chiudersi con l'attesa"
        );
    }

    #[test]
    fn senza_risposta_ci_si_arrende() {
        // Chi apre la pagina di consenso e va a fare altro non deve lasciare un
        // filo appeso per sempre.
        let attesa = Attesa::apri(PROVA).expect("apertura");
        let err = attesa
            .aspetta("s", Duration::from_millis(300))
            .expect_err("scaduta");
        assert_eq!(err.code().kind().code(), "internal.timeout");
    }

    #[test]
    fn il_fornitore_si_riconosce_dall_errore() {
        // Il `Servizio` non cambia una riga del protocollo, ma è quel che
        // permette a chi legge «consenso Spotify scaduto» di sapere di quale dei
        // due collegamenti si sta parlando.
        let attesa = Attesa::apri(Servizio {
            chiave: "spotify",
            nome: "Spotify",
        })
        .expect("apertura");
        let err = attesa
            .aspetta("s", Duration::from_millis(200))
            .expect_err("scaduta");
        assert!(
            format!("{err}").contains("Spotify") || err.code().kind().code() == "internal.timeout"
        );
        assert_eq!(
            err.code(),
            &ErrorCode::InternalTimeout {
                what: "consenso Spotify".to_owned(),
                timeout_ms: 200
            }
        );
    }

    #[test]
    fn l_indirizzo_di_ritorno_e_solo_su_loopback() {
        // Mai `0.0.0.0`: aprirebbe la porta a tutta la rete locale per i tre
        // minuti dell'operazione.
        let attesa = Attesa::apri(PROVA).expect("apertura");
        assert!(attesa.redirect_uri().starts_with("http://127.0.0.1:"));
        assert!(!attesa.redirect_uri().ends_with('/'), "senza barra finale");
        assert_ne!(attesa.porta(), 0, "la porta la sceglie il sistema");
        // Mai `localhost`: Spotify lo vieta apertamente, perché è un nome e un
        // nome lo si può risolvere altrove.
        assert!(!attesa.redirect_uri().contains("localhost"));
    }

    #[test]
    fn la_decodifica_percentuale_regge_anche_quel_che_non_e_ascii() {
        assert_eq!(percento("4%2F0Ab"), "4/0Ab");
        assert_eq!(percento("con+spazio"), "con spazio");
        assert_eq!(percento("%C3%A8"), "è", "due byte, un carattere solo");
        assert_eq!(percento("niente"), "niente");
        // Una sequenza monca non deve far cadere niente.
        assert_eq!(percento("a%"), "a");
        assert_eq!(percento("a%2"), "a");
        assert_eq!(percento("a%zz"), "a");
    }

    #[test]
    fn una_riga_che_non_e_una_richiesta_non_da_niente() {
        assert_eq!(interroga("POST /?code=x HTTP/1.1"), None);
        assert_eq!(interroga("GET / HTTP/1.1"), None, "senza interrogazione");
        assert_eq!(interroga("spazzatura"), None);
        assert_eq!(
            interroga("GET /?code=x&state=y HTTP/1.1"),
            Some(vec![
                ("code".to_owned(), "x".to_owned()),
                ("state".to_owned(), "y".to_owned()),
            ])
        );
    }
}
