//! Le animazioni nominate: dichiarate una volta, richiamate dalle parti.
//!
//! Un'animazione ha un nome, sta scritta una volta in `motion.animations`, e le
//! parti la richiamano per innesco — come si fa per i motivi (`patterns`) e per
//! le curve (`motion.easings`). Ripeterla per esteso su ogni parte sarebbe lo
//! stesso valore scritto in venti posti, che diverge alla prima modifica.
//!
//! # Ogni durata passa da `calc(… * var(--motion-scale, 1))`
//!
//! `prefers-reduced-motion` nell'app azzera `--motion-scale`, e ogni durata del
//! foglio è un `calc()` che passa di lì. Il compilatore non ha un ramo in cui
//! emette una durata nuda, e la prova che lo verifica guarda l'output.
//!
//! # I cicli infiniti, e perché nel formato 2 si possono
//!
//! Il formato 1 li vietava con una ragione vera: una scala a zero non ferma un
//! ciclo infinito, lo rende infinitamente lento, e la regola che lo spegne —
//! `animation: none` sotto `prefers-reduced-motion` — **una skin non la poteva
//! scrivere**. Era vero perché nessuno la scriveva per lei.
//!
//! Il compilatore è l'unico autore del CSS, quindi quella regola ora la scrive
//! lui: per ogni parte animata, `animation: none !important` sotto la
//! preferenza di sistema, sotto quella chiesta da Rymembra e sotto una skin a
//! intensità `none`, sempre, senza che la skin possa ometterla. Un ciclo
//! dichiarato da una skin si ferma esattamente come le quattro animazioni
//! infinite del motore. La prova che ogni parte animata abbia la sua regola di
//! arresto sta in [`crate::compile`].
//!
//! Resta un confine: il ciclo infinito è ammesso **solo sugli inneschi di
//! stato**. Un'entrata che non finisce non è un'entrata, e un'uscita che non
//! finisce tiene l'elemento montato per sempre.
//!
//! # «In riproduzione» vuol dire «finché suona»
//!
//! Un'animazione sull'innesco `playing` non riparte a ogni pausa: si ferma dov'è
//! e riprende da lì, come un disco che smette di girare. Il compilatore la mette
//! nella regola base della parte con `animation-play-state` legato a una
//! variabile, e lo stato `playing` accende la variabile. È l'unico innesco che
//! si comporta così, ed è la ragione per cui ha un nome suo invece di essere
//! uno stato come gli altri.
//!
//! # Le entrate, le uscite, le condivise
//!
//! `enter` sulle parti che non sono presenze è la regola base, e parte quando
//! l'elemento entra nel documento. Sulle **presenze** — menù, modali,
//! notifiche, la colonna, il contenuto della pagina — entrata, uscita e volo
//! condiviso sono transizioni di vista: il renderer avvolge quelle parti in
//! `<ViewTransition>` e tiene montato quel che esce finché l'animazione
//! finisce, cosa che il foglio da solo non può fare.

use crate::effects::CostClass;
use crate::parts::Ombra;
use crate::stati::PartState;
use crate::tokens::ColorValue;
use crate::values::{Duration, Easing, EasingKeyword, Length};

// ── I tetti ─────────────────────────────────────────────────────────────────

/// Quante animazioni un documento può dichiarare.
///
/// Non è più un limite di leggibilità — lo Studio le elenca con una ricerca —
/// ma una diga: un manifest che ne dichiara diecimila è un attacco, non una
/// skin.
pub const MAX_ANIMAZIONI: usize = 64;

/// Quanti fotogrammi ha un'animazione, al minimo.
///
/// Due: con uno solo non c'è interpolazione, c'è uno stato — e uno stato si
/// scrive in `states`, dove costa una transizione invece di un'animazione.
pub const MIN_FOTOGRAMMI: usize = 2;

/// Quanti fotogrammi ha un'animazione, al massimo.
pub const MAX_FOTOGRAMMI: usize = 12;

