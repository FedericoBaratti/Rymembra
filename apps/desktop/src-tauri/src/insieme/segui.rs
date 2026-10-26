//! Chi segue: il regolatore a dieci colpi al secondo, collegato al motore.
//!
//! Il regolatore (`rymembra_play::sincronia::regolatore`) è puro: confronta
//! l'ancora dell'host con quella del motore di questo PC e dice una
//! [`Mossa`]. Qui c'è chi glielo fa fare davvero: un filo suo che dieci volte
//! al secondo prende l'ultima ancora arrivata, la stima dell'orologio, quel
//! che dice il motore ([`Motore::da_me`]), e esegue la mossa
//! ([`Motore::applica`]).
//!
//! # Perché un filo suo e non quello della sessione
//!
//! Perché una mossa può aspettare la rete: `Riapri` apre la voce in rete e la
//! posiziona, e su un MP3 posizionare con esattezza può voler dire rileggerlo
//! dall'inizio. Il filo della sessione intanto deve continuare a ricevere le
//! ancore, l'orologio, e a rispondere alla finestra.
//!
//! # Mai senza stima
//!
//! Il regolatore accetta `orologio = None`, e vuol dire «stessa base dei
//! tempi»: vale per due motori nello stesso processo, nel banco della fase 0.
//! Fra due PC le basi sono diverse di uno scarto qualunque, e `None` vorrebbe
//! dire inseguire un errore di ore. Finché l'[`Orologio`] non ha una stima si
//! aspetta, e la finestra mostra «aggancio».
//!
//! # Quando si dice «agganciato»
//!
//! Quando l'errore filtrato del regolatore resta entro [`SOGLIA_AGGANCIO_NS`]
//! per [`DURATA_AGGANCIO_NS`] di fila, senza mosse di mezzo. Con l'host in
//! pausa non c'è niente da misurare, e basta avere la stima: la sincronia si
//! vedrà alla ripresa, che è programmata.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCodeKind};
use rymembra_play::sincronia::orologio::{Orologio, Scambio, Stima, Taratura as TaraturaOrologio};
use rymembra_play::sincronia::regolatore::{
    DaMe, DallOspite, Mossa, Regolatore, Taratura as TaraturaRegolatore,
};

use crate::nota;

/// Ogni quanto si fa un passo: dieci volte al secondo, come nel banco.
pub const PASSO: Duration = Duration::from_millis(100);

/// L'errore entro cui si dice agganciato: cinque millisecondi, il p95 della
/// soglia della fase 0. Sotto, due casse nella stessa stanza si sentono una.
pub const SOGLIA_AGGANCIO_NS: u64 = 5_000_000;

/// Per quanto l'errore deve restare sotto la soglia: due secondi, venti passi.
/// Meno, e un'ancora fortunata dopo un salto accenderebbe il segno per un
/// istante.
pub const DURATA_AGGANCIO_NS: u64 = 2_000_000_000;

/// Il motore di questo PC, come lo vede il regolatore. È il ponte
/// `riproduzione::insieme`; nelle prove, un motore finto.
pub trait Motore {
    /// L'ancora del motore.
    fn da_me(&self) -> Option<DaMe>;
    /// La frequenza d'uscita.
    fn frequenza(&self) -> Option<u32>;
    /// Esegue una mossa.
    ///
    /// # Errori
    ///
    /// Quelli dell'apertura della voce.
    fn applica(&self, mossa: Mossa, frequenza: u32) -> Result<(), AppError>;
}

/// Il motore vero, attraverso il ponte.
pub struct PonteVero(pub tauri::AppHandle);

impl Motore for PonteVero {
    fn da_me(&self) -> Option<DaMe> {
        crate::riproduzione::insieme::da_me(&self.0)
    }

    fn frequenza(&self) -> Option<u32> {
        crate::riproduzione::insieme::frequenza(&self.0)
    }

    fn applica(&self, mossa: Mossa, frequenza: u32) -> Result<(), AppError> {
        crate::riproduzione::insieme::applica(&self.0, mossa, frequenza)
    }
}

/// Com'è agganciato chi segue, per la finestra.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Aggancio {
    /// Vedi il `//!`.
    pub agganciato: bool,
    /// L'errore filtrato, ns; positivo se questo PC è avanti.
    pub errore_ns: Option<i64>,
    /// L'incertezza della stima dell'orologio, ns.
    pub incertezza_ns: Option<u64>,
}

