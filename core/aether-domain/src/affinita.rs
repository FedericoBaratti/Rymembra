//! Quanto due brani si somigliano, e quanto uno somiglia a chi ascolta.
//!
//! # Perché non sta dove sta l'autoplay
//!
//! [`crate::regole`] è il vocabolario che l'utente vede nell'editor delle
//! playlist intelligenti, e [`crate::scelta`] è il modo di preferire un
//! candidato a un altro dentro un catalogo. L'affinità non è né l'una né
//! l'altra cosa: non è un predicato che qualcuno scriverebbe a mano — «distanza
//! sonora minore di 1,4» non è una riga che si compone da un menù — e non
//! sceglie fra copie dello stesso brano, ma ordina brani diversi.
//!
//! Sta qui e non in `aether-app` per la ragione di tutto questo crate: non
//! guarda l'orologio, non apre niente e non ha stato. Un punteggio si prova
//! passandogli i numeri, e le prove che contano — quella sulla ridistribuzione
//! dei pesi, sopra tutte — sono aritmetica pura.
//!
//! # I tre strati, e perché sono tre
//!
//! - **Sonoro.** Ricavato dal file: come suona. Sempre disponibile, non chiede
//!   niente a nessuno, e non sa niente di cosa la gente ascolti.
//! - **Vicinanza.** Ricavato dagli ascolti aggregati altrui: accanto a cosa
//!   viene ascoltato. È la cosa che il suono non può sapere — due pezzi possono
//!   suonare quasi identici e appartenere a mondi che non si toccano.
//! - **Gusto.** Ricavato dalla cronologia di chi sta qui: cosa piace a
//!   *questa* persona.
//!
//! Nessuno dei tre è sempre presente, ed è il caso normale e non l'eccezione:
//! senza rete manca la vicinanza, prima della prima analisi manca il suono, su
//! un brano mai sentito manca il gusto. [`punteggio`] è costruita attorno a
//! questo, ed è l'unica funzione del modulo in cui un errore non si vedrebbe.
//!
//! # Il vettore non ha una lunghezza scritta qui
//!
//! Perché i descrittori li produce `aether-play`, che dipende da questo crate e
//! non il contrario. Scrivere qui il numero delle dimensioni vorrebbe dire due
//! costanti da tenere allineate a ogni cambio dell'estrattore, e la seconda si
//! accorgerebbe di essere sbagliata dando distanze plausibili invece che un
//! errore. Le funzioni prendono quindi delle fette, e la forma — quali indici
//! sono il timbro, quali il ritmo — arriva come [`Gruppo`] da chi quella forma
//! la produce.

/// Un tratto contiguo di descrittori, e quanto conta nella distanza.
///
/// # Perché a gruppi e non un peso per dimensione
///
/// Perché i pesi sono una manciata di decisioni — quanto conta il timbro
/// rispetto al ritmo — e non una per coefficiente del cepstro. Con un peso per
/// dimensione, il timbro deciderebbe quasi tutto per il solo fatto di occupare
/// più della metà del vettore: una scelta presa dal numero di coefficienti
/// invece che da qualcuno.
///
/// La distanza dentro un gruppo è una **media** e non una somma, appunto perché
/// il numero di dimensioni del gruppo non deve pesare due volte.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gruppo {
    /// Come si chiama, per i messaggi e per le prove.
    pub nome: &'static str,
    /// Primo indice del tratto.
    pub inizio: usize,
    /// Primo indice **fuori** dal tratto.
    pub fine: usize,
    /// Quanto conta. La somma dei pesi di tutti i gruppi dovrebbe fare uno:
    /// è ciò che rende leggibile [`RIFERIMENTO`].
    pub peso: f32,
}

/// Sotto quanti brani una scala non descrive una libreria.
///
/// Cinquanta. Uno scarto quadratico medio calcolato su dieci brani non descrive
/// una libreria: descrive quei dieci brani, e normalizzarci sopra farebbe
/// sembrare enormi differenze che sono solo il campione. Sotto questa soglia
/// [`Scala::misura`] restituisce `None`, e chi chiama ripiega su quel che
/// faceva prima — che è sempre una risposta valida.
pub const MINIMO_BRANI: usize = 50;

/// La distanza quadra attesa fra due brani presi a caso.
///
/// Non è una taratura: discende dalla normalizzazione. Dopo lo z-score ogni
/// dimensione ha varianza uno, quindi lo scarto quadratico atteso fra due
/// valori indipendenti è `E[(z₁ − z₂)²] = 2`; la distanza di gruppo è una media
/// e i pesi sommano a uno, quindi il totale atteso è due anche lui.
///
/// Da qui il fatto che [`somiglianza`] valga esattamente 0,5 fra due brani
/// estranei — che è il numero che rende leggibile ogni altro valore.
pub const RIFERIMENTO: f32 = 2.0;

