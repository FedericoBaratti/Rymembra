//! La scrittura di un file che non si lascia vedere a metà.
//!
//! # Il difetto che chiude
//!
//! `std::fs::write(percorso, byte)` apre il file **col nome giusto**, lo tronca
//! e ci scrive dentro. Se il processo muore a metà — la corrente, un'uscita
//! forzata, un disco che si riempie al terzo megabyte — sul disco resta un file
//! col nome giusto e il contenuto sbagliato: mezza copertina, un documento di
//! skin troncato a metà di una graffa, una bozza dello Studio vuota. E chi lo
//! rilegge non ha modo di accorgersene, perché il nome è l'unica cosa che guarda.
//!
//! Lo store delle copertine lo sapeva e si scriveva da sé la cura
//! (`write_atomic`, in `covers.rs`). Era giusta, e aveva un difetto piccolo: il
//! nome temporaneo era `tmp{pid}`, cioè **lo stesso per tutti i fili del
//! processo**. Due fili che salvavano la stessa copertina nello stesso istante —
//! la scansione e l'arricchimento, sullo stesso album — scrivevano nello stesso
//! file temporaneo, e il primo rinomina portava via il file che il secondo
//! stava ancora riempiendo. Qui la cura sta una volta sola, per tutti, e il nome
//! temporaneo non si ripete.
//!
//! # Cosa promette, e cosa no
//!
//! Promette che **chi legge il nome trova il file vecchio o il file nuovo,
//! intero**. Mai un pezzo del nuovo, mai un file vuoto al posto del vecchio. È
//! la sola cosa che conta per chi rilegge alla cieca: lo store delle
//! copertine, le skin installate, le bozze e la chat dello Studio.
//!
//! Il percorso è in quattro passi:
//!
//! 1. si scrive accanto, nella **stessa cartella**, sotto un nome che nessun
//!    altro sta usando: `<nome>.<pid>-<contatore>.tmp`. Stessa cartella perché un
//!    rinomina è atomico solo dentro lo stesso volume — fra due dischi diventa
//!    una copia, cioè di nuovo una finestra in cui il file è a metà;
//! 2. `sync_all`: i byte vanno sul **supporto**, non solo nella cache del
//!    sistema. Senza, dopo un'interruzione di corrente il rinomina può essere
//!    sul disco e il contenuto no — e si ritorna al file col nome giusto e
//!    dentro zeri, che è esattamente il guasto da togliere;
//! 3. si rinomina sopra il nome vero. Su Windows `std::fs::rename` sostituisce
//!    un file esistente (`MoveFileExW` con `MOVEFILE_REPLACE_EXISTING`);
//! 4. se qualcosa va storto, il temporaneo si toglie. Un temporaneo che resta
//!    dopo un crash non fa danni — nessuno lo legge, perché nessuno lo cerca per
//!    nome — e chi tiene una cartella piena di file può riconoscerlo con
//!    [`e_temporaneo`] e spazzarlo.
//!
//! Non promette la **durata del rinomina** dopo un crash del sistema: servirebbe
//! un `fsync` della cartella, che su Windows non esiste in forma portabile. Il
//! caso peggiore è ritrovare il file vecchio dopo un'interruzione di corrente —
//! che è una perdita di lavoro, non un file rotto.
//!
//! # Per chi arriva dopo
//!
//! Chiunque scriva un file che qualcun altro rileggerà senza sapere com'è
//! andata la scrittura passi di qui: skin, bozze e chat dello Studio, file di
//! stato. Non serve per i file che si **accodano** (il diario) né per i file che
//! l'utente ha scelto e riscrive in un colpo (un'esportazione che fallisce si
//! ripete), ma serve per tutto quel che Rymembra rilegge all'avvio fidandosi.
//!
//! E per i file **dell'utente** che si modificano sul posto invece di
//! riscriverli da capo — i tag dentro un brano — c'è [`modifica_con_riserva`]:
//! non la stessa promessa, perché il file non si sostituisce, ma una copia di
//! riserva accanto che lo rimette com'era se la modifica non arriva in fondo.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCode};

use crate::files::io_error;

/// L'ultima parte del nome di ogni file temporaneo scritto da qui.
///
/// Pubblica perché chi pulisce una cartella deve poter riconoscere gli avanzi
/// di una scrittura interrotta: vedi [`e_temporaneo`].
pub const SUFFISSO_TEMPORANEO: &str = ".tmp";

/// Il numero che distingue i temporanei scritti dallo stesso processo.
///
/// Un contatore di processo e non l'identificativo del filo: i fili si
/// riusano, un contatore che cresce no. Insieme al `pid` fa un nome che non si
/// ripete né fra due fili dello stesso processo né fra due processi — e per il
/// caso residuo, un `pid` riusato da un processo nuovo che ritrova l'avanzo di
/// uno vecchio col suo stesso numero, c'è `create_new` più sotto.
static CONTATORE: AtomicU64 = AtomicU64::new(0);

