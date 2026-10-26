//! Il regolatore del membro: dalle due ancore, la mossa da chiedere al motore.
//!
//! L'ospite è l'orologio maestro: suona per conto suo, e di tanto in tanto
//! dice «il punto P del brano si sente all'istante T» (sul suo orologio). Il
//! membro ha la stessa frase per sé, dal suo motore. Il regolatore le
//! confronta e decide se e come correggere. Non tocca il motore: restituisce
//! una [`Mossa`], e chi lo chiama la esegue. Così si prova con un motore finto,
//! e il tempo passa quanto vuole la prova.
//!
//! # L'errore
//!
//! L'istante dell'ospite si porta sul mio orologio con la [`Stima`]
//! dell'[`orologio`](super::orologio) (`L_h`); senza stima si assume la
//! stessa base, ed è il caso di due motori nello stesso processo. Allora,
//! con `P` in nanosecondi di brano:
//!
//! ```text
//! e = P_m − (P_h + (udito_mio − L_h))
//! ```
//!
//! cioè il mio punto meno quello che l'ospite sente nel mio stesso istante.
//! **Positivo se il membro è avanti.**
//!
//! Il filtro è in due passi, e i due passi servono a cose diverse:
//!
//! - la **mediana degli ultimi 5** campioni toglie i valori anomali isolati
//!   (un'ancora presa a cavallo di un intoppo del driver) senza ritardare un
//!   gradino vero: due campioni storti su cinque non la spostano, tre
//!   campioni coerenti sì. Su questa si decidono gradini e riaperture;
//! - la **media esponenziale** sopra la mediana toglie il rumore fine, quello
//!   che la mediana lascia passare, ed è quella che vede il
//!   proporzionale-integrale. Ha una costante di tempo di un secondo: niente,
//!   rispetto ai venti secondi in cui il regolatore corregge.
//!
//! # Tre modi di correggere, e perché queste soglie
//!
//! Ogni correzione ha un costo che si sente, e un errore che si sente; le
//! soglie stanno dove il secondo supera il primo.
//!
//! - **Oltre 500 ms, o brano diverso da più di 1 s, o l'ospite ha saltato di
//!   più di 200 ms**: si riapre il brano al punto giusto, con un avvio
//!   programmato 250 ms più in là. Costa un buco di un quarto di secondo, ma
//!   un gradino di mezzo secondo costerebbe altrettanto silenzio, e con un
//!   salto all'indietro più dell'anello che c'è. 250 ms bastano ad aprire un
//!   file e a far partire il primo blocco; se non bastano, il motore trasforma
//!   il ritardo in un salto e la sincronia resta esatta. Il secondo di grazia
//!   sul brano diverso è per il gapless: al cambio i due PC passano al brano
//!   dopo a qualche millisecondo di distanza, e per quei millisecondi le
//!   etichette non coincidono senza che ci sia niente da fare. Il salto
//!   dell'ospite oltre 200 ms è un suo `vai_a`: aspettare di vederlo
//!   nell'errore costerebbe tre campioni.
//! - **Fra 20 e 500 ms per 3 campioni**: un gradino, cioè silenzio inserito o
//!   musica saltata, con la porta del motore che chiude e riapre in 12 ms. Si
//!   sente come un piccolo vuoto, ed è esatto al fotogramma. Venti
//!   millisecondi sono più o meno dove due altoparlanti nella stessa stanza
//!   smettono di fondersi in uno; sotto, il gradino costerebbe più di quel che
//!   toglie. Tre campioni perché la mediana lo vuole: un gradino su un valore
//!   anomalo è il difetto peggiore che un regolatore possa avere.
//! - **Entro 20 ms**: si cambia la velocità, togliendo o ripetendo un
//!   fotogramma ogni tanto (`Motore::deriva`), entro ±200 ppm: un fotogramma
//!   ogni cinquemila, che non si sente. Proporzionale-integrale:
//!   - `Kp = 50 ppm/ms`: un millisecondo d'errore si recupera in una ventina
//!     di secondi, lento abbastanza da non inseguire il rumore delle ancore;
//!   - `Ki = 1 ppm/(ms·s)`: l'integrale porta a zero l'errore che resterebbe
//!     con la sola parte proporzionale quando i due quarzi derivano (con
//!     100 ppm di deriva, 2 ms). Con questi due guadagni l'anello ha uno
//!     smorzamento di 0,8: si assesta senza oscillare;
//!   - **zona morta di 0,3 ms**: lì dentro la parte proporzionale tace e
//!     l'integratore è fermo, perché il rumore delle ancore è di quell'ordine
//!     e inseguirlo vorrebbe dire un comando che trema. La zona morta è
//!     *continua*: fuori, proporzionale e integrale lavorano su `e ∓ 0,3 ms`,
//!     non su `e`. Così il comando non salta di 15 ppm attraversando il
//!     bordo, e — soprattutto — l'integratore non accumula i 0,3 ms del bordo
//!     come se fossero errore: lo farebbe a ogni passaggio, e l'errore
//!     girerebbe lentamente da un bordo all'altro;
//!   - **anti-saturazione**: con l'uscita al tetto l'integrale non cresce più
//!     in quella direzione, altrimenti dopo un lungo errore ci metterebbe
//!     altrettanto a scaricarsi;
//!   - **l'integratore parte dalla pendenza dei primi 10 s**: partire da zero
//!     vorrebbe dire aspettare un minuto perché l'integrale scopra la deriva da
//!     solo. La pendenza dell'errore in quei dieci secondi *è* la deriva — ma
//!     togliendo il contenuto che il regolatore stesso ha fatto avanzare o
//!     arretrare con la parte proporzionale, che altrimenti la falserebbe.
//!
//! Il prezzo della zona morta, misurato sul motore simulato delle prove: con
//! 100 ppm di deriva, nei primi dieci secondi l'errore sale a 0,9 ms; mentre
//! la parte proporzionale lo riporta al bordo l'integrale accumula qualche ppm
//! di troppo, e una volta dentro la zona — dove sta fermo — quel resto fa
//! scivolare l'errore lentamente verso l'altro bordo. Ne viene un giro lento,
//! di minuti, entro ±0,4 ms: sotto ogni soglia che si senta, ed è il motivo
//! per cui la prova chiede meno di 0,5 ms e non meno di 0,3.
//!
//! Le altre due righe della tabella sono lo stato: ospite in pausa e io no,
//! `Pausa`; ospite in corso e io no, `AvviaA` — a un istante calcolato perché
//! si parta già allineati, non un «riprendi» seguito da una correzione.
//!
//! L'integratore **resta** durante la pausa, dopo un gradino e dopo una
//! riapertura: la deriva è una proprietà delle due schede, non del brano.
//! Si azzera con [`Regolatore::azzera`], da chiamare quando cambia il
//! dispositivo d'uscita.
//!
//! # Dopo ogni mossa si aspetta
//!
//! Un gradino, un avvio e una riapertura cambiano l'errore di colpo, e i
//! campioni presi a metà dicono cose false. Dopo ciascuno il regolatore
//! aspetta che la mossa sia finita (con una scadenza di 2 s, se il motore non
//! la porta a termine), poi butta la storia del filtro e ricomincia a
//! misurare. Per il gradino aspetta sia la durata prevista sia che
//! `in_correzione` torni falso: il motore la alza già nella richiesta, ma il
//! regolatore non ci conta.

use super::orologio::Stima;

/// Nanosecondi in un millisecondo, per i guadagni.
const NS_PER_MS: f64 = 1e6;

/// Nanosecondi in un secondo.
const NS_PER_S: f64 = 1e9;

/// Un miliardo, per le conversioni intere.
const MILIARDO: i128 = 1_000_000_000;

/// Quanto la porta del motore impiega a chiudersi o ad aprirsi: vedi
/// `crate::uscita`. Un gradino ne costa due, più il suo silenzio.
const PORTA_NS: u64 = 12_000_000;

/// Quanti campioni tiene la mediana.
const MEDIANA: usize = 5;

