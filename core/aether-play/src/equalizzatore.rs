//! Dieci filtri in cascata, e i numeri che li descrivono.
//!
//! # Perché le bande stanno a ottave
//!
//! Perché l'orecchio le sente così. Dieci fasce larghe uguali in hertz — 0-2
//! kHz, 2-4, e via fino a 20 — sembrano una divisione equa e non lo sono: la
//! prima conterrebbe da sola i bassi, i medi e quasi tutte le fondamentali di
//! qualunque strumento, mentre le ultime cinque si spartirebbero la sola aria
//! sopra i 10 kHz. Nove cursori su dieci non farebbero quasi niente, e il
//! decimo farebbe tutto.
//!
//! A ottave — 31, 62, 125, 250, 500 Hz, 1, 2, 4, 8, 16 kHz — ogni cursore pesa
//! all'ascolto più o meno quanto gli altri, che è l'unica cosa che rende usabile
//! un equalizzatore grafico.
//!
//! # Perché lo stato è in `f64` e la forma è trasposta
//!
//! La banda più bassa sta a 31,25 Hz: su un'uscita a 48 kHz è un rapporto di
//! uno a millecinquecento, e i poli di quel biquad finiscono a un millesimo dal
//! cerchio unitario. In `f32` la forma diretta I lì dentro perde cifre nella
//! sottrazione fra numeri quasi uguali, e il risultato non è un filtro
//! visibilmente sbagliato — è un filtro che rumoreggia sotto al suono.
//!
//! La forma diretta II trasposta in `f64` non ha il problema, e per giunta
//! tiene due variabili di stato per sezione invece di quattro. Costa un milione
//! scarso di moltiplicazioni al secondo su un'uscita stereo a 48 kHz, che è
//! niente.
//!
//! In `f64` c'è anche un vantaggio che non si nota finché non manca: i
//! subnormali. Lo stato di un biquad decade da solo durante il silenzio, e
//! quando entra nell'intervallo subnormale ogni operazione costa decine di
//! cicli invece di uno. In `f32` succede dopo qualche secondo di silenzio
//! digitale; in `f64` quell'intervallo comincia a 10⁻³⁰⁸, cioè mai.
//!
//! Per la stessa ragione qui **non** si usa `mul_add`, che pure sarebbe la
//! forma più precisa di `a·b + c`: su x86-64 la FMA non è abilitata di serie,
//! e senza istruzione hardware `f64::mul_add` diventa una chiamata a `fma()`
//! della libreria matematica — centinaia di cicli, dentro la callback audio, per
//! guadagnare un bit di mantissa che nessuno sente.
//!
//! # Cosa questo modulo non sa
//!
//! Non sa cos'è una scheda audio, non tocca atomiche e non parla con nessun
//! altro filo. Riceve dei decibel e una frequenza di campionamento, restituisce
//! dei numeri, e applica quei numeri a dei campioni. È fatto così perché è la
//! parte che si prova senza aprire un dispositivo — e perché su Android sarà
//! identica.

/// Quante bande.
pub const BANDE: usize = 10;

/// Il centro di ogni banda, in hertz.
///
/// Ottave esatte a partire da 31,25 Hz, che è mille diviso trentadue. Partire da
/// un numero più tondo — 31 o 32 — spezzerebbe il rapporto di due fra una banda
/// e la successiva proprio dove i filtri sono più stretti e più delicati.
pub const CENTRI_HZ: [f32; BANDE] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Di quanto si può alzare o abbassare una banda, in decibel.
///
/// Dodici e non ventiquattro: oltre, un equalizzatore grafico smette di
/// correggere e comincia a distruggere — e la preamplificazione automatica che
/// tiene i picchi sotto fondo scala dovrebbe abbassare tutto il resto di
/// altrettanto, cioè seppellire il brano per alzare una banda.
pub const LIMITE_DB: f32 = 12.0;

/// Il fattore di merito di una banda larga un'ottava.
///
/// Dal ricettario di Robert Bristow-Johnson: `Q = √(2^BW) / (2^BW − 1)`, che per
/// una larghezza di banda di un'ottava fa esattamente `√2`.
const Q: f64 = std::f64::consts::SQRT_2;

