//! Il contenitore `.ryprofile`: come si scrive, come si rilegge, e perché è uno
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
//! Quindi estensione propria, `.ryprofile`, e il v1 resta **leggibile** e non si
//! scrive più. Il riconoscimento non passa dall'estensione — che chiunque può
//! cambiare — ma dal primo byte: `PK\x03\x04` è un archivio, `{` è il v1.
//!
//! # La lista chiusa dei nomi
//!
//! Identica per forma e per ragione a quella di [`rymembra_skin::package`]: un
//! archivio arriva da fuori, e l'unica difesa che regge contro un percorso come
//! `../../.ssh/authorized_keys` è **non avere un percorso da normalizzare**. Un
//! nome che non sta nella lista non si legge; non lo si salta, si rifiuta
//! l'archivio, perché una voce inattesa in un formato a lista chiusa non è
//! sciatteria di chi l'ha scritto, è qualcuno che sta provando qualcosa.
//!
//! # Cosa si comprime e cosa no
//!
//! I due JSON in `deflate`: sono testo, e comprimono di dieci volte. Tutto il
//! resto `Stored`. `.gz`, `.jpg` e `.ryskin` sono **già** compressi, e
//! ricomprimerli paga CPU per zero byte guadagnati — è la stessa decisione, con
//! la stessa motivazione, che `rymembra_skin::package::write_skin_package`
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

use rymembra_domain::eredita;
use rymembra_domain::errors::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// Come si riconosce un profilo, in tutte e due le versioni del formato.
///
/// È la firma che si **scrive**. In lettura se ne accetta anche una seconda, la
/// firma dei profili di prima ([`eredita::FIRMA_PROFILO`]): vedi
/// [`firma_riconosciuta`].
pub const FIRMA: &str = "rymembra.profilo";

/// La firma letta da un file è quella di un profilo che sappiamo leggere?
///
/// Due firme e nessun'altra: la nostra e quella dei profili scritti dalla
/// versione di prima, che stanno su chiavette e in cartelle di backup di
/// persone che non li riesporteranno. Una firma diversa da entrambe resta
/// rifiutata: è un JSON o uno zip qualunque, e il suo posto non è qui.
#[must_use]
pub fn firma_riconosciuta(firma: &str) -> bool {
    firma == FIRMA || firma == eredita::FIRMA_PROFILO
}

/// La versione del formato che questa build scrive.
pub const VERSIONE: u32 = 2;

/// L'estensione del file.
pub const ESTENSIONE: &str = "ryprofile";

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
    /// `.ryskin` in venti megabyte per suo proprio limite: il tetto qui serve a
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
/// `..`, le barre e i due punti di un percorso di Windows sono tutto ciò che
/// serve a scrivere altrove, e qui l'identificativo arriva da un file che
/// qualcuno ha passato all'utente. Né un dispositivo di Windows: `skin/nul.ryskin`
/// su Windows 10 scrive nel nulla.
///
/// La regola è [`rymembra_domain::paths::id_di_file`], la stessa delle skin
/// installate e delle bozze dello Studio.
#[must_use]
pub fn id_sicuro(id: &str) -> bool {
    rymembra_domain::paths::id_di_file(id)
}

/// Un'impronta è esadecimale minuscola e nient'altro.
///
/// Il criterio è quello dello store in cui la voce si estrae
/// ([`crate::covers::e_un_impronta`]), e non una copia: prima questa accettava
/// anche le maiuscole, che lo store su NTFS confonde con le minuscole.
fn impronta_sicura(hash: &str) -> bool {
    crate::covers::e_un_impronta(hash)
}

