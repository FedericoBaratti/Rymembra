//! Le prove di `http`, contro un servitore finto su questa macchina. «In testa al
//! modulo», nei commenti di qui, vuol dire in testa a `http.rs`.

use super::richieste::{
    dns_fallito, leggi_fino_a, quanto_ha_chiesto, secondi_in_ms, senza_rete, totale_da,
};
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
        "quel che non è né http né https non è un indirizzo per Rymembra"
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
    let ascolto = std::net::TcpListener::bind("127.0.0.1:0").expect("una porta su questa macchina");
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
    assert!(AGENTE.starts_with("Rymembra/"));
    assert!(
        AGENTE.contains(env!("CARGO_PKG_VERSION")),
        "scritta a mano era ferma alla 0.1: MusicBrainz limita chi si presenta con una versione che non esiste"
    );
    assert!(
        AGENTE.contains("+https://github.com/federicobaratti/rymembra"),
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
    "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
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

fn sempre_ciao(_: &str) -> String {
    pezzo("ciao")
}

#[test]
fn un_intervallo_porta_le_intestazioni_di_chi_lo_chiede() {
    // Il caso per cui `intervallo_con` esiste: un servizio che serve i byte
    // solo a chi mostra il permesso. L'intervallo lo scrive il metodo, e
    // uno passato da chi chiama non deve fargli concorrenza.
    let servitore = Servitore::avvia(sempre_ciao);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let pezzo = rete
        .intervallo_con(
            &format!("{}/brano.flac", servitore.base),
            0,
            4,
            &|| true,
            &[("Authorization", "Bearer segreto"), ("Range", "bytes=9-9")],
        )
        .expect("il pezzo arriva");
    assert_eq!(pezzo.byte, b"ciao");
    let richieste = servitore.richieste();
    assert_eq!(richieste.len(), 1);
    let testa = richieste[0].to_lowercase();
    assert!(testa.contains("authorization: bearer segreto"), "{testa}");
    assert!(testa.contains("range: bytes=0-3"), "{testa}");
    assert!(
        !testa.contains("bytes=9-9"),
        "l'intervallo è uno solo, quello del metodo: {testa}"
    );
}

#[test]
fn intervallo_finche_e_intervallo_con_senza_intestazioni() {
    let servitore = Servitore::avvia(sempre_ciao);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let pezzo = rete
        .intervallo_finche(&format!("{}/brano.flac", servitore.base), 0, 4, &|| true)
        .expect("il pezzo arriva");
    assert_eq!(pezzo.byte, b"ciao");
    let testa = servitore.richieste()[0].to_lowercase();
    assert!(
        !testa.contains("authorization"),
        "chi non passa intestazioni non ne manda: {testa}"
    );
}

#[test]
fn con_rimandi_sorvegliati_il_permesso_di_un_intervallo_resta_al_primo_salto() {
    let servitore = Servitore::avvia(risponde_con_un_salto);
    let rete = servitore.cliente(Rimandi::Sorvegliati {
        quanti: 3,
        cancello: Cancello::nuovo(|_| true),
    });
    let pezzo = rete
        .intervallo_con(
            &format!("{}/uno", servitore.base),
            0,
            4,
            &|| true,
            &[("Authorization", "Bearer segreto")],
        )
        .expect("il salto è ammesso");
    assert_eq!(pezzo.byte, b"ciao");
    let richieste = servitore.richieste();
    assert_eq!(richieste.len(), 2);
    assert!(richieste[0].contains("Bearer segreto"));
    assert!(
        !richieste[1].contains("Bearer segreto"),
        "l'indirizzo a cui si è rimandati non è chi ha ricevuto la chiave: {}",
        richieste[1]
    );
}

fn rimanda_o_risponde(percorso: &str) -> String {
    match percorso {
        "/contenuto" => rimando("https://firmato.esempio/brano?tempauth=segreta"),
        "/relativo" => rimando("/firmato/brano?t=1"),
        "/fuori" => rimando("http://esempio.com/brano"),
        "/sul-posto" => {
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nciao".to_owned()
        }
        _ => {
            let corpo = r#"{"error":{"code":"itemNotFound"}}"#;
            format!(
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corpo}",
                corpo.len()
            )
        }
    }
}