/// Sotto questo scostamento, una banda vale ferma.
///
/// Serve a far tornare `piatto` quando i cursori sono a zero ma non
/// esattamente: mezzo centesimo di decibel non si sente, e riconoscerlo come
/// «niente da fare» è ciò che permette alla callback di saltare tutto il lavoro.
const SOGLIA_DB: f32 = 0.05;

/// Quanti canali teniamo in memoria.
///
/// Otto, che è lo stesso caso peggiore con cui `motore.rs` dimensiona l'anello.
/// Non è un limite del dispositivo: è la misura di un array che deve stare
/// dentro una chiusura senza allocare.
const CANALI_MAX: usize = 8;

/// Oltre questa frazione della frequenza di campionamento, una banda si spegne.
///
/// A 44,1 kHz la banda dei 16 kHz sta già al 73 % di Nyquist, dove la
/// trasformazione bilineare del ricettario deforma la campana in modo visibile;
/// su un dispositivo aperto a 32 kHz quella stessa banda cadrebbe **oltre**
/// Nyquist, cioè su coefficienti che non descrivono più niente. Spegnerla è
/// l'unica risposta onesta: là sopra non c'è segnale da alzare comunque.
const FRAZIONE_MASSIMA: f64 = 0.45;

/// Quante frequenze si guardano per misurare il picco di una curva.
///
/// Otto per ottava per undici ottave, a partire dal centro della banda più
/// bassa. L'ancoraggio non è un dettaglio: così la griglia cade **esattamente**
/// su tutti e dieci i centri, che è dove stanno i massimi delle campane, e su
/// sette punti intermedi per ottava, che è dove sta il massimo della somma di
/// due campane vicine.
///
/// Una griglia più rada e ancorata a un numero tondo sbagliava la misura di
/// oltre un decibel, e la preamplificazione lasciava saturare: le sue prove
/// stanno in fondo al file.
const PUNTI_RISPOSTA: usize = 89;

/// Da dove comincia la griglia, in hertz: il centro della banda più bassa.
const PRIMO_PUNTO_HZ: f64 = 31.25;

/// Di quanto si sale a ogni punto della griglia: un ottavo di ottava, cioè
/// `2^(1/8)`. Scritto per esteso perché `powf` non è utilizzabile in una
/// costante; che sia il numero giusto lo verifica una prova.
const PASSO_PUNTI: f64 = 1.090_507_732_665_257_7;

/// Un biquad, già normalizzato su `a0`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sezione {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Sezione {
    /// La sezione che lascia passare tutto com'è.
    const fn identita() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    /// Una campana centrata su `centro_hz`, dal ricettario RBJ.
    fn peaking(centro_hz: f64, frequenza: f64, guadagno_db: f64) -> Self {
        let ampiezza = 10.0f64.powf(guadagno_db / 40.0);
        let w0 = std::f64::consts::TAU * centro_hz / frequenza;
        let alfa = w0.sin() / (2.0 * Q);
        let coseno = w0.cos();
        // `a0` vale `1 + alfa/A` con entrambi positivi: è sempre maggiore di
        // uno, quindi la divisione non ha un caso degenere da coprire.
        let a0 = 1.0 + alfa / ampiezza;
        Self {
            b0: (1.0 + alfa * ampiezza) / a0,
            b1: -2.0 * coseno / a0,
            b2: (1.0 - alfa * ampiezza) / a0,
            a1: -2.0 * coseno / a0,
            a2: (1.0 - alfa / ampiezza) / a0,
        }
    }

    /// Il modulo della risposta a una certa pulsazione.
    ///
    /// I quattro valori trigonometrici arrivano da fuori perché non dipendono
    /// dalla sezione: calcolarli qui vorrebbe dire ripetere dieci volte gli
    /// stessi seni per ogni punto della griglia.
    fn modulo(&self, cos1: f64, sin1: f64, cos2: f64, sin2: f64) -> f64 {
        let num_re = self.b0 + self.b1 * cos1 + self.b2 * cos2;
        let num_im = self.b1 * sin1 + self.b2 * sin2;
        let den_re = 1.0 + self.a1 * cos1 + self.a2 * cos2;
        let den_im = self.a1 * sin1 + self.a2 * sin2;
        let denominatore = den_re * den_re + den_im * den_im;
        if denominatore <= 0.0 {
            return 1.0;
        }
        ((num_re * num_re + num_im * num_im) / denominatore).sqrt()
    }
}