/// Quanto può durare un'ripetizione, in millisecondi.
///
/// Dieci secondi: un giro lento di vinile ne chiede due, un respiro sei.
pub const MAX_DURATA_MS: f64 = 10_000.0;

/// Quanto può attendere prima di partire, in millisecondi.
pub const MAX_RITARDO_MS: f64 = 5_000.0;

/// Quante volte può ripetersi, quando non è infinita.
pub const MAX_ITERAZIONI: u32 = 16;

/// Il passo massimo della cascata, in millisecondi.
pub const MAX_CASCATA_MS: f64 = 400.0;

/// Quante voci di un elenco partono in cascata; dalla tredicesima in poi
/// partono insieme all'ultima.
///
/// Senza tetto, la duecentesima scheda di una pagina aspetterebbe duecento
/// passi prima di comparire: un elenco che si svela per un minuto non è
/// un'animazione, è un elenco rotto.
pub const CASCATA_VOCI: u32 = 12;

// ── Il costo ────────────────────────────────────────────────────────────────

/// Budget del movimento di una singola parte.
///
/// Sulla stessa scala di [`CostClass`]. Sedici è quattro animazioni composite
/// suonate una volta — entrare, reagire al puntatore, alla pressione, al
/// fuoco — oppure tre e un ciclo. Sforarlo è un avviso, non un errore.
pub const MOTION_COST_BUDGET: u32 = 16;

/// Budget del movimento dell'intera finestra: la somma su tutte le parti.
///
/// È la domanda che il budget per parte non sa fare: sei superfici che si
/// muovono ognuna nel suo budget compongono comunque una finestra irrequieta.
pub const WINDOW_MOTION_BUDGET: u32 = 96;

/// Quanto aggiunge un ciclo infinito al costo di un'animazione.
///
/// Un ciclo tiene il livello composto in vita per sempre, e il costo di «per
/// sempre» non può essere il numero delle ripetizioni. Vale quanto un'altra
/// animazione composita: il vinile che gira costa quanto un'entrata più un
/// passaggio del mouse.
pub const COSTO_CICLO: u32 = 4;

/// Quanto costa un'animazione, per la parte che la porta.
///
/// Si parte dal livello composto che ogni animazione chiede, e si aggiunge
/// quel che le proprietà animate costano in più: i filtri campionano i pixel
/// attorno, i colori e le ombre ridisegnano l'elemento a ogni fotogramma, un
/// ritaglio ricalcola la forma. Le ripetizioni oltre la prima valgono uno
/// ciascuna, un ciclo [`COSTO_CICLO`].
///
/// **I fotogrammi non contano, ed è una scelta**: il motore interpola fra due
/// fermate esattamente come fra dodici.
#[must_use]
pub fn animation_cost(animation: &Animation) -> u32 {
    let mut costo = CostClass::Composited.weight();
    let tocca = |pred: fn(&AnimFrame) -> bool| animation.frames.iter().any(pred);
    if tocca(AnimFrame::ha_filtri) {
        costo += 2;
    }
    if tocca(AnimFrame::ha_pittura) {
        costo += CostClass::Composited.weight();
    }
    if tocca(|f| f.clip.is_some()) {
        costo += CostClass::Paint.weight();
    }
    costo
        + match animation.iterations {
            Iterazioni::Volte(n) => n.saturating_sub(1),
            Iterazioni::Infinite => COSTO_CICLO,
        }
}

// ── La forma ────────────────────────────────────────────────────────────────

/// Un ritaglio animabile: come si scopre la parte.
#[derive(Debug, Clone, PartialEq)]
pub enum Ritaglio {
    /// Un rettangolo rientrato dai quattro lati: una tendina.
    Inset {
        /// Dall'alto.
        top: Length,
        /// Da destra.
        right: Length,
        /// Dal basso.
        bottom: Length,
        /// Da sinistra.
        left: Length,
    },
    /// Un cerchio che si allarga.
    Circle {
        /// Il raggio.
        radius: Length,
        /// Il centro.
        at: (Length, Length),
    },
    /// Un poligono da tre a otto punti, in percentuale della parte.
    Polygon(Vec<(Length, Length)>),
}

