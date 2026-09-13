//! Le animazioni nominate: dichiarate una volta, richiamate dalle parti.
//!
//! Il formato aveva già le durate, le curve e la transizione di rotta, e non
//! aveva un modo di dire «questo pezzo si muove così». Le tre cose che c'erano
//! sono valori; quella che mancava è un **legame**: un'animazione ha un nome,
//! sta scritta una volta in cima al documento, e le parti la richiamano — come
//! si fa già per i motivi (`patterns`) e per le curve (`motion.easings`).
//! Ripeterla per esteso su ogni parte sarebbe la stessa cosa che il vecchio
//! albero faceva coi gradienti: lo stesso valore scritto in venti posti, che
//! diverge alla prima modifica.
//!
//! # Ogni durata passa da `calc(… * var(--motion-scale, 1))`
//!
//! Non è uno stile di scrittura, è l'unico modo che questa animazione ha di
//! fermarsi. `prefers-reduced-motion` nell'app non spegne le animazioni una per
//! una: azzera `--motion-scale` su `:root` e su `[data-motion]`, e ogni durata
//! del foglio è un `calc()` che passa di lì. Una `animation-duration` letterale
//! **aggira** quell'interruttore, e una skin non può scrivere la media query
//! che la rimedierebbe. Il compilatore quindi non ha un ramo in cui emette una
//! durata nuda, e la prova che lo verifica guarda l'output e non l'intenzione.
//!
//! # `iterations: infinite` è vietato, e non è una precauzione
//!
//! Le quattro animazioni che nell'app ripetono all'infinito —
//! `aggiornamento-ignoto`, `scorri`, `scintilla`, `lettura-scorre` — sono
//! fermate da altrettante regole `prefers-reduced-motion` scritte **a mano**,
//! una per una, in `stile.css`: una scala a zero non ferma un ciclo infinito,
//! lo rende infinitamente lento, che è la stessa banda parcheggiata a metà
//! barra. Quelle regole le può scrivere solo il foglio del motore, mai una
//! skin. Un'animazione infinita dichiarata da una skin sarebbe quindi **l'unica
//! cosa nell'applicazione che chi ha chiesto meno movimento non può fermare** —
//! e il divieto vale per la stessa ragione per cui `MotionIntensity` si compone
//! con la preferenza di sistema invece di sovrascriverla: la skin è una scelta
//! estetica di chi ha scritto il tema, la preferenza è una condizione di chi
//! guarda.
//!
//! Un pulsare si fa lo stesso: `direction: "alternate"` con `iterations: 2` va
//! e torna una volta, e con `both` resta dov'è arrivato.
//!
//! # Le cinque `@keyframes` di `stile.css` restano del motore
//!
//! Non si portano in questo formato, e non per dimenticanza:
//!
//! - **quattro sono infinite** (`aggiornamento-ignoto`, `scorri`, `scintilla`,
//!   `lettura-scorre`), quindi il divieto qui sopra le esclude da sé;
//! - fra quelle, `aggiornamento-ignoto` e `lettura-scorre` dicono **una
//!   condizione del sistema** — «sto lavorando e non so quanto manca» — e
//!   `scintilla` e `scorri` dicono «qui arriverà qualcosa». È leggibilità, non
//!   decorazione: una skin che le ridisegnasse potrebbe far sparire la
//!   distinzione fra una barra che sa quanto manca e una che non lo sa;
//! - **`batte`** è l'unica finita, ed è già scritta come
//!   `calc(var(--dur-2) * var(--motion-scale, 1))`. Diventa skinnabile senza
//!   aggiungere niente il giorno in cui `.cuore` entra nel registro delle
//!   parti: sarà un'animazione sul trigger `active`, che è esattamente ciò che
//!   `aria-pressed` è.
//!
//! # I trigger, e il confine che disegnano
//!
//! `enter`, `hover`, `active`, `focus`, `disabled`. Il confine da tenere a
//! mente è: **le skin animano stati del DOM, non eventi dell'applicazione.**
//! Un'animazione «quando finisce un brano» o «quando arriva una notifica»
//! vorrebbe che il formato conoscesse il ciclo di vita dell'app, e non lo
//! conosce; i quattro stati invece sono già [`PartState`], hanno già un
//! selettore nel registro, e sono già quelli che il compilatore sa scrivere. È
//! il motivo per cui [`AnimTrigger`] li **riusa** invece di riscriverli: due
//! tabelle di selettori sono due tabelle che possono divergere.
//!
//! `enter` non è un gancio nuovo: è la regola base della parte, cioè la stessa
//! che porta lo sfondo e il bordo. Un'animazione lì parte quando l'elemento
//! entra nel documento, che è quel che il CSS fa da solo.
//!
//! **`exit` resta fuori dal v1.** In CSS puro non esiste: perché un elemento
//! che se ne va possa animarsi, qualcuno deve tenerlo montato finché
//! l'animazione finisce, e quel qualcuno è il renderer, non il foglio.
//! Dichiararlo senza il gancio ripeterebbe alla lettera il difetto di `shadow`
//! nel vecchio albero — un campo che chi legge lo schema deve capire e poi
//! scartare, perché non può valere niente di utile.
//!
//! # Niente colori nei fotogrammi
//!
//! Un cambio di colore si fa già, e meglio, con `states` più
//! `--transition-fast`/`--transition-med`: sono le due variabili che il foglio
//! scala, quindi la transizione rispetta la preferenza gratis. Dentro un
//! fotogramma un colore costerebbe un repaint per fotogramma — mentre opacità e
//! trasformazione le anima il compositore senza ridisegnare niente — e non si
//! comporrebbe col [`Paint`](crate::effects::Paint) multilivello che
//! `background` accetta: un `background` è una pila, e un keyframe scriverebbe
//! un colore solo sopra tutta la pila.
//!
//! # Perché [`AnimFrame`] non è [`RouteFrame`](crate::document::RouteFrame)
//!
//! La strada corta sarebbe allargare `RouteFrame` con `translateX` e `rotate`.
//! `RouteFrame` però è il vocabolario **già spedito** della transizione di
//! rotta, e allargarlo vorrebbe dire che da domani `routeTransition` accetta
//! una rotazione — su `::view-transition-old(root)`, cioè su un'istantanea
//! dell'intera finestra. Un tipo condiviso fra due usi diventa il minimo comune
//! multiplo dei due, non il massimo comun divisore.