/// L'ultima ancora dell'ospite, così come arriva.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DallOspite {
    /// La voce di sessione: uguale su tutti i PC.
    pub etichetta: u64,
    /// Il punto del brano, in microsecondi.
    pub posizione_us: u64,
    /// L'istante in cui l'ospite lo sente, sul **suo** orologio.
    pub udito_ospite_ns: u64,
    /// Se l'ospite sta suonando. In pausa, `posizione_us` è il punto da cui
    /// ripartirà e `udito_ospite_ns` non conta.
    pub in_corso: bool,
    /// Un avvio che l'ospite ha programmato in anticipo — una ripresa, un
    /// salto, una voce scelta — e non la posizione di una voce che suona già.
    /// Una voce diversa annunciata così è un gesto, non un passaggio gapless:
    /// non si aspetta [`Taratura::etichetta_diversa_ns`].
    pub programmata: bool,
}

/// L'ultima ancora mia, dal mio motore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaMe {
    /// La voce di sessione del brano che suono; `None` fuori sessione.
    pub etichetta: Option<u64>,
    /// Il punto del brano, in microsecondi. In pausa, quello del prossimo
    /// fotogramma (`Motore::in_attesa`).
    pub posizione_us: u64,
    /// L'istante in cui lo sento, sul mio orologio. In pausa non conta.
    pub udito_ns: u64,
    /// Se sto suonando.
    pub in_corso: bool,
    /// Se un gradino è chiesto o in corso.
    pub in_correzione: bool,
    /// Quanta musica c'è già nell'anello, in microsecondi
    /// (`Motore::riserva_us`): è quanta se ne può saltare.
    pub riserva_us: u64,
}

/// Cosa chiedere al motore.
///
/// Le convenzioni di segno sono quelle del motore, non quelle dell'errore:
/// vedi la documentazione di ciascuna voce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mossa {
    /// Niente da fare.
    Niente,
    /// Riaprire il brano `etichetta` in pausa a `posizione_us`, e farlo
    /// sentire all'istante `quando_ns`: `apri_fermo` e poi `riprendi_a`.
    ///
    /// `posizione_us` è il punto che l'ospite suonerà in `quando_ns`. Chi apre
    /// a millisecondi interi (`apri_fermo` li vuole così) arrotonda per
    /// difetto e anticipa l'avvio della stessa quantità: il punto `da_ms` va
    /// sentito in `quando_ns − (posizione_us − da_ms·1000)·1000`. Altrimenti
    /// fino a 1 ms d'errore resta a carico della deriva, che ci mette venti
    /// secondi a toglierlo.
    Riapri {
        /// La voce di sessione da aprire.
        etichetta: u64,
        /// Il punto da cui aprire, in microsecondi.
        posizione_us: u64,
        /// Quando farlo sentire, sul mio orologio.
        quando_ns: u64,
    },
    /// `Motore::gradino`: positivo inserisce tanti fotogrammi di silenzio
    /// (sono **avanti**), negativo ne salta altrettanti di musica (sono
    /// **indietro**).
    Gradino(i64),
    /// `Motore::deriva`, in parti per milione: positivo consuma la musica più
    /// in fretta (sono indietro), negativo più piano (sono avanti). Si
    /// manda solo quando il valore cambia.
    Deriva(i32),
    /// `Motore::pausa`: l'ospite si è fermato.
    Pausa,
    /// Ripartire dalla pausa a un istante preciso: in `quando_ns` si deve
    /// sentire il fotogramma che sta `salta` fotogrammi **dopo** quello in
    /// attesa. `salta` non è mai negativo: se sono avanti, l'avvio slitta più
    /// in là invece.
    ///
    /// Il modo di eseguirla è `riprendi_a(quando_ns − salta·1e9/frequenza)`:
    /// se quell'istante è già passato, il motore salta da sé la musica che
    /// doveva già essere uscita. **Non** `gradino(−salta)` seguito da
    /// `riprendi_a`: in pausa la callback lascia cadere i gradini chiesti.
    AvviaA {
        /// L'istante, sul mio orologio.
        quando_ns: u64,
        /// Quanti fotogrammi saltare, alla frequenza d'uscita.
        salta: i64,
    },
    /// Tutti e due fermi, ma non allo stesso punto: l'ospite ha saltato (o
    /// cambiato voce) mentre era in pausa. Mettersi lì, **fermi**: la voce
    /// `etichetta` al punto `posizione_us`, senza suonare. Così la barra di chi
    /// segue dice il punto vero, e alla ripresa basta un avvio programmato
    /// invece di una riapertura dalla rete.
    Posiziona {
        /// La voce di sessione.
        etichetta: u64,
        /// Il punto, in microsecondi.
        posizione_us: u64,
    },
}

/// Le soglie e i guadagni. [`Default`] dà quelli della specifica.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Taratura {
    /// Sotto questo errore la parte proporzionale tace e l'integratore sta
    /// fermo: 0,3 ms.
    pub zona_morta_ns: u64,
    /// Oltre questo errore si corregge col gradino: 20 ms.
    pub soglia_gradino_ns: u64,
    /// Oltre questo errore si riapre: 500 ms.
    pub soglia_riapri_ns: u64,
    /// Un salto dell'ospite oltre questo fa riaprire subito: 200 ms.
    pub salto_ospite_ns: u64,
    /// Quanti campioni di fila oltre la soglia servono per agire: 3.
    pub campioni_di_fila: u32,
    /// La costante di tempo della media esponenziale: 1 s.
    pub costante_media_ns: u64,
    /// Il guadagno proporzionale, in ppm per millisecondo: 50.
    pub kp_ppm_per_ms: f64,
    /// Il guadagno integrale, in ppm per millisecondo per secondo: 1.
    pub ki_ppm_per_ms_s: f64,
    /// Il tetto della deriva chiesta: 200 ppm.
    pub ppm_massimo: i32,
    /// Per quanto si misura la pendenza prima di inizializzare
    /// l'integratore: 10 s.
    pub avvio_integratore_ns: u64,
    /// Per quanto si tollera un brano diverso da quello dell'ospite: 1 s.
    pub etichetta_diversa_ns: u64,
    /// Quanto avanti si programma una riapertura o un avvio: 250 ms.
    pub anticipo_ns: u64,
    /// Quanta musica in più del salto deve esserci nell'anello: 20 ms.
    pub margine_riserva_ns: u64,
    /// Quanto si aspetta al massimo che una mossa finisca: 2 s.
    pub attesa_massima_ns: u64,
}

impl Default for Taratura {
    fn default() -> Self {
        Self {
            zona_morta_ns: 300_000,
            soglia_gradino_ns: 20_000_000,
            soglia_riapri_ns: 500_000_000,
            salto_ospite_ns: 200_000_000,
            campioni_di_fila: 3,
            costante_media_ns: 1_000_000_000,
            kp_ppm_per_ms: 50.0,
            ki_ppm_per_ms_s: 1.0,
            ppm_massimo: 200,
            avvio_integratore_ns: 10_000_000_000,
            etichetta_diversa_ns: 1_000_000_000,
            anticipo_ns: 250_000_000,
            margine_riserva_ns: 20_000_000,
            attesa_massima_ns: 2_000_000_000,
        }
    }
}

/// Lo stato interno, da leggere: per il diario e per il CSV del banco.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Diagnosi {
    /// L'ultimo errore dopo la mediana, in ns; `None` se non se ne misura.
    pub errore_ns: Option<i64>,
    /// L'errore dopo la media esponenziale, quello che vede il PI.
    pub errore_filtrato_ns: Option<i64>,
    /// L'ultima deriva chiesta, in ppm.
    pub ppm: i32,
    /// La parte integrale, in ppm.
    pub integrale_ppm: f64,
    /// Se l'integratore è già stato inizializzato dalla pendenza.
    pub integratore_pronto: bool,
}

/// Cosa sta aspettando il regolatore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fase {
    /// Niente: si misura e si decide.
    Libero,
    /// Un gradino chiesto: finisce non prima di `pronto_dal`, e quando
    /// `in_correzione` è falso.
    Correzione { pronto_dal: u64, scadenza: u64 },
    /// Un avvio programmato in `quando`.
    Avvio { quando: u64, scadenza: u64 },
    /// Una riapertura di `etichetta`, da sentire in `quando`.
    Riapertura {
        etichetta: u64,
        quando: u64,
        scadenza: u64,
    },
    /// Una pausa chiesta.
    Pausa { scadenza: u64 },
}