/// Quanto arriva a guadagnare la cascata nel suo punto peggiore.
///
/// Serve alla preamplificazione, e **si misura invece di indovinarla**. La
/// scorciatoia ovvia — attenuare della banda più alzata — è sbagliata di
/// parecchio: dieci campane larghe un'ottava si sovrappongono, e alzarle tutte
/// di dodici decibel ne produce una ventina al centro dello spettro. Attenuare
/// di dodici lascerebbe otto decibel di saturazione, cioè esattamente il guasto
/// che la preamplificazione esiste per evitare.
///
/// Non scende mai sotto uno: una curva fatta di soli tagli non ha niente da
/// attenuare, e abbassarla ancora vorrebbe dire che togliere i bassi rende
/// tutto il resto più piano.
fn picco(sezioni: &[Sezione; BANDE], frequenza: f64) -> f64 {
    let mut massimo = 1.0f64;
    let mut hz = PRIMO_PUNTO_HZ;
    for _ in 0..PUNTI_RISPOSTA {
        let w = std::f64::consts::TAU * hz / frequenza;
        // La griglia sale: oltre Nyquist non c'è più niente da guardare.
        if w >= std::f64::consts::PI {
            break;
        }
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        let mut ampiezza = 1.0f64;
        for sezione in sezioni {
            ampiezza *= sezione.modulo(cos1, sin1, cos2, sin2);
        }
        massimo = massimo.max(ampiezza);
        hz *= PASSO_PUNTI;
    }
    massimo
}

/// Una curva, tradotta nei numeri che servono per applicarla.
///
/// `Copy` e senza puntatori, perché è quel che attraversa l'anello verso la
/// callback audio: là dentro non si può allocare, e un tipo che si sposta
/// copiando qualche centinaio di byte sullo stack è l'unico modo di consegnare
/// cinquanta numeri in virgola mobile tutti insieme — senza che nessuno possa
/// leggerne metà di una curva e metà dell'altra.
#[derive(Clone, Copy, Debug)]
pub struct Coefficienti {
    sezioni: [Sezione; BANDE],
    preamp: f32,
    piatto: bool,
}

impl Coefficienti {
    /// Nessuna banda mossa.
    #[must_use]
    pub const fn piatti() -> Self {
        Self {
            sezioni: [Sezione::identita(); BANDE],
            preamp: 1.0,
            piatto: true,
        }
    }

    /// Traduce una curva in decibel per una certa frequenza di campionamento.
    ///
    /// I guadagni si tagliano a ±[`LIMITE_DB`] e quelli non finiti valgono zero:
    /// questa funzione è raggiungibile da un valore conservato su disco, e un
    /// `NaN` che arrivasse fino ai coefficienti renderebbe muto il lettore
    /// finché qualcuno non riscrive quel valore a mano.
    ///
    /// Una curva con meno di [`BANDE`] valori si completa con degli zeri, una
    /// con più si tronca: il giorno in cui le bande cambiassero di numero, una
    /// curva conservata dalla versione di prima deve valere «adattabile», non
    /// «illeggibile».
    #[must_use]
    pub fn calcola(guadagni_db: &[f32], attivo: bool, frequenza: u32) -> Self {
        if !attivo || frequenza == 0 {
            return Self::piatti();
        }
        let frequenza = f64::from(frequenza);
        let limite = frequenza * FRAZIONE_MASSIMA;

        let mut sezioni = [Sezione::identita(); BANDE];
        let mut mossa = false;

        for (banda, sezione) in sezioni.iter_mut().enumerate() {
            let grezzo = guadagni_db.get(banda).copied().unwrap_or(0.0);
            // Il controllo di finitezza **prima** del taglio: `clamp` su un
            // `NaN` restituisce `NaN`, e da lì in poi ogni confronto è falso —
            // compreso quello che dovrebbe scartarlo.
            let db = if grezzo.is_finite() {
                grezzo.clamp(-LIMITE_DB, LIMITE_DB)
            } else {
                0.0
            };
            if db.abs() < SOGLIA_DB {
                continue;
            }
            let Some(&centro) = CENTRI_HZ.get(banda) else {
                continue;
            };
            let centro = f64::from(centro);
            if centro >= limite {
                continue;
            }
            mossa = true;
            *sezione = Sezione::peaking(centro, frequenza, f64::from(db));
        }

        if !mossa {
            return Self::piatti();
        }

        // L'attenuazione automatica: quanto basta perché la curva non porti il
        // segnale sopra fondo scala. Senza, alzare quattro bande manderebbe in
        // saturazione l'uscita — e sul percorso a virgola mobile non c'è nemmeno
        // il taglio che `a_i16` fa prima di convertire.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "un fattore fra zero e uno: la precisione di un f32 su un \
                      guadagno è un milionesimo di decibel"
        )]
        let preamp = (1.0 / picco(&sezioni, frequenza)) as f32;

        Self {
            sezioni,
            preamp,
            piatto: false,
        }
    }

    /// Nessuna banda è mossa: chi applica può saltare tutto.
    #[must_use]
    pub const fn piatto(&self) -> bool {
        self.piatto
    }

    /// Di quanto il segnale viene attenuato prima dei filtri, da 0 a 1.
    ///
    /// È l'inverso del picco misurato della curva, non l'inverso della banda
    /// più alzata: vedi [`picco`].
    #[must_use]
    pub const fn preamp(&self) -> f32 {
        self.preamp
    }
}

