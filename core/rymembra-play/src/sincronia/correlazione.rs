//! Di quanto un canale arriva dopo l'altro, in una registrazione stereo.
//!
//! È lo strumento della misura acustica: il PC A suona nel canale sinistro di
//! un ingresso di linea, il PC B nel destro, e quello che conta è il ritardo
//! fra i due, non quello che ciascuno dice di sé. Qui c'è solo il conto; la
//! lettura del file e le finestre stanno in `examples/misura_sfasamento.rs`.
//!
//! # Il segno
//!
//! **Positivo vuol dire destra in ritardo**: se `destra[n + Δ] = sinistra[n]`,
//! la misura è `+Δ`. Con A a sinistra e B a destra, positivo è «B suona dopo
//! A». Lo stesso segno lo usano i due esempi, e la misura a due bande.
//!
//! # Perché la correlazione diretta e non la FFT
//!
//! La finestra è un secondo a 48 kHz e il ritardo si cerca entro ±50 ms:
//! 4801 sfasamenti per 48 000 prodotti, poco più di duecento milioni di
//! moltiplicazioni. In una build ottimizzata sono qualche centesimo di
//! secondo, e la misura gira a mano, non dentro la callback. La FFT
//! risparmierebbe un ordine di grandezza al prezzo di una dipendenza (o di una
//! trasformata scritta qui) e del riempimento a zeri da far tornare giusto ai
//! bordi; la somma diretta invece è quella della definizione, e si controlla a
//! occhio. Il prodotto scalare tiene otto accumulatori separati: senza, ogni
//! somma aspetterebbe la precedente e il compilatore non potrebbe affiancarle.
//!
//! # Perché normalizzata sull'energia di tutta la finestra
//!
//! Il prodotto a ogni sfasamento si divide per la radice delle energie dei
//! **due canali interi**, non dei soli pezzi che a quello sfasamento si
//! sovrappongono. Il valore sta fra −1 e 1, vale 1 per la stessa forma d'onda
//! soltanto spostata (purché tutta dentro la finestra), e diventa la
//! `qualita` della misura.
//!
//! La normalizzazione sui soli pezzi sovrapposti sembra più giusta, ed è stata
//! la prima scritta. Ma agli sfasamenti estremi i pezzi sono corti, e se lì
//! dentro non c'è nessuna raffica restano due tratti di fondo: due rumori
//! qualunque, o — con gli inviluppi della misura a due bande — due tratti
//! quasi costanti, che fra loro correlano quasi perfettamente. Ne veniva un
//! «picco» al bordo alto quanto quello vero. Con l'energia di tutta la finestra
//! un tratto di solo fondo pesa quanto pesa davvero: quasi niente.
//!
//! # Sotto il campione
//!
//! A 48 kHz un campione è 20,8 µs; la misura deve stare ben sotto il
//! millisecondo, e due misure che oscillano di un campione sporcherebbero la
//! tendenza. La parabola per i tre punti attorno al picco ne dà la posizione
//! fra un campione e l'altro. Su una raffica a 4 kHz il picco della
//! correlazione è un coseno con dodici campioni per periodo, e lì la parabola
//! sbaglia di qualche millesimo di campione: niente, rispetto a quel che serve.
//!
//! # Quando non rispondere
//!
//! Meglio nessuna misura che una sbagliata: una finestra rifiutata si scarta,
//! una misura falsa sposta la mediana. Si risponde `None` se:
//!
//! - l'energia di un canale è troppo bassa ([`ENERGIA_MINIMA`]): è silenzio, o
//!   il rumore di fondo dell'ingresso;
//! - il picco sta sul bordo dell'intervallo cercato: il ritardo vero è fuori,
//!   e il massimo trovato è solo il punto più alto della discesa;
//! - il picco è basso ([`QUALITA_MINIMA`]): i due canali non si somigliano;
//! - il picco è **ambiguo**: fuori dal suo intorno c'è un altro massimo che ne
//!   vale più dell'80% ([`AMBIGUITA`]).
//!
//! L'intorno escluso, [`INTORNO_MS`], è la scelta che fa funzionare
//! l'ultima regola. Una raffica a 4 kHz ha una correlazione che oscilla a
//! 4 kHz dentro l'inviluppo: il picco vicino, un periodo più in là (0,25 ms),
//! vale circa il 90% del principale. Contato come «secondo picco», ogni
//! raffica sarebbe ambigua. Un millisecondo è mezza raffica: lì l'inviluppo è
//! già sceso sotto il 20%, mentre un segnale periodico — un seno, una fila di
//! clic regolari — ha un altro picco alto quanto il primo a ogni periodo, e
//! oltre il millisecondo lo si trova. Detto altrimenti: entro un millisecondo
//! dal picco si può ancora sbagliare di un periodo della portante, e quello
//! la regola non lo vede; oltre, sì.
//!
//! # Con un microfono solo: due bande in un canale
//!
//! Senza un ingresso di linea stereo, A e B finiscono nello stesso canale
//! (il microfono del portatile), e non c'è più un «sinistro» e un «destro» da
//! confrontare. Allora A suona le raffiche a una frequenza (4 kHz) e B le
//! stesse raffiche, con gli stessi tempi, a un'altra (7 kHz): stessa linea del
//! tempo, spettri separati. [`sfasamento_fra_bande`] separa le due bande,
//! ne prende gli **inviluppi** e li correla come se fossero i due canali.
//! Positivo vuol dire, di nuovo, B in ritardo.
//!
//! Le scelte, e il loro perché:
//!
//! - **Fase zero.** Un passa-banda ritarda il suo inviluppo del ritardo di
//!   gruppo, che per un biquad vale circa `Q/(π·f₀)` e cambia con la
//!   frequenza: a 4 e a 7 kHz la differenza è un quarto di millisecondo, cioè
//!   l'errore che si vuole misurare. Il filtro passa in avanti e poi
//!   all'indietro, e il ritardo si annulla esattamente.
//! - **Stessa larghezza di banda, non stesso Q.** I biquad delle due bande
//!   sono larghi tutti [`LARGHEZZA_BANDA_HZ`]: così allargano i due inviluppi
//!   allo stesso modo, e le due forme da correlare sono uguali. Con lo stesso
//!   Q la banda a 7 kHz sarebbe quasi il doppio di quella a 4.
//! - **Due biquad per banda.** Con uno solo, la portante di B a 7 kHz entra
//!   nella banda di A attenuata di soli 27 dB (andata e ritorno): se B suona
//!   più forte di A, nella banda di A c'è un'eco di B che sposta la misura. Con
//!   due in cascata, sempre in avanti e all'indietro, la portante dell'altra
//!   banda passa sotto i −50 dB, e la banda risultante è larga circa 440 Hz:
//!   una raffica di 2 ms ne esce più larga, ma uguale nelle due bande.
//! - **La guardia della diafonia.** Anche così, se B tace la sua banda contiene
//!   l'eco di A, ed è un'eco perfettamente allineata: la misura direbbe «0 ms»,
//!   cioè «perfettamente a tempo», proprio quando B non c'è. Allora ciascuna
//!   banda deve superare di almeno 20 dB ([`MARGINE_DIAFONIA`]) quello che
//!   l'altra ci lascerebbe passare, calcolato dalla risposta dei filtri;
//!   altrimenti `None`. Vuol dire che B può arrivare al microfono fino a una
//!   trentina di dB più piano di A, e viceversa.
//! - **Inviluppo e non portante.** Le due portanti sono diverse, quindi non si
//!   possono correlare fra loro; gli inviluppi sì. L'inviluppo è il valore
//!   assoluto lisciato con una media mobile di mezzo millisecondo, che toglie
//!   l'ondulazione al doppio della portante; poi gli si toglie la media della
//!   finestra e lo si porta a energia unitaria, perché la correlazione guardi
//!   la forma e non il livello. Il picco della correlazione di due inviluppi è
//!   largo, qualche millisecondo: l'intorno senza secondi picchi è quindi più
//!   largo ([`INTORNO_INVILUPPO_MS`]), e non c'è la portante a renderlo
//!   ambiguo. La parabola lo trova lo stesso al centesimo di millisecondo.

