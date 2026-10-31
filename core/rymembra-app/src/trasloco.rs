//! Il trasloco dei dati della versione precedente nella casa di Rymembra.
//!
//! Chi aveva già installato la versione di prima ha tutto dentro la sua
//! cartella dati: il database con il suo `-wal`, le copertine, le skin, le
//! bozze, le copie del profilo, il diario. Cambiare nome al programma cambia
//! la cartella in cui il programma cerca (`%APPDATA%\<identificativo>`), e se
//! non facesse niente l'utente aprirebbe Rymembra e ritroverebbe una libreria
//! vuota, con tutta la sua nell'altra cartella. Questo modulo porta i dati
//! dalla cartella vecchia alla nuova, una volta, e poi rinomina i file che
//! portano ancora il nome di prima.
//!
//! # Le tre parti, e perché sono separate
//!
//! * **Scegliere la cartella** ([`scegli_cartella`]) e **calcolare i percorsi**
//!   ([`percorsi_per`]) sono funzioni pure: guardano le due variabili d'ambiente
//!   e la cartella di serie, non toccano il disco. Si provano come chiamate di
//!   funzione, e chi le usa può farlo senza aver ancora deciso di muovere
//!   qualcosa.
//! * **Traslocare** ([`trasloca`]) è l'unica parte che scrive sul disco. Sposta
//!   la cartella solo se è quella di serie e solo se quella di prima esiste, poi
//!   rinomina sul posto i file che hanno ancora il nome vecchio, **anche** nelle
//!   cartelle scelte a mano: un'istanza di prova con la sua variabile
//!   d'ambiente ha gli stessi `.db` di prima, e non c'è ragione di lasciarglieli.
//! * **Raccontare** ([`righe_di_diario`], [`messaggio_di_rifiuto`]) trasforma
//!   l'esito in testo: righe per il diario di bordo, un messaggio per l'utente
//!   quando il trasloco non può andare avanti.
//!
//! # Le regole che non si negoziano
//!
//! 1. **Mai sovrascrivere.** Una voce si sposta o si rinomina solo se il nome
//!    di destinazione è libero. Se non lo è, si registra un conflitto e si va
//!    avanti: i dati di Rymembra vincono e quelli di prima restano dov'erano.
//! 2. **Mai cancellare.** L'unica cartella che si rimuove è una cartella vuota,
//!    quella nuova creata dal solo avvio o quella vecchia svuotata dallo
//!    spostamento voce per voce.
//! 3. **Il database è l'ultimo.** `-wal` e `-shm` si rinominano prima, il file
//!    `.db` per ultimo: finché esiste il nome di prima, nessun codice nuovo ha
//!    aperto quel database, e un'esecuzione interrotta a metà riprende dal punto
//!    giusto la volta dopo.
//! 4. **Se la versione di prima è aperta, non si tocca niente.** Un database
//!    SQLite aperto da un altro processo non si sposta a metà: [`trasloca`]
//!    sonda i file prima di muoverli e, se qualcuno li tiene, risponde
//!    [`Errore::InUso`] senza aver cambiato nulla.
//! 5. **Un guasto fatale è solo quello sul database o sulla cartella.** Una skin
//!    o un log che non si lasciano rinominare finiscono in
//!    [`Rapporto::saltati`] e si riprovano all'avvio dopo; non fermano
//!    l'applicazione.
//!
//! Nel database non ci sono percorsi assoluti dentro la cartella dati (le
//! copertine stanno per impronta, le skin e le bozze per identificativo), quindi
//! spostare la cartella non richiede di riscrivere nessuna riga.
//!
//! # Come si sposta, caso per caso
//!
//! Solo se la scelta è la cartella di serie e quella di prima esiste:
//!
//! * **Nella nuova c'è già `rymembra.db`**: non si tocca niente. Rymembra ha già
//!   una libreria sua, che vince; la cartella di prima resta com'è e se ne
//!   registra il conflitto.
//! * **La nuova non c'è, o è vuota**: si sposta la cartella intera con un
//!   `rename`, che sullo stesso volume è atomico: o c'è la vecchia o c'è la
//!   nuova, mai una via di mezzo. Qualche tentativo, perché un antivirus o
//!   Esplora risorse tengono una cartella per un attimo; se non basta, si
//!   ripiega sullo spostamento voce per voce.
//! * **La nuova esiste e ha qualcosa dentro**: spostamento voce per voce. Una
//!   voce passa solo se il nome di destinazione è libero, le cartelle presenti
//!   da entrambe le parti si fondono ricorrendo, le altre coincidenze sono
//!   conflitti. Il database e i suoi `-wal` e `-shm` passano **per ultimi, e
//!   insieme**: i tre nomi sono una unità sola, e se anche una sola delle tre
//!   destinazioni è già occupata non ne passa nessuna (un `.db` accanto al
//!   giornale di un altro è peggio di un database che aspetta). Passano nell'ordine
//!   `-wal`, `-shm`, `.db`, e se uno spostamento fallisce quelli già fatti
//!   tornano dov'erano. Se qualcosa si inceppa prima, la libreria è ancora tutta
//!   dov'era e il giro dopo riprende da lì. Qualunque guasto in questa fase è
//!   fatale.
//!
//! Prima di muovere qualunque cosa si **sonda** il database di prima: aprirlo in
//! esclusiva, su Windows, riesce solo se nessun altro processo lo tiene, e lo
//! tiene chiunque l'abbia aperto. Senza la sonda si sposterebbe un database
//! sotto i piedi della versione di prima, che ha una connessione viva e
//! scritture nel giornale non ancora riportate nel file.
//!
//! Poi, nella cartella nuova, le rinomine sul posto: `-wal`, `-shm` e per ultimo
//! `.db`; le copie `schema-NNN.bak`; il diario; le skin; le copie del profilo.
//! Quel che sta sotto `copertine/`, `skin/bozze/`, `skin/istantanee/` e
//! `procura/` non cambia nome e non si guarda nemmeno.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use rymembra_domain::eredita;

use crate::db;

/// Il file del database di adesso, accanto ai suoi `-wal` e `-shm`.
///
/// Il nome di prima è [`eredita::NOME_DATABASE`]. Questo è il nome con cui lo
/// apre l'applicazione (`src-tauri/src/stato.rs`).
const NOME_DATABASE: &str = "rymembra.db";

/// La parte fissa del nome del diario di adesso: `rymembra.log`,
/// `rymembra.1.log`. Quella di prima è [`eredita::STEM_DIARIO`].
const STEM_DIARIO: &str = "rymembra";

/// L'estensione dei pacchetti skin di adesso, senza il punto. Quella di prima è
/// [`eredita::ESTENSIONE_SKIN`].
const ESTENSIONE_SKIN: &str = "ryskin";

/// Dove stanno il diario e le skin dentro la cartella dati.
const CARTELLA_DIARIO: &str = "diario";
/// Dove stanno i pacchetti skin installati. Le bozze e le istantanee dello Studio
/// vivono in sottocartelle, che qui non si guardano.
const CARTELLA_SKIN: &str = "skin";

/// Come cominciano le copie di sicurezza del profilo (`prima-<ms>.<estensione>`).
/// La cartella è [`crate::profilo::CARTELLA_COPIE`], l'estensione di adesso
/// [`crate::profilo::archivio::ESTENSIONE`].
const PREFISSO_COPIA: &str = "prima-";

/// Quanti tentativi per una cosa che può essere occupata per un attimo.
const TENTATIVI: u32 = 3;

/// La cartella dati scelta per questa esecuzione, e come ci si è arrivati.
///
/// Esce da [`scegli_cartella`]. I due indicatori non sono decorazione: dicono
/// se la cartella è quella di serie (e quindi se ha senso cercarvi accanto
/// quella di prima) e se l'ha dettata una variabile d'ambiente di prima.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scelta {
    /// La cartella in cui vivono i dati di questa esecuzione.
    pub cartella: PathBuf,
    /// L'ha scelta una variabile d'ambiente (quella nuova o quella di prima),
    /// invece di essere la cartella di serie.
    pub personalizzata: bool,
    /// A dettarla è stata la variabile d'ambiente **di prima**, perché quella
    /// nuova non c'era o era vuota. Serve a chi vuole dirlo nel diario.
    pub da_variabile_precedente: bool,
}

/// Sceglie la cartella dei dati fra le due variabili d'ambiente e quella di
/// serie.
///
/// La precedenza è questa:
///
/// 1. la variabile nuova (`RYMEMBRA_DATI`), se c'è e **non è vuota**;
/// 2. altrimenti la variabile di prima ([`eredita::VARIABILE_DATI`]), se c'è e
///    non è vuota: chi la teneva impostata, di solito per separare un'istanza
///    di prova da quella vera, se la vede rispettata;
/// 3. altrimenti `predefinita`.
///
/// Una variabile impostata ma vuota si ignora: è quel che succede quando la si
/// «toglie» da una shell senza toglierla davvero, e non vuol dire «usa la
/// cartella corrente».
#[must_use]
pub fn scegli_cartella(
    nuova: Option<OsString>,
    vecchia: Option<OsString>,
    predefinita: PathBuf,
) -> Scelta {
    if let Some(valore) = nuova.filter(|valore| !valore.is_empty()) {
        return Scelta {
            cartella: PathBuf::from(valore),
            personalizzata: true,
            da_variabile_precedente: false,
        };
    }
    if let Some(valore) = vecchia.filter(|valore| !valore.is_empty()) {
        return Scelta {
            cartella: PathBuf::from(valore),
            personalizzata: true,
            da_variabile_precedente: true,
        };
    }
    Scelta {
        cartella: predefinita,
        personalizzata: false,
        da_variabile_precedente: false,
    }
}

/// Le due cartelle fra cui [`trasloca`] lavora.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Percorsi {
    /// La cartella dei dati di questa esecuzione, quella che il programma
    /// userà: dove i dati devono arrivare.
    pub nuova: PathBuf,
    /// La cartella della versione precedente, se ha senso cercarla: solo per la
    /// cartella di serie, dove le due stanno fianco a fianco. Con una cartella
    /// scelta a mano non c'è niente da spostare, e qui c'è `None`.
    pub precedente: Option<PathBuf>,
}

/// Calcola le cartelle del trasloco a partire dalla scelta.
///
/// La precedente è la **sorella** della nuova, con l'identificativo di prima
/// ([`eredita::IDENTIFICATORE`]) al posto dell'ultimo componente, e c'è solo se
/// la scelta non è personalizzata: chi ha indicato una cartella a mano sa cosa
/// vuole, e non si va a prendere i dati di un'altra. Se per un caso la sorella
/// coincidesse con la nuova, la precedente non c'è: spostare una cartella su se
/// stessa non è un trasloco.
#[must_use]
pub fn percorsi_per(scelta: &Scelta) -> Percorsi {
    let precedente = if scelta.personalizzata {
        None
    } else {
        let sorella = scelta.cartella.with_file_name(eredita::IDENTIFICATORE);
        (sorella != scelta.cartella).then_some(sorella)
    };
    Percorsi {
        nuova: scelta.cartella.clone(),
        precedente,
    }
}

/// Che cosa ha fatto [`trasloca`], voce per voce.
///
/// Serve a due cose: scrivere nel diario di bordo cosa è successo ([`righe_di_diario`])
/// e provare nei test che il trasloco ha fatto esattamente quel che doveva. I
/// nomi sono percorsi relativi alla cartella nuova, o descrizioni brevi quando
/// una voce non ha un nome proprio. Un trasloco che non ha niente da fare
/// restituisce il rapporto vuoto (`Default`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rapporto {
    /// La cartella di prima è stata portata nella nuova (spostata intera o
    /// fusa voce per voce). `false` se non c'era niente da portare.
    pub spostata: bool,
    /// Le voci rinominate sul posto, dal nome di prima a quello di adesso.
    pub rinominati: Vec<String>,
    /// Le voci che non si sono potute spostare o rinominare perché il nome di
    /// destinazione era già occupato. Non è un errore: sono rimaste dov'erano,
    /// e quelle di Rymembra hanno vinto.
    pub conflitti: Vec<String>,
    /// Le voci secondarie che non si sono lasciate toccare (un file aperto, un
    /// permesso negato). Si riprova all'avvio dopo; non fermano niente.
    pub saltati: Vec<String>,
}

