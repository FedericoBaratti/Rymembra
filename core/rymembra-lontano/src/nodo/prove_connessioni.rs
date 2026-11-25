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
    /// Gli eventi che il nodo ha dato all'app, per genere.
    presenti: AtomicUsize,
    diari: AtomicUsize,
    /// `rispondi` risponde a `Leggi` con una risposta più grande di qualunque
    /// finestra di flusso: chi non legge la lascia a metà.
    risposte_enormi: AtomicBool,
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

    fn rispondi(&self, _da: &str, domanda: Domanda) -> Result<Risposta, AppError> {
        let adesso = self.in_corso.fetch_add(1, Ordering::SeqCst) + 1;
        self.massimo.fetch_max(adesso, Ordering::SeqCst);
        let fine = Instant::now() + Duration::from_secs(10);
        while self.ferma.load(Ordering::SeqCst) && Instant::now() < fine {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.in_corso.fetch_sub(1, Ordering::SeqCst);
        if self.risposte_enormi.load(Ordering::SeqCst) && matches!(domanda, Domanda::Leggi { .. }) {
            return Ok(Risposta::Byte {
                byte: vec![0; 12 * 1024 * 1024],
                totale: 12 * 1024 * 1024,
                versione: "v1".to_owned(),
            });
        }
        Ok(Risposta::Fatto)
    }

    fn dimenticato(&self, _da: &str) {}

    fn insieme(&self, _da: &str, flusso: Flusso) {
        if let Ok(mut flussi) = self.flussi.lock() {
            flussi.push(flusso);
        }
    }

    fn evento(&self, evento: Evento) {
        match evento {
            Evento::Presente { .. } => self.presenti.fetch_add(1, Ordering::SeqCst),
            Evento::Diario(_) => self.diari.fetch_add(1, Ordering::SeqCst),
            _ => 0,
        };
    }
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

/// Prova ad aprire una connessione verso `a`, per chiave e da questi
/// indirizzi, entro `entro`: `true` se si apre.
fn si_apre(
    da: &Nodo,
    a: EndpointId,
    indirizzi: &[SocketAddr],
    alpn: &[u8],
    entro: Duration,
) -> bool {
    let mut indirizzo = EndpointAddr::new(a);
    for diretto in indirizzi {
        indirizzo = indirizzo.with_ip_addr(*diretto);
    }
    let endpoint = da.endpoint.clone();
    let alpn = alpn.to_vec();
    da.runtime().expect("acceso").block_on(async move {
        match tokio::time::timeout(entro, endpoint.connect(indirizzo, &alpn)).await {
            Ok(Ok(conn)) => {
                // Il cancello di una porta aperta chiude subito: conta solo se
                // resta in piedi un momento.
                let chiusa = tokio::time::timeout(Duration::from_millis(300), conn.closed()).await;
                chiusa.is_err()
            }
            _ => false,
        }
    })
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
fn chiuso_mentre_altri_lo_tengono_non_accoglie_piu_nessuno() {
    // Reperto 02-7: «Raggiungibile da internet» spento, il nodo si fermava
    // solo all'ultimo `Arc`. Con `chiudi` la porta si chiude subito, anche con
    // un comando che lo tiene ancora.
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    del_pc.ammetti(&telefono.chiave(), true);
    let id = pc.endpoint.id();
    let indirizzi = pc.socket();
    let pc = Arc::new(pc);
    let tenuto = Arc::clone(&pc);

    // Acceso: la libreria e l'associazione rispondono.
    let aperta = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
    chiedi_sulla(&telefono, &aperta, una_domanda()).expect("acceso: risponde");
    pc.invita().expect("un QR");
    let entro = Duration::from_secs(3);
    assert!(si_apre(&telefono, id, &indirizzi, ALPN_LONTANO, entro));
    assert!(si_apre(&telefono, id, &indirizzi, ALPN_ASSOCIA, entro));

    pc.chiudi();
    assert!(!tenuto.invito_attivo(), "il QR non vale più");
    // La connessione già aperta si chiude.
    let chiusa = aperta.clone();
    let chiusa_in_tempo = telefono.runtime().expect("acceso").block_on(async move {
        tokio::time::timeout(Duration::from_secs(5), chiusa.closed())
            .await
            .is_ok()
    });
    assert!(chiusa_in_tempo, "la connessione aperta si chiude");
    // E nessuna nuova si apre, su nessuna porta.
    for alpn in [ALPN_LONTANO, ALPN_ASSOCIA, ALPN_INSIEME] {
        assert!(
            !si_apre(&telefono, id, &indirizzi, alpn, entro),
            "{}",
            String::from_utf8_lossy(alpn)
        );
    }
    // Chi lo tiene ancora lo trova spento.
    assert_eq!(
        codice(tenuto.chiedi(&telefono.chiave(), una_domanda(), entro)),
        ErrorCodeKind::LontanoSpento
    );
    assert_eq!(
        tenuto.invita().expect_err("spento").code().kind(),
        ErrorCodeKind::LontanoSpento
    );
    // Una seconda volta non fa niente; l'ultimo `Arc` spegne il runtime.
    tenuto.chiudi();
    drop(pc);
    drop(tenuto);
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
fn un_dispositivo_tiene_al_piu_n_connessioni() {
    // Reperto R01-4: un dispositivo collegato poteva aprire decine di
    // migliaia di connessioni, ognuna viva col keepalive, col suo stato QUIC,
    // un compito `servi` e una voce in `aperte`.
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    del_pc.ammetti(&telefono.chiave(), true);
    let connessioni: Vec<Connection> = (0..20)
        .map(|_| {
            let conn = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
            // Servita: il PC l'ha accolta e tenuta prima della prossima.
            chiedi_sulla(&telefono, &conn, una_domanda()).expect("risponde");
            conn
        })
        .collect();
    let vive = || {
        connessioni
            .iter()
            .filter(|c| c.close_reason().is_none())
            .count()
    };
    assert!(
        aspetta(|| vive() <= MASSIME_CONNESSIONI),
        "{} connessioni vive",
        vive()
    );
    // Le più vecchie si chiudono senza dire «non associato»: lo è ancora.
    let prima = connessioni.first().expect("venti");
    assert!(
        chiusa_perche(prima).is_none(),
        "{:?}",
        chiusa_perche(prima).map(|err| err.code().kind())
    );
    // Le più nuove restano, e servono.
    let ultima = connessioni.last().expect("venti");
    assert_eq!(
        chiedi_sulla(&telefono, ultima, una_domanda()).expect("la più nuova risponde"),
        Risposta::Fatto
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

#[test]
fn un_pari_non_sveglia_il_filo_a_raffica() {
    // Reperto S1-5: ogni annuncio era un evento per il filo del collegamento, che
    // ricompone il pannello e lo manda alla finestra. Un dispositivo associato
    // ne mandava a migliaia, ognuno con un nome diverso.
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    del_pc.ammetti(&telefono.chiave(), true);
    let conn = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
    let mut rifiutati = 0;
    for n in 0..300 {
        let annuncio = Domanda::Annuncio(Presenza {
            nome: format!("PC {n}"),
            ..Presenza::default()
        });
        if let Err(err) = chiedi_sulla(&telefono, &conn, annuncio) {
            assert_eq!(err.code().kind(), ErrorCodeKind::LontanoIrraggiungibile);
            rifiutati += 1;
        }
    }
    let arrivati = del_pc.presenti.load(Ordering::SeqCst);
    assert!(
        arrivati <= ANNUNCI_AL_MINUTO,
        "{arrivati} eventi al filo per 300 annunci in pochi secondi"
    );
    assert_eq!(
        arrivati + rifiutati,
        300,
        "gli altri lo sanno: errore detto"
    );
    // La connessione resta, e le altre domande si servono.
    assert_eq!(
        chiedi_sulla(&telefono, &conn, una_domanda()).expect("risponde ancora"),
        Risposta::Fatto
    );
}

/// Una domanda che non si legge: la lunghezza dice cinque byte e i cinque byte
/// non sono JSON. Aspetta che il nodo l'abbia lasciata.
fn una_domanda_sbagliata(da: &Nodo, conn: &Connection) {
    let conn = conn.clone();
    da.runtime().expect("acceso").block_on(async move {
        let (mut invio, mut ricevi) = conn.open_bi().await.expect("un flusso");
        let _ = invio
            .write_all(&[0, 0, 0, 5, b'x', b'x', b'x', b'x', b'x'])
            .await;
        let _ = invio.finish();
        let _ = tokio::time::timeout(Duration::from_secs(5), ricevi.read_to_end(1024)).await;
    });
}

#[test]
fn un_pari_che_sbaglia_le_domande_non_riempie_il_diario_e_si_ignora_per_un_po() {
    // Reperto S1-5: ogni domanda che cadeva era una riga di diario, e ventimila
    // righe fanno ruotare il file: la storia di tutta la sessione, sparita.
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    del_pc.ammetti(&telefono.chiave(), true);
    let conn = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
    for _ in 0..200 {
        una_domanda_sbagliata(&telefono, &conn);
    }
    let righe = del_pc.diari.load(Ordering::SeqCst);
    assert!(
        righe <= RIGHE_DI_DIARIO_AL_MINUTO,
        "{righe} righe di diario per 200 domande sbagliate"
    );
    // Non è scollegato: la connessione è in piedi...
    assert!(conn.close_reason().is_none(), "il nodo non lo scollega");
    // ...ma per un po' non si ascolta: nemmeno una domanda giusta ha risposta,
    // e non perché sia lenta.
    let esito = chiedi_sulla(&telefono, &conn, una_domanda());
    assert!(
        esito.is_err(),
        "una domanda dopo troppe sbagliate non si serve: {esito:?}"
    );
    assert!(conn.close_reason().is_none(), "e la connessione resta");
}

#[test]
fn le_domande_che_l_altro_annulla_non_sono_domande_sbagliate() {
    // Chi cambia pagina annulla le domande in volo: un dispositivo onesto ne fa
    // decine al minuto, e non per questo si ignora.
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    del_pc.ammetti(&telefono.chiave(), true);
    let conn = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
    let runtime = telefono.runtime().expect("acceso");
    for _ in 0..(SBAGLIATE_AL_MINUTO * 3) {
        let conn = conn.clone();
        runtime.block_on(async move {
            let (mut invio, mut ricevi) = conn.open_bi().await.expect("un flusso");
            let _ = invio.write_all(&[0, 0, 0, 5]).await;
            // Annullata a metà, come la pagina che si chiude.
            let _ = invio.reset(VarInt::from_u32(0));
            let _ = ricevi.stop(VarInt::from_u32(0));
        });
    }
    assert_eq!(
        chiedi_sulla(&telefono, &conn, una_domanda()).expect("risponde"),
        Risposta::Fatto
    );
}

#[test]
fn una_risposta_che_l_altro_non_legge_libera_il_posto() {
    // Reperto S1-7: `scrivi_risposta` non aveva una scadenza. Un dispositivo che
    // chiede otto risposte più grandi di qualunque finestra di flusso e non le
    // legge teneva fermi tutti i suoi posti, e nessun'altra domanda — nemmeno
    // `Presenza` — aveva risposta.
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    del_pc.ammetti(&telefono.chiave(), true);
    del_pc.risposte_enormi.store(true, Ordering::SeqCst);
    let conn = connessione_grezza(&telefono, &pc, ALPN_LONTANO);
    // Gli otto flussi di risposta restano in mano a chi li ha aperti, senza leggerli.
    let non_letti = {
        let conn = conn.clone();
        telefono.runtime().expect("acceso").block_on(async move {
            let mut tenuti = Vec::new();
            for _ in 0..DOMANDE_INSIEME {
                let (mut invio, ricevi) = conn.open_bi().await.expect("un flusso");
                let leggi = cornice(
                    &Domanda::Leggi {
                        brano: 1,
                        da: 0,
                        quanti: 1,
                    },
                    MASSIMA_DOMANDA,
                )
                .expect("si scrive");
                invio.write_all(&leggi).await.expect("scritta");
                let _ = invio.finish();
                tenuti.push(ricevi);
            }
            tenuti
        })
    };
    // La nona domanda ha risposta: i posti si liberano quando la scrittura
    // scade (due secondi nelle prove, trenta nell'applicazione).
    let fine = Instant::now() + Duration::from_secs(9);
    let esito = loop {
        let esito = chiedi_sulla(&telefono, &conn, Domanda::Presenza);
        if esito.is_ok() || Instant::now() > fine {
            break esito;
        }
    };
    assert!(
        matches!(esito, Ok(Risposta::Presenza(_))),
        "la nona domanda non ha risposta: {esito:?}"
    );
    drop(non_letti);
}

#[test]
fn una_connessione_all_associazione_che_non_apre_un_flusso_si_chiude() {
    // Reperto S1-11: con un QR a schermo, ogni connessione su `rymembra/associa/1`
    // che non apriva un flusso teneva un compito per duecento secondi, e il tetto
    // di tentativi è per chiave: chi ne cambia a ogni giro non lo sente.
    let (pc, _) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    pc.invita().expect("un QR");
    let conn = connessione_grezza(&telefono, &pc, ALPN_ASSOCIA);
    let chiusa = {
        let conn = conn.clone();
        telefono.runtime().expect("acceso").block_on(async move {
            tokio::time::timeout(Duration::from_secs(8), conn.closed())
                .await
                .is_ok()
        })
    };
    assert!(chiusa, "la connessione senza una domanda resta aperta");
}
