//! Lo spettro: dai campioni che escono alle dieci bande che si disegnano.
//!
//! # Perché è vero e non finto
//!
//! Perché l'alternativa non era averlo: uno spettro inventato dietro la
//! copertina sarebbe stata l'unica bugia dell'interfaccia, e il bottone è
//! rimasto spento per anni proprio per non dirla. I campioni li prende la
//! callback audio — l'unico posto che vede quel che esce davvero — e li passa a
//! un anello senza lucchetti, il terzo del crate dopo quello dei campioni e
//! quello delle curve dell'equalizzatore.
//!
//! # Dove si prende il segnale, e perché lì
//!
//! **Dopo l'equalizzatore e prima del volume.** Dopo l'equalizzatore perché una
//! curva che alza i bassi si deve vedere: è la stessa curva che le barre
//! descrivono. Prima del volume perché uno spettro che si abbassa quando si
//! abbassa la manopola descrive la manopola, non la musica.
//!
//! # Le bande sono quelle dell'equalizzatore
//!
//! [`CENTRI_HZ`](crate::CENTRI_HZ), le stesse dieci. Non è una comodità: è quel
//! che fa sì che `eq-bars` ed `eq-slider` — due parti che il registro delle
//! skin mette una accanto all'altra — descrivano la stessa cosa. La barra sopra
//! il cursore dei 250 Hz dice quanta energia c'è **a** 250 Hz.
//!
//! # Le misure, e perché queste
//!
//! Una finestra di 4096 campioni. A 48 kHz sono 85 ms e 11,7 Hz per bin: la
//! banda più bassa, quella dei 31 Hz, occupa da 22 a 44 Hz, cioè due bin. Con
//! 1024 campioni ne sarebbe stata mezzo, e la prima barra avrebbe mostrato la
//! continua invece del basso.
//!
//! # Le altre bande, quelle fini
//!
//! Dieci barre dicono «c'è un basso e ci sono degli acuti», e per il cursore
//! dell'equalizzatore è esattamente quel che serve. Per una scena che riempie
//! lo schermo non basta: dieci blocchi larghi un decimo di finestra si muovono
//! tutti insieme, e quel che si guarda non è più la musica ma dieci rettangoli.
//!
//! Accanto alle ottave, quindi, lo stesso trasformato produce una seconda
//! lettura: da otto a 1024 bande a **larghezza logaritmica uguale** fra 20 Hz e
//! 20 kHz. Sono la stessa energia divisa più fitta, non un'analisi diversa —
//! una sola trasformata, due riduzioni.
//!
//! # Quanto dettaglio c'è davvero, e quando comincia l'interpolazione
//!
//! Va detto invece di lasciarlo scoprire: la finestra dà 11,7 Hz per bin, e una
//! banda logaritmica larga meno di questo **non ha un bin tutto suo**. Con 64
//! bande succede sotto i 300 Hz, con 1024 succede quasi ovunque tranne che in
//! cima. Là la banda non inventa niente e non ripete il bin vicino: prende la
//! potenza interpolata fra i due bin che la circondano, cioè la stessa curva
//! descritta con più punti. Chiedere 1024 barre non fa comparire dettaglio che
//! nella finestra non c'è — fa comparire una curva più liscia, ed è una cosa
//! diversa che è giusto sapere.
//!
//! # Perché cambiare risoluzione non cambia l'altezza
//!
//! Ogni banda **somma** i suoi bin, per la stessa ragione per cui li somma la
//! banda d'ottava (il commento è più sotto). Ma una banda su otto copre otto
//! volte lo spettro di una banda su sessantaquattro, e sommando otto volte più
//! bin arriverebbe nove decibel più in alto: cambiare risoluzione sposterebbe
//! tutte le barre in su o in giù, che è l'unica cosa che una manopola del
//! *dettaglio* non deve fare. La somma si divide quindi per quante bande ci
//! sono, tarata su 64: la risoluzione cambia quanto è fitta la curva, non
//! quanto è alta.
//!
//! Vale per quel che riempie lo spettro — la musica, il rumore — e **non** per
//! un tono puro, la cui energia sta in due bin e non si allarga insieme alla
//! banda: là più bande vogliono dire un picco più stretto e più alto, che è il
//! significato della parola risoluzione.

use std::f64::consts::PI;

use crate::equalizzatore::{BANDE, CENTRI_HZ};

/// Quanti campioni per trasformata. Potenza di due, per il radix-2.
const FINESTRA: usize = 4096;

/// La soglia sotto cui una banda è «niente».
///
/// Settanta decibel sotto il fondo scala. Più in basso si comincia a disegnare
/// il rumore di quantizzazione, che è una barra che si muove sempre e non dice
/// niente.
const FONDO_DB: f32 = -70.0;

/// Quanto in fretta una barra sale, per lettura.
///
/// Quasi tutto in un colpo: un attacco lento fa perdere proprio i colpi, che
/// sono la cosa che si guarda.
const SALITA: f32 = 0.55;

/// Quanto in fretta scende.
///
/// Molto più lenta della salita. Con una discesa rapida le barre sfarfallano
/// invece di respirare, ed è la differenza fra uno spettro che si guarda e uno
/// che stanca.
const DISCESA: f32 = 0.12;

