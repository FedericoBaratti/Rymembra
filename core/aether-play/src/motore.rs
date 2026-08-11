//! Il filo che decodifica, e quello che tiene il tempo.
//!
//! # Dove sta la posizione
//!
//! Non dove verrebbe da cercarla. Il decodificatore va **avanti** rispetto a
//! quel che si sente: riempie un anello che vale duecento millisecondi, quindi
//! quando ha appena decodificato il secondo 30 di una canzone, dalle casse esce
//! il 29,8. Dire all'utente «30» sarebbe mentire, e si vedrebbe: il cursore
//! arriverebbe in fondo prima della fine del brano.
//!
//! La posizione vera la conosce solo la callback, che conta i fotogrammi che ha
//! davvero consegnato al dispositivo. Questo filo li legge da un'atomica e li
//! traduce in un brano e in un istante attraverso i **segni**: ogni volta che i
//! campioni di un brano nuovo cominciano a entrare nell'anello, si annota a
//! quale fotogramma d'uscita cominceranno. Quando il contatore della callback
//! supera quel numero, quel brano sta suonando — non un istante prima.
//!
//! È lo stesso meccanismo che rende corretto il gapless: fra due brani attaccati
//! non c'è nessun evento, nessuna riapertura, nessuna pausa. C'è un segno.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};

use crate::decodifica::{Decodificatore, Sorgente};
use crate::equalizzatore::{BANDE, Coefficienti};
use crate::uscita::{FormatoUscita, RISERVA_MS, Uscita};
use crate::{Condiviso, guadagno};

/// In quanti passi si passa da una curva dell'equalizzatore all'altra.
///
/// Cambiare di colpo i coefficienti di dieci sezioni — cosa che succede
/// caricando un preset — produce un transitorio che si sente come uno schiocco.
/// Dieci passi da quattro millisecondi l'uno, che è il sonno di questo filo,
/// fanno una quarantina di millisecondi: impercettibili come ritardo,
/// sufficienti a non sentire il gradino. È lo stesso ragionamento della `RAMPA`
/// del volume in [`crate::uscita`], applicato dove la rampa non poteva arrivare.
const PASSI_EQ: u8 = 10;

/// Cosa è successo, per chi sta sopra.
#[derive(Debug, Clone)]
pub enum Evento {
    /// Questo brano ha cominciato a uscire dalle casse **adesso**.
    ///
    /// Con il gapless è anche il segnale che il precedente è finito: non arriva
    /// un «finito» separato perché fra i due non c'è un istante in cui non
    /// suoni niente, e inventarne uno vorrebbe dire mentire su cosa è successo.
    Iniziato {
        /// Quale brano.
        track_id: i64,
    },
    /// Non c'è più niente da suonare: l'ultimo brano è uscito per intero.
    Fermato,
    /// Qualcosa è andato storto.
    Errore(Box<AppError>),
}

/// Dove siamo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Posizione {
    /// Quale brano sta suonando.
    pub track_id: Option<i64>,
    /// A che punto è, in millisecondi.
    pub ms: u64,
    /// Quanto dura, secondo il database.
    pub durata_ms: u64,
    /// È in pausa.
    pub in_pausa: bool,
}

/// Quel che si può chiedere al motore.
enum Comando {
    Suona(Box<Sorgente>),
    Prepara(Option<Box<Sorgente>>),
    Riprendi,
    Pausa,
    Ferma,
    VaiA(u64),
    Volume {
        volume: f32,
        muto: bool,
    },
    ReplayGain {
        attivo: bool,
        bersaglio_db: f32,
    },
    Equalizzatore {
        guadagni: [f32; BANDE],
        attivo: bool,
    },
    Chiudi,
}

/// La maniglia sul motore.
///
/// Ogni metodo manda un messaggio e torna subito: nessuno di questi si blocca
/// aspettando il disco o la scheda audio, perché tutti vengono chiamati dal
/// filo che serve i comandi dell'interfaccia.
pub struct Motore {
    comandi: Sender<Comando>,
    condiviso: Arc<Condiviso>,
    posizione: Arc<std::sync::Mutex<Posizione>>,
    formato: FormatoUscita,
    /// Il lettore dello spettro.
    ///
    /// Dietro un lucchetto, e va bene: chi lo prende è il filo che serve i
    /// comandi, non la callback audio. Il lucchetto sta **fuori** dal percorso
    /// in tempo reale, che è l'unico posto in cui non poteva stare.
    spettro: std::sync::Mutex<crate::spettro::Spettro>,
    /// Tenerla viva tiene acceso il flusso.
    _uscita: Uscita,
}

