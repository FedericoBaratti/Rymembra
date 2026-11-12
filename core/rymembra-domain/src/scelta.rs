//! Quale risultato di un catalogo è il brano che si sta cercando.
//!
//! Un catalogo risponde a una stringa di ricerca con una manciata di risultati,
//! e fra quelli ci sono la registrazione giusta, un concerto del '77, una cover,
//! un remix e un file che si chiama come la canzone ma dura quaranta minuti.
//!
//! Questo modulo è la decisione in mezzo, e sta qui — nel dominio — perché è
//! precisamente il genere di decisione che [`crate`] descrive: si può sbagliare
//! in silenzio, e sbagliando mette in libreria la registrazione di qualcun altro
//! col nome giusto nei tag. Provarla deve costare una chiamata di funzione, non
//! una rete.
//!
//! # La durata è un cancello, la natura e l'autore sono la scelta
//!
//! L'ordine fra i criteri è **la** decisione di progetto di questo file, e va
//! nell'una direzione e non nell'altra:
//!
//! - un autore ufficiale con la durata sbagliata è la **registrazione**
//!   sbagliata — il live, la versione allungata, il disco intero in un file solo;
//! - la durata giusta su un autore qualunque è almeno la canzone giusta.
//!
//! Quindi la durata elimina, e solo fra i sopravvissuti natura e autore
//! scelgono.
//!
//! # Perché la natura esiste, e perché non è più un filtro secco
//!
//! Nell'albero di prima le parole come `live` e `remix` **scartavano** il
//! candidato, e con una fonte in cui tutto è in studio quella era la scelta
//! giusta. I cataloghi liberi non sono così: il Live Music Archive è fatto
//! *soltanto* di registrazioni dal vivo, e scartarle vorrebbe dire dichiarare
//! introvabile ogni brano di ogni artista che ci sta dentro.
//!
//! Quindi la natura non scarta: **retrocede**. Un candidato in studio batte
//! sempre un'alternativa, ma se l'alternativa è tutto quel che c'è, chi chiama
//! decide — con `alternative_ammesse` — se prenderla o dire «non c'è». E in
//! tutti e due i casi la natura viaggia fino all'interfaccia, perché «ho preso
//! un live» è un'informazione e il silenzio no.
//!
//! # Prima di tutto, titolo e artista
//!
//! Sopra la durata c'è un cancello che non si apre con niente: il titolo **e**
//! l'interprete del candidato devono essere quelli chiesti, a meno di come li
//! si scrive (vedi [`combacia`]). Senza, l'archivio di Spotify — che non porta
//! durate — lasciava vincere il solo scaricabile fra otto risultati, e un
//! «Paranoid Android» caricato da un fan finiva sul disco col nome di «Karma
//! Police». Le registrazioni diverse ammesse dall'interruttore sono quindi un
//! live o una ristampa **dello stesso artista**; la cover di un altro non è il
//! brano chiesto, qualunque cosa dica l'interruttore. Quel che non passa resta
//! da comprare.

use crate::abbinamento::{primo_artista, senza_decorazioni};
use crate::esterno::{BranoEsterno, Disponibilita, Fonte, Licenza};
use crate::keys::normalize_key;
use crate::text::{collapse_whitespace, fold_text};

/// Lo scarto di durata entro cui due registrazioni sono la stessa.
///
/// Trenta secondi, per la ragione già scritta in
/// [`crate::abbinamento::TOLLERANZA_MS`]: sono i silenzi di coda e gli stacchi
/// che cambiano fra un rip e l'altro. Qui è più larga che nell'abbinamento in
/// libreria perché un catalogo ci mette del suo — un secondo di silenzio in
/// testa, una coda che sfuma, l'applauso tagliato dove capita.
pub const BANDA_STRETTA_MS: u64 = 30_000;

/// Oltre questo scarto non è la stessa registrazione, e nessun autore lo salva.
pub const BANDA_LARGA_MS: u64 = 60_000;

/// Che registrazione promette di essere un candidato.
///
/// Non è un giudizio di qualità: è la risposta alla domanda «è *questa* la
/// registrazione chiesta, o un'altra dello stesso brano?».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Natura {
    /// Niente nel titolo dice che sia una registrazione diversa da quella chiesta.
    #[default]
    Studio,
    /// Il titolo o la collezione dicono che è dal vivo.
    DalVivo,
    /// Remix, cover, versione acustica, strumentale: un'altra registrazione.
    Alternativa,
}

impl Natura {
    /// Quanto è vicina a ciò che si è chiesto, dal più al meno.
    #[must_use]
    pub const fn peso(self) -> u8 {
        match self {
            Self::Studio => 2,
            Self::DalVivo => 1,
            Self::Alternativa => 0,
        }
    }

    /// Il nome della variante, per chi la deve mostrare.
    ///
    /// Scritto a mano e non derivato: un giorno l'enum si riordina per
    /// leggibilità, e serializzare la forma del tipo vorrebbe dire che il nome
    /// che la finestra riceve dipende da come è scritto il codice.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Studio => "studio",
            Self::DalVivo => "dalVivo",
            Self::Alternativa => "alternativa",
        }
    }
}

/// Quanto chi pubblica somiglia alla fonte del brano.
///
/// L'ordine delle varianti **è** l'ordine di preferenza, e [`Affidabilita::peso`]
/// lo rende esplicito invece di lasciarlo dedurre da un `derive(Ord)`: un giorno
/// qualcuno riordinerà l'enum per leggibilità, e senza il peso scritto a mano
/// cambierebbe il comportamento senza cambiare nessuna riga che sembri decidere
/// qualcosa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Affidabilita {
    /// Chi pubblica si chiama come l'interprete.
    NomeAutore,
    /// Il catalogo lo segna come caricamento verificato o curato.
    Verificata,
    /// Tutto il resto: potrebbe essere chiunque.
    #[default]
    Ignota,
}

impl Affidabilita {
    /// Quanto vale, dal più al meno affidabile.
    #[must_use]
    pub const fn peso(self) -> u8 {
        match self {
            Self::NomeAutore => 2,
            Self::Verificata => 1,
            Self::Ignota => 0,
        }
    }

    /// Il nome della variante, per chi la deve mostrare.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::NomeAutore => "nomeAutore",
            Self::Verificata => "verificata",
            Self::Ignota => "ignota",
        }
    }
}

