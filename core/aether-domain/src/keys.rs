//! Le chiavi di identità: quando due righe sono lo stesso brano.
//!
//! Sul disco un brano è un percorso, nel database è un `id`. Nessuno dei due
//! attraversa la sincronizzazione: il percorso è locale, l'id è locale. Serve un
//! terzo nome, derivato dai soli tag, che due dispositivi calcolino uguale senza
//! essersi mai parlati.
//!
//! # Perché la durata non fa parte della chiave
//!
//! La prima versione la includeva, ed era la scelta ovvia: due brani con lo
//! stesso titolo e durata diversa sono due registrazioni diverse. Solo che lo
//! stesso brano ri-codificato da sorgenti diverse — YouTube contro Spotify, mp3
//! contro m4a — differisce di interi secondi. Il risultato pratico è stato che i
//! brani uguali sembravano diversi, e il recupero automatico li ri-scaricava
//! all'infinito. L'album basta a distinguere due titoli omonimi, e non deriva
//! dalla codifica.
//!
//! [`upgrade_legacy_track_key`] esiste per le chiavi della prima versione, che
//! sono ancora nei file di sync già scritti.
//!
//! # Due chiavi e non una
//!
//! [`TrackKey`] è la chiave **mostrata e scambiata**: la calcolano i documenti
//! di sincronizzazione e i backup, e quando l'utente corregge l'artista di un
//! brano si ricalcola dai valori corretti — deve, altrimenti gli altri
//! dispositivi continuerebbero a cercare quel brano sotto il nome sbagliato.
//!
//! [`ContentKey`] è la chiave **del contenuto**: si calcola dai tag grezzi
//! letti dal file — le `Option` come escono dalla lettura, prima di qualunque
//! ripiego — e nessuna correzione la tocca. Serve a una domanda sola: «questo
//! file che è comparso altrove è lo stesso brano di quella riga che è
//! sparita?». A quella domanda una chiave che cambia a ogni correzione
//! risponde male: più si corregge, più il brano diventa irriconoscibile quando
//! il suo file si sposta.
//!
//! I ripieghi restano fuori per la stessa ragione, e non è un dettaglio: il
//! ripiego di un titolo è la radice del nome del file, e un'identità che
//! dipende dal nome si perde appena qualcuno rinomina. Chi rinomina un file lo
//! fa proprio quando i tag non ci sono — il nome è l'unica cosa che lo
//! distingue — quindi il caso sarebbe rotto esattamente dove morde di più. Per
//! quei file la chiave è `durata|dimensione`, che un rinomino non cambia.
//!
//! Sono due tipi distinti e non due costruttori dello stesso tipo perché
//! confonderle è esattamente l'errore da rendere impossibile: mandare una
//! `ContentKey` in un documento di sync rilegherebbe in silenzio gli ascolti di
//! un altro brano.

use crate::text::{collapse_whitespace, fold_text};

/// Il separatore fra i segmenti. Non può comparire dentro un segmento: la
/// rimozione della punteggiatura lo toglie sempre.
const SEPARATOR: char = '|';

/// Apostrofi e primi ricurvi, unificati sull'apostrofo dritto.
const SINGLE_QUOTES: [char; 6] = [
    '\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}', '\u{2032}', '\u{2035}',
];
/// Virgolette e doppi primi ricurvi, unificati sulle virgolette dritte.
const DOUBLE_QUOTES: [char; 6] = [
    '\u{201C}', '\u{201D}', '\u{201E}', '\u{201F}', '\u{2033}', '\u{2036}',
];
/// Ogni variante di trattino, unificata sul meno ASCII.
const DASHES: [char; 7] = [
    '\u{2010}', '\u{2011}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{2015}', '\u{2212}',
];

