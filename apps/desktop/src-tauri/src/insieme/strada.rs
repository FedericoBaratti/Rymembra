//! Che strada prende un gesto del lettore quando c'è una sessione.
//!
//! # Dove si chiama
//!
//! In testa al **corpo** di ogni comando di trasporto e di coda di
//! `riproduzione`, non nel solo strato di Tauri: i tasti multimediali, i
//! pulsanti della miniatura, il vassoio e le scorciatoie arrivano chiamando
//! direttamente quelle funzioni (`media.rs`), e una regola messa solo davanti
//! alla finestra la scavalcherebbero tutti. La prova
//! `ogni_gesto_ha_una_regola_in_ogni_ruolo` tiene la `match` esaustiva: un
//! gesto nuovo senza strada non compila.
//!
//! # Le regole
//!
//! Fuori sessione tutto è [`Regola::Locale`]. In sessione:
//!
//! - **chi ospita** suona per tutti: trasporto locale; i brani che mette in
//!   coda diventano voci di sessione ([`Regola::LocaleConVoci`]); quel che
//!   cambierebbe l'ordine o la durata per gli altri senza che lo vedano — la
//!   radio, lo shuffle, l'autoplay, il timer — si rifiuta;
//! - **chi segue** non suona da sé: trasporto e coda si **inoltrano** all'host
//!   (fase 2), che li esegue negli stessi percorsi dei gesti locali, **se**
//!   chi ospita glieli ha concessi ([`Diritti`]); quel che non gli ha concesso
//!   si ferma qui ([`Regola::NonConcesso`]), e il resto si rifiuta. Volume,
//!   equalizzatore, uscita e latenza non passano di qui: sono di ciascuno, e
//!   restano locali sempre, a ogni livello.
//!
//! Fermare qui quel che non è concesso è una cortesia, non la porta: la porta
//! è l'host, che ricontrolla ogni gesto all'arrivo (`fasi`). Serve ai tasti
//! multimediali, al vassoio e alla miniatura, che non passano dalla finestra:
//! senza, ogni colpo di chi ascolta e basta andrebbe all'host per tornare
//! indietro con un no.

use rymembra_domain::errors::AppError;
use rymembra_insieme::messaggi::{Ambito, Diritti};

/// Il ruolo di questo PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ruolo {
    /// Nessuna sessione.
    Fuori,
    /// Si ospita.
    Host,
    /// Si segue, con quel che chi ospita ha concesso.
    Membro(Diritti),
}

/// Un gesto del lettore che una sessione deve poter fermare o inoltrare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gesto {
    /// Pausa.
    Pausa,
    /// Ripresa.
    Riprendi,
    /// Pausa o ripresa, secondo lo stato.
    Alterna,
    /// Il brano dopo.
    Prossimo,
    /// Il brano prima, o l'inizio di questo.
    Precedente,
    /// Un salto nel brano corrente.
    VaiA {
        /// Dove, in millisecondi.
        ms: u64,
    },
    /// Sostituisce la coda e fa suonare.
    Suona {
        /// I brani, id di libreria.
        ids: Vec<i64>,
        /// Da quale.
        da: usize,
    },
    /// La radio da un brano.
    Radio,
    /// Accoda in fondo.
    Accoda {
        /// I brani.
        ids: Vec<i64>,
    },
    /// Mette subito dopo il corrente.
    Dopo {
        /// I brani.
        ids: Vec<i64>,
    },
    /// Fa suonare la voce in quella posizione della coda.
    CodaVai {
        /// La posizione.
        indice: usize,
    },
    /// Toglie voci dalla coda.
    CodaTogli {
        /// Le posizioni.
        indici: Vec<usize>,
    },
    /// Sposta una voce.
    CodaRiordina {
        /// Da dove.
        da: usize,
        /// A dove.
        a: usize,
    },
    /// Svuota la coda.
    CodaSvuota,
    /// Rimette la coda che un gesto ha sostituito («Annulla»).
    CodaRipristina,
    /// Shuffle acceso o spento.
    Shuffle,
    /// Il modo di ripetizione.
    Ripeti,
    /// L'autoplay a coda finita.
    Autoplay,
    /// Il timer di spegnimento.
    Timer,
}

/// La strada di un gesto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regola {
    /// Si fa qui, come sempre.
    Locale,
    /// Si fa qui, ma i brani di libreria diventano voci della sessione prima di
    /// entrare in coda. Solo chi ospita.
    LocaleConVoci,
    /// Si manda all'host. Solo chi segue.
    Inoltra,
    /// Si manderebbe all'host, ma chi ospita non l'ha concesso a chi segue:
    /// `insieme.nonConcesso`.
    NonConcesso,
    /// Non si fa: `insieme.gestoNonDisponibile`.
    Rifiuta,
}

