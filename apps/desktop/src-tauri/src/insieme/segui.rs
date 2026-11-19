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
//! Tranne quando l'host è fermo: la pausa e il punto da cui ripartirà sono
//! posizioni, non istanti, e chi segue si ferma subito anche senza stima — a
//! metà sessione la stima può sparire per qualche secondo (decisione D-31).
//! E se ogni scambio supera il tetto dell'rtt per [`TROPPO_LENTA_DOPO_NS`],
//! la finestra dice che la rete è troppo lenta per seguire invece di
//! «aggancio…» per sempre.
//!
//! # Quando si dice «agganciato»
//!
//! Quando l'errore filtrato del regolatore resta entro [`SOGLIA_AGGANCIO_NS`]
//! per [`DURATA_AGGANCIO_NS`] di fila, senza mosse di mezzo, e la stima
//! dell'orologio non è più incerta della stessa soglia. Con l'host in pausa
//! non c'è niente da misurare: bastano un orologio altrettanto preciso e la
//! voce dell'host in mano al suo punto, perché la sincronia si vedrà alla
//! ripresa, che è programmata (decisione D-31: prima bastava la stima).

use crate::nucleo::Nucleo;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCodeKind};
pub use rymembra_play::sincronia::orologio::Esito as EsitoScambio;
use rymembra_play::sincronia::orologio::{
    Orologio, Perche, Scambio, Stima, Taratura as TaraturaOrologio,
};
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
pub struct PonteVero(pub Nucleo);

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
    /// Da trenta secondi la rete è troppo lenta per una stima: vedi
    /// [`Condivisa::troppo_lenta`].
    pub rete_lenta: bool,
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
    /// L'host è raggiunto via internet: tetto dell'rtt e anticipo più larghi.
    pub lontano: bool,
    /// Da quando (ns locali, l'istante d'arrivo dello scambio) ogni scambio
    /// supera il tetto dell'rtt, senza una stima. Vedi
    /// [`Condivisa::troppo_lenta`].
    lenti_dal: Mutex<Option<u64>>,
}

/// Il tetto dell'andata e ritorno di uno scambio d'orologio con un host
/// raggiunto via internet: 400 ms. Quello di casa (100 ms) scarterebbe quasi
/// tutto quel che passa da un relay o da una rete mobile, e lo stimatore non
/// darebbe mai uno scarto. Il filtro sul minimo resta: i campioni migliori
/// pesano di più, e un rtt alto allarga la soglia dei salti
/// (`Taratura::salto_ns`).
const RTT_TETTO_INTERNET_NS: u64 = 400_000_000;

/// Quanto avanti chi segue un host raggiunto via internet programma le sue
/// riaperture: 400 ms invece dei 250 di casa (decisione D-31). Via relay
/// l'ancora arriva già con un paio di centinaia di millisecondi di ritardo, e
/// aprire una voce in rete fa più giri: con 250 ms si entrava quasi sempre in
/// ritardo, cioè con un salto. È lo stesso numero di chi ospita una sessione
/// con qualcuno via internet (`riproduzione::insieme::ANTICIPO_INTERNET_NS`).
pub const ANTICIPO_INTERNET_NS: u64 = 400_000_000;

/// Dopo quanto, con ogni scambio d'orologio oltre il tetto dell'rtt e nessuna
/// stima, la finestra dice «rete troppo lenta per seguire» invece di
/// «aggancio…»: trenta secondi (decisione D-31). Il README parlava di «may
/// take a while», ma con ogni scambio scartato la stima non arriva mai.
pub const TROPPO_LENTA_DOPO_NS: u64 = 30_000_000_000;

impl Condivisa {
    /// Tutto vuoto: nessuna ancora, nessuna stima. Per le prove: il filo la
    /// crea con [`Condivisa::nuova_per`], che sa da che strada arriva l'host.
    #[cfg(test)]
    #[must_use]
    pub fn nuova() -> Self {
        Self::nuova_per(false)
    }