/// La punteggiatura ASCII che viene rimossa dopo la piegatura.
///
/// # Il backslash non c'è, ed è così anche nell'originale
///
/// La classe di caratteri del JavaScript è
/// `[!"#$%&'()*+,\-./:;<=>?@[\]^_`{|}~]`: dentro, `\-` e `\]` sono un trattino e
/// una quadra chiusa protetti, non un backslash. Il risultato è che `\` è
/// l'unico segno di punteggiatura ASCII che **sopravvive** in una chiave.
///
/// Sembra una svista, e probabilmente lo è. Ma è una svista che sta già dentro
/// le chiavi salvate: «correggerla» qui vorrebbe dire calcolare per quei brani
/// una chiave diversa da quella con cui sono stati scritti su Drive, cioè
/// perdere l'aggancio proprio a quelli con un backslash nel titolo. Si replica.
const fn is_stripped_punctuation(c: char) -> bool {
    matches!(c, '\u{21}'..='\u{2F}' | '\u{3A}'..='\u{40}' | '\u{5B}' | '\u{5D}'..='\u{60}' | '\u{7B}'..='\u{7E}')
}

/// Normalizza un valore di tag per usarlo dentro una chiave.
///
/// Unifica la punteggiatura tipografica, piega (diacritici e maiuscole), toglie
/// la punteggiatura ASCII, collassa gli spazi. Esposta perché è il mattone di
/// tutte le chiavi e va provata da sola.
///
/// ```
/// use aether_domain::keys::normalize_key;
/// assert_eq!(normalize_key(Some("(Don't Fear) The Reaper")), "dont fear the reaper");
/// assert_eq!(normalize_key(None), "");
/// ```
#[must_use]
pub fn normalize_key(input: Option<&str>) -> String {
    let Some(raw) = input else {
        return String::new();
    };

    // L'unificazione precede la piegatura, come nell'originale: le sostituzioni
    // producono ASCII, che la piegatura non tocca, quindi l'ordine fra le due è
    // indifferente — ma restare aderenti costa nulla e toglie una domanda.
    let unified: String = raw
        .chars()
        .map(|c| {
            if SINGLE_QUOTES.contains(&c) {
                '\''
            } else if DOUBLE_QUOTES.contains(&c) {
                '"'
            } else if DASHES.contains(&c) {
                '-'
            } else if c == '\u{2026}' {
                '.'
            } else {
                c
            }
        })
        .collect();

    let folded = fold_text(&unified);
    let without_punctuation: String = folded
        .chars()
        .filter(|c| !is_stripped_punctuation(*c))
        .collect();
    collapse_whitespace(&without_punctuation)
}

/// Gli articoli che non contano nell'ordine alfabetico.
///
/// Quattro lingue e non una: una libreria musicale italiana ha «The Cure»
/// accanto a «I Cani» e a «Los Lobos», e ordinarne una sola sotto la lettera
/// giusta è peggio che non ordinarne nessuna — chi cerca sa dove guardare solo
/// se la regola vale sempre.
///
/// La lista è **chiusa e corta di proposito**. «El» non c'è: è anche un nome
/// («El-P», «El Guincho»), e ordinare El Guincho sotto G è più sbagliato che
/// lasciarlo sotto E. Nel dubbio, non si tocca.
/// L'apostrofo va **prima** dello spazio: si toglie dal nome vero, non da
/// quello normalizzato, perché `normalize_key` butta via la punteggiatura
/// ASCII — apostrofo compreso — e a quel punto «L'Arc» è già diventato «larc»,
/// dove nessun articolo si riconosce più.
const ARTICOLI: &[&str] = &[
    "l'", // italiano e francese, senza spazio
    "the ", "a ", "an ", // inglese
    "il ", "lo ", "la ", "i ", "gli ", "le ",  // italiano
    "les ", // francese
    "los ", "las ", // spagnolo
    "der ", "die ", "das ", // tedesco
];

