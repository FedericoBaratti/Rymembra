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

/// Un numero complesso, giusto quanto serve alla trasformata.
#[derive(Debug, Clone, Copy, Default)]
struct C {
    re: f32,
    im: f32,
}

impl C {
    const fn nuovo(re: f32, im: f32) -> Self {
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
    const fn potenza(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
}

/// L'indice con i bit rovesciati, su `bit` posizioni.
const fn rovescia(mut valore: usize, bit: u32) -> usize {
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
fn trasforma(dati: &mut [C]) {
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

/// Il lettore dello spettro.
///
/// Vive dal lato del **motore**, non della callback: prende un lucchetto,
/// alloca alla nascita e non più, e fa il suo lavoro nel filo di chi lo
/// interroga. Nessuno dei divieti di `uscita.rs` vale qui — e nessuno di quelli
/// che valgono lì è stato attraversato per costruirlo.
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
    livelli: [f32; BANDE],
    frequenza: f32,
}

impl Spettro {
    /// Costruisce il lettore. `frequenza` è quella con cui si è aperta l'uscita.
    pub(crate) fn nuovo(ricevi: rtrb::Consumer<f32>, frequenza: u32) -> Self {
        let bit = FINESTRA.trailing_zeros();
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
            livelli: [0.0; BANDE],
            frequenza: if frequenza == 0 { 48_000.0 } else { frequenza as f32 },
        }
    }

    /// Ritira i campioni arrivati e restituisce le bande, ognuna in `0..=1`.
    ///
    /// Restituisce sempre qualcosa: a flusso fermo le barre **scendono** invece
    /// di sparire di colpo, che è la stessa inerzia che hanno mentre suona.
    pub fn leggi(&mut self) -> [f32; BANDE] {
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

        let grezze = if self.piena {
            self.calcola()
        } else {
            [FONDO_DB; BANDE]
        };

        for (livello, db) in self.livelli.iter_mut().zip(grezze) {
            let voluto = ((db - FONDO_DB) / -FONDO_DB).clamp(0.0, 1.0);
            let passo = if voluto > *livello { SALITA } else { DISCESA };
            *livello += (voluto - *livello) * passo;
        }
        self.livelli
    }

    /// La potenza di ogni banda, in decibel.
    fn calcola(&mut self) -> [f32; BANDE] {
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
                potenza += self.lavoro.get(indice).copied().unwrap_or_default().potenza();
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
    fn misura(frequenza: u32, mut genera: impl FnMut(usize) -> f32) -> [f32; BANDE] {
        let (mut manda, ricevi) = rtrb::RingBuffer::<f32>::new(FINESTRA * 2);
        let mut spettro = Spettro::nuovo(ricevi, frequenza);
        let mut t = 0usize;
        let mut bande = [0.0; BANDE];
        // Parecchi giri: le barre hanno un'inerzia, e la prima lettura sarebbe
        // il valore a metà della salita invece di quello a regime.
        for _ in 0..60 {
            for _ in 0..FINESTRA {
                let _ = manda.push(genera(t));
                t += 1;
            }
            bande = spettro.leggi();
        }
        bande
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
        let acceso = spettro.leggi().get(5).copied().unwrap_or(0.0);

        // Silenzio, e nessun campione nuovo.
        for _ in 0..40 {
            for _ in 0..FINESTRA {
                let _ = manda.push(0.0);
            }
            spettro.leggi();
        }
        let dopo = spettro.leggi().get(5).copied().unwrap_or(0.0);
        assert!(dopo < acceso * 0.2, "da {acceso} è scesa solo a {dopo}");
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
        assert!(errore < 1e-3, "energia {energia_tempo} contro {energia_freq}");
    }
}