/// La retta dell'errore nei primi secondi, per la deriva naturale.
#[derive(Debug, Clone, Copy, Default)]
struct Pendenza {
    inizio: Option<u64>,
    /// Il contenuto che le correzioni del regolatore hanno fatto avanzare
    /// (positivo) o arretrare, in ns: va tolto dall'errore.
    compensato_ns: f64,
    n: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
}

impl Pendenza {
    /// Un campione d'errore in `adesso`, dopo `dt_ns` passati con `ppm`.
    fn aggiungi(&mut self, adesso: u64, errore_ns: f64, ppm: i32, dt_ns: u64) {
        let inizio = *self.inizio.get_or_insert(adesso);
        self.compensato_ns += f64::from(ppm) * 1e-6 * dt_ns as f64;
        let x = adesso.saturating_sub(inizio) as f64 / NS_PER_S;
        let y = errore_ns - self.compensato_ns;
        self.n += 1.0;
        self.sx += x;
        self.sy += y;
        self.sxx += x * x;
        self.sxy += x * y;
    }

    /// La deriva naturale in ppm (positiva se il membro tende ad andare
    /// avanti), se si misura da almeno `durata_ns`.
    fn deriva_ppm(&self, adesso: u64, durata_ns: u64) -> Option<f64> {
        let inizio = self.inizio?;
        if adesso.saturating_sub(inizio) < durata_ns || self.n < 3.0 {
            return None;
        }
        let denominatore = self.n * self.sxx - self.sx * self.sx;
        if denominatore <= 0.0 {
            return None;
        }
        // ns d'errore al secondo: 1000 ns/s sono 1 ppm.
        Some((self.n * self.sxy - self.sx * self.sy) / denominatore / 1000.0)
    }
}

/// L'ancora dell'ospite con il suo istante già sul mio orologio.
#[derive(Debug, Clone, Copy)]
struct Ospite {
    ancora: DallOspite,
    locale: u64,
}

impl Ospite {
    /// Il punto che l'ospite suona nel mio istante `t`, in ns di brano.
    fn posizione_ns_a(&self, t: u64) -> i128 {
        let p = i128::from(self.ancora.posizione_us) * 1000;
        if self.ancora.in_corso {
            p + i128::from(t) - i128::from(self.locale)
        } else {
            p
        }
    }
}

/// Il regolatore del membro. Vedi la documentazione del modulo.
#[derive(Debug, Clone)]
pub struct Regolatore {
    t: Taratura,
    fase: Fase,
    /// Gli ultimi campioni d'errore, in ns, ad anello.
    ultimi: [f64; MEDIANA],
    quanti: usize,
    posto: usize,
    /// L'ultima mediana.
    mediana_ns: Option<f64>,
    /// La media esponenziale sopra la mediana.
    media_ns: Option<f64>,
    /// `adesso` dell'ultimo campione del PI.
    ultimo_campione: Option<u64>,
    /// Quanti campioni di fila oltre la soglia del gradino.
    fuori_di_fila: u32,
    integrale_ppm: f64,
    integratore_pronto: bool,
    pendenza: Pendenza,
    /// L'ultima deriva chiesta; `None` se il motore potrebbe non averla (una
    /// riapertura, o mai chiesta).
    ppm_dato: Option<i32>,
    /// L'ancora precedente dell'ospite, per vederne i salti.
    ospite_prima: Option<DallOspite>,
    /// Da quando le etichette non coincidono.
    diversa_dal: Option<u64>,
    /// L'ultimo punto (voce, µs) chiesto con [`Mossa::Posiziona`]: vedi
    /// `posiziona`. Si dimentica appena l'ospite riparte.
    posizionata: Option<(u64, u64)>,
}

impl Regolatore {
    /// Un regolatore senza storia.
    #[must_use]
    pub fn nuovo(t: Taratura) -> Self {
        Self {
            t,
            fase: Fase::Libero,
            ultimi: [0.0; MEDIANA],
            quanti: 0,
            posto: 0,
            mediana_ns: None,
            media_ns: None,
            ultimo_campione: None,
            fuori_di_fila: 0,
            integrale_ppm: 0.0,
            integratore_pronto: false,
            pendenza: Pendenza::default(),
            ppm_dato: None,
            ospite_prima: None,
            diversa_dal: None,
            posizionata: None,
        }
    }

    /// La mossa appena chiesta non si è potuta fare (la voce non si è aperta,
    /// per esempio): il regolatore torna libero e al passo dopo misura e
    /// decide da capo, invece di aspettare la scadenza di una mossa che non è
    /// mai partita — fino a più di due secondi in cui ignorava anche le pause
    /// e i salti dell'ospite. La deriva imparata resta: la scheda è la stessa.
    pub fn mossa_non_riuscita(&mut self) {
        self.fase = Fase::Libero;
        self.diversa_dal = None;
        self.ricomincia_a_misurare();
    }

    /// Da capo, integratore compreso: va chiamato quando cambia il
    /// dispositivo d'uscita, perché la deriva imparata era di quello.
    pub fn azzera(&mut self) {
        *self = Self::nuovo(self.t);
    }

    /// Lo stato interno, per chi lo vuole scrivere da qualche parte.
    #[must_use]
    pub fn diagnosi(&self) -> Diagnosi {
        Diagnosi {
            errore_ns: self.mediana_ns.and_then(intero_i64),
            errore_filtrato_ns: self.media_ns.and_then(intero_i64),
            ppm: self.ppm_dato.unwrap_or(0),
            integrale_ppm: self.integrale_ppm,
            integratore_pronto: self.integratore_pronto,
        }
    }

    /// Un passo: le ultime ancore, l'istante di adesso sul mio orologio, la
    /// stima dell'orologio dell'ospite (`None`: stessa base) e la frequenza
    /// d'uscita del mio motore. Restituisce una mossa sola.
    ///
    /// Va chiamato a ritmo regolare (il banco usa 10 Hz), con l'ancora più
    /// recente di ciascuno; la stessa ancora due volte non fa danni. Chi ha
    /// due PC e ancora nessuna stima dell'orologio **non** deve passare
    /// `None`, che vuol dire «stessa base»: deve aspettare.
    pub fn passo(
        &mut self,
        adesso_ns: u64,
        ospite: Option<&DallOspite>,
        mia: Option<&DaMe>,
        orologio: Option<&Stima>,
        frequenza: u32,
    ) -> Mossa {
        let Some(&ancora) = ospite else {
            return Mossa::Niente;
        };
        let locale = match orologio {
            Some(stima) => stima.locale_da_ospite(ancora.udito_ospite_ns),
            None => Some(ancora.udito_ospite_ns),
        };
        let Some(locale) = locale else {
            return Mossa::Niente;
        };
        if frequenza == 0 {
            return Mossa::Niente;
        }
        let ospite = Ospite { ancora, locale };
        let mia = mia.copied();

        if self.ha_saltato(&ancora) && ancora.in_corso {
            return self.riapri(adesso_ns, &ospite);
        }
        if !self.attesa_finita(adesso_ns, &ancora, mia.as_ref()) {
            return Mossa::Niente;
        }
        if !ancora.in_corso {
            self.diversa_dal = None;
            return match mia {
                Some(m) if m.in_corso => {
                    self.fase = Fase::Pausa {
                        scadenza: adesso_ns.saturating_add(self.t.attesa_massima_ns),
                    };
                    Mossa::Pausa
                }
                _ => self.posiziona(&ancora, mia.as_ref()),
            };
        }
        self.posizionata = None;
        let Some(m) = mia.filter(|m| m.etichetta == Some(ancora.etichetta)) else {
            // Brano diverso, o nessuna ancora mia. Se sono fermo non c'è niente
            // da aspettare; se suono, può essere il passaggio gapless. Non se
            // l'ospite l'ha programmato: è un «prossimo», una voce scelta, e
            // aspettare vorrebbe dire sentire il brano vecchio un secondo di
            // più, sopra il titolo nuovo.
            let fermo = mia.is_some_and(|m| !m.in_corso);
            let dal = *self.diversa_dal.get_or_insert(adesso_ns);
            if fermo
                || ancora.programmata
                || adesso_ns.saturating_sub(dal) > self.t.etichetta_diversa_ns
            {
                return self.riapri(adesso_ns, &ospite);
            }
            return Mossa::Niente;
        };
        self.diversa_dal = None;
        if !m.in_corso {
            return self.avvia(adesso_ns, &ospite, &m, frequenza);
        }
        self.regola(adesso_ns, &ospite, &m, frequenza)
    }

