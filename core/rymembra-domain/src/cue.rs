//! Immagini CD: un file audio solo per tutto il disco, e un `.cue` che lo taglia.
//!
//! È il modo in cui EAC e i suoi simili salvano un CD «com'è»: un `Album.flac`
//! (o `.ape`, o `.wav`) da settanta minuti e accanto un foglio di testo che dice
//! dove comincia ogni brano. Senza il foglio la libreria vede un brano solo,
//! lungo quanto il disco, intitolato come l'album; con il foglio i brani sono
//! quelli veri.
//!
//! # L'identità di un brano d'immagine
//!
//! Ogni brano diventa una voce **virtuale** della camminata, con il percorso
//! `…\Album.cue|03`. Il `|` non può stare nel nome di un file Windows, quindi
//! non c'è un file vero che si possa confondere con quella voce; e il numero a
//! due cifre ordina i brani come li ordina il disco. Così tutto quel che a valle
//! ragiona per percorso — l'unicità della riga, il piano della scansione,
//! l'albero delle cartelle, le playlist — continua a funzionare senza sapere
//! che quel percorso non si apre.
//!
//! # Perché i fogli si leggono qui e non accanto al disco
//!
//! Le decisioni che contano sono tre, e tutte e tre possono sbagliare un disco
//! intero: se un foglio descrive davvero un'immagine, quale file della cartella
//! è l'immagine, e come le voci virtuali prendono il posto del file vero. Sono
//! state misurate sui 620 fogli di una libreria reale, e i casi che sbagliavano
//! — la cartella di brani separati con un foglio rimasto da un vecchio rip, che
//! un «prendi l'unico file grande» avrebbe tagliato a pezzi — stanno nei test
//! come chiamate di funzione.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use encoding_rs::{Encoding, WINDOWS_1252};

use crate::paths::{PathRules, base_name, file_stem, is_supported_audio_path, path_key};
use crate::scan_plan::DiscoveredFile;

/// Il carattere che separa il percorso del `.cue` dal numero del brano nei
/// percorsi virtuali: `…\Album.cue|03`. Vietato nei nomi di file Windows.
pub const SEPARATORE_BRANO: char = '|';

/// Un foglio `.cue` che descrive un'immagine: un solo file, almeno due brani.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foglio {
    /// Il nome scritto dopo `FILE`, senza virgolette e senza il tipo.
    ///
    /// È il nome che il programma di rip ha dato all'immagine **quando l'ha
    /// scritta**: spesso un `.wav` poi compresso in `.flac` o `.ape` senza
    /// toccare il foglio. Per questo non si apre mai così com'è: lo risolve
    /// [`trova_immagine`].
    pub file: String,
    /// Il `TITLE` del disco.
    pub titolo: Option<String>,
    /// Il `PERFORMER` del disco.
    pub interprete: Option<String>,
    /// `REM DATE`, così come è scritto.
    pub data: Option<String>,
    /// `REM GENRE`.
    pub genere: Option<String>,
    /// `REM DISCNUMBER`.
    pub disco: Option<u32>,
    /// Almeno due, nell'ordine del foglio, con inizi strettamente crescenti.
    pub tracce: Vec<TracciaCue>,
}

/// Un brano del foglio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracciaCue {
    /// `TRACK NN`, fra 1 e 99.
    pub numero: u8,
    /// Il `TITLE` del brano.
    pub titolo: Option<String>,
    /// Il `PERFORMER` del brano.
    pub interprete: Option<String>,
    /// `INDEX 01` in fotogrammi CD (1/75 s), dall'inizio del file.
    ///
    /// In fotogrammi e non in millisecondi perché è l'unità del foglio: la
    /// conversione la fa chi suona, in campioni, in modo esatto. Arrotondare
    /// qui vorrebbe dire due brani contigui che si sovrappongono o lasciano un
    /// buco di qualche campione, e su un disco dal vivo quel buco si sente.
    pub inizio_cd: u32,
}

impl Foglio {
    /// La fine del brano `i` (indice in `tracce`) = inizio del successivo; `None` per l'ultimo (fino alla fine dell'immagine).
    ///
    /// Le pause fra un brano e l'altro (`INDEX 00`) stanno in coda al
    /// precedente, come le suona un lettore CD: così il disco, ascoltato di
    /// fila, non perde nemmeno un fotogramma.
    #[must_use]
    pub fn fine_cd(&self, i: usize) -> Option<u32> {
        let successivo = i.checked_add(1)?;
        self.tracce.get(successivo).map(|traccia| traccia.inizio_cd)
    }
}

/// Legge un foglio dai suoi byte, decodificati così: BOM UTF-8, BOM UTF-16 LE/BE, UTF-8 valido, altrimenti cp1252 (`encoding_rs`).
/// `None` se non descrive un'immagine: zero o più di un FILE, meno di 2 TRACK, INDEX 01 mancanti o non strettamente crescenti.
///
/// Anche numeri di traccia fuori da 1..=99 o ripetuti, e tempi malformati
/// sull'`INDEX 01`, rendono il foglio inutilizzabile: un brano senza un inizio
/// certo non si può tagliare, e tagliarlo a caso vuol dire suonare il pezzo
/// sbagliato con il titolo giusto.
///
/// # Perché più di un `FILE` non è un'immagine
///
/// Perché vuol dire un file per brano: è il foglio che accompagna i brani
/// separati, e quei brani la camminata li trova già da sé. Espanderli due
/// volte darebbe ogni brano doppio.
#[must_use]
pub fn leggi_foglio(byte: &[u8]) -> Option<Foglio> {
    let testo = decodifica(byte);

    let mut quanti_file = 0_usize;
    let mut file = None;
    let mut titolo = None;
    let mut interprete = None;
    let mut data = None;
    let mut genere = None;
    let mut disco = None;
    let mut tracce: Vec<Bozza> = Vec::new();

    // `\r` e `\n` tutti e due: un foglio scritto su un Mac di vent'anni fa va
    // a capo col solo `\r`, e le righe vuote che il CRLF lascia si saltano.
    for riga in testo.split(['\r', '\n']) {
        let Some((parola, resto)) = spezza(riga) else {
            continue;
        };
        if parola.eq_ignore_ascii_case("FILE") {
            quanti_file = quanti_file.saturating_add(1);
            file = pulito(nome_del_file(resto));
        } else if parola.eq_ignore_ascii_case("TRACK") {
            // Un numero che non sta in 1..=99 non si può rappresentare nel
            // percorso virtuale a due cifre: il foglio intero non vale.
            let numero = resto
                .split_whitespace()
                .next()
                .and_then(numero_di_traccia)?;
            tracce.push(Bozza {
                numero,
                titolo: None,
                interprete: None,
                inizio: None,
            });
        } else if parola.eq_ignore_ascii_case("INDEX") {
            let mut parti = resto.split_whitespace();
            let (Some(quale), Some(tempo)) = (parti.next(), parti.next()) else {
                continue;
            };
            // Solo `INDEX 01` dice dove comincia il brano. `INDEX 00` è la
            // pausa che lo precede, e resta in coda al brano prima.
            if cifre(quale) != Some(1) {
                continue;
            }
            let Some(traccia) = tracce.last_mut() else {
                continue;
            };
            if traccia.inizio.is_none() {
                traccia.inizio = Some(fotogrammi(tempo)?);
            }
        } else if parola.eq_ignore_ascii_case("TITLE") {
            let campo = match tracce.last_mut() {
                Some(traccia) => &mut traccia.titolo,
                None => &mut titolo,
            };
            if campo.is_none() {
                *campo = pulito(valore(resto));
            }
        } else if parola.eq_ignore_ascii_case("PERFORMER") {
            let campo = match tracce.last_mut() {
                Some(traccia) => &mut traccia.interprete,
                None => &mut interprete,
            };
            if campo.is_none() {
                *campo = pulito(valore(resto));
            }
        } else if parola.eq_ignore_ascii_case("REM") && tracce.is_empty() {
            // Le `REM` del disco sono quelle prima del primo brano; dopo
            // descriverebbero il brano, e di quelle non serve niente.
            let Some((chiave, dopo)) = spezza(resto) else {
                continue;
            };
            if chiave.eq_ignore_ascii_case("DATE") {
                data = data.or_else(|| pulito(valore(dopo)));
            } else if chiave.eq_ignore_ascii_case("GENRE") {
                genere = genere.or_else(|| pulito(valore(dopo)));
            } else if chiave.eq_ignore_ascii_case("DISCNUMBER") {
                disco = disco.or_else(|| numero_del_disco(valore(dopo)));
            }
        }
    }

    if quanti_file != 1 || tracce.len() < 2 {
        return None;
    }
    let file = file?;

    let mut visti = HashSet::with_capacity(tracce.len());
    let mut precedente: Option<u32> = None;
    let mut fatte = Vec::with_capacity(tracce.len());
    for bozza in tracce {
        let inizio = bozza.inizio?;
        if !visti.insert(bozza.numero) || precedente.is_some_and(|prima| inizio <= prima) {
            return None;
        }
        precedente = Some(inizio);
        fatte.push(TracciaCue {
            numero: bozza.numero,
            titolo: bozza.titolo,
            interprete: bozza.interprete,
            inizio_cd: inizio,
        });
    }

    Some(Foglio {
        file,
        titolo,
        interprete,
        data,
        genere,
        disco,
        tracce: fatte,
    })
}