/// La regola: vedi il `//!`. Pura, e per questo provata a tappeto.
#[must_use]
pub const fn regola(ruolo: Ruolo, gesto: &Gesto) -> Regola {
    match ruolo {
        Ruolo::Fuori => Regola::Locale,
        Ruolo::Host => match gesto {
            Gesto::Pausa
            | Gesto::Riprendi
            | Gesto::Alterna
            | Gesto::Prossimo
            | Gesto::Precedente
            | Gesto::VaiA { .. }
            | Gesto::CodaVai { .. }
            | Gesto::CodaTogli { .. }
            | Gesto::CodaRiordina { .. }
            | Gesto::CodaSvuota
            | Gesto::Ripeti => Regola::Locale,
            Gesto::Suona { .. } | Gesto::Accoda { .. } | Gesto::Dopo { .. } => {
                Regola::LocaleConVoci
            }
            // «Annulla» rimetterebbe una coda di prima della sessione, fatta di
            // brani che gli altri non hanno: si rifiuta invece di indovinare.
            Gesto::CodaRipristina
            | Gesto::Radio
            | Gesto::Shuffle
            | Gesto::Autoplay
            | Gesto::Timer => Regola::Rifiuta,
        },
        Ruolo::Membro(diritti) => match ambito(gesto) {
            Some(ambito) if diritti.concede(ambito) => Regola::Inoltra,
            Some(_) => Regola::NonConcesso,
            None => Regola::Rifiuta,
        },
    }
}

/// Di che cosa è fatto un gesto che chi segue manda all'host, per i
/// [`Diritti`]; `None` per quel che chi segue non manda mai.
///
/// È la stessa divisione di `Azione::ambito`, vista dalla parte del lettore:
/// la prova `ogni_gesto_inoltrato_ha_l_ambito_delle_sue_azioni` le tiene
/// d'accordo. Un brano scelto dalla coda è della coda e non dei
/// comandi: sposta tutti su un'altra voce, come la sposterebbe un brano
/// aggiunto e fatto partire.
#[must_use]
pub const fn ambito(gesto: &Gesto) -> Option<Ambito> {
    match gesto {
        Gesto::Pausa
        | Gesto::Riprendi
        | Gesto::Alterna
        | Gesto::Prossimo
        | Gesto::Precedente
        | Gesto::VaiA { .. }
        | Gesto::Ripeti => Some(Ambito::Comandi),
        Gesto::Suona { .. }
        | Gesto::Accoda { .. }
        | Gesto::Dopo { .. }
        | Gesto::CodaVai { .. }
        | Gesto::CodaTogli { .. }
        | Gesto::CodaRiordina { .. } => Some(Ambito::Coda),
        // Svuotare toglierebbe la coda a tutti; il resto cambierebbe la musica
        // degli altri senza che lo vedano: vedi il `//!`.
        Gesto::CodaSvuota
        | Gesto::CodaRipristina
        | Gesto::Radio
        | Gesto::Shuffle
        | Gesto::Autoplay
        | Gesto::Timer => None,
    }
}

/// Com'è andata la strada, per il comando che l'ha chiesta.
#[derive(Debug)]
pub enum Instradato {
    /// Il comando prosegue qui, come sempre.
    Locale,
    /// Il comando prosegue qui con questi id al posto di quelli che aveva:
    /// le maniglie (negative) delle voci di sessione appena create. Solo chi
    /// ospita, per [`Regola::LocaleConVoci`].
    ConVoci(Vec<i64>),
    /// Il gesto è stato mandato all'host: il comando torna `Ok(())` senza fare
    /// altro. Quel che cambia arriva con il prossimo stato della sessione.
    Inoltrato,
    /// Il gesto non si fa: il comando torna questo errore.
    Rifiutato(AppError),
}

#[cfg(test)]
mod prove {
    use super::*;

    fn tutti() -> Vec<Gesto> {
        vec![
            Gesto::Pausa,
            Gesto::Riprendi,
            Gesto::Alterna,
            Gesto::Prossimo,
            Gesto::Precedente,
            Gesto::VaiA { ms: 1 },
            Gesto::Suona {
                ids: vec![1],
                da: 0,
            },
            Gesto::Radio,
            Gesto::Accoda { ids: vec![1] },
            Gesto::Dopo { ids: vec![1] },
            Gesto::CodaVai { indice: 0 },
            Gesto::CodaTogli { indici: vec![0] },
            Gesto::CodaRiordina { da: 0, a: 1 },
            Gesto::CodaSvuota,
            Gesto::CodaRipristina,
            Gesto::Shuffle,
            Gesto::Ripeti,
            Gesto::Autoplay,
            Gesto::Timer,
        ]
    }

    const LIVELLI: [Diritti; 3] = [Diritti::Ascolta, Diritti::Comanda, Diritti::Tutto];

