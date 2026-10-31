//! I pezzi del riduttore: quel che `passo` fa per ogni evento, messaggio e gesto.
//!
//! Quando un commento qui dice «vedi il `//!`», è quello di `fasi`, il modulo padre.

use std::collections::BTreeMap;

use rymembra_domain::errors::{AppError, Domain, ErrorCode, ErrorCodeKind};
use rymembra_insieme::IdConnessione;
use rymembra_insieme::messaggi::{Diritti, MASSIMO_PERSONE, Messaggio, Persona};

use super::{
    ATTESA_AMMISSIONE_MS, AVVISO_ALLONTANATO, AVVISO_ASSOCIAZIONE_ANNULLATA,
    AVVISO_CALIBRAZIONE_ACCESA, AVVISO_CALIBRAZIONE_SPENTA, AVVISO_CHIUSA, AVVISO_ENTRATO,
    AVVISO_FINITA, AVVISO_HOST_SPARITO, AVVISO_INVITO_MANDATO, AVVISO_INVITO_RIFIUTATO,
    AVVISO_INVITO_SCADUTO, AVVISO_INVITO_SENZA_RISPOSTA, AVVISO_NON_TI_RICONOSCE,
    AVVISO_RICHIESTA_MANDATA, AVVISO_RICHIESTA_RIFIUTATA, AVVISO_RICHIESTA_SCADUTA,
    AVVISO_RICHIESTA_SENZA_RISPOSTA, AVVISO_SEI_DENTRO, AVVISO_SEI_USCITO,
    AVVISO_TI_HA_ALLONTANATO, AVVISO_USCITO, Associazione, CODICE_ALLONTANATO, Comando, Effetto,
    Fase, InvitoMandato, InvitoRicevuto, MembroDiQui, Ospitata, RichiestaRicevuta,
    SCADENZA_ASSOCIAZIONE_MS, SCADENZA_INVITO_MS, SCADENZA_RICHIESTA_MS, Seguita, Situazione, Via,
    avviso_dei_diritti, chiave_del_codice,
};
use crate::insieme::gesti::Secchiello;
use crate::riproduzione::insieme::UID_CALIBRAZIONE;

// ── i pezzi ─────────────────────────────────────────────────────────────────

pub(super) fn manda(a: IdConnessione, messaggio: Messaggio) -> Effetto {
    Effetto::Manda { a, messaggio }
}

pub(super) fn avviso(chiave: &str, nome: Option<String>) -> Effetto {
    Effetto::Avviso {
        chiave: chiave.to_owned(),
        nome,
    }
}

fn errore(codice: ErrorCode) -> Effetto {
    Effetto::Errore(AppError::new(codice))
}

fn rifiutato(codice: ErrorCodeKind) -> Messaggio {
    Messaggio::Rifiutato {
        codice: codice.code().to_owned(),
    }
}

fn nome_di(s: &Situazione, id: IdConnessione) -> Option<String> {
    s.connessioni.get(&id).map(|conn| conn.nome.clone())
}

/// Il tetto: chi ospita più i membri, contro `MASSIMO_PERSONE`.
fn piena(o: &Ospitata) -> bool {
    o.membri.len().saturating_add(1) >= MASSIMO_PERSONE
}

fn sessione_piena() -> ErrorCode {
    ErrorCode::InsiemeSessionePiena {
        massimo: u32::try_from(MASSIMO_PERSONE).unwrap_or(u32::MAX),
    }
}

fn non_disponibile(gesto: &str) -> ErrorCode {
    ErrorCode::InsiemeGestoNonDisponibile {
        gesto: gesto.to_owned(),
    }
}

/// Chi ospita: fa entrare `conn` come membro con quei `diritti`, se c'è posto.
pub(super) fn ammetti(
    s: &mut Situazione,
    conn: IdConnessione,
    diritti: Diritti,
    fx: &mut Vec<Effetto>,
) -> bool {
    let nome = nome_di(s, conn);
    let Fase::Ospita(o) = &mut s.fase else {
        return false;
    };
    // Già dentro: lo stesso `Ammesso` di prima, col livello con cui è entrato.
    // Succede quando la risposta a un invito e una richiesta dello stesso PC
    // si incrociano: per cambiarlo c'è «In sessione», non un secondo sì.
    if let Some(gia) = o.membri.iter().find(|m| m.conn == conn) {
        fx.push(manda(
            conn,
            Messaggio::Ammesso {
                membro: gia.membro,
                diritti: gia.diritti,
            },
        ));
        return true;
    }
    if piena(o) {
        fx.push(manda(conn, rifiutato(ErrorCodeKind::InsiemeSessionePiena)));
        return false;
    }
    let membro = o.prossimo;
    o.prossimo = o.prossimo.saturating_add(1);
    o.membri.push(MembroDiQui {
        membro,
        conn,
        secchiello: Secchiello::default(),
        diritti,
    });
    // Un PC dentro non ha più niente da chiedere né da rispondere.
    o.inviti.retain(|i| i.a != conn);
    o.richieste.retain(|r| r.da != conn);
    fx.push(manda(conn, Messaggio::Ammesso { membro, diritti }));
    fx.push(Effetto::PersoneCambiate);
    fx.push(avviso(AVVISO_ENTRATO, nome));
    true
}

