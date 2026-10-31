//! Le primitive di tempo della callback, blocco per blocco: l'avvio
//! programmato, il gradino, la deriva e l'ancora. Girano dentro la callback,
//! quindi valgono anche qui i tre divieti scritti in testa a `uscita.rs`.

use std::sync::atomic::Ordering;

use crate::Condiviso;
use crate::ancora::AncoraGrezza;

use super::{
    Andamento, Battito, CANALI_DERIVA, DERIVA_MASSIMA_PPM, Fase, LATENZA_MASSIMA_MS, MILIONE, Modo,
    Voce,
};

/// Le primitive di tempo, per un blocco: chi decide fotogramma per fotogramma
/// se si suona, si tace, si salta o si compone.
///
/// Vive quanto il blocco: quel che deve sopravvivere — la fase del gradino,
/// l'accumulo della deriva — sta in [`Andamento`].
pub(super) struct Regia {
    /// Il fotogramma del blocco da cui l'avvio programmato lascia suonare;
    /// quelli prima tacciono. `None`: nessuna attesa.
    attesa: Option<usize>,
    /// L'avvio letto a inizio blocco: lo si azzera solo se è ancora quello,
    /// così un avvio nuovo scritto nel frattempo vale per il blocco dopo.
    avvio: u64,
    /// La deriva del blocco in parti per milione, già tagliata; zero: spenta.
    ppm: i64,
    /// I canali dell'uscita.
    canali: usize,
    /// Il fotogramma del blocco da cui è uscito il primo campione d'anello.
    pub(super) primo: Option<usize>,
    /// Il primo campione d'anello non era il primo canale di un fotogramma:
    /// niente ancora per questo blocco.
    pub(super) storto: bool,
    /// I campioni saltati da un gradino in questo blocco: si contano.
    pub(super) saltati: u64,
    /// I fotogrammi saltati prima del primo campione preso: spostano il numero
    /// del fotogramma dell'ancora.
    pub(super) saltati_prima: u64,
    /// Il fotogramma composto dalla deriva, quando il modo è
    /// [`Modo::Cornice`].
    pub(super) cornice: [f32; CANALI_DERIVA],
}

/// Dove cade un avvio programmato rispetto al primo fotogramma di un blocco.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cade {
    /// Fra tanti fotogrammi, almeno uno: si tace fin lì.
    Fra(u64),
    /// Già passato, di tanti fotogrammi interi: si parte subito, e quel che si
    /// è perso si salta.
    Ritardo(u64),
}

/// Dove cade l'avvio `avvio_ns` in un blocco il cui primo fotogramma si sente
/// a `playback_ns`.
///
/// Il fotogramma `k` si sente a `playback + k/frequenza`: il primo che non
/// suona **prima** dell'avvio è `k = ⌈(avvio − playback)·frequenza / 10⁹⌉`. In
/// `i128`, perché la differenza può essere negativa e il prodotto per la
/// frequenza non sta in un `i64` oltre i tre anni.
#[expect(
    clippy::integer_division,
    reason = "fotogrammi interi da un tempo: la divisione per eccesso è scritta \
              a mano, e il resto è meno di un fotogramma"
)]
pub(super) fn dove_cade(avvio_ns: u64, playback_ns: u64, frequenza: u32) -> Cade {
    const MILIARDO: i128 = 1_000_000_000;
    let distanza = i128::from(avvio_ns) - i128::from(playback_ns);
    let frequenza = i128::from(frequenza);
    if distanza > 0 {
        let fra = (distanza.saturating_mul(frequenza) + MILIARDO - 1) / MILIARDO;
        Cade::Fra(u64::try_from(fra).unwrap_or(u64::MAX))
    } else {
        let persi = distanza.saturating_neg().saturating_mul(frequenza) / MILIARDO;
        Cade::Ritardo(u64::try_from(persi).unwrap_or(u64::MAX))
    }
}