/// Una misura di sfasamento.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Misura {
    /// Di quanto la destra (o la banda di B) arriva dopo la sinistra (o la
    /// banda di A), in millisecondi. Negativo se arriva prima.
    pub ms: f64,
    /// La correlazione normalizzata al picco, fra 0 e 1.
    pub qualita: f32,
}

/// Sotto questa energia media per campione un canale vale silenzio.
///
/// 1e-8 è −80 dBFS efficaci: sotto il rumore di qualunque ingresso di linea
/// acceso, ben sotto una raffica registrata anche piano (una sola raffica da
/// 2 ms a −26 dBFS in un secondo vale già 1e-6).
pub const ENERGIA_MINIMA: f64 = 1e-8;

/// Sotto questa correlazione i due canali non si somigliano abbastanza.
///
/// Due rumori indipendenti, su un secondo a 48 kHz, arrivano a qualche
/// centesimo; la stessa raffica uscita da due schede diverse sta sopra 0,9.
/// Non è nella specifica: si aggiunge alle due regole che ci sono, così un
/// rumore che per caso avesse un massimo isolato non passa per una misura.
pub const QUALITA_MINIMA: f64 = 0.2;

/// Un secondo picco che vale più di questa frazione del primo rende la misura
/// ambigua.
pub const AMBIGUITA: f64 = 0.8;