    /// Tutti e due fermi: se il mio punto è lontano da quello dell'ospite (o la
    /// voce è un'altra, o non ne ho), [`Mossa::Posiziona`] — una volta per
    /// punto, perché una mossa che non riesce non va ripetuta a ogni passo.
    fn posiziona(&mut self, ancora: &DallOspite, mia: Option<&DaMe>) -> Mossa {
        let lontano = mia.is_none_or(|m| {
            m.etichetta != Some(ancora.etichetta)
                || u128::from(m.posizione_us.abs_diff(ancora.posizione_us)).saturating_mul(1000)
                    > u128::from(self.t.salto_ospite_ns)
        });
        let punto = (ancora.etichetta, ancora.posizione_us);
        if !lontano || self.posizionata == Some(punto) {
            return Mossa::Niente;
        }
        self.posizionata = Some(punto);
        Mossa::Posiziona {
            etichetta: ancora.etichetta,
            posizione_us: ancora.posizione_us,
        }
    }

    /// Se l'ospite ha saltato rispetto alla sua ancora precedente.
    ///
    /// Si confrontano solo due ancore dello stesso brano e nello stesso
    /// stato: da una pausa a una ripresa la distanza dipende da quando l'ospite
    /// è ripartito, che non si sa.
    fn ha_saltato(&mut self, ancora: &DallOspite) -> bool {
        let Some(prima) = self.ospite_prima.replace(*ancora) else {
            return false;
        };
        if prima.etichetta != ancora.etichetta || prima.in_corso != ancora.in_corso {
            return false;
        }
        let mut atteso = i128::from(prima.posizione_us) * 1000;
        if prima.in_corso {
            atteso += i128::from(ancora.udito_ospite_ns) - i128::from(prima.udito_ospite_ns);
        }
        let adesso = i128::from(ancora.posizione_us) * 1000;
        (adesso - atteso).unsigned_abs() > u128::from(self.t.salto_ospite_ns)
    }

    /// Se la mossa in corso è finita; se sì, si torna a misurare da capo.
    ///
    /// Una riapertura finisce anche se nel frattempo l'ospite è passato a un
    /// altro brano: aspettare quello vecchio fino alla scadenza non serve.
    fn attesa_finita(&mut self, adesso: u64, ospite: &DallOspite, mia: Option<&DaMe>) -> bool {
        let suona = |m: &&DaMe| m.in_corso && !m.in_correzione;
        let finita = match self.fase {
            Fase::Libero => return true,
            Fase::Correzione {
                pronto_dal,
                scadenza,
            } => {
                adesso >= scadenza
                    || (adesso >= pronto_dal && !mia.is_some_and(|m| m.in_correzione))
            }
            Fase::Avvio { quando, scadenza } => {
                adesso >= scadenza || (adesso >= quando && mia.filter(suona).is_some())
            }
            Fase::Riapertura {
                etichetta,
                quando,
                scadenza,
            } => {
                adesso >= scadenza
                    || ospite.etichetta != etichetta
                    || (adesso >= quando
                        && mia
                            .filter(suona)
                            .is_some_and(|m| m.etichetta == Some(etichetta)))
            }
            Fase::Pausa { scadenza } => adesso >= scadenza || mia.is_none_or(|m| !m.in_corso),
        };
        if finita {
            self.fase = Fase::Libero;
            self.ricomincia_a_misurare();
        }
        finita
    }

    /// Butta la storia del filtro: i campioni presi prima di una mossa non
    /// dicono niente su dopo. L'integratore resta.
    fn ricomincia_a_misurare(&mut self) {
        self.quanti = 0;
        self.posto = 0;
        self.mediana_ns = None;
        self.media_ns = None;
        self.ultimo_campione = None;
        self.fuori_di_fila = 0;
        if !self.integratore_pronto {
            self.pendenza = Pendenza::default();
        }
    }

    /// Tutti e due suonano lo stesso brano: si misura e si corregge.
    fn regola(&mut self, adesso: u64, ospite: &Ospite, m: &DaMe, frequenza: u32) -> Mossa {
        let errore = i128::from(m.posizione_us) * 1000 - ospite.posizione_ns_a(m.udito_ns);
        let mediana = self.filtra(errore as f64);
        let ampiezza = mediana.abs();
        if ampiezza > self.t.soglia_gradino_ns as f64 {
            self.fuori_di_fila = self.fuori_di_fila.saturating_add(1);
            if self.fuori_di_fila < self.t.campioni_di_fila {
                return Mossa::Niente;
            }
            if ampiezza > self.t.soglia_riapri_ns as f64 {
                return self.riapri(adesso, ospite);
            }
            return self.gradino(adesso, mediana, ospite, m, frequenza);
        }
        self.fuori_di_fila = 0;
        self.proporzionale_integrale(adesso, mediana)
    }

    /// Aggiunge un campione e restituisce la mediana di quelli che ci sono.
    fn filtra(&mut self, errore: f64) -> f64 {
        if let Some(posto) = self.ultimi.get_mut(self.posto) {
            *posto = errore;
        }
        self.posto = (self.posto + 1) % MEDIANA;
        self.quanti = (self.quanti + 1).min(MEDIANA);
        let mut copia = self.ultimi;
        let presenti = copia.get_mut(..self.quanti).unwrap_or_default();
        let mediana = mediana(presenti).unwrap_or(errore);
        self.mediana_ns = Some(mediana);
        mediana
    }

    /// Un gradino esatto, o una riapertura se all'indietro l'anello non basta.
    fn gradino(
        &mut self,
        adesso: u64,
        errore_ns: f64,
        ospite: &Ospite,
        m: &DaMe,
        frequenza: u32,
    ) -> Mossa {
        let Some(ns) = intero(errore_ns) else {
            return Mossa::Niente;
        };
        let fotogrammi = fotogrammi_di(ns, frequenza);
        if fotogrammi == 0 {
            return Mossa::Niente;
        }
        if fotogrammi < 0 {
            // Sono indietro: si salta musica, e deve già essere nell'anello.
            let serve = ns.unsigned_abs() + u128::from(self.t.margine_riserva_ns);
            if u128::from(m.riserva_us) * 1000 < serve {
                return self.riapri(adesso, ospite);
            }
        }
        let silenzio = if fotogrammi > 0 {
            u64::try_from(ns).unwrap_or(u64::MAX)
        } else {
            0
        };
        // Chiusura, silenzio, riapertura, e una porta di margine.
        let pronto_dal = adesso.saturating_add(3 * PORTA_NS).saturating_add(silenzio);
        self.fase = Fase::Correzione {
            pronto_dal,
            scadenza: pronto_dal.saturating_add(self.t.attesa_massima_ns),
        };
        Mossa::Gradino(fotogrammi)
    }