/// Chi ospita: toglie `conn` dai membri. Il suo numero, se c'era.
fn togli_membro(o: &mut Ospitata, conn: IdConnessione) -> Option<u32> {
    let membro = o.membri.iter().find(|m| m.conn == conn)?.membro;
    o.membri.retain(|m| m.conn != conn);
    Some(membro)
}

/// Chi ospita chiude la sessione: a chi dirlo, ognuno una volta. I membri, e
/// poi chi ha una richiesta in sospeso o un invito: `SmettiDiOspitare` chiude
/// le loro connessioni senza un perché, e chi stava entrando leggeva «non
/// risponde più», come per un PC caduto, invece di «ha terminato». La
/// chiusura è cortese, e il `Fine` arriva prima. A chi non ha ancora risposto
/// a un invito il `Fine` non dice niente (è libero, e lo ignora): l'invito
/// sparisce con la connessione.
fn a_chi_dire_fine(o: &Ospitata) -> Vec<IdConnessione> {
    let mut a_chi: Vec<IdConnessione> = Vec::new();
    let tutti = o
        .membri
        .iter()
        .map(|m| m.conn)
        .chain(o.richieste.iter().map(|r| r.da))
        .chain(o.inviti.iter().map(|i| i.a));
    for conn in tutti {
        if !a_chi.contains(&conn) {
            a_chi.push(conn);
        }
    }
    a_chi
}

pub(super) fn spegni(s: &mut Situazione, fx: &mut Vec<Effetto>) {
    match &s.fase {
        Fase::Spenta => return,
        Fase::Ospita(o) => {
            for conn in a_chi_dire_fine(o) {
                fx.push(manda(conn, Messaggio::Fine { codice: None }));
            }
            fx.push(Effetto::EsciDalLettore);
            fx.push(Effetto::SmettiDiOspitare);
            fx.push(avviso(AVVISO_CHIUSA, None));
        }
        Fase::Membro(seguita) => {
            fx.push(manda(seguita.host, Messaggio::Esco));
            fx.push(Effetto::EsciDalLettore);
            fx.push(avviso(AVVISO_SEI_USCITO, None));
        }
        Fase::Entrando { host, via, .. } => {
            fx.push(manda(*host, ritiro(*via)));
        }
        Fase::Libera { inviti } => {
            for invito in inviti {
                fx.push(manda(
                    invito.da,
                    Messaggio::RispostaInvito {
                        invito: invito.invito,
                        accetta: false,
                    },
                ));
            }
        }
        // Un'uscita rimasta a metà. Ogni fase che mette `Uscendo` chiede
        // `EsciDalLettore` nello stesso passo, e il filo della sessione lo
        // esegue nello stesso giro, fino a `Uscito`; `Spegni` arriva sempre
        // in un giro suo (`ferma`, l'interruttore). Vista qui, `Uscendo` è il
        // resto di un giro caduto in panico a metà degli effetti: aspettare
        // un `Uscito` che non arriverà più voleva dire una sessione bloccata
        // su «uscendo» fino al riavvio. Si spegne e basta.
        Fase::Uscendo { .. } => {}
    }
    fx.push(Effetto::FermaRete);
    s.fase = match s.fase {
        // L'uscita del lettore appena chiesta qui sopra: la fase si chiude
        // quando torna `Uscito`.
        Fase::Ospita(_) | Fase::Membro(_) => Fase::Uscendo { spegni: true },
        _ => Fase::Spenta,
    };
    s.connessioni.clear();
    s.associazione = None;
}

/// Il messaggio con cui chi sta entrando ci ripensa.
///
/// Dopo un invito accettato è `Esco` e non una risposta negativa: l'host può
/// averlo già ammesso, e `Esco` vale in tutti e due i casi. Dopo una richiesta
/// è `NonChiedoPiu`; se l'host avesse già ammesso, l'`Ammesso` arriverà a un
/// PC libero, che risponde `Esco` da sé.
fn ritiro(via: Via) -> Messaggio {
    match via {
        Via::Invito(_) => Messaggio::Esco,
        Via::Richiesta => Messaggio::NonChiedoPiu,
    }
}

