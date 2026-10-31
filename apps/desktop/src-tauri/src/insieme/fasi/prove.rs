//! Le prove del riduttore: `passo` provato a tappeto con dei numeri, senza rete né orologio.

use super::pezzi::ammetti;
use super::*;
use crate::riproduzione::insieme::UID_CALIBRAZIONE;
use rymembra_insieme::messaggi::{MASSIMO_PERSONE, Ripeti};

const HOST: IdConnessione = IdConnessione(1);
const MARCO: IdConnessione = IdConnessione(2);
const LUCA: IdConnessione = IdConnessione(3);

fn conn(pari: Option<i64>, nome: &str, ospita: bool, in_entrata: bool) -> Connessione {
    Connessione {
        chiave: ChiavePubblica([u8::try_from(pari.unwrap_or(0)).unwrap_or(0); 32]),
        nome: nome.to_owned(),
        dichiarato: nome.to_owned(),
        ospita,
        in_entrata,
        pari,
    }
}

/// Un PC acceso, collegato a un host fidato.
fn libero_con_host() -> Situazione {
    let mut s = Situazione::default();
    (s, _) = passo(s, Evento::Acceso, 0);
    (s, _) = passo(
        s,
        Evento::Aperta {
            id: HOST,
            connessione: conn(Some(10), "Sala", true, false),
        },
        0,
    );
    s
}

#[test]
fn un_associato_che_non_riconosce_piu_questo_pc_lo_si_dice_una_volta() {
    let s = libero_con_host();
    let (s, fx) = passo(
        s,
        Evento::Chiusa {
            id: HOST,
            codice: Some(ErrorCodeKind::InsiemeNonAssociato),
        },
        10,
    );
    assert_eq!(
        fx,
        vec![avviso(AVVISO_NON_TI_RICONOSCE, Some("Sala".to_owned()))]
    );
    // Ci riprova, e di nuovo no: niente secondo avviso.
    let (s, _) = passo(
        s,
        Evento::Aperta {
            id: IdConnessione(9),
            connessione: conn(Some(10), "Sala", true, false),
        },
        20,
    );
    let (s, fx) = passo(
        s,
        Evento::Chiusa {
            id: IdConnessione(9),
            codice: Some(ErrorCodeKind::InsiemeNonAssociato),
        },
        30,
    );
    assert!(fx.is_empty(), "{fx:?}");
    // Una chiusura qualunque non è un «non ti riconosco».
    let (_, fx) = passo(
        libero_con_host(),
        Evento::Chiusa {
            id: HOST,
            codice: Some(ErrorCodeKind::InsiemeConnessioneCaduta),
        },
        10,
    );
    assert!(fx.is_empty(), "{fx:?}");
    assert!(s.non_riconoscono.contains(&10));
}

/// Un PC che ospita, con `n` fidati collegati (pari 100.., conn 100..).
fn host_con(n: u64) -> Situazione {
    let mut s = Situazione::default();
    (s, _) = passo(s, Evento::Acceso, 0);
    (s, _) = passo(s, Evento::Ospitato { porta: 47811 }, 0);
    for i in 0..n {
        let pari = i64::try_from(100 + i).unwrap();
        (s, _) = passo(
            s,
            Evento::Aperta {
                id: IdConnessione(100 + i),
                connessione: conn(Some(pari), &format!("pc{i}"), false, true),
            },
            0,
        );
    }
    s
}

fn mandati(fx: &[Effetto]) -> Vec<(IdConnessione, Messaggio)> {
    fx.iter()
        .filter_map(|e| match e {
            Effetto::Manda { a, messaggio } => Some((*a, messaggio.clone())),
            _ => None,
        })
        .collect()
}

fn avvisi(fx: &[Effetto]) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effetto::Avviso { chiave, .. } => Some(chiave.clone()),
            _ => None,
        })
        .collect()
}

fn errori(fx: &[Effetto]) -> Vec<&'static str> {
    fx.iter()
        .filter_map(|e| match e {
            Effetto::Errore(err) => Some(err.code().kind().code()),
            _ => None,
        })
        .collect()
}

fn invita_e_accetta(s: Situazione, conn: IdConnessione, pari: i64) -> (Situazione, Vec<Effetto>) {
    invita_e_accetta_con(s, conn, pari, Diritti::Tutto)
}

/// Come [`invita_e_accetta`], con il livello scelto nell'invito.
fn invita_e_accetta_con(
    s: Situazione,
    conn: IdConnessione,
    pari: i64,
    diritti: Diritti,
) -> (Situazione, Vec<Effetto>) {
    let (s, fx) = passo(s, Evento::Comando(Comando::Invita { pari, diritti }), 1_000);
    let invito = mandati(&fx)
        .into_iter()
        .find_map(|(_, m)| match m {
            Messaggio::Invito { invito, .. } => Some(invito),
            _ => None,
        })
        .expect("l'invito parte");
    passo(
        s,
        Evento::Ricevuto {
            id: conn,
            messaggio: Messaggio::RispostaInvito {
                invito,
                accetta: true,
            },
        },
        2_000,
    )
}

fn invito_arrivato(s: Situazione, da: IdConnessione, invito: u64, adesso: u64) -> Situazione {
    passo(
        s,
        Evento::Ricevuto {
            id: da,
            messaggio: Messaggio::Invito {
                invito,
                scade_fra_ms: SCADENZA_INVITO_MS,
            },
        },
        adesso,
    )
    .0
}

fn numeri_degli_inviti(s: &Situazione) -> Vec<u64> {
    match &s.fase {
        Fase::Libera { inviti } => inviti.iter().map(|i| i.numero).collect(),
        altro => panic!("non libera: {altro:?}"),
    }
}

/// Porta un PC libero dentro la sessione dell'host, per invito.
fn membro() -> Situazione {
    let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
    let numero = numeri_degli_inviti(&s)[0];
    let (s, _) = passo(
        s,
        Evento::Comando(Comando::Rispondi {
            invito: numero,
            accetta: true,
        }),
        100,
    );
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: HOST,
            messaggio: Messaggio::Ammesso {
                membro: 3,
                diritti: Diritti::Tutto,
            },
        },
        200,
    );
    assert!(fx.contains(&Effetto::EntraComeMembro { host: HOST }));
    assert!(matches!(&s.fase, Fase::Membro(m) if m.io == 3));
    s
}