/// Quel che il filo della sessione e quello della sincronia si passano.
#[derive(Debug)]
pub struct Condivisa {
    /// L'ultima ancora dell'host.
    pub ancora: Mutex<Option<DallOspite>>,
    /// La stima dell'orologio dell'host, nutrita dagli scambi.
    pub orologio: Mutex<Orologio>,
    /// L'ultimo aggancio misurato.
    pub aggancio: Mutex<Aggancio>,
    /// Il filo deve uscire.
    pub fermo: AtomicBool,
    /// L'uscita audio è cambiata: il regolatore riparte da capo.
    pub uscita_cambiata: AtomicBool,
}

impl Condivisa {
    /// Tutto vuoto: nessuna ancora, nessuna stima.
    #[must_use]
    pub fn nuova() -> Self {
        Self {
            ancora: Mutex::new(None),
            orologio: Mutex::new(Orologio::nuovo(TaraturaOrologio::default())),
            aggancio: Mutex::new(Aggancio::default()),
            fermo: AtomicBool::new(false),
            uscita_cambiata: AtomicBool::new(false),
        }
    }

    /// Uno scambio d'orologio arrivato dal trasporto.
    pub fn scambio(&self, s: Scambio) {
        self.orologio
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .aggiungi(s);
    }

    /// Un'ancora nuova dall'host. Vero se cambia lo stato (voce, in corso, o
    /// un salto): allora conviene un passo subito, senza aspettare il colpo.
    pub fn ancora(&self, nuova: DallOspite) -> bool {
        let mut ancora = self.ancora.lock().unwrap_or_else(PoisonError::into_inner);
        let cambia = ancora.is_none_or(|prima| {
            prima.etichetta != nuova.etichetta || prima.in_corso != nuova.in_corso
        });
        *ancora = Some(nuova);
        cambia
    }

    /// L'aggancio, adesso.
    #[must_use]
    pub fn aggancio(&self) -> Aggancio {
        *self.aggancio.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Lo stato del filo fra un passo e l'altro.
#[derive(Debug)]
pub struct Seguito {
    regolatore: Regolatore,
    frequenza: Option<u32>,
    /// Da quando l'errore è entro la soglia, ns locali.
    entro_dal: Option<u64>,
    /// L'ultimo guasto di una mossa, per non scriverlo nel diario a ogni passo.
    ultimo_guasto: Option<ErrorCodeKind>,
}

impl Default for Seguito {
    fn default() -> Self {
        Self {
            regolatore: Regolatore::nuovo(TaraturaRegolatore::default()),
            frequenza: None,
            entro_dal: None,
            ultimo_guasto: None,
        }
    }
}

/// Com'è andato un passo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Esito {
    /// La mossa chiesta al motore.
    pub mossa: Mossa,
    /// L'aggancio dopo il passo.
    pub aggancio: Aggancio,
}

impl Seguito {
    /// Da capo: l'uscita è cambiata, e la deriva imparata era dell'altra.
    pub fn azzera(&mut self) {
        self.regolatore.azzera();
        self.entro_dal = None;
    }