#[test]
fn il_rimando_di_un_get_si_prende_senza_andarci() {
    let servitore = Servitore::avvia(rimanda_o_risponde);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let chiave = [("Authorization", "Bearer segreto")];

    let assoluto = rete
        .dove_rimanda(&format!("{}/contenuto", servitore.base), &chiave)
        .expect("un 302 è la risposta che si cerca");
    assert_eq!(
        assoluto.as_deref(),
        Some("https://firmato.esempio/brano?tempauth=segreta")
    );
    let relativo = rete
        .dove_rimanda(&format!("{}/relativo", servitore.base), &chiave)
        .expect("un bersaglio relativo si risolve");
    assert_eq!(
        relativo.as_deref(),
        Some(format!("{}/firmato/brano?t=1", servitore.base).as_str())
    );
    let richieste = servitore.richieste();
    assert_eq!(
        richieste.len(),
        2,
        "il bersaglio non riceve niente: la domanda era solo «dove»"
    );
    assert!(richieste.iter().all(|r| r.contains("Bearer segreto")));
}

#[test]
fn un_get_che_risponde_sul_posto_non_ha_rimando() {
    let servitore = Servitore::avvia(rimanda_o_risponde);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let esito = rete
        .dove_rimanda(&format!("{}/sul-posto", servitore.base), &[])
        .expect("un 200 non è un guasto");
    assert_eq!(esito, None);
}

#[test]
fn un_rimando_che_si_prende_deve_restare_in_https() {
    let servitore = Servitore::avvia(rimanda_o_risponde);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let errore = rete
        .dove_rimanda(&format!("{}/fuori", servitore.base), &[])
        .expect_err("un indirizzo in chiaro fuori di casa non si tiene");
    assert_eq!(errore.code().kind().code(), "net.badSchema");
}

#[test]
fn uno_stato_di_errore_al_posto_del_rimando_dice_lo_stato_e_il_corpo() {
    let servitore = Servitore::avvia(rimanda_o_risponde);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let errore = rete
        .dove_rimanda(&format!("{}/sparito", servitore.base), &[])
        .expect_err("un 404 non è un rimando");
    assert!(
        matches!(errore.code(), ErrorCode::NetHttp { status: 404, .. }),
        "{errore}"
    );
    assert!(
        errore.cause().is_some_and(|c| c.contains("itemNotFound")),
        "il corpo dice quale cosa manca: {errore}"
    );
}

#[test]
fn un_410_non_si_ritenta() {
    // Il segnale del cursore scaduto dei servizi cloud: ritentarlo
    // vorrebbe dire chiedere tre volte un cursore che non vale più.
    let rete = Rete::nuova("prova", Duration::from_secs(1));
    let mut volte = 0;
    let esito: Result<(), AppError> = rete.ritenta(3, || {
        volte += 1;
        Err(AppError::new(ErrorCode::NetHttp {
            status: 410,
            url: None,
        }))
    });
    assert!(esito.is_err());
    assert_eq!(volte, 1);
}

/// Il bilancio di un intervallo guarda anche la pausa prima del tentativo
/// dopo. Guardato solo nel momento del guasto, un `429` con `Retry-After:
/// 30` caduto a trenta secondi faceva aspettare altri trenta secondi e
/// partire il terzo tentativo a un minuto, oltre i quarantacinque promessi.
/// Qui in piccolo: bilancio di 1,5 s, pause chieste di 1 s.
#[test]
fn il_bilancio_vale_anche_per_la_pausa() {
    let rete = Rete::nuova("prova", Duration::from_secs(1));
    let inizio = std::time::Instant::now();
    let mut volte = 0;
    let esito: Result<(), AppError> = rete.ripeti(
        3,
        &|| true,
        &AppError::is_retryable,
        Some(inizio + Duration::from_millis(1500)),
        || {
            volte += 1;
            Err(AppError::new(ErrorCode::NetRateLimited {
                service: None,
                retry_after_ms: Some(1000),
            }))
        },
    );
    let speso = inizio.elapsed();
    assert!(
        matches!(
            esito.as_ref().map_err(AppError::code),
            Err(ErrorCode::NetRateLimited { .. })
        ),
        "il guasto si racconta per quel che è: {esito:?}"
    );
    assert_eq!(
        volte, 2,
        "il secondo guasto cade a un secondo: un'altra pausa di un secondo sfora il bilancio"
    );
    assert!(
        speso < Duration::from_millis(1400),
        "si racconta subito invece di aspettare ancora: {speso:?}"
    );
}

