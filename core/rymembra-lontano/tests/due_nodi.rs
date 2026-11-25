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
use rymembra_lontano::protocollo::{
    CAPACITA_CATALOGO, CAPACITA_TESTI, SCRITTURE_AL_MINUTO, ScritturaTesto,
};
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
    /// Le domande arrivate a `rispondi`, nell'ordine.
    viste: Mutex<Vec<Domanda>>,
    /// Cosa fa chi ha una build di prima: le domande nuove gli arrivano
    /// sconosciute, e rifiuta. Non le risponderebbe mai, anche se glien'ar-
    /// rivasse una.
    vecchia: Mutex<bool>,
    /// Un pari ostile: a ogni lettura di byte e a ogni copertina risponde con
    /// tanti byte, quanti che siano chiesti.
    byte_a_forza: Mutex<Option<usize>>,
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
        self.viste.lock().unwrap().push(domanda.clone());
        if let Some(quanti) = *self.byte_a_forza.lock().unwrap()
            && matches!(domanda, Domanda::Leggi { .. } | Domanda::Copertina { .. })
        {
            return Ok(Risposta::Byte {
                byte: vec![7; quanti],
                totale: u64::try_from(quanti).unwrap(),
                versione: "v1".to_owned(),
            });
        }
        match domanda {
            // Una build di prima non conosce le domande nuove: per lei sono
            // `Sconosciuta`, e il nodo rifiuta prima che arrivino qui.
            Domanda::CatalogoRiassunto
            | Domanda::CatalogoSecchio { .. }
            | Domanda::Valuta { .. }
            | Domanda::Testo { .. }
            | Domanda::TestoScrivi { .. }
                if *self.vecchia.lock().unwrap() =>
            {
                Err(AppError::new(ErrorCode::LontanoProtocollo))
            }
            // Un brano che non è condiviso non ha un testo da dare.
            Domanda::Testo { brano: 404 } => Err(AppError::new(ErrorCode::LontanoFileCambiato)),
            Domanda::Testo { brano } => Ok(Risposta::Dati(serde_json::json!({
                "sincronizzato": format!("[00:01.00]brano {brano}\n"),
                "piatto": null,
                "strumentale": false,
                "fonte": "mano",
                "scartoMs": 120,
                "cercato": true,
            }))),
            Domanda::TestoScrivi { .. } => Ok(Risposta::Fatto),
            Domanda::CatalogoRiassunto => Ok(Risposta::Dati(
                serde_json::json!({ "generazione": 7, "secchi": [] }),
            )),
            Domanda::CatalogoSecchio { da_id, a_id } => Ok(Risposta::Dati(
                serde_json::json!({ "generazione": 7, "daId": da_id, "aId": a_id, "brani": [] }),
            )),
            Domanda::Valuta { .. } => Ok(Risposta::Fatto),
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
    nodo_con(nome, &[])
}