    /// Come [`Condivisa::nuova`], per un host raggiunto via internet
    /// (`lontano`): l'orologio accetta scambi più lenti, vedi
    /// [`RTT_TETTO_INTERNET_NS`].
    #[must_use]
    pub fn nuova_per(lontano: bool) -> Self {
        let taratura = if lontano {
            TaraturaOrologio {
                rtt_tetto_ns: RTT_TETTO_INTERNET_NS,
                ..TaraturaOrologio::default()
            }
        } else {
            TaraturaOrologio::default()
        };
        Self {
            ancora: Mutex::new(None),
            orologio: Mutex::new(Orologio::nuovo(taratura)),
            aggancio: Mutex::new(Aggancio::default()),
            fermo: AtomicBool::new(false),
            uscita_cambiata: AtomicBool::new(false),
            lontano,
            lenti_dal: Mutex::new(None),
        }
    }

    /// Uno scambio d'orologio arrivato dal trasporto. Restituisce che cosa ne
    /// è stato: un [`EsitoScambio::Ricominciato`] lo deve sapere il filo della
    /// sessione (vedi `filo::rete`), che rimette i ping alla cadenza svelta.
    pub fn scambio(&self, s: Scambio) -> EsitoScambio {
        let mut orologio = self.orologio.lock().unwrap_or_else(PoisonError::into_inner);
        let esito = orologio.aggiungi(s);
        let senza_stima = orologio.stima().is_none();
        drop(orologio);
        let mut dal = self
            .lenti_dal
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if senza_stima && esito == EsitoScambio::Scartato(Perche::Lento) {
            dal.get_or_insert(s.t4);
        } else if !matches!(esito, EsitoScambio::Scartato(Perche::Incoerente)) {
            // Uno scambio che passa il tetto (o una stima che c'è): la rete
            // non è troppo lenta. Uno incoerente non dice niente della rete.
            *dal = None;
        }
        esito
    }

