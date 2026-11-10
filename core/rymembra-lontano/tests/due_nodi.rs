//! Due nodi veri nello stesso processo, senza relay né ricerche: solo
//! `127.0.0.1`. L'associazione col QR (gettone giusto, sbagliato, riusato,
//! rifiutato), il cancello per chi non è associato, le domande in tutte e due
//! le direzioni, i byte, la presenza.

// Come in `rymembra-insieme/tests`: `clippy.toml` concede `unwrap`, `expect`
// e `panic!` ai soli `#[test]`, non agli aiutanti di una crate di prova a sé.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_lontano::nodo::TENTATIVI_AL_MINUTO;
use rymembra_lontano::{
    Domanda, Evento, Flusso, Identita, Impostazioni, Nodo, Ospite, PariNoto, Presentazione,
    Presenza, Rete, Risposta,
};

/// Un'app finta: ricorda chi è associato e cosa è successo.
#[derive(Default)]
struct Finta {
    associati: Mutex<HashSet<String>>,
    /// Cosa risponde chi mostra il QR.
    consente: Mutex<bool>,
    nuovi: Mutex<Vec<(String, Presentazione)>>,
    eventi: Mutex<Vec<Evento>>,
    dimenticati: Mutex<Vec<String>>,
    /// I flussi di «Ascolta insieme» arrivati, con chi li ha aperti.
    flussi: Mutex<Vec<(String, Flusso)>>,
    /// Chi mostra il QR non risponde finché chi l'ha letto non rinuncia.
    tentenna: Mutex<bool>,
    /// Le domande ritirate da chi aveva letto il QR.
    ritirate: Mutex<Vec<String>>,
}

impl Ospite for Finta {
    fn associato(&self, chiave: &str) -> bool {
        self.associati.lock().unwrap().contains(chiave)
    }

    fn conferma(&self, _chiave: &str, _chi: &Presentazione) -> bool {
        if *self.tentenna.lock().unwrap() {
            // Come la finestrella vera: risponde no quando la domanda è
            // ritirata, o allo scadere.
            let fine = Instant::now() + Duration::from_secs(20);
            while Instant::now() < fine && self.ritirate.lock().unwrap().is_empty() {
                std::thread::sleep(Duration::from_millis(20));
            }
            return false;
        }
        *self.consente.lock().unwrap()
    }

    fn ritirata(&self, chiave: &str) {
        self.ritirate.lock().unwrap().push(chiave.to_owned());
    }

    fn nuovo_pari(&self, chiave: &str, chi: &Presentazione, _indirizzi: &[SocketAddr]) {
        self.associati.lock().unwrap().insert(chiave.to_owned());
        self.nuovi
            .lock()
            .unwrap()
            .push((chiave.to_owned(), chi.clone()));
    }

    fn rispondi(&self, da: &str, domanda: Domanda) -> Result<Risposta, AppError> {
        match domanda {
            Domanda::Leggi { brano: 404, .. } => Err(AppError::new(ErrorCode::LontanoFileCambiato)),
            Domanda::Leggi {
                da: inizio, quanti, ..
            } => {
                let fine = (inizio + quanti).min(1000);
                Ok(Risposta::Byte {
                    byte: (inizio..fine)
                        .map(|n| u8::try_from(n % 251).unwrap())
                        .collect(),
                    totale: 1000,
                    versione: "v1".to_owned(),
                })
            }
            Domanda::Album { da: inizio, quanti } => Ok(Risposta::Dati(serde_json::json!({
                "chi": da, "da": inizio, "quanti": quanti
            }))),
            _ => Err(AppError::new(ErrorCode::InternalUnexpected {
                detail: None,
            })),
        }
    }

    fn dimenticato(&self, da: &str) {
        self.associati.lock().unwrap().remove(da);
        self.dimenticati.lock().unwrap().push(da.to_owned());
    }

    fn evento(&self, evento: Evento) {
        self.eventi.lock().unwrap().push(evento);
    }

