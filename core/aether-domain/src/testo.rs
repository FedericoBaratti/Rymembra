//! Il testo di un brano, con i tempi o senza.
//!
//! Sta nel dominio per la ragione di sempre: qui non si legge nessun file e non
//! si guarda nessun orologio, quindi tutto quel che segue si prova come una
//! chiamata di funzione. Chi il `.lrc` lo trova sul disco è `aether_app::testi`,
//! chi lo chiede alla rete è `aether_meta::lrclib`, chi lo disegna è la
//! finestra: nessuno dei tre sa interpretare un timestamp, e questa è la
//! ragione per cui l'interpretazione è una sola.
//!
//! # Perché il formato si interpreta qui e non nella finestra
//!
//! Perché sarebbe stato naturale farlo là — un `.lrc` è tre righe di
//! espressione regolare in TypeScript — e sarebbe stato il secondo parser. Il
//! primo serve comunque a chi sceglie fra le risposte del catalogo, a chi
//! decide se un testo aderisce alla durata del file, e a chi il `.lrc` deve
//! **riscriverlo** dopo che qualcuno l'ha sincronizzato a mano. Due parser dello
//! stesso formato divergono su quel che il formato non dice — e quel che l'LRC
//! non dice è quasi tutto.
//!
//! # Quel che il formato non dice
//!
//! Non esiste una specifica dell'LRC: esiste un uso, e trent'anni di file
//! scritti da programmi diversi. Le decisioni prese qui, ognuna motivata dov'è
//! scritta, sono:
//!
//! * i centesimi possono essere una, due o tre cifre, e il separatore decimale
//!   può essere un punto o una virgola;
//! * un `[…]` che non è né un tempo né una chiave conosciuta **è testo** — è il
//!   caso di `[Ritornello]`, che i testi piatti usano davvero;
//! * `[offset:]` non si applica ai tempi, si conserva: applicarlo vorrebbe dire
//!   che riscrivere il file ne cambia i numeri;
//! * i tempi si riordinano, perché un file con le righe fuori ordine esiste e
//!   la ricerca binaria di [`riga_attiva`] ha bisogno che siano in ordine.

use crate::enrich::{VETO_DURATA_SEC, punteggio_durata, scarto_secondi, similarity};

// ── le costanti che si vedono ───────────────────────────────────────────────

/// Quanto prima del suo tempo una riga si accende.
///
/// Centoventi millisecondi. Non è una correzione di sincronia — i tempi di un
/// `.lrc` sono giusti — è il tempo che serve all'occhio per arrivare sulla riga
/// prima che la voce ci arrivi. Senza, la riga si illumina *mentre* la parola è
/// già cominciata, e la sensazione è di un testo che insegue.
///
/// L'avanzamento dentro la riga si calcola invece dal tempo **vero**, quindi in
/// questi centoventi millisecondi la riga è accesa e ferma a zero: si vede dove
/// guardare, e non si vede scorrere niente che non stia scorrendo.
///
/// # Perché centoventi e non centocinquanta
///
/// Erano centocinquanta, e dentro quel numero c'era **due cose mescolate**: il
/// tempo dell'occhio e la latenza d'uscita che nessuno compensava. Adesso il
/// motore compensa la parte che sa misurare — il buffer del dispositivo, una
/// decina di millisecondi, vedi `aether_play::uscita::annota_latenza` — e quella
/// che l'utente dichiara (`audio.latenza_ms`). Quel che la stima di `cpal` non
/// vede resta qui dentro: su un'uscita cablata sono una trentina di
/// millisecondi fra mixer di sistema, driver e conversione.
///
/// Centocinquanta per l'occhio meno quei trenta fa centoventi. È un conto, non
/// una misura: il numero definitivo lo fissa l'autore contando i fotogrammi fra
/// un click udibile e l'accensione della riga, e quando lo farà questa riga va
/// riscritta con quel che ha visto — non cambiata in silenzio.
///
/// # Perché sta qui e non anche nel TypeScript
///
/// Perché era scritto in due posti, a mano, ed è durato finché nessuno ha
/// toccato nessuno dei due: la ricerca della riga accesa esiste sia qui
/// ([`riga_attiva`]) sia nel pannello dei testi, che gira venti volte al secondo
/// e non può attraversare l'IPC per ogni riga. La copia però non serviva al
/// **numero**: ora viaggia nello stato della riproduzione, e la finestra lo legge
/// invece di ripeterlo.
pub const ANTICIPO_MS: i64 = 120;

/// Quanto silenzio in fondo rende sospetto un testo.
///
/// Settantacinque secondi. Una coda strumentale lunga esiste — un finale
/// ripetuto, una traccia nascosta — ma un testo che finisce più di un minuto e
/// un quarto prima della fine del file è, molto più spesso, il testo della
/// versione radio applicato all'estesa.
pub const CODA_SOSPETTA_MS: u64 = 75_000;

/// Di quanto i tempi possono sforare la fine del file prima di essere sbagliati.
///
/// Dieci secondi, la stessa grazia che [`crate::enrich::GRAZIA_MS`] concede alle
/// durate: è il rumore fra due codifiche dello stesso master. Oltre, il testo
/// continua dopo che la canzone è finita, e non c'è nessuna edizione in cui
/// questo sia giusto.
pub const OLTRE_LA_FINE_MS: u64 = 10_000;

/// Quanto deve valere un candidato per essere accettato senza che nessuno guardi.
///
/// Vale la stessa disciplina dell'arricchimento: nel dubbio ci si astiene. Un
/// testo sbagliato è peggio di nessun testo, perché scorre e sembra giusto.
pub const SOGLIA_SCELTA: f64 = 0.66;

/// Quanto devono somigliarsi i titoli perché il resto conti.
///
/// Un veto separato dal punteggio: senza, un artista che coincide e una durata
/// che coincide basterebbero a prendere il testo della traccia accanto nello
/// stesso disco.
pub const SOGLIA_TITOLO: f64 = 0.55;

/// Quanto indietro si cerca un attacco a cui agganciare una battuta.
///
/// Duecentocinquanta millisecondi. La finestra è **asimmetrica** rispetto a
/// [`FINESTRA_DOPO_MS`], e non è una svista: chi batte il tempo a orecchio
/// arriva sempre in ritardo sul suono, mai in anticipo — il suono deve prima
/// raggiungerlo. Una finestra simmetrica aggancerebbe una battuta in ritardo
/// all'attacco *successivo*, che è il difetto peggiore possibile qui, perché
/// sposta la riga di una parola intera invece che di un decimo di secondo.
pub const FINESTRA_PRIMA_MS: i64 = 250;

/// Quanto avanti si cerca un attacco a cui agganciare una battuta.
///
/// Centoventi millisecondi, cioè poco: serve solo a chi anticipa perché *sa*
/// che parola arriva, e non deve essere abbastanza larga da catturare la sillaba
/// dopo.
pub const FINESTRA_DOPO_MS: i64 = 120;

/// Le chiavi che nell'LRC stanno fra parentesi quadre e non sono tempi.
///
/// Elenco chiuso, e non «tutto quel che contiene i due punti»: `[Ritornello: 2]`
/// è testo, e una regola generosa lo mangerebbe. Sono le chiavi che i programmi
/// che scrivono `.lrc` usano davvero.
const CHIAVI: &[&str] = &[
    "al", "ar", "au", "by", "id", "la", "length", "offset", "re", "ti", "tool", "ve",
];