use crate::effects::CostClass;
use crate::parts::PartState;
use crate::values::{Duration, Easing, EasingKeyword};

// ── I tetti ─────────────────────────────────────────────────────────────────

/// Quante animazioni un documento può dichiarare.
///
/// Otto è il numero oltre il quale l'elenco nello Studio smette di essere una
/// cosa che si guarda tutta insieme. Non è un limite di costo — una dichiarata
/// e mai assegnata costa zero — è un limite di leggibilità del documento.
pub const MAX_ANIMAZIONI: usize = 8;

/// Quanti fotogrammi ha un'animazione, al minimo.
///
/// Due: con uno solo non c'è interpolazione, c'è uno stato — e uno stato si
/// scrive in `states`, dove costa una transizione invece di un'animazione.
pub const MIN_FOTOGRAMMI: usize = 2;

/// Quanti fotogrammi ha un'animazione, al massimo.
pub const MAX_FOTOGRAMMI: usize = 6;

/// Quanto può durare un'animazione, in millisecondi.
///
/// Due secondi. Oltre, un'animazione su uno stato del DOM non è più un
/// movimento: è un elemento che resta a metà strada mentre chi guarda ha già
/// cliccato altrove.
pub const MAX_DURATA_MS: f64 = 2000.0;

/// Quanto può attendere prima di partire, in millisecondi.
pub const MAX_RITARDO_MS: f64 = 1000.0;

/// Quante volte può ripetersi.
///
/// Quattro, e mai «infinite»: vedi il `//!` di questo modulo. Il tetto basso
/// non è estetico — è il modo in cui il costo di [`animation_cost`] resta
/// confrontabile con quello di una superficie.
pub const MAX_ITERAZIONI: u32 = 4;

/// Quanti trigger può assegnare una singola parte.
pub const MAX_TRIGGER_PER_PARTE: usize = 3;

