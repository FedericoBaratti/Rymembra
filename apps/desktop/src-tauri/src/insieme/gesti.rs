//! Fase 2: i gesti di chi segue, dal tasto premuto sul suo PC al comando
//! eseguito sul PC che ospita.
//!
//! # Due traduzioni, tutte e due pure
//!
//! - [`traduci`], da chi segue: un [`Gesto`] del suo lettore diventa una o più
//!   [`Azione`] sul filo. Le posizioni della coda («la terza riga») diventano
//!   `uid` leggendo lo **specchio** — la coda di chi segue è quella della
//!   sessione, nello stesso ordine — perché fra il gesto e il suo arrivo la
//!   coda dell'host può cambiare, e una posizione allora nominerebbe un'altra
//!   voce. Un `uid` invece resta quello.
//! - [`piano`], da chi ospita: un'[`Azione`] arrivata diventa un [`Passo`],
//!   cioè uno dei comandi di sempre del lettore (`pausa`, `coda_vai`, …)
//!   con la posizione **di adesso**, letta da una sola istantanea.
//!
//! # Idempotenti
//!
//! Due persone premono «avanti» insieme: l'host riceve due `Prossimo` e deve
//! saltare un brano, non due. Per questo `Prossimo` e `Precedente` portano la
//! voce che chi ha premuto vedeva suonare (`da`), e si fanno solo se suona
//! ancora quella; lo stesso `VaiA`, che nomina la voce in cui saltare. Una voce
//! da far suonare, togliere o spostare che nel frattempo non c'è più è un
//! gesto già superato: si risponde «fatto», perché non c'è niente di sbagliato
//! da dire a chi l'ha chiesto. Per pausa e ripresa vince l'ultimo arrivato.
//!
//! # Il secchiello
//!
//! Al più [`CAPIENZA`] comandi di fila per membro, poi uno ogni
//! [`PASSO_MS`]: un tasto multimediale tenuto premuto, o una finestra che
//! impazzisce, non deve poter far girare la coda di tutti a cento colpi al
//! secondo. Oltre si risponde `insieme.troppiTentativi`.

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_insieme::messaggi::{Azione, Dove, MASSIMO_AGGIUNTE};

use super::strada::Gesto;
use rymembra_domain::queue::RepeatMode;

use super::voci::{ripeti_da_filo, ripeti_sul_filo};
use crate::riproduzione::insieme::Istantanea;

/// Quanti comandi di fila può mandare un membro prima che il secchiello si
/// faccia sentire.
pub const CAPIENZA: u64 = 10;

/// Ogni quanto il secchiello si riempie di un comando: dieci al secondo.
pub const PASSO_MS: u64 = 100;

/// Che cosa diventa un gesto di chi segue.
#[derive(Debug, Clone, PartialEq)]
pub enum Traduzione {
    /// Queste azioni, nell'ordine. Vuota: il gesto non ha niente su cui agire
    /// (un salto senza una voce che suona, una riga che non c'è).
    Azioni(Vec<Azione>),
    /// Dei brani della libreria di questo PC da aggiungere alla sessione: le
    /// voci le descrive il filo della sessione, che dà i gettoni.
    Aggiungi {
        /// Le righe, nell'ordine.
        ids: Vec<i64>,
        /// Dove.
        dove: Dove,
        /// La prima parte subito.
        suona: bool,
    },
}

/// Il gesto non si può fare in sessione, col suo nome stabile.
/// Un gesto che nomina una voce che non c'è più (o non c'è ancora): la
/// coda è cambiata fra quel che si vedeva e il gesto.
fn superato() -> AppError {
    AppError::new(ErrorCode::InsiemeSuperato)
}

fn non_disponibile(gesto: &Gesto) -> AppError {
    AppError::new(ErrorCode::InsiemeGestoNonDisponibile {
        gesto: super::nome_del_gesto(gesto).to_owned(),
    })
}

/// Le righe della libreria fra quelle chieste: le maniglie (le voci della
/// sessione, id negativi) non si aggiungono una seconda volta, e il tetto è
/// quello di un'aggiunta sola. Il segno lo guarda `ospiti`, attraverso il
/// ponte: vedi il suo `//!`.
fn righe(ids: &[i64]) -> Vec<i64> {
    ids.iter()
        .copied()
        .filter(|id| crate::riproduzione::insieme::e_di_libreria(*id))
        .take(MASSIMO_AGGIUNTE)
        .collect()
}

