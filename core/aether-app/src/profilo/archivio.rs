//! Il contenitore `.aeprofile`: come si scrive, come si rilegge, e perché è uno
//! zip invece di un JSON più grande.
//!
//! # Perché un file nuovo e non il `.json` di prima
//!
//! Il profilo v1 è un JSON di diciassette righe: si apre con un editor, si
//! legge, si capisce. Era una proprietà, non un caso. Da questa release il
//! profilo porta anche la cronologia, i testi, le copertine e i pacchetti skin
//! — cioè centinaia di megabyte di roba binaria — e un `.json` che da domani
//! contiene trecento megabyte di archivio tradisce il proprio nome: chi lo apre
//! con un editor trova byte, non righe.
//!
//! Quindi estensione propria, `.aeprofile`, e il v1 resta **leggibile** e non si
//! scrive più. Il riconoscimento non passa dall'estensione — che chiunque può
//! cambiare — ma dal primo byte: `PK\x03\x04` è un archivio, `{` è il v1.
//!
//! # La lista chiusa dei nomi
//!
//! Identica per forma e per ragione a quella di [`aether_skin::package`]: un
//! archivio arriva da fuori, e l'unica difesa che regge contro un percorso come
//! `../../.ssh/authorized_keys` è **non avere un percorso da normalizzare**. Un
//! nome che non sta nella lista non si legge; non lo si salta, si rifiuta
//! l'archivio, perché una voce inattesa in un formato a lista chiusa non è
//! sciatteria di chi l'ha scritto, è qualcuno che sta provando qualcosa.
//!
//! # Cosa si comprime e cosa no
//!
//! I due JSON in `deflate`: sono testo, e comprimono di dieci volte. Tutto il
//! resto `Stored`. `.gz`, `.jpg` e `.aeskin` sono **già** compressi, e
//! ricomprimerli paga CPU per zero byte guadagnati — è la stessa decisione, con
//! la stessa motivazione, che `aether_skin::package::write_skin_package`
//! prende per le proprie risorse.
//!
//! # Streaming, e non per eleganza
//!
//! Un profilo con le copertine di una libreria vera pesa centinaia di megabyte.
//! Il vincolo di questa release è che la memoria a riposo non peggiori, e
//! l'unico modo di rispettarlo è non tenere mai l'archivio in memoria: qui si
//! scrive da un file al file, con il buffer di un `BufReader`, e si legge una
//! voce alla volta direttamente sul disco. In questo modulo non esiste un
//! `Vec<u8>` che contenga più di una voce, e le voci che diventano `Vec` sono
//! solo quelle piccole e limitate — i due JSON e i pacchetti skin, che hanno
//! già un tetto di venti megabyte imposto da chi li valida.

use std::io::{BufReader, BufWriter, Read as _, Write as _};
use std::path::{Path, PathBuf};

use aether_domain::errors::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// Come si riconosce un profilo, in tutte e due le versioni del formato.
pub const FIRMA: &str = "aether.profilo";

/// La versione del formato che questa build scrive.
pub const VERSIONE: u32 = 2;

/// L'estensione del file.
pub const ESTENSIONE: &str = "aeprofile";

/// Il manifesto.
pub const NOME_MANIFESTO: &str = "manifesto.json";

/// Le preferenze: il documento v1, intatto.
pub const NOME_PREFERENZE: &str = "preferenze.json";

/// Quel che il documento di sincronia non copre.
pub const NOME_BIBLIOTECA: &str = "biblioteca.v1.json";

/// La cartella dei documenti di sincronia dentro l'archivio.
const CARTELLA_SINCRONIA: &str = "sincronia/";

/// La cartella delle copertine.
const CARTELLA_COPERTINE: &str = "copertine/";

/// La cartella delle skin.
const CARTELLA_SKIN: &str = "skin/";

/// La cartella delle bozze dello Studio.
const CARTELLA_BOZZE: &str = "skin/bozze/";

/// I limiti dell'archivio non fidato.
///
/// Le tre difese sono quelle di sempre — traversal, bomba di decompressione,
/// tipo mentito — e i numeri sono quelli di un profilo vero, non di una skin:
/// una libreria da ventimila brani ha ventimila copertine, quindi il tetto
/// delle voci sta in centinaia di migliaia e non in decine.
pub mod limiti {
    /// L'archivio, come sta sul disco.
    ///
    /// Due gibibyte: molto più grande di qualunque profilo vero — le copertine
    /// di ventimila brani stanno in trecento megabyte — e abbastanza piccolo da
    /// non poter riempire un disco per sbaglio.
    pub const MAX_ARCHIVIO: u64 = 2 * 1024 * 1024 * 1024;

    /// Quante voci.
    ///
    /// Duecentomila: una copertina piena più la sua miniatura per ognuna di
    /// centomila copertine distinte, che è già il doppio di una libreria molto
    /// grande.
    pub const MAX_VOCI: usize = 200_000;

    /// Una singola voce, una volta aperta.
    ///
    /// Sessantaquattro mebibyte. Una copertina sta in duecento kilobyte, un
    /// `.aeskin` in venti megabyte per suo proprio limite: il tetto qui serve a
    /// fermare la voce costruita apposta, non a stringere quelle vere.
    pub const MAX_VOCE: u64 = 64 * 1024 * 1024;

