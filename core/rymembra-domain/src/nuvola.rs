//! Le cartelle che stanno su un servizio cloud, come le vede la libreria.
//!
//! # La scelta: un percorso, non una colonna
//!
//! Un brano di OneDrive è una riga di `tracks` con un `path` come tutte le
//! altre: `nuvola://onedrive-k3x7q2ab/Musica/Rock/01 Intro.flac`. E una cartella
//! di Dropbox è una voce di `library.roots` come `\\nas\Musica`. Non c'è una
//! seconda specie di brano né una seconda lista di cartelle, ed è il punto.
//!
//! Tutto quel che la libreria sa fare con un percorso lo fa anche con questo
//! senza saperlo: [`crate::paths::path_key`] e [`crate::paths::is_under`]
//! decidono cosa sta sotto cosa, [`crate::album::album_folder`] raggruppa per
//! cartella madre, il titolo di ripiego viene dal nome del file e il
//! suggerimento di formato dall'estensione. La scansione confronta dimensione e
//! data come per un file del disco, la guardia sulle rimozioni di massa vale
//! uguale, e il pannello «Cartelle» disegna l'albero dalle righe come sempre.
//! Una colonna `servizio` accanto a `path` avrebbe chiesto a ciascuno di quei
//! posti di imparare un secondo modo di dire «dove sta questo brano».
//!
//! Quel che un percorso così **non** sa fare — aprirsi con `std::fs` — lo
//! scopre chi apre, e chi apre passa da [`e_nuvola`]: è l'unico predicato «è
//! nel cloud» del programma. Una seconda scrittura della stessa domanda
//! (`starts_with("nuvola:")` sparso qua e là) sarebbe il primo posto in cui le
//! due risposte smettono di coincidere.
//!
//! # Perché i segmenti si codificano
//!
//! Perché i nomi dei servizi ammettono caratteri che qui dentro hanno un altro
//! significato. Google Drive accetta `/` nel nome di un file — per lui non è un
//! separatore, perché le cartelle sono genitori e non prefissi — e `|` è il
//! separatore dei brani d'immagine ([`crate::cue::SEPARATORE_BRANO`]): un
//! `Live | Unplugged.flac` diventerebbe il brano 0 del file `Live `. Si
//! codificano col `%` esattamente i caratteri che romperebbero qualcosa, e
//! nient'altro: un nome con l'accento resta leggibile anche nel database.
//!
//! # Perché il conto è un'impronta e non un numero progressivo
//!
//! Perché le cartelle viaggiano: la sincronia porta `library.roots` sugli altri
//! PC, e il backup lo porta su un PC nuovo. Un conto chiamato «il primo
//! OneDrive collegato su questo PC» vorrebbe dire una cosa diversa altrove. Un
//! conto che viene dall'identità stabile dell'account — l'id del drive per
//! Graph, il `permissionId` per Google, l'`account_id` per Dropbox — è lo
//! stesso ovunque quell'account si colleghi: chi collega OneDrive sul portatile
//! ritrova le cartelle che aveva scelto sul fisso, con le loro righe, i loro
//! ascolti e i loro voti. L'impronta la calcola `rymembra-cloud`; qui si decide
//! solo che forma ha ([`e_conto`]).

use crate::paths::{extension_of, file_stem, is_supported_audio_path};

/// Il prefisso di ogni percorso nel cloud.
///
/// Con le due barre perché `nuvola:` da solo, su Windows, somiglia troppo a una
/// lettera di unità scritta male; e perché fra i percorsi che la libreria
/// conosce nessuno comincia così: non un disco (`C:\`), non una share (`\\`),
/// non un brano di catalogo (che `path` non ce l'ha).
pub const SCHEMA: &str = "nuvola://";

