//! Le prove della revisione: una per ciascun difetto trovato, scritta **prima**
//! della correzione — e lì falliva — e verde dopo.
//!
//! Le prove di concetto del revisore stanno fuori dal repository; queste ne
//! rifanno gli attacchi dentro il crate, col capo grezzo di `prove_grezze`
//! dove serve parlare il protocollo a mano.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use rymembra_domain::errors::ErrorCodeKind;

use crate::cornice::{Tipo, comprimi_al_livello, gonfia};
use crate::identita::ChiavePubblica;
use crate::messaggi::{Gettone, Messaggio, VERSIONE};
use crate::prove_grezze::{ATTESA, Grezzo, aspetta, chiudo, host, json_app, stato_di_prova, vera};
use crate::rete::IdConnessione;
use crate::servente::prove::{Cartella, byte_di_prova};
use crate::trasporto::{Byte32, Trasporto, Uso};
use crate::{Evento, Identita, Tempi};

/// Un capo grezzo che si collega a un host vero, con la chiave e l'uso dati.
fn grezzo_verso(indirizzo: SocketAddr, identita: Identita, uso: Uso) -> Grezzo {
    let presa = TcpStream::connect(indirizzo).expect("connessione");
    let (grezzo, saluto) = Grezzo::stringi_con(identita, presa, true, VERSIONE, uso);
    assert!(matches!(saluto, Some(Trasporto::Ciao { .. })), "{saluto:?}");
    grezzo
}

/// Apre una connessione di controllo vera e aspetta l'apertura.
fn aperta(
    membro: &crate::Rete,
    eventi: &std::sync::mpsc::Receiver<Evento>,
    a: SocketAddr,
) -> IdConnessione {
    let id = membro.collega(a);
    aspetta(eventi, ATTESA, |e| match e {
        Evento::Aperta { id: aperta, .. } if *aperta == id => Some(()),
        _ => None,
    })
    .expect("aperta");
    id
}

const DUE_MEGA: u64 = 2 * 1024 * 1024;

// ── 1. i tentativi d'associazione ──────────────────────────────────────────

#[test]
fn sull_host_c_e_una_sola_associazione_alla_volta() {
    // La prova A del revisore: dodici membri associano insieme, e l'host
    // mostrava dodici codici. Un intermediario poteva aprire N strette e
    // rivelare solo dove il codice coincideva.
    let (_host, eventi_host, indirizzo) = host(Tempi::default());
    let mut membri = Vec::new();
    for _ in 0..6 {
        let (membro, eventi) = vera(Tempi::default());
        let id = aperta(&membro, &eventi, indirizzo);
        membri.push((membro, eventi, id));
    }
    for (membro, _, id) in &membri {
        membro.associa(*id).expect("da chi chiede parte");
    }
    let mut codici = 0;
    let fine = Instant::now() + Duration::from_secs(2);
    while let Ok(evento) = eventi_host.recv_timeout(fine.saturating_duration_since(Instant::now()))
    {
        if matches!(evento, Evento::Codice { .. }) {
            codici += 1;
        }
    }
    assert_eq!(codici, 1, "un codice solo sullo schermo dell'host");
    // Gli altri sentono «occupato», e restano collegati: dalla revisione del
    // 26 ottobre non è più un `Chiudo` con `troppiTentativi`.
    let rimandati = membri
        .iter()
        .filter(|(_, eventi, id)| {
            aspetta(eventi, Duration::from_secs(1), |e| match e {
                Evento::AssociazioneRimandata { id: suo, codice } if suo == id => Some(Ok(*codice)),
                Evento::Chiusa { id: chiusa, codice } if chiusa == id => Some(Err(*codice)),
                _ => None,
            }) == Some(Ok(ErrorCodeKind::InsiemeAssociazioneOccupata))
        })
        .count();
    assert_eq!(rimandati, 5, "gli altri sono rimandati, non chiusi");
    for (_, eventi, id) in &membri {
        assert!(
            aspetta(eventi, Duration::from_millis(100), |e| match e {
                Evento::Chiusa { id: chiusa, .. } if chiusa == id => Some(()),
                _ => None,
            })
            .is_none(),
            "nessuna connessione chiusa"
        );
    }
}

// ── 2. strette e posti ─────────────────────────────────────────────────────