#[test]
fn l_host_che_sparisce_fa_uscire_chi_segue_con_l_avviso() {
    let s = membro();
    let (s, fx) = passo(
        s,
        Evento::Chiusa {
            id: HOST,
            codice: Some(ErrorCodeKind::InsiemeConnessioneCaduta),
        },
        5_000,
    );
    assert_eq!(s.fase, Fase::Uscendo { spegni: false });
    assert!(fx.contains(&Effetto::EsciDalLettore));
    assert_eq!(avvisi(&fx), vec![AVVISO_HOST_SPARITO]);
    // Niente messaggi a una connessione che non c'è più.
    assert!(mandati(&fx).is_empty());
    let (s, _) = passo(s, Evento::Uscito, 5_100);
    assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
}

#[test]
fn l_host_che_sparisce_durante_l_ingresso_riporta_liberi() {
    let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
    let numero = numeri_degli_inviti(&s)[0];
    let (s, _) = passo(
        s,
        Evento::Comando(Comando::Rispondi {
            invito: numero,
            accetta: true,
        }),
        100,
    );
    assert!(matches!(s.fase, Fase::Entrando { .. }));
    let (s, fx) = passo(
        s,
        Evento::Chiusa {
            id: HOST,
            codice: None,
        },
        200,
    );
    assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
    assert_eq!(avvisi(&fx), vec![AVVISO_HOST_SPARITO]);
    assert!(
        !fx.contains(&Effetto::EsciDalLettore),
        "il lettore non era entrato"
    );
}

#[test]
fn un_invito_scaduto_sparisce_e_la_risposta_tardiva_e_un_errore() {
    let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
    let numero = numeri_degli_inviti(&s)[0];
    assert_eq!(prossima_scadenza(&s), Some(SCADENZA_INVITO_MS));
    let (s, _) = passo(s, Evento::Tempo, SCADENZA_INVITO_MS - 1);
    assert_eq!(numeri_degli_inviti(&s), vec![numero], "non ancora");
    let (s, fx) = passo(s, Evento::Tempo, SCADENZA_INVITO_MS);
    assert!(numeri_degli_inviti(&s).is_empty());
    assert_eq!(avvisi(&fx), vec![AVVISO_INVITO_SCADUTO]);
    assert!(
        mandati(&fx).is_empty(),
        "all'host non si dice niente: {fx:?}"
    );
    let (s, fx) = passo(
        s,
        Evento::Comando(Comando::Rispondi {
            invito: numero,
            accetta: true,
        }),
        SCADENZA_INVITO_MS + 10,
    );
    assert_eq!(errori(&fx), vec!["insieme.rifiutato"]);
    assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
}

#[test]
fn la_durata_di_un_invito_ha_il_tetto_di_chi_lo_riceve() {
    let s = passo(
        libero_con_host(),
        Evento::Ricevuto {
            id: HOST,
            messaggio: Messaggio::Invito {
                invito: 1,
                scade_fra_ms: 24 * 3_600_000,
            },
        },
        1_000,
    )
    .0;
    assert_eq!(prossima_scadenza(&s), Some(1_000 + SCADENZA_INVITO_MS));
}

#[test]
fn dall_host_un_invito_scaduto_avvisa_e_il_si_tardivo_non_fa_entrare() {
    let s = host_con(1);
    let (s, fx) = passo(
        s,
        Evento::Comando(Comando::Invita {
            pari: 100,
            diritti: Diritti::Tutto,
        }),
        0,
    );
    let invito = match mandati(&fx).as_slice() {
        [(_, Messaggio::Invito { invito, .. })] => *invito,
        altro => panic!("{altro:?}"),
    };
    let (s, fx) = passo(s, Evento::Tempo, SCADENZA_INVITO_MS);
    assert_eq!(avvisi(&fx), vec![AVVISO_INVITO_SENZA_RISPOSTA]);
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(100),
            messaggio: Messaggio::RispostaInvito {
                invito,
                accetta: true,
            },
        },
        SCADENZA_INVITO_MS + 5,
    );
    assert_eq!(
        mandati(&fx),
        vec![(
            IdConnessione(100),
            Messaggio::Rifiutato {
                codice: "insieme.rifiutato".to_owned()
            }
        )]
    );
    assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.is_empty()));
}

#[test]
fn la_sessione_piena_dice_di_no_al_nono_membro() {
    // Otto membri più l'host fanno nove: il tetto.
    let mut s = host_con(9);
    for i in 0..8 {
        let (dopo, fx) =
            invita_e_accetta(s, IdConnessione(100 + i), i64::try_from(100 + i).unwrap());
        assert!(fx.contains(&Effetto::PersoneCambiate), "membro {i}");
        s = dopo;
    }
    assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.len() == 8));
    assert_eq!(persone_di_qui(&s, "io").len(), MASSIMO_PERSONE);
    // Invitare il nono è già un errore.
    let (s, fx) = passo(
        s,
        Evento::Comando(Comando::Invita {
            pari: 108,
            diritti: Diritti::Tutto,
        }),
        3_000,
    );
    assert_eq!(errori(&fx), vec!["insieme.sessionePiena"]);
    // E una richiesta riceve il no sul filo.
    let (_, fx) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(108),
            messaggio: Messaggio::Chiedi,
        },
        3_000,
    );
    assert_eq!(
        mandati(&fx),
        vec![(
            IdConnessione(108),
            Messaggio::Rifiutato {
                codice: "insieme.sessionePiena".to_owned()
            }
        )]
    );
}

#[test]
fn due_si_con_un_posto_solo_fanno_entrare_il_primo() {
    let mut s = host_con(9);
    for i in 0..7 {
        s = invita_e_accetta(s, IdConnessione(100 + i), i64::try_from(100 + i).unwrap()).0;
    }
    // Un posto libero, due inviti in volo.
    let (s, fx1) = passo(
        s,
        Evento::Comando(Comando::Invita {
            pari: 107,
            diritti: Diritti::Tutto,
        }),
        1_000,
    );
    let (s, fx2) = passo(
        s,
        Evento::Comando(Comando::Invita {
            pari: 108,
            diritti: Diritti::Tutto,
        }),
        1_000,
    );
    let numero = |fx: &[Effetto]| {
        mandati(fx)
            .into_iter()
            .find_map(|(_, m)| match m {
                Messaggio::Invito { invito, .. } => Some(invito),
                _ => None,
            })
            .unwrap()
    };
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(107),
            messaggio: Messaggio::RispostaInvito {
                invito: numero(&fx1),
                accetta: true,
            },
        },
        2_000,
    );
    assert!(fx.contains(&Effetto::PersoneCambiate));
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(108),
            messaggio: Messaggio::RispostaInvito {
                invito: numero(&fx2),
                accetta: true,
            },
        },
        2_000,
    );
    assert_eq!(
        mandati(&fx),
        vec![(
            IdConnessione(108),
            Messaggio::Rifiutato {
                codice: "insieme.sessionePiena".to_owned()
            }
        )]
    );
    assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.len() == 8));
}