    /// Un passo. Vedi il `//!` del modulo per le due regole: niente senza
    /// stima, e quando si dice agganciato.
    pub fn passo(
        &mut self,
        adesso: u64,
        ancora: Option<DallOspite>,
        stima: Option<Stima>,
        motore: &dyn Motore,
    ) -> Esito {
        let incertezza_ns = stima.map(|s| s.incertezza_ns);
        let fermo = |entro_dal: &mut Option<u64>| {
            *entro_dal = None;
            Esito {
                mossa: Mossa::Niente,
                aggancio: Aggancio {
                    agganciato: false,
                    errore_ns: None,
                    incertezza_ns,
                },
            }
        };
        let (Some(stima), Some(frequenza)) = (stima, motore.frequenza()) else {
            return fermo(&mut self.entro_dal);
        };
        if self.frequenza.is_some_and(|f| f != frequenza) {
            // Un'altra frequenza è quasi sempre un'altra uscita: lo stesso
            // azzeramento del cambio d'uscita, anche se nessuno l'ha detto.
            self.azzera();
        }
        self.frequenza = Some(frequenza);
        let mia = motore.da_me();
        let mossa = self.regolatore.passo(
            adesso,
            ancora.as_ref(),
            mia.as_ref(),
            Some(&stima),
            frequenza,
        );
        if mossa != Mossa::Niente {
            match motore.applica(mossa, frequenza) {
                Ok(()) => self.ultimo_guasto = None,
                Err(err) => {
                    // La mossa non è partita: il regolatore non deve
                    // aspettarne la fine (ignorerebbe pause e salti
                    // dell'host per più di due secondi), ma decidere di nuovo
                    // al passo dopo.
                    self.regolatore.mossa_non_riuscita();
                    let codice = err.code().kind();
                    if self.ultimo_guasto != Some(codice) {
                        nota!(
                            "[insieme] mossa del regolatore non riuscita codice={}",
                            codice.code()
                        );
                    }
                    self.ultimo_guasto = Some(codice);
                }
            }
        }
        let diagnosi = self.regolatore.diagnosi();
        let errore_ns = diagnosi.errore_filtrato_ns;
        let in_corso = ancora.is_some_and(|a| a.in_corso);
        let agganciato = match (ancora, mossa) {
            (None, _) => {
                self.entro_dal = None;
                false
            }
            // Ospite fermo: niente da misurare, basta la stima.
            (Some(_), _) if !in_corso => {
                self.entro_dal = None;
                true
            }
            // Una mossa vera vuol dire che non si era agganciati.
            (
                Some(_),
                Mossa::Riapri { .. }
                | Mossa::Gradino(_)
                | Mossa::AvviaA { .. }
                | Mossa::Pausa
                | Mossa::Posiziona { .. },
            ) => {
                self.entro_dal = None;
                false
            }
            (Some(_), Mossa::Niente | Mossa::Deriva(_)) => match errore_ns {
                Some(e) if e.unsigned_abs() <= SOGLIA_AGGANCIO_NS => {
                    let dal = *self.entro_dal.get_or_insert(adesso);
                    adesso.saturating_sub(dal) >= DURATA_AGGANCIO_NS
                }
                _ => {
                    self.entro_dal = None;
                    false
                }
            },
        };
        Esito {
            mossa,
            aggancio: Aggancio {
                agganciato,
                errore_ns: if in_corso { errore_ns } else { None },
                incertezza_ns,
            },
        }
    }
}

/// Il ciclo del filo della sincronia: un passo ogni [`PASSO`], e subito quando
/// arriva un'ancora che cambia lo stato. Esce quando `fermo` si alza o il
/// campanello si chiude.
pub fn ciclo(
    condivisa: &Condivisa,
    motore: &dyn Motore,
    sveglia: &Receiver<()>,
    adesso: fn() -> u64,
) {
    let mut seguito = Seguito::default();
    loop {
        if condivisa.fermo.load(Ordering::Acquire) || crate::spegnimento::in_uscita() {
            return;
        }
        if condivisa.uscita_cambiata.swap(false, Ordering::AcqRel) {
            seguito.azzera();
        }
        let ancora = *condivisa
            .ancora
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let stima = condivisa
            .orologio
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stima();
        let esito = seguito.passo(adesso(), ancora, stima, motore);
        *condivisa
            .aggancio
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = esito.aggancio;
        match sveglia.recv_timeout(PASSO) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Fa partire il filo della sincronia.
///
/// # Errori
///
/// Quello di `Builder::spawn`.
pub fn avvia(
    app: tauri::AppHandle,
    condivisa: Arc<Condivisa>,
    sveglia: Receiver<()>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("rymembra-insieme-sincronia".to_owned())
        .spawn(move || {
            let motore = PonteVero(app);
            // Sorvegliato: un panico nel decodificatore sotto `applica` non
            // deve portarsi via il processo, e la sessione lo vede come un
            // aggancio che non arriva più.
            crate::stato::sorvegliato("rymembra-insieme-sincronia", || {
                ciclo(
                    &condivisa,
                    &motore,
                    &sveglia,
                    rymembra_play::tempo::adesso_ns,
                );
            });
        })
}

#[cfg(test)]
#[expect(
    clippy::integer_division,
    reason = "microsecondi dai nanosecondi nel motore finto: il troncamento è quello del motore vero"
)]
mod prove {
    use super::*;
    use std::cell::RefCell;