/// Da chi segue: un gesto in azioni sul filo. Vedi il `//!`.
///
/// # Errori
///
/// `insieme.gestoNonDisponibile` per i gesti che in sessione non si fanno
/// (la `strada` li rifiuta prima di arrivare qui), e per un «suona» che non
/// porta nessuna riga della libreria.
pub fn traduci(gesto: &Gesto, specchio: &Istantanea) -> Result<Traduzione, AppError> {
    let uid = |indice: usize| specchio.coda.get(indice).copied();
    let azioni = match gesto {
        Gesto::Pausa => vec![Azione::Pausa],
        Gesto::Riprendi => vec![Azione::Riprendi],
        Gesto::Alterna => {
            if specchio.in_pausa {
                vec![Azione::Riprendi]
            } else {
                vec![Azione::Pausa]
            }
        }
        // Senza una voce che suona nello specchio, un «avanti», un
        // «indietro» o un salto non saprebbero da dove partire: l'host li
        // scarterebbe. Si dice subito, invece di un tasto che non fa niente.
        Gesto::Prossimo => vec![Azione::Prossimo {
            da: Some(specchio.corrente.ok_or_else(superato)?),
        }],
        Gesto::Precedente => vec![Azione::Precedente {
            da: Some(specchio.corrente.ok_or_else(superato)?),
        }],
        Gesto::VaiA { ms } => vec![Azione::VaiA {
            etichetta: specchio.corrente.ok_or_else(superato)?,
            ms: *ms,
        }],
        Gesto::CodaVai { indice } => vec![Azione::Vai {
            uid: uid(*indice).ok_or_else(superato)?,
        }],
        Gesto::CodaTogli { indici } => {
            let azioni: Vec<Azione> = indici
                .iter()
                .filter_map(|i| uid(*i))
                .map(|uid| Azione::Togli { uid })
                .collect();
            if azioni.is_empty() && !indici.is_empty() {
                return Err(superato());
            }
            azioni
        }
        Gesto::CodaRiordina { da, a } => sposta(&specchio.coda, *da, *a).into_iter().collect(),
        Gesto::Ripeti => vec![Azione::Ripeti {
            modo: ripeti_sul_filo(specchio.ripeti.next()),
        }],
        Gesto::Suona { ids, da } => {
            let scelte = righe(ids.get(*da..).unwrap_or_default());
            if scelte.is_empty() {
                return Err(non_disponibile(gesto));
            }
            // Parte subito solo il brano cliccato. Se quello non c'è (una
            // maniglia, una riga che manca) gli altri si aggiungono e basta:
            // far partire il primo superstite al suo posto sarebbe suonare un
            // brano che nessuno ha scelto.
            let suona = ids
                .get(*da)
                .is_some_and(|cliccato| scelte.first() == Some(cliccato));
            return Ok(Traduzione::Aggiungi {
                ids: scelte,
                dove: Dove::Dopo,
                suona,
            });
        }
        Gesto::Accoda { ids } | Gesto::Dopo { ids } => {
            return Ok(Traduzione::Aggiungi {
                ids: righe(ids),
                dove: if matches!(gesto, Gesto::Accoda { .. }) {
                    Dove::InFondo
                } else {
                    Dove::Dopo
                },
                suona: false,
            });
        }
        Gesto::Radio
        | Gesto::CodaSvuota
        | Gesto::CodaRipristina
        | Gesto::Shuffle
        | Gesto::Autoplay
        | Gesto::Timer => return Err(non_disponibile(gesto)),
    };
    Ok(Traduzione::Azioni(azioni))
}

/// Da chi segue, quando lo specchio non si legge (il lettore che sta
/// entrando o uscendo): passano solo i gesti che non nominano una voce —
/// pausa, ripresa e le aggiunte. Un «avanti» senza la voce che si vedeva
/// sarebbe un `Prossimo { da: None }`, che l'host scarterebbe dicendo
/// «fatto»: meglio un no detto subito, che un tasto che non fa niente.
///
/// # Errori
///
/// `insieme.gestoNonDisponibile` per ogni altro gesto.
pub fn traduci_senza_specchio(gesto: &Gesto) -> Result<Traduzione, AppError> {
    match gesto {
        Gesto::Pausa
        | Gesto::Riprendi
        | Gesto::Suona { .. }
        | Gesto::Accoda { .. }
        | Gesto::Dopo { .. } => traduci(
            gesto,
            &Istantanea {
                coda: Vec::new(),
                corrente: None,
                in_pausa: false,
                ripeti: RepeatMode::Off,
            },
        ),
        _ => Err(non_disponibile(gesto)),
    }
}