/// Il blocco di `ureq` nel `Cargo.lock` del workspace, fino al pacchetto
/// dopo.
fn ureq_nel_lock() -> String {
    let lock = include_str!("../../../../Cargo.lock").replace("\r\n", "\n");
    lock.split("[[package]]")
        .find(|blocco| blocco.trim_start().starts_with("name = \"ureq\"\n"))
        .unwrap_or_default()
        .to_owned()
}

/// Il proxy di Windows lo legge `ureq` soltanto con `win-system-proxy`, e
/// quella funzione si vede da fuori in un posto solo: `winreg` fra le
/// dipendenze di `ureq` nel `Cargo.lock`. Senza, un portatile d'ufficio
/// con il proxy impostato in Windows e nessuna uscita diretta vedeva fallire
/// Drive, i cataloghi e lo scrobbling, mentre l'aggiornamento — `reqwest`,
/// che il registro lo legge — funzionava. La sottostringa senza la
/// virgoletta di chiusura regge anche la forma `"winreg 0.56.0"` di un lock
/// che ne ha due versioni.
#[test]
fn la_rete_legge_il_proxy_di_windows() {
    let blocco = ureq_nel_lock();
    assert!(!blocco.is_empty(), "ureq deve stare nel Cargo.lock");
    assert!(
        blocco.contains("\"winreg"),
        "ureq senza `win-system-proxy` ignora il proxy di Windows:\n{blocco}"
    );
}

/// Un proxy che non risponde: una porta di questa macchina presa e subito
/// lasciata.
fn proxy_morto() -> ureq::Proxy {
    let porta = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|ascolto| ascolto.local_addr())
        .map(|indirizzo| indirizzo.port())
        .expect("una porta su questa macchina");
    ureq::Proxy::new(&format!("http://127.0.0.1:{porta}")).expect("un proxy si scrive")
}

/// Verso questa macchina si va diretti anche quando l'agente ha un proxy,
/// per ogni via da cui una richiesta può partire: è la regola `<local>` di
/// Windows (vedi [`senza_proxy`]). Un proxy d'ufficio non sa dove sia il
/// `127.0.0.1` di chi lo chiama, e Ollama o LM Studio smetterebbero di
/// rispondere il giorno in cui il proxy si accende.
#[test]
fn verso_questa_macchina_non_si_passa_dal_proxy() {
    let servitore = Servitore::avvia(sempre_ciao);
    let configurazione = di_base()
        .timeout_global(Some(Duration::from_secs(5)))
        .https_only(false)
        .proxy(Some(proxy_morto()))
        .build();
    let rete = Rete {
        agente: configurazione.into(),
        ..servitore.cliente(Rimandi::Nessuno)
    };
    let url = format!("{}/brano.mp3", servitore.base);

    for metodo in [
        Metodo::Get,
        Metodo::Post,
        Metodo::Patch,
        Metodo::Put,
        Metodo::Delete,
        Metodo::Propfind,
    ] {
        let risposta = rete
            .esegui(Richiesta {
                metodo,
                url: &url,
                intestazioni: &[],
                corpo: Corpo::Niente,
            })
            .unwrap_or_else(|err| panic!("{metodo:?} è passato dal proxy: {err}"));
        assert_eq!(risposta.corpo, b"ciao", "{metodo:?}");
    }
    let pezzo = rete
        .intervallo(&url, 0, 4)
        .unwrap_or_else(|err| panic!("l'intervallo è passato dal proxy: {err}"));
    assert_eq!(pezzo.byte, b"ciao");
    let mut preso = Vec::new();
    rete.preleva(&url, &[], &mut preso, &|| false, &mut |_, _| {})
        .unwrap_or_else(|err| panic!("il prelievo è passato dal proxy: {err}"));
    assert_eq!(preso, b"ciao");
    let mut arrivato = Vec::new();
    rete.flusso(&url, &[], Corpo::Niente, &|| false, &mut |pezzo| {
        arrivato.extend_from_slice(pezzo);
        Ok(())
    })
    .unwrap_or_else(|err| panic!("il flusso è passato dal proxy: {err}"));
    assert_eq!(arrivato, b"ciao");
    rete.dove_rimanda(&url, &[])
        .unwrap_or_else(|err| panic!("la domanda del rimando è passata dal proxy: {err}"));
}

