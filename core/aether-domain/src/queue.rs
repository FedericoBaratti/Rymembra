//! La coda di riproduzione: cosa suona adesso, cosa suona dopo.
//!
//! Qui non si sente niente. Questo modulo decide **quale brano**, e il motore
//! audio decide come farlo uscire dagli altoparlanti: la separazione è la stessa
//! che c'è fra [`crate::scan_plan`] e chi cammina davvero sul disco, e serve alla
//! stessa cosa. Una coda che si prova come una chiamata di funzione è una coda
//! che si può provare in tutti i casi che contano — la fine dell'elenco, il
//! brano tolto mentre suona, lo shuffle acceso a metà — invece che nei due o tre
//! che si riescono a mettere in scena aprendo una finestra.
//!
//! # La coda non si mescola: si mescola l'ordine
//!
//! È la scelta strutturale di tutto il modulo, ed è ripresa dal vecchio albero
//! (`usePlayerStore.ts`) perché era giusta. I brani stanno in un `Vec` che non
//! cambia ordine quando si accende lo shuffle; quel che cambia è `order`, una
//! **permutazione** di indici che dice in che ordine attraversarli.
//!
//! Mescolare l'elenco vero avrebbe due conseguenze, entrambe visibili
//! all'utente: spegnere lo shuffle non potrebbe più ricostruire l'ordine
//! originale — è stato distrutto — e il pannello della coda cambierebbe
//! contenuto sotto le dita di chi lo sta guardando. Con la permutazione,
//! spegnere lo shuffle è tornare all'identità, e il brano che sta suonando resta
//! quello che sta suonando.
//!
//! # Il mescolamento prende un seme
//!
//! `shuffled_order` non chiama un generatore globale: riceve un `seed`. Il
//! dominio non guarda l'orologio e non tiene stato nascosto, e la conseguenza
//! pratica è che un ordine mescolato è riproducibile — cioè provabile. Il seme
//! lo procura chi ha un orologio, che è il livello sopra.

/// Come si ripete alla fine della coda.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RepeatMode {
    /// Finita la coda ci si ferma.
    #[default]
    Off,
    /// Il brano corrente ricomincia da solo.
    One,
    /// Finita la coda si riparte dall'inizio.
    All,
}

impl RepeatMode {
    /// Il modo successivo nel giro del pulsante: spento, tutto, uno.
    ///
    /// Quest'ordine e non «spento, uno, tutto»: è quello del vecchio albero, ed
    /// è anche quello meno sorprendente, perché mette accanto i due modi che
    /// continuano a suonare e lascia in fondo quello che si incaglia su un brano
    /// solo.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

/// Cosa deve fare il motore dopo aver chiesto alla coda di spostarsi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Ricominciare da capo il brano che sta già suonando.
    Restart,
    /// Passare a questo brano.
    Track(i64),
    /// Fermarsi: non c'è un dopo.
    Stop,
}

/// Sotto questa soglia, «precedente» vuol dire il brano prima.
///
/// Sopra, vuol dire ricominciare questo. È la convenzione di ogni lettore
/// esistente e la ragione è che il gesto ha due significati: a tre secondi
/// dall'inizio si è sbagliato brano, a tre minuti si vuole risentire il
/// ritornello.
pub const RIAVVIO_SOTTO_MS: u64 = 3_000;

/// La coda di riproduzione.
///
/// Gli invarianti, mantenuti da ogni metodo: `order` è sempre una permutazione
/// di `0..tracks.len()`, e `order_pos` — quando c'è — indica sempre una
/// posizione valida dentro `order`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Queue {
    /// Gli identificativi dei brani, nell'ordine in cui sono stati messi.
    tracks: Vec<i64>,
    /// La permutazione: in che ordine attraversare `tracks`.
    order: Vec<usize>,
    /// Dove siamo dentro `order`. `None` quando la coda è vuota o ferma.
    ///
    /// Un `Option` e non un `-1`: nel vecchio albero la posizione «nessuna» era
    /// un indice negativo, e ogni lettura doveva ricordarsi di controllarlo.
    order_pos: Option<usize>,
    /// Lo shuffle è acceso.
    shuffle: bool,
    /// Come si ripete.
    repeat: RepeatMode,
}