/// Lo spostamento della coda come lo fa il lettore (`Queue::reorder`: toglie
/// da `da`, rimette in `a`), detto per `uid`: la voce, e quella dopo cui
/// finisce (`None` in testa).
fn sposta(coda: &[u64], da: usize, a: usize) -> Option<Azione> {
    if da >= coda.len() || a >= coda.len() || da == a {
        return None;
    }
    let mut dopo = coda.to_vec();
    let uid = dopo.remove(da);
    dopo.insert(a, uid);
    Some(Azione::Sposta {
        uid,
        dopo: a.checked_sub(1).and_then(|prima| dopo.get(prima).copied()),
    })
}

/// Quel che chi ospita fa per un'azione: uno dei comandi del lettore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passo {
    /// Niente: il gesto è superato, o non c'è niente su cui farlo.
    Niente,
    /// `riproduzione::pausa`.
    Pausa,
    /// `riproduzione::riprendi`.
    Riprendi,
    /// `riproduzione::prossimo`.
    Prossimo,
    /// `riproduzione::precedente`.
    Precedente,
    /// `riproduzione::vai_a`, in ms.
    VaiA(u64),
    /// Fa suonare quella voce: `ponte::suona_voce`, che ne trova la posizione
    /// sotto lo stesso lucchetto che la usa. Una posizione presa da una
    /// fotografia e usata dopo potrebbe nominare un'altra voce.
    Vai(u64),
    /// Toglie quella voce: `ponte::togli_voci`, per la stessa ragione.
    Togli(u64),
    /// `coda_riordina`.
    Riordina {
        /// Da dove.
        da: usize,
        /// A dove.
        a: usize,
    },
    /// `imposta_ripeti`: il modo arriva com'è, non come «il prossimo», perché
    /// fra il gesto e il suo arrivo il modo dell'host può essere cambiato.
    Ripeti(RepeatMode),
    /// Il gesto nomina una voce che non suona più, o che non è più in coda:
    /// qualcun altro l'ha già cambiata. Non si fa, e chi l'ha chiesto riceve
    /// `insieme.superato` — non un «fatto» per un tasto che non ha fatto
    /// niente.
    Superato,
    /// Un gesto che questa versione non conosce, da un membro più nuovo: si
    /// risponde `insieme.gestoNonDisponibile`, e la sessione continua.
    Sconosciuto,
}

/// Il nome di un'azione per il diario: il gesto e basta, senza voci né
/// posizioni.
#[must_use]
pub const fn nome_dell_azione(azione: &Azione) -> &'static str {
    match azione {
        Azione::Pausa => "pausa",
        Azione::Riprendi => "riprendi",
        Azione::Prossimo { .. } => "prossimo",
        Azione::Precedente { .. } => "precedente",
        Azione::VaiA { .. } => "vai_a",
        Azione::Vai { .. } => "vai",
        Azione::Togli { .. } => "togli",
        Azione::Sposta { .. } => "sposta",
        Azione::Ripeti { .. } => "ripeti",
        Azione::Aggiungi { .. } => "aggiungi",
        Azione::Sconosciuta => "sconosciuto",
    }
}