/// Quante volte si prova a rinominare prima di arrendersi.
///
/// Su Windows un rinomina sopra un file appena creato può fallire con «accesso
/// negato» per qualche decina di millisecondi: è l'antivirus, o l'indicizzatore
/// di ricerca, che ha aperto il file per guardarlo senza condividerne la
/// cancellazione. Non è un guasto e passa da sé; arrendersi al primo colpo
/// vorrebbe dire una copertina persa ogni tanto, a caso, solo sulle macchine
/// con un antivirus zelante.
const TENTATIVI_DI_RINOMINA: u32 = 5;

/// La pausa fra due tentativi, moltiplicata per il numero del tentativo.
const PAUSA_FRA_I_TENTATIVI: Duration = Duration::from_millis(15);

/// Quante volte si cerca un nome temporaneo libero.
///
/// Uno basta sempre, salvo un avanzo di un processo morto col nostro stesso
/// `pid` e lo stesso numero di contatore: allora si passa al numero dopo.
const TENTATIVI_DI_NOME: u32 = 8;

/// Scrive `byte` in `percorso` in modo che nessuno possa leggerne metà.
///
/// Crea le cartelle che mancano. Se `percorso` esiste già viene **sostituito**,
/// intero: chi lo legge durante la scrittura trova il contenuto di prima.
///
/// Vedi il `//!` di questo modulo per i quattro passi e per quello che non si
/// promette.
///
/// # Errori
///
/// Quelli di [`io_error`], col percorso **vero** e non quello del temporaneo
/// quando è il rinomina a fallire: è il file che l'utente riconosce, e il nome
/// temporaneo non esiste più quando legge il messaggio. Una scrittura che
/// fallisce prima del rinomina porta il percorso del temporaneo, perché è lì
/// che il disco ha detto di no — ed è la differenza fra «disco pieno» su una
/// cartella e su un'altra.
pub fn scrivi_atomico(percorso: &Path, byte: &[u8]) -> Result<(), AppError> {
    if let Some(cartella) = percorso.parent()
        && !cartella.as_os_str().is_empty()
    {
        std::fs::create_dir_all(cartella)
            .map_err(|err| io_error(&cartella.display().to_string(), &err))?;
    }

    let (temporaneo, mut file) = apri_temporaneo(percorso)?;

    // Da qui ogni uscita per errore toglie il temporaneo. Una chiusura e non
    // un `Drop` perché l'unico percorso di successo lo consuma col rinomina, e
    // una guardia da disinnescare lì sarebbe una riga in più da non
    // dimenticare, per niente.
    let butta = |err: &std::io::Error, dove: &Path| {
        let _ = std::fs::remove_file(&temporaneo);
        io_error(&dove.display().to_string(), err)
    };

    if let Err(err) = file.write_all(byte).and_then(|()| file.sync_all()) {
        drop(file);
        return Err(butta(&err, &temporaneo));
    }
    // Chiuso **prima** del rinomina: su Windows un file ancora aperto senza
    // condivisione della cancellazione non si lascia rinominare.
    drop(file);

    rinomina_con_pazienza(&temporaneo, percorso).map_err(|err| butta(&err, percorso))
}

/// Il `detail` dell'errore con cui [`modifica_con_riserva`] dice che il file
/// non si è potuto rimettere com'era, e che la sua riserva è rimasta accanto.
///
/// Pubblico perché chi chiama in un ciclo — il ritorno dei tag nei file — conta
/// questi casi a parte: sono gli unici in cui accanto a un brano dell'utente
/// resta una copia che qualcuno dovrà guardare. Vedi [`riserva_rimasta`].
pub const RISERVA_RIMASTA: &str = "riserva rimasta";

/// L'errore dice che una riserva è rimasta accanto al file, perché il file non
/// si è potuto rimettere com'era?
#[must_use]
pub fn riserva_rimasta(err: &AppError) -> bool {
    matches!(
        err.code(),
        ErrorCode::FsWriteFailed { detail: Some(detail), .. } if detail == RISERVA_RIMASTA
    )
}

