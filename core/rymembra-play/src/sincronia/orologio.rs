//! L'orologio dell'ospite, visto dal membro: scarto e deriva fra due PC.
//!
//! Le ancore dell'ospite dicono «il punto P si sente all'istante T» con T sul
//! **suo** orologio. Per confrontarle con le mie serve sapere quanto vale T sul
//! mio. I due orologi sono la base dei tempi di processo di ciascun PC
//! (`rymembra_play::tempo`), quindi differiscono di uno scarto qualunque —
//! i due programmi sono partiti in momenti diversi — e di una deriva: due
//! quarzi non battono mai alla stessa frequenza, e qualche decina di parti per
//! milione è normale.
//!
//! # Lo scambio
//!
//! Come in NTP: il membro manda una domanda in `t1` (orologio suo), l'ospite
//! la riceve in `t2` e risponde in `t3` (orologio suo), la risposta arriva in
//! `t4`. Allora
//!
//! - il tempo di andata e ritorno è `rtt = (t4 − t1) − (t3 − t2)`;
//! - lo scarto è `((t2 − t1) + (t3 − t4)) / 2`, ospite meno locale;
//! - e quello scarto è giusto **a meno di ±rtt/2**: è esatto se andata e
//!   ritorno durano uguale, e sbaglia di metà della differenza altrimenti. Lo
//!   scambio non può sapere quale delle due ha preso più tempo.
//!
//! # Perché l'rtt minimo
//!
//! Il ritardo di rete non è rumore attorno a un valore: è un minimo fisso (il
//! cavo, le schede, il sistema) più un'attesa in coda che c'è o non c'è. Uno
//! scambio con l'rtt più basso è quello che non ha fatto code, e quindi quello
//! il cui errore, limitato da rtt/2, è il più piccolo. Per questo:
//!
//! - gli scambi si raccolgono in **secchi da 2 s**, e di ogni secchio vale
//!   solo quello con l'rtt minore: gli altri non portano informazione, solo
//!   più coda;
//! - si scarta lo scambio con `rtt > max(2·rtt_min, rtt_min + 1 ms)`, dove
//!   `rtt_min` è il minimo della finestra: il doppio basta in una rete veloce
//!   (0,2 ms contro 0,4), il millisecondo in più evita di buttare tutto quando
//!   il minimo è già piccolissimo e il rumore del sistema è dello stesso
//!   ordine;
//! - oltre 100 ms si scarta comunque: è una rete che non ha un minimo, solo
//!   code. Sotto, il filtro relativo qui sopra tiene solo gli scambi vicini al
//!   minimo, e l'incertezza della stima — che il pannello mostra — dice quanto
//!   fidarsene. Il tetto era di 20 ms, e su un Wi-Fi carico, con un rtt minimo
//!   che non scende sotto i 25–30 ms, **ogni** scambio diventava «lento»: chi
//!   seguiva non aveva mai una stima, e quindi non partiva mai. Meglio una
//!   stima con qualche millisecondo d'incertezza che un PC muto.
//!
//! # Perché la regressione pesata
//!
//! Con la deriva lo scarto non è una costante ma una retta nel tempo:
//! `scarto(l) = scarto_rif + deriva·(l − riferimento)`. I punti (uno per
//! secchio, sulla finestra degli ultimi 120 s) si adattano ai minimi quadrati,
//! ma non con lo stesso peso: un punto con l'rtt di 0,3 ms sopra il minimo può
//! sbagliare di 0,15 ms, uno sul minimo di quasi niente. Il peso è
//! `1 / (rtt − rtt_min + 50 µs)²` — l'inverso del quadrato dell'errore
//! possibile, come vuole la regressione pesata; i 50 µs sono il rumore che
//! resta anche sul minimo (lo scheduler, il timestamp preso un attimo dopo il
//! `recv`), e impediscono che un solo punto valga infinito.
//!
//! Poi un **secondo passaggio** senza i punti il cui residuo supera tre volte
//! la deviazione mediana: un punto che la rete ha ritardato da una parte sola
//! può avere un rtt buono e uno scarto storto, e il primo adattamento lo
//! scopre come residuo. La mediana e non la deviazione standard, perché è
//! quella che il punto storto non sposta.
//!
//! # Quando fidarsi
//!
//! - Lo scarto si dà dopo **5 punti** (dieci secondi): uno solo potrebbe
//!   essere quello sbagliato.
//! - La deriva solo con **20 s** di dati: su un intervallo più corto la
//!   pendenza la decide il rumore. Prima, lo scarto è la media pesata e la
//!   deriva vale zero.
//! - **Tre residui di fila oltre 10 ms**, o oltre l'rtt dello scambio se è
//!   più grande, vogliono dire che l'altro orologio è cambiato davvero —
//!   l'ospite ha riavviato il programma, e la sua base dei tempi con lui: si
//!   butta tutto e si ricomincia ([`Esito::Ricominciato`]). Uno o due da soli
//!   si scartano: è più probabile un pacchetto rimasto fermo da qualche parte.
//!   L'rtt conta perché uno scambio può sbagliare fino a rtt/2 senza che
//!   niente sia cambiato: su un Wi-Fi da 30 ms una coda tutta all'andata ne
//!   sposta lo scarto di più di 10 ms, e non è un riavvio — quello sposta lo
//!   scarto di secondi.
//! - L'**incertezza** è `max(rtt_min/2, 2σ)`, con σ lo scarto quadratico
//!   medio pesato dei residui: la prima parte è il limite che nessuna media
//!   toglie (l'asimmetria fra andata e ritorno non si vede dagli scambi), la
//!   seconda il rumore che resta.
//!
//! # Aritmetica
//!
//! Gli istanti sono `u64` e possono stare ovunque: due basi dei tempi
//! diverse, e niente vieta valori vicini a `u64::MAX` in una prova. Tutti i
//! conti sugli istanti si fanno in `i128`, che non trabocca; quello che non
//! torna in un `u64` o in un `i64` diventa `None` o
//! [`Perche::Incoerente`], mai un panico. Solo la regressione è in virgola
//! mobile, e lavora su differenze dal primo punto: lì i numeri sono piccoli.