/// Un brano mentre il foglio si legge: l'inizio può ancora mancare.
struct Bozza {
    numero: u8,
    titolo: Option<String>,
    interprete: Option<String>,
    inizio: Option<u32>,
}

/// I byte del foglio come testo.
///
/// Le codifiche sono quelle misurate: su 620 fogli veri, 523 in UTF-8 (con o
/// senza BOM), 96 in windows-1252 — il Blocco note di un Windows italiano,
/// dove «Canterò» è un byte solo — e uno in UTF-16, col suo BOM.
fn decodifica(byte: &[u8]) -> Cow<'_, str> {
    if let Some((codifica, lunghezza_bom)) = Encoding::for_bom(byte) {
        let resto = byte.get(lunghezza_bom..).unwrap_or_default();
        return codifica.decode_without_bom_handling(resto).0;
    }
    match std::str::from_utf8(byte) {
        Ok(testo) => Cow::Borrowed(testo),
        Err(_) => WINDOWS_1252.decode_without_bom_handling(byte).0,
    }
}

/// La parola chiave in testa alla riga e il resto, senza spazi intorno.
fn spezza(riga: &str) -> Option<(&str, &str)> {
    let riga = riga.trim();
    if riga.is_empty() {
        return None;
    }
    match riga.find(char::is_whitespace) {
        Some(taglio) => Some((riga.get(..taglio)?, riga.get(taglio..)?.trim_start())),
        None => Some((riga, "")),
    }
}

/// Un valore fra virgolette o senza.
///
/// Fra virgolette arriva fino all'**ultima**: un titolo con delle virgolette
/// dentro, che lo standard non prevede ma qualche programma scrive lo stesso,
/// resta intero. Senza virgolette vale tutto il resto della riga, spazi
/// compresi: `TITLE Via del Campo` è scorretto, ma il titolo è quello.
fn valore(resto: &str) -> &str {
    let resto = resto.trim();
    match resto.strip_prefix('"') {
        Some(dentro) => match dentro.rfind('"') {
            Some(fine) => dentro.get(..fine).unwrap_or(dentro),
            None => dentro,
        },
        None => resto,
    }
}

/// Il nome dopo `FILE`, senza il tipo che lo segue (`WAVE`, `MP3`, `BINARY`…).
///
/// Senza virgolette il tipo è l'ultima parola, se ce n'è più d'una: così
/// `FILE a b.wav WAVE` dà `a b.wav`, e `FILE a.wav` senza tipo dà `a.wav`.
fn nome_del_file(resto: &str) -> &str {
    let resto = resto.trim();
    if resto.starts_with('"') {
        return valore(resto);
    }
    match resto.rfind(char::is_whitespace) {
        Some(taglio) => resto.get(..taglio).map_or(resto, str::trim_end),
        None => resto,
    }
}

/// Il testo senza caratteri di controllo e senza spazi intorno; `None` se non
/// resta niente.
fn pulito(grezzo: &str) -> Option<String> {
    let senza: String = grezzo.chars().filter(|c| !c.is_control()).collect();
    let senza = senza.trim();
    (!senza.is_empty()).then(|| senza.to_owned())
}

/// Un numero di sole cifre ASCII. `str::parse` accetterebbe anche `+3`.
fn cifre(testo: &str) -> Option<u32> {
    if testo.is_empty() || !testo.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    testo.parse().ok()
}

/// `TRACK NN`, fra 1 e 99.
fn numero_di_traccia(testo: &str) -> Option<u8> {
    let numero = u8::try_from(cifre(testo)?).ok()?;
    (1..=99).contains(&numero).then_some(numero)
}

/// `REM DISCNUMBER 2`, anche nella forma `2/3`. Il disco zero non esiste.
fn numero_del_disco(testo: &str) -> Option<u32> {
    let prima = testo.split('/').next().unwrap_or_default().trim();
    cifre(prima).filter(|&numero| numero > 0)
}

/// `mm:ss:ff` in fotogrammi CD: (mm·60 + ss)·75 + ff.
///
/// I minuti possono superare 99 — un'immagine di un doppio dal vivo arriva a
/// 150 — e quindi non hanno un numero fisso di cifre. Secondi e fotogrammi
/// fuori misura vogliono dire un foglio rotto, non un tempo da indovinare.
fn fotogrammi(tempo: &str) -> Option<u32> {
    let mut parti = tempo.split(':');
    let minuti = cifre(parti.next()?)?;
    let secondi = cifre(parti.next()?)?;
    let fotogramma = cifre(parti.next()?)?;
    if parti.next().is_some() || secondi >= 60 || fotogramma >= 75 {
        return None;
    }
    minuti
        .checked_mul(60)?
        .checked_add(secondi)?
        .checked_mul(75)?
        .checked_add(fotogramma)
}