    /// La deriva, dal proporzionale-integrale sull'errore filtrato.
    fn proporzionale_integrale(&mut self, adesso: u64, mediana: f64) -> Mossa {
        let dt_ns = self
            .ultimo_campione
            .map_or(0, |prima| adesso.saturating_sub(prima))
            .min(1_000_000_000);
        self.ultimo_campione = Some(adesso);
        let dt = dt_ns as f64;
        let media = match self.media_ns {
            Some(v) => v + (mediana - v) * dt / (self.t.costante_media_ns as f64 + dt),
            None => mediana,
        };
        self.media_ns = Some(media);

        // La zona morta continua: vedi la documentazione del modulo.
        let morta = self.t.zona_morta_ns as f64;
        let fuori_ms = if media.abs() <= morta {
            0.0
        } else {
            (media - morta.copysign(media)) / NS_PER_MS
        };
        let massimo = f64::from(self.t.ppm_massimo.max(0));
        let proporzionale = -self.t.kp_ppm_per_ms * fuori_ms;

        if self.integratore_pronto {
            if fuori_ms != 0.0 {
                let candidato =
                    self.integrale_ppm - self.t.ki_ppm_per_ms_s * fuori_ms * dt / NS_PER_S;
                let prima = (proporzionale + self.integrale_ppm).abs();
                let dopo = (proporzionale + candidato).abs();
                // Anti-saturazione: al tetto, l'integrale può solo scaricarsi.
                if dopo <= massimo || dopo < prima {
                    self.integrale_ppm = candidato.clamp(-massimo, massimo);
                }
            }
        } else {
            self.pendenza
                .aggiungi(adesso, mediana, self.ppm_dato.unwrap_or(0), dt_ns);
            if let Some(naturale) = self
                .pendenza
                .deriva_ppm(adesso, self.t.avvio_integratore_ns)
            {
                self.integrale_ppm = (-naturale).clamp(-massimo, massimo);
                self.integratore_pronto = true;
            }
        }

        let uscita = (proporzionale + self.integrale_ppm).clamp(-massimo, massimo);
        let ppm = ppm_intero(uscita);
        if self.ppm_dato == Some(ppm) {
            return Mossa::Niente;
        }
        self.ppm_dato = Some(ppm);
        Mossa::Deriva(ppm)
    }

    /// Ospite in corso e io in pausa sullo stesso brano: si parte già
    /// allineati.
    fn avvia(&mut self, adesso: u64, ospite: &Ospite, m: &DaMe, frequenza: u32) -> Mossa {
        let presto = adesso.saturating_add(self.t.anticipo_ns);
        // Positivo se il mio punto in attesa è avanti rispetto a quello che
        // l'ospite suonerà in `presto`.
        let errore = i128::from(m.posizione_us) * 1000 - ospite.posizione_ns_a(presto);
        if errore.unsigned_abs() > u128::from(self.t.soglia_riapri_ns) {
            return self.riapri(adesso, ospite);
        }
        let (quando, salta) = if errore >= 0 {
            // Avanti: si aspetta che l'ospite arrivi al mio punto.
            let attesa = u64::try_from(errore).unwrap_or(u64::MAX);
            (presto.saturating_add(attesa), 0)
        } else {
            let serve = errore.unsigned_abs() + u128::from(self.t.margine_riserva_ns);
            if u128::from(m.riserva_us) * 1000 < serve {
                return self.riapri(adesso, ospite);
            }
            (presto, fotogrammi_di(-errore, frequenza))
        };
        self.fase = Fase::Avvio {
            quando,
            scadenza: quando.saturating_add(self.t.attesa_massima_ns),
        };
        Mossa::AvviaA {
            quando_ns: quando,
            salta,
        }
    }

    /// Si riapre il brano dell'ospite dove sarà fra `anticipo_ns`.
    fn riapri(&mut self, adesso: u64, ospite: &Ospite) -> Mossa {
        let quando = adesso.saturating_add(self.t.anticipo_ns);
        let posizione_ns = ospite.posizione_ns_a(quando).max(0);
        let posizione_us = u64::try_from(dividi(posizione_ns, 1000)).unwrap_or(u64::MAX);
        let etichetta = ospite.ancora.etichetta;
        self.fase = Fase::Riapertura {
            etichetta,
            quando,
            scadenza: quando.saturating_add(self.t.attesa_massima_ns),
        };
        self.ppm_dato = None;
        self.diversa_dal = None;
        self.ricomincia_a_misurare();
        Mossa::Riapri {
            etichetta,
            posizione_us,
            quando_ns: quando,
        }
    }
}

/// Nanosecondi di brano in fotogrammi, arrotondati al più vicino.
fn fotogrammi_di(ns: i128, frequenza: u32) -> i64 {
    let fotogrammi = dividi(ns.saturating_mul(i128::from(frequenza)), MILIARDO);
    i64::try_from(fotogrammi).unwrap_or(if ns < 0 { i64::MIN } else { i64::MAX })
}