#[test]
fn le_connessioni_mute_non_rubano_le_strette() {
    // La prova B del revisore: connessioni che non dicono niente tenevano le
    // quattro strette per cinque secondi, e nessun membro onesto entrava.
    let (_host, _eventi, indirizzo) = host(Tempi::default());
    let mut mute: Vec<TcpStream> = (0..6)
        .map(|_| TcpStream::connect(indirizzo).expect("muta"))
        .collect();
    std::thread::sleep(Duration::from_millis(100));
    let (membro, eventi) = vera(Tempi::default());
    let id = membro.collega(indirizzo);
    let esito = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Aperta { id: a, .. } if *a == id => Some(Ok(())),
        Evento::Chiusa { id: c, codice } if *c == id => Some(Err(*codice)),
        _ => None,
    });
    assert_eq!(esito, Some(Ok(())), "l'onesto entra");
    // E una muta si butta presto: il primo messaggio della stretta ha un
    // secondo, non cinque.
    let muta = mute.first_mut().expect("una");
    muta.set_read_timeout(Some(Duration::from_secs(4)))
        .expect("scadenza");
    let inizio = Instant::now();
    let mut buffer = [0_u8; 8];
    let _ = std::io::Read::read(muta, &mut buffer);
    assert!(
        inizio.elapsed() < Duration::from_secs(2),
        "chiusa dopo {:?}",
        inizio.elapsed()
    );
}

#[test]
fn una_chiave_non_fidata_apre_solo_connessioni_di_controllo() {
    // Il rifiuto arriva al posto del saluto: chi bussa lo sa subito.
    let (_host, _eventi, indirizzo) = host(Tempi::default());
    for uso in [Uso::Lettura, Uso::Servizio] {
        let presa = TcpStream::connect(indirizzo).expect("connessione");
        let (_grezzo, saluto) = Grezzo::stringi_con(
            Identita::nuova().expect("chiave"),
            presa,
            true,
            VERSIONE,
            uso,
        );
        assert_eq!(saluto, chiudo("insieme.nonAssociato"), "{uso:?}");
    }
}

#[test]
fn al_piu_otto_estranei_in_tutto() {
    let (_host, _eventi, indirizzo) = host(Tempi::default());
    let mut dentro: Vec<Grezzo> = (0..8)
        .map(|_| {
            grezzo_verso(
                indirizzo,
                Identita::nuova().expect("chiave"),
                Uso::Controllo,
            )
        })
        .collect();
    let presa = TcpStream::connect(indirizzo).expect("connessione");
    let (_nono, saluto) = Grezzo::stringi_con(
        Identita::nuova().expect("chiave"),
        presa,
        true,
        VERSIONE,
        Uso::Controllo,
    );
    assert_eq!(
        saluto,
        chiudo("insieme.irraggiungibile"),
        "il nono estraneo non entra"
    );
    for grezzo in &mut dentro {
        assert_eq!(
            grezzo.leggi(Duration::from_millis(100)),
            None,
            "i primi otto restano"
        );
    }
}

// ── 3. un gettone non si ruba ──────────────────────────────────────────────

#[test]
fn inoltra_non_ruba_il_gettone_di_un_file_dell_host() {
    let (host_rete, _eventi, indirizzo) = host(Tempi::default());
    let (membro, _eventi_membro) = vera(Tempi::default());
    let cartella = Cartella::nuova("revisione-gettone");
    let contenuto = byte_di_prova(1_000);
    let gettone = Gettone([3; 16]);
    host_rete.servi(gettone, cartella.file("dell-host.flac", &contenuto));
    // Un membro che annuncia come suo il gettone di un brano dell'host.
    let ladro = Identita::nuova().expect("chiave").pubblica();
    let _ = host_rete.inoltra(gettone, ladro);
    host_rete.pari(vec![membro.pubblica(), ladro], Vec::new());
    ammetti(&host_rete, &[membro.pubblica()]);
    let (byte, _) = membro
        .lettore(indirizzo, host_rete.pubblica())
        .pezzo(&gettone, 0, 1_000, None)
        .expect("il file dell'host, non i byte del ladro");
    assert_eq!(byte, contenuto);
}

// ── 4. la coda d'uscita ────────────────────────────────────────────────────

/// Un capo grezzo fidato con una connessione di lettura, e un file da
/// `mega` mebibyte servito sotto un gettone.
fn lettore_grezzo(mega: usize) -> (crate::Rete, Grezzo, Gettone, crate::FileServito, Cartella) {
    let (host_rete, _eventi, indirizzo) = host(Tempi::default());
    let cartella = Cartella::nuova("revisione-coda");
    let file = cartella.file("grande.flac", &byte_di_prova(mega * 1024 * 1024));
    let gettone = Gettone([4; 16]);
    host_rete.servi(gettone, file.clone());
    let identita = Identita::nuova().expect("chiave");
    host_rete.pari(vec![identita.pubblica()], Vec::new());
    ammetti(&host_rete, &[identita.pubblica()]);
    let grezzo = grezzo_verso(indirizzo, identita, Uso::Lettura);
    (host_rete, grezzo, gettone, file, cartella)
}