/// Fra i file della cartella del `.cue` (soli nomi), quale è l'immagine. Regole in ordine:
///
/// 1. nome esatto del FILE (senza distinguere maiuscole);
/// 2. stesso nome base del FILE con estensione audio supportata (incluso `ape`);
/// 3. stesso nome base del `.cue` con estensione audio supportata.
///
/// Mai «l'unico file grande»: sbaglia sulle cartelle con brani separati. `None` se nessuna regola trova.
///
/// # Le regole, misurate
///
/// Sui fogli veri: 260 trovano il FILE esatto; 174 lo trovano con un'altra
/// estensione — il rip di EAC in `.wav`, compresso dopo in `.flac` o `.ape`
/// senza correggere il foglio —; 4 solo col nome del `.cue`: due hanno un FILE
/// senza estensione ma con un punto nel nome (`…BWV.1019-1023`), che la regola
/// 2 taglia nel posto sbagliato, e due un FILE e un nome sul disco passati per
/// due codifiche diverse (`Klavierstücke` contro `Klavierstьcke`, `¹2` contro
/// `№2`). Ventitré non trovano niente, ed è giusto: ventidue sono cartelle di
/// brani già separati con un foglio che nomina un'immagine che non c'è più —
/// ventuno di De André col loro `Range.wav` —, e una è un `.wv`. «L'unico file
/// grande della cartella» lì avrebbe preso il primo brano e lo avrebbe
/// tagliato in quattordici pezzi.
///
/// In ogni regola il candidato deve essere un formato che sappiamo leggere:
/// un'immagine `.wv` (WavPack) col nome giusto resta fuori, perché un brano
/// d'immagine che non si apre è peggio dell'immagine che non c'è. E del FILE
/// conta l'ultimo segmento: qualche programma ci scrive il percorso intero
/// della macchina che ha fatto il rip.
///
/// A pari regola vince il nome più piccolo, così la risposta non dipende
/// dall'ordine in cui il disco elenca la cartella.
#[must_use]
pub fn trova_immagine<'a>(nome_cue: &str, foglio: &Foglio, vicini: &[&'a str]) -> Option<&'a str> {
    per_regola(nome_cue, foglio, vicini)
        .into_iter()
        .flatten()
        .next()
}

/// La risposta di ognuna delle tre regole di [`trova_immagine`], nell'ordine:
/// il nome che ciascuna sceglierebbe da sola, o `None`.
///
/// Tutte e tre e non solo la prima che trova, per [`espandi_immagini`]: quando
/// il nome che la prima regola sceglie è già di un altro foglio, si prova con
/// la regola dopo invece di arrendersi. Il nome che la regola sceglie e basta,
/// non gli altri che la passano: due immagini dello stesso nome base nella
/// stessa cartella (`Album.flac` e `Album.ape`) sono lo stesso disco in due
/// formati, e dare la seconda al foglio gemello che ha perso la prima vorrebbe
/// dire ogni brano due volte.
fn per_regola<'a>(nome_cue: &str, foglio: &Foglio, vicini: &[&'a str]) -> [Option<&'a str>; 3] {
    let audio: Vec<&'a str> = vicini
        .iter()
        .copied()
        .filter(|nome| is_supported_audio_path(nome))
        .collect();
    let scritto = base_name(&foglio.file);

    let esatto = scritto.to_lowercase();
    let radice_file = file_stem(scritto).to_lowercase();
    let radice_cue = file_stem(base_name(nome_cue)).to_lowercase();

    [
        il_primo(&audio, |nome| base_name(nome).to_lowercase() == esatto),
        il_primo(&audio, |nome| file_stem(nome).to_lowercase() == radice_file),
        il_primo(&audio, |nome| file_stem(nome).to_lowercase() == radice_cue),
    ]
}

/// Il più piccolo dei nomi che passano la regola, in ordine indipendente dal
/// caso e poi esatto.
fn il_primo<'a>(nomi: &[&'a str], regola: impl Fn(&str) -> bool) -> Option<&'a str> {
    nomi.iter()
        .copied()
        .filter(|nome| regola(nome))
        .min_by(|a, b| {
            a.to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| a.cmp(b))
        })
}

/// `…\Album.cue` + 3 → `…\Album.cue|03`.
///
/// Il numero viene dal foglio, quindi è fra 1 e 99 e le cifre sono due: è
/// quel che permette a [`brano_d_immagine`] di riconoscere il percorso.
#[must_use]
pub fn percorso_del_brano(percorso_cue: &str, numero: u8) -> String {
    format!("{percorso_cue}{SEPARATORE_BRANO}{numero:02}")
}

/// Il contrario: `Some((percorso_cue, numero))` solo se la parte prima di `|` finisce in `.cue` e dopo ci sono 2 cifre.
///
/// Le due cifre devono fare un brano vero: `|00` non lo è, perché nessun
/// foglio ha una traccia zero. L'estensione si guarda senza distinguere le
/// maiuscole: un `ALBUM.CUE` sul disco dà `ALBUM.CUE|01`.
#[must_use]
pub fn brano_d_immagine(percorso: &str) -> Option<(&str, u8)> {
    let (cue, numero) = percorso.rsplit_once(SEPARATORE_BRANO)?;
    if numero.len() != 2 || !numero.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let numero = numero_di_traccia(numero)?;
    // Come `extension_of`, ma senza allocare: questa domanda se la fa il piano
    // per ogni file che non è musica.
    let nome = base_name(cue);
    let punto = nome.rfind('.').filter(|&punto| punto > 0)?;
    nome.get(punto.checked_add(1)?..)?
        .eq_ignore_ascii_case("cue")
        .then_some((cue, numero))
}

/// Un `.cue` trovato e letto, con il file trovato dalla camminata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoglioTrovato {
    /// Il `.cue` com'è sul disco.
    pub cue: DiscoveredFile,
    /// Quel che dice.
    pub foglio: Foglio,
}

/// Un'immagine abbinata al suo foglio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abbinamento {
    /// Il `.cue`.
    pub cue: DiscoveredFile,
    /// Il file audio vero, quello che si apre.
    pub immagine: DiscoveredFile,
    /// Il foglio letto.
    pub foglio: Foglio,
}

/// La camminata con le immagini sostituite dai loro brani.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Espansione {
    /// I file trovati, senza le immagini abbinate e con una voce virtuale per
    /// ogni brano al loro posto.
    pub trovati: Vec<DiscoveredFile>,
    /// Le immagini abbinate, in ordine di percorso del `.cue`.
    pub abbinamenti: Vec<Abbinamento>,
}