/// Il servizio di un conto.
///
/// Un `enum` e non un tratto, come [`crate::scrobble::Servizio`]: serve a
/// **nominarli** — nel conto, negli errori, nelle schermate. Come si parla con
/// ciascuno è affare di `rymembra-cloud`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Servizio {
    /// OneDrive personale, attraverso Microsoft Graph.
    OneDrive,
    /// Google Drive: «Il mio Drive» e le cartelle condivise con l'account.
    GoogleDrive,
    /// Dropbox.
    Dropbox,
    /// Un server WebDAV qualunque: Nextcloud, ownCloud, pCloud, Koofr…
    WebDav,
    /// Un account MEGA, con email e password.
    Mega,
    /// Una cartella MEGA condivisa con un link, senza account.
    MegaLink,
}

impl Servizio {
    /// Il nome stabile: sta nel conto, quindi in ogni percorso. Non cambia mai.
    #[must_use]
    pub const fn chiave(self) -> &'static str {
        match self {
            Self::OneDrive => "onedrive",
            Self::GoogleDrive => "gdrive",
            Self::Dropbox => "dropbox",
            Self::WebDav => "webdav",
            Self::Mega => "mega",
            Self::MegaLink => "megalink",
        }
    }

    /// Il nome come lo scrive chi lo gestisce, per le schermate.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::OneDrive => "OneDrive",
            Self::GoogleDrive => "Google Drive",
            Self::Dropbox => "Dropbox",
            Self::WebDav => "WebDAV",
            Self::Mega | Self::MegaLink => "MEGA",
        }
    }

    /// Dal nome stabile.
    #[must_use]
    pub fn da_chiave(chiave: &str) -> Option<Self> {
        Self::tutti().into_iter().find(|s| s.chiave() == chiave)
    }

    /// Tutti, in ordine di dichiarazione.
    #[must_use]
    pub const fn tutti() -> [Self; 6] {
        [
            Self::OneDrive,
            Self::GoogleDrive,
            Self::Dropbox,
            Self::WebDav,
            Self::Mega,
            Self::MegaLink,
        ]
    }

    /// Presso chi si registra l'app con cui ci si collega, per i servizi che
    /// chiedono il consenso nel browser. `None` per chi si collega con un
    /// modulo (WebDAV, MEGA): lì l'utente scrive le sue credenziali, non
    /// quelle di un'app.
    #[must_use]
    pub const fn piattaforma(self) -> Option<crate::nuvola_app::Piattaforma> {
        use crate::nuvola_app::Piattaforma;
        match self {
            Self::OneDrive => Some(Piattaforma::Microsoft),
            Self::GoogleDrive => Some(Piattaforma::Google),
            Self::Dropbox => Some(Piattaforma::Dropbox),
            Self::WebDav | Self::Mega | Self::MegaLink => None,
        }
    }

    /// Il servizio di un conto, dal suo nome.
    #[must_use]
    pub fn del_conto(conto: &str) -> Option<Self> {
        let (chiave, impronta) = conto.split_once('-')?;
        if !impronta_valida(impronta) {
            return None;
        }
        Self::da_chiave(chiave)
    }
}

/// Quanti caratteri ha l'impronta di un conto.
///
/// Otto, in base32 minuscola: quaranta bit. Due account dello stesso servizio
/// collegati alla stessa libreria si confondono con probabilità di uno su mille
/// miliardi, e il percorso resta corto abbastanza da leggersi in un diario.
pub const LUNGHEZZA_IMPRONTA: usize = 8;

/// L'impronta di un conto ha la forma giusta?
fn impronta_valida(impronta: &str) -> bool {
    impronta.len() == LUNGHEZZA_IMPRONTA
        && impronta
            .bytes()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
}

/// `conto` è il nome di un conto: `<servizio>-<impronta>`.
#[must_use]
pub fn e_conto(conto: &str) -> bool {
    Servizio::del_conto(conto).is_some()
}

/// Il nome di un conto, dal servizio e dall'impronta già calcolata.
///
/// `None` se l'impronta non ha la forma di [`LUNGHEZZA_IMPRONTA`] caratteri di
/// base32 minuscola: un conto scritto male finirebbe in ogni percorso di
/// quell'account, e non si correggerebbe più senza riscriverli tutti.
#[must_use]
pub fn conto(servizio: Servizio, impronta: &str) -> Option<String> {
    impronta_valida(impronta).then(|| format!("{}-{impronta}", servizio.chiave()))
}