impl Motore {
    /// Apre il dispositivo e avvia i fili.
    pub fn avvia(osservatore: impl Fn(Evento) + Send + 'static) -> Result<Self, AppError> {
        let condiviso = Condiviso::nuovo();

        // L'anello si dimensiona prima di sapere la frequenza vera, quindi si
        // prende il caso peggiore ragionevole: 192 kHz su 8 canali. Sono meno di
        // due megabyte, e sbagliare per eccesso qui costa memoria mentre
        // sbagliare per difetto costa interruzioni del suono.
        let capacita = usize::try_from(
            192_000u64
                .saturating_mul(8)
                .saturating_mul(RISERVA_MS)
                .saturating_div(1000),
        )
        .unwrap_or(320_000);
        let (produttore, consumatore) = rtrb::RingBuffer::<f32>::new(capacita);

        // Il secondo anello, quello delle curve dell'equalizzatore. Trentadue
        // posti: la callback lo svuota a ogni blocco, quindi in pratica non si
        // riempie mai — e se si riempisse, chi spinge se le tiene invece di
        // perderle.
        let (manda_coefficienti, ricevi_coefficienti) = rtrb::RingBuffer::<Coefficienti>::new(32);

        // Il terzo anello: i campioni che escono, per lo spettro. Otto
        // fotogrammi di finestra — a 48 kHz mezzo secondo — perché chi legge lo
        // fa una trentina di volte al secondo e un ritardo dello scheduler non
        // deve buttare via mezza finestra. Quando è pieno si perde: lo spettro
        // è decorazione, e l'unica alternativa sarebbe far aspettare la
        // callback, cioè l'unica cosa che il tempo reale vieta.
        let (manda_spettro, ricevi_spettro) = rtrb::RingBuffer::<f32>::new(32_768);

        let uscita = crate::uscita::apri(
            consumatore,
            ricevi_coefficienti,
            manda_spettro,
            Arc::clone(&condiviso),
        )?;
        let formato = uscita.formato;
        condiviso
            .canali
            .store(u32::from(formato.canali), Ordering::Relaxed);

        let posizione = Arc::new(std::sync::Mutex::new(Posizione::default()));
        let (manda, ricevi) = std::sync::mpsc::channel();

        let contesto = Contesto {
            comandi: ricevi,
            produttore,
            condiviso: Arc::clone(&condiviso),
            posizione: Arc::clone(&posizione),
            osservatore: Box::new(osservatore),
            formato,
            corrente: None,
            prossimo: None,
            segni: VecDeque::new(),
            resto: VecDeque::new(),
            blocco: Vec::new(),
            spinti: 0,
            annunciato: None,
            fine_dichiarata: true,
            volume: 0.8,
            muto: false,
            replaygain_attivo: true,
            bersaglio_db: -18.0,
            replaygain_corrente: None,
            coefficienti: manda_coefficienti,
            eq_correnti: [0.0; BANDE],
            eq_bersaglio: [0.0; BANDE],
            eq_passi: 0,
            eq_in_attesa: None,
        };

        std::thread::Builder::new()
            .name("aether-decodifica".to_owned())
            .spawn(move || filo(contesto))
            .map_err(|err| {
                AppError::new(ErrorCode::PlaybackEngineUnavailable)
                    .with_cause(format!("filo della decodifica: {err}"))
            })?;

        Ok(Self {
            comandi: manda,
            condiviso,
            posizione,
            formato,
            spettro: std::sync::Mutex::new(crate::spettro::Spettro::nuovo(
                ricevi_spettro,
                formato.frequenza,
            )),
            _uscita: uscita,
        })
    }