// ── quel che un testo è ─────────────────────────────────────────────────────

/// Una parola con il suo tempo, nell'LRC esteso.
///
/// Il testo si conserva **così com'è**, con gli spazi ai bordi: sono quelli che
/// separano una parola dalla successiva quando la riga si ricompone, e
/// rifilarli qui vorrebbe dire riattaccare le parole fra loro.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parola {
    /// Quando comincia, in millisecondi dall'inizio del brano.
    pub ms: u32,
    /// Il pezzo di riga che le appartiene.
    pub testo: String,
}

/// Una riga di testo con il suo tempo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Riga {
    /// Quando comincia, in millisecondi dall'inizio del brano.
    pub ms: u32,
    /// La riga, senza i tempi e senza gli spazi ai bordi.
    pub testo: String,
    /// I tempi delle singole parole, se il file li porta. Quasi sempre vuoto.
    pub parole: Vec<Parola>,
}

/// Il testo di un brano, in una delle tre forme in cui può esistere.
///
/// Le tre non sono esclusive per costruzione ma lo sono di fatto: o ci sono le
/// [`Riga`], o c'è il [`piatto`](Self::piatto), o è
/// [`strumentale`](Self::strumentale). La quarta possibilità — nessuna delle tre
/// — è «non si sa ancora», e si distingue perché è [`Testo::vuoto`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Testo {
    /// Le righe con i tempi, in ordine crescente. Vuoto se il testo è piatto.
    pub righe: Vec<Riga>,
    /// Il testo senza tempi, quando i tempi non ci sono.
    pub piatto: Option<String>,
    /// Il brano non ha parole. Non è «non l'ho trovato»: è una risposta.
    pub strumentale: bool,
    /// Lo scarto dichiarato dal file, nel verso dello standard: vedi
    /// [`posizione_corretta`].
    pub offset_ms: i32,
}

impl Testo {
    /// Non si sa niente di questo brano.
    #[must_use]
    pub fn vuoto(&self) -> bool {
        self.righe.is_empty()
            && !self.strumentale
            && self.piatto.as_ref().is_none_or(|p| p.trim().is_empty())
    }

    /// Il testo ha i tempi, quindi si può far scorrere.
    #[must_use]
    pub fn sincronizzato(&self) -> bool {
        !self.righe.is_empty()
    }

    /// Il testo senza i tempi, che ci fossero o no.
    ///
    /// Serve a due cose che sembrano una: mostrare un testo che non scorre, e
    /// dare all'editor di sincronizzazione le righe da cui partire quando quel
    /// che si ha è un `.lrc` di un'altra edizione.
    #[must_use]
    pub fn come_piatto(&self) -> String {
        if self.righe.is_empty() {
            return self.piatto.clone().unwrap_or_default();
        }
        let mut fuori = String::new();
        for riga in &self.righe {
            fuori.push_str(&riga.testo);
            fuori.push('\n');
        }
        fuori
    }
}

/// Quanto un testo aderisce alla durata del file su cui lo si vuole mostrare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aderenza {
    /// I tempi stanno dentro il brano. Si mostra e basta.
    Buona,
    /// Ci sta, ma finisce molto prima. Si mostra, dicendo che è da verificare.
    Sospetta,
    /// I tempi vanno oltre la fine: è il testo di un'altra edizione.
    Fuori,
}

// ── leggere ─────────────────────────────────────────────────────────────────

/// Interpreta un `.lrc`, o un testo piatto, o qualunque cosa ci sia in mezzo.
///
/// Non fallisce mai, e la scelta è deliberata: quel che arriva qui è il tag di
/// un file di qualcun altro o un documento scaricato, e l'unico esito peggiore
/// di un testo interpretato male è una schermata che dice «errore» a chi voleva
/// leggere le parole di una canzone. Le righe che non si capiscono si tengono
/// come testo; quelle che si capiscono prendono il loro tempo.
///
/// ```
/// use aether_domain::testo::leggi;
/// let testo = leggi("[00:12.34]la prima riga\n[00:15.00]la seconda");
/// assert_eq!(testo.righe.len(), 2);
/// assert_eq!(testo.righe.first().map(|r| r.ms), Some(12_340));
/// ```
#[must_use]
pub fn leggi(grezzo: &str) -> Testo {
    let mut righe: Vec<Riga> = Vec::new();
    let mut senza_tempo: Vec<&str> = Vec::new();
    let mut offset_ms = 0_i32;

    // Il BOM sta in testa al file, non in testa a ogni riga: si toglie una volta
    // sola, e `lines()` pensa da sé al CRLF.
    for riga in grezzo.trim_start_matches('\u{FEFF}').lines() {
        let (tag, resto) = separa_tag(riga);
        let mut tempi: Vec<u32> = Vec::new();
        for uno in tag {
            if let Some(ms) = tempo_ms(uno) {
                tempi.push(ms);
                continue;
            }
            if let Some((chiave, valore)) = uno.split_once(':')
                && chiave.trim().eq_ignore_ascii_case("offset")
            {
                offset_ms = valore.trim().parse::<i32>().unwrap_or(0);
            }
        }

        if tempi.is_empty() {
            senza_tempo.push(riga);
            continue;
        }

        let (piatta, parole) = separa_parole(resto);
        let piatta = piatta.trim().to_owned();
        for ms in tempi {
            // Una riga con più tempi è la stessa riga cantata più volte, e ogni
            // volta è una riga sua: fondere i tempi in una sola voce
            // significherebbe accenderla al primo e lasciarla accesa fino
            // all'ultimo.
            righe.push(Riga {
                ms,
                testo: piatta.clone(),
                parole: parole.clone(),
            });
        }
    }

    if righe.is_empty() {
        let piatto = senza_tempo.join("\n").trim().to_owned();
        return Testo {
            righe: Vec::new(),
            piatto: (!piatto.is_empty()).then_some(piatto),
            strumentale: false,
            offset_ms,
        };
    }

    // Stabile, e non `sort_unstable`: due righe allo stesso millesimo capitano
    // — un coro sopra una strofa — e l'ordine fra loro è quello del file, che è
    // l'unica informazione che abbiamo su quale vada letta prima.
    righe.sort_by_key(|r| r.ms);
    Testo {
        righe,
        piatto: None,
        strumentale: false,
        offset_ms,
    }
}

/// I contenuti dei `[…]` in testa alla riga, e quel che resta.
///
/// Un gruppo che non è né un tempo né una [chiave conosciuta](CHIAVI) **non** è
/// un tag: la scansione si ferma prima, e `[Ritornello]` resta la riga che è.
fn separa_tag(riga: &str) -> (Vec<&str>, &str) {
    let mut tag: Vec<&str> = Vec::new();
    let mut resto = riga;
    loop {
        let candidato = resto.trim_start();
        let Some(dopo) = candidato.strip_prefix('[') else {
            break;
        };
        let Some(fine) = dopo.find(']') else {
            break;
        };
        let (Some(dentro), Some(avanti)) = (dopo.get(..fine), dopo.get(fine.saturating_add(1)..))
        else {
            break;
        };
        if !e_un_tag(dentro) {
            break;
        }
        tag.push(dentro);
        resto = avanti;
    }
    (tag, resto)
}