    /// Da [`TROPPO_LENTA_DOPO_NS`] ogni scambio supera il tetto dell'rtt, e
    /// non c'è una stima: chi segue non può seguire, e la finestra lo dice.
    #[must_use]
    pub fn troppo_lenta(&self, adesso: u64) -> bool {
        self.lenti_dal
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|dal| adesso.saturating_sub(dal) >= TROPPO_LENTA_DOPO_NS)
    }

    /// Un'ancora nuova dall'host. Vero se cambia lo stato (voce, in corso, o
    /// un salto): allora conviene un passo subito, senza aspettare il colpo.
    pub fn ancora(&self, nuova: DallOspite) -> bool {
        let mut ancora = self.ancora.lock().unwrap_or_else(PoisonError::into_inner);
        let cambia = ancora.is_none_or(|prima| {
            prima.etichetta != nuova.etichetta
                || prima.in_corso != nuova.in_corso
                // Un avvio programmato nuovo — un salto dell'host nella
                // stessa voce — non deve aspettare il colpo dopo.
                || (nuova.programmata && prima != nuova)
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
    /// L'ultima riapertura non riuscita, e fino a quando non riprovarla: vedi
    /// «Una riapertura che non riesce» in [`Seguito::passo`].
    rimandata: Option<Rimandata>,
}

/// Una riapertura non riuscita, che non si ritenta prima di `fino`.
#[derive(Debug, Clone, Copy)]
struct Rimandata {
    /// La voce che non si è aperta.
    etichetta: u64,
    /// Il perché.
    codice: ErrorCodeKind,
    /// Quante volte di fila.
    volte: u32,
    /// Non prima di quando riprovare, ns locali.
    fino: u64,
    /// L'ancora dell'host di allora: un'ancora programmata diversa è un
    /// gesto nuovo dell'host, e si riprova subito.
    ancora: DallOspite,
}

/// La prima attesa dopo una riapertura non riuscita: mezzo secondo, poi il
/// doppio a ogni volta, fino a [`RIMANDA_AL_PIU_NS`].
const RIMANDA_DOPO_NS: u64 = 500_000_000;

/// L'attesa più lunga fra due riaperture non riuscite: otto secondi.
const RIMANDA_AL_PIU_NS: u64 = 8_000_000_000;

impl Default for Seguito {
    fn default() -> Self {
        Self::per(false)
    }
}

impl Seguito {
    /// Da capo, per un host raggiunto via internet (`lontano`) o in casa:
    /// via internet il regolatore programma le riaperture
    /// [`ANTICIPO_INTERNET_NS`] più in là.
    #[must_use]
    pub fn per(lontano: bool) -> Self {
        let taratura = if lontano {
            TaraturaRegolatore {
                anticipo_ns: ANTICIPO_INTERNET_NS,
                ..TaraturaRegolatore::default()
            }
        } else {
            TaraturaRegolatore::default()
        };
        Self {
            regolatore: Regolatore::nuovo(taratura),
            frequenza: None,
            entro_dal: None,
            ultimo_guasto: None,
            rimandata: None,
        }
    }
}

/// La voce che il motore di qui ha in mano è quella dell'host, al punto
/// dell'host: con l'host fermo, è quel che serve per dirsi agganciati (la
/// ripresa è programmata). Un punto lontano più del salto che fa riaprire il
/// regolatore vuol dire che il motore non ci è arrivato.
fn al_punto_dell_host(mia: Option<&DaMe>, ancora: &DallOspite) -> bool {
    let salto_us = TaraturaRegolatore::default()
        .salto_ospite_ns
        .checked_div(1_000)
        .unwrap_or(0);
    mia.is_some_and(|m| {
        m.etichetta == Some(ancora.etichetta)
            && m.posizione_us.abs_diff(ancora.posizione_us) <= salto_us
    })
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

    /// Un passo. Vedi il `//!` del modulo per le regole: niente senza stima
    /// (tranne la pausa), e quando si dice agganciato.
    ///
    /// # Una riapertura che non riesce
    ///
    /// Non si ritenta al passo dopo: la stessa voce, con lo stesso perché,
    /// aspetta mezzo secondo, poi uno, due, fino a otto. Una voce che non si
    /// apre per un motivo che non passa da solo — un brano di catalogo che di
    /// qui non si raggiunge, un gettone rifiutato — si chiedeva dieci volte al
    /// secondo, ognuna con un filo nuovo e una richiesta all'host (reperto
    /// R05-4). L'attesa vale solo per le riaperture: pause e posizioni
    /// dell'host passano sempre, e una voce nuova (o un avvio programmato
    /// nuovo dell'host) si prova subito.
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
                    rete_lenta: false,
                },
            }
        };
        let Some(frequenza) = motore.frequenza() else {
            return fermo(&mut self.entro_dal);
        };
        // Senza stima non si fa niente che dipenda dall'orologio: vedi «Mai
        // senza stima». Ma un'ancora ferma non ne dipende — la pausa e il
        // punto da cui ripartire sono posizioni, non istanti — e chi segue si
        // deve fermare subito quando l'host si ferma, anche se la stima è
        // appena sparita (reperto R05-3, decisione D-31). Lì il regolatore
        // riceve `None`, che per un'ancora ferma non conta.
        let host_fermo = ancora.is_some_and(|a| !a.in_corso);
        if stima.is_none() && !host_fermo {
            return fermo(&mut self.entro_dal);
        }
        if self.frequenza.is_some_and(|f| f != frequenza) {
            // Un'altra frequenza è quasi sempre un'altra uscita: lo stesso
            // azzeramento del cambio d'uscita, anche se nessuno l'ha detto.
            self.azzera();
        }
        self.frequenza = Some(frequenza);
        if let (Some(r), Some(a)) = (self.rimandata, ancora.as_ref())
            && (a.etichetta != r.etichetta || (a.programmata && *a != r.ancora))
        {
            self.rimandata = None;
        }
        let mia = motore.da_me();
        let mut mossa = self.regolatore.passo(
            adesso,
            ancora.as_ref(),
            mia.as_ref(),
            stima.as_ref(),
            frequenza,
        );
        if let Mossa::Riapri { etichetta, .. } = mossa
            && self
                .rimandata
                .is_some_and(|r| r.etichetta == etichetta && adesso < r.fino)
        {
            // Non adesso: vedi «Una riapertura che non riesce». Il regolatore
            // torna libero, come per una mossa non riuscita, così non aspetta
            // la fine di una riapertura mai partita.
            self.regolatore.mossa_non_riuscita();
            mossa = Mossa::Niente;
        }
        if mossa != Mossa::Niente {
            match motore.applica(mossa, frequenza) {
                Ok(()) => {
                    self.ultimo_guasto = None;
                    if matches!(mossa, Mossa::Riapri { .. }) {
                        self.rimandata = None;
                    }
                }
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
                    if let (Mossa::Riapri { etichetta, .. }, Some(a)) = (mossa, ancora) {
                        self.rimanda(etichetta, codice, a, adesso);
                    }
                }
            }
        }
        let diagnosi = self.regolatore.diagnosi();
        let errore_ns = diagnosi.errore_filtrato_ns;
        let in_corso = ancora.is_some_and(|a| a.in_corso);
        // Un orologio che sbaglia più della soglia non può dire che due PC
        // suonano entro la soglia: via internet l'incertezza può essere di
        // decine di millisecondi (decisione D-31).
        let orologio_preciso = incertezza_ns.is_some_and(|i| i <= SOGLIA_AGGANCIO_NS);
        let agganciato = match (ancora, mossa) {
            (None, _) => {
                self.entro_dal = None;
                false
            }
            // Ospite fermo: niente da misurare. Basta un orologio preciso e la
            // voce dell'host in mano, al suo punto: la ripresa è programmata.
            // Prima bastava la stima, e «agganciato» compariva anche con la
            // voce che non si era potuta aprire (decisione D-31).
            (Some(a), _) if !in_corso => {
                self.entro_dal = None;
                orologio_preciso && al_punto_dell_host(mia.as_ref(), &a)
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
                    orologio_preciso && adesso.saturating_sub(dal) >= DURATA_AGGANCIO_NS
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
                rete_lenta: false,
            },
        }
    }

    /// Una riapertura di `etichetta` non è riuscita per `codice`: la prossima
    /// non prima di un'attesa che raddoppia, se il perché è lo stesso.
    fn rimanda(&mut self, etichetta: u64, codice: ErrorCodeKind, ancora: DallOspite, adesso: u64) {
        let volte = self
            .rimandata
            .filter(|r| r.etichetta == etichetta && r.codice == codice)
            .map_or(1, |r| r.volte.saturating_add(1));
        let attesa = RIMANDA_DOPO_NS
            .saturating_mul(1_u64 << volte.saturating_sub(1).min(8))
            .min(RIMANDA_AL_PIU_NS);
        self.rimandata = Some(Rimandata {
            etichetta,
            codice,
            volte,
            fino: adesso.saturating_add(attesa),
            ancora,
        });
    }
}