/// Quante parti possono portare almeno un'animazione.
///
/// Dieci superfici che si muovono insieme sono già una finestra irrequieta, e
/// il costo per parte non se ne accorgerebbe: ognuna può stare nel suo budget
/// mentre l'insieme non sta in piedi. È la stessa distinzione che c'è fra
/// [`SURFACE_COST_BUDGET`](crate::effects::SURFACE_COST_BUDGET) e
/// [`SHELL_COST_BUDGET`](crate::layout::SHELL_COST_BUDGET): quanto costa una
/// cosa, e quante ce ne sono.
pub const MAX_PARTI_ANIMATE: usize = 10;

// ── Il costo ────────────────────────────────────────────────────────────────

/// Budget del movimento di una singola parte.
///
/// # Come è tarato
///
/// Sulla stessa scala di [`CostClass`], perché è la sola già in uso e due scale
/// di costo darebbero due numeri che non si sommano. Un'animazione, finché
/// dura, obbliga il motore a promuovere l'elemento a un livello proprio: è
/// esattamente [`CostClass::Composited`], cioè **quattro** — la stessa classe
/// del `chamfer`, che fa la stessa cosa per la stessa ragione. Ogni ripetizione
/// oltre la prima tiene quel livello in vita più a lungo, e vale **uno**.
///
/// Dodici è quindi **tre animazioni suonate una volta ciascuna**, cioè il
/// massimo dei trigger per parte al minimo delle iterazioni: entrare, reagire
/// al puntatore, reagire alla pressione. Chi fa quelle tre cose sta esattamente
/// nel budget; chi ne fa ripetere una — un pulsare a `alternate` con due
/// iterazioni, che è il rimedio previsto al divieto di `infinite` — lo sfora di
/// uno, ed è il momento giusto per dirglielo. È la stessa forma della taratura
/// di [`SURFACE_COST_BUDGET`](crate::effects::SURFACE_COST_BUDGET), dove una
/// tinta più tre motivi fanno dieci esatti.
///
/// Il tetto duro sui trigger (tre) e sulle iterazioni (quattro) da solo
/// lascerebbe passare 3 × 4 = dodici ripetizioni, cioè ventuno: un budget che
/// i tetti non possono superare non è un budget, è un commento.
pub const MOTION_COST_BUDGET: u32 = 12;

/// Quanto costa un'animazione, per la parte che la porta.
///
/// **I fotogrammi non contano, ed è una scelta.** Il motore interpola fra due
/// fermate esattamente come fra sei: il numero di fermate cambia quanto è
/// lunga la regola, non quanto lavoro si fa per fotogramma. Contarli darebbe un
/// numero più grande e meno vero, e un modello di costo che conta qualcosa di
/// gratuito è un modello che chi lo legge impara a ignorare. Il tetto su
/// [`MAX_FOTOGRAMMI`] esiste, ma è del formato, non del costo.
///
/// Nemmeno le proprietà contano: opacità, traslazione, scala e rotazione sono
/// tutte e quattro composite. È lo stesso motivo per cui i colori restano
/// fuori dai fotogrammi — quelle sì che costerebbero, e in modo diverso.
#[must_use]
pub const fn animation_cost(animation: &Animation) -> u32 {
    CostClass::Composited
        .weight()
        .saturating_add(animation.iterations.saturating_sub(1))
}

/// Il costo sommato di un gruppo di animazioni.
///
/// Gemella di [`stack_cost`](crate::effects::stack_cost), e per la stessa
/// ragione: il budget si confronta con una somma, e una somma scritta in due
/// posti è una somma che diverge.
#[must_use]
pub fn motion_cost(animations: &[&Animation]) -> u32 {
    animations
        .iter()
        .map(|animation| animation_cost(animation))
        .sum()
}

/// Il gruppo sfora il budget del movimento di una parte?
#[must_use]
pub fn exceeds_motion_budget(animations: &[&Animation]) -> bool {
    motion_cost(animations) > MOTION_COST_BUDGET
}

// ── La forma ────────────────────────────────────────────────────────────────

