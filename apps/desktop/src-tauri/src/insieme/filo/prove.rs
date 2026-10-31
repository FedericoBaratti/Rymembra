//! Le prove del filo della sessione. Alcune leggono il sorgente (`corpo_di`):
//! per loro il testo è quello del padre e dei suoi sottomoduli.

use super::ospita::{codice_sul_filo, esito_dell_aggiunta, prossima_da_preparare};
use super::*;
use rymembra_insieme::messaggi::{Persona, SorgenteSulFilo};
use rymembra_insieme::{CampioneOrologio, Impronta};
use std::collections::VecDeque;

fn fidati() -> Fidati {
    Fidati::da(vec![rymembra_app::insieme::Pari {
        id: 4,
        chiave: ChiavePubblica([4; 32]).in_testo(),
        nome: "PC di Marco".to_owned(),
        bloccato: false,
        associato_ms: 1,
        visto_ms: None,
    }])
}

#[test]
fn la_voce_che_suona_passa_davanti_e_le_sessioni_vecchie_si_buttano() {
    let mut in_fila: VecDeque<DaPreparare> = VecDeque::from([
        DaPreparare::Blocco {
            sessione: 1,
            uids: vec![1, 2],
        },
        DaPreparare::Blocco {
            sessione: 2,
            uids: vec![10, 11],
        },
        DaPreparare::Blocco {
            sessione: 2,
            uids: vec![12, 13],
        },
        DaPreparare::Subito {
            sessione: 2,
            uid: 40,
        },
    ]);
    assert_eq!(prossima_da_preparare(&mut in_fila), Some((2, vec![40])));
    assert_eq!(prossima_da_preparare(&mut in_fila), Some((2, vec![10, 11])));
    assert_eq!(prossima_da_preparare(&mut in_fila), Some((2, vec![12, 13])));
    assert_eq!(prossima_da_preparare(&mut in_fila), None);
}

#[test]
fn alla_finestra_vanno_solo_chiavi_della_sessione() {
    let della_sessione = AppError::new(ErrorCode::InsiemeIdentitaPersa);
    let del_database = AppError::new(ErrorCode::DbLocked);
    assert_eq!(
        chiave_per_la_finestra(&della_sessione, None),
        Some("errors.insieme.identitaPersa")
    );
    assert_eq!(chiave_per_la_finestra(&del_database, None), None);
    assert_eq!(
        chiave_per_la_finestra(&del_database, Some(ErrorCodeKind::InsiemeRifiutato)),
        Some("errors.insieme.rifiutato")
    );
}

#[test]
fn il_diario_della_rete_non_ha_indirizzi_ne_nomi_ne_codici() {
    let indirizzo: SocketAddr = "192.168.1.57:47811".parse().unwrap();
    let eventi = [
        rymembra_insieme::Evento::Aperta {
            id: IdConnessione(3),
            pari: ChiavePubblica([4; 32]),
            nome: "PC di Marco".to_owned(),
            ospita: true,
            in_entrata: false,
            indirizzo,
        },
        rymembra_insieme::Evento::Aperta {
            id: IdConnessione(5),
            pari: ChiavePubblica([9; 32]),
            nome: "Portatile di Luca".to_owned(),
            ospita: false,
            in_entrata: true,
            indirizzo,
        },
        rymembra_insieme::Evento::Codice {
            id: IdConnessione(5),
            codice: "4821".to_owned(),
        },
        rymembra_insieme::Evento::Chiusa {
            id: IdConnessione(3),
            codice: Some(ErrorCodeKind::InsiemeConnessioneCaduta),
        },
        rymembra_insieme::Evento::Associato {
            id: IdConnessione(5),
            pari: ChiavePubblica([9; 32]),
        },
        rymembra_insieme::Evento::Trovato {
            indirizzo,
            impronta: Impronta([1; 8]),
        },
        rymembra_insieme::Evento::Orologio {
            id: IdConnessione(3),
            campione: CampioneOrologio {
                t1: 1,
                t2: 2,
                t3: 3,
                t4: 4,
            },
        },
    ];
    let chiave_hex = ChiavePubblica([9; 32]).in_testo();
    let impronta = ChiavePubblica([9; 32]).impronta().to_string();
    let mut righe = Vec::new();
    for evento in &eventi {
        if let Some(riga) = riga_della_rete(evento, &fidati()) {
            for vietato in [
                "192.168",
                "47811",
                "Marco",
                "Luca",
                "4821",
                chiave_hex.as_str(),
                &chiave_hex[..16],
                impronta.as_str(),
            ] {
                assert!(!riga.contains(vietato), "«{vietato}» in «{riga}»");
            }
            righe.push(riga);
        }
    }
    assert_eq!(righe.len(), 5, "{righe:#?}");
    assert!(
        righe[0].contains("pari #4"),
        "il fidato per numero: {}",
        righe[0]
    );
    assert!(righe[1].contains("estraneo"));
    assert!(righe[3].contains("insieme.connessioneCaduta"));
}