    /// Accende o spegne la presa dello spettro.
    ///
    /// Spenta, la callback non scrive niente: chi non ha la schermata aperta
    /// non paga un `push` per campione. Spegnendola si svuota anche quel che
    /// era rimasto, così riaprendo la schermata le barre non ripartono da un
    /// pezzo di musica di dieci minuti fa.
    pub fn guarda_spettro(&self, acceso: bool) {
        self.condiviso
            .spettro
            .store(acceso, std::sync::atomic::Ordering::Relaxed);
        if !acceso && let Ok(mut lettore) = self.spettro.lock() {
            lettore.leggi();
        }
    }

    /// Le dieci bande, ognuna in `0..=1`, nell'ordine di [`crate::CENTRI_HZ`].
    ///
    /// Restituisce `None` solo se il lucchetto è avvelenato — cioè se qualcuno
    /// è caduto tenendolo, che in questo crate non può succedere ma non si
    /// dichiara impossibile con un `unwrap`.
    pub fn spettro(&self) -> Option<[f32; crate::equalizzatore::BANDE]> {
        self.spettro.lock().ok().map(|mut lettore| lettore.leggi())
    }

    /// Come si è aperto il dispositivo.
    #[must_use]
    pub const fn formato(&self) -> FormatoUscita {
        self.formato
    }

    /// Comincia a suonare questo brano, adesso, scartando quel che c'era.
    pub fn suona(&self, sorgente: Sorgente) {
        self.manda(Comando::Suona(Box::new(sorgente)));
    }

    /// Tiene pronto il brano dopo, per attaccarlo senza buco.
    ///
    /// È tutto il gapless: quando il corrente finisce, i campioni del prossimo
    /// sono già in coda per entrare nell'anello, e fra i due non c'è nessuna
    /// apertura di file da aspettare.
    pub fn prepara(&self, sorgente: Option<Sorgente>) {
        self.manda(Comando::Prepara(sorgente.map(Box::new)));
    }

    /// Riprende.
    pub fn riprendi(&self) {
        self.manda(Comando::Riprendi);
    }

    /// Mette in pausa.
    pub fn pausa(&self) {
        self.manda(Comando::Pausa);
    }

    /// Smette e dimentica tutto.
    pub fn ferma(&self) {
        self.manda(Comando::Ferma);
    }

    /// Salta a un punto del brano corrente.
    pub fn vai_a(&self, ms: u64) {
        self.manda(Comando::VaiA(ms));
    }

    /// Cambia volume e silenziamento.
    pub fn volume(&self, volume: f32, muto: bool) {
        self.manda(Comando::Volume { volume, muto });
    }

    /// Accende o spegne la correzione ReplayGain.
    pub fn replaygain(&self, attivo: bool, bersaglio_db: f32) {
        self.manda(Comando::ReplayGain {
            attivo,
            bersaglio_db,
        });
    }

    /// Cambia la curva dell'equalizzatore.
    ///
    /// I guadagni sono in decibel, uno per banda, nell'ordine di
    /// [`crate::equalizzatore::CENTRI_HZ`]. Una curva più corta si completa con
    /// degli zeri e una più lunga si tronca: chi chiama può arrivare da un
    /// valore conservato su disco, e adattarsi qui è meglio che pretendere che
    /// tutti sappiano già quante bande ci sono.
    ///
    /// Il taglio a ±[`crate::equalizzatore::LIMITE_DB`] e lo scarto dei valori
    /// non finiti li fa [`Coefficienti::calcola`], che è l'unico posto in cui
    /// quella regola deve stare.
    pub fn equalizzatore(&self, guadagni: &[f32], attivo: bool) {
        let mut curva = [0.0f32; BANDE];
        for (posto, valore) in curva.iter_mut().zip(guadagni) {
            *posto = *valore;
        }
        self.manda(Comando::Equalizzatore {
            guadagni: curva,
            attivo,
        });
    }