/// Un fotogramma di un'animazione nominata.
///
/// Ogni campo è facoltativo: un fotogramma dice soltanto quel che cambia.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimFrame {
    /// Dove sta, in percentuale del percorso: da 0 a 100.
    pub at: f64,
    /// Opacità.
    pub opacity: Option<f64>,
    /// Spostamento orizzontale, in px o in % della parte.
    pub translate_x: Option<Length>,
    /// Spostamento verticale, in px o in % della parte.
    pub translate_y: Option<Length>,
    /// Scala sui due assi.
    pub scale: Option<f64>,
    /// Scala orizzontale.
    pub scale_x: Option<f64>,
    /// Scala verticale.
    pub scale_y: Option<f64>,
    /// Rotazione, in gradi.
    pub rotate: Option<f64>,
    /// Inclinazione orizzontale, in gradi.
    pub skew_x: Option<f64>,
    /// Inclinazione verticale, in gradi.
    pub skew_y: Option<f64>,
    /// Sfocatura.
    pub blur: Option<Length>,
    /// Luminosità: 1 invariata.
    pub brightness: Option<f64>,
    /// Saturazione: 1 invariata.
    pub saturate: Option<f64>,
    /// Rotazione della tinta, in gradi.
    pub hue_rotate: Option<f64>,
    /// Colore di fondo.
    pub background_color: Option<ColorValue>,
    /// Colore del testo.
    pub color: Option<ColorValue>,
    /// Colore del bordo.
    pub border_color: Option<ColorValue>,
    /// Ombra.
    pub shadow: Option<Ombra>,
    /// Ritaglio.
    pub clip: Option<Ritaglio>,
}

impl AnimFrame {
    /// Non dichiara niente: sarebbe una fermata vuota in mezzo al percorso.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        Self {
            at: self.at,
            ..Self::default()
        } == *self
    }

    /// Anima un filtro.
    #[must_use]
    pub const fn ha_filtri(&self) -> bool {
        self.blur.is_some()
            || self.brightness.is_some()
            || self.saturate.is_some()
            || self.hue_rotate.is_some()
    }

    /// Anima qualcosa che si ridisegna: un colore o un'ombra.
    #[must_use]
    pub const fn ha_pittura(&self) -> bool {
        self.background_color.is_some()
            || self.color.is_some()
            || self.border_color.is_some()
            || self.shadow.is_some()
    }

    /// I colori che il fotogramma dichiara.
    #[must_use]
    pub fn colori(&self) -> Vec<&ColorValue> {
        let mut colori: Vec<&ColorValue> = [
            self.background_color.as_ref(),
            self.color.as_ref(),
            self.border_color.as_ref(),
        ]
        .into_iter()
        .flatten()
        .collect();
        if let Some(Ombra::Valore(ombra)) = self.shadow.as_ref() {
            colori.extend(ombra.layers.iter().map(|l| &l.color));
        }
        colori
    }
}

/// Quante volte si ripete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Iterazioni {
    /// Un numero di volte, da 1 a [`MAX_ITERAZIONI`].
    Volte(u32),
    /// Finché lo stato dura. Solo sugli inneschi di stato.
    Infinite,
}

impl Iterazioni {
    /// Come si scrive in CSS.
    #[must_use]
    pub fn css(self) -> String {
        match self {
            Self::Volte(n) => n.to_string(),
            Self::Infinite => "infinite".to_owned(),
        }
    }
}