fn voce(uid: u64, di: u32) -> VoceSulFilo {
    VoceSulFilo {
        uid,
        di,
        titolo: "t".to_owned(),
        artista: "a".to_owned(),
        album: "b".to_owned(),
        durata_ms: 1,
        numero: None,
        anno: None,
        copertina: None,
        sorgente: SorgenteSulFilo::Catalogo {
            riferimento: "x".to_owned(),
        },
        replaygain_db: None,
        picco: None,
        copia: None,
    }
}

#[test]
fn nel_lettore_chi_segue_vede_di_chi_e_la_voce_e_i_suoi_diritti() {
    let persone = vec![
        Persona {
            membro: 0,
            nome: "Sala".to_owned(),

            diritti: Diritti::Tutto,
        },
        Persona {
            membro: 2,
            nome: "Io".to_owned(),

            diritti: Diritti::Tutto,
        },
        Persona {
            membro: 3,
            nome: "Marco".to_owned(),

            diritti: Diritti::Tutto,
        },
    ];
    let dettagli = |agganciato, locale, calibrazione| Dettagli {
        agganciato,
        locale,
        calibrazione,
        revisione: 0,
    };
    let n = nel_lettore_di_chi_segue(
        "Sala (qui)",
        &persone,
        2,
        Diritti::Comanda,
        Some(&voce(1, 0)),
        dettagli(false, false, false),
    );
    assert!(!n.ospita);
    assert_eq!(n.host, "Sala (qui)");
    assert_eq!(n.persone, 3);
    assert_eq!(n.proprietario.as_deref(), Some("Sala"));
    assert!(!n.locale && !n.agganciato && !n.calibrazione);
    assert_eq!(
        n.diritti,
        Diritti::Comanda,
        "quel che chi ospita ha concesso"
    );
    let mia = nel_lettore_di_chi_segue(
        "Sala",
        &persone,
        2,
        Diritti::Tutto,
        Some(&voce(1, 2)),
        dettagli(true, true, true),
    );
    assert_eq!(mia.proprietario, None, "una voce mia non ha proprietario");
    assert!(mia.agganciato && mia.locale && mia.calibrazione);
    let niente = nel_lettore_di_chi_segue(
        "Sala",
        &persone,
        2,
        Diritti::Ascolta,
        None,
        dettagli(true, false, false),
    );
    assert_eq!(niente.proprietario, None);
}

#[test]
fn il_ruolo_va_e_torna_dall_atomico_coi_diritti() {
    for ruolo in [
        Ruolo::Fuori,
        Ruolo::Host,
        Ruolo::Membro(Diritti::Ascolta),
        Ruolo::Membro(Diritti::Comanda),
        Ruolo::Membro(Diritti::Tutto),
    ] {
        assert_eq!(ruolo_dal_byte(ruolo_in_byte(ruolo)), ruolo);
    }
    assert_eq!(ruolo_dal_byte(200), Ruolo::Fuori);
}

#[test]
fn nel_lettore_chi_ospita_e_sempre_agganciato_e_ha_i_gesti() {
    let dettagli = Dettagli {
        agganciato: false,
        locale: true,
        calibrazione: true,
        revisione: 0,
    };
    let n = nel_lettore_di_chi_ospita("Sala", 3, None, dettagli);
    assert!(n.ospita && n.agganciato && n.locale && n.calibrazione);
    assert_eq!(n.diritti, Diritti::Tutto);
    assert_eq!(n.proprietario, None);
    assert_eq!(n.persone, 3);
    let n = nel_lettore_di_chi_ospita("Sala", 3, Some("Marco".to_owned()), dettagli);
    assert_eq!(n.proprietario.as_deref(), Some("Marco"));
}

