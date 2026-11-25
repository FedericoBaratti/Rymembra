//! Il testo di un brano, con i tempi o senza.
//!
//! Sta nel dominio per la ragione di sempre: qui non si legge nessun file e non
//! si guarda nessun orologio, quindi tutto quel che segue si prova come una
//! chiamata di funzione. Chi il `.lrc` lo trova sul disco è `rymembra_app::testi`,
//! chi lo chiede alla rete è `rymembra_meta::lrclib`, chi lo disegna è la
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
//!   la ricerca binaria di [`riga_attiva`] ha bisogno che siano in ordine;
//! * un testo costruito per costare molto si **taglia**, non si rifiuta: i tetti
//!   sui tempi, le parole e le righe sono le costanti `MASSIM…` qui sotto, con il
//!   perché di ciascuno accanto a lui.

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
/// decina di millisecondi, vedi `rymembra_play::uscita::annota_latenza` — e quella
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

// ── i tetti del lettore ─────────────────────────────────────────────────────
//
// Un `.lrc` arriva da posti di cui non ci si può fidare — il file accanto a un
// album scaricato, il tag di un brano comprato, la risposta di un catalogo in
// cui chiunque può pubblicare, un altro dispositivo — e [`leggi`] non fallisce
// mai. Quindi il suo **costo** deve essere limitato qualunque cosa riceva, e il
// solo posto in cui si può dirlo una volta per tutti è qui: un tetto sui byte
// in ingresso (chi apre il file, chi riceve la risposta) non basta, perché
// `[0:0]` sono cinque byte e un tempo valido, e `<0:0>x` sei e una parola.
// Quarantottomila byte di quei due, senza tetti qui dentro, erano quattromila
// parole clonate per ciascuno dei quattromilaottocento tempi: un miliardo di
// byte. Con duecentocinquantasei kibibyte, decine di gigabyte.
//
// Nessun tetto taglia un LRC vero. Uno vero ha al più qualche decina di tempi
// per riga (un ritornello ripetuto cinque, dieci volte), qualche decina di
// parole per riga anche nel karaoke sillaba per sillaba, qualche centinaio di
// righe, qualche migliaio per un'opera intera. I valori sotto stanno a un
// ordine di grandezza sopra, non di più: servono a fermare un testo costruito,
// non a giudicare quello di nessuno.
//
// Gli eccessi **si scartano**, e non restano nel testo come caratteri. È la
// proprietà che il modulo dichiara — `scrivi(leggi(scrivi(x))) == scrivi(x)` —
// a imporlo: un tag che oltre il tetto restasse nella riga come testo verrebbe
// riletto come tag dal giro dopo, e ogni riscrittura ne mangerebbe un altro
// pezzo.

/// Quanti tempi **distinti**, al più, ha una riga: `[00:12.00][01:15.00]…`.
///
/// Trentadue: una strofa ripetuta dieci volte è già un caso raro. Oltre, i tag
/// si consumano e non contano.
pub const MASSIMI_TEMPI_PER_RIGA: usize = 32;

/// Quante parole con un tempo, al più, ha una riga dell'LRC esteso.
///
/// Cinquecentododici: il karaoke che temporizza ogni sillaba o ogni carattere
/// fa qualche decina di tempi per riga. Dalla parola successiva la riga **finisce
/// lì**: il resto, tag e testo, si scarta — non si accoda all'ultima parola,
/// perché due pezzi di testo che si toccano senza il tag in mezzo possono
/// comporre un tag nuovo.
///
/// Il pezzo che sta **prima** della prima parola (`Oh <00:10.50>…`) non ha un tag,
/// ma [`leggi`] gliene dà uno — quello della riga — e [`scrivi`] lo scrive: alla
/// rilettura è una parola come le altre, e se fosse la cinquecentotredicesima la
/// riga si accorcerebbe di una parola a ogni giro. Per questo conta nel tetto: una
/// riga che lo ha ne ha al più cinquecentoundici con un tempo.
pub const MASSIME_PAROLE_PER_RIGA: usize = 512;

/// Quante righe, al più, [`leggi`] costruisce **prima** della fusione dei tempi
/// uguali: quattro volte le righe che un testo ricevuto può avere
/// (`MASSIME_RIGHE` in `rymembra_app::testi::condiviso`), per lasciare posto ai
/// ritornelli. Le righe che non ci stanno si scartano per intero.
pub const MASSIME_RIGHE_ESPANSE: usize = 16_000;

/// Quante parole con un tempo, al più, [`leggi`] costruisce in tutto, ripetizioni
/// dei tempi comprese: cinque volte quelle che un testo ricevuto può avere. Una
/// riga che farebbe superarlo si scarta per intero, come per le righe.
pub const MASSIME_PAROLE_ESPANSE: usize = 200_000;

/// Quante righe **secondarie**, al più, ha una riga: le altre righe che il file
/// mette allo stesso tempo, di solito una traduzione e una pronuncia.
///
/// Sedici: i file con le traduzioni ne hanno una, due, di rado quattro. Servono
/// a fermare mille righe diverse allo stesso tempo — `[00:00.00]uno`,
/// `[00:00.00]due`… —, che la fusione confrontava ognuna con tutte le
/// precedenti: il quadrato del numero di righe.
pub const MASSIME_SECONDARIE_PER_RIGA: usize = 16;

/// Quanto può essere lungo, in byte, il contenuto di un tag di tempo
/// (`hh:mm:ss.xxx` sono dodici): serve a cercare il `>` che lo chiude **solo
/// lì vicino**. Senza, ogni `<` di un testo andava a cercare il suo `>` fino in
/// fondo alla riga, e un testo di sole parentesi aperte costava il quadrato
/// della sua lunghezza.
pub const MASSIMA_LUNGHEZZA_TEMPO: usize = 32;

/// Quanto può valere, al più, lo scarto che un LRC dichiara con `[offset:]`: un'ora.
///
/// È lo stesso tetto della correzione di chi ascolta
/// (`rymembra_app::testi::condiviso::MASSIMO_SCARTO_MS`) e per la stessa
/// ragione: oltre non è una sincronizzazione sbagliata di poco, è un numero
/// sbagliato, e con il pulsante di correzione a più o meno un'ora il testo non
/// si raddrizzerebbe più. Uno scarto fuori dal tetto vale come uno che non si
/// legge: zero.
pub const MASSIMO_OFFSET_MS: i32 = 3_600_000;

