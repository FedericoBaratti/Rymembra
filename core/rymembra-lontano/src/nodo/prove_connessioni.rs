//! Le prove del cancello sulle connessioni già aperte, e del tetto di domande
//! per dispositivo: servono le connessioni grezze di un altro nodo, che solo
//! da qui dentro si possono aprire più d'una verso lo stesso dispositivo.

use std::collections::HashSet;
use std::io::{Read as _, Write as _};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

/// Un'app finta: chi è associato, i flussi arrivati, e le domande in corso.
#[derive(Default)]
struct Finta {
    associati: Mutex<HashSet<String>>,
    flussi: Mutex<Vec<Flusso>>,
    /// Le domande dentro `rispondi` adesso, e il massimo visto.
    in_corso: AtomicUsize,
    massimo: AtomicUsize,
    /// `rispondi` aspetta finché è vero (al più qualche secondo).
    ferma: AtomicBool,
}

impl Ospite for Finta {
    fn associato(&self, chiave: &str) -> bool {
        self.associati
            .lock()
            .is_ok_and(|associati| associati.contains(chiave))
    }

    fn conferma(&self, _chiave: &str, _chi: &Presentazione) -> bool {
        false
    }

    fn nuovo_pari(&self, _chiave: &str, _chi: &Presentazione, _indirizzi: &[SocketAddr]) {}

    fn rispondi(&self, _da: &str, _domanda: Domanda) -> Result<Risposta, AppError> {
        let adesso = self.in_corso.fetch_add(1, Ordering::SeqCst) + 1;
        self.massimo.fetch_max(adesso, Ordering::SeqCst);
        let fine = Instant::now() + Duration::from_secs(10);
        while self.ferma.load(Ordering::SeqCst) && Instant::now() < fine {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.in_corso.fetch_sub(1, Ordering::SeqCst);
        Ok(Risposta::Fatto)
    }

    fn dimenticato(&self, _da: &str) {}

    fn insieme(&self, _da: &str, flusso: Flusso) {
        if let Ok(mut flussi) = self.flussi.lock() {
            flussi.push(flusso);
        }
    }

    fn evento(&self, _evento: Evento) {}
}

impl Finta {
    fn ammetti(&self, chiave: &str, dentro: bool) {
        let mut associati = self.associati.lock().expect("lucchetto");
        if dentro {
            associati.insert(chiave.to_owned());
        } else {
            associati.remove(chiave);
        }
    }
}

/// Rimette in moto `rispondi` anche se la prova cade a metà.
struct Libera<'a>(&'a AtomicBool);

impl Drop for Libera<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn nodo(nome: &str) -> (Nodo, Arc<Finta>) {
    let finta = Arc::new(Finta::default());
    let nodo = Nodo::avvia(
        Impostazioni {
            identita: Identita::nuova(),
            rete: Rete::SoloQuesta,
            io: Presentazione {
                nome: nome.to_owned(),
                piattaforma: "prova".to_owned(),
                dispositivo: format!("d-{nome}"),
                chiave_insieme: None,
            },
            presenza: Presenza {
                nome: nome.to_owned(),
                ..Presenza::default()
            },
        },
        Arc::clone(&finta) as Arc<dyn Ospite>,
    )
    .expect("il nodo parte");
    (nodo, finta)
}

/// Una connessione nuova di `da` verso `a`, aperta a mano: il nodo da solo
/// ne terrebbe una per dispositivo.
fn connessione_grezza(da: &Nodo, a: &Nodo, alpn: &[u8]) -> Connection {
    let mut indirizzo = EndpointAddr::new(a.endpoint.id());
    for diretto in a.socket() {
        indirizzo = indirizzo.with_ip_addr(diretto);
    }
    let endpoint = da.endpoint.clone();
    let alpn = alpn.to_vec();
    da.runtime()
        .expect("acceso")
        .block_on(async move {
            tokio::time::timeout(Duration::from_secs(10), endpoint.connect(indirizzo, &alpn)).await
        })
        .expect("in tempo")
        .expect("connessa")
}

fn chiedi_sulla(da: &Nodo, conn: &Connection, domanda: Domanda) -> Result<Risposta, AppError> {
    let conn = conn.clone();
    da.runtime().expect("acceso").block_on(async move {
        tokio::time::timeout(Duration::from_secs(10), chiedi_su(&conn, domanda))
            .await
            .unwrap_or_else(|_| Err(irraggiungibile("la prova ha aspettato troppo")))
    })
}

fn codice(esito: Result<Risposta, AppError>) -> ErrorCodeKind {
    esito.expect_err("un errore").code().kind()
}