/// Le risposte a `quante` letture: quanti `Pezzo` e quanti rifiuti con quel
/// codice.
fn conta_risposte(grezzo: &mut Grezzo, quante: usize, rifiuto: &str) -> (usize, usize) {
    let (mut pezzi, mut rifiuti) = (0, 0);
    for _ in 0..quante {
        match grezzo.leggi(Duration::from_secs(60)) {
            Some(Trasporto::Pezzo { .. }) => pezzi += 1,
            Some(Trasporto::LetturaRifiutata { codice, .. }) if codice == rifiuto => rifiuti += 1,
            altro => panic!("risposta inattesa: {altro:?}"),
        }
    }
    (pezzi, rifiuti)
}

#[test]
fn chi_non_legge_le_risposte_non_fa_accumulare_piu_di_otto_mega() {
    // Dodici letture da due mega chieste senza leggere le risposte: prima
    // l'host le preparava tutte, ventiquattro mega in coda (fino a 512 MiB con
    // la coda piena). Il file poi cambia: le letture servite dopo il cambio
    // lo dicono, e sono quelle che la contropressione ha fatto aspettare.
    let (_host, mut grezzo, gettone, file, _cartella) = lettore_grezzo(24);
    for i in 0..12_u32 {
        grezzo.manda(&Trasporto::Leggi {
            lettura: i,
            gettone,
            da: u64::from(i) * DUE_MEGA,
            quanti: DUE_MEGA,
        });
    }
    std::thread::sleep(Duration::from_millis(500));
    std::fs::write(&file.percorso, byte_di_prova(24 * 1024 * 1024 - 1)).expect("riscrittura");
    let (pezzi, cambiati) = conta_risposte(&mut grezzo, 12, "insieme.fileCambiato");
    assert!(
        pezzi <= 6,
        "{pezzi} letture preparate prima di poterle mandare"
    );
    assert_eq!(pezzi + cambiati, 12);
}

#[test]
fn al_piu_due_connessioni_di_lettura_e_una_di_servizio_per_chiave() {
    let (host_rete, _eventi, indirizzo) = host(Tempi::default());
    let identita = Identita::nuova().expect("chiave");
    let testo = identita.in_testo();
    host_rete.pari(vec![identita.pubblica()], Vec::new());
    let stessa = || Identita::da_testo(&testo).expect("la stessa chiave");
    let mut prima = grezzo_verso(indirizzo, identita, Uso::Lettura);
    let _seconda = grezzo_verso(indirizzo, stessa(), Uso::Lettura);
    let _terza = grezzo_verso(indirizzo, stessa(), Uso::Lettura);
    assert_eq!(
        prima.leggi(Duration::from_secs(2)),
        Some(Trasporto::Chiudo { codice: None }),
        "la più vecchia lascia il posto"
    );
    let mut servizio = grezzo_verso(indirizzo, stessa(), Uso::Servizio);
    let _altro = grezzo_verso(indirizzo, stessa(), Uso::Servizio);
    assert_eq!(
        servizio.leggi(Duration::from_secs(2)),
        Some(Trasporto::Chiudo { codice: None })
    );
}

#[test]
fn una_terza_connessione_di_lettura_manda_via_quella_ferma_e_non_quella_che_lavora() {
    // Prima andava via la più vecchia per apertura, anche se stava mandando
    // i byte del brano che suona mentre l'altra era lì senza far niente.
    let (host_rete, _eventi, indirizzo) = host(Tempi::default());
    let cartella = Cartella::nuova("revisione-vivaio");
    let gettone = Gettone([10; 16]);
    host_rete.servi(
        gettone,
        cartella.file("grande.flac", &byte_di_prova(24 * 1024 * 1024)),
    );
    let identita = Identita::nuova().expect("chiave");
    let testo = identita.in_testo();
    host_rete.pari(vec![identita.pubblica()], Vec::new());
    ammetti(&host_rete, &[identita.pubblica()]);
    let stessa = || Identita::da_testo(&testo).expect("la stessa chiave");
    // La più vecchia lavora: dodici letture da due mega, le risposte non
    // ancora lette — chi legge sull'host aspetta posto in coda.
    let mut al_lavoro = grezzo_verso(indirizzo, identita, Uso::Lettura);
    for i in 0..12_u32 {
        al_lavoro.manda(&Trasporto::Leggi {
            lettura: i,
            gettone,
            da: u64::from(i) * DUE_MEGA,
            quanti: DUE_MEGA,
        });
    }
    std::thread::sleep(Duration::from_millis(300));
    let mut ferma = grezzo_verso(indirizzo, stessa(), Uso::Lettura);
    let _terza = grezzo_verso(indirizzo, stessa(), Uso::Lettura);
    assert_eq!(
        ferma.leggi(Duration::from_secs(2)),
        Some(Trasporto::Chiudo { codice: None }),
        "va via quella ferma"
    );
    let (pezzi, _) = conta_risposte(&mut al_lavoro, 12, "nessun rifiuto");
    assert_eq!(pezzi, 12, "quella al lavoro finisce il suo lavoro");
}