/// Un risultato di un catalogo, ridotto a quel che serve per sceglierlo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidato {
    /// Come si torna a prenderlo. Per l'Internet Archive è l'indirizzo del file.
    pub url: String,
    /// Il titolo, come lo scrive chi lo ha caricato.
    pub titolo: String,
    /// Chi lo pubblica: l'autore, la band, la collezione.
    pub autore: Option<String>,
    /// Il catalogo dichiara il caricamento verificato o curato.
    pub autore_verificato: bool,
    /// La durata dichiarata, in secondi. `None` quando il catalogo non la dà.
    pub durata_sec: Option<u32>,
    /// Che registrazione promette di essere.
    pub natura: Natura,
    /// Da quale catalogo arriva.
    pub fonte: Fonte,
    /// Sotto che licenza sta, secondo quel catalogo.
    pub licenza: Licenza,
    /// Che cosa se ne può fare.
    ///
    /// Sta qui e non solo su [`BranoEsterno`] perché è un criterio di scelta
    /// come gli altri: chi cerca un file da tenere non ha nessun motivo di
    /// preferire quello che non può tenere, e senza il campo dovrebbe andare a
    /// richiederlo alla fonte una seconda volta per scoprirlo.
    pub disponibilita: Disponibilita,
    /// L'estensione del file, quando il catalogo la dichiara.
    ///
    /// Serve al momento di scrivere su disco, e serve **prima**: un `.ogg` su
    /// una libreria che non sa aprirlo è un file inutile scaricato per intero.
    pub estensione: Option<String>,
    /// La pagina pubblica del brano.
    ///
    /// Da mostrare, non da leggere: i termini di certi cataloghi obbligano a un
    /// rimando visibile accanto al brano.
    pub pagina: Option<String>,
    /// L'album, il concerto o la raccolta da cui viene, quando il catalogo lo
    /// dice.
    ///
    /// Non serve all'abbinamento — [`scegli`] non lo guarda, perché il brano
    /// cercato porta già il proprio album e confrontarli darebbe un criterio in
    /// più che sbaglia sulle compilation. Serve a **mostrarlo**: su un item
    /// dell'Internet Archive dieci risultati sono dieci pezzi dello stesso
    /// concerto, e senza il nome di quel concerto accanto sono dieci righe che
    /// si somigliano e basta. Ed è quel che finisce in `tracks.album` quando un
    /// brano di catalogo entra in libreria.
    pub album: Option<String>,
}

impl Candidato {
    /// Se ne può tenere una copia sul disco.
    ///
    /// Servono **tutte e due** le cose: la fonte che consegna
    /// ([`Disponibilita::Scaricabile`]) e una licenza che permette la copia.
    /// Un candidato uscito da un catalogo le ha già insieme — è quel che
    /// [`Disponibilita::decidi`] calcola — ma uno ricostruito da una riga di
    /// `desiderati` no: l'importazione da un link non ne salvava la licenza, e
    /// un profilo importato le scrive tutte e due come vuole. Una disponibilità
    /// senza licenza non è un permesso.
    #[must_use]
    pub const fn si_puo_tenere(&self) -> bool {
        matches!(self.disponibilita, Disponibilita::Scaricabile) && self.licenza.permette_copia()
    }
}

/// Le parole che rivelano un'altra registrazione dal vivo.
const PAROLE_DAL_VIVO: &[&str] = &["live", "dal vivo", "concert", "concerto", "unplugged"];

/// Le parole che rivelano un'altra registrazione, punto.
///
/// Non sono «parole brutte»: sono parole che, se stanno nel titolo del risultato
/// e **non** in quello chiesto, dicono che quel file non è il brano chiesto. La
/// condizione doppia è essenziale — chi cerca un remix deve poterlo trovare, e
/// un brano che si chiama davvero «Karaoke» non deve diventare introvabile.
const PAROLE_ALTERNATIVA: &[&str] = &[
    "cover",
    "remix",
    "mashup",
    "karaoke",
    "reaction",
    "reazione",
    "nightcore",
    "sped up",
    "slowed",
    "8d",
    "instrumental",
    "strumentale",
    "acoustic",
    "acustica",
    "tutorial",
    "demo",
    "rehearsal",
];

/// La stringa di ricerca stretta: interprete e titolo come la fonte li scrive.
#[must_use]
pub fn query_stretta(brano: &BranoEsterno) -> String {
    let artista = brano.artist.as_deref().unwrap_or("");
    collapse_whitespace(&format!("{artista} {}", brano.title))
}

/// La stringa di ricerca larga, per quando la stretta non trova niente.
///
/// Solo il primo interprete e il titolo senza decorazioni, riusando le due
/// funzioni con cui [`crate::abbinamento`] ripulisce le stesse stringhe per
/// l'abbinamento in libreria. Riusarle e non riscriverle non è economia di
/// righe: è ciò che impedisce che «cercato nel catalogo» e «cercato in libreria»
/// vogliano dire due cose diverse.
///
/// `None` quando non aggiunge niente — coincide con la stretta o è vuota — così
/// chi chiama non paga una seconda richiesta identica alla prima.
#[must_use]
pub fn query_larga(brano: &BranoEsterno) -> Option<String> {
    let artista = brano.artist.as_deref().map(primo_artista).unwrap_or("");
    let titolo = senza_decorazioni(&brano.title);
    let larga = collapse_whitespace(&format!("{artista} {titolo}"));
    if larga.is_empty() || larga == query_stretta(brano) {
        return None;
    }
    Some(larga)
}

/// La parola compare nel testo come parola intera.
///
/// Il confronto per sottostringa non basta e il caso che lo dimostra è in
/// classifica da cinquant'anni: «Stayin' Alive» contiene `live`, e con un
/// `contains` diventerebbe un concerto.
fn contiene_parola(piegato: &str, parola: &str) -> bool {
    piegato.match_indices(parola).any(|(inizio, _)| {
        let prima = piegato
            .get(..inizio)
            .and_then(|p| p.chars().next_back())
            .is_none_or(|c| !c.is_alphanumeric());
        let dopo = piegato
            .get(inizio.saturating_add(parola.len())..)
            .and_then(|p| p.chars().next())
            .is_none_or(|c| !c.is_alphanumeric());
        prima && dopo
    })
}