/// Quante barre fini si possono chiedere.
///
/// Potenze di due, e non una scala continua: il numero di barre decide anche
/// quanto è larga ognuna sullo schermo, e una manopola continua produrrebbe
/// larghezze frazionarie diverse a ogni scatto. Otto è il minimo che somigli
/// ancora a uno spettro; 1024 è dove la finestra ha finito il dettaglio da dare
/// (vedi la nota sull'interpolazione in testa al modulo) e da lì in su si
/// disegnerebbero solo punti in più sulla stessa curva.
pub const RISOLUZIONI: [u16; 8] = [8, 16, 32, 64, 128, 256, 512, 1024];

/// Quante barre se nessuno ha ancora scelto.
///
/// Sessantaquattro: abbastanza fitte da far vedere il movimento dentro una
/// nota invece che dentro un'ottava, abbastanza poche da restare barre
/// riconoscibili anche su una finestra stretta.
pub const RISOLUZIONE_DI_SERIE: u16 = 64;

/// La più piccola risoluzione accettata.
pub const RISOLUZIONE_MIN: u16 = 8;

/// La più grande risoluzione accettata.
pub const RISOLUZIONE_MAX: u16 = 1024;

/// La risoluzione su cui è tarata l'altezza delle barre.
///
/// Il perché è in testa al modulo: senza questo riferimento, cambiare il
/// dettaglio alzerebbe o abbasserebbe tutta la scena.
const RISOLUZIONE_RIFERIMENTO: f32 = 64.0;

/// Il fondo della scala delle bande fini.
///
/// Venti hertz e non zero: sotto c'è l'infrasuono, che nessuna cassa riproduce
/// e che nella finestra è indistinguibile dalla continua.
const BASSA_HZ: f32 = 20.0;

/// La cima della scala delle bande fini.
///
/// Ventimila hertz, cioè il limite dell'udito. Se il dispositivo si è aperto
/// più in basso, i bordi si fermano a Nyquist e le bande di sopra restano al
/// fondo scala invece di specchiare quel che c'è sotto.
const ALTA_HZ: f32 = 20_000.0;

/// Un numero complesso, giusto quanto serve alla trasformata.
///
/// `pub(crate)` perché la trasformata la usa anche `crate::attacchi`, che cerca
/// gli inizi delle parole in un file fermo invece delle bande di un suono che
/// scorre. Due domande diverse sulla stessa aritmetica: scriverla due volte
/// vorrebbe dire due bit di rimescolamento che prima o poi divergono.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct C {
    pub(crate) re: f32,
    pub(crate) im: f32,
}

impl C {
    pub(crate) const fn nuovo(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    fn per(self, altro: Self) -> Self {
        Self {
            re: self.re * altro.re - self.im * altro.im,
            im: self.re * altro.im + self.im * altro.re,
        }
    }

    const fn piu(self, altro: Self) -> Self {
        Self {
            re: self.re + altro.re,
            im: self.im + altro.im,
        }
    }

    const fn meno(self, altro: Self) -> Self {
        Self {
            re: self.re - altro.re,
            im: self.im - altro.im,
        }
    }

    /// Il quadrato del modulo. Senza radice: serve una potenza, e la radice la
    /// disferebbe il logaritmo subito dopo.
    pub(crate) const fn potenza(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
}

/// L'indice con i bit rovesciati, su `bit` posizioni.
pub(crate) const fn rovescia(mut valore: usize, bit: u32) -> usize {
    let mut fuori = 0;
    let mut i = 0;
    while i < bit {
        fuori = (fuori << 1) | (valore & 1);
        valore >>= 1;
        i += 1;
    }
    fuori
}

/// La trasformata, in posto, radix-2 decimata in frequenza.
///
/// # Perché scritta a mano
///
/// La regola del repo è che una dipendenza nuova si argomenta. Per una
/// trasformata reale di lunghezza **fissa**, chiamata trenta volte al secondo
/// su quattromila campioni, l'argomento non regge: sono sessanta righe, si
/// provano, e non portano un albero di pacchetti da aggiornare.
///
/// I fattori girano in `f64` anche se i campioni sono in `f32`: la ricorrenza
/// moltiplicativa accumula errore per dodici stadi, e in `f32` l'ultimo stadio
/// arriverebbe con le fasi visibilmente storte.
pub(crate) fn trasforma(dati: &mut [C]) {
    let n = dati.len();
    if n < 2 {
        return;
    }
    let mut lunghezza = 2usize;
    while lunghezza <= n {
        // Uno scorrimento e non una divisione: `lunghezza` è una potenza di due
        // per costruzione, e scriverlo così toglie di mezzo la domanda su cosa
        // succede al resto — che è quel che il divieto sulle divisioni intere
        // esiste per far porre.
        let meta = lunghezza >> 1;
        // Il passo del fattore di rotazione per questo stadio.
        let angolo = -2.0 * PI / lunghezza as f64;
        let (seno, coseno) = angolo.sin_cos();
        for blocco in dati.chunks_exact_mut(lunghezza) {
            let (bassi, alti) = blocco.split_at_mut(meta);
            // In `f64` per la ragione scritta sopra.
            let (mut wr, mut wi) = (1.0f64, 0.0f64);
            for (a, b) in bassi.iter_mut().zip(alti.iter_mut()) {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "il fattore di rotazione sta sul cerchio unitario: \
                              il valore è sempre in [-1, 1] e la conversione a f32 \
                              perde precisione, non intervallo"
                )]
                let w = C::nuovo(wr as f32, wi as f32);
                let t = w.per(*b);
                *b = a.meno(t);
                *a = a.piu(t);
                // La ricorrenza: (wr, wi) ← (wr, wi) · (coseno, seno).
                let nuovo_wr = wr * coseno - wi * seno;
                wi = wr * seno + wi * coseno;
                wr = nuovo_wr;
            }
        }
        lunghezza <<= 1;
    }
}