use std::collections::VecDeque;

/// Un miliardo: nanosecondi in un secondo, parti per miliardo nell'unità.
const MILIARDO: i128 = 1_000_000_000;

/// Uno scambio in stile NTP fra il membro e l'ospite, in nanosecondi.
///
/// `t1` e `t4` sono sull'orologio del membro, `t2` e `t3` su quello
/// dell'ospite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scambio {
    /// Partenza della domanda, sull'orologio del membro.
    pub t1: u64,
    /// Arrivo della domanda, sull'orologio dell'ospite.
    pub t2: u64,
    /// Partenza della risposta, sull'orologio dell'ospite.
    pub t3: u64,
    /// Arrivo della risposta, sull'orologio del membro.
    pub t4: u64,
}

/// La stima del rapporto fra i due orologi.
///
/// `ospite = locale + scarto_ns + deriva_ppb·(locale − riferimento_locale)/1e9`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stima {
    /// L'istante locale a cui vale `scarto_ns`: il baricentro pesato dei punti.
    pub riferimento_locale: u64,
    /// Ospite meno locale, in `riferimento_locale`.
    pub scarto_ns: i64,
    /// Di quanto cambia lo scarto per ogni nanosecondo locale, in parti per
    /// miliardo. Positivo se l'orologio dell'ospite va più svelto.
    pub deriva_ppb: i64,
    /// Di quanto la stima può sbagliare: `max(rtt_min/2, 2σ)`.
    pub incertezza_ns: u64,
    /// Su quanti punti (uno per secchio) si regge l'adattamento finale.
    pub punti: u32,
}

impl Stima {
    /// Ospite meno locale nell'istante locale `locale`.
    fn scarto_in(&self, locale: u64) -> Option<i128> {
        let dx = i128::from(locale) - i128::from(self.riferimento_locale);
        let pendenza = dx.checked_mul(i128::from(self.deriva_ppb))?;
        Some(i128::from(self.scarto_ns) + dividi(pendenza, MILIARDO))
    }

    /// Un istante locale portato sull'orologio dell'ospite. `None` se il
    /// risultato non sta in un `u64`.
    #[must_use]
    pub fn ospite_da_locale(&self, ns: u64) -> Option<u64> {
        u64::try_from(i128::from(ns) + self.scarto_in(ns)?).ok()
    }

    /// Un istante dell'ospite portato sull'orologio locale. `None` se il
    /// risultato non sta in un `u64`.
    ///
    /// È l'inversa esatta di [`Stima::ospite_da_locale`], non un'iterazione:
    /// `ospite − rif − scarto = (locale − rif)·(1 + deriva)`, e si divide.
    #[must_use]
    pub fn locale_da_ospite(&self, ns: u64) -> Option<u64> {
        let rif = i128::from(self.riferimento_locale);
        let denominatore = MILIARDO + i128::from(self.deriva_ppb);
        if denominatore <= 0 {
            return None;
        }
        let numeratore =
            (i128::from(ns) - rif - i128::from(self.scarto_ns)).checked_mul(MILIARDO)?;
        u64::try_from(rif + dividi(numeratore, denominatore)).ok()
    }
}

/// Cosa è stato di uno scambio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Esito {
    /// È diventato il rappresentante del suo secchio.
    Accettato,
    /// Non entra nella stima, per questa ragione.
    Scartato(Perche),
    /// L'altro orologio è cambiato: la storia è stata buttata, e questo
    /// scambio è il primo punto della nuova.
    Ricominciato,
}

/// Perché uno scambio è stato scartato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Perche {
    /// Gli istanti non hanno senso: la risposta arriva prima della domanda,
    /// l'ospite risponde prima di ricevere, l'rtt è negativo, o lo scarto non
    /// sta in un `i64`.
    Incoerente,
    /// L'rtt supera il tetto assoluto (100 ms di serie).
    Lento,
    /// L'rtt supera `max(2·rtt_min, rtt_min + 1 ms)`: ha fatto coda.
    ///
    /// Non entra nella stima, ma resta come rappresentante del suo secchio se è
    /// il migliore che ha: se la rete è peggiorata per sempre, sono i punti che
    /// restano quando i vecchi escono dalla finestra.
    Disturbato,
    /// Nel suo secchio c'è già uno scambio con l'rtt minore o uguale.
    Superato,
    /// È più vecchio della finestra.
    Vecchio,
    /// Lo scarto si allontana dalla stima più della soglia — 10 ms, o l'rtt
    /// dello scambio se è più grande — ma non è ancora il terzo di fila.
    Salto,
}

/// Le soglie dell'orologio. [`Default`] dà quelle della specifica.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Taratura {
    /// La larghezza di un secchio: 2 s.
    pub secchio_ns: u64,
    /// La finestra della regressione: 120 s.
    pub finestra_ns: u64,
    /// L'rtt oltre il quale uno scambio si scarta comunque: 100 ms.
    pub rtt_tetto_ns: u64,
    /// Si scarta oltre `rtt_fattore · rtt_min`, se è più di
    /// `rtt_min + rtt_margine_ns`: 2.
    pub rtt_fattore: u64,
    /// Vedi `rtt_fattore`: 1 ms.
    pub rtt_margine_ns: u64,
    /// Il rumore che resta anche sull'rtt minimo, nel peso: 50 µs.
    pub peso_base_ns: u64,
    /// Nel secondo passaggio si tolgono i residui oltre questa volte la
    /// deviazione mediana: 3.
    pub fattore_mediana: f64,
    /// …ma mai sotto questo residuo, così una deviazione mediana quasi nulla
    /// (dati puliti) non toglie punti buoni: 20 µs.
    pub residuo_minimo_ns: u64,
    /// Quanti punti servono per dare uno scarto: 5.
    pub punti_minimi: u32,
    /// Quanti nanosecondi di dati servono per dare una deriva: 20 s.
    pub durata_deriva_ns: u64,
    /// Un residuo oltre questo è un salto: 10 ms. Se l'rtt dello scambio è più
    /// grande, la soglia è l'rtt: fin lì il residuo si spiega con l'asimmetria
    /// della rete.
    pub salto_ns: u64,
    /// Quanti salti di fila fanno ricominciare: 3.
    pub salti_di_fila: u32,
}