/// Che registrazione promette un titolo, dato quello che si è chiesto.
///
/// Le fonti che sanno già la risposta — una collezione di soli concerti — non
/// devono chiamarla: mettono [`Natura::DalVivo`] e basta. Serve a chi ha in mano
/// soltanto una stringa.
#[must_use]
pub fn natura_dal_titolo(titolo: &str, titolo_atteso: &str) -> Natura {
    let piegato = fold_text(titolo);
    let atteso = fold_text(titolo_atteso);
    let nuova = |parole: &[&str]| {
        parole
            .iter()
            .any(|p| contiene_parola(&piegato, p) && !contiene_parola(&atteso, p))
    };
    if nuova(PAROLE_ALTERNATIVA) {
        Natura::Alternativa
    } else if nuova(PAROLE_DAL_VIVO) {
        Natura::DalVivo
    } else {
        Natura::Studio
    }
}

/// Quanto chi pubblica somiglia alla fonte del brano.
#[must_use]
pub fn affidabilita(autore: Option<&str>, verificato: bool, artista: Option<&str>) -> Affidabilita {
    let Some(autore) = autore.map(str::trim).filter(|c| !c.is_empty()) else {
        return if verificato {
            Affidabilita::Verificata
        } else {
            Affidabilita::Ignota
        };
    };

    // Il confronto col nome dell'interprete passa dalla piegatura di
    // `rymembra_domain::text`, la stessa che usa l'abbinamento in libreria: senza,
    // «Bjork» e «Björk» sarebbero due artisti diversi e l'autore ufficiale
    // varrebbe come uno qualunque.
    if let Some(artista) = artista {
        let atteso = fold_text(primo_artista(artista));
        if !atteso.is_empty() && fold_text(autore) == atteso {
            return Affidabilita::NomeAutore;
        }
    }

    if verificato {
        Affidabilita::Verificata
    } else {
        Affidabilita::Ignota
    }
}

/// Di quanto il candidato è più lungo di quel che la fonte dichiara, in ms.
///
/// `None` quando una delle due durate non si sa: è il caso che [`banda`] tiene
/// in gara nella banda larga, e dirlo «0» qui vorrebbe dire disegnare un
/// combaciare che nessuno ha verificato.
///
/// # Perché firmato, e perché non lo usa [`banda`]
///
/// Perché al momento di **scegliere** il verso non conta — trenta secondi in
/// più e trenta in meno sono ugualmente lontani — mentre al momento di
/// **mostrare** conta tutto: `+8 s` su un autore ufficiale è un'introduzione
/// parlata o una coda che sfuma, `−8 s` è una versione tagliata. Sono la stessa
/// sottrazione per due domande diverse, e tenerle separate costa tre righe
/// mentre unirle costerebbe un `unsigned_abs` dentro il criterio di scelta.
#[must_use]
pub fn scarto_durata(attesa_ms: Option<u64>, durata_sec: Option<u32>) -> Option<i64> {
    let (attesa, sua_sec) = (attesa_ms?, durata_sec?);
    let sua_ms = i64::from(sua_sec).saturating_mul(1000);
    Some(sua_ms.saturating_sub(i64::try_from(attesa).ok()?))
}

/// Le parole che in coda a un titolo, senza parentesi né trattino, dicono
/// soltanto l'edizione: «Karma Police Remastered 2009».
///
/// Più corta di quella dell'abbinamento in libreria, e di proposito: qui una
/// parola tolta per sbaglio fa passare una registrazione al posto di un'altra.
/// `mix`, `demo`, `acoustic` non ci sono — dicono un'altra registrazione, ed è
/// [`natura_dal_titolo`] a dirlo, non questa ripulitura a nasconderlo.
const CODE_EDIZIONE: &[&str] = &[
    "live",
    "remaster",
    "remastered",
    "rimasterizzato",
    "version",
    "versione",
    "edit",
    "mono",
    "stereo",
];

/// Dove comincia l'elenco degli ospiti in un titolo, quando non è fra
/// parentesi: «Feel Good Inc. feat. De La Soul».
const OSPITI_NEL_TITOLO: &[&str] = &[" feat.", " feat ", " featuring ", " ft. ", " ft "];

/// Una parola di quattro cifre: un anno, se sta accanto a un'edizione.
fn e_un_anno(parola: &str) -> bool {
    parola.len() == 4 && parola.chars().all(|c| c.is_ascii_digit())
}

/// Il titolo nella forma in cui due titoli si confrontano.
///
/// Riusa le ripuliture che l'abbinamento in libreria fa già —
/// [`senza_decorazioni`] per le parentesi e le code d'edizione,
/// [`normalize_key`] per maiuscole, accenti, apostrofi e punteggiatura — più
/// tre cose che lì non servono: il taglio degli ospiti scritti senza parentesi,
/// la `&` letta come «and» (come per gli interpreti: «Rock & Roll» è «Rock and
/// Roll»), e le parole di [`CODE_EDIZIONE`] rimaste in fondo, con l'anno che le
/// accompagna. Se la ripulitura non lascia niente — una canzone che si chiama
/// davvero «Live» — si torna al titolo intero.
///
/// # L'anno, solo accanto a un'edizione
///
/// «Karma Police Remastered 2009» e «Karma Police 2017 Remaster» sono «Karma
/// Police»; «Class of 1984» no, e non è «Class of 1999». Fino alla 3.2.0 ogni
/// anno in fondo si toglieva, e i due ultimi diventavano lo stesso titolo.
fn titolo_da_confrontare(titolo: &str) -> String {
    let senza = senza_decorazioni(titolo);
    // `to_ascii_lowercase` conserva la lunghezza in byte: la posizione trovata
    // nella copia vale anche nell'originale, come in `primo_artista`.
    let minuscolo = senza.to_ascii_lowercase();
    let taglio = OSPITI_NEL_TITOLO
        .iter()
        .filter_map(|ospite| minuscolo.find(ospite))
        .min();
    let senza_ospiti = taglio.map_or(senza.as_str(), |dove| senza.get(..dove).unwrap_or(&senza));
    let piegato = normalize_key(Some(&senza_ospiti.replace('&', " and ")));
    let mut parole: Vec<&str> = piegato.split(' ').filter(|p| !p.is_empty()).collect();
    // Vero appena si è tolta una parola d'edizione: l'anno che la precedeva
    // («2017 Remaster») è suo.
    let mut dopo_edizione = false;
    while parole.len() > 1 {
        let Some(ultima) = parole.last().copied() else {
            break;
        };
        let prima = parole.len().checked_sub(2).and_then(|n| parole.get(n));
        let edizione = CODE_EDIZIONE.contains(&ultima);
        let anno_suo = e_un_anno(ultima)
            && (dopo_edizione || prima.is_some_and(|p| CODE_EDIZIONE.contains(p)));
        if !edizione && !anno_suo {
            break;
        }
        dopo_edizione = edizione;
        parole.pop();
    }
    let pulito = parole.join(" ");
    if pulito.is_empty() {
        normalize_key(Some(titolo))
    } else {
        pulito
    }
}

