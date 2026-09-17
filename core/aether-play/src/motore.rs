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
//! Fra il decodificatore e l'orecchio però non c'è un solo ritardo: ce ne sono
//! **tre**, e distinguerli è tutto il mestiere di questo file.
//!
//! 1. **L'anello nostro**, fra il filo della decodifica e la callback. Duecento
//!    millisecondi garantiti, qualche secondo su un'uscita normale — vedi
//!    [`RISERVA_MS`]. Questo lo sappiamo esattamente, perché i fotogrammi che ne
//!    escono li conta la callback: è il contatore
//!    `Condiviso::fotogrammi`.
//! 2. **Il buffer del dispositivo**, fra la callback e il convertitore. La
//!    callback consegna i campioni *al driver*, non alle casse, e quel che
//!    conta — i `suonati` — sono fotogrammi **entrati nel buffer del
//!    dispositivo**. Fino a ieri questo file scriveva che erano «i fotogrammi
//!    che la callback ha davvero consegnato al dispositivo», e la frase era
//!    letteralmente giusta e praticamente falsa: consegnati al dispositivo non
//!    vuol dire usciti dalle casse. Questo pezzo si **misura**:
//!    `crate::uscita::annota_latenza` lo legge da `cpal` a ogni blocco e lo
//!    scrive in `Condiviso::latenza_fotogrammi`.
//! 3. **La catena d'uscita** — mixer di sistema, driver, DAC, e su un'uscita
//!    senza fili la radio. Questo non si misura da nessuna parte: `cpal` non lo
//!    vede, e su Bluetooth vale più degli altri due insieme. Si **dichiara a
//!    mano**, con la preferenza `audio.latenza_ms`.
//!
//! Da qui i due nomi che [`Contesto::aggiorna`] usa e non confonde: i
//! **`suonati`** sono i fotogrammi contati dalla callback (1), gli **`uditi`**
//! sono i `suonati` meno la somma di (2) e (3). Gli `uditi` governano quel che
//! si racconta — quale brano sta suonando e a che punto è — perché è quel che
//! l'orecchio sta ricevendo adesso. I `suonati` grezzi governano una cosa sola,
//! la fine del brano, per la ragione scritta in [`Contesto::forse_fine`].
//!
//! La traduzione da fotogrammi a brano e istante passa dai **segni**: ogni volta
//! che i campioni di un brano nuovo cominciano a entrare nell'anello si annota a
//! quale fotogramma d'uscita cominceranno, e da quale punto del brano. Quando il
//! contatore raggiunge quel numero, quel brano sta suonando — non un istante
//! prima.
//!
//! È lo stesso meccanismo che rende corretto il gapless: fra due brani attaccati
//! non c'è nessun evento, nessuna riapertura, nessuna pausa. C'è un segno.

use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode, ErrorCodeKind};

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

/// Il codice di [`Condiviso::causa_perdita`] per il filo di decodifica caduto.
///
/// Uno e due li assegna la callback dell'uscita ai due `StreamError` di cpal
/// (vedi `crate::uscita::codice_errore`); il tre è di qua, e vale «il motore c'è
/// ancora, il dispositivo pure, ma non arriva più un campione». Un numero e non
/// un `enum` perché deve stare in un `AtomicU32` che la callback audio può
/// leggere senza lucchetti.
const CAUSA_FILO_CADUTO: u32 = 3;

/// Quante volte, in questa sessione, il filo della decodifica è caduto.
///
/// # Perché un contatore globale e non un campo del motore
///
/// Perché chi se ne serve arriva **dopo** che quel motore è stato buttato via:
/// il sorvegliante ricostruisce il motore e poi rimette la puntina dov'era, e
/// deve poter distinguere «il dispositivo era sparito» — dove rimettere la
/// puntina è la cosa giusta — da «quel file ha fatto cadere il decodificatore»,
/// dove rimetterla vuol dire un motore ricostruito ogni due secondi. Fra i due
/// istanti il motore in cui il conteggio sarebbe vissuto non esiste più.
///
/// Cresce e basta; nessuno lo azzera. Chi lo legge confronta il valore con
/// quello che aveva la volta scorsa, non con lo zero.
static CADUTI: AtomicU32 = AtomicU32::new(0);

/// Quante volte il filo della decodifica è caduto da quando l'applicazione è
/// aperta.
///
/// Serve a chi ricostruisce il motore per non rimettere la puntina nello stesso
/// solco: se il numero è cresciuto da quando quel brano era stato aperto
/// l'ultima volta, è quel brano ad aver fatto cadere il filo. Vedi la casella
/// `CADUTI` qui sopra.
#[must_use]
pub fn fili_caduti() -> u32 {
    CADUTI.load(Ordering::Acquire)
}

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
    Suona(Box<BranoAperto>),
    Prepara(Option<Box<BranoAperto>>),
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
    Latenza {
        ms: i64,
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
    /// Il nome dell'uscita su cui si è aperto davvero. Vedi
    /// [`Motore::dispositivo`].
    dispositivo: String,
    /// Tenerla viva tiene acceso il flusso.
    _uscita: Uscita,
}

impl Motore {
    /// Apre il dispositivo e avvia i fili.
    ///
    /// `voluto` è il nome dell'uscita da aprire, `None` per quella predefinita
    /// di sistema. Chiedere un nome che non c'è **non** fallisce: si ripiega
    /// sul predefinito, e [`Motore::dispositivo`] dice dove si è finiti.
    pub fn avvia(
        osservatore: impl Fn(Evento) + Send + 'static,
        voluto: Option<String>,
    ) -> Result<Self, AppError> {
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
            voluto,
        )?;
        let formato = uscita.formato;
        let dispositivo = uscita.dispositivo.clone();
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
            .spawn(move || filo_sorvegliato(contesto))
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
            dispositivo,
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
    /// Restituisce `None` se il lucchetto è avvelenato — cioè se qualcuno è
    /// caduto tenendolo. Non è più un'ipotesi di scuola: il filo della
    /// decodifica **può** cadere, e cadere è un fatto previsto da quando lo
    /// sorveglia `filo_sorvegliato`. Quel filo questo lucchetto non lo
    /// prende, quindi in pratica il `None` resta il caso che non arriva mai; ma
    /// la ragione per non dichiararlo impossibile con un `unwrap` adesso è un
    /// fatto e non una prudenza.
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

    /// Su quale uscita si sta suonando.
    ///
    /// È il nome del dispositivo **davvero** aperto, che può non essere quello
    /// chiesto: una preferenza che punta a una scheda staccata ripiega sul
    /// predefinito. Chi sorveglia i dispositivi confronta questa stringa con
    /// quel che l'utente voleva e con il predefinito di adesso: sono i tre
    /// termini che dicono se vale la pena riaprire.
    #[must_use]
    pub fn dispositivo(&self) -> &str {
        &self.dispositivo
    }

    /// Comincia a suonare questo brano, adesso, scartando quel che c'era.
    ///
    /// Il brano arriva **già aperto**: il perché sta in [`BranoAperto`].
    pub fn suona(&self, brano: BranoAperto) {
        // Abbassato qui e non all'apertura del file, per la ragione — e per
        // l'ordine — scritti in [`Motore::pausa`].
        self.condiviso.in_pausa.store(false, Ordering::Release);
        self.manda(Comando::Suona(Box::new(brano)));
    }

    /// Tiene pronto il brano dopo, per attaccarlo senza buco.
    ///
    /// È tutto il gapless: quando il corrente finisce, i campioni del prossimo
    /// sono già in coda per entrare nell'anello, e fra i due non c'è nessuna
    /// apertura di file da aspettare.
    pub fn prepara(&self, brano: Option<BranoAperto>) {
        self.manda(Comando::Prepara(brano.map(Box::new)));
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

    /// Di quanti millisecondi la catena d'uscita ritarda il suono, dichiarati.
    ///
    /// È il terzo dei tre ritardi elencati nel `//!` di questo modulo: quello che
    /// nessuno misura — mixer di sistema, driver, DAC, e su un'uscita senza fili
    /// la radio. Si somma alla latenza che `cpal` riporta e il totale si **toglie**
    /// dalla posizione raccontata, così quel che il cursore e i testi dicono è
    /// quel che l'orecchio sta ricevendo adesso.
    ///
    /// Positivo ritarda la posizione riportata, negativo la anticipa: il secondo
    /// verso serve a chi trova che i testi arrivino **tardi** anche con la misura
    /// in mano, cioè quando l'anticipo dei testi è già stato tarato per un'altra
    /// uscita.
    ///
    /// Non tocca il suono, non tocca il gapless e non sposta la fine del brano:
    /// vedi [`Contesto::forse_fine`]. Chi chiama ritaglia il valore — qui entra
    /// quel che arriva da una preferenza, e un numero assurdo sposterebbe il
    /// cursore e basta.
    pub fn latenza(&self, ms: i64) {
        self.manda(Comando::Latenza { ms });
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

    /// Il motore non suona più, e non ricomincerà da solo.
    ///
    /// # Perché il nome parla del dispositivo e la bandiera no
    ///
    /// Perché le cause sono diventate tre e la conseguenza è rimasta una. Due
    /// vengono dal dispositivo — sparito, o guasto nel backend — e la terza dal
    /// filo della decodifica che è caduto: là il dispositivo c'è ancora e
    /// funziona, ma nessuno riempie più l'anello e dalle casse esce silenzio.
    /// Per chi legge questa risposta la differenza non conta: in tutti e tre i
    /// casi la cura è un motore nuovo, e la strada che lo costruisce è quella
    /// del sorvegliante delle uscite. A distinguere serve
    /// [`Motore::causa_perdita`], o [`Motore::filo_caduto`] per la sola domanda
    /// che cambia una decisione.
    ///
    /// Il nome è rimasto quello per non riscrivere i venti punti che lo
    /// chiamano; la bandiera sotto è documentata in `Condiviso::perso`.
    #[must_use]
    pub fn dispositivo_perso(&self) -> bool {
        self.condiviso.perso.load(Ordering::Acquire)
    }

    /// La perdita viene dal filo della decodifica caduto, non dal dispositivo.
    ///
    /// L'unica domanda che cambia una decisione invece di una frase: chi
    /// compone il banner sceglie fra `playback.deviceLost` — «l'audio se n'è
    /// andato» — e `playback.stalled`, che dice la verità di questo caso, cioè
    /// che il dispositivo c'è e la musica no.
    #[must_use]
    pub fn filo_caduto(&self) -> bool {
        self.dispositivo_perso()
            && self.condiviso.causa_perdita.load(Ordering::Acquire) == CAUSA_FILO_CADUTO
    }

    /// La causa della perdita nelle due forme in cui serve: la frase italiana
    /// per il diario, la chiave da tradurre per la finestra.
    ///
    /// Una tabella sola, perché le due forme sono la stessa informazione detta
    /// a due destinatari diversi. Il giorno in cui `cpal` aggiunge un terzo
    /// tipo di `StreamError` c'è un `match` solo da toccare — prima erano due,
    /// affiancati da un commento che chiedeva di ricordarsene, ed è
    /// precisamente il genere di richiesta che prima o poi qualcuno non
    /// esaudisce.
    ///
    /// I primi due codici li scrive la callback dell'uscita, il terzo il filo
    /// della decodifica quando cade: è l'unica riga della tabella che non parli
    /// del dispositivo, e la sua frase lo dice invece di nasconderlo dietro un
    /// «audio non disponibile» che manderebbe l'utente a controllare i cavi.
    const fn perdita(codice: u32) -> (&'static str, &'static str) {
        match codice {
            1 => ("dispositivo non più disponibile", "deviceNotAvailable"),
            2 => ("guasto del sistema audio", "systemFailure"),
            CAUSA_FILO_CADUTO => ("il decodificatore si è interrotto", "decoderCrashed"),
            _ => ("causa sconosciuta", "unknown"),
        }
    }

    /// Perché è sparito, in una parola che si può mettere in un registro.
    ///
    /// `None` finché il motore suona. Le tre cause non sono la stessa cosa per
    /// chi legge: «non c'è più» è un cavo staccato o un dispositivo predefinito
    /// cambiato — riaprire funziona quasi sempre — un guasto del backend è
    /// tutto il resto del dispositivo, e riaprire è un tentativo e non una
    /// cura, mentre «il decodificatore si è interrotto» non parla del
    /// dispositivo affatto: quello è al suo posto, ed è il filo che leggeva i
    /// byte a essere caduto su un file. Riaprire rimette in piedi il motore in
    /// tutti e tre i casi; solo nel terzo rimettere la puntina **sullo stesso
    /// brano** rifà cadere tutto, ed è per questo che la causa va detta e non
    /// solo contata.
    ///
    /// La frase è **italiana** e resta tale: il diario è italiano tutto, e una
    /// riga tradotta in mezzo alle altre sarebbe l'unica fuori posto. Chi deve
    /// invece mostrare la causa in finestra prende [`Motore::codice_perdita`].
    #[must_use]
    pub fn causa_perdita(&self) -> Option<&'static str> {
        if !self.dispositivo_perso() {
            return None;
        }
        Some(Self::perdita(self.condiviso.causa_perdita.load(Ordering::Acquire)).0)
    }

    /// La stessa causa, ma come chiave da tradurre.
    ///
    /// # Perché non basta [`Motore::causa_perdita`]
    ///
    /// Perché quella restituisce una frase italiana, e quella frase attraversa
    /// l'IPC e finisce stampata cruda nella finestra **accanto** a una stringa
    /// tradotta: chi ha l'interfaccia in inglese legge «There is no audio.
    /// dispositivo non più disponibile.» Mezza riga in una lingua e mezza
    /// nell'altra, con la minuscola in mezzo.
    ///
    /// Il diario continua a usare la frase, perché il diario è italiano tutto;
    /// alla finestra serve il codice, che si traduce. È la stessa regola che
    /// `riproduzione/mod.rs` scrive per `motivo_prossimo`: il codice e non la
    /// frase.
    #[must_use]
    pub fn codice_perdita(&self) -> Option<&'static str> {
        if !self.dispositivo_perso() {
            return None;
        }
        Some(Self::perdita(self.condiviso.causa_perdita.load(Ordering::Acquire)).1)
    }

    /// Quanti campioni sono stati serviti a vuoto: se cresce, il disco non sta
    /// dietro.
    #[must_use]
    pub fn vuoti(&self) -> u64 {
        self.condiviso.vuoti.load(Ordering::Relaxed)
    }

    fn manda(&self, comando: Comando) {
        // Il canale è chiuso quando il filo della decodifica non c'è più, e da
        // qui non c'è niente di utile da farne: chi doveva accorgersene lo ha
        // già fatto dall'altra parte, dove il filo cadendo alza `perso` e
        // annuncia `playback.stalled`. Propagare un errore da ogni pulsante
        // riempirebbe la finestra di avvisi identici per un guasto già
        // raccontato una volta, e il pulsante che non si può premere sarebbe
        // l'ultimo posto in cui raccontarlo.
        let _ = self.comandi.send(comando);
    }
}

impl Drop for Motore {
    /// # Perché la bandiera prima del comando
    ///
    /// Perché fra il `Chiudi` e la morte del filo passa un giro di ciclo, e in
    /// quel giro il filo può ancora annunciare qualcosa: un `Fermato` perché
    /// l'ultimo brano è finito proprio adesso, un `Iniziato` perché un segno è
    /// stato raggiunto. L'osservatore, però, non è morto con il motore — parla
    /// allo stato dell'applicazione, che nel frattempo ha già in mano il motore
    /// **nuovo**. Quell'annuncio in ritardo verrebbe letto come un fatto del
    /// motore nuovo: un `Fermato` che chiude l'ascolto in corso e fa avanzare
    /// una coda che sta suonando altrove.
    ///
    /// Alzando `abbandonato` per primo, ogni annuncio di questo motore da qui
    /// in poi cade nel vuoto. Vedi `Condiviso::abbandonato`.
    fn drop(&mut self) {
        self.condiviso.abbandonato.store(true, Ordering::Release);
        self.manda(Comando::Chiudi);
    }
}