/// Il contenuto di un `[…]` è un tag e non testo.
fn e_un_tag(dentro: &str) -> bool {
    if tempo_ms(dentro).is_some() {
        return true;
    }
    dentro
        .split_once(':')
        .is_some_and(|(chiave, _)| CHIAVI.iter().any(|c| chiave.trim().eq_ignore_ascii_case(c)))
}

/// I tempi delle parole dentro una riga, e la riga ricomposta.
///
/// Un `<…>` che non contiene un tempo resta dov'è, per la stessa ragione per cui
/// `[Ritornello]` resta dov'è: in un testo ci sono i segni di punteggiatura, e
/// non si mangiano.
fn separa_parole(testo: &str) -> (String, Vec<Parola>) {
    if !testo.contains('<') {
        return (testo.to_owned(), Vec::new());
    }

    let mut segmenti: Vec<(Option<u32>, String)> = Vec::new();
    let mut corrente: Option<u32> = None;
    let mut pezzo = String::new();
    let mut resto = testo;

    while let Some(apre) = resto.find('<') {
        let (Some(prima), Some(dopo)) = (resto.get(..apre), resto.get(apre.saturating_add(1)..))
        else {
            break;
        };
        let letto = dopo.find('>').and_then(|chiude| {
            let dentro = dopo.get(..chiude)?;
            let avanti = dopo.get(chiude.saturating_add(1)..)?;
            Some((tempo_ms(dentro)?, avanti))
        });
        match letto {
            Some((ms, avanti)) => {
                pezzo.push_str(prima);
                segmenti.push((corrente, std::mem::take(&mut pezzo)));
                corrente = Some(ms);
                resto = avanti;
            }
            None => {
                pezzo.push_str(prima);
                pezzo.push('<');
                resto = dopo;
            }
        }
    }
    pezzo.push_str(resto);
    segmenti.push((corrente, pezzo));

    let piatta: String = segmenti.iter().map(|(_, t)| t.as_str()).collect();
    let parole = segmenti
        .into_iter()
        .filter_map(|(ms, testo)| Some(Parola { ms: ms?, testo }))
        .filter(|p| !p.testo.trim().is_empty())
        .collect();
    (piatta, parole)
}

/// Legge `mm:ss`, `mm:ss.xx`, `mm:ss,xxx` o `hh:mm:ss.xx`.
///
/// # Le cifre della frazione contano
///
/// Due cifre sono centesimi, tre sono millesimi, una è decimi: `[00:01.5]` è un
/// secondo e mezzo, non un secondo e cinque millesimi. Un parser che leggesse la
/// frazione come un intero sbaglierebbe di un fattore cento su ogni file scritto
/// con una cifra sola, e sono i file scritti a mano.
fn tempo_ms(grezzo: &str) -> Option<u32> {
    let grezzo = grezzo.trim();
    let pezzi: Vec<&str> = grezzo.split(':').collect();
    let (ore, minuti, secondi) = match pezzi.as_slice() {
        [m, s] => ("0", *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };

    let ore: u32 = ore.trim().parse().ok()?;
    let minuti: u32 = minuti.trim().parse().ok()?;
    let secondi = secondi.trim();
    let (interi, frazione) = secondi.split_once(['.', ',']).unwrap_or((secondi, ""));
    let interi: u32 = interi.parse().ok()?;
    if interi > 59 && pezzi.len() == 3 {
        return None;
    }

    let mut millesimi = 0_u32;
    for (posizione, c) in frazione.chars().take(3).enumerate() {
        let cifra = c.to_digit(10)?;
        // Il peso a tabella e non con una divisione: la divisione intera è
        // vietata dal workspace, e qui la tabella si legge anche meglio.
        let peso = match posizione {
            0 => 100,
            1 => 10,
            _ => 1,
        };
        millesimi = millesimi.saturating_add(cifra.saturating_mul(peso));
    }

    Some(
        ore.saturating_mul(3_600_000)
            .saturating_add(minuti.saturating_mul(60_000))
            .saturating_add(interi.saturating_mul(1_000))
            .saturating_add(millesimi),
    )
}

// ── scrivere ────────────────────────────────────────────────────────────────

/// Riscrive un `.lrc` che chiunque altro sappia leggere.
///
/// # Perché i centesimi e non i millesimi
///
/// Perché è la forma che tutti leggono, ed è quella che il catalogo usa. I
/// millesimi sarebbero più precisi e li leggono quasi tutti: «quasi» è il
/// problema, su un file che finisce accanto alla musica di qualcuno e viene
/// aperto da programmi che non abbiamo scelto noi.
///
/// La conseguenza va detta: scrivere **arrotonda** al centesimo. Quindi
/// `leggi(scrivi(x)) == x` vale sui tempi allineati al centesimo, e
/// `scrivi(leggi(scrivi(x))) == scrivi(x)` vale sempre — cioè riscrivere un file
/// non lo cambia mai due volte. È la proprietà che conta, ed è provata.
#[must_use]
pub fn scrivi(testo: &Testo) -> String {
    let mut fuori = String::new();
    if testo.offset_ms != 0 {
        fuori.push_str("[offset:");
        if testo.offset_ms > 0 {
            fuori.push('+');
        }
        fuori.push_str(&testo.offset_ms.to_string());
        fuori.push_str("]\n");
    }
    for riga in &testo.righe {
        fuori.push('[');
        fuori.push_str(&scrivi_tempo(riga.ms));
        fuori.push(']');
        if riga.parole.is_empty() {
            fuori.push_str(&riga.testo);
        } else {
            for parola in &riga.parole {
                fuori.push('<');
                fuori.push_str(&scrivi_tempo(parola.ms));
                fuori.push('>');
                fuori.push_str(&parola.testo);
            }
        }
        fuori.push('\n');
    }
    fuori
}

/// `mm:ss.xx`, con i minuti che passano tranquillamente il sessanta.
///
/// Niente campo delle ore: un brano di sessantadue minuti si scrive `[62:00.00]`
/// e ogni lettore lo capisce, mentre `[01:02:00.00]` è la forma che i lettori
/// interpretano in due modi diversi.
#[expect(
    clippy::integer_division,
    reason = "un orologio è fatto di divisioni intere: i minuti sono i centesimi diviso seimila, e il resto è il campo dopo"
)]
fn scrivi_tempo(ms: u32) -> String {
    // Cinque millesimi prima di dividere: è l'arrotondamento al centesimo più
    // vicino invece del troncamento, cioè mezzo centesimo di errore massimo
    // invece di uno.
    let centesimi = ms.saturating_add(5) / 10;
    let minuti = centesimi / 6_000;
    let resto = centesimi % 6_000;
    let secondi = resto / 100;
    let frazione = resto % 100;
    format!("{minuti:02}:{secondi:02}.{frazione:02}")
}

// ── mostrare ────────────────────────────────────────────────────────────────