impl Default for Coefficienti {
    fn default() -> Self {
        Self::piatti()
    }
}

/// La memoria di una sezione, nella forma diretta II trasposta.
#[derive(Clone, Copy, Debug, Default)]
struct Memoria {
    s1: f64,
    s2: f64,
}

/// Quel che una sezione non ha ancora finito di raccontare.
const VUOTA: Memoria = Memoria { s1: 0.0, s2: 0.0 };

/// I filtri, con dentro quel che si ricordano.
///
/// Vive nella callback audio, quindi ha una misura fissa e nessuna allocazione:
/// un `Vec` per canale sarebbe più elegante da leggere e sarebbe una `malloc`
/// in un posto dove `malloc` può prendere un lucchetto globale.
pub struct Stato {
    /// La memoria, per canale e per banda.
    memoria: [[Memoria; BANDE]; CANALI_MAX],
    coefficienti: Coefficienti,
}

impl Stato {
    /// Filtri fermi e memoria vuota.
    #[must_use]
    pub const fn nuovo() -> Self {
        Self {
            memoria: [[VUOTA; BANDE]; CANALI_MAX],
            coefficienti: Coefficienti::piatti(),
        }
    }

    /// Adotta una curva nuova.
    ///
    /// La memoria **non** si svuota: il suono sta continuando, e azzerare lo
    /// stato dei filtri a metà di un'onda produrrebbe esattamente il gradino che
    /// tutto il resto della catena si preoccupa di evitare. Il transitorio di un
    /// cambio di coefficienti si smorza da solo in pochi millisecondi; a non far
    /// sentire i cambi *grossi* — il caricamento di un preset — ci pensa chi
    /// manda le curve, interpolandole.
    pub fn aggiorna(&mut self, coefficienti: Coefficienti) {
        self.coefficienti = coefficienti;
    }

    /// Dimentica quel che è passato, tenendo la curva.
    ///
    /// Serve dopo un salto: la coda dei filtri appartiene al punto di prima, e
    /// lasciarla suonare sopra il punto nuovo è un rimasuglio esattamente come
    /// lo sono i campioni ancora nell'anello.
    pub fn azzera(&mut self) {
        self.memoria = [[VUOTA; BANDE]; CANALI_MAX];
    }

    /// Nessuna banda è mossa.
    #[must_use]
    pub const fn piatto(&self) -> bool {
        self.coefficienti.piatto
    }