/// In che verso scorre un'animazione.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimDirection {
    /// Sempre dal primo fotogramma all'ultimo.
    #[default]
    Normal,
    /// Sempre dall'ultimo al primo.
    Reverse,
    /// Andata e ritorno: la ripetizione pari torna indietro.
    Alternate,
    /// Come [`Self::Alternate`], ma comincia dal ritorno.
    AlternateReverse,
}

impl AnimDirection {
    /// Come si scrive in CSS. È anche il nome nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Reverse => "reverse",
            Self::Alternate => "alternate",
            Self::AlternateReverse => "alternate-reverse",
        }
    }

    /// Tutti, nell'ordine in cui si mostrano.
    pub const ALL: &'static [Self] = &[
        Self::Normal,
        Self::Reverse,
        Self::Alternate,
        Self::AlternateReverse,
    ];

    /// Dal nome scritto nel documento.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|d| d.as_str() == raw)
    }
}

/// Quel che l'animazione lascia prima di partire e dopo aver finito.
///
/// Il formato 1 scriveva sempre `both` e non lo lasciava dichiarare, con una
/// ragione che resta vera per le entrate: un'animazione che non tiene lo stato
/// finale fa saltare la parte al valore di partenza nell'istante in cui
/// finisce. Un ciclo invece vuole spesso `none` — il pulsare che si ferma deve
/// tornare com'era — e per quello il campo esiste. Il valore di serie resta
/// `both`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Riempimento {
    /// Tiene il primo fotogramma prima e l'ultimo dopo.
    #[default]
    Both,
    /// Tiene l'ultimo fotogramma dopo.
    Forwards,
    /// Tiene il primo fotogramma prima.
    Backwards,
    /// Non tiene niente.
    None,
}

impl Riempimento {
    /// Come si scrive in CSS. È anche il nome nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Forwards => "forwards",
            Self::Backwards => "backwards",
            Self::None => "none",
        }
    }

    /// Tutti.
    pub const ALL: &'static [Self] = &[Self::Both, Self::Forwards, Self::Backwards, Self::None];
}

/// Una curva: scritta per esteso, o richiamata per nome da `motion.easings`.
///
/// Il richiamo per nome chiude il vicolo cieco del formato 1: le curve della
/// skin si compilavano in `--skin-ease-<nome>`, e nessun campo del documento
/// le poteva usare.
#[derive(Debug, Clone, PartialEq)]
pub enum Curva {
    /// Scritta per esteso, o una parola chiave del CSS.
    Valore(Easing),
    /// Una curva dichiarata in `motion.easings`.
    Nome(String),
}

impl Default for Curva {
    fn default() -> Self {
        Self::Valore(Easing::Keyword(EasingKeyword::Ease))
    }
}

/// Un'animazione dichiarata in `motion.animations`.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    /// Quanto dura una ripetizione. Al massimo [`MAX_DURATA_MS`].
    pub duration: Duration,
    /// La curva. Assente nel documento: `ease`, cioè il default del CSS.
    pub easing: Curva,
    /// Quanto attende prima di partire.
    pub delay: Option<Duration>,
    /// Quante volte si ripete.
    pub iterations: Iterazioni,
    /// In che verso.
    pub direction: AnimDirection,
    /// Quel che tiene prima e dopo.
    pub fill: Riempimento,
    /// Il punto attorno a cui ruota e da cui scala, se non il centro.
    pub origin: Option<(Length, Length)>,
    /// Le fermate, in ordine di [`AnimFrame::at`] crescente, la prima a 0.
    pub frames: Vec<AnimFrame>,
}

impl Animation {
    /// Quanto costa, alla parte che la porta.
    #[must_use]
    pub fn cost(&self) -> u32 {
        animation_cost(self)
    }
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            duration: Duration { ms: 0.0 },
            easing: Curva::default(),
            delay: None,
            iterations: Iterazioni::Volte(1),
            direction: AnimDirection::Normal,
            fill: Riempimento::Both,
            origin: None,
            frames: Vec::new(),
        }
    }
}