/// I nanosecondi di tanti fotogrammi, alla frequenza d'uscita.
#[expect(
    clippy::integer_division,
    reason = "il resto è meno di un nanosecondo per fotogramma: l'orologio che \
              lo deve confrontare va a passi di cento"
)]
fn ns_di_fotogrammi(fotogrammi: u64, frequenza: u32) -> u64 {
    let ns = u128::from(fotogrammi).saturating_mul(1_000_000_000) / u128::from(frequenza.max(1));
    u64::try_from(ns).unwrap_or(u64::MAX)
}

/// L'ancora di un blocco il cui primo campione d'anello è uscito al fotogramma
/// `k` ed era il fotogramma d'anello `contenuto`. `None` senza frequenza.
///
/// # Quando la misura si butta
///
/// Con la stessa regola di `Contesto::ritardo_fotogrammi`: un anticipo oltre
/// [`LATENZA_MASSIMA_MS`] non è un buffer lento, è un orologio che ha detto una
/// sciocchezza. Allora l'istante di partenza è quello della callback — il
/// `playback` meno l'anticipo — e l'ancora lo dice, perché chi la legge sappia
/// che la latenza del buffer non c'è dentro.
pub(super) fn ancora_del_blocco(
    battito: &Battito,
    k: usize,
    contenuto: u64,
    epoca: u32,
    in_corso: bool,
) -> Option<AncoraGrezza> {
    if battito.frequenza == 0 {
        return None;
    }
    let tetto = LATENZA_MASSIMA_MS.saturating_mul(1_000_000);
    let scartata = battito.anticipo_ns > tetto;
    let (base, controprova) = if scartata {
        (
            battito.playback_ns.saturating_sub(battito.anticipo_ns),
            battito.ingresso_ns,
        )
    } else {
        (
            battito.playback_ns,
            battito.ingresso_ns.saturating_add(battito.anticipo_ns),
        )
    };
    let dopo = ns_di_fotogrammi(u64::try_from(k).unwrap_or(u64::MAX), battito.frequenza);
    Some(AncoraGrezza {
        contenuto,
        udito_ns: base.saturating_add(dopo),
        anticipo_ns: battito.anticipo_ns,
        controprova_ns: controprova.saturating_add(dopo),
        epoca,
        in_corso,
        latenza_scartata: scartata,
    })
}

/// Il gradino è stato eseguito: lo si conta, e la porta si riapre.
fn fatto(condiviso: &Condiviso) -> Fase {
    condiviso.gradini_fatti.fetch_add(1, Ordering::Relaxed);
    Fase::Riapre
}

/// La porta si è riaperta del tutto: non c'è più niente in correzione — a meno
/// che nel frattempo non ne sia stata chiesta un'altra.
///
/// L'ordine conta: prima si abbassa, poi si guarda se c'è altro. Chi chiede un
/// gradino scrive la richiesta **prima** di alzare la bandiera, quindi o la
/// richiesta si vede qui, o la sua bandiera arriva dopo questa scrittura.
fn libera(condiviso: &Condiviso) {
    condiviso.in_correzione.store(false, Ordering::SeqCst);
    if condiviso.gradino.load(Ordering::SeqCst) != 0 {
        condiviso.in_correzione.store(true, Ordering::SeqCst);
    }
}

/// Fa cadere il gradino in corso e quello chiesto: vedi [`Regia::del_blocco`].
///
/// Legge prima di scrivere: a riposo, con niente da far cadere, non scrive
/// niente.
fn lascia_cadere_il_gradino(andamento: &mut Andamento, condiviso: &Condiviso) {
    andamento.fase = Fase::Libera;
    if condiviso.gradino.load(Ordering::Relaxed) != 0 {
        condiviso.gradino.store(0, Ordering::Release);
    }
    if condiviso.in_correzione.load(Ordering::Relaxed) {
        condiviso.in_correzione.store(false, Ordering::Release);
    }
}