impl Queue {
    /// Una coda vuota.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // ── quel che si legge ───────────────────────────────────────────────────

    /// Il brano che sta suonando.
    #[must_use]
    pub fn current(&self) -> Option<i64> {
        self.current_index()
            .and_then(|i| self.tracks.get(i))
            .copied()
    }

    /// L'indice, dentro l'elenco dei brani, di quello che sta suonando.
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.order_pos.and_then(|pos| self.order.get(pos)).copied()
    }

    /// Quanti brani ci sono.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// La coda è vuota.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Lo shuffle è acceso.
    #[must_use]
    pub const fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Come si ripete.
    #[must_use]
    pub const fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    /// I brani nell'ordine in cui suoneranno.
    ///
    /// È questo che il pannello della coda mostra — non `tracks`. Con lo shuffle
    /// acceso i due differiscono, e mostrare l'elenco non mescolato vorrebbe dire
    /// mostrare un «dopo» che non è il dopo.
    #[must_use]
    pub fn in_play_order(&self) -> Vec<i64> {
        self.order
            .iter()
            .filter_map(|&i| self.tracks.get(i))
            .copied()
            .collect()
    }

    /// La posizione del brano corrente dentro [`Self::in_play_order`].
    #[must_use]
    pub const fn position(&self) -> Option<usize> {
        self.order_pos
    }

    /// Il brano dopo, senza spostarsi.
    ///
    /// Serve al motore per aprirlo in anticipo: il gapless è esattamente questo,
    /// avere i campioni del brano seguente già pronti quando il corrente finisce.
    /// Con `Repeat::One` il brano dopo è quello corrente.
    #[must_use]
    pub fn peek_next(&self) -> Option<i64> {
        if self.repeat == RepeatMode::One {
            return self.current();
        }
        let pos = self.order_pos?;
        let next = pos.checked_add(1)?;
        if next < self.order.len() {
            self.order
                .get(next)
                .and_then(|&i| self.tracks.get(i))
                .copied()
        } else if self.repeat == RepeatMode::All {
            self.order
                .first()
                .and_then(|&i| self.tracks.get(i))
                .copied()
        } else {
            None
        }
    }

    // ── quel che si cambia ──────────────────────────────────────────────────

    /// Sostituisce la coda e comincia dal brano indicato.
    ///
    /// Con lo shuffle acceso il brano scelto va **in testa** all'ordine, non in
    /// una posizione a caso: chi clicca un brano vuole sentire quello, e poi il
    /// resto mescolato.
    pub fn play_tracks(&mut self, tracks: Vec<i64>, start: usize, seed: u64) {
        if tracks.is_empty() {
            self.clear();
            return;
        }
        let start = start.min(tracks.len().saturating_sub(1));
        self.order = if self.shuffle {
            shuffled_order(tracks.len(), start, seed)
        } else {
            (0..tracks.len()).collect()
        };
        self.order_pos = Some(if self.shuffle { 0 } else { start });
        self.tracks = tracks;
    }

    /// Salta al brano che sta a questo indice nell'elenco.
    ///
    /// Restituisce il brano su cui si è finiti, o `None` se l'indice non esiste.
    pub fn play_index(&mut self, index: usize) -> Option<i64> {
        let pos = self.order.iter().position(|&i| i == index)?;
        self.order_pos = Some(pos);
        self.current()
    }

    /// Salta alla posizione indicata di [`Self::in_play_order`].
    ///
    /// La variante che serve all'interfaccia: chi clicca una riga del pannello
    /// della coda indica una posizione in quel che sta guardando, non un indice
    /// dell'elenco interno. Con lo shuffle acceso i due numeri differiscono, e
    /// confonderli manderebbe a suonare un brano diverso da quello cliccato.
    pub fn play_at(&mut self, pos: usize) -> Option<i64> {
        if pos >= self.order.len() {
            return None;
        }
        self.order_pos = Some(pos);
        self.current()
    }

    /// Toglie il brano che sta a questa posizione di [`Self::in_play_order`].
    pub fn remove_at(&mut self, pos: usize) {
        let Some(&index) = self.order.get(pos) else {
            return;
        };
        self.remove(index);
    }

    /// Accoda in fondo.
    pub fn enqueue(&mut self, ids: &[i64]) {
        let base = self.tracks.len();
        self.tracks.extend_from_slice(ids);
        self.order.extend(base..self.tracks.len());
        if self.order_pos.is_none() && !self.order.is_empty() {
            self.order_pos = Some(0);
        }
    }

    /// Mette subito dopo il brano corrente.
    pub fn play_next(&mut self, ids: &[i64]) {
        if self.order_pos.is_none() {
            self.enqueue(ids);
            return;
        }
        let base = self.tracks.len();
        self.tracks.extend_from_slice(ids);
        let dopo = self.order_pos.map_or(0, |p| p.saturating_add(1));
        let inserimento = dopo.min(self.order.len());
        self.order
            .splice(inserimento..inserimento, base..self.tracks.len());
    }

    /// Toglie il brano che sta a questo indice nell'elenco.
    ///
    /// Se era quello che suonava, la posizione resta dov'è: lì è scivolato il
    /// brano seguente, e la coda è già pronta per lui. Fermare la riproduzione
    /// perché si è tolto dalla coda il brano in corso sarebbe una sorpresa —
    /// togliere dalla coda vuol dire «non risuonarlo», non «zitto adesso».
    pub fn remove(&mut self, index: usize) {
        if index >= self.tracks.len() {
            return;
        }
        let tolto = self.order.iter().position(|&i| i == index);
        self.tracks.remove(index);
        self.order.retain(|&i| i != index);
        for i in &mut self.order {
            if *i > index {
                *i -= 1;
            }
        }
        self.order_pos = match (self.order_pos, tolto) {
            _ if self.order.is_empty() => None,
            (Some(pos), Some(t)) if t < pos => Some(pos.saturating_sub(1)),
            (Some(pos), _) => Some(pos.min(self.order.len().saturating_sub(1))),
            (None, _) => None,
        };
    }

    /// Sposta un brano dentro l'ordine di riproduzione.
    ///
    /// `from` e `to` sono posizioni in [`Self::in_play_order`], cioè in quel che
    /// l'utente sta guardando mentre trascina — non indici dell'elenco interno.
    /// La posizione corrente si aggiusta perché il brano che suona deve restare
    /// quello che suona anche quando gli si sposta qualcosa attorno.
    pub fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.order.len() || to >= self.order.len() || from == to {
            return;
        }
        let spostato = self.order.remove(from);
        self.order.insert(to, spostato);
        self.order_pos = self.order_pos.map(|pos| {
            if from == pos {
                to
            } else {
                let mut nuova = pos;
                if from < pos {
                    nuova = nuova.saturating_sub(1);
                }
                if to <= nuova {
                    nuova = nuova.saturating_add(1);
                }
                nuova.min(self.order.len().saturating_sub(1))
            }
        });
    }

    /// Svuota tutto.
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.order.clear();
        self.order_pos = None;
    }

    /// Va al brano dopo.
    ///
    /// `manual` distingue il pulsante «prossimo» dalla fine naturale di un brano,
    /// e la differenza sta tutta in `Repeat::One`: un brano che finisce da solo
    /// ricomincia, ma chi preme «prossimo» vuole andare avanti. Confonderli
    /// significa un pulsante che non fa niente, che è il modo più rapido di far
    /// credere che l'applicazione si sia bloccata.
    pub fn advance(&mut self, manual: bool) -> Step {
        if self.order.is_empty() {
            return Step::Stop;
        }
        if !manual && self.repeat == RepeatMode::One {
            return Step::Restart;
        }
        let pos = match self.order_pos {
            Some(pos) => pos,
            None => {
                self.order_pos = Some(0);
                return self.current().map_or(Step::Stop, Step::Track);
            }
        };
        let next = pos.saturating_add(1);
        if next < self.order.len() {
            self.order_pos = Some(next);
        } else if self.repeat == RepeatMode::All {
            self.order_pos = Some(0);
        } else {
            return Step::Stop;
        }
        self.current().map_or(Step::Stop, Step::Track)
    }

    /// Va al brano prima, o ricomincia questo.
    ///
    /// Vedi [`RIAVVIO_SOTTO_MS`]: il gesto ha due significati e li separa la
    /// posizione nel brano.
    ///
    /// # In testa alla coda
    ///
    /// Con `Repeat::All` si avvolge all'**ultimo** brano, esattamente come
    /// [`Self::advance`] avvolge al primo. Senza, i due versi della stessa coda
    /// circolare si comporterebbero diversamente: andando avanti la coda gira,
    /// tornando indietro si incaglia sul primo brano. Con gli altri due modi non
    /// c'è un brano prima del primo, e allora il gesto vuol dire «ricomincia».
    pub fn previous(&mut self, position_ms: u64) -> Step {
        if self.order.is_empty() {
            return Step::Stop;
        }
        let pos = self.order_pos.unwrap_or(0);
        if position_ms >= RIAVVIO_SOTTO_MS {
            return Step::Restart;
        }
        let prima = match pos.checked_sub(1) {
            Some(prima) => prima,
            None if self.repeat == RepeatMode::All => self.order.len().saturating_sub(1),
            None => return Step::Restart,
        };
        self.order_pos = Some(prima);
        self.current().map_or(Step::Stop, Step::Track)
    }

    /// Accende o spegne lo shuffle, tenendo fermo il brano corrente.
    ///
    /// Acceso: il brano corrente va in testa a un ordine nuovo. Spento: l'ordine
    /// torna quello dell'elenco, e la posizione si ritrova lì dov'è il brano
    /// corrente. In nessuno dei due casi cambia quel che si sta sentendo — che è
    /// il punto: lo shuffle riguarda il dopo, non l'adesso.
    pub fn toggle_shuffle(&mut self, seed: u64) {
        let corrente = self.current_index().unwrap_or(0);
        if self.shuffle {
            self.shuffle = false;
            self.order = (0..self.tracks.len()).collect();
            self.order_pos = if self.tracks.is_empty() {
                None
            } else {
                Some(corrente.min(self.tracks.len().saturating_sub(1)))
            };
        } else {
            self.shuffle = true;
            self.order = shuffled_order(self.tracks.len(), corrente, seed);
            self.order_pos = if self.order.is_empty() { None } else { Some(0) };
        }
    }

    /// Passa al modo di ripetizione successivo, e lo restituisce.
    pub fn cycle_repeat(&mut self) -> RepeatMode {
        self.repeat = self.repeat.next();
        self.repeat
    }

    /// Impone un modo di ripetizione.
    pub const fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    // ── conservare e riprendere ─────────────────────────────────────────────

    /// Lo stato da scrivere per ritrovare la coda al prossimo avvio.
    #[must_use]
    pub fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            tracks: self.tracks.clone(),
            order: self.order.clone(),
            order_pos: self.order_pos,
            shuffle: self.shuffle,
            repeat: self.repeat,
        }
    }

    /// Ricostruisce una coda da uno stato conservato.
    ///
    /// **Non si fida.** Quel che arriva è stato scritto da una versione
    /// precedente, o modificato a mano, o troncato da un disco pieno: se
    /// l'ordine non è una permutazione valida dell'elenco viene buttato e
    /// rifatto, invece di lasciare la coda in uno stato in cui `order` punta a
    /// brani che non ci sono. Una coda ripresa storta si nota tardi e in un
    /// posto lontano da qui.
    #[must_use]
    pub fn restore(snapshot: QueueSnapshot) -> Self {
        let QueueSnapshot {
            tracks,
            order,
            order_pos,
            shuffle,
            repeat,
        } = snapshot;
        let valido = order.len() == tracks.len() && {
            let mut visti = vec![false; tracks.len()];
            order.iter().all(|&i| match visti.get_mut(i) {
                Some(v) if !*v => {
                    *v = true;
                    true
                }
                _ => false,
            })
        };
        let order = if valido {
            order
        } else {
            (0..tracks.len()).collect()
        };
        let order_pos = order_pos.filter(|&p| p < order.len());
        Self {
            tracks,
            order,
            order_pos,
            shuffle,
            repeat,
        }
    }
}