pub(super) fn chiusa(
    s: &mut Situazione,
    id: IdConnessione,
    codice: Option<ErrorCodeKind>,
    fx: &mut Vec<Effetto>,
) {
    let nome = nome_di(s, id);
    let pari = s.connessioni.remove(&id).and_then(|c| c.pari);
    // Per questo PC l'altro è associato, per l'altro no: l'ha dimenticato, o
    // ha cambiato identità. Senza dirlo, questo PC riprovava a collegarsi per
    // sempre e la persona vedeva un PC associato che non risponde.
    if codice == Some(ErrorCodeKind::InsiemeNonAssociato)
        && let Some(pari) = pari
        && s.non_riconoscono.insert(pari)
    {
        fx.push(avviso(AVVISO_NON_TI_RICONOSCE, nome.clone()));
    }
    if s.associazione.as_ref().is_some_and(|a| a.conn == id) {
        s.associazione = None;
        // Il codice diverso è un guasto da leggere; una connessione caduta a
        // metà è un'associazione che non c'è stata, e basta dirlo.
        let chiave = match codice {
            None
            | Some(
                ErrorCodeKind::InsiemeConnessioneCaduta | ErrorCodeKind::InsiemeIrraggiungibile,
            ) => AVVISO_ASSOCIAZIONE_ANNULLATA,
            Some(kind) if kind.domain() == Domain::Insieme => kind.i18n_key(),
            Some(_) => AVVISO_ASSOCIAZIONE_ANNULLATA,
        };
        fx.push(avviso(chiave, nome.clone()));
    }
    match &mut s.fase {
        Fase::Libera { inviti } => inviti.retain(|i| i.da != id),
        Fase::Entrando { host, .. } if *host == id => {
            fx.push(avviso(chiave_della_caduta(codice), nome));
            s.fase = Fase::Libera { inviti: Vec::new() };
        }
        Fase::Ospita(o) => {
            o.inviti.retain(|i| i.a != id);
            o.richieste.retain(|r| r.da != id);
            if let Some(membro) = togli_membro(o, id) {
                fx.push(Effetto::MembroUscito { membro });
                fx.push(Effetto::PersoneCambiate);
                fx.push(avviso(AVVISO_USCITO, nome));
            }
        }
        // L'host è sparito: connessione caduta o quindici secondi di
        // silenzio, che il trasporto già traduce in una chiusura. Niente
        // passaggio di ruolo: si esce e lo si dice.
        Fase::Membro(seguita) if seguita.host == id => {
            fx.push(Effetto::EsciDalLettore);
            fx.push(avviso(chiave_della_caduta(codice), nome));
            s.fase = Fase::Uscendo { spegni: false };
        }
        _ => {}
    }
}

/// L'avviso per l'host che si chiude sotto a chi segue o entra: una notizia
/// se la connessione è solo caduta (o tace da quindici secondi), il guasto
/// col suo codice se l'altro capo ha detto perché — una versione diversa, un
/// messaggio fuori regola.
fn chiave_della_caduta(codice: Option<ErrorCodeKind>) -> &'static str {
    match codice {
        Some(kind)
            if kind.domain() == Domain::Insieme
                && !matches!(
                    kind,
                    ErrorCodeKind::InsiemeConnessioneCaduta | ErrorCodeKind::InsiemeIrraggiungibile
                ) =>
        {
            kind.i18n_key()
        }
        _ => AVVISO_HOST_SPARITO,
    }
}