impl Regia {
    /// Le decisioni del blocco, prese una volta all'inizio.
    ///
    /// A riposo — avvio a zero, nessun gradino, deriva a zero — legge quattro
    /// atomici e non scrive niente.
    pub(super) fn del_blocco(
        voce: &mut Voce,
        condiviso: &Condiviso,
        battito: &Battito,
        tace: bool,
        canali: usize,
    ) -> Self {
        let mut regia = Self {
            attesa: None,
            avvio: 0,
            ppm: 0,
            canali,
            primo: None,
            storto: false,
            saltati: 0,
            saltati_prima: 0,
            cornice: [0.0; CANALI_DERIVA],
        };
        if tace {
            // Una pausa o uno svuotamento fanno cadere il gradino, in corso o
            // chiesto: era calcolato su un tempo che non c'è più, e chi
            // l'aveva chiesto lo rifarà sulla musica che ripartirà. La porta
            // si chiude lo stesso, per la pausa.
            lascia_cadere_il_gradino(&mut voce.andamento, condiviso);
            return regia;
        }

        let avvio = condiviso.avvio_ns.load(Ordering::Acquire);
        if avvio != 0 && battito.frequenza > 0 {
            match dove_cade(avvio, battito.playback_ns, battito.frequenza) {
                Cade::Fra(k) => {
                    regia.attesa = Some(usize::try_from(k).unwrap_or(usize::MAX));
                    regia.avvio = avvio;
                }
                Cade::Ritardo(persi) => {
                    let _ = condiviso.avvio_ns.compare_exchange(
                        avvio,
                        0,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    );
                    // In ritardo e a porta chiusa: si parte adesso, e la musica
                    // che doveva già essere uscita si salta — un gradino come
                    // gli altri, che la porta chiusa fa partire subito. A
                    // porta aperta invece la musica stava già andando, e un
                    // avvio nel passato non ha niente da far ripartire.
                    if persi > 0 && voce.andamento.porta == 0 {
                        condiviso
                            .gradino
                            .fetch_sub(i64::try_from(persi).unwrap_or(i64::MAX), Ordering::AcqRel);
                        condiviso.in_correzione.store(true, Ordering::Release);
                    }
                }
            }
        }

        // Un gradino si prende solo a fase libera e senza un avvio da
        // aspettare: la porta chiusa del gradino consumerebbe musica proprio
        // mentre l'avvio la tiene ferma.
        if regia.attesa.is_none()
            && voce.andamento.fase == Fase::Libera
            && condiviso.gradino.load(Ordering::Acquire) != 0
        {
            let chiesto = condiviso.gradino.swap(0, Ordering::AcqRel);
            if chiesto != 0 {
                voce.andamento.fase = Fase::Chiude { quanto: chiesto };
                condiviso.in_correzione.store(true, Ordering::Release);
            }
        }

        let ppm = condiviso.deriva_ppm.load(Ordering::Relaxed);
        if ppm != 0 && canali <= CANALI_DERIVA {
            regia.ppm = i64::from(ppm.clamp(-DERIVA_MASSIMA_PPM, DERIVA_MASSIMA_PPM));
        }
        regia
    }

