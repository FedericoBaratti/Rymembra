//! Prove con un capo «grezzo»: l'altra parte della connessione la fa la prova
//! a mano, con gli stessi pezzi del crate (stretta, cifratore, saluto), e può
//! quindi fare quel che un Rymembra vero non fa — rivelare un nonce falso,
//! dichiarare un'altra versione, tacere, non rispondere a una lettura,
//! annunciare una cornice più grande del tetto.
//!
//! Stanno dentro il crate, e non in `tests/`, perché il capo grezzo usa moduli
//! privati: renderli pubblici solo per le prove vorrebbe dire offrire al
//! desktop un modo di parlare il protocollo senza la `Rete`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::canale::{self, Cifratore, Decifratore};
use crate::codice;
use crate::collegamento::json;
use crate::cornice::{MASSIMO_MESSAGGIO, Tipo, comprimi_al_livello, gonfia};
use crate::identita::{ChiavePubblica, Identita};
use crate::messaggi::{
    DescrittoreCopia, Diritti, Gettone, Messaggio, Persona, Ripeti, SorgenteSulFilo, StatoSessione,
    VERSIONE, VoceSulFilo,
};
use crate::trasporto::{Byte32, Trasporto, Uso};
use crate::{Evento, Impostazioni, Rete, Tempi};