impl Default for Taratura {
    fn default() -> Self {
        Self {
            secchio_ns: 2_000_000_000,
            finestra_ns: 120_000_000_000,
            rtt_tetto_ns: 100_000_000,
            rtt_fattore: 2,
            rtt_margine_ns: 1_000_000,
            peso_base_ns: 50_000,
            fattore_mediana: 3.0,
            residuo_minimo_ns: 20_000,
            punti_minimi: 5,
            durata_deriva_ns: 20_000_000_000,
            salto_ns: 10_000_000,
            salti_di_fila: 3,
        }
    }
}

/// Il rappresentante di un secchio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Punto {
    /// Quale secchio: l'istante locale diviso per la sua larghezza.
    secchio: u64,
    /// L'istante locale dello scambio: il punto medio fra `t1` e `t4`.
    x: u64,
    /// Ospite meno locale.
    scarto: i64,
    /// Il tempo di andata e ritorno.
    rtt: u64,
}

impl Punto {
    /// Le grandezze di uno scambio, o `None` se gli istanti non hanno senso.
    #[expect(
        clippy::integer_division,
        reason = "il punto medio al nanosecondo: il mezzo nanosecondo perso non conta"
    )]
    fn di(s: &Scambio, secchio_ns: u64) -> Option<Self> {
        let andata_e_ritorno = i128::from(s.t4) - i128::from(s.t1);
        let presso_l_ospite = i128::from(s.t3) - i128::from(s.t2);
        if andata_e_ritorno < 0 || presso_l_ospite < 0 {
            return None;
        }
        let rtt = u64::try_from(andata_e_ritorno - presso_l_ospite).ok()?;
        let somma = (i128::from(s.t2) - i128::from(s.t1)) + (i128::from(s.t3) - i128::from(s.t4));
        let scarto = i64::try_from(dividi(somma, 2)).ok()?;
        // Il punto medio, senza sommare due u64 che potrebbero traboccare.
        let x = s.t1 + (s.t4 - s.t1) / 2;
        Some(Self {
            secchio: x.checked_div(secchio_ns).unwrap_or(x),
            x,
            scarto,
            rtt,
        })
    }
}

/// La stima dello scarto e della deriva fra il mio orologio e quello
/// dell'ospite. Vedi la documentazione del modulo.
#[derive(Debug, Clone)]
pub struct Orologio {
    taratura: Taratura,
    /// Un punto per secchio, in ordine di secchio.
    secchi: VecDeque<Punto>,
    /// L'istante locale più recente visto, anche in uno scambio scartato:
    /// la finestra scorre con quello.
    ultimo: Option<u64>,
    /// Quanti residui oltre la soglia di fila.
    salti: u32,
    /// L'adattamento corrente, anche sotto i punti minimi: serve a giudicare
    /// i salti fin dal secondo scambio. Accanto, quanti punti hanno passato il
    /// filtro dell'rtt: è su quelli, e non sui tenuti dal secondo passaggio,
    /// che si contano i punti minimi.
    adattamento: Option<(Stima, u32)>,
}

impl Orologio {
    /// Un orologio senza storia.
    #[must_use]
    pub fn nuovo(t: Taratura) -> Self {
        Self {
            taratura: t,
            secchi: VecDeque::new(),
            ultimo: None,
            salti: 0,
            adattamento: None,
        }
    }

    /// Aggiunge uno scambio e dice cosa ne è stato.
    pub fn aggiungi(&mut self, s: Scambio) -> Esito {
        let esito = self.valuta(&s);
        // Si allarga solo se la stima c'era già: vedi [`adatta`].
        let aveva_stima = self
            .adattamento
            .is_some_and(|(_, filtrati)| filtrati >= self.taratura.punti_minimi);
        self.adattamento = adatta(&self.secchi, &self.taratura, aveva_stima);
        esito
    }

    /// La stima, se ci sono abbastanza punti.
    #[must_use]
    pub fn stima(&self) -> Option<Stima> {
        self.adattamento
            .filter(|(_, filtrati)| *filtrati >= self.taratura.punti_minimi)
            .map(|(stima, _)| stima)
    }

    /// Un istante dell'ospite sul mio orologio. `None` senza una stima.
    #[must_use]
    pub fn locale_da_ospite(&self, ns: u64) -> Option<u64> {
        self.stima()?.locale_da_ospite(ns)
    }

    /// Un istante mio sull'orologio dell'ospite. `None` senza una stima.
    #[must_use]
    pub fn ospite_da_locale(&self, ns: u64) -> Option<u64> {
        self.stima()?.ospite_da_locale(ns)
    }