/// La posizione da confrontare con i tempi delle righe.
///
/// # Il verso dello scarto
///
/// Lo standard dice: `+` anticipa il testo, `-` lo ritarda. Cioè il tempo a cui
/// una riga va davvero mostrata è `riga.ms - offset`, e confrontare la posizione
/// con quello equivale a confrontare `posizione + offset` con `riga.ms`. La
/// seconda forma è questa funzione, e si preferisce perché lascia i tempi delle
/// righe intatti: applicare lo scarto ai numeri vorrebbe dire che riscrivere il
/// file lo applica una seconda volta.
///
/// I due scarti si sommano perché sono la stessa grandezza da due mani diverse:
/// quello del file, scritto da chi l'ha sincronizzato, e quello dell'utente, che
/// corregge la differenza fra la sua edizione e quella di allora.
#[must_use]
pub fn posizione_corretta(posizione_ms: i64, offset_ms: i32, scarto_ms: i32) -> i64 {
    posizione_ms
        .saturating_add(i64::from(offset_ms))
        .saturating_add(i64::from(scarto_ms))
}

/// Quale riga è accesa a questa posizione, [`ANTICIPO_MS`] compreso.
///
/// `None` prima della prima riga: è il tempo dell'introduzione, e in quel tempo
/// non c'è niente da illuminare.
///
/// Le righe devono essere in ordine di tempo — [`leggi`] le riordina — perché
/// questa è una ricerca binaria: su un testo di duecento righe interrogato
/// venti volte al secondo, la scansione lineare sarebbe quattromila confronti al
/// secondo per rispondere sempre la stessa cosa.
#[must_use]
pub fn riga_attiva(righe: &[Riga], posizione_ms: i64) -> Option<usize> {
    let soglia = posizione_ms.saturating_add(ANTICIPO_MS);
    righe
        .partition_point(|r| i64::from(r.ms) <= soglia)
        .checked_sub(1)
}

/// Quanto è avanzata la riga accesa, da 0 a 1.
///
/// È quel che fa sembrare sincronizzato alla parola un testo che è
/// sincronizzato alla riga: l'illuminazione attraversa la riga nel tempo che
/// separa il suo tempo da quello della successiva.
///
/// Zero durante l'anticipo, per la ragione scritta in [`ANTICIPO_MS`]. Zero
/// anche sull'ultima riga, che non ha una successiva da cui misurare: preferire
/// «ferma» a «inventata» è la stessa scelta che si fa in tutto questo modulo.
#[must_use]
pub fn avanzamento(righe: &[Riga], indice: usize, posizione_ms: i64) -> f64 {
    let (Some(riga), Some(prossima)) = (righe.get(indice), righe.get(indice.saturating_add(1)))
    else {
        return 0.0;
    };
    let inizio = i64::from(riga.ms);
    let fine = i64::from(prossima.ms);
    let campo = fine.saturating_sub(inizio);
    if campo <= 0 {
        return 0.0;
    }
    let fatto = posizione_ms.saturating_sub(inizio);
    if fatto <= 0 {
        return 0.0;
    }
    if fatto >= campo {
        return 1.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "due tempi dentro un brano stanno abbondantemente nei 53 bit di un f64"
    )]
    let quota = fatto as f64 / campo as f64;
    quota
}

/// Il testo sta dentro il brano su cui lo si vuole mostrare.
///
/// Serve a non far scorrere in silenzio il testo di un'altra edizione. Il caso
/// che questa funzione prende non è il testo sbagliato — quello lo ferma la
/// [scelta](scegli) — ma il testo **giusto** della versione sbagliata: stessa
/// canzone, stesso artista, tre minuti di differenza.
#[must_use]
pub fn verifica_durata(testo: &Testo, durata_ms: u64) -> Aderenza {
    let Some(ultimo) = testo.righe.last().map(|r| u64::from(r.ms)) else {
        return Aderenza::Buona;
    };
    // Durata sconosciuta: non si sa, e «non si sa» non è «è sbagliato».
    if durata_ms == 0 {
        return Aderenza::Buona;
    }
    if ultimo > durata_ms.saturating_add(OLTRE_LA_FINE_MS) {
        return Aderenza::Fuori;
    }
    if durata_ms.saturating_sub(ultimo) > CODA_SOSPETTA_MS {
        return Aderenza::Sospetta;
    }
    Aderenza::Buona
}

// ── scegliere ───────────────────────────────────────────────────────────────

/// Un brano come lo si sta cercando.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cercato<'a> {
    /// Il titolo, dal tag del file.
    pub titolo: &'a str,
    /// L'artista, dal tag del file.
    pub artista: &'a str,
    /// L'album, quando c'è.
    pub album: Option<&'a str>,
    /// La durata del file, in millisecondi.
    pub durata_ms: Option<u64>,
}

/// Una risposta del catalogo, ridotta a quel che serve per sceglierla.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidato {
    /// L'identificativo nel catalogo, per ricordare da dove viene.
    pub id: i64,
    /// Il titolo secondo il catalogo.
    pub titolo: String,
    /// L'artista secondo il catalogo.
    pub artista: String,
    /// L'album secondo il catalogo, quando lo dice.
    pub album: Option<String>,
    /// La durata secondo il catalogo, in millisecondi.
    pub durata_ms: Option<u64>,
    /// Il catalogo dichiara che il brano non ha parole.
    pub strumentale: bool,
    /// Il candidato porta i tempi, non solo il testo.
    pub sincronizzato: bool,
}

/// Quale risposta del catalogo è questo brano, se qualcuna lo è.
///
/// # Perché serve, visto che si chiede per titolo e artista
///
/// Perché la domanda esatta fallisce e si ripiega sulla ricerca, e una ricerca
/// per «Nightcall» risponde con la versione originale, tre remix, due cover e un
/// karaoke. Le sei hanno lo stesso titolo e quasi lo stesso artista: quel che le
/// distingue è la durata, ed è per questo che si preferisce sempre chiedere con
/// la durata in mano.
///
/// # Le regole, in ordine di forza
///
/// 1. **Veto di durata**: oltre [`VETO_DURATA_SEC`] non è lo stesso brano, e
///    nessun punteggio lo recupera. È lo stesso veto dell'arricchimento.
/// 2. **Veto di titolo**: sotto [`SOGLIA_TITOLO`] non lo si guarda nemmeno.
/// 3. **Punteggio**, e sopra [`SOGLIA_SCELTA`] si prende il migliore.
///
/// Il bonus per il sincronizzato non è una preferenza estetica: fra due testi
/// ugualmente probabili, quello con i tempi è più utile e — dettaglio che conta
/// — è stato verificato da qualcuno che l'ha ascoltato riga per riga.
#[must_use]
pub fn scegli(candidati: &[Candidato], cercato: &Cercato<'_>) -> Option<usize> {
    let mut migliore: Option<(usize, f64)> = None;
    for (indice, candidato) in candidati.iter().enumerate() {
        if candidato.strumentale {
            // Uno strumentale è una risposta, ma è una risposta che si accetta
            // solo se il titolo la conferma bene: è anche la risposta che un
            // catalogo dà quando qualcuno ha caricato una voce vuota.
            if similarity(cercato.titolo, &candidato.titolo) < 0.9 {
                continue;
            }
        }
        let fuori_durata = scarto_secondi(cercato.durata_ms, candidato.durata_ms)
            .is_some_and(|scarto| scarto > VETO_DURATA_SEC);
        if fuori_durata {
            continue;
        }
        let titolo = similarity(cercato.titolo, &candidato.titolo);
        if titolo < SOGLIA_TITOLO {
            continue;
        }
        let artista = similarity(cercato.artista, &candidato.artista);
        let durata = punteggio_durata(cercato.durata_ms, candidato.durata_ms);
        let album = match (cercato.album, candidato.album.as_deref()) {
            (Some(a), Some(b)) => similarity(a, b),
            // Mezzo punto quando manca, come fa `punteggio_durata`: un album che
            // non si sa non deve né premiare né punire.
            _ => 0.5,
        };
        let mut punteggio = 0.40 * titolo + 0.25 * artista + 0.25 * durata + 0.10 * album;
        if candidato.sincronizzato {
            punteggio += 0.08;
        }
        if punteggio < SOGLIA_SCELTA {
            continue;
        }
        if migliore.is_none_or(|(_, quanto)| punteggio > quanto) {
            migliore = Some((indice, punteggio));
        }
    }
    migliore.map(|(indice, _)| indice)
}