// ── 5. il JSON di un estraneo ──────────────────────────────────────────────

#[test]
fn un_json_grande_da_un_estraneo_chiude_prima_di_leggerlo() {
    let (host_rete, _eventi, indirizzo) = host(Tempi::default());
    let grande = format!(r#"{{"tipo":"dalFuturo","x":"{}"}}"#, "a".repeat(64 * 1024));
    let mut estraneo = grezzo_verso(
        indirizzo,
        Identita::nuova().expect("chiave"),
        Uso::Controllo,
    );
    estraneo
        .cifratore
        .aggiungi(Tipo::Json, grande.as_bytes())
        .expect("cifra");
    estraneo
        .cifratore
        .manda(&mut estraneo.presa)
        .expect("scrive");
    assert_eq!(
        estraneo.leggi(Duration::from_secs(2)),
        chiudo("insieme.protocollo")
    );
    // Da un fidato lo stesso messaggio è solo un tipo sconosciuto.
    let fidato = Identita::nuova().expect("chiave");
    host_rete.pari(vec![fidato.pubblica()], Vec::new());
    let mut fidato = grezzo_verso(indirizzo, fidato, Uso::Controllo);
    fidato
        .cifratore
        .aggiungi(Tipo::Json, grande.as_bytes())
        .expect("cifra");
    fidato.cifratore.manda(&mut fidato.presa).expect("scrive");
    assert_eq!(fidato.leggi(Duration::from_millis(500)), None);
}

#[test]
fn chi_diventa_fidato_mentre_l_host_aspetta_puo_mandare_subito_un_messaggio_grande() {
    // I tetti si fissavano prima dell'attesa: il primo messaggio dopo
    // l'associazione — lo `Stato`, cento kibibyte — trovava ancora i quattro
    // dell'estraneo, e la connessione si chiudeva con `insieme.protocollo`.
    let (host_rete, eventi, indirizzo) = host(Tempi::default());
    let identita = Identita::nuova().expect("chiave");
    let sua = identita.pubblica();
    let mut grezzo = grezzo_verso(indirizzo, identita, Uso::Controllo);
    aspetta(&eventi, ATTESA, |e| match e {
        Evento::Aperta { .. } => Some(()),
        _ => None,
    })
    .expect("aperta");
    // Il filo che legge è già fermo ad aspettare, da estraneo.
    std::thread::sleep(Duration::from_millis(200));
    host_rete.pari(vec![sua], Vec::new());
    let grande = format!(r#"{{"tipo":"dalFuturo","x":"{}"}}"#, "a".repeat(64 * 1024));
    grezzo
        .cifratore
        .aggiungi(Tipo::Json, grande.as_bytes())
        .expect("cifra");
    grezzo.cifratore.manda(&mut grezzo.presa).expect("scrive");
    assert_eq!(
        grezzo.leggi(Duration::from_millis(500)),
        None,
        "da fidato è un tipo sconosciuto, non un protocollo violato"
    );
}

#[test]
fn un_gesto_di_una_versione_piu_nuova_arriva_al_desktop_e_non_chiude() {
    // Un `Azione` nuova faceva fallire la lettura di tutto il messaggio: la
    // connessione si chiudeva con `insieme.protocollo` invece di lasciare al
    // desktop la risposta («gesto non disponibile»).
    let (host_rete, eventi, indirizzo) = host(Tempi::default());
    let identita = Identita::nuova().expect("chiave");
    host_rete.pari(vec![identita.pubblica()], Vec::new());
    let mut grezzo = grezzo_verso(indirizzo, identita, Uso::Controllo);
    grezzo
        .cifratore
        .aggiungi(
            Tipo::Json,
            br#"{"tipo":"app","messaggio":{"tipo":"comando","id":7,"azione":{"tipo":"dalFuturo","x":1}}}"#,
        )
        .expect("cifra");
    grezzo.cifratore.manda(&mut grezzo.presa).expect("scrive");
    let gesto = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Ricevuto {
            messaggio: Messaggio::Comando { id, azione },
            ..
        } => Some((*id, azione.clone())),
        _ => None,
    });
    assert_eq!(gesto, Some((7, crate::messaggi::Azione::Sconosciuta)));
    assert_eq!(
        grezzo.leggi(Duration::from_millis(300)),
        None,
        "la connessione resta"
    );
}