    fn valuta(&mut self, s: &Scambio) -> Esito {
        let t = self.taratura;
        let Some(punto) = Punto::di(s, t.secchio_ns) else {
            return Esito::Scartato(Perche::Incoerente);
        };
        let ultimo = self.ultimo.map_or(punto.x, |u| u.max(punto.x));
        self.ultimo = Some(ultimo);
        let inizio = ultimo.saturating_sub(t.finestra_ns);
        self.secchi.retain(|p| p.x >= inizio);
        if punto.x < inizio {
            return Esito::Scartato(Perche::Vecchio);
        }
        if punto.rtt > t.rtt_tetto_ns {
            return Esito::Scartato(Perche::Lento);
        }
        if let Some(minimo) = self.secchi.iter().map(|p| p.rtt).min()
            && punto.rtt > soglia_rtt(minimo, &t)
        {
            // Scartato dalla stima, ma tenuto come rappresentante del suo
            // secchio se è il migliore che ha: finché i punti vecchi sono nella
            // finestra [`adatta`] lo esclude col filtro dell'rtt, e quando la
            // rete è peggiorata per sempre sono questi i punti che restano.
            // Buttarli voleva dire una finestra che si vuotava dei vecchi senza
            // riempirsi dei nuovi, e una stima che spariva. Il controllo dei
            // salti non lo riguarda: non entra in nessuna stima finché non c'è
            // un minimo che lo accetti.
            let _ = self.metti_nel_secchio(punto);
            return Esito::Scartato(Perche::Disturbato);
        }
        // La soglia del salto cresce con l'rtt dello scambio: uno scambio
        // sbaglia fino a rtt/2 per l'asimmetria fra andata e ritorno, la stima
        // fino a rtt_min/2 ≤ rtt/2, quindi un residuo entro l'rtt si spiega con
        // la rete. Fissa a 10 ms, su un Wi-Fi da 30 ms tre code tutte da una
        // parte di fila — scambi buoni, sotto la soglia dell'rtt — buttavano la
        // storia come per un riavvio dell'ospite. Il riavvio vero sposta lo
        // scarto di secondi, oltre il tetto di 100 ms che limita l'rtt.
        if let Some((stima, _)) = self.adattamento
            && let Some(atteso) = stima.scarto_in(punto.x)
            && (i128::from(punto.scarto) - atteso).unsigned_abs()
                > u128::from(t.salto_ns.max(punto.rtt))
        {
            self.salti = self.salti.saturating_add(1);
            if self.salti < t.salti_di_fila {
                return Esito::Scartato(Perche::Salto);
            }
            self.secchi.clear();
            self.secchi.push_back(punto);
            self.salti = 0;
            return Esito::Ricominciato;
        }
        self.salti = 0;
        self.metti_nel_secchio(punto)
    }

    /// Il punto diventa il rappresentante del suo secchio, se il secchio è
    /// vuoto o se ha l'rtt minore di quello che c'era.
    fn metti_nel_secchio(&mut self, punto: Punto) -> Esito {
        match self.secchi.iter().position(|p| p.secchio >= punto.secchio) {
            Some(i)
                if self
                    .secchi
                    .get(i)
                    .is_some_and(|p| p.secchio == punto.secchio) =>
            {
                match self.secchi.get_mut(i) {
                    Some(presente) if presente.rtt > punto.rtt => {
                        *presente = punto;
                        Esito::Accettato
                    }
                    _ => Esito::Scartato(Perche::Superato),
                }
            }
            Some(i) => {
                self.secchi.insert(i, punto);
                Esito::Accettato
            }
            None => {
                self.secchi.push_back(punto);
                Esito::Accettato
            }
        }
    }
}

/// L'rtt oltre il quale uno scambio ha fatto coda.
fn soglia_rtt(minimo: u64, t: &Taratura) -> u64 {
    minimo
        .saturating_mul(t.rtt_fattore)
        .max(minimo.saturating_add(t.rtt_margine_ns))
}

/// Un punto pronto per la regressione: tutto relativo al primo punto.
#[derive(Debug, Clone, Copy)]
struct Campione {
    dx: f64,
    dy: f64,
    peso: f64,
}

/// Una retta pesata: `dy = a + b·(dx − xm)`.
#[derive(Debug, Clone, Copy)]
struct Retta {
    xm: f64,
    a: f64,
    b: f64,
}

impl Retta {
    fn residuo(&self, c: &Campione) -> f64 {
        c.dy - (self.a + self.b * (c.dx - self.xm))
    }
}

/// I minimi quadrati pesati, con la pendenza solo se `con_deriva`.
fn retta(campioni: &[Campione], con_deriva: bool) -> Option<Retta> {
    let pesi: f64 = campioni.iter().map(|c| c.peso).sum();
    if pesi <= 0.0 || !pesi.is_finite() {
        return None;
    }
    let xm = campioni.iter().map(|c| c.peso * c.dx).sum::<f64>() / pesi;
    let ym = campioni.iter().map(|c| c.peso * c.dy).sum::<f64>() / pesi;
    let sxx: f64 = campioni
        .iter()
        .map(|c| c.peso * (c.dx - xm) * (c.dx - xm))
        .sum();
    let b = if con_deriva && sxx > 0.0 {
        campioni
            .iter()
            .map(|c| c.peso * (c.dx - xm) * (c.dy - ym))
            .sum::<f64>()
            / sxx
    } else {
        0.0
    };
    Some(Retta { xm, a: ym, b })
}

/// Se i campioni coprono abbastanza tempo per una pendenza.
fn copre(campioni: &[Campione], durata_ns: u64) -> bool {
    let (minimo, massimo) = campioni
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
            (lo.min(c.dx), hi.max(c.dx))
        });
    campioni.len() >= 3 && massimo - minimo >= durata_ns as f64
}