pub(super) fn adesso_ns() -> u64 {
    static BASE: OnceLock<Instant> = OnceLock::new();
    u64::try_from(BASE.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(u64::MAX)
}

pub(super) const ATTESA: Duration = Duration::from_secs(5);

/// Una `Rete` vera su `127.0.0.1`.
pub(super) fn vera(tempi: Tempi) -> (Rete, Receiver<Evento>) {
    Rete::avvia_con(
        Impostazioni {
            identita: Identita::nuova().expect("chiave"),
            nome: "vera".to_owned(),
            lega: IpAddr::V4(Ipv4Addr::LOCALHOST),
            porta: 0,
            adesso_ns,
        },
        tempi,
    )
    .expect("parte")
}

/// Un host vero, già in ascolto.
pub(super) fn host(tempi: Tempi) -> (Rete, Receiver<Evento>, SocketAddr) {
    let (rete, eventi) = vera(tempi);
    let porta = rete.ospita().expect("ospita");
    (rete, eventi, SocketAddr::from((Ipv4Addr::LOCALHOST, porta)))
}

/// Il primo evento che risponde a `cerca`, entro `entro`.
pub(super) fn aspetta<T>(
    eventi: &Receiver<Evento>,
    entro: Duration,
    mut cerca: impl FnMut(&Evento) -> Option<T>,
) -> Option<T> {
    let fine = Instant::now() + entro;
    loop {
        let evento = eventi
            .recv_timeout(fine.saturating_duration_since(Instant::now()))
            .ok()?;
        if let Some(trovato) = cerca(&evento) {
            return Some(trovato);
        }
    }
}

/// Il capo grezzo.
pub(super) struct Grezzo {
    pub(super) presa: TcpStream,
    pub(super) cifratore: Cifratore,
    pub(super) decifratore: Decifratore,
    pub(super) identita: Identita,
}

impl Grezzo {
    /// Stretta e saluto, con la versione e l'uso che si vuole. Restituisce
    /// anche il saluto dell'altra parte.
    pub(super) fn stringi(
        presa: TcpStream,
        iniziatore: bool,
        versione: u32,
        uso: Uso,
    ) -> (Self, Option<Trasporto>) {
        Self::stringi_con(
            Identita::nuova().expect("chiave"),
            presa,
            iniziatore,
            versione,
            uso,
        )
    }

    /// Come [`Grezzo::stringi`], con una chiave già fatta: serve quando chi
    /// si collega deve saperla prima.
    pub(super) fn stringi_con(
        identita: Identita,
        mut presa: TcpStream,
        iniziatore: bool,
        versione: u32,
        uso: Uso,
    ) -> (Self, Option<Trasporto>) {
        let canale = canale::stringi(
            &mut presa,
            &identita,
            iniziatore,
            None,
            Instant::now() + ATTESA,
        )
        .expect("stretta");
        let (cifratore, decifratore) = canale.dividi();
        let mut grezzo = Self {
            cifratore,
            decifratore,
            presa,
            identita,
        };
        grezzo.manda(&Trasporto::Ciao {
            versione,
            nome: "grezzo".to_owned(),
            ospita: !iniziatore,
            uso,
        });
        let saluto = grezzo.leggi(ATTESA);
        (grezzo, saluto)
    }

    pub(super) fn manda(&mut self, parola: &Trasporto) {
        self.cifratore
            .aggiungi(Tipo::Json, &json(parola))
            .expect("cifra");
        self.cifratore.manda(&mut self.presa).expect("scrive");
    }

    /// Il prossimo messaggio che non sia un battito, decompresso se arriva
    /// compresso.
    pub(super) fn leggi(&mut self, entro: Duration) -> Option<Trasporto> {
        loop {
            let (tipo, carico) = self.leggi_grezzo(entro)?;
            let carico = match tipo {
                Tipo::Json => carico,
                Tipo::JsonCompresso => gonfia(&carico, MASSIMO_MESSAGGIO).ok()?,
                Tipo::Byte => continue,
            };
            if let Ok(parola) = serde_json::from_slice::<Trasporto>(&carico) {
                return Some(parola);
            }
            return None;
        }
    }

    /// Il prossimo messaggio che non sia un battito, **come è arrivato**: il
    /// tipo di cornice e il carico, compresso se viaggiava compresso.
    pub(super) fn leggi_grezzo(&mut self, entro: Duration) -> Option<(Tipo, Vec<u8>)> {
        loop {
            let (tipo, carico, _) = self
                .decifratore
                .prossimo(&mut self.presa, entro, adesso_ns)
                .ok()?;
            let battito = tipo == Tipo::Json
                && serde_json::from_slice::<Trasporto>(&carico).ok() == Some(Trasporto::Battito);
            if !battito {
                return Some((tipo, carico));
            }
        }
    }

    /// Manda un carico così com'è, in una cornice di quel tipo.
    pub(super) fn manda_grezzo(&mut self, tipo: Tipo, carico: &[u8]) {
        self.cifratore.aggiungi(tipo, carico).expect("cifra");
        self.cifratore.manda(&mut self.presa).expect("scrive");
    }
}

/// Uno stato di sessione di `voci` voci che somiglia a uno vero: titoli,
/// interpreti e album che si ripetono come in una libreria, e i campi che non
/// si comprimono (gettoni, impronte) presi da blake3, cioè a caso ma uguali a
/// ogni giro.
pub(super) fn stato_di_prova(voci: usize, versione: u64) -> Messaggio {
    let hash = |seme: String| *blake3::hash(seme.as_bytes()).as_bytes();
    let esa = |seme: String, quanti: usize| {
        let byte = hash(seme);
        crate::esadecimale::scrivi(byte.get(..quanti).unwrap_or_default())
    };
    let gettone = |seme: String| {
        let byte = hash(seme);
        let mut sedici = [0_u8; 16];
        sedici.copy_from_slice(byte.get(..16).unwrap_or(&[0; 16]));
        Gettone(sedici)
    };
    let coda = (0..voci)
        .map(|i| {
            let disco = i.div_euclid(12);
            let numero = u64::try_from(i).unwrap_or(0);
            VoceSulFilo {
                uid: numero + 1,
                di: u32::try_from(i % 3).unwrap_or(0),
                titolo: format!("Titolo del brano numero {i} (versione dal vivo)"),
                artista: format!("Interprete {}", disco % 37),
                album: format!("Album {disco} — edizione rimasterizzata"),
                durata_ms: 180_000 + numero * 777 % 120_000,
                numero: Some(u32::try_from(i % 12 + 1).unwrap_or(1)),
                anno: Some(1970 + i32::try_from(disco % 50).unwrap_or(0)),
                copertina: Some(gettone(format!("copertina {disco}"))),
                sorgente: SorgenteSulFilo::File {
                    gettone: gettone(format!("file {i}")),
                    estensione: Some("flac".to_owned()),
                    dimensione: 20_000_000 + numero * 4_099,
                    segmento: None,
                },
                replaygain_db: Some(-7.25),
                picco: Some(0.987),
                copia: Some(DescrittoreCopia {
                    unc: None,
                    dimensione: 20_000_000 + numero * 4_099,
                    modificato_ms: 1_700_000_000_000 + i64::try_from(i).unwrap_or(0),
                    impronta_rapida: esa(format!("impronta {i}"), 32),
                    chiave_contenuto: Some(esa(format!("chiave {i}"), 20)),
                }),
            }
        })
        .collect();
    Messaggio::Stato(StatoSessione {
        versione,
        persone: vec![
            Persona {
                membro: 0,
                nome: "PC di Anna".to_owned(),
                diritti: Diritti::Tutto,
            },
            Persona {
                membro: 1,
                nome: "PC di Bruno".to_owned(),
                diritti: Diritti::Comanda,
            },
        ],
        coda,
        corrente: Some(1),
        in_pausa: false,
        ripeti: Ripeti::No,
    })
}

/// Il JSON di un messaggio dell'applicazione, come lo impacchetta `Rete::manda`.
pub(super) fn json_app(messaggio: &Messaggio) -> Vec<u8> {
    json(&crate::trasporto::AppDaMandare { messaggio })
}

#[test]
fn si_misura_quanto_comprime_uno_stato() {
    // Non una soglia: i numeri per scegliere il livello (vedi `LIVELLO`), da
    // leggere con `cargo test --release -p rymembra-insieme -- --nocapture
    // si_misura_quanto_comprime`. Nella build di prova i tempi valgono poco.
    let json = json_app(&stato_di_prova(971, 1));
    for livello in [1, 3, 6] {
        // Il migliore di tre giri: il primo paga le pagine fredde.
        let (mut comprimere, mut decomprimere) = (Duration::MAX, Duration::MAX);
        let mut compresso = Vec::new();
        for _ in 0..3 {
            let inizio = Instant::now();
            compresso = comprimi_al_livello(&json, livello).expect("si comprime");
            comprimere = comprimere.min(inizio.elapsed());
            let inizio = Instant::now();
            let tornato = gonfia(&compresso, MASSIMO_MESSAGGIO).expect("si decomprime");
            decomprimere = decomprimere.min(inizio.elapsed());
            assert!(tornato == json, "torna uguale");
        }
        eprintln!(
            "stato di 971 voci: {} byte di JSON, livello {livello}: {} byte compressi \
             ({:.1}%), comprimere {comprimere:?}, decomprimere {decomprimere:?}",
            json.len(),
            compresso.len(),
            100.0 * f64::from(u32::try_from(compresso.len()).unwrap_or(0))
                / f64::from(u32::try_from(json.len()).unwrap_or(1)),
        );
    }
}

pub(super) fn chiudo(codice: &str) -> Option<Trasporto> {
    Some(Trasporto::Chiudo {
        codice: Some(codice.to_owned()),
    })
}

#[test]
fn la_rivelazione_falsa_chiude_con_codice_rifiutato() {
    let (rete, eventi, indirizzo) = host(Tempi::default());
    let presa = TcpStream::connect(indirizzo).expect("connessione");
    let (mut grezzo, _) = Grezzo::stringi(presa, true, VERSIONE, Uso::Controllo);
    let id = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Aperta { id, .. } => Some(*id),
        _ => None,
    })
    .expect("aperta");

    let mia = grezzo.identita.pubblica();
    grezzo.manda(&Trasporto::Impegno {
        impegno: Byte32(codice::impegno(&[1; 32], &mia, &rete.pubblica())),
    });
    assert!(matches!(
        grezzo.leggi(ATTESA),
        Some(Trasporto::Nonce { .. })
    ));
    // Un nonce diverso da quello su cui ci si era impegnati: è proprio quel
    // che farebbe un terzo nel mezzo che cerca un codice uguale.
    grezzo.manda(&Trasporto::Rivela {
        nonce: Byte32([2; 32]),
    });
    assert_eq!(grezzo.leggi(ATTESA), chiudo("insieme.codiceRifiutato"));
    let mut codici = 0;
    let chiusa = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Codice { .. } => {
            codici += 1;
            None
        }
        Evento::Chiusa { id: chiusa, codice } if *chiusa == id => Some(*codice),
        _ => None,
    })
    .expect("chiusa");
    assert_eq!(chiusa.map(|k| k.code()), Some("insieme.codiceRifiutato"));
    assert_eq!(codici, 0, "nessun codice sullo schermo dell'host");
}