/// Un nome d'interprete nella forma in cui si confronta: piegato come una
/// chiave, con `&` letto come «and», e senza l'articolo in testa («The
/// Beatles» e «Beatles»).
fn nome_da_confrontare(nome: &str) -> String {
    let piegato = normalize_key(Some(&nome.replace('&', " and ")));
    piegato
        .strip_prefix("the ")
        .map_or_else(|| piegato.clone(), ToOwned::to_owned)
}

/// Dove finisce l'interprete principale e comincia un ospite.
///
/// Non sono i [`crate::abbinamento::SEPARATORI_ARTISTA`] dell'abbinamento in
/// libreria, e per due ragioni che lì costano poco e qui mettono sul disco la
/// cover di un altro col nome del brano chiesto:
///
/// - la `&` non c'è: «Simon & Garfunkel» è un nome solo, e tagliarlo faceva
///   valere un caricamento di «Simon» come il duo;
/// - `feat` vuole un confine **dopo** (`feat.`, `feat `, `featuring `): senza,
///   «Little Feat» diventava «Little» e «Sam Feather» «Sam».
///
/// La virgola resta, perché Spotify scrive così gli interpreti di un brano
/// («Gorillaz, De La Soul»), e con lei resta un limite che dal solo testo non
/// si toglie: «Tyler, The Creator» ha la stessa forma, e un caricamento di un
/// «Tyler» qualunque passa.
const OSPITI_DELL_INTERPRETE: &[&str] = &[
    ",",
    " feat.",
    " feat ",
    " featuring ",
    " ft.",
    " ft ",
    " with ",
    " vs ",
];

/// L'interprete principale: il campo fino al primo [`OSPITI_DELL_INTERPRETE`].
fn interprete_principale(artista: &str) -> &str {
    // `to_ascii_lowercase` conserva la lunghezza in byte: vedi `primo_artista`.
    let minuscolo = artista.to_ascii_lowercase();
    let taglio = OSPITI_DELL_INTERPRETE
        .iter()
        .filter_map(|ospite| minuscolo.find(ospite))
        .min();
    taglio.map_or(artista, |dove| artista.get(..dove).unwrap_or(artista))
}

/// I nomi sotto cui un interprete può comparire: il campo intero, il suo
/// interprete principale e, con una virgola sola, il nome girato.
///
/// Così «Gorillaz, De La Soul» combacia con un caricamento di «Gorillaz»,
/// «Simon & Garfunkel» con «Simon and Garfunkel», e «Bach, Johann Sebastian»
/// — come l'Internet Archive scrive i classici — con «Johann Sebastian Bach».
///
/// **Mai** gli ospiti: fino alla 3.2.0 contava ogni pezzo, in tutti e due i
/// versi, e bastava che un pezzo qualsiasi coincidesse — un utente chiamato
/// «Simon» era Simon & Garfunkel, e un «DJ X feat. Radiohead» era Radiohead.
fn artisti_da_confrontare(artista: &str) -> Vec<String> {
    let mut nomi = vec![nome_da_confrontare(artista)];
    let mut aggiungi = |nome: String| {
        if !nome.is_empty() && !nomi.contains(&nome) {
            nomi.push(nome);
        }
    };
    aggiungi(nome_da_confrontare(interprete_principale(artista)));
    if let Some((cognome, nome)) = artista.split_once(',')
        && !nome.contains(',')
    {
        aggiungi(nome_da_confrontare(&format!("{nome} {cognome}")));
    }
    nomi.retain(|n| !n.is_empty());
    nomi
}

/// Quante modifiche di un carattere separano due testi, fino a `tetto`.
///
/// `None` appena si capisce che sono di più: la tolleranza qui è di un refuso
/// ogni dieci caratteri, e contare oltre non serve a niente.
fn distanza_entro(a: &str, b: &str, tetto: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > tetto {
        return None;
    }
    // Levenshtein con due righe: titoli di qualche decina di caratteri.
    let mut prima: Vec<usize> = (0..=b.len()).collect();
    let mut adesso = vec![0_usize; b.len().saturating_add(1)];
    for (i, ca) in a.iter().enumerate() {
        if let Some(primo) = adesso.first_mut() {
            *primo = i.saturating_add(1);
        }
        for (j, cb) in b.iter().enumerate() {
            let costo = usize::from(ca != cb);
            let sostituzione = prima
                .get(j)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(costo);
            let cancellazione = prima
                .get(j.saturating_add(1))
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            let inserimento = adesso
                .get(j)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            if let Some(cella) = adesso.get_mut(j.saturating_add(1)) {
                *cella = sostituzione.min(cancellazione).min(inserimento);
            }
        }
        if adesso.iter().min().is_some_and(|minimo| *minimo > tetto) {
            return None;
        }
        std::mem::swap(&mut prima, &mut adesso);
    }
    prima.last().copied().filter(|d| *d <= tetto)
}

/// I numeri romani che in un titolo numerano: «Interlude II», «Pt. IV».
const ROMANI: &[&str] = &[
    "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv", "xv",
    "xvi", "xvii", "xviii", "xix", "xx",
];

/// Le parole dopo cui un numero romano di una lettera è un numero e non una
/// parola («Pt. I», «Vol. V»).
const PAROLE_DI_NUMERO: &[&str] = &[
    "pt", "part", "parte", "vol", "volume", "no", "nr", "op", "opus", "act", "atto", "episode",
    "ep", "chapter", "capitolo", "book", "libro", "movement",
];