/// L'adattamento dei punti della finestra, in due passaggi; accanto, quanti
/// punti hanno passato il filtro dell'rtt.
///
/// # Quando la soglia dell'rtt si allarga
///
/// Solo se `aveva_stima` — l'adattamento di prima aveva già i suoi
/// `punti_minimi` — e adesso il filtro ne lascia passare meno, con la finestra
/// che però ne ha abbastanza: allora la soglia sale all'rtt del
/// `punti_minimi`-esimo migliore. È il caso della rete peggiorata per sempre: i
/// punti vecchi, col loro minimo basso, escono dalla finestra uno per secchio,
/// e senza questa regola la stima spariva finché non uscivano tutti. Con i
/// pesi, quel che resta dei vecchi continua a contare quasi da solo; i nuovi
/// tengono soltanto il conto sopra il minimo.
///
/// All'avvio no, ed è voluto: un orologio nuovo su una rete rumorosa, con
/// cinque secchi ma meno di cinque vicini al minimo, darebbe una stima che di
/// fatto si regge sul punto migliore — esattamente quel che i cinque punti
/// esistono per evitare. Neanche dopo [`Esito::Ricominciato`]: lì i secchi sono
/// uno, e l'adattamento successivo non ha più la stima di prima.
#[expect(
    clippy::integer_division,
    reason = "metà dell'rtt minimo, al nanosecondo"
)]
fn adatta(secchi: &VecDeque<Punto>, t: &Taratura, aveva_stima: bool) -> Option<(Stima, u32)> {
    let minimo = secchi.iter().map(|p| p.rtt).min()?;
    let mut soglia = soglia_rtt(minimo, t);
    let minimi = usize::try_from(t.punti_minimi).unwrap_or(usize::MAX);
    if aveva_stima
        && secchi.len() >= minimi
        && secchi.iter().filter(|p| p.rtt <= soglia).count() < minimi
    {
        let mut rtt: Vec<u64> = secchi.iter().map(|p| p.rtt).collect();
        rtt.sort_unstable();
        if let Some(ultimo_dei_minimi) = minimi.checked_sub(1).and_then(|i| rtt.get(i)) {
            soglia = soglia.max(*ultimo_dei_minimi);
        }
    }
    let buoni: Vec<&Punto> = secchi.iter().filter(|p| p.rtt <= soglia).collect();
    let primo = **buoni.first()?;
    let base = t.peso_base_ns.max(1) as f64;
    let campioni: Vec<Campione> = buoni
        .iter()
        .map(|p| {
            let errore = (p.rtt - minimo) as f64 + base;
            Campione {
                dx: (i128::from(p.x) - i128::from(primo.x)) as f64,
                dy: (i128::from(p.scarto) - i128::from(primo.scarto)) as f64,
                peso: (base / errore) * (base / errore),
            }
        })
        .collect();

    let primo_giro = retta(&campioni, copre(&campioni, t.durata_deriva_ns))?;
    let mut scarti: Vec<f64> = campioni
        .iter()
        .map(|c| primo_giro.residuo(c).abs())
        .collect();
    let tetto = (t.fattore_mediana * mediana(&mut scarti)?).max(t.residuo_minimo_ns as f64);
    let tenuti: Vec<Campione> = campioni
        .iter()
        .copied()
        .filter(|c| primo_giro.residuo(c).abs() <= tetto)
        .collect();
    let (usati, finale) = match retta(&tenuti, copre(&tenuti, t.durata_deriva_ns)) {
        Some(r) => (tenuti, r),
        None => (campioni, primo_giro),
    };

    let pesi: f64 = usati.iter().map(|c| c.peso).sum();
    let sigma = (usati
        .iter()
        .map(|c| c.peso * finale.residuo(c) * finale.residuo(c))
        .sum::<f64>()
        / pesi)
        .sqrt();
    let incertezza = (minimo / 2).max(intero(2.0 * sigma).and_then(|v| u64::try_from(v).ok())?);

    let riferimento = i128::from(primo.x) + intero(finale.xm)?;
    let scarto = i128::from(primo.scarto) + intero(finale.a)?;
    let stima = Stima {
        riferimento_locale: u64::try_from(riferimento).ok()?,
        scarto_ns: i64::try_from(scarto).ok()?,
        deriva_ppb: i64::try_from(intero(finale.b * 1e9)?).ok()?,
        incertezza_ns: incertezza,
        punti: u32::try_from(usati.len()).unwrap_or(u32::MAX),
    };
    Some((stima, u32::try_from(buoni.len()).unwrap_or(u32::MAX)))
}

/// La mediana, riordinando sul posto.
#[expect(
    clippy::integer_division,
    reason = "l'indice di mezzo: con un numero pari di valori si prende quello \
              alto dei due, ed è voluto — è una soglia, non una statistica da \
              riportare"
)]
fn mediana(valori: &mut [f64]) -> Option<f64> {
    valori.sort_by(f64::total_cmp);
    valori.get(valori.len() / 2).copied()
}

/// Un numero con la virgola arrotondato all'intero, o `None` se non è finito
/// o è fuori scala.
#[expect(
    clippy::cast_possible_truncation,
    reason = "il valore è arrotondato e controllato contro ±1e30, ben dentro un i128"
)]
fn intero(x: f64) -> Option<i128> {
    (x.is_finite() && x.abs() < 1e30).then(|| x.round() as i128)
}