#[test]
fn il_doppio_invito_dello_stesso_host_sostituisce_il_primo() {
    let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
    let s = invito_arrivato(s, HOST, 8, 1_000);
    let Fase::Libera { inviti } = &s.fase else {
        panic!()
    };
    assert_eq!(inviti.len(), 1);
    assert_eq!(inviti[0].invito, 8);
    assert_eq!(inviti[0].scade, 1_000 + SCADENZA_INVITO_MS);
}

#[test]
fn dall_host_il_secondo_invito_annulla_il_primo() {
    let s = host_con(1);
    let (s, fx1) = passo(
        s,
        Evento::Comando(Comando::Invita {
            pari: 100,
            diritti: Diritti::Tutto,
        }),
        0,
    );
    let (s, _) = passo(
        s,
        Evento::Comando(Comando::Invita {
            pari: 100,
            diritti: Diritti::Tutto,
        }),
        10,
    );
    let Fase::Ospita(o) = &s.fase else { panic!() };
    assert_eq!(o.inviti.len(), 1);
    // La risposta al primo non fa entrare: il numero non vale più.
    let primo = match mandati(&fx1).as_slice() {
        [(_, Messaggio::Invito { invito, .. })] => *invito,
        altro => panic!("{altro:?}"),
    };
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(100),
            messaggio: Messaggio::RispostaInvito {
                invito: primo,
                accetta: true,
            },
        },
        20,
    );
    assert!(!fx.contains(&Effetto::PersoneCambiate));
    assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.is_empty()));
}

#[test]
fn inviti_di_due_host_accettarne_uno_rifiuta_l_altro() {
    let mut s = libero_con_host();
    (s, _) = passo(
        s,
        Evento::Aperta {
            id: LUCA,
            connessione: conn(Some(11), "Studio", true, false),
        },
        0,
    );
    let s = invito_arrivato(s, HOST, 7, 0);
    let s = invito_arrivato(s, LUCA, 7, 0);
    let numeri = numeri_degli_inviti(&s);
    assert_eq!(numeri.len(), 2, "stesso numero da due host: due inviti");
    let (s, fx) = passo(
        s,
        Evento::Comando(Comando::Rispondi {
            invito: numeri[1],
            accetta: true,
        }),
        10,
    );
    assert_eq!(
        mandati(&fx),
        vec![
            (
                HOST,
                Messaggio::RispostaInvito {
                    invito: 7,
                    accetta: false
                }
            ),
            (
                LUCA,
                Messaggio::RispostaInvito {
                    invito: 7,
                    accetta: true
                }
            ),
        ]
    );
    assert!(matches!(s.fase, Fase::Entrando { host: LUCA, .. }));
}

#[test]
fn uscire_durante_l_ingresso_e_poi_ricevere_ammesso_risponde_esco() {
    let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
    let numero = numeri_degli_inviti(&s)[0];
    let (s, _) = passo(
        s,
        Evento::Comando(Comando::Rispondi {
            invito: numero,
            accetta: true,
        }),
        100,
    );
    let (s, fx) = passo(s, Evento::Comando(Comando::Esci), 150);
    assert_eq!(mandati(&fx), vec![(HOST, Messaggio::Esco)]);
    assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
    // L'host aveva già ammesso: l'`Ammesso` arriva a un PC libero.
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: HOST,
            messaggio: Messaggio::Ammesso {
                membro: 1,
                diritti: Diritti::Tutto,
            },
        },
        200,
    );
    assert_eq!(mandati(&fx), vec![(HOST, Messaggio::Esco)]);
    assert!(!fx.contains(&Effetto::EntraComeMembro { host: HOST }));
    assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
}

#[test]
fn ritirare_una_richiesta_manda_non_chiedo_piu() {
    let s = libero_con_host();
    let (s, fx) = passo(s, Evento::Comando(Comando::Chiedi { pari: 10 }), 0);
    assert_eq!(mandati(&fx), vec![(HOST, Messaggio::Chiedi)]);
    let (s, fx) = passo(s, Evento::Comando(Comando::Esci), 10);
    assert_eq!(mandati(&fx), vec![(HOST, Messaggio::NonChiedoPiu)]);
    assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
}

#[test]
fn una_richiesta_senza_risposta_scade_da_tutte_e_due_le_parti() {
    // Chi chiede.
    let (s, _) = passo(
        libero_con_host(),
        Evento::Comando(Comando::Chiedi { pari: 10 }),
        0,
    );
    let (s, fx) = passo(s, Evento::Tempo, SCADENZA_RICHIESTA_MS);
    assert_eq!(mandati(&fx), vec![(HOST, Messaggio::NonChiedoPiu)]);
    assert_eq!(avvisi(&fx), vec![AVVISO_RICHIESTA_SENZA_RISPOSTA]);
    assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
    // Chi ospita.
    let s = host_con(1);
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(100),
            messaggio: Messaggio::Chiedi,
        },
        0,
    );
    assert!(avvisi(&fx).is_empty(), "la richiesta sta nel pannello");
    let (s, fx) = passo(s, Evento::Tempo, SCADENZA_RICHIESTA_MS);
    assert_eq!(
        mandati(&fx),
        vec![(
            IdConnessione(100),
            Messaggio::Rifiutato {
                codice: "insieme.rifiutato".to_owned()
            }
        )]
    );
    assert_eq!(avvisi(&fx), vec![AVVISO_RICHIESTA_SCADUTA]);
    assert!(matches!(&s.fase, Fase::Ospita(o) if o.richieste.is_empty()));
}

#[test]
fn una_richiesta_accettata_fa_entrare() {
    let s = host_con(1);
    let (s, _) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(100),
            messaggio: Messaggio::Chiedi,
        },
        0,
    );
    let numero = match &s.fase {
        Fase::Ospita(o) => o.richieste[0].numero,
        _ => panic!(),
    };
    let (s, fx) = passo(
        s,
        Evento::Comando(Comando::Ammetti {
            richiesta: numero,
            accetta: true,
            diritti: Diritti::Comanda,
        }),
        10,
    );
    assert_eq!(
        mandati(&fx),
        vec![(
            IdConnessione(100),
            Messaggio::Ammesso {
                membro: 1,
                diritti: Diritti::Comanda,
            }
        )]
    );
    assert_eq!(avvisi(&fx), vec![AVVISO_ENTRATO]);
    assert_eq!(
        persone_di_qui(&s, "Sala"),
        vec![
            Persona {
                membro: 0,
                nome: "Sala".to_owned(),
                diritti: Diritti::Tutto,
            },
            Persona {
                membro: 1,
                nome: "pc0".to_owned(),
                diritti: Diritti::Comanda,
            }
        ]
    );
}