/// Il percorso è nel cloud?
///
/// L'unica domanda di questo genere nel programma: vedi il commento in testa.
#[must_use]
pub fn e_nuvola(percorso: &str) -> bool {
    percorso.starts_with(SCHEMA)
}

/// I caratteri che un segmento non può contenere così come sono.
///
/// `/` e `\` perché sono separatori per [`crate::paths`]; `|` perché separa il
/// brano dal foglio CUE; `%` perché è il carattere di fuga, e un nome che lo
/// contiene già (`100% Hits`) deve tornare uguale.
const DA_CODIFICARE: [char; 4] = ['%', '/', '\\', '|'];

/// Un nome di file o di cartella, pronto per stare in un percorso.
///
/// Un nome fatto solo di punti (`.` o `..`) si codifica per intero: in un
/// percorso vorrebbe dire «qui» o «sopra», e nessuno deve risalire da un brano
/// alla cartella madre per sbaglio. Un nome vuoto non è un nome e resta vuoto:
/// lo scarta chi compone.
#[must_use]
pub fn codifica_segmento(nome: &str) -> String {
    if !nome.is_empty() && nome.chars().all(|c| c == '.') {
        return "%2E".repeat(nome.chars().count());
    }
    let mut fuori = String::with_capacity(nome.len());
    for c in nome.chars() {
        if DA_CODIFICARE.contains(&c) {
            fuori.push_str(&format!("%{:02X}", u32::from(c)));
        } else {
            fuori.push(c);
        }
    }
    fuori
}

/// Il nome com'era sul servizio, da un segmento.
///
/// Decodifica solo le fughe che [`codifica_segmento`] produce: un `%` seguito
/// da qualcosa che non è una di quelle resta un `%`. Così un segmento scritto a
/// mano, o venuto da una versione futura con una regola più larga, si legge
/// lo stesso invece di diventare un errore.
#[must_use]
pub fn decodifica_segmento(segmento: &str) -> String {
    let mut fuori = String::with_capacity(segmento.len());
    let mut resto = segmento;
    while let Some(i) = resto.find('%') {
        fuori.push_str(resto.get(..i).unwrap_or_default());
        let dopo = resto.get(i + 1..).unwrap_or_default();
        let fuga = dopo.get(..2).and_then(|h| u8::from_str_radix(h, 16).ok());
        match fuga.map(char::from) {
            Some(c) if DA_CODIFICARE.contains(&c) || c == '.' => {
                fuori.push(c);
                resto = dopo.get(2..).unwrap_or_default();
            }
            _ => {
                fuori.push('%');
                resto = dopo;
            }
        }
    }
    fuori.push_str(resto);
    fuori
}

/// Il percorso di un conto e dei suoi segmenti, già codificati.
///
/// Chi chiama passa segmenti prodotti da [`codifica_segmento`] (o da
/// [`segmento_libero`]): qui non si codifica una seconda volta, perché un
/// segmento codificato due volte non si decodifica più nel nome giusto. I
/// segmenti vuoti si saltano.
#[must_use]
pub fn componi<'a>(conto: &str, segmenti: impl IntoIterator<Item = &'a str>) -> String {
    let mut fuori = format!("{SCHEMA}{conto}");
    for segmento in segmenti {
        if segmento.is_empty() {
            continue;
        }
        fuori.push('/');
        fuori.push_str(segmento);
    }
    fuori
}