// ── la compressione ────────────────────────────────────────────────────────

/// Un host vero e un capo grezzo fidato collegato per il controllo, con
/// l'id della connessione dalla parte dell'host.
fn host_e_grezzo_fidato() -> (crate::Rete, Grezzo, IdConnessione) {
    let (host_rete, eventi, indirizzo) = host(Tempi::default());
    let identita = Identita::nuova().expect("chiave");
    host_rete.pari(vec![identita.pubblica()], Vec::new());
    let grezzo = grezzo_verso(indirizzo, identita, Uso::Controllo);
    let id = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Aperta { id, .. } => Some(*id),
        _ => None,
    })
    .expect("aperta");
    (host_rete, grezzo, id)
}

#[test]
fn lo_stato_grande_viaggia_compresso_e_i_messaggi_piccoli_no() {
    let (host_rete, mut grezzo, id) = host_e_grezzo_fidato();
    // Un esito: pochi byte, in chiaro, uguali a prima.
    let esito = Messaggio::Esito {
        id: 7,
        codice: None,
    };
    host_rete.manda(id, &esito).expect("parte");
    assert_eq!(
        grezzo.leggi_grezzo(ATTESA),
        Some((Tipo::Json, json_app(&esito)))
    );
    // Lo stato di mille voci: compresso sul filo, e torna uguale.
    let stato = stato_di_prova(971, 1);
    let json = json_app(&stato);
    host_rete.manda(id, &stato).expect("parte");
    let Some((Tipo::JsonCompresso, compresso)) = grezzo.leggi_grezzo(ATTESA) else {
        panic!("lo stato doveva viaggiare compresso");
    };
    eprintln!(
        "stato di 971 voci sul filo: {} byte compressi per {} di JSON",
        compresso.len(),
        json.len()
    );
    assert!(compresso.len() * 3 < json.len(), "{}", compresso.len());
    assert!(
        gonfia(&compresso, crate::cornice::MASSIMO_MESSAGGIO).expect("si decomprime") == json,
        "e torna uguale"
    );
}

#[test]
fn un_messaggio_compresso_da_un_estraneo_chiude_col_protocollo() {
    // Anche piccolo, e anche se decompresso starebbe nel suo tetto: da un
    // estraneo i compressi non si accettano.
    let (_host, _eventi, indirizzo) = host(Tempi::default());
    let mut estraneo = grezzo_verso(
        indirizzo,
        Identita::nuova().expect("chiave"),
        Uso::Controllo,
    );
    let battito = comprimi_al_livello(&[b' '; 9000], 1).expect("si comprime");
    estraneo.manda_grezzo(Tipo::JsonCompresso, &battito);
    assert_eq!(
        estraneo.leggi(Duration::from_secs(2)),
        chiudo("insieme.protocollo")
    );
}

#[test]
fn una_bomba_compressa_da_un_fidato_chiude_col_protocollo() {
    let (_host, mut fidato, _id) = host_e_grezzo_fidato();
    // Sedici mebibyte di spazi in qualche kibibyte: sotto il tetto del filo,
    // sedici volte quello di un JSON.
    let mut spazi = vec![b' '; 16 * crate::cornice::MASSIMO_MESSAGGIO];
    if let Some(primo) = spazi.first_mut() {
        *primo = b'{';
    }
    let bomba = comprimi_al_livello(&spazi, 9).expect("si comprime");
    drop(spazi);
    assert!(bomba.len() < 64 * 1024, "{}", bomba.len());
    fidato.manda_grezzo(Tipo::JsonCompresso, &bomba);
    assert_eq!(
        fidato.leggi(Duration::from_secs(5)),
        chiudo("insieme.protocollo")
    );
}