    /// La somma di tutto ciò che si decomprime, letta **dall'indice**.
    ///
    /// È il controllo che manca a chi guarda solo le voci una per una: cento
    /// voci ciascuna sotto il proprio tetto fanno lo stesso una montagna, e la
    /// somma è l'unico posto in cui quella montagna si vede prima di averla
    /// scritta sul disco.
    pub const MAX_TOTALE: u64 = 4 * 1024 * 1024 * 1024;

    /// Rapporto massimo fra dichiarato e compresso, per voce.
    ///
    /// Duecento: il testo comprime bene — un JSON arriva a quindici volte — e
    /// una soglia bassa rifiuterebbe contenuto legittimo. Le bombe stanno negli
    /// ordini di mille volte e oltre.
    pub const MAX_RAPPORTO: f64 = 200.0;

    /// Oltre questo peso l'esportazione lo dice invece di scriverlo in silenzio.
    ///
    /// Mezzo gigabyte è il punto in cui un file smette di essere una cosa che si
    /// mette su una chiavetta senza pensarci.
    pub const AVVISO: u64 = 500 * 1024 * 1024;
}

/// Che cos'è una voce dell'archivio.
///
/// Lista chiusa: un nome che non produce una di queste non si legge, e non si
/// salta nemmeno — l'archivio si rifiuta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Voce {
    /// Il manifesto.
    Manifesto,
    /// Le preferenze, cioè il documento v1 intatto.
    Preferenze,
    /// Quel che la sincronia non copre.
    Biblioteca,
    /// Il documento di sincronia di un dispositivo, col suo identificativo.
    Sincronia(String),
    /// Una copertina, con la sua impronta. `miniatura` distingue le due forme.
    Copertina {
        /// L'impronta dei byte originali: è il nome del file nello store.
        hash: String,
        /// È la versione ridotta (`.t.jpg`).
        miniatura: bool,
    },
    /// Un pacchetto skin, col suo identificativo.
    Skin(String),
    /// Una bozza dello Studio, col suo identificativo.
    Bozza(String),
}