/// L'intorno del picco in cui non si cerca il secondo, in millisecondi.
///
/// Vedi la documentazione del modulo per il perché di un millisecondo.
pub const INTORNO_MS: f64 = 1.0;

/// Lo stesso intorno per la correlazione di due inviluppi, che ha un picco
/// solo ma largo: 4 ms.
pub const INTORNO_INVILUPPO_MS: f64 = 4.0;

/// La larghezza di ciascun biquad dei filtri di [`inviluppo`], in Hz.
pub const LARGHEZZA_BANDA_HZ: f64 = 1000.0;

/// Di quanto una banda deve superare la diafonia dall'altra: 10, cioè 20 dB.
pub const MARGINE_DIAFONIA: f64 = 10.0;

/// La media mobile che liscia l'inviluppo, in millisecondi.
const LISCIATURA_MS: f64 = 0.5;

/// Di quanto `destra` arriva dopo `sinistra`, cercato entro ±`massimo_ms`.
///
/// Si usa la parte comune dei due canali (la lunghezza minore). `None` se la
/// misura non è affidabile: vedi la documentazione del modulo.
#[must_use]
pub fn sfasamento(
    sinistra: &[f32],
    destra: &[f32],
    frequenza: u32,
    massimo_ms: f64,
) -> Option<Misura> {
    cerca(sinistra, destra, frequenza, massimo_ms, INTORNO_MS)
}

/// Di quanto la banda attorno a `hz_b` arriva dopo quella attorno a `hz_a`,
/// in un segnale solo che le contiene tutte e due, cercato entro
/// ±`massimo_ms`.
///
/// Vedi «Con un microfono solo» nella documentazione del modulo. `None` alle
/// stesse condizioni di [`sfasamento`], se una banda non supera la diafonia
/// dall'altra, o se una delle due frequenze non sta fra zero e metà della
/// frequenza di campionamento.
#[must_use]
pub fn sfasamento_fra_bande(
    segnale: &[f32],
    frequenza: u32,
    hz_a: f64,
    hz_b: f64,
    massimo_ms: f64,
) -> Option<Misura> {
    let f = f64::from(frequenza);
    let banda_a = Banda::nuova(f, hz_a)?;
    let banda_b = Banda::nuova(f, hz_b)?;
    let a = banda_a.inviluppo(segnale, f)?;
    let b = banda_b.inviluppo(segnale, f)?;
    // Quanto della portante dell'una passa nella banda dell'altra.
    let da_a_in_b = banda_b.guadagno(f, hz_a);
    let da_b_in_a = banda_a.guadagno(f, hz_b);
    let (sopra_a, sopra_b) = (sopra_il_fondo(&a), sopra_il_fondo(&b));
    if sopra_b <= MARGINE_DIAFONIA * da_a_in_b * sopra_a
        || sopra_a <= MARGINE_DIAFONIA * da_b_in_a * sopra_b
    {
        return None;
    }
    cerca(
        &normalizzato(a),
        &normalizzato(b),
        frequenza,
        massimo_ms,
        INTORNO_INVILUPPO_MS,
    )
}

/// L'inviluppo della banda attorno a `centro_hz`: due biquad passa-banda a
/// fase zero, valore assoluto, media mobile simmetrica di mezzo millisecondo.
///
/// Ha la scala del segnale: un seno lungo d'ampiezza 0,5 al centro della
/// banda dà circa 0,3 (il valore medio di un seno raddrizzato è 2/π del suo
/// picco); una raffica di 2 ms, che il filtro allarga, un po' meno. È quello
/// che serve per decidere se in una banda c'è qualcosa. `None` se
/// `centro_hz` non sta fra zero e metà di `frequenza`.
#[must_use]
pub fn inviluppo(segnale: &[f32], frequenza: u32, centro_hz: f64) -> Option<Vec<f32>> {
    let f = f64::from(frequenza);
    Banda::nuova(f, centro_hz)?.inviluppo(segnale, f)
}