#[test]
fn degli_stati_in_coda_parte_solo_l_ultimo() {
    // Il grezzo non legge: chi scrive sull'host resta fermo sui primi
    // comandi, grandi e incomprimibili, e dietro di loro si mettono in coda
    // tre stati e un esito. Parte solo il terzo stato, e l'esito dopo di lui.
    let (host_rete, mut grezzo, id) = host_e_grezzo_fidato();
    let a_caso = |seme: usize| {
        (0..8)
            .map(|k| {
                blake3::hash(format!("{seme}-{k}").as_bytes())
                    .to_hex()
                    .to_string()
            })
            .collect::<String>()
    };
    let Messaggio::Stato(stato) = stato_di_prova(200, 0) else {
        panic!("uno stato");
    };
    let voci = stato
        .coda
        .into_iter()
        .enumerate()
        .map(|(i, mut voce)| {
            voce.titolo = a_caso(i * 3);
            voce.artista = a_caso(i * 3 + 1);
            voce.album = a_caso(i * 3 + 2);
            voce
        })
        .collect();
    let riempitivo = Messaggio::Comando {
        id: 0,
        azione: crate::messaggi::Azione::Aggiungi {
            voci,
            dove: crate::messaggi::Dove::InFondo,
            suona: false,
        },
    };
    const RIEMPITIVI: usize = 12;
    for _ in 0..RIEMPITIVI {
        host_rete.manda(id, &riempitivo).expect("parte");
    }
    for versione in 1..=3 {
        host_rete
            .manda(id, &stato_di_prova(50, versione))
            .expect("parte");
    }
    host_rete
        .manda(
            id,
            &Messaggio::Esito {
                id: 9,
                codice: None,
            },
        )
        .expect("parte");
    let mut arrivati = Vec::new();
    loop {
        match grezzo.leggi(Duration::from_secs(10)) {
            Some(Trasporto::App {
                messaggio: Messaggio::Comando { .. },
            }) => arrivati.push("comando".to_owned()),
            Some(Trasporto::App {
                messaggio: Messaggio::Stato(stato),
            }) => arrivati.push(format!("stato {}", stato.versione)),
            Some(Trasporto::App {
                messaggio: Messaggio::Esito { .. },
            }) => {
                arrivati.push("esito".to_owned());
                break;
            }
            altro => panic!("inatteso: {altro:?} dopo {arrivati:?}"),
        }
    }
    let dopo: Vec<&str> = arrivati
        .iter()
        .skip(RIEMPITIVI)
        .map(String::as_str)
        .collect();
    assert_eq!(dopo, ["stato 3", "esito"], "{arrivati:?}");
}

// ── 6. la scoperta ─────────────────────────────────────────────────────────

#[test]
fn la_scoperta_non_ripete_lo_stesso_host_nello_stesso_giro() {
    let (_host, _eventi, indirizzo) = host(Tempi::default());
    let (membro, eventi) = vera(Tempi::default());
    membro.cerca(&[indirizzo, indirizzo, indirizzo]);
    let mut trovati = 0;
    let fine = Instant::now() + Duration::from_millis(800);
    while let Ok(evento) = eventi.recv_timeout(fine.saturating_duration_since(Instant::now())) {
        if matches!(evento, Evento::Trovato { .. }) {
            trovati += 1;
        }
    }
    assert_eq!(trovati, 1);
}

// ── 7. la revoca e i dati già in coda ──────────────────────────────────────

#[test]
fn la_revoca_ferma_anche_i_dati_gia_in_coda() {
    let (host_rete, mut grezzo, gettone, _file, _cartella) = lettore_grezzo(16);
    for i in 0..8_u32 {
        grezzo.manda(&Trasporto::Leggi {
            lettura: i,
            gettone,
            da: u64::from(i) * DUE_MEGA,
            quanti: DUE_MEGA,
        });
    }
    std::thread::sleep(Duration::from_millis(500));
    host_rete.revoca(&gettone);
    let (pezzi, scaduti) = conta_risposte(&mut grezzo, 8, "insieme.gettoneScaduto");
    assert!(
        pezzi <= 2,
        "{pezzi} letture consegnate dopo la revoca (solo quella già sul filo può)"
    );
    assert_eq!(pezzi + scaduti, 8);
}

// ── 8. i ruoli dell'associazione ───────────────────────────────────────────