#[test]
fn il_campo_del_lettore_si_ridisegna_solo_quando_cambia() {
    let dettagli = Dettagli {
        agganciato: true,
        locale: false,
        calibrazione: false,
        revisione: 0,
    };
    let una = Some(nel_lettore_di_chi_ospita("Sala", 2, None, dettagli));
    assert!(
        !diversi(&una, &una.clone()),
        "uguale: niente da ridisegnare"
    );
    assert!(diversi(&una, &None), "all'uscita sì");
    assert!(diversi(&None, &una), "all'ingresso sì");
    let calibrando = Some(nel_lettore_di_chi_ospita(
        "Sala",
        2,
        None,
        Dettagli {
            calibrazione: true,
            revisione: 0,
            ..dettagli
        },
    ));
    assert!(diversi(&una, &calibrando));
}

#[test]
fn chi_ospita_dice_di_chi_e_la_voce_che_suona_solo_se_e_di_un_membro() {
    let persone = vec![
        Persona {
            membro: 0,
            nome: "Sala".to_owned(),

            diritti: Diritti::Tutto,
        },
        Persona {
            membro: 4,
            nome: "Marco".to_owned(),

            diritti: Diritti::Tutto,
        },
    ];
    let mut servite = Servite::default();
    let mut di_marco = voce(1_099_511_627_777, 4);
    di_marco.sorgente = SorgenteSulFilo::File {
        gettone: Gettone([7; 16]),
        estensione: None,
        dimensione: 1,
        segmento: None,
    };
    servite.ospita_voce(di_marco).unwrap();
    assert_eq!(
        proprietario_di_chi_ospita(Some(1_099_511_627_777), &servite, &persone).as_deref(),
        Some("Marco")
    );
    // Una voce di questo PC (che `Servite` descrive con `di: 0`), una che
    // non c'è, e nessuna corrente: nessun proprietario.
    assert_eq!(
        proprietario_di_chi_ospita(Some(3), &servite, &persone),
        None
    );
    assert_eq!(proprietario_di_chi_ospita(None, &servite, &persone), None);
    assert_eq!(
        proprietario_di_chi_ospita(Some(ponte::UID_CALIBRAZIONE), &servite, &persone),
        None
    );
}

#[test]
fn i_proprietari_sono_le_voci_altrui_in_coda_col_nome_del_loro_pc() {
    let persone = vec![
        Persona {
            membro: 0,
            nome: "Sala".to_owned(),

            diritti: Diritti::Tutto,
        },
        Persona {
            membro: 4,
            nome: "Marco".to_owned(),

            diritti: Diritti::Tutto,
        },
    ];
    // Le voci 1 e 3 sono di altri, la 2 di questo PC; la 9 di un membro
    // che non c'è più non si nomina.
    let di = |uid: u64| match uid {
        1 => Some((-11, 0)),
        3 => Some((-13, 4)),
        9 => Some((-19, 7)),
        _ => None,
    };
    let p = proprietari(&[1, 2, 3, 9], &persone, di);
    let coppie: Vec<(i64, &str)> = p.iter().map(|x| (x.id, x.nome.as_str())).collect();
    assert_eq!(coppie, vec![(-11, "Sala"), (-13, "Marco")]);
}

/// Il corpo di una funzione di `filo` o dei suoi sottomoduli, senza le prove.
fn corpo_di(firma: &str) -> &'static str {
    let filo = concat!(
        include_str!("rete.rs"),
        include_str!("ospita.rs"),
        include_str!("membro.rs"),
        include_str!("../filo.rs"),
    );
    let filo = filo.split("\n#[cfg(test)]").next().unwrap_or(filo);
    let da = filo.find(firma).expect(firma);
    let resto = &filo[da..];
    &resto[..resto.find("\n    }\n").unwrap_or(resto.len())]
}