/// Una radice di `library.roots`, letta al confine di chi la usa.
///
/// # Perché un tipo, e non un `if` in ogni posto
///
/// Perché le radici le leggono in una dozzina: la scansione, la sorveglianza,
/// il cestino, la cartella dei download, il trasloco, la rimappatura dei
/// profili, il backup e la sincronia. Una radice del cloud per ciascuno di loro
/// vuol dire una cosa precisa — sorveglia non la guarda col `notify`, il
/// cestino non esiste, i download non ci finiscono, `canonicalize` non va
/// nemmeno tentato — e un `match` su questo tipo obbliga chi scrive il
/// prossimo consumatore a deciderlo, invece di scoprirlo quando
/// `fs::metadata("nuvola://…")` fallisce e lo si legge come «disco staccato».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Radice<'a> {
    /// Una cartella del disco o della rete locale: tutto come prima.
    Locale(&'a str),
    /// Una cartella di un servizio.
    Nuvola {
        /// Il conto: `onedrive-k3x7q2ab`.
        conto: &'a str,
        /// Il percorso intero, schema compreso.
        percorso: &'a str,
    },
}

impl<'a> Radice<'a> {
    /// Legge una radice.
    ///
    /// Un percorso che comincia con [`SCHEMA`] ma non ha un conto valido è
    /// comunque del cloud — un conto che non si riconosce — e non torna
    /// [`Radice::Locale`]: trattarlo da cartella del disco vorrebbe dire
    /// chiedere a Windows di aprirlo.
    #[must_use]
    pub fn da(percorso: &'a str) -> Self {
        match percorso.strip_prefix(SCHEMA) {
            Some(resto) => Self::Nuvola {
                conto: resto.split('/').next().unwrap_or_default(),
                percorso,
            },
            None => Self::Locale(percorso),
        }
    }

    /// Il percorso così com'è scritto.
    #[must_use]
    pub const fn percorso(self) -> &'a str {
        match self {
            Self::Locale(percorso) | Self::Nuvola { percorso, .. } => percorso,
        }
    }

    /// È una cartella del disco?
    #[must_use]
    pub const fn locale(self) -> Option<&'a str> {
        match self {
            Self::Locale(percorso) => Some(percorso),
            Self::Nuvola { .. } => None,
        }
    }
}

/// Le radici del disco, in ordine, senza quelle del cloud.
///
/// Per i consumatori che con il cloud non hanno niente da fare — la
/// sorveglianza col `notify`, la risoluzione dei nomi di rete — e che fino a
/// ieri leggevano l'elenco intero.
#[must_use]
pub fn solo_locali(radici: &[String]) -> Vec<String> {
    radici
        .iter()
        .filter_map(|r| Radice::da(r).locale().map(str::to_owned))
        .collect()
}

/// Il conto di un percorso del cloud.
#[must_use]
pub fn conto_di(percorso: &str) -> Option<&str> {
    match Radice::da(percorso) {
        Radice::Nuvola { conto, .. } => Some(conto),
        Radice::Locale(_) => None,
    }
}

/// I nomi, decodificati, dei segmenti dopo il conto.
#[must_use]
pub fn segmenti(percorso: &str) -> Vec<String> {
    percorso
        .strip_prefix(SCHEMA)
        .map(|resto| {
            resto
                .split('/')
                .skip(1)
                .filter(|s| !s.is_empty())
                .map(decodifica_segmento)
                .collect()
        })
        .unwrap_or_default()
}

/// Il file interessa allo specchio di un servizio?
///
/// L'audio che la libreria sa suonare, i fogli CUE (per le immagini dei CD) e i
/// testi `.lrc` accanto ai brani. Nient'altro: lo specchio di un Drive con
/// trentamila fotografie non deve tenerne nemmeno una.
#[must_use]
pub fn interessa(nome: &str) -> bool {
    is_supported_audio_path(nome) || matches!(extension_of(nome).as_str(), "cue" | "lrc")
}

/// La voce è una cartella che il servizio non ha?
///
/// # Il caso
///
/// Lo specchio vuole **una** cima per conto: ogni voce senza madre ha il
/// percorso del conto, e due cime si darebbero lo stesso. Un servizio che ha
/// voci senza una madre visibile — le cartelle che altri condividono con un
/// account Google, la cui madre sta nel drive di chi le condivide — le appende
/// a una cartella sua sotto la cima, che sul servizio non esiste. Nel selettore
/// la si attraversa ma non la si sceglie: non ha un elenco suo da chiedere.
///
/// # Perché `~`
///
/// Perché nessun servizio comincia così un identificativo: Graph scrive
/// `D3E1F2A3B4C5!1234`, Drive e MEGA usano lettere, cifre, `-` e `_`, Dropbox e
/// WebDAV un percorso che comincia con `/`.
#[must_use]
pub fn e_virtuale(id: &str) -> bool {
    id.starts_with('~')
}