/// Un fotogramma di un'animazione nominata.
///
/// Solo opacità e trasformazione, come per la transizione di rotta e per la
/// stessa ragione: sono le proprietà che il compositore anima senza
/// ridisegnare. Il vincolo è del tipo, non di una raccomandazione — non esiste
/// un campo in cui infilare qualcos'altro.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AnimFrame {
    /// Dove sta, in percentuale del percorso: da 0 a 100.
    pub at: f64,
    /// Opacità.
    pub opacity: Option<f64>,
    /// Scala.
    pub scale: Option<f64>,
    /// Spostamento orizzontale, in pixel.
    pub translate_x: Option<f64>,
    /// Spostamento verticale, in pixel.
    pub translate_y: Option<f64>,
    /// Rotazione, in gradi.
    pub rotate: Option<f64>,
}

impl AnimFrame {
    /// Non dichiara niente: sarebbe una fermata vuota in mezzo al percorso.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.opacity.is_none()
            && self.scale.is_none()
            && self.translate_x.is_none()
            && self.translate_y.is_none()
            && self.rotate.is_none()
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
    ///
    /// È la metà utile del divieto di `infinite`: un pulsare è
    /// `alternate` con due iterazioni, e finisce dov'era partito.
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

/// Un'animazione dichiarata in `motion.animations`.
///
/// `fill` non c'è, e non è un campo dimenticato: il compilatore scrive sempre
/// `both`. Un'animazione che non tiene lo stato finale fa **saltare** la parte
/// al valore di partenza nell'istante in cui finisce, ed è il difetto che chi
/// scrive una skin scopre dopo aver provato tutto il resto. Renderlo
/// dichiarabile vorrebbe dire offrire una manopola il cui unico altro valore è
/// un bug.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    /// Quanto dura una ripetizione. Al massimo [`MAX_DURATA_MS`].
    pub duration: Duration,
    /// La curva. Assente nel documento: `ease`, cioè il default del CSS.
    pub easing: Easing,
    /// Quanto attende prima di partire.
    pub delay: Option<Duration>,
    /// Quante volte si ripete: da 1 a [`MAX_ITERAZIONI`]. Mai «infinite».
    pub iterations: u32,
    /// In che verso.
    pub direction: AnimDirection,
    /// Le fermate, in ordine di [`AnimFrame::at`] crescente, la prima a 0.
    pub frames: Vec<AnimFrame>,
}

impl Animation {
    /// Quanto costa, alla parte che la porta.
    #[must_use]
    pub const fn cost(&self) -> u32 {
        animation_cost(self)
    }
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            duration: Duration { ms: 0.0 },
            easing: Easing::Keyword(EasingKeyword::Ease),
            delay: None,
            iterations: 1,
            direction: AnimDirection::Normal,
            frames: Vec::new(),
        }
    }
}

/// Che cosa fa partire un'animazione.
///
/// I quattro stati **sono** [`PartState`], non una copia: hanno già un
/// selettore nel registro, e riscriverlo qui vorrebbe dire due tabelle da
/// tenere allineate a mano — cioè la stessa cosa che il vecchio albero faceva
/// con costo e proprietà degli effetti, e che qui è un `match` esaustivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimTrigger {
    /// Quando la parte entra nel documento: è la sua regola base.
    Enter,
    /// Uno dei quattro stati del DOM che il registro conosce già.
    State(PartState),
}

impl AnimTrigger {
    /// Come si scrive nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enter => "enter",
            Self::State(stato) => stato.as_str(),
        }
    }

    /// Tutti, nell'ordine in cui il compilatore li emette.
    ///
    /// `enter` per primo perché è la regola base, e a parità di specificità nel
    /// CSS vince chi viene dopo: uno stato deve poter dire l'ultima parola su
    /// quel che la base ha già detto.
    pub const ALL: &'static [Self] = &[
        Self::Enter,
        Self::State(PartState::Hover),
        Self::State(PartState::Active),
        Self::State(PartState::Focus),
        Self::State(PartState::Disabled),
    ];

    /// Dal nome scritto nel documento.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == raw)
    }

    /// I nomi ammessi, per il messaggio di un trigger sconosciuto.
    #[must_use]
    pub fn nomi() -> Vec<&'static str> {
        Self::ALL.iter().map(|t| t.as_str()).collect()
    }
}