/// Perché il trasloco non è potuto andare avanti.
///
/// Sono i due guasti **fatali**: chi li riceve non deve aprire il database né
/// creare niente nella cartella nuova, e deve dire all'utente cosa fare
/// ([`messaggio_di_rifiuto`]). Un guasto su una voce secondaria non passa di
/// qui: finisce in [`Rapporto::saltati`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Errore {
    /// Un altro processo tiene i file del database di prima: spostarli adesso
    /// lo rovinerebbe. Di solito è la versione di prima ancora aperta, ma può
    /// essere qualunque programma che li tenga, come un antivirus o un
    /// programma di backup che li sta leggendo.
    ///
    /// Quasi sempre lo scopre la sonda, prima di muovere qualunque cosa. Se il
    /// file viene occupato dopo la sonda, qualche voce può essere già passata:
    /// mai cancellata, e il giro dopo riprende da dov'era arrivato.
    InUso {
        /// Il file o la cartella che un altro processo tiene occupati.
        percorso: PathBuf,
    },
    /// Un'operazione sul disco è fallita e non si può ripiegare.
    Io {
        /// Dove è fallita.
        percorso: PathBuf,
        /// Perché, nelle parole del sistema.
        causa: String,
    },
}

impl fmt::Display for Errore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InUso { percorso } => write!(
                f,
                "i dati della versione precedente sono in uso da un altro programma: {}",
                percorso.display()
            ),
            Self::Io { percorso, causa } => write!(
                f,
                "trasloco dei dati della versione precedente non riuscito su {}: {causa}",
                percorso.display()
            ),
        }
    }
}

impl std::error::Error for Errore {}

/// Porta i dati della versione precedente nella cartella di questa esecuzione.
///
/// Fa, nell'ordine:
///
/// 1. se `precedente` c'è ed esiste, **sonda** i file del database (`.db`,
///    `-wal`, `-shm`) per capire se la versione di prima è ancora aperta; se lo
///    è risponde [`Errore::InUso`] senza aver toccato niente;
/// 2. sposta la cartella di prima in quella nuova: per intero se la nuova non
///    esiste (o è vuota), voce per voce se c'è già qualcosa dentro; se nella
///    nuova esiste già il database di Rymembra non tocca niente e registra un
///    conflitto;
/// 3. rinomina sul posto, nella cartella nuova, i file che portano ancora il
///    nome di prima: database e copie di sicurezza, diario, skin, copie del
///    profilo. Lo fa **a ogni avvio** e anche per le cartelle scelte a mano, ma
///    ogni passo agisce solo se il nome vecchio c'è e quello nuovo no.
///
/// È idempotente: una seconda esecuzione sullo stesso disco non fa niente e
/// restituisce il rapporto vuoto. Non sovrascrive e non cancella mai dati.
///
/// # Errors
///
/// [`Errore::InUso`] se la versione di prima tiene aperto il database;
/// [`Errore::Io`] se la cartella o il database non si lasciano portare.
pub fn trasloca(percorsi: &Percorsi) -> Result<Rapporto, Errore> {
    trasloca_con(percorsi, &Attese::DI_SERIE)
}

/// [`trasloca`] con le pause scelte da chi chiama: le prove non aspettano un
/// secondo e mezzo per ogni file occupato.
fn trasloca_con(percorsi: &Percorsi, attese: &Attese) -> Result<Rapporto, Errore> {
    let mut rapporto = Rapporto::default();
    if let Some(precedente) = &percorsi.precedente {
        porta_la_cartella(precedente, &percorsi.nuova, attese, &mut rapporto)?;
    }
    rinomina_sul_posto(&percorsi.nuova, attese, &mut rapporto)?;
    Ok(rapporto)
}

// ── le attese, i tentativi, le domande al disco ─────────────────────────────

/// Le pause fra un tentativo e l'altro.
///
/// Sono due numeri a parte e non una costante, per due ragioni: sono due
/// attese di natura diversa (una per capire se qualcuno ha davvero aperto il
/// database, l'altra per lasciar passare un antivirus), e le prove devono
/// poterle azzerare.
#[derive(Debug, Clone, Copy)]
struct Attese {
    /// Fra due prove di apertura in esclusiva dei file del database. Tre prove
    /// in un secondo e mezzo: chi ha davvero Aether aperto non lo chiude in
    /// quel tempo, e chi ha un antivirus che ha appena guardato il file sì.
    sonda: Duration,
    /// Fra due tentativi di rinominare una cartella o un file del database.
    /// Tre tentativi in circa seicento millisecondi.
    rinomina: Duration,
}

impl Attese {
    /// Le attese vere.
    const DI_SERIE: Self = Self {
        sonda: Duration::from_millis(750),
        rinomina: Duration::from_millis(300),
    };

    /// Nessuna attesa, per le prove.
    #[cfg(test)]
    const NESSUNA: Self = Self {
        sonda: Duration::ZERO,
        rinomina: Duration::ZERO,
    };
}

/// Prova `TENTATIVI` volte, con `pausa` fra l'una e l'altra, e restituisce
/// l'esito dell'ultima.
fn con_tentativi<T>(pausa: Duration, mut prova: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut esito = prova();
    for _ in 1..TENTATIVI {
        if esito.is_ok() {
            break;
        }
        thread::sleep(pausa);
        esito = prova();
    }
    esito
}

/// C'è **qualcosa** a quel nome?
///
/// Non segue i collegamenti simbolici (un collegamento rotto occupa il nome
/// lo stesso) e, nel dubbio, risponde sì: se il disco non dice «non esiste» non
/// si è sicuri che sia libero, e il trasloco non sovrascrive mai.
fn esiste(percorso: &Path) -> bool {
    match fs::symlink_metadata(percorso) {
        Ok(_) => true,
        Err(err) => err.kind() != io::ErrorKind::NotFound,
    }
}

/// È una cartella vera (non un collegamento a una cartella)?
fn e_cartella_vera(percorso: &Path) -> bool {
    fs::symlink_metadata(percorso).is_ok_and(|dati| dati.is_dir())
}

/// La cartella non ha niente dentro.
///
/// Se non si lascia leggere — o il suo elenco si interrompe alla prima voce —
/// **non** è vuota: nel dubbio si suppone che contenga qualcosa, e il trasloco
/// non sovrascrive. È la regola «mai sovrascrivere» del modulo, vista da qui.
fn e_vuota(cartella: &Path) -> bool {
    fs::read_dir(cartella).is_ok_and(|mut voci| voci.next().is_none())
}

/// Il percorso com'è scritto nel rapporto: relativo, con `/`.
fn mostra(percorso: &Path) -> String {
    percorso
        .components()
        .map(|parte| parte.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `percorso` relativo a `radice`, come testo per il rapporto.
fn relativo(radice: &Path, percorso: &Path) -> String {
    mostra(percorso.strip_prefix(radice).unwrap_or(percorso))
}

/// Il database di prima o di adesso, con i due file che gli stanno accanto, **nell'ordine
/// in cui si spostano e si rinominano**: il `-wal`, il `-shm` e per ultimo il `.db`.
///
/// L'ordine è l'invariante del trasloco: finché esiste il nome di prima, nessun
/// codice nuovo ha aperto quel database, e una rinomina interrotta si riprende
/// da dove s'era fermata. Rinominando il `.db` per primo, un'interruzione
/// lascerebbe un database nuovo con il giornale ancora sotto il nome vecchio, e
/// SQLite lo aprirebbe senza le ultime scritture.
fn file_del_database(cartella: &Path, nome: &str) -> [PathBuf; 3] {
    [
        cartella.join(format!("{nome}-wal")),
        cartella.join(format!("{nome}-shm")),
        cartella.join(nome),
    ]
}

/// Questo percorso è uno dei tre file del database di prima?
fn e_del_database_di_prima(percorso: &Path) -> bool {
    percorso
        .file_name()
        .and_then(OsStr::to_str)
        .and_then(|nome| nome.strip_prefix(eredita::NOME_DATABASE))
        .is_some_and(|resto| matches!(resto, "" | "-wal" | "-shm"))
}

/// Apre il file in lettura e scrittura **senza condividerlo con nessuno**, e lo
/// richiude subito.
///
/// Su Windows riesce solo se nessun altro processo ha il file aperto, qualunque
/// sia il modo in cui lo tiene: SQLite lo tiene aperto per tutta la vita della
/// connessione. Non scrive niente e non cambia la data del file. Dove
/// `share_mode` non esiste la sonda non c'è: lì un file aperto si sposta lo
/// stesso, e quel che succede dopo lo decide il sistema.
#[cfg(windows)]
fn apri_in_esclusiva(percorso: &Path) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt as _;
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(percorso)
        .map(|_| ())
}

/// Dove non c'è una condivisione da negare la sonda non ha niente da chiedere.
#[cfg(not(windows))]
fn apri_in_esclusiva(_percorso: &Path) -> io::Result<()> {
    Ok(())
}

/// L'errore è «un altro processo tiene il file»?
///
/// `ERROR_SHARING_VIOLATION` (32) e `ERROR_LOCK_VIOLATION` (33): il primo è il
/// file aperto senza condivisione, il secondo un intervallo di byte bloccato,
/// che è come SQLite protegge il database.
fn e_violazione_di_condivisione(err: &io::Error) -> bool {
    cfg!(windows) && matches!(err.raw_os_error(), Some(32 | 33))
}

/// Un guasto del disco su `percorso`, come errore del trasloco.
///
/// Se è un file occupato **e** è uno dei tre del database di prima, è la
/// versione di prima che è tornata ad aprirlo (dopo la sonda, o in una cartella
/// scelta a mano), e il messaggio giusto è quello del programma aperto.
fn guasto(percorso: &Path, err: &io::Error) -> Errore {
    if e_violazione_di_condivisione(err) && e_del_database_di_prima(percorso) {
        Errore::InUso {
            percorso: percorso.to_path_buf(),
        }
    } else {
        Errore::Io {
            percorso: percorso.to_path_buf(),
            causa: err.to_string(),
        }
    }
}

/// Sposta in fila le coppie `(da, a)`, nell'ordine dato, **come una unità sola**:
/// o passano tutte, o nessuna.
///
/// Serve ai tre file del database, che non devono mai trovarsi divisi: un `-wal`
/// col nome nuovo e il `.db` ancora col vecchio è un giornale che la versione di
/// prima potrebbe riaprire senza il suo database, o viceversa. Ogni spostamento
/// ha i suoi tentativi; se nemmeno così uno passa, quelli già fatti si rimettono
/// dov'erano, dall'ultimo al primo, e l'errore è quello dello spostamento
/// fallito. Un nome di partenza che nel frattempo è stato occupato non si
/// sovrascrive: quel file resta dov'è.
///
/// Le destinazioni le controlla chi chiama: qui si sposta e basta.
fn muovi_in_fila(coppie: &[(&Path, &Path)], attese: &Attese) -> Result<(), Errore> {
    let mut fatte: Vec<(&Path, &Path)> = Vec::new();
    for &(da, a) in coppie {
        if let Err(err) = con_tentativi(attese.rinomina, || fs::rename(da, a)) {
            for (origine, destinazione) in fatte.into_iter().rev() {
                if !esiste(origine) {
                    let _ = fs::rename(destinazione, origine);
                }
            }
            return Err(guasto(da, &err));
        }
        fatte.push((da, a));
    }
    Ok(())
}

/// Controlla che nessun processo tenga i file del database di `cartella`.
///
/// Solo quelli che esistono; tre prove con una pausa in mezzo. Una violazione
/// di condivisione che dura è [`Errore::InUso`], qualunque altro guasto
/// [`Errore::Io`]. Non tocca niente.
fn sonda_il_database(cartella: &Path, attese: &Attese) -> Result<(), Errore> {
    for percorso in file_del_database(cartella, eredita::NOME_DATABASE) {
        if !esiste(&percorso) {
            continue;
        }
        match con_tentativi(attese.sonda, || apri_in_esclusiva(&percorso)) {
            Ok(()) => {}
            // Sparito fra il controllo e la prova: Aether l'ha chiuso e ha
            // ripulito, che è il meglio che potesse capitare.
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(guasto(&percorso, &err)),
        }
    }
    Ok(())
}

// ── spostare la cartella ────────────────────────────────────────────────────

/// Porta la cartella di prima dentro quella nuova, se c'è e se serve.
///
/// I casi sono quelli del `//!` del modulo. Qui ogni guasto è fatale: chi
/// riceve l'errore non ha perso niente, perché le due strade (il `rename` e lo
/// spostamento voce per voce) tengono il database per ultimo.
fn porta_la_cartella(
    precedente: &Path,
    nuova: &Path,
    attese: &Attese,
    rapporto: &mut Rapporto,
) -> Result<(), Errore> {
    if !precedente.is_dir() {
        return Ok(());
    }
    // Una cartella dentro l'altra, o la stessa: non è un trasloco, e muoverla
    // vorrebbe dire spostare una cartella dentro se stessa. Non succede con i
    // percorsi di `percorsi_per`, che sono sorelle.
    if precedente == nuova || nuova.starts_with(precedente) || precedente.starts_with(nuova) {
        rapporto.saltati.push(format!(
            "{}: la cartella di prima e quella nuova si contengono a vicenda",
            precedente.display()
        ));
        return Ok(());
    }

    // Un file al posto della cartella nuova: non c'è dove mettere niente, e non
    // si decide da soli di toglierlo di mezzo.
    if fs::metadata(nuova).is_ok_and(|dati| !dati.is_dir()) {
        return Err(Errore::Io {
            percorso: nuova.to_path_buf(),
            causa: "esiste ma non è una cartella".to_owned(),
        });
    }

    // I dati di Rymembra vincono. Si guarda **prima** della sonda: chi ha già la
    // sua libreria nuova non deve restare senza Rymembra perché la versione di
    // prima è ancora aperta.
    if esiste(&nuova.join(NOME_DATABASE)) {
        if !e_vuota(precedente) {
            rapporto.conflitti.push(format!(
                "{}: la cartella di prima resta dov'è, perché in quella nuova c'è già {NOME_DATABASE}",
                precedente.display()
            ));
        }
        return Ok(());
    }

    sonda_il_database(precedente, attese)?;

    match fs::metadata(nuova) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            sposta_la_cartella_intera(precedente, nuova, attese, rapporto)
        }
        Ok(dati) if dati.is_dir() && e_vuota(nuova) => {
            // Una cartella vuota è quella che l'avvio ha creato da sé: si toglie
            // e al suo posto ci va l'intera. Se non si toglie, nessun danno: la
            // fusione voce per voce la riempie.
            if fs::remove_dir(nuova).is_ok() {
                sposta_la_cartella_intera(precedente, nuova, attese, rapporto)
            } else {
                fondi_le_cartelle(precedente, nuova, attese, rapporto)
            }
        }
        Ok(dati) if dati.is_dir() => fondi_le_cartelle(precedente, nuova, attese, rapporto),
        // Un file, o una cartella che non si lascia leggere: non c'è dove
        // mettere niente, e non si decide da soli di cancellarla.
        Ok(_) => Err(Errore::Io {
            percorso: nuova.to_path_buf(),
            causa: "esiste ma non è una cartella".to_owned(),
        }),
        Err(err) => Err(Errore::Io {
            percorso: nuova.to_path_buf(),
            causa: err.to_string(),
        }),
    }
}