/// Blocca e Dimentica con la libreria occupata: il cambio vale in memoria
/// e nel trasporto anche se la rilettura non riesce, nell'ordine che il
/// commento sopra i due rami vuole (Fidati, poi `PariTolto`, poi
/// `Rete::pari`); e una rilettura non riuscita si dice e si ritenta.
#[test]
fn bloccare_e_dimenticare_non_dipendono_dalla_rilettura() {
    let comando = corpo_di("fn comando(&mut self");
    let blocca = &comando[comando.find("Richiesta::Blocca").expect("Blocca")..];
    let in_memoria = blocca.find("self.fidati.segna_bloccato(id, bloccato)");
    let tolto = blocca.find("Evento::PariTolto");
    let rete = blocca.find("self.ricarica_fidati()");
    assert!(
        in_memoria.is_some() && in_memoria < tolto && tolto < rete,
        "Blocca: {in_memoria:?} {tolto:?} {rete:?}"
    );
    let dimentica = &comando[comando.find("Richiesta::Dimentica").expect("Dimentica")..];
    let in_memoria = dimentica.find("self.fidati.togli(id)");
    let tolto = dimentica.find("Evento::PariTolto");
    let rete = dimentica.find("self.ricarica_fidati()");
    assert!(
        in_memoria.is_some() && in_memoria < tolto && tolto < rete,
        "Dimentica: {in_memoria:?} {tolto:?} {rete:?}"
    );
    let ricarica = corpo_di("fn ricarica_fidati(");
    assert!(
        ricarica.contains("self.fidati_da_rileggere = Some("),
        "una rilettura non riuscita si ritenta"
    );
    assert!(ricarica.contains("nota!("), "e si scrive nel diario");
    assert!(corpo_di("fn scadenze(").contains("self.rileggi_se_tocca(adesso)"));
    assert!(corpo_di("fn rileggi_se_tocca(").contains(".fidati_da_rileggere"));
    assert!(corpo_di("fn attesa(").contains("self.fidati_da_rileggere"));
    // E la rete non parte con dei fidati mai letti.
    let accendi = corpo_di("fn accendi(");
    let guardia = accendi.find("if !self.fidati_letti").expect("la guardia");
    let avvia = accendi.find("Rete::avvia(").expect("l'avvio");
    assert!(guardia < avvia);
}

/// Chi ospita libera le copertine delle voci dei membri che escono dalla
/// coda (il campanello), di chi esce, e delle aggiunte rifiutate o rimaste
/// fuori; chi segue toglie dalle «chieste» i gettoni liberati, così una
/// voce che torna richiede la sua copertina.
#[test]
fn le_copertine_delle_voci_uscite_si_liberano_anche_da_chi_ospita() {
    for firma in [
        "fn campanello(",
        "fn membro_uscito(",
        "fn togli_l_aggiunta(",
    ] {
        assert!(
            corpo_di(firma).contains("self.dimentica_le_copertine("),
            "{firma}"
        );
    }
    assert!(corpo_di("fn specchia(").contains("dal.chieste.remove("));
}

/// L'uscita dal lettore finisce sempre con `Uscito`, anche se il lettore
/// cade in panico mentre rimette la coda di prima.
#[test]
fn l_uscita_dal_lettore_arriva_a_uscito_anche_dopo_un_panico() {
    let esci = corpo_di("fn esci_dal_lettore(");
    let sorvegliata = esci
        .find("crate::stato::sorvegliato(\"rymembra-insieme\", || ponte::esci(&self.app));")
        .expect("ponte::esci sorvegliata");
    let uscito = esci.find("self.nutri(Evento::Uscito)").expect("Uscito");
    assert!(sorvegliata < uscito);
}

#[test]
fn un_aggiunta_del_membro_a_coda_piena_risponde_coda_piena() {
    let a = UID_DEI_MEMBRI + 1;
    let b = UID_DEI_MEMBRI + 2;
    let voci = [(a, -5), (b, -6)];
    let piena = Some("insieme.codaPiena".to_owned());
    // Coda piena: niente dentro, niente «vai» (direbbe «superato»), il
    // codice al membro e tutte e due da dimenticare.
    let mut vai = Vec::new();
    let esito = esito_dell_aggiunta(&voci, 0, Ok(()), Some(a), |uid| {
        vai.push(uid);
        None
    });
    assert_eq!(esito, (piena.clone(), vec![a, b]));
    assert!(vai.is_empty(), "nessun «vai» su una voce rimasta fuori");
    // Metà dentro: la prima suona, la seconda resta fuori, e il membro lo
    // sa lo stesso.
    let esito = esito_dell_aggiunta(&voci, 1, Ok(()), Some(a), |uid| {
        vai.push(uid);
        None
    });
    assert_eq!(esito, (piena, vec![b]));
    assert_eq!(vai, vec![a]);
    // Tutte dentro: l'esito è quello del «vai», o niente.
    let esito = esito_dell_aggiunta(&voci, 2, Ok(()), Some(a), |_| {
        Some("insieme.superato".to_owned())
    });
    assert_eq!(esito, (Some("insieme.superato".to_owned()), vec![]));
    assert_eq!(
        esito_dell_aggiunta(&voci, 2, Ok(()), None, |_| unreachable!()),
        (None, vec![])
    );
    // Un errore dopo l'accoda (l'apertura della prima, a coda vuota): le
    // voci entrate restano, si dimenticano solo le altre.
    let esito = esito_dell_aggiunta(
        &voci,
        1,
        Err("insieme.irraggiungibile".to_owned()),
        Some(a),
        |_| unreachable!(),
    );
    assert_eq!(esito, (Some("insieme.irraggiungibile".to_owned()), vec![b]));
}