    fn insieme(&self, da: &str, flusso: Flusso) {
        self.flussi.lock().unwrap().push((da.to_owned(), flusso));
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
                chiave_insieme: Some(format!("insieme-{nome}")),
            },
            presenza: Presenza {
                nome: nome.to_owned(),
                piattaforma: "prova".to_owned(),
                versione: "0".to_owned(),
                ..Presenza::default()
            },
        },
        Arc::clone(&finta) as Arc<dyn Ospite>,
    )
    .expect("il nodo parte");
    (nodo, finta)
}

fn codice(esito: Result<impl std::fmt::Debug, AppError>) -> ErrorCodeKind {
    esito.expect_err("un errore").code().kind()
}

/// Aspetta che `vale` diventi vero, al più qualche secondo.
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

#[test]
fn col_gettone_giusto_ci_si_associa_e_ci_si_parla() {
    let (pc, del_pc) = nodo("PC");
    let (telefono, del_telefono) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;

    let invito = pc.invita().unwrap();
    assert!(!invito.indirizzi.is_empty(), "{invito:?}");
    // Il link dice chi lo mostra — un nome dichiarato, e l'impronta della
    // chiave, la stessa che il PC mostra accanto al QR.
    let letto = rymembra_lontano::Invito::da_link(&invito.link()).unwrap();
    assert_eq!(letto.nome.as_deref(), Some("PC"));
    assert_eq!(letto.impronta(), pc.impronta());
    let (chiave_pc, chi) = telefono.collega(&invito.link()).expect("associati");
    assert_eq!(chiave_pc, pc.chiave());
    assert_eq!(chi.nome, "PC");
    assert_eq!(chi.chiave_insieme.as_deref(), Some("insieme-PC"));
    // Tutti e due si ricordano dell'altro.
    let visti_dal_pc = del_pc.nuovi.lock().unwrap().clone();
    assert_eq!(visti_dal_pc.len(), 1);
    assert_eq!(visti_dal_pc[0].0, telefono.chiave());
    assert_eq!(visti_dal_pc[0].1.dispositivo, "d-Telefono");
    assert!(del_telefono.associato(&pc.chiave()));

    // Il telefono chiede al PC.
    let risposta = telefono
        .chiedi(
            &pc.chiave(),
            Domanda::Album {
                da: 0,
                quanti: 10_000,
            },
            Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(
        risposta,
        Risposta::Dati(serde_json::json!({
            "chi": telefono.chiave(), "da": 0, "quanti": 500
        })),
        "la pagina nei tetti, e chi chiede è chi dice di essere"
    );
    let Risposta::Byte {
        byte,
        totale,
        versione,
    } = telefono
        .chiedi(
            &pc.chiave(),
            Domanda::Leggi {
                brano: 1,
                da: 990,
                quanti: 100,
            },
            Duration::from_secs(10),
        )
        .unwrap()
    else {
        panic!("dei byte");
    };
    assert_eq!(byte.len(), 10);
    assert_eq!(byte[0], u8::try_from(990 % 251).unwrap());
    assert_eq!((totale, versione.as_str()), (1000, "v1"));

    // Un rifiuto arriva col suo codice; un guasto di un altro dominio no.
    assert_eq!(
        codice(telefono.chiedi(
            &pc.chiave(),
            Domanda::Leggi {
                brano: 404,
                da: 0,
                quanti: 1
            },
            Duration::from_secs(10)
        )),
        ErrorCodeKind::LontanoFileCambiato
    );
    assert_eq!(
        codice(telefono.chiedi(
            &pc.chiave(),
            Domanda::BraniAlbum {
                album: "a".to_owned()
            },
            Duration::from_secs(10)
        )),
        ErrorCodeKind::LontanoProtocollo
    );

    // E il PC chiede al telefono, sulla stessa connessione o su una sua.
    assert!(matches!(
        pc.chiedi(
            &telefono.chiave(),
            Domanda::Presenza,
            Duration::from_secs(10)
        )
        .unwrap(),
        Risposta::Presenza(Presenza { ref nome, .. }) if nome == "Telefono"
    ));
}

#[test]
fn il_gettone_vale_una_volta_e_non_si_indovina() {
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    let (altro, _) = nodo("Altro");
    *del_pc.consente.lock().unwrap() = true;

    let invito = pc.invita().unwrap();
    // Un gettone sbagliato.
    let mut sbagliato = invito.clone();
    sbagliato.gettone[0] ^= 1;
    assert_eq!(
        codice(altro.collega(&sbagliato.link())),
        ErrorCodeKind::LontanoGettoneScaduto
    );
    // Quello giusto funziona ancora, una volta.
    telefono.collega(&invito.link()).expect("il primo");
    assert_eq!(
        codice(altro.collega(&invito.link())),
        ErrorCodeKind::LontanoGettoneScaduto,
        "usato"
    );
    // Un QR nuovo, e chi lo mostra dice di no.
    *del_pc.consente.lock().unwrap() = false;
    let nuovo = pc.invita().unwrap();
    assert_eq!(
        codice(altro.collega(&nuovo.link())),
        ErrorCodeKind::LontanoRifiutato
    );
    // Annullato, non vale più nemmeno da giusto.
    let annullato = pc.invita().unwrap();
    pc.annulla_invito();
    assert_eq!(
        codice(altro.collega(&annullato.link())),
        ErrorCodeKind::LontanoGettoneScaduto
    );
    // Il proprio link non collega a sé stessi.
    assert_eq!(
        codice(pc.collega(&pc.invita().unwrap().link())),
        ErrorCodeKind::LontanoLinkNonValido
    );
}

#[test]
fn cinque_gettoni_sbagliati_bruciano_il_qr() {
    // Un PC suo: con quelli della prova di sopra si passerebbero i dieci
    // tentativi al minuto, e l'ultimo lo fermerebbe il tetto, non il QR
    // bruciato.
    let (pc, del_pc) = nodo("PC");
    let (altro, _) = nodo("Altro");
    let bersaglio = pc.invita().unwrap();
    for n in 0..5_u8 {
        let mut tentato = bersaglio.clone();
        tentato.gettone[1] ^= n + 1;
        assert_eq!(
            codice(altro.collega(&tentato.link())),
            ErrorCodeKind::LontanoGettoneScaduto,
            "sbagliato {n}"
        );
    }
    assert!(!pc.invito_attivo(), "bruciato");
    *del_pc.consente.lock().unwrap() = true;
    assert_eq!(
        codice(altro.collega(&bersaglio.link())),
        ErrorCodeKind::LontanoGettoneScaduto,
        "bruciato dai tentativi"
    );
}

#[test]
fn un_qr_che_non_vale_piu_si_dice_sempre_scaduto() {
    // Senza un QR attivo il PC chiude l'associazione appena arriva, e la
    // chiusura corre con le prime domande di chi ha letto il QR: deve dire
    // sempre «QR scaduto», mai «non risponde», qualunque delle due arrivi
    // prima. (Oltre i tentativi al minuto si chiude allo stesso modo: vedi
    // `con_un_qr_a_schermo_il_tetto_dei_tentativi_resta`.)
    let (pc, _) = nodo("PC");
    let (altro, _) = nodo("Altro");
    let invito = pc.invita().unwrap();
    pc.annulla_invito();
    for tentativo in 0..14 {
        assert_eq!(
            codice(altro.collega(&invito.link())),
            ErrorCodeKind::LontanoGettoneScaduto,
            "tentativo {tentativo}"
        );
    }
}

#[test]
fn senza_un_qr_a_schermo_i_tentativi_non_si_contano() {
    // Reperto 02-12: il tetto dei tentativi si contava prima di guardare se
    // c'era un QR. Chi conosce la chiave del nodo (in casa la annuncia mDNS;
    // un dispositivo dimenticato la ricorda) consumava i tentativi del minuto
    // senza nessun QR a schermo, e il QR vero, mostrato subito dopo,
    // rispondeva «QR scaduto» a chi lo leggeva davvero.
    let (pc, del_pc) = nodo("PC");
    let (estraneo, _) = nodo("Estraneo");
    let (telefono, _) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    let vecchio = pc.invita().unwrap();
    pc.annulla_invito();
    for tentativo in 0..TENTATIVI_AL_MINUTO + 2 {
        assert_eq!(
            codice(estraneo.collega(&vecchio.link())),
            ErrorCodeKind::LontanoGettoneScaduto,
            "tentativo {tentativo}"
        );
    }
    let vero = pc.invita().unwrap();
    telefono
        .collega(&vero.link())
        .expect("il QR appena mostrato vale per chi lo legge");
}

#[test]
fn con_un_qr_a_schermo_il_tetto_dei_tentativi_resta() {
    let (pc, del_pc) = nodo("PC");
    let (altro, _) = nodo("Altro");
    let (telefono, _) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    // Gettoni sbagliati, al più quattro per QR: il quinto lo brucerebbe, e il
    // rifiuto verrebbe dal QR bruciato, non dal tetto.
    let mut invito = pc.invita().unwrap();
    for tentativo in 0..TENTATIVI_AL_MINUTO {
        if tentativo > 0 && tentativo % 4 == 0 {
            invito = pc.invita().unwrap();
        }
        let mut sbagliato = invito.clone();
        sbagliato.gettone[0] ^= u8::try_from(tentativo + 1).unwrap();
        assert_eq!(
            codice(altro.collega(&sbagliato.link())),
            ErrorCodeKind::LontanoGettoneScaduto,
            "sbagliato {tentativo}"
        );
    }
    assert!(pc.invito_attivo(), "l'ultimo QR vale ancora");
    // Il minuto è pieno: anche il gettone giusto resta fuori, e il QR non si
    // consuma.
    assert_eq!(
        codice(telefono.collega(&invito.link())),
        ErrorCodeKind::LontanoGettoneScaduto,
        "oltre il tetto"
    );
    assert!(pc.invito_attivo(), "il tetto non consuma il QR");
}

#[test]
fn chi_non_e_associato_resta_fuori() {
    let (pc, _) = nodo("PC");
    let (estraneo, _) = nodo("Estraneo");
    estraneo.pari(vec![PariNoto {
        chiave: pc.chiave(),
        indirizzi: pc.socket(),
    }]);
    assert_eq!(
        codice(estraneo.chiedi(&pc.chiave(), Domanda::Presenza, Duration::from_secs(10))),
        ErrorCodeKind::LontanoNonAssociato
    );
}

#[test]
fn la_presenza_arriva_da_sola_e_cambia() {
    let (pc, del_pc) = nodo("PC");
    let (telefono, del_telefono) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    telefono.collega(&pc.invita().unwrap().link()).unwrap();
    pc.pari(vec![PariNoto {
        chiave: telefono.chiave(),
        indirizzi: telefono.socket(),
    }]);
    telefono.pari(vec![PariNoto {
        chiave: pc.chiave(),
        indirizzi: pc.socket(),
    }]);
    let presente = |finta: &Finta, chiave: &str, acceso: bool| {
        finta.eventi.lock().unwrap().iter().any(|evento| {
            matches!(evento, Evento::Presente { chiave: c, presenza }
                if c == chiave && presenza.insieme_acceso == acceso)
        })
    };
    assert!(aspetta(|| presente(&del_telefono, &pc.chiave(), false)));
    assert!(aspetta(|| presente(&del_pc, &telefono.chiave(), false)));
    assert!(telefono.raggiungibile(&pc.chiave()));

    // Il PC accende «Ascolta insieme»: il telefono lo sa senza chiedere.
    pc.presenza(Presenza {
        nome: "PC".to_owned(),
        insieme_acceso: true,
        ..Presenza::default()
    });
    assert!(aspetta(|| presente(&del_telefono, &pc.chiave(), true)));

    // Il PC si spegne: il telefono lo vede andare.
    let chiave_pc = pc.chiave();
    drop(pc);
    assert!(aspetta(|| {
        del_telefono
            .eventi
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Evento::Assente { chiave } if *chiave == chiave_pc))
    }));
}