/// La sigla di un identificativo: gli ultimi sei caratteri alfanumerici.
///
/// Gli ultimi e non i primi, perché è lì che gli identificativi dei servizi
/// cambiano da un file all'altro: OneDrive scrive `D3E1F2A3B4C5!1234`, dove la
/// parte prima del `!` è il drive e resta uguale per tutti.
fn sigla(id: &str) -> String {
    let alfanumerici: Vec<char> = id.chars().filter(char::is_ascii_alphanumeric).collect();
    let da = alfanumerici.len().saturating_sub(6);
    alfanumerici.get(da..).unwrap_or_default().iter().collect()
}

/// Il segmento di un file in una cartella in cui il suo nome potrebbe essere
/// già preso.
///
/// # Il caso
///
/// Google Drive e MEGA ammettono due file con lo stesso nome nella stessa
/// cartella — caricare due volte `01.mp3` ne fa due — e su Windows la libreria
/// confronta i percorsi senza maiuscole ([`crate::paths::path_key`]), quindi
/// anche `Song.mp3` e `song.mp3` di un Nextcloud sono la stessa chiave. Due
/// righe con la stessa chiave si contendono ascolti e voti.
///
/// # La regola
///
/// Il primo che arriva tiene il nome. Gli altri prendono `nome~<sigla>.ext`,
/// dove la sigla viene dal loro identificativo, e se anche quello è preso
/// `nome~<sigla>-2.ext` e così via. L'estensione resta in fondo perché è da lì
/// che il decodificatore riceve il suggerimento di formato.
///
/// «Il primo che arriva» vuol dire che il risultato dipende da cosa c'era già:
/// è lo specchio a ricordare il segmento dato, e a non ricalcolarlo. Un file che
/// aveva il nome libero non lo perde perché ne arriva un secondo.
///
/// `occupato` risponde per un segmento **già codificato**; il confronto senza
/// maiuscole lo fa chi lo chiede, che sa con quali regole confronta.
#[must_use]
pub fn segmento_libero(nome: &str, id: &str, occupato: &dyn Fn(&str) -> bool) -> String {
    let semplice = codifica_segmento(nome);
    if !occupato(&semplice) {
        return semplice;
    }
    let estensione = extension_of(nome);
    let radice = if estensione.is_empty() {
        nome
    } else {
        file_stem(nome)
    };
    let coda = if estensione.is_empty() {
        String::new()
    } else {
        // L'estensione com'era scritta, maiuscole comprese: `extension_of` la
        // restituisce minuscola, ma il nome che si mostra è quello del servizio.
        let lunga = nome.len().saturating_sub(estensione.len());
        format!(".{}", nome.get(lunga..).unwrap_or(&estensione))
    };
    let sigla = sigla(id);
    let mut n = 1_u32;
    loop {
        let candidato = if n == 1 {
            format!("{radice}~{sigla}{coda}")
        } else {
            format!("{radice}~{sigla}-{n}{coda}")
        };
        let codificato = codifica_segmento(&candidato);
        if !occupato(&codificato) {
            return codificato;
        }
        n = n.saturating_add(1);
    }
}