/// Il nome sotto cui ordinare, senza l'articolo iniziale.
///
/// «The Cure» finisce sotto C, «I Cani» sotto C, «Los Lobos» sotto L. Il
/// confronto avviene sul risultato di [`normalize_key`], quindi maiuscole,
/// diacritici e punteggiatura sono già fuori dai piedi.
///
/// # Perché sta nel dominio
///
/// È una regola su cosa significa «in ordine alfabetico» per una libreria
/// musicale, e vale identica sul telefono. Scritta nella finestra, sarebbe la
/// prima cosa che l'interfaccia Android riscrive in modo leggermente diverso,
/// e due dispositivi ordinerebbero gli stessi artisti in due modi.
///
/// Il nome mostrato resta quello vero: questa funzione produce **solo** la
/// chiave d'ordinamento, e «The Cure» continua a chiamarsi The Cure.
///
/// ```
/// use aether_domain::keys::sort_name;
/// assert_eq!(sort_name("The Cure"), "cure");
/// assert_eq!(sort_name("Los Lobos"), "lobos");
/// assert_eq!(sort_name("L'Arc~en~Ciel"), "arcenciel");
/// assert_eq!(sort_name("The The"), "the");
/// // Un nome che è solo un articolo resta sé stesso, e finisce sotto la sua
/// // lettera invece che in cima a tutto con una chiave vuota.
/// assert_eq!(sort_name("The"), "the");
/// // «El» non è nella lista: El-P resta sotto E.
/// assert_eq!(sort_name("El-P"), "elp");
/// ```
#[must_use]
pub fn sort_name(name: &str) -> String {
    let pulito = name.trim();
    for articolo in ARTICOLI {
        // Gli articoli sono tutti ASCII, quindi tagliare per byte taglia anche
        // per carattere; e `get` restituisce `None` se il taglio cadesse dentro
        // un carattere multibyte, che è la ragione per cui non si usa l'indice
        // diretto.
        let (Some(inizio), Some(resto)) =
            (pulito.get(..articolo.len()), pulito.get(articolo.len()..))
        else {
            continue;
        };
        if !inizio.eq_ignore_ascii_case(articolo) {
            continue;
        }
        // Un nome fatto **solo** di un articolo non si svuota: «The The» ha un
        // resto, «The» da solo no, e una chiave vuota lo manderebbe in cima a
        // tutto invece che sotto T.
        if resto.trim().is_empty() {
            continue;
        }
        return normalize_key(Some(resto));
    }
    normalize_key(Some(pulito))
}

/// I tre tag da cui si deriva l'identità di un brano.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrackKeyInput<'a> {
    /// L'interprete.
    pub artist: Option<&'a str>,
    /// Il titolo.
    pub title: Option<&'a str>,
    /// L'album, che disambigua due titoli omonimi.
    pub album: Option<&'a str>,
}

/// L'identità di un brano fra dispositivi: `artista|titolo|album`, normalizzati.
///
/// È un tipo a sé e non una `String` perché il codice che la usa maneggia anche
/// percorsi, titoli e id, e scambiarli è un errore che il compilatore può
/// evitare invece di lasciarlo scoprire a runtime.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackKey(String);

/// Quel che serve a calcolare l'identità di **contenuto** di un brano.
///
/// I tre tag sono gli stessi di [`TrackKeyInput`]; durata e dimensione sono il
/// ripiego per i file che non ne hanno abbastanza.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContentKeyInput<'a> {
    /// L'interprete, **come sta nei tag del file**.
    pub artist: Option<&'a str>,
    /// Il titolo, come sta nei tag del file.
    pub title: Option<&'a str>,
    /// L'album, che disambigua due titoli omonimi.
    pub album: Option<&'a str>,
    /// La durata in millisecondi, per il ripiego.
    pub duration_ms: Option<i64>,
    /// La dimensione del file in byte, per il ripiego.
    pub file_size: Option<i64>,
}