/// Il nome dentro l'archivio del documento di sincronia di un dispositivo.
///
/// Lo stesso nome che il documento ha nel deposito condiviso, e non è
/// decorazione: i byte sono gli stessi che `rymembra_sync::documento::serializza`
/// produce, quindi il file dentro l'archivio è **quel** file, non una sua
/// traduzione. Chi apre lo zip con un gestore di archivi e ne estrae uno lo può
/// mettere in una cartella di sincronia e funziona.
#[must_use]
pub fn nome_sincronia(dispositivo: &str) -> String {
    format!(
        "{CARTELLA_SINCRONIA}{}",
        rymembra_sync::documento::nome_file(dispositivo)
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
    format!("{CARTELLA_SKIN}{id}.ryskin")
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
        // `.ryskin`, e anche l'estensione dei profili di prima: l'identificativo
        // è lo stesso, e l'estrazione scrive sempre col nome di adesso.
        let id = foglia
            .strip_suffix(".ryskin")
            .or_else(|| foglia.strip_suffix(format!(".{}", eredita::ESTENSIONE_SKIN).as_str()))?;
        return id_sicuro(id).then(|| Voce::Skin(id.to_owned()));
    }
    if let Some(foglia) = nome.strip_prefix(CARTELLA_SINCRONIA) {
        let id = rymembra_sync::documento::dispositivo_da_nome(foglia)?;
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
    /// La firma. In scrittura sempre [`FIRMA`]; in lettura anche quella di
    /// prima ([`eredita::FIRMA_PROFILO`]), vedi [`firma_riconosciuta`].
    ///
    /// Il campo, nei profili di prima, si chiamava `aether`: l'alias è uno dei
    /// due soli posti del codice dove quel nome può comparire per esteso.
    #[serde(alias = "aether")]
    pub rymembra: String,
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
    /// La versione di Rymembra che l'ha scritto, per la diagnosi.
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
    /// Le chiavi del catalogo che, quando l'archivio è stato scritto, in
    /// questa libreria **non c'erano**.
    ///
    /// Serve a «Torna com'era prima»: la copia di sicurezza porta le chiavi che
    /// c'erano, e senza questo elenco una preferenza che il profilo ha
    /// aggiunto — la skin, l'equalizzatore su un computer nuovo — restava dopo
    /// l'annullamento. Un elenco esplicito e non «quel che manca dalla copia»:
    /// una copia scritta da una versione con un catalogo più corto non direbbe
    /// niente delle chiavi venute dopo, e dimenticarle sarebbe un guasto. Senza
    /// l'elenco (le copie di prima) non si dimentica niente. Una versione di
    /// prima lo ignora: il manifesto non rifiuta i campi che non conosce.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assenti: Vec<String>,
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
///
/// # Su un file provvisorio, e il profilo di prima resta finché non c'è il nuovo
///
/// Si scrive accanto alla destinazione, in `<destinazione>.parziale`, e solo
/// [`Scrittore::chiudi`] riuscito lo rinomina sopra. Aprire direttamente la
/// destinazione la azzerava subito: riesportare sulla chiavetta il profilo del
/// mese scorso, e vedersi riempire la chiavetta o staccarla a metà, voleva dire
/// aver perso quello vecchio e avere al suo posto un archivio rotto — anzi,
/// uno zip **chiuso** con dentro metà delle voci, perché `ZipWriter` scrive la
/// directory centrale anche quando viene buttato. Se `chiudi` non arriva in
/// fondo, o lo scrittore cade senza `chiudi`, il provvisorio si cancella. Il
/// prezzo è lo spazio di tutti e due finché si scrive: su una chiavetta quasi
/// piena un'esportazione che prima riusciva sovrascrivendo ora fallisce, ma
/// lasciando intatto il file di prima, che è il verso giusto dell'errore. Il
/// nome provvisorio non finisce in [`ESTENSIONE`], così le copie di sicurezza
/// non lo scambiano per una copia.
pub struct Scrittore {
    /// `None` solo dopo [`Scrittore::chiudi`], o mentre `Drop` lo butta.
    zip: Option<ZipWriter<BufWriter<std::fs::File>>>,
    /// Dove finirà: il nome dei messaggi d'errore.
    percorso: PathBuf,
    /// Dove si scrive intanto.
    provvisorio: PathBuf,
    /// Il provvisorio è diventato la destinazione: `Drop` non deve toccarlo.
    rinominato: bool,
}

impl Drop for Scrittore {
    fn drop(&mut self) {
        if self.rinominato {
            return;
        }
        // Prima si chiude il file — su Windows un file aperto non si cancella
        // — e poi si toglie. Best-effort: l'errore da riportare, se c'è, è
        // quello che ha interrotto la scrittura.
        drop(self.zip.take());
        let _ = std::fs::remove_file(&self.provvisorio);
    }
}

impl Scrittore {
    /// Apre un archivio nuovo. Quel che c'era si sostituisce solo a
    /// [`Scrittore::chiudi`] riuscito: vedi la nota sul tipo.
    ///
    /// # Errori
    ///
    /// `fs.writeFailed` se il file non si crea.
    pub fn crea(destinazione: &Path) -> Result<Self, AppError> {
        if let Some(cartella) = destinazione.parent() {
            std::fs::create_dir_all(cartella).map_err(|err| scrittura(cartella, &err))?;
        }
        let provvisorio = {
            let mut nome = destinazione.as_os_str().to_owned();
            nome.push(".parziale");
            PathBuf::from(nome)
        };
        let file =
            std::fs::File::create(&provvisorio).map_err(|err| scrittura(destinazione, &err))?;
        Ok(Self {
            zip: Some(ZipWriter::new(BufWriter::new(file))),
            percorso: destinazione.to_owned(),
            provvisorio,
            rinominato: false,
        })
    }

    /// Lo zip aperto. Manca solo dopo `chiudi`, che consuma lo scrittore:
    /// qui non può succedere, e se succedesse è un guasto di scrittura.
    fn zip(&mut self) -> Result<&mut ZipWriter<BufWriter<std::fs::File>>, AppError> {
        let percorso = self.percorso.display().to_string();
        self.zip.as_mut().ok_or_else(|| {
            AppError::new(ErrorCode::FsWriteFailed {
                path: percorso,
                detail: Some("l'archivio è già chiuso".to_owned()),
            })
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
        let zip = self.zip()?;
        zip.start_file(nome, Self::compresso())
            .and_then(|()| zip.write_all(testo.as_bytes()).map_err(Into::into))
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
        let zip = self.zip()?;
        zip.start_file(nome, Self::intatto())
            .and_then(|()| zip.write_all(byte).map_err(Into::into))
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
        let percorso = self.percorso.display().to_string();
        let zip = self.zip()?;
        zip.start_file(nome, Self::intatto()).map_err(|err| {
            AppError::new(ErrorCode::FsWriteFailed {
                path: percorso,
                detail: Some(format!("«{nome}»")),
            })
            .with_cause(err.to_string())
        })?;
        // `copy` su un `BufReader` lavora con il suo buffer e non alloca in
        // proporzione al file: è la riga che tiene la promessa dello streaming.
        std::io::copy(&mut BufReader::new(file), zip).map_err(|err| scrittura(sorgente, &err))?;
        Ok(true)
    }

    /// Chiude l'archivio e dice quanto pesa.
    ///
    /// # Errori
    ///
    /// `fs.writeFailed` se la chiusura o lo svuotamento del buffer falliscono.
    pub fn chiudi(mut self) -> Result<u64, AppError> {
        let percorso = self.percorso.clone();
        let zip = self.zip.take().ok_or_else(|| {
            AppError::new(ErrorCode::FsWriteFailed {
                path: percorso.display().to_string(),
                detail: Some("l'archivio è già chiuso".to_owned()),
            })
        })?;
        let mut buffer = zip
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
        let byte = file
            .metadata()
            .map(|meta| meta.len())
            .map_err(|err| lettura(&percorso, &err))?;
        // Chiuso prima di rinominare: su Windows un file aperto non si sposta.
        // `rename` sostituisce la destinazione se c'è; se non riesce, `Drop`
        // toglie il provvisorio e il profilo di prima resta com'era.
        drop(file);
        std::fs::rename(&self.provvisorio, &percorso).map_err(|err| scrittura(&percorso, &err))?;
        self.rinominato = true;
        Ok(byte)
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
    /// I byte aperti davvero finora, da tutte le voci lette.
    aperti: u64,
    /// Quanti se ne possono aprire in tutto: [`limiti::MAX_TOTALE`].
    budget: u64,
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
    /// `rymembra_skin::package::read_skin_package`: prima la dimensione del file,
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
        let mut aperti = 0;
        let crudo = leggi_voce(&mut zip, &indice_manifesto, &mut aperti, limiti::MAX_TOTALE)?;
        let manifesto: Manifesto = serde_json::from_slice(&crudo)
            .map_err(|err| rifiutato(format!("il manifesto non si legge: {err}")))?;

        if !firma_riconosciuta(&manifesto.rymembra) {
            return Err(rifiutato(format!(
                "il file dice di essere «{}», non un profilo di Rymembra",
                manifesto.rymembra
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
            aperti,
            budget: limiti::MAX_TOTALE,
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
    /// `settings.corrupt` se la voce non si decomprime, se è più grande di
    /// quanto l'indice dichiarasse, o se supera quel che resta del budget.
    pub fn byte(&mut self, ammessa: &Ammessa) -> Result<Vec<u8>, AppError> {
        leggi_voce(&mut self.zip, ammessa, &mut self.aperti, self.budget)
    }

    /// Estrae una voce su un file, **solo se quel file non c'è già**.
    ///
    /// Restituisce `false` se c'era. È la regola del ripristino di
    /// `rymembra_cloud::pacchetti`, e qui vale con una ragione in più: le
    /// copertine sono indirizzate dal contenuto, quindi un file già presente con
    /// quel nome ha per costruzione gli stessi byte, e riscriverlo sarebbe una
    /// scrittura su disco per non cambiare niente.
    ///
    /// # Errori
    ///
    /// `fs.writeFailed` se la scrittura fallisce, `settings.corrupt` se la voce
    /// non si decomprime (un CRC che non torna), se è più grande di quanto
    /// l'indice dichiarasse, o se supera quel che resta del budget. In ogni
    /// caso d'errore il provvisorio se ne va: nello store non resta niente.
    pub fn estrai(&mut self, ammessa: &Ammessa, destinazione: &Path) -> Result<bool, AppError> {
        if destinazione.exists() {
            return Ok(false);
        }
        if let Some(cartella) = destinazione.parent() {
            std::fs::create_dir_all(cartella).map_err(|err| scrittura(cartella, &err))?;
        }
        let tetto = tetto_della_voce(ammessa, self.aperti, self.budget);
        let mut voce = self
            .zip
            .by_index(ammessa.indice)
            .map_err(|err| rifiutato(format!("«{}» illeggibile: {err}", ammessa.nome)))?;

        // Si scrive prima su un nome provvisorio e poi si rinomina: senza,
        // un'interruzione a metà lascerebbe nello store una copertina troncata
        // con il nome giusto, e siccome le copertine si scrivono «solo se
        // mancano» quella troncata non verrebbe mai più sostituita. Il nome
        // provvisorio lo riconosce anche `CoverStore::spazza_avanzi`, per
        // quando a interrompere è la morte del processo.
        let provvisorio = destinazione.with_extension("parziale");
        let file =
            std::fs::File::create(&provvisorio).map_err(|err| scrittura(&provvisorio, &err))?;
        // Da qui ogni uscita per errore toglie il provvisorio: un'estrazione
        // fallita — un CRC che non torna su un profilo scaricato a metà — lo
        // lasciava nello store per sempre, fino a 64 MiB a voce.
        let butta = |errore: AppError| {
            let _ = std::fs::remove_file(&provvisorio);
            errore
        };
        let mut uscita = BufWriter::new(file);
        // Un byte oltre il tetto: è il modo di sapere che la voce ne ha di più.
        // E si legge fino alla fine quando non ne ha di più, che è il momento
        // in cui il lettore di `zip` controlla il CRC.
        let copiato = std::io::copy(
            &mut voce.by_ref().take(tetto.saturating_add(1)),
            &mut uscita,
        )
        .and_then(|scritto| uscita.flush().map(|()| scritto));
        // Chiuso prima di togliere o rinominare: su Windows un file aperto non
        // si lascia fare né l'una né l'altra cosa.
        drop(uscita);
        let scritto = match copiato {
            Ok(scritto) => scritto,
            // Dati che non tornano — un CRC, un flusso `deflate` rotto — sono
            // l'archivio; tutto il resto è il disco di qui.
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::InvalidData
                        | std::io::ErrorKind::InvalidInput
                        | std::io::ErrorKind::UnexpectedEof
                ) =>
            {
                return Err(butta(rifiutato(format!(
                    "«{}» non si estrae: {err}",
                    ammessa.nome
                ))));
            }
            Err(err) => return Err(butta(scrittura(&provvisorio, &err))),
        };
        self.aperti = self.aperti.saturating_add(scritto);
        if scritto > tetto {
            return Err(butta(oltre_il_tetto(ammessa, tetto)));
        }
        std::fs::rename(&provvisorio, destinazione)
            .map_err(|err| butta(scrittura(destinazione, &err)))?;
        Ok(true)
    }
}

/// Quanti byte aperti si accettano da una voce: quelli che l'indice
/// **dichiara** per lei, e non più di quel che resta del budget.
///
/// La dichiarata e non [`limiti::MAX_VOCE`]: rapporto e totale si controllano
/// sull'indice ([`indice`]), e reggono solo se la dichiarata è vera. Un
/// mebibyte che si dichiara dieci byte passava tutti e due i controlli e poi si
/// scriveva intero. Tenendo ogni voce alla sua dichiarata, i byte aperti in
/// tutto non superano la somma dichiarata — e il budget lo dice comunque, anche
/// per chi legge la stessa voce due volte.
fn tetto_della_voce(ammessa: &Ammessa, aperti: u64, budget: u64) -> u64 {
    ammessa.byte.min(budget.saturating_sub(aperti))
}

/// L'errore di una voce che ha dato più byte del suo tetto.
fn oltre_il_tetto(ammessa: &Ammessa, tetto: u64) -> AppError {
    if tetto < ammessa.byte {
        rifiutato(format!(
            "«{}» supera il budget dei byte che l'archivio può aprire in tutto ({})",
            ammessa.nome,
            limiti::MAX_TOTALE
        ))
    } else {
        rifiutato(format!(
            "«{}» è più grande di quanto l'indice dichiarasse",
            ammessa.nome
        ))
    }
}

/// L'indice: i nomi e le dimensioni dichiarate, senza aprire niente.
///
/// È il punto in cui una bomba si ferma. Dopo, la memoria è già stata chiesta.
/// Regge perché la lettura tiene ogni voce alla sua dichiarata
/// ([`tetto_della_voce`]): senza, rapporto e totale guardavano numeri scritti da
/// chi ha costruito l'archivio, e niente li confrontava coi byte veri.
///
/// Le voci che condividono gli stessi byte compressi
/// (`ZipArchive::has_overlapping_files`) non si cercano: quel controllo è
/// quadratico nelle voci, cioè minuti a duecentomila copertine, e quel che una
/// sovrapposizione moltiplica sono i byte aperti, che hanno già il tetto per
/// voce e il budget del [`Lettore`].
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
/// accetta davvero — la dichiarata, e non più di quel che resta di `budget`
/// (vedi [`tetto_della_voce`]) — e `aperti` sale di quelli aperti davvero.
fn leggi_voce(
    zip: &mut ZipArchive<BufReader<std::fs::File>>,
    ammessa: &Ammessa,
    aperti: &mut u64,
    budget: u64,
) -> Result<Vec<u8>, AppError> {
    let tetto = tetto_della_voce(ammessa, *aperti, budget);
    let mut voce = zip
        .by_index(ammessa.indice)
        .map_err(|err| rifiutato(format!("«{}» illeggibile: {err}", ammessa.nome)))?;
    let mut byte = Vec::new();
    let letto = voce
        .by_ref()
        .take(tetto.saturating_add(1))
        .read_to_end(&mut byte);
    let quanti = u64::try_from(byte.len()).unwrap_or(u64::MAX);
    *aperti = aperti.saturating_add(quanti);
    letto.map_err(|err| rifiutato(format!("«{}» illeggibile: {err}", ammessa.nome)))?;
    if quanti > tetto {
        return Err(oltre_il_tetto(ammessa, tetto));
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
            classifica("sincronia/rymembra-portatile.v1.json.gz"),
            Some(Voce::Sincronia("portatile".to_owned()))
        );
        assert_eq!(
            classifica("skin/notte.ryskin"),
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
    fn un_dispositivo_di_windows_non_e_un_identificativo() {
        // `skin/nul.ryskin`, `skin/bozze/con.json`: su Windows 10 sono il
        // dispositivo, e il ripristino scrive nel nulla o fallisce.
        for dispositivo in ["nul", "con", "COM1", "lpt9", "Aux", "com0"] {
            assert!(!id_sicuro(dispositivo), "«{dispositivo}»");
        }
        assert_eq!(classifica("skin/nul.ryskin"), None);
        assert_eq!(classifica("skin/bozze/con.json"), None);
        assert!(id_sicuro("console"));
    }

    #[test]
    fn un_nome_che_esce_dalla_cartella_non_e_un_nome() {
        // Non c'è nessun percorso da normalizzare: questi non producono una
        // voce, quindi non arrivano mai a essere un percorso.
        assert_eq!(classifica("../../fuori.txt"), None);
        assert_eq!(classifica("copertine/../../fuori.jpg"), None);
        assert_eq!(classifica("skin/../fuori.ryskin"), None);
        assert_eq!(classifica("skin/C:\\altrove.ryskin"), None);
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
        let fuori = dir.path().join("fuori.ryprofile");
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
        let bomba = dir.path().join("bomba.ryprofile");
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
        let muto = dir.path().join("muto.ryprofile");
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
        let archivio = dir.path().join("p.ryprofile");
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

    /// Cosa c'è nella cartella, per nome.
    fn nomi_in(cartella: &Path) -> Vec<String> {
        let mut nomi: Vec<String> = std::fs::read_dir(cartella)
            .expect("cartella")
            .flatten()
            .map(|voce| voce.file_name().to_string_lossy().into_owned())
            .collect();
        nomi.sort();
        nomi
    }

    #[test]
    fn un_esportazione_interrotta_non_tocca_il_profilo_di_prima() {
        // Sulla chiavetta c'è il profilo del mese scorso. Riesportarlo con lo
        // stesso nome lo azzerava subito: se a metà la chiavetta si riempie o
        // si stacca, il profilo vecchio è perso e quello nuovo è uno zip senza
        // directory centrale.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let profilo = dir.path().join("Rymembra.ryprofile");
        std::fs::write(&profilo, b"il profilo di prima").expect("profilo vecchio");
        {
            let mut scrittore = Scrittore::crea(&profilo).expect("archivio");
            scrittore
                .aggiungi_testo(NOME_MANIFESTO, &manifesto_finto())
                .expect("manifesto");
            // E qui si interrompe: nessun `chiudi`.
        }
        assert_eq!(
            std::fs::read(&profilo).expect("rilettura"),
            b"il profilo di prima"
        );
        assert_eq!(
            nomi_in(dir.path()),
            vec!["Rymembra.ryprofile"],
            "e nessun file a metà resta in giro"
        );
    }

    #[test]
    fn un_esportazione_riuscita_sostituisce_il_profilo_di_prima() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let profilo = dir.path().join("Rymembra.ryprofile");
        std::fs::write(&profilo, b"il profilo di prima").expect("profilo vecchio");
        let mut scrittore = Scrittore::crea(&profilo).expect("archivio");
        scrittore
            .aggiungi_testo(NOME_MANIFESTO, &manifesto_finto())
            .expect("manifesto");
        let byte = scrittore.chiudi().expect("chiusura");
        assert_eq!(byte, std::fs::metadata(&profilo).expect("metadati").len());
        assert!(e_un_archivio(&profilo).expect("lettura"));
        assert!(Lettore::apri(&profilo).is_ok());
        assert_eq!(nomi_in(dir.path()), vec!["Rymembra.ryprofile"]);
    }

    #[test]
    fn il_primo_byte_distingue_l_archivio_dal_json_di_prima() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let vecchio = dir.path().join("profilo.json");
        std::fs::write(&vecchio, r#"{"rymembra":"rymembra.profilo"}"#).expect("v1");
        assert_eq!(e_un_archivio(&vecchio), Ok(false));

        let nuovo = dir.path().join("profilo.ryprofile");
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
        let futuro = dir.path().join("futuro.ryprofile");
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

    #[test]
    fn i_nomi_di_prima_si_leggono_ancora() {
        // Le skin con l'estensione di prima e i documenti di sincronia col
        // prefisso di prima: stanno dentro i profili che qualcuno ha su una
        // chiavetta, e l'estrazione li scrive comunque col nome di adesso.
        assert_eq!(
            classifica(&format!("skin/notte.{}", eredita::ESTENSIONE_SKIN)),
            Some(Voce::Skin("notte".to_owned()))
        );
        assert_eq!(
            classifica(&format!(
                "sincronia/{}portatile.v1.json.gz",
                eredita::PREFISSO_DISPOSITIVO
            )),
            Some(Voce::Sincronia("portatile".to_owned()))
        );
        // Le regole di sicurezza valgono per i nomi di prima come per i nuovi.
        let vecchia = eredita::ESTENSIONE_SKIN;
        assert_eq!(classifica(&format!("skin/../fuori.{vecchia}")), None);
        assert_eq!(classifica(&format!(r"skin/C:\altrove.{vecchia}")), None);
        assert_eq!(classifica(&format!("skin/.{vecchia}")), None);
        // Un'altra estensione non è una skin, e le bozze hanno la loro cartella.
        assert_eq!(classifica("skin/notte.exe"), None);
        assert_eq!(classifica(&format!("skin/bozze/prova.{vecchia}")), None);
    }

    #[test]
    fn si_scrive_solo_coi_nomi_di_adesso() {
        let nomi = [
            nome_skin("notte"),
            nome_bozza("notte"),
            nome_sincronia("portatile"),
            nome_copertina("0f1e2d3c", false),
            NOME_MANIFESTO.to_owned(),
            NOME_PREFERENZE.to_owned(),
            NOME_BIBLIOTECA.to_owned(),
            format!("profilo.{ESTENSIONE}"),
            FIRMA.to_owned(),
        ];
        for nome in nomi {
            assert!(
                !nome.contains(eredita::STEM_DIARIO),
                "«{nome}» porta il nome di prima"
            );
            assert!(!nome.ends_with(eredita::ESTENSIONE_SKIN), "{nome}");
            assert!(!nome.ends_with(eredita::ESTENSIONE_PROFILO), "{nome}");
        }
    }

    #[test]
    fn un_manifesto_di_prima_si_legge() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        // Il campo e la firma di prima, com'erano scritti allora.
        let di_prima = manifesto_finto().replace(
            "\"rymembra\":\"rymembra.profilo\"",
            &format!("\"aether\":\"{}\"", eredita::FIRMA_PROFILO),
        );
        assert!(di_prima.contains("\"aether\":"), "{di_prima}");
        let archivio = dir.path().join("di-prima.zip");
        {
            let mut scrittore = Scrittore::crea(&archivio).expect("archivio");
            scrittore
                .aggiungi_testo(NOME_MANIFESTO, &di_prima)
                .expect("manifesto");
            scrittore.chiudi().expect("chiusura");
        }

        let lettore = Lettore::apri(&archivio).expect("il manifesto di prima si legge");
        assert_eq!(lettore.manifesto.rymembra, eredita::FIRMA_PROFILO);
        assert_eq!(lettore.manifesto.versione, 2);

        // Il campo nuovo con la firma di prima, e il campo di prima con la firma
        // nuova, sono i due modi in cui un file scritto a mano potrebbe
        // mescolarli: la firma è quel che conta, e quella è una delle due.
        for mescolato in [
            manifesto_finto().replace(FIRMA, eredita::FIRMA_PROFILO),
            di_prima.replace(eredita::FIRMA_PROFILO, FIRMA),
        ] {
            let archivio = dir.path().join("mescolato.zip");
            let mut scrittore = Scrittore::crea(&archivio).expect("archivio");
            scrittore
                .aggiungi_testo(NOME_MANIFESTO, &mescolato)
                .expect("manifesto");
            scrittore.chiudi().expect("chiusura");
            assert!(Lettore::apri(&archivio).is_ok(), "{mescolato}");
        }
    }

    #[test]
    fn una_firma_estranea_resta_rifiutata() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let campo_di_prima = format!("\"aether\":\"{}\"", eredita::FIRMA_PROFILO);
        for estranea in [
            manifesto_finto().replace(FIRMA, "un.altro.profilo"),
            manifesto_finto().replace(
                "\"rymembra\":\"rymembra.profilo\"",
                "\"aether\":\"un.altro.profilo\"",
            ),
            // La firma di un profilo di Rymembra con una cifra in più non è la
            // stessa firma.
            manifesto_finto().replace(
                "\"rymembra\":\"rymembra.profilo\"",
                &campo_di_prima.replace(".profilo\"", ".profilo2\""),
            ),
        ] {
            let archivio = dir.path().join("estraneo.zip");
            let mut scrittore = Scrittore::crea(&archivio).expect("archivio");
            scrittore
                .aggiungi_testo(NOME_MANIFESTO, &estranea)
                .expect("manifesto");
            scrittore.chiudi().expect("chiusura");
            let err = Lettore::apri(&archivio).expect_err("deve rifiutare");
            assert_eq!(err.code().kind().code(), "settings.corrupt", "{estranea}");
        }
        assert!(firma_riconosciuta(FIRMA));
        assert!(firma_riconosciuta(eredita::FIRMA_PROFILO));
        assert!(!firma_riconosciuta(""));
        assert!(!firma_riconosciuta("Rymembra.profilo"));
    }

    /// Riscrive la dimensione aperta che l'archivio **dichiara** per la voce
    /// `nome`, nell'intestazione locale e nell'indice centrale: è quel che fa
    /// chi costruisce una bomba, perché rapporto e totale guardano quel numero.
    fn dichiara(byte: &mut [u8], nome: &str, dichiarata: u32) {
        let nome = nome.as_bytes();
        let mut riscritte = 0;
        for i in 0..byte.len().saturating_sub(46) {
            let (firma, dimensione, inizio_nome) = match &byte[i..i + 4] {
                b"PK\x03\x04" => (true, i + 22, i + 30),
                b"PK\x01\x02" => (true, i + 24, i + 46),
                _ => (false, 0, 0),
            };
            if firma && byte.get(inizio_nome..inizio_nome + nome.len()) == Some(nome) {
                byte[dimensione..dimensione + 4].copy_from_slice(&dichiarata.to_le_bytes());
                riscritte += 1;
            }
        }
        assert_eq!(riscritte, 2, "intestazione locale e indice centrale");
    }

    /// Un archivio con il manifesto e una voce in `deflate` che si apre in
    /// `vera` byte e ne dichiara `dichiarata`.
    fn bugiardo(dir: &Path, nome_voce: &str, vera: usize, dichiarata: u32) -> PathBuf {
        let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(NOME_MANIFESTO, Scrittore::intatto())
            .expect("manifesto");
        zip.write_all(manifesto_finto().as_bytes())
            .expect("manifesto");
        zip.start_file(nome_voce, Scrittore::compresso())
            .expect("voce");
        zip.write_all(&vec![b' '; vera]).expect("voce");
        let mut byte = zip.finish().expect("chiusura").into_inner();
        dichiara(&mut byte, nome_voce, dichiarata);
        let percorso = dir.join("bugiardo.ryprofile");
        std::fs::write(&percorso, &byte).expect("archivio");
        percorso
    }

    #[test]
    fn una_voce_che_si_apre_oltre_la_dichiarata_si_rifiuta() {
        // Un mebibyte che dice di essere dieci byte: rapporto e totale, letti
        // dall'indice, restavano sotto soglia, e l'estrazione scriveva tutto
        // il mebibyte — fino a 64 MiB per voce, per ogni voce.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let hash = "ab".repeat(32);
        let archivio = bugiardo(dir.path(), &nome_copertina(&hash, false), 1024 * 1024, 10);
        let mut lettore = Lettore::apri(&archivio).expect("l'indice passa: dice dieci byte");
        let voce = lettore
            .voce_di(|voce| matches!(voce, Voce::Copertina { .. }))
            .expect("la copertina c'è");

        let destinazione = dir
            .path()
            .join("store")
            .join("ab")
            .join(format!("{hash}.jpg"));
        let err = lettore
            .estrai(&voce, &destinazione)
            .expect_err("più grande di quanto dichiarasse");
        assert_eq!(err.code().kind().code(), "settings.corrupt");
        assert!(!destinazione.exists());
        assert_eq!(
            nomi_in(destinazione.parent().expect("cartella")),
            Vec::<String>::new(),
            "niente a metà nello store"
        );
    }

    #[test]
    fn un_documento_che_si_apre_oltre_il_dichiarato_si_rifiuta() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let archivio = bugiardo(dir.path(), NOME_BIBLIOTECA, 1024 * 1024, 10);
        let mut lettore = Lettore::apri(&archivio).expect("l'indice passa");
        let voce = lettore
            .voce_di(|voce| *voce == Voce::Biblioteca)
            .expect("la biblioteca c'è");
        let err = lettore
            .byte(&voce)
            .expect_err("più grande di quanto dichiarasse");
        assert_eq!(err.code().kind().code(), "settings.corrupt");
    }

    #[test]
    fn i_byte_aperti_hanno_un_budget_su_tutto_l_archivio() {
        // Ogni voce sotto il proprio tetto, e tante: i byte aperti davvero si
        // contano su tutto l'archivio, manifesto compreso.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let archivio = dir.path().join("p.ryprofile");
        {
            let mut scrittore = Scrittore::crea(&archivio).expect("archivio");
            scrittore
                .aggiungi_testo(NOME_MANIFESTO, &manifesto_finto())
                .expect("manifesto");
            for n in 0..3 {
                scrittore
                    .aggiungi_byte(&nome_copertina(&format!("ab{n:02}"), false), &[b'x'; 100])
                    .expect("jpg");
            }
            scrittore.chiudi().expect("chiusura");
        }
        let mut lettore = Lettore::apri(&archivio).expect("lettura");
        assert!(lettore.aperti > 0, "il manifesto si conta");
        lettore.budget = lettore.aperti + 250;
        let voci = lettore.voci_di(|voce| matches!(voce, Voce::Copertina { .. }));
        let store = dir.path().join("store");
        let mut esiti = Vec::new();
        for (n, voce) in voci.iter().enumerate() {
            esiti.push(
                lettore
                    .estrai(voce, &store.join(format!("{n}.jpg")))
                    .is_ok(),
            );
        }
        assert_eq!(esiti, vec![true, true, false], "la terza supera il budget");
        assert!(!store.join("2.jpg").exists());
    }

    /// Un manifesto minimo, scritto a mano perché la prova non dipenda da chi lo
    /// costruisce.
    fn manifesto_finto() -> String {
        r#"{"rymembra":"rymembra.profilo","versione":2,"creatoMs":0,"identita":"x",
            "dispositivo":"prova","applicazione":"2.3.1","abbinamento":"track_key",
            "contiene":{"preferenze":0,"sincronia":false,"biblioteca":false,
                        "copertine":0,"skin":0,"bozze":0},"fuori":[]}"#
            .replace([' ', '\n'], "")
    }
}