    const MS: u64 = 1_000_000;
    const F: u32 = 48_000;

    /// Un motore finto: suona alla velocità del tempo, e obbedisce alle
    /// mosse come il motore vero (apre fermo, parte all'istante chiesto).
    #[derive(Debug, Default)]
    struct Finto {
        stato: RefCell<Stato>,
        mosse: RefCell<Vec<Mossa>>,
        adesso: RefCell<u64>,
    }

    #[derive(Debug, Default, Clone, Copy)]
    struct Stato {
        etichetta: Option<u64>,
        /// Il punto da cui parte, µs.
        posizione_us: u64,
        /// Parte (o è partito) in questo istante.
        parte: Option<u64>,
    }

    impl Finto {
        fn punto_us(&self, t: u64) -> (u64, bool) {
            let s = *self.stato.borrow();
            match s.parte {
                Some(parte) if t >= parte => (s.posizione_us + (t - parte) / 1_000, true),
                _ => (s.posizione_us, false),
            }
        }
    }

    impl Motore for Finto {
        fn da_me(&self) -> Option<DaMe> {
            let t = *self.adesso.borrow();
            let s = *self.stato.borrow();
            let (posizione_us, in_corso) = self.punto_us(t);
            s.etichetta.map(|etichetta| DaMe {
                etichetta: Some(etichetta),
                posizione_us,
                udito_ns: t,
                in_corso,
                in_correzione: false,
                riserva_us: 2_000_000,
            })
        }

        fn frequenza(&self) -> Option<u32> {
            Some(F)
        }

        fn applica(&self, mossa: Mossa, _frequenza: u32) -> Result<(), AppError> {
            self.mosse.borrow_mut().push(mossa);
            let t = *self.adesso.borrow();
            let mut s = self.stato.borrow_mut();
            match mossa {
                Mossa::Riapri {
                    etichetta,
                    posizione_us,
                    quando_ns,
                } => {
                    *s = Stato {
                        etichetta: Some(etichetta),
                        posizione_us,
                        parte: Some(quando_ns),
                    };
                }
                Mossa::AvviaA { quando_ns, .. } => s.parte = Some(quando_ns),
                Mossa::Pausa => {
                    drop(s);
                    let (p, _) = self.punto_us(t);
                    let mut s = self.stato.borrow_mut();
                    s.posizione_us = p;
                    s.parte = None;
                }
                Mossa::Posiziona {
                    etichetta,
                    posizione_us,
                } => {
                    *s = Stato {
                        etichetta: Some(etichetta),
                        posizione_us,
                        parte: None,
                    };
                }
                Mossa::Niente | Mossa::Gradino(_) | Mossa::Deriva(_) => {}
            }
            Ok(())
        }
    }

    /// L'orologio dell'host è avanti di un'ora sul nostro.
    const SCARTO: i64 = 3_600_000_000_000;

    fn stima() -> Stima {
        Stima {
            riferimento_locale: 0,
            scarto_ns: SCARTO,
            deriva_ppb: 0,
            incertezza_ns: 200_000,
            punti: 10,
        }
    }

    /// L'host suona la voce 5 dal punto 30 s all'istante locale 0.
    fn ancora_dell_host(t_locale: u64) -> DallOspite {
        DallOspite {
            etichetta: 5,
            posizione_us: 30_000_000 + t_locale / 1_000,
            udito_ospite_ns: t_locale.saturating_add_signed(SCARTO),
            in_corso: true,
            programmata: false,
        }
    }

    #[test]
    fn senza_stima_dell_orologio_non_si_muove_niente() {
        let motore = Finto::default();
        let mut seguito = Seguito::default();
        for passo in 0..50 {
            let t = passo * 100 * MS;
            *motore.adesso.borrow_mut() = t;
            let esito = seguito.passo(t, Some(ancora_dell_host(t)), None, &motore);
            assert_eq!(esito.mossa, Mossa::Niente);
            assert!(!esito.aggancio.agganciato);
        }
        assert!(motore.mosse.borrow().is_empty(), "mai `orologio = None`");
    }