/// La mediana, riordinando sul posto; con un numero pari di valori, la media
/// dei due di mezzo.
#[expect(
    clippy::integer_division,
    reason = "l'indice di mezzo di cinque valori al più: la divisione per due è \
              quella che si vuole, col resto che decide se i valori di mezzo sono \
              uno o due"
)]
fn mediana(valori: &mut [f64]) -> Option<f64> {
    valori.sort_by(f64::total_cmp);
    let mezzo = valori.len() / 2;
    let alto = *valori.get(mezzo)?;
    if valori.len() % 2 == 1 {
        Some(alto)
    } else {
        Some(0.5 * (alto + *valori.get(mezzo.checked_sub(1)?)?))
    }
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

/// Come [`intero`], ma in un `i64`.
fn intero_i64(x: f64) -> Option<i64> {
    i64::try_from(intero(x)?).ok()
}

/// Le ppm come le vuole il motore.
#[expect(
    clippy::cast_possible_truncation,
    reason = "il valore è già limitato al tetto della deriva, poche centinaia: \
              arrotondato sta in un i32"
)]
fn ppm_intero(x: f64) -> i32 {
    if x.is_finite() {
        x.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    } else {
        0
    }
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

    const F: u32 = 48_000;
    const MS: u64 = 1_000_000;
    const SECONDO: u64 = 1_000_000_000;
    /// Il passo della simulazione.
    const PASSO: u64 = MS;
    /// Ogni quanto si chiama il regolatore: 10 Hz, come il desktop.
    const OGNI: u64 = 100 * MS;
    /// Quanto dopo l'istante dell'ancora si sente il suo punto: la latenza
    /// d'uscita, uguale per i due.
    const LATENZA: u64 = 20 * MS;

    /// Un generatore pseudo-casuale con seme fisso.
    struct Lcg(u64);

    impl Lcg {
        fn uniforme(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }

        /// Uniforme fra −a e a.
        fn fino_a(&mut self, a: f64) -> f64 {
            a * (2.0 * self.uniforme() - 1.0)
        }
    }

    /// Un tempo con la virgola come intero.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "tempi simulati, positivi e lontani da u64::MAX"
    )]
    fn intero_u64(x: f64) -> u64 {
        x.max(0.0).round() as u64
    }

    /// Nanosecondi di brano in microsecondi, per difetto, come li riporta il
    /// motore.
    fn us(ns: f64) -> u64 {
        intero_u64((ns / 1000.0).floor())
    }

    /// L'ospite: suona a velocità uno sul suo orologio, che è il mio più
    /// `scarto`.
    struct OspiteFinto {
        etichetta: u64,
        /// Il punto del brano, in ns, nell'istante della simulazione.
        contenuto: f64,
        in_corso: bool,
        scarto: u64,
        programmata: bool,
    }

    /// Il motore del membro, che esegue le mosse come le descrive la
    /// specifica (A4). Se un segno del regolatore fosse sbagliato, qui
    /// l'errore crescerebbe invece di calare.
    struct MembroFinto {
        etichetta: Option<u64>,
        /// Il punto del brano, in ns, nell'istante della simulazione.
        contenuto: f64,
        in_corso: bool,
        /// Di quanto la sua scheda va più svelta del mio orologio.
        deriva_naturale_ppm: f64,
        /// La deriva chiesta: positiva consuma più in fretta.
        ppm: i32,
        /// Un silenzio inserito: il contenuto sta fermo fra i due istanti.
        silenzio: Option<(u64, u64)>,
        correzione_fino: u64,
        /// Un avvio programmato: `riprendi_a`.
        avvio: Option<u64>,
        /// Una riapertura: pronta in, etichetta, punto in ns, avvio.
        apertura: Option<(u64, u64, f64, u64)>,
        /// Quanta musica c'è nell'anello, in ns.
        riserva: f64,
    }

    impl MembroFinto {
        /// Un passo di simulazione da `t`.
        fn avanza(&mut self, t: u64) {
            let fine = t + PASSO;
            if let Some((pronta, etichetta, punto, quando)) = self.apertura
                && pronta <= t
            {
                self.apertura = None;
                self.etichetta = Some(etichetta);
                self.contenuto = punto;
                self.in_corso = false;
                self.avvio = Some(quando);
            }
            let mut da = t;
            if let Some(quando) = self.avvio {
                if quando >= fine || self.apertura.is_some() {
                    return;
                }
                if quando < t {
                    // In ritardo: il motore salta la musica che doveva già
                    // essere uscita.
                    self.contenuto += (t - quando) as f64;
                } else {
                    da = quando;
                }
                self.avvio = None;
                self.in_corso = true;
            }
            if !self.in_corso {
                return;
            }
            let mut suona = (fine - da) as f64;
            if let Some((inizio, termine)) = self.silenzio {
                let a = inizio.max(da);
                let b = termine.min(fine);
                if b > a {
                    suona -= (b - a) as f64;
                }
                if termine <= fine {
                    self.silenzio = None;
                }
            }
            let velocita = 1.0 + (self.deriva_naturale_ppm + f64::from(self.ppm)) * 1e-6;
            self.contenuto += suona * velocita;
        }

        #[expect(
            clippy::integer_division,
            reason = "apri_fermo vuole millisecondi interi: il troncamento è \
                      quello che fa chi lo chiama"
        )]
        fn esegui(&mut self, t: u64, mossa: Mossa) {
            match mossa {
                Mossa::Niente => {}
                Mossa::Gradino(n) => {
                    let durata = n.unsigned_abs() as f64 * 1e9 / f64::from(F);
                    if n > 0 {
                        let inizio = t + PORTA_NS;
                        self.silenzio = Some((inizio, inizio + intero_u64(durata)));
                        self.correzione_fino = t + 2 * PORTA_NS + intero_u64(durata);
                    } else {
                        self.contenuto += durata.min(self.riserva);
                        self.correzione_fino = t + 2 * PORTA_NS;
                    }
                }
                Mossa::Deriva(ppm) => self.ppm = ppm,
                Mossa::Pausa => {
                    self.in_corso = false;
                    self.avvio = None;
                }
                // Fermi, al punto: la voce e la posizione, niente avvio.
                Mossa::Posiziona {
                    etichetta,
                    posizione_us,
                } => {
                    self.etichetta = Some(etichetta);
                    self.contenuto = (posizione_us * 1000) as f64;
                    self.in_corso = false;
                    self.avvio = None;
                }
                Mossa::AvviaA { quando_ns, salta } => {
                    // Come dice la documentazione: riprendi_a(quando − salta/f).
                    let prima = intero_u64(salta as f64 * 1e9 / f64::from(F));
                    self.avvio = Some(quando_ns - prima);
                }
                Mossa::Riapri {
                    etichetta,
                    posizione_us,
                    quando_ns,
                } => {
                    // apri_fermo a millisecondi interi, e l'avvio anticipato
                    // del resto, come dice la documentazione.
                    let da_ms = posizione_us / 1000;
                    let resto = (posizione_us - da_ms * 1000) * 1000;
                    self.in_corso = false;
                    self.avvio = None;
                    self.silenzio = None;
                    self.apertura = Some((
                        t + 80 * MS,
                        etichetta,
                        (da_ms * MS) as f64,
                        quando_ns - resto,
                    ));
                }
            }
        }
    }

    /// I due motori, il regolatore, e il tempo che passa.
    struct Banco {
        t: u64,
        ospite: OspiteFinto,
        membro: MembroFinto,
        regolatore: Regolatore,
        caso: Lcg,
        /// Il rumore delle ancore: uniforme entro ± questo, in ns.
        rumore: f64,
        stima: Option<Stima>,
        /// L'ultima ancora dell'ospite: si rinnova ogni mezzo secondo, o
        /// subito se la si mette a `None` (l'ospite la manda a ogni cambio).
        ancora_ospite: Option<DallOspite>,
        /// Ancore mie storte: in quell'istante, tanti ns in più.
        disturbo: Vec<(u64, f64)>,
        mosse: Vec<(u64, Mossa)>,
    }

    impl Banco {
        /// Tutti e due sullo stesso brano, al minuto, il membro spostato di
        /// `errore` ns e con la scheda che deriva di `deriva_ppm`.
        fn nuovo(errore: f64, deriva_ppm: f64) -> Self {
            Self {
                t: 1000 * SECONDO,
                ospite: OspiteFinto {
                    etichetta: 1,
                    contenuto: 60e9,
                    in_corso: true,
                    scarto: 0,
                    programmata: false,
                },
                membro: MembroFinto {
                    etichetta: Some(1),
                    contenuto: 60e9 + errore,
                    in_corso: true,
                    deriva_naturale_ppm: deriva_ppm,
                    ppm: 0,
                    silenzio: None,
                    correzione_fino: 0,
                    avvio: None,
                    apertura: None,
                    riserva: 3e9,
                },
                regolatore: Regolatore::nuovo(Taratura::default()),
                caso: Lcg(2026),
                rumore: 200_000.0,
                stima: None,
                ancora_ospite: None,
                disturbo: Vec::new(),
                mosse: Vec::new(),
            }
        }

        /// L'errore vero: il membro meno l'ospite, adesso.
        fn errore(&self) -> f64 {
            assert_eq!(self.membro.etichetta, Some(self.ospite.etichetta));
            self.membro.contenuto - self.ospite.contenuto
        }

        fn ancora_dell_ospite(&mut self) -> DallOspite {
            let o = &self.ospite;
            let avanti = if o.in_corso { LATENZA as f64 } else { 0.0 };
            DallOspite {
                etichetta: o.etichetta,
                posizione_us: us(o.contenuto + avanti),
                udito_ospite_ns: intero_u64(
                    (self.t + LATENZA + o.scarto) as f64 + self.caso.fino_a(self.rumore),
                ),
                in_corso: o.in_corso,
                programmata: o.programmata,
            }
        }

        fn mia(&mut self) -> Option<DaMe> {
            let m = &self.membro;
            if m.apertura.is_some() {
                return None;
            }
            let in_corso = m.in_corso && m.avvio.is_none();
            let mut ancora = DaMe {
                etichetta: m.etichetta,
                posizione_us: us(m.contenuto),
                udito_ns: 0,
                in_corso,
                in_correzione: self.t < m.correzione_fino,
                riserva_us: us(m.riserva),
            };
            if in_corso {
                if m.silenzio.is_some() {
                    // Senza musica, il motore non pubblica ancore.
                    return None;
                }
                let velocita = 1.0 + (m.deriva_naturale_ppm + f64::from(m.ppm)) * 1e-6;
                let mut punto = m.contenuto + LATENZA as f64 * velocita;
                if let Some((_, storto)) = self.disturbo.iter().find(|(t, _)| *t == self.t) {
                    punto += storto;
                }
                ancora.posizione_us = us(punto);
                ancora.udito_ns =
                    intero_u64((self.t + LATENZA) as f64 + self.caso.fino_a(self.rumore));
            }
            Some(ancora)
        }

        fn regola(&mut self) {
            if self.ancora_ospite.is_none() || self.t.is_multiple_of(500 * MS) {
                self.ancora_ospite = Some(self.ancora_dell_ospite());
            }
            let mia = self.mia();
            let mossa = self.regolatore.passo(
                self.t,
                self.ancora_ospite.as_ref(),
                mia.as_ref(),
                self.stima.as_ref(),
                F,
            );
            if mossa != Mossa::Niente {
                self.mosse.push((self.t, mossa));
            }
            self.membro.esegui(self.t, mossa);
        }

        fn avanti(&mut self, durata: u64) {
            let fine = self.t + durata;
            while self.t < fine {
                if self.ospite.in_corso {
                    self.ospite.contenuto += PASSO as f64;
                }
                self.membro.avanza(self.t);
                self.t += PASSO;
                if self.t.is_multiple_of(OGNI) {
                    self.regola();
                }
            }
        }

        /// L'ospite cambia qualcosa: la sua ancora nuova arriva subito.
        fn cambia_ospite(&mut self, cambio: impl FnOnce(&mut OspiteFinto)) {
            cambio(&mut self.ospite);
            self.ancora_ospite = None;
        }

        fn gradini(&self) -> Vec<i64> {
            self.mosse
                .iter()
                .filter_map(|(_, m)| match m {
                    Mossa::Gradino(n) => Some(*n),
                    _ => None,
                })
                .collect()
        }

        fn derive(&self) -> Vec<i32> {
            self.mosse
                .iter()
                .filter_map(|(_, m)| match m {
                    Mossa::Deriva(p) => Some(*p),
                    _ => None,
                })
                .collect()
        }

        fn riaperture(&self) -> usize {
            self.mosse
                .iter()
                .filter(|(_, m)| matches!(m, Mossa::Riapri { .. }))
                .count()
        }

        fn pause(&self) -> usize {
            self.mosse
                .iter()
                .filter(|(_, m)| matches!(m, Mossa::Pausa))
                .count()
        }
    }

    #[test]
    fn aggancia_entro_5_ms_in_meno_di_un_secondo() {
        for errore_ms in [-450.0, -150.0, -40.0, 25.0, 120.0, 300.0] {
            let mut b = Banco::nuovo(errore_ms * 1e6, 0.0);
            b.avanti(SECONDO);
            let e = b.errore();
            assert!(
                e.abs() < 5e6,
                "da {errore_ms} ms: dopo un secondo {} ms",
                e / 1e6
            );
            // Un gradino solo, col segno del motore: avanti vuol dire
            // silenzio, cioè positivo; ed esatto entro il rumore delle ancore.
            let gradini = b.gradini();
            assert_eq!(gradini.len(), 1, "da {errore_ms} ms: {:?}", b.mosse);
            let atteso = errore_ms * f64::from(F) / 1000.0;
            assert!(
                (gradini[0] as f64 - atteso).abs() < 15.0,
                "da {errore_ms} ms: gradino {} invece di {atteso}",
                gradini[0]
            );
            b.avanti(3 * SECONDO);
            assert!(b.errore().abs() < 1e6, "e poi resta lì");
            assert_eq!(b.gradini().len(), 1);
        }
    }

    #[test]
    fn con_cento_ppm_di_deriva_sta_sotto_mezzo_millisecondo_entro_un_minuto() {
        for deriva in [100.0, -100.0] {
            let mut b = Banco::nuovo(0.0, deriva);
            b.avanti(60 * SECONDO);
            let mut peggiore = 0.0f64;
            for _ in 0..600 {
                b.avanti(100 * MS);
                peggiore = peggiore.max(b.errore().abs());
            }
            assert!(
                peggiore < 0.5e6,
                "{deriva} ppm: fino a {} ms dopo il primo minuto",
                peggiore / 1e6
            );
            assert!(b.gradini().is_empty());
            let d = b.regolatore.diagnosi();
            assert!(
                (d.integrale_ppm + deriva).abs() < 10.0,
                "{deriva} ppm: l'integrale vale {}",
                d.integrale_ppm
            );
        }
    }

    #[test]
    fn nella_zona_morta_il_comando_non_oscilla() {
        let mut b = Banco::nuovo(100_000.0, 0.0);
        b.avanti(120 * SECONDO);
        let derive = b.derive();
        // La prima deriva (zero) e, al più, l'integratore che parte dalla
        // pendenza: poi niente.
        assert!(derive.len() <= 2, "{derive:?}");
        assert!(derive.iter().all(|p| p.abs() <= 3), "{derive:?}");
        assert!(b.gradini().is_empty());
        assert!(b.errore().abs() < 0.5e6);
    }

    #[test]
    fn un_valore_anomalo_isolato_non_provoca_gradini() {
        let mut b = Banco::nuovo(0.0, 0.0);
        b.avanti(5 * SECONDO);
        let t = b.t;
        // Uno da 80 ms, uno da 900 ms, e due vicini ma non di fila.
        b.disturbo = vec![
            (t + 100 * MS, 80e6),
            (t + 2000 * MS, 900e6),
            (t + 3000 * MS, -60e6),
            (t + 3200 * MS, -60e6),
        ];
        b.avanti(5 * SECONDO);
        assert!(b.gradini().is_empty(), "{:?}", b.mosse);
        assert_eq!(b.riaperture(), 0);
        assert!(b.errore().abs() < 0.5e6);
    }

    #[test]
    fn riapre_se_l_ospite_salta_e_riparte_allineato() {
        let mut b = Banco::nuovo(0.0, 0.0);
        b.avanti(5 * SECONDO);
        b.cambia_ospite(|o| o.contenuto += 30e9);
        b.avanti(100 * MS);
        let &(t, mossa) = b.mosse.last().unwrap();
        let Mossa::Riapri {
            etichetta,
            posizione_us,
            quando_ns,
        } = mossa
        else {
            panic!("attesa una riapertura, non {mossa:?}");
        };
        assert_eq!(etichetta, 1);
        assert_eq!(quando_ns, t + 250 * MS);
        // Il punto che l'ospite suonerà allora, entro il rumore della sua
        // ancora.
        let atteso = (b.ospite.contenuto + 250e6) / 1000.0;
        assert!((posizione_us as f64 - atteso).abs() < 250.0);
        b.avanti(1500 * MS);
        assert_eq!(b.riaperture(), 1);
        assert!(b.errore().abs() < 1e6, "{} ms", b.errore() / 1e6);
    }

    #[test]
    fn riapre_se_il_brano_resta_diverso_per_piu_di_un_secondo() {
        let mut b = Banco::nuovo(0.0, 0.0);
        b.avanti(5 * SECONDO);
        b.cambia_ospite(|o| {
            o.etichetta = 2;
            o.contenuto = 0.0;
        });
        b.avanti(1000 * MS);
        assert_eq!(b.riaperture(), 0, "dentro il secondo di grazia no");
        b.avanti(300 * MS);
        assert_eq!(b.riaperture(), 1);
        b.avanti(2 * SECONDO);
        assert_eq!(b.membro.etichetta, Some(2));
        assert!(b.errore().abs() < 1e6);
    }

    #[test]
    fn un_brano_diverso_programmato_dall_ospite_si_riapre_subito() {
        let mut b = Banco::nuovo(0.0, 0.0);
        b.avanti(5 * SECONDO);
        // Un «prossimo»: l'ospite annuncia la voce 2 dall'inizio, in anticipo.
        b.cambia_ospite(|o| {
            o.etichetta = 2;
            o.contenuto = 0.0;
            o.programmata = true;
        });
        b.avanti(100 * MS);
        assert_eq!(b.riaperture(), 1, "niente secondo di grazia: {:?}", b.mosse);
        b.ospite.programmata = false;
        b.avanti(2 * SECONDO);
        assert_eq!(b.membro.etichetta, Some(2));
        assert!(b.errore().abs() < 1e6);
    }

    #[test]
    fn una_mossa_non_riuscita_lascia_il_regolatore_libero() {
        let mut r = Regolatore::nuovo(Taratura::default());
        let ospite = DallOspite {
            etichetta: 3,
            posizione_us: 10_000_000,
            udito_ospite_ns: 1_000 * SECONDO,
            in_corso: true,
            programmata: true,
        };
        let mia = DaMe {
            etichetta: Some(1),
            posizione_us: 0,
            udito_ns: 0,
            in_corso: false,
            in_correzione: false,
            riserva_us: 0,
        };
        let prima = r.passo(1_000 * SECONDO, Some(&ospite), Some(&mia), None, F);
        assert!(
            matches!(prima, Mossa::Riapri { etichetta: 3, .. }),
            "{prima:?}"
        );
        // Senza dirgli niente, aspetterebbe la scadenza della riapertura.
        assert_eq!(
            r.passo(
                1_000 * SECONDO + 100 * MS,
                Some(&ospite),
                Some(&mia),
                None,
                F
            ),
            Mossa::Niente
        );
        r.mossa_non_riuscita();
        let dopo = r.passo(
            1_000 * SECONDO + 200 * MS,
            Some(&ospite),
            Some(&mia),
            None,
            F,
        );
        assert!(
            matches!(dopo, Mossa::Riapri { etichetta: 3, .. }),
            "{dopo:?}"
        );
    }

    #[test]
    fn fermi_tutti_e_due_si_va_dove_l_ospite_ha_saltato_una_volta_sola() {
        let mut r = Regolatore::nuovo(Taratura::default());
        let ospite = |posizione_us| DallOspite {
            etichetta: 4,
            posizione_us,
            udito_ospite_ns: 1_000 * SECONDO,
            in_corso: false,
            programmata: false,
        };
        let mia = |posizione_us| DaMe {
            etichetta: Some(4),
            posizione_us,
            udito_ns: 0,
            in_corso: false,
            in_correzione: false,
            riserva_us: 0,
        };
        let t = 1_000 * SECONDO;
        // Allo stesso punto: niente.
        assert_eq!(
            r.passo(
                t,
                Some(&ospite(60_000_000)),
                Some(&mia(60_000_000)),
                None,
                F
            ),
            Mossa::Niente
        );
        // L'ospite salta a 2:00 mentre è fermo: ci si mette lì, fermi, una volta.
        assert_eq!(
            r.passo(
                t,
                Some(&ospite(120_000_000)),
                Some(&mia(60_000_000)),
                None,
                F
            ),
            Mossa::Posiziona {
                etichetta: 4,
                posizione_us: 120_000_000
            }
        );
        assert_eq!(
            r.passo(
                t + 100 * MS,
                Some(&ospite(120_000_000)),
                Some(&mia(60_000_000)),
                None,
                F
            ),
            Mossa::Niente
        );
        // Senza una voce in mano, anche.
        assert_eq!(
            r.passo(t + 200 * MS, Some(&ospite(30_000_000)), None, None, F),
            Mossa::Posiziona {
                etichetta: 4,
                posizione_us: 30_000_000
            }
        );
        // Arrivati: niente più.
        assert_eq!(
            r.passo(
                t + 300 * MS,
                Some(&ospite(30_000_000)),
                Some(&mia(30_000_000)),
                None,
                F
            ),
            Mossa::Niente
        );
    }

    #[test]
    fn riapre_se_l_errore_supera_mezzo_secondo() {
        let mut b = Banco::nuovo(2e9, 0.0);
        b.avanti(300 * MS);
        assert!(matches!(b.mosse.as_slice(), [(_, Mossa::Riapri { .. })]));
        b.avanti(2 * SECONDO);
        assert!(b.errore().abs() < 1e6);
        assert!(b.gradini().is_empty());
    }

    #[test]
    fn riapre_invece_di_saltare_se_l_anello_non_basta() {
        let mut b = Banco::nuovo(-300e6, 0.0);
        b.membro.riserva = 100e6;
        b.avanti(300 * MS);
        assert!(matches!(b.mosse.as_slice(), [(_, Mossa::Riapri { .. })]));
        assert!(b.gradini().is_empty());
        b.avanti(2 * SECONDO);
        assert!(b.errore().abs() < 1e6);
    }

    #[test]
    fn segue_l_ospite_in_pausa_e_riparte_allineato() {
        let mut b = Banco::nuovo(0.0, 0.0);
        b.avanti(5 * SECONDO);
        b.cambia_ospite(|o| o.in_corso = false);
        b.avanti(200 * MS);
        assert_eq!(b.pause(), 1);
        assert!(!b.membro.in_corso);
        b.avanti(10 * SECONDO);
        assert_eq!(b.pause(), 1, "una volta sola");

        b.cambia_ospite(|o| o.in_corso = true);
        b.avanti(100 * MS);
        let &(t, mossa) = b.mosse.last().unwrap();
        let Mossa::AvviaA { quando_ns, salta } = mossa else {
            panic!("atteso un avvio, non {mossa:?}");
        };
        assert!(quando_ns >= t + 250 * MS);
        assert!(salta >= 0);
        b.avanti(SECONDO);
        assert!(b.membro.in_corso);
        assert!(b.errore().abs() < 0.5e6, "{} ms", b.errore() / 1e6);
        assert!(b.gradini().is_empty());
    }

    #[test]
    fn da_fermo_e_indietro_riparte_saltando_da_fermo_e_avanti_aspetta() {
        // Indietro di 50 ms mentre l'ospite suona: quando il regolatore
        // decide, l'ospite è già 100 ms più in là, e l'avvio è fra 250.
        let mut b = Banco::nuovo(-50e6, 0.0);
        b.membro.in_corso = false;
        b.avanti(100 * MS);
        let &(t, mossa) = b.mosse.last().unwrap();
        let Mossa::AvviaA { quando_ns, salta } = mossa else {
            panic!("atteso un avvio, non {mossa:?}");
        };
        assert_eq!(quando_ns, t + 250 * MS);
        assert!((salta - 400 * 48).abs() <= 15, "salta {salta}");
        b.avanti(SECONDO);
        assert!(b.errore().abs() < 0.5e6, "{} ms", b.errore() / 1e6);

        // Avanti di 400 ms: l'ospite ci arriva dopo i 100 ms del primo passo
        // e i 250 dell'anticipo, quindi l'avvio slitta di altri 50.
        let mut b = Banco::nuovo(400e6, 0.0);
        b.membro.in_corso = false;
        b.avanti(100 * MS);
        let &(t, mossa) = b.mosse.last().unwrap();
        let Mossa::AvviaA { quando_ns, salta } = mossa else {
            panic!("atteso un avvio, non {mossa:?}");
        };
        assert_eq!(salta, 0);
        assert!((quando_ns as f64 - (t + 300 * MS) as f64).abs() < 0.3e6);
        b.avanti(SECONDO);
        assert!(b.errore().abs() < 0.5e6, "{} ms", b.errore() / 1e6);
    }

    #[test]
    fn l_integratore_resta_in_pausa_e_azzera_lo_cancella() {
        let mut b = Banco::nuovo(0.0, 100.0);
        b.avanti(40 * SECONDO);
        let prima = b.regolatore.diagnosi();
        assert!(prima.integratore_pronto);
        assert!((prima.integrale_ppm + 100.0).abs() < 15.0, "{prima:?}");

        b.cambia_ospite(|o| o.in_corso = false);
        b.avanti(20 * SECONDO);
        assert_eq!(b.regolatore.diagnosi().integrale_ppm, prima.integrale_ppm);

        b.cambia_ospite(|o| o.in_corso = true);
        b.avanti(2 * SECONDO);
        let dopo = b.regolatore.diagnosi();
        assert!(dopo.integratore_pronto);
        assert!((dopo.integrale_ppm - prima.integrale_ppm).abs() < 1.0);
        assert!(b.errore().abs() < 0.5e6);

        b.regolatore.azzera();
        let zero = b.regolatore.diagnosi();
        assert_eq!(zero.integrale_ppm, 0.0);
        assert!(!zero.integratore_pronto);
        assert_eq!(zero.ppm, 0);
        assert_eq!(zero.errore_ns, None);
    }

    #[test]
    fn con_l_orologio_di_un_altro_pc_converte_le_ancore() {
        let mut b = Banco::nuovo(-80e6, 0.0);
        b.ospite.scarto = 7 * SECONDO;
        b.stima = Some(Stima {
            riferimento_locale: 0,
            scarto_ns: 7_000_000_000,
            deriva_ppb: 0,
            incertezza_ns: 100_000,
            punti: 30,
        });
        b.avanti(SECONDO);
        assert!(b.errore().abs() < 5e6);
        assert_eq!(b.gradini().len(), 1);
        assert_eq!(b.riaperture(), 0);
    }

    #[test]
    fn senza_ancore_o_senza_un_orologio_valido_non_fa_niente() {
        let mut r = Regolatore::nuovo(Taratura::default());
        assert_eq!(r.passo(SECONDO, None, None, None, F), Mossa::Niente);
        let ospite = DallOspite {
            etichetta: 1,
            posizione_us: 0,
            udito_ospite_ns: SECONDO,
            in_corso: true,
            programmata: false,
        };
        assert_eq!(
            r.passo(SECONDO, Some(&ospite), None, None, 0),
            Mossa::Niente
        );
        // Una stima che non sa portare l'istante dell'ospite da me.
        let assurda = Stima {
            riferimento_locale: 0,
            scarto_ns: i64::MAX,
            deriva_ppb: 0,
            incertezza_ns: 0,
            punti: 9,
        };
        assert_eq!(
            r.passo(SECONDO, Some(&ospite), None, Some(&assurda), F),
            Mossa::Niente
        );
    }
}