#[test]
fn chi_ospita_non_chiede_l_associazione() {
    let (host_rete, eventi_host, indirizzo) = host(Tempi::default());
    let (membro, eventi) = vera(Tempi::default());
    aperta(&membro, &eventi, indirizzo);
    let dall_host = aspetta(&eventi_host, ATTESA, |e| match e {
        Evento::Aperta { id, .. } => Some(*id),
        _ => None,
    })
    .expect("aperta sull'host");
    assert!(host_rete.associa(dall_host).is_err());
}

#[test]
fn un_impegno_da_chi_ospita_chiude_la_connessione() {
    let ascolto = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("ascolto");
    let indirizzo = ascolto.local_addr().expect("indirizzo");
    let (membro, _eventi) = vera(Tempi::default());
    let _id = membro.collega(indirizzo);
    let (presa, _) = ascolto.accept().expect("accetta");
    let (mut grezzo, _) = Grezzo::stringi(presa, false, VERSIONE, Uso::Controllo);
    grezzo.manda(&Trasporto::Impegno {
        impegno: Byte32([1; 32]),
    });
    assert_eq!(
        grezzo.leggi(Duration::from_secs(2)),
        chiudo("insieme.protocollo")
    );
}

// ── 9. chi può leggere ─────────────────────────────────────────────────────

#[test]
fn un_fidato_che_non_e_nella_sessione_non_legge() {
    let (host_rete, _eventi, indirizzo) = host(Tempi::default());
    let (membro, _eventi_membro) = vera(Tempi::default());
    let cartella = Cartella::nuova("revisione-ammessi");
    let gettone = Gettone([5; 16]);
    host_rete.servi(gettone, cartella.file("a.flac", &byte_di_prova(100)));
    host_rete.pari(vec![membro.pubblica()], Vec::new());
    let err = membro
        .lettore(indirizzo, host_rete.pubblica())
        .pezzo(&gettone, 0, 10, None)
        .expect_err("fidato ma fuori dalla sessione");
    assert_eq!(err.code().kind().code(), "insieme.gettoneScaduto");
}

// ── 10. le stampe di prova ─────────────────────────────────────────────────

#[test]
fn le_stampe_di_prova_non_mostrano_nomi_indirizzi_titoli_e_codici() {
    let aperta = Evento::Aperta {
        id: IdConnessione(1),
        pari: ChiavePubblica([7; 32]),
        nome: "PC di Anna".to_owned(),
        ospita: true,
        in_entrata: false,
        indirizzo: "192.168.1.20:47811".parse().expect("indirizzo"),
    };
    let stampa = format!("{aperta:?}");
    assert!(
        !stampa.contains("Anna") && !stampa.contains("192.168"),
        "{stampa}"
    );
    let codice = Evento::Codice {
        id: IdConnessione(1),
        codice: "4821".to_owned(),
    };
    assert!(!format!("{codice:?}").contains("4821"));
    let trovato = Evento::Trovato {
        indirizzo: "10.0.0.7:47811".parse().expect("indirizzo"),
        impronta: ChiavePubblica([7; 32]).impronta(),
    };
    assert!(!format!("{trovato:?}").contains("10.0.0.7"));
    let stato: Messaggio = serde_json::from_str(
        r#"{"tipo":"stato","versione":1,"persone":[{"membro":0,"nome":"Marco"}],
            "coda":[{"uid":1,"di":0,"titolo":"Titolo segreto","artista":"Interprete segreto",
            "album":"Album segreto","durataMs":1,"numero":null,"anno":null,"copertina":null,
            "sorgente":{"tipo":"catalogo","riferimento":"catalogo:segreto"},
            "replaygainDb":null,"picco":null,
            "copia":{"unc":"\\\\nas\\segreta\\a.flac","dimensione":1,"modificatoMs":1,
            "improntaRapida":"ab","chiaveContenuto":null}}],
            "corrente":1,"inPausa":false,"ripeti":"no"}"#,
    )
    .expect("uno stato");
    let stampa = format!("{stato:?}");
    for segreto in ["Marco", "segret"] {
        assert!(!stampa.contains(segreto), "{segreto} in {stampa}");
    }
}

/// Le chiavi ammesse a leggere. Prima della correzione non c'era: chi era
/// fidato leggeva.
fn ammetti(rete: &crate::Rete, chiavi: &[ChiavePubblica]) {
    rete.ammessi(chiavi.to_vec());
}

// ── le prove che prima non compilavano ─────────────────────────────────────