/// Media e scarto quadratico medio di ogni dimensione, su tutta la libreria.
///
/// Serve a mettere le dimensioni sulla stessa scala prima di confrontarle.
/// Senza, la dimensione con la varianza più grande deciderebbe la distanza per
/// un accidente di unità di misura — un centroide spettrale in hertz contro una
/// varianza cepstrale adimensionale — invece che perché descrive qualcosa di
/// più importante.
#[derive(Debug, Clone, PartialEq)]
pub struct Scala {
    medie: Vec<f32>,
    scarti: Vec<f32>,
    brani: usize,
}

impl Scala {
    /// Misura la scala di una libreria.
    ///
    /// `None` sotto [`MINIMO_BRANI`], e `None` se due impronte hanno lunghezza
    /// diversa: mescolare due tracciati di descrittori darebbe una media che non
    /// descrive né l'uno né l'altro, ed è un guasto da far vedere subito invece
    /// che una distanza plausibile e sbagliata.
    ///
    /// # L'accumulo è di Welford
    ///
    /// Una passata sola, e senza sommare i quadrati. La somma dei quadrati su
    /// decine di migliaia di valori è la ricetta classica per una varianza
    /// negativa: due numeri grandi che si sottraggono lasciano il rumore della
    /// virgola mobile. Qui la media si aggiorna a ogni passo e la somma
    /// accumulata è quella degli scarti dalla media corrente, che resta piccola.
    #[must_use]
    pub fn misura<'a, I>(impronte: I) -> Option<Self>
    where
        I: IntoIterator<Item = &'a [f32]>,
    {
        let mut medie: Vec<f64> = Vec::new();
        let mut m2: Vec<f64> = Vec::new();
        let mut brani: usize = 0;

        for impronta in impronte {
            if brani == 0 {
                if impronta.is_empty() {
                    return None;
                }
                medie = vec![0.0; impronta.len()];
                m2 = vec![0.0; impronta.len()];
            } else if impronta.len() != medie.len() {
                return None;
            }

            brani = brani.saturating_add(1);
            let quanti = f64::from(u32::try_from(brani).ok()?);

            for ((valore, media), somma) in impronta.iter().zip(medie.iter_mut()).zip(m2.iter_mut())
            {
                let x = f64::from(*valore);
                if !x.is_finite() {
                    // Un descrittore non finito è un difetto dell'estrattore, e
                    // qui contaminerebbe la media di tutta la libreria: si
                    // butta il brano, non la scala.
                    return None;
                }
                let scarto = x - *media;
                *media += scarto / quanti;
                *somma += scarto * (x - *media);
            }
        }

        if brani < MINIMO_BRANI {
            return None;
        }
        let quanti = f64::from(u32::try_from(brani).ok()?);

        Some(Self {
            medie: medie.iter().copied().map(stretta).collect(),
            scarti: m2.iter().map(|s| stretta((s / quanti).sqrt())).collect(),
            brani,
        })
    }

    /// Ricostruisce una scala già calcolata, come la rilegge il database.
    ///
    /// `None` se le due metà non hanno la stessa lunghezza o sono vuote: una
    /// scala a metà è peggio di nessuna scala, perché non si annuncia.
    #[must_use]
    pub fn da_parti(medie: Vec<f32>, scarti: Vec<f32>, brani: usize) -> Option<Self> {
        if medie.is_empty() || medie.len() != scarti.len() {
            return None;
        }
        if !medie.iter().chain(scarti.iter()).all(|v| v.is_finite()) {
            return None;
        }
        Some(Self {
            medie,
            scarti,
            brani,
        })
    }

    /// Quante dimensioni ha l'impronta che questa scala descrive.
    #[must_use]
    pub fn dimensioni(&self) -> usize {
        self.medie.len()
    }

    /// Su quanti brani è stata misurata.
    #[must_use]
    pub const fn brani(&self) -> usize {
        self.brani
    }

    /// Le medie, per conservarle.
    #[must_use]
    pub fn medie(&self) -> &[f32] {
        &self.medie
    }

    /// Gli scarti, per conservarli.
    #[must_use]
    pub fn scarti(&self) -> &[f32] {
        &self.scarti
    }