/// Un'animazione richiamata da una parte: il nome, e la definizione risolta.
///
/// Porta la definizione con sé come fa
/// [`Paint::Pattern`](crate::effects::Paint::Pattern), e per la stessa ragione:
/// chi deve sapere **quanto costa** o **quanto dura** non deve tenersi accanto
/// la tabella delle animazioni per scoprirlo. Il nome serve al solo
/// compilatore, che ci scrive il nome dei `@keyframes`.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationRef {
    /// Il nome dichiarato in `motion.animations`.
    pub name: String,
    /// La definizione, copiata qui al momento di validare.
    pub def: Animation,
}

/// Le animazioni che una parte assegna, per trigger.
///
/// # Perché sta in [`PartStyle`](crate::parts::PartStyle) e non in
/// [`PartAppearance`](crate::parts::PartAppearance)
///
/// `PartAppearance` è usata **identica** per la base e per i quattro stati: è
/// la ragione per cui base e stati non possono divergere. Un campo che lì
/// valesse solo alla base sarebbe una trappola — comparirebbe nello schema di
/// `states.hover`, si potrebbe scrivere, e non farebbe niente. E ci sarebbe di
/// peggio: un'animazione dentro `states.hover` avrebbe due modi di dire la
/// stessa cosa, `animations.hover` e quello, che è precisamente il tipo di
/// doppione che prima o poi si contraddice.
///
/// Ma la ragione vera viene prima: un'animazione **non è un valore d'aspetto**.
/// Uno sfondo dice com'è fatta la superficie adesso; un'animazione dice come si
/// arriva da un aspetto a un altro. È un legame, e i legami di questo formato —
/// `$pattern`, `$token`, il richiamo a una curva — stanno tutti al livello di
/// chi li usa, non dentro il valore.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartAnimations {
    /// All'ingresso: la regola base della parte.
    pub enter: Option<AnimationRef>,
    /// Al passaggio del puntatore.
    pub hover: Option<AnimationRef>,
    /// Quando è attiva.
    pub active: Option<AnimationRef>,
    /// Col fuoco da tastiera.
    pub focus: Option<AnimationRef>,
    /// Quando è disabilitata.
    pub disabled: Option<AnimationRef>,
}

impl PartAnimations {
    /// L'animazione di un trigger, se assegnata.
    #[must_use]
    pub const fn get(&self, trigger: AnimTrigger) -> Option<&AnimationRef> {
        match trigger {
            AnimTrigger::Enter => self.enter.as_ref(),
            AnimTrigger::State(PartState::Hover) => self.hover.as_ref(),
            AnimTrigger::State(PartState::Active) => self.active.as_ref(),
            AnimTrigger::State(PartState::Focus) => self.focus.as_ref(),
            AnimTrigger::State(PartState::Disabled) => self.disabled.as_ref(),
        }
    }

    /// Assegna un trigger. Sostituisce quel che c'era.
    pub fn set(&mut self, trigger: AnimTrigger, riferimento: AnimationRef) {
        let posto = match trigger {
            AnimTrigger::Enter => &mut self.enter,
            AnimTrigger::State(PartState::Hover) => &mut self.hover,
            AnimTrigger::State(PartState::Active) => &mut self.active,
            AnimTrigger::State(PartState::Focus) => &mut self.focus,
            AnimTrigger::State(PartState::Disabled) => &mut self.disabled,
        };
        *posto = Some(riferimento);
    }