pub(super) fn ricevuto(
    s: &mut Situazione,
    id: IdConnessione,
    messaggio: Messaggio,
    adesso: u64,
    fx: &mut Vec<Effetto>,
) {
    // Il trasporto non consegna messaggi di chiavi non fidate; qui si guarda
    // lo stesso, perché un PC dimenticato un istante fa può avere ancora un
    // messaggio in volo.
    let Some(conn) = s.connessioni.get(&id) else {
        return;
    };
    if conn.pari.is_none() {
        return;
    }
    let nome = Some(conn.nome.clone());
    let da_un_host = conn.ospita && !conn.in_entrata;
    match messaggio {
        Messaggio::Invito {
            invito,
            scade_fra_ms,
        } => {
            match &mut s.fase {
                Fase::Libera { inviti } if da_un_host => {
                    inviti.retain(|i| i.da != id);
                    let numero = s.numeratore;
                    inviti.push(InvitoRicevuto {
                        numero,
                        invito,
                        da: id,
                        // La durata la dice chi invita, ma il tetto è nostro: un
                        // invito che resta nel banner per un'ora non è un invito.
                        scade: adesso.saturating_add(scade_fra_ms.min(SCADENZA_INVITO_MS)),
                    });
                    s.numeratore = s.numeratore.saturating_add(1);
                    fx.push(Effetto::MostraFinestra);
                }
                // In sessione, entrando o mentre si ospita: no, subito.
                _ => fx.push(manda(
                    id,
                    Messaggio::RispostaInvito {
                        invito,
                        accetta: false,
                    },
                )),
            }
        }
        Messaggio::RispostaInvito { invito, accetta } => {
            let Fase::Ospita(o) = &mut s.fase else {
                return;
            };
            // Il livello scelto con l'invito, se l'invito vale ancora.
            let atteso = o
                .inviti
                .iter()
                .find(|i| i.invito == invito && i.a == id && i.scade > adesso)
                .map(|i| i.diritti);
            o.inviti.retain(|i| !(i.invito == invito && i.a == id));
            if !accetta {
                if atteso.is_some() {
                    fx.push(avviso(AVVISO_INVITO_RIFIUTATO, nome));
                }
                return;
            }
            if let Some(diritti) = atteso {
                ammetti(s, id, diritti, fx);
            } else {
                // Scaduto, sostituito da uno più nuovo, o mai mandato: un sì
                // fuori tempo non fa entrare nessuno.
                fx.push(manda(id, rifiutato(ErrorCodeKind::InsiemeRifiutato)));
            }
        }
        Messaggio::Chiedi => match &mut s.fase {
            Fase::Ospita(o) if conn.in_entrata => {
                if o.membri.iter().any(|m| m.conn == id) {
                    return;
                }
                if piena(o) {
                    fx.push(manda(id, rifiutato(ErrorCodeKind::InsiemeSessionePiena)));
                    return;
                }
                o.richieste.retain(|r| r.da != id);
                let numero = s.numeratore;
                o.richieste.push(RichiestaRicevuta {
                    numero,
                    da: id,
                    scade: adesso.saturating_add(SCADENZA_RICHIESTA_MS),
                });
                s.numeratore = s.numeratore.saturating_add(1);
                // Nessun avviso: la richiesta compare nel pannello, con i suoi
                // due tasti, che è il posto dove si risponde.
            }
            _ => fx.push(manda(id, rifiutato(ErrorCodeKind::InsiemeRifiutato))),
        },
        Messaggio::NonChiedoPiu => {
            if let Fase::Ospita(o) = &mut s.fase {
                o.richieste.retain(|r| r.da != id);
            }
        }
        Messaggio::Ammesso { membro, diritti } => match &s.fase {
            Fase::Entrando { host, .. } if *host == id => {
                s.fase = Fase::Membro(Seguita {
                    host: id,
                    io: membro,
                    diritti,
                    persone: Vec::new(),
                    versione: None,
                    calibrazione: false,
                });
                // L'entrata del lettore per **ultima**: se non riesce torna
                // come `LettoreRifiuta`, e quel che ne segue (i ping spenti,
                // l'uscita) deve venire dopo gli effetti di qui, non prima.
                fx.push(Effetto::Sincronizza { id, attivo: true });
                fx.push(avviso(AVVISO_SEI_DENTRO, nome));
                fx.push(Effetto::EntraComeMembro { host: id });
            }
            // Già dentro con questo host: un doppione, niente da fare.
            Fase::Membro(seguita) if seguita.host == id => {}
            // Non lo si aspettava: vedi il `//!`.
            _ => fx.push(manda(id, Messaggio::Esco)),
        },
        Messaggio::Rifiutato { codice } => {
            if let Fase::Entrando { host, via, .. } = &s.fase
                && *host == id
            {
                // Un no semplice è una notizia: alla richiesta ha detto no la
                // persona, all'invito accettato il tempo (era già scaduto
                // dall'altra parte). Ogni altro perché — la sessione piena —
                // è il codice col suo testo.
                let chiave = match (codice == ErrorCodeKind::InsiemeRifiutato.code(), via) {
                    (true, Via::Richiesta) => AVVISO_RICHIESTA_RIFIUTATA,
                    (true, Via::Invito(_)) => AVVISO_INVITO_SCADUTO,
                    (false, _) => chiave_del_codice(&codice),
                };
                fx.push(avviso(chiave, nome));
                s.fase = Fase::Libera { inviti: Vec::new() };
            }
        }
        Messaggio::Esco => {
            if let Fase::Ospita(o) = &mut s.fase
                && let Some(membro) = togli_membro(o, id)
            {
                fx.push(Effetto::MembroUscito { membro });
                fx.push(Effetto::PersoneCambiate);
                fx.push(avviso(AVVISO_USCITO, nome));
            }
        }
        Messaggio::Fine { codice } => {
            let chiave = match codice.as_deref() {
                None => AVVISO_FINITA,
                Some(c) if c == CODICE_ALLONTANATO.code() => AVVISO_TI_HA_ALLONTANATO,
                Some(c) => chiave_del_codice(c),
            };
            match &s.fase {
                Fase::Membro(seguita) if seguita.host == id => {
                    fx.push(Effetto::Sincronizza { id, attivo: false });
                    fx.push(Effetto::EsciDalLettore);
                    fx.push(avviso(chiave, nome));
                    s.fase = Fase::Uscendo { spegni: false };
                }
                Fase::Entrando { host, .. } if *host == id => {
                    fx.push(avviso(chiave, nome));
                    s.fase = Fase::Libera { inviti: Vec::new() };
                }
                _ => {}
            }
        }
        Messaggio::Stato(stato) => {
            if let Fase::Membro(seguita) = &mut s.fase
                && seguita.host == id
                && seguita.versione.is_none_or(|v| stato.versione > v)
            {
                // Chi entra ed esce, visto da chi segue: dal secondo stato in
                // poi, perché il primo è la sessione com'era, non un ingresso.
                if seguita.versione.is_some() {
                    let altri = |persone: &[Persona]| -> Vec<Persona> {
                        persone
                            .iter()
                            .filter(|p| p.membro != 0 && p.membro != seguita.io)
                            .cloned()
                            .collect()
                    };
                    let prima = altri(&seguita.persone);
                    let dopo = altri(&stato.persone);
                    for p in dopo
                        .iter()
                        .filter(|p| !prima.iter().any(|q| q.membro == p.membro))
                    {
                        fx.push(avviso(AVVISO_ENTRATO, Some(p.nome.clone())));
                    }
                    for p in prima
                        .iter()
                        .filter(|p| !dopo.iter().any(|q| q.membro == p.membro))
                    {
                        fx.push(avviso(AVVISO_USCITO, Some(p.nome.clone())));
                    }
                }
                // Che cosa può fare questo PC: dalla sua `Persona`. Uno stato
                // che non lo nomina (un host a metà di un cambio) lo lascia
                // com'era.
                let diritti = stato
                    .persone
                    .iter()
                    .find(|p| p.membro == seguita.io)
                    .map_or(seguita.diritti, |p| p.diritti);
                if diritti != seguita.diritti {
                    seguita.diritti = diritti;
                    fx.push(Effetto::DirittiCambiati(diritti));
                    fx.push(avviso(avviso_dei_diritti(diritti), nome));
                }
                seguita.versione = Some(stato.versione);
                seguita.persone.clone_from(&stato.persone);
                fx.push(Effetto::Specchia(Box::new(stato)));
            }
        }
        Messaggio::Ancora(ancora) => {
            if let Fase::Membro(seguita) = &mut s.fase
                && seguita.host == id
            {
                // I clic della calibrazione si riconoscono dall'etichetta: è
                // l'unica voce che non sta in coda.
                let calibrazione = ancora.etichetta == UID_CALIBRAZIONE;
                if calibrazione != seguita.calibrazione {
                    seguita.calibrazione = calibrazione;
                    fx.push(if calibrazione {
                        avviso(AVVISO_CALIBRAZIONE_ACCESA, nome)
                    } else {
                        avviso(AVVISO_CALIBRAZIONE_SPENTA, None)
                    });
                }
                fx.push(Effetto::Segui(ancora));
            }
        }
        // Fase 2: il gesto di un membro, se il secchiello lo lascia passare.
        // Lo esegue il filo, negli stessi percorsi dei gesti di chi ospita, e
        // risponde lui con l'`Esito`.
        Messaggio::Comando { id: numero, azione } => {
            let Fase::Ospita(o) = &mut s.fase else {
                return;
            };
            let Some(m) = o.membri.iter_mut().find(|m| m.conn == id) else {
                return;
            };
            // Un gesto di una versione più nuova, che questa non sa fare: un
            // no detto con il suo perché, e la sessione va avanti. Prima del
            // secchiello, che è per i gesti veri.
            if !azione.conosciuta() {
                fx.push(manda(
                    id,
                    Messaggio::Esito {
                        id: numero,
                        codice: Some(ErrorCodeKind::InsiemeGestoNonDisponibile.code().to_owned()),
                    },
                ));
                return;
            }
            // Quel che chi ospita non gli ha concesso: un no col suo perché,
            // anche questo prima del secchiello. È la sola porta che conta —
            // il membro si ferma da sé, ma un membro di un'altra versione no —
            // e sta prima di `Esegui` anche per `Aggiungi`, che là
            // registrerebbe le voci e ne inoltrerebbe i gettoni.
            if !azione
                .ambito()
                .is_some_and(|ambito| m.diritti.concede(ambito))
            {
                fx.push(manda(
                    id,
                    Messaggio::Esito {
                        id: numero,
                        codice: Some(ErrorCodeKind::InsiemeNonConcesso.code().to_owned()),
                    },
                ));
                return;
            }
            if m.secchiello.prendi(adesso) {
                fx.push(Effetto::Esegui {
                    da: id,
                    membro: m.membro,
                    comando: numero,
                    azione: Box::new(azione),
                });
            } else {
                fx.push(manda(
                    id,
                    Messaggio::Esito {
                        id: numero,
                        codice: Some(ErrorCodeKind::InsiemeTroppiTentativi.code().to_owned()),
                    },
                ));
            }
        }
        // Un gesto di questo PC che l'host non ha fatto: il perché, come
        // guasto. Tranne il secchiello: chi tiene premuto un tasto lo sa già,
        // e una fascia rossa per ogni colpo in più sarebbe solo rumore.
        Messaggio::Esito {
            codice: Some(codice),
            ..
        } => {
            if matches!(&s.fase, Fase::Membro(seguita) if seguita.host == id)
                && codice != ErrorCodeKind::InsiemeTroppiTentativi.code()
            {
                fx.push(avviso(chiave_del_codice(&codice), None));
            }
        }
        Messaggio::Esito { codice: None, .. } | Messaggio::Sconosciuto => {}
    }
}