/// Un identificativo che diventa un nome di file non deve poter uscire dalla
/// cartella.
///
/// Volutamente la stessa regola di `aether_cloud::pacchetti::id_sicuro` e di
/// `skin::id_sicuro`: `..`, le barre e i due punti di un percorso di Windows
/// sono tutto ciò che serve a scrivere altrove, e qui l'identificativo arriva da
/// un file che qualcuno ha passato all'utente.
#[must_use]
pub fn id_sicuro(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Un'impronta è esadecimale minuscola e nient'altro.
fn impronta_sicura(hash: &str) -> bool {
    hash.len() >= 2 && hash.len() <= 128 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Il nome dentro l'archivio del documento di sincronia di un dispositivo.
///
/// Lo stesso nome che il documento ha nel deposito condiviso, e non è
/// decorazione: i byte sono gli stessi che `aether_sync::documento::serializza`
/// produce, quindi il file dentro l'archivio è **quel** file, non una sua
/// traduzione. Chi apre lo zip con un gestore di archivi e ne estrae uno lo può
/// mettere in una cartella di sincronia e funziona.
#[must_use]
pub fn nome_sincronia(dispositivo: &str) -> String {
    format!(
        "{CARTELLA_SINCRONIA}{}",
        aether_sync::documento::nome_file(dispositivo)
    )
}

/// Il nome dentro l'archivio di una copertina.
///
/// Le sottocartelle per i primi due caratteri sono quelle di
/// [`crate::covers::CoverStore`], e per lo stesso motivo: una cartella con
/// ventimila voci rallenta ogni lettura, e su qualche filesystem molto più che
/// linearmente. Qui c'è la ragione in più che l'archivio si estrae **dentro**
/// quello store, quindi il nome nell'archivio è già il percorso di destinazione.
#[must_use]
pub fn nome_copertina(hash: &str, miniatura: bool) -> String {
    let prefisso = hash.get(..2).unwrap_or("__");
    let coda = if miniatura { "t.jpg" } else { "jpg" };
    format!("{CARTELLA_COPERTINE}{prefisso}/{hash}.{coda}")
}

/// Il nome dentro l'archivio di un pacchetto skin.
#[must_use]
pub fn nome_skin(id: &str) -> String {
    format!("{CARTELLA_SKIN}{id}.aeskin")
}

/// Il nome dentro l'archivio di una bozza dello Studio.
#[must_use]
pub fn nome_bozza(id: &str) -> String {
    format!("{CARTELLA_BOZZE}{id}.json")
}

/// Il nome è ammesso? Lista chiusa: non c'è nulla da normalizzare.
fn classifica(nome: &str) -> Option<Voce> {
    match nome {
        NOME_MANIFESTO => return Some(Voce::Manifesto),
        NOME_PREFERENZE => return Some(Voce::Preferenze),
        NOME_BIBLIOTECA => return Some(Voce::Biblioteca),
        _ => {}
    }

    // Le bozze **prima** delle skin: `skin/bozze/…` comincia per `skin/`, e
    // provare le skin per prime le farebbe cadere sul controllo del nome.
    if let Some(foglia) = nome.strip_prefix(CARTELLA_BOZZE) {
        let id = foglia.strip_suffix(".json")?;
        return id_sicuro(id).then(|| Voce::Bozza(id.to_owned()));
    }
    if let Some(foglia) = nome.strip_prefix(CARTELLA_SKIN) {
        let id = foglia.strip_suffix(".aeskin")?;
        return id_sicuro(id).then(|| Voce::Skin(id.to_owned()));
    }
    if let Some(foglia) = nome.strip_prefix(CARTELLA_SINCRONIA) {
        let id = aether_sync::documento::dispositivo_da_nome(foglia)?;
        return id_sicuro(id).then(|| Voce::Sincronia(id.to_owned()));
    }
    if let Some(foglia) = nome.strip_prefix(CARTELLA_COPERTINE) {
        let (prefisso, file) = foglia.split_once('/')?;
        let (hash, miniatura) = match file.strip_suffix(".t.jpg") {
            Some(hash) => (hash, true),
            None => (file.strip_suffix(".jpg")?, false),
        };
        // Il prefisso deve **discendere** dall'impronta, non essere un secondo
        // dato: senza questo confronto `copertine/aa/bb….jpg` sarebbe un nome
        // ammesso che si estrae dove non appartiene.
        if !impronta_sicura(hash) || hash.get(..2) != Some(prefisso) {
            return None;
        }
        return Some(Voce::Copertina {
            hash: hash.to_owned(),
            miniatura,
        });
    }
    None
}

/// Cosa dichiara di essere un archivio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifesto {
    /// La firma. Sempre [`FIRMA`].
    pub aether: String,
    /// La versione del formato.
    pub versione: u32,
    /// Quando è stato scritto, in millisecondi.
    pub creato_ms: i64,
    /// L'identità della libreria da cui viene.
    ///
    /// Serve a una domanda sola: gli identificativi di brano qua dentro parlano
    /// della stessa raccolta di musica di questa macchina? Se no, la parte di
    /// libreria non si applica — vedi il `//!` di [`crate::profilo`].
    pub identita: String,
    /// Il dispositivo che l'ha scritto.
    pub dispositivo: String,
    /// La versione di Aether che l'ha scritto, per la diagnosi.
    pub applicazione: String,
    /// Su quale chiave i brani si riattaccano.
    ///
    /// Scritto e non sottinteso: dal 2.3.1 in `tracks` ci sono due chiavi —
    /// `track_key`, che segue i valori corretti, e `content_key`, che segue i
    /// tag grezzi — e un archivio che non dicesse quale ha usato sarebbe
    /// interpretabile in due modi che danno risultati diversi.
    pub abbinamento: String,
    /// Quante cose porta.
    pub contiene: Contiene,
    /// Cosa ha lasciato fuori, e non è la stessa cosa di quel che non ha.
    ///
    /// Un elenco che si legge aprendo l'archivio: un profilo che *tace* le
    /// proprie omissioni ha lo stesso difetto di un elenco di esclusioni,
    /// spostato di un passo.
    #[serde(default)]
    pub fuori: Vec<String>,
}

/// Quante cose porta un archivio.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contiene {
    /// Quante preferenze.
    pub preferenze: usize,
    /// C'è il documento di sincronia.
    pub sincronia: bool,
    /// C'è la parte di libreria che il documento non copre.
    pub biblioteca: bool,
    /// Quante copertine distinte.
    pub copertine: usize,
    /// Quanti pacchetti skin.
    pub skin: usize,
    /// Quante bozze dello Studio.
    pub bozze: usize,
}

/// L'archivio non è leggibile, o dichiara qualcosa che non si accetta.
fn rifiutato(causa: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::SettingsCorrupt {
        quarantined_as: None,
    })
    .with_cause(causa.into())
}

/// Una scrittura fallita, nominando il file.
fn scrittura(percorso: &Path, err: &std::io::Error) -> AppError {
    AppError::new(ErrorCode::FsWriteFailed {
        path: percorso.display().to_string(),
        detail: Some(err.kind().to_string()),
    })
    .with_cause(err.to_string())
}

/// Una lettura fallita, nominando il file.
fn lettura(percorso: &Path, err: &std::io::Error) -> AppError {
    AppError::new(ErrorCode::FsReadFailed {
        path: percorso.display().to_string(),
        detail: Some(err.kind().to_string()),
    })
    .with_cause(err.to_string())
}

// ── scrivere ────────────────────────────────────────────────────────────────

/// Scrive un archivio, una voce alla volta, direttamente sul disco.
///
/// Non tiene niente in memoria oltre alla voce che sta scrivendo, e per le voci
/// che vengono da un file nemmeno quella: [`Scrittore::aggiungi_file`] copia con
/// il buffer di un `BufReader`.
pub struct Scrittore {
    zip: ZipWriter<BufWriter<std::fs::File>>,
    percorso: PathBuf,
}