// ── sincronizzare a mano ────────────────────────────────────────────────────

/// Le parole di una riga ricompongono la riga a cui sono attaccate.
///
/// # Il caso che questa funzione esiste per fermare
///
/// I tempi delle parole si battono su un testo, e quel testo può cambiare fra
/// la battuta e il salvataggio: chi corregge un refuso in una riga già battuta
/// si ritrova con dei tempi che non sanno più a cosa appartengono. Riassegnarli
/// per posizione sarebbe la cosa peggiore possibile — i tempi resterebbero
/// plausibili, il testo pure, e lo sfasamento si vedrebbe solo cantando —
/// quindi quel che non combacia **si butta**, e la riga torna sincronizzata al
/// verso e basta. Perdere i tempi delle parole di una riga è un lavoro da
/// rifare; tenerli sbagliati è un file che mente.
///
/// # La regola, e perché è la concatenazione
///
/// Perché è l'inverso esatto di quel che fa [`leggi`]: `separa_parole` ricava
/// la riga piatta **concatenando** i pezzi, quindi una riga e le sue parole
/// combaciano se e solo se rimetterle in fila la ridà. Contare le parole non
/// basterebbe — «due parole» è vero anche dopo che se ne è cambiata una — e
/// confrontare parola per parola sarebbe la stessa cosa scritta più lunga.
///
/// Il confronto è sui bordi rifilati perché i bordi sono già rifilati da una
/// parte sola: [`Riga::testo`] arriva senza spazi ai lati, mentre
/// [`Parola::testo`] li conserva tutti — è la ragione scritta su [`Parola`].
///
/// Una riga **senza** parole combacia sempre: non c'è niente che possa
/// contraddire.
///
/// ```
/// use aether_domain::testo::{Parola, Riga, parole_combaciano};
/// let riga = Riga {
///     ms: 0,
///     testo: "una due".to_owned(),
///     parole: vec![
///         Parola { ms: 0, testo: "una ".to_owned() },
///         Parola { ms: 500, testo: "due".to_owned() },
///     ],
/// };
/// assert!(parole_combaciano(&riga));
/// ```
#[must_use]
pub fn parole_combaciano(riga: &Riga) -> bool {
    if riga.parole.is_empty() {
        return true;
    }
    let rimesse: String = riga.parole.iter().map(|p| p.testo.as_str()).collect();
    rimesse.trim() == riga.testo.trim()
}

/// Raddrizza delle battute date a orecchio, usando gli attacchi del suono.
///
/// # Il problema che risolve
///
/// Premere un tasto a ogni riga mentre la canzone suona è il modo più veloce di
/// sincronizzare un testo, e produce tempi sistematicamente **in ritardo**: fra
/// il momento in cui l'orecchio sente e quello in cui il dito preme passano
/// centocinquanta-trecento millisecondi, che è precisamente l'intervallo in cui
/// un testo si vede «inseguire».
///
/// Correggerlo con un numero fisso non funziona, perché quel ritardo cambia da
/// persona a persona e cambia con la stanchezza. Correggerlo a mano riga per
/// riga funziona e costa mezz'ora per canzone.
///
/// # Come lo risolve
///
/// Il suono sa già dov'è l'inizio di ogni parola: sono gli **attacchi**, e
/// `aether_play::attacchi` li tira fuori dal file. Allora:
///
/// 1. ogni battuta si aggancia all'attacco più vicino nella finestra
///    [`FINESTRA_PRIMA_MS`]`..=`[`FINESTRA_DOPO_MS`], che è asimmetrica per la
///    ragione scritta là;
/// 2. un attacco vale per **una battuta sola**. Senza questo vincolo, due righe
///    vicine cadrebbero tutt'e due sullo stesso attacco e otterrebbero lo stesso
///    tempo, cioè una riga che non si accende mai;
/// 3. degli scarti (battuta − attacco) si prende la **mediana**, che è la
///    latenza di reazione di quella persona in quel momento — misurata, non
///    indovinata — e si sottrae alle battute che un attacco non l'hanno trovato.
///    La mediana e non la media: basta una battuta data due secondi tardi
///    perché la media diventi inutilizzabile, e in una sessione di
///    sincronizzazione una battuta sbagliata c'è sempre.
///
/// Il risultato è monotono: una battuta non può finire prima di quella prima di
/// lei. Se l'aggancio la porterebbe indietro, resta dov'era la precedente —
/// meglio due righe attaccate che due righe scambiate.
///
/// # Senza attacchi
///
/// Se `attacchi` è vuoto — un file che non si è potuto decodificare, o un brano
/// senza percussioni riconoscibili — le battute tornano com'erano. È la scelta
/// giusta: non si sa di quanto correggere, e correggere di un numero inventato
/// sarebbe peggio del ritardo che si aveva.
#[must_use]
pub fn aggancia(battute: &[u32], attacchi: &[u32]) -> Vec<u32> {
    if attacchi.is_empty() {
        return battute.to_vec();
    }

    // Prima passata: chi trova il suo attacco, e con che scarto.
    let mut agganciate: Vec<Option<u32>> = Vec::with_capacity(battute.len());
    let mut scarti: Vec<i64> = Vec::new();
    let mut primo_libero = 0_usize;

    for battuta in battute {
        let battuta = i64::from(*battuta);
        let mut scelto: Option<(usize, u32, i64)> = None;
        for (indice, attacco) in attacchi.iter().enumerate().skip(primo_libero) {
            let attacco_ms = i64::from(*attacco);
            if attacco_ms < battuta.saturating_sub(FINESTRA_PRIMA_MS) {
                continue;
            }
            if attacco_ms > battuta.saturating_add(FINESTRA_DOPO_MS) {
                break;
            }
            let distanza = (battuta - attacco_ms).abs();
            if scelto.is_none_or(|(_, _, gia)| distanza < gia.abs()) {
                scelto = Some((indice, *attacco, battuta - attacco_ms));
            }
        }
        match scelto {
            Some((indice, attacco, scarto)) => {
                // L'attacco è consumato: la battuta dopo cercherà dal successivo.
                primo_libero = indice.saturating_add(1);
                scarti.push(scarto);
                agganciate.push(Some(attacco));
            }
            None => agganciate.push(None),
        }
    }

    let latenza = mediana(&mut scarti);

    // Seconda passata: chi non ha trovato niente si corregge della mediana, e
    // nessuno finisce prima di chi lo precede.
    let mut fuori: Vec<u32> = Vec::with_capacity(battute.len());
    let mut minimo = 0_i64;
    for (indice, battuta) in battute.iter().enumerate() {
        let corretta = match agganciate.get(indice).copied().flatten() {
            Some(attacco) => i64::from(attacco),
            None => i64::from(*battuta).saturating_sub(latenza),
        };
        let tenuta = corretta.max(minimo).max(0);
        minimo = tenuta;
        fuori.push(u32::try_from(tenuta).unwrap_or(u32::MAX));
    }
    fuori
}