/// I numeri di un testo già ripulito, nell'ordine in cui compaiono.
///
/// Le cifre a gruppi — «no1» e «no 1» danno tutti e due `1` — e i numeri
/// romani. Un romano di una lettera conta solo dove non può essere una parola:
/// in fondo al titolo («Interlude I») o dopo una delle [`PAROLE_DI_NUMERO`].
/// Altrove `i` è il pronome inglese o l'articolo italiano, e `v`, `x` sono
/// lettere.
fn numeri(testo: &str) -> Vec<&str> {
    let mut trovati = Vec::new();
    let mut inizio: Option<usize> = None;
    for (dove, c) in testo
        .char_indices()
        .chain(std::iter::once((testo.len(), ' ')))
    {
        match (c.is_ascii_digit(), inizio) {
            (true, None) => inizio = Some(dove),
            (false, Some(da)) => {
                trovati.extend(testo.get(da..dove));
                inizio = None;
            }
            _ => {}
        }
    }
    let parole: Vec<&str> = testo.split(' ').filter(|p| !p.is_empty()).collect();
    for (n, parola) in parole.iter().enumerate() {
        if !ROMANI.contains(parola) {
            continue;
        }
        let ultima = n.saturating_add(1) == parole.len();
        let dopo_un_numerale = n
            .checked_sub(1)
            .and_then(|p| parole.get(p))
            .is_some_and(|p| PAROLE_DI_NUMERO.contains(p));
        if parola.len() >= 2 || ultima || dopo_un_numerale {
            trovati.push(parola);
        }
    }
    trovati
}

/// Quante modifiche separano due testi già ripuliti, se sono lo stesso a meno
/// di un refuso ogni dieci caratteri di quello atteso; `None` se non lo sono.
///
/// Uguaglianza, e non contenimento: «Heaven» non è «Stairway to Heaven», in
/// nessuno dei due versi.
///
/// # Un numero diverso non è un refuso
///
/// I [`numeri`] devono coincidere uno a uno, e la tolleranza vale sul resto.
/// Fino alla 3.2.0 un carattere diverso era un refuso qualunque: «Sunrise Pt.
/// 2» passava per «Sunrise Pt. 1», «Symphony No. 10» per «Symphony No. 1», e
/// la parte sbagliata finiva sul disco col nome di quella chiesta.
fn distanza(atteso: &str, suo: &str) -> Option<usize> {
    if atteso.is_empty() || suo.is_empty() {
        return None;
    }
    if atteso == suo {
        return Some(0);
    }
    if numeri(atteso) != numeri(suo) {
        return None;
    }
    #[expect(
        clippy::integer_division,
        reason = "un refuso ogni dieci caratteri interi: il resto non è un refuso in più"
    )]
    let tetto = atteso.chars().count() / 10;
    if tetto == 0 {
        return None;
    }
    distanza_entro(atteso, suo, tetto)
}

/// Due testi già ripuliti sono lo stesso: vedi [`distanza`].
fn quasi_uguali(atteso: &str, suo: &str) -> bool {
    distanza(atteso, suo).is_some()
}

/// Di quanto il titolo del candidato si scosta da quello chiesto, se il
/// candidato è il brano chiesto per titolo **e** per interprete; `None` se non
/// lo è.
///
/// È [`combacia`] con in più la misura: zero è il titolo esatto (a meno di come
/// lo si scrive), uno e oltre sono refusi tollerati. [`scegli_candidato`] la
/// mette in testa ai suoi criteri.
fn scarto_del_titolo(candidato: &Candidato, brano: &BranoEsterno) -> Option<usize> {
    let (Some(artista), Some(autore)) = (brano.artist.as_deref(), candidato.autore.as_deref())
    else {
        return None;
    };
    let scarto = distanza(
        &titolo_da_confrontare(&brano.title),
        &titolo_da_confrontare(&candidato.titolo),
    )?;
    let attesi = artisti_da_confrontare(artista);
    let suoi = artisti_da_confrontare(autore);
    attesi
        .iter()
        .any(|atteso| suoi.iter().any(|suo| quasi_uguali(atteso, suo)))
        .then_some(scarto)
}

/// Il candidato è il brano chiesto per titolo **e** per interprete.
///
/// Il cancello in testa a [`scegli_candidato`]: vedi la carta del modulo. Un
/// lato senza interprete — un candidato che non dice chi lo pubblica, un brano
/// che non dice di chi è — non si può confermare, e non passa.
#[must_use]
pub fn combacia(candidato: &Candidato, brano: &BranoEsterno) -> bool {
    scarto_del_titolo(candidato, brano).is_some()
}

/// Due titoli sono lo stesso brano, con la regola del titolo di [`combacia`].
///
/// Per chi deve confermare un brano che non dichiara l'interprete da nessuno
/// dei due lati: [`combacia`] non lo conferma mai, perché un lato senza
/// interprete potrebbe essere chiunque, ma quando mancano tutti e due resta il
/// titolo, ed è quel che finirà nel tag.
#[must_use]
pub fn stesso_titolo(atteso: &str, suo: &str) -> bool {
    quasi_uguali(&titolo_da_confrontare(atteso), &titolo_da_confrontare(suo))
}

/// La banda di durata in cui cade un candidato: 0 stretta, 1 larga.
///
/// Restituisce `None` quando la durata lo esclude del tutto.
fn banda(attesa_ms: Option<u64>, durata_sec: Option<u32>) -> Option<(u8, u64)> {
    let (Some(attesa), Some(sua_sec)) = (attesa_ms, durata_sec) else {
        // Una delle due durate non si sa: il candidato resta in gara, ma nella
        // banda larga e con lo scarto peggiore possibile, così perde ogni pari
        // merito contro uno di cui la durata si conosce e combacia.
        return Some((1, u64::MAX));
    };
    let scarto = attesa.abs_diff(u64::from(sua_sec).saturating_mul(1000));
    if scarto <= BANDA_STRETTA_MS {
        Some((0, scarto))
    } else if scarto <= BANDA_LARGA_MS {
        Some((1, scarto))
    } else {
        None
    }
}

/// La chiave d'ordine di [`scegli_candidato`]: più piccola è meglio.
///
/// Titolo a un refuso (0 esatto, 1 no), banda, natura, affidabilità, scarto di
/// durata: nell'ordine scritto sulla funzione.
type Chiave = (u8, u8, std::cmp::Reverse<u8>, std::cmp::Reverse<u8>, u64);