/// La cartella di prima, intera, al posto di quella nuova che non c'è.
///
/// Qualche tentativo di `rename`; se nemmeno così, ripiega sulla fusione voce
/// per voce, che sa muovere anche quel che si può e lasciare fermo il resto.
fn sposta_la_cartella_intera(
    precedente: &Path,
    nuova: &Path,
    attese: &Attese,
    rapporto: &mut Rapporto,
) -> Result<(), Errore> {
    if con_tentativi(attese.rinomina, || fs::rename(precedente, nuova)).is_ok() {
        rapporto.spostata = true;
        return Ok(());
    }
    fs::create_dir_all(nuova).map_err(|err| guasto(nuova, &err))?;
    fondi_le_cartelle(precedente, nuova, attese, rapporto)
}

/// Sposta il contenuto di `precedente` dentro `nuova`, voce per voce.
///
/// Alla fine la cartella di prima se ne va **se è rimasta vuota**: `remove_dir`
/// rifiuta una cartella che ha ancora qualcosa dentro, ed è esattamente la
/// garanzia che serve. Un errore in quel passo non conta.
fn fondi_le_cartelle(
    precedente: &Path,
    nuova: &Path,
    attese: &Attese,
    rapporto: &mut Rapporto,
) -> Result<(), Errore> {
    Fusione { attese, rapporto }.cartella(precedente, nuova, Path::new(""), true)?;
    let _ = fs::remove_dir(precedente);
    Ok(())
}

/// Lo spostamento voce per voce.
struct Fusione<'a> {
    /// Le pause per i tentativi.
    attese: &'a Attese,
    /// Dove si scrive quel che succede.
    rapporto: &'a mut Rapporto,
}

impl Fusione<'_> {
    /// Sposta il contenuto di `da` dentro `a`.
    ///
    /// `qui` è il percorso di `da` dentro la cartella di prima, per il rapporto.
    /// Al primo livello il database passa per ultimo, e i suoi tre file insieme:
    /// vedi [`Fusione::database`].
    fn cartella(
        &mut self,
        da: &Path,
        a: &Path,
        qui: &Path,
        primo_livello: bool,
    ) -> Result<(), Errore> {
        let mut nomi: Vec<OsString> = Vec::new();
        for voce in fs::read_dir(da).map_err(|err| guasto(da, &err))? {
            nomi.push(voce.map_err(|err| guasto(da, &err))?.file_name());
        }
        if primo_livello {
            nomi = in_ordine_di_spostamento(nomi);
        } else {
            nomi.sort();
        }
        let mut database_a_posto = false;
        for nome in nomi {
            if primo_livello && e_del_database_di_prima(Path::new(&nome)) {
                // I tre nomi stanno in coda, uno dopo l'altro: al primo si porta
                // la unità intera, e per gli altri due non c'è più niente da fare.
                if !database_a_posto {
                    self.database(da, a, qui)?;
                    database_a_posto = true;
                }
                continue;
            }
            self.voce(&da.join(&nome), &a.join(&nome), &qui.join(&nome))?;
        }
        Ok(())
    }

    /// Sposta il database di prima — `-wal`, `-shm` e `.db` — come una unità sola.
    ///
    /// Una voce qualunque, se la destinazione è occupata, resta dov'è e basta. Il
    /// database no: tre file che stanno insieme non si possono dividere fra due
    /// cartelle, o SQLite apre un `.db` col giornale di un altro, o un giornale
    /// senza il suo `.db`. Perciò:
    ///
    /// * se **una qualunque** delle tre destinazioni esiste già — anche quella di
    ///   un file che di là non c'è — non ne passa nessuna, e ognuno di quelli che
    ///   ci sono si registra come conflitto;
    /// * altrimenti passano nell'ordine `-wal`, `-shm`, `.db`, e se uno fallisce
    ///   quelli già passati tornano dov'erano ([`muovi_in_fila`]): la libreria
    ///   resta intera dove stava, e il giro dopo la ritrova.
    fn database(&mut self, da: &Path, a: &Path, qui: &Path) -> Result<(), Errore> {
        let sorgenti = file_del_database(da, eredita::NOME_DATABASE);
        let destinazioni = file_del_database(a, eredita::NOME_DATABASE);
        let presenti: Vec<(&Path, &Path)> = sorgenti
            .iter()
            .zip(&destinazioni)
            .filter(|(sorgente, _)| esiste(sorgente))
            .map(|(sorgente, destinazione)| (sorgente.as_path(), destinazione.as_path()))
            .collect();
        if presenti.is_empty() {
            return Ok(());
        }
        if destinazioni.iter().any(|destinazione| esiste(destinazione)) {
            for (sorgente, _) in presenti {
                let nome = sorgente.file_name().unwrap_or_default();
                self.rapporto.conflitti.push(mostra(&qui.join(nome)));
            }
            return Ok(());
        }
        muovi_in_fila(&presenti, self.attese)?;
        self.rapporto.spostata = true;
        Ok(())
    }

    /// Sposta una voce, se il nome di destinazione è libero; la fonde se è una
    /// cartella da entrambe le parti; altrimenti è un conflitto.
    fn voce(&mut self, da: &Path, a: &Path, qui: &Path) -> Result<(), Errore> {
        if !esiste(a) {
            let Err(err) = con_tentativi(self.attese.rinomina, || fs::rename(da, a)) else {
                self.rapporto.spostata = true;
                return Ok(());
            };
            // Una cartella che non si lascia spostare intera — un file aperto
            // da qualcuno là dentro — si sposta pezzo per pezzo. Un file, o un
            // pezzo che nemmeno così si muove, è il guasto che ferma tutto: il
            // database passa per ultimo, quindi la libreria è ancora dov'era.
            if !e_cartella_vera(da) {
                return Err(guasto(da, &err));
            }
            fs::create_dir(a).map_err(|errore| guasto(a, &errore))?;
            self.cartella(da, a, qui, false)?;
            let _ = fs::remove_dir(da);
            return Ok(());
        }
        if e_cartella_vera(da) && e_cartella_vera(a) {
            self.cartella(da, a, qui, false)?;
            // Se è rimasta vuota si toglie; se no ha dei conflitti dentro, e
            // resta con loro.
            let _ = fs::remove_dir(da);
        } else {
            self.rapporto.conflitti.push(mostra(qui));
        }
        Ok(())
    }
}

/// I nomi del primo livello nell'ordine in cui si spostano: tutto il resto, e
/// poi il `-wal`, il `-shm` e il `.db` di prima.
fn in_ordine_di_spostamento(mut nomi: Vec<OsString>) -> Vec<OsString> {
    nomi.sort();
    let per_ultimi: Vec<OsString> = file_del_database(Path::new(""), eredita::NOME_DATABASE)
        .iter()
        .map(|percorso| percorso.clone().into_os_string())
        .collect();
    let (database, mut altre): (Vec<OsString>, Vec<OsString>) =
        nomi.into_iter().partition(|nome| per_ultimi.contains(nome));
    altre.extend(
        per_ultimi
            .into_iter()
            .filter(|nome| database.contains(nome)),
    );
    altre
}

// ── rinominare sul posto ────────────────────────────────────────────────────

/// Rinomina nella cartella nuova tutto quello che porta ancora il nome di prima.
///
/// Non crea niente: se la cartella non c'è, non c'è da rinominare. Il database è
/// fatale, il resto no.
fn rinomina_sul_posto(
    nuova: &Path,
    attese: &Attese,
    rapporto: &mut Rapporto,
) -> Result<(), Errore> {
    if !nuova.is_dir() {
        return Ok(());
    }
    rinomina_il_database(nuova, attese, rapporto)?;
    rinomina_le_istantanee(nuova, rapporto);
    rinomina_il_diario(nuova, rapporto);
    rinomina_le_skin(nuova, rapporto);
    rinomina_le_copie_del_profilo(nuova, rapporto);
    Ok(())
}

