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
use crate::{Condiviso, correzione, dissolvi, punto_curva, risali, volume_uscita};

/// In quanti passi si passa da una curva dell'equalizzatore all'altra.
///
/// Cambiare di colpo i coefficienti di dieci sezioni — cosa che succede
/// caricando un preset — produce un transitorio che si sente come uno schiocco.
/// Dieci passi da quattro millisecondi l'uno, che è il sonno di questo filo,
/// fanno una quarantina di millisecondi: impercettibili come ritardo,
/// sufficienti a non sentire il gradino. È lo stesso ragionamento della `RAMPA`
/// del volume in [`crate::uscita`], applicato dove la rampa non poteva arrivare.
const PASSI_EQ: u8 = 10;

/// In quanto tempo un brano entrante si riprende tutta l'ampiezza, quando
/// quello che usciva è finito prima della fine della curva.
///
/// Quaranta millisecondi, gli stessi della rampa del volume in
/// [`crate::uscita`]: non si sentono come un cambio di livello, e non sono un
/// gradino. Il perché di una ripresa breve invece del resto della dissolvenza
/// sta in [`risali`].
const RIPRESA_MS: u64 = 40;

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
    ///
    /// Unico campo che non viene dall'istantanea: lo riempie
    /// [`Motore::posizione`] leggendo il bit condiviso nell'istante in cui
    /// qualcuno chiede dove siamo. Il perché sta in [`Motore::pausa`].
    pub in_pausa: bool,
}

