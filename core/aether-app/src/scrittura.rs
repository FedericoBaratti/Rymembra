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
//! ripete), ma serve per tutto quel che Aether rilegge all'avvio fidandosi.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aether_domain::errors::AppError;

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

/// `std::fs::rename`, ritentato quando Windows dice «accesso negato».
///
/// Solo quel caso: un disco pieno, un percorso sbagliato o una cartella sparita
/// non passano aspettando, e ritentarli vorrebbe dire soltanto metterci di più
/// a dire la stessa cosa.
fn rinomina_con_pazienza(da: &Path, a: &Path) -> std::io::Result<()> {
    let mut tentativo = 1;
    loop {
        match std::fs::rename(da, a) {
            Ok(()) => return Ok(()),
            Err(err)
                if err.kind() == std::io::ErrorKind::PermissionDenied
                    && tentativo < TENTATIVI_DI_RINOMINA =>
            {
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
}