fn aspetta(vale: impl Fn() -> bool) -> bool {
    let fine = Instant::now() + Duration::from_secs(10);
    while Instant::now() < fine {
        if vale() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    vale()
}

fn una_domanda() -> Domanda {
    Domanda::Album { da: 0, quanti: 1 }
}

#[test]
fn un_dispositivo_non_piu_associato_non_legge_su_nessuna_connessione() {
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    del_pc.ammetti(&telefono.chiave(), true);

    // Due connessioni dello stesso dispositivo: capita con due chiamate
    // incrociate, o a chi le apre apposta.
    let prima = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
    let seconda = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
    assert_eq!(
        chiedi_sulla(&telefono, &prima, una_domanda()).expect("associato: risponde"),
        Risposta::Fatto
    );
    assert_eq!(
        chiedi_sulla(&telefono, &seconda, una_domanda()).expect("anche sull'altra"),
        Risposta::Fatto
    );

    // Non più associato — senza che nessuno chiuda niente: il cancello si
    // guarda a ogni domanda, non solo quando la connessione si apre.
    del_pc.ammetti(&telefono.chiave(), false);
    assert_eq!(
        codice(chiedi_sulla(&telefono, &prima, una_domanda())),
        ErrorCodeKind::LontanoNonAssociato
    );
    // Neanche la presenza: un dispositivo dimenticato non risulta presente.
    assert_eq!(
        codice(chiedi_sulla(
            &telefono,
            &seconda,
            Domanda::Annuncio(Presenza::default())
        )),
        ErrorCodeKind::LontanoNonAssociato
    );
}

#[test]
fn la_revoca_chiude_ogni_connessione_e_ogni_flusso_di_quella_chiave() {
    revoca_e_guarda(|pc, telefono| pc.revoca(telefono));
}

#[test]
fn chi_esce_dall_elenco_perde_ogni_connessione_e_ogni_flusso() {
    revoca_e_guarda(|pc, _| pc.pari(Vec::new()));
}

/// Due connessioni della libreria e un flusso di «Ascolta insieme» del
/// telefono, poi il PC lo toglie con `revoca`: tutto si chiude.
fn revoca_e_guarda(revoca: impl FnOnce(&Nodo, &str)) {
    let (pc, del_pc) = nodo("PC");
    let (telefono, del_telefono) = nodo("Telefono");
    del_pc.ammetti(&telefono.chiave(), true);
    del_telefono.ammetti(&pc.chiave(), true);
    pc.pari(vec![PariNoto {
        chiave: telefono.chiave(),
        indirizzi: telefono.socket(),
    }]);

    let prima = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
    let seconda = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
    for conn in [&prima, &seconda] {
        chiedi_sulla(&telefono, conn, una_domanda()).expect("associato: risponde");
    }
    // E un flusso di «Ascolta insieme» già aperto.
    let mut di_qua = telefono
        .apri_insieme(&pc.chiave(), Duration::from_secs(10))
        .expect("il flusso si apre");
    di_qua.set_write_timeout(Some(Duration::from_secs(5)));
    di_qua.write_all(b"ciao").expect("scritto");
    assert!(aspetta(|| !del_pc
        .flussi
        .lock()
        .expect("lucchetto")
        .is_empty()));
    let mut di_la = del_pc.flussi.lock().expect("lucchetto").pop().expect("uno");
    di_la.set_read_timeout(Some(Duration::from_secs(5)));
    let mut letti = [0_u8; 4];
    di_la.read_exact(&mut letti).expect("i primi byte");
    assert_eq!(&letti, b"ciao");

    // Il cancello lo dice fuori, e il PC lo toglie.
    del_pc.ammetti(&telefono.chiave(), false);
    revoca(&pc, &telefono.chiave());

    for conn in [&prima, &seconda] {
        let chiusa = conn.clone();
        let chiusa_in_tempo = telefono.runtime().expect("acceso").block_on(async move {
            tokio::time::timeout(Duration::from_secs(5), chiusa.closed())
                .await
                .is_ok()
        });
        assert!(chiusa_in_tempo, "ogni connessione della libreria si chiude");
    }
    let letto = di_la.read(&mut letti);
    assert!(
        letto.is_err() || letto.is_ok_and(|n| n == 0),
        "il flusso di «Ascolta insieme» si chiude"
    );
    di_qua.set_read_timeout(Some(Duration::from_secs(5)));
    let dall_altra = di_qua.read(&mut letti);
    assert!(
        dall_altra.is_err() || dall_altra.is_ok_and(|n| n == 0),
        "e lo sente anche chi l'aveva aperto"
    );
}

#[test]
fn le_domande_servite_insieme_si_contano_per_dispositivo_non_per_connessione() {
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    del_pc.ammetti(&telefono.chiave(), true);
    del_pc.ferma.store(true, Ordering::SeqCst);
    let _libera = Libera(&del_pc.ferma);

    let connessioni: Vec<Connection> = (0..3)
        .map(|_| connessione_grezza(&telefono, &pc, ALPN_LONTANO))
        .collect();
    let runtime = telefono.runtime().expect("acceso");
    let mut domande = Vec::new();
    for conn in &connessioni {
        for _ in 0..DOMANDE_INSIEME {
            let conn = conn.clone();
            domande.push(runtime.spawn(async move {
                tokio::time::timeout(Duration::from_secs(20), chiedi_su(&conn, una_domanda())).await
            }));
        }
    }
    // Tutte le domande arrivano: quelle che il PC serve aspettano dentro
    // `rispondi`, le altre un posto.
    assert!(aspetta(
        || del_pc.massimo.load(Ordering::SeqCst) >= DOMANDE_INSIEME
    ));
    std::thread::sleep(Duration::from_millis(700));
    let massimo = del_pc.massimo.load(Ordering::SeqCst);
    del_pc.ferma.store(false, Ordering::SeqCst);
    for domanda in domande {
        let esito = runtime.block_on(domanda).expect("il compito finisce");
        assert!(matches!(esito, Ok(Ok(Risposta::Fatto))), "{esito:?}");
    }
    assert_eq!(
        massimo, DOMANDE_INSIEME,
        "al più {DOMANDE_INSIEME} per dispositivo, su quante connessioni voglia"
    );
}
