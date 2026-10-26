//! Due (e tre) Rymembra veri, su `127.0.0.1`, attraverso la sola API pubblica.
//!
//! Mai `0.0.0.0`: su Windows farebbe comparire la richiesta del firewall a chi
//! lancia le prove. Ogni capo lega `127.0.0.1` sulla porta zero, così le prove
//! possono girare in parallelo senza pestarsi le porte.

// Un test dichiara le sue aspettative con `expect` e con `panic!`: i lint del
// workspace li vietano per il codice di produzione, e `clippy.toml` li concede
// ai soli `#[test]` e ai moduli `cfg(test)`, non agli aiutanti di una crate di
// prova a sé. È la stessa eccezione di `rymembra-app/tests`.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use rymembra_insieme::messaggi::{Diritti, Messaggio};
use rymembra_insieme::{
    ChiavePubblica, Evento, FileServito, Gettone, IdConnessione, Identita, Impostazioni, Rete,
    Tempi,
};

/// La base dei tempi di processo, come `rymembra_play::tempo`: una sola per
/// tutti i capi, così gli istanti dell'orologio sono confrontabili.
fn adesso_ns() -> u64 {
    static BASE: OnceLock<Instant> = OnceLock::new();
    u64::try_from(BASE.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Quanto si aspetta un evento che deve arrivare.
const ATTESA: Duration = Duration::from_secs(5);

/// Un Rymembra: la rete e i suoi eventi, con quelli non ancora guardati.
struct Capo {
    rete: Rete,
    eventi: Receiver<Evento>,
    da_parte: RefCell<VecDeque<Evento>>,
}

fn capo(nome: &str) -> Capo {
    capo_con(nome, Tempi::default())
}

fn capo_con(nome: &str, tempi: Tempi) -> Capo {
    capo_di(Identita::nuova().expect("chiave"), nome, tempi)
}

fn capo_di(identita: Identita, nome: &str, tempi: Tempi) -> Capo {
    let (rete, eventi) = Rete::avvia_con(
        Impostazioni {
            identita,
            nome: nome.to_owned(),
            lega: IpAddr::V4(Ipv4Addr::LOCALHOST),
            porta: 0,
            adesso_ns,
        },
        tempi,
    )
    .expect("parte");
    Capo {
        rete,
        eventi,
        da_parte: RefCell::new(VecDeque::new()),
    }
}

impl Capo {
    /// Il primo evento per cui `cerca` dice qualcosa, entro `entro`. Quelli
    /// che non gli interessano restano da parte per la domanda dopo.
    fn prova_ad_aspettare<T>(
        &self,
        entro: Duration,
        mut cerca: impl FnMut(&Evento) -> Option<T>,
    ) -> Option<T> {
        {
            let mut da_parte = self.da_parte.borrow_mut();
            if let Some(posto) = da_parte.iter().position(|e| cerca(e).is_some()) {
                let evento = da_parte.remove(posto)?;
                return cerca(&evento);
            }
        }
        let fine = Instant::now() + entro;
        loop {
            let resta = fine.saturating_duration_since(Instant::now());
            let evento = self.eventi.recv_timeout(resta).ok()?;
            if let Some(trovato) = cerca(&evento) {
                return Some(trovato);
            }
            self.da_parte.borrow_mut().push_back(evento);
        }
    }

    fn aspetta<T>(&self, cosa: &str, cerca: impl FnMut(&Evento) -> Option<T>) -> T {
        self.prova_ad_aspettare(ATTESA, cerca)
            .unwrap_or_else(|| panic!("non è arrivato: {cosa}"))
    }

    /// Ospita e restituisce l'indirizzo a cui collegarsi.
    fn ospita(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.rete.ospita().expect("ospita")))
    }

    fn chiave(&self) -> ChiavePubblica {
        self.rete.pubblica()
    }

    /// Si fida di questi, e di nessun altro.
    fn fidati(&self, di: &[&Capo]) {
        self.rete
            .pari(di.iter().map(|c| c.chiave()).collect(), Vec::new());
    }

    /// Lascia leggere i suoi gettoni a questi: i membri della sessione.
    fn ammetti(&self, chi: &[&Capo]) {
        self.rete.ammessi(chi.iter().map(|c| c.chiave()).collect());
    }

    /// Collega e aspetta l'apertura dai due lati. Restituisce gli id.
    fn collega_a(&self, host: &Capo, indirizzo: SocketAddr) -> (IdConnessione, IdConnessione) {
        let id = self.rete.collega(indirizzo);
        self.aspetta("aperta, dal membro", |e| match e {
            Evento::Aperta { id: aperta, .. } if *aperta == id => Some(()),
            _ => None,
        });
        let mia = self.chiave();
        let dall_host = host.aspetta("aperta, dall'host", |e| match e {
            Evento::Aperta { id, pari, .. } if *pari == mia => Some(*id),
            _ => None,
        });
        (id, dall_host)
    }

    fn chiusa(&self, id: IdConnessione) -> Option<rymembra_domain::errors::ErrorCodeKind> {
        self.aspetta("chiusa", |e| match e {
            Evento::Chiusa { id: chiusa, codice } if *chiusa == id => Some(*codice),
            _ => None,
        })
    }
}

impl Drop for Capo {
    fn drop(&mut self) {
        self.rete.ferma();
    }
}

fn codice_di(err: &rymembra_domain::errors::AppError) -> &'static str {
    err.code().kind().code()
}

/// Una cartella temporanea che si cancella da sola.
struct Cartella(PathBuf);