/// Modifica **sul posto** un file che c'è già, con una copia di riserva accanto
/// che lo rimette com'era se la modifica non arriva in fondo.
///
/// È la protezione per chi non ha i byte nuovi in mano ma un programma che li
/// produce riscrivendo un file: `lofty`, che apre il file che gli si dà e ci
/// riscrive dentro i tag — spostando l'audio, se il tag è cresciuto. Fatto sul
/// file dell'utente senza rete, un guasto a metà lascia un brano troncato col
/// nome giusto, e il brano è suo, non una cache che si rifà.
///
/// I passi:
///
/// 1. si prenota accanto un nome che nessun altro sta usando —
///    `<nome>.<pid>-<contatore>.tmp`, lo stesso di [`scrivi_atomico`] — e ci si
///    copia l'originale, con `sync_all`: la riserva è sul supporto **prima** che
///    l'originale venga toccato;
/// 2. `modifica` riceve il percorso **vero** e scrive lì, esattamente come si
///    scriveva prima;
/// 3. se riesce, si porta sul supporto anche l'originale e la riserva se ne va;
/// 4. se `modifica` torna un errore — o va in panico, perché un file storto può
///    far cadere un parser — l'originale si rimette com'era dalla riserva, e
///    solo dopo la riserva se ne va.
///
/// # Perché sul posto, e non su una copia che poi prende il posto del file
///
/// Era la prima forma di questa funzione, ed è sbagliata proprio dove sta la
/// libreria di chi ne ha una grande, cioè su un NAS. Un rinomina sopra
/// l'originale mette al suo posto un **file nuovo**: i permessi scritti su quel
/// file — un ACE che nega la scrittura, una DACL esplicita — non ci sono più, e
/// su Samba il file nuovo nasce con la maschera della condivisione. E su SMB un
/// brano aperto da qualcuno — che suona, che il preparatore ha già aperto, che
/// l'analisi sta misurando — non si lascia sostituire, mentre la scrittura sul
/// posto passava. Scrivendo sul posto il file resta **lo stesso**: identità,
/// permessi, condivisione e controlli d'accesso sono quelli di sempre, e un
/// file di sola lettura rifiuta la scrittura da sé, come prima.
///
/// # Cosa si promette, e cosa no
///
/// * Un **errore** o un **panico** nella modifica lasciano il file com'era,
///   byte per byte. Il ripristino ricopia la riserva **dentro** il file, sul
///   posto, che tiene identità e permessi; solo se quello è rifiutato si
///   rinomina la riserva sopra il file — che ha di nuovo il difetto dei
///   permessi, ma è meglio di un brano rotto.
/// * Se nemmeno il ripristino riesce, la riserva **resta**, e l'errore dice
///   dove: è la copia intera del brano com'era.
/// * Un'**interruzione vera** — la corrente, un processo terminato — durante
///   la scrittura può lasciare il file rovinato, come prima. La differenza è che
///   accanto c'è la riserva intera: si riconosce dal nome, che è quello del
///   brano più `.<numero>-<numero>.tmp`, e dalla dimensione, che è quella del
///   brano prima. Per recuperarlo si toglie il file rovinato e si rinomina la
///   riserva togliendo il suffisso. Nessuno la cancella da solo, apposta: dopo
///   un'interruzione può essere l'unica copia buona, e [`e_temporaneo`], che la
///   riconosce, non la autorizza a sparire.
/// * Serve lo **spazio** di una seconda copia, e un nome che allungato del
///   suffisso stia ancora nei limiti del filesystem. Se la riserva non si può
///   fare — disco pieno, nome troppo lungo — l'errore torna **prima** di
///   toccare l'originale.
///
/// # Errori
///
/// Quelli di [`io_error`] se la riserva non si fa; quelli di `modifica` se la
/// modifica fallisce e l'originale è stato rimesso com'era; `fs.writeFailed`
/// col percorso della riserva e [`RISERVA_RIMASTA`] nel dettaglio se il
/// ripristino non è riuscito.
pub fn modifica_con_riserva(
    percorso: &Path,
    modifica: impl FnOnce(&Path) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let (riserva, prenotato) = apri_temporaneo(percorso)?;
    drop(prenotato);
    // Da qui la guardia: finché la riserva non è intera, cadere vuol dire
    // soltanto toglierla — l'originale non è stato toccato.
    let mut guardia = Riserva {
        originale: percorso,
        riserva: &riserva,
        pronta: false,
        chiusa: false,
    };
    // `std::fs::copy` e non una lettura e una scrittura: su Windows è
    // `CopyFileExW`, che su una condivisione SMB può chiedere al server di
    // copiare da sé invece di far passare il brano due volte per la rete.
    std::fs::copy(percorso, &riserva)
        .and_then(|_| riserva_scrivibile(&riserva))
        .and_then(|()| OpenOptions::new().write(true).open(&riserva))
        .and_then(|copia| copia.sync_all())
        .map_err(|err| io_error(&riserva.display().to_string(), &err))?;
    guardia.pronta = true;

    match modifica(percorso) {
        Ok(()) => {
            // L'originale sul supporto prima di togliere la riserva: senza, dopo
            // un'interruzione di corrente il file potrebbe avere sul disco solo
            // metà della scrittura appena riuscita, e nessuna copia accanto.
            // Best-effort: la scrittura è riuscita, e un `sync` rifiutato non
            // è una ragione per dire il contrario.
            let _ = OpenOptions::new()
                .write(true)
                .open(percorso)
                .and_then(|file| file.sync_all());
            guardia.conferma();
            Ok(())
        }
        Err(err) => match guardia.ripristina() {
            Ok(()) => Err(err),
            Err(rimasta) => Err(rimasta.with_message(err.to_string())),
        },
    }
}