/// La mediana di un insieme di scarti; zero se non ce ne sono.
///
/// Riordina quel che riceve, ed è il motivo per cui lo prende in prestito
/// mutabile: la copia costerebbe un vettore per una funzione chiamata una volta
/// per canzone, e chi chiama non ha più bisogno dell'ordine di prima.
#[expect(
    clippy::integer_division,
    reason = "l'indice di mezzo di un elenco è una divisione intera per due, e il resto qui non vuol dire niente"
)]
fn mediana(valori: &mut [i64]) -> i64 {
    if valori.is_empty() {
        return 0;
    }
    valori.sort_unstable();
    let mezzo = valori.len() / 2;
    // Con un numero pari di scarti si prende quello **basso** dei due centrali
    // invece della media: la media di due interi qui non aggiunge precisione —
    // gli scarti sono già quantizzati dal passo degli attacchi — e in cambio
    // introdurrebbe una divisione che il workspace vieta.
    valori.get(mezzo).copied().unwrap_or(0)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Le righe di prova non sono di nessuna canzone: servono i tempi, non le
    /// parole, e un testo vero qui sarebbe testo di qualcun altro nel nostro
    /// repository.
    const UNO: &str = "[00:12.34]prima riga\n[00:15.00]seconda riga\n[01:02.50]terza riga";

    #[test]
    fn i_tempi_si_leggono_in_tutte_le_scritture_che_i_file_usano() {
        assert_eq!(tempo_ms("00:12.34"), Some(12_340));
        assert_eq!(tempo_ms("00:12.345"), Some(12_345));
        assert_eq!(tempo_ms("00:12"), Some(12_000));
        assert_eq!(tempo_ms("00:12,34"), Some(12_340));
        // Una cifra sola sono decimi. È il caso che un parser distratto sbaglia
        // di un fattore cento.
        assert_eq!(tempo_ms("00:01.5"), Some(1_500));
        assert_eq!(tempo_ms("01:02:03.00"), Some(3_723_000));
        // I minuti passano il sessanta senza diventare ore.
        assert_eq!(tempo_ms("62:00.00"), Some(3_720_000));
        assert_eq!(tempo_ms("ciao"), None);
        assert_eq!(tempo_ms(""), None);
        assert_eq!(tempo_ms("ar:qualcuno"), None);
    }

    #[test]
    fn una_riga_con_i_tempi_diventa_una_riga() {
        let testo = leggi(UNO);
        assert_eq!(testo.righe.len(), 3);
        assert_eq!(testo.righe.first().map(|r| r.ms), Some(12_340));
        assert_eq!(
            testo.righe.first().map(|r| r.testo.as_str()),
            Some("prima riga")
        );
        assert_eq!(testo.righe.last().map(|r| r.ms), Some(62_500));
        assert!(testo.piatto.is_none());
        assert!(testo.sincronizzato());
    }

    #[test]
    fn i_marcatori_di_sezione_restano_testo() {
        // Il caso per cui `separa_tag` guarda dentro le parentesi invece di
        // fidarsi della forma: senza, questa riga sparirebbe dal testo.
        let testo = leggi("[Ritornello]\nqualcosa\n[Strofa 2]\naltro");
        assert_eq!(
            testo.piatto.as_deref(),
            Some("[Ritornello]\nqualcosa\n[Strofa 2]\naltro")
        );
        assert!(testo.righe.is_empty());
    }

    #[test]
    fn i_tag_conosciuti_invece_spariscono() {
        let testo = leggi("[ti:un titolo]\n[ar:qualcuno]\n[00:01.00]prima riga");
        assert_eq!(testo.righe.len(), 1);
        assert_eq!(
            testo.righe.first().map(|r| r.testo.as_str()),
            Some("prima riga")
        );
    }

    #[test]
    fn lo_scarto_si_conserva_e_non_si_applica() {
        let testo = leggi("[offset:+500]\n[00:10.00]prima riga");
        assert_eq!(testo.offset_ms, 500);
        // I tempi restano quelli scritti: applicarli qui vorrebbe dire
        // riapplicarli alla riscrittura.
        assert_eq!(testo.righe.first().map(|r| r.ms), Some(10_000));
        assert_eq!(posizione_corretta(9_500, 500, 0), 10_000);
        assert_eq!(posizione_corretta(9_500, 500, -200), 9_800);
    }

    #[test]
    fn una_riga_cantata_due_volte_ha_due_tempi() {
        let testo = leggi("[00:10.00][01:20.00]la stessa riga");
        assert_eq!(testo.righe.len(), 2);
        assert_eq!(
            testo.righe.iter().map(|r| r.ms).collect::<Vec<_>>(),
            vec![10_000, 80_000]
        );
    }

    #[test]
    fn le_righe_fuori_ordine_si_rimettono_in_ordine() {
        let testo = leggi("[00:30.00]terza\n[00:10.00]prima\n[00:20.00]seconda");
        assert_eq!(
            testo.righe.iter().map(|r| r.ms).collect::<Vec<_>>(),
            vec![10_000, 20_000, 30_000]
        );
    }

    #[test]
    fn bom_e_crlf_non_si_vedono() {
        let testo = leggi("\u{FEFF}[00:01.00]prima riga\r\n[00:02.00]seconda riga\r\n");
        assert_eq!(testo.righe.len(), 2);
        assert_eq!(
            testo.righe.first().map(|r| r.testo.as_str()),
            Some("prima riga")
        );
    }

    #[test]
    fn un_testo_senza_tempi_e_piatto() {
        let testo = leggi("prima riga\nseconda riga\n");
        assert!(testo.righe.is_empty());
        assert_eq!(testo.piatto.as_deref(), Some("prima riga\nseconda riga"));
        assert!(!testo.sincronizzato());
        assert!(!testo.vuoto());
    }

    #[test]
    fn niente_e_vuoto() {
        assert!(leggi("").vuoto());
        assert!(leggi("   \n  \n").vuoto());
        assert!(
            !Testo {
                strumentale: true,
                ..Testo::default()
            }
            .vuoto()
        );
    }

    #[test]
    fn riscrivere_non_cambia_niente_due_volte() {
        let una = leggi(UNO);
        let scritto = scrivi(&una);
        // I tempi di `UNO` sono allineati al centesimo: qui l'andata e ritorno è
        // l'identità.
        assert_eq!(leggi(&scritto), una);
        // E in generale riscrivere è stabile, che i tempi siano allineati o no.
        let storto = Testo {
            righe: vec![Riga {
                ms: 12_347,
                testo: "prima riga".to_owned(),
                parole: Vec::new(),
            }],
            ..Testo::default()
        };
        let primo = scrivi(&storto);
        assert_eq!(primo, "[00:12.35]prima riga\n");
        assert_eq!(scrivi(&leggi(&primo)), primo);
    }

    #[test]
    fn lo_scarto_si_riscrive_col_segno() {
        let testo = leggi("[offset:+500]\n[00:10.00]prima riga");
        assert!(scrivi(&testo).starts_with("[offset:+500]\n"));
        let testo = leggi("[offset:-250]\n[00:10.00]prima riga");
        assert!(scrivi(&testo).starts_with("[offset:-250]\n"));
    }

    #[test]
    fn le_parole_si_leggono_e_si_riscrivono() {
        let testo = leggi("[00:10.00]<00:10.00>una <00:10.50>due");
        let prima = testo.righe.first().expect("una riga");
        assert_eq!(prima.testo, "una due");
        assert_eq!(prima.parole.len(), 2);
        assert_eq!(prima.parole.first().map(|p| p.ms), Some(10_000));
        assert_eq!(prima.parole.last().map(|p| p.ms), Some(10_500));
        assert_eq!(scrivi(&leggi(&scrivi(&testo))), scrivi(&testo));
    }

    #[test]
    fn un_a2_salvato_si_rilegge_con_le_parole() {
        // Il giro completo di quel che fa l'editor a livello di parola: si
        // compone un testo con i tempi delle parole, lo si scrive come lo
        // scriverebbe `testo_salva`, e lo si rilegge come lo rileggerà la
        // catena delle fonti quando troverà il `.a2.lrc` accanto al brano.
        let scritto_a_mano = Testo {
            righe: vec![
                Riga {
                    ms: 10_000,
                    testo: "prima riga qui".to_owned(),
                    parole: vec![
                        Parola {
                            ms: 10_000,
                            testo: "prima ".to_owned(),
                        },
                        Parola {
                            ms: 10_400,
                            testo: "riga ".to_owned(),
                        },
                        Parola {
                            ms: 10_900,
                            testo: "qui".to_owned(),
                        },
                    ],
                },
                Riga {
                    ms: 20_000,
                    testo: "seconda".to_owned(),
                    parole: vec![Parola {
                        ms: 20_000,
                        testo: "seconda".to_owned(),
                    }],
                },
            ],
            ..Testo::default()
        };

        let a2 = scrivi(&scritto_a_mano);
        assert_eq!(
            a2,
            concat!(
                "[00:10.00]<00:10.00>prima <00:10.40>riga <00:10.90>qui
",
                "[00:20.00]<00:20.00>seconda
",
            )
        );

        let riletto = leggi(&a2);
        assert_eq!(riletto, scritto_a_mano);
        // E le parole ricompongono la riga: è la proprietà su cui
        // `parole_combaciano` decide se tenerle.
        assert!(riletto.righe.iter().all(parole_combaciano));
    }

    #[test]
    fn le_parole_che_non_ricompongono_la_riga_non_combaciano() {
        // Una riga senza parole non ha niente da contraddire.
        assert!(parole_combaciano(&Riga {
            ms: 0,
            testo: "una due".to_owned(),
            parole: Vec::new(),
        }));
        // Il caso vero: si è battuto su un testo e poi il testo è cambiato.
        assert!(!parole_combaciano(&Riga {
            ms: 0,
            testo: "una tre".to_owned(),
            parole: vec![
                Parola {
                    ms: 0,
                    testo: "una ".to_owned(),
                },
                Parola {
                    ms: 500,
                    testo: "due".to_owned(),
                },
            ],
        }));
        // Contare non basterebbe: qui le parole sono due come i tempi.
        assert!(!parole_combaciano(&Riga {
            ms: 0,
            testo: "una due".to_owned(),
            parole: vec![
                Parola {
                    ms: 0,
                    testo: "una ".to_owned(),
                },
                Parola {
                    ms: 500,
                    testo: "tre".to_owned(),
                },
            ],
        }));
    }

    #[test]
    fn un_minore_che_non_e_un_tempo_resta_dov_e() {
        let testo = leggi("[00:10.00]tre <2 e non < di due");
        assert_eq!(
            testo.righe.first().map(|r| r.testo.as_str()),
            Some("tre <2 e non < di due")
        );
        assert!(testo.righe.first().is_some_and(|r| r.parole.is_empty()));
    }

    #[test]
    fn la_riga_attiva_sta_ferma_prima_dell_inizio() {
        let righe = leggi(UNO).righe;
        assert_eq!(riga_attiva(&righe, 0), None);
        // L'anticipo accende la riga prima del suo tempo, e non prima
        // dell'anticipo.
        assert_eq!(riga_attiva(&righe, 12_340 - ANTICIPO_MS - 1), None);
        assert_eq!(riga_attiva(&righe, 12_340 - ANTICIPO_MS), Some(0));
        assert_eq!(riga_attiva(&righe, 12_340), Some(0));
        assert_eq!(riga_attiva(&righe, 14_000), Some(0));
        assert_eq!(riga_attiva(&righe, 15_000), Some(1));
        // Dopo l'ultima riga resta accesa l'ultima: il brano continua, e
        // spegnere tutto lascerebbe la schermata vuota sul finale.
        assert_eq!(riga_attiva(&righe, 9_999_999), Some(2));
        assert_eq!(riga_attiva(&[], 1_000), None);
        // Una posizione negativa capita, con uno scarto grande e il brano appena
        // partito.
        assert_eq!(riga_attiva(&righe, -5_000), None);
    }

    #[test]
    fn l_avanzamento_attraversa_la_riga() {
        let righe = leggi("[00:10.00]prima\n[00:20.00]seconda").righe;
        assert!((avanzamento(&righe, 0, 10_000) - 0.0).abs() < 1e-9);
        assert!((avanzamento(&righe, 0, 15_000) - 0.5).abs() < 1e-9);
        assert!((avanzamento(&righe, 0, 20_000) - 1.0).abs() < 1e-9);
        // Durante l'anticipo è zero, non negativo.
        assert!((avanzamento(&righe, 0, 9_900) - 0.0).abs() < 1e-9);
        // L'ultima riga non ha una successiva: ferma, non inventata.
        assert!((avanzamento(&righe, 1, 25_000) - 0.0).abs() < 1e-9);
        assert!((avanzamento(&righe, 9, 25_000) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn l_aderenza_ferma_il_testo_di_un_altra_edizione() {
        let testo = leggi(UNO); // l'ultima riga sta a 62,5 secondi
        assert_eq!(verifica_durata(&testo, 70_000), Aderenza::Buona);
        // Il testo continua dopo la fine: è di un'altra edizione.
        assert_eq!(verifica_durata(&testo, 50_000), Aderenza::Fuori);
        // Finisce troppo presto: può essere una coda strumentale, o può essere
        // la versione radio applicata all'estesa.
        assert_eq!(verifica_durata(&testo, 200_000), Aderenza::Sospetta);
        // La grazia copre il rumore fra due codifiche dello stesso master.
        assert_eq!(verifica_durata(&testo, 60_000), Aderenza::Buona);
        // Senza durata non si giudica.
        assert_eq!(verifica_durata(&testo, 0), Aderenza::Buona);
        // Un testo piatto non ha tempi da verificare.
        assert_eq!(
            verifica_durata(&leggi("solo parole"), 1_000),
            Aderenza::Buona
        );
    }

    fn candidato(titolo: &str, artista: &str, durata_ms: u64, sincronizzato: bool) -> Candidato {
        Candidato {
            id: 1,
            titolo: titolo.to_owned(),
            artista: artista.to_owned(),
            album: None,
            durata_ms: Some(durata_ms),
            strumentale: false,
            sincronizzato,
        }
    }

    #[test]
    fn si_preferisce_chi_ha_i_tempi_a_parita_di_tutto() {
        let candidati = vec![
            candidato("Poetica", "Cesare Cremonini", 240_000, false),
            candidato("Poetica", "Cesare Cremonini", 240_000, true),
        ];
        let cercato = Cercato {
            titolo: "Poetica",
            artista: "Cesare Cremonini",
            album: None,
            durata_ms: Some(240_000),
        };
        assert_eq!(scegli(&candidati, &cercato), Some(1));
    }

    #[test]
    fn la_durata_sbagliata_non_si_recupera_con_niente() {
        // Stesso titolo, stesso artista, i tempi: e dura un minuto di più. È il
        // remix, ed è esattamente il caso che il veto esiste per fermare.
        let candidati = vec![candidato("Poetica", "Cesare Cremonini", 300_000, true)];
        let cercato = Cercato {
            titolo: "Poetica",
            artista: "Cesare Cremonini",
            album: None,
            durata_ms: Some(240_000),
        };
        assert_eq!(scegli(&candidati, &cercato), None);
    }

    #[test]
    fn un_titolo_diverso_non_passa_nemmeno_col_resto_giusto() {
        let candidati = vec![candidato(
            "Un'altra canzone",
            "Cesare Cremonini",
            240_000,
            true,
        )];
        let cercato = Cercato {
            titolo: "Poetica",
            artista: "Cesare Cremonini",
            album: None,
            durata_ms: Some(240_000),
        };
        assert_eq!(scegli(&candidati, &cercato), None);
    }

    #[test]
    fn senza_durata_si_decide_lo_stesso_ma_sul_testo() {
        let candidati = vec![Candidato {
            durata_ms: None,
            ..candidato("Poetica", "Cesare Cremonini", 0, true)
        }];
        let cercato = Cercato {
            titolo: "Poetica",
            artista: "Cesare Cremonini",
            album: None,
            durata_ms: None,
        };
        assert_eq!(scegli(&candidati, &cercato), Some(0));
    }

    #[test]
    fn uno_strumentale_lo_si_accetta_solo_se_il_titolo_e_lo_stesso() {
        let cercato = Cercato {
            titolo: "Poetica",
            artista: "Cesare Cremonini",
            album: None,
            durata_ms: Some(240_000),
        };
        // «Poetica II» passa la soglia del titolo — è lo stesso artista, la
        // stessa durata — ma non è la stessa canzone, e accettarla vorrebbe dire
        // dire a chi ascolta «questo brano non ha parole». Una decorazione fra
        // parentesi non servirebbe a niente qui: `similarity` la toglie già.
        let dubbio = vec![Candidato {
            strumentale: true,
            ..candidato("Poetica II", "Cesare Cremonini", 240_000, false)
        }];
        assert_eq!(scegli(&dubbio, &cercato), None);
        let certo = vec![Candidato {
            strumentale: true,
            ..candidato("Poetica", "Cesare Cremonini", 240_000, false)
        }];
        assert_eq!(scegli(&certo, &cercato), Some(0));
    }

    #[test]
    fn niente_candidati_niente_scelta() {
        let cercato = Cercato {
            titolo: "Poetica",
            artista: "Cesare Cremonini",
            album: None,
            durata_ms: Some(240_000),
        };
        assert_eq!(scegli(&[], &cercato), None);
    }

    #[test]
    fn le_battute_si_agganciano_agli_attacchi() {
        // Attacchi veri a 10, 20 e 30 secondi; battute date con duecento
        // millisecondi di ritardo, come le darebbe una mano umana.
        let attacchi = vec![10_000, 20_000, 30_000];
        let battute = vec![10_200, 20_180, 30_240];
        assert_eq!(aggancia(&battute, &attacchi), vec![10_000, 20_000, 30_000]);
    }

    #[test]
    fn chi_non_trova_un_attacco_si_corregge_della_mediana() {
        // Due battute agganciano con 200 ms di ritardo; la terza cade in un
        // punto dove non c'è nessun attacco vicino, e si corregge di 200.
        let attacchi = vec![10_000, 20_000];
        let battute = vec![10_200, 20_200, 45_000];
        assert_eq!(aggancia(&battute, &attacchi), vec![10_000, 20_000, 44_800]);
    }

    #[test]
    fn una_battuta_data_male_non_rovina_le_altre() {
        // La mediana esiste per questo: una battuta data due secondi tardi
        // sposterebbe la media di seicento millisecondi, e con lei ogni riga
        // non agganciata.
        let attacchi = vec![10_000, 20_000, 30_000];
        let battute = vec![10_100, 20_100, 32_100, 45_000];
        let esito = aggancia(&battute, &attacchi);
        // Le prime due agganciano; la terza è troppo lontana da 30 000 e resta
        // libera, la quarta pure. Entrambe si correggono di 100, non di 700.
        assert_eq!(esito.get(3), Some(&44_900));
    }

    #[test]
    fn un_attacco_serve_una_battuta_sola() {
        // Due righe vicinissime, un attacco solo: la seconda non deve prendersi
        // lo stesso tempo della prima, o non si accenderebbe mai.
        let attacchi = vec![10_000];
        let battute = vec![10_050, 10_100];
        let esito = aggancia(&battute, &attacchi);
        assert_eq!(esito.first(), Some(&10_000));
        assert!(
            esito.get(1).is_some_and(|secondo| *secondo > 10_000),
            "la seconda riga non può cadere sullo stesso millesimo: {esito:?}"
        );
    }

    #[test]
    fn la_finestra_e_asimmetrica() {
        // Un attacco 200 ms **prima** della battuta si aggancia: è il ritardo
        // della mano.
        assert_eq!(aggancia(&[10_200], &[10_000]), vec![10_000]);
        // Uno 200 ms **dopo** no: sarebbe la sillaba successiva.
        assert_eq!(aggancia(&[10_000], &[10_200]), vec![10_000]);
    }

    #[test]
    fn senza_attacchi_le_battute_restano_quelle() {
        let battute = vec![1_000, 2_000, 3_000];
        assert_eq!(aggancia(&battute, &[]), battute);
        assert!(aggancia(&[], &[1_000]).is_empty());
    }

    #[test]
    fn l_esito_non_torna_mai_indietro() {
        // Un attacco che tirerebbe la seconda riga prima della prima: si tiene
        // dov'era la prima invece di scambiarle.
        let attacchi = vec![10_000, 9_000];
        let esito = aggancia(&[10_100, 9_100], &attacchi);
        assert!(
            esito.windows(2).all(|due| due.first() <= due.get(1)),
            "i tempi devono restare in ordine: {esito:?}"
        );
    }

    #[test]
    fn il_piatto_si_ricava_anche_da_un_testo_sincronizzato() {
        assert_eq!(
            leggi(UNO).come_piatto(),
            "prima riga\nseconda riga\nterza riga\n"
        );
        assert_eq!(leggi("solo parole").come_piatto(), "solo parole");
    }
}