pub(super) fn codice_arrivato(
    s: &mut Situazione,
    id: IdConnessione,
    codice: String,
    adesso: u64,
    fx: &mut Vec<Effetto>,
) {
    match &mut s.associazione {
        Some(a) if a.conn == id => a.codice = Some(codice),
        // Una per volta: la seconda si chiude con un no, che il trasporto
        // traduce in `codiceRifiutato` per l'altro capo.
        Some(_) => fx.push(Effetto::ConfermaCodice { id, uguale: false }),
        None => {
            s.associazione = Some(Associazione {
                conn: id,
                codice: Some(codice),
                confermato: false,
                scade: adesso.saturating_add(SCADENZA_ASSOCIAZIONE_MS),
            });
            // Il codice va letto su **tutti e due** gli schermi: chi ospita
            // può avere la finestra nel vassoio mentre l'altro preme «Associa».
            fx.push(Effetto::MostraFinestra);
        }
    }
}

pub(super) fn pari_tolto(s: &mut Situazione, pari: i64, fx: &mut Vec<Effetto>) {
    let toccate: Vec<IdConnessione> = s
        .connessioni
        .iter()
        .filter(|(_, conn)| conn.pari == Some(pari))
        .map(|(id, _)| *id)
        .collect();
    for id in toccate {
        let nome = nome_di(s, id);
        if let Some(conn) = s.connessioni.get_mut(&id) {
            conn.pari = None;
        }
        match &mut s.fase {
            Fase::Libera { inviti } => inviti.retain(|i| i.da != id),
            Fase::Entrando { host, via, .. } if *host == id => {
                fx.push(manda(id, ritiro(*via)));
                s.fase = Fase::Libera { inviti: Vec::new() };
            }
            Fase::Ospita(o) => {
                o.inviti.retain(|i| i.a != id);
                o.richieste.retain(|r| r.da != id);
                if let Some(membro) = togli_membro(o, id) {
                    fx.push(manda(
                        id,
                        Messaggio::Fine {
                            codice: Some(ErrorCodeKind::InsiemeNonAssociato.code().to_owned()),
                        },
                    ));
                    fx.push(Effetto::MembroUscito { membro });
                    fx.push(Effetto::PersoneCambiate);
                    fx.push(avviso(AVVISO_ALLONTANATO, nome));
                }
            }
            Fase::Membro(seguita) if seguita.host == id => {
                fx.push(manda(id, Messaggio::Esco));
                fx.push(Effetto::Sincronizza { id, attivo: false });
                fx.push(Effetto::EsciDalLettore);
                fx.push(avviso(AVVISO_SEI_USCITO, None));
                s.fase = Fase::Uscendo { spegni: false };
            }
            _ => {}
        }
    }
}