impl Scrittore {
    /// Apre un archivio nuovo, sovrascrivendo quel che c'era.
    ///
    /// # Errori
    ///
    /// `fs.writeFailed` se il file non si crea.
    pub fn crea(destinazione: &Path) -> Result<Self, AppError> {
        if let Some(cartella) = destinazione.parent() {
            std::fs::create_dir_all(cartella).map_err(|err| scrittura(cartella, &err))?;
        }
        let file =
            std::fs::File::create(destinazione).map_err(|err| scrittura(destinazione, &err))?;
        Ok(Self {
            zip: ZipWriter::new(BufWriter::new(file)),
            percorso: destinazione.to_owned(),
        })
    }

    /// Le opzioni per il testo: `deflate`, perché comprime di dieci volte.
    fn compresso() -> SimpleFileOptions {
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
    }

    /// Le opzioni per ciò che è già compresso: `Stored`.
    fn intatto() -> SimpleFileOptions {
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
    }

    /// Aggiunge del testo, compresso.
    ///
    /// # Errori
    ///
    /// `fs.writeFailed` se la scrittura fallisce.
    pub fn aggiungi_testo(&mut self, nome: &str, testo: &str) -> Result<(), AppError> {
        self.zip
            .start_file(nome, Self::compresso())
            .and_then(|()| self.zip.write_all(testo.as_bytes()).map_err(Into::into))
            .map_err(|err| {
                AppError::new(ErrorCode::FsWriteFailed {
                    path: self.percorso.display().to_string(),
                    detail: Some(format!("«{nome}»")),
                })
                .with_cause(err.to_string())
            })
    }

    /// Aggiunge byte già compressi, senza ricomprimerli.
    ///
    /// # Errori
    ///
    /// `fs.writeFailed` se la scrittura fallisce.
    pub fn aggiungi_byte(&mut self, nome: &str, byte: &[u8]) -> Result<(), AppError> {
        self.zip
            .start_file(nome, Self::intatto())
            .and_then(|()| self.zip.write_all(byte).map_err(Into::into))
            .map_err(|err| {
                AppError::new(ErrorCode::FsWriteFailed {
                    path: self.percorso.display().to_string(),
                    detail: Some(format!("«{nome}»")),
                })
                .with_cause(err.to_string())
            })
    }

    /// Copia un file dentro l'archivio, senza mai tenerlo tutto in memoria.
    ///
    /// Restituisce `false` se il file non c'era: una copertina sparita dallo
    /// store non è un guasto dell'esportazione — è una riga di `cover_art` che
    /// ha perso il suo file, cosa che capita quando qualcuno svuota la cache a
    /// mano — e far fallire l'intero profilo per quello sarebbe sproporzionato.
    ///
    /// # Errori
    ///
    /// `fs.writeFailed` se la scrittura nell'archivio fallisce.
    pub fn aggiungi_file(&mut self, nome: &str, sorgente: &Path) -> Result<bool, AppError> {
        let Ok(file) = std::fs::File::open(sorgente) else {
            return Ok(false);
        };
        self.zip.start_file(nome, Self::intatto()).map_err(|err| {
            AppError::new(ErrorCode::FsWriteFailed {
                path: self.percorso.display().to_string(),
                detail: Some(format!("«{nome}»")),
            })
            .with_cause(err.to_string())
        })?;
        // `copy` su un `BufReader` lavora con il suo buffer e non alloca in
        // proporzione al file: è la riga che tiene la promessa dello streaming.
        std::io::copy(&mut BufReader::new(file), &mut self.zip)
            .map_err(|err| scrittura(sorgente, &err))?;
        Ok(true)
    }

    /// Chiude l'archivio e dice quanto pesa.
    ///
    /// # Errori
    ///
    /// `fs.writeFailed` se la chiusura o lo svuotamento del buffer falliscono.
    pub fn chiudi(self) -> Result<u64, AppError> {
        let percorso = self.percorso;
        let mut buffer = self
            .zip
            .finish()
            .map_err(|err| rifiutato(format!("l'archivio non si è chiuso: {err}")))?;
        buffer.flush().map_err(|err| scrittura(&percorso, &err))?;
        let file = buffer
            .into_inner()
            .map_err(|err| scrittura(&percorso, err.error()))?;
        // `sync_all` prima di dire «fatto»: il profilo finisce quasi sempre su
        // una chiavetta, e una chiavetta si stacca appena il programma dice che
        // ha finito. Senza questa riga «ha finito» vuol dire «è nella cache del
        // sistema», e la differenza fra le due la scopre chi ritrova un file
        // troncato sull'altro computer.
        file.sync_all().map_err(|err| scrittura(&percorso, &err))?;
        file.metadata()
            .map(|meta| meta.len())
            .map_err(|err| lettura(&percorso, &err))
    }
}

// ── leggere ─────────────────────────────────────────────────────────────────

/// Una voce ammessa, come l'indice la dichiara.
#[derive(Debug, Clone)]
pub struct Ammessa {
    /// L'indice nell'archivio.
    indice: usize,
    /// Il nome, per i messaggi.
    pub nome: String,
    /// Che cos'è.
    pub voce: Voce,
    /// Quanto dichiara di pesare una volta aperta.
    pub byte: u64,
}