/// La riserva di [`modifica_con_riserva`], che fa la cosa giusta anche se chi la
/// tiene cade in panico.
///
/// Una guardia e non un ramo d'errore, perché fra la riserva e la fine gira
/// codice d'altri: un parser che va in panico davanti a un file storto
/// srotolerebbe oltre ogni `map_err`, lasciando l'originale scritto a metà.
/// Cadendo, la guardia fa quel che farebbe un errore: rimette l'originale.
/// Solo [`Riserva::conferma`] la toglie senza toccare niente.
struct Riserva<'a> {
    /// Il file che si sta modificando.
    originale: &'a Path,
    /// La sua copia accanto.
    riserva: &'a Path,
    /// La riserva è intera e sul supporto: da qui in poi, cadere vuol dire
    /// rimettere l'originale. Prima, vuol dire soltanto togliere una copia a
    /// metà.
    pronta: bool,
    /// Qualcuno ha già deciso — conferma o ripristino — e il `Drop` non deve
    /// fare niente.
    chiusa: bool,
}

impl Riserva<'_> {
    /// La modifica è riuscita: la riserva non serve più.
    fn conferma(mut self) {
        self.chiusa = true;
        let _ = std::fs::remove_file(self.riserva);
    }

    /// La modifica non è riuscita: l'originale torna com'era. Vedi [`rimetti`].
    fn ripristina(mut self) -> Result<(), AppError> {
        self.chiusa = true;
        rimetti(self.originale, self.riserva)
    }
}

impl Drop for Riserva<'_> {
    fn drop(&mut self) {
        if self.chiusa {
            return;
        }
        if self.pronta {
            // Qui non c'è nessuno a cui dire com'è andata: si srotola da un
            // panico. Se il ripristino non riesce, la riserva resta dov'è, e il
            // nome dice cos'è.
            let _ = rimetti(self.originale, self.riserva);
        } else {
            let _ = std::fs::remove_file(self.riserva);
        }
    }
}

/// Toglie la sola lettura alla riserva, che la copia eredita dall'originale.
///
/// Su Windows `CopyFileExW` porta con sé gli attributi, e una riserva di sola
/// lettura non si lascia aprire per il `sync_all` né cancellare dopo: un brano
/// di sola lettura — che la scrittura sul posto rifiuterà da sé, come sempre —
/// lascerebbe accanto a sé una copia intera che nessuno toglie. Si tocca solo
/// la riserva, mai l'originale, e solo il permesso di scrittura del
/// proprietario.
fn riserva_scrivibile(riserva: &Path) -> std::io::Result<()> {
    let mut permessi = std::fs::metadata(riserva)?.permissions();
    if !permessi.readonly() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        permessi.set_mode(permessi.mode() | 0o200);
    }
    #[cfg(not(unix))]
    {
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "fuori da Unix è l'attributo «sola lettura» e nient'altro, ed è della riserva"
        )]
        permessi.set_readonly(false);
    }
    std::fs::set_permissions(riserva, permessi)
}

/// Rimette `originale` com'era dalla `riserva`, e toglie la riserva solo se ci
/// è riuscito.
///
/// Se l'originale ha ancora gli stessi byte della riserva non c'è niente da
/// rimettere: la modifica è fallita prima di scrivere — un file di sola
/// lettura, un'apertura rifiutata — e riscriverlo vorrebbe dire inciampare
/// nello stesso rifiuto e lasciare una riserva per un file mai toccato.
///
/// Altrimenti prima **sul posto**: si riapre l'originale, lo si svuota e ci si
/// ricopia la riserva. Tiene identità e permessi del file, ed è possibile ogni
/// volta che lo era la scrittura che si sta disfacendo — è la stessa apertura.
/// Se è rifiutata, o si interrompe a metà, la riserva si rinomina sopra
/// l'originale: i permessi propri del file vanno persi, ma il contenuto torna
/// intero.
///
/// # Errori
///
/// `fs.writeFailed` col percorso della riserva e [`RISERVA_RIMASTA`] nel
/// dettaglio, se né l'una né l'altra strada riesce. La riserva allora resta.
fn rimetti(originale: &Path, riserva: &Path) -> Result<(), AppError> {
    if stessi_byte(originale, riserva) {
        let _ = std::fs::remove_file(riserva);
        return Ok(());
    }
    let rimesso = ricopia_sul_posto(riserva, originale)
        .or_else(|_| rinomina_con_pazienza(riserva, originale));
    match rimesso {
        Ok(()) => {
            // Dopo un rinomina la riserva non c'è già più: `NotFound` qui è il
            // successo, non un guasto.
            let _ = std::fs::remove_file(riserva);
            Ok(())
        }
        Err(err) => Err(AppError::new(ErrorCode::FsWriteFailed {
            path: riserva.display().to_string(),
            detail: Some(RISERVA_RIMASTA.to_owned()),
        })
        .with_cause(format!(
            "il file non si è potuto rimettere com'era ({err}): la sua copia intera è rimasta in {}",
            riserva.display()
        ))),
    }
}