    /// Dove siamo.
    #[must_use]
    pub fn posizione(&self) -> Posizione {
        self.posizione
            .lock()
            .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g)
    }

    /// Il dispositivo è sparito da sotto i piedi.
    #[must_use]
    pub fn dispositivo_perso(&self) -> bool {
        self.condiviso.perso.load(Ordering::Acquire)
    }

    /// Perché è sparito, in una parola che si può mettere in un registro.
    ///
    /// `None` finché non è sparito. Le due cause non sono la stessa cosa per chi
    /// legge: «non c'è più» è un cavo staccato o un dispositivo predefinito
    /// cambiato — riaprire funziona quasi sempre — mentre un guasto del backend
    /// è tutto il resto, e riaprire è un tentativo, non una cura.
    #[must_use]
    pub fn causa_perdita(&self) -> Option<&'static str> {
        if !self.dispositivo_perso() {
            return None;
        }
        Some(match self.condiviso.causa_perdita.load(Ordering::Acquire) {
            1 => "dispositivo non più disponibile",
            2 => "guasto del sistema audio",
            _ => "causa sconosciuta",
        })
    }

    /// Quanti campioni sono stati serviti a vuoto: se cresce, il disco non sta
    /// dietro.
    #[must_use]
    pub fn vuoti(&self) -> u64 {
        self.condiviso.vuoti.load(Ordering::Relaxed)
    }

    fn manda(&self, comando: Comando) {
        // Un motore morto non deve far fallire un clic: l'utente riceve già la
        // diagnosi dal fatto che non si sente niente, e propagare un errore da
        // ogni pulsante riempirebbe la finestra di avvisi identici.
        let _ = self.comandi.send(comando);
    }
}

impl Drop for Motore {
    fn drop(&mut self) {
        self.manda(Comando::Chiudi);
    }
}

/// Un brano già aperto in attesa di attaccarsi al corrente.
///
/// Porta con sé durata e ReplayGain: quando toccherà a lui, la [`Sorgente`] da
/// cui vengono non esisterà più — è stata consumata dall'apertura — e senza
/// questi due campi il brano attaccato in gapless arriverebbe in interfaccia
/// con durata zero e senza correzione di volume.
struct Preparato {
    decodificatore: Decodificatore,
    durata_ms: u64,
    replaygain_db: Option<f32>,
}

/// Un brano che comincerà a sentirsi a un certo fotogramma d'uscita.
struct Segno {
    /// Da quale fotogramma d'uscita in poi si sente questo brano.
    da: u64,
    track_id: i64,
    durata_ms: u64,
    /// Quanto era già stato saltato quando è cominciato (per i salti).
    offset_ms: u64,
    replaygain_db: Option<f32>,
}

struct Contesto {
    comandi: Receiver<Comando>,
    produttore: rtrb::Producer<f32>,
    condiviso: Arc<Condiviso>,
    posizione: Arc<std::sync::Mutex<Posizione>>,
    osservatore: Box<dyn Fn(Evento) + Send>,
    formato: FormatoUscita,
    corrente: Option<Decodificatore>,
    prossimo: Option<Preparato>,
    /// Dove comincia ogni brano, in fotogrammi d'uscita.
    segni: VecDeque<Segno>,
    /// Campioni decodificati che non sono ancora entrati nell'anello.
    resto: VecDeque<f32>,
    blocco: Vec<f32>,
    /// Fotogrammi spinti nell'anello da quando si è azzerato il conteggio.
    spinti: u64,
    /// L'ultimo brano annunciato con [`Evento::Iniziato`].
    annunciato: Option<i64>,
    /// L'evento di fine è già stato mandato.
    fine_dichiarata: bool,
    volume: f32,
    muto: bool,
    replaygain_attivo: bool,
    bersaglio_db: f32,
    replaygain_corrente: Option<f32>,
    /// Da qui le curve dell'equalizzatore raggiungono la callback.
    coefficienti: rtrb::Producer<Coefficienti>,
    /// La curva che sta suonando adesso, in decibel.
    eq_correnti: [f32; BANDE],
    /// Quella verso cui si sta andando.
    eq_bersaglio: [f32; BANDE],
    /// Quanti passi mancano per arrivarci.
    eq_passi: u8,
    /// Una curva che l'anello non ha accettato, da riprovare al giro dopo.
    eq_in_attesa: Option<Coefficienti>,
}