/// L'identità di **contenuto** di un brano: quel che il file dice di sé, e che
/// nessuna correzione dell'utente cambia.
///
/// # A cosa serve, e perché non basta [`TrackKey`]
///
/// A riconoscere un file che si è spostato. `scan_plan::match_moved_tracks`
/// confronta la chiave delle righe che sparirebbero con quella dei file appena
/// comparsi: se combaciano, la riga si aggiorna invece di essere cancellata e
/// reinserita — e con lei restano ascolti, voto, preferiti e appartenenza alle
/// playlist.
///
/// Con [`TrackKey`] quel confronto si degrada a ogni correzione: la riga porta
/// la chiave calcolata dall'artista **corretto**, il file appena letto quella
/// calcolata dall'artista **scritto nei tag**, e le due non combaciano più. Il
/// brano si duplica, e la sua storia se ne va con la riga cancellata.
///
/// # Le due forme, e perché non possono confondersi
///
/// - `artista|titolo|album`, quando artista e titolo normalizzati non sono
///   vuoti. È **byte per byte** quel che produrrebbe [`TrackKey::compute`] sugli
///   stessi valori, perché è proprio quello che chiama: la normalizzazione è una
///   sola, e riscriverla qui vorrebbe dire lasciarle divergere il giorno che una
///   delle due cambia.
/// - `durata|dimensione`, altrimenti. **Non è un ramo raro**: è il ramo di ogni
///   file senza tag, che una libreria vera ne ha a centinaia. Di un file così
///   le uniche due cose che restano vere quando lo si sposta *o lo si rinomina*
///   sono quanto dura e quanto pesa — il nome no, ed è la ragione per cui la
///   chiave non lo guarda.
///
/// Le due forme non possono collidere: la prima ha sempre tre segmenti, la
/// seconda sempre due, e il separatore non compare mai dentro un segmento —
/// [`normalize_key`] lo toglie, e i numeri non ne contengono.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentKey(String);

impl ContentKey {
    /// Calcola la chiave dai tag grezzi, col ripiego su durata e dimensione.
    ///
    /// «Grezzi» va preso alla lettera: chi chiama passa le `Option` come le ha
    /// lette dal file, **mai** i valori già ripiegati che finiscono nelle
    /// colonne `title`/`artist`/`album`. Un ripiego viene dal nome del file, e
    /// un'identità che dipende dal nome non sopravvive a un rinomino.
    ///
    /// ```
    /// use aether_domain::keys::{ContentKey, ContentKeyInput};
    /// let con_tag = ContentKey::compute(ContentKeyInput {
    ///     artist: Some("Pink Floyd"),
    ///     title: Some("Hey You"),
    ///     album: Some("The Wall"),
    ///     duration_ms: Some(283_000),
    ///     file_size: Some(9_000_000),
    /// });
    /// assert_eq!(con_tag.as_str(), "pink floyd|hey you|the wall");
    ///
    /// let senza_tag = ContentKey::compute(ContentKeyInput {
    ///     duration_ms: Some(283_000),
    ///     file_size: Some(9_000_000),
    ///     ..ContentKeyInput::default()
    /// });
    /// assert_eq!(senza_tag.as_str(), "283000|9000000");
    /// ```
    #[must_use]
    pub fn compute(input: ContentKeyInput<'_>) -> Self {
        // La condizione si valuta sui valori **normalizzati**: un titolo di soli
        // spazi o di sola punteggiatura è vuoto quanto un titolo assente, e
        // trattarlo come identità darebbe a tutti quei file la stessa chiave.
        //
        // Li normalizza due volte — qui per decidere, e di nuovo dentro
        // [`TrackKey::compute`] per costruire — ed è deliberato: due stringhe
        // corte allocate una volta per file letto non si misurano, mentre una
        // condizione decisa su una normalizzazione e una chiave costruita su
        // un'altra è il genere di divergenza che non si vede finché non
        // duplica una libreria.
        let artist = normalize_key(input.artist);
        let title = normalize_key(input.title);
        if !artist.is_empty() && !title.is_empty() {
            return Self(
                TrackKey::compute(TrackKeyInput {
                    artist: input.artist,
                    title: input.title,
                    album: input.album,
                })
                .into_string(),
            );
        }

        // Il ripiego. Un valore che manca lascia il suo segmento vuoto invece di
        // far collassare la chiave: `|` da solo è «di questo file non si sa
        // niente», ed è giusto che nessuno lo riconosca come identità — se ne
        // occupa chi confronta, non chi calcola.
        let durata = input.duration_ms.map(|ms| ms.to_string());
        let dimensione = input.file_size.map(|byte| byte.to_string());
        let durata = durata.as_deref().unwrap_or_default();
        let dimensione = dimensione.as_deref().unwrap_or_default();
        let mut key = String::with_capacity(durata.len() + dimensione.len() + 1);
        key.push_str(durata);
        key.push(SEPARATOR);
        key.push_str(dimensione);
        Self(key)
    }