/// La coda in una forma che si può scrivere e rileggere.
///
/// Sta qui e non nel livello che la salva perché gli invarianti che
/// [`Queue::restore`] verifica sono invarianti di dominio: chi scrive il JSON
/// non deve poter inventare una permutazione.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueSnapshot {
    /// Gli identificativi dei brani.
    pub tracks: Vec<i64>,
    /// La permutazione.
    pub order: Vec<usize>,
    /// La posizione dentro la permutazione.
    pub order_pos: Option<usize>,
    /// Lo shuffle era acceso.
    pub shuffle: bool,
    /// Come si ripeteva.
    pub repeat: RepeatMode,
}

/// Una permutazione di `0..len` che comincia da `first`.
///
/// Il brano scelto in testa, il resto mescolato con Fisher-Yates. Il generatore
/// è uno SplitMix64 scritto qui invece che preso da una dipendenza: servono
/// numeri distribuiti bene per rimescolare un elenco, non numeri
/// imprevedibili, e questo crate ha una dipendenza sola.
#[must_use]
pub(crate) fn shuffled_order(len: usize, first: usize, seed: u64) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let first = first.min(len.saturating_sub(1));
    let mut resto: Vec<usize> = (0..len).filter(|&i| i != first).collect();
    let mut stato = seed;
    let mut i = resto.len();
    while i > 1 {
        i -= 1;
        let limite = u64::try_from(i).unwrap_or(u64::MAX).saturating_add(1);
        let j = usize::try_from(prossimo(&mut stato) % limite).unwrap_or(0);
        resto.swap(i, j);
    }
    let mut order = Vec::with_capacity(len);
    order.push(first);
    order.extend(resto);
    order
}