fn filo(mut ctx: Contesto) {
    loop {
        // I comandi in attesa, tutti, prima di lavorare: fra un «pausa» e un
        // «riprendi» arrivati insieme deve vincere il secondo, non il primo.
        loop {
            match ctx.comandi.try_recv() {
                Ok(Comando::Chiudi) | Err(TryRecvError::Disconnected) => return,
                Ok(comando) => ctx.esegui(comando),
                Err(TryRecvError::Empty) => break,
            }
        }
        let lavorato = ctx.riempi();
        ctx.avanza_eq();
        ctx.aggiorna();
        if !lavorato {
            // Niente da decodificare, o anello pieno: quattro millisecondi sono
            // molto meno della riserva e molto più di un giro a vuoto.
            std::thread::sleep(Duration::from_millis(4));
        }
    }
}

impl Contesto {
    fn esegui(&mut self, comando: Comando) {
        match comando {
            Comando::Suona(sorgente) => self.avvia_brano(*sorgente),
            Comando::Prepara(sorgente) => self.prepara(sorgente.map(|s| *s)),
            Comando::Riprendi => {
                self.condiviso.in_pausa.store(false, Ordering::Release);
            }
            Comando::Pausa => {
                self.condiviso.in_pausa.store(true, Ordering::Release);
            }
            Comando::Ferma => self.ferma(),
            Comando::VaiA(ms) => self.vai_a(ms),
            Comando::Volume { volume, muto } => {
                self.volume = volume;
                self.muto = muto;
                self.applica_guadagno();
            }
            Comando::ReplayGain {
                attivo,
                bersaglio_db,
            } => {
                self.replaygain_attivo = attivo;
                self.bersaglio_db = bersaglio_db;
                self.applica_guadagno();
            }
            Comando::Equalizzatore { guadagni, attivo } => {
                // Spegnere non è un salto a piatto: è una corsa verso lo zero,
                // come qualunque altro cambio di curva. `Coefficienti::calcola`
                // riconosce da sola che dieci zeri non sono niente da fare, e
                // da lì in poi la callback smette di filtrare.
                self.eq_bersaglio = if attivo { guadagni } else { [0.0; BANDE] };
                self.eq_passi = PASSI_EQ;
            }
            Comando::Chiudi => {}
        }
    }

    /// Un passo verso la curva voluta, e la sua consegna alla callback.
    ///
    /// Interpola i **decibel** e ricalcola i coefficienti a ogni passo, invece
    /// di interpolare i coefficienti: la strada fra due biquad stabili passa per
    /// biquad che non lo sono, e un filtro instabile per due millisecondi non è
    /// un transitorio — è un fischio.
    fn avanza_eq(&mut self) {
        // Prima quel che era rimasto indietro: l'anello era pieno, e una curva
        // persa qui è un cursore che non arriva mai a destinazione.
        if let Some(rimasta) = self.eq_in_attesa.take()
            && self.coefficienti.push(rimasta).is_err()
        {
            self.eq_in_attesa = Some(rimasta);
            return;
        }
        if self.eq_passi == 0 {
            return;
        }

        self.eq_passi = self.eq_passi.saturating_sub(1);
        avvicina(&mut self.eq_correnti, &self.eq_bersaglio, self.eq_passi);

        let coefficienti = Coefficienti::calcola(&self.eq_correnti, true, self.formato.frequenza);
        if self.coefficienti.push(coefficienti).is_err() {
            self.eq_in_attesa = Some(coefficienti);
        }
    }

    fn applica_guadagno(&self) {
        let g = guadagno(
            self.volume,
            self.muto,
            self.replaygain_corrente,
            self.replaygain_attivo,
            self.bersaglio_db,
        );
        self.condiviso
            .guadagno
            .store(g.to_bits(), Ordering::Relaxed);
    }