/// Il rovescio della prova di sopra: un ospite col punto il proxy lo usa.
/// Senza, una regola che togliesse il proxy a tutti passerebbe ogni altra
/// prova. Qui il «proxy» è il servitore finto, e la cosa da vedere è che
/// gli arrivi la richiesta di aprire il tunnel verso l'ospite.
#[test]
fn un_ospite_col_punto_passa_dal_proxy() {
    let servitore = Servitore::avvia(sempre_ciao);
    let configurazione = di_base()
        .timeout_global(Some(Duration::from_secs(5)))
        .proxy(Some(
            ureq::Proxy::new(&servitore.base).expect("il servitore fa da proxy"),
        ))
        .build();
    let rete = Rete {
        agente: configurazione.into(),
        ..Rete::nuova("prova", Duration::from_secs(5))
    };
    // Il servitore non sa aprire un tunnel: la richiesta cade, e va bene.
    let _ = rete.esegui(Richiesta {
        metodo: Metodo::Get,
        url: "https://ospite.esempio/brano.mp3",
        intestazioni: &[],
        corpo: Corpo::Niente,
    });
    let richieste = servitore.richieste();
    assert!(
        richieste
            .iter()
            .any(|testa| testa.starts_with("CONNECT ospite.esempio:443")),
        "la richiesta doveva passare dal proxy: {richieste:?}"
    );
}

/// La regola `<local>` di Windows, che `ureq` butta via: la tabella che la
/// definisce.
#[test]
fn la_regola_local_di_windows() {
    for diretto in [
        "https://nas/musica/",
        "https://diskstation:5006/dav/",
        "http://localhost:11434/v1",
        "https://LOCALHOST/",
        "http://127.0.0.1:1234/v1",
        "http://127.4.5.6/",
        "http://[::1]:11434/v1",
    ] {
        assert!(senza_proxy(diretto), "{diretto} va diretto");
    }
    for al_proxy in [
        "https://nas.lan/musica/",
        "https://192.168.1.10/dav/",
        "https://[2001:db8::1]/",
        "https://www.googleapis.com/drive/v3",
        "http://localhost@esempio.com/",
        "https://",
    ] {
        assert!(!senza_proxy(al_proxy), "{al_proxy} passa dal proxy");
    }
}

/// Il client dei modelli in chiaro non ha proxy nemmeno nell'agente. Su un
/// computer senza proxy passa anche senza la riga: è una guardia, non la
/// prova del difetto — quella è `verso_questa_macchina_non_si_passa_dal_proxy`.
#[test]
fn il_client_di_casa_non_passa_dal_proxy() {
    let rete = Rete::verso(
        "ollama",
        Duration::from_secs(1),
        "http://127.0.0.1:11434/v1",
    )
    .expect("un indirizzo di casa costruisce");
    assert!(rete.agente.config().proxy().is_none());
}

/// Il limite scritto in testa al modulo: la forma per protocollo di
/// `ProxyServer` (`http=…;https=…`) per `ureq` non è un indirizzo, quindi
/// non diventa un proxy rotto che farebbe cadere tutto — si va diretti come
/// prima. `ureq` la legge come `http://{valore}`.
/// La risposta della RFC 9110 a un `Range` che comincia dopo l'ultimo byte
/// di un file da quattro.
fn in_fondo_al_file(_: &str) -> String {
    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */4\r\n\
     Content-Length: 0\r\nConnection: close\r\n\r\n"
        .to_owned()
}

