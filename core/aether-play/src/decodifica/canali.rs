//! Dai canali del file a quelli del dispositivo.
//!
//! # Il difetto che questo modulo chiude
//!
//! Prima c'era una riga sola, `canale.min(da - 1)`: l'uscita numero `n`
//! prendeva il canale numero `n` della sorgente, o l'ultimo se la sorgente ne
//! aveva meno. Fra stereo e stereo non si vedeva niente. Ma un dispositivo a
//! sei o otto canali non è un caso da laboratorio: sono gli impianti 5.1, e
//! soprattutto le **cuffie da gioco col 7.1 virtuale**, che Windows presenta
//! come un'uscita a otto canali. Là un disco stereo mandava il destro al
//! centrale, al subwoofer e a tutti e quattro i surround — cioè la chitarra
//! aperta a destra usciva da sei altoparlanti su otto. E all'inverso un 5.1 su
//! due casse perdeva il centrale, cioè la voce di quasi ogni film e di ogni
//! concerto registrato così.
//!
//! # Come si decide dove va un canale
//!
//! Per **posizione**, non per numero. symphonia dice di ogni sorgente quali
//! canali contiene (`Channels`), e quasi sempre lo dice con una maschera di
//! posizioni (`Position`, la stessa maschera di bit di `WAVEFORMATEXTENSIBLE`,
//! nello stesso ordine per i primi diciotto bit); il dispositivo si descrive
//! con la disposizione normale di Windows per il suo numero di canali — vedi
//! [`disposizione_normale`]. Un canale che esiste da tutte e due le parti va
//! dritto al suo posto; uno che manca nell'uscita passa per un
//! [`ripiego`], che è una tabella di sostituzioni con il loro peso.
//!
//! Quando la sorgente non dà una maschera — canali solo contati, come fa
//! Matroska, o un elenco che non nomina posizioni — vale la disposizione
//! normale per quel numero di canali, che è quel che un file senza maschera
//! intende. Vedi [`posizioni`].
//!
//! Le sostituzioni che contano sono quelle di ITU-R BS.775 per portare un 5.1
//! su due casse: centrale e surround entrano nei due frontali a meno tre
//! decibel, il subwoofer **si lascia fuori** — la norma lo dice esplicitamente,
//! perché il suo contenuto è già nei canali principali e sommarlo darebbe dei
//! bassi gonfi e un rischio di saturazione senza niente in cambio.
//!
//! # Perché si normalizza
//!
//! Perché le somme saturano. `L + 0,707·C + 0,707·Ls` è una riga che vale fino
//! a 2,41 quando i tre canali sono pieni nello stesso istante, e nel
//! virgola-mobile quel 2,41 arriva intatto fino al fondo della catena, dove il
//! dispositivo lo tosa di netto. Dividere tutta la matrice per la riga più
//! pesante garantisce che nessuna uscita superi l'unità: è quel che fa anche
//! `libswresample` per le uscite intere. Si paga in volume — un 5.1 portato su
//! due casse esce più piano di un disco stereo — e il ReplayGain è il posto
//! giusto per riguadagnarlo, non un tosatore.
//!
//! Si normalizza **l'intera** matrice per la riga peggiore, non riga per riga:
//! così il rapporto fra un canale e l'altro, cioè il missaggio, resta quello
//! che era.
//!
//! # Il mono, che è la sola eccezione
//!
//! Una sorgente a un canale solo va nei due frontali **a guadagno pieno**, e
//! solo lì: niente centrale, niente surround. Non a meno tre decibel, che pure
//! sarebbe la legge di panoramica «giusta» per un segnale messo al centro:
//!
//! - è quel che fa un impianto qualunque con un disco mono, e quel che ha
//!   sempre fatto questo lettore. Un disco mono di un'antologia degli anni
//!   Cinquanta non deve uscire tre decibel sotto il brano stereo che lo
//!   precede nella stessa coda;
//! - gli analizzatori che scrivono il ReplayGain 2.0 misurano il mono come
//!   «doppio mono», cioè esattamente così: attenuarlo qui vorrebbe dire
//!   correggere due volte;
//! - `crate::impronta` apre a due canali e fa la media: con il mono duplicato
//!   a pieno guadagno la media restituisce il segnale identico, e le impronte
//!   già salvate nel database restano confrontabili con quelle nuove.
//!
//! Un campione a pieno guadagno su due canali non satura: è lo stesso campione.
//!
//! # L'ordine dei piani
//!
//! È quello che symphonia promette, e dalla 0.6 lo promette per iscritto: con
//! una maschera il piano `i` è l'`i`-esimo bit acceso, l'«ordine canonico»; con
//! un elenco di etichette (`Channels::Custom`) il piano `i` è l'`i`-esima
//! etichetta, in qualunque ordine l'abbia scritta il file. La 0.5 lasciava che
//! qualche codec consegnasse i canali nell'ordine proprio — il 5.1 di Vorbis,
//! per esempio — sotto una maschera che diceva altro; la 0.6 li riordina lei,
//! e il decodificatore Opus di questo crate fa lo stesso (vedi `super::opus`).
//! Qui ci si fida di quella promessa; nella libreria di questo progetto non c'è
//! un file a più di due canali su cui metterla alla prova.