    /// Butta via quel che è già in viaggio verso le casse.
    ///
    /// La stretta di mano con la callback: si alza `svuota`, si aspetta che lo
    /// abbassi. Solo allora l'anello è certamente vuoto, e solo allora si può
    /// azzerare il contatore dei fotogrammi senza correre contro di lei — con
    /// l'anello vuoto la callback non conta niente, quindi la finestra è sicura.
    fn scarta_in_volo(&mut self) {
        self.resto.clear();
        self.condiviso.svuota.store(true, Ordering::Release);
        // La callback gira ogni pochi millisecondi. Se dopo un decimo di secondo
        // non ha risposto, il dispositivo è morto e insistere non aiuta.
        for _ in 0..100 {
            if !self.condiviso.svuota.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.condiviso.fotogrammi.store(0, Ordering::Relaxed);
        self.spinti = 0;
        self.segni.clear();
    }

    fn avvia_brano(&mut self, sorgente: Sorgente) {
        let track_id = sorgente.track_id;
        let durata_ms = sorgente.durata_ms;
        let replaygain_db = sorgente.replaygain_db;
        let aperto = Decodificatore::apri(sorgente, self.formato.frequenza, self.formato.canali);
        match aperto {
            Ok(decodificatore) => {
                self.scarta_in_volo();
                self.corrente = Some(decodificatore);
                self.prossimo = None;
                self.annunciato = None;
                self.fine_dichiarata = false;
                self.replaygain_corrente = replaygain_db;
                self.applica_guadagno();
                self.segni.push_back(Segno {
                    da: 0,
                    track_id,
                    durata_ms,
                    offset_ms: 0,
                    replaygain_db,
                });
                self.condiviso.in_pausa.store(false, Ordering::Release);
            }
            Err(err) => {
                // Il brano non si apre: si dice, e ci si ferma. Andare avanti da
                // soli al successivo è una decisione della coda, non del motore.
                (self.osservatore)(Evento::Errore(Box::new(err)));
            }
        }
    }

    fn prepara(&mut self, sorgente: Option<Sorgente>) {
        self.prossimo = match sorgente {
            None => None,
            Some(s) => {
                let durata_ms = s.durata_ms;
                let replaygain_db = s.replaygain_db;
                match Decodificatore::apri(s, self.formato.frequenza, self.formato.canali) {
                    Ok(decodificatore) => Some(Preparato {
                        decodificatore,
                        durata_ms,
                        replaygain_db,
                    }),
                    Err(err) => {
                        // Il prossimo non si apre: si segnala ora, mentre il
                        // corrente suona ancora, invece di scoprirlo nel
                        // silenzio fra i due.
                        (self.osservatore)(Evento::Errore(Box::new(err)));
                        None
                    }
                }
            }
        };
    }

    fn ferma(&mut self) {
        self.condiviso.in_pausa.store(true, Ordering::Release);
        self.scarta_in_volo();
        self.corrente = None;
        self.prossimo = None;
        self.annunciato = None;
        if !self.fine_dichiarata {
            self.fine_dichiarata = true;
            (self.osservatore)(Evento::Fermato);
        }
        self.scrivi_posizione(Posizione::default());
    }

    fn vai_a(&mut self, ms: u64) {
        let Some(decodificatore) = self.corrente.as_mut() else {
            return;
        };
        let track_id = decodificatore.track_id();
        if let Err(err) = decodificatore.cerca(ms) {
            (self.osservatore)(Evento::Errore(Box::new(err)));
            return;
        }
        // Il segno del brano corrente, per non perderne durata e ReplayGain.
        let (durata_ms, replaygain_db) = self
            .segni
            .iter()
            .find(|s| s.track_id == track_id)
            .map_or((0, None), |s| (s.durata_ms, s.replaygain_db));
        self.scarta_in_volo();
        // Dopo un salto, il brano che si sente è ancora quello: annunciarlo di
        // nuovo farebbe ripartire il conteggio dell'ascolto da capo.
        self.segni.push_back(Segno {
            da: 0,
            track_id,
            durata_ms,
            offset_ms: ms,
            replaygain_db,
        });
        self.fine_dichiarata = false;
    }

    /// Riempie l'anello finché c'è posto e c'è roba. `true` se ha fatto qualcosa.
    fn riempi(&mut self) -> bool {
        let mut fatto = false;
        loop {
            while let Some(&campione) = self.resto.front() {
                if self.produttore.push(campione).is_err() {
                    // Anello pieno: si riproverà al giro dopo.
                    return fatto;
                }
                self.resto.pop_front();
                fatto = true;
            }
            if !self.decodifica_un_blocco() {
                return fatto;
            }
            fatto = true;
        }
    }

    /// Un blocco dal decodificatore corrente dentro `resto`. `false` se non c'è
    /// più niente da decodificare.
    fn decodifica_un_blocco(&mut self) -> bool {
        let Some(decodificatore) = self.corrente.as_mut() else {
            return false;
        };
        match decodificatore.prossimo(&mut self.blocco) {
            Ok(true) => {
                self.resto.extend(self.blocco.iter().copied());
                self.spinti = self
                    .spinti
                    .saturating_add(fotogrammi(self.blocco.len(), self.formato.canali));
                true
            }
            Ok(false) => self.passa_al_prossimo(),
            Err(err) => {
                (self.osservatore)(Evento::Errore(Box::new(err)));
                self.passa_al_prossimo()
            }
        }
    }

    /// Il corrente è finito: attacca il preparato, se c'è.
    ///
    /// Il segno si annota **adesso**, al fotogramma dove finisce quel che è già
    /// stato spinto: è lì che il brano nuovo comincerà a sentirsi, e non un
    /// campione prima. È tutto quel che serve perché il gapless sia gapless.
    fn passa_al_prossimo(&mut self) -> bool {
        match self.prossimo.take() {
            Some(preparato) => {
                let track_id = preparato.decodificatore.track_id();
                self.segni.push_back(Segno {
                    da: self.spinti,
                    track_id,
                    durata_ms: preparato.durata_ms,
                    offset_ms: 0,
                    replaygain_db: preparato.replaygain_db,
                });
                self.corrente = Some(preparato.decodificatore);
                true
            }
            None => {
                self.corrente = None;
                false
            }
        }
    }

    /// Traduce i fotogrammi usciti in un brano e in un istante.
    fn aggiorna(&mut self) {
        let suonati = self.condiviso.fotogrammi.load(Ordering::Relaxed);

        // Il segno valido è l'ultimo già raggiunto dal contatore.
        while self.segni.len() > 1 {
            let prossimo_arrivato = self.segni.get(1).is_some_and(|s| s.da <= suonati);
            if prossimo_arrivato {
                self.segni.pop_front();
            } else {
                break;
            }
        }

        let in_pausa = self.condiviso.in_pausa.load(Ordering::Acquire);
        let Some(segno) = self.segni.front() else {
            self.scrivi_posizione(Posizione {
                in_pausa,
                ..Posizione::default()
            });
            self.forse_fine(suonati);
            return;
        };

        let scorsi = suonati.saturating_sub(segno.da);
        let ms = segno
            .offset_ms
            .saturating_add(ms_da_fotogrammi(scorsi, self.formato.frequenza));
        let (track_id, durata_ms, replaygain_db) =
            (segno.track_id, segno.durata_ms, segno.replaygain_db);

        if self.annunciato != Some(track_id) {
            self.annunciato = Some(track_id);
            self.replaygain_corrente = replaygain_db;
            self.applica_guadagno();
            (self.osservatore)(Evento::Iniziato { track_id });
        }

        self.scrivi_posizione(Posizione {
            track_id: Some(track_id),
            ms,
            durata_ms,
            in_pausa,
        });
        self.forse_fine(suonati);
    }

    /// L'ultimo campione è uscito davvero?
    ///
    /// Non basta che il decodificatore sia a secco: fra la fine della
    /// decodifica e la fine del suono ci sono i duecento millisecondi
    /// dell'anello. Dichiarare la fine quando finisce la decodifica taglierebbe
    /// la coda di ogni brano — e con una coda che avanza da sola, la
    /// taglierebbe a ogni brano dell'album.
    fn forse_fine(&mut self, suonati: u64) {
        if self.fine_dichiarata || self.corrente.is_some() || self.prossimo.is_some() {
            return;
        }
        if suonati >= self.spinti && self.resto.is_empty() {
            self.fine_dichiarata = true;
            self.condiviso.in_pausa.store(true, Ordering::Release);
            (self.osservatore)(Evento::Fermato);
        }
    }

    fn scrivi_posizione(&self, nuova: Posizione) {
        match self.posizione.lock() {
            Ok(mut g) => *g = nuova,
            Err(avvelenato) => *avvelenato.into_inner() = nuova,
        }
    }
}

/// Avvicina una curva alla successiva, sapendo quanti passi restano dopo questo.
///
/// Con `rimasti` a zero si atterra esatto invece di avvicinarsi per frazioni:
/// senza, resterebbe per sempre un decimo di decibel di scarto fra quel che
/// l'utente ha chiesto e quel che sente — e chi salva un preset salverebbe una
/// curva leggermente diversa da quella che stava ascoltando.
fn avvicina(correnti: &mut [f32; BANDE], bersaglio: &[f32; BANDE], rimasti: u8) {
    let divisore = f32::from(rimasti) + 1.0;
    for (corrente, voluto) in correnti.iter_mut().zip(bersaglio) {
        *corrente = if rimasti == 0 {
            *voluto
        } else {
            *corrente + (*voluto - *corrente) / divisore
        };
    }
}

/// Quanti fotogrammi ci sono in tanti campioni interlacciati.
#[expect(
    clippy::integer_division,
    reason = "un fotogramma è esattamente `canali` campioni: divisione esatta per \
              costruzione"
)]
fn fotogrammi(campioni: usize, canali: u16) -> u64 {
    let canali = u64::from(canali).max(1);
    u64::try_from(campioni).unwrap_or(0) / canali
}