/// `aether.db-wal`, `aether.db-shm` e **per ultimo** `aether.db`, col nome nuovo.
///
/// # Quando non si fa niente
///
/// * il database di prima non c'è: già fatto, o mai esistito. Se ci sono i suoi
///   compagni senza di lui, sono rimasugli di cui non si sa il padrone, e dargli
///   il nome nuovo li farebbe applicare da SQLite a un database che non è il
///   loro: si lasciano dove sono e si registra;
/// * **uno qualunque** dei tre ha il nome nuovo già occupato: nessuno dei tre si
///   rinomina, perché un database col giornale di un altro è peggio di due
///   database separati. Si registra il conflitto.
///
/// # Quando si ferma
///
/// Prima di rinominare si prova che nessuno tenga i file, a meno che non siano
/// appena arrivati dalla cartella di prima, dove la sonda è già passata: serve
/// alle cartelle scelte a mano e a quelle rimaste a metà.
/// Se una rinomina fallisce dopo che altre sono riuscite, si rimettono a posto
/// quelle fatte ([`muovi_in_fila`], la stessa che sposta i tre file dalla cartella
/// di prima), per non lasciare un `-wal` col nome nuovo e il `.db` col nome
/// vecchio dove la versione di prima potrebbe riaprirlo senza il suo giornale.
fn rinomina_il_database(
    nuova: &Path,
    attese: &Attese,
    rapporto: &mut Rapporto,
) -> Result<(), Errore> {
    let vecchi = file_del_database(nuova, eredita::NOME_DATABASE);
    let nuovi = file_del_database(nuova, NOME_DATABASE);
    let presenti: Vec<(&Path, &Path)> = vecchi
        .iter()
        .zip(&nuovi)
        .filter(|(vecchio, _)| esiste(vecchio))
        .map(|(vecchio, nuovo)| (vecchio.as_path(), nuovo.as_path()))
        .collect();
    if presenti.is_empty() {
        return Ok(());
    }

    let [_, _, database] = &vecchi;
    if !esiste(database) {
        for (vecchio, _) in presenti {
            rapporto.conflitti.push(format!(
                "{}: manca {}, lasciato dov'è",
                relativo(nuova, vecchio),
                eredita::NOME_DATABASE
            ));
        }
        return Ok(());
    }

    let occupati: Vec<&(&Path, &Path)> =
        presenti.iter().filter(|(_, nuovo)| esiste(nuovo)).collect();
    if !occupati.is_empty() {
        for (vecchio, nuovo) in occupati {
            rapporto.conflitti.push(format!(
                "{} (esiste già {})",
                relativo(nuova, vecchio),
                relativo(nuova, nuovo)
            ));
        }
        return Ok(());
    }

    // Se i file sono appena arrivati qui da quella di prima, la sonda l'hanno già
    // passata: rifarla darebbe a un antivirus che li sta guardando un secondo e
    // mezzo per far credere all'utente che la versione di prima sia aperta.
    if !rapporto.spostata {
        sonda_il_database(nuova, attese)?;
    }

    muovi_in_fila(&presenti, attese)?;
    for (vecchio, nuovo) in presenti {
        rapporto.rinominati.push(format!(
            "{} -> {}",
            relativo(nuova, vecchio),
            relativo(nuova, nuovo)
        ));
    }
    Ok(())
}

/// Rinomina `da` in `a` solo se `a` è libero; altrimenti registra il conflitto.
/// Un guasto del disco non ferma niente: si annota in `saltati`, e al prossimo
/// avvio si riprova.
fn rinomina_una_voce(radice: &Path, da: &Path, a: &Path, rapporto: &mut Rapporto) {
    let (da_mostrato, a_mostrato) = (relativo(radice, da), relativo(radice, a));
    if esiste(a) {
        rapporto
            .conflitti
            .push(format!("{da_mostrato} (esiste già {a_mostrato})"));
        return;
    }
    match fs::rename(da, a) {
        Ok(()) => rapporto
            .rinominati
            .push(format!("{da_mostrato} -> {a_mostrato}")),
        Err(err) => rapporto.saltati.push(format!("{da_mostrato}: {err}")),
    }
}

/// I **file** direttamente dentro `cartella`, con il nome (se è UTF-8) e in
/// ordine di nome. Una cartella che non c'è non è un guasto; una che non si
/// legge si annota fra i saltati.
fn file_in(cartella: &Path, radice: &Path, rapporto: &mut Rapporto) -> Vec<(String, PathBuf)> {
    let voci = match fs::read_dir(cartella) {
        Ok(voci) => voci,
        Err(err) => {
            if err.kind() != io::ErrorKind::NotFound {
                rapporto
                    .saltati
                    .push(format!("{}: {err}", relativo(radice, cartella)));
            }
            return Vec::new();
        }
    };
    let mut trovati: Vec<(String, PathBuf)> = voci
        .flatten()
        .filter(|voce| voce.file_type().is_ok_and(|tipo| !tipo.is_dir()))
        .filter_map(|voce| Some((voce.file_name().into_string().ok()?, voce.path())))
        .collect();
    trovati.sort();
    trovati
}

/// `aether.db.schema-NNN.bak` → `rymembra.db.schema-NNN.bak`.
///
/// Il nome nuovo non è scritto qui: lo dà [`db::nome_istantanea`], la stessa
/// funzione con cui il database le fa e con cui la potatura le riconosce
/// (`<nome del database>.schema-`). Il nome di prima si riconosce solo se è
/// **identico** a quello che la stessa funzione avrebbe dato al database di
/// prima: una copia con un nome appena diverso non è nostra e non si tocca.
fn rinomina_le_istantanee(nuova: &Path, rapporto: &mut Rapporto) {
    let database_di_prima = nuova.join(eredita::NOME_DATABASE);
    let database_nuovo = nuova.join(NOME_DATABASE);
    for (nome, percorso) in file_in(nuova, nuova, rapporto) {
        let Some(versione) = versione_dell_istantanea(&nome) else {
            continue;
        };
        if db::nome_istantanea(&database_di_prima, versione).file_name() != Some(OsStr::new(&nome))
        {
            continue;
        }
        let destinazione = db::nome_istantanea(&database_nuovo, versione);
        rinomina_una_voce(nuova, &percorso, &destinazione, rapporto);
    }
}

/// La versione di schema di un `aether.db.schema-NNN.bak`, se lo è.
fn versione_dell_istantanea(nome: &str) -> Option<u32> {
    let cifre = nome
        .strip_prefix(eredita::NOME_DATABASE)?
        .strip_prefix(".schema-")?
        .strip_suffix(".bak")?;
    if cifre.is_empty() || !cifre.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    cifre.parse().ok()
}

/// `diario/aether.log` e `diario/aether.N.log` → `rymembra.log`, `rymembra.N.log`.
///
/// Perché la rotazione di adesso parta da una serie sola: se i file di prima
/// restassero col loro nome, il diario nuovo ruoterebbe accanto a un diario
/// vecchio che nessuno più scrive né pota.
fn rinomina_il_diario(nuova: &Path, rapporto: &mut Rapporto) {
    let cartella = nuova.join(CARTELLA_DIARIO);
    for (nome, percorso) in file_in(&cartella, nuova, rapporto) {
        let Some(nuovo) = nome_del_diario_nuovo(&nome) else {
            continue;
        };
        rinomina_una_voce(nuova, &percorso, &cartella.join(nuovo), rapporto);
    }
}

/// Il nome nuovo di un file del diario di prima, se lo è.
fn nome_del_diario_nuovo(nome: &str) -> Option<String> {
    let coda = nome
        .strip_prefix(eredita::STEM_DIARIO)?
        .strip_suffix(".log")?;
    // Niente (`aether.log`) oppure `.N` con N in cifre (`aether.2.log`).
    let valido = coda.is_empty()
        || coda
            .strip_prefix('.')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit()));
    valido.then(|| format!("{STEM_DIARIO}{coda}.log"))
}

/// `skin/<id>.aeskin` → `skin/<id>.ryskin`.
///
/// Solo i file direttamente dentro `skin/`: le bozze (`skin/bozze/`) e le
/// istantanee (`skin/istantanee/`) sono sottocartelle con i loro nomi, che non
/// cambiano.
fn rinomina_le_skin(nuova: &Path, rapporto: &mut Rapporto) {
    let cartella = nuova.join(CARTELLA_SKIN);
    let suffisso = format!(".{}", eredita::ESTENSIONE_SKIN);
    for (nome, percorso) in file_in(&cartella, nuova, rapporto) {
        let Some(id) = nome.strip_suffix(&suffisso).filter(|id| !id.is_empty()) else {
            continue;
        };
        let destinazione = cartella.join(format!("{id}.{ESTENSIONE_SKIN}"));
        rinomina_una_voce(nuova, &percorso, &destinazione, rapporto);
    }
}

/// `profilo/prima-<ms>.aeprofile` → `profilo/prima-<ms>.ryprofile`.
///
/// Sono le copie di sicurezza che l'importazione di un profilo scrive prima di
/// toccare niente. `ruota_le_copie` e `ultima_copia` riconoscono solo
/// l'estensione di adesso: con quella di prima l'annullamento non troverebbe
/// l'ultima copia.
fn rinomina_le_copie_del_profilo(nuova: &Path, rapporto: &mut Rapporto) {
    let cartella = nuova.join(crate::profilo::CARTELLA_COPIE);
    let suffisso = format!(".{}", eredita::ESTENSIONE_PROFILO);
    for (nome, percorso) in file_in(&cartella, nuova, rapporto) {
        let Some(corpo) = nome
            .strip_prefix(PREFISSO_COPIA)
            .and_then(|resto| resto.strip_suffix(&suffisso))
            .filter(|corpo| !corpo.is_empty())
        else {
            continue;
        };
        let destinazione = cartella.join(format!(
            "{PREFISSO_COPIA}{corpo}.{}",
            crate::profilo::archivio::ESTENSIONE
        ));
        rinomina_una_voce(nuova, &percorso, &destinazione, rapporto);
    }
}

/// Le righe da scrivere nel diario di bordo per un trasloco riuscito.
///
/// Una riga per ogni cosa avvenuta, tutte col marchio `[trasloco]` come le
/// altre fasi dell'avvio hanno il loro. Un rapporto vuoto non produce righe: un
/// avvio in cui non è successo niente non lascia traccia.
#[must_use]
pub fn righe_di_diario(rapporto: &Rapporto) -> Vec<String> {
    let mut righe = Vec::new();
    if rapporto.spostata {
        righe.push(
            "[trasloco] dati della versione precedente portati nella nuova cartella".to_owned(),
        );
    }
    righe.extend(
        rapporto
            .rinominati
            .iter()
            .map(|voce| format!("[trasloco] rinominato {voce}")),
    );
    righe.extend(
        rapporto
            .conflitti
            .iter()
            .map(|voce| format!("[trasloco] conflitto, lasciato com'è: {voce}")),
    );
    righe.extend(
        rapporto
            .saltati
            .iter()
            .map(|voce| format!("[trasloco] saltato, si riprova al prossimo avvio: {voce}")),
    );
    righe
}