use symphonia::core::audio::{ChannelLabel, Channels, Position};

/// Meno tre decibel: il peso con cui un canale entra in due casse invece che in
/// una, così che la potenza sommata resti quella di prima.
const MENO_TRE_DB: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Quante sostituzioni di fila si accettano prima di lasciar cadere un canale.
///
/// La catena più lunga delle tabelle qui sotto è il centrale posteriore alto
/// su un'uscita mono: alto → centrale posteriore → posteriore → frontale →
/// centrale, quattro passi. Il tetto esiste per un altro motivo: che nessun
/// errore futuro nella tabella possa diventare una ricorsione infinita.
const PROFONDITA_MAX: u8 = 5;

/// La matrice che porta un fotogramma dalla sorgente all'uscita.
///
/// `pesi` è per righe: il peso del canale `i` della sorgente nell'uscita `u`
/// sta in `u * ingressi + i`. Il caso di gran lunga più comune — stereo su
/// stereo — ha una scorciatoia, [`Matrice::identita`], che copia senza
/// moltiplicare.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Matrice {
    ingressi: usize,
    uscite: usize,
    pesi: Vec<f32>,
    identita: bool,
}

impl Matrice {
    /// La matrice per una sorgente di `ingressi` canali, disposti come dice
    /// `disposizione`, verso un dispositivo di `uscite`.
    ///
    /// Una `disposizione` che non nomina le posizioni, o che non conta tanti
    /// canali quanti ne arrivano, non descrive quella sorgente e si scarta:
    /// vale allora la disposizione normale per quel numero, che è quel che un
    /// file senza maschera intende.
    ///
    /// Si costruisce quando la forma della sorgente cambia — all'apertura, e
    /// fra un anello e l'altro di un Ogg concatenato — e mai per pacchetto: è
    /// il posto in cui allocare non costa niente.
    pub(super) fn nuova(disposizione: &Channels, ingressi: usize, uscite: usize) -> Self {
        let mut pesi = vec![0.0; ingressi.saturating_mul(uscite)];
        if ingressi == 0 || uscite == 0 {
            return Self {
                ingressi,
                uscite,
                pesi,
                identita: false,
            };
        }
        let uscita = disposizione_normale(uscite);

        if ingressi == 1 {
            // Il mono: vedi l'intestazione del modulo.
            let frontali = Position::FRONT_LEFT | Position::FRONT_RIGHT;
            match uscita {
                Some(u) if u.contains(frontali) => {
                    for posto in [Position::FRONT_LEFT, Position::FRONT_RIGHT] {
                        if let Some(peso) = pesi.get_mut(indice_in(u, posto)) {
                            *peso = 1.0;
                        }
                    }
                }
                _ => {
                    if let Some(peso) = pesi.first_mut() {
                        *peso = 1.0;
                    }
                }
            }
        } else {
            let sorgente = posizioni(disposizione)
                .filter(|p| p.len() == ingressi)
                .or_else(|| disposizione_normale(ingressi).map(|d| d.iter().collect()));
            match (sorgente, uscita) {
                (Some(s), Some(u)) => {
                    let mut dove = Vec::new();
                    for (canale, posizione) in s.into_iter().enumerate() {
                        dove.clear();
                        destinazioni(posizione, u, 1.0, 0, &mut dove);
                        for (arrivo, peso) in &dove {
                            let riga = indice_in(u, *arrivo);
                            let posto = riga.saturating_mul(ingressi).saturating_add(canale);
                            if let Some(p) = pesi.get_mut(posto) {
                                *p += *peso;
                            }
                        }
                    }
                }
                // Più canali di quanti una maschera ne sappia nominare: non
                // c'è una posizione da rispettare, e l'indice è l'unica cosa
                // che resta. I canali in più si lasciano fuori, quelli che
                // mancano restano muti.
                _ => {
                    for canale in 0..ingressi.min(uscite) {
                        let posto = canale.saturating_mul(ingressi).saturating_add(canale);
                        if let Some(p) = pesi.get_mut(posto) {
                            *p = 1.0;
                        }
                    }
                }
            }
            normalizza(&mut pesi, ingressi);
        }

        let identita = ingressi == uscite
            && pesi.chunks_exact(ingressi).enumerate().all(|(u, riga)| {
                riga.iter()
                    .enumerate()
                    .all(|(i, p)| *p == if i == u { 1.0 } else { 0.0 })
            });
        Self {
            ingressi,
            uscite,
            pesi,
            identita,
        }
    }