/// SplitMix64: un passo del generatore.
fn prossimo(stato: &mut u64) -> u64 {
    *stato = stato.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *stato;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod prove {
    use super::*;

    fn coda(n: i64) -> Queue {
        let mut q = Queue::new();
        q.play_tracks((1..=n).collect(), 0, 7);
        q
    }

    #[test]
    fn una_coda_nuova_comincia_dal_brano_scelto() {
        let mut q = Queue::new();
        q.play_tracks(vec![10, 20, 30], 1, 0);
        assert_eq!(q.current(), Some(20));
        assert_eq!(q.position(), Some(1));
    }

    #[test]
    fn avanzare_arriva_in_fondo_e_si_ferma() {
        let mut q = coda(3);
        assert_eq!(q.advance(true), Step::Track(2));
        assert_eq!(q.advance(true), Step::Track(3));
        assert_eq!(q.advance(true), Step::Stop);
    }

    #[test]
    fn repeat_all_riparte_dall_inizio() {
        let mut q = coda(2);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.advance(true), Step::Track(2));
        assert_eq!(q.advance(true), Step::Track(1));
    }

    #[test]
    fn repeat_one_ricomincia_solo_se_il_brano_e_finito_da_solo() {
        let mut q = coda(3);
        q.set_repeat(RepeatMode::One);
        // Finito da solo: ricomincia.
        assert_eq!(q.advance(false), Step::Restart);
        assert_eq!(q.current(), Some(1));
        // Premuto «prossimo»: avanza lo stesso. Un pulsante che non fa niente
        // sembra un'applicazione bloccata.
        assert_eq!(q.advance(true), Step::Track(2));
    }

    #[test]
    fn precedente_ricomincia_se_il_brano_e_gia_andato_avanti() {
        let mut q = coda(3);
        q.advance(true);
        assert_eq!(q.previous(RIAVVIO_SOTTO_MS), Step::Restart);
        assert_eq!(q.current(), Some(2));
        assert_eq!(q.previous(500), Step::Track(1));
    }

    #[test]
    fn precedente_sul_primo_brano_lo_ricomincia() {
        let mut q = coda(3);
        assert_eq!(q.previous(0), Step::Restart);
    }

    #[test]
    fn con_ripeti_tutto_precedente_avvolge_come_avvolge_prossimo() {
        // I due versi di una coda circolare devono comportarsi allo stesso modo.
        // Prima andando avanti la coda girava e tornando indietro si incagliava
        // sul primo brano: la stessa coda, dichiarata circolare, che gira in un
        // verso solo.
        let mut q = coda(3);
        q.set_repeat(RepeatMode::All);

        // Indietro dal primo: si arriva all'ultimo.
        assert_eq!(q.previous(0), Step::Track(3));
        assert_eq!(q.position(), Some(2));
        // E avanti dall'ultimo si torna al primo, com'è sempre stato.
        assert_eq!(q.advance(true), Step::Track(1));
        assert_eq!(q.position(), Some(0));
    }

    #[test]
    fn senza_ripeti_tutto_precedente_sul_primo_resta_un_riavvio() {
        // L'avvolgimento è una conseguenza di «ripeti tutto», non un cambio di
        // significato del gesto: con gli altri due modi non c'è un brano prima
        // del primo.
        for modo in [RepeatMode::Off, RepeatMode::One] {
            let mut q = coda(3);
            q.set_repeat(modo);
            assert_eq!(q.previous(0), Step::Restart, "modo {modo:?}");
            assert_eq!(q.current(), Some(1), "modo {modo:?}");
        }
    }

    #[test]
    fn lo_shuffle_e_una_permutazione_e_comincia_dal_brano_scelto() {
        let order = shuffled_order(50, 12, 999);
        assert_eq!(order.first(), Some(&12));
        let mut ordinato = order.clone();
        ordinato.sort_unstable();
        assert_eq!(ordinato, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn lo_shuffle_con_lo_stesso_seme_da_lo_stesso_ordine() {
        assert_eq!(shuffled_order(30, 0, 42), shuffled_order(30, 0, 42));
        assert_ne!(shuffled_order(30, 0, 42), shuffled_order(30, 0, 43));
    }

    #[test]
    fn accendere_e_spegnere_lo_shuffle_non_cambia_il_brano_corrente() {
        let mut q = coda(20);
        q.advance(true);
        q.advance(true);
        let prima = q.current();
        q.toggle_shuffle(5);
        assert_eq!(q.current(), prima);
        assert_eq!(q.position(), Some(0));
        q.toggle_shuffle(5);
        assert_eq!(q.current(), prima);
        // Spento, l'ordine è di nuovo quello dell'elenco.
        assert_eq!(q.in_play_order(), (1..=20).collect::<Vec<_>>());
    }

    #[test]
    fn spegnere_lo_shuffle_ritrova_la_posizione_nell_elenco() {
        let mut q = coda(10);
        q.toggle_shuffle(3);
        q.advance(true);
        q.advance(true);
        let corrente = q.current();
        q.toggle_shuffle(3);
        assert_eq!(q.current(), corrente);
        // La posizione è quella del brano nell'elenco non mescolato.
        assert_eq!(q.position(), q.current_index());
    }

    #[test]
    fn accodare_mette_in_fondo_e_dopo_mette_subito_dopo() {
        let mut q = coda(3);
        q.enqueue(&[99]);
        assert_eq!(q.in_play_order(), vec![1, 2, 3, 99]);
        q.play_next(&[77]);
        assert_eq!(q.in_play_order(), vec![1, 77, 2, 3, 99]);
        // Il brano corrente non è cambiato.
        assert_eq!(q.current(), Some(1));
        assert_eq!(q.advance(true), Step::Track(77));
    }

    #[test]
    fn accodare_su_una_coda_vuota_fa_partire() {
        let mut q = Queue::new();
        q.enqueue(&[5, 6]);
        assert_eq!(q.current(), Some(5));
        let mut q = Queue::new();
        q.play_next(&[5, 6]);
        assert_eq!(q.current(), Some(5));
    }

    #[test]
    fn togliere_prima_del_corrente_tiene_il_corrente() {
        let mut q = coda(4);
        q.advance(true);
        q.advance(true);
        assert_eq!(q.current(), Some(3));
        q.remove(0);
        assert_eq!(q.current(), Some(3));
        assert_eq!(q.in_play_order(), vec![2, 3, 4]);
    }

    #[test]
    fn togliere_il_corrente_scivola_sul_seguente() {
        let mut q = coda(3);
        q.advance(true);
        assert_eq!(q.current(), Some(2));
        q.remove(1);
        assert_eq!(q.current(), Some(3));
    }

    #[test]
    fn togliere_l_ultimo_brano_svuota() {
        let mut q = coda(1);
        q.remove(0);
        assert!(q.is_empty());
        assert_eq!(q.current(), None);
        assert_eq!(q.advance(true), Step::Stop);
    }

    #[test]
    fn riordinare_non_fa_saltare_il_brano_corrente() {
        // Ogni coppia di posizioni, su una coda intera: il brano che suona deve
        // restare quello che suona, comunque gli si sposti qualcosa attorno.
        for from in 0..5 {
            for to in 0..5 {
                for pos in 0..5 {
                    let mut q = coda(5);
                    for _ in 0..pos {
                        q.advance(true);
                    }
                    let corrente = q.current();
                    q.reorder(from, to);
                    assert_eq!(
                        q.current(),
                        corrente,
                        "da {from} a {to} con il corrente in {pos}"
                    );
                }
            }
        }
    }

    #[test]
    fn riordinare_sposta_davvero() {
        let mut q = coda(4);
        q.reorder(0, 3);
        assert_eq!(q.in_play_order(), vec![2, 3, 4, 1]);
    }

    #[test]
    fn cliccare_una_riga_della_coda_suona_quella_riga() {
        // Con lo shuffle acceso l'indice visto e quello interno differiscono:
        // è il caso in cui confonderli farebbe partire un brano diverso da
        // quello cliccato.
        let mut q = coda(10);
        q.toggle_shuffle(4);
        let visti = q.in_play_order();
        for (posizione, atteso) in visti.iter().enumerate() {
            assert_eq!(q.play_at(posizione), Some(*atteso));
        }
        assert_eq!(q.play_at(99), None);
    }

    #[test]
    fn togliere_per_posizione_toglie_quello_che_si_vede() {
        let mut q = coda(5);
        q.toggle_shuffle(8);
        let visti = q.in_play_order();
        let da_togliere = visti.get(3).copied().expect("cinque brani");
        q.remove_at(3);
        assert!(!q.in_play_order().contains(&da_togliere));
        assert_eq!(q.len(), 4);
    }

    #[test]
    fn il_prossimo_si_sa_in_anticipo() {
        let mut q = coda(2);
        assert_eq!(q.peek_next(), Some(2));
        q.advance(true);
        assert_eq!(q.peek_next(), None);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.peek_next(), Some(1));
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.peek_next(), Some(2));
    }

    #[test]
    fn il_giro_della_ripetizione() {
        let mut q = Queue::new();
        assert_eq!(q.cycle_repeat(), RepeatMode::All);
        assert_eq!(q.cycle_repeat(), RepeatMode::One);
        assert_eq!(q.cycle_repeat(), RepeatMode::Off);
    }

    #[test]
    fn una_coda_conservata_si_ritrova_uguale() {
        let mut q = coda(6);
        q.toggle_shuffle(11);
        q.advance(true);
        q.set_repeat(RepeatMode::All);
        let ripresa = Queue::restore(q.snapshot());
        assert_eq!(ripresa, q);
        assert_eq!(ripresa.current(), q.current());
    }

    #[test]
    fn una_coda_conservata_storta_non_avvelena_la_ripresa() {
        // Un ordine che non è una permutazione: indici fuori dall'elenco e un
        // doppione. Si butta e si rifà, invece di puntare a brani che non ci sono.
        let ripresa = Queue::restore(QueueSnapshot {
            tracks: vec![1, 2, 3],
            order: vec![0, 0, 9],
            order_pos: Some(2),
            shuffle: true,
            repeat: RepeatMode::All,
        });
        assert_eq!(ripresa.in_play_order(), vec![1, 2, 3]);
        assert_eq!(ripresa.current(), Some(3));
    }

    #[test]
    fn una_posizione_conservata_fuori_dall_elenco_diventa_nessuna() {
        let ripresa = Queue::restore(QueueSnapshot {
            tracks: vec![1],
            order: vec![0],
            order_pos: Some(7),
            shuffle: false,
            repeat: RepeatMode::Off,
        });
        assert_eq!(ripresa.position(), None);
        assert_eq!(ripresa.current(), None);
    }

    #[test]
    fn una_coda_vuota_non_esplode() {
        let mut q = Queue::new();
        assert_eq!(q.advance(true), Step::Stop);
        assert_eq!(q.previous(0), Step::Stop);
        assert_eq!(q.peek_next(), None);
        assert_eq!(q.play_index(3), None);
        q.remove(0);
        q.reorder(0, 1);
        q.toggle_shuffle(1);
        assert!(q.is_empty());
    }
}