    #[test]
    fn da_fermo_si_riapre_la_voce_dell_host_e_poi_si_aggancia() {
        let motore = Finto::default();
        let mut seguito = Seguito::default();
        let mut agganciato_a = None;
        for passo in 0..80 {
            let t = 1_000 * MS + passo * 100 * MS;
            *motore.adesso.borrow_mut() = t;
            let esito = seguito.passo(t, Some(ancora_dell_host(t)), Some(stima()), &motore);
            if esito.aggancio.agganciato && agganciato_a.is_none() {
                agganciato_a = Some(passo);
            }
        }
        let mosse = motore.mosse.borrow();
        let Some(Mossa::Riapri {
            etichetta,
            posizione_us,
            quando_ns,
        }) = mosse.first().copied()
        else {
            panic!("la prima mossa è la riapertura: {mosse:?}");
        };
        assert_eq!(etichetta, 5);
        // Il punto che l'host suonerà all'istante dell'avvio.
        assert_eq!(posizione_us, 30_000_000 + quando_ns / 1_000);
        assert!(
            !mosse
                .iter()
                .skip(1)
                .any(|m| matches!(m, Mossa::Riapri { .. } | Mossa::Gradino(_))),
            "dopo la riapertura basta la deriva: {mosse:?}"
        );
        let (mio, in_corso) = motore.punto_us(9_000 * MS);
        assert!(in_corso);
        assert_eq!(mio, ancora_dell_host(9_000 * MS).posizione_us);
        // Due secondi entro la soglia dopo l'avvio, e il segno si accende.
        let passo = agganciato_a.expect("si aggancia");
        assert!((20..=40).contains(&passo), "agganciato al passo {passo}");
    }

    #[test]
    fn con_l_host_in_pausa_ci_si_ferma_e_si_resta_agganciati() {
        let motore = Finto::default();
        let mut seguito = Seguito::default();
        let mut t = 1_000 * MS;
        for _ in 0..40 {
            *motore.adesso.borrow_mut() = t;
            seguito.passo(t, Some(ancora_dell_host(t)), Some(stima()), &motore);
            t += 100 * MS;
        }
        let ferma = DallOspite {
            in_corso: false,
            programmata: false,
            ..ancora_dell_host(t)
        };
        *motore.adesso.borrow_mut() = t;
        let esito = seguito.passo(t, Some(ferma), Some(stima()), &motore);
        assert_eq!(esito.mossa, Mossa::Pausa);
        assert!(esito.aggancio.agganciato);
        assert_eq!(esito.aggancio.errore_ns, None, "in pausa non si misura");
        assert!(!motore.punto_us(t + 500 * MS).1);
    }

    #[test]
    fn un_altra_frequenza_azzera_il_regolatore() {
        struct Altro(Finto, std::cell::Cell<u32>);
        impl Motore for Altro {
            fn da_me(&self) -> Option<DaMe> {
                self.0.da_me()
            }
            fn frequenza(&self) -> Option<u32> {
                Some(self.1.get())
            }
            fn applica(&self, mossa: Mossa, f: u32) -> Result<(), AppError> {
                self.0.applica(mossa, f)
            }
        }
        let motore = Altro(Finto::default(), std::cell::Cell::new(F));
        let mut seguito = Seguito::default();
        let mut t = 1_000 * MS;
        for _ in 0..40 {
            *motore.0.adesso.borrow_mut() = t;
            seguito.passo(t, Some(ancora_dell_host(t)), Some(stima()), &motore);
            t += 100 * MS;
        }
        motore.1.set(44_100);
        *motore.0.adesso.borrow_mut() = t;
        let esito = seguito.passo(t, Some(ancora_dell_host(t)), Some(stima()), &motore);
        assert!(!esito.aggancio.agganciato, "da capo, si rimisura");
        assert_eq!(seguito.frequenza, Some(44_100));
    }

    #[test]
    fn un_ancora_che_cambia_stato_sveglia_il_filo() {
        let c = Condivisa::nuova();
        let a = ancora_dell_host(0);
        assert!(c.ancora(a));
        assert!(!c.ancora(DallOspite {
            posizione_us: a.posizione_us + 250_000,
            ..a
        }));
        assert!(c.ancora(DallOspite {
            in_corso: false,
            programmata: false,
            ..a
        }));
        assert!(c.ancora(DallOspite { etichetta: 6, ..a }));
    }
}