#[test]
fn una_versione_diversa_si_chiude_con_versione_dai_due_lati() {
    // Un PC più nuovo bussa a un host di questa versione.
    let (_rete, eventi, indirizzo) = host(Tempi::default());
    let presa = TcpStream::connect(indirizzo).expect("connessione");
    let (mut grezzo, saluto) = Grezzo::stringi(presa, true, VERSIONE + 1, Uso::Controllo);
    assert!(
        matches!(saluto, Some(Trasporto::Ciao { versione, .. }) if versione == VERSIONE),
        "la stretta si finisce, e il saluto dice la versione: {saluto:?}"
    );
    assert_eq!(grezzo.leggi(ATTESA), chiudo("insieme.versione"));
    assert!(
        aspetta(&eventi, Duration::from_millis(300), |e| match e {
            Evento::Aperta { .. } => Some(()),
            _ => None,
        })
        .is_none(),
        "l'host non annuncia una connessione che non si è aperta"
    );

    // E questo PC che bussa a un host più nuovo.
    let (membro, eventi) = vera(Tempi::default());
    let ascolto = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("ascolto");
    let indirizzo = ascolto.local_addr().expect("indirizzo");
    let id = membro.collega(indirizzo);
    let (presa, _) = ascolto.accept().expect("accetta");
    let (_grezzo, _) = Grezzo::stringi(presa, false, VERSIONE + 1, Uso::Controllo);
    let chiusa = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Chiusa { id: chiusa, codice } if *chiusa == id => Some(*codice),
        _ => None,
    })
    .expect("chiusa");
    assert_eq!(chiusa.map(|k| k.code()), Some("insieme.versione"));
}