/// Quanto, al più, è lungo in byte un testo che si legge da fuori: un `.lrc`
/// accanto al brano, un tag, la risposta di un catalogo, il testo di un altro
/// dispositivo.
///
/// Un quarto di mebibyte: un testo vero sta in pochi kibibyte, e un'opera intera
/// con i tempi di ogni parola in qualche centinaio. È un tetto **di chi apre**,
/// non di [`leggi`] — che non sa da dove viene quel che legge — e sta qui perché
/// tutti i punti d'ingresso lo condividano invece di scegliersi ognuno il suo.
pub const MASSIMO_TESTO_BYTE: usize = 256 * 1024;

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
    /// Le righe che il file mette **allo stesso tempo** di questa, di solito una
    /// traduzione o la pronuncia. Separate da `\n`, e quasi sempre `None`.
    ///
    /// Esiste perché due righe allo stesso millesimo non si possono accendere
    /// tutt'e due: [`riga_attiva`] prende l'ultima, e la prima — che nei file con
    /// la traduzione è l'originale, cioè quel che si canta — restava spenta per
    /// sempre. Vedi [`leggi`].
    pub secondaria: Option<String>,
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
/// use rymembra_domain::testo::leggi;
/// let testo = leggi("[00:12.34]la prima riga\n[00:15.00]la seconda");
/// assert_eq!(testo.righe.len(), 2);
/// assert_eq!(testo.righe.first().map(|r| r.ms), Some(12_340));
/// ```
#[must_use]
pub fn leggi(grezzo: &str) -> Testo {
    let mut righe: Vec<Riga> = Vec::new();
    let mut senza_tempo: Vec<&str> = Vec::new();
    let mut offset_ms = 0_i32;
    let mut parole_espanse = 0_usize;

    // Il BOM sta in testa al file, non in testa a ogni riga: si toglie una volta
    // sola. `lines()` pensa da sé al CRLF ma non al CR nudo, che è il fine riga
    // dei file scritti sui Mac di prima di OS X e di qualche editor di testi
    // giapponese: senza questa riga un file intero diventava una riga sola, con i
    // tempi della prima e il resto come testo.
    let normalizzato = grezzo
        .trim_start_matches('\u{FEFF}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    for riga in normalizzato.lines() {
        let (etichette, resto) = separa_tag(riga);
        if let Some(scarto) = etichette.offset {
            offset_ms = scarto;
        }
        let tempi = etichette.tempi;

        if tempi.is_empty() {
            // Una riga di sole intestazioni — `[ar:…]`, `[ti:…]`, `[offset:…]` —
            // non è testo nemmeno in un file senza tempi: si tiene solo quel che
            // le sta dopo, se c'è. `[Ritornello]` non passa di qui, perché
            // `separa_tag` si ferma prima e `ha_tag` resta falso.
            if !etichette.ha_tag {
                senza_tempo.push(riga);
            } else if !resto.trim().is_empty() {
                senza_tempo.push(resto);
            }
            continue;
        }

        // Il tetto sulle righe, prima di costruirne una: si guarda quel che la
        // riga **aggiungerebbe**, e una riga che non ci sta si scarta intera
        // (vedi i tetti in testa al modulo).
        if righe.len().saturating_add(tempi.len()) > MASSIME_RIGHE_ESPANSE {
            continue;
        }
        let (piatta, prefisso, parole) = separa_parole(resto);
        let costo = parole
            .len()
            .saturating_add(usize::from(prefisso.is_some()))
            .saturating_mul(tempi.len());
        if parole_espanse.saturating_add(costo) > MASSIME_PAROLE_ESPANSE {
            continue;
        }
        parole_espanse = parole_espanse.saturating_add(costo);
        let piatta = piatta.trim().to_owned();
        for ms in tempi {
            // Una riga con più tempi è la stessa riga cantata più volte, e ogni
            // volta è una riga sua: fondere i tempi in una sola voce
            // significherebbe accenderla al primo e lasciarla accesa fino
            // all'ultimo.
            //
            // Il pezzo prima della prima parola con un tempo — `Oh <00:10.50>…`,
            // chi scrive l'LRC esteso non tagga la parola che cade sul tempo
            // della riga — comincia con la riga, quindi prende il suo tempo: di
            // ciascuna, non della prima. Senza, la finestra, che quando ci sono
            // le parole disegna quelle, non lo mostrava.
            let parole = match &prefisso {
                Some(pezzo) => std::iter::once(Parola {
                    ms,
                    testo: pezzo.clone(),
                })
                .chain(parole.iter().cloned())
                .collect(),
                None => parole.clone(),
            };
            righe.push(Riga {
                ms,
                testo: piatta.clone(),
                parole,
                secondaria: None,
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
        righe: fondi_allo_stesso_tempo(righe),
        piatto: None,
        strumentale: false,
        offset_ms,
    }
}

/// Le righe allo stesso millesimo diventano una riga sola, con le altre come
/// [`Riga::secondaria`].
///
/// # Perché fonderle
///
/// Perché è la forma dei file con la traduzione: `[00:12.00]originale` e subito
/// sotto `[00:12.00]traduzione`. Tenute separate, [`riga_attiva`] accendeva
/// sempre la seconda — è l'ultima con quel tempo — e l'originale, cioè quel che
/// si sta cantando, non si accendeva mai. Fuse, si accendono insieme, e la
/// finestra mostra la seconda sotto la prima.
///
/// Vince come principale la prima **non vuota**, nell'ordine del file: una riga
/// di soli tempi seguita dal testo allo stesso tempo non deve lasciare il testo
/// in secondo piano. Le parole con i tempi sono quelle della principale; quelle
/// di una traduzione non avrebbero niente su cui accendersi. Un doppione esatto
/// — `[00:12.00][00:12.00]riga` — non diventa una traduzione di sé stesso.
fn fondi_allo_stesso_tempo(righe: Vec<Riga>) -> Vec<Riga> {
    let mut fuse: Vec<Riga> = Vec::with_capacity(righe.len());
    // Quante secondarie ha già l'ultima riga: si tiene qui e non si conta
    // rileggendole, perché altrimenti mille righe diverse allo stesso tempo
    // volevano dire mille letture di tutte le precedenti.
    let mut secondarie = 0_usize;
    for riga in righe {
        let Some(ultima) = fuse.last_mut().filter(|u| u.ms == riga.ms) else {
            fuse.push(riga);
            secondarie = 0;
            continue;
        };
        if riga.testo.is_empty() || riga.testo == ultima.testo {
            continue;
        }
        if ultima.testo.is_empty() {
            let secondaria = ultima.secondaria.take();
            *ultima = Riga { secondaria, ..riga };
            continue;
        }
        // Oltre il tetto una secondaria in più si scarta, senza cercarla fra
        // le altre: vedi [`MASSIME_SECONDARIE_PER_RIGA`].
        if secondarie >= MASSIME_SECONDARIE_PER_RIGA {
            continue;
        }
        match ultima.secondaria.as_mut() {
            Some(gia) if gia.lines().any(|l| l == riga.testo) => {}
            Some(gia) => {
                gia.push('\n');
                gia.push_str(&riga.testo);
                secondarie = secondarie.saturating_add(1);
            }
            None => {
                ultima.secondaria = Some(riga.testo);
                secondarie = 1;
            }
        }
    }
    fuse
}

/// I `[…]` in testa a una riga: i tempi che portano, lo scarto dichiarato, e se
/// ce n'era qualcuno.
#[derive(Debug, Default)]
struct Etichette {
    /// I tempi, **distinti** e nell'ordine in cui il file li dà, al più
    /// [`MASSIMI_TEMPI_PER_RIGA`]: quelli di troppo si consumano senza contare.
    tempi: Vec<u32>,
    /// L'ultimo `[offset:…]` della riga, già dentro [`MASSIMO_OFFSET_MS`].
    offset: Option<i32>,
    /// C'era almeno un tag, di qualunque genere.
    ha_tag: bool,
}

/// I contenuti dei `[…]` in testa alla riga, e quel che resta.
///
/// Un gruppo che non è né un tempo né una [chiave conosciuta](CHIAVI) **non** è
/// un tag: la scansione si ferma prima, e `[Ritornello]` resta la riga che è.
///
/// Non tiene i tag, ne tiene quel che dicono: uno che si ripete non occupa un
/// posto, e quelli oltre [`MASSIMI_TEMPI_PER_RIGA`] si **consumano** — escono dal
/// testo, che è quel che chiedono i tetti in testa al modulo
/// — ma non si ricordano.
fn separa_tag(riga: &str) -> (Etichette, &str) {
    let mut etichette = Etichette::default();
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
        if let Some(ms) = tempo_ms(dentro) {
            if etichette.tempi.len() < MASSIMI_TEMPI_PER_RIGA && !etichette.tempi.contains(&ms) {
                etichette.tempi.push(ms);
            }
        } else if let Some((chiave, valore)) = dentro.split_once(':')
            && CHIAVI.iter().any(|c| chiave.trim().eq_ignore_ascii_case(c))
        {
            if chiave.trim().eq_ignore_ascii_case("offset") {
                etichette.offset = Some(scarto_nei_tetti(valore));
            }
        } else {
            break;
        }
        etichette.ha_tag = true;
        resto = avanti;
    }
    (etichette, resto)
}

/// Lo scarto che un `[offset:…]` dichiara, o zero se non è uno scarto: un
/// numero che non si legge, e uno oltre [`MASSIMO_OFFSET_MS`].
fn scarto_nei_tetti(valore: &str) -> i32 {
    valore
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|scarto| scarto.unsigned_abs() <= MASSIMO_OFFSET_MS.unsigned_abs())
        .unwrap_or(0)
}

/// Un `<tempo>` all'inizio di `dopo` (che comincia subito dopo il `<`): il tempo
/// e quel che segue il `>`.
///
/// Il `>` si cerca solo entro [`MASSIMA_LUNGHEZZA_TEMPO`] byte: un tempo non è
/// più lungo, e cercarlo fino in fondo alla riga a ogni `<` rendeva quadratico un
/// testo fatto di sole parentesi aperte.
fn tempo_di_parola(dopo: &str) -> Option<(u32, &str)> {
    let chiude = dopo
        .bytes()
        .take(MASSIMA_LUNGHEZZA_TEMPO.saturating_add(1))
        .position(|b| b == b'>')?;
    let dentro = dopo.get(..chiude)?;
    let avanti = dopo.get(chiude.saturating_add(1)..)?;
    Some((tempo_ms(dentro)?, avanti))
}

/// I tempi delle parole dentro una riga, e la riga ricomposta.
///
/// Un `<…>` che non contiene un tempo resta dov'è, per la stessa ragione per cui
/// `[Ritornello]` resta dov'è: in un testo ci sono i segni di punteggiatura, e
/// non si mangiano.
///
/// Il pezzo che sta prima del primo tempo torna a parte, e solo quando ci sono
/// delle parole e il pezzo non è di soli spazi: non ha un tempo suo, e il tempo
/// che gli spetta — quello della riga — lo sa solo [`leggi`].
///
/// # Il tetto delle parole
///
/// Al tag che sarebbe il [`MASSIME_PAROLE_PER_RIGA`]-esimo più uno la riga
/// **finisce**: quel tag e tutto quel che lo segue si scartano. Non si accoda il
/// testo all'ultima parola togliendo solo i tag, perché due pezzi che si toccano
/// senza un tag in mezzo possono comporne uno nuovo — `x<0` e `:0>y` danno
/// `x<0:0>y` —, e la riscrittura lo leggerebbe come tempo.
///
/// Il pezzo prima del primo tag (se non è di soli spazi) ha già il suo posto fra
/// le parole: [`leggi`] lo riscrive come parola con il tempo della riga, quindi
/// ne toglie uno al tetto dei tag.
fn separa_parole(testo: &str) -> (String, Option<String>, Vec<Parola>) {
    if !testo.contains('<') {
        return (testo.to_owned(), None, Vec::new());
    }

    let mut segmenti: Vec<(Option<u32>, String)> = Vec::new();
    let mut corrente: Option<u32> = None;
    let mut pezzo = String::new();
    let mut resto = testo;
    let mut con_tempo = 0_usize;
    let mut tetto = MASSIME_PAROLE_PER_RIGA;

    while let Some(apre) = resto.find('<') {
        let (Some(prima), Some(dopo)) = (resto.get(..apre), resto.get(apre.saturating_add(1)..))
        else {
            break;
        };
        match tempo_di_parola(dopo) {
            Some((ms, avanti)) => {
                pezzo.push_str(prima);
                if con_tempo == 0 && !pezzo.trim().is_empty() {
                    tetto = tetto.saturating_sub(1);
                }
                if con_tempo >= tetto {
                    resto = "";
                    break;
                }
                con_tempo = con_tempo.saturating_add(1);
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
    let mut prefisso: Option<String> = None;
    let mut parole: Vec<Parola> = Vec::new();
    for (ms, testo) in segmenti {
        match ms {
            Some(ms) => parole.push(Parola { ms, testo }),
            // Solo il primo segmento è senza tempo: dopo il primo `<mm:ss>`
            // ogni pezzo ne ha uno.
            None => prefisso = Some(testo),
        }
    }
    parole.retain(|p| !p.testo.trim().is_empty());
    let prefisso = prefisso.filter(|p| !p.trim().is_empty() && !parole.is_empty());
    (piatta, prefisso, parole)
}

/// Legge `mm:ss`, `mm:ss.xx`, `mm:ss,xxx`, `mm:ss:xx` o `hh:mm:ss.xx`.
///
/// # Tre campi senza punto sono minuti, secondi e centesimi
///
/// `[01:02:03]` si scrive in due modi nel mondo: come `hh:mm:ss` e come
/// `mm:ss:cc`, con i centesimi dopo i due punti invece che dopo il punto. Il
/// secondo è quel che scrivono alcuni programmi di karaoke e parecchi file fatti
/// a mano, e un brano di un'ora e due minuti è molto più raro di un file così:
/// letto come ore, ogni riga cadeva un'ora più in là e il testo non si accendeva
/// mai. Quindi tre campi **con** la frazione — `01:02:03.00` — sono ore, minuti e
/// secondi; tre campi **senza** sono minuti, secondi e centesimi.
///
/// # Le cifre della frazione contano
///
/// Due cifre sono centesimi, tre sono millesimi, una è decimi: `[00:01.5]` è un
/// secondo e mezzo, non un secondo e cinque millesimi. Un parser che leggesse la
/// frazione come un intero sbaglierebbe di un fattore cento su ogni file scritto
/// con una cifra sola, e sono i file scritti a mano.
fn tempo_ms(grezzo: &str) -> Option<u32> {
    let grezzo = grezzo.trim();
    // Un tempo non è più lungo di così: chi guarda `[…]` e `<…>` di un testo
    // ostile non deve scomporre in pezzi un contenuto di centinaia di kibibyte
    // per scoprire che non lo è.
    if grezzo.len() > MASSIMA_LUNGHEZZA_TEMPO {
        return None;
    }
    let pezzi: Vec<&str> = grezzo.split(':').collect();
    let (ore, minuti, secondi, frazione) = match pezzi.as_slice() {
        [m, s] => {
            let s = s.trim();
            let (interi, frazione) = s.split_once(['.', ',']).unwrap_or((s, ""));
            ("0", *m, interi, frazione)
        }
        [h, m, s] if s.contains(['.', ',']) => {
            let s = s.trim();
            let (interi, frazione) = s.split_once(['.', ',']).unwrap_or((s, ""));
            (*h, *m, interi, frazione)
        }
        [m, s, centesimi] => {
            let centesimi = centesimi.trim();
            if centesimi.is_empty() || centesimi.len() > 3 {
                return None;
            }
            ("0", *m, s.trim(), centesimi)
        }
        _ => return None,
    };

    let ore: u32 = ore.trim().parse().ok()?;
    let minuti: u32 = minuti.trim().parse().ok()?;
    let interi: u32 = secondi.parse().ok()?;
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
/// `scrivi(leggi(scrivi(x))) == scrivi(x)` vale per ogni testo dentro i tetti del
/// lettore — cioè per ogni testo che [`leggi`] restituisce — le cui righe stanno a
/// un centesimo o più l'una dall'altra, quindi riscrivere un file non lo cambia
/// mai due volte. È la proprietà che conta, ed è provata, anche sugli ingressi che
/// i tetti tagliano. Due righe a **meno** di un centesimo (`[00:02.000]` e
/// `[00:02.004]`) scrivono lo stesso tempo, e la prima lettura le fonde — una
/// riga vuota così vicina a una piena sparisce —: è la seconda riscrittura a
/// trovarle già fuse, e da lì il testo è stabile.
///
/// Un [`Testo`] **costruito a mano** oltre i tetti (un editor che salva più di
/// [`MASSIME_RIGHE_ESPANSE`] righe, più di [`MASSIME_PAROLE_PER_RIGA`] parole in
/// una riga o più di [`MASSIME_SECONDARIE_PER_RIGA`] secondarie) si scrive per
/// intero, ma riletto si taglia: i tetti sono del lettore, non di chi scrive.
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
        // Le secondarie tornano come le si era lette: una riga ciascuna, allo
        // stesso tempo, dopo la principale. È la forma che gli altri lettori
        // conoscono, e rileggerla le rifonde.
        for secondaria in riga.secondaria.iter().flat_map(|s| s.lines()) {
            fuori.push('[');
            fuori.push_str(&scrivi_tempo(riga.ms));
            fuori.push(']');
            fuori.push_str(secondaria);
            fuori.push('\n');
        }
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
/// use rymembra_domain::testo::{Parola, Riga, parole_combaciano};
/// let riga = Riga {
///     ms: 0,
///     testo: "una due".to_owned(),
///     parole: vec![
///         Parola { ms: 0, testo: "una ".to_owned() },
///         Parola { ms: 500, testo: "due".to_owned() },
///     ],
///     secondaria: None,
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
/// `rymembra_play::attacchi` li tira fuori dal file. Allora:
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
/// Il risultato è strettamente crescente: una battuta non può finire prima di
/// quella prima di lei, e nemmeno sul suo stesso tempo. Se l'aggancio la
/// porterebbe indietro, si ferma un centesimo dopo la precedente — meglio due
/// righe attaccate che due righe scambiate, e «attaccate» vuol dire a un
/// centesimo l'una dall'altra: sullo stesso centesimo, riscritte e rilette,
/// sarebbero diventate una riga sola con la seconda come traduzione.
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
    // nessuno finisce prima di chi lo precede — né sul suo stesso centesimo.
    let mut fuori: Vec<u32> = Vec::with_capacity(battute.len());
    let mut minimo = 0_i64;
    for (indice, battuta) in battute.iter().enumerate() {
        let corretta = match agganciate.get(indice).copied().flatten() {
            Some(attacco) => i64::from(attacco),
            None => i64::from(*battuta).saturating_sub(latenza),
        };
        let tenuta = corretta.max(minimo).max(0);
        // La prossima almeno un centesimo dopo questa. Un millesimo non
        // basterebbe: [`scrivi`] arrotonda al centesimo, e due righe sullo
        // stesso centesimo, rilette, diventano una riga e la sua traduzione
        // (vedi `fondi_allo_stesso_tempo`). A dieci millesimi di distanza
        // l'arrotondamento le tiene sempre su due centesimi diversi.
        minimo = tenuta.saturating_add(PASSO_MINIMO_MS);
        fuori.push(u32::try_from(tenuta).unwrap_or(u32::MAX));
    }
    fuori
}

/// Di quanto, almeno, due battute raddrizzate da [`aggancia`] stanno lontane.
///
/// Un centesimo di secondo: è la risoluzione con cui [`scrivi`] riscrive i
/// tempi.
const PASSO_MINIMO_MS: i64 = 10;

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
    fn tre_campi_senza_punto_sono_centesimi_non_ore() {
        // `[01:02:03]` letto come ore cadeva a un'ora e due minuti, e il testo
        // non si accendeva mai.
        assert_eq!(tempo_ms("01:02:03"), Some(62_030));
        assert_eq!(tempo_ms("00:12:5"), Some(12_500));
        // Con la frazione restano ore, minuti e secondi.
        assert_eq!(tempo_ms("01:02:03.00"), Some(3_723_000));
        // I secondi oltre il 59 non sono un tempo in nessuna delle due forme.
        assert_eq!(tempo_ms("00:75:10"), None);
        assert_eq!(tempo_ms("00:12:"), None);
        let testo = leggi("[00:12:34]prima riga\n[00:15:00]seconda riga");
        assert_eq!(
            testo.righe.iter().map(|r| r.ms).collect::<Vec<_>>(),
            vec![12_340, 15_000]
        );
    }

    #[test]
    fn il_cr_nudo_chiude_una_riga() {
        let testo = leggi("[00:01.00]prima riga\r[00:02.00]seconda riga\r");
        assert_eq!(testo.righe.len(), 2);
        assert_eq!(
            testo.righe.get(1).map(|r| r.testo.as_str()),
            Some("seconda riga")
        );
    }

    #[test]
    fn la_traduzione_allo_stesso_tempo_si_accende_con_l_originale() {
        let testo = leggi(
            "[00:10.00]prima riga\n[00:10.00]first line\n[00:14.00]seconda riga\n[00:14.00]second line",
        );
        assert_eq!(testo.righe.len(), 2, "una riga per tempo: {testo:?}");
        let prima = testo.righe.first().expect("la prima");
        assert_eq!(prima.testo, "prima riga");
        assert_eq!(prima.secondaria.as_deref(), Some("first line"));
        // L'originale è quella che si accende, e non la traduzione.
        assert_eq!(riga_attiva(&testo.righe, 10_500), Some(0));

        // E si riscrive nella forma in cui era arrivata.
        assert_eq!(
            scrivi(&testo),
            "[00:10.00]prima riga\n[00:10.00]first line\n[00:14.00]seconda riga\n[00:14.00]second line\n"
        );
        assert_eq!(leggi(&scrivi(&testo)), testo);
    }

    #[test]
    fn un_doppione_o_un_tempo_vuoto_non_sono_traduzioni() {
        let testo = leggi("[00:10.00][00:10.00]la stessa riga");
        assert_eq!(testo.righe.len(), 1);
        assert_eq!(testo.righe.first().and_then(|r| r.secondaria.clone()), None);

        // Una riga di soli tempi seguita dal testo allo stesso tempo: il testo
        // resta la principale.
        let testo = leggi("[00:10.00]\n[00:10.00]la riga vera");
        assert_eq!(
            testo.righe.first().map(|r| r.testo.as_str()),
            Some("la riga vera")
        );
        assert_eq!(testo.righe.first().and_then(|r| r.secondaria.clone()), None);
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
                secondaria: None,
            }],
            ..Testo::default()
        };
        let primo = scrivi(&storto);
        assert_eq!(primo, "[00:12.35]prima riga\n");
        assert_eq!(scrivi(&leggi(&primo)), primo);
    }

    #[test]
    fn due_righe_a_meno_di_un_centesimo_si_fondono_alla_prima_riscrittura() {
        // B2 (fuzzing): una riga piena e una vuota a due millesimi di distanza sono due tempi per
        // `leggi`, ma `scrivi` li arrotonda allo stesso centesimo. La prima riscrittura li porta
        // sulla stessa riga e la lettura dopo li fonde (la riga vuota non è una traduzione): la
        // proprietà `scrivi(leggi(scrivi(x))) == scrivi(x)` vale per le righe a un centesimo o più,
        // e per queste si ferma alla seconda scrittura invece che alla prima.
        let letto = leggi("[00:02.00]uno\n[00:02.002]\n");
        assert_eq!(letto.righe.len(), 2, "per `leggi` i tempi sono distinti");
        let primo = scrivi(&letto);
        assert_eq!(primo, "[00:02.00]uno\n[00:02.00]\n");
        let secondo = scrivi(&leggi(&primo));
        assert_eq!(secondo, "[00:02.00]uno\n", "la lettura le fonde");
        assert_eq!(scrivi(&leggi(&secondo)), secondo, "e da lì è stabile");
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
                    secondaria: None,
                },
                Riga {
                    ms: 20_000,
                    testo: "seconda".to_owned(),
                    parole: vec![Parola {
                        ms: 20_000,
                        testo: "seconda".to_owned(),
                    }],
                    secondaria: None,
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
            secondaria: None,
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
            secondaria: None,
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
            secondaria: None,
        }));
    }

    #[test]
    fn il_pezzo_prima_della_prima_parola_prende_il_tempo_della_riga() {
        // Un LRC esteso che non tagga la prima parola, perché cade sul tempo
        // della riga: senza tempo, quel pezzo spariva dalle parole, e la
        // finestra — che disegna le parole quando ci sono — non lo mostrava.
        let testo = leggi("[00:10.00]prima <00:10.50>riga <00:11.00>qui");
        let riga = testo.righe.first().expect("una riga");
        assert_eq!(riga.testo, "prima riga qui");
        assert_eq!(
            riga.parole.first(),
            Some(&Parola {
                ms: 10_000,
                testo: "prima ".to_owned(),
            })
        );
        assert_eq!(riga.parole.len(), 3);
        assert!(parole_combaciano(riga));
        assert_eq!(leggi(&scrivi(&testo)), testo);

        // Una riga con due tempi è due righe, e il pezzo prende il tempo di
        // ciascuna.
        let due = leggi("[00:10.00][01:10.00]prima <00:10.50>riga");
        assert_eq!(
            due.righe
                .iter()
                .map(|r| r.parole.first().map(|p| p.ms))
                .collect::<Vec<_>>(),
            vec![Some(10_000), Some(70_000)]
        );

        // Un pezzo di soli spazi non è una parola: sparisce come prima.
        let spazi = leggi("[00:10.00] <00:10.00>una <00:10.50>due");
        assert_eq!(
            spazi.righe.first().map(|r| r.parole.len()),
            Some(2),
            "{spazi:?}"
        );
    }

    #[test]
    fn le_intestazioni_non_entrano_nel_testo_piatto() {
        // Un `.lrc` senza tempi ma con le intestazioni: sono tag e non testo,
        // come in un file sincronizzato.
        let testo = leggi("[ar:qualcuno]\n[ti:un titolo]\n[offset:+0]\nprima riga\nseconda");
        assert_eq!(testo.piatto.as_deref(), Some("prima riga\nseconda"));
        // Un tag con del testo dopo, sulla stessa riga: resta il testo.
        let testo = leggi("[ar:qualcuno]prima riga\nseconda");
        assert_eq!(testo.piatto.as_deref(), Some("prima riga\nseconda"));
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
        // subito dopo la prima invece di scambiarle.
        let attacchi = vec![10_000, 9_000];
        let esito = aggancia(&[10_100, 9_100], &attacchi);
        assert!(
            esito.windows(2).all(|due| due.first() < due.get(1)),
            "i tempi devono restare in ordine: {esito:?}"
        );
    }

    #[test]
    fn due_righe_raddrizzate_non_cadono_sullo_stesso_centesimo() {
        // La prima riga battuta 110 ms in anticipo, la seconda subito dopo: la
        // prima si aggancia a 10 100, la seconda non trova un attacco libero e
        // si corregge della mediana (200), cioè cadrebbe a 9 950 — prima della
        // prima. Tenerla dov'era la prima vorrebbe dire due righe allo stesso
        // tempo, che riscritte e rilette diventano una riga e la sua traduzione.
        let battute = vec![9_990, 10_150, 20_200, 30_200, 40_200];
        let attacchi = vec![10_100, 20_000, 30_000, 40_000];
        let esito = aggancia(&battute, &attacchi);
        assert!(
            esito
                .windows(2)
                .all(|due| due[1] >= due[0].saturating_add(10)),
            "a un centesimo l'una dall'altra almeno: {esito:?}"
        );
        assert_ne!(scrivi_tempo(esito[0]), scrivi_tempo(esito[1]));

        let testo = Testo {
            righe: esito
                .iter()
                .enumerate()
                .map(|(posto, ms)| Riga {
                    ms: *ms,
                    testo: format!("riga {posto}"),
                    ..Riga::default()
                })
                .collect(),
            ..Testo::default()
        };
        let riletto = leggi(&scrivi(&testo));
        assert_eq!(riletto.righe.len(), 5, "{riletto:?}");
        assert!(riletto.righe.iter().all(|r| r.secondaria.is_none()));
    }

    #[test]
    fn il_piatto_si_ricava_anche_da_un_testo_sincronizzato() {
        assert_eq!(
            leggi(UNO).come_piatto(),
            "prima riga\nseconda riga\nterza riga\n"
        );
        assert_eq!(leggi("solo parole").come_piatto(), "solo parole");
    }

    // ── i tetti del lettore: gli ingressi ostili ─────────────────────────────
    //
    // Le prove che seguono costruiscono i testi che [`leggi`] non deve più
    // moltiplicare. Girano **una alla volta** (`una_alla_volta`): alcune sono
    // di centinaia di kibibyte, e quattro insieme su un PC carico sono il
    // genere di cosa che lo fa spegnere.

    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static UNA_ALLA_VOLTA: Mutex<()> = Mutex::new(());

    fn una_alla_volta() -> std::sync::MutexGuard<'static, ()> {
        UNA_ALLA_VOLTA
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `[0:1][0:2]…[0:n]`: `n` tempi distinti, nessuno ripetuto.
    fn tempi_distinti(n: usize) -> String {
        (1..=n).map(|s| format!("[0:{s}]")).collect()
    }

    /// `<0:1>x<0:2>x…`: `n` parole con tempi distinti.
    fn parole_distinte(n: usize) -> String {
        (1..=n).map(|s| format!("<0:{s}>x")).collect()
    }

    fn parole_in_tutto(testo: &Testo) -> usize {
        testo.righe.iter().map(|r| r.parole.len()).sum()
    }

    /// Riscrivere un testo già riscritto non lo cambia: la proprietà del modulo,
    /// provata su un ingresso qualunque.
    fn riscrittura_stabile(grezzo: &str) {
        let una = scrivi(&leggi(grezzo));
        let due = scrivi(&leggi(&una));
        assert_eq!(una, due, "la riscrittura non è stabile per {grezzo:.80}");
    }

    #[test]
    fn una_riga_con_centinaia_di_tempi_ne_tiene_al_massimo_trentadue() {
        let _uno = una_alla_volta();
        let testo = leggi(&format!("{}x", tempi_distinti(200)));
        assert!(
            testo.righe.len() <= MASSIMI_TEMPI_PER_RIGA,
            "{} righe da una riga sola",
            testo.righe.len()
        );
        // I primi trentadue restano, nell'ordine in cui il file li dà.
        assert_eq!(testo.righe.len(), MASSIMI_TEMPI_PER_RIGA);
        assert_eq!(testo.righe.last().map(|r| r.ms), Some(32_000));
    }

    #[test]
    fn i_tempi_uguali_di_una_riga_contano_una_volta() {
        let _uno = una_alla_volta();
        // Cento volte lo stesso tempo non sono cento tempi: sono uno, come
        // diceva già la fusione dopo averne costruite cento.
        let testo = leggi(&format!("{}ciao", "[0:5]".repeat(100)));
        assert_eq!(testo.righe.len(), 1);
        // Il trentatreesimo tempo **distinto** si scarta, e un ripetuto prima
        // di lui non ne occupa il posto.
        let tanti = format!(
            "{}{}x",
            "[0:1]".repeat(500),
            tempi_distinti(MASSIMI_TEMPI_PER_RIGA + 1)
        );
        assert_eq!(leggi(&tanti).righe.len(), MASSIMI_TEMPI_PER_RIGA);
    }

    #[test]
    fn la_bomba_dei_tag_e_delle_parole_non_moltiplica_niente() {
        let _uno = una_alla_volta();
        // La forma dei revisori in scala: tempi uguali, parole oltre il tetto.
        let bomba = format!("{}{}", "[0:0]".repeat(300), "<0:0>x".repeat(600));
        let testo = leggi(&bomba);
        assert_eq!(testo.righe.len(), 1);
        let prima = testo.righe.first().expect("una riga");
        assert!(
            prima.parole.len() <= MASSIME_PAROLE_PER_RIGA,
            "{} parole in una riga",
            prima.parole.len()
        );
        assert_eq!(prima.parole.len(), MASSIME_PAROLE_PER_RIGA);
    }

    #[test]
    fn il_prodotto_tempi_per_parole_ha_un_tetto() {
        let _uno = una_alla_volta();
        // Tempi distinti, quindi niente fusione che li salvi: ognuno è una riga
        // con le sue parole.
        let testo = leggi(&format!("{}{}", tempi_distinti(300), parole_distinte(400)));
        assert!(
            parole_in_tutto(&testo) <= MASSIMI_TEMPI_PER_RIGA * MASSIME_PAROLE_PER_RIGA,
            "{} parole in tutto",
            parole_in_tutto(&testo)
        );
    }

    #[test]
    fn le_righe_espanse_hanno_un_tetto() {
        let _uno = una_alla_volta();
        // Una riga per ogni minuto: tempi distinti, nessuna fusione.
        let grezzo: String = (1..=MASSIME_RIGHE_ESPANSE + 1000)
            .map(|minuto| format!("[{minuto}:00]x\n"))
            .collect();
        let testo = leggi(&grezzo);
        assert!(
            testo.righe.len() <= MASSIME_RIGHE_ESPANSE,
            "{} righe",
            testo.righe.len()
        );
        assert_eq!(testo.righe.len(), MASSIME_RIGHE_ESPANSE);
    }

    #[test]
    fn le_parole_espanse_hanno_un_tetto() {
        let _uno = una_alla_volta();
        // Quattrocento righe da cinquecentododici parole: ognuna sta nel suo
        // tetto, tutte insieme no.
        let riga = parole_distinte(MASSIME_PAROLE_PER_RIGA);
        let grezzo: String = (1..=400).map(|n| format!("[{n}:00]{riga}\n")).collect();
        let testo = leggi(&grezzo);
        assert!(
            parole_in_tutto(&testo) <= MASSIME_PAROLE_ESPANSE,
            "{} parole in tutto",
            parole_in_tutto(&testo)
        );
        // Le righe che non ci stanno si scartano per intero, e le prime restano.
        assert!(
            testo
                .righe
                .iter()
                .all(|r| r.parole.len() == MASSIME_PAROLE_PER_RIGA)
        );
        assert_eq!(testo.righe.first().map(|r| r.ms), Some(60_000));
    }

    #[test]
    fn un_testo_di_sole_parentesi_aperte_si_legge_in_tempo_lineare() {
        let _uno = una_alla_volta();
        // Ogni `<` cercava il suo `>` fino in fondo: il quadrato di duecentomila.
        for grezzo in [
            format!("[0:0]{}", "<".repeat(200_000)),
            format!("[0:0]{}>", "<".repeat(200_000)),
            format!("[0:0]{}>", "<0".repeat(100_000)),
        ] {
            let inizio = Instant::now();
            let testo = leggi(&grezzo);
            let durata = inizio.elapsed();
            assert!(
                durata < Duration::from_millis(500),
                "{durata:?} per {} byte",
                grezzo.len()
            );
            assert_eq!(testo.righe.len(), 1);
        }
    }

    /// Gli ingressi ostili della grandezza che un testo ricevuto può avere
    /// ([`MASSIMO_TESTO_BYTE`]) e di quella di una scrittura da un altro
    /// dispositivo (quarantottomila byte), uno per famiglia di difetto. Con un
    /// tetto **largo**: qui non si misura un tempo, si impedisce che tornino i
    /// secondi — o i gigabyte — di prima.
    ///
    /// Le prove strutturali (il numero di righe e di parole del risultato) sono
    /// quelle sopra; questa dice che il lavoro per arrivarci non è esploso.
    #[test]
    fn gli_ingressi_ostili_si_leggono_in_poco_tempo_e_in_poco_spazio() {
        let _uno = una_alla_volta();
        let grande = MASSIMO_TESTO_BYTE;
        // `5·T + 6·W ≤ byte`, metà dei byte ciascuno: il massimo di `T·W`.
        let bomba = |tempi: usize, parole: usize| {
            format!("{}{}", "[0:0]".repeat(tempi), "<0:0>x".repeat(parole))
        };
        let famiglie: Vec<(&str, String)> = vec![
            ("la bomba da 48 KB", bomba(4_800, 4_000)),
            ("la bomba da 256 KiB", bomba(26_214, 21_845)),
            (
                "tempi distinti in una riga sola",
                format!("{}x", tempi_distinti(28_000)),
            ),
            (
                "tante righe con un tempo distinto",
                (1..=30_000).map(|m| format!("[{m}:00]x\n")).collect(),
            ),
            (
                "tante righe diverse allo stesso tempo",
                (1..=45_000).map(|n| format!("[0:0]r{n}\n")).collect(),
            ),
            (
                "tanti tag di intestazione",
                format!("{}[0:1]x", "[ar:x]".repeat(43_000)),
            ),
            (
                "parole oltre il tetto, in tante righe",
                (1..=87)
                    .map(|n| format!("[{n}:00]{}\n", parole_distinte(MASSIME_PAROLE_PER_RIGA)))
                    .collect(),
            ),
            (
                "una riga sola lunghissima senza tag",
                format!("[0:1]{}", "a".repeat(grande)),
            ),
        ];
        // La bomba dei revisori, a grandezza intera: una riga, e le sue parole
        // nel tetto. Era 19,2 milioni di parole clonate, circa un gigabyte.
        let intera = leggi(&bomba(4_800, 4_000));
        assert_eq!(intera.righe.len(), 1);
        assert_eq!(parole_in_tutto(&intera), MASSIME_PAROLE_PER_RIGA);
        for (nome, grezzo) in famiglie {
            let inizio = Instant::now();
            let testo = leggi(&grezzo);
            let durata = inizio.elapsed();
            let righe = testo.righe.len();
            let parole = parole_in_tutto(&testo);
            eprintln!(
                "{nome}: {} byte, {durata:?}, {righe} righe, {parole} parole",
                grezzo.len()
            );
            assert!(
                durata < Duration::from_secs(3),
                "{nome}: {durata:?} per {} byte",
                grezzo.len()
            );
            assert!(righe <= MASSIME_RIGHE_ESPANSE, "{nome}: {righe} righe");
            assert!(parole <= MASSIME_PAROLE_ESPANSE, "{nome}: {parole} parole");
        }
    }

    #[test]
    fn le_righe_diverse_allo_stesso_tempo_hanno_un_tetto() {
        let _uno = una_alla_volta();
        // Una traduzione e una pronuncia sono normali, e restano tutte.
        let normale = leggi("[0:1]originale\n[0:1]traduzione\n[0:1]pronuncia");
        assert_eq!(normale.righe.len(), 1);
        assert_eq!(
            normale.righe.first().and_then(|r| r.secondaria.as_deref()),
            Some("traduzione\npronuncia")
        );
        // Mille righe diverse allo stesso tempo, no.
        let grezzo: String = (0..1000).map(|n| format!("[0:1]r{n}\n")).collect();
        let testo = leggi(&grezzo);
        let prima = testo.righe.first().expect("una riga");
        assert_eq!(
            prima.secondaria.as_deref().map_or(0, |s| s.lines().count()),
            MASSIME_SECONDARIE_PER_RIGA
        );
        riscrittura_stabile(&grezzo);
    }

    #[test]
    fn uno_scarto_oltre_un_ora_non_e_uno_scarto() {
        assert_eq!(leggi("[offset:2000000000]\n[00:01.00]x").offset_ms, 0);
        assert_eq!(leggi("[offset:-2000000000]\n[00:01.00]x").offset_ms, 0);
        assert_eq!(
            leggi("[offset:3600000]\n[00:01.00]x").offset_ms,
            MASSIMO_OFFSET_MS
        );
        assert_eq!(leggi("[offset:-250]\n[00:01.00]x").offset_ms, -250);
    }

    #[test]
    fn la_riga_che_supera_le_parole_finisce_alla_parola_prima() {
        let _uno = una_alla_volta();
        // Cinquecentododici parole, l'ultima che finisce con un `<0` che
        // diventerebbe un tag se le si accostasse a `:0>y` dopo il tag scartato.
        let mut grezzo = String::from("[0:0]");
        grezzo.push_str(&parole_distinte(MASSIME_PAROLE_PER_RIGA - 1));
        grezzo.push_str("<0:600>ultima<0<0:601>:0>y");
        let testo = leggi(&grezzo);
        let prima = testo.righe.first().expect("una riga");
        assert_eq!(prima.parole.len(), MASSIME_PAROLE_PER_RIGA);
        // Il tag di troppo si scarta, e con lui tutto quel che segue: né `:0>`
        // né `y` si accodano all'ultima parola.
        assert_eq!(
            prima.parole.last().map(|p| p.testo.as_str()),
            Some("ultima<0")
        );
        assert!(!prima.testo.contains('y'));
        assert!(!scrivi(&testo).contains(":0>y"));
        riscrittura_stabile(&grezzo);
    }

    #[test]
    fn il_pezzo_prima_della_prima_parola_conta_fra_le_parole_della_riga() {
        let _uno = una_alla_volta();
        // B2 (fuzzing): `Oh <0:1>…` non ha un tag per il pezzo davanti, ma `leggi` lo trasforma in
        // una parola con il tempo della riga e `scrivi` lo scrive con un tag. Con cinquecentododici
        // parole con un tempo la riga ne aveva cinquecentotredici, e la rilettura dell'ultima non
        // ne vedeva il tag: ogni giro ne perdeva una, e `scrivi(leggi(scrivi(x))) != scrivi(x)`.
        let mut grezzo = String::from("[0:0]Oh ");
        grezzo.push_str(&parole_distinte(MASSIME_PAROLE_PER_RIGA + 5));
        let testo = leggi(&grezzo);
        let prima = testo.righe.first().expect("una riga");
        assert_eq!(
            prima.parole.len(),
            MASSIME_PAROLE_PER_RIGA,
            "il pezzo prima conta come una parola"
        );
        assert_eq!(prima.parole.first().map(|p| p.testo.as_str()), Some("Oh "));
        assert_eq!(
            leggi(&scrivi(&testo)).righe.first().map(|r| r.parole.len()),
            Some(MASSIME_PAROLE_PER_RIGA),
            "e la rilettura ne trova altrettante"
        );
        riscrittura_stabile(&grezzo);
        // Sotto il tetto il pezzo davanti non toglie niente a nessuno.
        let mut meno = String::from("[0:0]Oh ");
        meno.push_str(&parole_distinte(MASSIME_PAROLE_PER_RIGA - 1));
        let senza_taglio = leggi(&meno);
        assert_eq!(
            senza_taglio.righe.first().map(|r| r.parole.len()),
            Some(MASSIME_PAROLE_PER_RIGA)
        );
        riscrittura_stabile(&meno);
    }

    #[test]
    fn riscrivere_i_testi_ostili_non_cambia_niente_due_volte() {
        let _uno = una_alla_volta();
        let con_il_tetto = |tempi: usize, parole: usize| {
            format!("{}{}", tempi_distinti(tempi), parole_distinte(parole))
        };
        for grezzo in [
            // La bomba dei revisori, in scala.
            format!("{}{}", "[0:0]".repeat(300), "<0:0>x".repeat(600)),
            // Un tempo sotto il tetto, uno sopra, uno molto sopra.
            con_il_tetto(MASSIMI_TEMPI_PER_RIGA - 1, 3),
            con_il_tetto(MASSIMI_TEMPI_PER_RIGA, 3),
            con_il_tetto(MASSIMI_TEMPI_PER_RIGA + 1, 3),
            con_il_tetto(200, 3),
            // Una parola sotto il tetto, una sopra, molto sopra.
            con_il_tetto(2, MASSIME_PAROLE_PER_RIGA - 1),
            con_il_tetto(2, MASSIME_PAROLE_PER_RIGA),
            con_il_tetto(2, MASSIME_PAROLE_PER_RIGA + 1),
            con_il_tetto(2, 900),
            // Il pezzo che accostato a un altro comporrebbe un tag.
            format!(
                "[0:0]{}x<0<0:1>:0>y",
                parole_distinte(MASSIME_PAROLE_PER_RIGA)
            ),
            // Tag e parentesi che non sono tag, in mezzo e in fondo.
            "[0:0]<<0:0>0:0>[0:1]<0:1>>>[x]<".to_owned(),
            format!("[0:0]{}", "<".repeat(5_000)),
            format!("{}[offset:9999999999]\n[0:1]x", "[ar:x]".repeat(40)),
        ] {
            riscrittura_stabile(&grezzo);
        }
    }
}