/// Quel che una lettura restituisce: le ottave e le bande fini.
///
/// Due letture della stessa trasformata e non due analisi: le ottave sono
/// quelle dell'equalizzatore e servono alle dieci barre sotto la copertina, le
/// fini sono quante ne ha chieste chi guarda e servono alla scena in profondità.
/// Stanno insieme perché vengono dallo stesso mezzo secondo di suono, e
/// calcolarle in due momenti diversi vorrebbe dire farle descrivere due istanti
/// diversi della stessa canzone.
#[derive(Debug, Clone)]
pub struct Bande {
    /// Le dieci d'ottava, nell'ordine di [`CENTRI_HZ`], ognuna in `0..=1`.
    pub ottave: [f32; BANDE],
    /// Le fini, dalla più bassa alla più alta, ognuna in `0..=1`.
    pub fini: Vec<f32>,
}

/// I bordi di ogni banda fine, in unità di bin.
///
/// In bin e non in hertz perché è così che li usa chi somma, e convertirli a
/// ogni lettura vorrebbe dire rifare 1024 divisioni trenta volte al secondo per
/// ottenere sempre gli stessi numeri.
fn bordi_di(quante: usize, frequenza: f32) -> Vec<(f32, f32)> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "FINESTRA è 4096: sta in f32 senza perdere niente"
    )]
    let per_bin = frequenza / FINESTRA as f32;
    // L'ultimo bin utile: oltre metà finestra c'è lo specchio, e il commento
    // sulla frequenza di Nyquist nella riduzione a ottave vale identico qui.
    #[expect(
        clippy::cast_precision_loss,
        reason = "FINESTRA/2 è 2048: sta in f32 senza perdere niente"
    )]
    let massimo = (FINESTRA >> 1) as f32 - 1.0;
    let rapporto = ALTA_HZ / BASSA_HZ;
    (0..quante)
        .map(|i| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "quante è al più 1024, e i sta sotto"
            )]
            let (dentro, oltre) = (i as f32, (i + 1) as f32);
            #[expect(clippy::cast_precision_loss, reason = "quante è al più 1024")]
            let enne = quante as f32;
            let da_hz = BASSA_HZ * rapporto.powf(dentro / enne);
            let a_hz = BASSA_HZ * rapporto.powf(oltre / enne);
            (
                (da_hz / per_bin).clamp(1.0, massimo),
                (a_hz / per_bin).clamp(1.0, massimo),
            )
        })
        .collect()
}

/// L'inerzia di una barra: sale quasi di colpo, scende piano.
///
/// Una funzione e non due copie del calcolo: le ottave e le fini devono salire
/// e scendere con lo **stesso** passo, altrimenti le dieci barre sotto la
/// copertina e la scena dietro descriverebbero lo stesso colpo di rullante in
/// due momenti diversi — che è la cosa che si nota per prima e non si sa dire.
fn inerzia(livello: f32, db: f32) -> f32 {
    let voluto = ((db - FONDO_DB) / -FONDO_DB).clamp(0.0, 1.0);
    let passo = if voluto > livello { SALITA } else { DISCESA };
    livello + (voluto - livello) * passo
}

/// Il lettore dello spettro.
///
/// Vive dal lato del **motore**, non della callback: prende un lucchetto,
/// alloca alla nascita e non più, e fa il suo lavoro nel filo di chi lo
/// interroga. Nessuno dei divieti di `uscita.rs` vale qui — e nessuno di quelli
/// che valgono lì è stato attraversato per costruirlo.
///
/// «Alloca alla nascita e non più» ha un'eccezione dichiarata: cambiare
/// risoluzione rifà tre vettori. Succede quando un dito preme una linguetta,
/// non trenta volte al secondo.
pub struct Spettro {
    ricevi: rtrb::Consumer<f32>,
    /// Gli ultimi [`FINESTRA`] campioni, come anello.
    finestra: Vec<f32>,
    /// Dove scrivere il prossimo.
    cursore: usize,
    /// La finestra è stata riempita almeno una volta.
    piena: bool,
    /// La finestratura di Hann, calcolata una volta.
    hann: Vec<f32>,
    /// Il rimescolamento dei bit, calcolato una volta.
    ordine: Vec<usize>,
    /// Lo spazio di lavoro della trasformata.
    lavoro: Vec<C>,
    /// I livelli mostrati, con la loro inerzia.
    bande: Bande,
    /// I bordi delle bande fini, in bin. Cambiano solo con la risoluzione.
    bordi: Vec<(f32, f32)>,
    /// I decibel delle bande fini prima dell'inerzia.
    ///
    /// Un vettore tenuto invece di uno restituito: è l'unico modo di calcolare
    /// 1024 numeri trenta volte al secondo senza allocare 1024 numeri trenta
    /// volte al secondo.
    grezze_fini: Vec<f32>,
    frequenza: f32,
}