/// Le voci dello specchio da togliere dopo un elenco.
///
/// # Perché l'elenco deve essere completo
///
/// Perché un elenco interrotto — la rete caduta alla pagina sette, un `429`
/// che non è passato, Rymembra chiuso a metà — non dice niente di quel che non
/// ha raggiunto. Togliere dallo specchio le voci che «non sono arrivate»
/// vorrebbe dire togliere dalla libreria, alla scansione dopo, i brani delle
/// pagine che non si sono lette: con i loro ascolti, i loro voti, le loro
/// playlist. La guardia sulle rimozioni di massa della scansione ne fermerebbe
/// solo una parte (più di cinquanta righe **e** più di un quinto della
/// libreria): le perdite piccole passerebbero in silenzio.
///
/// Quindi: un elenco che non è arrivato in fondo non toglie niente. Le voci
/// sparite le toglie il prossimo elenco completo, oppure una cancellazione
/// esplicita nel flusso dei cambiamenti.
#[must_use]
pub fn da_togliere<'a>(
    nello_specchio: impl IntoIterator<Item = &'a str>,
    elencati: &std::collections::HashSet<&str>,
    completo: bool,
) -> Vec<&'a str> {
    if !completo {
        return Vec::new();
    }
    nello_specchio
        .into_iter()
        .filter(|id| !elencati.contains(id))
        .collect()
}

/// Una voce di un servizio — un file o una cartella — come la racconta il
/// servizio.
///
/// # Perché sta nel dominio
///
/// Perché è il perno fra due crate che non si devono vedere, come
/// [`crate::scrobble`]: la produce `rymembra-cloud`, che parla con la rete e
/// non vede il database, e la consuma lo specchio di `rymembra-app`, che vede
/// il database e non la rete. Un valore puro su cui si incontrano, e nessuno
/// dei due sale sull'altro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoceRemota {
    /// L'identificativo stabile sul servizio: non cambia quando il file si
    /// rinomina o si sposta. Per WebDAV, che non ne ha, è l'`href`.
    pub id: String,
    /// La cartella madre. `None` per la cima del drive (o della cartella del
    /// link), che non ha una madre da nominare.
    pub genitore: Option<String>,
    /// Il nome com'è sul servizio, non codificato.
    pub nome: String,
    /// È una cartella.
    pub cartella: bool,
    /// I byte del file. Zero per le cartelle.
    pub dimensione: u64,
    /// L'ultima modifica, in millisecondi Unix troncati — la stessa forma che
    /// la scansione salva per i file del disco, perché il confronto
    /// «cambiato o no» è lo stesso.
    pub modificato_ms: i64,
    /// Quel che il servizio cambia a ogni nuova versione del contenuto: etag,
    /// `rev`, md5. Decide se la copia in cache vale ancora.
    pub versione: Option<String>,
    /// Quel che il servizio sa già dei tag, senza che si legga il file.
    pub tag: Option<TagNoti>,
}

/// I tag che un servizio dà insieme all'elenco.
///
/// Oggi solo OneDrive personale li ha (la faccetta `audio` di Graph): con quei
/// campi un brano entra in libreria già col suo titolo e il suo album, prima
/// che si legga un byte. Non sostituiscono i tag del file — la lettura vera
/// arriva dopo e ha l'ultima parola — ma tolgono i primi minuti di «01 -
/// Traccia.flac» in mezzo alla libreria.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagNoti {
    /// Titolo.
    pub titolo: Option<String>,
    /// Interprete.
    pub artista: Option<String>,
    /// Album.
    pub album: Option<String>,
    /// Artista dell'album.
    pub artista_album: Option<String>,
    /// Genere.
    pub genere: Option<String>,
    /// Anno.
    pub anno: Option<i32>,
    /// Numero di traccia.
    pub traccia: Option<u32>,
    /// Numero del disco.
    pub disco: Option<u32>,
    /// Durata in millisecondi.
    pub durata_ms: Option<u64>,
}

/// Un cambiamento sul servizio, dal flusso dei cambiamenti (delta di Graph,
/// `changes` di Drive, il cursore di Dropbox…).
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "i cambiamenti arrivano a pagine e si consumano subito: una scatola per voce               costerebbe un'allocazione per file a ogni pagina del delta, per risparmiare               memoria che vive il tempo di applicarla"
)]
pub enum Cambio {
    /// Una voce nuova o cambiata — anche solo rinominata o spostata.
    Voce(VoceRemota),
    /// Una voce cancellata, **detto dal servizio**. È l'unico modo, insieme a
    /// un elenco completo, per togliere qualcosa dallo specchio: vedi
    /// [`da_togliere`].
    Tolta {
        /// L'identificativo della voce.
        id: String,
    },
}