    #[test]
    fn ogni_gesto_ha_una_regola_in_ogni_ruolo() {
        for gesto in tutti() {
            assert_eq!(regola(Ruolo::Fuori, &gesto), Regola::Locale, "{gesto:?}");
            assert_ne!(regola(Ruolo::Host, &gesto), Regola::Inoltra, "{gesto:?}");
            assert_ne!(
                regola(Ruolo::Host, &gesto),
                Regola::NonConcesso,
                "{gesto:?}"
            );
            for diritti in LIVELLI {
                let membro = Ruolo::Membro(diritti);
                assert_ne!(
                    regola(membro, &gesto),
                    Regola::Locale,
                    "chi segue non suona da sé: {gesto:?}"
                );
                assert_ne!(regola(membro, &gesto), Regola::LocaleConVoci);
            }
        }
    }

    /// La tabella intera, gesto per gesto e livello per livello.
    #[test]
    fn chi_segue_manda_solo_quel_che_gli_e_concesso() {
        use Regola::{Inoltra, NonConcesso, Rifiuta};
        for gesto in tutti() {
            let attese = match gesto {
                Gesto::Pausa
                | Gesto::Riprendi
                | Gesto::Alterna
                | Gesto::Prossimo
                | Gesto::Precedente
                | Gesto::VaiA { .. }
                | Gesto::Ripeti => [NonConcesso, Inoltra, Inoltra],
                Gesto::Suona { .. }
                | Gesto::Accoda { .. }
                | Gesto::Dopo { .. }
                | Gesto::CodaVai { .. }
                | Gesto::CodaTogli { .. }
                | Gesto::CodaRiordina { .. } => [NonConcesso, NonConcesso, Inoltra],
                Gesto::CodaSvuota
                | Gesto::CodaRipristina
                | Gesto::Radio
                | Gesto::Shuffle
                | Gesto::Autoplay
                | Gesto::Timer => [Rifiuta, Rifiuta, Rifiuta],
            };
            for (diritti, attesa) in LIVELLI.into_iter().zip(attese) {
                assert_eq!(
                    regola(Ruolo::Membro(diritti), &gesto),
                    attesa,
                    "{gesto:?} con {diritti:?}"
                );
            }
        }
    }

    /// Chi segue si ferma da sé con `ambito`, chi ospita con `Azione::ambito`:
    /// se le due divisioni non fossero d'accordo, un gesto fermato qui
    /// passerebbe di là, o il contrario — e chi comanda vedrebbe un tasto
    /// acceso che risponde «non concesso».
    #[test]
    fn ogni_gesto_inoltrato_ha_l_ambito_delle_sue_azioni() {
        use crate::insieme::gesti::{Traduzione, traduci};
        let specchio = crate::riproduzione::insieme::Istantanea {
            coda: vec![10, 11, 12],
            corrente: Some(11),
            in_pausa: false,
            ripeti: rymembra_domain::queue::RepeatMode::Off,
        };
        let mut provati = 0;
        for gesto in tutti() {
            let Some(atteso) = ambito(&gesto) else {
                continue;
            };
            match traduci(&gesto, &specchio) {
                Ok(Traduzione::Azioni(azioni)) => {
                    assert!(!azioni.is_empty(), "{gesto:?}");
                    for azione in azioni {
                        assert_eq!(azione.ambito(), Some(atteso), "{gesto:?} → {azione:?}");
                    }
                }
                // Diventa un `Azione::Aggiungi`, che è della coda.
                Ok(Traduzione::Aggiungi { .. }) => {
                    assert_eq!(atteso, Ambito::Coda, "{gesto:?}");
                }
                Err(err) => panic!("{gesto:?}: {}", err.code().kind().code()),
            }
            provati += 1;
        }
        assert_eq!(provati, 13, "ogni gesto che chi segue manda");
    }

    #[test]
    fn quel_che_cambierebbe_la_musica_degli_altri_di_nascosto_si_rifiuta() {
        for gesto in [
            Gesto::Radio,
            Gesto::Shuffle,
            Gesto::Autoplay,
            Gesto::Timer,
            Gesto::CodaRipristina,
        ] {
            assert_eq!(regola(Ruolo::Host, &gesto), Regola::Rifiuta, "{gesto:?}");
            for diritti in LIVELLI {
                assert_eq!(
                    regola(Ruolo::Membro(diritti), &gesto),
                    Regola::Rifiuta,
                    "{gesto:?}"
                );
            }
        }
    }

    #[test]
    fn i_brani_di_chi_ospita_diventano_voci() {
        for gesto in [
            Gesto::Suona {
                ids: vec![3],
                da: 0,
            },
            Gesto::Accoda { ids: vec![3] },
            Gesto::Dopo { ids: vec![3] },
        ] {
            assert_eq!(regola(Ruolo::Host, &gesto), Regola::LocaleConVoci);
            assert_eq!(
                regola(Ruolo::Membro(Diritti::Tutto), &gesto),
                Regola::Inoltra
            );
        }
    }
}