#[test]
fn dimenticami_toglie_l_associazione_dall_altra_parte() {
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    telefono.collega(&pc.invita().unwrap().link()).unwrap();
    assert_eq!(
        telefono
            .chiedi(&pc.chiave(), Domanda::Dimenticami, Duration::from_secs(10))
            .unwrap(),
        Risposta::Fatto
    );
    assert_eq!(*del_pc.dimenticati.lock().unwrap(), vec![telefono.chiave()]);
    assert!(aspetta(|| !telefono.raggiungibile(&pc.chiave())));
    assert_eq!(
        codice(telefono.chiedi(&pc.chiave(), Domanda::Presenza, Duration::from_secs(10))),
        ErrorCodeKind::LontanoNonAssociato
    );
}

#[test]
fn un_flusso_di_insieme_porta_i_byte_nei_due_versi_e_finisce() {
    use std::io::{Read as _, Write as _};

    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    let invito = pc.invita().unwrap();
    telefono.collega(&invito.link()).expect("associati");

    // Chi apre parla per primo: il flusso arriva all'altro coi primi byte.
    let mut di_qua = telefono
        .apri_insieme(&pc.chiave(), Duration::from_secs(10))
        .expect("il flusso si apre");
    di_qua.set_write_timeout(Some(Duration::from_secs(5)));
    di_qua.write_all(b"ciao").unwrap();
    assert!(
        aspetta(|| !del_pc.flussi.lock().unwrap().is_empty()),
        "il PC riceve il flusso"
    );
    let (da, mut di_la) = del_pc.flussi.lock().unwrap().pop().unwrap();
    assert_eq!(da, telefono.chiave(), "chi apre è chi dice di essere");
    assert_eq!(di_la.chiave_remota(), di_qua_chiave(&telefono));

    di_la.set_read_timeout(Some(Duration::from_secs(5)));
    let mut letti = [0_u8; 4];
    di_la.read_exact(&mut letti).unwrap();
    assert_eq!(&letti, b"ciao");

    // E indietro.
    di_la.set_write_timeout(Some(Duration::from_secs(5)));
    di_la.write_all(b"pronto").unwrap();
    di_qua.set_read_timeout(Some(Duration::from_secs(5)));
    let mut risposta = [0_u8; 6];
    di_qua.read_exact(&mut risposta).unwrap();
    assert_eq!(&risposta, b"pronto");

    // Una lettura senza niente da leggere scade, come un socket.
    di_qua.set_read_timeout(Some(Duration::from_millis(200)));
    let scaduta = di_qua.read(&mut risposta).unwrap_err();
    assert_eq!(scaduta.kind(), std::io::ErrorKind::TimedOut);

    // Chiusa la metà in scrittura, l'altro legge la fine.
    di_qua.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(di_la.read(&mut letti).unwrap(), 0);

    // Tagliato da una copia, chi aspetta si sveglia.
    let copia = di_la.try_clone();
    copia.shutdown(std::net::Shutdown::Both).unwrap();
    di_qua.set_read_timeout(Some(Duration::from_secs(5)));
    assert!(di_qua.read(&mut risposta).is_err() || di_qua.read(&mut risposta).unwrap() == 0);
}