/// Un nodo che dichiara le capacità `capacita`: nessuna è una build di prima.
fn nodo_con(nome: &str, capacita: &[&str]) -> (Nodo, Arc<Finta>) {
    let finta = Arc::new(Finta::default());
    *finta.vecchia.lock().unwrap() = capacita.is_empty();
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
                capacita: capacita.iter().map(|c| (*c).to_owned()).collect(),
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

/// Due PC, e un terzo: il link copiato dall'uno e incollato sull'altro collega
/// come una scansione (F4, richiesta #2). Dichiarano tutti la stessa
/// piattaforma (qui `prova`, in produzione `windows`: per il nodo è una
/// stringa), e si distinguono solo per il nome. La chiave di «Ascolta insieme»
/// di ciascuno arriva all'altro nella stretta di mano —
/// `Presentazione.chiave_insieme` — e non nel link, che porta il nome, la chiave
/// del nodo, il gettone, gli indirizzi e il relay.
#[test]
fn due_pc_si_collegano_col_link_e_si_distinguono_dal_nome() {
    let (studio, dello_studio) = nodo("STUDIO");
    let (salotto, del_salotto) = nodo("SALOTTO");
    let (portatile, del_portatile) = nodo("PORTATILE");
    *dello_studio.consente.lock().unwrap() = true;

    // Il link dello STUDIO, com'esce da «Copia il link».
    let invito = studio.invita().unwrap();
    let link = invito.link();
    let letto = rymembra_lontano::Invito::da_link(&link).unwrap();
    assert_eq!(letto.nome.as_deref(), Some("STUDIO"));
    assert_eq!(letto.chiave, invito.chiave);
    // La chiave di «Ascolta insieme» non sta nel link, né come testo né nella
    // forma letta.
    assert!(!link.contains("insieme-STUDIO"));
    assert!(!format!("{letto:?}").contains("insieme-"));

    // Il SALOTTO lo incolla.
    let (chiave, chi) = salotto.collega(&link).expect("il salotto si collega");
    assert_eq!(chiave, studio.chiave());
    assert_eq!(chi.nome, "STUDIO");
    assert_eq!(chi.piattaforma, "prova");
    // Nella stretta di mano: la chiave dello STUDIO al salotto...
    assert_eq!(chi.chiave_insieme.as_deref(), Some("insieme-STUDIO"));
    // ... e quella del SALOTTO allo STUDIO.
    let visti = dello_studio.nuovi.lock().unwrap().clone();
    assert_eq!(visti.len(), 1);
    assert_eq!(visti[0].0, salotto.chiave());
    assert_eq!(visti[0].1.nome, "SALOTTO");
    assert_eq!(visti[0].1.piattaforma, "prova");
    assert_eq!(
        visti[0].1.chiave_insieme.as_deref(),
        Some("insieme-SALOTTO")
    );
    assert!(del_salotto.associato(&studio.chiave()));

    // Un link nuovo, e il PORTATILE: lo STUDIO ha due dispositivi, uguali nella
    // piattaforma e diversi nel nome. Il link di prima, nel frattempo usato,
    // non vale più.
    let secondo = studio.invita().unwrap();
    assert_eq!(
        codice(portatile.collega(&link)),
        ErrorCodeKind::LontanoGettoneScaduto,
        "il link di prima non collega più"
    );
    portatile
        .collega(&secondo.link())
        .expect("il portatile si collega");
    let visti = dello_studio.nuovi.lock().unwrap().clone();
    assert_eq!(visti.len(), 2);
    let nomi: HashSet<&str> = visti.iter().map(|(_, chi)| chi.nome.as_str()).collect();
    assert_eq!(nomi, HashSet::from(["SALOTTO", "PORTATILE"]));
    assert!(del_portatile.associato(&studio.chiave()));

    // E si parlano, in tutte e due le direzioni.
    for (chi, a) in [
        (&salotto, &studio),
        (&portatile, &studio),
        (&studio, &portatile),
    ] {
        assert!(matches!(
            chi.chiedi(&a.chiave(), Domanda::Presenza, Duration::from_secs(10))
                .unwrap(),
            Risposta::Presenza(Presenza { ref piattaforma, .. }) if piattaforma == "prova"
        ));
    }
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
fn i_gettoni_sbagliati_non_bruciano_il_qr() {
    // Fino al reperto R01-3 cinque gettoni sbagliati, da chiunque, toglievano
    // il QR. Il gettone è di 128 bit: sbagliarlo non avvicina a niente, e il
    // QR resta per chi lo inquadra davvero — anche per chi ha sbagliato.
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
    assert!(pc.invito_attivo(), "il QR vale ancora");
    *del_pc.consente.lock().unwrap() = true;
    altro
        .collega(&bersaglio.link())
        .expect("col gettone giusto ci si collega");
}

#[test]
fn un_estraneo_non_brucia_il_qr_di_un_altro() {
    // Reperto R01-3: cinque gettoni sbagliati da chiunque bruciavano il QR, e
    // i dieci tentativi al minuto erano di tutti. Chi in casa legge la chiave
    // del nodo da mDNS rendeva «scaduto» ogni QR appena compariva.
    let (pc, del_pc) = nodo("PC");
    let (estraneo, _) = nodo("Estraneo");
    let (telefono, _) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    let vero = pc.invita().unwrap();
    for tentativo in 0..TENTATIVI_AL_MINUTO + 2 {
        let mut sbagliato = vero.clone();
        sbagliato.gettone[0] ^= u8::try_from(tentativo + 1).unwrap();
        assert_eq!(
            codice(estraneo.collega(&sbagliato.link())),
            ErrorCodeKind::LontanoGettoneScaduto,
            "sbagliato {tentativo}"
        );
    }
    assert!(pc.invito_attivo(), "il QR vale ancora per chi lo legge");
    telefono
        .collega(&vero.link())
        .expect("chi inquadra il QR vero si collega");
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
fn con_un_qr_a_schermo_il_tetto_dei_tentativi_resta_per_chi_bussa() {
    let (pc, del_pc) = nodo("PC");
    let (altro, _) = nodo("Altro");
    let (telefono, _) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    let invito = pc.invita().unwrap();
    for tentativo in 0..TENTATIVI_AL_MINUTO {
        let mut sbagliato = invito.clone();
        sbagliato.gettone[0] ^= u8::try_from(tentativo + 1).unwrap();
        assert_eq!(
            codice(altro.collega(&sbagliato.link())),
            ErrorCodeKind::LontanoGettoneScaduto,
            "sbagliato {tentativo}"
        );
    }
    assert!(pc.invito_attivo(), "il QR vale ancora");
    // Il suo minuto è pieno: anche col gettone giusto resta fuori, e il QR
    // non si consuma.
    assert_eq!(
        codice(altro.collega(&invito.link())),
        ErrorCodeKind::LontanoGettoneScaduto,
        "oltre il tetto"
    );
    assert!(pc.invito_attivo(), "il tetto non consuma il QR");
    // Il minuto degli altri no (reperto R01-3).
    telefono
        .collega(&invito.link())
        .expect("chi non ha bussato ha i suoi tentativi");
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
fn la_presenza_dell_altro_arriva_nei_tetti() {
    // Reperto 02-13, dalla risposta: chi chiede la presenza la riceve
    // tagliata, quanto che l'altro dica di sé — fin dove sta nei 16 KiB che una
    // risposta di presenza può pesare (S1-2): oltre, è un guasto di protocollo,
    // provato in `rymembra_lontano::protocollo`.
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    telefono.collega(&pc.invita().unwrap().link()).unwrap();
    pc.presenza(Presenza {
        nome: "PC ".repeat(3_000),
        piattaforma: "prova".to_owned(),
        versione: format!("3.2.0\u{202E}{}", "9".repeat(5_000)),
        ..Presenza::default()
    });
    let Risposta::Presenza(presenza) = telefono
        .chiedi(&pc.chiave(), Domanda::Presenza, Duration::from_secs(10))
        .unwrap()
    else {
        panic!("una presenza");
    };
    assert!(
        presenza.nome.chars().count() <= 64,
        "{}",
        presenza.nome.len()
    );
    assert!(presenza.nome.starts_with("PC PC"));
    assert!(
        presenza.versione.chars().count() <= 64,
        "{}",
        presenza.versione.len()
    );
    assert!(!presenza.versione.contains('\u{202E}'));
    assert_eq!(presenza.piattaforma, "prova");
}

#[test]
fn una_presenza_oltre_il_suo_tetto_e_un_guasto_di_protocollo() {
    // Reperto S1-2: la risposta a `Presenza` arriva a ogni connessione, e con il
    // tetto delle risposte di dati (8 MiB) un pari ne faceva costruire a serde
    // centinaia di mebibyte. Oltre i 16 KiB non si apre: né tagliata né altro.
    let (pc, del_pc) = nodo("PC");
    let (telefono, _) = nodo("Telefono");
    *del_pc.consente.lock().unwrap() = true;
    telefono.collega(&pc.invita().unwrap().link()).unwrap();
    pc.presenza(Presenza {
        nome: "PC ".repeat(50_000),
        piattaforma: "prova".to_owned(),
        ..Presenza::default()
    });
    match telefono.chiedi(&pc.chiave(), Domanda::Presenza, Duration::from_secs(10)) {
        Err(err) => assert_eq!(err.code().kind(), ErrorCodeKind::LontanoProtocollo),
        Ok(Risposta::Presenza(presenza)) => {
            panic!("una presenza da {} caratteri si apre", presenza.nome.len())
        }
        Ok(altra) => panic!("{altra:?}"),
    }
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

    // I gettoni sbagliati non lo finiscono, quanti che siano (reperto R01-3).
    let bersaglio = pc.invita().unwrap();
    for n in 0..5_u8 {
        let mut tentato = bersaglio.clone();
        tentato.gettone[2] ^= n + 1;
        let _ = altro.collega(&tentato.link());
    }
    assert_eq!(inviti_finiti(&del_pc), 1, "cinque sbagliati: vale ancora");
    assert!(pc.invito_attivo());
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
fn chiuso_mentre_si_collega_il_comando_finisce_spento() {
    // Reperto 02-7: il `collega` in attesa del sì teneva il nodo acceso dopo
    // l'interruttore spento. Chiuso, il comando torna subito, e con un errore
    // che dice perché: non «annullato» (che la finestra tace), non «non
    // risponde».
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
    assert!(aspetta(|| inviti_finiti(&del_pc) == 1));
    std::thread::sleep(Duration::from_millis(200));
    let partito = Instant::now();
    telefono.chiudi();
    let esito = in_attesa.join().unwrap();
    assert_eq!(codice(esito), ErrorCodeKind::LontanoSpento);
    assert!(
        partito.elapsed() < Duration::from_secs(5),
        "non aspetta la scadenza: {:?}",
        partito.elapsed()
    );
    assert!(
        aspetta(|| *del_pc.ritirate.lock().unwrap() == vec![telefono.chiave()]),
        "chi mostrava il QR toglie la domanda"
    );
    assert!(
        del_pc.nuovi.lock().unwrap().is_empty(),
        "nessuno è associato"
    );
    assert_eq!(
        codice(telefono.collega(&pc.invita().unwrap().link())),
        ErrorCodeKind::LontanoSpento,
        "e un comando dopo lo trova spento"
    );
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

// ── la libreria unica: capacità, versioni miste, scritture ─────────────────

/// Associa `a` e `b` (chi mostra il QR è `b`) e fa che si seguano e si
/// conoscano: ognuno ha la presenza dell'altro.
fn collegati(a: &Nodo, b: &Nodo, di_b: &Finta, di_a: &Finta) {
    *di_b.consente.lock().unwrap() = true;
    a.collega(&b.invita().unwrap().link()).unwrap();
    b.pari(vec![PariNoto {
        chiave: a.chiave(),
        indirizzi: a.socket(),
    }]);
    a.pari(vec![PariNoto {
        chiave: b.chiave(),
        indirizzi: b.socket(),
    }]);
    let presenze = |finta: &Finta, chiave: &str| {
        finta
            .eventi
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Evento::Presente { chiave: c, .. } if c == chiave))
    };
    assert!(aspetta(|| presenze(di_a, &b.chiave())), "a conosce b");
    assert!(aspetta(|| presenze(di_b, &a.chiave())), "b conosce a");
}

fn nuove_domande() -> Vec<Domanda> {
    vec![
        Domanda::CatalogoRiassunto,
        Domanda::CatalogoSecchio {
            da_id: 1,
            a_id: 500,
        },
        Domanda::Valuta {
            brano: 1,
            liked: Some(true),
            rating: Some(5),
            quando_ms: 1,
            liked_ms: None,
            rating_ms: None,
            chiave: "v".to_owned(),
        },
        Domanda::Testo { brano: 1 },
        Domanda::TestoScrivi {
            brano: 1,
            quando_ms: 0,
            cosa: ScritturaTesto::Scarto { scarto_ms: 100 },
        },
    ]
}

#[test]
fn a_un_pari_di_prima_le_domande_nuove_non_si_fanno_e_quelle_di_sempre_si() {
    // Sul telefono e sul PC dell'utente gira una build che non dichiara
    // `catalogo`: qui un nodo nuovo parla con uno così.
    let (nuovo, del_nuovo) = nodo_con("Nuovo", &[CAPACITA_CATALOGO, CAPACITA_TESTI]);
    let (vecchio, del_vecchio) = nodo("Vecchio");
    collegati(&nuovo, &vecchio, &del_vecchio, &del_nuovo);
    del_vecchio.viste.lock().unwrap().clear();

    // Il nodo sa cosa dichiara l'altro.
    assert!(!nuovo.dichiara(&vecchio.chiave(), CAPACITA_CATALOGO));
    assert!(!nuovo.dichiara(&vecchio.chiave(), CAPACITA_TESTI));
    assert!(vecchio.dichiara(&nuovo.chiave(), CAPACITA_CATALOGO));
    assert!(vecchio.dichiara(&nuovo.chiave(), CAPACITA_TESTI));

    for domanda in nuove_domande() {
        // Rifiutata in locale con lo stesso codice con cui risponderebbe una
        // build di prima, e senza mandare niente.
        assert_eq!(
            codice(nuovo.chiedi(&vecchio.chiave(), domanda.clone(), Duration::from_secs(10))),
            ErrorCodeKind::LontanoProtocollo,
            "{domanda:?}"
        );
    }
    assert!(
        del_vecchio.viste.lock().unwrap().is_empty(),
        "al pari di prima non è arrivata nessuna domanda nuova: {:?}",
        del_vecchio.viste.lock().unwrap()
    );

    // Le domande di oggi continuano ad avere risposta, nei due versi.
    for (chi, a_chi) in [(&nuovo, &vecchio), (&vecchio, &nuovo)] {
        let Risposta::Dati(dati) = chi
            .chiedi(
                &a_chi.chiave(),
                Domanda::Album { da: 0, quanti: 20 },
                Duration::from_secs(10),
            )
            .unwrap()
        else {
            panic!("dei dati");
        };
        assert_eq!(dati["quanti"], 20);
    }
    assert!(matches!(
        nuovo
            .chiedi(
                &vecchio.chiave(),
                Domanda::Leggi {
                    brano: 1,
                    da: 0,
                    quanti: 10
                },
                Duration::from_secs(10)
            )
            .unwrap(),
        Risposta::Byte { .. }
    ));
    // E il pari di prima che le domande nuove le riceverebbe lo stesso, per
    // errore, le rifiuta e la connessione resta in piedi.
    assert!(vecchio.raggiungibile(&nuovo.chiave()) || nuovo.raggiungibile(&vecchio.chiave()));
}

#[test]
fn fra_due_pari_che_dichiarano_il_catalogo_le_domande_nuove_passano() {
    let (a, di_a) = nodo_con("A", &[CAPACITA_CATALOGO]);
    let (b, di_b) = nodo_con("B", &[CAPACITA_CATALOGO]);
    collegati(&a, &b, &di_b, &di_a);
    assert!(a.dichiara(&b.chiave(), CAPACITA_CATALOGO));
    assert!(b.dichiara(&a.chiave(), CAPACITA_CATALOGO));
    di_b.viste.lock().unwrap().clear();
    let Risposta::Dati(riassunto) = a
        .chiedi(
            &b.chiave(),
            Domanda::CatalogoRiassunto,
            Duration::from_secs(10),
        )
        .unwrap()
    else {
        panic!("un riassunto");
    };
    assert_eq!(riassunto["generazione"], 7);
    assert_eq!(
        a.chiedi(
            &b.chiave(),
            Domanda::Valuta {
                brano: 1,
                liked: Some(true),
                rating: Some(4),
                quando_ms: 10,
                liked_ms: None,
                rating_ms: None,
                chiave: "v1".to_owned()
            },
            Duration::from_secs(10)
        )
        .unwrap(),
        Risposta::Fatto
    );
    assert_eq!(di_b.viste.lock().unwrap().len(), 2);
}

#[test]
fn senza_aver_visto_la_presenza_le_domande_nuove_non_partono() {
    // Prima che l'altro si sia presentato non si sa cosa sa fare: niente
    // domande nuove, e nessuna sorpresa a un pari di prima.
    let (a, _) = nodo_con("A", &[CAPACITA_CATALOGO]);
    let (b, di_b) = nodo_con("B", &[CAPACITA_CATALOGO]);
    *di_b.consente.lock().unwrap() = true;
    a.collega(&b.invita().unwrap().link()).unwrap();
    // Nessuno segue nessuno: nessuna presenza scambiata.
    assert_eq!(
        codice(a.chiedi(
            &b.chiave(),
            Domanda::CatalogoRiassunto,
            Duration::from_secs(10)
        )),
        ErrorCodeKind::LontanoProtocollo
    );
    assert!(di_b.viste.lock().unwrap().is_empty());
    // Una volta presentati, passa.
    a.chiedi(&b.chiave(), Domanda::Presenza, Duration::from_secs(10))
        .unwrap();
    a.pari(vec![PariNoto {
        chiave: b.chiave(),
        indirizzi: b.socket(),
    }]);
    assert!(aspetta(|| a.dichiara(&b.chiave(), CAPACITA_CATALOGO)));
    a.chiedi(
        &b.chiave(),
        Domanda::CatalogoRiassunto,
        Duration::from_secs(10),
    )
    .unwrap();
}

#[test]
fn le_scritture_di_un_pari_hanno_un_tetto_al_minuto_e_gli_altri_non_ne_risentono() {
    let (server, del_server) = nodo_con("Server", &[CAPACITA_CATALOGO]);
    let (uno, di_uno) = nodo_con("Uno", &[CAPACITA_CATALOGO]);
    let (due, di_due) = nodo_con("Due", &[CAPACITA_CATALOGO]);
    collegati(&uno, &server, &del_server, &di_uno);
    collegati(&due, &server, &del_server, &di_due);
    let valuta = |n: usize| Domanda::Valuta {
        brano: 1,
        liked: Some(n.is_multiple_of(2)),
        rating: Some(3),
        quando_ms: i64::try_from(n).unwrap(),
        liked_ms: None,
        rating_ms: None,
        chiave: format!("v{n}"),
    };
    for n in 0..SCRITTURE_AL_MINUTO {
        uno.chiedi(&server.chiave(), valuta(n), Duration::from_secs(10))
            .unwrap_or_else(|err| panic!("la scrittura {n}: {err:?}"));
    }
    // Oltre il tetto: `lontano.irraggiungibile`, che fa tenere la scrittura in
    // coda e riprovare dopo, e al server non arriva.
    let prima = del_server.viste.lock().unwrap().len();
    assert_eq!(
        codice(uno.chiedi(&server.chiave(), valuta(999), Duration::from_secs(10))),
        ErrorCodeKind::LontanoIrraggiungibile
    );
    assert_eq!(del_server.viste.lock().unwrap().len(), prima);
    // Un altro dispositivo ha il suo conto.
    assert_eq!(
        due.chiedi(&server.chiave(), valuta(1), Duration::from_secs(10))
            .unwrap(),
        Risposta::Fatto
    );
    // E le letture del catalogo di chi è al tetto non sono scritture.
    uno.chiedi(
        &server.chiave(),
        Domanda::CatalogoRiassunto,
        Duration::from_secs(10),
    )
    .unwrap();
}

#[test]
fn a_un_pari_col_catalogo_ma_senza_i_testi_le_domande_dei_testi_non_si_fanno() {
    // Una build di mezzo: sa il catalogo e non i testi. Ha la sua capacità
    // ogni famiglia di domande, e a lei i testi non si chiedono.
    let (nuovo, del_nuovo) = nodo_con("Nuovo", &[CAPACITA_CATALOGO, CAPACITA_TESTI]);
    let (di_mezzo, del_di_mezzo) = nodo_con("DiMezzo", &[CAPACITA_CATALOGO]);
    collegati(&nuovo, &di_mezzo, &del_di_mezzo, &del_nuovo);
    assert!(nuovo.dichiara(&di_mezzo.chiave(), CAPACITA_CATALOGO));
    assert!(!nuovo.dichiara(&di_mezzo.chiave(), CAPACITA_TESTI));
    del_di_mezzo.viste.lock().unwrap().clear();

    for domanda in [
        Domanda::Testo { brano: 1 },
        Domanda::TestoScrivi {
            brano: 1,
            quando_ms: 0,
            cosa: ScritturaTesto::Rifiuta,
        },
    ] {
        assert_eq!(
            codice(nuovo.chiedi(&di_mezzo.chiave(), domanda.clone(), Duration::from_secs(10))),
            ErrorCodeKind::LontanoProtocollo,
            "{domanda:?}"
        );
    }
    assert!(del_di_mezzo.viste.lock().unwrap().is_empty());
    // Il catalogo, invece, passa.
    nuovo
        .chiedi(
            &di_mezzo.chiave(),
            Domanda::CatalogoRiassunto,
            Duration::from_secs(10),
        )
        .unwrap();
}

#[test]
fn fra_due_pari_che_dichiarano_i_testi_le_due_domande_passano() {
    let (a, di_a) = nodo_con("A", &[CAPACITA_CATALOGO, CAPACITA_TESTI]);
    let (b, di_b) = nodo_con("B", &[CAPACITA_CATALOGO, CAPACITA_TESTI]);
    collegati(&a, &b, &di_b, &di_a);
    assert!(a.dichiara(&b.chiave(), CAPACITA_TESTI));
    di_b.viste.lock().unwrap().clear();

    // Il testo già composto da chi serve.
    let Risposta::Dati(testo) = a
        .chiedi(
            &b.chiave(),
            Domanda::Testo { brano: 7 },
            Duration::from_secs(10),
        )
        .unwrap()
    else {
        panic!("un testo");
    };
    assert_eq!(testo["fonte"], "mano");
    assert_eq!(testo["scartoMs"], 120);
    assert_eq!(testo["sincronizzato"], "[00:01.00]brano 7\n");
    // Un brano che non c'è (o non è condiviso) lo dice come ogni altra lettura.
    assert_eq!(
        codice(a.chiedi(
            &b.chiave(),
            Domanda::Testo { brano: 404 },
            Duration::from_secs(10)
        )),
        ErrorCodeKind::LontanoFileCambiato
    );

    // E la correzione, in ognuna delle sue forme.
    for cosa in [
        ScritturaTesto::Scarto { scarto_ms: -90 },
        ScritturaTesto::Mano {
            lrc: "[00:01.00]la cura\n".to_owned(),
        },
        ScritturaTesto::Scelta {
            id_catalogo: 9,
            piatto: Some("la cura".to_owned()),
            sincronizzato: None,
            strumentale: false,
        },
        ScritturaTesto::Rifiuta,
    ] {
        assert_eq!(
            a.chiedi(
                &b.chiave(),
                Domanda::TestoScrivi {
                    brano: 7,
                    cosa,
                    quando_ms: 0,
                },
                Duration::from_secs(10)
            )
            .unwrap(),
            Risposta::Fatto
        );
    }
    assert_eq!(di_b.viste.lock().unwrap().len(), 2 + 4);
}

#[test]
fn le_correzioni_dei_testi_sono_scritture_e_hanno_il_tetto_al_minuto() {
    let (server, del_server) = nodo_con("Server", &[CAPACITA_CATALOGO, CAPACITA_TESTI]);
    let (uno, di_uno) = nodo_con("Uno", &[CAPACITA_CATALOGO, CAPACITA_TESTI]);
    collegati(&uno, &server, &del_server, &di_uno);
    let correggi = |n: usize| Domanda::TestoScrivi {
        brano: 1,
        quando_ms: 0,
        cosa: ScritturaTesto::Scarto {
            scarto_ms: i32::try_from(n).unwrap(),
        },
    };
    for n in 0..SCRITTURE_AL_MINUTO {
        uno.chiedi(&server.chiave(), correggi(n), Duration::from_secs(10))
            .unwrap_or_else(|err| panic!("la correzione {n}: {err:?}"));
    }
    // Il tetto è uno solo per tutte le scritture: oltre, anche un voto sente
    // `lontano.irraggiungibile`, e la correzione resta in posta.
    let prima = del_server.viste.lock().unwrap().len();
    assert_eq!(
        codice(uno.chiedi(&server.chiave(), correggi(999), Duration::from_secs(10))),
        ErrorCodeKind::LontanoIrraggiungibile
    );
    assert_eq!(del_server.viste.lock().unwrap().len(), prima);
    // Le letture di un testo non sono scritture.
    uno.chiedi(
        &server.chiave(),
        Domanda::Testo { brano: 1 },
        Duration::from_secs(10),
    )
    .unwrap();
}

/// Come finisce una domanda che aspetta dei byte: i byte letti, o il guasto. Un
/// `Ok` si dice senza stampare i byte, che possono essere megabyte.
fn byte_letti(esito: Result<Risposta, AppError>) -> Result<usize, ErrorCodeKind> {
    match esito {
        Ok(Risposta::Byte { byte, .. }) => Ok(byte.len()),
        Ok(altra) => panic!("non dei byte: {altra:?}"),
        Err(err) => Err(err.code().kind()),
    }
}

#[test]
fn una_risposta_di_byte_oltre_quanto_si_e_chiesto_non_si_legge() {
    // Reperti S8-8 e S1-2: il tetto delle risposte di byte era uno solo, 8 MiB,
    // anche per una copertina (1 MiB al più) o per un pezzo da cento byte; chi
    // chiedeva allocava e scaricava tutto prima di dire di no.
    let (server, del_server) = nodo("Server");
    let (cliente, di_cliente) = nodo("Cliente");
    collegati(&cliente, &server, &del_server, &di_cliente);
    let tempo = Duration::from_secs(20);
    let copertina = || Domanda::Copertina {
        impronta: "ab".repeat(16),
    };
    let leggi = |quanti: u64| Domanda::Leggi {
        brano: 1,
        da: 0,
        quanti,
    };

    // Quel che si chiede, arriva: una copertina nel suo tetto, un pezzo intero.
    *del_server.byte_a_forza.lock().unwrap() = Some(100 * 1024);
    assert_eq!(
        byte_letti(cliente.chiedi(&server.chiave(), copertina(), tempo)),
        Ok(100 * 1024)
    );
    assert_eq!(
        byte_letti(cliente.chiedi(&server.chiave(), leggi(100 * 1024), tempo)),
        Ok(100 * 1024)
    );
    // Una copertina da due mebibyte non è una copertina: oltre il suo tetto, e
    // l'altro lo sa dalla testa.
    *del_server.byte_a_forza.lock().unwrap() = Some(2 * 1024 * 1024);
    assert_eq!(
        byte_letti(cliente.chiedi(&server.chiave(), copertina(), tempo)),
        Err(ErrorCodeKind::LontanoProtocollo),
        "una copertina da 2 MiB"
    );
    // Un pezzo più lungo di quanto se ne è chiesto, nemmeno.
    *del_server.byte_a_forza.lock().unwrap() = Some(1000);
    assert_eq!(
        byte_letti(cliente.chiedi(&server.chiave(), leggi(10), tempo)),
        Err(ErrorCodeKind::LontanoProtocollo),
        "mille byte a una domanda che ne voleva dieci"
    );
    // E un pezzo da due mebibyte, quando se ne chiedono due, sta nel tetto.
    *del_server.byte_a_forza.lock().unwrap() = Some(2 * 1024 * 1024);
    assert_eq!(
        byte_letti(cliente.chiedi(&server.chiave(), leggi(u64::MAX), tempo)),
        Ok(2 * 1024 * 1024)
    );
}