/// Legge un archivio, una voce alla volta.
///
/// `Debug` non stampa l'archivio: mostra il manifesto e quante voci ci sono,
/// che è quel che serve a una prova o a una riga di diario. Il `ZipArchive`
/// tiene un file aperto e un indice di duecentomila nomi, e nessuno dei due si
/// legge in un messaggio d'errore.
pub struct Lettore {
    zip: ZipArchive<BufReader<std::fs::File>>,
    /// Le voci ammesse, nell'ordine dell'indice.
    pub voci: Vec<Ammessa>,
    /// Il manifesto, letto all'apertura.
    pub manifesto: Manifesto,
}

impl std::fmt::Debug for Lettore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lettore")
            .field("manifesto", &self.manifesto)
            .field("voci", &self.voci.len())
            .finish()
    }
}

/// Il primo byte dice cos'è: un archivio, o il JSON del v1.
///
/// Non l'estensione, che chiunque può cambiare, e nemmeno un tentativo di
/// interpretarlo come JSON — che su trecento megabyte di zip vorrebbe dire
/// leggerli tutti per scoprire che non lo sono.
///
/// # Errori
///
/// `fs.readFailed` se il file non si apre.
pub fn e_un_archivio(percorso: &Path) -> Result<bool, AppError> {
    let mut file = std::fs::File::open(percorso).map_err(|err| lettura(percorso, &err))?;
    let mut inizio = [0_u8; 4];
    let letti = file
        .read(&mut inizio)
        .map_err(|err| lettura(percorso, &err))?;
    Ok(letti == 4 && inizio == *b"PK\x03\x04")
}

impl Lettore {
    /// Apre un archivio, controllando l'indice **prima** di decomprimere niente.
    ///
    /// L'ordine dei controlli è deliberato, ed è quello di
    /// `aether_skin::package::read_skin_package`: prima la dimensione del file,
    /// poi i nomi e le dimensioni dichiarate nell'indice, e solo dopo la
    /// decompressione. Un controllo fatto dopo aver decompresso non protegge da
    /// niente, perché la memoria è già stata chiesta.
    ///
    /// # Errori
    ///
    /// `settings.corrupt` se il file non si legge come archivio, se una voce non
    /// è ammessa, se l'indice sfora un limite, o se il manifesto non è un
    /// manifesto di profilo. `fs.readFailed` se il file non si apre.
    pub fn apri(percorso: &Path) -> Result<Self, AppError> {
        let file = std::fs::File::open(percorso).map_err(|err| lettura(percorso, &err))?;
        let peso = file
            .metadata()
            .map(|meta| meta.len())
            .map_err(|err| lettura(percorso, &err))?;
        if peso > limiti::MAX_ARCHIVIO {
            return Err(rifiutato(format!(
                "l'archivio pesa {peso} byte, oltre il limite di {}",
                limiti::MAX_ARCHIVIO
            )));
        }

        let mut zip = ZipArchive::new(BufReader::new(file))
            .map_err(|err| rifiutato(format!("archivio non leggibile: {err}")))?;
        let voci = indice(&mut zip)?;

        let indice_manifesto = voci
            .iter()
            .find(|ammessa| ammessa.voce == Voce::Manifesto)
            .ok_or_else(|| rifiutato(format!("manca {NOME_MANIFESTO}")))?
            .clone();
        let crudo = leggi_voce(&mut zip, &indice_manifesto)?;
        let manifesto: Manifesto = serde_json::from_slice(&crudo)
            .map_err(|err| rifiutato(format!("il manifesto non si legge: {err}")))?;

        if manifesto.aether != FIRMA {
            return Err(rifiutato(format!(
                "il file dice di essere «{}», non un profilo di Aether",
                manifesto.aether
            )));
        }
        if manifesto.versione > VERSIONE {
            return Err(rifiutato(format!(
                "profilo di formato {} scritto da una versione più recente: questa legge fino al {VERSIONE}",
                manifesto.versione
            )));
        }

        Ok(Self {
            zip,
            voci,
            manifesto,
        })
    }

    /// Le voci di un certo tipo, con il loro nome.
    pub fn voci_di(&self, filtro: impl Fn(&Voce) -> bool) -> Vec<Ammessa> {
        self.voci
            .iter()
            .filter(|ammessa| filtro(&ammessa.voce))
            .cloned()
            .collect()
    }

    /// La prima voce di un certo tipo, se c'è.
    pub fn voce_di(&self, filtro: impl Fn(&Voce) -> bool) -> Option<Ammessa> {
        self.voci
            .iter()
            .find(|ammessa| filtro(&ammessa.voce))
            .cloned()
    }

    /// I byte di una voce, con il tetto di [`limiti::MAX_VOCE`].
    ///
    /// Per le voci piccole e limitate — i due JSON, il documento di sincronia, un
    /// pacchetto skin. Le copertine non passano di qui: si estraggono con
    /// [`Lettore::estrai`], che non le tiene mai in memoria.
    ///
    /// # Errori
    ///
    /// `settings.corrupt` se la voce non si decomprime o è più grande di quanto
    /// l'indice dichiarasse.
    pub fn byte(&mut self, ammessa: &Ammessa) -> Result<Vec<u8>, AppError> {
        leggi_voce(&mut self.zip, ammessa)
    }