    /// Un campione di un canale, filtrato.
    ///
    /// Un campione alla volta e non un blocco, perché è così che la callback
    /// legge dall'anello: uno per volta, senza sapere in anticipo quanti ne
    /// troverà.
    ///
    /// Un canale oltre quelli che teniamo restituisce il campione com'è. Non può
    /// succedere — il dispositivo non si apre con più di otto canali — ma qui
    /// dentro non esiste un modo accettabile di cadere.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "da f64 a f32 sul valore d'uscita: la precisione in più serviva dentro \
                  la cascata, non fuori — e il campione torna nel formato in cui \
                  l'anello lo tiene"
    )]
    pub fn applica(&mut self, campione: f32, canale: usize) -> f32 {
        if self.coefficienti.piatto || !campione.is_finite() {
            return campione;
        }
        let Some(memoria) = self.memoria.get_mut(canale) else {
            return campione;
        };

        let mut x = f64::from(campione) * f64::from(self.coefficienti.preamp);
        for (sezione, ricordo) in self.coefficienti.sezioni.iter().zip(memoria.iter_mut()) {
            let y = sezione.b0 * x + ricordo.s1;
            ricordo.s1 = sezione.b1 * x - sezione.a1 * y + ricordo.s2;
            ricordo.s2 = sezione.b2 * x - sezione.a2 * y;
            x = y;
        }
        x as f32
    }

    /// Un blocco interlacciato, filtrato sul posto.
    ///
    /// Non la usa la callback — là i campioni arrivano uno per volta — ma le
    /// prove e gli esempi, dove si lavora su fette intere.
    pub fn applica_blocco(&mut self, dati: &mut [f32], canali: usize) {
        if self.coefficienti.piatto || canali == 0 {
            return;
        }
        for indice in 0..dati.len() {
            let Some(&campione) = dati.get(indice) else {
                break;
            };
            let filtrato = self.applica(campione, indice % canali);
            if let Some(posto) = dati.get_mut(indice) {
                *posto = filtrato;
            }
        }
    }
}

impl Default for Stato {
    fn default() -> Self {
        Self::nuovo()
    }
}