    /// Quanti canali si aspetta in ingresso.
    pub(super) const fn ingressi(&self) -> usize {
        self.ingressi
    }

    /// Porta un blocco interlacciato della sorgente in coda a `fuori`.
    ///
    /// Un blocco che finisse a metà di un fotogramma perde il fotogramma
    /// spezzato: sarebbe già rotto a monte, e inventarne i canali mancanti non
    /// lo aggiusterebbe.
    pub(super) fn applica_interlacciato(&self, campioni: &[f32], fuori: &mut Vec<f32>) {
        if self.ingressi == 0 || self.uscite == 0 {
            return;
        }
        if self.identita {
            let intero = campioni
                .len()
                .saturating_sub(campioni.len() % self.ingressi);
            fuori.extend_from_slice(campioni.get(..intero).unwrap_or_default());
            return;
        }
        for fotogramma in campioni.chunks_exact(self.ingressi) {
            self.applica_fotogramma(fotogramma, fuori);
        }
    }

    /// Porta un fotogramma solo in coda a `fuori`.
    pub(super) fn applica_fotogramma(&self, fotogramma: &[f32], fuori: &mut Vec<f32>) {
        if self.ingressi == 0 {
            return;
        }
        if self.identita {
            fuori.extend_from_slice(fotogramma);
            return;
        }
        for riga in self.pesi.chunks_exact(self.ingressi) {
            let valore: f32 = riga.iter().zip(fotogramma).map(|(p, c)| p * c).sum();
            fuori.push(valore);
        }
    }

    /// Il peso del canale `ingresso` nell'uscita `uscita`, per le prove.
    #[cfg(test)]
    fn peso(&self, uscita: usize, ingresso: usize) -> f32 {
        self.pesi
            .get(uscita * self.ingressi + ingresso)
            .copied()
            .unwrap_or(f32::NAN)
    }
}