    /// Estrae una voce su un file, **solo se quel file non c'è già**.
    ///
    /// Restituisce `false` se c'era. È la regola del ripristino di
    /// `aether_cloud::pacchetti`, e qui vale con una ragione in più: le
    /// copertine sono indirizzate dal contenuto, quindi un file già presente con
    /// quel nome ha per costruzione gli stessi byte, e riscriverlo sarebbe una
    /// scrittura su disco per non cambiare niente.
    ///
    /// # Errori
    ///
    /// `fs.writeFailed` se la scrittura fallisce, `settings.corrupt` se la voce
    /// è più grande di quanto l'indice dichiarasse.
    pub fn estrai(&mut self, ammessa: &Ammessa, destinazione: &Path) -> Result<bool, AppError> {
        if destinazione.exists() {
            return Ok(false);
        }
        if let Some(cartella) = destinazione.parent() {
            std::fs::create_dir_all(cartella).map_err(|err| scrittura(cartella, &err))?;
        }
        let mut voce = self
            .zip
            .by_index(ammessa.indice)
            .map_err(|err| rifiutato(format!("«{}» illeggibile: {err}", ammessa.nome)))?;

        // Si scrive prima su un nome provvisorio e poi si rinomina: senza,
        // un'interruzione a metà lascerebbe nello store una copertina troncata
        // con il nome giusto, e siccome le copertine si scrivono «solo se
        // mancano» quella troncata non verrebbe mai più sostituita.
        let provvisorio = destinazione.with_extension("parziale");
        let scritto = {
            let file =
                std::fs::File::create(&provvisorio).map_err(|err| scrittura(&provvisorio, &err))?;
            let mut uscita = BufWriter::new(file);
            let scritto = std::io::copy(
                &mut voce.by_ref().take(limiti::MAX_VOCE.saturating_add(1)),
                &mut uscita,
            )
            .map_err(|err| scrittura(&provvisorio, &err))?;
            uscita
                .flush()
                .map_err(|err| scrittura(&provvisorio, &err))?;
            scritto
        };
        if scritto > limiti::MAX_VOCE {
            let _ = std::fs::remove_file(&provvisorio);
            return Err(rifiutato(format!(
                "«{}» è più grande di quanto l'indice dichiarasse",
                ammessa.nome
            )));
        }
        std::fs::rename(&provvisorio, destinazione).map_err(|err| scrittura(destinazione, &err))?;
        Ok(true)
    }
}

/// L'indice: i nomi e le dimensioni dichiarate, senza aprire niente.
///
/// È il punto in cui una bomba si ferma. Dopo, la memoria è già stata chiesta.
fn indice(zip: &mut ZipArchive<BufReader<std::fs::File>>) -> Result<Vec<Ammessa>, AppError> {
    if zip.len() > limiti::MAX_VOCI {
        return Err(rifiutato(format!(
            "l'archivio ha {} voci, oltre il limite di {}",
            zip.len(),
            limiti::MAX_VOCI
        )));
    }

    let mut ammesse = Vec::new();
    let mut totale: u64 = 0;
    for indice in 0..zip.len() {
        // `by_index_raw` legge dall'indice senza preparare alcun decompressore.
        let voce_zip = zip
            .by_index_raw(indice)
            .map_err(|err| rifiutato(format!("voce {indice} illeggibile: {err}")))?;
        let nome = voce_zip.name().to_owned();
        let dichiarata = voce_zip.size();
        let compressa = voce_zip.compressed_size();
        drop(voce_zip);

        if nome.ends_with('/') {
            // Le cartelle non servono: la struttura è fissa e i nomi la
            // portano già dentro.
            continue;
        }

        let Some(voce) = classifica(&nome) else {
            return Err(rifiutato(format!(
                "«{nome}» non è un nome che questo formato ammette"
            )));
        };

        if dichiarata > limiti::MAX_VOCE {
            return Err(rifiutato(format!(
                "«{nome}» dichiara {dichiarata} byte, oltre il limite di {}",
                limiti::MAX_VOCE
            )));
        }
        if compressa > 0 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "il rapporto serve a confrontarlo con una soglia e a stamparlo senza \
                          decimali: i bit che un u64 enorme perderebbe in fondo non spostano né \
                          l'una né l'altro"
            )]
            let rapporto = dichiarata as f64 / compressa as f64;
            if rapporto > limiti::MAX_RAPPORTO {
                return Err(rifiutato(format!(
                    "«{nome}» ha un rapporto di compressione {rapporto:.0}×, sospetto"
                )));
            }
        }
        totale = totale.saturating_add(dichiarata);
        if totale > limiti::MAX_TOTALE {
            return Err(rifiutato(
                "quel che l'archivio dichiara di contenere supera il limite totale".to_owned(),
            ));
        }

        ammesse.push(Ammessa {
            indice,
            nome,
            voce,
            byte: dichiarata,
        });
    }
    Ok(ammesse)
}