#[test]
fn solo_i_codici_della_sessione_viaggiano_nell_esito() {
    assert_eq!(
        codice_sul_filo("insieme.gestoNonDisponibile"),
        "insieme.gestoNonDisponibile"
    );
    assert_eq!(codice_sul_filo("db.locked"), "insieme.rifiutato");
    assert_eq!(codice_sul_filo("internal.aborted"), "insieme.rifiutato");
}

#[test]
fn la_vetrina_mette_le_scadenze_sull_orologio_di_parete() {
    let mut s = Situazione::default();
    (s, _) = fasi::passo(s, Evento::Acceso, 0);
    (s, _) = fasi::passo(
        s,
        Evento::Aperta {
            id: IdConnessione(3),
            connessione: Connessione {
                chiave: ChiavePubblica([4; 32]),
                nome: "PC di Marco".to_owned(),
                dichiarato: "Marco".to_owned(),
                ospita: true,
                in_entrata: false,
                pari: Some(4),
            },
        },
        0,
    );
    (s, _) = fasi::passo(
        s,
        Evento::Ricevuto {
            id: IdConnessione(3),
            messaggio: Messaggio::Invito {
                invito: 1,
                scade_fra_ms: 60_000,
            },
        },
        1_000,
    );
    let impostazioni = rymembra_app::insieme::Impostazioni {
        attivo: true,
        ..Default::default()
    };
    let fidati = fidati();
    let vicinato = Vicinato::default();
    let v = vetrina(&Parti {
        s: &s,
        impostazioni: &impostazioni,
        nome: "Io",
        fidati: &fidati,
        vicinato: &vicinato,
        aggancio: None,
        calibrazione: false,
        proprietari: &[],
        indirizzi_di_qui: &[],
        adesso_ms: 1_700_000_000_000,
        ora: 11_000,
    });
    assert_eq!(v.fase, FaseIpc::Libera);
    assert_eq!(v.inviti.len(), 1);
    assert_eq!(v.inviti[0].scade_ms, 1_700_000_000_000 + 50_000);
    assert_eq!(v.inviti[0].da, "PC di Marco");
    assert_eq!(v.inviti[0].pari, 4);
    assert!(v.fidati[0].presente && v.fidati[0].ospita && !v.fidati[0].in_sessione);
    assert!(v.sincronia.is_none());
}

#[test]
fn la_vetrina_arrotonda_l_errore_al_decimo_di_millisecondo() {
    let s = Situazione::default();
    let impostazioni = rymembra_app::insieme::Impostazioni::default();
    let fidati = Fidati::default();
    let vicinato = Vicinato::default();
    let v = vetrina(&Parti {
        s: &s,
        impostazioni: &impostazioni,
        nome: "Io",
        fidati: &fidati,
        vicinato: &vicinato,
        calibrazione: false,
        proprietari: &[],
        indirizzi_di_qui: &[],
        aggancio: Some(Aggancio {
            agganciato: true,
            errore_ns: Some(1_234_567),
            incertezza_ns: Some(260_000),
        }),
        adesso_ms: 0,
        ora: 0,
    });
    let sincronia = v.sincronia.unwrap();
    assert_eq!(sincronia.errore_ms, Some(1.2));
    assert_eq!(sincronia.incertezza_ms, Some(0.3));
}