/// Il motore comune: correlazione, picco, ambiguità entro `intorno_ms`,
/// parabola.
fn cerca(
    sinistra: &[f32],
    destra: &[f32],
    frequenza: u32,
    massimo_ms: f64,
    intorno_ms: f64,
) -> Option<Misura> {
    let n = sinistra.len().min(destra.len());
    let sinistra = sinistra.get(..n)?;
    let destra = destra.get(..n)?;
    if frequenza == 0 || n < 4 || !massimo_ms.is_finite() || massimo_ms <= 0.0 {
        return None;
    }
    let f = f64::from(frequenza);
    // Almeno metà della finestra deve sovrapporsi anche allo sfasamento più
    // grande: oltre, la correlazione si fa su un pezzo troppo corto per dire
    // qualcosa.
    let massimo = intero_non_negativo((massimo_ms * f / 1000.0).floor())?.min(n.checked_shr(1)?);
    if massimo < 2 {
        return None;
    }

    let energia_s = prodotto(sinistra, sinistra);
    let energia_d = prodotto(destra, destra);
    let soglia = ENERGIA_MINIMA * conta(n);
    if energia_s < soglia || energia_d < soglia {
        return None;
    }
    let denominatore = (energia_s * energia_d).sqrt();
    if !(denominatore > 0.0 && denominatore.is_finite()) {
        return None;
    }

    // r[i] è la correlazione allo sfasamento i − massimo.
    let mut r = Vec::with_capacity(massimo.checked_mul(2)?.checked_add(1)?);
    for i in 0..=massimo.checked_mul(2)? {
        let somma = if i >= massimo {
            let k = i - massimo;
            // destra[n + k] contro sinistra[n].
            prodotto(sinistra.get(..n - k)?, destra.get(k..)?)
        } else {
            let m = massimo - i;
            // sinistra[n + m] contro destra[n].
            prodotto(sinistra.get(m..)?, destra.get(..n - m)?)
        };
        r.push(somma / denominatore);
    }

    let (picco, valore) = r
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if picco == 0 || picco == r.len() - 1 || valore < QUALITA_MINIMA {
        return None;
    }

    let intorno = intero_non_negativo((intorno_ms * f / 1000.0).round())?;
    let secondo = r
        .iter()
        .enumerate()
        .filter(|(i, _)| i.abs_diff(picco) > intorno)
        .map(|(_, v)| *v)
        .fold(f64::NEG_INFINITY, f64::max);
    if secondo > AMBIGUITA * valore {
        return None;
    }

    let prima = *r.get(picco - 1)?;
    let dopo = *r.get(picco + 1)?;
    let curvatura = prima - 2.0 * valore + dopo;
    let fra = if curvatura < 0.0 {
        (0.5 * (prima - dopo) / curvatura).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let campioni = conta(picco) - conta(massimo) + fra;
    Some(Misura {
        ms: campioni * 1000.0 / f,
        qualita: qualita(valore),
    })
}

/// Quanto un inviluppo sale sopra la sua media: il livello di quel che c'è
/// nella banda, senza il fondo.
fn sopra_il_fondo(x: &[f32]) -> f64 {
    let n = conta(x.len().max(1));
    let media = x.iter().map(|v| f64::from(*v)).sum::<f64>() / n;
    let picco = x.iter().map(|v| f64::from(*v)).fold(0.0, f64::max);
    (picco - media).max(0.0)
}

/// Senza la media della finestra, e con energia media unitaria: la
/// correlazione guarda la forma, non il livello. Un inviluppo piatto resta
/// tutto zero, e la correlazione lo scarta per l'energia.
fn normalizzato(mut x: Vec<f32>) -> Vec<f32> {
    let n = conta(x.len().max(1));
    let media = x.iter().map(|v| f64::from(*v)).sum::<f64>() / n;
    let energia = x
        .iter()
        .map(|v| (f64::from(*v) - media).powi(2))
        .sum::<f64>()
        / n;
    let scala = if energia > 0.0 && energia.is_finite() {
        energia.sqrt().recip()
    } else {
        0.0
    };
    for v in &mut x {
        *v = campione((f64::from(*v) - media) * scala);
    }
    x
}

/// Il filtro di una banda: due biquad uguali in cascata, passati in avanti e
/// poi all'indietro.
#[derive(Debug, Clone, Copy)]
struct Banda {
    biquad: Biquad,
}

impl Banda {
    fn nuova(frequenza: f64, centro: f64) -> Option<Self> {
        Some(Self {
            biquad: Biquad::passa_banda(frequenza, centro, centro / LARGHEZZA_BANDA_HZ)?,
        })
    }

    /// Il guadagno di tutta la banda (quattro passaggi) a `hz`.
    fn guadagno(&self, frequenza: f64, hz: f64) -> f64 {
        self.biquad
            .guadagno(std::f64::consts::TAU * hz / frequenza)
            .powi(4)
    }

    /// L'inviluppo: vedi [`inviluppo`].
    fn inviluppo(&self, segnale: &[f32], frequenza: f64) -> Option<Vec<f32>> {
        let mut x: Vec<f64> = segnale.iter().map(|v| f64::from(*v)).collect();
        self.biquad.filtra(&mut x);
        self.biquad.filtra(&mut x);
        x.reverse();
        self.biquad.filtra(&mut x);
        self.biquad.filtra(&mut x);
        x.reverse();

        // La media mobile, centrata: 2·lato + 1 campioni.
        let lato = intero_non_negativo((LISCIATURA_MS * frequenza / 2000.0).round())?;
        let mut somme = Vec::with_capacity(x.len() + 1);
        let mut totale = 0.0f64;
        somme.push(totale);
        for v in &x {
            totale += v.abs();
            somme.push(totale);
        }
        let n = x.len();
        let mut fuori = Vec::with_capacity(n);
        for i in 0..n {
            let da = i.saturating_sub(lato);
            let a = (i + lato + 1).min(n);
            let somma = somme.get(a)? - somme.get(da)?;
            fuori.push(campione(somma / conta(a - da)));
        }
        Some(fuori)
    }
}

/// Un biquad, nella forma trasposta: è quella che regge meglio i numeri
/// piccoli di una banda stretta.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    /// Il passa-banda del ricettario di Robert Bristow-Johnson, con guadagno
    /// unitario al centro. `None` se il centro non sta fra zero e Nyquist.
    fn passa_banda(frequenza: f64, centro: f64, q: f64) -> Option<Self> {
        let w0 = std::f64::consts::TAU * centro / frequenza;
        if !(w0 > 0.0 && w0 < std::f64::consts::PI && q > 0.0 && q.is_finite()) {
            return None;
        }
        let alfa = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alfa;
        Some(Self {
            b0: alfa / a0,
            b2: -alfa / a0,
            a1: -2.0 * w0.cos() / a0,
            a2: (1.0 - alfa) / a0,
        })
    }

    /// Il modulo della risposta alla pulsazione `w` (radianti per campione).
    fn guadagno(&self, w: f64) -> f64 {
        // (b0 + b2·e^{−2jw}) / (1 + a1·e^{−jw} + a2·e^{−2jw})
        let (re_n, im_n) = (
            self.b0 + self.b2 * (2.0 * w).cos(),
            -self.b2 * (2.0 * w).sin(),
        );
        let (re_d, im_d) = (
            1.0 + self.a1 * w.cos() + self.a2 * (2.0 * w).cos(),
            -self.a1 * w.sin() - self.a2 * (2.0 * w).sin(),
        );
        (re_n.hypot(im_n) / re_d.hypot(im_d)).min(1.0)
    }

    /// Filtra sul posto, partendo da fermo.
    fn filtra(&self, x: &mut [f64]) {
        let (mut z1, mut z2) = (0.0f64, 0.0f64);
        for v in x {
            let ingresso = *v;
            let uscita = self.b0 * ingresso + z1;
            // Il termine b1 di un passa-banda è zero.
            z1 = -self.a1 * uscita + z2;
            z2 = self.b2 * ingresso - self.a2 * uscita;
            *v = uscita;
        }
    }
}