    /// Lo z-score di un'impronta grezza, scritto in `fuori`.
    ///
    /// `false` — e `fuori` svuotato — se l'impronta non ha le dimensioni che
    /// questa scala descrive.
    ///
    /// # Uno scarto nullo dà zero, non infinito
    ///
    /// Una dimensione costante in tutta la libreria — capita: una libreria di
    /// soli MP3 a 320 kbps ha lo stesso taglio in alto dappertutto — avrebbe
    /// scarto zero, e la divisione darebbe infinito o `NaN` a seconda del
    /// numeratore. Vale zero, e la ragione non è difensiva: se lì nessuno varia,
    /// quella dimensione non distingue niente e non deve contribuire alla
    /// distanza.
    ///
    /// Prende un buffer invece di restituirlo perché chi chiama normalizza
    /// duecento candidati di fila sul filo che prepara il brano successivo, e
    /// duecento allocazioni in quel punto sono duecento allocazioni di troppo.
    pub fn normalizza_in(&self, grezza: &[f32], fuori: &mut Vec<f32>) -> bool {
        fuori.clear();
        if grezza.len() != self.medie.len() {
            return false;
        }
        for ((valore, media), scarto) in
            grezza.iter().zip(self.medie.iter()).zip(self.scarti.iter())
        {
            let z = if *scarto > 0.0 {
                (*valore - *media) / *scarto
            } else {
                0.0
            };
            fuori.push(if z.is_finite() { z } else { 0.0 });
        }
        true
    }

    /// Come [`Self::normalizza_in`], allocando. Per chi ne normalizza una sola.
    #[must_use]
    pub fn normalizza(&self, grezza: &[f32]) -> Option<Vec<f32>> {
        let mut fuori = Vec::with_capacity(self.medie.len());
        self.normalizza_in(grezza, &mut fuori).then_some(fuori)
    }
}

/// Da doppia a singola precisione, che è la forma in cui un'impronta vive.
///
/// L'accumulo di [`Scala::misura`] è in `f64` perché è lì che si perde una
/// varianza; il risultato torna in `f32` perché è così che sta nel database e
/// così arrivano i descrittori. I valori in gioco — medie e scarti di
/// descrittori acustici — stanno comodamente nell'intervallo di `f32`, e quel
/// che si perde è oltre la sesta cifra.
#[allow(
    clippy::cast_possible_truncation,
    reason = "restringimento voluto e discusso qui sopra"
)]
fn stretta(x: f64) -> f32 {
    x as f32
}

/// La distanza quadra pesata fra due impronte **già normalizzate**.
///
/// `None` se le due fette non hanno la stessa lunghezza, o se un gruppo esce
/// dai loro estremi: una distanza calcolata su un tratto mancante sarebbe più
/// piccola di quella vera, cioè direbbe che due brani si somigliano perché non
/// si è guardato.
///
/// Un gruppo vuoto vale zero e non fa fallire: è la conseguenza legittima di
/// una versione dell'estrattore che ha smesso di produrre un tratto.
#[must_use]
pub fn distanza2(a: &[f32], b: &[f32], gruppi: &[Gruppo]) -> Option<f32> {
    if a.len() != b.len() {
        return None;
    }
    let mut totale = 0.0_f32;
    for gruppo in gruppi {
        let da = a.get(gruppo.inizio..gruppo.fine)?;
        let db = b.get(gruppo.inizio..gruppo.fine)?;
        if da.is_empty() {
            continue;
        }
        let somma: f32 = da
            .iter()
            .zip(db.iter())
            .map(|(x, y)| {
                let d = *x - *y;
                d * d
            })
            .sum();
        // La media e non la somma: un gruppo di ventisei dimensioni non deve
        // contare ventisei volte un gruppo di una. Vedi `Gruppo`.
        totale += gruppo.peso * (somma / da.len() as f32);
    }
    totale.is_finite().then_some(totale)
}

/// Da distanza a somiglianza, in `0..=1`.
///
/// Uno per due brani identici, circa 0,5 per due brani estranei — vedi
/// [`RIFERIMENTO`] — e in discesa dolce da lì. Non è una sigmoide tarata su
/// niente: è la forma più semplice che manda zero in uno e l'infinito in zero
/// senza avere un punto in cui si appiattisce di colpo.
#[must_use]
pub fn somiglianza(distanza2: f32) -> f32 {
    if !distanza2.is_finite() || distanza2 < 0.0 {
        return 0.0;
    }
    1.0 / (1.0 + distanza2 / RIFERIMENTO)
}