impl Spettro {
    /// Costruisce il lettore. `frequenza` è quella con cui si è aperta l'uscita.
    pub(crate) fn nuovo(ricevi: rtrb::Consumer<f32>, frequenza: u32) -> Self {
        let bit = FINESTRA.trailing_zeros();
        let frequenza = if frequenza == 0 {
            48_000.0
        } else {
            #[expect(
                clippy::cast_precision_loss,
                reason = "una frequenza di campionamento sta sotto il milione"
            )]
            let f = frequenza as f32;
            f
        };
        let quante = usize::from(RISOLUZIONE_DI_SERIE);
        Self {
            ricevi,
            finestra: vec![0.0; FINESTRA],
            cursore: 0,
            piena: false,
            // Hann e non rettangolare: senza, un tono che non cade esattamente
            // su un bin sparge la sua energia su tutto lo spettro, e ogni banda
            // si accende un po'. È il difetto che fa sembrare rotto un
            // visualizzatore altrimenti giusto.
            hann: (0..FINESTRA)
                .map(|i| {
                    let x = 2.0 * PI * i as f64 / FINESTRA as f64;
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "la finestra di Hann sta in [0, 1] per costruzione"
                    )]
                    let v = (0.5 - 0.5 * x.cos()) as f32;
                    v
                })
                .collect(),
            ordine: (0..FINESTRA).map(|i| rovescia(i, bit)).collect(),
            lavoro: vec![C::default(); FINESTRA],
            bande: Bande {
                ottave: [0.0; BANDE],
                fini: vec![0.0; quante],
            },
            bordi: bordi_di(quante, frequenza),
            grezze_fini: vec![FONDO_DB; quante],
            frequenza,
        }
    }

    /// Cambia quante bande fini si calcolano.
    ///
    /// Fuori da [`RISOLUZIONE_MIN`]`..=`[`RISOLUZIONE_MAX`] si stringe invece di
    /// fallire: chi chiama arriva da una preferenza scritta su disco, e una
    /// preferenza vecchia o storta deve valere «il più vicino che so fare», non
    /// «lo spettro non funziona più».
    ///
    /// Le barre ripartono da zero e risalgono con la loro inerzia: tenere i
    /// livelli di prima vorrebbe dire spalmare dieci vecchie altezze su cento
    /// barre nuove, cioè mostrare per un istante uno spettro che non è mai
    /// esistito.
    pub fn dettaglio(&mut self, quante: u16) {
        let quante = usize::from(quante.clamp(RISOLUZIONE_MIN, RISOLUZIONE_MAX));
        if quante == self.bande.fini.len() {
            return;
        }
        self.bordi = bordi_di(quante, self.frequenza);
        self.bande.fini = vec![0.0; quante];
        self.grezze_fini = vec![FONDO_DB; quante];
    }

    /// Quante bande fini si stanno calcolando.
    #[must_use]
    pub fn quante_fini(&self) -> usize {
        self.bande.fini.len()
    }

    /// Ritira i campioni arrivati e restituisce le bande, ognuna in `0..=1`.
    ///
    /// Restituisce sempre qualcosa: a flusso fermo le barre **scendono** invece
    /// di sparire di colpo, che è la stessa inerzia che hanno mentre suona.
    pub fn leggi(&mut self) -> &Bande {
        // Si svuota tutto quel che c'è: la callback ne produce quarantottomila
        // al secondo e questo si chiama trenta volte, quindi ogni giro ne trova
        // un migliaio e mezzo. Prenderne solo una parte lascerebbe l'anello
        // pieno e lo spettro indietro di un tempo che cresce.
        while let Ok(campione) = self.ricevi.pop() {
            if let Some(posto) = self.finestra.get_mut(self.cursore) {
                *posto = campione;
            }
            self.cursore += 1;
            if self.cursore >= FINESTRA {
                self.cursore = 0;
                self.piena = true;
            }
        }

        // Una trasformata sola per tutte e due le riduzioni: è la ragione per
        // cui la finestra si trasforma qui e non dentro chi somma i bin.
        let grezze = if self.piena {
            self.trasformata();
            self.fini();
            self.ottave()
        } else {
            self.grezze_fini.fill(FONDO_DB);
            [FONDO_DB; BANDE]
        };

        for (livello, db) in self.bande.ottave.iter_mut().zip(grezze) {
            *livello = inerzia(*livello, db);
        }
        for (livello, db) in self.bande.fini.iter_mut().zip(self.grezze_fini.iter()) {
            *livello = inerzia(*livello, *db);
        }
        &self.bande
    }

    /// La potenza di ogni banda fine, in decibel, dentro [`Self::grezze_fini`].
    ///
    /// Va chiamata dopo [`Self::trasformata`]: legge i bin che quella lascia in
    /// [`Self::lavoro`].
    fn fini(&mut self) {
        // Tre prestiti separati e non `self`: il compilatore sa dividere i campi
        // di una struttura dentro una funzione, e questo è ciò che permette di
        // scrivere in `grezze_fini` mentre si legge `lavoro`.
        let lavoro = &self.lavoro;
        let bordi = &self.bordi;
        let grezze = &mut self.grezze_fini;
        let potenza_bin = |indice: usize| lavoro.get(indice).copied().unwrap_or_default().potenza();

        #[expect(
            clippy::cast_precision_loss,
            reason = "FINESTRA è 4096: sta in f32 senza perdere niente"
        )]
        let scala = (FINESTRA as f32 * 0.5).powi(2);
        #[expect(clippy::cast_precision_loss, reason = "le bande sono al più 1024")]
        let compensazione = bordi.len() as f32 / RISOLUZIONE_RIFERIMENTO;

        for (uscita, &(da, a)) in grezze.iter_mut().zip(bordi.iter()) {
            let larghezza = a - da;
            let potenza = if larghezza <= 0.0 {
                // Una banda sopra Nyquist: i bordi si sono stretti su un punto.
                0.0
            } else if larghezza < 1.0 {
                // Più stretta di un bin. Qui la finestra ha finito il dettaglio
                // e la banda **non** ripete il bin in cui cade: prende la
                // potenza interpolata fra i due che la circondano, moltiplicata
                // per quanto bin occupa. Ripetere il valore darebbe una scalinata
                // — tutte le bande dentro lo stesso bin identiche, e un gradino
                // dove il bin cambia — che è il difetto che fa sembrare rotto uno
                // spettro a mille barre.
                let centro = (da + a) * 0.5;
                let sotto = centro.floor();
                let frazione = centro - sotto;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "i bordi stanno in 1..FINESTRA/2 per costruzione"
                )]
                let indice = sotto as usize;
                let primo = potenza_bin(indice);
                let secondo = potenza_bin(indice + 1);
                (primo + (secondo - primo) * frazione) * larghezza
            } else {
                // Larga almeno un bin: si sommano quelli coperti, contando i due
                // ai bordi per la frazione che sta dentro. Senza le frazioni,
                // due bande vicine si contenderebbero il bin di confine — una lo
                // prenderebbe tutto e l'altra niente — e sul confine comparirebbe
                // un gradino che nel suono non c'è.
                let mut somma = 0.0;
                let mut bin = da.floor();
                let fine = a.ceil();
                while bin < fine {
                    let peso = ((bin + 1.0).min(a) - bin.max(da)).max(0.0);
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "i bordi stanno in 1..FINESTRA/2 per costruzione"
                    )]
                    let indice = bin as usize;
                    somma += potenza_bin(indice) * peso;
                    bin += 1.0;
                }
                somma
            };
            let normalizzata = potenza * compensazione / scala;
            *uscita = (10.0 * normalizzata.max(1e-20).log10()).max(FONDO_DB);
        }
    }

    /// Finestra i campioni e li trasforma, lasciando i bin in [`Self::lavoro`].
    fn trasformata(&mut self) {
        // La componente continua si toglie **prima** di finestrare, e non è
        // pignoleria. Moltiplicare un valore fisso per una finestra di Hann dà
        // la finestra stessa, il cui spettro non è solo il bin zero: sborda sui
        // due accanto, che a 48 kHz sono 12 e 23 Hz, cioè dentro la banda dei
        // 31. Senza questa riga un offset qualunque nella catena si vedrebbe
        // come un basso enorme che non c'è.
        let media = self.finestra.iter().sum::<f32>() / FINESTRA as f32;

        // La finestra è un anello: si legge dal più vecchio, cioè dal cursore.
        for (posto, i) in self.lavoro.iter_mut().zip(0..FINESTRA) {
            let da = (self.cursore + i) % FINESTRA;
            let campione = self.finestra.get(da).copied().unwrap_or(0.0) - media;
            let peso = self.hann.get(i).copied().unwrap_or(0.0);
            *posto = C::nuovo(campione * peso, 0.0);
        }
        // Il rimescolamento dei bit, dentro lo stesso spazio.
        let mescolato: Vec<C> = self
            .ordine
            .iter()
            .map(|&da| self.lavoro.get(da).copied().unwrap_or_default())
            .collect();
        self.lavoro = mescolato;
        trasforma(&mut self.lavoro);
    }

    /// La potenza di ogni banda d'ottava, in decibel.
    ///
    /// Va chiamata dopo [`Self::trasformata`], come [`Self::fini`].
    fn ottave(&self) -> [f32; BANDE] {
        #[expect(
            clippy::cast_precision_loss,
            reason = "FINESTRA è 4096: sta in f32 senza perdere niente"
        )]
        let per_bin = self.frequenza / FINESTRA as f32;
        let mut fuori = [FONDO_DB; BANDE];
        for (uscita, centro) in fuori.iter_mut().zip(CENTRI_HZ) {
            // La banda d'ottava attorno al centro: da c/√2 a c·√2. Sono le
            // stesse campane che l'equalizzatore alza e abbassa.
            let basso = centro / std::f32::consts::SQRT_2;
            let alto = centro * std::f32::consts::SQRT_2;
            let da = (basso / per_bin).floor().max(1.0);
            let a = (alto / per_bin).ceil();
            let mut potenza = 0.0f32;
            // Serve solo a distinguere «banda vuota perché fuori dallo spettro»
            // da «banda a zero»: la prima resta al fondo, la seconda ci arriva.
            let mut quanti = 0u32;
            let mut bin = da;
            // Metà finestra: oltre la frequenza di Nyquist lo spettro di un
            // segnale reale è lo specchio di quel che c'è sotto, e sommarlo
            // conterebbe ogni banda due volte.
            while bin <= a && bin < (FINESTRA >> 1) as f32 {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "bin è ≥ 1 e < FINESTRA/2 per la condizione del ciclo"
                )]
                let indice = bin as usize;
                potenza += self
                    .lavoro
                    .get(indice)
                    .copied()
                    .unwrap_or_default()
                    .potenza();
                quanti += 1;
                bin += 1.0;
            }
            if quanti > 0 {
                // **Somma** e non media, ed è la differenza fra un analizzatore
                // d'ottava e un grafico storto. Le bande d'ottava hanno una
                // larghezza proporzionale al centro: quella dei 31 Hz occupa due
                // bin, quella dei 16 kHz novecento. Mediando, la banda larga
                // divide la sua energia per novecento e la stretta per due —
                // con il risultato che gli acuti restano a zero anche su un
                // pezzo che ne è pieno. Sommando, ogni banda dice quanta energia
                // c'è nella sua ottava, che è la domanda a cui deve rispondere.
                let normalizzata = potenza / ((FINESTRA as f32 * 0.5).powi(2));
                *uscita = (10.0 * normalizzata.max(1e-20).log10()).max(FONDO_DB);
            }
        }
        fuori
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Riempie il lettore con un segnale generato, e legge finché si assesta.
    fn misura_a(frequenza: u32, quante: u16, mut genera: impl FnMut(usize) -> f32) -> Bande {
        let (mut manda, ricevi) = rtrb::RingBuffer::<f32>::new(FINESTRA * 2);
        let mut spettro = Spettro::nuovo(ricevi, frequenza);
        spettro.dettaglio(quante);
        let mut t = 0usize;
        let mut bande = spettro.leggi().clone();
        // Parecchi giri: le barre hanno un'inerzia, e la prima lettura sarebbe
        // il valore a metà della salita invece di quello a regime.
        for _ in 0..60 {
            for _ in 0..FINESTRA {
                let _ = manda.push(genera(t));
                t += 1;
            }
            bande = spettro.leggi().clone();
        }
        bande
    }

    /// Le sole ottave, per le prove che parlano dell'equalizzatore.
    fn misura(frequenza: u32, genera: impl FnMut(usize) -> f32) -> [f32; BANDE] {
        misura_a(frequenza, RISOLUZIONE_DI_SERIE, genera).ottave
    }

    /// L'indice della banda fine più alta, e quanto sta in alto.
    fn piu_alta(fini: &[f32]) -> (usize, f32) {
        fini.iter()
            .copied()
            .enumerate()
            .fold((0, 0.0), |(indice, massimo), (i, v)| {
                if v > massimo {
                    (i, v)
                } else {
                    (indice, massimo)
                }
            })
    }

    /// Il centro in hertz della banda fine `i`, con `quante` bande.
    fn centro_di(i: usize, quante: usize) -> f32 {
        BASSA_HZ * (ALTA_HZ / BASSA_HZ).powf((i as f32 + 0.5) / quante as f32)
    }

    /// Il bordo basso in hertz della banda fine `i`, con `quante` bande.
    fn bordo_di(i: usize, quante: usize) -> f32 {
        BASSA_HZ * (ALTA_HZ / BASSA_HZ).powf(i as f32 / quante as f32)
    }

    /// Un rumore ripetibile: serve energia in tutti i bin, non un tono.
    fn rumore(mut seme: u32) -> impl FnMut(usize) -> f32 {
        move |_| {
            seme = seme.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            #[expect(
                clippy::cast_precision_loss,
                reason = "il campione di prova è un rumore, non una misura"
            )]
            let x = (seme >> 8) as f32 / 8_388_608.0;
            x - 1.0
        }
    }

    fn seno(hz: f32, frequenza: u32) -> impl FnMut(usize) -> f32 {
        let passo = 2.0 * std::f32::consts::PI * hz / frequenza as f32;
        move |t| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "il campione di prova non arriva mai a 2^24"
            )]
            let x = t as f32;
            (x * passo).sin()
        }
    }

    #[test]
    fn un_tono_accende_la_sua_banda_e_lascia_stare_le_altre() {
        // La proprietà che rende lo spettro **vero**: mille hertz devono
        // accendere la banda dei mille hertz, non una qualunque.
        let bande = misura(48_000, seno(1_000.0, 48_000));
        let sua = bande.get(5).copied().unwrap_or(0.0);
        assert!(sua > 0.5, "la banda dei 1 kHz è a {sua}, doveva accendersi");
        for (i, altra) in bande.iter().enumerate() {
            if i == 5 {
                continue;
            }
            assert!(
                *altra < sua * 0.5,
                "la banda {i} è a {altra} contro {sua}: il tono si sta spargendo"
            );
        }
    }

    #[test]
    fn un_tono_basso_accende_la_banda_bassa() {
        // La ragione della finestra da 4096: con 1024 la banda dei 31 Hz non
        // sarebbe nemmeno un bin, e questa prova non potrebbe passare.
        let bande = misura(48_000, seno(31.25, 48_000));
        let sua = bande.first().copied().unwrap_or(0.0);
        assert!(sua > 0.4, "la banda dei 31 Hz è a {sua}");
        let media = bande.get(4).copied().unwrap_or(0.0);
        assert!(media < sua * 0.5, "i 500 Hz sono a {media} contro {sua}");
    }

    #[test]
    fn il_silenzio_non_accende_niente() {
        let bande = misura(48_000, |_| 0.0);
        for (i, banda) in bande.iter().enumerate() {
            assert!(*banda < 0.02, "la banda {i} è a {banda} sul silenzio");
        }
    }

    #[test]
    fn una_continua_non_e_un_basso() {
        // Un valore fisso è frequenza zero, e finisce nel bin zero: la prima
        // banda **non** deve accenderlo. È il difetto che fa sembrare che ci
        // sia un basso enorme quando c'è un offset nella catena.
        let bande = misura(48_000, |_| 0.7);
        let bassa = bande.first().copied().unwrap_or(0.0);
        assert!(bassa < 0.1, "la continua accende i 31 Hz a {bassa}");
    }

    #[test]
    fn le_barre_scendono_quando_il_suono_finisce() {
        // A flusso fermo le barre non spariscono di colpo: la discesa ha la sua
        // inerzia, ed è quel che rende lo spettro guardabile invece che a
        // scatti.
        let (mut manda, ricevi) = rtrb::RingBuffer::<f32>::new(FINESTRA * 2);
        let mut spettro = Spettro::nuovo(ricevi, 48_000);
        let mut suona = seno(1_000.0, 48_000);
        let mut t = 0usize;
        for _ in 0..60 {
            for _ in 0..FINESTRA {
                let _ = manda.push(suona(t));
                t += 1;
            }
            spettro.leggi();
        }
        let acceso = spettro.leggi().ottave.get(5).copied().unwrap_or(0.0);

        // Silenzio, e nessun campione nuovo.
        for _ in 0..40 {
            for _ in 0..FINESTRA {
                let _ = manda.push(0.0);
            }
            spettro.leggi();
        }
        let dopo = spettro.leggi().ottave.get(5).copied().unwrap_or(0.0);
        assert!(dopo < acceso * 0.2, "da {acceso} è scesa solo a {dopo}");
    }

    #[test]
    fn le_bande_fini_sono_quante_ne_ho_chieste() {
        for quante in RISOLUZIONI {
            let bande = misura_a(48_000, quante, |_| 0.0);
            assert_eq!(bande.fini.len(), usize::from(quante));
        }
    }

    #[test]
    fn una_risoluzione_impossibile_si_stringe_invece_di_rompere() {
        // Il caso vero: una preferenza scritta da una versione che accettava
        // altri numeri, o un valore arrivato storto dall'interfaccia. Deve
        // valere «il più vicino che so fare».
        let (_manda, ricevi) = rtrb::RingBuffer::<f32>::new(FINESTRA);
        let mut spettro = Spettro::nuovo(ricevi, 48_000);
        spettro.dettaglio(0);
        assert_eq!(spettro.quante_fini(), usize::from(RISOLUZIONE_MIN));
        spettro.dettaglio(u16::MAX);
        assert_eq!(spettro.quante_fini(), usize::from(RISOLUZIONE_MAX));
    }

    #[test]
    fn un_tono_accende_la_banda_fine_che_lo_contiene() {
        // La stessa proprietà delle ottave, chiesta alla scala fitta: mille
        // hertz devono accendere la banda dei mille hertz. Senza questa, un
        // errore di un fattore due nei bordi darebbe comunque uno spettro che si
        // muove — e nessuno se ne accorgerebbe guardandolo.
        let quante = 64usize;
        let bande = misura_a(48_000, 64, seno(1_000.0, 48_000));
        let (indice, valore) = piu_alta(&bande.fini);
        let centro = centro_di(indice, quante);
        assert!(valore > 0.4, "la banda più alta è a {valore}");
        assert!(
            (centro / 1_000.0).log2().abs() < 0.1,
            "il picco è a {centro} Hz invece che a mille"
        );
    }

    #[test]
    fn cambiare_risoluzione_non_cambia_l_altezza() {
        // La ragione della compensazione, provata su un rumore — cioè su
        // qualcosa che riempie tutte le bande, che è quel che fa la musica. Senza
        // la divisione per il numero di bande, ogni raddoppio dimezzerebbe la
        // larghezza di ogni banda e la scena si abbasserebbe di tre decibel: da
        // 8 a 1024 sarebbero ventun decibel, cioè una manopola del *dettaglio*
        // che in realtà è una manopola del volume.
        //
        // Su un **tono** puro la stessa invarianza non c'è, e non è un difetto:
        // l'energia di una sinusoide sta in due bin, non si allarga con la
        // banda, e più bande vogliono dire un picco più stretto e più alto. È il
        // significato della parola risoluzione.
        let media_a = |quante: u16| {
            let bande = misura_a(48_000, quante, rumore(12_345));
            #[expect(clippy::cast_precision_loss, reason = "le bande sono al più 1024")]
            let quante = bande.fini.len() as f32;
            bande.fini.iter().sum::<f32>() / quante
        };
        let poche = media_a(16);
        let tante = media_a(256);
        assert!(
            (poche - tante).abs() < 0.06,
            "con 16 bande il rumore sta a {poche}, con 256 a {tante}"
        );
    }

    #[test]
    fn le_bande_fini_sotto_il_bin_non_fanno_una_scalinata() {
        // Con 1024 bande sotto i mille hertz ce ne sono decine dentro lo stesso
        // bin: se ognuna ripetesse il valore del suo bin, il rumore rosa
        // disegnerebbe dei gradini invece di una curva. La prova guarda proprio
        // il basso, dove i gradini sarebbero, e chiede che due bande vicine non
        // siano mai identiche al bit.
        let bande = misura_a(48_000, 1024, rumore(12_345));
        // Le bande fra 100 e 400 Hz: la fascia in cui una banda su 1024 è larga
        // meno di un bin da 11,7 Hz.
        let mut uguali = 0u32;
        let mut confronti = 0u32;
        for i in 0..1023 {
            let centro = centro_di(i, 1024);
            if !(100.0..400.0).contains(&centro) {
                continue;
            }
            let qui = bande.fini.get(i).copied().unwrap_or(0.0);
            let dopo = bande.fini.get(i + 1).copied().unwrap_or(0.0);
            confronti += 1;
            if (qui - dopo).abs() < f32::EPSILON {
                uguali += 1;
            }
        }
        assert!(confronti > 50, "la fascia di prova è vuota: {confronti}");
        assert!(
            uguali * 4 < confronti,
            "{uguali} coppie identiche su {confronti}: è una scalinata"
        );
    }

    #[test]
    fn il_silenzio_non_accende_nessuna_banda_fine() {
        let bande = misura_a(48_000, 256, |_| 0.0);
        for (i, banda) in bande.fini.iter().enumerate() {
            assert!(*banda < 0.02, "la banda fine {i} è a {banda} sul silenzio");
        }
    }

    #[test]
    fn le_bande_sopra_nyquist_restano_al_fondo() {
        // Un dispositivo aperto a 22 kHz non ha niente da dire sopra gli 11: le
        // bande di lassù devono restare a zero invece di specchiare quel che c'è
        // sotto, che è l'errore classico di chi non ferma i bordi a Nyquist.
        //
        // Il confronto è sul **bordo basso** e non sul centro: la banda che sta
        // a cavallo di Nyquist ha dentro dei bin veri, e chiederle di essere
        // vuota vorrebbe dire chiederle di buttare via il suono che ci trova.
        let bande = misura_a(22_050, 64, rumore(999));
        for (i, banda) in bande.fini.iter().enumerate() {
            if bordo_di(i, 64) < 11_025.0 {
                continue;
            }
            assert!(*banda < 0.02, "la banda fine {i} è a {banda} sopra Nyquist");
        }
    }

    #[test]
    fn la_trasformata_conserva_l_energia() {
        // Parseval, come controllo di sanità: quel che entra nel dominio del
        // tempo deve ritrovarsi in quello della frequenza. Se la trasformata
        // fosse sbagliata le prove qui sopra potrebbero passare per caso — un
        // tono finisce comunque in **qualche** bin — mentre questa no.
        let n = 64usize;
        let mut dati: Vec<C> = (0..n)
            .map(|i| {
                #[expect(clippy::cast_precision_loss, reason = "n è 64")]
                let x = i as f32;
                C::nuovo((x * 0.7).sin() + 0.3 * (x * 2.1).cos(), 0.0)
            })
            .collect();
        let energia_tempo: f32 = dati.iter().map(|c| c.potenza()).sum();

        let bit = n.trailing_zeros();
        let ordine: Vec<usize> = (0..n).map(|i| rovescia(i, bit)).collect();
        let mescolato: Vec<C> = ordine
            .iter()
            .map(|&da| dati.get(da).copied().unwrap_or_default())
            .collect();
        dati = mescolato;
        trasforma(&mut dati);

        #[expect(clippy::cast_precision_loss, reason = "n è 64")]
        let enne = n as f32;
        let energia_freq: f32 = dati.iter().map(|c| c.potenza()).sum::<f32>() / enne;
        let errore = (energia_tempo - energia_freq).abs() / energia_tempo;
        assert!(
            errore < 1e-3,
            "energia {energia_tempo} contro {energia_freq}"
        );
    }
}