/// Che cosa fa partire un'animazione.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimTrigger {
    /// Quando la parte entra: nel documento, o — per le presenze — in scena.
    Enter,
    /// Quando una presenza se ne va. Solo sulle presenze.
    Exit,
    /// Quando una parte condivisa vola da una schermata all'altra.
    Share,
    /// Quando la parte entra nella vista mentre si scorre.
    InView,
    /// Quando il contenuto della parte cambia: la copertina di un brano nuovo.
    Change,
    /// Uno degli stati del DOM. `playing` è «finché suona»: vedi il `//!`.
    State(PartState),
}

impl AnimTrigger {
    /// Come si scrive nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enter => "enter",
            Self::Exit => "exit",
            Self::Share => "share",
            Self::InView => "inView",
            Self::Change => "change",
            Self::State(stato) => stato.as_str(),
        }
    }

    /// Tutti, nell'ordine in cui il compilatore li considera.
    pub const ALL: &'static [Self] = &[
        Self::Enter,
        Self::Exit,
        Self::Share,
        Self::InView,
        Self::Change,
        Self::State(PartState::Hover),
        Self::State(PartState::Pressed),
        Self::State(PartState::Selected),
        Self::State(PartState::Current),
        Self::State(PartState::Open),
        Self::State(PartState::Checked),
        Self::State(PartState::Playing),
        Self::State(PartState::Paused),
        Self::State(PartState::Focus),
        Self::State(PartState::Disabled),
    ];

    /// Dal nome scritto nel documento.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == raw)
    }

    /// I nomi ammessi, per il messaggio di un innesco sconosciuto.
    #[must_use]
    pub fn nomi() -> Vec<&'static str> {
        Self::ALL.iter().map(|t| t.as_str()).collect()
    }

    /// Ammette un ciclo infinito.
    ///
    /// Solo gli stati, e fra gli stati tutti: un ciclo dura quanto lo stato
    /// che lo accende, e uno stato finisce.
    #[must_use]
    pub const fn ammette_ciclo(self) -> bool {
        matches!(self, Self::State(_))
    }

    /// Si realizza come transizione di vista, e vale solo sulle presenze o
    /// sulle parti condivise.
    #[must_use]
    pub const fn di_vista(self) -> bool {
        matches!(self, Self::Exit | Self::Share)
    }
}

/// Un'animazione richiamata da una parte: il nome, e la definizione risolta.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationRef {
    /// Il nome dichiarato in `motion.animations`.
    pub name: String,
    /// La definizione, copiata qui al momento di validare.
    pub def: Animation,
}

/// Come una parte passa da un aspetto all'altro quando cambia stato.
///
/// Non è un'animazione: è la durata e la curva delle **transizioni** che il
/// componente dichiara già — il colore che sfuma al passaggio del mouse. Una
/// skin lenta e morbida e una secca e immediata sono due skin diverse anche
/// con gli stessi colori.
#[derive(Debug, Clone, PartialEq)]
pub struct Transizione {
    /// Quanto dura.
    pub duration: Duration,
    /// Con che curva.
    pub easing: Curva,
}

/// Il movimento di una parte: le animazioni per innesco, la cascata, la
/// transizione.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartMotion {
    voci: Vec<(AnimTrigger, AnimationRef)>,
    /// Il passo della cascata: ogni voce di un elenco parte tanto dopo la
    /// precedente.
    pub stagger: Option<Duration>,
    /// La transizione fra gli stati.
    pub transition: Option<Transizione>,
}

impl PartMotion {
    /// L'animazione di un innesco, se assegnata.
    #[must_use]
    pub fn get(&self, trigger: AnimTrigger) -> Option<&AnimationRef> {
        self.voci
            .iter()
            .find(|(t, _)| *t == trigger)
            .map(|(_, a)| a)
    }