    /// Nessun trigger assegnato: non c'è niente da emettere.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        AnimTrigger::ALL
            .iter()
            .all(|trigger| self.get(*trigger).is_none())
    }

    /// I trigger assegnati, nell'ordine di [`AnimTrigger::ALL`].
    ///
    /// L'ordine è quello e non quello di scrittura nel documento: un oggetto
    /// JSON non ha un ordine su cui fare affidamento, e il foglio prodotto
    /// dev'essere lo stesso a ogni compilazione.
    #[must_use]
    pub fn assegnate(&self) -> Vec<(AnimTrigger, &AnimationRef)> {
        AnimTrigger::ALL
            .iter()
            .filter_map(|trigger| self.get(*trigger).map(|voce| (*trigger, voce)))
            .collect()
    }

    /// I nomi richiamati, per sapere quali animazioni sono usate.
    #[must_use]
    pub fn nomi_usati(&self) -> Vec<&str> {
        self.assegnate()
            .into_iter()
            .map(|(_, voce)| voce.name.as_str())
            .collect()
    }

    /// Quanto costa il movimento di questa parte.
    ///
    /// **Solo le animazioni assegnate.** Una dichiarata e mai richiamata costa
    /// zero: si compila in un `@keyframes` che nessuno usa, esattamente come un
    /// motivo inutilizzato si compila in una variabile che nessuno legge — un
    /// problema, ma non un costo, e infatti è un avviso di tipo diverso.
    #[must_use]
    pub fn costo(&self) -> u32 {
        motion_cost(
            &self
                .assegnate()
                .into_iter()
                .map(|(_, voce)| &voce.def)
                .collect::<Vec<_>>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn animazione(iterations: u32) -> Animation {
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
    fn tre_animazioni_suonate_una_volta_stanno_esattamente_nel_budget() {
        // È la taratura scritta a parole in `MOTION_COST_BUDGET`, messa in un
        // numero: entrare, reagire al puntatore, reagire alla pressione.
        let una = animazione(1);
        let tre = [&una, &una, &una];
        assert_eq!(motion_cost(&tre), MOTION_COST_BUDGET);
        assert!(!exceeds_motion_budget(&tre));

        // E la quarta ripetizione di una qualunque lo sfora: è il momento in
        // cui vale la pena dirlo, non prima.
        let pulsa = animazione(2);
        assert!(exceeds_motion_budget(&[&una, &una, &pulsa]));
    }

    #[test]
    fn i_fotogrammi_non_entrano_nel_costo() {
        // Un modello di costo che conta qualcosa di gratuito è un modello che
        // chi lo legge impara a ignorare.
        let mut lunga = animazione(1);
        lunga.frames = (0..MAX_FOTOGRAMMI)
            .map(|indice| AnimFrame {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "sei fotogrammi al massimo: nessun indice arriva dove un f64 perde cifre"
                )]
                at: indice as f64 * 20.0,
                opacity: Some(1.0),
                ..AnimFrame::default()
            })
            .collect();
        assert_eq!(lunga.cost(), animazione(1).cost());
    }

    #[test]
    fn i_trigger_sono_i_quattro_stati_piu_l_ingresso() {
        // Se un giorno il registro guadagnasse un quinto stato, questa
        // asserzione cadrebbe — ed è il modo giusto di accorgersene, perché il
        // trigger nuovo va deciso, non ereditato in silenzio.
        assert_eq!(AnimTrigger::ALL.len(), PartState::ALL.len() + 1);
        assert_eq!(
            AnimTrigger::nomi(),
            ["enter", "hover", "active", "focus", "disabled"]
        );
        assert_eq!(AnimTrigger::parse("exit"), None);
        assert_eq!(
            AnimTrigger::parse("focus"),
            Some(AnimTrigger::State(PartState::Focus))
        );
    }

    #[test]
    fn l_ordine_dei_trigger_non_dipende_da_come_sono_stati_scritti() {
        let mut prima = PartAnimations::default();
        prima.set(
            AnimTrigger::State(PartState::Focus),
            AnimationRef {
                name: "b".to_owned(),
                def: animazione(1),
            },
        );
        prima.set(
            AnimTrigger::Enter,
            AnimationRef {
                name: "a".to_owned(),
                def: animazione(1),
            },
        );
        assert_eq!(prima.nomi_usati(), ["a", "b"]);
        assert!(!prima.is_empty());
        assert!(PartAnimations::default().is_empty());
    }

    #[test]
    fn il_verso_si_scrive_come_in_css() {
        assert_eq!(AnimDirection::default(), AnimDirection::Normal);
        assert_eq!(
            AnimDirection::parse("alternate-reverse"),
            Some(AnimDirection::AlternateReverse)
        );
        assert_eq!(AnimDirection::parse("alternateReverse"), None);
    }
}