/// Divisione arrotondata al più vicino, con `denominatore > 0`.
fn dividi(numeratore: i128, denominatore: i128) -> i128 {
    let quoziente = numeratore.div_euclid(denominatore);
    let resto = numeratore.rem_euclid(denominatore);
    if resto >= denominatore - resto {
        quoziente + 1
    } else {
        quoziente
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un generatore pseudo-casuale con seme fisso.
    struct Lcg(u64);

    impl Lcg {
        fn uniforme(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }

        /// Un'attesa esponenziale di media `media`: la forma delle code di
        /// rete, sempre positive, quasi sempre piccole, ogni tanto lunghe.
        fn coda(&mut self, media: f64) -> f64 {
            -self.uniforme().ln() * media
        }
    }

    /// Un istante con la virgola come istante intero.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "istanti simulati, positivi e lontani da u64::MAX"
    )]
    fn ns(x: f64) -> u64 {
        x.round() as u64
    }

    /// Due PC simulati: il mio orologio è il tempo vero, quello dell'ospite
    /// ha uno scarto e una deriva, e la rete ha un ritardo minimo per verso
    /// più una coda esponenziale.
    struct Rete {
        caso: Lcg,
        scarto: f64,
        deriva: f64,
        andata: f64,
        ritorno: f64,
        coda: f64,
    }

    impl Rete {
        fn nuova(scarto: f64, deriva_ppm: f64) -> Self {
            Self {
                caso: Lcg(2026),
                scarto,
                deriva: deriva_ppm * 1e-6,
                andata: 150_000.0,
                ritorno: 150_000.0,
                coda: 80_000.0,
            }
        }

        /// L'orologio dell'ospite nell'istante locale `l`.
        fn ospite(&self, l: f64) -> f64 {
            l + self.scarto + self.deriva * l
        }

        fn scambio(&mut self, t1: u64) -> Scambio {
            let l1 = t1 as f64;
            let l2 = l1 + self.andata + self.caso.coda(self.coda);
            let l3 = l2 + 30_000.0 + self.caso.coda(10_000.0);
            let l4 = l3 + self.ritorno + self.caso.coda(self.coda);
            Scambio {
                t1,
                t2: ns(self.ospite(l2)),
                t3: ns(self.ospite(l3)),
                t4: ns(l4),
            }
        }

        /// Uno scambio ogni mezzo secondo da `da` per `quanti` volte.
        fn scambia(&mut self, o: &mut Orologio, da: u64, quanti: u64) -> Vec<Esito> {
            (0..quanti)
                .map(|i| o.aggiungi(self.scambio(da + i * 500_000_000)))
                .collect()
        }
    }

    const SECONDO: u64 = 1_000_000_000;

    /// Quanto sbaglia la stima, in ns, nell'istante locale `l`.
    fn errore(o: &Orologio, rete: &Rete, l: u64) -> f64 {
        let stimato = o.ospite_da_locale(l).expect("una stima") as f64;
        stimato - rete.ospite(l as f64)
    }

    #[test]
    fn uno_scarto_costante_si_ritrova() {
        let mut rete = Rete::nuova(123_456_789_012.0, 0.0);
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, 5 * SECONDO, 150);
        let stima = o.stima().expect("una stima");
        assert!(errore(&o, &rete, 60 * SECONDO).abs() < 30_000.0);
        assert!(stima.deriva_ppb.abs() < 1000, "deriva {}", stima.deriva_ppb);
        assert!(stima.incertezza_ns >= 150_000, "almeno rtt_min/2");
        // Le due conversioni sono una l'inversa dell'altra.
        let h = o.ospite_da_locale(20 * SECONDO).unwrap();
        assert_eq!(o.locale_da_ospite(h), Some(20 * SECONDO));
    }

    #[test]
    fn cento_ppm_di_deriva_si_stimano_entro_un_ppm() {
        for ppm in [100.0, -100.0] {
            let mut rete = Rete::nuova(-7_000_000_000.0, ppm);
            let mut o = Orologio::nuovo(Taratura::default());
            // Un po' più della finestra: la regressione la vede piena.
            rete.scambia(&mut o, SECONDO, 300);
            let stima = o.stima().expect("una stima");
            let atteso = ppm * 1000.0;
            assert!(
                (stima.deriva_ppb as f64 - atteso).abs() < 1000.0,
                "{ppm} ppm: stimati {} ppb",
                stima.deriva_ppb
            );
            // E la conversione è giusta anche all'ultimo scambio, dove la
            // deriva ha già portato lo scarto lontano dal primo.
            let ultimo = SECONDO + 299 * 500_000_000;
            assert!(errore(&o, &rete, ultimo).abs() < 30_000.0);
            for l in [75 * SECONDO, ultimo] {
                let h = o.ospite_da_locale(l).unwrap();
                let indietro = o.locale_da_ospite(h).unwrap();
                assert!(indietro.abs_diff(l) <= 1, "{l} → {h} → {indietro}");
            }
        }
    }

    #[test]
    fn senza_venti_secondi_di_dati_la_deriva_resta_zero() {
        let mut rete = Rete::nuova(0.0, 100.0);
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, SECONDO, 30);
        let stima = o.stima().expect("cinque punti ci sono");
        assert_eq!(stima.deriva_ppb, 0);
        rete.scambia(&mut o, SECONDO + 30 * 500_000_000, 20);
        assert_ne!(o.stima().unwrap().deriva_ppb, 0, "oltre i 20 s sì");
    }

    #[test]
    fn prima_di_cinque_punti_non_c_e_stima() {
        let mut rete = Rete::nuova(1_000_000.0, 0.0);
        let mut o = Orologio::nuovo(Taratura::default());
        assert_eq!(o.stima(), None);
        assert_eq!(o.locale_da_ospite(SECONDO), None);
        // Quattro secchi da 2 s: sedici scambi al mezzo secondo.
        rete.scambia(&mut o, 0, 16);
        assert_eq!(o.stima(), None);
        // Il quinto.
        rete.scambia(&mut o, 8 * SECONDO, 4);
        assert!(o.stima().is_some());
    }

    #[test]
    fn un_ritardo_asimmetrico_sbaglia_meno_di_mezzo_rtt_minimo() {
        // Andata 0,2 ms, ritorno 1 ms: lo scarto misurato è spostato di
        // (0,2 − 1)/2 = −0,4 ms, e nessuno scambio può accorgersene. Deve
        // restare dentro rtt_min/2, e dentro l'incertezza dichiarata.
        let mut rete = Rete::nuova(5_000_000.0, 0.0);
        rete.andata = 200_000.0;
        rete.ritorno = 1_000_000.0;
        rete.coda = 20_000.0;
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, 0, 80);
        let stima = o.stima().unwrap();
        let sbaglio = errore(&o, &rete, 20 * SECONDO).abs();
        let rtt_min = 200_000.0 + 1_000_000.0 + 30_000.0;
        assert!(sbaglio <= rtt_min / 2.0, "sbaglia di {sbaglio} ns");
        assert!(
            sbaglio > 300_000.0,
            "l'asimmetria c'è, e si vede: {sbaglio}"
        );
        assert!(stima.incertezza_ns as f64 >= sbaglio, "{stima:?}");
    }

    #[test]
    fn un_wifi_lento_da_comunque_una_stima() {
        // Un rtt minimo di 30 ms (15 ms per verso) più code fino a 10 ms:
        // il Wi-Fi carico di un ufficio. Col tetto di 20 ms non c'era stima.
        let mut rete = Rete::nuova(-3_000_000.0, 0.0);
        rete.andata = 15_000_000.0;
        rete.ritorno = 15_000_000.0;
        rete.coda = 10_000_000.0;
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, 0, 40);
        let stima = o.stima().expect("una stima anche su una rete lenta");
        // Il ritardo è simmetrico: la stima è buona, e l'incertezza dichiarata
        // copre quel che sbaglia.
        let sbaglio = errore(&o, &rete, 20 * SECONDO).abs();
        assert!(sbaglio < 2_000_000.0, "{} ms", sbaglio / 1e6);
        assert!(stima.incertezza_ns as f64 >= sbaglio, "{stima:?}");
    }

    #[test]
    fn i_picchi_di_rtt_si_scartano_e_non_spostano_la_stima() {
        let mut rete = Rete::nuova(42_000_000.0, 0.0);
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, 0, 40);
        let prima = o.stima().unwrap();

        // Un picco da 130 ms: oltre il tetto, qualunque cosa ci sia nel secchio.
        let mut picco = rete.scambio(25 * SECONDO);
        picco.t4 += 130_000_000;
        assert_eq!(o.aggiungi(picco), Esito::Scartato(Perche::Lento));

        // Uno da 5 ms, con lo scarto storto di 2 ms: sotto il tetto, ma più
        // del doppio del minimo, e anche di minimo + 1 ms.
        let mut coda = rete.scambio(26 * SECONDO);
        coda.t2 += 4_000_000;
        coda.t3 += 4_000_000;
        coda.t4 += 4_000_000;
        assert_eq!(o.aggiungi(coda), Esito::Scartato(Perche::Disturbato));

        // Uno appena peggiore del rappresentante del suo secchio, che è
        // quello fra 18 e 20 s.
        let mut peggiore = rete.scambio(19 * SECONDO + 200_000_000);
        peggiore.t4 += 500_000;
        assert_eq!(o.aggiungi(peggiore), Esito::Scartato(Perche::Superato));

        let dopo = o.stima().unwrap();
        assert_eq!(prima.scarto_ns, dopo.scarto_ns);
        assert_eq!(prima.deriva_ppb, dopo.deriva_ppb);
    }

    #[test]
    fn tre_salti_di_fila_fanno_ricominciare_uno_solo_no() {
        let mut rete = Rete::nuova(1_000_000_000.0, 0.0);
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, 0, 40);
        let prima = o.stima().unwrap();

        // Un pacchetto fermo da qualche parte: un residuo grande isolato.
        rete.scarto += 50_000_000.0;
        assert_eq!(
            o.aggiungi(rete.scambio(20 * SECONDO)),
            Esito::Scartato(Perche::Salto)
        );
        rete.scarto -= 50_000_000.0;
        assert_eq!(o.aggiungi(rete.scambio(22 * SECONDO)), Esito::Accettato);
        // Un secondo isolato: il conto era ripartito da zero.
        rete.scarto += 50_000_000.0;
        assert_eq!(
            o.aggiungi(rete.scambio(24 * SECONDO)),
            Esito::Scartato(Perche::Salto)
        );
        let intanto = o.stima().unwrap();
        assert!((intanto.scarto_ns - prima.scarto_ns).abs() < 20_000);

        // L'ospite ha riavviato: lo scarto è cambiato davvero.
        assert_eq!(
            o.aggiungi(rete.scambio(26 * SECONDO)),
            Esito::Scartato(Perche::Salto)
        );
        assert_eq!(o.aggiungi(rete.scambio(28 * SECONDO)), Esito::Ricominciato);
        assert_eq!(o.stima(), None, "un punto solo: si riparte da capo");
        rete.scambia(&mut o, 30 * SECONDO, 20);
        assert!(errore(&o, &rete, 35 * SECONDO).abs() < 50_000.0);
    }

    #[test]
    fn tre_code_asimmetriche_su_un_wifi_lento_non_sono_un_riavvio() {
        // Il Wi-Fi carico di `un_wifi_lento_da_comunque_una_stima`: 15 ms per
        // verso. Uno scambio con 25 ms di coda tutta all'andata ha un rtt di
        // 55 ms — sotto la soglia di 60 — e uno scarto storto di +12,5 ms, che
        // è il suo errore lecito (fino a rtt/2) e non un orologio cambiato.
        // Tre così di fila oltre la soglia fissa di 10 ms buttavano la storia
        // come per un riavvio dell'ospite, e la stima spariva.
        let mut rete = Rete::nuova(-3_000_000.0, 0.0);
        rete.andata = 15_000_000.0;
        rete.ritorno = 15_000_000.0;
        rete.coda = 1_000_000.0;
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, 0, 40);
        assert!(o.stima().is_some());
        for secondo in [21, 22, 23] {
            let mut storto = rete.scambio(secondo * SECONDO);
            storto.t2 += 25_000_000;
            storto.t3 += 25_000_000;
            storto.t4 += 25_000_000;
            let esito = o.aggiungi(storto);
            assert_ne!(
                esito,
                Esito::Ricominciato,
                "a {secondo} s una coda asimmetrica è passata per un riavvio"
            );
            assert!(o.stima().is_some(), "a {secondo} s la stima è sparita");
        }
    }

    #[test]
    fn i_bordi_dell_aritmetica_non_fanno_panico() {
        let mut o = Orologio::nuovo(Taratura::default());
        let incoerente = Esito::Scartato(Perche::Incoerente);
        // La risposta arriva prima della domanda.
        let s = Scambio {
            t1: 10,
            t2: 20,
            t3: 30,
            t4: 5,
        };
        assert_eq!(o.aggiungi(s), incoerente);
        // L'ospite risponde prima di ricevere.
        let s = Scambio {
            t1: 10,
            t2: 30,
            t3: 20,
            t4: 50,
        };
        assert_eq!(o.aggiungi(s), incoerente);
        // L'ospite ci mette più dell'intero giro: rtt negativo.
        let s = Scambio {
            t1: 10,
            t2: 100,
            t3: 200,
            t4: 50,
        };
        assert_eq!(o.aggiungi(s), incoerente);
        // Uno scarto che non sta in un i64.
        let s = Scambio {
            t1: u64::MAX - 1000,
            t2: 0,
            t3: 10,
            t4: u64::MAX - 500,
        };
        assert_eq!(o.aggiungi(s), incoerente);
        assert_eq!(o.stima(), None);

        // Tutti e due gli orologi vicini a u64::MAX: funziona come altrove.
        let mut o = Orologio::nuovo(Taratura::default());
        let base = u64::MAX - 200 * SECONDO;
        for i in 0..100u64 {
            let t1 = base + i * 500_000_000;
            let s = Scambio {
                t1,
                t2: t1 + 100_000 - 3_000_000,
                t3: t1 + 130_000 - 3_000_000,
                t4: t1 + 230_000,
            };
            assert_ne!(o.aggiungi(s), incoerente);
        }
        let stima = o.stima().unwrap();
        assert_eq!(stima.scarto_ns, -3_000_000);
        // Oltre la fine dello spazio: None, non un giro.
        let abbondante = Stima {
            scarto_ns: 5_000_000,
            ..stima
        };
        assert_eq!(abbondante.ospite_da_locale(u64::MAX - 1000), None);
        assert_eq!(abbondante.locale_da_ospite(0), None);
        assert_eq!(stima.locale_da_ospite(0), Some(3_000_000));
        assert!(stima.locale_da_ospite(u64::MAX - 10 * SECONDO).is_some());

        // Una deriva assurda: niente divisione per zero né traboccamenti.
        let assurda = Stima {
            deriva_ppb: -1_000_000_000,
            ..stima
        };
        assert_eq!(assurda.locale_da_ospite(base), None);
        let enorme = Stima {
            deriva_ppb: i64::MAX,
            riferimento_locale: 0,
            ..stima
        };
        assert_eq!(enorme.ospite_da_locale(u64::MAX), None);
    }

    #[test]
    fn gli_scambi_fuori_finestra_si_scartano_e_la_finestra_scorre() {
        let mut rete = Rete::nuova(0.0, 0.0);
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, 200 * SECONDO, 4);
        assert_eq!(
            o.aggiungi(rete.scambio(10 * SECONDO)),
            Esito::Scartato(Perche::Vecchio)
        );
        // Dopo la finestra, dei primi punti non resta niente.
        rete.scambia(&mut o, 400 * SECONDO, 20);
        assert!(o.stima().unwrap().punti <= 10);
    }

    #[test]
    fn la_finestra_si_rifa_quando_la_rete_peggiora_per_sempre() {
        // Il minimo passa da 0,3 a 6 ms: per 120 s gli scambi nuovi sono
        // «disturbati», poi i vecchi escono dalla finestra e il minimo è il
        // nuovo.
        let mut rete = Rete::nuova(9_000_000.0, 0.0);
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, 0, 60);
        rete.andata = 3_000_000.0;
        rete.ritorno = 3_000_000.0;
        let esiti = rete.scambia(&mut o, 30 * SECONDO, 400);
        assert_eq!(esiti.first(), Some(&Esito::Scartato(Perche::Disturbato)));
        assert!(esiti.iter().rev().take(20).any(|e| *e == Esito::Accettato));
        assert!(errore(&o, &rete, 220 * SECONDO).abs() < 100_000.0);
    }

    #[test]
    fn quando_la_rete_peggiora_la_stima_non_sparisce() {
        // Lo stesso peggioramento, uno scambio al secondo come fa chi segue, e
        // la domanda è un'altra: la stima c'è **sempre**? I punti vecchi
        // uscivano dalla finestra uno per secchio, i nuovi restavano fuori
        // perché «disturbati» rispetto a loro, e per una quindicina di secondi
        // chi seguiva restava senza stima — «non agganciato» senza una ragione
        // che si veda.
        let mut rete = Rete::nuova(9_000_000.0, 0.0);
        let mut o = Orologio::nuovo(Taratura::default());
        rete.scambia(&mut o, 0, 60);
        rete.andata = 3_000_000.0;
        rete.ritorno = 3_000_000.0;
        for secondo in 30..260u64 {
            let _ = o.aggiungi(rete.scambio(secondo * SECONDO));
            assert!(
                o.stima().is_some(),
                "a {secondo} s, dopo il peggioramento, la stima è sparita"
            );
        }
        assert!(errore(&o, &rete, 259 * SECONDO).abs() < 100_000.0);
    }

    #[test]
    fn un_orologio_nuovo_non_allarga_la_soglia() {
        // Il contrappeso: all'avvio la regola dei cinque punti resta quella.
        // Un primo scambio pulito, poi la rete peggiora: sei secchi pieni, ma
        // uno solo vicino al minimo. Allargare qui darebbe una stima che si
        // regge su quel punto solo.
        let mut rete = Rete::nuova(1_000_000.0, 0.0);
        let mut o = Orologio::nuovo(Taratura::default());
        let _ = o.aggiungi(rete.scambio(0));
        rete.andata = 3_000_000.0;
        rete.ritorno = 3_000_000.0;
        for secchio in 1..6u64 {
            assert_eq!(
                o.aggiungi(rete.scambio(secchio * 2 * SECONDO)),
                Esito::Scartato(Perche::Disturbato)
            );
        }
        assert_eq!(o.stima(), None, "una stima retta da un punto solo");
    }
}