/// Toglie da `trovati` le immagini abbinate e aggiunge una voce virtuale per traccia.
/// - path = `percorso_del_brano(cue.path, numero)`;
/// - size_bytes = dimensione dell'IMMAGINE;
/// - modified_ms = max(cue.modified_ms, immagine.modified_ms).
///
/// Butta ogni file vero il cui percorso sembri virtuale (`brano_d_immagine` → Some).
/// Le immagini si cercano fra i `trovati` della stessa cartella del `.cue`, con la cartella confrontata con `path_key(rules)`.
/// Un'immagine reclamata da due fogli va a quello che ha già dei brani in
/// libreria — `con_brani` sono le chiavi (`path_key(rules)`) dei `.cue` che ne
/// hanno — e a parità al primo in ordine di percorso del `.cue`.
///
/// # Perché proprio quella dimensione e quella data
///
/// Sono i due numeri che la riga del brano salva, e che il piano confronta alla
/// scansione successiva. Se il foglio cambia, la data più recente è la sua e i
/// brani si rileggono; se cambia l'immagine, idem. Se non cambia niente, i
/// numeri tornano uguali e nessun brano si rilegge — e un disco da quattordici
/// brani è quattordici righe, da moltiplicare per qualche centinaio di dischi.
///
/// # L'ordine
///
/// Le voci virtuali stanno dove stava l'immagine, nell'ordine del foglio; il
/// resto resta nell'ordine della camminata. Stessi ingressi, stessa uscita.
///
/// # Due fogli per la stessa immagine
///
/// Non è un'eccezione: sul NAS misurato sono 138 su 438. EAC scrive il foglio
/// per il `.wav`, chi comprime ne aggiunge uno per il `.flac` e lascia
/// l'altro — `Album.cue` e `Album.flac.cue`, `… WAV.cue` e `… FLAC.cue` — e i
/// due dicono la stessa cosa a parte il nome dopo `FILE` (in un caso anche il
/// titolo del disco). Espanderli tutti e due darebbe ogni brano doppio; ne
/// vale uno, e sempre lo stesso, perché la scelta non deve cambiare fra una
/// scansione e l'altra.
///
/// «Sempre lo stesso» vuol dire **quello che c'era già**, prima ancora che il
/// primo in ordine di percorso. Un `.cue` nuovo che si ordina prima del
/// vincitore — un `Album (FLAC).cue` comparso accanto a un `Album.cue` — non
/// gli ruba l'immagine: se gliela rubasse cambierebbero tutti i percorsi
/// virtuali del disco, e con i titoli diversi fra i due fogli gli spostamenti
/// non si riconoscerebbero, cioè ascolti, voti e playlist del disco persi.
/// L'ordine di percorso decide solo fra fogli che di brani non ne hanno, o che
/// ne hanno tutti e due.
///
/// E chi perde non si arrende subito: se il nome che la prima regola di
/// [`trova_immagine`] sceglie è già preso, prova quello della regola dopo. È
/// il foglio di `CD2` copiato da quello di `CD1` senza correggere il `FILE`:
/// il `FILE` porta a `CD1.flac`, che è dell'altro, ma il suo nome porta a
/// `CD2.flac`, che è il suo.
///
/// Un file vero che sembra virtuale può esistere solo dove `|` è ammesso nei
/// nomi: tenerlo vorrebbe dire due righe con lo stesso percorso, una delle
/// quali punta a un brano che non c'è.
#[must_use]
pub fn espandi_immagini(
    trovati: Vec<DiscoveredFile>,
    fogli: Vec<FoglioTrovato>,
    con_brani: &HashSet<String>,
    rules: PathRules,
) -> Espansione {
    let trovati: Vec<DiscoveredFile> = trovati
        .into_iter()
        .filter(|file| brano_d_immagine(&file.path).is_none())
        .collect();

    // I candidati di ogni cartella: solo musica, così il foglio stesso e le
    // copertine non entrano nemmeno nella domanda.
    let mut per_cartella: HashMap<String, Vec<usize>> = HashMap::new();
    for (indice, file) in trovati.iter().enumerate() {
        if is_supported_audio_path(&file.path) {
            per_cartella
                .entry(path_key(cartella(&file.path), rules))
                .or_default()
                .push(indice);
        }
    }

    // Prima chi ha già i brani in libreria, poi in ordine di percorso: è
    // l'ordine in cui i fogli scelgono, e chi sceglie prima vince.
    let mut fogli = fogli;
    fogli.sort_by_cached_key(|trovato| {
        let chiave = path_key(&trovato.cue.path, rules);
        (
            !con_brani.contains(&chiave),
            chiave,
            trovato.cue.path.clone(),
        )
    });

    let mut cue_visti: HashSet<String> = HashSet::with_capacity(fogli.len());
    // Le immagini già prese, per indice nei trovati.
    let mut prese: HashSet<usize> = HashSet::new();
    // Ogni abbinamento con l'indice della sua immagine.
    let mut scelti: Vec<(usize, Abbinamento)> = Vec::new();
    for FoglioTrovato { cue, foglio } in fogli {
        // Lo stesso `.cue` due volte (la camminata che lo vede in due grafie)
        // darebbe due volte gli stessi percorsi virtuali.
        if !cue_visti.insert(path_key(&cue.path, rules)) {
            continue;
        }
        let Some(indici) = per_cartella.get(&path_key(cartella(&cue.path), rules)) else {
            continue;
        };
        let candidati: Vec<(usize, &str)> = indici
            .iter()
            .filter_map(|&indice| Some((indice, base_name(&trovati.get(indice)?.path))))
            .collect();
        let nomi: Vec<&str> = candidati.iter().map(|&(_, nome)| nome).collect();
        // La prima regola che porta a un'immagine non ancora presa: una già
        // presa resta di chi ha scelto prima.
        let libera = per_regola(base_name(&cue.path), &foglio, &nomi)
            .into_iter()
            .flatten()
            .filter_map(|scelto| candidati.iter().find(|&&(_, nome)| nome == scelto))
            .map(|&(indice, _)| indice)
            .find(|indice| !prese.contains(indice));
        let Some(indice) = libera else {
            continue;
        };
        let Some(immagine) = trovati.get(indice).cloned() else {
            continue;
        };
        prese.insert(indice);
        scelti.push((
            indice,
            Abbinamento {
                cue,
                immagine,
                foglio,
            },
        ));
    }
    // Gli abbinamenti escono in ordine di percorso del `.cue`, comunque
    // abbiano scelto: è l'ordine che [`Espansione::abbinamenti`] promette.
    scelti.sort_by_cached_key(|(_, abbinamento)| {
        (
            path_key(&abbinamento.cue.path, rules),
            abbinamento.cue.path.clone(),
        )
    });
    // Indice dell'immagine nei trovati → indice dell'abbinamento.
    let reclamate: HashMap<usize, usize> = scelti
        .iter()
        .enumerate()
        .map(|(posto, &(indice, _))| (indice, posto))
        .collect();
    let abbinamenti: Vec<Abbinamento> = scelti
        .into_iter()
        .map(|(_, abbinamento)| abbinamento)
        .collect();

    let in_piu: usize = abbinamenti
        .iter()
        .map(|abbinamento| abbinamento.foglio.tracce.len())
        .sum();
    let mut uscita = Vec::with_capacity(trovati.len().saturating_add(in_piu));
    for (indice, file) in trovati.into_iter().enumerate() {
        match reclamate
            .get(&indice)
            .and_then(|&quale| abbinamenti.get(quale))
        {
            Some(abbinamento) => uscita.extend(voci_virtuali(abbinamento)),
            None => uscita.push(file),
        }
    }

    Espansione {
        trovati: uscita,
        abbinamenti,
    }
}