#[test]
fn il_fidato_bloccato_mentre_e_invitato_non_entra_piu() {
    let s = host_con(1);
    let (s, fx) = passo(
        s,
        Evento::Comando(Comando::Invita {
            pari: 100,
            diritti: Diritti::Tutto,
        }),
        0,
    );
    let invito = match mandati(&fx).as_slice() {
        [(_, Messaggio::Invito { invito, .. })] => *invito,
        altro => panic!("{altro:?}"),
    };
    let (s, _) = passo(s, Evento::PariTolto { pari: 100 }, 10);
    assert!(matches!(&s.fase, Fase::Ospita(o) if o.inviti.is_empty()));
    // Il suo sì era già in volo: non fa entrare, e non lo si legge
    // nemmeno, perché la chiave non è più fidata.
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(100),
            messaggio: Messaggio::RispostaInvito {
                invito,
                accetta: true,
            },
        },
        20,
    );
    assert!(fx.is_empty(), "{fx:?}");
    assert!(matches!(&s.fase, Fase::Ospita(o) if o.membri.is_empty()));
}

#[test]
fn il_fidato_bloccato_mentre_e_in_sessione_esce_con_fine() {
    let s = invita_e_accetta(host_con(2), IdConnessione(100), 100).0;
    let s = invita_e_accetta(s, IdConnessione(101), 101).0;
    let (s, fx) = passo(s, Evento::PariTolto { pari: 100 }, 3_000);
    assert_eq!(
        mandati(&fx),
        vec![(
            IdConnessione(100),
            Messaggio::Fine {
                codice: Some("insieme.nonAssociato".to_owned())
            }
        )]
    );
    assert!(fx.contains(&Effetto::PersoneCambiate));
    assert_eq!(connessioni_dei_membri(&s), vec![IdConnessione(101)]);
    // La rete poi chiude la connessione: niente da rifare.
    let (_, fx) = passo(
        s,
        Evento::Chiusa {
            id: IdConnessione(100),
            codice: None,
        },
        3_100,
    );
    assert!(fx.is_empty(), "{fx:?}");
}

#[test]
fn chi_segue_e_blocca_l_host_esce_dalla_sessione() {
    let s = membro();
    let (s, fx) = passo(s, Evento::PariTolto { pari: 10 }, 1_000);
    assert_eq!(mandati(&fx), vec![(HOST, Messaggio::Esco)]);
    assert!(fx.contains(&Effetto::EsciDalLettore));
    assert_eq!(s.fase, Fase::Uscendo { spegni: false });
}

#[test]
fn i_messaggi_di_un_pc_non_fidato_non_contano() {
    let mut s = Situazione::default();
    (s, _) = passo(s, Evento::Acceso, 0);
    (s, _) = passo(
        s,
        Evento::Aperta {
            id: MARCO,
            connessione: conn(None, "Sconosciuto", true, false),
        },
        0,
    );
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: MARCO,
            messaggio: Messaggio::Invito {
                invito: 1,
                scade_fra_ms: 1_000,
            },
        },
        0,
    );
    assert!(fx.is_empty());
    assert_eq!(numeri_degli_inviti(&s), Vec::<u64>::new());
}

#[test]
fn lo_stato_vecchio_si_scarta_e_l_ancora_passa_solo_dall_host() {
    let s = membro();
    let stato = |versione| {
        Messaggio::Stato(StatoSessione {
            versione,
            persone: vec![Persona {
                membro: 0,
                nome: "Sala".to_owned(),
                diritti: Diritti::Tutto,
            }],
            coda: vec![],
            corrente: None,
            in_pausa: true,
            ripeti: Ripeti::No,
        })
    };
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: HOST,
            messaggio: stato(5),
        },
        0,
    );
    assert_eq!(fx.len(), 1);
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: HOST,
            messaggio: stato(4),
        },
        0,
    );
    assert!(fx.is_empty(), "più vecchio");
    let ancora = AncoraSulFilo {
        etichetta: 1,
        posizione_us: 0,
        udito_ns: 0,
        in_corso: true,
        programmata: false,
    };
    let (_, fx) = passo(
        s,
        Evento::Ricevuto {
            id: HOST,
            messaggio: Messaggio::Ancora(ancora),
        },
        0,
    );
    assert_eq!(fx, vec![Effetto::Segui(ancora)]);
}

#[test]
fn la_fine_dall_host_dice_perche() {
    for (codice, chiave) in [
        (None, AVVISO_FINITA),
        (Some("insieme.rifiutato"), AVVISO_TI_HA_ALLONTANATO),
        (Some("insieme.nonAssociato"), "errors.insieme.nonAssociato"),
        // Un codice d'un altro dominio non passa: vale un rifiuto.
        (Some("db.corrupt"), "errors.insieme.rifiutato"),
    ] {
        let (s, fx) = passo(
            membro(),
            Evento::Ricevuto {
                id: HOST,
                messaggio: Messaggio::Fine {
                    codice: codice.map(ToOwned::to_owned),
                },
            },
            0,
        );
        assert_eq!(avvisi(&fx), vec![chiave], "{codice:?}");
        assert_eq!(s.fase, Fase::Uscendo { spegni: false });
    }
}

#[test]
fn terminare_manda_fine_a_tutti_e_spegnere_in_sessione_aspetta_il_lettore() {
    let s = invita_e_accetta(host_con(2), IdConnessione(100), 100).0;
    let s = invita_e_accetta(s, IdConnessione(101), 101).0;
    let (t, fx) = passo(s.clone(), Evento::Comando(Comando::Termina), 0);
    assert_eq!(
        mandati(&fx),
        vec![
            (IdConnessione(100), Messaggio::Fine { codice: None }),
            (IdConnessione(101), Messaggio::Fine { codice: None }),
        ]
    );
    assert!(fx.contains(&Effetto::SmettiDiOspitare));
    assert_eq!(
        passo(t, Evento::Uscito, 1).0.fase,
        Fase::Libera { inviti: vec![] }
    );

    let (t, fx) = passo(s, Evento::Spegni, 0);
    assert_eq!(fx.last(), Some(&Effetto::FermaRete));
    assert_eq!(t.fase, Fase::Uscendo { spegni: true });
    assert_eq!(passo(t, Evento::Uscito, 1).0.fase, Fase::Spenta);
}