/// Sceglie il file da prendere, o nessuno.
///
/// Passa solo chi [`combacia`]: titolo e interprete sono un cancello, non un
/// criterio. Fra chi passa, l'ordine dei criteri, dal più forte al più debole:
///
/// 1. il titolo esatto prima di uno a un refuso: il refuso tollerato potrebbe
///    non esserlo, e un titolo esatto in banda larga è più sicuro di un titolo
///    quasi uguale in banda stretta;
/// 2. la banda di durata — stretta prima di larga, e fuori dalla larga si scarta;
/// 3. la [`Natura`] — la registrazione chiesta prima di un'altra dello stesso brano;
/// 4. l'[`Affidabilita`] di chi pubblica;
/// 5. lo scarto di durata;
/// 6. l'ordine in cui il catalogo li ha restituiti, che è la sua idea di
///    pertinenza e vale come ultimo spareggio invece di un `first()` arbitrario.
///
/// Con `alternative_ammesse` a `false` tutto ciò che non è [`Natura::Studio`]
/// esce di gara: è il comportamento giusto per chi sta ricostruendo un disco, e
/// quello sbagliato per chi cerca in un archivio di concerti. La scelta non è di
/// questo modulo.
///
/// `None` quando non ne resta nessuno: è un esito legittimo e **terminale**, da
/// non confondere con «la ricerca è fallita». Chi chiama deve poter distinguere
/// il brano che nel catalogo non c'è — inutile ritentarlo per sempre — da quello
/// per cui la richiesta è andata storta.
#[must_use]
pub fn scegli_candidato<'a>(
    candidati: &'a [Candidato],
    brano: &BranoEsterno,
    alternative_ammesse: bool,
) -> Option<&'a Candidato> {
    let mut migliore: Option<(&Candidato, Chiave)> = None;

    for candidato in candidati {
        if candidato.url.trim().is_empty() {
            continue;
        }
        if !alternative_ammesse && candidato.natura != Natura::Studio {
            continue;
        }
        let Some(titolo) = scarto_del_titolo(candidato, brano) else {
            continue;
        };
        let Some((banda_sua, scarto)) = banda(brano.duration_ms, candidato.durata_sec) else {
            continue;
        };
        let natura = candidato.natura.peso();
        let fiducia = affidabilita(
            candidato.autore.as_deref(),
            candidato.autore_verificato,
            brano.artist.as_deref(),
        )
        .peso();

        // Il `<` stretto lascia il primo a pari merito: è l'ultimo criterio,
        // l'ordine del catalogo.
        let sua = (
            u8::from(titolo > 0),
            banda_sua,
            std::cmp::Reverse(natura),
            std::cmp::Reverse(fiducia),
            scarto,
        );
        if migliore.is_none_or(|(_, ora)| sua < ora) {
            migliore = Some((candidato, sua));
        }
    }

    migliore.map(|(candidato, ..)| candidato)
}

#[cfg(test)]
mod prove {
    use super::*;

    fn brano(artista: &str, titolo: &str, durata_ms: Option<u64>) -> BranoEsterno {
        BranoEsterno {
            title: titolo.to_owned(),
            artist: Some(artista.to_owned()),
            duration_ms: durata_ms,
            ..BranoEsterno::default()
        }
    }

    fn cand(url: &str, titolo: &str, autore: &str, durata_sec: u32) -> Candidato {
        Candidato {
            url: url.to_owned(),
            titolo: titolo.to_owned(),
            autore: Some(autore.to_owned()),
            durata_sec: Some(durata_sec),
            ..Candidato::default()
        }
    }

    #[test]
    fn lautore_che_si_chiama_come_linterprete_vince_a_parita_di_durata() {
        let b = brano("Radiohead", "Karma Police", Some(264_000));
        let candidati = [
            cand("https://a/uno", "Karma Police", "Raccolta Anni 90", 264),
            cand("https://a/due", "Karma Police", "Radiohead", 264),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/due")
        );
    }

    #[test]
    fn la_durata_e_un_cancello_che_lautore_non_apre() {
        // Un'ora di disco intero sul canale verificato dell'artista, con il
        // titolo della canzone, non è la canzone.
        let b = brano("Radiohead", "Karma Police", Some(264_000));
        let candidati = [
            Candidato {
                autore_verificato: true,
                ..cand("https://a/album", "Karma Police", "Radiohead", 3_180)
            },
            cand("https://a/brano", "Karma Police", "radiohead", 262),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/brano")
        );
    }

    #[test]
    fn oltre_la_banda_larga_non_resta_nessuno() {
        let b = brano("Radiohead", "Karma Police", Some(264_000));
        let candidati = [cand("https://a/x", "Karma Police", "Radiohead", 400)];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
    }

    #[test]
    fn lo_studio_batte_il_live_anche_se_il_live_e_verificato() {
        // Fino alla 3.2 lo studio vinceva anche su un autore qualunque; adesso
        // l'autore dev'essere quello chiesto, e fra due suoi caricamenti la
        // natura viene prima della spunta.
        let b = brano("Grateful Dead", "Sugaree", Some(300_000));
        let candidati = [
            Candidato {
                natura: Natura::DalVivo,
                autore_verificato: true,
                ..cand("https://a/live", "Sugaree", "Grateful Dead", 300)
            },
            cand("https://a/studio", "Sugaree", "Grateful Dead", 300),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/studio")
        );
    }

    #[test]
    fn senza_alternative_ammesse_un_live_non_e_una_risposta() {
        let b = brano("Grateful Dead", "Sugaree", Some(300_000));
        let candidati = [Candidato {
            natura: Natura::DalVivo,
            ..cand("https://a/live", "Sugaree", "Grateful Dead", 300)
        }];
        assert!(scegli_candidato(&candidati, &b, true).is_some());
        assert_eq!(scegli_candidato(&candidati, &b, false), None);
    }

    #[test]
    fn stayin_alive_non_e_un_concerto() {
        // Il caso che il confronto per sottostringa sbaglierebbe.
        assert_eq!(
            natura_dal_titolo("Stayin' Alive", "Stayin' Alive"),
            Natura::Studio
        );
        assert_eq!(
            natura_dal_titolo("Sugaree (Live at Winterland)", "Sugaree"),
            Natura::DalVivo
        );
    }