#[cfg(test)]
mod prove {
    use std::collections::HashSet;

    use super::*;
    use crate::album::album_folder;
    use crate::cue::brano_d_immagine;
    use crate::paths::{PathRules, is_under, path_key};

    const WIN: PathRules = PathRules {
        case_insensitive: true,
    };

    #[test]
    fn il_servizio_si_legge_dal_conto() {
        for servizio in Servizio::tutti() {
            let conto = conto(servizio, "k3x7q2ab").expect("impronta valida");
            assert_eq!(Servizio::del_conto(&conto), Some(servizio));
            assert!(e_conto(&conto));
        }
        assert_eq!(conto(Servizio::OneDrive, "K3X9Q2AB"), None, "maiuscole");
        assert_eq!(conto(Servizio::OneDrive, "k3x9q2a"), None, "corta");
        assert_eq!(
            conto(Servizio::OneDrive, "k3x9q2a1"),
            None,
            "1 non è base32"
        );
        assert!(!e_conto("ftp-k3x7q2ab"));
        assert!(!e_conto("onedrive"));
    }

    #[test]
    fn i_nomi_stabili_non_cambiano() {
        // Stanno dentro ogni percorso di ogni libreria: cambiarne uno vorrebbe
        // dire perdere tutti i brani di quel servizio.
        let chiavi: Vec<&str> = Servizio::tutti().iter().map(|s| s.chiave()).collect();
        assert_eq!(
            chiavi,
            [
                "onedrive", "gdrive", "dropbox", "webdav", "mega", "megalink"
            ]
        );
        for chiave in chiavi {
            assert!(!chiave.contains('-'), "il trattino separa l'impronta");
        }
    }

    #[test]
    fn un_segmento_torna_uguale() {
        for nome in [
            "01 Intro.flac",
            "Live | Unplugged.flac",
            "AC/DC",
            r"a\b",
            "100% Hits",
            "%2F già scritto così",
            "Perché è così",
            "..",
            ".",
            "...",
            ".hidden",
        ] {
            let codificato = codifica_segmento(nome);
            assert!(
                !codificato.contains(['/', '\\', '|']),
                "{nome} → {codificato}"
            );
            assert_eq!(decodifica_segmento(&codificato), nome);
        }
        assert_eq!(codifica_segmento(".."), "%2E%2E");
    }

    #[test]
    fn una_fuga_sconosciuta_resta_com_era() {
        assert_eq!(decodifica_segmento("50%"), "50%");
        assert_eq!(decodifica_segmento("50%zz"), "50%zz");
        assert_eq!(
            decodifica_segmento("%41"),
            "%41",
            "solo le fughe che produciamo"
        );
    }

    #[test]
    fn il_percorso_funziona_con_le_regole_di_sempre() {
        let radice = componi("onedrive-k3x7q2ab", ["Musica", "Rock"]);
        assert_eq!(radice, "nuvola://onedrive-k3x7q2ab/Musica/Rock");
        let brano = componi(
            "onedrive-k3x7q2ab",
            ["Musica", "Rock", &codifica_segmento("Live | 1977.flac")],
        );
        assert!(is_under(&brano, &radice, WIN));
        assert!(!is_under(
            &componi("onedrive-k3x7q2ab", ["Musica", "Rockabilly", "a.mp3"]),
            &radice,
            WIN
        ));
        // Il `|` codificato non è un brano d'immagine.
        assert_eq!(brano_d_immagine(&brano), None);
        // La cartella madre raggruppa l'album.
        assert_eq!(
            album_folder(&brano),
            "nuvola:/onedrive-k3x7q2ab/Musica/Rock"
        );
        assert_eq!(
            path_key(&brano, WIN),
            "nuvola://onedrive-k3x7q2ab/musica/rock/live %7c 1977.flac"
        );
        assert_eq!(segmenti(&brano), ["Musica", "Rock", "Live | 1977.flac"]);
        assert_eq!(
            componi("onedrive-k3x7q2ab", ["", "a"]),
            "nuvola://onedrive-k3x7q2ab/a"
        );
    }