/// Chi aspettava di entrare (una richiesta in sospeso, un invito appena
/// accettato) e chi era stato invitato sente la fine della sessione col
/// suo perché, «ha terminato», non «non risponde più» come per un PC
/// caduto. Anche quando chi ospita spegne o chiude l'app.
#[test]
fn terminare_con_una_richiesta_in_attesa_lo_dice_a_chi_chiedeva() {
    let chiede = |s: Situazione| {
        passo(
            s,
            Evento::Ricevuto {
                id: IdConnessione(100),
                messaggio: Messaggio::Chiedi,
            },
            0,
        )
        .0
    };
    let invita = |s: Situazione| {
        passo(
            s,
            Evento::Comando(Comando::Invita {
                pari: 101,
                diritti: Diritti::Tutto,
            }),
            0,
        )
        .0
    };
    let s = invita(chiede(host_con(2)));
    assert!(matches!(&s.fase, Fase::Ospita(o) if o.richieste.len() == 1 && o.inviti.len() == 1));
    let attesi = vec![
        (IdConnessione(100), Messaggio::Fine { codice: None }),
        (IdConnessione(101), Messaggio::Fine { codice: None }),
    ];
    let (_, fx) = passo(s.clone(), Evento::Comando(Comando::Termina), 0);
    assert_eq!(mandati(&fx), attesi);
    let (_, fx) = passo(s, Evento::Spegni, 0);
    assert_eq!(mandati(&fx), attesi);
}

/// Una fase `Uscendo` vista da `Spegni` è un resto: l'uscita del lettore
/// chiama `Uscito` nello stesso giro in cui la fase la mette, e `Spegni`
/// arriva sempre in un giro suo. Un'uscita rimasta a metà (un panico in
/// un effetto) si deve poter spegnere, e non aspettare un `Uscito` che
/// non arriverà più.
#[test]
fn spegnere_da_uscendo_rimasta_spegne() {
    for spegni in [false, true] {
        let s = Situazione {
            fase: Fase::Uscendo { spegni },
            ..Situazione::default()
        };
        let (s, fx) = passo(s, Evento::Spegni, 0);
        assert_eq!(s.fase, Fase::Spenta, "da Uscendo {{ spegni: {spegni} }}");
        assert!(fx.contains(&Effetto::FermaRete));
    }
}

#[test]
fn da_spenta_ogni_comando_risponde_spenta() {
    let (_, fx) = passo(Situazione::default(), Evento::Comando(Comando::Ospita), 0);
    assert_eq!(errori(&fx), vec!["insieme.spenta"]);
}

#[test]
fn l_associazione_scade_e_una_seconda_si_rifiuta() {
    let mut s = Situazione::default();
    (s, _) = passo(s, Evento::Acceso, 0);
    (s, _) = passo(
        s,
        Evento::Aperta {
            id: MARCO,
            connessione: conn(None, "Marco", true, false),
        },
        0,
    );
    let (s, fx) = passo(s, Evento::Comando(Comando::Associa { id: MARCO }), 0);
    assert_eq!(fx, vec![Effetto::AssociaInRete { id: MARCO }]);
    let (s, fx) = passo(
        s,
        Evento::Codice {
            id: MARCO,
            codice: "4821".to_owned(),
        },
        10,
    );
    assert!(fx.is_empty());
    // Un secondo codice su un'altra connessione: no.
    let (s, fx) = passo(
        s,
        Evento::Codice {
            id: LUCA,
            codice: "1111".to_owned(),
        },
        20,
    );
    assert_eq!(
        fx,
        vec![Effetto::ConfermaCodice {
            id: LUCA,
            uguale: false
        }]
    );
    let (s, fx) = passo(s, Evento::Tempo, SCADENZA_ASSOCIAZIONE_MS);
    assert_eq!(
        fx,
        vec![
            Effetto::ConfermaCodice {
                id: MARCO,
                uguale: false
            },
            avviso(AVVISO_ASSOCIAZIONE_ANNULLATA, Some("Marco".to_owned())),
        ]
    );
    assert_eq!(s.associazione, None);
}

#[test]
fn l_associazione_confermata_e_riuscita_rende_fidato() {
    let mut s = Situazione::default();
    (s, _) = passo(s, Evento::Acceso, 0);
    (s, _) = passo(
        s,
        Evento::Aperta {
            id: MARCO,
            connessione: conn(None, "Marco", false, true),
        },
        0,
    );
    // Chi ospita non chiama `associa`: gli arriva il codice.
    let (s, fx) = passo(
        s,
        Evento::Codice {
            id: MARCO,
            codice: "0042".to_owned(),
        },
        0,
    );
    assert_eq!(fx, vec![Effetto::MostraFinestra]);
    let (s, fx) = passo(
        s,
        Evento::Comando(Comando::ConfermaCodice {
            associazione: MARCO.0,
            uguale: true,
        }),
        5,
    );
    assert_eq!(
        fx,
        vec![Effetto::ConfermaCodice {
            id: MARCO,
            uguale: true
        }]
    );
    let (s, fx) = passo(
        s,
        Evento::Associato {
            id: MARCO,
            pari: 42,
        },
        9,
    );
    assert_eq!(avvisi(&fx), vec![AVVISO_ASSOCIATO]);
    assert_eq!(s.connessioni[&MARCO].pari, Some(42));
    assert_eq!(s.associazione, None);
}

#[test]
fn il_gesto_di_un_membro_si_esegue_finche_il_secchiello_lo_lascia_passare() {
    let mut s = invita_e_accetta(host_con(1), IdConnessione(100), 100).0;
    let comando = |numero| Evento::Ricevuto {
        id: IdConnessione(100),
        messaggio: Messaggio::Comando {
            id: numero,
            azione: Azione::Pausa,
        },
    };
    for numero in 0..10 {
        let (dopo, fx) = passo(s, comando(numero), 5_000);
        assert_eq!(
            fx,
            vec![Effetto::Esegui {
                da: IdConnessione(100),
                membro: 1,
                comando: numero,
                azione: Box::new(Azione::Pausa),
            }]
        );
        s = dopo;
    }
    // L'undicesimo nello stesso istante: no, col perché.
    let (s, fx) = passo(s, comando(10), 5_000);
    assert_eq!(
        fx,
        vec![manda(
            IdConnessione(100),
            Messaggio::Esito {
                id: 10,
                codice: Some("insieme.troppiTentativi".to_owned())
            }
        )]
    );
    // Un passo dopo, di nuovo.
    let (_, fx) = passo(s, comando(11), 5_100);
    assert!(matches!(
        fx.as_slice(),
        [Effetto::Esegui { comando: 11, .. }]
    ));
}