    /// Il primo campione di un fotogramma: come esce tutto il fotogramma.
    pub(super) fn fotogramma(
        &mut self,
        voce: &mut Voce,
        condiviso: &Condiviso,
        numero: usize,
        tace: bool,
        allineato: bool,
        presi: &mut u64,
    ) -> Modo {
        let aspetta = self.aspetta(condiviso, numero);
        if tace || aspetta {
            // La pausa di sempre: la porta si chiude, e chiusa tace.
            voce.andamento.muovi_porta(false);
            return if voce.andamento.porta == 0 {
                Modo::Zero
            } else {
                Modo::Anello
            };
        }
        match voce.andamento.fase {
            Fase::Libera => {
                voce.andamento.muovi_porta(true);
                self.forse_deriva(voce, allineato, presi)
            }
            Fase::Chiude { quanto } => {
                voce.andamento.muovi_porta(false);
                if voce.andamento.porta > 0 {
                    return Modo::Anello;
                }
                // La porta si è chiusa adesso. Il silenzio comincia dal
                // fotogramma dopo, e questo consuma a guadagno zero: vedi
                // l'esattezza in [`Fase`].
                if quanto > 0 {
                    voce.andamento.fase = Fase::Esegue { resto: quanto };
                    return Modo::Anello;
                }
                self.salta_o_aspetta(voce, condiviso, quanto.unsigned_abs(), allineato)
            }
            Fase::Esegue { resto } => {
                if resto > 0 {
                    let resto = resto - 1;
                    voce.andamento.fase = if resto == 0 {
                        fatto(condiviso)
                    } else {
                        Fase::Esegue { resto }
                    };
                    return Modo::Zero;
                }
                self.salta_o_aspetta(voce, condiviso, resto.unsigned_abs(), allineato)
            }
            Fase::Riapre => {
                voce.andamento.muovi_porta(true);
                if voce.andamento.porta >= voce.andamento.corsa {
                    voce.andamento.fase = Fase::Libera;
                    libera(condiviso);
                }
                Modo::Anello
            }
        }
    }

    /// Il fotogramma `numero` deve ancora tacere per l'avvio programmato. Al
    /// fotogramma dell'avvio l'attesa finisce, e la casella si azzera.
    fn aspetta(&mut self, condiviso: &Condiviso, numero: usize) -> bool {
        match self.attesa {
            Some(k) if numero < k => true,
            Some(_) => {
                self.attesa = None;
                let _ = condiviso.avvio_ns.compare_exchange(
                    self.avvio,
                    0,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );
                false
            }
            None => false,
        }
    }

    /// A porta chiusa, salta quel che manca di un gradino all'indietro.
    ///
    /// Fatto tutto, il fotogramma consuma a guadagno zero e la porta si
    /// riapre; se l'anello non ne aveva abbastanza, il resto resta in sospeso e
    /// il fotogramma tace.
    fn salta_o_aspetta(
        &mut self,
        voce: &mut Voce,
        condiviso: &Condiviso,
        quanti: u64,
        allineato: bool,
    ) -> Modo {
        let mancano = self.salta(voce, quanti, allineato);
        if mancano == 0 {
            voce.andamento.fase = fatto(condiviso);
            return Modo::Anello;
        }
        voce.andamento.fase = Fase::Esegue {
            resto: 0i64.saturating_sub_unsigned(mancano),
        };
        Modo::Zero
    }