    /// Assegna un innesco. Sostituisce quel che c'era.
    pub fn set(&mut self, trigger: AnimTrigger, riferimento: AnimationRef) {
        self.voci.retain(|(t, _)| *t != trigger);
        self.voci.push((trigger, riferimento));
        let posto = |t: &AnimTrigger| AnimTrigger::ALL.iter().position(|x| x == t);
        self.voci.sort_by_key(|(t, _)| posto(t));
    }

    /// Nessuna animazione, nessuna cascata, nessuna transizione.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.voci.is_empty() && self.stagger.is_none() && self.transition.is_none()
    }

    /// Nessuna animazione assegnata.
    #[must_use]
    pub fn senza_animazioni(&self) -> bool {
        self.voci.is_empty()
    }

    /// Gli inneschi assegnati, nell'ordine di [`AnimTrigger::ALL`].
    #[must_use]
    pub fn assegnate(&self) -> Vec<(AnimTrigger, &AnimationRef)> {
        self.voci.iter().map(|(t, a)| (*t, a)).collect()
    }

    /// I nomi richiamati, per sapere quali animazioni sono usate.
    #[must_use]
    pub fn nomi_usati(&self) -> Vec<&str> {
        self.voci.iter().map(|(_, a)| a.name.as_str()).collect()
    }

    /// Quanto costa il movimento di questa parte.
    ///
    /// **Solo le animazioni assegnate.** Una dichiarata e mai richiamata costa
    /// zero: si compila in un `@keyframes` che nessuno usa.
    #[must_use]
    pub fn costo(&self) -> u32 {
        self.voci.iter().map(|(_, a)| animation_cost(&a.def)).sum()
    }

    /// Quanto di questo movimento può essere in corso **nello stesso
    /// momento**, che è la domanda del budget della finestra.
    ///
    /// Una parte o entra o esce: le due animazioni non girano mai insieme, e
    /// sommarle direbbe che un menù che si apre e si chiude pesa il doppio di
    /// uno che si apre soltanto. Conta la più cara delle due.
    #[must_use]
    pub fn costo_insieme(&self) -> u32 {
        let di = |trigger| self.get(trigger).map(|a| animation_cost(&a.def));
        let alternate = match (di(AnimTrigger::Enter), di(AnimTrigger::Exit)) {
            (Some(entrata), Some(uscita)) => entrata.min(uscita),
            _ => 0,
        };
        self.costo() - alternate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn animazione(iterations: Iterazioni) -> Animation {
        Animation {
            duration: Duration { ms: 240.0 },
            iterations,
            frames: vec![
                AnimFrame {
                    at: 0.0,
                    opacity: Some(0.0),
                    ..AnimFrame::default()
                },
                AnimFrame {
                    at: 100.0,
                    opacity: Some(1.0),
                    ..AnimFrame::default()
                },
            ],
            ..Animation::default()
        }
    }

    #[test]
    fn quattro_animazioni_semplici_stanno_esattamente_nel_budget() {
        let una = animazione(Iterazioni::Volte(1));
        assert_eq!(una.cost(), 4);
        assert_eq!(una.cost() * 4, MOTION_COST_BUDGET);
    }

    #[test]
    fn un_ciclo_costa_quanto_un_altra_animazione_e_non_quanto_sedici() {
        let giro = animazione(Iterazioni::Infinite);
        assert_eq!(giro.cost(), 4 + COSTO_CICLO);
        // Il vinile e tre stati stanno nel budget; due cicli e due stati no.
        assert!(giro.cost() + 4 * 2 <= MOTION_COST_BUDGET);
    }

    #[test]
    fn colori_e_ritagli_costano_piu_della_trasformazione() {
        let mut tinta = animazione(Iterazioni::Volte(1));
        tinta.frames[1].background_color = Some(ColorValue::Literal(crate::values::Rgba {
            r: 1,
            g: 2,
            b: 3,
            a: 1.0,
        }));
        assert_eq!(tinta.cost(), 8);
        let mut tendina = animazione(Iterazioni::Volte(1));
        tendina.frames[0].clip = Some(Ritaglio::Circle {
            radius: Length::px(0.0),
            at: (Length::px(0.0), Length::px(0.0)),
        });
        assert_eq!(tendina.cost(), 7);
    }

    #[test]
    fn entrata_e_uscita_pesano_sulla_finestra_una_volta_sola() {
        let riferimento = |def: Animation| AnimationRef {
            name: "x".to_owned(),
            def,
        };
        let mut motion = PartMotion::default();
        motion.set(
            AnimTrigger::Enter,
            riferimento(animazione(Iterazioni::Volte(1))),
        );
        motion.set(
            AnimTrigger::Exit,
            riferimento(animazione(Iterazioni::Volte(3))),
        );
        motion.set(
            AnimTrigger::State(PartState::Hover),
            riferimento(animazione(Iterazioni::Volte(1))),
        );
        assert_eq!(motion.costo(), 4 + 6 + 4);
        // Resta la più cara fra entrata e uscita, e lo stato.
        assert_eq!(motion.costo_insieme(), 6 + 4);
    }

    #[test]
    fn i_fotogrammi_non_entrano_nel_costo() {
        let mut lunga = animazione(Iterazioni::Volte(1));
        lunga.frames = (0..MAX_FOTOGRAMMI)
            .map(|indice| AnimFrame {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "dodici fotogrammi al massimo: nessun indice arriva dove un f64 perde cifre"
                )]
                at: indice as f64 * 9.0,
                opacity: Some(1.0),
                ..AnimFrame::default()
            })
            .collect();
        assert_eq!(lunga.cost(), animazione(Iterazioni::Volte(1)).cost());
    }

    #[test]
    fn solo_gli_stati_ammettono_un_ciclo() {
        assert!(AnimTrigger::State(PartState::Playing).ammette_ciclo());
        assert!(AnimTrigger::State(PartState::Hover).ammette_ciclo());
        for trigger in [
            AnimTrigger::Enter,
            AnimTrigger::Exit,
            AnimTrigger::Share,
            AnimTrigger::InView,
            AnimTrigger::Change,
        ] {
            assert!(!trigger.ammette_ciclo(), "{}", trigger.as_str());
        }
    }

    #[test]
    fn gli_inneschi_sono_cinque_piu_gli_stati() {
        assert_eq!(AnimTrigger::ALL.len(), PartState::ALL.len() + 5);
        assert_eq!(AnimTrigger::parse("exit"), Some(AnimTrigger::Exit));
        assert_eq!(AnimTrigger::parse("inView"), Some(AnimTrigger::InView));
        assert_eq!(AnimTrigger::parse("active"), None);
    }

    #[test]
    fn l_ordine_degli_inneschi_non_dipende_da_come_sono_stati_scritti() {
        let mut prima = PartMotion::default();
        prima.set(
            AnimTrigger::State(PartState::Focus),
            AnimationRef {
                name: "b".to_owned(),
                def: animazione(Iterazioni::Volte(1)),
            },
        );
        prima.set(
            AnimTrigger::Enter,
            AnimationRef {
                name: "a".to_owned(),
                def: animazione(Iterazioni::Volte(1)),
            },
        );
        assert_eq!(prima.nomi_usati(), ["a", "b"]);
        assert!(!prima.is_empty());
        assert!(PartMotion::default().is_empty());
    }

    #[test]
    fn un_fotogramma_vuoto_e_vuoto_anche_se_ha_un_posto() {
        let vuoto = AnimFrame {
            at: 50.0,
            ..AnimFrame::default()
        };
        assert!(vuoto.is_empty());
        let pieno = AnimFrame {
            at: 50.0,
            rotate: Some(180.0),
            ..AnimFrame::default()
        };
        assert!(!pieno.is_empty());
    }
}