/// Le voci virtuali di un'immagine: una per brano, con la dimensione
/// dell'immagine e la data più recente fra foglio e immagine.
fn voci_virtuali(abbinamento: &Abbinamento) -> impl Iterator<Item = DiscoveredFile> + '_ {
    let modificato = abbinamento
        .cue
        .modified_ms
        .max(abbinamento.immagine.modified_ms);
    abbinamento
        .foglio
        .tracce
        .iter()
        .map(move |traccia| DiscoveredFile {
            path: percorso_del_brano(&abbinamento.cue.path, traccia.numero),
            size_bytes: abbinamento.immagine.size_bytes,
            modified_ms: modificato,
        })
}

/// La cartella che contiene il percorso, con qualunque separatore.
fn cartella(percorso: &str) -> &str {
    match percorso.rfind(['/', '\\']) {
        Some(taglio) => percorso.get(..taglio).unwrap_or_default(),
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: PathRules = PathRules {
        case_insensitive: true,
    };

    /// Un foglio come lo scrive EAC: CRLF, rientri, virgolette, `REM` in testa,
    /// la pausa del secondo brano in `INDEX 00`.
    const EAC: &str = "REM GENRE \"Cantautori\"\r\n\
        REM DATE 1971\r\n\
        REM DISCID 9A0B2C0D\r\n\
        REM COMMENT \"ExactAudioCopy v1.6\"\r\n\
        PERFORMER \"Fabrizio De André\"\r\n\
        TITLE \"Non al denaro non all'amore né al cielo\"\r\n\
        FILE \"Non al denaro.wav\" WAVE\r\n\
        \x20 TRACK 01 AUDIO\r\n\
        \x20   TITLE \"La collina\"\r\n\
        \x20   PERFORMER \"Fabrizio De André\"\r\n\
        \x20   INDEX 01 00:00:00\r\n\
        \x20 TRACK 02 AUDIO\r\n\
        \x20   TITLE \"Un matto\"\r\n\
        \x20   INDEX 00 04:01:20\r\n\
        \x20   INDEX 01 04:03:00\r\n\
        \ttrack 03 audio\r\n\
        \t\ttitle Un giudice\r\n\
        \t\tindex 01 06:40:74\r\n";

    fn letto(testo: &str) -> Foglio {
        leggi_foglio(testo.as_bytes()).expect("un foglio valido")
    }

    #[test]
    fn un_foglio_di_eac_si_legge_intero() {
        let foglio = letto(EAC);
        assert_eq!(foglio.file, "Non al denaro.wav");
        assert_eq!(
            foglio.titolo.as_deref(),
            Some("Non al denaro non all'amore né al cielo")
        );
        assert_eq!(foglio.interprete.as_deref(), Some("Fabrizio De André"));
        assert_eq!(foglio.data.as_deref(), Some("1971"));
        assert_eq!(foglio.genere.as_deref(), Some("Cantautori"));
        assert_eq!(foglio.disco, None);
        let numeri: Vec<u8> = foglio.tracce.iter().map(|t| t.numero).collect();
        assert_eq!(numeri, vec![1, 2, 3]);
        // Il titolo del brano non prende quello del disco, e l'interprete del
        // brano manca dove il foglio non lo scrive.
        assert_eq!(foglio.tracce[0].titolo.as_deref(), Some("La collina"));
        assert_eq!(
            foglio.tracce[0].interprete.as_deref(),
            Some("Fabrizio De André")
        );
        assert_eq!(foglio.tracce[1].interprete, None);
        // Senza virgolette, e con le parole chiave in minuscolo.
        assert_eq!(foglio.tracce[2].titolo.as_deref(), Some("Un giudice"));
    }

    #[test]
    fn l_inizio_e_l_index_01_e_la_pausa_resta_al_brano_prima() {
        let foglio = letto(EAC);
        let inizi: Vec<u32> = foglio.tracce.iter().map(|t| t.inizio_cd).collect();
        // 04:03:00 e non 04:01:20: la pausa dell'`INDEX 00` è coda del primo.
        assert_eq!(inizi, vec![0, (4 * 60 + 3) * 75, (6 * 60 + 40) * 75 + 74]);
        assert_eq!(foglio.fine_cd(0), Some((4 * 60 + 3) * 75));
        assert_eq!(foglio.fine_cd(1), Some((6 * 60 + 40) * 75 + 74));
        assert_eq!(foglio.fine_cd(2), None, "l'ultimo arriva in fondo al file");
        assert_eq!(foglio.fine_cd(99), None);
        assert_eq!(foglio.fine_cd(usize::MAX), None);
    }

    #[test]
    fn utf8_con_il_bom() {
        let mut byte = b"\xEF\xBB\xBF".to_vec();
        byte.extend_from_slice(EAC.as_bytes());
        let foglio = leggi_foglio(&byte).expect("valido");
        assert_eq!(foglio.genere.as_deref(), Some("Cantautori"));
        assert_eq!(foglio, letto(EAC));
    }

    #[test]
    fn cp1252_come_lo_scrive_il_blocco_note() {
        let testo = "PERFORMER \"Lucio Dalla\"\r\nTITLE \"Canterò\"\r\nFILE \"Canterò.wav\" WAVE\r\n\
            TRACK 01 AUDIO\r\nTITLE \"Perché\"\r\nINDEX 01 00:00:00\r\n\
            TRACK 02 AUDIO\r\nTITLE \"Città\"\r\nINDEX 01 03:00:00\r\n";
        let (cp1252, _, guasti) = encoding_rs::WINDOWS_1252.encode(testo);
        assert!(!guasti);
        assert!(
            std::str::from_utf8(&cp1252).is_err(),
            "non deve sembrare UTF-8"
        );
        let foglio = leggi_foglio(&cp1252).expect("valido");
        assert_eq!(foglio.titolo.as_deref(), Some("Canterò"));
        assert_eq!(foglio.file, "Canterò.wav");
        assert_eq!(foglio.tracce[0].titolo.as_deref(), Some("Perché"));
        assert_eq!(foglio.tracce[1].titolo.as_deref(), Some("Città"));
    }

    #[test]
    fn utf16_col_bom_nei_due_versi() {
        // A mano: il codificatore di `encoding_rs` per UTF-16 scrive UTF-8,
        // come vuole lo standard WHATWG, e il test non proverebbe niente.
        let mut le: Vec<u8> = vec![0xFF, 0xFE];
        let mut be: Vec<u8> = vec![0xFE, 0xFF];
        for unita in EAC.encode_utf16() {
            le.extend_from_slice(&unita.to_le_bytes());
            be.extend_from_slice(&unita.to_be_bytes());
        }
        assert_eq!(leggi_foglio(&le), Some(letto(EAC)));
        assert_eq!(leggi_foglio(&be), Some(letto(EAC)));
    }

    #[test]
    fn il_nome_del_file_con_e_senza_virgolette() {
        let con = |riga: &str| {
            leggi_foglio(
                format!(
                    "{riga}\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 01:00:00\n"
                )
                .as_bytes(),
            )
            .map(|foglio| foglio.file)
        };
        assert_eq!(con("FILE \"a b.wav\" WAVE").as_deref(), Some("a b.wav"));
        assert_eq!(con("FILE a.wav WAVE").as_deref(), Some("a.wav"));
        assert_eq!(con("FILE a b.flac FLAC").as_deref(), Some("a b.flac"));
        assert_eq!(con("FILE a.wav").as_deref(), Some("a.wav"));
        assert_eq!(con("file \"x.bin\" BINARY").as_deref(), Some("x.bin"));
        assert_eq!(con("FILE \"x.mp3\" MP3").as_deref(), Some("x.mp3"));
        // Un FILE vuoto non nomina niente.
        assert_eq!(con("FILE \"\" WAVE"), None);
    }

    #[test]
    fn le_rem_del_disco() {
        let foglio = letto(
            "REM DATE \"1999-05-01\"\nREM GENRE Rock\nREM DISCNUMBER 2\nREM TOTALDISCS 3\n\
             FILE x.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\n\
             REM GENRE Jazz\nINDEX 01 00:10:00\n",
        );
        assert_eq!(foglio.data.as_deref(), Some("1999-05-01"));
        // Una `REM` dopo il primo brano non è del disco.
        assert_eq!(foglio.genere.as_deref(), Some("Rock"));
        assert_eq!(foglio.disco, Some(2));
        assert_eq!(
            letto("REM DISCNUMBER 1/2\nFILE x.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 00:10:00\n").disco,
            Some(1)
        );
    }

    #[test]
    fn stringhe_vuote_e_caratteri_di_controllo() {
        let foglio = letto(
            "TITLE \"\"\nPERFORMER \"  \"\nFILE x.wav WAVE\nTRACK 01 AUDIO\nTITLE \"Uno\x07\x1b\"\n\
             INDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 00:10:00\n",
        );
        assert_eq!(foglio.titolo, None);
        assert_eq!(foglio.interprete, None);
        assert_eq!(foglio.tracce[0].titolo.as_deref(), Some("Uno"));
        assert_eq!(foglio.tracce[1].titolo, None);
    }

    #[test]
    fn i_minuti_possono_superare_cento() {
        let foglio = letto(
            "FILE x.flac WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 123:45:67\n",
        );
        assert_eq!(foglio.tracce[1].inizio_cd, (123 * 60 + 45) * 75 + 67);
    }

    #[test]
    fn i_fogli_che_non_descrivono_un_immagine() {
        let due = "TRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 01:00:00\n";
        // Un file per brano: sono brani separati, che la camminata trova già.
        assert_eq!(
            leggi_foglio(
                b"FILE \"01.flac\" WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n\
                  FILE \"02.flac\" WAVE\nTRACK 02 AUDIO\nINDEX 01 00:00:00\n"
            ),
            None
        );
        assert_eq!(leggi_foglio(due.as_bytes()), None, "nessun FILE");
        assert_eq!(
            leggi_foglio(b"FILE x.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n"),
            None,
            "un brano solo è il file stesso"
        );
        let casi = [
            (
                "tempi all'indietro",
                "TRACK 01 AUDIO\nINDEX 01 01:00:00\nTRACK 02 AUDIO\nINDEX 01 00:30:00\n",
            ),
            (
                "tempi uguali",
                "TRACK 01 AUDIO\nINDEX 01 01:00:00\nTRACK 02 AUDIO\nINDEX 01 01:00:00\n",
            ),
            (
                "INDEX 01 mancante",
                "TRACK 01 AUDIO\nINDEX 00 00:00:00\nTRACK 02 AUDIO\nINDEX 01 01:00:00\n",
            ),
            (
                "traccia zero",
                "TRACK 00 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 01:00:00\n",
            ),
            (
                "traccia cento",
                "TRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 100 AUDIO\nINDEX 01 01:00:00\n",
            ),
            (
                "traccia ripetuta",
                "TRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 01 AUDIO\nINDEX 01 01:00:00\n",
            ),
            (
                "fotogramma 75",
                "TRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 01:00:75\n",
            ),
            (
                "secondi 60",
                "TRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 01:60:00\n",
            ),
            (
                "tempo storto",
                "TRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 01:00\n",
            ),
        ];
        for (nota, corpo) in casi {
            assert_eq!(
                leggi_foglio(format!("FILE x.wav WAVE\n{corpo}").as_bytes()),
                None,
                "{nota}"
            );
        }
        assert!(leggi_foglio(format!("FILE x.wav WAVE\n{due}").as_bytes()).is_some());
    }

    fn foglio_di(file: &str) -> Foglio {
        letto(&format!(
            "FILE \"{file}\" WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 01:00:00\n"
        ))
    }

    #[test]
    fn la_cartella_di_brani_separati_non_ha_un_immagine() {
        // Il caso vero: un foglio rimasto da un vecchio rip nomina `Range.wav`,
        // e accanto ci sono i brani già separati. «L'unico file grande» avrebbe
        // preso il primo brano e l'avrebbe tagliato a pezzi.
        let vicini = [
            "01 - Via Della Poverta' (Desolation Row).flac",
            "02 - Le Passanti.flac",
            "Fabrizio De André - Canzoni.cue",
            "folder.jpg",
        ];
        assert_eq!(
            trova_immagine(
                "Fabrizio De André - Canzoni.cue",
                &foglio_di("Range.wav"),
                &vicini
            ),
            None
        );
    }

    #[test]
    fn il_wav_del_rip_compresso_dopo() {
        let foglio = foglio_di("Album.wav");
        assert_eq!(
            trova_immagine(
                "Album.cue",
                &foglio,
                &["Album.cue", "Album.flac", "cover.jpg"]
            ),
            Some("Album.flac")
        );
        assert_eq!(
            trova_immagine("x.cue", &foglio, &["album.APE", "Album.log"]),
            Some("album.APE"),
            "anche in .ape, e senza badare alle maiuscole"
        );
    }

    #[test]
    fn il_nome_esatto_senza_maiuscole_e_senza_percorso() {
        assert_eq!(
            trova_immagine("a.cue", &foglio_di("ALBUM.FLAC"), &["album.flac"]),
            Some("album.flac")
        );
        // Il percorso della macchina del rip non conta, solo l'ultimo segmento.
        assert_eq!(
            trova_immagine("a.cue", &foglio_di(r"D:\Rip\Album.flac"), &["Album.flac"]),
            Some("Album.flac")
        );
        assert_eq!(
            trova_immagine("a.cue", &foglio_di("sotto/Album.wav"), &["Album.ape"]),
            Some("Album.ape")
        );
    }

    #[test]
    fn le_regole_si_provano_in_ordine() {
        // Il nome esatto vince sulla stessa base con un'altra estensione…
        assert_eq!(
            trova_immagine(
                "CD.cue",
                &foglio_di("Album.flac"),
                &["Album.ape", "Album.flac", "CD.flac"]
            ),
            Some("Album.flac")
        );
        // …che vince sul nome del foglio.
        assert_eq!(
            trova_immagine("CD.cue", &foglio_di("Album.wav"), &["CD.flac", "Album.ape"]),
            Some("Album.ape")
        );
        // E il nome del foglio è l'ultima risorsa.
        assert_eq!(
            trova_immagine(
                "CD.cue",
                &foglio_di("CDImage.wav"),
                &["CD.flac", "altro.flac"]
            ),
            Some("CD.flac")
        );
    }

    #[test]
    fn a_pari_regola_l_ordine_della_cartella_non_conta() {
        let foglio = foglio_di("Album.wav");
        let a = trova_immagine("x.cue", &foglio, &["Album.flac", "Album.ape"]);
        let b = trova_immagine("x.cue", &foglio, &["Album.ape", "Album.flac"]);
        assert_eq!(a, b);
        assert!(a.is_some());
    }

    #[test]
    fn un_formato_che_non_leggiamo_non_e_un_immagine() {
        // WavPack: il nome è giusto, ma un brano che non si apre è peggio
        // dell'immagine che non c'è.
        assert_eq!(trova_immagine("x.cue", &foglio_di("x.wv"), &["x.wv"]), None);
        assert_eq!(
            trova_immagine("x.cue", &foglio_di("x.wav"), &["x.wv", "x.cue"]),
            None
        );
        assert_eq!(
            trova_immagine("x.cue", &foglio_di("x.bin"), &["x.bin"]),
            None
        );
    }

    #[test]
    fn il_percorso_virtuale_andata_e_ritorno() {
        let cue = r"\\nas\Cd\Album\Album.cue";
        let brano = percorso_del_brano(cue, 3);
        assert_eq!(brano, r"\\nas\Cd\Album\Album.cue|03");
        assert_eq!(brano_d_immagine(&brano), Some((cue, 3)));
        assert_eq!(
            brano_d_immagine(&percorso_del_brano(cue, 99)),
            Some((cue, 99))
        );
        assert_eq!(
            brano_d_immagine("C:/M/ALBUM.CUE|12"),
            Some(("C:/M/ALBUM.CUE", 12))
        );
    }

    #[test]
    fn quel_che_non_e_un_brano_d_immagine() {
        for percorso in [
            "x.mp3|03",
            "x.cue|3",
            "x.cue|003",
            "x.cue|00",
            "x.cue|ab",
            "x.cue|+3",
            "x.cue",
            "x.cue|",
            r"C:\M\.cue|03",
            r"C:\M\a.flac",
            r"C:\M\x.cue\a.flac",
        ] {
            assert_eq!(brano_d_immagine(percorso), None, "{percorso}");
        }
    }

    fn file(path: &str, size_bytes: u64, modified_ms: i64) -> DiscoveredFile {
        DiscoveredFile {
            path: path.to_owned(),
            size_bytes,
            modified_ms,
        }
    }

    fn trovato(path: &str, modified_ms: i64, foglio: Foglio) -> FoglioTrovato {
        FoglioTrovato {
            cue: file(path, 1_000, modified_ms),
            foglio,
        }
    }

    fn percorsi(espansione: &Espansione) -> Vec<&str> {
        espansione.trovati.iter().map(|f| f.path.as_str()).collect()
    }

    #[test]
    fn l_immagine_diventa_i_suoi_brani() {
        let foglio = letto(EAC);
        let trovati = vec![
            file(r"C:\M\A\cover.jpg", 50_000, 1),
            file(r"C:\M\A\Album.cue", 1_000, 7_000),
            file(r"C:\M\A\Album.flac", 300_000_000, 5_000),
            file(r"C:\M\B\brano.mp3", 5_000_000, 2),
        ];
        let espansione = espandi_immagini(
            trovati,
            vec![trovato(r"C:\M\A\Album.cue", 7_000, foglio.clone())],
            &HashSet::new(),
            WIN,
        );
        assert_eq!(
            percorsi(&espansione),
            vec![
                r"C:\M\A\cover.jpg",
                r"C:\M\A\Album.cue",
                r"C:\M\A\Album.cue|01",
                r"C:\M\A\Album.cue|02",
                r"C:\M\A\Album.cue|03",
                r"C:\M\B\brano.mp3",
            ]
        );
        // La dimensione dell'immagine e la data più recente fra le due: sono
        // i numeri che la riga salva, e che tornano uguali alla passata dopo.
        for voce in espansione.trovati.iter().filter(|f| f.path.contains('|')) {
            assert_eq!(voce.size_bytes, 300_000_000);
            assert_eq!(voce.modified_ms, 7_000);
        }
        assert_eq!(espansione.abbinamenti.len(), 1);
        let abbinamento = &espansione.abbinamenti[0];
        assert_eq!(abbinamento.immagine.path, r"C:\M\A\Album.flac");
        assert_eq!(abbinamento.cue.path, r"C:\M\A\Album.cue");
        assert_eq!(abbinamento.foglio, foglio);
    }

    #[test]
    fn la_data_piu_recente_puo_essere_quella_dell_immagine() {
        let espansione = espandi_immagini(
            vec![file(r"C:\M\A\Album.ape", 200_000_000, 9_000)],
            vec![trovato(r"C:\M\A\Album.cue", 3_000, foglio_di("Album.wav"))],
            &HashSet::new(),
            WIN,
        );
        assert_eq!(espansione.trovati.len(), 2);
        assert!(espansione.trovati.iter().all(|f| f.modified_ms == 9_000));
    }

    #[test]
    fn la_cartella_si_confronta_con_la_chiave() {
        // Stessa cartella scritta in due grafie: su Windows è la stessa.
        let espansione = espandi_immagini(
            vec![file("c:/m/a/album.flac", 200_000_000, 1)],
            vec![trovato(r"C:\M\A\Album.cue", 1, foglio_di("Album.flac"))],
            &HashSet::new(),
            WIN,
        );
        assert_eq!(espansione.abbinamenti.len(), 1);
        // Un'immagine con lo stesso nome in un'altra cartella non è sua.
        let altrove = espandi_immagini(
            vec![file(r"C:\M\B\Album.flac", 200_000_000, 1)],
            vec![trovato(r"C:\M\A\Album.cue", 1, foglio_di("Album.flac"))],
            &HashSet::new(),
            WIN,
        );
        assert!(altrove.abbinamenti.is_empty());
        assert_eq!(percorsi(&altrove), vec![r"C:\M\B\Album.flac"]);
    }

    #[test]
    fn un_foglio_senza_immagine_non_tocca_niente() {
        let trovati = vec![
            file(
                r"C:\M\A\01 - Via Della Poverta' (Desolation Row).flac",
                30_000_000,
                1,
            ),
            file(r"C:\M\A\Range.cue", 1_000, 1),
        ];
        let espansione = espandi_immagini(
            trovati.clone(),
            vec![trovato(r"C:\M\A\Range.cue", 1, foglio_di("Range.wav"))],
            &HashSet::new(),
            WIN,
        );
        assert_eq!(espansione.trovati, trovati);
        assert!(espansione.abbinamenti.is_empty());
    }

    #[test]
    fn un_file_vero_che_sembra_virtuale_si_butta() {
        // Dove `|` è ammesso nei nomi un file così può esistere, e terrebbe
        // lo stesso percorso di un brano d'immagine.
        let espansione = espandi_immagini(
            vec![
                file("/m/a/Album.cue|01", 5_000_000, 1),
                file("/m/a/altro|01", 5_000_000, 1),
            ],
            vec![],
            &HashSet::new(),
            WIN,
        );
        assert_eq!(percorsi(&espansione), vec!["/m/a/altro|01"]);
    }

    #[test]
    fn nessun_brano_due_volte() {
        // Lo stesso foglio arrivato due volte, e un file vero col nome del
        // primo brano: ne resta uno per percorso.
        let foglio = foglio_di("Album.flac");
        let espansione = espandi_immagini(
            vec![
                file(r"C:\M\A\Album.cue|01", 5_000_000, 1),
                file(r"C:\M\A\Album.flac", 200_000_000, 1),
            ],
            vec![
                trovato(r"C:\M\A\Album.cue", 1, foglio.clone()),
                trovato(r"c:\m\a\album.cue", 1, foglio),
            ],
            &HashSet::new(),
            WIN,
        );
        let mut chiavi: Vec<String> = espansione
            .trovati
            .iter()
            .map(|f| path_key(&f.path, WIN))
            .collect();
        let quante = chiavi.len();
        chiavi.sort();
        chiavi.dedup();
        assert_eq!(chiavi.len(), quante);
        assert_eq!(quante, 2);
        assert_eq!(espansione.abbinamenti.len(), 1);
    }

    #[test]
    fn un_immagine_reclamata_due_volte_va_al_primo_foglio() {
        let espansione = espandi_immagini(
            vec![file(r"C:\M\A\Album.flac", 200_000_000, 1)],
            // In ordine inverso apposta: decide il percorso, non l'arrivo.
            vec![
                trovato(r"C:\M\A\Z.cue", 1, foglio_di("Album.wav")),
                trovato(r"C:\M\A\B.cue", 1, foglio_di("Album.wav")),
            ],
            &HashSet::new(),
            WIN,
        );
        assert_eq!(espansione.abbinamenti.len(), 1);
        assert_eq!(espansione.abbinamenti[0].cue.path, r"C:\M\A\B.cue");
        assert_eq!(
            percorsi(&espansione),
            vec![r"C:\M\A\B.cue|01", r"C:\M\A\B.cue|02"]
        );
    }

    #[test]
    fn stessi_ingressi_stessa_uscita() {
        let trovati = vec![
            file(r"C:\M\B\B.flac", 200_000_000, 1),
            file(r"C:\M\A\A.ape", 200_000_000, 1),
            file(r"C:\M\A\x.mp3", 5_000_000, 1),
        ];
        let fogli = vec![
            trovato(r"C:\M\B\B.cue", 1, foglio_di("B.wav")),
            trovato(r"C:\M\A\A.cue", 1, foglio_di("A.wav")),
        ];
        let mut rovesci = fogli.clone();
        rovesci.reverse();
        let a = espandi_immagini(trovati.clone(), fogli, &HashSet::new(), WIN);
        let b = espandi_immagini(trovati, rovesci, &HashSet::new(), WIN);
        assert_eq!(a, b);
        assert_eq!(
            a.abbinamenti
                .iter()
                .map(|x| x.cue.path.as_str())
                .collect::<Vec<_>>(),
            vec![r"C:\M\A\A.cue", r"C:\M\B\B.cue"]
        );
    }

    #[test]
    fn un_foglio_nuovo_non_ruba_l_immagine_a_chi_ha_gia_i_brani() {
        // `Album.cue` ha i suoi brani in libreria; accanto compare
        // `Album (FLAC).cue`, che si ordina prima. Se vincesse lui
        // cambierebbero tutti i percorsi virtuali del disco, e con i titoli
        // diversi gli ascolti non lo seguirebbero.
        let trovati = vec![file(r"C:\M\A\Album.flac", 200_000_000, 1)];
        let fogli = vec![
            trovato(r"C:\M\A\Album.cue", 1, foglio_di("Album.wav")),
            trovato(r"C:\M\A\Album (FLAC).cue", 1, foglio_di("Album.flac")),
        ];
        let con_brani: HashSet<String> = [path_key(r"c:\m\a\album.cue", WIN)].into();
        let espansione = espandi_immagini(trovati.clone(), fogli.clone(), &con_brani, WIN);
        assert_eq!(espansione.abbinamenti.len(), 1);
        assert_eq!(espansione.abbinamenti[0].cue.path, r"C:\M\A\Album.cue");
        assert_eq!(
            percorsi(&espansione),
            vec![r"C:\M\A\Album.cue|01", r"C:\M\A\Album.cue|02"]
        );
        // Senza brani in libreria decide il percorso, come sempre.
        let da_capo = espandi_immagini(trovati, fogli, &HashSet::new(), WIN);
        assert_eq!(da_capo.abbinamenti[0].cue.path, r"C:\M\A\Album (FLAC).cue");
    }

    #[test]
    fn chi_perde_l_immagine_prova_la_regola_dopo() {
        // Il foglio di CD2 copiato da quello di CD1 senza correggere il FILE:
        // la prima regola porta a `CD1.flac`, che è già di CD1; la terza, col
        // nome del foglio, a `CD2.flac`.
        let espansione = espandi_immagini(
            vec![
                file(r"C:\M\A\CD1.flac", 200_000_000, 1),
                file(r"C:\M\A\CD2.flac", 200_000_000, 1),
            ],
            vec![
                trovato(r"C:\M\A\CD1.cue", 1, foglio_di("CD1.flac")),
                trovato(r"C:\M\A\CD2.cue", 1, foglio_di("CD1.flac")),
            ],
            &HashSet::new(),
            WIN,
        );
        let coppie: Vec<(&str, &str)> = espansione
            .abbinamenti
            .iter()
            .map(|a| (a.cue.path.as_str(), a.immagine.path.as_str()))
            .collect();
        assert_eq!(
            coppie,
            vec![
                (r"C:\M\A\CD1.cue", r"C:\M\A\CD1.flac"),
                (r"C:\M\A\CD2.cue", r"C:\M\A\CD2.flac"),
            ]
        );
        assert_eq!(espansione.trovati.len(), 4);

        // Ma solo il nome che la regola sceglie, non gli altri che la passano:
        // lo stesso disco in due formati, e il gemello che perde il primo non
        // si prende il secondo — sarebbe ogni brano due volte.
        let gemelli = espandi_immagini(
            vec![
                file(r"C:\M\B\Album.ape", 200_000_000, 1),
                file(r"C:\M\B\Album.flac", 200_000_000, 1),
            ],
            vec![
                trovato(r"C:\M\B\Album.cue", 1, foglio_di("Album.wav")),
                trovato(r"C:\M\B\Album.wav.cue", 1, foglio_di("Album.wav")),
            ],
            &HashSet::new(),
            WIN,
        );
        assert_eq!(gemelli.abbinamenti.len(), 1);
        assert!(
            percorsi(&gemelli).contains(&r"C:\M\B\Album.flac"),
            "{:?}",
            percorsi(&gemelli)
        );
    }
}