/// Un 416 che non dice quanto è lungo il file.
fn rifiuta_senza_dire(_: &str) -> String {
    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        .to_owned()
}

#[test]
fn un_416_in_fondo_al_file_e_la_fine() {
    let servitore = Servitore::avvia(in_fondo_al_file);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let pezzo = rete
        .intervallo(&format!("{}/brano.mp3", servitore.base), 4, 4)
        .expect("dopo l'ultimo byte c'è la fine, non un guasto");
    assert!(pezzo.byte.is_empty());
    assert_eq!(pezzo.totale, Some(4), "il 416 dice anche quanto è lungo");

    let servitore = Servitore::avvia(rifiuta_senza_dire);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let pezzo = rete
        .intervallo(&format!("{}/brano.mp3", servitore.base), 8, 4)
        .expect("da metà in poi, un 416 muto è la fine");
    assert!(pezzo.byte.is_empty());
    assert_eq!(pezzo.totale, None);
}

/// La guardia: un 416 che dice una fine **dopo** il punto chiesto è un
/// rifiuto vero, e un 416 muto al primo byte anche. Presi per una fine,
/// troncherebbero il brano in silenzio.
#[test]
fn un_416_a_meta_file_resta_un_errore() {
    let servitore = Servitore::avvia(in_fondo_al_file);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let errore = rete
        .intervallo(&format!("{}/brano.mp3", servitore.base), 2, 4)
        .expect_err("il file arriva a quattro: a due non è finito");
    assert!(
        matches!(errore.code(), ErrorCode::NetHttp { status: 416, .. }),
        "{errore}"
    );

    let servitore = Servitore::avvia(rifiuta_senza_dire);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let errore = rete
        .intervallo(&format!("{}/brano.mp3", servitore.base), 0, 4)
        .expect_err("al primo byte un 416 non è la fine di niente");
    assert!(
        matches!(errore.code(), ErrorCode::NetHttp { status: 416, .. }),
        "{errore}"
    );
}

/// Quante richieste ha visto il servitore che non dice la lunghezza.
static FINE_IGNOTA: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Un servizio che accetta `Range` ma non dichiara quanto è lungo il file,
/// e alla finestra dopo l'ultimo byte risponde 416 come vuole la RFC.
fn senza_lunghezza_poi_416(_: &str) -> String {
    if FINE_IGNOTA.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
        return "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-3/*\r\n\
                Content-Length: 4\r\nConnection: close\r\n\r\nciao"
            .to_owned();
    }
    in_fondo_al_file("")
}

/// Il difetto dal punto di vista di chi ascolta: senza lunghezza
/// dichiarata, la fine del brano arrivava come 416 e diventava «rete
/// interrotta». La prova `senza_lunghezza_dichiarata_la_fine_si_riconosce_dal_vuoto`
/// di `flusso` usa una sorgente che risponde vuoto, cosa che HTTP non fa.
#[test]
fn un_flusso_senza_lunghezza_finisce_sul_416_e_non_su_un_guasto() {
    let servitore = Servitore::avvia(senza_lunghezza_poi_416);
    let rete = servitore.cliente(Rimandi::Nessuno);
    let mut flusso = crate::flusso::FlussoHttp::nuovo(
        &rete,
        format!("{}/brano.mp3", servitore.base),
        Cancello::nuovo(|_| true),
    )
    .expect("la prima finestra arriva");
    assert_eq!(flusso.lunghezza(), None, "il servizio non l'ha detta");
    let mut tutto = Vec::new();
    std::io::Read::read_to_end(&mut flusso, &mut tutto)
        .expect("la fine del brano non è un guasto di rete");
    assert_eq!(tutto, b"ciao");
    assert_eq!(flusso.lunghezza(), Some(4), "e dal 416 la si impara");
}

#[test]
fn un_proxy_per_protocollo_non_si_legge_e_si_va_diretti() {
    assert!(ureq::Proxy::new("http://http=proxy:80;https=proxy:443").is_err());
    assert!(ureq::Proxy::new("http://proxy.ufficio:8080").is_ok());
}