/// Da chi ospita: che cosa fare per un'azione, con la coda di adesso.
/// `Aggiungi` non passa di qui: vuole voci da registrare, non un comando.
#[must_use]
pub fn piano(azione: &Azione, adesso: &Istantanea) -> Passo {
    let posto = |uid: u64| adesso.coda.iter().position(|u| *u == uid);
    match azione {
        Azione::Pausa => Passo::Pausa,
        Azione::Riprendi => Passo::Riprendi,
        Azione::Prossimo { da } if *da == adesso.corrente => Passo::Prossimo,
        Azione::Precedente { da } if *da == adesso.corrente => Passo::Precedente,
        Azione::VaiA { etichetta, ms } if adesso.corrente == Some(*etichetta) => Passo::VaiA(*ms),
        Azione::Vai { uid } => posto(*uid).map_or(Passo::Superato, |_| Passo::Vai(*uid)),
        Azione::Togli { uid } => posto(*uid).map_or(Passo::Superato, |_| Passo::Togli(*uid)),
        Azione::Sposta { uid, dopo } => {
            let Some(da) = posto(*uid) else {
                return Passo::Superato;
            };
            let a = match dopo {
                None => 0,
                Some(dopo) if dopo == uid => return Passo::Niente,
                Some(dopo) => {
                    let Some(p) = posto(*dopo) else {
                        return Passo::Superato;
                    };
                    // Tolta la voce, quella dopo cui va scende di uno se
                    // stava dopo di lei; e la voce va subito dietro.
                    (if p > da { p - 1 } else { p }).saturating_add(1)
                }
            };
            if a == da || a >= adesso.coda.len() {
                Passo::Niente
            } else {
                Passo::Riordina { da, a }
            }
        }
        Azione::Ripeti { modo } => {
            let modo = ripeti_da_filo(*modo);
            if modo == adesso.ripeti {
                Passo::Niente
            } else {
                Passo::Ripeti(modo)
            }
        }
        Azione::Prossimo { .. } | Azione::Precedente { .. } | Azione::VaiA { .. } => {
            Passo::Superato
        }
        Azione::Aggiungi { .. } => Passo::Niente,
        Azione::Sconosciuta => Passo::Sconosciuto,
    }
}

/// Il secchiello di un membro: vedi il `//!`. È un GCRA — l'istante teorico
/// in cui il secchiello sarà di nuovo vuoto — e non un contatore che si
/// ricarica a scatti: un comando ogni [`PASSO_MS`] passa sempre, e dieci di
/// fila dopo un po' di silenzio anche.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Secchiello {
    vuoto_alle: u64,
}