    #[test]
    fn la_radice_si_riconosce() {
        assert_eq!(Radice::da(r"C:\Musica"), Radice::Locale(r"C:\Musica"));
        assert_eq!(Radice::da(r"\\nas\Musica"), Radice::Locale(r"\\nas\Musica"));
        let nuvola = "nuvola://dropbox-abcdefgh/Musica";
        assert_eq!(
            Radice::da(nuvola),
            Radice::Nuvola {
                conto: "dropbox-abcdefgh",
                percorso: nuvola
            }
        );
        assert_eq!(Radice::da(nuvola).locale(), None);
        assert_eq!(conto_di(nuvola), Some("dropbox-abcdefgh"));
        assert_eq!(conto_di(r"C:\Musica"), None);
        // Uno schema con un conto che non si riconosce resta del cloud.
        assert!(matches!(
            Radice::da("nuvola://"),
            Radice::Nuvola { conto: "", .. }
        ));
        assert_eq!(
            solo_locali(&[r"C:\A".into(), nuvola.into(), r"\\nas\B".into()]),
            [r"C:\A", r"\\nas\B"]
        );
    }

    #[test]
    fn interessano_audio_fogli_e_testi() {
        for sì in ["a.flac", "B.MP3", "disco.cue", "a.lrc", "a.a2.lrc", "x.ape"] {
            assert!(interessa(sì), "{sì}");
        }
        for no in [
            "foto.jpg",
            "nota.txt",
            "video.mp4",
            "senza",
            "cartella.flac/",
        ] {
            assert!(!interessa(no), "{no}");
        }
    }

    #[test]
    fn le_cartelle_virtuali_non_si_confondono_con_quelle_dei_servizi() {
        assert!(e_virtuale("~condivisi"));
        for vera in [
            "D3E1F2A3B4C5!1234",
            "1aBcD-eF_gh2IjKlMnOpQrStUvWxYz012",
            "0AKl9xyzPVA",
            "root",
            "/musica/rock",
            "/remote.php/dav/files/m/Musica/",
            "",
        ] {
            assert!(!e_virtuale(vera), "{vera}");
        }
    }

    #[test]
    fn il_primo_tiene_il_nome_e_gli_altri_prendono_una_sigla() {
        let presi: HashSet<String> = HashSet::from(["01.mp3".to_owned()]);
        let occupato = |s: &str| presi.contains(&s.to_lowercase());
        assert_eq!(segmento_libero("02.mp3", "x", &occupato), "02.mp3");
        assert_eq!(
            segmento_libero("01.mp3", "1AbCdEfGhI", &occupato),
            "01~dEfGhI.mp3"
        );
        assert_eq!(
            segmento_libero("01.MP3", "D3E1F2A3B4C5!1234", &occupato),
            "01~C51234.MP3"
        );
        // Anche la sigla presa: si conta.
        let presi2: HashSet<String> =
            HashSet::from(["01.mp3".to_owned(), "01~abcdef.mp3".to_owned()]);
        let occupato2 = |s: &str| presi2.contains(&s.to_lowercase());
        assert_eq!(
            segmento_libero("01.mp3", "abcdef", &occupato2),
            "01~abcdef-2.mp3"
        );
        // Senza estensione, e con un carattere da codificare.
        let presi3: HashSet<String> = HashSet::from(["a%7cb".to_owned()]);
        let occupato3 = |s: &str| presi3.contains(&s.to_lowercase());
        assert_eq!(segmento_libero("a|b", "zz9", &occupato3), "a%7Cb~zz9");
    }

    #[test]
    fn un_elenco_interrotto_non_toglie_niente() {
        let specchio = ["a", "b", "c"];
        let elencati: HashSet<&str> = HashSet::from(["a"]);
        assert!(da_togliere(specchio, &elencati, false).is_empty());
        assert_eq!(da_togliere(specchio, &elencati, true), ["b", "c"]);
    }
}