#[test]
fn il_silenzio_chiude_la_connessione_con_connessione_caduta() {
    let tempi = Tempi {
        battito: Duration::from_millis(100),
        silenzio: Duration::from_millis(400),
        ..Tempi::default()
    };
    let (_rete, eventi, indirizzo) = host(tempi);
    let presa = TcpStream::connect(indirizzo).expect("connessione");
    // Il grezzo saluta e poi tace: niente battiti.
    let (_grezzo, _) = Grezzo::stringi(presa, true, VERSIONE, Uso::Controllo);
    let id = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Aperta { id, .. } => Some(*id),
        _ => None,
    })
    .expect("aperta");
    let aperta = Instant::now();
    let chiusa = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Chiusa { id: chiusa, codice } if *chiusa == id => Some(*codice),
        _ => None,
    })
    .expect("chiusa");
    let dopo = aperta.elapsed();
    assert_eq!(chiusa.map(|k| k.code()), Some("insieme.connessioneCaduta"));
    assert!(
        dopo >= Duration::from_millis(300) && dopo < Duration::from_secs(2),
        "chiusa dopo il silenzio, non prima e non molto dopo: {dopo:?}"
    );
}

#[test]
fn una_cornice_oltre_il_mebibyte_chiude_la_connessione() {
    let (_rete, eventi, indirizzo) = host(Tempi::default());
    let presa = TcpStream::connect(indirizzo).expect("connessione");
    let (mut grezzo, _) = Grezzo::stringi(presa, true, VERSIONE, Uso::Controllo);
    let id = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Aperta { id, .. } => Some(*id),
        _ => None,
    })
    .expect("aperta");
    // Una testa che annuncia un byte più del tetto, e nient'altro.
    let mut testa = u32::try_from(MASSIMO_MESSAGGIO + 1)
        .expect("sta")
        .to_be_bytes()
        .to_vec();
    testa.push(1);
    grezzo.cifratore.cifra_chiaro(&testa).expect("cifra");
    grezzo.cifratore.manda(&mut grezzo.presa).expect("scrive");
    assert_eq!(grezzo.leggi(ATTESA), chiudo("insieme.protocollo"));
    let chiusa = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Chiusa { id: chiusa, codice } if *chiusa == id => Some(*codice),
        _ => None,
    })
    .expect("chiusa");
    assert_eq!(chiusa.map(|k| k.code()), Some("insieme.protocollo"));
}