/// I byte di una voce, con il tetto applicato **anche** in lettura.
///
/// Non è una ripetizione dell'indice: la dimensione lì è dichiarata da chi ha
/// costruito l'archivio, quindi può mentire. Questa è la quantità di byte che si
/// accetta davvero.
fn leggi_voce(
    zip: &mut ZipArchive<BufReader<std::fs::File>>,
    ammessa: &Ammessa,
) -> Result<Vec<u8>, AppError> {
    let mut voce = zip
        .by_index(ammessa.indice)
        .map_err(|err| rifiutato(format!("«{}» illeggibile: {err}", ammessa.nome)))?;
    let mut byte = Vec::new();
    voce.by_ref()
        .take(limiti::MAX_VOCE.saturating_add(1))
        .read_to_end(&mut byte)
        .map_err(|err| rifiutato(format!("«{}» illeggibile: {err}", ammessa.nome)))?;
    if byte.len() as u64 > limiti::MAX_VOCE {
        return Err(rifiutato(format!(
            "«{}» è più grande di quanto l'indice dichiarasse",
            ammessa.nome
        )));
    }
    Ok(byte)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_nomi_ammessi_sono_quelli_del_formato_e_nessun_altro() {
        assert_eq!(classifica(NOME_MANIFESTO), Some(Voce::Manifesto));
        assert_eq!(classifica(NOME_PREFERENZE), Some(Voce::Preferenze));
        assert_eq!(classifica(NOME_BIBLIOTECA), Some(Voce::Biblioteca));
        assert_eq!(
            classifica("sincronia/aether-portatile.v1.json.gz"),
            Some(Voce::Sincronia("portatile".to_owned()))
        );
        assert_eq!(
            classifica("skin/notte.aeskin"),
            Some(Voce::Skin("notte".to_owned()))
        );
        assert_eq!(
            classifica("skin/bozze/prova.json"),
            Some(Voce::Bozza("prova".to_owned()))
        );
        assert_eq!(
            classifica("copertine/ab/abcdef.jpg"),
            Some(Voce::Copertina {
                hash: "abcdef".to_owned(),
                miniatura: false
            })
        );
        assert_eq!(
            classifica("copertine/ab/abcdef.t.jpg"),
            Some(Voce::Copertina {
                hash: "abcdef".to_owned(),
                miniatura: true
            })
        );
    }

    #[test]
    fn un_nome_che_esce_dalla_cartella_non_e_un_nome() {
        // Non c'è nessun percorso da normalizzare: questi non producono una
        // voce, quindi non arrivano mai a essere un percorso.
        assert_eq!(classifica("../../fuori.txt"), None);
        assert_eq!(classifica("copertine/../../fuori.jpg"), None);
        assert_eq!(classifica("skin/../fuori.aeskin"), None);
        assert_eq!(classifica("skin/C:\\altrove.aeskin"), None);
        assert_eq!(classifica("qualsiasi.txt"), None);
        // Il prefisso deve discendere dall'impronta, o la copertina si
        // estrarrebbe in una sottocartella che non è la sua.
        assert_eq!(classifica("copertine/zz/abcdef.jpg"), None);
        // E l'impronta è esadecimale: `..` non lo è.
        assert_eq!(classifica("copertine/../nonesa.jpg"), None);
    }

    #[test]
    fn i_nomi_si_scrivono_e_si_rileggono_uguali() {
        // Il giro completo: quel che si scrive deve classificarsi come quel che
        // è. Senza questa prova le due metà del formato potrebbero divergere di
        // un carattere e nessuno se ne accorgerebbe fino al primo profilo vero.
        let hash = "0f1e2d3c";
        assert_eq!(
            classifica(&nome_copertina(hash, false)),
            Some(Voce::Copertina {
                hash: hash.to_owned(),
                miniatura: false
            })
        );
        assert_eq!(
            classifica(&nome_copertina(hash, true)),
            Some(Voce::Copertina {
                hash: hash.to_owned(),
                miniatura: true
            })
        );
        assert_eq!(
            classifica(&nome_skin("sala")),
            Some(Voce::Skin("sala".to_owned()))
        );
        assert_eq!(
            classifica(&nome_bozza("sala")),
            Some(Voce::Bozza("sala".to_owned()))
        );
        assert_eq!(
            classifica(&nome_sincronia("fisso")),
            Some(Voce::Sincronia("fisso".to_owned()))
        );
    }

    #[test]
    fn un_archivio_ostile_non_passa_i_limiti() {
        let dir = tempfile::tempdir().expect("cartella temporanea");

        // 1. Un nome fuori dalla lista chiusa: si rifiuta l'archivio, non si
        //    salta la voce.
        let fuori = dir.path().join("fuori.aeprofile");
        {
            let mut scrittore = Scrittore::crea(&fuori).expect("archivio");
            scrittore
                .aggiungi_testo("../../fuori.txt", "x")
                .expect("voce");
            scrittore.chiudi().expect("chiusura");
        }
        let err = Lettore::apri(&fuori).expect_err("deve rifiutare");
        assert_eq!(err.code().kind().code(), "settings.corrupt");
        assert!(
            err.cause().is_some_and(|causa| causa.contains("fuori.txt")),
            "e deve dire quale nome ha trovato: {:?}",
            err.cause()
        );

        // 2. Un rapporto di compressione da bomba. Un megabyte di zeri sta in
        //    poco più di un kilobyte: è mille volte, cioè cinque volte oltre la
        //    soglia.
        let bomba = dir.path().join("bomba.aeprofile");
        {
            let mut scrittore = Scrittore::crea(&bomba).expect("archivio");
            scrittore
                .aggiungi_testo(NOME_PREFERENZE, &"0".repeat(1024 * 1024))
                .expect("voce");
            scrittore.chiudi().expect("chiusura");
        }
        let err = Lettore::apri(&bomba).expect_err("deve rifiutare");
        assert!(
            err.cause().is_some_and(|causa| causa.contains("rapporto")),
            "e deve dire che il rapporto è sospetto: {:?}",
            err.cause()
        );

        // 3. Senza manifesto non è un profilo.
        let muto = dir.path().join("muto.aeprofile");
        {
            let mut scrittore = Scrittore::crea(&muto).expect("archivio");
            scrittore
                .aggiungi_byte(NOME_BIBLIOTECA, b"{}")
                .expect("voce");
            scrittore.chiudi().expect("chiusura");
        }
        assert!(Lettore::apri(&muto).is_err());
    }

    #[test]
    fn una_copertina_che_c_e_non_si_riscrive() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let archivio = dir.path().join("p.aeprofile");
        let nome = nome_copertina("ab12", false);
        {
            let mut scrittore = Scrittore::crea(&archivio).expect("archivio");
            scrittore
                .aggiungi_testo(NOME_MANIFESTO, &manifesto_finto())
                .expect("manifesto");
            scrittore
                .aggiungi_byte(&nome, b"quella nuova")
                .expect("jpg");
            scrittore.chiudi().expect("chiusura");
        }

        let dentro = dir.path().join("store").join("ab").join("ab12.jpg");
        std::fs::create_dir_all(dentro.parent().expect("cartella")).expect("cartella");
        std::fs::write(&dentro, b"quella di qui").expect("copertina locale");

        let mut lettore = Lettore::apri(&archivio).expect("lettura");
        let voce = lettore
            .voce_di(|voce| matches!(voce, Voce::Copertina { .. }))
            .expect("la copertina c'è");
        assert_eq!(
            lettore.estrai(&voce, &dentro),
            Ok(false),
            "una copertina che c'è non si riscrive"
        );
        assert_eq!(
            std::fs::read(&dentro).expect("rilettura"),
            b"quella di qui",
            "e quella di qui resta quella di qui"
        );

        let altrove = dir.path().join("store").join("ab").join("altro.jpg");
        assert_eq!(lettore.estrai(&voce, &altrove), Ok(true));
        assert_eq!(std::fs::read(&altrove).expect("rilettura"), b"quella nuova");
    }

    #[test]
    fn il_primo_byte_distingue_l_archivio_dal_json_di_prima() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let vecchio = dir.path().join("profilo.json");
        std::fs::write(&vecchio, r#"{"aether":"aether.profilo"}"#).expect("v1");
        assert_eq!(e_un_archivio(&vecchio), Ok(false));

        let nuovo = dir.path().join("profilo.aeprofile");
        {
            let mut scrittore = Scrittore::crea(&nuovo).expect("archivio");
            scrittore
                .aggiungi_testo(NOME_MANIFESTO, &manifesto_finto())
                .expect("manifesto");
            scrittore.chiudi().expect("chiusura");
        }
        assert_eq!(e_un_archivio(&nuovo), Ok(true));
    }

    #[test]
    fn un_archivio_di_domani_si_rifiuta_nominando_il_numero() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let futuro = dir.path().join("futuro.aeprofile");
        {
            let mut scrittore = Scrittore::crea(&futuro).expect("archivio");
            scrittore
                .aggiungi_testo(
                    NOME_MANIFESTO,
                    &manifesto_finto().replace("\"versione\":2", "\"versione\":99"),
                )
                .expect("manifesto");
            scrittore.chiudi().expect("chiusura");
        }
        let err = Lettore::apri(&futuro).expect_err("deve rifiutare");
        assert!(
            err.cause().is_some_and(|causa| causa.contains("99")),
            "e deve dire quale formato ha trovato: {:?}",
            err.cause()
        );
    }

    /// Un manifesto minimo, scritto a mano perché la prova non dipenda da chi lo
    /// costruisce.
    fn manifesto_finto() -> String {
        r#"{"aether":"aether.profilo","versione":2,"creatoMs":0,"identita":"x",
            "dispositivo":"prova","applicazione":"2.3.1","abbinamento":"track_key",
            "contiene":{"preferenze":0,"sincronia":false,"biblioteca":false,
                        "copertine":0,"skin":0,"bozze":0},"fuori":[]}"#
            .replace([' ', '\n'], "")
    }
}