/// Il testo da mostrare all'utente quando il trasloco non può andare avanti.
///
/// Bilingue, italiano poi inglese, separati da una riga vuota: il dialogo
/// compare prima che la finestra e le sue traduzioni esistano, quindi il testo
/// deve bastare a se stesso. Nomina il programma di prima perché l'utente
/// capisca che cosa chiudere.
///
/// Promette una cosa sola, ed è vera in ogni caso: **nessun dato è stato
/// cancellato**, e al prossimo avvio il trasloco riprende da dove si era
/// fermato. Non promette che non sia stato spostato niente: il rifiuto può
/// arrivare a metà di una fusione, quando qualche voce è già passata nella
/// cartella nuova, e dirlo sarebbe falso.
///
/// Per [`Errore::InUso`] il testo non dà per certo che sia la versione di prima:
/// l'errore nasce da un file del database che qualcuno tiene aperto, e di solito
/// è lei, ma a tenerlo può essere anche un antivirus o un programma di backup
/// che lo sta leggendo.
#[must_use]
pub fn messaggio_di_rifiuto(errore: &Errore) -> String {
    match errore {
        Errore::InUso { .. } => concat!(
            "Rymembra non è riuscito a prendere i dati di Aether: i suoi file sono in uso da un altro programma. ",
            "Di solito è Aether ancora aperto, anche solo dall'icona vicino all'orologio; ",
            "a volte è un antivirus o un programma di backup che li sta leggendo. ",
            "Chiudi Aether se è aperto, aspetta qualche secondo e riapri Rymembra: riprenderà da dove si era fermato. ",
            "Nessun dato è stato cancellato.\n\n",
            "Rymembra could not take over Aether's data: its files are in use by another program. ",
            "Usually it is Aether still running, even if only from the icon next to the clock; ",
            "sometimes it is an antivirus or a backup program that is reading them. ",
            "Close Aether if it is running, wait a few seconds and open Rymembra again: it will pick up where it left off. ",
            "No data has been deleted."
        )
        .to_owned(),
        Errore::Io { percorso, causa } => {
            let dove = percorso.display();
            format!(
                "Rymembra non è riuscito a prendere i dati di Aether: {dove} non si lascia spostare ({causa}). \
                 Nessun dato è stato cancellato. Chiudi gli altri programmi che potrebbero usare quei file \
                 e riapri Rymembra: riprenderà da dove si era fermato.\n\n\
                 Rymembra could not take over Aether's data: {dove} could not be moved ({causa}). \
                 No data has been deleted. Close any other program that may be using those files \
                 and open Rymembra again: it will pick up where it left off."
            )
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// La cartella di serie dei test: finta, e con l'ultimo componente che somiglia
    /// all'identificativo di adesso, come sul disco vero.
    fn predefinita() -> PathBuf {
        PathBuf::from("dati-finti").join("io.github.federicobaratti.rymembra")
    }

    fn os(testo: &str) -> Option<OsString> {
        Some(OsString::from(testo))
    }

    #[test]
    fn una_cartella_che_non_si_legge_non_e_vuota() {
        // Fissa il comportamento prudente che il commento di `e_vuota`
        // descriveva al contrario: nel dubbio, piena.
        let base = tempfile::tempdir().expect("cartella");
        let vuota = base.path().join("vuota");
        fs::create_dir(&vuota).expect("cartella vuota");
        assert!(e_vuota(&vuota));
        assert!(!e_vuota(&base.path().join("non-c-e")));
        fs::write(vuota.join("x"), b"x").expect("un file");
        assert!(!e_vuota(&vuota));
    }

    #[test]
    fn precedenza_delle_variabili() {
        // Nessuna delle due: la cartella di serie.
        let scelta = scegli_cartella(None, None, predefinita());
        assert_eq!(scelta.cartella, predefinita());
        assert!(!scelta.personalizzata);
        assert!(!scelta.da_variabile_precedente);

        // Solo quella nuova.
        let scelta = scegli_cartella(os("nuova"), None, predefinita());
        assert_eq!(scelta.cartella, PathBuf::from("nuova"));
        assert!(scelta.personalizzata);
        assert!(!scelta.da_variabile_precedente);

        // Solo quella di prima: rispettata, e lo dice.
        let scelta = scegli_cartella(None, os("vecchia"), predefinita());
        assert_eq!(scelta.cartella, PathBuf::from("vecchia"));
        assert!(scelta.personalizzata);
        assert!(scelta.da_variabile_precedente);

        // Entrambe: vince la nuova.
        let scelta = scegli_cartella(os("nuova"), os("vecchia"), predefinita());
        assert_eq!(scelta.cartella, PathBuf::from("nuova"));
        assert!(scelta.personalizzata);
        assert!(!scelta.da_variabile_precedente);
    }

    #[test]
    fn le_variabili_vuote_si_ignorano() {
        // Nuova vuota, vecchia piena: tocca alla vecchia.
        let scelta = scegli_cartella(os(""), os("vecchia"), predefinita());
        assert_eq!(scelta.cartella, PathBuf::from("vecchia"));
        assert!(scelta.da_variabile_precedente);

        // Nuova piena, vecchia vuota: tocca alla nuova.
        let scelta = scegli_cartella(os("nuova"), os(""), predefinita());
        assert_eq!(scelta.cartella, PathBuf::from("nuova"));
        assert!(!scelta.da_variabile_precedente);

        // Entrambe vuote: la cartella di serie, non personalizzata.
        let scelta = scegli_cartella(os(""), os(""), predefinita());
        assert_eq!(scelta.cartella, predefinita());
        assert!(!scelta.personalizzata);
        assert!(!scelta.da_variabile_precedente);
    }

    #[test]
    fn la_precedente_c_e_solo_per_la_cartella_di_serie() {
        let scelta = scegli_cartella(None, None, predefinita());
        let percorsi = percorsi_per(&scelta);
        assert_eq!(percorsi.nuova, predefinita());
        assert_eq!(
            percorsi.precedente,
            Some(PathBuf::from("dati-finti").join(eredita::IDENTIFICATORE))
        );

        // Né la variabile nuova né quella di prima portano a cercare la precedente.
        for scelta in [
            scegli_cartella(os("nuova"), None, predefinita()),
            scegli_cartella(None, os("vecchia"), predefinita()),
            scegli_cartella(os("nuova"), os("vecchia"), predefinita()),
        ] {
            let percorsi = percorsi_per(&scelta);
            assert_eq!(percorsi.nuova, scelta.cartella);
            assert_eq!(percorsi.precedente, None);
        }
    }

    #[test]
    fn la_precedente_e_la_sorella_della_nuova() {
        let scelta = scegli_cartella(None, None, predefinita());
        let percorsi = percorsi_per(&scelta);
        let precedente = percorsi.precedente.expect("precedente");
        assert_eq!(precedente.parent(), percorsi.nuova.parent());
        assert_eq!(
            precedente.file_name(),
            Some(std::ffi::OsStr::new(eredita::IDENTIFICATORE))
        );
        assert_ne!(precedente, percorsi.nuova);
    }

    #[test]
    fn una_sorella_uguale_alla_nuova_non_e_un_trasloco() {
        let stessa = PathBuf::from("dati-finti").join(eredita::IDENTIFICATORE);
        let percorsi = percorsi_per(&scegli_cartella(None, None, stessa.clone()));
        assert_eq!(percorsi.nuova, stessa);
        assert_eq!(percorsi.precedente, None);
    }

    #[test]
    fn il_rapporto_di_serie_e_vuoto() {
        let rapporto = Rapporto::default();
        assert!(!rapporto.spostata);
        assert!(rapporto.rinominati.is_empty());
        assert!(rapporto.conflitti.is_empty());
        assert!(rapporto.saltati.is_empty());
        assert!(righe_di_diario(&rapporto).is_empty());
    }

    #[test]
    fn le_righe_di_diario_raccontano_ogni_voce() {
        let rapporto = Rapporto {
            spostata: true,
            rinominati: vec!["a.db -> b.db".to_owned()],
            conflitti: vec!["skin/x".to_owned()],
            saltati: vec!["diario/y.log".to_owned()],
        };
        let righe = righe_di_diario(&rapporto);
        assert_eq!(righe.len(), 4);
        assert!(righe.iter().all(|riga| riga.starts_with("[trasloco] ")));
        assert!(righe.iter().any(|riga| riga.contains("a.db -> b.db")));
        assert!(righe.iter().any(|riga| riga.contains("skin/x")));
        assert!(righe.iter().any(|riga| riga.contains("diario/y.log")));
    }

    #[test]
    fn il_messaggio_di_rifiuto_e_bilingue_e_rassicura() {
        let in_uso = messaggio_di_rifiuto(&Errore::InUso {
            percorso: PathBuf::from("x.db"),
        });
        assert!(in_uso.contains("Rymembra non è riuscito"));
        assert!(in_uso.contains("Rymembra could not"));
        // La promessa vera, in tutte e due le lingue: niente cancellato.
        assert!(in_uso.contains("Nessun dato è stato cancellato"));
        assert!(in_uso.contains("No data has been deleted"));
        // E non quella falsa: un rifiuto a metà di una fusione ha già spostato
        // qualcosa, quindi «non toccato» non si può dire.
        assert!(!in_uso.contains("toccat"));
        assert!(!in_uso.contains("touched"));

        let io = messaggio_di_rifiuto(&Errore::Io {
            percorso: PathBuf::from("cartella-x"),
            causa: "accesso negato".to_owned(),
        });
        // Il percorso e la causa compaiono in entrambe le lingue.
        assert_eq!(io.matches("cartella-x").count(), 2);
        assert_eq!(io.matches("accesso negato").count(), 2);
        assert!(io.contains("Nessun dato è stato cancellato"));
        assert!(io.contains("No data has been deleted"));
        assert!(!io.contains("toccat"));
    }

    #[test]
    fn il_messaggio_per_il_file_in_uso_non_accusa_solo_la_versione_di_prima() {
        // L'errore nasce da un file che qualcuno tiene aperto, e non sempre è la
        // versione di prima: un antivirus o un programma di backup fa lo stesso
        // effetto. Il testo lo dice, e dice che cosa fare.
        let testo = messaggio_di_rifiuto(&Errore::InUso {
            percorso: PathBuf::from("x.db"),
        });
        let (italiano, inglese) = testo.split_once("\n\n").expect("due lingue");
        assert!(italiano.contains("icona vicino all'orologio"));
        assert!(italiano.contains("antivirus"));
        assert!(italiano.contains("backup"));
        assert!(italiano.contains("riapri Rymembra"));
        assert!(inglese.contains("icon next to the clock"));
        assert!(inglese.contains("antivirus"));
        assert!(inglese.contains("backup"));
        assert!(inglese.contains("open Rymembra again"));
        // Chiudere la versione di prima non è l'unica istruzione, né la prima
        // affermazione: non si dice «è ancora aperto» come fosse certo.
        assert!(!italiano.contains("è ancora aperto"));
        assert!(!inglese.contains("is still running."));
    }

    #[test]
    fn l_errore_si_stampa_e_si_propaga() {
        let errore = Errore::Io {
            percorso: PathBuf::from("cartella-x"),
            causa: "accesso negato".to_owned(),
        };
        let testo = errore.to_string();
        assert!(testo.contains("cartella-x"));
        assert!(testo.contains("accesso negato"));
        let come_dyn: &dyn std::error::Error = &errore;
        assert_eq!(come_dyn.to_string(), testo);
        assert!(
            Errore::InUso {
                percorso: PathBuf::from("x.db")
            }
            .to_string()
            .contains("in uso")
        );
    }

    // ── il trasloco sul disco ───────────────────────────────────────────────
    //
    // Tutte in una cartella temporanea: i percorsi sono quelli veri di
    // `percorsi_per` (due sorelle, la nuova e quella di prima), ma dentro la
    // cartella di prova e mai in `%APPDATA%`.

    use std::collections::BTreeMap;

    /// Le due cartelle di una macchina di prova: `<base>/<id nuovo>` e
    /// `<base>/<id di prima>`, calcolate dal codice vero.
    fn percorsi_di_prova(base: &Path) -> Percorsi {
        let nuova = base.join("io.github.federicobaratti.rymembra");
        percorsi_per(&scegli_cartella(None, None, nuova))
    }

    fn precedente_di(percorsi: &Percorsi) -> PathBuf {
        percorsi.precedente.clone().expect("la precedente c'è")
    }

    /// Scrive un file, creando le cartelle che servono.
    fn scrivi(radice: &Path, voce: &str, contenuto: &str) {
        let percorso = radice.join(voce);
        fs::create_dir_all(percorso.parent().expect("un padre")).expect("cartella");
        fs::write(percorso, contenuto).expect("file");
    }

    fn leggi(radice: &Path, voce: &str) -> String {
        fs::read_to_string(radice.join(voce)).expect("file leggibile")
    }

    /// L'albero com'è: percorso relativo (con `/`) → contenuto, e le cartelle
    /// con un segno. Due alberi uguali sono due cartelle indistinguibili.
    fn albero(radice: &Path) -> BTreeMap<String, String> {
        let mut voci = BTreeMap::new();
        for voce in walkdir::WalkDir::new(radice).min_depth(1) {
            let voce = voce.expect("voce");
            let nome = relativo(radice, voce.path());
            let contenuto = if voce.file_type().is_dir() {
                "<cartella>".to_owned()
            } else {
                fs::read_to_string(voce.path()).expect("contenuto")
            };
            voci.insert(nome, contenuto);
        }
        voci
    }

    /// Una cartella dati com'era sul disco di chi usava la versione di prima, o
    /// com'è quando ha già i nomi di adesso: stessa roba, nomi diversi.
    fn riempi(radice: &Path, di_prima: bool) {
        let (database, diario, skin, profilo) = if di_prima {
            (
                eredita::NOME_DATABASE,
                eredita::STEM_DIARIO,
                eredita::ESTENSIONE_SKIN,
                eredita::ESTENSIONE_PROFILO,
            )
        } else {
            (NOME_DATABASE, STEM_DIARIO, ESTENSIONE_SKIN, "ryprofile")
        };
        scrivi(radice, database, "DB");
        scrivi(radice, &format!("{database}-wal"), "WAL");
        scrivi(radice, &format!("{database}-shm"), "SHM");
        scrivi(radice, &format!("{database}.schema-029.bak"), "BAK29");
        scrivi(radice, &format!("{database}.schema-030.bak"), "BAK30");
        scrivi(radice, &format!("diario/{diario}.log"), "LOG");
        scrivi(radice, &format!("diario/{diario}.1.log"), "LOG1");
        scrivi(radice, &format!("skin/notte.{skin}"), "SKIN");
        scrivi(
            radice,
            &format!("profilo/prima-1700000000000.{profilo}"),
            "PROFILO",
        );
        // Quel che non cambia nome, mai.
        scrivi(radice, "copertine/ab/abcdef.jpg", "JPG");
        scrivi(radice, "skin/bozze/b1.json", "BOZZA");
        scrivi(radice, "skin/istantanee/i1.json", "ISTANTANEA");
        scrivi(radice, "procura/x.txt", "PROCURA");
    }

    /// Esegue il trasloco senza attese.
    fn vai(percorsi: &Percorsi) -> Result<Rapporto, Errore> {
        trasloca_con(percorsi, &Attese::NESSUNA)
    }

    /// Come [`vai`], per una cartella scelta a mano (nessuna precedente).
    fn vai_sul_posto(nuova: &Path) -> Result<Rapporto, Errore> {
        vai(&Percorsi {
            nuova: nuova.to_path_buf(),
            precedente: None,
        })
    }

    fn ordinati(mut voci: Vec<String>) -> Vec<String> {
        voci.sort();
        voci
    }

    /// Le rinomine di un trasloco completo della cartella `riempi(.., true)`.
    fn rinomine_attese() -> Vec<String> {
        let vecchio = eredita::NOME_DATABASE;
        let log = eredita::STEM_DIARIO;
        ordinati(vec![
            format!("{vecchio}-wal -> rymembra.db-wal"),
            format!("{vecchio}-shm -> rymembra.db-shm"),
            format!("{vecchio} -> rymembra.db"),
            format!("{vecchio}.schema-029.bak -> rymembra.db.schema-029.bak"),
            format!("{vecchio}.schema-030.bak -> rymembra.db.schema-030.bak"),
            format!("diario/{log}.log -> diario/rymembra.log"),
            format!("diario/{log}.1.log -> diario/rymembra.1.log"),
            format!(
                "skin/notte.{} -> skin/notte.ryskin",
                eredita::ESTENSIONE_SKIN
            ),
            format!(
                "profilo/prima-1700000000000.{} -> profilo/prima-1700000000000.ryprofile",
                eredita::ESTENSIONE_PROFILO
            ),
        ])
    }

    #[test]
    fn senza_cartella_precedente_non_fa_niente() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let nuova = base.path().join("nuova");

        // Né la precedente né la nuova: il trasloco non crea niente.
        let rapporto = vai(&Percorsi {
            nuova: nuova.clone(),
            precedente: Some(base.path().join("non-c-e")),
        })
        .expect("niente da fare");
        assert_eq!(rapporto, Rapporto::default());
        assert!(!nuova.exists());

        // Nessuna precedente da cercare: lo stesso.
        let rapporto = vai_sul_posto(&nuova).expect("niente da fare");
        assert_eq!(rapporto, Rapporto::default());
        assert!(!nuova.exists());
        assert!(righe_di_diario(&rapporto).is_empty());

        // Una nuova già a posto e senza precedente: nemmeno la tocca.
        riempi(&nuova, false);
        let prima = albero(base.path());
        assert_eq!(
            vai_sul_posto(&nuova).expect("niente da fare"),
            Rapporto::default()
        );
        assert_eq!(albero(base.path()), prima);
    }

    #[test]
    fn sposta_tutto_quando_la_nuova_non_esiste() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        let precedente = precedente_di(&percorsi);
        riempi(&precedente, true);

        let rapporto = vai(&percorsi).expect("trasloco");

        assert!(rapporto.spostata);
        assert!(rapporto.conflitti.is_empty(), "{:?}", rapporto.conflitti);
        assert!(rapporto.saltati.is_empty(), "{:?}", rapporto.saltati);
        assert_eq!(ordinati(rapporto.rinominati.clone()), rinomine_attese());
        // Il database per ultimo, anche nel rapporto: il `-wal` e il `-shm`
        // prima, il `.db` dopo.
        let posizione = |fine: &str| {
            rapporto
                .rinominati
                .iter()
                .position(|voce| voce.ends_with(fine))
                .expect("rinomina presente")
        };
        assert!(posizione("-> rymembra.db-wal") < posizione("-> rymembra.db-shm"));
        assert!(posizione("-> rymembra.db-shm") < posizione("-> rymembra.db"));

        // La cartella di prima non c'è più, e la nuova è quella di prima con i
        // nomi di adesso: identica a una costruita da zero con i nomi nuovi.
        assert!(!precedente.exists());
        let atteso = tempfile::tempdir().expect("cartella temporanea");
        riempi(atteso.path(), false);
        assert_eq!(albero(&percorsi.nuova), albero(atteso.path()));
        // I contenuti viaggiano interi: il giornale in particolare.
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db"), "DB");
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db-wal"), "WAL");

        // E il diario racconta tutto.
        let righe = righe_di_diario(&rapporto);
        assert_eq!(righe.len(), 1 + rapporto.rinominati.len());
    }

    #[test]
    fn e_idempotente() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        riempi(&precedente_di(&percorsi), true);

        vai(&percorsi).expect("primo trasloco");
        let dopo_il_primo = albero(base.path());

        let secondo = vai(&percorsi).expect("secondo trasloco");
        assert_eq!(secondo, Rapporto::default(), "nessun effetto, nessuna riga");
        assert!(righe_di_diario(&secondo).is_empty());
        assert_eq!(albero(base.path()), dopo_il_primo);

        // Anche la terza: non è questione di una volta sola.
        assert_eq!(vai(&percorsi).expect("terzo"), Rapporto::default());
    }

    #[test]
    fn una_nuova_vuota_viene_rimpiazzata() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        let precedente = precedente_di(&percorsi);
        riempi(&precedente, true);
        // Quella che un avvio qualunque ha creato da sé e non ha riempito.
        fs::create_dir_all(&percorsi.nuova).expect("nuova vuota");

        let rapporto = vai(&percorsi).expect("trasloco");

        assert!(rapporto.spostata);
        assert!(rapporto.conflitti.is_empty());
        assert!(!precedente.exists());
        let atteso = tempfile::tempdir().expect("cartella temporanea");
        riempi(atteso.path(), false);
        assert_eq!(albero(&percorsi.nuova), albero(atteso.path()));
    }

    #[test]
    fn con_rymembra_db_presente_non_tocca_niente() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        let precedente = precedente_di(&percorsi);
        riempi(&precedente, true);
        // Rymembra ha già la sua libreria: il caso è «niente», non «metà».
        scrivi(&percorsi.nuova, "rymembra.db", "NUOVO");
        scrivi(&percorsi.nuova, "copertine/zz/nuova.jpg", "NUOVA");
        let prima = albero(base.path());

        let rapporto = vai(&percorsi).expect("trasloco");

        assert!(!rapporto.spostata);
        assert!(rapporto.rinominati.is_empty());
        assert!(rapporto.saltati.is_empty());
        assert_eq!(rapporto.conflitti.len(), 1, "{:?}", rapporto.conflitti);
        assert_eq!(
            albero(base.path()),
            prima,
            "né una voce spostata né una persa"
        );
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db"), "NUOVO");
        assert!(precedente.join(eredita::NOME_DATABASE).exists());

        // E il conflitto si ripete a ogni avvio, finché la cartella di prima c'è:
        // non è un errore, è un fatto che il diario ha diritto di ricordare.
        assert_eq!(vai(&percorsi).expect("secondo").conflitti.len(), 1);
    }

    #[test]
    fn fonde_se_la_nuova_ha_solo_il_diario() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        let precedente = precedente_di(&percorsi);
        riempi(&precedente, true);
        // La nuova ha già un diario suo, con un file che si chiama come quello
        // che arriverà dopo la rinomina, e uno che non c'entra.
        scrivi(&percorsi.nuova, "diario/rymembra.log", "DIARIO NUOVO");
        scrivi(&percorsi.nuova, "diario/altro.log", "ALTRO");

        let rapporto = vai(&percorsi).expect("trasloco");

        assert!(rapporto.spostata);
        assert!(!precedente.exists(), "svuotata, e allora se ne va");
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db"), "DB");
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db-wal"), "WAL");
        assert_eq!(leggi(&percorsi.nuova, "copertine/ab/abcdef.jpg"), "JPG");
        assert_eq!(leggi(&percorsi.nuova, "skin/notte.ryskin"), "SKIN");
        assert_eq!(leggi(&percorsi.nuova, "skin/bozze/b1.json"), "BOZZA");
        assert_eq!(leggi(&percorsi.nuova, "procura/x.txt"), "PROCURA");
        // Il diario nuovo è intatto; quello di prima, che avrebbe preso il suo
        // nome, resta col suo: nessuno dei due sovrascrive l'altro.
        assert_eq!(
            leggi(&percorsi.nuova, "diario/rymembra.log"),
            "DIARIO NUOVO"
        );
        assert_eq!(leggi(&percorsi.nuova, "diario/altro.log"), "ALTRO");
        assert_eq!(
            leggi(
                &percorsi.nuova,
                &format!("diario/{}.log", eredita::STEM_DIARIO)
            ),
            "LOG"
        );
        assert_eq!(leggi(&percorsi.nuova, "diario/rymembra.1.log"), "LOG1");
        assert_eq!(rapporto.conflitti.len(), 1, "{:?}", rapporto.conflitti);
        assert!(rapporto.conflitti[0].contains("diario/"));
    }

    // ── il trio del database nella fusione ──────────────────────────────────
    //
    // `-wal`, `-shm` e `.db` sono una unità: nella fusione passano tutti e tre
    // o non passa nessuno, a differenza di ogni altra voce, che se la
    // destinazione è occupata resta lì da sola.

    /// I tre nomi del database di prima, nell'ordine in cui si spostano, col
    /// contenuto che gli dà `riempi`.
    fn il_trio_di_prima() -> [(String, &'static str); 3] {
        let db = eredita::NOME_DATABASE;
        [
            (format!("{db}-wal"), "WAL"),
            (format!("{db}-shm"), "SHM"),
            (db.to_owned(), "DB"),
        ]
    }

    #[test]
    fn nella_fusione_il_trio_del_database_arriva_intero() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        let precedente = precedente_di(&percorsi);
        riempi(&precedente, true);
        // La nuova ha già qualcosa, ma non un database: è la strada della fusione.
        scrivi(&percorsi.nuova, "diario/altro.log", "ALTRO");

        let rapporto = vai(&percorsi).expect("trasloco");

        assert!(rapporto.spostata);
        assert!(rapporto.conflitti.is_empty(), "{:?}", rapporto.conflitti);
        assert!(rapporto.saltati.is_empty(), "{:?}", rapporto.saltati);
        assert!(!precedente.exists(), "svuotata, e allora se ne va");
        assert_eq!(ordinati(rapporto.rinominati), rinomine_attese());
        // I tre file ci sono, tutti, col nome di adesso e il contenuto di prima.
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db-wal"), "WAL");
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db-shm"), "SHM");
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db"), "DB");
        let atteso = tempfile::tempdir().expect("cartella temporanea");
        riempi(atteso.path(), false);
        scrivi(atteso.path(), "diario/altro.log", "ALTRO");
        assert_eq!(albero(&percorsi.nuova), albero(atteso.path()));
    }

    #[test]
    fn con_una_destinazione_occupata_nessuno_dei_tre_passa() {
        // A turno, ognuno dei tre nomi occupato nella cartella nuova da un file
        // che non è di prima. Il database di prima resta intero dov'è: spostarne
        // due su tre vorrebbe dire dividerlo fra due cartelle.
        for (occupato, _) in il_trio_di_prima() {
            let base = tempfile::tempdir().expect("cartella temporanea");
            let percorsi = percorsi_di_prova(base.path());
            let precedente = precedente_di(&percorsi);
            riempi(&precedente, true);
            scrivi(&percorsi.nuova, &occupato, "OCCUPANTE");

            let rapporto = vai(&percorsi).expect("un conflitto non è un errore");

            // Il trio è dov'era, tutto e com'era.
            for (nome, contenuto) in il_trio_di_prima() {
                assert_eq!(leggi(&precedente, &nome), contenuto, "{nome}");
            }
            // E nella nuova, sotto un nome di database, c'è soltanto l'occupante:
            // nemmeno uno dei tre, né col nome di prima né con quello di adesso.
            let nomi_del_database: Vec<String> = il_trio_di_prima()
                .into_iter()
                .map(|(nome, _)| nome)
                .chain(["rymembra.db-wal", "rymembra.db-shm", "rymembra.db"].map(str::to_owned))
                .collect();
            let in_nuova = albero(&percorsi.nuova);
            for (nome, contenuto) in &in_nuova {
                if nomi_del_database.contains(nome) {
                    assert_eq!(contenuto, "OCCUPANTE", "{nome}");
                }
            }
            // Ognuno dei tre è un conflitto registrato.
            for (nome, _) in il_trio_di_prima() {
                assert!(
                    rapporto.conflitti.contains(&nome),
                    "{nome} non è fra i conflitti: {:?}",
                    rapporto.conflitti
                );
            }
            // Il resto, che non ha di questi problemi, è passato.
            assert_eq!(leggi(&percorsi.nuova, "copertine/ab/abcdef.jpg"), "JPG");
            assert!(precedente.exists(), "il trio la tiene in piedi");
        }
    }

    #[test]
    fn un_giornale_estraneo_nella_nuova_ferma_anche_il_database_che_non_ha_il_suo() {
        // Il database di prima c'è da solo, senza giornale; nella nuova c'è un
        // `-wal` che non è il suo. Se il `.db` passasse, SQLite lo aprirebbe
        // applicandogli i frame di un altro: la destinazione occupata è quella
        // del `-wal`, anche se di qua quel file non c'è.
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        let precedente = precedente_di(&percorsi);
        scrivi(&precedente, eredita::NOME_DATABASE, "DB");
        scrivi(&precedente, "copertine/ab/abcdef.jpg", "JPG");
        let giornale = format!("{}-wal", eredita::NOME_DATABASE);
        scrivi(&percorsi.nuova, &giornale, "ESTRANEO");

        let rapporto = vai(&percorsi).expect("un conflitto non è un errore");

        assert_eq!(leggi(&precedente, eredita::NOME_DATABASE), "DB");
        assert!(!percorsi.nuova.join(eredita::NOME_DATABASE).exists());
        assert!(!percorsi.nuova.join("rymembra.db").exists());
        assert_eq!(leggi(&percorsi.nuova, &giornale), "ESTRANEO");
        assert!(
            rapporto
                .conflitti
                .contains(&eredita::NOME_DATABASE.to_owned()),
            "{:?}",
            rapporto.conflitti
        );
    }

    #[test]
    fn se_uno_spostamento_del_trio_fallisce_quelli_fatti_tornano_indietro() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let (da, a) = (base.path().join("da"), base.path().join("a"));
        scrivi(&da, "uno", "1");
        scrivi(&da, "due", "2");
        fs::create_dir_all(&a).expect("destinazione");
        // Il terzo non c'è: lo spostamento fallisce dopo i primi due.
        let coppie = [
            (da.join("uno"), a.join("uno")),
            (da.join("due"), a.join("due")),
            (da.join("tre"), a.join("tre")),
        ];
        let viste: Vec<(&Path, &Path)> = coppie
            .iter()
            .map(|(da, a)| (da.as_path(), a.as_path()))
            .collect();

        let errore = muovi_in_fila(&viste, &Attese::NESSUNA).expect_err("il terzo non c'è");

        assert!(
            matches!(&errore, Errore::Io { percorso, .. } if *percorso == da.join("tre")),
            "{errore:?}"
        );
        assert_eq!(leggi(&da, "uno"), "1", "il primo è tornato");
        assert_eq!(leggi(&da, "due"), "2", "il secondo è tornato");
        assert!(!a.join("uno").exists());
        assert!(!a.join("due").exists());

        // E se tutti e tre ci sono, passano tutti, nell'ordine dato.
        scrivi(&da, "tre", "3");
        muovi_in_fila(&viste, &Attese::NESSUNA).expect("ora ci sono tutti");
        assert_eq!(leggi(&a, "uno"), "1");
        assert_eq!(leggi(&a, "due"), "2");
        assert_eq!(leggi(&a, "tre"), "3");
        assert!(!da.join("uno").exists());
    }

    #[test]
    fn riprende_dopo_interruzione() {
        // Prima interruzione: una rinomina del database fatta a metà. Il `-wal`
        // e il `-shm` hanno già il nome nuovo, il `.db` ancora no.
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        scrivi(&percorsi.nuova, "rymembra.db-wal", "WAL");
        scrivi(&percorsi.nuova, "rymembra.db-shm", "SHM");
        scrivi(&percorsi.nuova, eredita::NOME_DATABASE, "DB");

        let rapporto = vai(&percorsi).expect("ripresa");
        assert_eq!(
            rapporto.rinominati,
            [format!("{} -> rymembra.db", eredita::NOME_DATABASE)]
        );
        assert!(rapporto.conflitti.is_empty(), "{:?}", rapporto.conflitti);
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db"), "DB");
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db-wal"), "WAL");
        assert!(!percorsi.nuova.join(eredita::NOME_DATABASE).exists());

        // Seconda interruzione: lo spostamento voce per voce è arrivato a metà.
        // La nuova ha già parte delle cose e **non** ha ancora nessun database;
        // la precedente ha il resto, database compresi (che passano per ultimi).
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        let precedente = precedente_di(&percorsi);
        scrivi(&percorsi.nuova, "copertine/ab/abcdef.jpg", "JPG");
        scrivi(
            &percorsi.nuova,
            &format!("skin/notte.{}", eredita::ESTENSIONE_SKIN),
            "SKIN",
        );
        scrivi(&precedente, eredita::NOME_DATABASE, "DB");
        scrivi(
            &precedente,
            &format!("{}-wal", eredita::NOME_DATABASE),
            "WAL",
        );
        scrivi(
            &precedente,
            &format!("profilo/prima-1.{}", eredita::ESTENSIONE_PROFILO),
            "PROFILO",
        );
        scrivi(&precedente, "copertine/cd/altra.jpg", "ALTRA");

        let rapporto = vai(&percorsi).expect("ripresa");
        assert!(rapporto.spostata);
        assert!(rapporto.conflitti.is_empty(), "{:?}", rapporto.conflitti);
        assert!(!precedente.exists());
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db"), "DB");
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db-wal"), "WAL");
        assert_eq!(leggi(&percorsi.nuova, "skin/notte.ryskin"), "SKIN");
        assert_eq!(
            leggi(&percorsi.nuova, "profilo/prima-1.ryprofile"),
            "PROFILO"
        );
        assert_eq!(leggi(&percorsi.nuova, "copertine/ab/abcdef.jpg"), "JPG");
        assert_eq!(leggi(&percorsi.nuova, "copertine/cd/altra.jpg"), "ALTRA");
    }

    #[test]
    fn non_sovrascrive_mai() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        let precedente = precedente_di(&percorsi);
        let (skin_di_prima, copia_di_prima) = (
            format!("skin/notte.{}", eredita::ESTENSIONE_SKIN),
            format!("profilo/prima-1.{}", eredita::ESTENSIONE_PROFILO),
        );

        // La nuova non ha il database, ma ha altre cose che coincidono per nome
        // con quelle di prima, con un contenuto diverso.
        scrivi(&percorsi.nuova, "copertine/ab/abcdef.jpg", "NUOVA");
        scrivi(&percorsi.nuova, "skin/notte.ryskin", "SKIN NUOVA");
        scrivi(&percorsi.nuova, "profilo/prima-1.ryprofile", "COPIA NUOVA");
        scrivi(&precedente, eredita::NOME_DATABASE, "DB");
        scrivi(&precedente, "copertine/ab/abcdef.jpg", "VECCHIA");
        scrivi(&precedente, "copertine/ab/altra.jpg", "ALTRA");
        scrivi(&precedente, &skin_di_prima, "SKIN VECCHIA");
        scrivi(&precedente, &copia_di_prima, "COPIA VECCHIA");

        let rapporto = vai(&percorsi).expect("trasloco");

        // Quel che era già della nuova è intatto.
        assert_eq!(leggi(&percorsi.nuova, "copertine/ab/abcdef.jpg"), "NUOVA");
        assert_eq!(leggi(&percorsi.nuova, "skin/notte.ryskin"), "SKIN NUOVA");
        assert_eq!(
            leggi(&percorsi.nuova, "profilo/prima-1.ryprofile"),
            "COPIA NUOVA"
        );
        // Quel che coincideva è rimasto dov'era, col suo nome e il suo
        // contenuto: la copertina nella cartella di prima, che non si è svuotata,
        // la skin e la copia nella nuova, dove sono arrivate ma non hanno potuto
        // prendere il nome di adesso.
        assert_eq!(
            leggi(&precedente, "copertine/ab/abcdef.jpg"),
            "VECCHIA",
            "la copertina in conflitto resta nella cartella di prima"
        );
        assert_eq!(leggi(&percorsi.nuova, &skin_di_prima), "SKIN VECCHIA");
        assert_eq!(leggi(&percorsi.nuova, &copia_di_prima), "COPIA VECCHIA");
        // E quel che non coincideva è passato.
        assert_eq!(leggi(&percorsi.nuova, "copertine/ab/altra.jpg"), "ALTRA");
        assert_eq!(leggi(&percorsi.nuova, "rymembra.db"), "DB");

        assert_eq!(rapporto.conflitti.len(), 3, "{:?}", rapporto.conflitti);
        assert!(rapporto.saltati.is_empty());
        assert!(precedente.exists());

        // Un secondo giro non sposta e non perde niente: il database di Rymembra
        // c'è, quindi la cartella di prima non si tocca più.
        let prima = albero(base.path());
        vai(&percorsi).expect("secondo giro");
        assert_eq!(albero(base.path()), prima);
    }

    #[test]
    fn in_una_cartella_scelta_a_mano_i_due_database_non_si_pestano() {
        // Sul posto, con tutti e due i nomi presenti: i file di prima restano
        // insieme, e quelli di adesso pure. Il giornale di uno non va all'altro.
        let base = tempfile::tempdir().expect("cartella temporanea");
        let nuova = base.path().join("mia");
        let vecchio = eredita::NOME_DATABASE;
        scrivi(&nuova, "rymembra.db", "NUOVO");
        scrivi(&nuova, vecchio, "VECCHIO");
        scrivi(&nuova, &format!("{vecchio}-wal"), "WAL VECCHIO");
        let prima = albero(base.path());

        let rapporto = vai_sul_posto(&nuova).expect("trasloco");

        assert!(rapporto.rinominati.is_empty(), "{:?}", rapporto.rinominati);
        assert_eq!(rapporto.conflitti.len(), 1);
        assert_eq!(albero(base.path()), prima);
    }

    #[test]
    fn un_wal_senza_il_suo_database_non_si_porta_dietro_al_nome_nuovo() {
        // Un giornale orfano rinominato diventerebbe il giornale del prossimo
        // `rymembra.db`, e SQLite ne applicherebbe i frame a un database che
        // non è il suo.
        let base = tempfile::tempdir().expect("cartella temporanea");
        let nuova = base.path().join("mia");
        scrivi(&nuova, &format!("{}-wal", eredita::NOME_DATABASE), "ORFANO");

        let rapporto = vai_sul_posto(&nuova).expect("trasloco");

        assert!(rapporto.rinominati.is_empty());
        assert_eq!(rapporto.conflitti.len(), 1);
        assert!(!nuova.join("rymembra.db-wal").exists());
    }

    #[test]
    fn cartella_personalizzata_rinomina_sul_posto() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        // Una cartella scelta con la variabile d'ambiente, con dentro i nomi di
        // prima; e, accanto, una cartella di prima che **non** va toccata perché
        // chi ha scelto a mano sa cosa vuole.
        let personale = base.path().join("scelta");
        let scelta = scegli_cartella(
            Some(personale.clone().into_os_string()),
            None,
            base.path().join("io.github.federicobaratti.rymembra"),
        );
        assert!(scelta.personalizzata);
        let percorsi = percorsi_per(&scelta);
        assert_eq!(percorsi.precedente, None);
        riempi(&personale, true);
        let sorella = base.path().join(eredita::IDENTIFICATORE);
        riempi(&sorella, true);
        let sorella_prima = albero(&sorella);

        let rapporto = vai(&percorsi).expect("trasloco");

        assert!(!rapporto.spostata);
        assert_eq!(ordinati(rapporto.rinominati), rinomine_attese());
        let atteso = tempfile::tempdir().expect("cartella temporanea");
        riempi(atteso.path(), false);
        assert_eq!(albero(&personale), albero(atteso.path()));
        assert_eq!(albero(&sorella), sorella_prima, "la sorella non si tocca");
    }

    #[test]
    fn le_copie_bak_seguono_nome_istantanea() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let cartella = base.path().join("dati");
        let vecchio = cartella.join(eredita::NOME_DATABASE);
        let nuovo = cartella.join("rymembra.db");
        // Quel che il database di prima ha scritto, col nome che ha dato lui.
        for versione in [12, 27, 30] {
            let copia = db::nome_istantanea(&vecchio, versione);
            scrivi(
                &cartella,
                &relativo(&cartella, &copia),
                &format!("V{versione}"),
            );
        }
        // Somiglianze che non sono copie: non si toccano.
        let estranee = [
            format!("{}.schema-xx.bak", eredita::NOME_DATABASE),
            format!("{}.schema-.bak", eredita::NOME_DATABASE),
            format!("{}.schema-030.bak.parziale", eredita::NOME_DATABASE),
            format!("{}.schema-0030.bak", eredita::NOME_DATABASE),
            format!("altro.{}.schema-030.bak", eredita::NOME_DATABASE),
        ];
        for nome in &estranee {
            scrivi(&cartella, nome, "ESTRANEA");
        }

        let rapporto = vai_sul_posto(&cartella).expect("trasloco");

        // Le tre copie hanno **esattamente** il nome che darebbe il database di
        // adesso, lo stesso su cui la potatura si basa: se le due funzioni si
        // separassero, le copie rinominate smetterebbero di essere potate.
        for versione in [12, 27, 30] {
            let attesa = db::nome_istantanea(&nuovo, versione);
            assert_eq!(
                leggi(&cartella, &relativo(&cartella, &attesa)),
                format!("V{versione}")
            );
            assert!(!db::nome_istantanea(&vecchio, versione).exists());
        }
        assert_eq!(rapporto.rinominati.len(), 3);
        for nome in &estranee {
            assert_eq!(leggi(&cartella, nome), "ESTRANEA");
        }
        assert!(rapporto.conflitti.is_empty());
    }

    #[test]
    fn i_log_e_le_copie_del_profilo_si_rinominano() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let nuova = base.path().join("dati");
        let log = eredita::STEM_DIARIO;
        let skin = eredita::ESTENSIONE_SKIN;
        let profilo = eredita::ESTENSIONE_PROFILO;
        for voce in [
            format!("diario/{log}.log"),
            format!("diario/{log}.1.log"),
            format!("diario/{log}.2.log"),
            // Non sono del diario: non si toccano.
            format!("diario/{log}.x.log"),
            format!("diario/{log}.log.bak"),
            "diario/altro.log".to_owned(),
            format!("skin/uno.{skin}"),
            format!("skin/due.punti.{skin}"),
            // Non sono skin da rinominare: un'altra estensione, e le sottocartelle.
            "skin/leggimi.txt".to_owned(),
            format!("skin/bozze/bozza.{skin}"),
            format!("skin/istantanee/foto.{skin}"),
            format!("profilo/prima-1700000000000.{profilo}"),
            format!("profilo/prima-1800000000000.{profilo}"),
            // Non è una copia di sicurezza: un profilo esportato a mano.
            format!("profilo/mio.{profilo}"),
        ] {
            scrivi(&nuova, &voce, &voce);
        }

        let rapporto = vai_sul_posto(&nuova).expect("trasloco");

        assert!(rapporto.conflitti.is_empty(), "{:?}", rapporto.conflitti);
        assert!(rapporto.saltati.is_empty(), "{:?}", rapporto.saltati);
        assert_eq!(
            rapporto.rinominati.len(),
            3 + 2 + 2,
            "{:?}",
            rapporto.rinominati
        );
        for nuovo in [
            "diario/rymembra.log",
            "diario/rymembra.1.log",
            "diario/rymembra.2.log",
            "skin/uno.ryskin",
            "skin/due.punti.ryskin",
            "profilo/prima-1700000000000.ryprofile",
            "profilo/prima-1800000000000.ryprofile",
        ] {
            assert!(nuova.join(nuovo).is_file(), "manca {nuovo}");
        }
        for intatto in [
            format!("diario/{log}.x.log"),
            format!("diario/{log}.log.bak"),
            "diario/altro.log".to_owned(),
            "skin/leggimi.txt".to_owned(),
            format!("skin/bozze/bozza.{skin}"),
            format!("skin/istantanee/foto.{skin}"),
            format!("profilo/mio.{profilo}"),
        ] {
            assert_eq!(leggi(&nuova, &intatto), intatto);
        }
        // Le copie hanno il nome che il codice del profilo sa leggere: è quel
        // che permette ad «annulla» di trovare l'ultima.
        let copia = crate::profilo::copia_di_sicurezza(&nuova, 1_700_000_000_000);
        assert!(copia.is_file(), "{}", copia.display());
        assert_eq!(
            crate::profilo::ultima_copia(&nuova),
            Some(crate::profilo::copia_di_sicurezza(
                &nuova,
                1_800_000_000_000
            ))
        );
    }

    #[test]
    fn il_database_si_sposta_per_ultimo() {
        let db = eredita::NOME_DATABASE;
        let (wal, shm) = (format!("{db}-wal"), format!("{db}-shm"));
        let nomi = ["zeta", db, "alfa", shm.as_str(), wal.as_str(), "beta"]
            .map(OsString::from)
            .to_vec();
        assert_eq!(
            in_ordine_di_spostamento(nomi),
            ["alfa", "beta", "zeta", wal.as_str(), shm.as_str(), db].map(OsString::from)
        );
        // Senza database, solo l'ordine alfabetico; con il solo `-wal`, in fondo.
        assert_eq!(
            in_ordine_di_spostamento(vec!["b".into(), "a".into()]),
            ["a", "b"].map(OsString::from)
        );
        assert_eq!(
            in_ordine_di_spostamento(vec![wal.as_str().into(), "a".into()]),
            ["a", wal.as_str()].map(OsString::from)
        );
    }

    #[test]
    fn una_nuova_che_e_un_file_e_un_errore_senza_perdite() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let percorsi = percorsi_di_prova(base.path());
        riempi(&precedente_di(&percorsi), true);
        scrivi(
            base.path(),
            "io.github.federicobaratti.rymembra",
            "sono un file",
        );
        let prima = albero(base.path());

        let errore = vai(&percorsi).expect_err("non c'è dove mettere i dati");
        assert!(matches!(errore, Errore::Io { ref percorso, .. } if *percorso == percorsi.nuova));
        assert_eq!(albero(base.path()), prima, "niente toccato");
    }

    #[test]
    fn i_percorsi_che_si_contengono_non_si_spostano() {
        let base = tempfile::tempdir().expect("cartella temporanea");
        let precedente = base.path().join("prima");
        riempi(&precedente, true);
        let prima = albero(base.path());

        let rapporto = vai(&Percorsi {
            nuova: precedente.join("dentro"),
            precedente: Some(precedente.clone()),
        })
        .expect("niente da fare");

        assert!(!rapporto.spostata);
        assert_eq!(rapporto.saltati.len(), 1);
        assert_eq!(albero(base.path()), prima);
    }

    #[cfg(windows)]
    mod windows {
        use super::*;
        use std::os::windows::fs::OpenOptionsExt as _;

        /// Il file aperto come lo apre SQLite: in lettura e scrittura,
        /// condividendolo con chi legge e scrive ma **non** con chi lo cancella o
        /// lo rinomina.
        fn aperto_come_sqlite(percorso: &Path) -> fs::File {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0b011)
                .open(percorso)
                .expect("apertura")
        }

        #[test]
        fn un_db_aperto_ferma_tutto() {
            let base = tempfile::tempdir().expect("cartella temporanea");
            let percorsi = percorsi_di_prova(base.path());
            let precedente = precedente_di(&percorsi);
            riempi(&precedente, true);
            let tenuto = aperto_come_sqlite(&precedente.join(eredita::NOME_DATABASE));
            let prima = albero(base.path());

            // Con le attese vere: tre prove, un secondo e mezzo.
            let inizio = std::time::Instant::now();
            let esito = trasloca(&percorsi);
            assert!(
                inizio.elapsed() >= Duration::from_millis(1_400),
                "le prove non hanno aspettato"
            );
            assert!(
                matches!(&esito, Err(Errore::InUso { percorso }) if percorso.ends_with(eredita::NOME_DATABASE)),
                "{esito:?}"
            );
            // Niente è stato toccato: non c'è nemmeno la cartella nuova.
            assert_eq!(albero(base.path()), prima);
            assert!(!percorsi.nuova.exists());

            // Chiuso Aether, il trasloco va.
            drop(tenuto);
            let rapporto = vai(&percorsi).expect("trasloco");
            assert!(rapporto.spostata);
            assert_eq!(leggi(&percorsi.nuova, "rymembra.db"), "DB");
        }

        #[test]
        fn con_un_file_aperto_dentro_non_si_perde_niente() {
            // Una copertina tenuta aperta da qualcuno (Esplora risorse, un
            // antivirus): la cartella intera può non lasciarsi rinominare, e
            // allora si sposta pezzo per pezzo. Comunque vada, arriva tutto.
            let base = tempfile::tempdir().expect("cartella temporanea");
            let percorsi = percorsi_di_prova(base.path());
            let precedente = precedente_di(&percorsi);
            riempi(&precedente, true);
            let tenuta =
                fs::File::open(precedente.join("copertine/ab/abcdef.jpg")).expect("apertura");

            let rapporto = vai(&percorsi).expect("trasloco");
            drop(tenuta);

            assert!(rapporto.spostata);
            assert!(rapporto.conflitti.is_empty(), "{:?}", rapporto.conflitti);
            assert!(!precedente.exists());
            let atteso = tempfile::tempdir().expect("cartella temporanea");
            riempi(atteso.path(), false);
            assert_eq!(albero(&percorsi.nuova), albero(atteso.path()));
        }

        #[test]
        fn un_db_aperto_in_una_cartella_scelta_a_mano_non_si_rinomina() {
            let base = tempfile::tempdir().expect("cartella temporanea");
            let nuova = base.path().join("mia");
            riempi(&nuova, true);
            let tenuto = aperto_come_sqlite(&nuova.join(eredita::NOME_DATABASE));
            let prima = albero(base.path());

            let esito = vai_sul_posto(&nuova);
            assert!(matches!(esito, Err(Errore::InUso { .. })), "{esito:?}");
            assert_eq!(albero(base.path()), prima);

            drop(tenuto);
            assert!(vai_sul_posto(&nuova).is_ok());
        }

        #[test]
        fn una_skin_bloccata_viene_saltata_e_si_riprova() {
            let base = tempfile::tempdir().expect("cartella temporanea");
            let nuova = base.path().join("mia");
            riempi(&nuova, true);
            // La skin è tenuta aperta senza permettere che la si rinomini.
            let tenuta = fs::OpenOptions::new()
                .read(true)
                .share_mode(0b001)
                .open(nuova.join(format!("skin/notte.{}", eredita::ESTENSIONE_SKIN)))
                .expect("apertura");

            let rapporto = vai_sul_posto(&nuova).expect("una skin non ferma il trasloco");
            assert_eq!(rapporto.saltati.len(), 1, "{:?}", rapporto.saltati);
            assert!(rapporto.saltati[0].contains("skin/notte"));
            // Il database e il resto sono passati lo stesso.
            assert_eq!(leggi(&nuova, "rymembra.db"), "DB");
            assert!(nuova.join("diario/rymembra.log").is_file());

            // Al prossimo avvio, liberata, passa.
            drop(tenuta);
            let secondo = vai_sul_posto(&nuova).expect("secondo giro");
            assert_eq!(
                secondo.rinominati,
                [format!(
                    "skin/notte.{} -> skin/notte.ryskin",
                    eredita::ESTENSIONE_SKIN
                )]
            );
            assert!(secondo.saltati.is_empty());
            assert_eq!(leggi(&nuova, "skin/notte.ryskin"), "SKIN");
        }
    }
}