/// Un host grezzo che accetta una connessione di lettura, la saluta, legge la
/// prima richiesta e poi fa quel che dice `risposta`, col numero della
/// lettura. Manda un segnale quando la richiesta è arrivata.
fn host_grezzo(
    risposta: impl FnOnce(&mut Grezzo, u32) + Send + 'static,
) -> (
    SocketAddr,
    ChiavePubblica,
    Receiver<()>,
    std::thread::JoinHandle<()>,
) {
    let ascolto = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("ascolto");
    let indirizzo = ascolto.local_addr().expect("indirizzo");
    let identita = Identita::nuova().expect("chiave");
    let chiave = identita.pubblica();
    let (arrivata, prendi_arrivata) = channel();
    let filo = std::thread::spawn(move || {
        let (presa, _) = ascolto.accept().expect("accetta");
        let (mut grezzo, _) = Grezzo::stringi_con(identita, presa, false, VERSIONE, Uso::Controllo);
        let Some(Trasporto::Leggi { lettura, .. }) = grezzo.leggi(ATTESA) else {
            panic!("doveva arrivare una lettura");
        };
        let _ = arrivata.send(());
        risposta(&mut grezzo, lettura);
    });
    (indirizzo, chiave, prendi_arrivata, filo)
}

#[test]
fn l_annullo_durante_l_attesa_torna_subito_e_butta_la_connessione() {
    // L'host grezzo legge la richiesta e non risponde mai. Quando la
    // connessione si butta, lo vede chiudere dall'altra parte.
    let (fatto, aspetta_fatto) = channel();
    let (indirizzo, chiave, arrivata, filo) = host_grezzo(move |grezzo, _| {
        let fine = grezzo.leggi(ATTESA);
        let _ = fatto.send(fine);
    });
    let (membro, _eventi) = vera(Tempi::default());
    let lettore = membro.lettore(indirizzo, chiave);
    let annullo = Arc::new(AtomicBool::new(false));
    let per_il_filo = Arc::clone(&annullo);
    let lettura = std::thread::spawn(move || {
        let esito = lettore.pezzo(&Gettone([1; 16]), 0, 100, Some(&per_il_filo));
        (esito, Instant::now())
    });
    arrivata.recv_timeout(ATTESA).expect("la richiesta arriva");
    // Il lettore è fermo ad aspettare una risposta che non verrà.
    std::thread::sleep(Duration::from_millis(50));
    let alzata = Instant::now();
    annullo.store(true, Ordering::SeqCst);
    let (esito, tornata) = lettura.join().expect("filo");
    let ritardo = tornata.saturating_duration_since(alzata);
    let err = esito.expect_err("annullata");
    assert_eq!(err.code().kind().code(), "internal.aborted");
    assert!(ritardo < Duration::from_millis(100), "{ritardo:?}");
    eprintln!("annullo: la lettura è tornata {ritardo:?} dopo la bandiera");
    // Dall'altra parte la connessione si è chiusa senza `Chiudo`: buttata.
    assert_eq!(
        aspetta_fatto
            .recv_timeout(ATTESA)
            .expect("il grezzo vede la fine"),
        None
    );
    filo.join().expect("grezzo");
}

#[test]
fn una_risposta_che_annuncia_troppi_byte_chiude_la_connessione() {
    let (indirizzo, chiave, _arrivata, filo) = host_grezzo(|grezzo, lettura| {
        grezzo.manda(&Trasporto::Pezzo {
            lettura,
            totale: u64::MAX,
            byte: crate::servente::LETTURA_MASSIMA + 1,
        });
        assert_eq!(grezzo.leggi(ATTESA), chiudo("insieme.protocollo"));
    });
    let (membro, _eventi) = vera(Tempi::default());
    let err = membro
        .lettore(indirizzo, chiave)
        .pezzo(&Gettone([1; 16]), 0, 100, None)
        .expect_err("protocollo");
    // La lettura caduta si richiede una volta su una connessione nuova (vedi
    // «La riconnessione» in `cliente`), e questo host grezzo ne accetta una
    // sola: la seconda non trova nessuno. Prima della revisione del 26
    // ottobre una connessione appena aperta non si richiedeva, e il perché
    // era `connessioneCaduta`.
    assert_eq!(err.code().kind().code(), "insieme.irraggiungibile");
    filo.join().expect("grezzo");
}