#[test]
fn un_gesto_di_una_versione_piu_nuova_ha_il_suo_no_e_non_tocca_il_secchiello() {
    let s = invita_e_accetta(host_con(1), IdConnessione(100), 100).0;
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(100),
            messaggio: Messaggio::Comando {
                id: 4,
                azione: Azione::Sconosciuta,
            },
        },
        5_000,
    );
    assert_eq!(
        fx,
        vec![manda(
            IdConnessione(100),
            Messaggio::Esito {
                id: 4,
                codice: Some("insieme.gestoNonDisponibile".to_owned())
            }
        )]
    );
    assert!(matches!(s.fase, Fase::Ospita(_)), "la sessione continua");
}

#[test]
fn un_comando_da_chi_non_e_membro_non_si_esegue() {
    let s = host_con(1);
    let (_, fx) = passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(100),
            messaggio: Messaggio::Comando {
                id: 1,
                azione: Azione::Pausa,
            },
        },
        0,
    );
    assert!(fx.is_empty(), "{fx:?}");
}

#[test]
fn l_esito_negativo_dall_host_e_un_avviso_tranne_il_secchiello() {
    let esito = |codice: Option<&str>| Evento::Ricevuto {
        id: HOST,
        messaggio: Messaggio::Esito {
            id: 1,
            codice: codice.map(ToOwned::to_owned),
        },
    };
    let (s, fx) = passo(membro(), esito(Some("insieme.gestoNonDisponibile")), 0);
    assert_eq!(avvisi(&fx), vec!["errors.insieme.gestoNonDisponibile"]);
    let (s, fx) = passo(s, esito(Some("insieme.troppiTentativi")), 0);
    assert!(fx.is_empty());
    let (_, fx) = passo(s, esito(None), 0);
    assert!(fx.is_empty());
}