/// Ricopia la riserva dentro l'originale, sul posto, e la porta sul supporto.
fn ricopia_sul_posto(riserva: &Path, originale: &Path) -> std::io::Result<()> {
    let mut da = std::fs::File::open(riserva)?;
    // `truncate` e non `create`: l'originale c'è, e se nel frattempo non ci
    // fosse più non è qui che va ricreato con permessi che non erano i suoi.
    let mut a = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(originale)?;
    std::io::copy(&mut da, &mut a)?;
    a.sync_all()
}

/// I due file hanno gli stessi byte? `false` anche quando uno dei due non si
/// legge: nel dubbio si rimette, che è la strada che non perde niente.
///
/// Si paga solo sul ramo dell'errore, dove leggere il brano due volte costa
/// meno che riscriverlo per niente.
fn stessi_byte(a: &Path, b: &Path) -> bool {
    use std::io::Read as _;
    let lunghezza = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    if lunghezza(a).is_none() || lunghezza(a) != lunghezza(b) {
        return false;
    }
    let (Ok(mut da_a), Ok(mut da_b)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    let mut pezzo_a = vec![0_u8; 64 * 1024];
    let mut pezzo_b = vec![0_u8; 64 * 1024];
    loop {
        let letti = match da_a.read(&mut pezzo_a) {
            // Le lunghezze sono uguali: finito l'uno, è finito anche l'altro.
            Ok(0) => return true,
            Ok(letti) => letti,
            Err(_) => return false,
        };
        let (Some(di_a), Some(di_b)) = (pezzo_a.get(..letti), pezzo_b.get_mut(..letti)) else {
            return false;
        };
        if da_b.read_exact(di_b).is_err() || di_a != &*di_b {
            return false;
        }
    }
}

/// Un file temporaneo nuovo accanto a `percorso`, aperto in scrittura.
///
/// `create_new`: se il nome c'è già non lo si riusa — sarebbe l'avanzo di
/// qualcun altro, o peggio il temporaneo di un filo che lo sta ancora
/// riempiendo — e si passa al numero dopo.
fn apri_temporaneo(percorso: &Path) -> Result<(PathBuf, std::fs::File), AppError> {
    let nome = percorso
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pid = std::process::id();
    let mut ultimo_errore = None;
    for _ in 0..TENTATIVI_DI_NOME {
        let numero = CONTATORE.fetch_add(1, Ordering::Relaxed);
        let temporaneo =
            percorso.with_file_name(format!("{nome}.{pid}-{numero}{SUFFISSO_TEMPORANEO}"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporaneo)
        {
            Ok(file) => return Ok((temporaneo, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                ultimo_errore = Some((temporaneo, err));
            }
            Err(err) => return Err(io_error(&temporaneo.display().to_string(), &err)),
        }
    }
    // Otto nomi occupati di fila non sono sfortuna: è una cartella piena di
    // avanzi col nostro `pid`, e l'ultimo errore dice dove.
    let (dove, err) = ultimo_errore.unwrap_or_else(|| {
        (
            percorso.to_path_buf(),
            std::io::Error::from(std::io::ErrorKind::AlreadyExists),
        )
    });
    Err(io_error(&dove.display().to_string(), &err))
}

/// `ERROR_SHARING_VIOLATION`: qualcuno ha il file aperto senza condividere
/// quel che serve.
const VIOLAZIONE_DI_CONDIVISIONE: i32 = 32;

/// `ERROR_LOCK_VIOLATION`: qualcuno tiene un lucchetto su una parte del file.
const VIOLAZIONE_DI_LUCCHETTO: i32 = 33;

/// `std::fs::rename`, ritentato quando Windows dice che il file è occupato.
///
/// Tre risposte valgono «aspetta»: «accesso negato», e le due violazioni — di
/// condivisione e di lucchetto — con cui Windows risponde quando un antivirus o
/// l'indicizzatore di ricerca hanno appena aperto il file per guardarlo. Le
/// ultime due `std` non le sa nominare e le lascia come codici grezzi: senza
/// guardarle a mano, il rinomina si arrendeva al primo colpo proprio nel caso
/// per cui i tentativi esistono.
///
/// Solo quei casi: un disco pieno, un percorso sbagliato o una cartella sparita
/// non passano aspettando, e ritentarli vorrebbe dire soltanto metterci di più
/// a dire la stessa cosa.
fn rinomina_con_pazienza(da: &Path, a: &Path) -> std::io::Result<()> {
    let occupato = |err: &std::io::Error| {
        err.kind() == std::io::ErrorKind::PermissionDenied
            || matches!(
                err.raw_os_error(),
                Some(VIOLAZIONE_DI_CONDIVISIONE | VIOLAZIONE_DI_LUCCHETTO)
            )
    };
    let mut tentativo = 1;
    loop {
        match std::fs::rename(da, a) {
            Ok(()) => return Ok(()),
            Err(err) if occupato(&err) && tentativo < TENTATIVI_DI_RINOMINA => {
                std::thread::sleep(PAUSA_FRA_I_TENTATIVI * tentativo);
                tentativo += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

/// Il file è l'avanzo di una scrittura di [`scrivi_atomico`] interrotta?
///
/// Guarda solo il nome — `<nome>.<pid>-<contatore>.tmp` — e non l'età: chi
/// spazza decide da sé quanto aspettare, e **deve** aspettare, perché un
/// temporaneo giovane può essere quello di una scrittura in corso su un altro
/// filo. Vedi `covers::CoverStore::spazza_avanzi`.
///
/// Riconosce anche la forma che lo store delle copertine usava prima di questo
/// modulo, `<impronta>.tmp<pid>`: sono avanzi dello stesso genere, e le
/// cartelle di chi aggiorna ne possono contenere.
#[must_use]
pub fn e_temporaneo(percorso: &Path) -> bool {
    let Some(nome) = percorso.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if let Some(senza) = nome.strip_suffix(SUFFISSO_TEMPORANEO)
        && let Some((_, firma)) = senza.rsplit_once('.')
        && let Some((pid, numero)) = firma.split_once('-')
        && solo_cifre(pid)
        && solo_cifre(numero)
    {
        return true;
    }
    // La forma di prima: `<impronta>.tmp<pid>`, con l'estensione `.jpg`
    // sostituita da `with_extension`.
    nome.rsplit_once(".tmp")
        .is_some_and(|(prima, pid)| !prima.is_empty() && solo_cifre(pid))
}

/// Una sequenza non vuota di cifre ASCII.
fn solo_cifre(testo: &str) -> bool {
    !testo.is_empty() && testo.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod prove {
    use super::*;

    /// I nomi dei file in una cartella, ordinati.
    fn nomi(cartella: &Path) -> Vec<String> {
        let mut nomi: Vec<String> = std::fs::read_dir(cartella)
            .expect("cartella")
            .filter_map(|voce| Some(voce.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        nomi.sort();
        nomi
    }

    #[test]
    fn scrive_crea_le_cartelle_e_non_lascia_temporanei() {
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("a").join("b").join("documento.json");
        scrivi_atomico(&percorso, b"{\"v\":1}").expect("scrittura");
        assert_eq!(std::fs::read(&percorso).expect("lettura"), b"{\"v\":1}");
        assert_eq!(
            nomi(percorso.parent().expect("cartella")),
            ["documento.json"]
        );
    }

    #[test]
    fn sostituisce_il_file_che_c_era_intero() {
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("skin.css");
        scrivi_atomico(&percorso, &vec![b'a'; 10_000]).expect("prima");
        scrivi_atomico(&percorso, b"corto").expect("seconda");
        assert_eq!(
            std::fs::read(&percorso).expect("lettura"),
            b"corto",
            "il file nuovo, intero, e niente del vecchio in coda"
        );
        assert_eq!(nomi(dir.path()), ["skin.css"]);
    }

    #[test]
    fn due_fili_sullo_stesso_file_non_si_pestano_i_piedi() {
        // Il difetto di `tmp{pid}`: due fili dello stesso processo scrivevano lo
        // stesso temporaneo, e il primo rinomina portava via il file che il
        // secondo stava riempiendo. Qui otto fili scrivono ciascuno un
        // contenuto riconoscibile sullo stesso nome, cento volte: alla fine il
        // file dev'essere **uno** dei contenuti, intero, e nessun temporaneo
        // deve restare.
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = std::sync::Arc::new(dir.path().join("copertina.jpg"));
        let via = std::sync::Arc::new(std::sync::Barrier::new(8));
        let fili: Vec<_> = (0..8u8)
            .map(|filo| {
                let percorso = std::sync::Arc::clone(&percorso);
                let via = std::sync::Arc::clone(&via);
                std::thread::spawn(move || {
                    via.wait();
                    let contenuto = vec![b'a' + filo; 64 * 1024];
                    for _ in 0..100 {
                        // Su Windows due rinomina sullo stesso nome possono
                        // scontrarsi anche dopo i tentativi: quel che conta
                        // qui è che nessun esito lasci un file a metà.
                        let _ = scrivi_atomico(&percorso, &contenuto);
                    }
                })
            })
            .collect();
        for filo in fili {
            filo.join().expect("il filo è finito");
        }
        let letto = std::fs::read(&*percorso).expect("lettura");
        assert_eq!(letto.len(), 64 * 1024, "mai un file a metà");
        let primo = letto.first().copied().expect("non vuoto");
        assert!(
            letto.iter().all(|&b| b == primo),
            "il contenuto di un filo solo, non un miscuglio"
        );
        assert_eq!(nomi(dir.path()), ["copertina.jpg"], "nessun avanzo");
    }

    #[test]
    fn un_errore_toglie_il_temporaneo() {
        // Il rinomina fallisce perché al posto del file c'è una **cartella**
        // piena: il temporaneo scritto accanto non deve restare.
        let dir = tempfile::tempdir().expect("cartella");
        let percorso = dir.path().join("occupato");
        std::fs::create_dir_all(percorso.join("dentro")).expect("cartella al posto del file");
        scrivi_atomico(&percorso, b"x").expect_err("non si scrive sopra una cartella");
        assert_eq!(
            nomi(dir.path()),
            ["occupato"],
            "il temporaneo se n'è andato"
        );
    }

    #[test]
    fn riconosce_gli_avanzi_e_solo_quelli() {
        for avanzo in [
            "ab12.jpg.4242-7.tmp",
            "documento.json.1-0.tmp",
            // La forma dello store di prima: `with_extension("tmp{pid}")`.
            "0123abcd.tmp9876",
            "0123abcd.t.tmp9876",
        ] {
            assert!(e_temporaneo(Path::new(avanzo)), "{avanzo} è un avanzo");
        }
        for vero in [
            "0123abcd.jpg",
            "0123abcd.t.jpg",
            "note.tmp",
            "a.b-c.tmp",
            ".tmp",
            "tmp",
            "copertina.tmpx",
        ] {
            assert!(!e_temporaneo(Path::new(vero)), "{vero} non è un avanzo");
        }
    }

    /// Un brano finto e riconoscibile: abbastanza grande da non stare in un
    /// pezzo solo di [`stessi_byte`].
    fn audio() -> Vec<u8> {
        (0..200 * 1024_u32)
            .map(|i| u8::try_from(i % 251).expect("sotto 251"))
            .collect()
    }

    /// Le riserve rimaste in una cartella.
    fn riserve(cartella: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(cartella)
            .expect("cartella")
            .filter_map(|voce| Some(voce.ok()?.path()))
            .filter(|p| e_temporaneo(p))
            .collect()
    }

    /// Un errore a metà della modifica rimette l'originale com'era, byte per
    /// byte, e non lascia la riserva accanto.
    ///
    /// La modifica qui tronca il file **vero** prima di arrendersi: è quel che
    /// resta sul disco quando un riscrittore di tag si ferma a metà.
    #[test]
    fn una_modifica_che_si_interrompe_rimette_l_originale() {
        let dir = tempfile::tempdir().expect("cartella");
        let brano = dir.path().join("brano.flac");
        std::fs::write(&brano, audio()).expect("brano");

        let esito = modifica_con_riserva(&brano, |vero| {
            std::fs::write(vero, b"mezzo").expect("il file si tronca");
            Err(AppError::new(ErrorCode::InternalAborted {
                what: Some("prova".to_owned()),
            }))
        });
        assert_eq!(
            esito.map_err(|e| e.code().kind().code().to_owned()),
            Err("internal.aborted".to_owned()),
            "torna l'errore della modifica, non quello del ripristino"
        );
        assert_eq!(std::fs::read(&brano).expect("letto"), audio());
        assert_eq!(nomi(dir.path()), ["brano.flac"], "nessuna riserva rimasta");
    }

    /// Anche un panico dentro la modifica — un parser davanti a un file storto
    /// — rimette l'originale e porta via la riserva.
    #[test]
    fn un_panico_nella_modifica_rimette_l_originale() {
        let dir = tempfile::tempdir().expect("cartella");
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, audio()).expect("brano");

        let caduta = std::panic::catch_unwind(|| {
            let _ = modifica_con_riserva(&brano, |vero| {
                std::fs::write(vero, b"").expect("troncato");
                panic!("il parser cade");
            });
        });
        assert!(caduta.is_err(), "il panico arriva fin qui");
        assert_eq!(std::fs::read(&brano).expect("letto"), audio());
        assert_eq!(nomi(dir.path()), ["brano.mp3"], "nessuna riserva rimasta");
    }

    /// Mentre la modifica gira, la riserva c'è, è intera e ha il nome che
    /// [`e_temporaneo`] riconosce; a modifica riuscita se ne va, e il file è
    /// quello nuovo.
    #[test]
    fn una_modifica_riuscita_resta_e_toglie_la_riserva() {
        let dir = tempfile::tempdir().expect("cartella");
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"tag vecchio|audio").expect("brano");

        modifica_con_riserva(&brano, |vero| {
            let durante = riserve(dir.path());
            assert_eq!(durante.len(), 1, "una riserva accanto durante la modifica");
            let riserva = durante.first().expect("una");
            assert_eq!(
                std::fs::read(riserva).expect("riserva"),
                b"tag vecchio|audio"
            );
            std::fs::write(vero, b"tag nuovo|audio").expect("modifica");
            Ok(())
        })
        .expect("modifica");

        assert_eq!(std::fs::read(&brano).expect("letto"), b"tag nuovo|audio");
        assert_eq!(nomi(dir.path()), ["brano.mp3"]);
    }

    /// Il file modificato è **lo stesso** file, non uno nuovo col suo nome: un
    /// secondo nome dello stesso file vede la modifica.
    ///
    /// È la proprietà che porta con sé permessi, condivisione e identità, e che
    /// un rinomina sopra l'originale — la forma di prima — perdeva: il
    /// collegamento fisico sarebbe rimasto col contenuto vecchio.
    #[test]
    fn il_file_modificato_resta_lo_stesso_file() {
        let dir = tempfile::tempdir().expect("cartella");
        let brano = dir.path().join("brano.mp3");
        let altro_nome = dir.path().join("lo stesso.mp3");
        std::fs::write(&brano, b"prima").expect("brano");
        std::fs::hard_link(&brano, &altro_nome).expect("collegamento fisico");

        modifica_con_riserva(&brano, |vero| {
            std::fs::write(vero, b"dopo").expect("modifica");
            Ok(())
        })
        .expect("modifica");

        assert_eq!(std::fs::read(&altro_nome).expect("letto"), b"dopo");
    }

    /// Una riserva che non si può fare — qui un nome che col suffisso passa i
    /// 255 caratteri — ferma tutto **prima** di toccare l'originale.
    #[test]
    fn senza_riserva_l_originale_non_si_tocca() {
        let dir = tempfile::tempdir().expect("cartella");
        let brano = dir.path().join(format!("{}.mp3", "x".repeat(246)));
        std::fs::write(&brano, b"intatto").expect("brano dal nome lungo");

        let mut chiamata = false;
        let esito = modifica_con_riserva(&brano, |_| {
            chiamata = true;
            Ok(())
        });
        assert!(esito.is_err(), "la riserva non ci stava");
        assert!(!chiamata, "la modifica non doveva nemmeno partire");
        assert_eq!(std::fs::read(&brano).expect("letto"), b"intatto");
        assert_eq!(nomi(dir.path()).len(), 1, "nessun avanzo");
    }

    /// Un file di sola lettura rifiuta la scrittura da sé, come sempre; e
    /// siccome non è stato toccato, non c'è niente da rimettere e la riserva —
    /// che dall'originale ha ereditato la sola lettura — se ne va lo stesso.
    #[test]
    fn un_file_di_sola_lettura_resta_com_era_e_senza_riserve() {
        let dir = tempfile::tempdir().expect("cartella");
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"intoccabile").expect("brano");
        let mut permessi = std::fs::metadata(&brano).expect("permessi").permissions();
        permessi.set_readonly(true);
        std::fs::set_permissions(&brano, permessi.clone()).expect("sola lettura");

        let esito = modifica_con_riserva(&brano, |vero| {
            std::fs::write(vero, b"no").map_err(|err| io_error(&vero.display().to_string(), &err))
        });
        assert_eq!(
            esito.map_err(|e| e.code().kind().code().to_owned()),
            Err("fs.permissionDenied".to_owned())
        );
        assert_eq!(std::fs::read(&brano).expect("letto"), b"intoccabile");
        assert_eq!(nomi(dir.path()), ["brano.mp3"], "nessuna riserva rimasta");

        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "è un file di prova, e va solo tolto"
        )]
        permessi.set_readonly(false);
        let _ = std::fs::set_permissions(&brano, permessi);
    }

    /// Se nemmeno il ripristino riesce, la riserva **resta**, intera, e
    /// l'errore dice dove sta.
    ///
    /// Qui la modifica mette al posto del file una cartella: non ci si ricopia
    /// dentro, e non ci si rinomina sopra.
    #[test]
    fn se_il_ripristino_non_riesce_la_riserva_resta() {
        let dir = tempfile::tempdir().expect("cartella");
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, audio()).expect("brano");

        let err = modifica_con_riserva(&brano, |vero| {
            std::fs::remove_file(vero).expect("tolto");
            std::fs::create_dir(vero).expect("una cartella al suo posto");
            Err(AppError::new(ErrorCode::InternalAborted {
                what: Some("prova".to_owned()),
            }))
        })
        .expect_err("il ripristino non può riuscire");

        assert!(riserva_rimasta(&err), "l'errore lo dice: {err}");
        let rimaste = riserve(dir.path());
        let riserva = rimaste.first().expect("la riserva è rimasta");
        assert_eq!(std::fs::read(riserva).expect("riserva"), audio());
        assert!(
            err.cause()
                .is_some_and(|causa| causa.contains(&riserva.display().to_string())),
            "e l'errore dice dove: {err}"
        );
        let _ = std::fs::remove_dir(&brano);
    }
}