#[test]
fn una_lettura_su_una_connessione_nuova_che_cade_si_richiede_una_volta() {
    // Prima si richiedeva solo su una connessione riusata: una appena aperta
    // che cadeva — mandata via dall'host per far posto, o l'host riavviato —
    // faceva fallire la lettura.
    let ascolto = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("ascolto");
    let indirizzo = ascolto.local_addr().expect("indirizzo");
    let identita = Identita::nuova().expect("chiave");
    let chiave = identita.pubblica();
    let testo = identita.in_testo();
    let filo = std::thread::spawn(move || {
        // La prima legge la richiesta e cade senza rispondere.
        let (presa, _) = ascolto.accept().expect("accetta");
        let (mut grezzo, _) = Grezzo::stringi_con(identita, presa, false, VERSIONE, Uso::Controllo);
        assert!(matches!(
            grezzo.leggi(ATTESA),
            Some(Trasporto::Leggi { .. })
        ));
        drop(grezzo);
        // La seconda risponde.
        let (presa, _) = ascolto.accept().expect("accetta la seconda");
        let stessa = Identita::da_testo(&testo).expect("la stessa chiave");
        let (mut grezzo, _) = Grezzo::stringi_con(stessa, presa, false, VERSIONE, Uso::Controllo);
        let Some(Trasporto::Leggi { lettura, .. }) = grezzo.leggi(ATTESA) else {
            panic!("doveva arrivare la lettura richiesta");
        };
        grezzo.manda(&Trasporto::Pezzo {
            lettura,
            totale: 3,
            byte: 3,
        });
        grezzo
            .cifratore
            .aggiungi(Tipo::Byte, &[1, 2, 3])
            .expect("cifra");
        grezzo.cifratore.manda(&mut grezzo.presa).expect("scrive");
        let _ = grezzo.leggi(Duration::from_millis(300));
    });
    let (membro, _eventi) = vera(Tempi::default());
    let letto = membro
        .lettore(indirizzo, chiave)
        .pezzo(&Gettone([1; 16]), 0, 3, None)
        .expect("richiesta su una connessione nuova");
    assert_eq!(letto, (vec![1, 2, 3], 3));
    filo.join().expect("grezzo");
}

#[test]
fn l_annullo_durante_la_stretta_torna_subito() {
    // Un host che accetta il TCP e non finisce mai la stretta: prima
    // l'annullo si guardava solo dopo il collegamento e mai durante la
    // stretta, e la lettura tornava dopo i cinque secondi della stretta.
    let ascolto = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("ascolto");
    let indirizzo = ascolto.local_addr().expect("indirizzo");
    let (membro, _eventi) = vera(Tempi::default());
    let lettore = membro.lettore(indirizzo, Identita::nuova().expect("chiave").pubblica());
    let annullo = Arc::new(AtomicBool::new(false));
    let per_il_filo = Arc::clone(&annullo);
    let lettura = std::thread::spawn(move || {
        let esito = lettore.pezzo(&Gettone([1; 16]), 0, 100, Some(&per_il_filo));
        (esito, Instant::now())
    });
    // Il collegamento TCP è fatto (la coda dell'ascolto lo accetta): il
    // lettore è nella stretta, che aspetta una risposta che non verrà.
    let (_muta, _) = ascolto.accept().expect("accetta");
    std::thread::sleep(Duration::from_millis(100));
    let alzata = Instant::now();
    annullo.store(true, Ordering::SeqCst);
    let (esito, tornata) = lettura.join().expect("filo");
    let ritardo = tornata.saturating_duration_since(alzata);
    let err = esito.expect_err("annullata");
    assert_eq!(err.code().kind().code(), "internal.aborted");
    assert!(ritardo < Duration::from_millis(200), "{ritardo:?}");
}