    #[test]
    fn chi_cerca_un_remix_lo_trova() {
        // La parola sta in tutti e due i titoli: non dice più niente di nuovo.
        assert_eq!(
            natura_dal_titolo("Song (Todd Terje Remix)", "Song - Todd Terje Remix"),
            Natura::Studio
        );
        assert_eq!(
            natura_dal_titolo("Song (Todd Terje Remix)", "Song"),
            Natura::Alternativa
        );
    }

    #[test]
    fn una_durata_ignota_resta_in_gara_ma_perde_i_pari_merito() {
        let b = brano("A", "T", Some(200_000));
        let candidati = [
            Candidato {
                durata_sec: None,
                ..cand("https://a/muta", "T", "A", 0)
            },
            cand("https://a/nota", "T", "A", 200),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/nota")
        );
    }

    #[test]
    fn un_titolo_diverso_non_e_una_risposta() {
        // L'archivio di Spotify non porta durate: senza un titolo da
        // confrontare, il solo scaricabile fra otto risultati vinceva, e
        // finiva sul disco col nome della canzone chiesta.
        let b = brano("Radiohead", "Karma Police", None);
        let candidati = [cand("https://a/u", "Paranoid Android", "Radiohead", 380)];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
        // Il contenimento non è una corrispondenza, in nessuno dei due versi.
        let b = brano("Led Zeppelin", "Stairway to Heaven", None);
        let candidati = [cand("https://a/u", "Heaven", "Led Zeppelin", 300)];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
        let b = brano("Led Zeppelin", "Heaven", None);
        let candidati = [cand(
            "https://a/u",
            "Stairway to Heaven",
            "Led Zeppelin",
            300,
        )];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
    }

    #[test]
    fn un_altro_artista_non_e_una_risposta() {
        // La durata giusta su un caricamento di qualcun altro è una cover, o
        // un'altra canzone con lo stesso nome: non è il brano chiesto.
        let b = brano("Radiohead", "Karma Police", Some(264_000));
        let candidati = [cand("https://a/u", "Karma Police", "Fan Qualunque", 264)];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
        // E chi non dice chi è non si può confermare.
        let senza = [Candidato {
            autore: None,
            ..cand("https://a/u", "Karma Police", "x", 264)
        }];
        assert_eq!(scegli_candidato(&senza, &b, true), None);
        // Né si conferma un brano che non dice di chi è.
        let anonimo = BranoEsterno {
            artist: None,
            ..b.clone()
        };
        let giusto = [cand("https://a/u", "Karma Police", "Radiohead", 264)];
        assert_eq!(scegli_candidato(&giusto, &anonimo, true), None);
    }

    #[test]
    fn titolo_e_artista_si_confrontano_su_testo_normalizzato() {
        // Maiuscole, accenti, punteggiatura, «feat.», le code d'edizione e un
        // refuso: sono lo stesso brano scritto da due persone diverse.
        for ((artista, titolo), (autore, suo)) in [
            (("Björk", "Jóga"), ("bjork", "JOGA")),
            (
                ("Radiohead", "Karma Police"),
                ("Radiohead", "Karma Police (Remastered)"),
            ),
            (
                ("Radiohead", "Karma Police"),
                ("Radiohead", "Karma Police - 2017 Remaster"),
            ),
            (
                ("Radiohead", "Karma Police"),
                ("Radiohead", "Karma Police Remastered 2009"),
            ),
            (
                ("Gorillaz", "Feel Good Inc."),
                (
                    "Gorillaz feat. De La Soul",
                    "Feel Good Inc (feat. De La Soul)",
                ),
            ),
            (
                ("Gorillaz, De La Soul", "Feel Good Inc."),
                ("Gorillaz", "Feel Good Inc. feat. De La Soul"),
            ),
            (
                ("Guns N' Roses", "Sweet Child O' Mine"),
                ("Guns N Roses", "Sweet Child O Mine"),
            ),
            (("The Beatles", "Let It Be"), ("Beatles", "Let It Be")),
            (
                ("Simon & Garfunkel", "The Boxer"),
                ("Simon and Garfunkel", "The Boxer"),
            ),
            (
                ("Grateful Dead", "Sugaree"),
                ("Grateful Dead", "Sugaree ->"),
            ),
            (
                ("Radiohead", "Karma Police"),
                ("Radiohead", "Karma Pollice"),
            ),
        ] {
            let b = brano(artista, titolo, None);
            let candidati = [cand("https://a/u", suo, autore, 260)];
            assert!(
                scegli_candidato(&candidati, &b, true).is_some(),
                "«{autore} – {suo}» per «{artista} – {titolo}»"
            );
        }
    }

    #[test]
    fn senza_una_licenza_che_lo_permetta_non_si_tiene() {
        // La disponibilità di una riga ricostruita da `desiderati` non è un
        // permesso: l'importazione da un link non salvava la licenza, e un
        // profilo importato la scrive come vuole. Senza una licenza che
        // permetta la copia, non si tiene.
        let scaricabile = |licenza: Licenza| Candidato {
            licenza,
            disponibilita: Disponibilita::Scaricabile,
            ..Candidato::default()
        };
        assert!(!scaricabile(Licenza::Sconosciuta).si_puo_tenere());
        assert!(!scaricabile(Licenza::TutteRiservate).si_puo_tenere());
        assert!(scaricabile(Licenza::LiberaNonCommerciale).si_puo_tenere());
        assert!(scaricabile(Licenza::CreativeCommons("by".to_owned())).si_puo_tenere());
        // E una licenza libera non basta senza il permesso della fonte.
        assert!(
            !Candidato {
                licenza: Licenza::PubblicoDominio,
                disponibilita: Disponibilita::SoloAscolto,
                ..Candidato::default()
            }
            .si_puo_tenere()
        );
    }

    #[test]
    fn la_tolleranza_e_un_refuso_ogni_dieci_caratteri() {
        assert_eq!(distanza_entro("kitten", "sitting", 3), Some(3));
        assert_eq!(distanza_entro("kitten", "sitting", 2), None);
        // Un titolo corto non ha margine: «Hey Dude» non è «Hey Jude».
        assert!(!quasi_uguali("hey jude", "hey dude"));
        assert!(quasi_uguali("karma police", "karma pollice"));
        assert!(!quasi_uguali("karma police", "karma polizia"));
    }