/// Un ascolto, come sta in `play_history`.
///
/// # «Contato», e cosa vuol dire per chi legge questi numeri
///
/// In `play_history` finiscono **solo** gli ascolti che [`crate::listen`] ha
/// lasciato passare: chi salta un brano dopo dieci secondi non lascia nessuna
/// riga. Quella tabella non contiene quindi gli abbandoni, e nessuna penalità
/// per i salti si può ricavare da lì — è una cosa da sapere prima di andarla a
/// cercare.
///
/// Quel che resta è comunque un segnale vero: `ms_ascoltati` diviso la durata
/// dice se si è arrivati in fondo o si è passati oltre appena scattata la
/// soglia, ed è quello che [`gusto`] usa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ascolto {
    /// Quando l'ascolto è cominciato, in millisecondi dall'epoca.
    pub quando_ms: i64,
    /// Quanto se n'è sentito, in millisecondi.
    pub ms_ascoltati: i64,
}

/// Dopo quanti giorni un ascolto vale metà, come prova del gusto di oggi.
///
/// Sei mesi. Non è una penalità di ripetizione — quella è un'altra cosa e sta
/// in [`ripetizione`]. È che un disco amato nel 2019 e mai più toccato è **prova
/// più debole** dei gusti di adesso di uno suonato la settimana scorsa, e la
/// cronologia importata da Spotify può arrivare da dieci anni fa.
pub const EMIVITA_GUSTO_GIORNI: f32 = 180.0;

/// Quanti ascolti recenti e completi bastano a dire «questo piace».
///
/// Sei. Oltre, il punteggio non sale: fra un brano sentito sei volte e uno
/// sentito sessanta la differenza esiste, ma non è sei volte più grande, e
/// lasciarla crescere vorrebbe dire una classifica dominata per sempre dai
/// dieci brani più consumati — cioè il contrario di quel che serve a chi vuole
/// sentire qualcosa.
pub const RIFERIMENTO_ASCOLTI: f32 = 6.0;

/// Dopo quanti giorni «l'ho appena sentito» vale metà.
///
/// Sette. È la versione morbida del filtro netto a trenta giorni che la cascata
/// dell'autoplay già applica: quello esclude, questo pesa.
pub const EMIVITA_RIPETIZIONE_GIORNI: f32 = 7.0;

/// Millisecondi in un giorno, in virgola mobile.
const GIORNO_MS: f32 = 86_400_000.0;

/// Quanto piace, in `0..=1`.
///
/// `None` per un brano mai sentito, che **non** vuol dire «non piace»: vuol dire
/// che questo strato non ha niente da dire, e [`punteggio`] ridistribuisce il
/// suo peso sugli altri due. Confonderlo con zero metterebbe in fondo alla
/// classifica esattamente i brani che non si sono ancora scoperti.
///
/// `durata_ms` a zero — una durata sconosciuta — fa contare ogni ascolto per
/// intero: la frazione non si può calcolare, e il fatto che una riga esista
/// significa comunque che la soglia di [`crate::listen`] era stata superata.
#[must_use]
pub fn gusto(ascolti: &[Ascolto], durata_ms: i64, adesso_ms: i64) -> Option<f32> {
    if ascolti.is_empty() {
        return None;
    }
    let mut somma = 0.0_f32;
    for ascolto in ascolti {
        let quota = if durata_ms > 0 {
            (ascolto.ms_ascoltati as f32 / durata_ms as f32).clamp(0.0, 1.0)
        } else {
            1.0
        };
        somma += quota * decadimento(ascolto.quando_ms, adesso_ms, EMIVITA_GUSTO_GIORNI);
    }
    Some((somma / RIFERIMENTO_ASCOLTI).clamp(0.0, 1.0))
}

/// Quanto è stato sentito **da poco**, in `0..=1`.
///
/// Uno adesso, mezzo dopo [`EMIVITA_RIPETIZIONE_GIORNI`], zero per un brano mai
/// sentito. È il numero che [`punteggio`] sottrae.
#[must_use]
pub fn ripetizione(ultimo_ms: Option<i64>, adesso_ms: i64) -> f32 {
    match ultimo_ms {
        Some(quando) => decadimento(quando, adesso_ms, EMIVITA_RIPETIZIONE_GIORNI),
        None => 0.0,
    }
}