/// Quel che si può chiedere al motore.
enum Comando {
    Suona(Box<Sorgente>),
    Prepara(Option<Box<Sorgente>>),
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
    Dissolvenza {
        ms: u64,
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

        let contesto = Contesto::nuovo(
            ricevi,
            produttore,
            manda_coefficienti,
            Arc::clone(&condiviso),
            Arc::clone(&posizione),
            formato,
            Box::new(osservatore),
        );

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
            let _ = lettore.leggi();
        }
    }

    /// Quante bande fini calcolare accanto alle dieci d'ottava.
    ///
    /// Vedi [`crate::spettro::RISOLUZIONI`]. Un valore fuori scala si stringe
    /// invece di fallire: chi chiama arriva da una preferenza su disco.
    pub fn spettro_dettaglio(&self, quante: u16) {
        if let Ok(mut lettore) = self.spettro.lock() {
            lettore.dettaglio(quante);
        }
    }

    /// Le bande, dentro una funzione che le legge senza copiarle.
    ///
    /// Una chiusura e non un valore restituito: le bande fini sono fino a
    /// milleventiquattro numeri letti trenta volte al secondo, e restituirle
    /// vorrebbe dire allocare un vettore trenta volte al secondo per buttarlo
    /// via subito dopo averlo serializzato. Chi chiama prende quel che gli
    /// serve — di solito la forma che va sul filo verso la finestra — e il
    /// lucchetto si chiude appena finito.
    ///
    /// Restituisce `None` solo se il lucchetto è avvelenato — cioè se qualcuno
    /// è caduto tenendolo, che in questo crate non può succedere ma non si
    /// dichiara impossibile con un `unwrap`.
    pub fn spettro<R>(&self, prendi: impl FnOnce(&crate::spettro::Bande) -> R) -> Option<R> {
        self.spettro
            .lock()
            .ok()
            .map(|mut lettore| prendi(lettore.leggi()))
    }

    /// Come si è aperto il dispositivo.
    #[must_use]
    pub const fn formato(&self) -> FormatoUscita {
        self.formato
    }

    /// Comincia a suonare questo brano, adesso, scartando quel che c'era.
    pub fn suona(&self, sorgente: Sorgente) {
        // Abbassato qui e non all'apertura del file, per la ragione — e per
        // l'ordine — scritti in [`Motore::pausa`].
        self.condiviso.in_pausa.store(false, Ordering::Release);
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
        self.condiviso.in_pausa.store(false, Ordering::Release);
    }

    /// Mette in pausa.
    ///
    /// # Perché non passa dalla coda dei comandi
    ///
    /// Ci passava, e il comando non faceva altro che alzare questo bit: la
    /// pausa vera sta nella callback dell'uscita, che legge `in_pausa` a ogni
    /// blocco e manda silenzio senza consumare l'anello. Il giro per il filo
    /// della decodifica non serviva a mettere in pausa — serviva solo a farsi
    /// aspettare.
    ///
    /// E chi aspettava era la finestra. `pausa()` tornava subito, il filo
    /// alzava il bit qualche millisecondo dopo, e il chiamante che nel
    /// frattempo componeva lo stato da mandare leggeva il valore **di prima**:
    /// il pulsante mostrava il triangolo mentre il brano suonava e le due
    /// stanghette a brano fermo, per sempre, perché dopo una pausa non arriva
    /// nessun evento a correggere il tiro.
    ///
    /// Alzarlo qui rimette anche l'ordine giusto. Pausa, ripresa e avvio
    /// scrivono tutti dal filo di chi preme, quindi valgono nell'ordine in cui
    /// si preme: con l'avvio in coda e la pausa immediata, una pausa premuta
    /// subito dopo un avvio se la sarebbe mangiata l'avvio, arrivando dopo.
    pub fn pausa(&self) {
        self.condiviso.in_pausa.store(true, Ordering::Release);
    }

    /// Smette e dimentica tutto.
    pub fn ferma(&self) {
        // Il bit prima del comando: svuotare l'anello è lavoro del filo, dire
        // che siamo fermi no — vedi [`Motore::pausa`].
        self.condiviso.in_pausa.store(true, Ordering::Release);
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

    /// Quanto si sovrappongono due brani, in millisecondi. Zero: per niente.
    ///
    /// A zero il passaggio resta il gapless di sempre — i campioni del brano
    /// dopo cominciano dove finiscono quelli di prima, senza un istante di
    /// silenzio e senza un istante di sovrapposizione.
    pub fn dissolvenza(&self, ms: u64) {
        self.manda(Comando::Dissolvenza { ms });
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
    ///
    /// Il tempo viene dall'istantanea che scrive il filo della decodifica; la
    /// pausa no, viene dal bit condiviso letto adesso. Chi ha appena premuto
    /// pausa non deve aspettare il prossimo giro di quel filo per sentirselo
    /// dire — vedi [`Motore::pausa`].
    #[must_use]
    pub fn posizione(&self) -> Posizione {
        let istantanea = self
            .posizione
            .lock()
            .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g);
        Posizione {
            in_pausa: self.condiviso.in_pausa.load(Ordering::Acquire),
            ..istantanea
        }
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
    /// La correzione del brano che si sta **decodificando**.
    ///
    /// Non di quello che si sente: da quando il guadagno del brano viaggia coi
    /// campioni invece di stare in uno scalare globale, quel che conta è a chi
    /// appartengono i campioni che stanno uscendo dal decodificatore adesso —
    /// e con una dissolvenza in corso i due non sono lo stesso brano.
    rg_decodifica: Option<f32>,
    /// Quanto dura la sovrapposizione fra due brani. Zero: nessuna.
    dissolvenza_ms: u64,
    /// La durata dichiarata del brano corrente, per sapere quando finisce.
    durata_corrente_ms: u64,
    /// Quanti fotogrammi di sovrapposizione sono già stati mescolati.
    dissolvenza_fatti: u64,
    /// Quanto dura la sovrapposizione **cominciata**, in fotogrammi.
    ///
    /// Congelata quando comincia invece di essere ricalcolata a ogni blocco: la
    /// manopola si può muovere mentre due brani si stanno già sovrapponendo, e
    /// cambiare il denominatore a metà curva sposterebbe il guadagno di colpo —
    /// un gradino proprio nel punto in cui la dissolvenza esiste per non farne.
    /// Il valore nuovo vale dal passaggio dopo, che è quel che il comando
    /// promette a chi lo manda.
    dissolvenza_durata: u64,
    /// Il segno del brano entrante è già stato annotato.
    segno_dissolvenza: bool,
    /// Il brano che sta entrando, per tutta la sovrapposizione.
    ///
    /// # Perché non basta `prossimo`
    ///
    /// Perché a metà sovrapposizione il brano entrante viene annunciato, e chi
    /// sta sopra risponde all'annuncio preparando il brano **ancora dopo** —
    /// cioè scrivendo in `prossimo` mentre la dissolvenza lo sta ancora
    /// leggendo. Finché i due erano la stessa casella, la seconda metà della
    /// curva faceva entrare il brano sbagliato e poi il motore attaccava
    /// quello: chi ascoltava sentiva gracchiare e si ritrovava un brano più
    /// avanti nella coda. Qui il brano che entra è al riparo, e `prossimo`
    /// torna a essere quel che dice di essere.
    entrante: Option<Preparato>,
    /// Il blocco appena uscito dal decodificatore del brano che entra.
    blocco_entrante: Vec<f32>,
    /// I campioni dell'entrante decodificati e non ancora mescolati.
    ///
    /// Due decodificatori consegnano blocchi di lunghezza diversa — un
    /// pacchetto MP3 sono 1152 fotogrammi, uno FLAC anche 4096 — e mescolarne
    /// uno contro l'altro vorrebbe dire buttare via quel che avanza o riempire
    /// di silenzio quel che manca. Tre quarti del brano entrante nel cestino a
    /// ogni blocco: è così che una dissolvenza diventa un raspare. Da qui
    /// invece la sovrapposizione prende i campioni che le servono, esatti, e
    /// tiene il resto per il blocco dopo.
    coda_entrante: Vec<f32>,
    /// Quanto dura la ripresa del brano entrante. Zero: nessuna in corso.
    ///
    /// Si accende quando il brano uscente finisce **prima** della fine della
    /// curva — succede quando la durata dichiarata dal database è più lunga
    /// dell'audio vero — e il brano che stava entrando resta solo a mezza
    /// ampiezza. Vedi [`risali`].
    entrata_durata: u64,
    /// A che punto è quella ripresa, in fotogrammi.
    entrata_fatti: u64,
    /// Da quale guadagno è ripartita: quello che la curva gli aveva dato.
    entrata_da: f32,
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
    /// Tutte le manopole al valore di riposo, e niente di aperto.
    ///
    /// Fuori da [`Motore::avvia`] perché è l'unico modo di provare questo filo
    /// senza una scheda audio: qui dentro non si apre nessun dispositivo, e le
    /// prove in fondo al file costruiscono un contesto vero — decodificatori
    /// veri, dissolvenza vera — con un anello loro. Il valore di riposo di
    /// ventisette campi scritto in due posti diversi sarebbe invece il modo
    /// sicuro di provare qualcosa che non è quel che gira davvero.
    fn nuovo(
        comandi: Receiver<Comando>,
        produttore: rtrb::Producer<f32>,
        coefficienti: rtrb::Producer<Coefficienti>,
        condiviso: Arc<Condiviso>,
        posizione: Arc<std::sync::Mutex<Posizione>>,
        formato: FormatoUscita,
        osservatore: Box<dyn Fn(Evento) + Send>,
    ) -> Self {
        Self {
            comandi,
            produttore,
            condiviso,
            posizione,
            osservatore,
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
            rg_decodifica: None,
            dissolvenza_ms: 0,
            durata_corrente_ms: 0,
            dissolvenza_fatti: 0,
            dissolvenza_durata: 0,
            segno_dissolvenza: false,
            entrante: None,
            blocco_entrante: Vec::new(),
            coda_entrante: Vec::new(),
            entrata_durata: 0,
            entrata_fatti: 0,
            entrata_da: 1.0,
            coefficienti,
            eq_correnti: [0.0; BANDE],
            eq_bersaglio: [0.0; BANDE],
            eq_passi: 0,
            eq_in_attesa: None,
        }
    }

    fn esegui(&mut self, comando: Comando) {
        match comando {
            Comando::Suona(sorgente) => self.avvia_brano(*sorgente),
            Comando::Prepara(sorgente) => self.prepara(sorgente.map(|s| *s)),
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
                // Il guadagno d'uscita non dipende più dal ReplayGain, ma la
                // chiamata resta: `volume_uscita` va ripubblicato comunque, e
                // costa un `store` atomico.
                self.applica_guadagno();
            }
            Comando::Dissolvenza { ms } => {
                self.dissolvenza_ms = ms;
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

    /// Pubblica alla callback il guadagno d'uscita: volume e silenziamento.
    ///
    /// La correzione ReplayGain **non** passa più di qui. Sta nei campioni, e
    /// li raggiunge in [`Contesto::decodifica_un_blocco`]: durante una
    /// dissolvenza ci sono due brani con due correzioni diverse, e un unico
    /// numero atomico non può dirle entrambe.
    ///
    /// Il prezzo, dichiarato: cambiare il livello di normalizzazione ora si
    /// sente dopo la riserva dell'anello — qualche secondo — invece che
    /// all'istante, perché i campioni già decodificati portano il guadagno di
    /// prima. È una manopola che si tocca dalle impostazioni una volta ogni
    /// tanto, e il cambio arriva senza un salto.
    fn applica_guadagno(&self) {
        let g = volume_uscita(self.volume, self.muto);
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
                self.rg_decodifica = replaygain_db;
                self.durata_corrente_ms = durata_ms;
                self.azzera_dissolvenza();
                self.applica_guadagno();
                self.segni.push_back(Segno {
                    da: 0,
                    track_id,
                    durata_ms,
                    offset_ms: 0,
                    replaygain_db,
                });
            }
            Err(err) => {
                // Il brano non si apre: si dice, e ci si ferma. Andare avanti da
                // soli al successivo è una decisione della coda, non del motore.
                //
                // Il bit torna su qui: l'aveva abbassato [`Motore::suona`] al
                // clic, e lasciarlo giù vorrebbe dire una finestra che dice «in
                // riproduzione» per un file che non si è mai aperto.
                self.condiviso.in_pausa.store(true, Ordering::Release);
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
        self.scarta_in_volo();
        self.corrente = None;
        self.prossimo = None;
        self.azzera_dissolvenza();
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
        // Una sovrapposizione in corso non sopravvive a un salto: i campioni
        // già mescolati sono appena stati buttati, e il brano che stava
        // entrando torna a essere quello che verrà **dopo** — dal suo inizio,
        // non da dove la curva l'aveva portato. Senza questo riavvolgimento
        // attaccherebbe a metà, e senza il ritorno in `prossimo` non
        // attaccherebbe affatto: al suo posto suonerebbe quello ancora dopo,
        // che chi sta sopra ha preparato all'annuncio di metà curva.
        if let Some(mut entrante) = self.entrante.take() {
            if entrante.decodificatore.cerca(0).is_ok() {
                self.prossimo = Some(entrante);
            }
            // Se non si riavvolge si lascia perdere: un brano che non sa
            // tornare al proprio inizio non è materiale per una dissolvenza, e
            // in `prossimo` resta quel che c'era.
            //
            // L'annuncio di metà curva invece era già partito, e per chi sta
            // sopra il brano corrente è quello che stava entrando. Riscriverlo
            // qui evita che il ritorno al brano di prima sembri un brano nuovo,
            // cioè che un salto faccia avanzare la coda.
            self.annunciato = Some(track_id);
        }
        self.azzera_dissolvenza();
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
                // La correzione del brano si applica ai **suoi** campioni,
                // adesso, invece che a tutto quel che esce dall'anello. È
                // l'unico modo perché due brani sovrapposti possano avere due
                // ReplayGain diversi, e per la catena non cambia niente: il
                // filtro dell'equalizzatore è lineare, quindi moltiplicare
                // prima o dopo di lui dà lo stesso suono.
                let g = correzione(
                    self.rg_decodifica,
                    self.replaygain_attivo,
                    self.bersaglio_db,
                );
                if (g - 1.0).abs() > f32::EPSILON {
                    for campione in &mut self.blocco {
                        *campione *= g;
                    }
                }
                // Prima la salita del brano che è rimasto solo a metà curva,
                // poi l'eventuale curva nuova: sono due cose diverse che
                // possono capitare allo stesso blocco — un brano brevissimo fa
                // in tempo a finire di entrare e a cominciare a uscire — e in
                // quel caso i due guadagni si moltiplicano, che è esattamente
                // quel che deve succedere.
                self.forse_risali();
                self.forse_dissolvi();
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

    /// Quanti fotogrammi dura la sovrapposizione, se ci sarà.
    ///
    /// Zero quando la dissolvenza è spenta, quando non c'è un brano dopo, o
    /// quando la durata del corrente non si sa: senza sapere dove finisce non
    /// c'è modo di sapere quando cominciare, e cominciare a caso vorrebbe dire
    /// tagliare un brano nel mezzo.
    /// Una volta cominciata vale quella con cui è cominciata: vedi
    /// [`Contesto::dissolvenza_durata`].
    fn fotogrammi_dissolvenza(&self) -> u64 {
        if self.entrante.is_some() {
            return self.dissolvenza_durata;
        }
        if self.dissolvenza_ms == 0 || self.prossimo.is_none() || self.durata_corrente_ms == 0 {
            return 0;
        }
        fotogrammi_da_ms(self.dissolvenza_ms, self.formato.frequenza)
    }

    /// La sovrapposizione è finita: resta solo un brano.
    ///
    /// Non tocca la ripresa: quando la curva si è interrotta a metà, la salita
    /// del brano rimasto solo comincia proprio qui e deve sopravvivere a questa
    /// chiamata. Vedi [`Contesto::forse_riprendi`].
    fn chiudi_sovrapposizione(&mut self) {
        self.entrante = None;
        self.coda_entrante.clear();
        self.dissolvenza_fatti = 0;
        self.dissolvenza_durata = 0;
        self.segno_dissolvenza = false;
    }

    /// Butta via una sovrapposizione, con tutto quel che ci stava dentro.
    ///
    /// Questa invece spegne anche la ripresa: la chiamano un brano nuovo, uno
    /// stop e un salto, cioè i tre casi in cui quel che stava succedendo non
    /// deve lasciare tracce nei campioni che verranno.
    fn azzera_dissolvenza(&mut self) {
        self.chiudi_sovrapposizione();
        self.entrata_durata = 0;
        self.entrata_fatti = 0;
        self.entrata_da = 1.0;
    }

    /// Il brano uscente è finito prima della fine della curva: prepara la
    /// salita che porterà quello entrante a piena ampiezza.
    ///
    /// Il guadagno di partenza è quello che la curva gli aveva dato all'ultimo
    /// blocco mescolato, letto dalla stessa [`punto_curva`] che l'ha
    /// applicato: è così che la ripresa attacca senza gradino. Se invece la
    /// curva era finita — il caso normale — il brano è già a piena ampiezza e
    /// non c'è niente da riprendere.
    fn forse_riprendi(&mut self) {
        let restanti = self
            .dissolvenza_durata
            .saturating_sub(self.dissolvenza_fatti);
        if restanti == 0 {
            return;
        }
        let (_, dentro) = punto_curva(self.dissolvenza_fatti, self.dissolvenza_durata);
        self.entrata_da = dentro;
        self.entrata_fatti = 0;
        // Mai più lunga di quel che restava della curva: se mancava un decimo
        // di quaranta millisecondi, il brano era già quasi a piena ampiezza e
        // allungare la salita vorrebbe dire inventare un livello che non c'era.
        self.entrata_durata = fotogrammi_da_ms(RIPRESA_MS, self.formato.frequenza).min(restanti);
    }

    /// Un pezzo di quella salita, sul blocco appena decodificato.
    fn forse_risali(&mut self) {
        if self.entrata_durata == 0 {
            return;
        }
        let canali = self.formato.canali;
        risali(
            &mut self.blocco,
            self.entrata_da,
            self.entrata_fatti,
            self.entrata_durata,
            canali,
        );
        self.entrata_fatti = self
            .entrata_fatti
            .saturating_add(fotogrammi(self.blocco.len(), canali));
        if self.entrata_fatti >= self.entrata_durata {
            self.entrata_durata = 0;
        }
    }

    /// Tiene nella coda dell'entrante almeno tanti campioni, se ne ha ancora.
    ///
    /// Decodifica quanto serve, e non un pacchetto per blocco: le due lunghezze
    /// non hanno nessuna ragione di coincidere — dipendono dal codec di due file
    /// diversi — e trattarle come se coincidessero vuol dire perdere o
    /// inventare campioni a ogni giro. Vedi [`Contesto::coda_entrante`].
    fn riempi_entrante(&mut self, quanti: usize) {
        let attivo = self.replaygain_attivo;
        let bersaglio = self.bersaglio_db;
        let Some(entrante) = self.entrante.as_mut() else {
            return;
        };
        // Il brano che entra porta la **sua** correzione, non quella di quello
        // che esce.
        let g = correzione(entrante.replaygain_db, attivo, bersaglio);
        while self.coda_entrante.len() < quanti {
            // Un `let ... else` e non un `match`: gli altri due casi — il brano
            // entrante finito, o illeggibile — vogliono la stessa cosa, cioè
            // che non succeda niente. Una dissolvenza mancata è un gapless, che
            // è il comportamento di prima; non è un guasto da segnalare a
            // nessuno.
            let Ok(true) = entrante.decodificatore.prossimo(&mut self.blocco_entrante) else {
                return;
            };
            if (g - 1.0).abs() > f32::EPSILON {
                for campione in &mut self.blocco_entrante {
                    *campione *= g;
                }
            }
            self.coda_entrante
                .extend(self.blocco_entrante.iter().copied());
        }
    }

    /// Mescola il brano che entra dentro il blocco di quello che esce.
    ///
    /// # Perché qui e non nella callback
    ///
    /// Perché l'anello fra i due fili è un flusso piatto di `f32` **senza
    /// confini di traccia**: chi legge di là non sa dove finisce un brano e
    /// comincia l'altro, e non potrebbe saperlo senza un secondo anello e un
    /// secondo decodificatore dentro il percorso realtime — dove non si può né
    /// allocare né prendere un lucchetto. Di qua invece i due decodificatori ci
    /// sono già entrambi: `prossimo` è aperto da quando il brano corrente è
    /// cominciato, ed è quel che rende il gapless gapless.
    fn forse_dissolvi(&mut self) {
        let durata = self.fotogrammi_dissolvenza();
        if durata == 0 {
            return;
        }
        let Some(corrente) = self.corrente.as_ref() else {
            return;
        };
        // Quanto manca alla fine, secondo quel che il decodificatore ha già
        // consegnato. Una volta cominciata, la dissolvenza va avanti anche se
        // il conto dicesse il contrario: fermarla a metà lascerebbe il brano
        // entrante a mezza ampiezza.
        let totali = fotogrammi_da_ms(self.durata_corrente_ms, self.formato.frequenza);
        let mancano = totali.saturating_sub(corrente.consegnati());
        if mancano > durata && self.dissolvenza_fatti == 0 {
            return;
        }

        // La sovrapposizione comincia: il brano preparato lascia la casella del
        // «prossimo» e passa in quella dell'«entrante», dove nessuno lo
        // sostituirà. Da qui in poi `prossimo` è libero di ricevere il brano
        // ancora dopo — cosa che succede fra pochi millisecondi, appena
        // l'annuncio di metà curva arriva a chi sta sopra. Vedi
        // [`Contesto::entrante`].
        if self.entrante.is_none() {
            let Some(preparato) = self.prossimo.take() else {
                return;
            };
            self.entrante = Some(preparato);
            self.coda_entrante.clear();
            self.dissolvenza_durata = durata;
        }

        // Il segno del brano entrante si annota **a metà** della
        // sovrapposizione, e nel futuro: `Segno.da` è in fotogrammi d'uscita, e
        // `aggiorna` lo raccoglie quando il contatore lo raggiunge. Metà e non
        // l'inizio perché è lì che il brano nuovo diventa quello che si sta
        // ascoltando: prima è un sottofondo sotto quello vecchio, e annunciarlo
        // allora vorrebbe dire una finestra che cambia titolo mentre si sente
        // ancora l'altro — e uno scrobble attribuito al brano sbagliato.
        if !self.segno_dissolvenza {
            #[expect(
                clippy::integer_division,
                reason = "metà dissolvenza: mezzo fotogramma non sposta il \
                          punto in cui il brano nuovo prende il sopravvento"
            )]
            let meta = durata / 2;
            if let Some(preparato) = self.entrante.as_ref() {
                self.segni.push_back(Segno {
                    da: self.spinti.saturating_add(meta),
                    track_id: preparato.decodificatore.track_id(),
                    durata_ms: preparato.durata_ms,
                    offset_ms: 0,
                    replaygain_db: preparato.replaygain_db,
                });
            }
            self.segno_dissolvenza = true;
            self.fine_dichiarata = false;
        }

        // Tanti campioni quanti ne mescola questo blocco, né uno di meno né uno
        // buttato: quel che il decodificatore dell'entrante consegna in più
        // resta in coda per il blocco dopo.
        self.riempi_entrante(self.blocco.len());

        let canali = self.formato.canali;
        let fatti = self.dissolvenza_fatti;
        dissolvi(&mut self.blocco, &self.coda_entrante, fatti, durata, canali);
        // `dissolvi` si ferma dove finisce il blocco uscente: consumati sono
        // quelli che ci sono entrati davvero. Se l'entrante ne aveva di meno —
        // il suo ultimo blocco, o un brano più corto della sovrapposizione — la
        // coda si svuota e quel che manca è silenzio, cioè un gapless.
        let consumati = self.coda_entrante.len().min(self.blocco.len());
        self.coda_entrante.drain(..consumati);
        // La curva avanza col brano che **esce**: è lui a dettare il tempo
        // d'uscita, ed è la sua fine il punto in cui la sovrapposizione deve
        // essere finita.
        self.dissolvenza_fatti = fatti.saturating_add(fotogrammi(self.blocco.len(), canali));
    }

    /// Il corrente è finito: attacca il preparato, se c'è.
    ///
    /// Il segno si annota **adesso**, al fotogramma dove finisce quel che è già
    /// stato spinto: è lì che il brano nuovo comincerà a sentirsi, e non un
    /// campione prima. È tutto quel che serve perché il gapless sia gapless.
    fn passa_al_prossimo(&mut self) -> bool {
        // Chi prende il posto è l'entrante, se una sovrapposizione era in
        // corso: è lui che si stava già sentendo, ed è già avanti di tutta la
        // dissolvenza. In quel caso `prossimo` è il brano ancora dopo e deve
        // restare dov'è — prenderlo qui vorrebbe dire saltarne uno.
        let dissolveva = self.entrante.is_some();
        match self.entrante.take().or_else(|| self.prossimo.take()) {
            Some(preparato) => {
                let track_id = preparato.decodificatore.track_id();
                // Con una dissolvenza in corso il segno è già stato annotato a
                // metà sovrapposizione, e rimetterlo qui vorrebbe dire lo
                // stesso brano che comincia due volte.
                if !self.segno_dissolvenza {
                    self.segni.push_back(Segno {
                        da: self.spinti,
                        track_id,
                        durata_ms: preparato.durata_ms,
                        offset_ms: 0,
                        replaygain_db: preparato.replaygain_db,
                    });
                }
                if dissolveva {
                    // Se la curva non era finita, il brano che entrava è
                    // rimasto solo a mezza ampiezza: da qui comincia la sua
                    // salita.
                    self.forse_riprendi();
                    // I campioni decodificati per la sovrapposizione e non
                    // ancora mescolati sono il seguito esatto di quel che è
                    // appena uscito: ripartire dal decodificatore li salterebbe,
                    // e il brano attaccherebbe qualche decina di millisecondi
                    // più avanti di dove si era interrotto. Passano per
                    // `blocco` — che il decodificatore ha appena svuotato — così
                    // la salita gliela applica la stessa riga di codice che la
                    // applicherà a tutti i blocchi dopo di loro.
                    self.blocco.clear();
                    self.blocco.append(&mut self.coda_entrante);
                    self.forse_risali();
                    self.resto.extend(self.blocco.iter().copied());
                    self.spinti = self
                        .spinti
                        .saturating_add(fotogrammi(self.blocco.len(), self.formato.canali));
                }
                self.rg_decodifica = preparato.replaygain_db;
                self.durata_corrente_ms = preparato.durata_ms;
                self.chiudi_sovrapposizione();
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

        let Some(segno) = self.segni.front() else {
            self.scrivi_posizione(Posizione::default());
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
            // Nessun `applica_guadagno` qui: la correzione del brano è già nei
            // campioni da quando sono stati decodificati. Prima andava
            // riapplicata **adesso** — al fotogramma in cui il brano nuovo
            // cominciava a sentirsi — ed era l'unica ragione per cui `Segno`
            // portava con sé un `replaygain_db`.
            (self.osservatore)(Evento::Iniziato { track_id });
        }
        let _ = replaygain_db;

        // `in_pausa` resta al valore di riposo: lo riempie [`Motore::posizione`]
        // quando qualcuno legge, dal bit condiviso.
        self.scrivi_posizione(Posizione {
            track_id: Some(track_id),
            ms,
            durata_ms,
            ..Posizione::default()
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

/// I fotogrammi che stanno in un tempo, alla frequenza d'uscita.
#[expect(
    clippy::integer_division,
    reason = "il resto è meno di un millisecondo di campioni: sulla soglia \
              della dissolvenza vale un fotogramma su quarantottomila"
)]
fn fotogrammi_da_ms(ms: u64, frequenza: u32) -> u64 {
    ms.saturating_mul(u64::from(frequenza)) / 1000
}

/// I millisecondi corrispondenti a tanti fotogrammi.
#[expect(
    clippy::integer_division,
    reason = "il cursore si disegna in millisecondi interi: il resto è meno \
              di un millisecondo, cioè meno di un pixel su qualunque barra"
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

    // ── il passaggio fra due brani, con due decodificatori veri ─────────────
    //
    // Queste prove costruiscono un `Contesto` come quello che gira davvero —
    // stessi decodificatori, stessa dissolvenza, stesso anello — e gli danno
    // due WAV fatti a mano invece di due file. Manca solo il dispositivo, che
    // qui non serve: quel che si vuole guardare è la sequenza di campioni che
    // *sarebbe* uscita, ed è esattamente quel che finisce nell'anello.

    /// Byte in memoria che si comportano come un file.
    ///
    /// Il motore vuole un [`crate::decodifica::Flusso`] e non un percorso: è la
    /// stessa giuntura che su Android porta i byte dallo Storage Access
    /// Framework, e qui porta un WAV costruito a mano senza toccare il disco.
    struct Byte(std::io::Cursor<Vec<u8>>);

    impl std::io::Read for Byte {
        fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.0, dove)
        }
    }

    impl std::io::Seek for Byte {
        fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
            std::io::Seek::seek(&mut self.0, da)
        }
    }

    impl crate::decodifica::Flusso for Byte {
        fn lunghezza(&self) -> Option<u64> {
            u64::try_from(self.0.get_ref().len()).ok()
        }
    }

    /// Un WAV mono a 16 bit con i campioni dati.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "i campioni di prova stanno fra -1 e 1: per costruzione il \
                  prodotto entra in un i16"
    )]
    fn wav(frequenza: u32, campioni: &[f32]) -> Vec<u8> {
        let dati = u32::try_from(campioni.len() * 2).unwrap_or(0);
        let mut byte = Vec::with_capacity(44 + campioni.len() * 2);
        byte.extend_from_slice(b"RIFF");
        byte.extend_from_slice(&(36 + dati).to_le_bytes());
        byte.extend_from_slice(b"WAVEfmt ");
        byte.extend_from_slice(&16u32.to_le_bytes());
        byte.extend_from_slice(&1u16.to_le_bytes()); // PCM
        byte.extend_from_slice(&1u16.to_le_bytes()); // un canale
        byte.extend_from_slice(&frequenza.to_le_bytes());
        byte.extend_from_slice(&(frequenza * 2).to_le_bytes());
        byte.extend_from_slice(&2u16.to_le_bytes()); // allineamento
        byte.extend_from_slice(&16u16.to_le_bytes()); // bit per campione
        byte.extend_from_slice(b"data");
        byte.extend_from_slice(&dati.to_le_bytes());
        for campione in campioni {
            byte.extend_from_slice(&((campione * 32_767.0) as i16).to_le_bytes());
        }
        byte
    }

    /// Un brano di un secondo, tutto allo stesso livello: si riconosce a
    /// orecchio nell'elenco dei campioni.
    fn brano(track_id: i64, frequenza: u32, livello: f32) -> Sorgente {
        brano_da(track_id, frequenza, livello, 1_000)
    }

    /// Come sopra, ma `veri_ms` dice quanto il file **contiene**: la durata
    /// dichiarata resta un secondo. I due valori diversi sono il caso di un
    /// file che mente sulla propria durata, che è quel che il database si
    /// ritrova per un MP3 a bitrate variabile senza intestazione Xing.
    fn brano_da(track_id: i64, frequenza: u32, livello: f32, veri_ms: u64) -> Sorgente {
        let quanti = fotogrammi_da_ms(veri_ms, frequenza);
        let campioni = vec![livello; usize::try_from(quanti).unwrap_or(48_000)];
        Sorgente {
            track_id,
            media: Box::new(Byte(std::io::Cursor::new(wav(frequenza, &campioni)))),
            estensione: Some("wav".to_owned()),
            durata_ms: 1_000,
            replaygain_db: None,
        }
    }

    /// Il filo della decodifica senza il filo e senza il dispositivo.
    struct Banco {
        ctx: Contesto,
        consumatore: rtrb::Consumer<f32>,
        /// Da tenere vivi: il canale dei comandi e l'anello delle curve non
        /// vengono usati, ma se cadessero il contesto parlerebbe con dei morti.
        _manda: Sender<Comando>,
        _curve: rtrb::Consumer<Coefficienti>,
    }

    fn banco(frequenza: u32) -> Banco {
        let (manda, ricevi) = std::sync::mpsc::channel();
        let (produttore, consumatore) = rtrb::RingBuffer::<f32>::new(16_384);
        let (curve, prese) = rtrb::RingBuffer::<Coefficienti>::new(32);
        let ctx = Contesto::nuovo(
            ricevi,
            produttore,
            curve,
            Condiviso::nuovo(),
            Arc::new(std::sync::Mutex::new(Posizione::default())),
            FormatoUscita {
                frequenza,
                canali: 1,
            },
            Box::new(|_| {}),
        );
        Banco {
            ctx,
            consumatore,
            _manda: manda,
            _curve: prese,
        }
    }

    /// Quanti campioni stanno a un livello, entro un errore che copre la
    /// quantizzazione a 16 bit e il ricampionatore.
    fn quanti_a(campioni: &[f32], livello: f32) -> usize {
        campioni
            .iter()
            .filter(|c| (**c - livello).abs() < 0.01)
            .count()
    }

    /// Fa girare il contesto fino alla fine di tutto, raccogliendo quel che
    /// sarebbe uscito dalle casse. `dopo_l_inizio` è quel che chi sta sopra fa
    /// quando il motore comincia a sovrapporre due brani.
    fn suona_tutto(banco: &mut Banco, mut dopo_l_inizio: impl FnMut(&mut Contesto)) -> Vec<f32> {
        let mut fuori = Vec::new();
        let mut avvisato = false;
        // Un tetto ai giri: una prova che non finisce è una prova che blocca
        // l'intera suite, e tre brani da un secondo non ne chiedono nemmeno un
        // decimo.
        for _ in 0..100_000 {
            let lavorato = banco.ctx.riempi();
            while let Ok(campione) = banco.consumatore.pop() {
                fuori.push(campione);
            }
            if !avvisato && banco.ctx.entrante.is_some() {
                dopo_l_inizio(&mut banco.ctx);
                avvisato = true;
            }
            if !lavorato && banco.ctx.corrente.is_none() {
                break;
            }
        }
        fuori
    }

    #[test]
    fn la_dissolvenza_manda_avanti_il_brano_dopo_non_quello_dopo_ancora() {
        // Il guasto che questa prova tiene chiuso: a metà sovrapposizione il
        // motore annuncia il brano che entra, chi sta sopra risponde preparando
        // il brano **ancora dopo**, e finché «preparato» ed «entrante» erano la
        // stessa casella quella preparazione scippava il brano a metà curva.
        // Chi ascoltava sentiva gracchiare e si ritrovava un brano più avanti.
        let mut banco = banco(48_000);
        banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
        banco
            .ctx
            .esegui(Comando::Suona(Box::new(brano(1, 48_000, 0.8))));
        banco
            .ctx
            .esegui(Comando::Prepara(Some(Box::new(brano(2, 24_000, 0.4)))));

        let fuori = suona_tutto(&mut banco, |ctx| {
            ctx.esegui(Comando::Prepara(Some(Box::new(brano(3, 48_000, 0.1)))));
        });

        // Il secondo brano deve sentirsi **da solo**, cioè al suo livello: fra
        // la fine della prima dissolvenza e l'inizio della seconda c'è un
        // quinto di secondo in cui non c'è nient'altro. Se al suo posto fosse
        // entrato il terzo, di campioni a 0,4 non ce ne sarebbe nessuno.
        let secondo = quanti_a(&fuori, 0.4);
        assert!(
            secondo > 5_000,
            "il brano che entra non si sente mai da solo: {secondo} campioni a 0,4"
        );
        // E il terzo deve sentirsi dopo, non al posto suo.
        assert!(quanti_a(&fuori, 0.1) > 5_000, "il terzo brano non arriva");
    }

    #[test]
    fn il_brano_che_entra_non_perde_campioni_per_strada() {
        // L'altro guasto, quello che si sentiva come un raspare: i due
        // decodificatori consegnano blocchi di lunghezza diversa — qui 1152
        // fotogrammi contro 2048, perché il secondo brano va ricampionato — e
        // mescolarne uno contro l'altro buttava via tutto quel che avanzava.
        // Il brano entrante ci arrivava in fondo quasi al doppio della
        // velocità, e la coda finiva prima del dovuto: la durata totale è il
        // modo più semplice di accorgersene.
        let mut banco = banco(48_000);
        banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
        banco
            .ctx
            .esegui(Comando::Suona(Box::new(brano(1, 48_000, 0.8))));
        banco
            .ctx
            .esegui(Comando::Prepara(Some(Box::new(brano(2, 24_000, 0.4)))));

        let fuori = suona_tutto(&mut banco, |_| {});

        // Due brani da un secondo che si sovrappongono per quattro decimi: un
        // secondo e sei decimi, non uno e due.
        let atteso = 48_000 + 48_000 - 19_200;
        let scarto = i64::try_from(fuori.len()).unwrap_or(0) - atteso;
        assert!(
            scarto.abs() < 2_400,
            "durata sbagliata di {scarto} fotogrammi: {} invece di {atteso}",
            fuori.len()
        );
    }

    #[test]
    fn un_brano_che_finisce_prima_della_curva_non_fa_saltare_quello_dopo() {
        // La sovrapposizione comincia quando alla fine manca quanto dura la
        // dissolvenza, e «quanto manca» si sa dalla durata dichiarata. Qui il
        // primo brano ne dichiara mille millisecondi e ne contiene ottocento:
        // il suo decodificatore finisce a metà curva, e quello entrante resta
        // solo a mezza ampiezza. Senza la ripresa salterebbe di colpo a tutta.
        let mut banco = banco(48_000);
        banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
        banco
            .ctx
            .esegui(Comando::Suona(Box::new(brano_da(1, 48_000, 0.8, 800))));
        banco
            .ctx
            .esegui(Comando::Prepara(Some(Box::new(brano(2, 48_000, 0.4)))));

        let fuori = suona_tutto(&mut banco, |_| {});

        // Finché i due si sovrappongono la somma sta sopra 0,8; appena il primo
        // finisce resta solo il secondo, che a piena ampiezza fa 0,4.
        let taglio = fuori
            .iter()
            .position(|c| *c < 0.5)
            .expect("il primo brano non finisce mai");
        let ripresa = fuori.get(taglio).copied().unwrap_or(0.0);
        assert!(
            ripresa < 0.36,
            "il brano entrante salta a piena ampiezza: {ripresa}"
        );
        // E a piena ampiezza ci arriva, in una quarantina di millisecondi.
        let dopo = fuori.get(taglio + 2_400).copied().unwrap_or(0.0);
        assert!(
            (dopo - 0.4).abs() < 0.01,
            "la ripresa non arriva a piena ampiezza: {dopo}"
        );
        // Senza gradini per strada, che è tutto il punto.
        let salto = fuori.get(taglio..taglio + 2_400).map_or(1.0, |pezzo| {
            pezzo
                .windows(2)
                .map(|due| (due[1] - due[0]).abs())
                .fold(0.0_f32, f32::max)
        });
        assert!(salto < 0.005, "gradino di {salto} dentro la ripresa");
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