impl Cartella {
    fn nuova(nome: &str) -> Self {
        static PROSSIMA: AtomicUsize = AtomicUsize::new(0);
        let percorso = std::env::temp_dir().join(format!(
            "rymembra-insieme-{nome}-{}-{}",
            std::process::id(),
            PROSSIMA.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&percorso).expect("cartella");
        Self(percorso)
    }

    fn file(&self, nome: &str, byte: &[u8]) -> FileServito {
        let percorso = self.0.join(nome);
        std::fs::write(&percorso, byte).expect("scrittura");
        FileServito::da_percorso(&percorso).expect("metadati")
    }
}

impl Drop for Cartella {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn byte_di_prova(quanti: usize) -> Vec<u8> {
    (0..quanti)
        .map(|i| u8::try_from(i % 251).unwrap_or(0))
        .collect()
}

// ── stretta e saluto ───────────────────────────────────────────────────────

#[test]
fn la_stretta_e_il_saluto_aprono_la_connessione_sui_due_capi() {
    let host = capo("PC di Anna");
    let membro = capo("PC di Bruno");
    let indirizzo = host.ospita();
    let id = membro.rete.collega(indirizzo);
    let chiave_host = host.chiave();
    let chiave_membro = membro.chiave();
    membro.aspetta("aperta, dal membro", |e| match e {
        Evento::Aperta {
            id: aperta,
            pari,
            nome,
            ospita,
            in_entrata,
            indirizzo: dove,
        } if *aperta == id => {
            assert_eq!(*pari, chiave_host);
            assert_eq!(nome, "PC di Anna");
            assert!(*ospita, "l'host dichiara di ospitare");
            assert!(!*in_entrata);
            assert_eq!(*dove, indirizzo);
            Some(())
        }
        _ => None,
    });
    host.aspetta("aperta, dall'host", |e| match e {
        Evento::Aperta {
            pari,
            nome,
            ospita,
            in_entrata,
            ..
        } => {
            assert_eq!(*pari, chiave_membro);
            assert_eq!(nome, "PC di Bruno");
            assert!(!*ospita);
            assert!(*in_entrata);
            Some(())
        }
        _ => None,
    });
}

#[test]
fn un_host_che_non_c_e_da_una_chiusa_irraggiungibile() {
    let membro = capo("membro");
    // Una porta libera adesso, e chiusa subito dopo: nessuno ascolta.
    let porta = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|a| a.local_addr())
        .expect("porta")
        .port();
    let id = membro
        .rete
        .collega(SocketAddr::from((Ipv4Addr::LOCALHOST, porta)));
    assert_eq!(
        membro.chiusa(id).map(|k| k.code()),
        Some("insieme.irraggiungibile")
    );
}

// ── fiducia ────────────────────────────────────────────────────────────────

#[test]
fn un_messaggio_da_una_chiave_non_fidata_si_scarta_e_la_connessione_si_chiude() {
    let host = capo("host");
    let membro = capo("membro");
    // Il membro si fida dell'host, l'host del membro no.
    membro.fidati(&[&host]);
    let indirizzo = host.ospita();
    let (id, dall_host) = membro.collega_a(&host, indirizzo);
    membro.rete.manda(id, &Messaggio::Chiedi).expect("parte");
    assert_eq!(
        host.chiusa(dall_host).map(|k| k.code()),
        Some("insieme.nonAssociato")
    );
    assert_eq!(
        membro.chiusa(id).map(|k| k.code()),
        Some("insieme.nonAssociato"),
        "il perché arriva anche a chi l'ha mandato"
    );
    assert!(
        host.prova_ad_aspettare(Duration::from_millis(200), |e| match e {
            Evento::Ricevuto { .. } => Some(()),
            _ => None,
        })
        .is_none(),
        "il messaggio non arriva al desktop"
    );
}

#[test]
fn fra_fidati_i_messaggi_passano_nei_due_sensi_e_quelli_fuori_dai_tetti_non_partono() {
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    membro.fidati(&[&host]);
    let indirizzo = host.ospita();
    let (id, dall_host) = membro.collega_a(&host, indirizzo);

    membro.rete.manda(id, &Messaggio::Chiedi).expect("parte");
    host.aspetta("chiedi", |e| match e {
        Evento::Ricevuto {
            id,
            messaggio: Messaggio::Chiedi,
        } if *id == dall_host => Some(()),
        _ => None,
    });
    host.rete
        .manda(
            dall_host,
            &Messaggio::Ammesso {
                membro: 3,
                diritti: Diritti::Ascolta,
            },
        )
        .expect("parte");
    membro.aspetta("ammesso", |e| match e {
        Evento::Ricevuto {
            messaggio:
                Messaggio::Ammesso {
                    membro: 3,
                    diritti: Diritti::Ascolta,
                },
            ..
        } => Some(()),
        _ => None,
    });

    let lungo = Messaggio::Rifiutato {
        codice: "x".repeat(65),
    };
    let err = membro.rete.manda(id, &lungo).expect_err("fuori dai tetti");
    assert_eq!(codice_di(&err), "insieme.protocollo");
    let err = membro
        .rete
        .manda(IdConnessione(999_999), &Messaggio::Chiedi)
        .expect_err("non c'è");
    assert_eq!(codice_di(&err), "insieme.connessioneCaduta");
}

#[test]
fn una_chiave_bloccata_mentre_e_collegata_viene_chiusa() {
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    let indirizzo = host.ospita();
    let (id, dall_host) = membro.collega_a(&host, indirizzo);
    host.rete.pari(Vec::new(), vec![membro.chiave()]);
    assert_eq!(
        host.chiusa(dall_host).map(|k| k.code()),
        Some("insieme.nonAssociato")
    );
    assert_eq!(
        membro.chiusa(id).map(|k| k.code()),
        Some("insieme.nonAssociato"),
        "chi è bloccato non sa di esserlo: per lui è come essere dimenticato"
    );
    // E una connessione nuova si chiude subito, senza `Aperta` sull'host.
    let di_nuovo = membro.rete.collega(indirizzo);
    assert!(membro.chiusa(di_nuovo).is_some());
}

// ── associazione ───────────────────────────────────────────────────────────

/// Collega e associa fino al codice; restituisce gli id e i due codici.
fn fino_al_codice(
    host: &Capo,
    membro: &Capo,
    indirizzo: SocketAddr,
) -> (IdConnessione, IdConnessione, String, String) {
    let (id, dall_host) = membro.collega_a(host, indirizzo);
    membro.rete.associa(id).expect("associa");
    let dal_membro = membro.aspetta("codice, dal membro", |e| match e {
        Evento::Codice { id: suo, codice } if *suo == id => Some(codice.clone()),
        _ => None,
    });
    let sull_host = host.aspetta("codice, dall'host", |e| match e {
        Evento::Codice { id, codice } if *id == dall_host => Some(codice.clone()),
        _ => None,
    });
    (id, dall_host, dal_membro, sull_host)
}

#[test]
fn l_associazione_da_lo_stesso_codice_sui_due_capi_e_poi_la_fiducia() {
    let host = capo("host");
    let membro = capo("membro");
    let indirizzo = host.ospita();
    let (id, dall_host, dal_membro, sull_host) = fino_al_codice(&host, &membro, indirizzo);
    assert_eq!(dal_membro, sull_host, "lo stesso codice sui due schermi");
    assert_eq!(dal_membro.len(), 4);
    assert!(dal_membro.chars().all(|c| c.is_ascii_digit()));

    membro.rete.conferma_codice(id, true);
    host.rete.conferma_codice(dall_host, true);
    let chiave_host = host.chiave();
    let chiave_membro = membro.chiave();
    membro.aspetta("associato, dal membro", |e| match e {
        Evento::Associato { id: suo, pari } if *suo == id => {
            assert_eq!(*pari, chiave_host);
            Some(())
        }
        _ => None,
    });
    host.aspetta("associato, dall'host", |e| match e {
        Evento::Associato { pari, .. } => {
            assert_eq!(*pari, chiave_membro);
            Some(())
        }
        _ => None,
    });
    // Fidati da subito, senza aspettare `pari`: un messaggio passa.
    membro.rete.manda(id, &Messaggio::Chiedi).expect("parte");
    host.aspetta("chiedi dopo l'associazione", |e| match e {
        Evento::Ricevuto {
            messaggio: Messaggio::Chiedi,
            ..
        } => Some(()),
        _ => None,
    });
}

#[test]
fn un_no_al_codice_chiude_con_codice_rifiutato_sui_due_capi() {
    let host = capo("host");
    let membro = capo("membro");
    let indirizzo = host.ospita();
    let (id, dall_host, _, _) = fino_al_codice(&host, &membro, indirizzo);
    host.rete.conferma_codice(dall_host, false);
    assert_eq!(
        host.chiusa(dall_host).map(|k| k.code()),
        Some("insieme.codiceRifiutato")
    );
    assert_eq!(
        membro.chiusa(id).map(|k| k.code()),
        Some("insieme.codiceRifiutato")
    );
}

#[test]
fn tre_associazioni_fallite_e_la_quarta_e_troppi_tentativi() {
    let host = capo("host");
    let segreta = Identita::nuova().expect("chiave");
    let testo = segreta.in_testo();
    let membro = capo_di(segreta, "membro", Tempi::default());
    let indirizzo = host.ospita();
    for _ in 0..3 {
        let (id, dall_host, _, _) = fino_al_codice(&host, &membro, indirizzo);
        membro.rete.conferma_codice(id, false);
        assert_eq!(
            membro.chiusa(id).map(|k| k.code()),
            Some("insieme.codiceRifiutato")
        );
        // Anche sull'host: il fallimento si conta quando la connessione
        // finisce, e il quarto tentativo deve trovarlo contato.
        assert_eq!(
            host.chiusa(dall_host).map(|k| k.code()),
            Some("insieme.codiceRifiutato")
        );
    }
    // Il quarto lo ferma chi chiede…
    let (id, _) = membro.collega_a(&host, indirizzo);
    let err = membro.rete.associa(id).expect_err("troppi");
    assert_eq!(codice_di(&err), "insieme.troppiTentativi");
    assert!(
        matches!(
            err.code(),
            rymembra_domain::errors::ErrorCode::InsiemeTroppiTentativi {
                riprova_fra_ms: Some(ms)
            } if *ms > 0
        ),
        "dice fra quanto si può riprovare"
    );
    // …e anche chi risponde, se chi chiede non tiene il conto: la stessa
    // chiave da un'altra rete, che il conto non ce l'ha.
    let gemello = capo_di(
        Identita::da_testo(&testo).expect("la stessa chiave"),
        "gemello",
        Tempi::default(),
    );
    let (id, _) = gemello.collega_a(&host, indirizzo);
    gemello.rete.associa(id).expect("da qui si parte");
    assert_eq!(
        gemello.chiusa(id).map(|k| k.code()),
        Some("insieme.troppiTentativi")
    );
}

#[test]
fn un_host_occupato_rimanda_senza_chiudere_e_senza_contare_il_tentativo() {
    // Prima l'host occupato chiudeva con `troppiTentativi`, e chi chiedeva lo
    // contava come un tentativo fallito: tre di fila, e restava fermo dieci
    // minuti per colpa di un'associazione altrui.
    let host = capo("host");
    let anna = capo("anna");
    let bruno = capo("bruno");
    let indirizzo = host.ospita();
    // Anna è al codice: l'host è occupato con lei.
    let (id_anna, dall_host_anna, _, _) = fino_al_codice(&host, &anna, indirizzo);
    let (id_bruno, _) = bruno.collega_a(&host, indirizzo);
    // Più volte dei tre tentativi per chiave.
    for _ in 0..4 {
        bruno.rete.associa(id_bruno).expect("da chi chiede parte");
        let esito = bruno.aspetta("rimandata", |e| match e {
            Evento::AssociazioneRimandata { id, codice } if *id == id_bruno => Some(Ok(*codice)),
            Evento::Chiusa { id, codice } if *id == id_bruno => Some(Err(*codice)),
            _ => None,
        });
        assert_eq!(
            esito.map(|k| k.code()),
            Ok("insieme.associazioneOccupata"),
            "rimandata, con la connessione ancora aperta"
        );
    }
    // Anna finisce; adesso tocca a Bruno, che non è fermo.
    anna.rete.conferma_codice(id_anna, true);
    host.rete.conferma_codice(dall_host_anna, true);
    host.aspetta("associato, Anna", |e| match e {
        Evento::Associato { id, .. } if *id == dall_host_anna => Some(()),
        _ => None,
    });
    bruno
        .rete
        .associa(id_bruno)
        .expect("nessun tentativo contato");
    bruno.aspetta("codice, Bruno", |e| match e {
        Evento::Codice { id, .. } if *id == id_bruno => Some(()),
        _ => None,
    });
}

#[test]
fn un_codice_non_confermato_in_tempo_scade_con_associazione_scaduta_sui_due_capi() {
    // Prima si chiudeva con `codiceRifiutato`: a due persone che si erano
    // solo distratte diceva che i codici non coincidevano.
    let tempi = Tempi {
        battito: Duration::from_millis(50),
        associazione: Duration::from_millis(400),
        ..Tempi::default()
    };
    let host = capo_con("host", tempi);
    let membro = capo_con("membro", tempi);
    let indirizzo = host.ospita();
    let (id, dall_host, _, _) = fino_al_codice(&host, &membro, indirizzo);
    // Nessuno risponde al codice.
    assert_eq!(
        membro.chiusa(id).map(|k| k.code()),
        Some("insieme.associazioneScaduta")
    );
    assert_eq!(
        host.chiusa(dall_host).map(|k| k.code()),
        Some("insieme.associazioneScaduta")
    );
}

#[test]
fn un_estraneo_si_chiude_con_estraneo_scaduto_ma_non_mentre_si_associa() {
    let tempi = Tempi {
        battito: Duration::from_millis(50),
        vita_estraneo: Duration::from_millis(500),
        ..Tempi::default()
    };
    let host = capo_con("host", tempi);
    let indirizzo = host.ospita();

    // Un estraneo che guarda e basta: il perché arriva ai due capi.
    let curioso = capo_con("curioso", tempi);
    let (id, dall_host) = curioso.collega_a(&host, indirizzo);
    assert_eq!(
        curioso.chiusa(id).map(|k| k.code()),
        Some("insieme.estraneoScaduto")
    );
    assert_eq!(
        host.chiusa(dall_host).map(|k| k.code()),
        Some("insieme.estraneoScaduto")
    );

    // Uno che si sta associando resta oltre la vita di un estraneo, col
    // codice sugli schermi, e il sì dei due arriva a buon fine.
    let membro = capo_con("membro", tempi);
    let (id, dall_host, _, _) = fino_al_codice(&host, &membro, indirizzo);
    assert!(
        membro
            .prova_ad_aspettare(Duration::from_millis(1_200), |e| match e {
                Evento::Chiusa { id: chiusa, codice } if *chiusa == id => Some(*codice),
                _ => None,
            })
            .is_none(),
        "il codice è sugli schermi: la connessione resta"
    );
    membro.rete.conferma_codice(id, true);
    host.rete.conferma_codice(dall_host, true);
    membro.aspetta("associato, dal membro", |e| match e {
        Evento::Associato { id: suo, .. } if *suo == id => Some(()),
        _ => None,
    });
    host.aspetta("associato, dall'host", |e| match e {
        Evento::Associato { id: suo, .. } if *suo == dall_host => Some(()),
        _ => None,
    });
    // E da fidato non scade più.
    assert!(
        membro
            .prova_ad_aspettare(Duration::from_millis(600), |e| match e {
                Evento::Chiusa { id: chiusa, codice } if *chiusa == id => Some(*codice),
                _ => None,
            })
            .is_none(),
        "associato, resta"
    );
}

#[test]
fn con_la_propria_chiave_non_ci_si_associa() {
    let segreta = Identita::nuova().expect("chiave");
    let testo = segreta.in_testo();
    let host = capo_di(segreta, "host", Tempi::default());
    let gemello = capo_di(
        Identita::da_testo(&testo).expect("la stessa chiave"),
        "gemello",
        Tempi::default(),
    );
    let indirizzo = host.ospita();
    let (id, _) = gemello.collega_a(&host, indirizzo);
    let err = gemello.rete.associa(id).expect_err("stessa identità");
    assert_eq!(codice_di(&err), "insieme.codiceRifiutato");
}

// ── scoperta ───────────────────────────────────────────────────────────────

#[test]
fn la_scoperta_a_mano_trova_l_host_su_127_0_0_1() {
    let host = capo("host");
    let membro = capo("membro");
    let indirizzo = host.ospita();
    membro.rete.cerca(&[indirizzo]);
    let impronta = host.chiave().impronta();
    let trovato = membro.aspetta("trovato", |e| match e {
        Evento::Trovato {
            indirizzo,
            impronta: sua,
        } if *sua == impronta => Some(*indirizzo),
        _ => None,
    });
    assert_eq!(trovato, indirizzo);
}

#[test]
fn chi_non_ospita_non_risponde_alla_scoperta() {
    let host = capo("host");
    let membro = capo("membro");
    let indirizzo = host.ospita();
    host.rete.smetti_di_ospitare();
    // Il filo della scoperta esce al suo giro: si lascia il tempo di farlo.
    std::thread::sleep(Duration::from_millis(200));
    membro.rete.cerca(&[indirizzo]);
    assert!(
        membro
            .prova_ad_aspettare(Duration::from_millis(500), |e| match e {
                Evento::Trovato { .. } => Some(()),
                _ => None,
            })
            .is_none()
    );
}

// ── letture ────────────────────────────────────────────────────────────────

#[test]
fn le_letture_danno_l_intervallo_esatto_la_fine_e_i_rifiuti_giusti() {
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    host.ammetti(&[&membro]);
    let indirizzo = host.ospita();
    let cartella = Cartella::nuova("letture");
    let contenuto = byte_di_prova(300_000);
    let file = cartella.file("brano.flac", &contenuto);
    let gettone = Gettone::nuovo().expect("gettone");
    host.rete.servi(gettone, file.clone());
    let lettore = membro.rete.lettore(indirizzo, host.chiave());

    let (byte, totale) = lettore
        .pezzo(&gettone, 1_000, 5_000, None)
        .expect("si legge");
    assert_eq!(totale, 300_000);
    assert_eq!(byte, contenuto.get(1_000..6_000).expect("dentro"));

    let (byte, _) = lettore
        .pezzo(&gettone, 299_000, 5_000, None)
        .expect("si legge");
    assert_eq!(
        byte,
        contenuto.get(299_000..).expect("dentro"),
        "meno alla fine"
    );
    let (byte, totale) = lettore
        .pezzo(&gettone, 400_000, 10, None)
        .expect("si legge");
    assert!(byte.is_empty());
    assert_eq!(totale, 300_000);

    let (byte, _) = lettore.pezzo(&gettone, 0, 300_000, None).expect("si legge");
    assert!(byte == contenuto, "tutto il file, in una richiesta");

    // Il file riscritto: stessa misura, altra data.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&file.percorso, byte_di_prova(300_000)).expect("riscrittura");
    let err = lettore.pezzo(&gettone, 0, 10, None).expect_err("cambiato");
    assert_eq!(codice_di(&err), "insieme.fileCambiato");

    host.rete.revoca(&gettone);
    let err = lettore.pezzo(&gettone, 0, 10, None).expect_err("revocato");
    assert_eq!(codice_di(&err), "insieme.gettoneScaduto");
}

#[test]
fn una_lettura_oltre_i_due_mega_si_fa_a_finestre() {
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    host.ammetti(&[&membro]);
    let indirizzo = host.ospita();
    let cartella = Cartella::nuova("finestre");
    let contenuto = byte_di_prova(5 * 1024 * 1024 + 123);
    let gettone = Gettone::nuovo().expect("gettone");
    host.rete
        .servi(gettone, cartella.file("lungo.flac", &contenuto));
    let lettore = membro.rete.lettore(indirizzo, host.chiave());
    let quanti = u64::try_from(contenuto.len()).expect("sta");
    let (byte, totale) = lettore
        .pezzo(&gettone, 0, quanti + 10, None)
        .expect("si legge");
    assert_eq!(totale, quanti);
    assert!(
        byte == contenuto,
        "cinque mega e un po', a finestre, uguali"
    );
}

#[test]
fn le_copertine_arrivano_intere() {
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    host.ammetti(&[&membro]);
    let indirizzo = host.ospita();
    let copertina: Arc<[u8]> = Arc::from(byte_di_prova(200_000));
    let gettone = Gettone::nuovo().expect("gettone");
    host.rete.servi_copertina(gettone, Arc::clone(&copertina));
    let lettore = membro.rete.lettore(indirizzo, host.chiave());
    assert_eq!(lettore.copertina(&gettone).expect("arriva"), &*copertina);
}

#[test]
fn chi_non_e_fidato_o_e_bloccato_non_legge() {
    let host = capo("host");
    let estraneo = capo("estraneo");
    let bloccato = capo("bloccato");
    host.rete.pari(Vec::new(), vec![bloccato.chiave()]);
    let indirizzo = host.ospita();
    let cartella = Cartella::nuova("rifiuti");
    let gettone = Gettone::nuovo().expect("gettone");
    host.rete
        .servi(gettone, cartella.file("a.flac", &byte_di_prova(100)));

    let err = estraneo
        .rete
        .lettore(indirizzo, host.chiave())
        .pezzo(&gettone, 0, 10, None)
        .expect_err("non fidato");
    assert_eq!(codice_di(&err), "insieme.nonAssociato");

    let err = bloccato
        .rete
        .lettore(indirizzo, host.chiave())
        .pezzo(&gettone, 0, 10, None)
        .expect_err("bloccato");
    // L'host chiude subito dopo la stretta, senza dire perché: per chi è
    // bloccato l'host semplicemente non risponde.
    assert_eq!(codice_di(&err), "insieme.irraggiungibile");

    // E una chiave sbagliata dall'altra parte: non è l'host che si cercava.
    let err = estraneo
        .rete
        .lettore(indirizzo, estraneo.chiave())
        .pezzo(&gettone, 0, 10, None)
        .expect_err("chiave sbagliata");
    assert_eq!(codice_di(&err), "insieme.irraggiungibile");
}

#[test]
fn una_lettura_gia_annullata_torna_subito() {
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    host.ammetti(&[&membro]);
    let indirizzo = host.ospita();
    let lettore = membro.rete.lettore(indirizzo, host.chiave());
    let annullo = AtomicBool::new(true);
    let inizio = Instant::now();
    let err = lettore
        .pezzo(&Gettone([0; 16]), 0, 10, Some(&annullo))
        .expect_err("annullata");
    assert_eq!(codice_di(&err), "internal.aborted");
    assert!(inizio.elapsed() < Duration::from_millis(100));
}

#[test]
fn si_misura_quanto_costa_una_lettura_su_127_0_0_1() {
    // Non una soglia: un numero per il rapporto. La prima lettura paga la
    // connessione e la stretta; le altre no.
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    host.ammetti(&[&membro]);
    let indirizzo = host.ospita();
    let cartella = Cartella::nuova("misura");
    let contenuto = byte_di_prova(8 * 1024 * 1024);
    let gettone = Gettone::nuovo().expect("gettone");
    host.rete
        .servi(gettone, cartella.file("misura.flac", &contenuto));
    let lettore = membro.rete.lettore(indirizzo, host.chiave());

    let inizio = Instant::now();
    lettore.pezzo(&gettone, 0, 256 * 1024, None).expect("prima");
    let prima = inizio.elapsed();
    let inizio = Instant::now();
    lettore
        .pezzo(&gettone, 256 * 1024, 256 * 1024, None)
        .expect("seconda");
    let seconda = inizio.elapsed();
    let inizio = Instant::now();
    lettore
        .pezzo(&gettone, 1024 * 1024, 2 * 1024 * 1024, None)
        .expect("due mega");
    let due_mega = inizio.elapsed();
    eprintln!(
        "misura su 127.0.0.1 (build di prova): prima lettura 256 KiB con stretta {prima:?}, \
         seconda 256 KiB {seconda:?}, 2 MiB {due_mega:?}"
    );
}

// ── compressione ───────────────────────────────────────────────────────────

/// Una staffetta su `127.0.0.1` fra chi si collega e `verso`: copia i byte
/// nei due sensi e conta quelli che vanno da `verso` a chi si è collegato.
fn staffetta(verso: SocketAddr) -> (SocketAddr, Arc<AtomicUsize>) {
    use std::io::{Read as _, Write as _};
    use std::net::{Shutdown, TcpListener, TcpStream};
    let ascolto = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("ascolto");
    let indirizzo = ascolto.local_addr().expect("indirizzo");
    let contati = Arc::new(AtomicUsize::new(0));
    let per_il_filo = Arc::clone(&contati);
    let copia = |mut da: TcpStream, mut a: TcpStream, conto: Option<Arc<AtomicUsize>>| {
        std::thread::spawn(move || {
            let mut buffer = vec![0_u8; 64 * 1024];
            while let Ok(letti) = da.read(&mut buffer) {
                if letti == 0 {
                    break;
                }
                if let Some(conto) = &conto {
                    conto.fetch_add(letti, Ordering::SeqCst);
                }
                if a.write_all(&buffer[..letti]).is_err() {
                    break;
                }
            }
            let _ = a.shutdown(Shutdown::Write);
        });
    };
    std::thread::spawn(move || {
        let (dentro, _) = ascolto.accept().expect("accetta");
        let fuori = TcpStream::connect(verso).expect("verso l'host");
        copia(
            dentro.try_clone().expect("copia"),
            fuori.try_clone().expect("copia"),
            None,
        );
        copia(fuori, dentro, Some(per_il_filo));
    });
    (indirizzo, contati)
}

/// Uno stato di sessione di `voci` voci, coi campi che non si comprimono
/// (gettoni, impronte) a caso.
fn stato_grande(voci: usize) -> Messaggio {
    use rymembra_insieme::messaggi::{
        DescrittoreCopia, Persona, Ripeti, SorgenteSulFilo, StatoSessione, VoceSulFilo,
    };
    let mut seme = 0x9e37_79b9_7f4a_7c15_u64;
    let mut a_caso = move || {
        seme ^= seme << 13;
        seme ^= seme >> 7;
        seme ^= seme << 17;
        seme
    };
    let mut gettone = || {
        let mut byte = [0_u8; 16];
        byte[..8].copy_from_slice(&a_caso().to_le_bytes());
        byte[8..].copy_from_slice(&a_caso().to_le_bytes());
        Gettone(byte)
    };
    let coda = (0..voci)
        .map(|i| {
            let n = u64::try_from(i).unwrap();
            let disco = i.div_euclid(12);
            VoceSulFilo {
                uid: n + 1,
                di: u32::try_from(i % 3).unwrap(),
                titolo: format!("Titolo del brano numero {i}"),
                artista: format!("Interprete {}", disco % 37),
                album: format!("Album {disco}"),
                durata_ms: 180_000 + n * 777 % 120_000,
                numero: Some(u32::try_from(i % 12 + 1).unwrap()),
                anno: Some(1970 + i32::try_from(disco % 50).unwrap()),
                copertina: Some(gettone()),
                sorgente: SorgenteSulFilo::File {
                    gettone: gettone(),
                    estensione: Some("flac".to_owned()),
                    dimensione: 20_000_000 + n * 4_099,
                    segmento: None,
                },
                replaygain_db: Some(-7.25),
                picco: Some(0.987),
                copia: Some(DescrittoreCopia {
                    unc: None,
                    dimensione: 20_000_000 + n * 4_099,
                    modificato_ms: 1_700_000_000_000 + i64::try_from(i).unwrap(),
                    impronta_rapida: gettone().0.iter().map(|b| format!("{b:02x}")).collect(),
                    chiave_contenuto: None,
                }),
            }
        })
        .collect();
    Messaggio::Stato(StatoSessione {
        versione: 1,
        persone: vec![Persona {
            membro: 0,
            nome: "host".to_owned(),
            diritti: Diritti::Tutto,
        }],
        coda,
        corrente: Some(1),
        in_pausa: false,
        ripeti: Ripeti::No,
    })
}

#[test]
fn uno_stato_di_mille_voci_arriva_uguale_e_sul_filo_viaggia_compresso() {
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    membro.fidati(&[&host]);
    let indirizzo = host.ospita();
    let (staffetta, dall_host) = staffetta(indirizzo);
    let (id, sull_host) = membro.collega_a(&host, staffetta);
    let prima = dall_host.load(Ordering::SeqCst);
    let stato = stato_grande(1000);
    let json = serde_json::to_vec(&stato).expect("si serializza").len();
    host.rete.manda(sull_host, &stato).expect("parte");
    membro.aspetta("lo stato", |e| match e {
        Evento::Ricevuto { id: suo, messaggio } if *suo == id => {
            assert!(*messaggio == stato, "lo stato arriva uguale");
            Some(())
        }
        _ => None,
    });
    let sul_filo = dall_host.load(Ordering::SeqCst) - prima;
    eprintln!("stato di 1000 voci: {json} byte di JSON, {sul_filo} sul filo");
    assert!(
        sul_filo * 3 < json,
        "{sul_filo} byte sul filo per {json} di JSON"
    );
}

// ── orologio ───────────────────────────────────────────────────────────────

#[test]
fn l_orologio_da_campioni_coerenti_quattro_volte_al_secondo() {
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    membro.fidati(&[&host]);
    let indirizzo = host.ospita();
    let (id, _) = membro.collega_a(&host, indirizzo);
    let inizio = Instant::now();
    membro.rete.sincronizza(id, true);
    let mut campioni = Vec::new();
    while campioni.len() < 5 {
        campioni.push(membro.aspetta("orologio", |e| match e {
            Evento::Orologio { id: suo, campione } if *suo == id => Some(*campione),
            _ => None,
        }));
    }
    let durata = inizio.elapsed();
    for c in &campioni {
        assert!(c.t1 <= c.t4, "{c:?}");
        assert!(c.t2 <= c.t3, "{c:?}");
        // Stessa base dei tempi per i due capi: qui vale anche l'ordine
        // intero, ed è la prova che gli istanti sono presi dove devono.
        assert!(c.t1 <= c.t2 && c.t3 <= c.t4, "{c:?}");
    }
    assert!(
        durata < Duration::from_millis(1_600),
        "cinque campioni in poco più di un secondo: {durata:?}"
    );
    membro.rete.sincronizza(id, false);
    // Spenti, non ne arrivano più (al più uno già in volo).
    std::thread::sleep(Duration::from_millis(300));
    let dopo = membro
        .eventi
        .try_iter()
        .filter(|e| matches!(e, Evento::Orologio { .. }))
        .count();
    assert!(dopo <= 1, "{dopo} campioni dopo lo spegnimento");
}

// ── battito e silenzio ─────────────────────────────────────────────────────

#[test]
fn i_battiti_tengono_aperta_una_connessione_che_tace() {
    let tempi = Tempi {
        battito: Duration::from_millis(50),
        silenzio: Duration::from_millis(300),
        ..Tempi::default()
    };
    let host = capo_con("host", tempi);
    let membro = capo_con("membro", tempi);
    let indirizzo = host.ospita();
    let (id, _) = membro.collega_a(&host, indirizzo);
    // Quattro silenzi interi senza dirsi niente.
    assert!(
        membro
            .prova_ad_aspettare(Duration::from_millis(1_200), |e| match e {
                Evento::Chiusa { id: chiusa, .. } if *chiusa == id => Some(()),
                _ => None,
            })
            .is_none(),
        "i battiti bastano a tenerla aperta"
    );
}

// ── chiusure ───────────────────────────────────────────────────────────────

#[test]
fn smettere_di_ospitare_chiude_le_connessioni_in_entrata() {
    let host = capo("host");
    let membro = capo("membro");
    let indirizzo = host.ospita();
    let (id, dall_host) = membro.collega_a(&host, indirizzo);
    host.rete.smetti_di_ospitare();
    assert_eq!(
        host.chiusa(dall_host),
        None,
        "l'ha chiusa l'host, senza un perché"
    );
    assert_eq!(membro.chiusa(id), None);
}

#[test]
fn chiudere_mentre_si_apre_da_una_chiusa_sola_col_perche_del_desktop() {
    let host = capo("host");
    let membro = capo("membro");
    let indirizzo = host.ospita();
    let id = membro.rete.collega(indirizzo);
    membro.rete.chiudi(
        id,
        Some(rymembra_domain::errors::ErrorCodeKind::InsiemeRifiutato),
    );
    assert_eq!(
        membro.chiusa(id).map(|k| k.code()),
        Some("insieme.rifiutato")
    );
    assert!(
        membro
            .prova_ad_aspettare(Duration::from_millis(300), |e| match e {
                Evento::Chiusa { id: chiusa, .. } if *chiusa == id => Some(()),
                _ => None,
            })
            .is_none(),
        "una volta sola"
    );
}

// ── tre capi (fase 2) ──────────────────────────────────────────────────────

#[test]
fn tre_capi_un_membro_legge_il_brano_dell_altro_attraverso_l_host() {
    let host = capo("host");
    let anna = capo("anna");
    let bruno = capo("bruno");
    host.fidati(&[&anna, &bruno]);
    host.ammetti(&[&anna, &bruno]);
    anna.fidati(&[&host]);
    bruno.fidati(&[&host]);
    let indirizzo = host.ospita();
    anna.collega_a(&host, indirizzo);
    bruno.collega_a(&host, indirizzo);

    // Anna ha un brano suo in coda: lo serve, e apre la connessione di
    // servizio verso l'host.
    let cartella = Cartella::nuova("tre-capi");
    let contenuto = byte_di_prova(3 * 1024 * 1024 + 7);
    let gettone = Gettone::nuovo().expect("gettone");
    anna.rete
        .servi(gettone, cartella.file("di-anna.flac", &contenuto));
    let copertina: Arc<[u8]> = Arc::from(byte_di_prova(50_000));
    let gettone_copertina = Gettone::nuovo().expect("gettone");
    anna.rete
        .servi_copertina(gettone_copertina, Arc::clone(&copertina));
    anna.rete.offri(indirizzo, host.chiave());
    // L'host sa di chi sono quei byte.
    host.rete.inoltra(gettone, anna.chiave());
    host.rete.inoltra(gettone_copertina, anna.chiave());

    // Bruno legge dall'host come da chiunque: non sa che i byte sono di Anna.
    // La connessione di servizio si apre in un filo: si riprova finché non
    // c'è, entro l'attesa.
    let lettore = bruno.rete.lettore(indirizzo, host.chiave());
    let fine = Instant::now() + ATTESA;
    let quanti = u64::try_from(contenuto.len()).expect("sta");
    let (byte, totale) = loop {
        match lettore.pezzo(&gettone, 0, quanti, None) {
            Ok(letto) => break letto,
            Err(err) if Instant::now() < fine => {
                assert_eq!(codice_di(&err), "insieme.irraggiungibile");
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => panic!("la connessione di servizio non arriva: {err}"),
        }
    };
    assert_eq!(totale, quanti);
    assert!(
        byte == contenuto,
        "i byte di Anna, uguali, attraverso l'host"
    );
    assert_eq!(
        lettore.copertina(&gettone_copertina).expect("copertina"),
        &*copertina
    );

    // E l'host stesso, che deve suonare il brano di Anna, lo legge senza
    // passare dalla rete verso sé stesso.
    let (byte, _) = host
        .rete
        .lettore_locale()
        .pezzo(&gettone, 100, 1_000, None)
        .expect("l'host legge il brano di Anna");
    assert_eq!(byte, contenuto.get(100..1_100).expect("dentro"));

    // Anna revoca: chi legge lo sa.
    anna.rete.revoca(&gettone);
    let err = lettore.pezzo(&gettone, 0, 10, None).expect_err("revocato");
    assert_eq!(codice_di(&err), "insieme.gettoneScaduto");
    // L'host revoca l'inoltro: lo stesso.
    host.rete.revoca(&gettone_copertina);
    let err = lettore.copertina(&gettone_copertina).expect_err("revocato");
    assert_eq!(codice_di(&err), "insieme.gettoneScaduto");

    // `ferma` fa uscire tutti i fili, dei tre capi.
    drop(lettore);
    for capo in [&host, &anna, &bruno] {
        capo.rete.ferma();
    }
    for (nome, capo) in [("host", &host), ("anna", &anna), ("bruno", &bruno)] {
        assert!(
            capo.rete.aspetta_fili(Duration::from_secs(3)),
            "{nome}: {} fili ancora vivi",
            capo.rete.fili_in_vita()
        );
    }
}

#[test]
fn ferma_fa_uscire_tutti_i_fili_anche_a_meta_di_tutto() {
    let host = capo("host");
    let membro = capo("membro");
    host.fidati(&[&membro]);
    host.ammetti(&[&membro]);
    membro.fidati(&[&host]);
    let indirizzo = host.ospita();
    let (id, _) = membro.collega_a(&host, indirizzo);
    membro.rete.sincronizza(id, true);
    membro.rete.cerca(&[indirizzo]);
    let cartella = Cartella::nuova("ferma");
    let gettone = Gettone::nuovo().expect("gettone");
    host.rete
        .servi(gettone, cartella.file("a.flac", &byte_di_prova(1_000)));
    let lettore = membro.rete.lettore(indirizzo, host.chiave());
    lettore.pezzo(&gettone, 0, 10, None).expect("si legge");
    assert!(host.rete.fili_in_vita() > 0 && membro.rete.fili_in_vita() > 0);

    let inizio = Instant::now();
    membro.rete.ferma();
    host.rete.ferma();
    assert!(
        membro.rete.aspetta_fili(Duration::from_secs(3)),
        "membro: {} fili",
        membro.rete.fili_in_vita()
    );
    assert!(
        host.rete.aspetta_fili(Duration::from_secs(3)),
        "host: {} fili",
        host.rete.fili_in_vita()
    );
    eprintln!("ferma: tutti i fili usciti in {:?}", inizio.elapsed());
    // Dopo, niente si apre più.
    assert!(host.rete.ospita().is_err());
    let id = membro.rete.collega(indirizzo);
    assert_eq!(membro.chiusa(id), None);
}