/// Il decadimento a metà: uno all'istante, mezzo dopo un'emivita.
///
/// Un istante nel futuro — l'orologio che va indietro, una cronologia importata
/// con un fuso sbagliato — vale uno e non più di uno: un ascolto non può essere
/// più recente di adesso.
fn decadimento(quando_ms: i64, adesso_ms: i64, emivita_giorni: f32) -> f32 {
    if emivita_giorni <= 0.0 {
        return 0.0;
    }
    let eta_ms = adesso_ms.saturating_sub(quando_ms);
    if eta_ms <= 0 {
        return 1.0;
    }
    let eta_giorni = eta_ms as f32 / GIORNO_MS;
    let peso = 0.5_f32.powf(eta_giorni / emivita_giorni);
    if peso.is_finite() {
        peso.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Quanto conta ogni strato.
///
/// I tre positivi sommano a uno, e non è un vincolo controllato ma il modo in
/// cui vanno letti: sono quote. La ripetizione è a parte perché è una penalità e
/// non una prova — vedi [`punteggio`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pesi {
    /// Quanto conta somigliare nel suono.
    pub sonora: f32,
    /// Quanto conta essere ascoltati insieme dagli altri.
    pub vicinanza: f32,
    /// Quanto conta piacere a chi sta qui.
    pub gusto: f32,
    /// Quanto si toglie a un brano appena sentito.
    pub ripetizione: f32,
}

impl Default for Pesi {
    /// I pesi di serie.
    ///
    /// Il suono davanti perché è l'unico strato sempre presente e perché è
    /// quello che risponde alla domanda letterale — «un brano che somigli a
    /// questo». Il gusto subito dietro: fra due brani ugualmente somiglianti,
    /// quello che piace vince. La vicinanza per ultima perché è la sola che
    /// parla di gente che non è qui, ed è anche l'unica che può mancare per
    /// ragioni che non dipendono dalla libreria.
    ///
    /// La ripetizione a mezzo punto è tarata su una cosa sola: qualcosa sentito
    /// un'ora fa deve perdere abbastanza da non poter vincere, e qualcosa
    /// sentito un mese fa non deve accorgersene — a trenta giorni il
    /// decadimento vale già meno di tre centesimi.
    fn default() -> Self {
        Self {
            sonora: 0.40,
            vicinanza: 0.25,
            gusto: 0.35,
            ripetizione: 0.50,
        }
    }
}

/// Il punteggio di un candidato, in `0..=1`.
///
/// `None` quando non si sa niente di niente: nessuno dei tre strati ha risposto,
/// e chi chiama deve ripiegare sulla cascata invece di ordinare degli zeri.
///
/// # La ridistribuzione, che è la cosa da non sbagliare
///
/// Uno strato che manca non vale zero: il suo peso si ridistribuisce sugli
/// altri. Trattarlo come zero farebbe sì che un brano con impronta e senza
/// vicini stia **sistematicamente** sotto uno che ha entrambe, e la classifica
/// diventerebbe in silenzio «quanto sappiamo di questo brano» invece di «quanto
/// c'entra». È il difetto che fa sembrare arbitrario un sistema del genere, ed è
/// una divisione.
///
/// ```
/// use aether_domain::affinita::{punteggio, Pesi};
/// let pesi = Pesi { sonora: 0.5, vicinanza: 0.5, gusto: 0.0, ripetizione: 0.0 };
/// // Un solo strato presente, e vale quel che dice: non la sua metà.
/// assert_eq!(punteggio(Some(0.8), None, None, 0.0, &pesi), Some(0.8));
/// // Nessuno strato: nessuna risposta, non uno zero.
/// assert_eq!(punteggio(None, None, None, 0.0, &pesi), None);
/// ```
///
/// La penalità di ripetizione si sottrae **dopo** la ridistribuzione, sulla
/// stessa scala, e non partecipa alla divisione: è una penalità, non una prova.
#[must_use]
pub fn punteggio(
    sonora: Option<f32>,
    vicinanza: Option<f32>,
    gusto: Option<f32>,
    ripetizione: f32,
    pesi: &Pesi,
) -> Option<f32> {
    let mut somma = 0.0_f32;
    let mut peso = 0.0_f32;
    for (valore, quota) in [
        (sonora, pesi.sonora),
        (vicinanza, pesi.vicinanza),
        (gusto, pesi.gusto),
    ] {
        if let Some(v) = valore {
            if v.is_finite() && quota > 0.0 {
                somma += quota * v.clamp(0.0, 1.0);
                peso += quota;
            }
        }
    }
    if peso <= 0.0 {
        return None;
    }
    let penalita = if ripetizione.is_finite() {
        pesi.ripetizione * ripetizione.clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some((somma / peso - penalita).clamp(0.0, 1.0))
}

#[cfg(test)]
mod prove {
    use super::*;

    const GIORNO: i64 = 86_400_000;

    fn gruppi() -> Vec<Gruppo> {
        vec![
            Gruppo {
                nome: "primo",
                inizio: 0,
                fine: 2,
                peso: 0.5,
            },
            Gruppo {
                nome: "secondo",
                inizio: 2,
                fine: 4,
                peso: 0.5,
            },
        ]
    }

    /// Una libreria finta: `quanti` impronte con valori che variano davvero,
    /// così gli scarti non sono nulli.
    fn libreria(quanti: usize) -> Vec<Vec<f32>> {
        (0..quanti)
            .map(|i| {
                let x = i as f32;
                vec![x, x * 2.0, -x, x * 0.5]
            })
            .collect()
    }

    fn fette(v: &[Vec<f32>]) -> Vec<&[f32]> {
        v.iter().map(Vec::as_slice).collect()
    }

    // ── la scala ────────────────────────────────────────────────────────────

    #[test]
    fn una_libreria_piccola_non_da_una_scala() {
        let v = libreria(MINIMO_BRANI - 1);
        assert!(Scala::misura(fette(&v)).is_none());
    }

    #[test]
    fn una_libreria_grande_abbastanza_la_da() {
        let v = libreria(MINIMO_BRANI);
        let scala = Scala::misura(fette(&v)).unwrap();
        assert_eq!(scala.dimensioni(), 4);
        assert_eq!(scala.brani(), MINIMO_BRANI);
    }

    #[test]
    fn due_tracciati_diversi_non_si_mescolano() {
        let mut v = libreria(MINIMO_BRANI);
        v.push(vec![1.0, 2.0]);
        assert!(Scala::misura(fette(&v)).is_none());
    }

    #[test]
    fn un_descrittore_non_finito_butta_la_misura() {
        let mut v = libreria(MINIMO_BRANI);
        v.push(vec![f32::NAN, 0.0, 0.0, 0.0]);
        assert!(Scala::misura(fette(&v)).is_none());
    }

    #[test]
    fn la_media_e_lo_scarto_sono_quelli() {
        // Cinquanta valori 0..49 sulla prima dimensione: media 24,5 e scarto
        // di popolazione noto.
        let v = libreria(50);
        let scala = Scala::misura(fette(&v)).unwrap();
        assert!((scala.medie()[0] - 24.5).abs() < 1e-3);
        let atteso = (0..50)
            .map(|i| {
                let d = i as f64 - 24.5;
                d * d
            })
            .sum::<f64>()
            / 50.0;
        assert!((f64::from(scala.scarti()[0]) - atteso.sqrt()).abs() < 1e-3);
    }

    #[test]
    fn normalizzare_centra_e_riduce() {
        let v = libreria(MINIMO_BRANI);
        let scala = Scala::misura(fette(&v)).unwrap();
        let z = scala.normalizza(&v[0]).unwrap();
        assert_eq!(z.len(), 4);
        assert!(z.iter().all(|x| x.is_finite()));
        // Il brano che sta alla media dà zero su ogni dimensione.
        let medio: Vec<f32> = scala.medie().to_vec();
        let z = scala.normalizza(&medio).unwrap();
        assert!(z.iter().all(|x| x.abs() < 1e-4));
    }

    #[test]
    fn una_dimensione_costante_vale_zero_e_non_infinito() {
        // Quarta dimensione uguale per tutti: scarto nullo.
        let v: Vec<Vec<f32>> = (0..MINIMO_BRANI)
            .map(|i| vec![i as f32, 0.0, 0.0, 7.0])
            .collect();
        let scala = Scala::misura(fette(&v)).unwrap();
        let z = scala.normalizza(&[10.0, 0.0, 0.0, 999.0]).unwrap();
        assert_eq!(z[3], 0.0);
        assert!(z.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn un_impronta_di_lunghezza_sbagliata_non_si_normalizza() {
        let v = libreria(MINIMO_BRANI);
        let scala = Scala::misura(fette(&v)).unwrap();
        assert!(scala.normalizza(&[1.0, 2.0]).is_none());
    }

    #[test]
    fn una_scala_riletta_a_meta_non_si_ricostruisce() {
        assert!(Scala::da_parti(vec![1.0, 2.0], vec![1.0], 100).is_none());
        assert!(Scala::da_parti(vec![], vec![], 100).is_none());
        assert!(Scala::da_parti(vec![f32::INFINITY], vec![1.0], 100).is_none());
        assert!(Scala::da_parti(vec![1.0], vec![1.0], 100).is_some());
    }

    // ── la distanza ─────────────────────────────────────────────────────────

    #[test]
    fn due_impronte_uguali_distano_zero_e_somigliano_del_tutto() {
        let a = [1.0, 2.0, 3.0, 4.0];
        let d = distanza2(&a, &a, &gruppi()).unwrap();
        assert_eq!(d, 0.0);
        assert_eq!(somiglianza(d), 1.0);
    }

    #[test]
    fn due_estranei_stanno_a_meta() {
        // Scarto quadratico medio di due per dimensione: il caso atteso.
        let a = [0.0, 0.0, 0.0, 0.0];
        let s = 2.0_f32.sqrt();
        let b = [s, s, s, s];
        let d = distanza2(&a, &b, &gruppi()).unwrap();
        assert!((d - RIFERIMENTO).abs() < 1e-5);
        assert!((somiglianza(d) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn un_gruppo_largo_non_conta_piu_di_uno_stretto() {
        // Stesso scarto per dimensione, gruppi di ampiezza diversa e peso
        // uguale: i due contributi devono essere identici.
        let gruppi = [
            Gruppo {
                nome: "uno",
                inizio: 0,
                fine: 1,
                peso: 0.5,
            },
            Gruppo {
                nome: "tre",
                inizio: 1,
                fine: 4,
                peso: 0.5,
            },
        ];
        let a = [0.0, 0.0, 0.0, 0.0];
        let b = [1.0, 1.0, 1.0, 1.0];
        // Ogni gruppo dà media 1, pesata 0,5: totale 1.
        assert!((distanza2(&a, &b, &gruppi).unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn un_gruppo_fuori_dagli_estremi_non_da_una_distanza_piccola() {
        let gruppi = [Gruppo {
            nome: "oltre",
            inizio: 0,
            fine: 9,
            peso: 1.0,
        }];
        assert!(distanza2(&[1.0, 2.0], &[3.0, 4.0], &gruppi).is_none());
    }

    #[test]
    fn due_lunghezze_diverse_non_si_confrontano() {
        assert!(distanza2(&[1.0], &[1.0, 2.0], &gruppi()).is_none());
    }

    #[test]
    fn una_distanza_assurda_non_da_una_somiglianza_assurda() {
        assert_eq!(somiglianza(f32::NAN), 0.0);
        assert_eq!(somiglianza(-1.0), 0.0);
        assert!(somiglianza(f32::INFINITY) >= 0.0);
        assert!(somiglianza(1e30) < 1e-6);
    }

    // ── il gusto ────────────────────────────────────────────────────────────

    #[test]
    fn un_brano_mai_sentito_non_dice_niente() {
        assert_eq!(gusto(&[], 180_000, 0), None);
    }

    #[test]
    fn sei_ascolti_completi_di_oggi_saturano() {
        let ascolti: Vec<Ascolto> = (0..6)
            .map(|_| Ascolto {
                quando_ms: 1_000_000,
                ms_ascoltati: 180_000,
            })
            .collect();
        let g = gusto(&ascolti, 180_000, 1_000_000).unwrap();
        assert!((g - 1.0).abs() < 1e-5);
    }

    #[test]
    fn un_ascolto_a_meta_vale_meta() {
        let ascolti = [Ascolto {
            quando_ms: 0,
            ms_ascoltati: 90_000,
        }];
        let pieno = gusto(
            &[Ascolto {
                quando_ms: 0,
                ms_ascoltati: 180_000,
            }],
            180_000,
            0,
        )
        .unwrap();
        let meta = gusto(&ascolti, 180_000, 0).unwrap();
        assert!((meta - pieno / 2.0).abs() < 1e-5);
    }

    #[test]
    fn un_ascolto_di_sei_mesi_fa_vale_meta_di_uno_di_oggi() {
        let vecchio = gusto(
            &[Ascolto {
                quando_ms: 0,
                ms_ascoltati: 180_000,
            }],
            180_000,
            180 * GIORNO,
        )
        .unwrap();
        let nuovo = gusto(
            &[Ascolto {
                quando_ms: 180 * GIORNO,
                ms_ascoltati: 180_000,
            }],
            180_000,
            180 * GIORNO,
        )
        .unwrap();
        assert!((vecchio - nuovo / 2.0).abs() < 1e-4);
    }

    #[test]
    fn una_durata_sconosciuta_fa_contare_l_ascolto_per_intero() {
        let g = gusto(
            &[Ascolto {
                quando_ms: 0,
                ms_ascoltati: 1,
            }],
            0,
            0,
        )
        .unwrap();
        assert!((g - 1.0 / RIFERIMENTO_ASCOLTI).abs() < 1e-5);
    }

    #[test]
    fn sessanta_ascolti_non_valgono_dieci_volte_sei() {
        let sei: Vec<Ascolto> = (0..6)
            .map(|_| Ascolto {
                quando_ms: 0,
                ms_ascoltati: 180_000,
            })
            .collect();
        let sessanta: Vec<Ascolto> = (0..60)
            .map(|_| Ascolto {
                quando_ms: 0,
                ms_ascoltati: 180_000,
            })
            .collect();
        assert_eq!(
            gusto(&sei, 180_000, 0).unwrap(),
            gusto(&sessanta, 180_000, 0).unwrap()
        );
    }

    // ── la ripetizione ──────────────────────────────────────────────────────

    #[test]
    fn quel_che_non_si_e_mai_sentito_non_si_penalizza() {
        assert_eq!(ripetizione(None, 1_000_000), 0.0);
    }

    #[test]
    fn appena_sentito_pesa_tutto_e_dopo_una_settimana_meta() {
        assert!((ripetizione(Some(0), 0) - 1.0).abs() < 1e-6);
        assert!((ripetizione(Some(0), 7 * GIORNO) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn dopo_un_mese_la_ripetizione_e_trascurabile() {
        assert!(ripetizione(Some(0), 30 * GIORNO) < 0.06);
    }

    #[test]
    fn un_orologio_che_torna_indietro_non_da_una_ripetizione_maggiore_di_uno() {
        assert_eq!(ripetizione(Some(10 * GIORNO), 0), 1.0);
    }

    // ── la miscela ──────────────────────────────────────────────────────────

    #[test]
    fn senza_nessuno_strato_non_c_e_punteggio() {
        assert_eq!(punteggio(None, None, None, 0.0, &Pesi::default()), None);
    }

    #[test]
    fn uno_strato_che_manca_non_vale_zero() {
        let pesi = Pesi::default();
        // Stesso valore su tutti gli strati presenti: il punteggio è quello,
        // qualunque sia il numero di strati che rispondono.
        let tre = punteggio(Some(0.8), Some(0.8), Some(0.8), 0.0, &pesi).unwrap();
        let due = punteggio(Some(0.8), None, Some(0.8), 0.0, &pesi).unwrap();
        let uno = punteggio(Some(0.8), None, None, 0.0, &pesi).unwrap();
        assert!((tre - 0.8).abs() < 1e-6);
        assert!((due - 0.8).abs() < 1e-6);
        assert!((uno - 0.8).abs() < 1e-6);
    }

    #[test]
    fn sapere_di_piu_su_un_brano_non_lo_fa_vincere_da_solo() {
        // Il difetto che la ridistribuzione esiste per impedire: un brano
        // mediocre di cui si sa tutto non deve battere un brano ottimo di cui si
        // sa solo il suono.
        let pesi = Pesi::default();
        let so_tutto = punteggio(Some(0.5), Some(0.5), Some(0.5), 0.0, &pesi).unwrap();
        let so_poco = punteggio(Some(0.9), None, None, 0.0, &pesi).unwrap();
        assert!(so_poco > so_tutto);
    }

    #[test]
    fn quel_che_si_e_appena_sentito_non_puo_vincere() {
        let pesi = Pesi::default();
        let ottimo_ma_appena_sentito = punteggio(Some(1.0), None, None, 1.0, &pesi).unwrap();
        let discreto_e_dimenticato = punteggio(Some(0.6), None, None, 0.0, &pesi).unwrap();
        assert!(discreto_e_dimenticato > ottimo_ma_appena_sentito);
    }

    #[test]
    fn quel_che_si_e_sentito_un_mese_fa_non_se_ne_accorge() {
        let pesi = Pesi::default();
        let intoccato = punteggio(Some(0.8), None, None, 0.0, &pesi).unwrap();
        let un_mese_fa = punteggio(
            Some(0.8),
            None,
            None,
            ripetizione(Some(0), 30 * GIORNO),
            &pesi,
        )
        .unwrap();
        assert!(intoccato - un_mese_fa < 0.03);
    }

    #[test]
    fn un_punteggio_resta_dentro_gli_estremi() {
        let pesi = Pesi::default();
        for (s, v, g, r) in [
            (2.0_f32, 2.0_f32, 2.0_f32, -1.0_f32),
            (-5.0, -5.0, -5.0, 5.0),
            (f32::NAN, 0.5, 0.5, f32::NAN),
        ] {
            let p = punteggio(Some(s), Some(v), Some(g), r, &pesi).unwrap();
            assert!((0.0..=1.0).contains(&p), "fuori scala: {p}");
        }
    }

    #[test]
    fn uno_strato_con_peso_nullo_non_partecipa() {
        let pesi = Pesi {
            sonora: 0.0,
            vicinanza: 1.0,
            gusto: 0.0,
            ripetizione: 0.0,
        };
        // Il suono ha peso zero: il punteggio è quello della sola vicinanza.
        assert_eq!(punteggio(Some(0.1), Some(0.9), None, 0.0, &pesi), Some(0.9));
        // E se rispondesse solo lui, non c'è punteggio da dare.
        assert_eq!(punteggio(Some(0.1), None, None, 0.0, &pesi), None);
    }
}