    /// Butta fino a `quanti` fotogrammi interi dall'anello, contandoli.
    /// Restituisce quanti ne mancano.
    ///
    /// Un pezzo letto e liberato in un colpo solo: per dei `f32` liberare non
    /// ha niente da distruggere, e il salto costa quanto spostare un indice —
    /// non mille `pop`. Solo a fotogramma intero, perché saltare da metà
    /// fotogramma scambierebbe i canali per sempre.
    #[expect(
        clippy::integer_division,
        reason = "i fotogrammi interi che l'anello contiene: quel che avanza è un \
                  fotogramma a metà, e quello non si salta"
    )]
    fn salta(&mut self, voce: &mut Voce, quanti: u64, allineato: bool) -> u64 {
        if !allineato || quanti == 0 {
            return quanti;
        }
        let canali = self.canali.max(1);
        let interi = u64::try_from(voce.lettore.slots() / canali).unwrap_or(0);
        let fatti = quanti.min(interi);
        if fatti == 0 {
            return quanti;
        }
        let campioni = usize::try_from(fatti).unwrap_or(0).saturating_mul(canali);
        let Ok(pezzo) = voce.lettore.read_chunk(campioni) else {
            return quanti;
        };
        pezzo.commit_all();
        self.saltati = self
            .saltati
            .saturating_add(u64::try_from(campioni).unwrap_or(0));
        if self.primo.is_none() {
            self.saltati_prima = self.saltati_prima.saturating_add(fatti);
        }
        quanti - fatti
    }

    /// Un fotogramma a porta tutta aperta: se la deriva ha accumulato un
    /// fotogramma intero, lo toglie o lo aggiunge qui.
    ///
    /// Mai sul primo fotogramma di musica del blocco: è quello dell'ancora, e
    /// deve essere esattamente il fotogramma d'anello che il conteggio dice.
    fn forse_deriva(&mut self, voce: &mut Voce, allineato: bool, presi: &mut u64) -> Modo {
        if self.ppm == 0
            || self.primo.is_none()
            || !allineato
            || voce.andamento.porta < voce.andamento.corsa
        {
            return Modo::Anello;
        }
        voce.andamento.accumulo = voce.andamento.accumulo.saturating_add(self.ppm);
        let canali = self.canali;
        if voce.andamento.accumulo >= MILIONE {
            // Avanti: due fotogrammi escono come uno, la loro media. Se
            // l'anello non li ha si riprova al fotogramma dopo, con l'accumulo
            // ancora pieno.
            let Ok(pezzo) = voce.lettore.read_chunk(canali.saturating_mul(2)) else {
                return Modo::Anello;
            };
            let (a, b) = pezzo.as_slices();
            let mut tutti = a.iter().chain(b).copied();
            for posto in self.cornice.iter_mut().take(canali) {
                *posto = tutti.next().unwrap_or(0.0);
            }
            for posto in self.cornice.iter_mut().take(canali) {
                *posto = (*posto + tutti.next().unwrap_or(0.0)) * 0.5;
            }
            pezzo.commit_all();
            voce.andamento.accumulo -= MILIONE;
            let due = u64::try_from(canali.saturating_mul(2)).unwrap_or(0);
            *presi = presi.saturating_add(due);
            return Modo::Cornice;
        }
        if voce.andamento.accumulo <= -MILIONE {
            // Indietro: fra l'ultimo uscito e il prossimo si mette la loro
            // media. Il prossimo si **guarda** senza consumarlo — un pezzo
            // letto e non liberato — e uscirà al fotogramma dopo.
            let Ok(pezzo) = voce.lettore.read_chunk(canali) else {
                return Modo::Anello;
            };
            let (a, b) = pezzo.as_slices();
            for ((posto, prima), dopo) in self
                .cornice
                .iter_mut()
                .zip(voce.andamento.ultimo.iter())
                .zip(a.iter().chain(b))
            {
                *posto = (*prima + *dopo) * 0.5;
            }
            voce.andamento.accumulo += MILIONE;
            return Modo::Cornice;
        }
        Modo::Anello
    }

    /// Un campione è uscito dall'anello: annota il primo del blocco, e ricorda
    /// com'era per la deriva.
    pub(super) fn preso(
        &mut self,
        andamento: &mut Andamento,
        indice: usize,
        canale: usize,
        campione: f32,
    ) {
        if self.primo.is_none() && !self.storto {
            if canale == 0 {
                self.primo = Some(fotogramma_di(indice, self.canali));
            } else {
                self.storto = true;
            }
        }
        self.ricorda(andamento, canale, campione);
    }

    /// Ricorda l'ultimo campione uscito di un canale: la prima metà della
    /// media di un fotogramma aggiunto. Solo con la deriva accesa.
    pub(super) fn ricorda(&self, andamento: &mut Andamento, canale: usize, campione: f32) {
        if self.ppm != 0
            && let Some(posto) = andamento.ultimo.get_mut(canale)
        {
            *posto = campione;
        }
    }
}

/// Il fotogramma del blocco a cui appartiene un campione.
#[expect(
    clippy::integer_division,
    reason = "l'indice del fotogramma in un blocco interlacciato: il resto è il \
              canale"
)]
fn fotogramma_di(indice: usize, canali: usize) -> usize {
    indice / canali.max(1)
}