/// La disposizione che Windows dà a un dispositivo con quel numero di canali.
///
/// # Perché si deve indovinare
///
/// Perché l'uscita non la dichiara. cpal apre il flusso con
/// `KSAUDIO_SPEAKER_DIRECTOUT`, cioè «il canale `n` va all'altoparlante `n`», e
/// l'altoparlante `n` è quello del formato di missaggio del dispositivo, che
/// per ogni numero di canali è la configurazione normale di `ksmedia.h`:
/// `STEREO`, `QUAD`, `5POINT1`, `7POINT1_SURROUND`. Sono queste qui sotto.
///
/// La differenza fra il 5.1 «posteriore» e quello «laterale» non sposta
/// niente: posteriori e laterali si sostituiscono a vicenda a pieno peso nel
/// [`ripiego`], quindi un 5.1 laterale su un'uscita 5.1 posteriore va dritto
/// al suo posto lo stesso.
///
/// Il mono è il centrale, e non il sinistro come lo chiamano alcuni
/// contenitori: su un dispositivo a un canale solo quel canale è in mezzo, ed
/// è lì che una sorgente stereo deve arrivare sommata.
///
/// `None` oltre i canali che una maschera sa nominare: là si va per indice.
pub(super) fn disposizione_normale(canali: usize) -> Option<Position> {
    let fl = Position::FRONT_LEFT;
    let fr = Position::FRONT_RIGHT;
    let fc = Position::FRONT_CENTER;
    let lfe = Position::LFE1;
    let rl = Position::REAR_LEFT;
    let rr = Position::REAR_RIGHT;
    let rc = Position::REAR_CENTER;
    let sl = Position::SIDE_LEFT;
    let sr = Position::SIDE_RIGHT;
    let disposizione = match canali {
        0 => return None,
        1 => fc,
        2 => fl | fr,
        3 => fl | fr | fc,
        4 => fl | fr | rl | rr,
        5 => fl | fr | fc | sl | sr,
        6 => fl | fr | fc | lfe | rl | rr,
        7 => fl | fr | fc | lfe | rc | sl | sr,
        8 => fl | fr | fc | lfe | rl | rr | sl | sr,
        // Oltre l'otto non c'è una configurazione di Windows: si prendono i
        // primi bit, nell'ordine di `WAVEFORMATEXTENSIBLE`, finché ce ne sono.
        // Sono ventisei, quante le posizioni che symphonia sa nominare.
        n => {
            let bit = u32::try_from(n).ok()?;
            if bit > 26 {
                return None;
            }
            Position::from_bits_truncate(1_u64.checked_shl(bit)?.wrapping_sub(1))
        }
    };
    Some(disposizione)
}