#[test]
fn la_scoperta_risponde_al_piu_quattro_volte_al_secondo_allo_stesso_indirizzo() {
    let (rete, _eventi, indirizzo) = host(Tempi::default());
    let presa = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("socket");
    presa
        .set_read_timeout(Some(Duration::from_millis(300)))
        .expect("scadenza");
    // Dieci domande di fila, ciascuna col suo nonce.
    for i in 0_u8..10 {
        presa
            .send_to(&crate::scoperta::domanda(&[i; 16]), indirizzo)
            .expect("domanda");
    }
    let mut risposte = Vec::new();
    let mut buffer = [0_u8; 64];
    while let Ok((quanti, _)) = presa.recv_from(&mut buffer) {
        if let Some(risposta) = buffer
            .get(..quanti)
            .and_then(crate::scoperta::leggi_risposta)
        {
            risposte.push(risposta);
        }
    }
    assert_eq!(risposte.len(), 4, "il tetto per indirizzo");
    for (porta, impronta, _) in &risposte {
        assert_eq!(*porta, indirizzo.port());
        assert_eq!(*impronta, rete.pubblica().impronta());
    }
}

#[test]
fn una_revoca_durante_una_lettura_inoltrata_la_fa_finire_con_gettone_scaduto() {
    // L'host vero; il proprietario del brano è un capo grezzo collegato con
    // la connessione di servizio, che risponde solo quando la prova glielo
    // dice: così la revoca cade proprio mentre l'host aspetta i byte.
    let (host_rete, _eventi, indirizzo) = host(Tempi::default());
    let (membro, _eventi_membro) = vera(Tempi::default());
    let proprietario = Identita::nuova().expect("chiave");
    let chiave_proprietario = proprietario.pubblica();
    host_rete.pari(vec![chiave_proprietario, membro.pubblica()], Vec::new());
    host_rete.ammessi(vec![membro.pubblica()]);
    let gettone = Gettone([9; 16]);
    host_rete.inoltra(gettone, chiave_proprietario);

    let (chiesta, aspetta_chiesta) = channel();
    let (rispondi, aspetta_rispondi) = channel::<()>();
    let grezzo = std::thread::spawn(move || {
        let presa = TcpStream::connect(indirizzo).expect("connessione");
        let (mut grezzo, _) =
            Grezzo::stringi_con(proprietario, presa, true, VERSIONE, Uso::Servizio);
        let Some(Trasporto::Leggi { lettura, .. }) = grezzo.leggi(ATTESA) else {
            panic!("l'host doveva chiedere i byte");
        };
        let _ = chiesta.send(());
        let _ = aspetta_rispondi.recv_timeout(ATTESA);
        grezzo.manda(&Trasporto::Pezzo {
            lettura,
            totale: 3,
            byte: 3,
        });
        grezzo
            .cifratore
            .aggiungi(Tipo::Byte, &[1, 2, 3])
            .expect("cifra");
        grezzo.cifratore.manda(&mut grezzo.presa).expect("scrive");
        // Tiene la connessione finché l'host non ha risposto al membro.
        let _ = grezzo.leggi(Duration::from_millis(500));
    });

    let lettore = membro.lettore(indirizzo, host_rete.pubblica());
    let lettura = std::thread::spawn(move || {
        // La connessione di servizio del grezzo si registra in un filo
        // dell'host: si riprova finché c'è.
        let fine = Instant::now() + ATTESA;
        loop {
            match lettore.pezzo(&gettone, 0, 3, None) {
                Err(err)
                    if err.code().kind().code() == "insieme.irraggiungibile"
                        && Instant::now() < fine =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                esito => return esito,
            }
        }
    });
    aspetta_chiesta
        .recv_timeout(ATTESA)
        .expect("l'host ha chiesto i byte al proprietario");
    host_rete.revoca(&gettone);
    let _ = rispondi.send(());
    let err = lettura.join().expect("filo").expect_err("revocato a metà");
    assert_eq!(err.code().kind().code(), "insieme.gettoneScaduto");
    grezzo.join().expect("grezzo");
}