/// I millisecondi corrispondenti a tanti fotogrammi.
#[expect(
    clippy::integer_division,
    reason = "il cursore si disegna in millisecondi interi: il resto è meno di un \
              millisecondo, cioè meno di un pixel su qualunque barra"
)]
fn ms_da_fotogrammi(fotogrammi: u64, frequenza: u32) -> u64 {
    let frequenza = u64::from(frequenza).max(1);
    fotogrammi.saturating_mul(1000) / frequenza
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_fotogrammi_da_campioni_interlacciati() {
        assert_eq!(fotogrammi(8, 2), 4);
        assert_eq!(fotogrammi(9, 2), 4);
        // Zero canali non deve dividere per zero.
        assert_eq!(fotogrammi(8, 0), 8);
    }

    #[test]
    fn i_millisecondi_dai_fotogrammi() {
        assert_eq!(ms_da_fotogrammi(48_000, 48_000), 1_000);
        assert_eq!(ms_da_fotogrammi(0, 44_100), 0);
        // Frequenza zero non deve dividere per zero.
        assert_eq!(ms_da_fotogrammi(1, 0), 1_000);
    }

    #[test]
    fn una_posizione_nuova_e_vuota() {
        let p = Posizione::default();
        assert_eq!(p.track_id, None);
        assert_eq!(p.ms, 0);
    }

    #[test]
    fn la_curva_arriva_al_bersaglio_e_ci_resta() {
        let mut correnti = [0.0f32; BANDE];
        let bersaglio = [12.0f32; BANDE];
        for rimasti in (0..PASSI_EQ).rev() {
            avvicina(&mut correnti, &bersaglio, rimasti);
        }
        for (banda, valore) in correnti.iter().enumerate() {
            assert!(
                (valore - 12.0).abs() < f32::EPSILON,
                "banda {banda} è a {valore}, non a 12"
            );
        }
        // Un passo in più non la sposta.
        avvicina(&mut correnti, &bersaglio, 0);
        assert!((correnti.first().copied().unwrap_or(0.0) - 12.0).abs() < f32::EPSILON);
    }

    #[test]
    fn i_passi_intermedi_stanno_in_mezzo() {
        // Il punto di tutto l'esercizio: nessun salto. Ogni passo si muove, e
        // nessuno arriva prima del tempo.
        let mut correnti = [0.0f32; BANDE];
        let bersaglio = [12.0f32; BANDE];
        let mut precedente = 0.0f32;
        for rimasti in (1..PASSI_EQ).rev() {
            avvicina(&mut correnti, &bersaglio, rimasti);
            let adesso = correnti.first().copied().unwrap_or(0.0);
            assert!(adesso > precedente, "fermo a {adesso}");
            assert!(adesso < 12.0, "arrivato troppo presto a {adesso}");
            precedente = adesso;
        }
    }

    #[test]
    fn tornare_a_zero_e_una_corsa_come_le_altre() {
        // Spegnere l'equalizzatore passa di qui, non da un salto a piatto.
        let mut correnti = [8.0f32; BANDE];
        let bersaglio = [0.0f32; BANDE];
        avvicina(&mut correnti, &bersaglio, PASSI_EQ - 1);
        let primo = correnti.first().copied().unwrap_or(0.0);
        assert!(primo > 0.0 && primo < 8.0, "salto invece di rampa: {primo}");
    }
}