/// Un brano già aperto, pronto da dare al motore.
///
/// # Perché il motore riceve un brano aperto e non una sorgente da aprire
///
/// Perché aprire **legge**, e su una condivisione di rete legge molto più di
/// quanto sembri: il riconoscimento del contenitore si porta dietro tutti i
/// blocchi di metadati, e su un FLAC anche la copertina incorporata, che sono
/// spesso centinaia di kilobyte. Finché quel lavoro stava dentro il filo di
/// decodifica non c'era nessun posto in cui infilargli una scadenza — il filo
/// era già dentro la `ReadFile`, e da lì non lo tira fuori nessun sistema
/// operativo. La scadenza dei cinque secondi copriva la sola `File::open`, cioè
/// la parte veloce.
///
/// Spostando l'apertura di qua dal confine, chi chiama la può mandare su un filo
/// suo con una scadenza addosso — è quel che fa `sorgente_di` nell'applicazione
/// — e il filo di decodifica riceve qualcosa che è già pronto a consegnare
/// campioni.
///
/// Porta con sé durata e ReplayGain: quando toccherà a lui, la [`Sorgente`] da
/// cui vengono non esisterà più — è stata consumata dall'apertura — e senza
/// questi due campi il brano attaccato in gapless arriverebbe in interfaccia
/// con durata zero e senza correzione di volume.
pub struct BranoAperto {
    decodificatore: Decodificatore,
    durata_ms: u64,
    replaygain_db: Option<f32>,
}

impl BranoAperto {
    /// Apre una sorgente per un'uscita di questo formato.
    ///
    /// Il formato lo dice [`Motore::formato`]: è quello con cui il dispositivo
    /// si è aperto davvero, e il decodificatore ci ricampiona sopra.
    ///
    /// # Errori
    ///
    /// `playback.formatUnsupported` per un contenitore che non si riconosce,
    /// `fs.networkUnavailable` se a non rispondere è la condivisione —
    /// ritentabile, e chi chiama ci mette accanto il tasto «Riprova».
    pub fn apri(sorgente: Sorgente, formato: FormatoUscita) -> Result<Self, AppError> {
        let durata_ms = sorgente.durata_ms;
        let replaygain_db = sorgente.replaygain_db;
        let decodificatore = Decodificatore::apri(sorgente, formato.frequenza, formato.canali)?;
        Ok(Self {
            decodificatore,
            durata_ms,
            replaygain_db,
        })
    }

    /// Quale brano.
    #[must_use]
    pub const fn track_id(&self) -> i64 {
        self.decodificatore.track_id()
    }
}

impl std::fmt::Debug for BranoAperto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BranoAperto")
            .field("track_id", &self.decodificatore.track_id())
            .field("durata_ms", &self.durata_ms)
            .finish_non_exhaustive()
    }
}

/// Un brano che comincerà a sentirsi a un certo fotogramma d'uscita.
///
/// # La regola che lega i due numeri
///
/// `da` e `offset_ms` devono descrivere **lo stesso istante**: `da` dice quando
/// si sente, contato in fotogrammi d'uscita, e `offset_ms` dice a che punto del
/// brano si è in quel preciso momento. [`Contesto::aggiorna`] somma al secondo il
/// tempo scorso dal primo, quindi due istanti diversi dentro lo stesso segno
/// sfasano la posizione **per tutta la vita del segno**, cioè per tutto il brano.
///
/// I tre posti che annotano un segno lo fanno per tre istanti diversi, e ognuno
/// scrive la coppia coerente: l'avvio di un brano (`0`, `0`), un salto (`0`, il
/// millisecondo raggiunto), e metà dissolvenza (`spinti + meta`, mezza
/// dissolvenza in millisecondi).
struct Segno {
    /// Da quale fotogramma d'uscita in poi si sente questo brano.
    da: u64,
    track_id: i64,
    durata_ms: u64,
    /// A che punto del brano si è nell'istante indicato da `da`.
    ///
    /// Non è «quanto è stato saltato»: è la posizione dentro il brano in
    /// quell'istante, che per un salto coincide col punto raggiunto e per una
    /// dissolvenza vale mezza curva. Vedi la regola in testa al tipo.
    offset_ms: u64,
    replaygain_db: Option<f32>,
    /// A quale ascolto appartiene: un numero nuovo per ogni volta che un brano
    /// **comincia**, lo stesso numero per un salto dentro l'ascolto in corso.
    ///
    /// # Perché non basta `track_id`
    ///
    /// Perché lo stesso brano può cominciare due volte di fila. «Ripeti uno»
    /// prepara il corrente come proprio successivo, e una coda può contenere lo
    /// stesso brano due volte vicine: confrontando l'identificativo, il secondo
    /// inizio non si annunciava, chi sta sopra non preparava il giro dopo, e la
    /// musica si fermava alla seconda ripetizione — o il brano ancora dopo non
    /// partiva mai. L'annuncio segue l'ascolto, e l'ascolto è questo numero.
    ascolto: u64,
}

/// Un brano decodificato per intero che si sta ancora sentendo.
///
/// Il perché sta in [`Contesto::uscente`]. Qui interessa solo il numero che gli
/// sta accanto: è **l'ascolto**, non l'identificativo del brano, perché lo
/// stesso brano può essere in fila due volte — «ripeti uno» fa esattamente
/// questo — e allora `track_id` non distingue quale dei due si sta sentendo.
/// Vedi [`Segno::ascolto`].
struct Uscente {
    ascolto: u64,
    brano: BranoAperto,
}

struct Contesto {
    comandi: Receiver<Comando>,
    produttore: rtrb::Producer<f32>,
    condiviso: Arc<Condiviso>,
    posizione: Arc<std::sync::Mutex<Posizione>>,
    osservatore: Box<dyn Fn(Evento) + Send>,
    formato: FormatoUscita,
    corrente: Option<Decodificatore>,
    /// Il brano che si sta ancora **sentendo** quando la decodifica è già
    /// passata a quello dopo.
    ///
    /// # Perché va tenuto invece di essere lasciato cadere
    ///
    /// Perché `corrente` è il decodificatore, e il decodificatore corre avanti
    /// di tutto quel che l'anello tiene da parte: su un'uscita a 48 kHz stereo
    /// sono più di tre secondi (vedi [`crate::uscita::RISERVA_MS`], che spiega
    /// perché la riserva dichiarata sia molto meno di quel che l'anello regge).
    /// Negli ultimi tre secondi di ogni brano, quindi, il brano che si sente e
    /// il brano che si decodifica sono **due brani diversi**, e finché questo
    /// campo non c'era il decodificatore di quello che si sentiva veniva
    /// lasciato cadere in [`Contesto::passa_al_prossimo`].
    ///
    /// Se ne accorgeva chi trascinava il cursore in quei tre secondi: il salto
    /// cadeva sul decodificatore del brano **successivo** — si sentiva quello,
    /// dal punto chiesto, mentre l'interfaccia continuava a mostrare il titolo
    /// di prima e il cursore correva oltre la fine della barra. Poi il brano
    /// dopo finiva, in canna non c'era rimasto niente, e il motore restava lì:
    /// nessun `Iniziato`, nessun `Fermato`, la coda ferma.
    ///
    /// Vive quanto il **segno** a cui appartiene: lo posa
    /// [`Contesto::passa_al_prossimo`], lo riprende
    /// [`Contesto::riporta_sotto_la_puntina`], e lo lasciano cadere
    /// [`Contesto::aggiorna`] — quando quel segno esce dalla fila, cioè quando
    /// l'ultimo dei suoi campioni è stato udito — e
    /// [`Contesto::scarta_in_volo`]. Non è una cache: tenerlo più a lungo
    /// vorrebbe dire un file aperto in più per tutta la durata del brano
    /// seguente, che su Windows è un file che non si può cancellare.
    uscente: Option<Uscente>,
    prossimo: Option<BranoAperto>,
    /// Dove comincia ogni brano, in fotogrammi d'uscita.
    segni: VecDeque<Segno>,
    /// Campioni decodificati che non sono ancora entrati nell'anello.
    resto: VecDeque<f32>,
    blocco: Vec<f32>,
    /// Fotogrammi spinti nell'anello da quando si è azzerato il conteggio.
    spinti: u64,
    /// L'ultimo ascolto annunciato con [`Evento::Iniziato`]: vedi
    /// [`Segno::ascolto`].
    annunciato: Option<u64>,
    /// Il numero dell'ultimo ascolto cominciato. Vedi [`Contesto::nuovo_ascolto`].
    ascolti: u64,
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
    /// La latenza della catena d'uscita dichiarata a mano, in millisecondi.
    ///
    /// Il terzo dei tre ritardi del `//!`, quello che nessuna misura vede.
    /// Positivo toglie alla posizione riportata, negativo le aggiunge. Vedi
    /// [`Motore::latenza`].
    latenza_manuale_ms: i64,
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
    entrante: Option<BranoAperto>,
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

/// Il filo della decodifica, dentro la rete che lo raccoglie se cade.
///
/// # Perché un `catch_unwind` in un albero senza `unwrap`
///
/// Perché la regola che vieta `unwrap`, `expect`, `panic!` e l'indicizzazione
/// vale sul **nostro** codice, e questo filo passa la sua vita dentro codice di
/// terzi: symphonia decodifica byte arrivati da un disco o da una condivisione
/// di rete, rubato ricampiona. Un pacchetto troncato a metà da una Wi-Fi che
/// respira male è un ingresso che quei due non hanno mai visto, e un `assert!`
/// dentro un decodificatore è un modo perfettamente normale di reagirvi. La
/// regola ci difende da quel che scriviamo noi; questa funzione da quel che
/// chiamiamo.
///
/// # Cosa succedeva senza
///
/// Il `JoinHandle` di questo filo viene buttato via — nessuno lo aspetta — e il
/// profilo di rilascio è `panic = "unwind"`: il filo si smontava in silenzio, e
/// da fuori non cambiava **niente**. La callback di cpal continuava a girare su
/// un anello che nessuno riempiva più, cioè a consegnare zeri al dispositivo
/// per sempre; `perso` restava basso, quindi il sorvegliante delle uscite non
/// vedeva niente da riaprire; i comandi finivano in un canale senza ricevitore
/// e sparivano. L'utente premeva pausa e non succedeva nulla, premeva
/// «prossimo» e non succedeva nulla. Nel registro eventi di Windows non
/// compariva niente, perché il processo era vivo.
///
/// # Perché il contesto si presta invece di essere consumato
///
/// Perché quel che serve **dopo** la caduta sta tutto dentro: l'osservatore, per
/// annunciare il guasto a chi sta sopra, e `condiviso`, per alzare la bandiera
/// che fa ricostruire il motore. Passando `ctx` per valore dentro la chiusura,
/// dopo l'unwind sarebbe irraggiungibile — sarebbe stato smontato con lo stack
/// — e non resterebbe nessuno a dire cos'è successo. Prestandolo, il contesto
/// vive qui fuori e la chiusura lo tocca soltanto.
///
/// # Cosa dichiara `AssertUnwindSafe`
///
/// Che un `Contesto` osservato **dopo** un unwind è ancora buono da leggere. È
/// vero per quel poco che se ne fa: [`Contesto::caduto`] tocca la posizione — un
/// mutex, e un mutex avvelenato lo recupera — e l'osservatore, che è una
/// `Fn` senza stato mutabile suo. I decodificatori, le code e i buffer possono
/// benissimo essere rimasti a metà: nessuno li guarda più, perché questo filo
/// non decodificherà più niente e il motore intero verrà buttato.
///
/// La traccia del panico non si raccoglie: la stampa il gancio globale
/// installato da `crate::diario`, che gira **prima** dell'unwind e quindi prima
/// di qui. Di qua passa solo il messaggio, che è quel che va nel banner.
fn filo_sorvegliato(mut ctx: Contesto) {
    let esito = std::panic::catch_unwind(AssertUnwindSafe(|| filo(&mut ctx)));
    if let Err(carico) = esito {
        ctx.caduto(&messaggio_del_panico(carico.as_ref()));
    }
}

/// Il messaggio di un panico, che è tutto quel che se ne può raccontare.
///
/// `panic!("…")` con una stringa letterale consegna un `&'static str`, uno con
/// argomenti da formattare una `String`, e un panico che arriva da altrove può
/// portare qualunque cosa. I primi due si leggono, il terzo no: dire «causa
/// sconosciuta» è meglio che stampare l'indirizzo di una scatola.
fn messaggio_del_panico(carico: &(dyn std::any::Any + Send)) -> String {
    if let Some(testo) = carico.downcast_ref::<&'static str>() {
        return (*testo).to_owned();
    }
    if let Some(testo) = carico.downcast_ref::<String>() {
        return testo.clone();
    }
    "causa sconosciuta".to_owned()
}