/// Le curve di serie, nell'ordine in cui si mostrano.
///
/// Stanno qui e non nel crate dell'applicazione perché sono conoscenza audio —
/// dipendono da quali sono le bande e da quanto sono larghe — e perché il giorno
/// in cui ci sarà Android le vorrà identiche a queste, non tradotte una seconda
/// volta.
///
/// I valori sono in decibel, nell'ordine di [`CENTRI_HZ`].
pub const PRESET_DI_SERIE: [(&str, [f32; BANDE]); 9] = [
    ("Piatto", [0.0; BANDE]),
    ("Rock", [5.0, 4.0, 2.5, 0.5, -1.5, -1.0, 1.0, 3.0, 4.0, 4.0]),
    ("Pop", [-1.0, 0.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0, -1.5]),
    (
        "Voce",
        [-3.0, -3.0, -1.5, 1.0, 3.0, 4.0, 4.0, 2.5, 1.0, 0.0],
    ),
    ("Bassi", [7.0, 6.0, 4.5, 2.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    (
        "Acustico",
        [3.0, 3.0, 2.0, 0.0, 1.0, 1.0, 2.0, 3.0, 3.0, 2.0],
    ),
    (
        "Elettronica",
        [5.0, 4.5, 1.5, 0.0, -2.0, 1.0, 1.0, 2.0, 4.0, 5.0],
    ),
    (
        "Classica",
        [3.0, 3.0, 2.0, 1.0, -1.0, -1.0, 0.0, 2.0, 3.0, 3.5],
    ),
    // Di notte i bassi passano attraverso i muri e i medi no: questa curva toglie
    // quel che disturba gli altri e alza quel che rende comprensibile un brano a
    // volume basso.
    (
        "Notte",
        [-5.0, -4.0, -2.0, 1.5, 3.0, 3.0, 2.0, 0.0, -2.0, -3.0],
    ),
];

#[cfg(test)]
mod prove {
    use super::*;

    /// Un campione di un seno di prova.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "un generatore di prova: la precisione di un f32 è ben oltre quel che \
                  serve per misurare un guadagno in decibel"
    )]
    fn seno(hz: f64, n: usize, frequenza: f64) -> f32 {
        let t = f64::from(u32::try_from(n).unwrap_or(0)) / frequenza;
        (std::f64::consts::TAU * hz * t).sin() as f32
    }

    /// Mezzo secondo a 48 kHz: abbastanza perché la coda del filtro più stretto
    /// si sia esaurita e resti solo il guadagno a regime.
    const QUANTI: usize = 24_000;

    /// L'ampiezza di un seno a `hz` dopo essere passato dai filtri.
    ///
    /// Si guarda solo la seconda metà: la prima contiene l'attacco, che non dice
    /// niente sul guadagno a regime.
    fn ampiezza_a(stato: &mut Stato, hz: f64, frequenza: f64) -> f32 {
        let mut massimo = 0.0f32;
        for n in 0..QUANTI {
            let fuori = stato.applica(seno(hz, n, frequenza), 0);
            if n * 2 > QUANTI {
                massimo = massimo.max(fuori.abs());
            }
        }
        massimo
    }

    fn con(guadagni: &[f32], frequenza: u32) -> Stato {
        let mut stato = Stato::nuovo();
        stato.aggiorna(Coefficienti::calcola(guadagni, true, frequenza));
        stato
    }

    /// Una curva con una sola banda mossa.
    fn sola(banda: usize, db: f32) -> [f32; BANDE] {
        let mut guadagni = [0.0f32; BANDE];
        if let Some(posto) = guadagni.get_mut(banda) {
            *posto = db;
        }
        guadagni
    }

    #[test]
    fn una_curva_piatta_non_tocca_niente() {
        let mut stato = con(&[0.0; BANDE], 48_000);
        assert!(stato.piatto(), "dieci zeri devono valere «niente da fare»");
        let mut dati = [0.3, -0.7, 0.9, -0.2];
        let copia = dati;
        stato.applica_blocco(&mut dati, 2);
        assert_eq!(dati, copia);
    }

    #[test]
    fn uno_scostamento_impercettibile_vale_piatto() {
        // Il caso vero: un cursore riportato a zero con il dito, che lascia un
        // centesimo di decibel. Senza questa soglia la callback filtrerebbe per
        // sempre, per niente.
        assert!(Coefficienti::calcola(&sola(0, 0.01), true, 48_000).piatto());
    }

    #[test]
    fn spento_vale_piatto_anche_con_la_curva_alzata() {
        let coefficienti = Coefficienti::calcola(&[9.0; BANDE], false, 48_000);
        assert!(coefficienti.piatto());
        assert!((coefficienti.preamp() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn una_banda_alzata_alza_la_sua_frequenza() {
        // +12 dB sui 1000 Hz, che è la banda numero cinque. Il guadagno si
        // misura **al netto del preamp**: la campana alza di dodici decibel e il
        // preamp riabbassa di dodici, quindi a 1 kHz si torna circa a uno.
        let mut stato = con(&sola(5, 12.0), 48_000);
        let a_mille = ampiezza_a(&mut stato, 1000.0, 48_000.0);
        assert!(
            (0.9..=1.1).contains(&a_mille),
            "a 1 kHz l'ampiezza è {a_mille}, doveva tornare circa a uno"
        );
    }

    #[test]
    fn una_banda_alzata_lascia_stare_le_altre() {
        let mut stato = con(&sola(5, 12.0), 48_000);
        // Sei ottave sotto la campana: qui deve arrivare solo il preamp, cioè
        // l'attenuazione di dodici decibel, cioè un quarto.
        let lontano = ampiezza_a(&mut stato, 15.625, 48_000.0);
        assert!(
            lontano < 0.35,
            "a 15 Hz l'ampiezza è {lontano}: la campana dei 1000 Hz è troppo larga"
        );
    }

    #[test]
    fn una_banda_abbassata_abbassa() {
        let mut stato = con(&sola(5, -12.0), 48_000);
        let a_mille = ampiezza_a(&mut stato, 1000.0, 48_000.0);
        assert!(
            a_mille < 0.3,
            "a 1 kHz l'ampiezza è {a_mille}, doveva scendere a circa un quarto"
        );
        // Un taglio non attenua il resto: il preamp entra solo con i rialzi.
        let coefficienti = Coefficienti::calcola(&sola(5, -12.0), true, 48_000);
        assert!((coefficienti.preamp() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn il_preamp_tiene_i_picchi_sotto_fondo_scala() {
        // Tutte e dieci al massimo, che è il modo più veloce di far saturare un
        // equalizzatore che non si difende. Il seno si prova a più frequenze
        // perché il punto peggiore non è dove verrebbe da cercarlo: dieci
        // campane larghe un'ottava si sovrappongono, e il massimo della cascata
        // sta in mezzo allo spettro, non sulla banda più alzata.
        for hz in [60.0, 250.0, 1000.0, 3000.0, 8000.0] {
            let mut stato = con(&[LIMITE_DB; BANDE], 48_000);
            let mut massimo = 0.0f32;
            for n in 0..QUANTI {
                massimo = massimo.max(stato.applica(seno(hz, n, 48_000.0), 0).abs());
            }
            assert!(
                massimo <= 1.02,
                "a {hz} Hz il picco è {massimo}: l'uscita saturerebbe"
            );
        }
    }

    #[test]
    fn il_preamp_non_attenua_piu_del_necessario() {
        // L'errore opposto, altrettanto sbagliato: attenuare tanto da rendere
        // inutile il rialzo. Con una sola banda alzata di dodici decibel, al suo
        // centro si deve tornare circa a uno — non a un quarto.
        let coefficienti = Coefficienti::calcola(&sola(5, 12.0), true, 48_000);
        let atteso = 10.0f32.powf(-12.0 / 20.0);
        let scarto = (coefficienti.preamp() - atteso).abs();
        assert!(
            scarto < 0.02,
            "preamp {} invece di circa {atteso}",
            coefficienti.preamp()
        );
    }

    #[test]
    fn le_bande_troppo_alte_per_il_dispositivo_si_spengono() {
        // Un dispositivo a 32 kHz: Nyquist cade a 16 kHz, cioè esattamente sul
        // centro dell'ultima banda. Deve valere identità, non coefficienti
        // inventati.
        let guadagni = sola(BANDE - 1, 12.0);
        assert!(
            Coefficienti::calcola(&guadagni, true, 32_000).piatto(),
            "l'unica banda mossa era oltre il limite: non resta niente da fare"
        );
        // A 48 kHz invece quella stessa banda si accende.
        assert!(!Coefficienti::calcola(&guadagni, true, 48_000).piatto());
    }

    #[test]
    fn una_curva_corta_o_lunga_non_e_un_problema() {
        // Il caso vero: un `player.eq` scritto da una versione con un numero di
        // bande diverso.
        assert!(Coefficienti::calcola(&[], true, 48_000).piatto());
        assert!(!Coefficienti::calcola(&[6.0], true, 48_000).piatto());
        assert!(!Coefficienti::calcola(&[3.0f32; BANDE + 5], true, 48_000).piatto());
    }

    #[test]
    fn un_valore_impossibile_nella_curva_non_rompe_i_filtri() {
        let guadagni = [
            f32::NAN,
            f32::INFINITY,
            9_999.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        let coefficienti = Coefficienti::calcola(&guadagni, true, 48_000);
        // Il terzo si taglia a +12 e resta l'unico mosso; i primi due valgono
        // zero invece di propagarsi.
        assert!(!coefficienti.piatto());
        assert!(coefficienti.preamp() > 0.0 && coefficienti.preamp() <= 1.0);

        let mut stato = Stato::nuovo();
        stato.aggiorna(coefficienti);
        for n in 0..1000 {
            let campione = if n % 3 == 0 { 0.5 } else { -0.5 };
            let fuori = stato.applica(campione, 0);
            assert!(fuori.is_finite(), "campione {n} non finito: {fuori}");
        }
    }

    #[test]
    fn una_frequenza_di_zero_non_divide_per_zero() {
        assert!(Coefficienti::calcola(&[9.0; BANDE], true, 0).piatto());
    }

    #[test]
    fn un_canale_oltre_quelli_che_teniamo_non_indicizza_fuori() {
        let mut stato = con(&[6.0; BANDE], 48_000);
        assert!((stato.applica(0.5, CANALI_MAX) - 0.5).abs() < f32::EPSILON);
        assert!((stato.applica(0.5, usize::MAX) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn cambiare_curva_a_meta_flusso_non_fa_esplodere_niente() {
        let mut stato = con(&[LIMITE_DB; BANDE], 48_000);
        for n in 0..2000 {
            let _ = stato.applica(seno(440.0, n, 48_000.0), 0);
        }
        // Il salto peggiore: da tutto alzato a tutto abbassato, di colpo.
        stato.aggiorna(Coefficienti::calcola(&[-LIMITE_DB; BANDE], true, 48_000));
        for n in 2000..6000 {
            let fuori = stato.applica(seno(440.0, n, 48_000.0), 0);
            assert!(
                fuori.is_finite() && fuori.abs() < 4.0,
                "campione {n}: {fuori}"
            );
        }
    }

    #[test]
    fn azzerare_dimentica_il_passato_e_tiene_la_curva() {
        let mut stato = con(&[LIMITE_DB; BANDE], 48_000);
        for _ in 0..500 {
            let _ = stato.applica(0.9, 0);
        }
        stato.azzera();
        assert!(!stato.piatto(), "la curva doveva restare");
        // Azzerato, il primo campione è identico a quello di un filtro appena
        // costruito con la stessa curva.
        let mut pulito = con(&[LIMITE_DB; BANDE], 48_000);
        let scarto = (stato.applica(0.9, 0) - pulito.applica(0.9, 0)).abs();
        assert!(scarto < 1e-6, "scarto {scarto}");
    }

    #[test]
    fn i_canali_hanno_memorie_separate() {
        // Se le condividessero, un impulso a sinistra si sentirebbe a destra.
        let mut stato = con(&[LIMITE_DB; BANDE], 48_000);
        for _ in 0..200 {
            let _ = stato.applica(0.9, 0);
        }
        let destro = stato.applica(0.0, 1);
        assert!(
            destro.abs() < 1e-9,
            "il canale destro non ha mai ricevuto niente e vale {destro}"
        );
    }

    #[test]
    fn il_blocco_interlacciato_rispetta_i_canali() {
        let sorgente = [0.5f32, -0.5, 0.4, -0.4, 0.3, -0.3];
        let mut dati = sorgente;
        con(&[LIMITE_DB; BANDE], 48_000).applica_blocco(&mut dati, 2);
        // Lo stesso conto, fatto a mano campione per campione.
        let mut a_mano = con(&[LIMITE_DB; BANDE], 48_000);
        let atteso: Vec<f32> = sorgente
            .iter()
            .enumerate()
            .map(|(i, &v)| a_mano.applica(v, i % 2))
            .collect();
        assert_eq!(dati.to_vec(), atteso);
    }

    #[test]
    fn tutte_le_curve_di_serie_stanno_nei_limiti() {
        for (nome, curva) in PRESET_DI_SERIE {
            for (banda, db) in curva.iter().enumerate() {
                assert!(
                    db.abs() <= LIMITE_DB,
                    "«{nome}» banda {banda}: {db} dB è fuori dal limite"
                );
            }
        }
    }

    #[test]
    fn la_prima_curva_di_serie_e_quella_che_non_fa_niente() {
        // «Piatto» in cima non è un vezzo: è il modo in cui si torna indietro.
        let Some(&(nome, curva)) = PRESET_DI_SERIE.first() else {
            panic!("l'elenco dei preset non può essere vuoto");
        };
        assert_eq!(nome, "Piatto");
        assert!(Coefficienti::calcola(&curva, true, 48_000).piatto());
    }

    #[test]
    fn la_griglia_della_risposta_cade_sui_centri_delle_bande() {
        // Il difetto che questa prova impedisce: una griglia che passa **fra**
        // i centri misura un picco più basso di quello vero, la
        // preamplificazione attenua meno del necessario, e l'uscita satura.
        assert!(
            (PASSO_PUNTI.powi(8) - 2.0).abs() < 1e-12,
            "otto passi devono fare un'ottava esatta"
        );
        let primo = CENTRI_HZ.first().copied().unwrap_or(0.0);
        assert!((f64::from(primo) - PRIMO_PUNTO_HZ).abs() < 1e-9);
        // La griglia deve arrivare oltre l'ultimo centro: là sopra ogni campana
        // è già tornata a zero decibel, quindi non c'è nessun picco che possa
        // sfuggire, ma fermarsi *prima* dell'ultimo centro sì.
        let ultimo_punto = PRIMO_PUNTO_HZ * PASSO_PUNTI.powi(88);
        let ultimo_centro = f64::from(CENTRI_HZ.last().copied().unwrap_or(0.0));
        assert!(
            ultimo_punto > ultimo_centro * 2.0,
            "la griglia si ferma a {ultimo_punto} Hz, con l'ultima banda a {ultimo_centro}"
        );
    }

    #[test]
    fn i_centri_sono_ottave_esatte() {
        for coppia in CENTRI_HZ.windows(2) {
            let (Some(&basso), Some(&alto)) = (coppia.first(), coppia.get(1)) else {
                continue;
            };
            assert!(
                (alto / basso - 2.0).abs() < 1e-4,
                "{basso} Hz e {alto} Hz non distano un'ottava"
            );
        }
    }
}