pub(super) fn tempo(s: &mut Situazione, adesso: u64, fx: &mut Vec<Effetto>) {
    if let Some(a) = &s.associazione
        && a.scade <= adesso
    {
        let id = a.conn;
        fx.push(Effetto::ConfermaCodice { id, uguale: false });
        fx.push(avviso(AVVISO_ASSOCIAZIONE_ANNULLATA, nome_di(s, id)));
        s.associazione = None;
    }
    let nomi: BTreeMap<IdConnessione, String> = s
        .connessioni
        .iter()
        .map(|(id, conn)| (*id, conn.nome.clone()))
        .collect();
    match &mut s.fase {
        Fase::Libera { inviti } => {
            for invito in inviti.iter().filter(|i| i.scade <= adesso) {
                fx.push(avviso(AVVISO_INVITO_SCADUTO, nomi.get(&invito.da).cloned()));
            }
            inviti.retain(|i| i.scade > adesso);
        }
        Fase::Entrando { host, via, scade } if *scade <= adesso => {
            let host = *host;
            fx.push(manda(host, ritiro(*via)));
            // Una richiesta senza risposta è la persona dall'altra parte che
            // non c'è; un invito accettato senza `Ammesso` è l'host che non
            // risponde più, cioè sparito.
            let chiave = match via {
                Via::Richiesta => AVVISO_RICHIESTA_SENZA_RISPOSTA,
                Via::Invito(_) => AVVISO_HOST_SPARITO,
            };
            fx.push(avviso(chiave, nomi.get(&host).cloned()));
            s.fase = Fase::Libera { inviti: Vec::new() };
        }
        Fase::Ospita(o) => {
            for invito in o.inviti.iter().filter(|i| i.scade <= adesso) {
                fx.push(avviso(
                    AVVISO_INVITO_SENZA_RISPOSTA,
                    nomi.get(&invito.a).cloned(),
                ));
            }
            o.inviti.retain(|i| i.scade > adesso);
            for richiesta in o.richieste.iter().filter(|r| r.scade <= adesso) {
                fx.push(manda(
                    richiesta.da,
                    rifiutato(ErrorCodeKind::InsiemeRifiutato),
                ));
                fx.push(avviso(
                    AVVISO_RICHIESTA_SCADUTA,
                    nomi.get(&richiesta.da).cloned(),
                ));
            }
            o.richieste.retain(|r| r.scade > adesso);
        }
        _ => {}
    }
}