#[test]
fn un_estraneo_che_non_si_associa_non_resta_per_sempre_e_un_fidato_si() {
    let tempi = Tempi {
        vita_estraneo: Duration::from_millis(400),
        ..Tempi::default()
    };
    let (host_rete, _eventi, indirizzo) = host(tempi);
    let fidato = Identita::nuova().expect("chiave");
    host_rete.pari(vec![fidato.pubblica()], Vec::new());
    let mut estraneo = grezzo_verso(
        indirizzo,
        Identita::nuova().expect("chiave"),
        Uso::Controllo,
    );
    let mut amico = grezzo_verso(indirizzo, fidato, Uso::Controllo);
    // Tutti e due battono; solo il battito, niente associazione.
    let inizio = Instant::now();
    let mut chiuso = None;
    while inizio.elapsed() < Duration::from_secs(2) && chiuso.is_none() {
        estraneo.manda(&Trasporto::Battito);
        amico.manda(&Trasporto::Battito);
        chiuso = estraneo.leggi(Duration::from_millis(100));
    }
    // Col suo perché, dalla revisione del 26 ottobre: prima arrivava senza, e
    // non si distingueva da un host che smette di ospitare.
    assert_eq!(chiuso, chiudo("insieme.estraneoScaduto"));
    assert!(inizio.elapsed() < Duration::from_secs(2));
    amico.manda(&Trasporto::Battito);
    assert_eq!(
        amico.leggi(Duration::from_millis(300)),
        None,
        "il fidato resta"
    );
}

#[test]
fn collega_atteso_chiude_se_la_chiave_non_ha_l_impronta_annunciata() {
    let (host_rete, _eventi, indirizzo) = host(Tempi::default());
    let (membro, eventi) = vera(Tempi::default());
    let giusta = membro.collega_atteso(indirizzo, host_rete.pubblica().impronta());
    let esito = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Aperta { id, .. } if *id == giusta => Some(Ok(())),
        Evento::Chiusa { id, codice } if *id == giusta => Some(Err(*codice)),
        _ => None,
    });
    assert_eq!(esito, Some(Ok(())));
    // Una risposta alla scoperta falsa: l'impronta di un altro.
    let altra = Identita::nuova().expect("chiave").pubblica().impronta();
    let sbagliata = membro.collega_atteso(indirizzo, altra);
    let esito = aspetta(&eventi, ATTESA, |e| match e {
        Evento::Aperta { id, .. } if *id == sbagliata => Some(Ok(())),
        Evento::Chiusa { id, codice } if *id == sbagliata => Some(Err(*codice)),
        _ => None,
    });
    assert_eq!(
        esito,
        Some(Err(Some(ErrorCodeKind::InsiemeIrraggiungibile)))
    );
}

#[test]
fn gli_ammessi_leggono_e_chi_esce_dalla_sessione_smette() {
    let (host_rete, _eventi, indirizzo) = host(Tempi::default());
    let (membro, _eventi_membro) = vera(Tempi::default());
    let cartella = Cartella::nuova("revisione-ammessi-si");
    let contenuto = byte_di_prova(100);
    let gettone = Gettone([6; 16]);
    host_rete.servi(gettone, cartella.file("a.flac", &contenuto));
    host_rete.pari(vec![membro.pubblica()], Vec::new());
    host_rete.ammessi(vec![membro.pubblica()]);
    let lettore = membro.lettore(indirizzo, host_rete.pubblica());
    let (byte, _) = lettore.pezzo(&gettone, 0, 100, None).expect("ammesso");
    assert_eq!(byte, contenuto);
    host_rete.ammessi(Vec::new());
    let err = lettore
        .pezzo(&gettone, 0, 100, None)
        .expect_err("uscito dalla sessione");
    assert_eq!(err.code().kind().code(), "insieme.gettoneScaduto");
}

#[test]
fn inoltra_dice_se_il_gettone_era_libero() {
    let (host_rete, _eventi, _indirizzo) = host(Tempi::default());
    let cartella = Cartella::nuova("revisione-inoltra");
    let (mio, suo) = (Gettone([7; 16]), Gettone([8; 16]));
    host_rete.servi(mio, cartella.file("a.flac", &byte_di_prova(10)));
    let (anna, bruno) = (
        Identita::nuova().expect("chiave").pubblica(),
        Identita::nuova().expect("chiave").pubblica(),
    );
    assert!(
        !host_rete.inoltra(mio, anna),
        "il gettone di un file dell'host"
    );
    assert!(host_rete.inoltra(suo, anna));
    assert!(host_rete.inoltra(suo, anna), "lo stesso, di nuovo");
    assert!(
        !host_rete.inoltra(suo, bruno),
        "il gettone di un altro membro"
    );
}