    #[test]
    fn le_registrazioni_diverse_sono_dello_stesso_artista() {
        // Con l'interruttore acceso un live dello stesso artista passa, e
        // spento no; una cover di un altro non passa mai.
        let b = brano("Radiohead", "Creep", None);
        let live = [Candidato {
            natura: Natura::DalVivo,
            ..cand(
                "https://a/live",
                "Creep (Live at Glastonbury)",
                "Radiohead",
                260,
            )
        }];
        assert!(scegli_candidato(&live, &b, true).is_some());
        assert_eq!(scegli_candidato(&live, &b, false), None);
        let cover = [Candidato {
            natura: Natura::Alternativa,
            ..cand("https://a/cover", "Creep (Cover)", "Uno Qualunque", 238)
        }];
        assert_eq!(scegli_candidato(&cover, &b, true), None);
    }

    #[test]
    fn un_numero_diverso_non_e_un_refuso() {
        // Senza durate (l'archivio di Spotify) erano tutti nella banda larga, e
        // un carattere diverso valeva come un refuso qualunque: la parte 2 si
        // scaricava col nome della parte 1.
        let b = brano("Artista", "Sunrise, Pt. 1", None);
        let seconda = cand("https://a/due", "Sunrise, Pt. 2", "Artista", 300);
        assert_eq!(
            scegli_candidato(std::slice::from_ref(&seconda), &b, true),
            None
        );
        let candidati = [
            seconda,
            cand("https://a/uno", "Sunrise, Pt. 1", "Artista", 300),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/uno")
        );
        assert!(!quasi_uguali("symphony no 1", "symphony no 10"));
        assert!(!quasi_uguali("etude op 10 no 1", "etude op 10 no 12"));
        assert!(!quasi_uguali("interlude i", "interlude ii"));
        assert!(!quasi_uguali("sunrise pt i", "sunrise pt v"));
        // Un anno dentro il titolo è il titolo: si toglie solo accanto a una
        // parola d'edizione.
        let b = brano("Artista", "Class of 1984", None);
        let altro = [cand("https://a/u", "Class of 1999", "Artista", 200)];
        assert_eq!(scegli_candidato(&altro, &b, true), None);
        let b = brano("Radiohead", "Karma Police", None);
        for suo in ["Karma Police Remastered 2009", "Karma Police 2017 Remaster"] {
            let giusto = [cand("https://a/u", suo, "Radiohead", 260)];
            assert!(scegli_candidato(&giusto, &b, true).is_some(), "«{suo}»");
        }
        // «No.1» e «No. 1» sono lo stesso numero scritto in due modi.
        assert!(quasi_uguali("symphony no1", "symphony no 1"));
    }

    #[test]
    fn il_titolo_esatto_vince_prima_della_durata() {
        // Un refuso nella banda stretta contro il titolo esatto nella larga:
        // vince il titolo esatto, perché il refuso potrebbe non esserlo.
        let b = brano("Radiohead", "Karma Police", Some(264_000));
        let candidati = [
            cand("https://a/refuso", "Karma Pollice", "Radiohead", 264),
            cand("https://a/esatto", "Karma Police", "Radiohead", 300),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/esatto")
        );
    }

    #[test]
    fn un_pezzo_del_nome_non_e_l_artista() {
        // Un utente che si chiama «Simon» non è Simon & Garfunkel.
        let b = brano("Simon & Garfunkel", "The Boxer", None);
        let candidati = [cand("https://a/u", "The Boxer", "Simon", 300)];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
        let candidati = [cand("https://a/u", "The Boxer", "Garfunkel", 300)];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
        // «feat» vuole un confine dopo: «Little Feat» non è «Little».
        let b = brano("Little Feat", "Dixie Chicken", None);
        let candidati = [cand("https://a/u", "Dixie Chicken", "Little", 300)];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
        let b = brano("Sam Feather", "Song", None);
        let candidati = [cand("https://a/u", "Song", "Sam", 300)];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
        // E dal lato di chi carica: un ospite o un pezzo dopo la virgola non è
        // chi pubblica.
        let b = brano("Radiohead", "Creep", None);
        for autore in [
            "DJ X feat. Radiohead",
            "Covers, Radiohead",
            "Uno & Radiohead",
        ] {
            let candidati = [cand("https://a/u", "Creep", autore, 238)];
            assert_eq!(scegli_candidato(&candidati, &b, true), None, "«{autore}»");
        }
        // L'interprete principale invece basta, dai due lati.
        let b = brano("Radiohead feat Uno", "Creep", None);
        let candidati = [cand("https://a/u", "Creep", "Radiohead", 238)];
        assert!(scegli_candidato(&candidati, &b, true).is_some());
    }

    #[test]
    fn la_e_commerciale_e_il_cognome_davanti_si_normalizzano() {
        // «&» e «and» anche nei titoli, e «Cognome, Nome» come lo scrive
        // l'Internet Archive per i classici.
        for ((artista, titolo), (autore, suo)) in [
            (("Elvis", "Rock & Roll"), ("Elvis", "Rock and Roll")),
            (
                ("Johann Sebastian Bach", "Air"),
                ("Bach, Johann Sebastian", "Air"),
            ),
            (
                ("Bach, Johann Sebastian", "Air"),
                ("Johann Sebastian Bach", "Air"),
            ),
        ] {
            let b = brano(artista, titolo, None);
            let candidati = [cand("https://a/u", suo, autore, 260)];
            assert!(
                scegli_candidato(&candidati, &b, true).is_some(),
                "«{autore} – {suo}» per «{artista} – {titolo}»"
            );
        }
    }

    #[test]
    fn lo_scarto_e_firmato_perche_il_verso_conta_a_schermo() {
        assert_eq!(scarto_durata(Some(200_000), Some(208)), Some(8_000));
        assert_eq!(scarto_durata(Some(200_000), Some(192)), Some(-8_000));
        assert_eq!(scarto_durata(None, Some(192)), None);
    }

    #[test]
    fn la_query_larga_tace_quando_non_aggiunge_niente() {
        let b = brano("Radiohead", "Karma Police", None);
        assert_eq!(query_stretta(&b), "Radiohead Karma Police");
        assert_eq!(query_larga(&b), None);

        let b = brano("Gorillaz, De La Soul", "Feel Good Inc. (Remastered)", None);
        assert!(query_larga(&b).is_some_and(|q| q.starts_with("Gorillaz ")));
    }
}