/// Le posizioni dei canali di una sorgente, piano per piano, quando la
/// sorgente le dice.
///
/// Una maschera le dice nell'ordine canonico; un elenco di etichette le dice
/// nell'ordine proprio, ed è quello dei piani — vedi l'intestazione del
/// modulo. Tutto il resto non nomina posizioni, e allora `None`: canali solo
/// contati (Matroska, gli AIFF oltre i tre canali), le componenti di un
/// ambisonico, o un elenco in cui anche una sola etichetta non è una posizione
/// — mezza disposizione non è una disposizione.
fn posizioni(disposizione: &Channels) -> Option<Vec<Position>> {
    match disposizione {
        Channels::Positioned(maschera) => Some(maschera.iter().collect()),
        Channels::Custom(etichette) => etichette
            .iter()
            .map(|etichetta| match etichetta {
                ChannelLabel::Positioned(p) if p.bits().count_ones() == 1 => Some(*p),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// La posizione di un canale nell'ordine dei piani: quanti bit accesi lo
/// precedono.
fn indice_in(disposizione: Position, canale: Position) -> usize {
    let prima = disposizione.bits() & canale.bits().wrapping_sub(1);
    usize::try_from(prima.count_ones()).unwrap_or(usize::MAX)
}

/// Dove va un canale su un'uscita che lo ha, o che non lo ha.
///
/// Accumula in `fuori` le coppie (posizione dell'uscita, peso). Un canale che
/// l'uscita ha va dritto; uno che non ha passa per il [`ripiego`], e ogni
/// sostituzione moltiplica il proprio peso — così il centrale posteriore su due
/// casse arriva a metà ampiezza per ciascun lato: meno tre decibel per
/// diventare due posteriori, meno tre per diventare frontali.
fn destinazioni(
    canale: Position,
    uscita: Position,
    peso: f32,
    profondita: u8,
    fuori: &mut Vec<(Position, f32)>,
) {
    if uscita.contains(canale) {
        fuori.push((canale, peso));
        return;
    }
    if profondita >= PROFONDITA_MAX {
        return;
    }
    for (sostituto, fattore) in ripiego(canale, uscita) {
        destinazioni(sostituto, uscita, peso * fattore, profondita + 1, fuori);
    }
}

/// Con che cosa si sostituisce un canale che l'uscita non ha.
///
/// La tabella è quella di ITU-R BS.775 per il caso che conta — 5.1 su due
/// casse: centrale e surround nei frontali a meno tre decibel, subwoofer fuori
/// — allargata agli altri canali di `Position` con lo stesso criterio che usa
/// `libswresample`: un canale «in mezzo» si divide fra i suoi due vicini a meno
/// tre decibel, uno «accanto» prende il posto del vicino a pieno peso, uno
/// «in alto» scende al piano di sotto a meno tre decibel — e, con le posizioni
/// del 22.2 che la 0.6 di symphonia sa nominare, uno «in basso» sale al piano
/// di sopra allo stesso modo.
///
/// Posteriori e laterali sono lo stesso ruolo — il surround — e si sostituiscono
/// a vicenda a pieno peso: è ciò che rende uguali per l'orecchio il 5.1
/// «posteriore» e quello «laterale».
fn ripiego(canale: Position, uscita: Position) -> Vec<(Position, f32)> {
    let c = canale;
    let intero = 1.0;
    let meta = MENO_TRE_DB;
    // Il sostituto di un surround: il suo gemello se c'è, altrimenti il
    // frontale dello stesso lato.
    let surround = |gemello: Position, frontale: Position| {
        if uscita.contains(gemello) {
            vec![(gemello, intero)]
        } else {
            vec![(frontale, meta)]
        }
    };
    if c == Position::FRONT_LEFT || c == Position::FRONT_RIGHT {
        vec![(Position::FRONT_CENTER, meta)]
    } else if c == Position::FRONT_CENTER {
        vec![(Position::FRONT_LEFT, meta), (Position::FRONT_RIGHT, meta)]
    } else if c == Position::LFE1 {
        // BS.775: il subwoofer non entra nel missaggio a meno canali.
        Vec::new()
    } else if c == Position::LFE2 {
        vec![(Position::LFE1, intero)]
    } else if c == Position::REAR_LEFT {
        surround(Position::SIDE_LEFT, Position::FRONT_LEFT)
    } else if c == Position::REAR_RIGHT {
        surround(Position::SIDE_RIGHT, Position::FRONT_RIGHT)
    } else if c == Position::SIDE_LEFT {
        surround(Position::REAR_LEFT, Position::FRONT_LEFT)
    } else if c == Position::SIDE_RIGHT {
        surround(Position::REAR_RIGHT, Position::FRONT_RIGHT)
    } else if c == Position::REAR_CENTER {
        vec![(Position::REAR_LEFT, meta), (Position::REAR_RIGHT, meta)]
    } else if c == Position::FRONT_LEFT_CENTER || c == Position::FRONT_LEFT_WIDE {
        vec![(Position::FRONT_LEFT, intero)]
    } else if c == Position::FRONT_RIGHT_CENTER || c == Position::FRONT_RIGHT_WIDE {
        vec![(Position::FRONT_RIGHT, intero)]
    } else if c == Position::TOP_FRONT_LEFT || c == Position::BOTTOM_FRONT_LEFT {
        vec![(Position::FRONT_LEFT, meta)]
    } else if c == Position::TOP_FRONT_RIGHT || c == Position::BOTTOM_FRONT_RIGHT {
        vec![(Position::FRONT_RIGHT, meta)]
    } else if c == Position::TOP_FRONT_CENTER
        || c == Position::BOTTOM_FRONT_CENTER
        || c == Position::TOP_CENTER
    {
        vec![(Position::FRONT_CENTER, meta)]
    } else if c == Position::TOP_SIDE_LEFT {
        vec![(Position::SIDE_LEFT, meta)]
    } else if c == Position::TOP_SIDE_RIGHT {
        vec![(Position::SIDE_RIGHT, meta)]
    } else if c == Position::TOP_REAR_LEFT {
        vec![(Position::REAR_LEFT, meta)]
    } else if c == Position::TOP_REAR_RIGHT {
        vec![(Position::REAR_RIGHT, meta)]
    } else if c == Position::TOP_REAR_CENTER {
        vec![(Position::REAR_CENTER, meta)]
    } else {
        Vec::new()
    }
}

/// Divide tutta la matrice per la riga più pesante, se supera l'unità.
///
/// Vedi l'intestazione del modulo per il perché dell'intera matrice e non riga
/// per riga.
fn normalizza(pesi: &mut [f32], ingressi: usize) {
    if ingressi == 0 {
        return;
    }
    let peggiore = pesi
        .chunks_exact(ingressi)
        .map(|riga| riga.iter().map(|p| p.abs()).sum::<f32>())
        .fold(0.0_f32, f32::max);
    // Un soffio sopra l'unità, perché `0,707 + 0,707` in virgola mobile non è
    // esattamente `1,414` e una riga che vale uno per costruzione non deve
    // passare per una divisione.
    if peggiore > 1.0 + 1e-6 {
        for peso in pesi.iter_mut() {
            *peso /= peggiore;
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    const FL: Position = Position::FRONT_LEFT;
    const FR: Position = Position::FRONT_RIGHT;
    const FC: Position = Position::FRONT_CENTER;
    const LFE: Position = Position::LFE1;
    const RL: Position = Position::REAR_LEFT;
    const RR: Position = Position::REAR_RIGHT;
    const SL: Position = Position::SIDE_LEFT;
    const SR: Position = Position::SIDE_RIGHT;

    /// Una sorgente che dice le proprie posizioni con una maschera, che è il
    /// caso di quasi tutti i contenitori.
    fn maschera(posizioni: Position) -> Channels {
        Channels::Positioned(posizioni)
    }

    fn quasi(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    /// Quel che esce da un fotogramma solo.
    fn fotogramma(m: &Matrice, dentro: &[f32]) -> Vec<f32> {
        let mut fuori = Vec::new();
        m.applica_fotogramma(dentro, &mut fuori);
        fuori
    }

    #[test]
    fn il_mono_esce_dai_due_frontali_a_pieno_guadagno() {
        let m = Matrice::nuova(&maschera(FL), 1, 2);
        let mut fuori = Vec::new();
        m.applica_interlacciato(&[0.5, -0.5], &mut fuori);
        assert_eq!(fuori, vec![0.5, 0.5, -0.5, -0.5]);

        // Anche quando symphonia lo chiama centrale, com'è per qualche codec:
        // un canale solo è mono, qualunque nome porti.
        let m = Matrice::nuova(&maschera(FC), 1, 2);
        assert_eq!(fotogramma(&m, &[0.25]), vec![0.25, 0.25]);
    }

    #[test]
    fn il_mono_su_un_5_1_resta_nei_frontali() {
        // Niente centrale, niente surround: il mono di un disco del 1956 non
        // deve arrivare dal subwoofer.
        let m = Matrice::nuova(&maschera(FL), 1, 6);
        assert_eq!(fotogramma(&m, &[0.5]), vec![0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
        let m = Matrice::nuova(&Channels::None, 1, 8);
        assert_eq!(
            fotogramma(&m, &[0.5]),
            vec![0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn stereo_su_stereo_non_tocca_niente() {
        let m = Matrice::nuova(&maschera(FL | FR), 2, 2);
        assert!(m.identita);
        let mut fuori = Vec::new();
        m.applica_interlacciato(&[1.0, 2.0, 3.0, 4.0], &mut fuori);
        assert_eq!(fuori, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn lo_stereo_su_un_5_1_resta_davanti() {
        // Il difetto che ha fatto nascere questo modulo: il destro finiva nel
        // centrale, nel subwoofer e nei due surround.
        let m = Matrice::nuova(&maschera(FL | FR), 2, 6);
        assert_eq!(
            fotogramma(&m, &[0.3, -0.7]),
            vec![0.3, -0.7, 0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn lo_stereo_su_una_cuffia_7_1_resta_davanti() {
        // Le cuffie da gioco col 7.1 virtuale: otto canali per Windows, e il
        // posto in cui il difetto si sentiva di più.
        let m = Matrice::nuova(&maschera(FL | FR), 2, 8);
        assert_eq!(
            fotogramma(&m, &[0.3, -0.7]),
            vec![0.3, -0.7, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        // E senza maschera dichiarata, come per un WAV senza estensione:
        // due canali sono uno stereo.
        let m = Matrice::nuova(&Channels::None, 2, 8);
        assert_eq!(
            fotogramma(&m, &[0.3, -0.7]),
            vec![0.3, -0.7, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn il_5_1_su_due_casse_segue_bs_775() {
        // L R C LFE Ls Rs, nell'ordine dei bit.
        let m = Matrice::nuova(&maschera(FL | FR | FC | LFE | RL | RR), 6, 2);
        // BS.775: Lo = L + 0,707 C + 0,707 Ls, Ro = R + 0,707 C + 0,707 Rs,
        // poi tutto diviso per la riga più pesante, 1 + √2.
        let k = 1.0 + std::f32::consts::SQRT_2;
        let atteso = [
            // (uscita, ingresso, peso)
            (0, 0, 1.0 / k),
            (1, 0, 0.0),
            (0, 1, 0.0),
            (1, 1, 1.0 / k),
            (0, 2, MENO_TRE_DB / k),
            (1, 2, MENO_TRE_DB / k),
            (0, 3, 0.0),
            (1, 3, 0.0),
            (0, 4, MENO_TRE_DB / k),
            (1, 4, 0.0),
            (0, 5, 0.0),
            (1, 5, MENO_TRE_DB / k),
        ];
        for (u, i, p) in atteso {
            assert!(
                quasi(m.peso(u, i), p),
                "peso dell'ingresso {i} nell'uscita {u}: {} invece di {p}",
                m.peso(u, i)
            );
        }
        // Il caso peggiore — tutti i canali al massimo nello stesso istante —
        // non esce dall'unità: è la ragione della normalizzazione.
        let pieno = fotogramma(&m, &[1.0; 6]);
        assert!(pieno.iter().all(|c| *c <= 1.0 + 1e-6), "satura: {pieno:?}");
        assert!(quasi(pieno[0], 1.0) && quasi(pieno[1], 1.0));
    }

    #[test]
    fn il_5_1_laterale_su_due_casse_e_lo_stesso() {
        // Il 5.1 «laterale», che è come lo scrivono AAC e ffmpeg: stessi pesi
        // del posteriore, perché per l'orecchio è lo stesso surround.
        let m = Matrice::nuova(&maschera(FL | FR | FC | LFE | SL | SR), 6, 2);
        let k = 1.0 + std::f32::consts::SQRT_2;
        assert!(quasi(m.peso(0, 4), MENO_TRE_DB / k));
        assert!(quasi(m.peso(1, 5), MENO_TRE_DB / k));
        assert!(quasi(m.peso(0, 3), 0.0), "il subwoofer resta fuori");
    }

    #[test]
    fn il_7_1_su_due_casse_non_satura() {
        let m = Matrice::nuova(&maschera(FL | FR | FC | LFE | RL | RR | SL | SR), 8, 2);
        let pieno = fotogramma(&m, &[1.0; 8]);
        assert!(pieno.iter().all(|c| *c <= 1.0 + 1e-6), "satura: {pieno:?}");
        // I due lati restano separati: il surround sinistro non va a destra.
        assert!(quasi(m.peso(1, 4), 0.0) && quasi(m.peso(1, 6), 0.0));
        assert!(m.peso(0, 4) > 0.0 && m.peso(0, 6) > 0.0);
    }

    #[test]
    fn un_5_1_laterale_su_un_dispositivo_5_1_va_dritto() {
        // Il dispositivo a sei canali è `5POINT1` (posteriori), il file è
        // laterale: ciascun canale va al suo posto, e la matrice è l'identità.
        let m = Matrice::nuova(&maschera(FL | FR | FC | LFE | SL | SR), 6, 6);
        assert!(m.identita, "{m:?}");
    }

    #[test]
    fn un_quadrifonico_su_un_5_1_va_per_posizione() {
        // Per numero, il posteriore sinistro sarebbe finito nel centrale.
        let m = Matrice::nuova(&maschera(FL | FR | RL | RR), 4, 6);
        assert_eq!(
            fotogramma(&m, &[0.1, 0.2, 0.3, 0.4]),
            vec![0.1, 0.2, 0.0, 0.0, 0.3, 0.4]
        );
    }

    #[test]
    fn lo_stereo_su_un_canale_solo_e_la_media() {
        // Prima si prendeva il sinistro e basta: `crate::attacchi` apre a un
        // canale, e un disco con la voce a destra spariva.
        let m = Matrice::nuova(&maschera(FL | FR), 2, 1);
        assert_eq!(fotogramma(&m, &[0.8, 0.2]), vec![0.5]);
        assert_eq!(fotogramma(&m, &[1.0, 1.0]), vec![1.0]);
    }

    #[test]
    fn il_5_1_su_un_canale_solo_segue_le_proporzioni_bs_775() {
        // BS.775 per 3/2 → 1/0: 0,707 L + 0,707 R + C + 0,5 Ls + 0,5 Rs.
        let m = Matrice::nuova(&maschera(FL | FR | FC | LFE | RL | RR), 6, 1);
        let c = m.peso(0, 2);
        assert!(quasi(m.peso(0, 0) / c, MENO_TRE_DB));
        assert!(quasi(m.peso(0, 4) / c, 0.5));
        assert!(quasi(m.peso(0, 3), 0.0));
        let pieno = fotogramma(&m, &[1.0; 6]);
        assert!(quasi(pieno[0], 1.0), "{pieno:?}");
    }

    #[test]
    fn una_maschera_che_non_conta_i_canali_si_scarta() {
        // Tre canali dichiarati come stereo: la maschera non descrive quella
        // sorgente, e vale la disposizione normale a tre (FL FR FC).
        let m = Matrice::nuova(&maschera(FL | FR), 3, 2);
        assert!(quasi(m.peso(0, 2), m.peso(1, 2)) && m.peso(0, 2) > 0.0);
    }

    #[test]
    fn oltre_le_maschere_si_va_per_indice() {
        let m = Matrice::nuova(&Channels::None, 30, 2);
        let mut dentro = vec![0.0; 30];
        dentro[0] = 0.25;
        dentro[1] = 0.75;
        dentro[29] = 1.0;
        assert_eq!(fotogramma(&m, &dentro), vec![0.25, 0.75]);
    }

    #[test]
    fn zero_canali_non_esplode() {
        let mut fuori = Vec::new();
        Matrice::nuova(&Channels::None, 0, 2).applica_interlacciato(&[1.0], &mut fuori);
        Matrice::nuova(&Channels::None, 2, 0).applica_interlacciato(&[1.0, 1.0], &mut fuori);
        assert!(fuori.is_empty());
    }

    #[test]
    fn le_disposizioni_normali_contano_i_loro_canali() {
        for n in 1..=26 {
            let d = disposizione_normale(n).expect("una maschera per ogni numero fino a 26");
            assert_eq!(d.iter().count(), n, "la disposizione a {n} canali");
        }
        assert!(disposizione_normale(0).is_none());
        assert!(disposizione_normale(27).is_none());
    }

    #[test]
    fn un_elenco_in_ordine_proprio_va_per_etichetta() {
        // `Channels::Custom`, com'è per un CAF che descrive i canali uno per
        // uno: il piano `i` è l'`i`-esima etichetta, e qui il centrale viene
        // per primo. Per maschera sarebbe finito nel sinistro.
        let elenco = Channels::Custom(Box::new([
            ChannelLabel::Positioned(FC),
            ChannelLabel::Positioned(FL),
            ChannelLabel::Positioned(FR),
        ]));
        let m = Matrice::nuova(&elenco, 3, 2);
        assert!(quasi(m.peso(0, 0), m.peso(1, 0)) && m.peso(0, 0) > 0.0);
        assert!(m.peso(0, 1) > 0.0 && quasi(m.peso(1, 1), 0.0));
        assert!(m.peso(1, 2) > 0.0 && quasi(m.peso(0, 2), 0.0));
    }

    #[test]
    fn i_canali_solo_contati_valgono_la_disposizione_normale() {
        // Matroska conta i canali e non li nomina: due sono uno stereo, sei
        // sono il 5.1 normale, esattamente come per un file senza maschera.
        let contati = Matrice::nuova(&Channels::Discrete(2), 2, 8);
        assert_eq!(contati, Matrice::nuova(&maschera(FL | FR), 2, 8));
        let contati = Matrice::nuova(&Channels::Discrete(6), 6, 2);
        let normale = disposizione_normale(6).expect("il 5.1 ha una maschera");
        assert_eq!(contati, Matrice::nuova(&maschera(normale), 6, 2));
    }

    #[test]
    fn un_elenco_che_non_nomina_posizioni_si_scarta() {
        // Un'etichetta che non è una posizione, e l'elenco intero non descrive
        // più una disposizione: vale quella normale per quel numero di canali.
        let elenco = Channels::Custom(Box::new([
            ChannelLabel::Positioned(FL),
            ChannelLabel::Discrete(1),
        ]));
        assert_eq!(
            Matrice::nuova(&elenco, 2, 2),
            Matrice::nuova(&maschera(FL | FR), 2, 2)
        );
    }
}