    /// Adotta una chiave **già calcolata**, letta dal database.
    ///
    /// Vedi [`TrackKey::from_stored`]: non normalizza, e la separazione da
    /// [`Self::compute`] serve a rendere visibile nel punto di chiamata quale
    /// delle due cose sta succedendo.
    #[must_use]
    pub fn from_stored(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// La chiave come stringa.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consuma la chiave restituendo la stringa.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for ContentKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// L'identità di una playlist: il suo nome normalizzato.
///
/// Rinominare una playlist ne fa quindi un'entità nuova. È deliberato: senza un
/// identificativo stabile trasmesso fra i dispositivi, un rinomino e una
/// creazione sono indistinguibili, e trattarli diversamente richiederebbe di
/// indovinare.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaylistKey(String);

impl TrackKey {
    /// Calcola la chiave dai tag.
    #[must_use]
    pub fn compute(input: TrackKeyInput<'_>) -> Self {
        let artist = normalize_key(input.artist);
        let title = normalize_key(input.title);
        let album = normalize_key(input.album);
        let mut key = String::with_capacity(artist.len() + title.len() + album.len() + 2);
        key.push_str(&artist);
        key.push(SEPARATOR);
        key.push_str(&title);
        key.push(SEPARATOR);
        key.push_str(&album);
        Self(key)
    }

    /// Adotta una chiave **già calcolata**, letta dal database o da un file di
    /// sync.
    ///
    /// Non normalizza, e non deve: una chiave è già il risultato della
    /// normalizzazione, e ripassarla non è idempotente in generale. Il costruttore
    /// è separato da [`Self::compute`] proprio perché la differenza sia visibile
    /// nel punto di chiamata.
    #[must_use]
    pub fn from_stored(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// La chiave come stringa.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consuma la chiave restituendo la stringa.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl PlaylistKey {
    /// Calcola la chiave dal nome.
    #[must_use]
    pub fn compute(name: Option<&str>) -> Self {
        Self(normalize_key(name))
    }

    /// Adotta una chiave già calcolata. Vedi [`TrackKey::from_stored`].
    #[must_use]
    pub fn from_stored(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// La chiave come stringa.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consuma la chiave restituendo la stringa.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for TrackKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for PlaylistKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Porta una chiave della prima versione (`artista|titolo|album|durata`) alla
/// forma attuale, togliendo la coda numerica.
///
/// Riconoscerla è possibile senza ambiguità: i segmenti non possono contenere
/// `|`, quindi una chiave con esattamente quattro segmenti di cui l'ultimo è
/// tutto cifre è per forza della prima versione. Le chiavi attuali e quelle di
/// playlist passano invariate, e la funzione è idempotente.
///
/// ```
/// use aether_domain::keys::upgrade_legacy_track_key;
/// assert_eq!(upgrade_legacy_track_key("a|b|c|240"), "a|b|c");
/// assert_eq!(upgrade_legacy_track_key("a|b|c"), "a|b|c");
/// // Quattro segmenti ma l'ultimo non è un numero: non è una chiave v1.
/// assert_eq!(upgrade_legacy_track_key("a|b|c|d"), "a|b|c|d");
/// ```
#[must_use]
pub fn upgrade_legacy_track_key(key: &str) -> &str {
    let mut parts = key.split(SEPARATOR);
    let (Some(artist), Some(title), Some(album), Some(tail), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return key;
    };
    // `\d` di JavaScript senza flag unicode è ASCII: le cifre arabo-indiane non
    // contano, e `is_ascii_digit` è la traduzione esatta.
    if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
        return key;
    }
    // I tre segmenti sono contigui nella stringa originale: se ne restituisce
    // una fetta invece di ricomporli, così la funzione non alloca.
    let kept = artist.len() + title.len() + album.len() + 2;
    key.get(..kept).unwrap_or(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(artist: &str, title: &str, album: &str) -> String {
        TrackKey::compute(TrackKeyInput {
            artist: Some(artist),
            title: Some(title),
            album: Some(album),
        })
        .into_string()
    }

    #[test]
    fn due_scritture_dello_stesso_brano_danno_la_stessa_chiave() {
        assert_eq!(
            key(
                "Blue Öyster Cult",
                "(Don't Fear) The Reaper",
                "Agents of Fortune"
            ),
            key(
                "Blue Oyster Cult",
                "Dont Fear The Reaper",
                "Agents of Fortune"
            )
        );
    }

    #[test]
    fn il_backslash_sopravvive_come_nell_originale() {
        // Non è una scelta estetica: è ciò che sta nelle chiavi già scritte.
        assert_eq!(normalize_key(Some("a\\b")), "a\\b");
        assert_eq!(
            normalize_key(Some("!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~")),
            "\\"
        );
    }

    #[test]
    fn i_valori_assenti_danno_segmenti_vuoti_non_una_chiave_vuota() {
        // La chiave resta a tre segmenti: `||` è un'identità legittima (un file
        // senza tag), e collassarla a "" la confonderebbe con l'assenza di chiave.
        assert_eq!(key("", "", ""), "||");
        assert_eq!(
            TrackKey::compute(TrackKeyInput::default()).into_string(),
            "||"
        );
    }

    #[test]
    fn il_separatore_non_puo_finire_dentro_un_segmento() {
        // Se ci finisse, una chiave con quattro segmenti diventerebbe ambigua e
        // l'aggiornamento dalla v1 taglierebbe il pezzo sbagliato.
        assert_eq!(key("A|B", "C|D", "E|F"), "ab|cd|ef");
    }

    #[test]
    fn aggiornamento_dalla_prima_versione() {
        assert_eq!(
            upgrade_legacy_track_key("artist|title|album|240"),
            "artist|title|album"
        );
        assert_eq!(
            upgrade_legacy_track_key("artist|title|album|00240"),
            "artist|title|album"
        );
        assert_eq!(
            upgrade_legacy_track_key("artist|title|album|0"),
            "artist|title|album"
        );
        // Non numerico → non è una coda di durata.
        assert_eq!(
            upgrade_legacy_track_key("artist|title|album|24a"),
            "artist|title|album|24a"
        );
        // Cinque segmenti → non è una chiave, si lascia stare.
        assert_eq!(
            upgrade_legacy_track_key("a|b|c|240|extra"),
            "a|b|c|240|extra"
        );
        // Idempotente.
        assert_eq!(
            upgrade_legacy_track_key(upgrade_legacy_track_key("a|b|c|240")),
            "a|b|c"
        );
    }

    #[test]
    fn aggiornamento_su_chiavi_che_non_lo_sono() {
        assert_eq!(upgrade_legacy_track_key(""), "");
        assert_eq!(upgrade_legacy_track_key("solo"), "solo");
        assert_eq!(upgrade_legacy_track_key("a|b"), "a|b");
    }

    #[test]
    fn il_ripiego_scatta_quando_un_tag_manca_davvero() {
        // La forma pura del ripiego. Che poi arrivi fin qui **dalla scansione
        // vera** lo prova `aether_app::library::tests::
        // senza_tag_la_chiave_e_durata_e_dimensione`, perché è là che si
        // decide se a `compute` arrivano i tag grezzi o i ripieghi.
        //
        // Il ripiego scatta quando artista **o** titolo normalizzato è vuoto:
        // metà identità non è identità, e appaiare su di essa dichiarerebbe
        // «stesso brano» due file che hanno in comune solo un nome.
        let solo_titolo = ContentKey::compute(ContentKeyInput {
            title: Some("Hey You"),
            duration_ms: Some(283_000),
            file_size: Some(9_000_000),
            ..ContentKeyInput::default()
        });
        assert_eq!(solo_titolo.as_str(), "283000|9000000");

        let solo_artista = ContentKey::compute(ContentKeyInput {
            artist: Some("Pink Floyd"),
            duration_ms: Some(283_000),
            file_size: Some(9_000_000),
            ..ContentKeyInput::default()
        });
        assert_eq!(solo_artista.as_str(), "283000|9000000");

        // Un titolo di sola punteggiatura è vuoto quanto un titolo assente:
        // dopo `normalize_key` non resta niente.
        let punteggiatura = ContentKey::compute(ContentKeyInput {
            artist: Some("Pink Floyd"),
            title: Some("!!!"),
            duration_ms: Some(1),
            file_size: Some(2),
            ..ContentKeyInput::default()
        });
        assert_eq!(punteggiatura.as_str(), "1|2");

        // E quando non si sa nemmeno quello, la chiave resta a due segmenti
        // vuoti invece di collassare: `|` è «di questo file non si sa niente».
        assert_eq!(
            ContentKey::compute(ContentKeyInput::default()).as_str(),
            "|"
        );
    }

    #[test]
    fn la_chiave_di_contenuto_normalizza_come_quella_di_brano() {
        // Se le due normalizzazioni divergessero, il ripiego scatterebbe
        // quando non deve — o peggio non scatterebbe quando deve — e la cosa
        // non si vedrebbe da nessuna parte finché un file spostato non si
        // duplica. Qui si prova che sono la stessa, non che si somigliano.
        for (artista, titolo, album) in [
            ("Blue Öyster Cult", "(Don't Fear) The Reaper", "Agents"),
            ("A|B", "C|D", "E|F"),
            ("Björk", "Jóga", ""),
            ("a\\b", "c\\d", "e\\f"),
        ] {
            let contenuto = ContentKey::compute(ContentKeyInput {
                artist: Some(artista),
                title: Some(titolo),
                album: Some(album),
                duration_ms: Some(1),
                file_size: Some(2),
            });
            assert_eq!(contenuto.as_str(), key(artista, titolo, album));
        }
    }

    #[test]
    fn le_due_forme_non_si_possono_confondere() {
        // Tre segmenti contro due: una chiave di ripiego non può essere letta
        // come una chiave da tag, per nessun valore di durata e dimensione.
        let ripiego = ContentKey::compute(ContentKeyInput {
            duration_ms: Some(283_000),
            file_size: Some(9_000_000),
            ..ContentKeyInput::default()
        });
        assert_eq!(ripiego.as_str().split(SEPARATOR).count(), 2);

        let da_tag = ContentKey::compute(ContentKeyInput {
            artist: Some("Pink Floyd"),
            title: Some("Hey You"),
            album: Some("The Wall"),
            duration_ms: Some(283_000),
            file_size: Some(9_000_000),
        });
        assert_eq!(da_tag.as_str().split(SEPARATOR).count(), 3);
    }

    #[test]
    fn una_chiave_di_contenuto_letta_dal_database_non_si_rinormalizza() {
        assert_eq!(
            ContentKey::from_stored("283000|9000000").as_str(),
            "283000|9000000"
        );
        assert_eq!(
            ContentKey::from_stored("pink floyd|hey you|the wall").into_string(),
            "pink floyd|hey you|the wall"
        );
    }

    #[test]
    fn le_playlist_si_identificano_dal_nome_piegato() {
        assert_eq!(
            PlaylistKey::compute(Some("Rock ’n’ Roll")),
            PlaylistKey::compute(Some("rock 'n' roll"))
        );
        assert_eq!(PlaylistKey::compute(None).as_str(), "");
    }
}