pub(super) fn comando(s: &mut Situazione, comando: Comando, adesso: u64, fx: &mut Vec<Effetto>) {
    if s.fase == Fase::Spenta {
        fx.push(errore(ErrorCode::InsiemeSpenta));
        return;
    }
    match comando {
        Comando::Ospita => match &s.fase {
            Fase::Libera { .. } => fx.push(Effetto::Ospita),
            Fase::Ospita(_) => {}
            _ => fx.push(errore(ErrorCode::InsiemeGiaInSessione)),
        },
        Comando::Termina => match &s.fase {
            Fase::Ospita(o) => {
                for conn in a_chi_dire_fine(o) {
                    fx.push(manda(conn, Messaggio::Fine { codice: None }));
                }
                fx.push(Effetto::EsciDalLettore);
                fx.push(Effetto::SmettiDiOspitare);
                fx.push(avviso(AVVISO_CHIUSA, None));
                s.fase = Fase::Uscendo { spegni: false };
            }
            Fase::Membro(_) | Fase::Entrando { .. } => {
                fx.push(errore(non_disponibile("termina")));
            }
            _ => {}
        },
        // Per chi ospita «esci» è «termina»: non c'è una sessione senza di lui.
        Comando::Esci if matches!(s.fase, Fase::Ospita(_)) => {
            self::comando(s, Comando::Termina, adesso, fx);
        }
        Comando::Esci => match &s.fase {
            Fase::Membro(seguita) => {
                let host = seguita.host;
                fx.push(manda(host, Messaggio::Esco));
                fx.push(Effetto::Sincronizza {
                    id: host,
                    attivo: false,
                });
                fx.push(Effetto::EsciDalLettore);
                fx.push(avviso(AVVISO_SEI_USCITO, None));
                s.fase = Fase::Uscendo { spegni: false };
            }
            // «Annulla» mentre si entra: è un ritiro, non un errore, e non
            // merita un avviso — l'ha appena chiesto la persona.
            Fase::Entrando { host, via, .. } => {
                fx.push(manda(*host, ritiro(*via)));
                s.fase = Fase::Libera { inviti: Vec::new() };
            }
            _ => {}
        },
        Comando::Invita { pari, diritti } => {
            let Some(id) = s
                .connessioni
                .iter()
                .find(|(_, c)| c.pari == Some(pari) && c.in_entrata)
                .map(|(id, _)| *id)
            else {
                let codice = if matches!(s.fase, Fase::Ospita(_)) {
                    ErrorCode::InsiemeIrraggiungibile
                } else {
                    non_disponibile("invita")
                };
                fx.push(errore(codice));
                return;
            };
            let invito = s.numeratore;
            let Fase::Ospita(o) = &mut s.fase else {
                fx.push(errore(non_disponibile("invita")));
                return;
            };
            if o.membri.iter().any(|m| m.conn == id) {
                return;
            }
            if piena(o) {
                fx.push(errore(sessione_piena()));
                return;
            }
            // Un invito nuovo allo stesso PC sostituisce il vecchio: la sua
            // risposta, se arriva, non trova più il numero e vale un no.
            o.inviti.retain(|i| i.a != id);
            o.inviti.push(InvitoMandato {
                invito,
                a: id,
                scade: adesso.saturating_add(SCADENZA_INVITO_MS),
                diritti,
            });
            s.numeratore = s.numeratore.saturating_add(1);
            fx.push(manda(
                id,
                Messaggio::Invito {
                    invito,
                    scade_fra_ms: SCADENZA_INVITO_MS,
                },
            ));
            fx.push(avviso(AVVISO_INVITO_MANDATO, nome_di(s, id)));
        }
        Comando::Rispondi { invito, accetta } => {
            let Fase::Libera { inviti } = &mut s.fase else {
                fx.push(errore(ErrorCode::InsiemeGiaInSessione));
                return;
            };
            let Some(posto) = inviti
                .iter()
                .position(|i| i.numero == invito && i.scade > adesso)
            else {
                inviti.retain(|i| i.numero != invito);
                fx.push(errore(ErrorCode::InsiemeRifiutato));
                return;
            };
            let scelto = inviti.remove(posto);
            if !accetta {
                fx.push(manda(
                    scelto.da,
                    Messaggio::RispostaInvito {
                        invito: scelto.invito,
                        accetta: false,
                    },
                ));
                return;
            }
            // Accettarne uno rifiuta gli altri: vedi il `//!`.
            for altro in inviti.drain(..) {
                fx.push(manda(
                    altro.da,
                    Messaggio::RispostaInvito {
                        invito: altro.invito,
                        accetta: false,
                    },
                ));
            }
            fx.push(manda(
                scelto.da,
                Messaggio::RispostaInvito {
                    invito: scelto.invito,
                    accetta: true,
                },
            ));
            s.fase = Fase::Entrando {
                host: scelto.da,
                via: Via::Invito(scelto.invito),
                scade: adesso.saturating_add(ATTESA_AMMISSIONE_MS),
            };
        }
        Comando::Chiedi { pari } => {
            let Fase::Libera { inviti } = &mut s.fase else {
                fx.push(errore(ErrorCode::InsiemeGiaInSessione));
                return;
            };
            let Some(host) = s
                .connessioni
                .iter()
                .find(|(_, c)| c.pari == Some(pari) && c.ospita && !c.in_entrata)
                .map(|(id, _)| *id)
            else {
                fx.push(errore(ErrorCode::InsiemeIrraggiungibile));
                return;
            };
            for altro in inviti.drain(..) {
                fx.push(manda(
                    altro.da,
                    Messaggio::RispostaInvito {
                        invito: altro.invito,
                        accetta: false,
                    },
                ));
            }
            fx.push(manda(host, Messaggio::Chiedi));
            fx.push(avviso(AVVISO_RICHIESTA_MANDATA, nome_di(s, host)));
            s.fase = Fase::Entrando {
                host,
                via: Via::Richiesta,
                scade: adesso.saturating_add(SCADENZA_RICHIESTA_MS),
            };
        }
        Comando::Ammetti {
            richiesta,
            accetta,
            diritti,
        } => {
            let Fase::Ospita(o) = &mut s.fase else {
                fx.push(errore(non_disponibile("ammetti")));
                return;
            };
            let Some(posto) = o
                .richieste
                .iter()
                .position(|r| r.numero == richiesta && r.scade > adesso)
            else {
                o.richieste.retain(|r| r.numero != richiesta);
                fx.push(errore(ErrorCode::InsiemeRifiutato));
                return;
            };
            let scelta = o.richieste.remove(posto);
            if !accetta {
                fx.push(manda(scelta.da, rifiutato(ErrorCodeKind::InsiemeRifiutato)));
                return;
            }
            if !ammetti(s, scelta.da, diritti, fx) {
                fx.push(errore(sessione_piena()));
            }
        }
        Comando::Concedi { membro, diritti } => {
            let Fase::Ospita(o) = &mut s.fase else {
                fx.push(errore(non_disponibile("concedi")));
                return;
            };
            // Un membro già uscito: niente da cambiare, come per «Allontana».
            let Some(m) = o.membri.iter_mut().find(|m| m.membro == membro) else {
                return;
            };
            // Lo stesso livello due volte non è un cambio: niente stato in più,
            // e niente avviso a chi segue.
            if m.diritti != diritti {
                m.diritti = diritti;
                fx.push(Effetto::PersoneCambiate);
            }
        }
        Comando::Allontana { membro } => {
            let Fase::Ospita(o) = &mut s.fase else {
                fx.push(errore(non_disponibile("allontana")));
                return;
            };
            let Some(conn) = o.membri.iter().find(|m| m.membro == membro).map(|m| m.conn) else {
                return;
            };
            togli_membro(o, conn);
            fx.push(manda(
                conn,
                Messaggio::Fine {
                    codice: Some(CODICE_ALLONTANATO.code().to_owned()),
                },
            ));
            fx.push(Effetto::MembroUscito { membro });
            fx.push(Effetto::PersoneCambiate);
            fx.push(avviso(AVVISO_ALLONTANATO, nome_di(s, conn)));
        }
        Comando::Associa { id } => {
            let Some(conn) = s.connessioni.get(&id) else {
                fx.push(errore(ErrorCode::InsiemeIrraggiungibile));
                return;
            };
            // Già fidato: l'associazione c'è, e rifarla non serve.
            if conn.pari.is_some() {
                return;
            }
            // Una sola per volta, e vince l'ultima chiesta: quella di prima si
            // chiude con un no.
            if let Some(vecchia) = s.associazione.take()
                && vecchia.conn != id
            {
                fx.push(Effetto::ConfermaCodice {
                    id: vecchia.conn,
                    uguale: false,
                });
            }
            s.associazione = Some(Associazione {
                conn: id,
                codice: None,
                confermato: false,
                scade: adesso.saturating_add(SCADENZA_ASSOCIAZIONE_MS),
            });
            fx.push(Effetto::AssociaInRete { id });
        }
        Comando::ConfermaCodice {
            associazione,
            uguale,
        } => {
            let Some(a) = s
                .associazione
                .as_mut()
                .filter(|a| a.conn.0 == associazione && a.codice.is_some())
            else {
                fx.push(errore(ErrorCode::InsiemeCodiceRifiutato));
                return;
            };
            let id = a.conn;
            fx.push(Effetto::ConfermaCodice { id, uguale });
            if uguale {
                a.confermato = true;
            } else {
                s.associazione = None;
            }
        }
    }
}