/// Il ciclo del filo della sincronia: un passo ogni [`PASSO`], e subito quando
/// arriva un'ancora che cambia lo stato. Esce quando `fermo` si alza o il
/// campanello si chiude.
///
/// # Un panico in un passo
///
/// Ogni passo è [`crate::stato::sorvegliato`] da solo, con la scrittura
/// dell'aggancio dentro: una mossa può aprire una voce (decodificatore,
/// apertura in rete, dispositivo audio), e lì un file storto può far cadere
/// in panico. Prima il sorvegliato avvolgeva il ciclo intero: il panico lo
/// prendeva, ma il filo finiva e nessuno lo rifaceva — questo PC non seguiva
/// più niente fino all'uscita, e la finestra restava sull'ultimo aggancio
/// misurato, magari «agganciato». Adesso dopo una caduta il regolatore
/// riparte da capo, l'aggancio torna «non agganciato», e il passo dopo arriva
/// dopo [`crate::stato::attesa_dopo_la_caduta`] (5 s, poi il doppio): una voce
/// che fa cadere ogni riapertura non riempie il diario dieci volte al
/// secondo. L'attesa è sulla sveglia: un'ancora che cambia lo stato — un
/// altro brano dell'host — o l'uscita la interrompono, e si riprova subito.
pub fn ciclo(
    condivisa: &Condivisa,
    motore: &dyn Motore,
    sveglia: &Receiver<()>,
    adesso: fn() -> u64,
) {
    let mut seguito = Seguito::per(condivisa.lontano);
    let mut cadute_di_fila = 0_u32;
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
        let fatto = crate::stato::sorvegliato("rymembra-insieme-sincronia", || {
            let t = adesso();
            let mut esito = seguito.passo(t, ancora, stima, motore);
            esito.aggancio.rete_lenta = stima.is_none() && condivisa.troppo_lenta(t);
            *condivisa
                .aggancio
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = esito.aggancio;
        });
        let attesa = if fatto.is_some() {
            cadute_di_fila = 0;
            PASSO
        } else {
            cadute_di_fila = cadute_di_fila.saturating_add(1);
            seguito = Seguito::per(condivisa.lontano);
            *condivisa
                .aggancio
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Aggancio::default();
            crate::stato::attesa_dopo_la_caduta(cadute_di_fila)
        };
        match sveglia.recv_timeout(attesa) {
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
    app: Nucleo,
    condivisa: Arc<Condivisa>,
    sveglia: Receiver<()>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("rymembra-insieme-sincronia".to_owned())
        .spawn(move || {
            let motore = PonteVero(app);
            // Un panico in un passo lo prende `ciclo`, passo per passo, e il
            // filo continua (vedi «Un panico in un passo»). Questo è l'ultimo
            // riparo, per un panico fuori da un passo: il filo finisce, ma non
            // si porta via il processo.
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

    use rymembra_play::sincronia::orologio::Orologio;

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

    /// Un motore che cade in panico alla prima mossa — il decodificatore di
    /// una voce storta, sotto `applica` — e poi obbedisce. Alla seconda mossa
    /// guarda che aggancio la finestra vedeva, e alza `fermo`.
    struct Fragile {
        finto: Finto,
        condivisa: Arc<Condivisa>,
        mosse: std::cell::Cell<u32>,
        passi: std::cell::Cell<u32>,
        agganciato_dopo_la_caduta: std::cell::Cell<Option<bool>>,
    }

    impl Motore for Fragile {
        fn da_me(&self) -> Option<DaMe> {
            self.finto.da_me()
        }
        fn frequenza(&self) -> Option<u32> {
            // Una rete sotto la prova: se le mosse non arrivano, il ciclo
            // esce lo stesso dopo qualche passo invece di girare per sempre.
            self.passi.set(self.passi.get() + 1);
            if self.passi.get() > 20 {
                self.condivisa.fermo.store(true, Ordering::Release);
            }
            Some(F)
        }
        fn applica(&self, mossa: Mossa, f: u32) -> Result<(), AppError> {
            self.mosse.set(self.mosse.get() + 1);
            assert!(self.mosse.get() > 1, "decodificatore rotto");
            self.agganciato_dopo_la_caduta
                .set(Some(self.condivisa.aggancio().agganciato));
            self.condivisa.fermo.store(true, Ordering::Release);
            self.finto.applica(mossa, f)
        }
    }

    #[test]
    fn un_panico_in_una_mossa_non_ferma_la_sincronia() {
        const ADESSO: u64 = 1_000 * MS;
        let condivisa = Arc::new(Condivisa::nuova());
        // Un attimo prima si era agganciati.
        *condivisa.aggancio.lock().unwrap() = Aggancio {
            agganciato: true,
            errore_ns: Some(0),
            incertezza_ns: Some(200_000),
            rete_lenta: false,
        };
        // Una stima dell'orologio subito: un punto basta.
        *condivisa.orologio.lock().unwrap() = Orologio::nuovo(TaraturaOrologio {
            punti_minimi: 1,
            ..TaraturaOrologio::default()
        });
        let l = ADESSO - MS;
        condivisa.scambio(Scambio {
            t1: l,
            t2: (l + 100_000).saturating_add_signed(SCARTO),
            t3: (l + 200_000).saturating_add_signed(SCARTO),
            t4: l + 300_000,
        });
        assert!(condivisa.orologio.lock().unwrap().stima().is_some());
        condivisa.ancora(ancora_dell_host(ADESSO));
        let motore = Fragile {
            finto: Finto::default(),
            condivisa: Arc::clone(&condivisa),
            mosse: std::cell::Cell::new(0),
            passi: std::cell::Cell::new(0),
            agganciato_dopo_la_caduta: std::cell::Cell::new(None),
        };
        *motore.finto.adesso.borrow_mut() = ADESSO;
        // Fermo su un'altra voce: il regolatore riapre quella dell'host al
        // primo passo.
        motore.finto.stato.borrow_mut().etichetta = Some(4);
        // Un'ancora nuova arrivata nel frattempo: sveglia il filo prima
        // dell'attesa dopo la caduta.
        let (sveglia, orecchio) = std::sync::mpsc::channel();
        sveglia.send(()).unwrap();
        let uscito = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ciclo(&condivisa, &motore, &orecchio, || ADESSO);
        }));
        assert!(
            uscito.is_ok(),
            "il panico è uscito dal ciclo: la sincronia è ferma per tutta la sessione"
        );
        assert_eq!(motore.mosse.get(), 2, "dopo la caduta si riprova");
        assert_eq!(
            motore.agganciato_dopo_la_caduta.get(),
            Some(false),
            "dopo la caduta la finestra non deve leggere «agganciato»"
        );
    }

    /// Quaranta passi agganciati alla voce 5: chi segue suona.
    fn agganciato(motore: &Finto, seguito: &mut Seguito) -> u64 {
        let mut t = 1_000 * MS;
        for _ in 0..40 {
            *motore.adesso.borrow_mut() = t;
            seguito.passo(t, Some(ancora_dell_host(t)), Some(stima()), motore);
            t += 100 * MS;
        }
        assert!(motore.punto_us(t).1, "suona");
        t
    }

    #[test]
    fn senza_stima_una_pausa_dell_host_ferma_chi_segue() {
        // Reperto R05-3 e decisione D-31: a metà sessione la stima può
        // sparire (un riavvio dell'orologio dell'host, uno di quelli falsi di
        // R05-2), e per otto secondi chi segue non seguiva niente — nemmeno la
        // pausa, che non ha bisogno dell'orologio.
        let motore = Finto::default();
        let mut seguito = Seguito::default();
        let t = agganciato(&motore, &mut seguito);
        let ferma = DallOspite {
            in_corso: false,
            programmata: false,
            ..ancora_dell_host(t)
        };
        *motore.adesso.borrow_mut() = t;
        let esito = seguito.passo(t, Some(ferma), None, &motore);
        assert_eq!(esito.mossa, Mossa::Pausa);
        assert!(!motore.punto_us(t + 500 * MS).1, "fermo");
        assert!(
            !esito.aggancio.agganciato,
            "senza stima non si è agganciati"
        );
        // Ma un'ancora in corso, senza stima, resta senza mosse: quella sì
        // vuole l'orologio.
        let mosse = motore.mosse.borrow().len();
        let esito = seguito.passo(t + 100 * MS, Some(ancora_dell_host(t)), None, &motore);
        assert_eq!(esito.mossa, Mossa::Niente);
        assert_eq!(motore.mosse.borrow().len(), mosse);
    }

    #[test]
    fn con_l_host_in_pausa_si_e_agganciati_solo_con_la_voce_e_un_orologio_preciso() {
        // Decisione D-31: «agganciato» con l'host fermo voleva dire solo «c'è
        // una stima», anche con la voce non aperta o con un orologio incerto
        // di decine di millisecondi (via internet).
        let ferma = |t| DallOspite {
            in_corso: false,
            programmata: false,
            ..ancora_dell_host(t)
        };
        // Nessuna voce in mano: il motore non è riuscito ad aprirla.
        struct Muto;
        impl Motore for Muto {
            fn da_me(&self) -> Option<DaMe> {
                None
            }
            fn frequenza(&self) -> Option<u32> {
                Some(F)
            }
            fn applica(&self, _: Mossa, _: u32) -> Result<(), AppError> {
                Err(AppError::new(
                    rymembra_domain::errors::ErrorCode::PlaybackSourceUnavailable {
                        track_id: None,
                        path: None,
                    },
                ))
            }
        }
        let mut seguito = Seguito::default();
        let esito = seguito.passo(MS, Some(ferma(MS)), Some(stima()), &Muto);
        assert!(!esito.aggancio.agganciato, "senza la voce");
        // La voce c'è, ma l'orologio sbaglia di 30 ms.
        let motore = Finto::default();
        let mut seguito = Seguito::default();
        let t = agganciato(&motore, &mut seguito);
        let incerta = Stima {
            incertezza_ns: 30 * MS,
            ..stima()
        };
        *motore.adesso.borrow_mut() = t;
        let esito = seguito.passo(t, Some(ferma(t)), Some(incerta), &motore);
        assert!(!esito.aggancio.agganciato, "con un orologio incerto");
        // Voce e orologio buoni: sì.
        let esito = seguito.passo(t + 100 * MS, Some(ferma(t)), Some(stima()), &motore);
        assert!(esito.aggancio.agganciato);
        // E anche con l'host in corso un orologio incerto non è agganciato.
        let motore = Finto::default();
        let mut seguito = Seguito::default();
        let mut t = 1_000 * MS;
        let mut mai = true;
        for _ in 0..80 {
            *motore.adesso.borrow_mut() = t;
            let esito = seguito.passo(t, Some(ancora_dell_host(t)), Some(incerta), &motore);
            mai &= !esito.aggancio.agganciato;
            t += 100 * MS;
        }
        assert!(
            mai,
            "l'errore misurato è piccolo, ma l'orologio non lo sa dire"
        );
    }

    #[test]
    fn una_riapertura_che_non_riesce_non_si_ritenta_a_ogni_passo() {
        // Reperto R05-4: una voce che non si apre (un catalogo che di qui non
        // si raggiunge, un gettone rifiutato) si chiedeva a ogni passo, dieci
        // volte al secondo, con un filo nuovo e una richiesta ogni volta.
        #[derive(Default)]
        struct Rotto {
            tentate: std::cell::Cell<u32>,
        }
        impl Motore for Rotto {
            // Fermo, senza una voce in mano: il regolatore chiede subito la
            // riapertura, senza il secondo di grazia del gapless.
            fn da_me(&self) -> Option<DaMe> {
                Some(DaMe {
                    etichetta: None,
                    posizione_us: 0,
                    udito_ns: 0,
                    in_corso: false,
                    in_correzione: false,
                    riserva_us: 0,
                })
            }
            fn frequenza(&self) -> Option<u32> {
                Some(F)
            }
            fn applica(&self, mossa: Mossa, _: u32) -> Result<(), AppError> {
                if matches!(mossa, Mossa::Riapri { .. }) {
                    self.tentate.set(self.tentate.get() + 1);
                }
                Err(AppError::new(
                    rymembra_domain::errors::ErrorCode::PlaybackSourceUnavailable {
                        track_id: None,
                        path: None,
                    },
                ))
            }
        }
        let motore = Rotto::default();
        let mut seguito = Seguito::default();
        let mut t = 1_000 * MS;
        for _ in 0..20 {
            seguito.passo(t, Some(ancora_dell_host(t)), Some(stima()), &motore);
            t += 100 * MS;
        }
        let tentate = motore.tentate.get();
        assert!(
            (2..=4).contains(&tentate),
            "in due secondi {tentate} riaperture"
        );
        // Un'altra voce dell'host: si prova subito.
        let altra = DallOspite {
            etichetta: 6,
            programmata: true,
            ..ancora_dell_host(t)
        };
        seguito.passo(t, Some(altra), Some(stima()), &motore);
        assert_eq!(motore.tentate.get(), tentate + 1, "la voce nuova subito");
        // E la pausa dell'host passa lo stesso, anche durante l'attesa.
        let motore = Finto::default();
        let mut seguito = Seguito::default();
        let t = agganciato(&motore, &mut seguito);
        let esito = seguito.passo(
            t,
            Some(DallOspite {
                in_corso: false,
                programmata: false,
                ..ancora_dell_host(t)
            }),
            Some(stima()),
            &motore,
        );
        assert_eq!(esito.mossa, Mossa::Pausa);
    }

    #[test]
    fn una_rete_sempre_sopra_il_tetto_dopo_trenta_secondi_si_dice() {
        // Decisione D-31: con ogni scambio oltre il tetto dell'rtt la stima
        // non arriva mai, e la finestra restava su «aggancio…» per sempre.
        let c = Condivisa::nuova();
        let lento = |t: u64| Scambio {
            t1: t,
            t2: t + 75 * MS,
            t3: t + 75 * MS,
            t4: t + 150 * MS,
        };
        let mut t = 1_000 * MS;
        for _ in 0..29 {
            c.scambio(lento(t));
            t += 1_000 * MS;
        }
        assert!(!c.troppo_lenta(t), "dopo 29 s ancora no");
        c.scambio(lento(t));
        t += 2_000 * MS;
        c.scambio(lento(t));
        assert!(c.troppo_lenta(t), "dopo trenta secondi sì");
        // Uno scambio buono: non è più vero.
        c.scambio(Scambio {
            t1: t,
            t2: t + MS,
            t3: t + MS,
            t4: t + 2 * MS,
        });
        assert!(!c.troppo_lenta(t + 2 * MS));
    }

    #[test]
    fn via_internet_l_anticipo_e_di_quattrocento_millisecondi() {
        // Decisione D-31: via relay l'ancora arriva già con un paio di
        // centinaia di millisecondi di ritardo e l'apertura in rete fa più
        // giri, quindi con 250 ms chi segue entrava quasi sempre tardi.
        let mut lontano = Seguito::per(true);
        let mut vicino = Seguito::per(false);
        let motore = Finto::default();
        *motore.adesso.borrow_mut() = 1_000 * MS;
        let quando = |esito: Esito| match esito.mossa {
            Mossa::Riapri { quando_ns, .. } => quando_ns,
            altra => panic!("attesa una riapertura, non {altra:?}"),
        };
        // Una voce scelta dall'host: si riapre subito.
        let a = DallOspite {
            programmata: true,
            ..ancora_dell_host(1_000 * MS)
        };
        let t = 1_000 * MS;
        assert_eq!(
            quando(lontano.passo(t, Some(a), Some(stima()), &motore)),
            t + 400 * MS
        );
        let motore = Finto::default();
        *motore.adesso.borrow_mut() = t;
        assert_eq!(
            quando(vicino.passo(t, Some(a), Some(stima()), &motore)),
            t + 250 * MS
        );
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