impl Secchiello {
    /// Un comando adesso: vero se passa.
    pub fn prendi(&mut self, adesso: u64) -> bool {
        let da = self.vuoto_alle.max(adesso);
        if da.saturating_sub(adesso) > CAPIENZA.saturating_sub(1).saturating_mul(PASSO_MS) {
            return false;
        }
        self.vuoto_alle = da.saturating_add(PASSO_MS);
        true
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use rymembra_insieme::messaggi::Ripeti;

    fn specchio(coda: &[u64], corrente: Option<u64>) -> Istantanea {
        Istantanea {
            coda: coda.to_vec(),
            corrente,
            in_pausa: false,
            ripeti: RepeatMode::Off,
        }
    }

    fn azioni(gesto: &Gesto, s: &Istantanea) -> Vec<Azione> {
        match traduci(gesto, s).unwrap() {
            Traduzione::Azioni(a) => a,
            altro => panic!("{altro:?}"),
        }
    }

    #[test]
    fn i_gesti_di_trasporto_portano_la_voce_che_si_vedeva() {
        let s = specchio(&[10, 11, 12], Some(11));
        assert_eq!(
            azioni(&Gesto::Prossimo, &s),
            vec![Azione::Prossimo { da: Some(11) }]
        );
        assert_eq!(
            azioni(&Gesto::Precedente, &s),
            vec![Azione::Precedente { da: Some(11) }]
        );
        assert_eq!(
            azioni(&Gesto::VaiA { ms: 5_000 }, &s),
            vec![Azione::VaiA {
                etichetta: 11,
                ms: 5_000
            }]
        );
        assert_eq!(azioni(&Gesto::Alterna, &s), vec![Azione::Pausa]);
        let ferma = Istantanea {
            in_pausa: true,
            ..s.clone()
        };
        assert_eq!(azioni(&Gesto::Alterna, &ferma), vec![Azione::Riprendi]);
        for gesto in [Gesto::VaiA { ms: 1 }, Gesto::Prossimo, Gesto::Precedente] {
            let err = traduci(&gesto, &specchio(&[], None)).unwrap_err();
            assert_eq!(err.code().kind().code(), "insieme.superato", "{gesto:?}");
        }
    }

    #[test]
    fn le_posizioni_della_coda_diventano_uid() {
        let s = specchio(&[10, 11, 12, 13], Some(10));
        assert_eq!(
            azioni(&Gesto::CodaVai { indice: 2 }, &s),
            vec![Azione::Vai { uid: 12 }]
        );
        for gesto in [
            Gesto::CodaVai { indice: 9 },
            Gesto::CodaTogli { indici: vec![7, 8] },
        ] {
            let err = traduci(&gesto, &s).unwrap_err();
            assert_eq!(err.code().kind().code(), "insieme.superato", "{gesto:?}");
        }
        assert_eq!(
            azioni(
                &Gesto::CodaTogli {
                    indici: vec![1, 3, 7]
                },
                &s
            ),
            vec![Azione::Togli { uid: 11 }, Azione::Togli { uid: 13 }]
        );
        // 10 11 12 13: la 13 in seconda posizione → 10 13 11 12.
        assert_eq!(
            azioni(&Gesto::CodaRiordina { da: 3, a: 1 }, &s),
            vec![Azione::Sposta {
                uid: 13,
                dopo: Some(10)
            }]
        );
        // In testa.
        assert_eq!(
            azioni(&Gesto::CodaRiordina { da: 2, a: 0 }, &s),
            vec![Azione::Sposta {
                uid: 12,
                dopo: None
            }]
        );
        // In avanti: 10 11 12 13, la 10 in terza → 11 12 10 13.
        assert_eq!(
            azioni(&Gesto::CodaRiordina { da: 0, a: 2 }, &s),
            vec![Azione::Sposta {
                uid: 10,
                dopo: Some(12)
            }]
        );
    }

    #[test]
    fn uno_spostamento_tradotto_e_rieseguito_da_lo_stesso_ordine() {
        // Ogni coppia (da, a) su una coda di cinque: tradotta da chi segue,
        // pianificata da chi ospita sulla stessa coda, deve dare lo stesso
        // `reorder` — cioè la stessa coda finale.
        let coda = [1_u64, 2, 3, 4, 5];
        for da in 0..5 {
            for a in 0..5 {
                let s = specchio(&coda, None);
                let attesa = {
                    let mut v = coda.to_vec();
                    let x = v.remove(da);
                    v.insert(a, x);
                    v
                };
                let fatta = match azioni(&Gesto::CodaRiordina { da, a }, &s).as_slice() {
                    [] => coda.to_vec(),
                    [azione] => match piano(azione, &s) {
                        Passo::Riordina { da, a } => {
                            let mut v = coda.to_vec();
                            let x = v.remove(da);
                            v.insert(a, x);
                            v
                        }
                        Passo::Niente => coda.to_vec(),
                        altro => panic!("{altro:?}"),
                    },
                    altro => panic!("{altro:?}"),
                };
                assert_eq!(fatta, attesa, "da {da} a {a}");
            }
        }
    }

    #[test]
    fn ripeti_chiede_il_modo_dopo_e_l_host_lo_impone() {
        let mut s = specchio(&[], None);
        assert_eq!(
            azioni(&Gesto::Ripeti, &s),
            vec![Azione::Ripeti {
                modo: Ripeti::Tutta
            }]
        );
        s.ripeti = RepeatMode::One;
        assert_eq!(
            azioni(&Gesto::Ripeti, &s),
            vec![Azione::Ripeti { modo: Ripeti::No }]
        );
        let host = specchio(&[], None);
        assert_eq!(
            piano(&Azione::Ripeti { modo: Ripeti::Una }, &host),
            Passo::Ripeti(RepeatMode::One)
        );
        assert_eq!(
            piano(&Azione::Ripeti { modo: Ripeti::No }, &host),
            Passo::Niente
        );
    }

    #[test]
    fn senza_specchio_passano_solo_i_gesti_che_non_nominano_una_voce() {
        assert_eq!(
            traduci_senza_specchio(&Gesto::Pausa).unwrap(),
            Traduzione::Azioni(vec![Azione::Pausa])
        );
        assert!(matches!(
            traduci_senza_specchio(&Gesto::Accoda { ids: vec![4] }).unwrap(),
            Traduzione::Aggiungi { .. }
        ));
        for gesto in [
            Gesto::Alterna,
            Gesto::Prossimo,
            Gesto::Precedente,
            Gesto::VaiA { ms: 1 },
            Gesto::CodaVai { indice: 0 },
            Gesto::CodaTogli { indici: vec![0] },
            Gesto::CodaRiordina { da: 0, a: 1 },
            Gesto::Ripeti,
        ] {
            let err = traduci_senza_specchio(&gesto).unwrap_err();
            assert_eq!(
                err.code().kind().code(),
                "insieme.gestoNonDisponibile",
                "{gesto:?}"
            );
        }
    }

    #[test]
    fn suona_accoda_e_dopo_aggiungono_solo_righe_della_libreria() {
        let s = specchio(&[], None);
        assert_eq!(
            traduci(
                &Gesto::Suona {
                    ids: vec![5, 6, -3, 7],
                    da: 1
                },
                &s
            )
            .unwrap(),
            Traduzione::Aggiungi {
                ids: vec![6, 7],
                dove: Dove::Dopo,
                suona: true
            }
        );
        assert_eq!(
            traduci(&Gesto::Accoda { ids: vec![1] }, &s).unwrap(),
            Traduzione::Aggiungi {
                ids: vec![1],
                dove: Dove::InFondo,
                suona: false
            }
        );
        let tante: Vec<i64> = (1..=500).collect();
        match traduci(&Gesto::Dopo { ids: tante }, &s).unwrap() {
            Traduzione::Aggiungi { ids, dove, .. } => {
                assert_eq!(ids.len(), MASSIMO_AGGIUNTE);
                assert_eq!(dove, Dove::Dopo);
            }
            altro => panic!("{altro:?}"),
        }
        // Il brano cliccato è una maniglia: gli altri si aggiungono, ma non
        // parte nessuno al suo posto.
        assert_eq!(
            traduci(
                &Gesto::Suona {
                    ids: vec![5, -6, 7],
                    da: 1
                },
                &s
            )
            .unwrap(),
            Traduzione::Aggiungi {
                ids: vec![7],
                dove: Dove::Dopo,
                suona: false
            }
        );
        let err = traduci(
            &Gesto::Suona {
                ids: vec![-1],
                da: 0,
            },
            &s,
        )
        .unwrap_err();
        assert_eq!(err.code().kind().code(), "insieme.gestoNonDisponibile");
    }

    #[test]
    fn prossimo_e_precedente_sono_idempotenti() {
        let host = specchio(&[1, 2, 3], Some(2));
        assert_eq!(
            piano(&Azione::Prossimo { da: Some(2) }, &host),
            Passo::Prossimo
        );
        // Il secondo «avanti» arriva quando suona già la 3: superato, e chi
        // l'ha premuto lo sa.
        let dopo = specchio(&[1, 2, 3], Some(3));
        assert_eq!(
            piano(&Azione::Prossimo { da: Some(2) }, &dopo),
            Passo::Superato
        );
        assert_eq!(
            piano(&Azione::Precedente { da: Some(2) }, &dopo),
            Passo::Superato
        );
        assert_eq!(
            piano(
                &Azione::VaiA {
                    etichetta: 2,
                    ms: 9
                },
                &dopo
            ),
            Passo::Superato
        );
        assert_eq!(
            piano(
                &Azione::VaiA {
                    etichetta: 3,
                    ms: 9
                },
                &dopo
            ),
            Passo::VaiA(9)
        );
    }

    #[test]
    fn una_voce_che_non_c_e_piu_e_un_gesto_gia_fatto() {
        let host = specchio(&[1, 2, 3], Some(1));
        assert_eq!(piano(&Azione::Vai { uid: 3 }, &host), Passo::Vai(3));
        assert_eq!(piano(&Azione::Togli { uid: 2 }, &host), Passo::Togli(2));
        assert_eq!(piano(&Azione::Vai { uid: 9 }, &host), Passo::Superato);
        assert_eq!(piano(&Azione::Togli { uid: 9 }, &host), Passo::Superato);
        assert_eq!(
            piano(&Azione::Sposta { uid: 9, dopo: None }, &host),
            Passo::Superato
        );
        assert_eq!(
            piano(
                &Azione::Sposta {
                    uid: 3,
                    dopo: Some(9)
                },
                &host
            ),
            Passo::Superato
        );
        assert_eq!(
            piano(
                &Azione::Sposta {
                    uid: 2,
                    dopo: Some(1)
                },
                &host
            ),
            Passo::Niente,
            "è già lì"
        );
    }

    #[test]
    fn il_secchiello_lascia_passare_dieci_di_fila_e_poi_dieci_al_secondo() {
        let mut s = Secchiello::default();
        let passati = (0..15).filter(|_| s.prendi(1_000)).count();
        assert_eq!(passati, 10);
        assert!(!s.prendi(1_050));
        assert!(s.prendi(1_100), "un passo dopo, uno di nuovo");
        assert!(!s.prendi(1_100));
        // Dopo un secondo di silenzio il secchiello è di nuovo pieno.
        assert_eq!((0..15).filter(|_| s.prendi(3_000)).count(), 10);
    }
}