/// La chiave del telefono in byte, come la vede chi riceve il flusso.
fn di_qua_chiave(nodo: &Nodo) -> [u8; 32] {
    let testo = nodo.chiave();
    let mut byte = [0_u8; 32];
    for (i, coppia) in testo.as_bytes().chunks(2).enumerate() {
        byte[i] = u8::from_str_radix(std::str::from_utf8(coppia).unwrap(), 16).unwrap();
    }
    byte
}

#[test]
fn un_flusso_di_insieme_non_si_apre_verso_chi_non_e_associato() {
    let (pc, _) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    assert_eq!(
        codice(telefono.apri_insieme(&pc.chiave(), Duration::from_secs(5))),
        ErrorCodeKind::LontanoNonAssociato
    );
}

/// Quante volte il QR ha smesso di valere, dagli eventi.
fn inviti_finiti(finta: &Finta) -> usize {
    finta
        .eventi
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, Evento::InvitoFinito))
        .count()
}

#[test]
fn un_qr_che_smette_di_valere_si_dice() {
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    let (altro, _) = nodo("Altro");

    // Usato, anche se poi chi lo mostra dice di no: il QR non vale più, e
    // chi lo mostra lo deve togliere dallo schermo.
    let invito = pc.invita().unwrap();
    assert!(pc.invito_attivo());
    assert_eq!(
        codice(telefono.collega(&invito.link())),
        ErrorCodeKind::LontanoRifiutato
    );
    assert_eq!(inviti_finiti(&del_pc), 1);
    assert!(!pc.invito_attivo());

    // Un gettone sbagliato non lo finisce; il quinto sì.
    let bersaglio = pc.invita().unwrap();
    for n in 0..4_u8 {
        let mut tentato = bersaglio.clone();
        tentato.gettone[2] ^= n + 1;
        let _ = altro.collega(&tentato.link());
    }
    assert_eq!(inviti_finiti(&del_pc), 1, "quattro sbagliati: vale ancora");
    assert!(pc.invito_attivo());
    let mut quinto = bersaglio.clone();
    quinto.gettone[3] ^= 1;
    let _ = altro.collega(&quinto.link());
    assert_eq!(inviti_finiti(&del_pc), 2, "il quinto lo brucia");
    assert!(!pc.invito_attivo());
}