/// Un campione con la virgola come `f32`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "un inviluppo nella scala del segnale audio: un f32 è la sua precisione naturale"
)]
fn campione(x: f64) -> f32 {
    x as f32
}

/// Il prodotto scalare, con otto accumulatori indipendenti.
///
/// Otto alla volta e scritti per esteso, non con un iteratore per campione:
/// in una build ottimizzata non cambia niente, in una di prova (dove ogni
/// passo d'iteratore è una chiamata) fa la differenza fra secondi e decimi.
fn prodotto(a: &[f32], b: &[f32]) -> f64 {
    let mut s = [0.0f64; 8];
    let (pezzi_a, resto_a) = a.as_chunks::<8>();
    let (pezzi_b, resto_b) = b.as_chunks::<8>();
    for (pa, pb) in pezzi_a.iter().zip(pezzi_b) {
        let [a0, a1, a2, a3, a4, a5, a6, a7] = *pa;
        let [b0, b1, b2, b3, b4, b5, b6, b7] = *pb;
        let [s0, s1, s2, s3, s4, s5, s6, s7] = &mut s;
        *s0 += a0 as f64 * b0 as f64;
        *s1 += a1 as f64 * b1 as f64;
        *s2 += a2 as f64 * b2 as f64;
        *s3 += a3 as f64 * b3 as f64;
        *s4 += a4 as f64 * b4 as f64;
        *s5 += a5 as f64 * b5 as f64;
        *s6 += a6 as f64 * b6 as f64;
        *s7 += a7 as f64 * b7 as f64;
    }
    let mut totale: f64 = s.iter().sum();
    for (x, y) in resto_a.iter().zip(resto_b) {
        totale += f64::from(*x) * f64::from(*y);
    }
    totale
}