#[test]
fn le_voci_di_un_membro_escono_con_lui_in_ogni_modo_in_cui_esce() {
    let due = || {
        let s = invita_e_accetta(host_con(2), IdConnessione(100), 100).0;
        invita_e_accetta(s, IdConnessione(101), 101).0
    };
    let uscito = |fx: &[Effetto]| {
        fx.iter()
            .filter_map(|e| match e {
                Effetto::MembroUscito { membro } => Some(*membro),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let (_, fx) = passo(
        due(),
        Evento::Ricevuto {
            id: IdConnessione(101),
            messaggio: Messaggio::Esco,
        },
        0,
    );
    assert_eq!(uscito(&fx), vec![2]);
    let (_, fx) = passo(
        due(),
        Evento::Chiusa {
            id: IdConnessione(100),
            codice: None,
        },
        0,
    );
    assert_eq!(uscito(&fx), vec![1]);
    let (_, fx) = passo(due(), Evento::Comando(Comando::Allontana { membro: 2 }), 0);
    assert_eq!(uscito(&fx), vec![2]);
    let (_, fx) = passo(due(), Evento::PariTolto { pari: 100 }, 0);
    assert_eq!(uscito(&fx), vec![1]);
}

#[test]
fn allontanare_un_membro_gli_manda_fine_col_perche() {
    let s = invita_e_accetta(host_con(1), IdConnessione(100), 100).0;
    let (s, fx) = passo(s, Evento::Comando(Comando::Allontana { membro: 1 }), 0);
    assert_eq!(
        mandati(&fx),
        vec![(
            IdConnessione(100),
            Messaggio::Fine {
                codice: Some("insieme.rifiutato".to_owned())
            }
        )]
    );
    assert!(connessioni_dei_membri(&s).is_empty());
}

#[test]
fn chiave_del_codice_resta_nel_dominio_insieme() {
    assert_eq!(
        chiave_del_codice("insieme.sessionePiena"),
        "errors.insieme.sessionePiena"
    );
    assert_eq!(chiave_del_codice("db.corrupt"), "errors.insieme.rifiutato");
    assert_eq!(
        chiave_del_codice("insieme.\nqualcosa"),
        "errors.insieme.rifiutato"
    );
}

fn stato_con(versione: u64, membri: &[(u32, &str)]) -> Messaggio {
    let mut persone = vec![Persona {
        membro: 0,
        nome: "Sala".to_owned(),
        diritti: Diritti::Tutto,
    }];
    persone.extend(membri.iter().map(|(membro, nome)| Persona {
        membro: *membro,
        nome: (*nome).to_owned(),
        diritti: Diritti::Tutto,
    }));
    Messaggio::Stato(StatoSessione {
        versione,
        persone,
        coda: vec![],
        corrente: None,
        in_pausa: false,
        ripeti: Ripeti::No,
    })
}

#[test]
fn chi_segue_sente_chi_entra_e_chi_esce_ma_non_se_stesso() {
    // Questo PC è il membro 3 (vedi `membro`).
    let s = membro();
    let (s, fx) = passo(
        s,
        Evento::Ricevuto {
            id: HOST,
            messaggio: stato_con(1, &[(1, "Marco"), (3, "Io")]),
        },
        0,
    );
    assert!(
        avvisi(&fx).is_empty(),
        "il primo stato è la sessione com'era"
    );
    let (_, fx) = passo(
        s,
        Evento::Ricevuto {
            id: HOST,
            messaggio: stato_con(2, &[(3, "Io"), (4, "Luca")]),
        },
        0,
    );
    assert_eq!(
        fx.iter()
            .filter_map(|e| match e {
                Effetto::Avviso { chiave, nome } => Some((chiave.as_str(), nome.as_deref())),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![
            (AVVISO_ENTRATO, Some("Luca")),
            (AVVISO_USCITO, Some("Marco"))
        ]
    );
}

#[test]
fn la_calibrazione_dell_host_si_annuncia_a_chi_segue() {
    let ancora = |etichetta| Evento::Ricevuto {
        id: HOST,
        messaggio: Messaggio::Ancora(AncoraSulFilo {
            etichetta,
            posizione_us: 0,
            udito_ns: 0,
            in_corso: true,
            programmata: false,
        }),
    };
    let (s, fx) = passo(membro(), ancora(UID_CALIBRAZIONE), 0);
    assert_eq!(avvisi(&fx), vec![AVVISO_CALIBRAZIONE_ACCESA]);
    let (s, fx) = passo(s, ancora(UID_CALIBRAZIONE), 0);
    assert!(avvisi(&fx).is_empty(), "una volta sola");
    let (_, fx) = passo(s, ancora(5), 0);
    assert_eq!(avvisi(&fx), vec![AVVISO_CALIBRAZIONE_SPENTA]);
}

#[test]
fn esci_mentre_si_entra_e_un_annulla_senza_errori() {
    let (s, _) = passo(
        libero_con_host(),
        Evento::Comando(Comando::Chiedi { pari: 10 }),
        0,
    );
    let (s, fx) = passo(s, Evento::Comando(Comando::Esci), 5);
    assert!(errori(&fx).is_empty());
    assert!(avvisi(&fx).is_empty());
    assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
}

#[test]
fn un_no_dell_host_e_una_notizia_la_sessione_piena_un_guasto() {
    for (via_richiesta, codice, chiave) in [
        (true, "insieme.rifiutato", AVVISO_RICHIESTA_RIFIUTATA),
        (false, "insieme.rifiutato", AVVISO_INVITO_SCADUTO),
        (
            true,
            "insieme.sessionePiena",
            "errors.insieme.sessionePiena",
        ),
    ] {
        let s = if via_richiesta {
            passo(
                libero_con_host(),
                Evento::Comando(Comando::Chiedi { pari: 10 }),
                0,
            )
            .0
        } else {
            let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
            let numero = numeri_degli_inviti(&s)[0];
            passo(
                s,
                Evento::Comando(Comando::Rispondi {
                    invito: numero,
                    accetta: true,
                }),
                0,
            )
            .0
        };
        let (s, fx) = passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio: Messaggio::Rifiutato {
                    codice: codice.to_owned(),
                },
            },
            1,
        );
        assert_eq!(avvisi(&fx), vec![chiave], "{codice}");
        assert_eq!(s.fase, Fase::Libera { inviti: vec![] });
    }
}

#[test]
fn ogni_chiave_d_avviso_e_una_di_quelle_della_finestra() {
    // `richieste-interfaccia.md` §5: le sole che la finestra traduce.
    let note = [
        "hosting",
        "joinedSession",
        "joined",
        "left",
        "removed",
        "youWereRemoved",
        "youLeft",
        "ended",
        "closed",
        "hostGone",
        "inviteSent",
        "inviteDeclined",
        "inviteUnanswered",
        "inviteExpired",
        "requestSent",
        "requestDeclined",
        "requestUnanswered",
        "requestExpired",
        "paired",
        "pairingCancelled",
        "calibrationOn",
        "calibrationOff",
        "forgottenBy",
        "rightsListen",
        "rightsControl",
        "rightsAll",
    ];
    for chiave in [
        AVVISO_SI_OSPITA,
        AVVISO_SEI_DENTRO,
        AVVISO_ENTRATO,
        AVVISO_USCITO,
        AVVISO_ALLONTANATO,
        AVVISO_TI_HA_ALLONTANATO,
        AVVISO_SEI_USCITO,
        AVVISO_FINITA,
        AVVISO_CHIUSA,
        AVVISO_HOST_SPARITO,
        AVVISO_INVITO_MANDATO,
        AVVISO_INVITO_RIFIUTATO,
        AVVISO_INVITO_SENZA_RISPOSTA,
        AVVISO_INVITO_SCADUTO,
        AVVISO_RICHIESTA_MANDATA,
        AVVISO_RICHIESTA_RIFIUTATA,
        AVVISO_RICHIESTA_SENZA_RISPOSTA,
        AVVISO_RICHIESTA_SCADUTA,
        AVVISO_ASSOCIATO,
        AVVISO_ASSOCIAZIONE_ANNULLATA,
        AVVISO_CALIBRAZIONE_ACCESA,
        AVVISO_CALIBRAZIONE_SPENTA,
        AVVISO_NON_TI_RICONOSCE,
        AVVISO_DIRITTI_ASCOLTA,
        AVVISO_DIRITTI_COMANDA,
        AVVISO_DIRITTI_TUTTO,
    ] {
        let resto = chiave.strip_prefix("together.notice.").expect(chiave);
        assert!(note.contains(&resto), "{chiave}");
    }
}

#[test]
fn il_lettore_che_non_entra_fa_uscire_e_spegne_i_ping_dopo_averli_accesi() {
    let s = membro();
    let (s, fx) = passo(s, Evento::LettoreRifiuta, 300);
    assert_eq!(
        fx,
        vec![
            manda(HOST, Messaggio::Esco),
            Effetto::Sincronizza {
                id: HOST,
                attivo: false
            },
            Effetto::EsciDalLettore,
        ]
    );
    assert_eq!(s.fase, Fase::Uscendo { spegni: false });
    // E nel passo che fa entrare, l'entrata del lettore è l'ultimo
    // effetto: i ping spenti dal rifiuto vengono dopo quelli accesi.
    let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
    let numero = numeri_degli_inviti(&s)[0];
    let (s, _) = passo(
        s,
        Evento::Comando(Comando::Rispondi {
            invito: numero,
            accetta: true,
        }),
        100,
    );
    let (_, fx) = passo(
        s,
        Evento::Ricevuto {
            id: HOST,
            messaggio: Messaggio::Ammesso {
                membro: 1,
                diritti: Diritti::Tutto,
            },
        },
        200,
    );
    assert_eq!(fx.last(), Some(&Effetto::EntraComeMembro { host: HOST }));
}

// ── che cosa può fare chi entra ─────────────────────────────────────────

fn diritti_di(s: &Situazione, membro: u32) -> Option<Diritti> {
    persone_di_qui(s, "Sala")
        .into_iter()
        .find(|p| p.membro == membro)
        .map(|p| p.diritti)
}

fn gesto(numero: u32, azione: Azione) -> Evento {
    Evento::Ricevuto {
        id: IdConnessione(100),
        messaggio: Messaggio::Comando { id: numero, azione },
    }
}

fn non_concesso(numero: u32) -> Effetto {
    manda(
        IdConnessione(100),
        Messaggio::Esito {
            id: numero,
            codice: Some("insieme.nonConcesso".to_owned()),
        },
    )
}

fn concedi(membro: u32, diritti: Diritti) -> Evento {
    Evento::Comando(Comando::Concedi { membro, diritti })
}

#[test]
fn l_invito_porta_il_suo_livello_fino_all_ammesso() {
    for diritti in [Diritti::Ascolta, Diritti::Comanda, Diritti::Tutto] {
        let (s, fx) = invita_e_accetta_con(host_con(1), IdConnessione(100), 100, diritti);
        assert_eq!(
            mandati(&fx),
            vec![(
                IdConnessione(100),
                Messaggio::Ammesso { membro: 1, diritti }
            )]
        );
        assert_eq!(diritti_di(&s, 1), Some(diritti));
        assert_eq!(diritti_di(&s, 0), Some(Diritti::Tutto), "chi ospita");
    }
}

/// Un invito e una richiesta che si incrociano non sono un secondo sì:
/// il PC resta col livello con cui è entrato.
#[test]
fn rientrare_una_seconda_volta_non_cambia_il_livello() {
    let mut s = invita_e_accetta_con(host_con(1), IdConnessione(100), 100, Diritti::Ascolta).0;
    let mut fx = Vec::new();
    assert!(ammetti(&mut s, IdConnessione(100), Diritti::Tutto, &mut fx));
    assert_eq!(
        mandati(&fx),
        vec![(
            IdConnessione(100),
            Messaggio::Ammesso {
                membro: 1,
                diritti: Diritti::Ascolta
            }
        )]
    );
    assert_eq!(diritti_di(&s, 1), Some(Diritti::Ascolta));
}

#[test]
fn concedere_cambia_il_livello_e_rimanda_lo_stato_una_volta() {
    let s = invita_e_accetta_con(host_con(1), IdConnessione(100), 100, Diritti::Ascolta).0;
    let (s, fx) = passo(s, concedi(1, Diritti::Tutto), 3_000);
    assert_eq!(fx, vec![Effetto::PersoneCambiate]);
    assert_eq!(diritti_di(&s, 1), Some(Diritti::Tutto));
    // Lo stesso livello di nuovo: niente da rimandare.
    let (s, fx) = passo(s, concedi(1, Diritti::Tutto), 3_100);
    assert!(fx.is_empty(), "{fx:?}");
    // Un membro che non c'è più: niente, come per «Allontana».
    let (_, fx) = passo(s, concedi(9, Diritti::Ascolta), 3_200);
    assert!(fx.is_empty(), "{fx:?}");
    // Fuori dalla sessione non c'è niente da concedere.
    let (_, fx) = passo(libero_con_host(), concedi(1, Diritti::Tutto), 0);
    assert_eq!(errori(&fx), vec!["insieme.gestoNonDisponibile"]);
}

/// La porta è qui: chi ascolta non comanda e chi comanda non tocca la
/// coda, e i no non costano gettoni del secchiello.
#[test]
fn la_porta_dell_host_ferma_quel_che_non_e_concesso_prima_del_secchiello() {
    let mut s = invita_e_accetta_con(host_con(1), IdConnessione(100), 100, Diritti::Ascolta).0;
    let aggiungi = Azione::Aggiungi {
        voci: vec![],
        dove: rymembra_insieme::messaggi::Dove::InFondo,
        suona: false,
    };
    // Venti no nello stesso istante: tutti col perché, nessuno eseguito.
    for numero in 0..20 {
        let (dopo, fx) = passo(s, gesto(numero, Azione::Pausa), 5_000);
        assert_eq!(fx, vec![non_concesso(numero)]);
        s = dopo;
    }
    let (s, fx) = passo(s, gesto(20, aggiungi.clone()), 5_000);
    assert_eq!(fx, vec![non_concesso(20)], "nessuna voce registrata");
    // Comanda: la pausa sì, dieci di fila, perché il secchiello è pieno.
    let (mut s, _) = passo(s, concedi(1, Diritti::Comanda), 5_000);
    for numero in 30..40 {
        let (dopo, fx) = passo(s, gesto(numero, Azione::Pausa), 5_000);
        assert!(
            matches!(fx.as_slice(), [Effetto::Esegui { comando, .. }] if *comando == numero),
            "{fx:?}"
        );
        s = dopo;
    }
    // Ma non la coda.
    let (s, fx) = passo(s, gesto(40, Azione::Vai { uid: 7 }), 5_100);
    assert_eq!(fx, vec![non_concesso(40)]);
    let (s, fx) = passo(s, gesto(41, aggiungi), 5_100);
    assert_eq!(fx, vec![non_concesso(41)]);
    // Tutto: anche la coda.
    let (s, _) = passo(s, concedi(1, Diritti::Tutto), 5_200);
    let (_, fx) = passo(s, gesto(42, Azione::Vai { uid: 7 }), 5_200);
    assert!(
        matches!(fx.as_slice(), [Effetto::Esegui { comando: 42, .. }]),
        "{fx:?}"
    );
}

#[test]
fn chi_segue_sente_il_livello_nuovo_una_volta_sola() {
    // Questo PC è il membro 3, entrato con `Tutto` (vedi `membro`).
    let stato = |versione, diritti| {
        Messaggio::Stato(StatoSessione {
            versione,
            persone: vec![
                Persona {
                    membro: 0,
                    nome: "Sala".to_owned(),
                    diritti: Diritti::Tutto,
                },
                Persona {
                    membro: 3,
                    nome: "Io".to_owned(),
                    diritti,
                },
            ],
            coda: vec![],
            corrente: None,
            in_pausa: false,
            ripeti: Ripeti::No,
        })
    };
    let arriva = |s, messaggio| {
        passo(
            s,
            Evento::Ricevuto {
                id: HOST,
                messaggio,
            },
            0,
        )
    };
    let (s, fx) = arriva(membro(), stato(1, Diritti::Tutto));
    assert!(!fx.iter().any(|e| matches!(e, Effetto::DirittiCambiati(_))));
    assert!(avvisi(&fx).is_empty());
    let (s, fx) = arriva(s, stato(2, Diritti::Ascolta));
    assert!(fx.contains(&Effetto::DirittiCambiati(Diritti::Ascolta)));
    assert!(fx.contains(&Effetto::Avviso {
        chiave: AVVISO_DIRITTI_ASCOLTA.to_owned(),
        nome: Some("Sala".to_owned()),
    }));
    assert!(matches!(&s.fase, Fase::Membro(m) if m.diritti == Diritti::Ascolta));
    // Lo stesso livello nello stato dopo: niente.
    let (s, fx) = arriva(s, stato(3, Diritti::Ascolta));
    assert!(avvisi(&fx).is_empty(), "{fx:?}");
    // Uno stato che non nomina questo PC lo lascia com'era.
    let (s, fx) = arriva(s, stato_con(4, &[(1, "Marco")]));
    assert!(
        !avvisi(&fx).contains(&AVVISO_DIRITTI_TUTTO.to_owned()),
        "{fx:?}"
    );
    assert!(matches!(&s.fase, Fase::Membro(m) if m.diritti == Diritti::Ascolta));
}

#[test]
fn l_ammesso_dice_gia_che_cosa_si_puo_fare() {
    let s = invito_arrivato(libero_con_host(), HOST, 7, 0);
    let numero = numeri_degli_inviti(&s)[0];
    let (s, _) = passo(
        s,
        Evento::Comando(Comando::Rispondi {
            invito: numero,
            accetta: true,
        }),
        100,
    );
    let (s, _) = passo(
        s,
        Evento::Ricevuto {
            id: HOST,
            messaggio: Messaggio::Ammesso {
                membro: 2,
                diritti: Diritti::Comanda,
            },
        },
        200,
    );
    assert!(matches!(&s.fase, Fase::Membro(m) if m.diritti == Diritti::Comanda));
}