fn filo(ctx: &mut Contesto) {
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
            uscente: None,
            prossimo: None,
            segni: VecDeque::new(),
            resto: VecDeque::new(),
            blocco: Vec::new(),
            spinti: 0,
            annunciato: None,
            ascolti: 0,
            fine_dichiarata: true,
            volume: 0.8,
            muto: false,
            replaygain_attivo: true,
            bersaglio_db: -18.0,
            rg_decodifica: None,
            dissolvenza_ms: 0,
            latenza_manuale_ms: 0,
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
            Comando::Suona(brano) => self.avvia_brano(*brano),
            Comando::Prepara(brano) => self.prepara(brano.map(|b| *b)),
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
            // Vale dal prossimo giro d'orologio, che è fra quattro millisecondi:
            // non c'è niente da rifare e niente da svuotare, perché la latenza
            // non tocca i campioni — sposta soltanto quel che si racconta di
            // loro.
            Comando::Latenza { ms } => {
                self.latenza_manuale_ms = ms;
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
        // Con l'anello vuoto non si sente più niente di quel che c'era dentro:
        // il brano messo da parte non ha più nessuno che lo ascolti. Vedi
        // [`Contesto::uscente`].
        self.uscente = None;
    }

    /// Installa un brano già aperto e comincia a consegnarne i campioni.
    ///
    /// Non c'è nessun ramo d'errore, e non è una dimenticanza: il brano è
    /// arrivato aperto, quindi tutto quel che poteva andare storto — il
    /// contenitore irriconoscibile, la rete che non risponde — è già andato
    /// storto dall'altra parte del confine, dove c'era una scadenza a
    /// raccoglierlo e un `Result` da restituire a chi aveva premuto play. Vedi
    /// [`BranoAperto`].
    fn avvia_brano(&mut self, brano: BranoAperto) {
        let track_id = brano.decodificatore.track_id();
        let durata_ms = brano.durata_ms;
        let replaygain_db = brano.replaygain_db;
        self.scarta_in_volo();
        self.corrente = Some(brano.decodificatore);
        self.prossimo = None;
        self.annunciato = None;
        self.fine_dichiarata = false;
        self.rg_decodifica = replaygain_db;
        self.durata_corrente_ms = durata_ms;
        self.azzera_dissolvenza();
        self.applica_guadagno();
        let ascolto = self.nuovo_ascolto();
        self.segni.push_back(Segno {
            da: 0,
            track_id,
            durata_ms,
            offset_ms: 0,
            replaygain_db,
            ascolto,
        });
    }

    /// Il numero di un ascolto che comincia adesso: vedi [`Segno::ascolto`].
    const fn nuovo_ascolto(&mut self) -> u64 {
        self.ascolti = self.ascolti.wrapping_add(1);
        self.ascolti
    }

    /// Mette in canna il brano dopo, o toglie quello che c'era.
    ///
    /// Anche qui niente ramo d'errore: chi apre è chi chiama, e un successivo
    /// che non si apre lo scopre **lui**, mentre il corrente suona ancora — che
    /// era già il momento giusto per scoprirlo, solo dall'altra parte del
    /// confine. Vedi [`BranoAperto`].
    ///
    /// # Quando arriva dopo che il corrente è già finito
    ///
    /// Allora si attacca **adesso**, e non è un caso di scuola: è quel che
    /// succede a ogni brano più corto dell'anello. La decodifica corre avanti
    /// finché c'è posto davanti alla callback, e su un'uscita normale quel posto
    /// vale più di tre secondi di musica — vedi [`crate::uscita::RISERVA_MS`],
    /// che quel conto lo fa per esteso. Un intermezzo di due secondi è quindi
    /// decodificato **per intero** entro qualche decina di millisecondi dal
    /// `play`: [`Contesto::passa_al_prossimo`] scatta mentre dalle casse esce
    /// ancora l'inizio del brano, non trova niente in canna e azzera il
    /// corrente. Il preparatore — che parte dall'annuncio di `Iniziato`, aspetta
    /// l'antirimbalzo della raffica e poi apre un file — arriva qualche
    /// millisecondo più tardi, e arriva qui.
    ///
    /// Senza questa riga quel brano lasciava il motore in uno stato da cui non
    /// si usciva più: `corrente` vuoto, quindi
    /// [`Contesto::decodifica_un_blocco`] esce subito e non chiama mai
    /// `passa_al_prossimo`; `prossimo` pieno, quindi [`Contesto::forse_fine`]
    /// non dichiara la fine. Niente `Iniziato`, niente `Fermato`, la coda ferma
    /// e il cursore piantato sull'ultimo millisecondo — finché qualcuno non
    /// premeva un pulsante. Con un album di intermezzi la riproduzione moriva al
    /// primo.
    ///
    /// Attaccare qui non fa nessun buco: l'anello è ancora pieno di quel che
    /// resta del brano finito, e il segno che `passa_al_prossimo` annota cade su
    /// `spinti`, cioè esattamente dopo il suo ultimo campione. È lo stesso
    /// gapless di sempre, deciso qualche millisecondo più tardi.
    ///
    /// Non vale a fine dichiarata: lì il motore si è già detto fermo e la
    /// callback esce in silenzio, quindi attaccare un brano vorrebbe dire una
    /// musica che riparte da sola dopo che era finita — e senza che si senta,
    /// perché [`Condiviso::in_pausa`] è alzato. Chi prepara a motore fermo sta
    /// solo tenendo pronto il dopo per quando qualcuno ripremerà play.
    fn prepara(&mut self, brano: Option<BranoAperto>) {
        self.prossimo = brano;
        if self.corrente.is_none() && self.prossimo.is_some() && !self.fine_dichiarata {
            self.passa_al_prossimo();
        }
    }

    fn ferma(&mut self) {
        self.scarta_in_volo();
        self.corrente = None;
        self.prossimo = None;
        self.azzera_dissolvenza();
        self.annunciato = None;
        if !self.fine_dichiarata {
            self.fine_dichiarata = true;
            self.annuncia(Evento::Fermato);
        }
        self.scrivi_posizione(Posizione::default());
    }

    /// Rimette sotto la puntina il brano che si **sente**, se la decodifica è
    /// già passata a quello dopo. `false` se non c'è modo di farlo, e allora
    /// chi chiama deve lasciar perdere invece di andare avanti su `corrente`.
    ///
    /// # Perché serve
    ///
    /// Perché `corrente` non è il brano che esce dalle casse: è quello che il
    /// decodificatore sta macinando, e negli ultimi secondi di ogni brano i due
    /// sono diversi — la ragione, e il difetto che ne nasceva, stanno in
    /// [`Contesto::uscente`]. Chi deve agire su quel che si sente lo chiede a
    /// `segni.front()`, che è la stessa fonte da cui [`Contesto::aggiorna`]
    /// prende il titolo e la posizione mostrati.
    ///
    /// Lo scambio è quello che questa funzione fa già per l'entrante di una
    /// dissolvenza: il brano che stava in canna torna a essere il **prossimo**,
    /// riavvolto al suo inizio, e quello di prima torna corrente.
    fn riporta_sotto_la_puntina(&mut self) -> bool {
        // A metà sovrapposizione il segno in testa è già dell'entrante mentre
        // `corrente` è ancora l'uscente, e non è un disallineamento da
        // correggere: è il modo in cui un salto durante una dissolvenza torna
        // al brano di prima invece di far avanzare la coda. Vedi il seguito di
        // [`Contesto::vai_a`].
        if self.entrante.is_some() {
            return true;
        }
        let Some(testa) = self.segni.front() else {
            // Nessun segno: non c'è nessun brano che si sente, e non c'è niente
            // da riportare indietro.
            return true;
        };
        let (udito, ascolto) = (testa.track_id, testa.ascolto);
        if self
            .corrente
            .as_ref()
            .is_some_and(|d| d.track_id() == udito)
        {
            return true;
        }
        // Serve quello messo da parte, e serve che sia proprio l'ascolto in
        // testa: su brani più corti di quel che l'anello tiene da parte
        // `passa_al_prossimo` scatta due volte prima che il primo dei due
        // finisca di sentirsi, e il secondo passaggio si porta via il
        // decodificatore del primo. Lì non c'è niente da riportare indietro, e
        // il salto si lascia cadere.
        if self.uscente.as_ref().map(|u| u.ascolto) != Some(ascolto) {
            return false;
        }
        // Con qualcosa già in canna lo scambio perderebbe un brano: quello che
        // sposta indietro andrebbe scritto dove sta il seguente. Non capita —
        // chi prepara non consegna finché l'annuncio non è arrivato — ma se
        // capitasse, un salto lasciato cadere è meglio di una coda che salta una
        // riga.
        if self.prossimo.is_some() {
            return false;
        }
        let Some(uscente) = self.uscente.take().map(|u| u.brano) else {
            return false;
        };
        // Quel che stava in canna torna a essere il prossimo, dal suo inizio:
        // l'anello con i suoi campioni sta per essere buttato, e riattaccarlo a
        // metà lo farebbe cominciare dove nessuno l'ha sentito. Se non sa
        // tornare al proprio inizio si lascia perdere, come per l'entrante.
        if let Some(decodificatore) = self.corrente.take() {
            let mut futuro = BranoAperto {
                decodificatore,
                durata_ms: self.durata_corrente_ms,
                replaygain_db: self.rg_decodifica,
            };
            if futuro.decodificatore.cerca(0).is_ok() {
                self.prossimo = Some(futuro);
            }
        }
        self.durata_corrente_ms = uscente.durata_ms;
        self.rg_decodifica = uscente.replaygain_db;
        self.corrente = Some(uscente.decodificatore);
        true
    }

    fn vai_a(&mut self, ms: u64) {
        // Il salto cade sul brano che si sente, non su quello che si decodifica:
        // vedi [`Contesto::riporta_sotto_la_puntina`]. Quando i due non sono lo
        // stesso e non c'è modo di rimetterli in fila, il salto si lascia cadere
        // — la puntina resta dov'era, che è quel che fa anche un salto fallito
        // qui sotto.
        if !self.riporta_sotto_la_puntina() {
            return;
        }
        let Some(decodificatore) = self.corrente.as_mut() else {
            return;
        };
        let track_id = decodificatore.track_id();
        if let Err(err) = decodificatore.cerca(ms) {
            // Si annuncia e si torna indietro, **senza** fermare: un salto che
            // non riesce lascia la puntina dov'era, e dov'era è ancora un posto
            // buono da cui continuare.
            //
            // Vale anche quando il guasto viene dalla rete, ed è voluto:
            // `Decodificatore::cerca` lo riconosce per quel che è, quindi il
            // codice che esce di qui è già ritentabile — la finestra si annota
            // il punto e disegna «Riprova». Se poi la share è davvero morta, la
            // prima lettura che segue lo scopre e **là** il motore si ferma, dal
            // ramo di rete di [`Contesto::decodifica_un_blocco`]. Fermarsi già
            // qui vorrebbe dire buttare via un brano per un salto fallito su una
            // rete che magari sta solo respirando male.
            self.annuncia(Evento::Errore(Box::new(err)));
            return;
        }
        // Dove si è atterrati, che non è sempre dove si era chiesto: sopra la
        // fine del flusso `Decodificatore::cerca` mette un tetto invece di
        // fallire, e scrivere qui il millisecondo **chiesto** manderebbe il
        // cursore fuori dalla barra per gli ultimi istanti del brano.
        let raggiunto_ms = ms_da_fotogrammi(decodificatore.consegnati(), self.formato.frequenza);
        // Il segno del brano corrente, per non perderne durata e ReplayGain.
        let (durata_ms, replaygain_db) = self
            .segni
            .iter()
            .find(|s| s.track_id == track_id)
            .map_or((0, None), |s| (s.durata_ms, s.replaygain_db));
        // L'ascolto è quello del segno che si sta sentendo, qualunque brano sia.
        // Il brano corrente, se non era ancora stato annunciato — un salto un
        // istante dopo il play —, lo sarà col segno nuovo. Quello già annunciato
        // non lo sarà una seconda volta: il conteggio dell'ascolto ripartirebbe
        // da capo. E a metà dissolvenza il segno che si sente è già
        // dell'entrante: col suo numero, il ritorno al brano di prima non sembra
        // un brano nuovo, cioè un salto non fa avanzare la coda.
        let ascolto = match self.segni.front() {
            Some(segno) => segno.ascolto,
            None => self.nuovo_ascolto(),
        };
        self.scarta_in_volo();
        // Una sovrapposizione in corso non sopravvive a un salto: i campioni
        // già mescolati sono appena stati buttati, e il brano che stava
        // entrando torna a essere quello che verrà **dopo** — dal suo inizio,
        // non da dove la curva l'aveva portato. Senza questo riavvolgimento
        // attaccherebbe a metà, e senza il ritorno in `prossimo` non
        // attaccherebbe affatto: al suo posto suonerebbe quello ancora dopo,
        // che chi sta sopra ha preparato all'annuncio di metà curva.
        //
        // Se non si riavvolge si lascia perdere: un brano che non sa tornare al
        // proprio inizio non è materiale per una dissolvenza, e in `prossimo`
        // resta quel che c'era.
        //
        // L'annuncio di metà curva invece era già partito, e per chi sta sopra
        // il brano corrente è quello che stava entrando: non va disfatto né
        // rifatto, ed è quel che `ascolto`, preso qui sopra dal segno che si
        // sentiva, già garantisce.
        if let Some(mut entrante) = self.entrante.take()
            && entrante.decodificatore.cerca(0).is_ok()
        {
            self.prossimo = Some(entrante);
        }
        self.azzera_dissolvenza();
        self.segni.push_back(Segno {
            da: 0,
            track_id,
            durata_ms,
            offset_ms: raggiunto_ms,
            replaygain_db,
            ascolto,
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
                // # Perché un guasto di rete non fa passare al brano dopo
                //
                // Perché il brano dopo, con ogni probabilità, sta sulla stessa
                // condivisione che è appena morta. Passare a lui vuol dire
                // un'altra attesa di quaranta secondi, un altro errore, e così
                // via lungo tutta la coda: in un minuto l'utente ha perso il
                // punto in cui stava ascoltando, ha ricevuto una raffica di
                // avvisi e si ritrova la coda finita. Un cavo staccato non deve
                // costare la serata.
                //
                // Fermarsi invece la conserva: il brano resta quello, la
                // posizione pure, e la finestra offre «Riprova» perché il codice
                // che arriva è ritentabile. Vedi `Decodificatore::guasto`.
                //
                // Ogni altro guasto continua a far saltare il brano, ed è
                // giusto: un file davvero rotto non si aggiusta aspettando, e
                // fermare la riproduzione su di lui vorrebbe dire che un album
                // con un file corrotto in mezzo non arriva più in fondo.
                let di_rete = err.code().kind() == ErrorCodeKind::FsNetworkUnavailable;
                self.annuncia(Evento::Errore(Box::new(err)));
                if di_rete {
                    self.ferma();
                    return false;
                }
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
        //
        // # `da` e `offset_ms` descrivono lo stesso istante
        //
        // È la regola di tutto il tipo [`Segno`], e qui era rotta. `da` dice
        // *quando* si sente — il fotogramma d'uscita — e `offset_ms` dice *dove*
        // si è, dentro il brano, in quell'istante. Devono parlare dello stesso
        // momento, altrimenti `aggiorna` somma una posizione a un tempo scorso
        // che parte da un'altra parte.
        //
        // Questo blocco è il primo che contiene audio dell'entrante: il suo
        // fotogramma zero esce a `self.spinti`. Quindi quando il contatore
        // arriva a `spinti + meta`, l'entrante ha già prodotto `meta`
        // fotogrammi — è a metà dissolvenza del *suo* inizio, non al suo inizio.
        // Con `offset_ms: 0`, com'era scritto qui, la posizione riportata
        // restava indietro di mezza dissolvenza **per tutto il brano**: con sei
        // secondi di dissolvenza, tre secondi, dal primo all'ultimo istante. È
        // la causa numero uno dello sfasamento dei testi, e sballava insieme
        // scrubber, «riprendi dov'eri» e il pannello di Windows.
        //
        // # Metà di quel che si sovrappone davvero
        //
        // Che non è sempre la dissolvenza intera: dopo un salto negli ultimi
        // secondi, o con un brano più corto della dissolvenza, all'uscente
        // resta meno della curva — i fotogrammi di questo blocco più quelli che
        // mancano. Il segno a metà della curva intera cadeva **dopo** la fine
        // dell'uscente: con dodici secondi di dissolvenza e uno di brano, per
        // cinque secondi si sentiva solo il brano dopo, e il motore raccontava
        // quello di prima a ventisette secondi su ventidue.
        if !self.segno_dissolvenza {
            let sovrapposti = durata
                .min(mancano.saturating_add(fotogrammi(self.blocco.len(), self.formato.canali)));
            #[expect(
                clippy::integer_division,
                reason = "metà dissolvenza: mezzo fotogramma non sposta il \
                          punto in cui il brano nuovo prende il sopravvento"
            )]
            let meta = sovrapposti / 2;
            let ascolto = self.nuovo_ascolto();
            if let Some(preparato) = self.entrante.as_ref() {
                self.segni.push_back(Segno {
                    da: self.spinti.saturating_add(meta),
                    track_id: preparato.decodificatore.track_id(),
                    durata_ms: preparato.durata_ms,
                    offset_ms: ms_da_fotogrammi(meta, self.formato.frequenza),
                    replaygain_db: preparato.replaygain_db,
                    ascolto,
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
        // Il brano che esce dalla puntina non esce dalle casse: quel che ha
        // decodificato è ancora dentro l'anello, e per i prossimi secondi è
        // **lui** quello che si sente. Si mette da parte invece di lasciarlo
        // cadere, perché un salto che arriva adesso deve poterlo ritrovare.
        // Vedi [`Contesto::uscente`].
        //
        // Prima del `match` e non dentro il ramo che attacca il successivo:
        // l'ultimo brano di una coda finisce di decodificarsi con la stessa
        // manciata di secondi d'anticipo, e un salto in quei secondi è
        // esattamente il caso di chi vuole risentire la fine di un brano prima
        // che la musica smetta.
        //
        // Durata e ReplayGain sono ancora i suoi: chi li sovrascrive sta più in
        // basso. L'ascolto è quello del suo segno, cercato dal fondo perché con
        // una dissolvenza in corso il segno dell'entrante è già in fila.
        //
        // Si scrive solo se c'è davvero qualcosa da mettere da parte: questa
        // funzione viene chiamata una seconda volta quando un preparato arriva
        // **dopo** che il corrente è finito — vedi [`Contesto::prepara`] — e lì
        // `corrente` è già vuoto. Assegnare comunque cancellerebbe il brano che
        // in quel momento si sta ancora sentendo, che è esattamente quello
        // messo da parte al giro di prima.
        if let Some(decodificatore) = self.corrente.take() {
            let id = decodificatore.track_id();
            let ascolto = self
                .segni
                .iter()
                .rev()
                .find(|s| s.track_id == id)
                .map_or(0, |s| s.ascolto);
            self.uscente = Some(Uscente {
                ascolto,
                brano: BranoAperto {
                    decodificatore,
                    durata_ms: self.durata_corrente_ms,
                    replaygain_db: self.rg_decodifica,
                },
            });
        }
        match self.entrante.take().or_else(|| self.prossimo.take()) {
            Some(preparato) => {
                let track_id = preparato.decodificatore.track_id();
                // Con una dissolvenza in corso il segno è già stato annotato a
                // metà sovrapposizione, e rimetterlo qui vorrebbe dire lo
                // stesso brano che comincia due volte.
                //
                // # Perché qui `offset_ms` è zero, e non va «uniformato»
                //
                // Perché qui il brano attacca **dal suo primo campione**: senza
                // dissolvenza il passaggio è un gapless, e il fotogramma zero
                // dell'entrante esce esattamente a `self.spinti`. `da` e
                // `offset_ms` descrivono lo stesso istante — la regola di
                // [`Segno`] — e quell'istante è l'inizio del brano, quindi zero è
                // la verità.
                //
                // In [`Contesto::forse_dissolvi`] lo stesso campo vale mezza
                // dissolvenza, e non è un'incoerenza da appianare per simmetria:
                // là il segno è puntato a `spinti + meta`, cioè a un istante in
                // cui l'entrante ha già suonato `meta` fotogrammi. Chi
                // uniformasse le due rimetterebbe il difetto che quel commento
                // racconta.
                if !self.segno_dissolvenza {
                    let ascolto = self.nuovo_ascolto();
                    self.segni.push_back(Segno {
                        da: self.spinti,
                        track_id,
                        durata_ms: preparato.durata_ms,
                        offset_ms: 0,
                        replaygain_db: preparato.replaygain_db,
                        ascolto,
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

    /// Quanti fotogrammi separano il contatore della callback dall'orecchio.
    ///
    /// La somma dei due ritardi che la callback non conta: quello **misurato** —
    /// il buffer del dispositivo, che `cpal` riporta a ogni blocco — e quello
    /// **dichiarato a mano**, cioè tutto il resto della catena d'uscita. Il
    /// `//!` in testa al modulo li distingue tutti e tre.
    ///
    /// Può uscire **negativo**, e non è un errore: una correzione a mano negativa
    /// dice «la posizione raccontata arriva tardi, anticipala», ed è il verso che
    /// serve a chi ha già tarato l'anticipo dei testi su un'altra uscita.
    ///
    /// # Perché una misura assurda si butta
    ///
    /// Perché quel che `cpal` riporta è la durata del buffer del dispositivo:
    /// decine di millisecondi, non centinaia. Un quarto di secondo lì dentro non
    /// è un'uscita lenta — è un orologio del dispositivo che ha risposto una
    /// sciocchezza — e compensarlo sposterebbe cursore e testi di un quarto di
    /// secondo senza che nessuno possa risalire al perché. Sopra
    /// [`crate::uscita::LATENZA_MASSIMA_MS`] resta solo il numero che qualcuno ha
    /// scelto guardando l'effetto.
    fn ritardo_fotogrammi(&self) -> i64 {
        let frequenza = self.formato.frequenza;
        let misurata = self.condiviso.latenza_fotogrammi.load(Ordering::Relaxed);
        let tetto = fotogrammi_da_ms(crate::uscita::LATENZA_MASSIMA_MS, frequenza);
        let misurata = if misurata > tetto { 0 } else { misurata };
        let misurata = i64::try_from(misurata).unwrap_or(i64::MAX);
        misurata.saturating_add(fotogrammi_da_ms_con_segno(
            self.latenza_manuale_ms,
            frequenza,
        ))
    }

    /// Traduce i fotogrammi usciti in un brano e in un istante.
    ///
    /// Due contatori e non uno, ed è la distinzione che il `//!` del modulo
    /// spiega: i **`suonati`** sono i fotogrammi che la callback ha consegnato al
    /// buffer del dispositivo, gli **`uditi`** sono quelli che l'orecchio ha
    /// davvero ricevuto. Gli `uditi` governano sia la scelta del segno sia i
    /// millisecondi riportati — raccontare dove si è, per definizione, è
    /// raccontare dov'è l'orecchio. I `suonati` grezzi vanno a
    /// [`Contesto::forse_fine`], e il perché è scritto là.
    fn aggiorna(&mut self) {
        let suonati = self.condiviso.fotogrammi.load(Ordering::Relaxed);
        let ritardo = self.ritardo_fotogrammi();
        let uditi = if ritardo >= 0 {
            suonati.saturating_sub(u64::try_from(ritardo).unwrap_or(0))
        } else {
            suonati.saturating_add(ritardo.unsigned_abs())
        };

        // Il segno valido è l'ultimo già raggiunto da quel che si sente. Sui
        // `suonati` si cambierebbe brano mentre dalle casse esce ancora il
        // precedente, cioè si annuncerebbe un `Iniziato` in anticipo — e lo
        // scrobble partirebbe prima del primo campione udibile.
        while self.segni.len() > 1 {
            let prossimo_arrivato = self.segni.get(1).is_some_and(|s| s.da <= uditi);
            if prossimo_arrivato {
                self.segni.pop_front();
            } else {
                break;
            }
        }

        // Il brano messo da parte serve finché un suo campione deve ancora
        // essere udito: dopo non lo ascolta più nessuno, e tenerne aperto il
        // file costerebbe una cancellazione negata per tutta la durata del brano
        // dopo. Vedi [`Contesto::uscente`].
        //
        // La domanda è «il suo segno è ancora in fila?», e non «è ancora quello
        // in testa»: quando l'anello tiene da parte più di un brano —
        // dell'ordine dei tre secondi, vedi [`crate::uscita::RISERVA_MS`] — la
        // decodifica finisce il brano dopo *prima* che il primo smetta di
        // sentirsi, e `passa_al_prossimo` posa qui il secondo mentre in testa
        // c'è ancora il primo. Guardare la testa lo butterebbe via un brano in
        // anticipo, ed è il salto durante quel brano a restare senza.
        if self
            .uscente
            .as_ref()
            .is_some_and(|u| !self.segni.iter().any(|s| s.ascolto == u.ascolto))
        {
            self.uscente = None;
        }

        let Some(segno) = self.segni.front() else {
            self.scrivi_posizione(Posizione::default());
            self.forse_fine(suonati);
            return;
        };

        let scorsi = uditi.saturating_sub(segno.da);
        let ms = segno
            .offset_ms
            .saturating_add(ms_da_fotogrammi(scorsi, self.formato.frequenza));
        let (track_id, durata_ms, replaygain_db, ascolto) = (
            segno.track_id,
            segno.durata_ms,
            segno.replaygain_db,
            segno.ascolto,
        );

        // L'ascolto e non il brano: vedi [`Segno::ascolto`].
        if self.annunciato != Some(ascolto) {
            self.annunciato = Some(ascolto);
            // Nessun `applica_guadagno` qui: la correzione del brano è già nei
            // campioni da quando sono stati decodificati. Prima andava
            // riapplicata **adesso** — al fotogramma in cui il brano nuovo
            // cominciava a sentirsi — ed era l'unica ragione per cui `Segno`
            // portava con sé un `replaygain_db`.
            self.annuncia(Evento::Iniziato { track_id });
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
    ///
    /// # Perché legge i `suonati` e non gli `uditi`
    ///
    /// Perché la fine si dichiara quando l'ultimo campione ha lasciato **l'anello
    /// nostro**, non quando ha lasciato le casse. Questo evento è il segnale con
    /// cui chi sta sopra fa avanzare la coda e prepara il brano dopo, e ritardarlo
    /// della latenza d'uscita vorrebbe dire ritardare di altrettanto il brano
    /// successivo: su un album gapless si sentirebbe un buco fra una traccia e
    /// l'altra, lungo esattamente la latenza compensata. La compensazione serve a
    /// raccontare meglio dove si è, non a far suonare la musica più tardi.
    ///
    /// Il prezzo, dichiarato: il `Fermato` dell'ultimo brano della coda arriva una
    /// latenza d'uscita prima che il suono finisca davvero. Sono i millisecondi in
    /// cui il pulsante mostra già il triangolo mentre l'ultima coda esce dal DAC,
    /// e nessuno li vede.
    ///
    /// # Perché azzera la posizione
    ///
    /// Per la stessa ragione di [`Contesto::ferma`], che è l'altro modo in cui
    /// la musica finisce: dichiarata la fine non c'è più nessun brano nel
    /// motore, e lasciare l'ultimo `track_id` scritto nella posizione vorrebbe
    /// dire un motore che si dice fermo su un brano che non ha più. Chi sta
    /// sopra legge quel campo per distinguere «in pausa a metà» da «non ho
    /// niente in mano»: con l'ultimo brano ancora lì, «riprendi» chiedeva al
    /// motore vuoto di ripartire — cioè non faceva niente — mentre il pulsante
    /// diventava «pausa». I segni se ne vanno con lui: sono la mappa fra
    /// fotogrammi e brani, e i fotogrammi ricominceranno da zero.
    fn forse_fine(&mut self, suonati: u64) {
        if self.fine_dichiarata || self.corrente.is_some() || self.prossimo.is_some() {
            return;
        }
        if suonati >= self.spinti && self.resto.is_empty() {
            self.fine_dichiarata = true;
            self.condiviso.in_pausa.store(true, Ordering::Release);
            self.segni.clear();
            // Con la musica finita non c'è più niente da sentire, e il brano
            // messo da parte è solo un file che resta aperto — su Windows, un
            // file che non si può cancellare. Vedi [`Contesto::uscente`].
            self.uscente = None;
            self.annunciato = None;
            self.scrivi_posizione(Posizione::default());
            self.annuncia(Evento::Fermato);
        }
    }

    fn scrivi_posizione(&self, nuova: Posizione) {
        match self.posizione.lock() {
            Ok(mut g) => *g = nuova,
            Err(avvelenato) => *avvelenato.into_inner() = nuova,
        }
    }

    /// Dice a chi sta sopra cos'è successo — se questo motore ha ancora voce.
    ///
    /// L'unico punto da cui gli eventi escono, e la ragione è che un motore
    /// sostituito non deve più parlare: il suo osservatore è vivo e parla allo
    /// stato dell'applicazione, che nel frattempo ha in mano il motore nuovo.
    /// Vedi [`Condiviso::abbandonato`], che [`Motore::drop`] alza prima ancora
    /// di chiedere la chiusura.
    ///
    /// Tacere e non accodare: quel che questo motore aveva da dire riguardava
    /// dei campioni che non usciranno mai dalle casse.
    fn annuncia(&self, evento: Evento) {
        if self.condiviso.abbandonato.load(Ordering::Acquire) {
            return;
        }
        (self.osservatore)(evento);
    }

    /// Il filo della decodifica è caduto: lo dice, e lascia il motore in uno
    /// stato da cui si può ricostruire.
    ///
    /// # Perché `playback.stalled` e non gli altri due candidati
    ///
    /// `playback.decodeFailed` dice all'utente che il file è danneggiato e
    /// conviene sostituirlo: qui non lo sappiamo: un decodificatore che cade su
    /// byte troncati dalla rete parla di un cavo, non di un file da buttare, e
    /// il codice non è ritentabile — cioè niente «Riprova» proprio dove
    /// riprovare è l'unica mossa. `playback.engineUnavailable` è `Fatal`: non
    /// c'è nessun motore audio, chiudi e riapri l'applicazione — ed è falso,
    /// perché il dispositivo è aperto e a un motore nuovo risponderebbe subito.
    ///
    /// `playback.stalled` è invece esattamente questo fatto: la riproduzione si
    /// è impantanata. `Warning`, sempre ritentabile, già nel catalogo e già
    /// tradotto in tutte e due le lingue, con dentro i due campi che servono a
    /// tornare dov'era la puntina.
    ///
    /// # Perché la stessa bandiera del dispositivo perso
    ///
    /// Perché la cura è la stessa — un motore nuovo — e la via che lo
    /// ricostruisce è già scritta: il sorvegliante delle uscite legge `perso`,
    /// annota dove eravamo e riapre. Una seconda bandiera avrebbe voluto dire
    /// una seconda via di riapertura da tenere in pari con la prima.
    ///
    /// # Perché la causa **prima** della bandiera
    ///
    /// Perché il sorvegliante gira su un altro filo e legge le due caselle in
    /// quest'ordine: vista `perso` alta, va a chiedere la causa. Scrivendola
    /// dopo ci sarebbe una finestra — piccola, e larga abbastanza — in cui la
    /// causa è ancora zero, cioè «causa sconosciuta» nel diario e nel banner
    /// proprio nel caso che il banner esiste per raccontare.
    ///
    /// # Perché non tocca `in_pausa`
    ///
    /// Perché `annota_ripresa`, nell'applicazione, legge quel bit **grezzo**
    /// per decidere se la musica dovrà ripartire da sola dopo la riapertura.
    /// Alzarlo qui vorrebbe dire raccontargli che l'utente aveva messo in
    /// pausa, e un brano che stava suonando resterebbe fermo dopo un guasto che
    /// nessuno ha chiesto.
    fn caduto(&mut self, panico: &str) {
        CADUTI.fetch_add(1, Ordering::AcqRel);
        let istantanea = self
            .posizione
            .lock()
            .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g);
        self.condiviso
            .causa_perdita
            .store(CAUSA_FILO_CADUTO, Ordering::Release);
        self.condiviso.perso.store(true, Ordering::Release);
        self.annuncia(Evento::Errore(Box::new(
            AppError::new(ErrorCode::PlaybackStalled {
                track_id: istantanea.track_id,
                position_ms: Some(istantanea.ms),
            })
            .with_cause(format!("il filo della decodifica è caduto: {panico}")),
        )));
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

/// Come [`fotogrammi_da_ms`], ma su un tempo che può essere negativo.
///
/// Serve alla correzione di latenza dichiarata a mano, che ha due versi: si può
/// dire «il suono esce dopo» e «la posizione la racconti tardi». Il segno si
/// porta fuori e si rimette dopo, invece di scrivere una seconda divisione, così
/// la regola di arrotondamento resta una sola.
fn fotogrammi_da_ms_con_segno(ms: i64, frequenza: u32) -> i64 {
    let quanti = fotogrammi_da_ms(ms.unsigned_abs(), frequenza);
    let quanti = i64::try_from(quanti).unwrap_or(i64::MAX);
    if ms < 0 {
        quanti.saturating_neg()
    } else {
        quanti
    }
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

    /// Un brano che dura quel che dice di durare.
    ///
    /// [`brano`] e [`brano_da`] dichiarano sempre un secondo, che è quel che
    /// serve alla maggior parte delle prove. Qui la durata dichiarata segue il
    /// contenuto, perché alle prove dell'anello lungo servono brani di lunghezze
    /// **diverse**: è la differenza fra la durata di un brano e quanto l'anello
    /// tiene da parte a decidere se il brano che si sente e quello che si
    /// decodifica siano lo stesso.
    fn brano_onesto(track_id: i64, frequenza: u32, livello: f32, ms: u64) -> Sorgente {
        Sorgente {
            durata_ms: ms,
            ..brano_da(track_id, frequenza, livello, ms)
        }
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

    /// Apre una sorgente di prova per l'uscita di questo contesto.
    ///
    /// Da quando il motore riceve brani già aperti (vedi [`BranoAperto`]), le
    /// prove devono fare quel che fa l'applicazione: aprire di qua dal confine.
    /// Il `expect` è a posto perché queste sorgenti sono WAV costruiti due
    /// funzioni più su — se non si aprissero, il guasto sarebbe nel banco di
    /// prova e non in quel che si sta provando.
    fn aperto(ctx: &Contesto, sorgente: Sorgente) -> BranoAperto {
        BranoAperto::apri(sorgente, ctx.formato).expect("la sorgente di prova si apre")
    }

    /// Il filo della decodifica senza il filo e senza il dispositivo.
    struct Banco {
        ctx: Contesto,
        consumatore: rtrb::Consumer<f32>,
        /// Le caselle che il contesto condivide con la callback che qui non
        /// c'è: le prove del filo caduto ci leggono `perso` e la causa, quelle
        /// del motore abbandonato ci scrivono la bandiera.
        condiviso: Arc<Condiviso>,
        /// Da tenere vivi: il canale dei comandi e l'anello delle curve non
        /// vengono usati, ma se cadessero il contesto parlerebbe con dei morti.
        _manda: Sender<Comando>,
        _curve: rtrb::Consumer<Coefficienti>,
    }

    fn banco(frequenza: u32) -> Banco {
        banco_con(frequenza, Box::new(|_| {}))
    }

    /// Come [`banco`], ma con un osservatore che si sceglie.
    fn banco_con(frequenza: u32, osservatore: Box<dyn Fn(Evento) + Send>) -> Banco {
        banco_con_anello(frequenza, 16_384, osservatore)
    }

    /// Come sopra, ma l'anello è lungo quanto si vuole.
    ///
    /// # Perché una prova dovrebbe volerlo più lungo
    ///
    /// Perché i 16 384 campioni di [`banco_con`] sono un terzo di secondo a
    /// 48 kHz, mentre l'anello vero ne tiene più di tre — [`crate::uscita`] è
    /// dimensionata sul caso peggiore, 192 kHz a otto canali, e su un'uscita
    /// normale quegli stessi campioni sono secondi di musica. Con un terzo di
    /// secondo il brano che si sente e quello che si decodifica restano quasi
    /// sempre lo stesso; con l'anello vero non lo sono per gli ultimi secondi di
    /// **ogni** brano, ed è lì che vivono i difetti che questo banco serve a
    /// riprodurre.
    fn banco_con_anello(
        frequenza: u32,
        campioni: usize,
        osservatore: Box<dyn Fn(Evento) + Send>,
    ) -> Banco {
        let (manda, ricevi) = std::sync::mpsc::channel();
        let (produttore, consumatore) = rtrb::RingBuffer::<f32>::new(campioni);
        let (curve, prese) = rtrb::RingBuffer::<Coefficienti>::new(32);
        let condiviso = Condiviso::nuovo();
        let ctx = Contesto::nuovo(
            ricevi,
            produttore,
            curve,
            Arc::clone(&condiviso),
            Arc::new(std::sync::Mutex::new(Posizione::default())),
            FormatoUscita {
                frequenza,
                canali: 1,
            },
            osservatore,
        );
        Banco {
            ctx,
            consumatore,
            condiviso,
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

    // ── il racconto, non i campioni ─────────────────────────────────────────
    //
    // Le prove qui sopra guardano la sequenza di campioni che *sarebbe* uscita.
    // Quelle che seguono guardano l'altra metà del mestiere di questo file: cosa
    // il motore **dice** di star suonando, e a che punto. Per vederlo serve la
    // callback, che qui non c'è — e allora la si fa: i campioni che escono
    // dall'anello si contano in `Condiviso::fotogrammi`, esattamente come fa
    // lei, e `aggiorna` traduce quel conteggio in una posizione.
    //
    // Senza quel conteggio la posizione resta a zero per sempre, ed è la ragione
    // per cui nessuna prova di questo file guardava la posizione: il bug del
    // segno di dissolvenza è vissuto per mesi dentro un banco che non poteva
    // vederlo.

    /// Quanti fotogrammi la callback finta consuma per giro.
    ///
    /// Duecentocinquantasei: l'ordine di grandezza di quel che cpal chiede a
    /// 48 kHz. Serve a rendere vera la tolleranza delle prove — «un blocco» è
    /// cinque millisecondi e mezzo, non un margine scelto per far passare.
    const BLOCCO_FINTO: u64 = 256;

    /// Un giro di quel ciclo, visto da fuori.
    struct Passo {
        /// Quel che il motore raccontava in quell'istante.
        posizione: Posizione,
        /// I fotogrammi contati dalla callback finta, grezzi.
        suonati: u64,
        /// Quanti eventi erano stati annunciati fino a lì.
        ///
        /// Un conteggio e non l'elenco: serve a sapere **a quale giro** un
        /// annuncio è arrivato, che è la domanda delle prove sulla latenza. Quale
        /// annuncio sia lo dice la sequenza, che per un brano solo è
        /// `iniziato`, `fermato`.
        eventi: usize,
    }

    /// Fa girare il contesto con la callback finta, e raccoglie il racconto.
    ///
    /// I banchi di queste prove hanno un canale solo — vedi [`banco`] — quindi un
    /// campione è un fotogramma e non serve nessuna conversione.
    fn suona_raccontando(banco: &mut Banco, registro: &Registro) -> Vec<Passo> {
        let mut storia = Vec::new();
        // Lo stesso tetto di [`suona_tutto`], e per la stessa ragione: una prova
        // che non finisce blocca la suite intera.
        for _ in 0..200_000 {
            let lavorato = banco.ctx.riempi();
            let mut presi = 0u64;
            while presi < BLOCCO_FINTO {
                if banco.consumatore.pop().is_err() {
                    break;
                }
                presi = presi.saturating_add(1);
            }
            banco
                .condiviso
                .fotogrammi
                .fetch_add(presi, Ordering::Relaxed);
            banco.ctx.aggiorna();
            let posizione = banco
                .ctx
                .posizione
                .lock()
                .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g);
            storia.push(Passo {
                posizione,
                suonati: banco.condiviso.fotogrammi.load(Ordering::Relaxed),
                eventi: quanti_eventi(registro),
            });
            if !lavorato && presi == 0 && banco.ctx.corrente.is_none() {
                break;
            }
        }
        storia
    }

    /// Il primo istante in cui il motore dice di star suonando questo brano.
    fn primo_annuncio(storia: &[Passo], track_id: i64) -> Option<&Passo> {
        storia
            .iter()
            .find(|p| p.posizione.track_id == Some(track_id))
    }

    #[test]
    fn la_posizione_a_meta_dissolvenza_e_quella_del_brano_che_entra() {
        // Il bug che questa prova chiude, e che valeva tutto il brano: il segno
        // dell'entrante si annota a `spinti + meta`, cioè a un istante in cui
        // l'entrante ha già prodotto `meta` fotogrammi, e dichiarava
        // `offset_ms: 0`. Con quattrocento millisecondi di dissolvenza la
        // posizione riportata partiva da zero invece che da duecento e restava
        // indietro di duecento **fino alla fine del brano**. Con i sei secondi
        // che si possono scegliere dalle impostazioni sono tre secondi.
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
        let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

        let storia = suona_raccontando(&mut banco, &registro);

        let arrivo =
            primo_annuncio(&storia, 2).expect("il secondo brano non è mai stato annunciato");
        // Metà di quattrocento millisecondi, più al più il blocco che la
        // callback finta consuma per giro.
        assert!(
            (190..=230).contains(&arrivo.posizione.ms),
            "il brano che entra si annuncia a {} ms invece di ~200: il segno \
             dice un istante e l'offset un altro",
            arrivo.posizione.ms
        );
    }

    #[test]
    fn un_salto_verso_la_fine_non_porta_la_posizione_oltre_il_brano() {
        // Con la dissolvenza accesa, un salto a cento millisecondi dalla fine
        // fa cominciare una sovrapposizione che dura cento millisecondi e non
        // quattrocento. Il segno dell'entrante stava comunque a metà dei
        // quattrocento: per un decimo di secondo — con i dodici secondi che si
        // possono scegliere, cinque secondi — si sentiva già il brano dopo, e
        // il motore raccontava il primo a 1050, 1100 ms di un brano da mille.
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        banco.ctx.esegui(Comando::Dissolvenza { ms: 400 });
        let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
        banco.ctx.esegui(Comando::VaiA(900));

        let storia = suona_raccontando(&mut banco, &registro);

        let oltre = storia
            .iter()
            .filter(|p| p.posizione.track_id == Some(1))
            .map(|p| p.posizione.ms)
            .max()
            .unwrap_or(0);
        assert!(
            oltre <= 1_010,
            "il primo brano racconta {oltre} ms su mille: il secondo si sente già"
        );
        let arrivo =
            primo_annuncio(&storia, 2).expect("il secondo brano non è mai stato annunciato");
        // Metà della sovrapposizione vera — cento millisecondi — più al più un
        // blocco della callback finta e uno del decodificatore.
        assert!(
            arrivo.posizione.ms <= 90,
            "il brano che entra si annuncia a {} ms: metà di una dissolvenza \
             che non c'è stata",
            arrivo.posizione.ms
        );
    }

    #[test]
    fn il_gapless_resta_a_zero_sul_primo_campione() {
        // Il contrappeso della prova qui sopra, e la ragione per cui i due
        // `offset_ms` di `forse_dissolvi` e `passa_al_prossimo` devono restare
        // diversi: senza dissolvenza il brano nuovo attacca dal suo primo
        // campione, e lì zero è la verità. Chi «uniformasse» le due per simmetria
        // lo scoprirebbe qui.
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

        let storia = suona_raccontando(&mut banco, &registro);

        let arrivo =
            primo_annuncio(&storia, 2).expect("il secondo brano non è mai stato annunciato");
        assert!(
            arrivo.posizione.ms <= 20,
            "il gapless attacca a {} ms invece che da capo",
            arrivo.posizione.ms
        );
    }

    /// Suona un brano con sé stesso preparato come successivo, e dice cosa è
    /// stato annunciato. È quel che fa «ripeti uno», e una coda con lo stesso
    /// brano due volte di fila.
    fn stesso_brano_due_volte(dissolvenza_ms: u64) -> Vec<String> {
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        banco
            .ctx
            .esegui(Comando::Dissolvenza { ms: dissolvenza_ms });
        let primo = aperto(&banco.ctx, brano(7, 48_000, 0.8));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let di_nuovo = aperto(&banco.ctx, brano(7, 48_000, 0.8));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(di_nuovo))));
        let _ = suona_raccontando(&mut banco, &registro);
        eventi(&registro)
    }

    #[test]
    fn lo_stesso_brano_che_ricomincia_si_annuncia_di_nuovo() {
        // Il difetto che chiude: l'annuncio confrontava l'identificativo, il
        // secondo giro dello stesso brano passava muto, chi sta sopra non
        // preparava il terzo e «ripeti uno» si fermava dopo una ripetizione.
        for dissolvenza_ms in [0, 400] {
            let annunci = stesso_brano_due_volte(dissolvenza_ms);
            assert_eq!(
                annunci,
                ["iniziato:7", "iniziato:7", "fermato"],
                "con {dissolvenza_ms} ms di dissolvenza"
            );
        }
    }

    #[test]
    fn un_salto_non_annuncia_di_nuovo_il_brano() {
        // Il contrappeso: un salto dentro lo stesso ascolto non è un inizio, e
        // annunciarlo farebbe ripartire da capo il conteggio dell'ascolto. Vale
        // anche per il salto che arriva prima del primo annuncio: il brano deve
        // essere annunciato comunque, una volta.
        for prima_di_annunciare in [false, true] {
            let (registro, osservatore) = spia();
            let mut banco = banco_con(48_000, osservatore);
            let primo = aperto(&banco.ctx, brano(3, 48_000, 0.8));
            banco.ctx.esegui(Comando::Suona(Box::new(primo)));
            if !prima_di_annunciare {
                banco.ctx.aggiorna();
            }
            banco.ctx.esegui(Comando::VaiA(500));
            let _ = suona_raccontando(&mut banco, &registro);
            assert_eq!(
                eventi(&registro),
                ["iniziato:3", "fermato"],
                "salto {} del primo annuncio",
                if prima_di_annunciare { "prima" } else { "dopo" }
            );
        }
    }

    #[test]
    fn un_salto_negli_ultimi_secondi_non_finisce_nel_brano_dopo() {
        // L'anello tiene da parte molto più di quel che la riserva dichiara —
        // su un'uscita a 48 kHz stereo più di tre secondi, vedi
        // `uscita::RISERVA_MS` — quindi negli ultimi secondi di ogni brano il
        // decodificatore sta già macinando quello dopo, e `corrente` non è più
        // il brano che esce dalle casse.
        //
        // Finché `vai_a` guardava `corrente`, in quella finestra il salto
        // cadeva sul brano **successivo**: si sentiva quello dal punto chiesto,
        // l'interfaccia mostrava ancora il titolo di prima col cursore che
        // correva oltre la fine della barra, e quando quel brano finiva in
        // canna non era rimasto niente — nessun `Iniziato`, nessun `Fermato`,
        // la coda ferma finché qualcuno non premeva un tasto.
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

        // Si gira finché la decodifica è passata al secondo mentre dalle casse
        // esce ancora il primo: è esattamente la finestra del difetto.
        let mut finestra = false;
        for _ in 0..100_000 {
            banco.ctx.riempi();
            let mut presi = 0u64;
            while presi < BLOCCO_FINTO {
                if banco.consumatore.pop().is_err() {
                    break;
                }
                presi = presi.saturating_add(1);
            }
            banco
                .condiviso
                .fotogrammi
                .fetch_add(presi, Ordering::Relaxed);
            banco.ctx.aggiorna();
            let si_decodifica = banco.ctx.corrente.as_ref().map(|d| d.track_id());
            let si_sente = banco.ctx.segni.front().map(|s| s.track_id);
            if si_decodifica == Some(2) && si_sente == Some(1) {
                finestra = true;
                break;
            }
        }
        assert!(
            finestra,
            "il banco non arriva mai a decodificare il secondo mentre si sente il primo"
        );

        banco.ctx.esegui(Comando::VaiA(200));
        let storia = suona_raccontando(&mut banco, &registro);

        assert_eq!(
            eventi(&registro),
            ["iniziato:1", "iniziato:2", "fermato"],
            "la coda non è andata avanti da sola dopo il salto"
        );
        let oltre = storia
            .iter()
            .filter(|p| p.posizione.track_id == Some(1))
            .map(|p| p.posizione.ms)
            .max()
            .unwrap_or(0);
        assert!(
            oltre <= 1_010,
            "il primo brano racconta {oltre} ms su mille: il salto è finito nel brano dopo"
        );
        let tornato = storia
            .iter()
            .any(|p| p.posizione.track_id == Some(1) && (150..=350).contains(&p.posizione.ms));
        assert!(
            tornato,
            "dopo il salto il primo brano non riparte da dove era stato chiesto"
        );
    }

    #[test]
    fn con_l_anello_lungo_il_salto_cade_sul_brano_che_si_sente() {
        // Il caso che l'anello corto di [`banco_con`] non sa mostrare: quando
        // l'anello tiene da parte più di un brano, la decodifica finisce il
        // **secondo** prima che il primo smetta di sentirsi. Da quel momento
        // `uscente` porta il secondo mentre in testa c'è ancora il primo, e chi
        // lo lasciasse cadere al primo `pop_front` — cioè al cambio di brano —
        // butterebbe via proprio il decodificatore che il salto dentro il
        // secondo brano sta per chiedere.
        //
        // Le durate sono scelte per cadere in quella finestra: un anello da un
        // secondo, un primo brano più lungo (così la sua decodifica finisce
        // mentre lo si sente ancora) e un secondo più corto (così anche la sua
        // finisce prima che il primo sia stato udito per intero).
        const ANELLO_MS: u64 = 1_000;
        let (registro, osservatore) = spia();
        let mut banco = banco_con_anello(
            48_000,
            usize::try_from(fotogrammi_da_ms(ANELLO_MS, 48_000)).unwrap_or(48_000),
            osservatore,
        );
        let primo = aperto(&banco.ctx, brano_onesto(1, 48_000, 0.8, 2_000));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        // La coda che resta da consegnare, come farebbe chi sta sopra: uno alla
        // volta, appena la canna si libera.
        let mut resto = vec![
            brano_onesto(3, 48_000, 0.2, 800),
            brano_onesto(2, 48_000, 0.4, 800),
        ];

        let mut saltato = false;
        let mut storia = Vec::new();
        for _ in 0..200_000 {
            if banco.ctx.prossimo.is_none()
                && banco.ctx.corrente.is_some()
                && let Some(sorgente) = resto.pop()
            {
                let aperto = aperto(&banco.ctx, sorgente);
                banco.ctx.esegui(Comando::Prepara(Some(Box::new(aperto))));
            }
            let lavorato = banco.ctx.riempi();
            let mut presi = 0u64;
            while presi < BLOCCO_FINTO {
                if banco.consumatore.pop().is_err() {
                    break;
                }
                presi = presi.saturating_add(1);
            }
            banco
                .condiviso
                .fotogrammi
                .fetch_add(presi, Ordering::Relaxed);
            banco.ctx.aggiorna();
            let posizione = banco
                .ctx
                .posizione
                .lock()
                .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g);
            // Il salto arriva quando si sente il secondo brano ed è già passata
            // alla decodifica del terzo: la finestra descritta qui sopra.
            if !saltato
                && posizione.track_id == Some(2)
                && posizione.ms > 400
                && banco
                    .ctx
                    .corrente
                    .as_ref()
                    .is_some_and(|d| d.track_id() == 3)
            {
                banco.ctx.esegui(Comando::VaiA(100));
                saltato = true;
            }
            storia.push(posizione);
            if !lavorato && presi == 0 && banco.ctx.corrente.is_none() {
                break;
            }
        }

        assert!(saltato, "la finestra del difetto non si è mai aperta");
        assert_eq!(
            eventi(&registro),
            ["iniziato:1", "iniziato:2", "iniziato:3", "fermato"],
            "la coda non è andata avanti da sola dopo il salto"
        );
        // Che sia **tornato** indietro, non che sia passato di lì salendo: il
        // secondo brano attraversa i cento millisecondi anche senza nessun
        // salto, e un'asserzione che non guarda l'ordine passerebbe anche col
        // salto lasciato cadere.
        let dopo_il_salto = storia
            .iter()
            .position(|p| p.track_id == Some(2) && p.ms > 400)
            .map_or(storia.len(), |i| i.saturating_add(1));
        let tornato = storia
            .get(dopo_il_salto..)
            .unwrap_or_default()
            .iter()
            .any(|p| p.track_id == Some(2) && (50..=300).contains(&p.ms));
        assert!(
            tornato,
            "il secondo brano non è mai tornato a ~100 ms: il salto è stato \
             lasciato cadere, o è finito sul brano dopo"
        );
        let oltre = storia
            .iter()
            .filter(|p| p.track_id == Some(2))
            .map(|p| p.ms)
            .max()
            .unwrap_or(0);
        assert!(
            oltre <= 810,
            "il secondo brano racconta {oltre} ms su ottocento: il salto è \
             finito nel brano dopo"
        );
    }

    #[test]
    fn un_preparato_in_ritardo_non_si_porta_via_il_brano_che_si_sente() {
        // L'incrocio fra le due cose che l'anello lungo rende normali.
        //
        // Un brano più corto di quel che l'anello tiene da parte finisce di
        // decodificarsi prima che chi sta sopra abbia consegnato il successivo:
        // `passa_al_prossimo` scatta a mani vuote, lascia `corrente` a niente, e
        // mette da parte il brano che si sta ancora sentendo. Quando il
        // preparato arriva, [`Contesto::prepara`] richiama `passa_al_prossimo`
        // per attaccarlo — ed è lì che il brano messo da parte rischia di
        // sparire, cancellato da un `corrente` che ormai è vuoto.
        //
        // Se sparisce, il salto che arriva dopo non trova più il brano che si
        // sente e si lascia cadere: il cursore torna indietro da solo e chi
        // trascina non capisce perché.
        const ANELLO_MS: u64 = 1_000;
        let (registro, osservatore) = spia();
        let mut banco = banco_con_anello(
            48_000,
            usize::try_from(fotogrammi_da_ms(ANELLO_MS, 48_000)).unwrap_or(48_000),
            osservatore,
        );
        let primo = aperto(&banco.ctx, brano_onesto(1, 48_000, 0.8, 2_000));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));

        // Nessun preparato: si gira finché la decodifica finisce il primo brano
        // e non trova niente da attaccare.
        let mut storia = Vec::new();
        let giro = |banco: &mut Banco, storia: &mut Vec<Posizione>| {
            banco.ctx.riempi();
            let mut presi = 0u64;
            while presi < BLOCCO_FINTO {
                if banco.consumatore.pop().is_err() {
                    break;
                }
                presi = presi.saturating_add(1);
            }
            banco
                .condiviso
                .fotogrammi
                .fetch_add(presi, Ordering::Relaxed);
            banco.ctx.aggiorna();
            let posizione = banco
                .ctx
                .posizione
                .lock()
                .map_or_else(|avvelenato| *avvelenato.into_inner(), |g| *g);
            storia.push(posizione);
        };
        let mut a_mani_vuote = false;
        for _ in 0..100_000 {
            giro(&mut banco, &mut storia);
            if banco.ctx.corrente.is_none() && banco.ctx.prossimo.is_none() {
                a_mani_vuote = true;
                break;
            }
        }
        assert!(
            a_mani_vuote,
            "la decodifica non è mai arrivata in fondo al primo brano senza un \
             successivo da attaccare"
        );

        // Il preparato in ritardo, e subito dopo il salto.
        let secondo = aperto(&banco.ctx, brano_onesto(2, 48_000, 0.4, 800));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
        banco.ctx.esegui(Comando::VaiA(300));

        for _ in 0..200_000 {
            giro(&mut banco, &mut storia);
            if banco.ctx.corrente.is_none() && banco.ctx.prossimo.is_none() {
                break;
            }
        }

        assert_eq!(
            eventi(&registro),
            ["iniziato:1", "iniziato:2", "fermato"],
            "la coda non è andata avanti da sola dopo il salto"
        );
        let dopo_il_salto = storia
            .iter()
            .position(|p| p.track_id == Some(1) && p.ms > 600)
            .map_or(storia.len(), |i| i.saturating_add(1));
        let tornato = storia
            .get(dopo_il_salto..)
            .unwrap_or_default()
            .iter()
            .any(|p| p.track_id == Some(1) && (250..=500).contains(&p.ms));
        assert!(
            tornato,
            "il primo brano non è mai tornato a ~300 ms: il preparato in \
             ritardo si è portato via il brano che si sentiva"
        );
    }

    /// Un brano solo suonato fino in fondo, con le due latenze che si vogliono.
    ///
    /// `misurata` è quel che la callback avrebbe scritto in
    /// `Condiviso::latenza_fotogrammi`; `manuale_ms` è la preferenza
    /// `audio.latenza_ms`. Il brano e il numero di giri sono gli stessi in ogni
    /// chiamata — la decodifica non guarda la latenza — quindi due storie si
    /// possono confrontare **giro per giro**.
    fn un_brano_con_latenza(manuale_ms: i64, misurata: u64) -> Vec<Passo> {
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        banco
            .condiviso
            .latenza_fotogrammi
            .store(misurata, Ordering::Relaxed);
        banco.ctx.esegui(Comando::Latenza { ms: manuale_ms });
        let solo = aperto(&banco.ctx, brano(1, 48_000, 0.5));
        banco.ctx.esegui(Comando::Suona(Box::new(solo)));
        suona_raccontando(&mut banco, &registro)
    }

    /// Il giro di mezzo, dove il brano sta suonando e nessun estremo disturba.
    #[expect(
        clippy::integer_division,
        reason = "metà di un elenco di giri: mezzo giro non esiste, e quale dei \
                  due vicini si prenda non cambia niente"
    )]
    fn a_meta(storia: &[Passo]) -> &Passo {
        storia
            .get(storia.len() / 2)
            .expect("una storia vuota vuol dire che il banco non ha suonato")
    }

    #[test]
    fn la_latenza_dichiarata_sposta_indietro_la_posizione() {
        let senza = un_brano_con_latenza(0, 0);
        let con = un_brano_con_latenza(100, 0);
        assert_eq!(
            senza.len(),
            con.len(),
            "la latenza ha cambiato la decodifica: non deve toccare i campioni"
        );

        let qui = a_meta(&senza);
        let la = a_meta(&con);
        assert_eq!(
            qui.suonati, la.suonati,
            "i due giri non sono lo stesso giro"
        );
        let scarto = i64::try_from(qui.posizione.ms).unwrap_or(0)
            - i64::try_from(la.posizione.ms).unwrap_or(0);
        assert!(
            (90..=110).contains(&scarto),
            "cento millisecondi di latenza spostano la posizione di {scarto}"
        );
    }

    #[test]
    fn la_latenza_non_ritarda_la_fine() {
        // La fine si dichiara quando l'ultimo campione ha lasciato **l'anello
        // nostro**, non le casse: ritardarla della latenza vorrebbe dire
        // ritardare di altrettanto il brano dopo, cioè un buco fra due tracce di
        // un album gapless lungo esattamente la compensazione.
        //
        // Per un brano solo gli annunci sono due, in quest'ordine: `iniziato`,
        // `fermato`. Il giro in cui il conteggio arriva a due è il giro del
        // `Fermato`, e i fotogrammi contati lì devono essere gli stessi con e
        // senza latenza.
        let senza = un_brano_con_latenza(0, 0);
        let con = un_brano_con_latenza(500, 0);

        let fine = |storia: &[Passo]| {
            storia
                .iter()
                .find(|p| p.eventi >= 2)
                .map(|p| p.suonati)
                .expect("il brano non si è mai dichiarato finito")
        };
        assert_eq!(
            fine(&senza),
            fine(&con),
            "mezzo secondo di latenza ha spostato la fine del brano: il gapless \
             guadagnerebbe un buco lungo altrettanto"
        );
    }

    #[test]
    fn una_latenza_assurda_si_ignora() {
        // Quel che `cpal` riporta è la durata del buffer del dispositivo: decine
        // di millisecondi. Un secondo lì dentro non è un'uscita lenta, è un
        // orologio che ha risposto una sciocchezza — e compensarlo sposterebbe
        // cursore e testi di un secondo senza che nessuno possa capire perché.
        let senza = un_brano_con_latenza(0, 0);
        let assurda = un_brano_con_latenza(0, fotogrammi_da_ms(1_000, 48_000));
        assert_eq!(
            a_meta(&senza).posizione.ms,
            a_meta(&assurda).posizione.ms,
            "una latenza oltre il tetto è stata compensata lo stesso"
        );

        // E l'altra metà della stessa decisione: sotto il tetto la misura si usa,
        // altrimenti questa prova passerebbe anche con una misura ignorata sempre.
        let buona = un_brano_con_latenza(0, fotogrammi_da_ms(100, 48_000));
        let scarto = i64::try_from(a_meta(&senza).posizione.ms).unwrap_or(0)
            - i64::try_from(a_meta(&buona).posizione.ms).unwrap_or(0);
        assert!(
            (90..=110).contains(&scarto),
            "cento millisecondi misurati spostano la posizione di {scarto}"
        );
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
        let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let secondo = aperto(&banco.ctx, brano(2, 24_000, 0.4));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

        let fuori = suona_tutto(&mut banco, |ctx| {
            let terzo = aperto(ctx, brano(3, 48_000, 0.1));
            ctx.esegui(Comando::Prepara(Some(Box::new(terzo))));
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
        let primo = aperto(&banco.ctx, brano(1, 48_000, 0.8));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let secondo = aperto(&banco.ctx, brano(2, 24_000, 0.4));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

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
        let primo = aperto(&banco.ctx, brano_da(1, 48_000, 0.8, 800));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

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
    fn un_brano_piu_corto_dell_anello_non_pianta_la_riproduzione() {
        // Il bug che questa prova chiude, ed era una riproduzione che moriva:
        // un intermezzo di due secondi viene decodificato **per intero** nei
        // primi millisecondi, perché davanti alla callback c'è posto per più di
        // tre secondi di musica. Quando `passa_al_prossimo` scatta, il
        // preparatore — che parte dall'annuncio di `Iniziato`, aspetta
        // l'antirimbalzo della raffica e poi apre un file — non ha ancora
        // consegnato niente: il corrente si azzera, e il successivo arriva un
        // istante dopo, quando nessuno lo guarda più.
        //
        // Da lì non si usciva: `decodifica_un_blocco` esce subito senza un
        // corrente, quindi `passa_al_prossimo` non veniva più chiamato;
        // `forse_fine` non dichiara la fine con qualcosa in canna, quindi non
        // arrivava nemmeno un `Fermato`. Nessun evento, coda ferma, cursore
        // piantato sull'ultimo millisecondo.
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        // Duecento millisecondi a 48 kHz su un canale sono novemilaseicento
        // campioni, e l'anello del banco ne tiene sedicimilatrecentottantaquattro:
        // il brano ci sta tutto, che è la condizione da riprodurre.
        let primo = aperto(&banco.ctx, brano_da(1, 48_000, 0.8, 200));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        // Nessuno svuota l'anello: la decodifica corre fino in fondo al file
        // mentre dalle casse non è ancora uscito un campione.
        for _ in 0..100 {
            banco.ctx.riempi();
            banco.ctx.aggiorna();
        }
        assert!(
            banco.ctx.corrente.is_none(),
            "il banco non riproduce la corsa: il decodificatore non è arrivato in fondo"
        );

        // E adesso il preparatore, in ritardo di un soffio.
        let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.4));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));
        assert!(
            banco.ctx.corrente.is_some(),
            "il successivo resta in canna e il motore non suona più niente"
        );

        // Da qui il racconto torna quello di sempre: i due brani in ordine, e la
        // fine dichiarata una volta sola quando finiscono davvero.
        let storia = suona_raccontando(&mut banco, &registro);
        assert_eq!(eventi(&registro), ["iniziato:1", "iniziato:2", "fermato"]);
        // Il secondo si sente, e si sente al livello suo: attaccarlo senza buco
        // è metà del punto, e l'altra metà è che ci arrivi tutto.
        let suo = storia
            .iter()
            .filter(|p| p.posizione.track_id == Some(2))
            .count();
        assert!(suo > 0, "il secondo brano non si annuncia mai");
    }

    // ── la condivisione che muore a metà brano ──────────────────────────────

    /// Byte che a un certo punto smettono di rispondere.
    ///
    /// I primi `soglia` byte arrivano interi, poi ogni lettura restituisce il
    /// numero di Windows che si è scelto. Con il 64 —
    /// `ERROR_NETNAME_DELETED` — è una condivisione che è morta a metà brano:
    /// il modo in cui una VPN che cade o un NAS spento si presentano, e
    /// l'unico modo di riprodurre quel guasto senza un NAS vero. Con un numero
    /// qualunque che non sia di rete è invece un settore illeggibile del disco
    /// di casa. Le due prove che seguono si distinguono **solo** per quel
    /// numero, che è esattamente la distinzione che il motore deve saper fare.
    struct ByteCheFalliscono {
        dentro: std::io::Cursor<Vec<u8>>,
        soglia: u64,
        letti: u64,
        numero: i32,
    }

    impl std::io::Read for ByteCheFalliscono {
        fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
            if self.letti >= self.soglia {
                return Err(std::io::Error::from_raw_os_error(self.numero));
            }
            let quanti = std::io::Read::read(&mut self.dentro, dove)?;
            self.letti = self
                .letti
                .saturating_add(u64::try_from(quanti).unwrap_or(0));
            Ok(quanti)
        }
    }

    impl std::io::Seek for ByteCheFalliscono {
        fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
            std::io::Seek::seek(&mut self.dentro, da)
        }
    }

    impl crate::decodifica::Flusso for ByteCheFalliscono {
        fn lunghezza(&self) -> Option<u64> {
            // Il file **dichiara** la sua lunghezza vera: la share è morta dopo
            // che il sistema l'aveva già detta, ed è proprio questo a rendere
            // il guasto invisibile finché non si legge.
            u64::try_from(self.dentro.get_ref().len()).ok()
        }
    }

    /// Un brano che si interrompe a metà con il numero d'errore dato.
    ///
    /// La soglia sta a metà dei byte perché symphonia deve fare in tempo a
    /// riconoscere il WAV e a leggerne qualche pacchetto: un guasto che
    /// arrivasse subito verrebbe scambiato per un file di formato ignoto, che è
    /// un'altra strada e un altro errore, e la prova non guarderebbe più il
    /// punto che deve guardare.
    fn brano_che_si_interrompe(track_id: i64, frequenza: u32, numero: i32) -> Sorgente {
        let quanti = fotogrammi_da_ms(1_000, frequenza);
        let campioni = vec![0.5f32; usize::try_from(quanti).unwrap_or(48_000)];
        let byte = wav(frequenza, &campioni);
        let soglia = u64::try_from(byte.len().div_ceil(2)).unwrap_or(0);
        Sorgente {
            track_id,
            media: Box::new(ByteCheFalliscono {
                dentro: std::io::Cursor::new(byte),
                soglia,
                letti: 0,
                numero,
            }),
            estensione: Some("wav".to_owned()),
            durata_ms: 1_000,
            replaygain_db: None,
        }
    }

    /// La condivisione che sparisce: `ERROR_NETNAME_DELETED`.
    fn brano_che_muore(track_id: i64, frequenza: u32) -> Sorgente {
        brano_che_si_interrompe(track_id, frequenza, 64)
    }

    /// Il file davvero rotto sul disco di casa: `ERROR_CRC`, un settore che non
    /// si legge più. Aspettare non lo aggiusta, e infatti va saltato.
    fn brano_rotto(track_id: i64, frequenza: u32) -> Sorgente {
        brano_che_si_interrompe(track_id, frequenza, 23)
    }

    /// Gli eventi annunciati, in ordine, riletti dalla prova che li ha chiesti.
    type Registro = Arc<std::sync::Mutex<Vec<String>>>;

    /// La stessa scatola che il motore vero riceve dall'applicazione.
    type Osservatore = Box<dyn Fn(Evento) + Send>;

    /// Raccoglie gli eventi che il motore annuncia, per poterli leggere dopo.
    fn spia() -> (Registro, Osservatore) {
        let registro = Arc::new(std::sync::Mutex::new(Vec::new()));
        let dentro = Arc::clone(&registro);
        let osservatore: Osservatore = Box::new(move |evento| {
            let riga = match evento {
                Evento::Iniziato { track_id } => format!("iniziato:{track_id}"),
                Evento::Fermato => "fermato".to_owned(),
                Evento::Errore(err) => format!(
                    "errore:{}:{}",
                    err.code().kind().code(),
                    if err.is_retryable() {
                        "riprovabile"
                    } else {
                        "definitivo"
                    }
                ),
            };
            if let Ok(mut elenco) = dentro.lock() {
                elenco.push(riga);
            }
        });
        (registro, osservatore)
    }

    fn eventi(registro: &Registro) -> Vec<String> {
        registro.lock().map(|e| e.clone()).unwrap_or_default()
    }

    /// Quanti annunci sono arrivati, senza copiarli.
    ///
    /// [`eventi`] clona l'elenco, che va benissimo a fine prova e non va bene
    /// dentro un ciclo che gira qualche migliaio di volte: qui serve solo il
    /// numero, e copiare delle stringhe per contarle sarebbe il genere di spreco
    /// che rende lente le suite.
    fn quanti_eventi(registro: &Registro) -> usize {
        registro.lock().map(|e| e.len()).unwrap_or(0)
    }

    // ── il FLAC vero ──────────────────────────────────────────────────────

    /// Mezzo secondo a 44100, mono, a livello costante.
    ///
    /// # Perché un file e non dei byte costruiti qui
    ///
    /// Perché fino a ieri nessuna prova di questo crate decodificava un FLAC, e
    /// il FLAC è il formato su cui pesano le due cose che questo giro ha
    /// toccato: il riconoscimento del contenitore, che si porta dietro tutti i
    /// blocchi di metadati, e il salto, che passa per la seek table. Costruire
    /// un FLAC in memoria vorrebbe dire scrivere un codificatore — cioè provare
    /// il proprio codificatore contro il decodificatore di symphonia, che non
    /// dice niente su nessuno dei due.
    ///
    /// Sono centocinquantaquattro byte: un livello costante si comprime quasi a
    /// niente, ed è quel che serve a un campione che deve stare in un
    /// repository.
    ///
    /// 44100 e non 48000 apposta: così passa anche dal ricampionatore, che è la
    /// strada di tutti i file veri di questa libreria.
    const FLAC_COSTANTE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/campioni/costante.flac"
    ));

    fn flac(track_id: i64) -> Sorgente {
        Sorgente {
            track_id,
            media: Box::new(Byte(std::io::Cursor::new(FLAC_COSTANTE.to_vec()))),
            estensione: Some("flac".to_owned()),
            durata_ms: 500,
            replaygain_db: None,
        }
    }

    /// Tutti i campioni che un decodificatore consegna, fino alla fine.
    fn tutto(decodificatore: &mut crate::decodifica::Decodificatore) -> Vec<f32> {
        let mut fuori = Vec::new();
        let mut blocco = Vec::new();
        while decodificatore.prossimo(&mut blocco).expect("decodifica") {
            fuori.extend(blocco.iter().copied());
        }
        fuori
    }

    #[test]
    fn un_flac_vero_si_decodifica_e_si_ricampiona() {
        let mut decodificatore =
            crate::decodifica::Decodificatore::apri(flac(1), 48_000, 1).expect("il FLAC si apre");

        let campioni = tutto(&mut decodificatore);

        // Mezzo secondo a 44100 riportato a 48000 fa ventiquattromila
        // fotogrammi. Il ricampionatore lavora a blocchi e può consegnarne
        // qualcuno in meno o in più agli estremi: la tolleranza copre quello,
        // non un errore di frequenza — che sarebbe di migliaia di campioni.
        let attesi = 24_000_i64;
        let scarto = (i64::try_from(campioni.len()).unwrap_or(0) - attesi).abs();
        assert!(
            scarto < 500,
            "attesi ~{attesi} campioni a 48 kHz, ne sono arrivati {}",
            campioni.len()
        );

        // E il suono è quello che c'era dentro: un livello costante resta
        // costante, salvo le code del ricampionatore ai due estremi.
        let dentro = &campioni[1_000..campioni.len().saturating_sub(1_000)];
        assert!(
            quanti_a(dentro, 0.5) * 10 > dentro.len() * 9,
            "il livello non è quello inciso nel file"
        );
    }

    #[test]
    fn un_flac_vero_si_puo_saltare() {
        // Il salto su FLAC è la strada che [`Decodificatore::cerca`] percorre
        // davvero, ed è quella che nessun'altra prova di questo crate tocca: sul
        // WAV il punto si calcola, qui si cerca fra i blocchi.
        let intero = {
            let mut d = crate::decodifica::Decodificatore::apri(flac(1), 48_000, 1)
                .expect("il FLAC si apre");
            tutto(&mut d).len()
        };

        let mut decodificatore =
            crate::decodifica::Decodificatore::apri(flac(1), 48_000, 1).expect("il FLAC si apre");
        decodificatore.cerca(250).expect("il salto riesce");
        let dopo = tutto(&mut decodificatore).len();

        // Si confronta con il decodificato intero, non con un numero scritto a
        // mano, e la tolleranza è larga per una ragione precisa: **non si
        // atterra a metà esatta**. Questo campione non porta una `SEEKTABLE` —
        // è troppo corto perché il codificatore ne scriva una — quindi
        // symphonia si ferma al confine del blocco FLAC più vicino, che sono
        // 4096 fotogrammi alla volta. Su mezzo secondo la granularità si vede,
        // ed è il comportamento giusto: quel che conta è che il salto abbia
        // spostato la puntina e non l'abbia buttata fuori dal brano.
        assert!(
            dopo < intero,
            "dopo un salto in avanti deve uscire meno musica: {dopo} contro {intero}"
        );
        assert!(
            dopo * 4 > intero,
            "il salto ha portato quasi alla fine del brano: {dopo} contro {intero}"
        );
    }

    #[test]
    fn un_salto_oltre_la_fine_non_dice_che_il_file_e_danneggiato() {
        // Trascinare il cursore fino in fondo. Prima diceva che il file era
        // danneggiato: symphonia rifiuta con `SeekError(OutOfRange)` appena
        // `ts > n_frames`, il ripiego lo traduceva in `playback.decodeFailed`,
        // e il catalogo dichiara quel codice `Warning, Never` ritentabile —
        // cioè all'utente arrivava «questo file è danneggiato, sostituiscilo».
        // Su un FLAC succedeva **sempre**, perché il cursore arriva in fondo
        // ogni volta che qualcuno ce lo trascina.
        let mut decodificatore =
            crate::decodifica::Decodificatore::apri(flac(1), 48_000, 1).expect("il FLAC si apre");

        // Cinque secondi oltre la fine dichiarata: nessun dubbio che sia fuori.
        decodificatore
            .cerca(500 + 5_000)
            .expect("saltare oltre la fine non è un guasto");

        // Il tetto di `cerca` ferma il salto poco prima dell'ultimo fotogramma,
        // quindi quel che resta è la manciata di millisecondi del margine: si
        // consuma, e deve finire da sé.
        let resto = tutto(&mut decodificatore);
        assert!(
            // Ventiquattromila campioni a 48 kHz sono il mezzo secondo che il
            // file contiene per intero: restarne così tanti vorrebbe dire che
            // il salto non è avvenuto.
            resto.len() < 24_000,
            "dopo un salto in fondo non deve restare mezzo secondo di musica: {}",
            resto.len()
        );

        // E il giro dopo il brano è finito, senza errore: è così che la coda
        // avanza al brano successivo invece di fermarsi con un avviso.
        let mut blocco = Vec::new();
        assert!(
            !decodificatore
                .prossimo(&mut blocco)
                .expect("dopo la fine non c'è niente da leggere, e non è un guasto"),
            "il brano doveva dichiararsi finito"
        );
    }

    #[test]
    fn un_flac_su_una_share_che_muore_non_diventa_un_formato_ignoto() {
        // Il caso vero, e il più caro proprio su FLAC: il riconoscimento del
        // contenitore legge **tutti** i blocchi di metadati prima di poter dire
        // di che formato si tratta — su un disco con la copertina incorporata
        // sono centinaia di kilobyte, tutti dalla rete. Se la share cade lì in
        // mezzo, symphonia dice soltanto «nessun lettore adatto» e butta via il
        // numero del sistema: senza il testimone in `decodifica.rs` l'utente si
        // sentirebbe dire che il suo FLAC è di un formato che Aether non sa
        // leggere.
        let sorgente = Sorgente {
            track_id: 1,
            media: Box::new(ByteCheFalliscono {
                // Solo i primi quaranta byte esistono: l'intestazione `fLaC` e
                // l'inizio dello `STREAMINFO`. Il resto è la share che se n'è
                // andata mentre si leggevano i blocchi di metadati — che sul
                // FLAC sono la parte lunga della lettura.
                dentro: std::io::Cursor::new(FLAC_COSTANTE[..40].to_vec()),
                soglia: 40,
                letti: 0,
                numero: 64,
            }),
            estensione: Some("flac".to_owned()),
            durata_ms: 500,
            replaygain_db: None,
        };

        let Err(err) = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1) else {
            panic!("con la rete giù a metà intestazione non si apre niente");
        };

        assert_eq!(
            err.code().kind(),
            ErrorCodeKind::FsNetworkUnavailable,
            "un FLAC su share morta deve dire «rete» (causa: {:?})",
            err.cause()
        );
        assert!(err.code().is_retryable(), "la rete torna: si ritenta");
    }

    // ── l'Opus vero ────────────────────────────────────────────────────────

    /// Tre secondi di seno a 1 kHz, mono, ad ampiezza 0,5, a 16 kbit/s.
    ///
    /// # Perché un tono e non un livello costante come il FLAC
    ///
    /// Perché Opus la continua non la trasporta: SILK e CELT tolgono il DC
    /// tutti e due, e un livello fisso si ricostruirebbe come quasi-silenzio.
    /// Una prova che ci misurasse sopra un'ampiezza sarebbe verde per il motivo
    /// sbagliato — verde su un decodificatore rotto quanto su uno giusto. Un
    /// seno invece sopravvive, e il suo valore efficace è un numero che si può
    /// confrontare con quello che ne ricava l'implementazione di riferimento.
    ///
    /// # Perché un file inciso da libopus
    ///
    /// Perché è l'unico modo di provare qualcosa sul **nostro** decodificatore.
    /// `opus-decoder` è Rust puro e dichiara di passare i dodici vettori di
    /// RFC 8251, ma quei vettori non viaggiano col crate: qui il campione lo ha
    /// scritto l'implementazione di riferimento, quindi se il porto in Rust
    /// sbagliasse in modo grossolano questa prova diventerebbe rossa.
    /// Costruire i pacchetti a mano vorrebbe dire provare il proprio
    /// codificatore contro il proprio decodificatore, che non dice niente su
    /// nessuno dei due.
    ///
    /// # Perché tre secondi, che per un campione sono tanti
    ///
    /// Perché sotto non ci sono abbastanza **pagine Ogg**, e senza pagine il
    /// gapless non si esercita. Mezzo secondo di tono entra tutto in una pagina
    /// sola, e con una pagina sola symphonia non riesce a distinguere il
    /// silenzio di testa da quello di coda: attribuisce l'imbottitura finale al
    /// pre-skip, non taglia niente, e la prova finirebbe per misurare un caso
    /// che a un file vero non capita. A tre secondi le pagine audio sono
    /// quattro, `trim_start` e `trim_end` arrivano davvero a `decodifica::opus`,
    /// e quel che si prova è la strada che percorre la musica di qualcuno.
    ///
    /// Sedici kilobit al secondo per tenerlo sotto i diecimila byte: un tono
    /// puro si comprime bene, ed è quel che serve a un campione che deve stare
    /// in un repository.
    ///
    /// 48000 e non 44100 come il FLAC, e non è una scelta: Opus decodifica
    /// sempre a 48 kHz. Il ricampionatore, che il FLAC esercita, qui resta
    /// fuori dai piedi apposta — quel che si prova è il codec.
    const OPUS_TONO: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/campioni/tono.opus"
    ));

    /// Quanti fotogrammi dura [`OPUS_TONO`] una volta tolti pre-skip e coda.
    const OPUS_FOTOGRAMMI: i64 = 144_000;

    /// Il valore efficace che libopus ricava da [`OPUS_TONO`].
    ///
    /// Misurato decodificando quel file con l'implementazione di riferimento e
    /// scartando cinquanta millesimi ai due estremi, dove il tono attacca e
    /// smette. È il numero contro cui si confronta il nostro decodificatore.
    const OPUS_EFFICACE: f32 = 0.3519;

    fn opus(track_id: i64, estensione: &str) -> Sorgente {
        Sorgente {
            track_id,
            media: Box::new(Byte(std::io::Cursor::new(OPUS_TONO.to_vec()))),
            estensione: Some(estensione.to_owned()),
            durata_ms: 3_000,
            replaygain_db: None,
        }
    }

    /// Il valore efficace di un blocco di campioni.
    fn efficace(campioni: &[f32]) -> f32 {
        if campioni.is_empty() {
            return 0.0;
        }
        let somma: f32 = campioni.iter().map(|c| c * c).sum();
        (somma / campioni.len() as f32).sqrt()
    }

    /// Il tono, misurato al netto degli estremi.
    ///
    /// Cinquemila campioni via da una parte e dall'altra: all'inizio c'è
    /// l'attacco, in fondo la coda, e dopo un salto c'è anche il tratto in cui
    /// un codec con stato si riallinea. Nessuno dei tre dice niente sul livello
    /// del tono, che è quel che queste prove misurano.
    fn tono_di(campioni: &[f32]) -> f32 {
        if campioni.len() <= 10_000 {
            return 0.0;
        }
        efficace(&campioni[5_000..campioni.len() - 5_000])
    }

    #[test]
    fn un_opus_vero_si_decodifica_e_suona_il_tono_inciso() {
        let mut decodificatore =
            crate::decodifica::Decodificatore::apri(opus(1, "opus"), 48_000, 1)
                .expect("l'Opus si apre");

        let campioni = tutto(&mut decodificatore);

        // Tre secondi a 48000 fanno centoquarantaquattromila fotogrammi, ed è
        // un'uguaglianza **esatta**, non una tolleranza: non c'è ricampionatore
        // di mezzo, e i due tagli sono l'uno il complemento dell'altro.
        //
        // È anche l'unica asserzione che li prova. Senza il taglio di coda ne
        // arriverebbero 144648; senza quello di testa, 144312 — che è quel che
        // arrivava davvero prima che `decodifica::opus` si contasse il pre-skip
        // da sé, e che una tolleranza larga avrebbe lasciato passare. Un numero
        // diverso da questo dice quale delle due mani ha smesso di lavorare.
        assert_eq!(
            i64::try_from(campioni.len()).unwrap_or(0),
            OPUS_FOTOGRAMMI,
            "tre secondi a 48 kHz sono {OPUS_FOTOGRAMMI} fotogrammi tondi"
        );

        // E il tono è quello inciso. La tolleranza è larga perché un codec con
        // perdita non restituisce i campioni di partenza; è stretta abbastanza
        // da accorgersi di un guadagno applicato male, di un canale scambiato o
        // di rumore al posto della musica.
        let tono = tono_di(&campioni);
        assert!(
            (tono - OPUS_EFFICACE).abs() < 0.03,
            "il valore efficace dice {tono}, libopus su questo file dice {OPUS_EFFICACE}"
        );
        // E non è un livello continuo travestito da tono: un seno ha un picco
        // che sta una spanna sopra il suo valore efficace, e sotto l'unità.
        let picco = campioni.iter().fold(0.0_f32, |max, c| max.max(c.abs()));
        assert!(
            picco > tono && picco < 1.0,
            "picco {picco} contro valore efficace {tono}: non è la forma di un seno"
        );
    }

    #[test]
    fn un_opus_vero_si_puo_saltare() {
        // Il salto su Opus merita una prova sua: è un codec con stato — ogni
        // pacchetto continua il precedente — quindi dopo aver spostato la
        // puntina il decodificatore va azzerato, o quel che esce sono i resti
        // della finestra di prima. È `Decoder::reset`, che `cerca` chiama e che
        // qui si verifica che serva a qualcosa.
        let intero = {
            let mut d = crate::decodifica::Decodificatore::apri(opus(1, "opus"), 48_000, 1)
                .expect("l'Opus si apre");
            tutto(&mut d).len()
        };

        let mut decodificatore =
            crate::decodifica::Decodificatore::apri(opus(1, "opus"), 48_000, 1)
                .expect("l'Opus si apre");
        decodificatore.cerca(1_500).expect("il salto riesce");
        let dopo = tutto(&mut decodificatore);

        assert!(
            dopo.len() < intero,
            "dopo un salto in avanti deve uscire meno musica: {} contro {intero}",
            dopo.len()
        );
        // E quel che esce dopo il salto è ancora il tono, non il silenzio né il
        // rumore che darebbe uno stato non azzerato.
        let tono = tono_di(&dopo);
        assert!(
            (tono - OPUS_EFFICACE).abs() < 0.03,
            "dopo il salto il tono dovrebbe continuare: valore efficace {tono}"
        );
    }

    #[test]
    fn un_opus_dentro_un_ogg_si_suona_lo_stesso() {
        // Il caso che prima del decodificatore cadeva nel punto peggiore:
        // `.ogg` era già fra le estensioni suonabili, quindi il file passava il
        // cancello delle estensioni, il demultiplatore lo apriva, riconosceva
        // il flusso Opus — e poi `make()` non trovava un decodificatore, e
        // all'utente arrivava «formato non supportato» per un contenitore che
        // l'app apre benissimo.
        //
        // Sono gli stessi byte: quel che cambia è solo l'estensione dichiarata,
        // cioè il suggerimento che arriva al riconoscitore. Dentro un Ogg il
        // codec lo dice la pagina, non il nome del file.
        let mut decodificatore = crate::decodifica::Decodificatore::apri(opus(1, "ogg"), 48_000, 1)
            .expect("un Ogg che dentro è Opus si apre");

        let campioni = tutto(&mut decodificatore);
        let tono = tono_di(&campioni);
        assert!(
            (tono - OPUS_EFFICACE).abs() < 0.03,
            "il tono dentro l'Ogg dovrebbe essere lo stesso: valore efficace {tono}"
        );
    }

    /// Byte che smettono di rispondere **quando lo si decide**.
    ///
    /// A differenza di [`ByteCheFalliscono`], che cade dopo un tot di byte, qui
    /// l'interruttore è in mano alla prova: serve a far morire la share in un
    /// punto preciso della vita del decodificatore — fra l'apertura e il salto,
    /// per esempio, che non è un momento riproducibile contando i byte letti.
    ///
    /// Fallisce anche il riposizionamento, e non è un dettaglio: un salto su una
    /// condivisione morta è proprio una `SetFilePointer` che non torna.
    struct ByteCheMuoiono {
        dentro: std::io::Cursor<Vec<u8>>,
        morta: Arc<std::sync::atomic::AtomicBool>,
        numero: i32,
    }

    impl ByteCheMuoiono {
        fn e_morta(&self) -> bool {
            self.morta.load(Ordering::Relaxed)
        }
    }

    impl std::io::Read for ByteCheMuoiono {
        fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
            if self.e_morta() {
                return Err(std::io::Error::from_raw_os_error(self.numero));
            }
            std::io::Read::read(&mut self.dentro, dove)
        }
    }

    impl std::io::Seek for ByteCheMuoiono {
        fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
            if self.e_morta() {
                return Err(std::io::Error::from_raw_os_error(self.numero));
            }
            std::io::Seek::seek(&mut self.dentro, da)
        }
    }

    impl crate::decodifica::Flusso for ByteCheMuoiono {
        fn lunghezza(&self) -> Option<u64> {
            // Dichiarata sempre, anche da morta: il sistema l'aveva già detta
            // quando la share rispondeva ancora, ed è proprio questo a rendere
            // il flusso «saltabile» agli occhi di symphonia.
            u64::try_from(self.dentro.get_ref().len()).ok()
        }
    }

    /// Un brano con l'interruttore della rete, e l'interruttore.
    fn brano_con_interruttore(
        track_id: i64,
        frequenza: u32,
        numero: i32,
    ) -> (Sorgente, Arc<std::sync::atomic::AtomicBool>) {
        let quanti = fotogrammi_da_ms(2_000, frequenza);
        let campioni = vec![0.5f32; usize::try_from(quanti).unwrap_or(96_000)];
        let morta = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sorgente = Sorgente {
            track_id,
            media: Box::new(ByteCheMuoiono {
                dentro: std::io::Cursor::new(wav(frequenza, &campioni)),
                morta: Arc::clone(&morta),
                numero,
            }),
            estensione: Some("wav".to_owned()),
            durata_ms: 2_000,
            replaygain_db: None,
        };
        (sorgente, morta)
    }

    #[test]
    fn una_share_morta_all_apertura_non_dice_che_il_formato_e_ignoto() {
        // La rete cade fra la `File::open` e il riconoscimento del contenitore.
        // symphonia non riesce a leggere nemmeno l'intestazione, e il guasto
        // usciva come `playback.formatUnsupported` — che il catalogo dichiara
        // mai ritentabile, cioè niente «Riprova» per un file che non ha niente
        // che non va. Il percorso più caro è proprio quello del FLAC, dove il
        // riconoscimento legge anche la copertina incorporata prima di dire di
        // che formato si tratta.
        let (sorgente, morta) = brano_con_interruttore(1, 48_000, 64);
        morta.store(true, Ordering::Relaxed);

        let Err(err) = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1) else {
            panic!("con la rete giù non si apre niente");
        };

        assert_eq!(
            err.code().kind(),
            ErrorCodeKind::FsNetworkUnavailable,
            "la share morta all'apertura deve dire «rete», non «formato» (causa: {:?})",
            err.cause()
        );
        assert!(err.code().is_retryable(), "la rete torna: si ritenta");
    }

    #[test]
    fn un_contenitore_davvero_ignoto_resta_un_formato_non_supportato() {
        // Il contrappeso della prova qui sopra: il ramo di rete non deve
        // inghiottire il caso normale. Dei byte che non sono un formato audio
        // restano un formato che non si sa suonare, e riprovare non serve.
        let sorgente = Sorgente {
            track_id: 1,
            media: Box::new(Byte(std::io::Cursor::new(vec![0u8; 4_096]))),
            estensione: Some("wav".to_owned()),
            durata_ms: 0,
            replaygain_db: None,
        };

        let Err(err) = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1) else {
            panic!("dei byte a zero non sono un file audio");
        };

        assert_eq!(
            err.code().kind(),
            ErrorCodeKind::PlaybackFormatUnsupported,
            "senza rete di mezzo il codice resta quello del formato"
        );
    }

    #[test]
    fn una_share_che_muore_durante_un_salto_non_dice_che_il_file_e_rotto() {
        // Il buco più stretto e il più insidioso: il brano si è aperto quando la
        // rete c'era, si sente, e la share muore mentre qualcuno trascina il
        // cursore. Il salto è il momento peggiore in cui trovarsela morta —
        // `SeekMode::Accurate` va al punto e poi ridecodifica fino al fotogramma
        // esatto, e su FLAC ci mette in mezzo pure la seek table — e finché il
        // guasto usciva come `playback.decodeFailed`, l'utente si sentiva dire
        // che il file era danneggiato mentre gli mancava solo il cavo.
        let (sorgente, morta) = brano_con_interruttore(1, 48_000, 64);
        let mut decodificatore = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1)
            .expect("con la rete viva si apre");

        morta.store(true, Ordering::Relaxed);
        let err = decodificatore
            .cerca(1_000)
            .expect_err("su una share morta il salto non riesce");

        assert_eq!(
            err.code().kind(),
            ErrorCodeKind::FsNetworkUnavailable,
            "un salto a rete giù deve dire «rete», non «file danneggiato»"
        );
        assert!(
            err.code().is_retryable(),
            "senza questo la finestra non offre «Riprova» e non si annota dove tornare"
        );
    }

    #[test]
    fn un_salto_fallito_su_un_disco_rotto_resta_un_guasto_di_decodifica() {
        // Stessa prova, un numero diverso: `ERROR_CRC`, cioè un settore che non
        // si legge più sul disco di casa. Aspettare non lo aggiusta, e il codice
        // deve restare quello che non offre «Riprova».
        let (sorgente, morta) = brano_con_interruttore(1, 48_000, 23);
        let mut decodificatore = crate::decodifica::Decodificatore::apri(sorgente, 48_000, 1)
            .expect("con il disco sano si apre");

        morta.store(true, Ordering::Relaxed);
        let err = decodificatore
            .cerca(1_000)
            .expect_err("il salto non riesce");

        assert_eq!(err.code().kind(), ErrorCodeKind::PlaybackDecodeFailed);
        assert!(
            !err.code().is_retryable(),
            "rileggere un settore rotto dà lo stesso esito"
        );
    }

    #[test]
    fn la_share_che_muore_a_meta_brano_non_dice_che_il_file_e_rotto() {
        // Il danno che questa prova tiene chiuso: `playback.decodeFailed` in
        // italiano dice «questo file è danneggiato, sostituiscilo e rifai la
        // scansione». Detto per un cavo staccato è il consiglio peggiore
        // possibile — manda l'utente a buttare un file sano. Deve arrivare
        // invece l'errore di rete, che è ritentabile e dice la verità.
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        let primo = aperto(&banco.ctx, brano_che_muore(1, 48_000));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));

        let _ = suona_tutto(&mut banco, |_| {});

        let visti = eventi(&registro);
        assert!(
            visti
                .iter()
                .any(|e| e == "errore:fs.networkUnavailable:riprovabile"),
            "la share morta non si annuncia come guasto di rete ritentabile: {visti:?}"
        );
        assert!(
            !visti
                .iter()
                .any(|e| e.starts_with("errore:playback.decode")),
            "l'utente si sente dire che il file è danneggiato: {visti:?}"
        );
    }

    #[test]
    fn la_share_che_muore_non_brucia_la_coda() {
        // Il brano dopo sta sulla stessa condivisione morta: passare a lui vuol
        // dire un'altra attesa e un altro avviso, e così via fino in fondo alla
        // coda. Il motore deve fermarsi sul brano dove si era, non scorrere.
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        let primo = aperto(&banco.ctx, brano_che_muore(1, 48_000));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.2));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

        let fuori = suona_tutto(&mut banco, |_| {});

        let visti = eventi(&registro);
        // Il brano dopo sta a un livello suo: se se ne sente anche solo un
        // pezzo, il motore è passato a lui invece di fermarsi.
        assert!(
            quanti_a(&fuori, 0.2) == 0,
            "la coda è scorsa lo stesso sulla share morta: si sente il brano dopo"
        );
        assert!(
            visti.iter().any(|e| e == "fermato"),
            "il motore non ha dichiarato di essersi fermato: {visti:?}"
        );
        assert!(
            banco.ctx.corrente.is_none() && banco.ctx.prossimo.is_none(),
            "il motore ha lasciato roba in canna invece di fermarsi"
        );
    }

    #[test]
    fn un_file_davvero_rotto_continua_a_far_saltare_il_brano() {
        // Il rovescio della medaglia: un album con dentro un file corrotto deve
        // arrivare in fondo lo stesso. La distinzione la fa il codice
        // dell'errore, non il fatto che ci sia un errore.
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        let primo = aperto(&banco.ctx, brano_rotto(1, 48_000));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.2));
        banco.ctx.esegui(Comando::Prepara(Some(Box::new(secondo))));

        let fuori = suona_tutto(&mut banco, |_| {});

        let visti = eventi(&registro);
        assert!(
            visti
                .iter()
                .any(|e| e == "errore:playback.decodeFailed:definitivo"),
            "un settore illeggibile si annuncia come guasto di rete: {visti:?}"
        );
        assert!(
            !visti.iter().any(|e| e == "fermato"),
            "un file rotto ferma l'album invece di costare un fruscio: {visti:?}"
        );
        assert!(
            quanti_a(&fuori, 0.2) > 5_000,
            "il brano dopo quello rotto non si sente: la coda si è bruciata su di lui"
        );
    }

    // ── il filo che cade, e il motore che è stato sostituito ────────────────

    /// Byte che a metà lettura non restituiscono un errore: **panicano**.
    ///
    /// Il gemello di [`ByteCheFalliscono`], e la differenza è tutta lì. Quello
    /// riproduce una share che muore, cioè un guasto che sale come `Result` e
    /// che il motore sa raccontare. Questo riproduce l'altro modo in cui un
    /// file troncato può finire: un `assert` dentro il codice che lo legge.
    /// Nel nostro albero `unwrap`, `expect`, `panic!` e l'indicizzazione sono
    /// vietati, ma symphonia e rubato non sono il nostro albero — e byte
    /// arrivati a metà da una Wi-Fi che respira male sono esattamente
    /// l'ingresso che nessuno dei due ha mai visto.
    struct BytePanicanti {
        dentro: std::io::Cursor<Vec<u8>>,
        soglia: u64,
        letti: u64,
    }

    impl std::io::Read for BytePanicanti {
        fn read(&mut self, dove: &mut [u8]) -> std::io::Result<usize> {
            assert!(
                self.letti < self.soglia,
                "i byte finiscono qui, e non con garbo"
            );
            let quanti = std::io::Read::read(&mut self.dentro, dove)?;
            self.letti = self
                .letti
                .saturating_add(u64::try_from(quanti).unwrap_or(0));
            Ok(quanti)
        }
    }

    impl std::io::Seek for BytePanicanti {
        fn seek(&mut self, da: std::io::SeekFrom) -> std::io::Result<u64> {
            std::io::Seek::seek(&mut self.dentro, da)
        }
    }

    impl crate::decodifica::Flusso for BytePanicanti {
        fn lunghezza(&self) -> Option<u64> {
            u64::try_from(self.dentro.get_ref().len()).ok()
        }
    }

    /// Un brano che fa cadere chi lo legge, a metà.
    ///
    /// La soglia sta a metà per la stessa ragione di
    /// [`brano_che_si_interrompe`]: symphonia deve fare in tempo ad aprire il
    /// file, altrimenti la caduta arriverebbe dentro [`BranoAperto::apri`] —
    /// che è un altro filo e un altro discorso.
    fn brano_che_panica(track_id: i64, frequenza: u32) -> Sorgente {
        let quanti = fotogrammi_da_ms(1_000, frequenza);
        let campioni = vec![0.5f32; usize::try_from(quanti).unwrap_or(48_000)];
        let byte = wav(frequenza, &campioni);
        let soglia = u64::try_from(byte.len().div_ceil(2)).unwrap_or(0);
        Sorgente {
            track_id,
            media: Box::new(BytePanicanti {
                dentro: std::io::Cursor::new(byte),
                soglia,
                letti: 0,
            }),
            estensione: Some("wav".to_owned()),
            durata_ms: 1_000,
            replaygain_db: None,
        }
    }

    #[test]
    fn un_filo_che_cade_non_lascia_il_motore_muto() {
        // Il guasto che questa prova tiene chiuso, ed è il peggiore da
        // diagnosticare perché somiglia a niente: il filo della decodifica si
        // smontava in silenzio — il suo `JoinHandle` lo butta via nessuno — e
        // da fuori non cambiava una virgola. La callback continuava a servire
        // zeri, `perso` restava basso, il sorvegliante non vedeva niente da
        // riaprire, i comandi finivano in un canale senza ricevitore. L'utente
        // premeva pausa e non succedeva nulla.
        //
        // La traccia del panico compare su stderr durante questa prova: è il
        // gancio del diario che fa il suo lavoro, non un guasto della prova.
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);
        let condiviso = Arc::clone(&banco.condiviso);
        let primo = aperto(&banco.ctx, brano_che_panica(1, 48_000));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));

        let Banco {
            ctx,
            mut consumatore,
            condiviso: _,
            _manda,
            _curve,
        } = banco;

        // Qualcuno deve svuotare l'anello, altrimenti il filo si addormenta su
        // un anello pieno molto prima di arrivare ai byte che lo fanno cadere:
        // sono le casse, che qui non ci sono.
        let basta = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ferma = Arc::clone(&basta);
        let casse = std::thread::spawn(move || {
            while !ferma.load(Ordering::Relaxed) {
                while consumatore.pop().is_ok() {}
                std::thread::sleep(Duration::from_millis(1));
            }
        });

        let (fatto, aspetta) = std::sync::mpsc::channel();
        let decodifica = std::thread::spawn(move || {
            filo_sorvegliato(ctx);
            let _ = fatto.send(());
        });

        // Dieci secondi sono un'eternità per un brano da un secondo: se
        // scadono, il filo si è appeso invece di cadere, ed è comunque un
        // guasto da vedere.
        let tornato = aspetta.recv_timeout(Duration::from_secs(10));
        basta.store(true, Ordering::Relaxed);
        let _ = casse.join();
        let _ = decodifica.join();
        assert!(
            tornato.is_ok(),
            "il filo sorvegliato non è mai tornato: la rete non l'ha raccolto"
        );

        assert!(
            condiviso.perso.load(Ordering::Acquire),
            "il filo è caduto e nessuno ha alzato la bandiera: il sorvegliante \
             non avrà niente da riaprire"
        );
        assert_eq!(
            condiviso.causa_perdita.load(Ordering::Acquire),
            CAUSA_FILO_CADUTO,
            "la causa non distingue un decodificatore caduto da un cavo staccato"
        );

        let visti = eventi(&registro);
        assert!(
            visti
                .iter()
                .any(|e| e == "errore:playback.stalled:riprovabile"),
            "la caduta non si annuncia come riproduzione impantanata: {visti:?}"
        );
        assert!(
            !visti
                .iter()
                .any(|e| e.starts_with("errore:playback.decode")),
            "l'utente si sente dire che il file è danneggiato: {visti:?}"
        );
    }

    #[test]
    fn un_motore_abbandonato_non_fa_avanzare_la_coda_di_chi_lo_ha_sostituito() {
        let (registro, osservatore) = spia();
        let mut banco = banco_con(48_000, osservatore);

        // Il contrappeso: da vivo il motore parla. Senza questa metà la prova
        // passerebbe anche con un osservatore mai collegato.
        let primo = aperto(&banco.ctx, brano(1, 48_000, 0.5));
        banco.ctx.esegui(Comando::Suona(Box::new(primo)));
        banco.ctx.esegui(Comando::Ferma);
        assert_eq!(
            eventi(&registro),
            vec!["fermato".to_owned()],
            "un motore vivo deve dire quando si ferma"
        );

        // Il motore è stato sostituito: `Motore::drop` alza questa bandiera
        // **prima** di chiedere la chiusura, perché fra il comando e la morte
        // del filo passa un giro di ciclo — e in quel giro un `Fermato` in
        // ritardo chiuderebbe l'ascolto e farebbe avanzare la coda del motore
        // nuovo, che sta suonando.
        banco.condiviso.abbandonato.store(true, Ordering::Release);
        let secondo = aperto(&banco.ctx, brano(2, 48_000, 0.5));
        banco.ctx.esegui(Comando::Suona(Box::new(secondo)));
        banco.ctx.esegui(Comando::Ferma);

        assert_eq!(
            eventi(&registro),
            vec!["fermato".to_owned()],
            "un motore abbandonato parla ancora allo stato di quello che lo ha \
             sostituito"
        );
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