#[test]
fn chi_ha_letto_il_qr_puo_rinunciare_e_l_altro_toglie_la_domanda() {
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    *del_pc.tentenna.lock().unwrap() = true;
    let invito = pc.invita().unwrap();

    let telefono = Arc::new(telefono);
    let in_attesa = {
        let telefono = Arc::clone(&telefono);
        let link = invito.link();
        std::thread::spawn(move || telefono.collega(&link))
    };
    // Si annulla solo quando il telefono aspetta davvero la risposta: il QR è
    // già consumato dall'altra parte.
    assert!(aspetta(|| inviti_finiti(&del_pc) == 1));
    std::thread::sleep(Duration::from_millis(200));
    let partito = Instant::now();
    telefono.annulla_collegamento();
    let esito = in_attesa.join().unwrap();
    assert_eq!(codice(esito), ErrorCodeKind::InternalAborted);
    assert!(
        partito.elapsed() < Duration::from_secs(5),
        "l'annullo non aspetta la scadenza"
    );
    assert!(
        aspetta(|| *del_pc.ritirate.lock().unwrap() == vec![telefono.chiave()]),
        "chi mostrava il QR sa che la domanda è ritirata"
    );
    assert!(
        del_pc.nuovi.lock().unwrap().is_empty(),
        "nessuno è associato"
    );
    // Senza un'associazione in corso, annullare non fa niente.
    telefono.annulla_collegamento();
}

#[test]
fn si_sa_se_si_passa_diritti_o_dal_relay() {
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    telefono.collega(&pc.invita().unwrap().link()).unwrap();
    assert_eq!(telefono.via(&pc.chiave()), None, "nessuna connessione viva");
    telefono
        .chiedi(&pc.chiave(), Domanda::Presenza, Duration::from_secs(10))
        .unwrap();
    // Solo `127.0.0.1` e nessun relay: diritti.
    assert!(aspetta(|| matches!(
        telefono.via(&pc.chiave()),
        Some((rymembra_lontano::Via::Diretta, _))
    )));
    // E chiedere di riprovare non rompe niente.
    telefono.riprova_adesso();
    telefono
        .chiedi(&pc.chiave(), Domanda::Presenza, Duration::from_secs(10))
        .unwrap();
}