/// Un conteggio di campioni come numero con la virgola.
fn conta(n: usize) -> f64 {
    u32::try_from(n).map_or(f64::from(u32::MAX), f64::from)
}

/// Un numero già arrotondato, come indice.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "il valore è già intero, non negativo e controllato contro u32::MAX: \
              la conversione è esatta"
)]
fn intero_non_negativo(x: f64) -> Option<usize> {
    if x.is_finite() && (0.0..=f64::from(u32::MAX)).contains(&x) {
        usize::try_from(x as u32).ok()
    } else {
        None
    }
}

/// La correlazione al picco come `f32`, fra 0 e 1.
#[expect(
    clippy::cast_possible_truncation,
    reason = "un valore fra 0 e 1: la precisione di un f32 basta e avanza per una qualità"
)]
fn qualita(valore: f64) -> f32 {
    valore.clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un generatore pseudo-casuale con seme fisso: le prove danno sempre lo
    /// stesso risultato.
    struct Lcg(u64);

    impl Lcg {
        fn uniforme(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }

        /// Circa gaussiano, con varianza 1: la somma di dodici uniformi.
        fn rumore(&mut self) -> f64 {
            (0..12).map(|_| self.uniforme()).sum::<f64>() - 6.0
        }
    }

    /// Una raffica di 2 ms a 4 kHz con inviluppo di Hann, che comincia in
    /// `inizio` secondi, valutata nell'istante `t`: la funzione continua,
    /// così un ritardo di una frazione di campione è esatto.
    fn raffica(t: f64, inizio: f64) -> f64 {
        raffica_a(t, inizio, 4000.0)
    }

    /// La stessa raffica, con la portante a `hz`.
    fn raffica_a(t: f64, inizio: f64, hz: f64) -> f64 {
        let durata = 0.002;
        let tau = t - inizio;
        if !(0.0..durata).contains(&tau) {
            return 0.0;
        }
        let inviluppo = (std::f64::consts::PI * tau / durata).sin().powi(2);
        0.5 * inviluppo * (std::f64::consts::TAU * hz * tau).sin()
    }

    /// Un campione per il canale: la precisione di un `f32` è quella della
    /// scheda audio.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "un segnale di prova fra −1 e 1: un f32 è la sua precisione naturale"
    )]
    fn campione(x: f64) -> f32 {
        x as f32
    }

    /// Una finestra di `durata` secondi con le raffiche che cominciano negli
    /// istanti `raffiche`, e la sua copia ritardata di `ritardo_ms`; sotto,
    /// in tutti e due i canali, un fondo di rumore indipendente a −70 dB.
    fn coppia(
        frequenza: u32,
        ritardo_ms: f64,
        durata: f64,
        raffiche: &[f64],
    ) -> (Vec<f32>, Vec<f32>) {
        let f = f64::from(frequenza);
        let mut caso = Lcg(7);
        let segnale = |t: f64| raffiche.iter().map(|&r| raffica(t, r)).sum::<f64>();
        let mut sinistra = Vec::new();
        let mut destra = Vec::new();
        let mut n = 0u32;
        while f64::from(n) < durata * f {
            let t = f64::from(n) / f;
            sinistra.push(campione(segnale(t) + 3e-4 * caso.rumore()));
            destra.push(campione(
                segnale(t - ritardo_ms / 1000.0) + 3e-4 * caso.rumore(),
            ));
            n += 1;
        }
        (sinistra, destra)
    }

    /// Un decimo di secondo con una raffica: abbastanza per ogni ritardo fino
    /// a ±50 ms, e dieci volte meno conti di una finestra intera — in una
    /// build di prova, non ottimizzata, sono la differenza fra un attimo e un
    /// minuto.
    fn breve(frequenza: u32, ritardo_ms: f64) -> (Vec<f32>, Vec<f32>) {
        coppia(frequenza, ritardo_ms, 0.1, &[0.045])
    }

    #[test]
    fn ritrova_il_ritardo_delle_raffiche_a_44_1_e_a_48_khz() {
        for frequenza in [48_000, 44_100] {
            for ritardo in [0.0, 0.3, 5.0, 17.25, 40.0, -5.0, -17.25] {
                let (s, d) = breve(frequenza, ritardo);
                let misura = sfasamento(&s, &d, frequenza, 50.0)
                    .unwrap_or_else(|| panic!("nessuna misura per {ritardo} ms a {frequenza} Hz"));
                assert!(
                    (misura.ms - ritardo).abs() < 0.005,
                    "a {frequenza} Hz: attesi {ritardo} ms, misurati {} ms",
                    misura.ms
                );
                assert!(misura.qualita > 0.9, "qualità {}", misura.qualita);
            }
        }
    }

    #[test]
    fn ritrova_il_ritardo_anche_sulla_finestra_vera_da_un_secondo() {
        // La misura com'è usata: un secondo a 48 kHz, ±50 ms, due raffiche.
        let (s, d) = coppia(48_000, 17.25, 1.0, &[0.23, 0.71]);
        let misura = sfasamento(&s, &d, 48_000, 50.0).expect("una misura");
        assert!((misura.ms - 17.25).abs() < 0.005, "{} ms", misura.ms);
    }

    #[test]
    fn su_due_rumori_indipendenti_non_risponde() {
        let mut caso = Lcg(42);
        let s: Vec<f32> = (0..9600).map(|_| campione(0.1 * caso.rumore())).collect();
        let d: Vec<f32> = (0..9600).map(|_| campione(0.1 * caso.rumore())).collect();
        assert_eq!(sfasamento(&s, &d, 48_000, 50.0), None);
    }

    #[test]
    fn su_un_seno_non_risponde_perche_ogni_periodo_e_un_picco() {
        // Lo stesso seno a 1 kHz nei due canali, il destro in ritardo di
        // 0,3 ms: un ritardo che esiste, ma che un seno non può dire, perché
        // 1,3 ms, 2,3 ms e così via gli somigliano tutti allo stesso modo.
        let f = 48_000.0;
        let seno = |t: f64| campione(0.5 * (std::f64::consts::TAU * 1000.0 * t).sin());
        let s: Vec<f32> = (0..9600).map(|n| seno(f64::from(n) / f)).collect();
        let d: Vec<f32> = (0..9600).map(|n| seno(f64::from(n) / f - 0.0003)).collect();
        assert_eq!(sfasamento(&s, &d, 48_000, 50.0), None);
    }

    #[test]
    fn su_una_fila_regolare_di_raffiche_non_risponde() {
        // Una raffica ogni 10 ms: dentro ±50 ms ci sono dieci allineamenti
        // buoni uguali. Una raffica per volta, invece, si misura.
        let f = 48_000.0;
        let segnale = |t: f64| campione(raffica(t.rem_euclid(0.010), 0.004));
        let s: Vec<f32> = (0..9600).map(|n| segnale(f64::from(n) / f)).collect();
        let d: Vec<f32> = (0..9600)
            .map(|n| segnale(f64::from(n) / f - 0.003))
            .collect();
        assert_eq!(sfasamento(&s, &d, 48_000, 50.0), None);
    }

    #[test]
    fn sul_silenzio_e_quasi_silenzio_non_risponde() {
        let zeri = vec![0.0f32; 48_000];
        assert_eq!(sfasamento(&zeri, &zeri, 48_000, 50.0), None);
        // La stessa raffica, ma a −120 dB: la forma c'è, l'energia no.
        let (s, d) = breve(48_000, 5.0);
        let piano = |x: &[f32]| x.iter().map(|v| v * 1e-6).collect::<Vec<f32>>();
        assert_eq!(sfasamento(&piano(&s), &piano(&d), 48_000, 50.0), None);
    }

    #[test]
    fn un_ritardo_fuori_dall_intervallo_cercato_non_diventa_il_bordo() {
        let (s, d) = coppia(48_000, 60.0, 0.2, &[0.05]);
        assert_eq!(sfasamento(&s, &d, 48_000, 50.0), None);
    }

    /// Un canale solo, come dal microfono: le raffiche di A a `hz_a`, quelle
    /// di B a `hz_b` in ritardo di `ritardo_ms` (con la stessa linea del
    /// tempo), più un fondo di rumore a −50 dB. Un decimo di secondo.
    #[expect(
        clippy::integer_division,
        reason = "un decimo di secondo di campioni: 4800 e 4410, divisioni esatte"
    )]
    fn due_bande(frequenza: u32, b: Option<(f64, f64)>, ritardo_ms: f64) -> Vec<f32> {
        let f = f64::from(frequenza);
        let mut caso = Lcg(11);
        (0..frequenza / 10)
            .map(|n| {
                let t = f64::from(n) / f;
                let a = 0.4 * raffica_a(t, 0.045, 4000.0);
                let b = b.map_or(0.0, |(hz, ampiezza)| {
                    ampiezza * raffica_a(t - ritardo_ms / 1000.0, 0.045, hz)
                });
                campione(a + b + 3e-3 * caso.rumore())
            })
            .collect()
    }

    #[test]
    fn con_un_microfono_solo_ritrova_il_ritardo_fra_le_due_bande() {
        for frequenza in [48_000, 44_100] {
            for ritardo in [0.0, 0.3, 5.0, 17.25, -8.0] {
                let segnale = due_bande(frequenza, Some((7000.0, 0.3)), ritardo);
                let misura = sfasamento_fra_bande(&segnale, frequenza, 4000.0, 7000.0, 50.0)
                    .unwrap_or_else(|| panic!("nessuna misura per {ritardo} ms a {frequenza} Hz"));
                assert!(
                    (misura.ms - ritardo).abs() < 0.02,
                    "a {frequenza} Hz: attesi {ritardo} ms, misurati {} ms",
                    misura.ms
                );
                assert!(misura.qualita > 0.9, "qualità {}", misura.qualita);
            }
        }
    }

    #[test]
    fn misura_anche_con_b_molto_piu_forte_o_molto_piu_piano_di_a() {
        // A è 0,4: B da +20 dB a −26 dB rispetto ad A. La diafonia non deve
        // spostare la misura oltre il decimo di millisecondo, nemmeno con un
        // ritardo piccolo, dove l'eco di una banda nell'altra si sovrappone
        // all'inviluppo vero. (A −26 dB lo scostamento c'è, 0,09 ms, ma è il
        // rumore: il fondo a −50 dB lascia a B una ventina di dB.)
        for ampiezza in [4.0, 1.2, 0.1, 0.02] {
            for ritardo in [0.7, 6.0] {
                let segnale = due_bande(48_000, Some((7000.0, ampiezza)), ritardo);
                let misura = sfasamento_fra_bande(&segnale, 48_000, 4000.0, 7000.0, 50.0)
                    .unwrap_or_else(|| panic!("B a {ampiezza}, {ritardo} ms: nessuna misura"));
                assert!(
                    (misura.ms - ritardo).abs() < 0.1,
                    "B a {ampiezza}: attesi {ritardo} ms, misurati {} ms",
                    misura.ms
                );
            }
        }
    }

    #[test]
    fn con_una_banda_sola_non_risponde() {
        let segnale = due_bande(48_000, None, 0.0);
        assert_eq!(
            sfasamento_fra_bande(&segnale, 48_000, 4000.0, 7000.0, 50.0),
            None
        );
    }

    #[test]
    fn una_banda_oltre_nyquist_non_ha_inviluppo() {
        let segnale = due_bande(48_000, Some((7000.0, 0.3)), 0.0);
        assert_eq!(inviluppo(&segnale, 48_000, 30_000.0), None);
        assert_eq!(inviluppo(&segnale, 48_000, 0.0), None);
        assert_eq!(
            sfasamento_fra_bande(&segnale, 48_000, 4000.0, f64::NAN, 50.0),
            None
        );
        // E la scala dell'inviluppo è quella del segnale: un seno lungo
        // d'ampiezza 0,5 al centro della banda dà 2/π·0,5 ≈ 0,32.
        let seno: Vec<f32> = (0..4800)
            .map(|n| {
                campione(0.5 * (std::f64::consts::TAU * 4000.0 * f64::from(n) / 48_000.0).sin())
            })
            .collect();
        let a = inviluppo(&seno, 48_000, 4000.0).unwrap();
        let atteso = 0.5 * 2.0 / std::f64::consts::PI;
        assert!(
            (f64::from(a[2400]) - atteso).abs() < 0.02,
            "a metà {}",
            a[2400]
        );
    }

    #[test]
    fn argomenti_senza_senso_danno_none_e_non_un_panico() {
        let (s, d) = breve(48_000, 5.0);
        assert_eq!(sfasamento(&s, &d, 0, 50.0), None);
        assert_eq!(sfasamento(&s, &d, 48_000, 0.0), None);
        assert_eq!(sfasamento(&s, &d, 48_000, f64::NAN), None);
        assert_eq!(sfasamento(&s, &d, 48_000, -3.0), None);
        assert_eq!(sfasamento(&[], &[], 48_000, 50.0), None);
        assert_eq!(sfasamento(&s[..3], &d[..3], 48_000, 50.0), None);
        // Un intervallo più grande della finestra si accorcia da solo, a metà
        // finestra.
        let corta = sfasamento(&s[..1000], &d[..1000], 48_000, 5000.0);
        assert!(corta.is_none_or(|m| m.ms.abs() <= 500.0 / 48.0));
    }
}
