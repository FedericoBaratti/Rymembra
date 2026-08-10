//! Eseguire un processo figlio restando ascoltabili.
//!
//! Sembra utilità da poco, ed è il modulo con più trappole del crate. Tre
//! problemi che sembrano indipendenti hanno una sola soluzione:
//!
//! 1. **La scadenza.** `std::process::Child` non ha un'attesa con scadenza. Una
//!    ricerca che non risponde bloccherebbe il filo della coda per sempre.
//! 2. **L'annullamento.** Chi chiama ha un `AtomicBool`, non un `AbortSignal`:
//!    va guardato spesso, e fra un'occhiata e l'altra non si può stare fermi
//!    dentro una `read`.
//! 3. **Lo stallo delle pipe.** Se si legge lo stdout e si lascia riempire lo
//!    stderr, il figlio si blocca scrivendo su una pipe piena e non finisce mai.
//!    yt-dlp scrive su entrambe, quindi vanno drenate entrambe.
//!
//! La forma che risolve tutti e tre: **due fili di lettura** che spingono in un
//! canale, e il filo chiamante che aspetta con [`std::sync::mpsc::Receiver::recv_timeout`].
//! Fra un messaggio e l'altro guarda l'orologio e la chiusura di annullamento,
//! e il figlio resta suo — quindi può ucciderlo davvero.
//!
//! # Perché le righe si leggono a byte e non con `lines()`
//!
//! `BufRead::lines()` restituisce `Err` su UTF-8 non valido, e a quel punto la
//! lettura si interrompe: basterebbe un titolo di video con un byte storto per
//! perdere tutto l'avanzamento che viene dopo, compresa la riga `AETHER_D` che
//! porta il percorso del file. Con `read_until` e conversione tollerante, un
//! carattere illeggibile resta un carattere illeggibile e nient'altro si rompe.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// Ogni quanto si guardano orologio e annullamento quando non arriva niente.
///
/// Duecento millisecondi: abbastanza spesso perché «Annulla» sembri immediato,
/// abbastanza raro da non far girare a vuoto un filo per tutto lo scaricamento.
const BATTITO: Duration = Duration::from_millis(200);

/// Com'è finito un processo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Esito {
    /// Il codice d'uscita, se il processo è uscito da sé.
    pub codice: Option<i32>,
    /// Tutto lo stdout, quando è stato raccolto (vedi [`esegui`]).
    pub stdout: String,
    /// Tutto lo stderr.
    pub stderr: String,
    /// La scadenza è passata e il processo è stato ucciso.
    pub scaduto: bool,
    /// Chi chiama ha chiesto di smettere e il processo è stato ucciso.
    pub annullato: bool,
}

impl Esito {
    /// Il processo è uscito da sé annunciando successo.
    #[must_use]
    pub fn riuscito(&self) -> bool {
        !self.scaduto && !self.annullato && self.codice == Some(0)
    }
}

/// Da quale pipe è arrivata una riga.
enum Canale {
    Uscita(String),
    Errore(String),
    Chiuso,
}

/// Esegue il programma raccogliendo tutto, con una scadenza.
///
/// Per le invocazioni brevi che rispondono in un colpo solo — `--dump-single-json`,
/// `--version`. Per gli scaricamenti serve [`esegui_a_righe`], che consegna
/// l'avanzamento mentre succede invece che alla fine.
///
/// # Errori
///
/// Solo l'errore di sistema dello spawn: un processo che fallisce è un
/// [`Esito`] riuscito a raccontare, non un `Err`.
pub fn esegui(
    programma: &Path,
    argomenti: &[String],
    scadenza: Duration,
    annullato: &dyn Fn() -> bool,
) -> std::io::Result<Esito> {
    let mut uscita = String::new();
    let mut esito = esegui_interno(
        programma,
        argomenti,
        Some(scadenza),
        annullato,
        &mut |riga| {
            uscita.push_str(riga);
            uscita.push('\n');
        },
    )?;
    esito.stdout = uscita;
    Ok(esito)
}

/// Esegue il programma consegnando ogni riga di stdout appena arriva.
///
/// Senza scadenza complessiva: uno scaricamento può durare legittimamente dei
/// minuti, e una scadenza tarata sul caso peggiore non proteggerebbe da niente.
/// Chi chiama controlla la durata attraverso `annullato`, che è la stessa cosa
/// ma decisa da chi sa quanto è ragionevole aspettare.
///
/// # Errori
///
/// Come [`esegui`].
pub fn esegui_a_righe(
    programma: &Path,
    argomenti: &[String],
    annullato: &dyn Fn() -> bool,
    riga: &mut dyn FnMut(&str),
) -> std::io::Result<Esito> {
    esegui_interno(programma, argomenti, None, annullato, riga)
}

fn esegui_interno(
    programma: &Path,
    argomenti: &[String],
    scadenza: Option<Duration>,
    annullato: &dyn Fn() -> bool,
    riga: &mut dyn FnMut(&str),
) -> std::io::Result<Esito> {
    let mut comando = Command::new(programma);
    comando
        .args(argomenti)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    senza_finestra(&mut comando);

    let mut figlio = comando.spawn()?;
    let (mittente, ricevitore) = mpsc::channel();
    let fili = attacca_lettori(&mut figlio, &mittente);
    // Il mittente originale va lasciato cadere, o il canale non si chiude mai e
    // l'attesa non saprebbe distinguere «il figlio tace» da «il figlio ha finito».
    drop(mittente);

    let esito = raccogli(&mut figlio, &ricevitore, scadenza, annullato, riga);

    // I fili si aspettano **solo** quando il processo è finito da sé.
    //
    // Dopo un `kill` no, e non è pigrizia: le pipe restano aperte finché le
    // tiene aperte qualcuno, e a tenerle aperte può essere un *nipote* — un
    // processo che il figlio aveva avviato e che il kill non tocca. Aspettare i
    // lettori lì vorrebbe dire aspettare quel nipote, cioè aspettare
    // esattamente ciò da cui l'annullamento doveva liberarci: premuto
    // «Annulla», l'interfaccia resterebbe ferma per tutta la durata dello
    // scaricamento che si è appena chiesto di interrompere.
    //
    // Lasciati andare, i due fili muoiono da sé quando la pipe si chiude: non
    // tengono niente di condiviso e il loro invio fallisce senza far rumore.
    if !esito.scaduto && !esito.annullato {
        for filo in fili {
            // Un filo di lettura che è andato in panico non è una ragione per
            // far cadere la coda: l'esito del processo si conosce già.
            drop(filo.join());
        }
    }
    Ok(esito)
}

/// Attacca un filo a stdout e uno a stderr.
fn attacca_lettori(
    figlio: &mut Child,
    mittente: &Sender<Canale>,
) -> Vec<std::thread::JoinHandle<()>> {
    let mut fili = Vec::with_capacity(2);
    if let Some(uscita) = figlio.stdout.take() {
        let mittente = mittente.clone();
        fili.push(std::thread::spawn(move || {
            leggi_righe(uscita, &mittente, Canale::Uscita);
        }));
    }
    if let Some(errore) = figlio.stderr.take() {
        let mittente = mittente.clone();
        fili.push(std::thread::spawn(move || {
            leggi_righe(errore, &mittente, Canale::Errore);
        }));
    }
    fili
}

/// Legge una pipe riga per riga e la spinge nel canale.
fn leggi_righe(flusso: impl Read, mittente: &Sender<Canale>, avvolgi: fn(String) -> Canale) {
    let mut lettore = BufReader::new(flusso);
    let mut grezzo = Vec::new();
    loop {
        grezzo.clear();
        match lettore.read_until(b'\n', &mut grezzo) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let riga = String::from_utf8_lossy(&grezzo).trim_end().to_owned();
        if mittente.send(avvolgi(riga)).is_err() {
            // Chi aspettava se n'è andato (scadenza o annullamento): non c'è
            // nessuno a cui consegnare, e insistere terrebbe vivo il filo.
            break;
        }
    }
    drop(mittente.send(Canale::Chiuso));
}

/// Aspetta la fine tenendo d'occhio orologio e annullamento.
fn raccogli(
    figlio: &mut Child,
    ricevitore: &Receiver<Canale>,
    scadenza: Option<Duration>,
    annullato: &dyn Fn() -> bool,
    riga: &mut dyn FnMut(&str),
) -> Esito {
    let inizio = Instant::now();
    let mut stderr = String::new();
    let mut chiusi = 0_u8;

    loop {
        if annullato() {
            return termina(figlio, stderr, false, true);
        }
        if scadenza.is_some_and(|limite| inizio.elapsed() >= limite) {
            return termina(figlio, stderr, true, false);
        }

        match ricevitore.recv_timeout(BATTITO) {
            Ok(Canale::Uscita(testo)) => riga(&testo),
            Ok(Canale::Errore(testo)) => {
                stderr.push_str(&testo);
                stderr.push('\n');
            }
            // Entrambe le pipe chiuse: il figlio ha finito di parlare, e adesso
            // `wait` non può più bloccarsi perché non resta niente da drenare.
            Ok(Canale::Chiuso) => {
                chiusi = chiusi.saturating_add(1);
                if chiusi >= 2 {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let codice = figlio.wait().ok().and_then(|stato| stato.code());
    Esito {
        codice,
        stdout: String::new(),
        stderr,
        scaduto: false,
        annullato: false,
    }
}

/// Uccide il figlio e confeziona l'esito.
fn termina(figlio: &mut Child, stderr: String, scaduto: bool, annullato: bool) -> Esito {
    drop(figlio.kill());
    // Raccogliere il figlio ucciso non è pignoleria: senza `wait` resta uno
    // zombie, e una coda di cento brani ne lascerebbe cento.
    drop(figlio.wait());
    Esito {
        codice: None,
        stdout: String::new(),
        stderr,
        scaduto,
        annullato,
    }
}

/// Su Windows, niente finestra di console per il figlio.
///
/// yt-dlp è un'applicazione da console: senza questo, ogni brano di una coda fa
/// lampeggiare una finestra nera sopra a quel che l'utente sta facendo. Non è
/// estetica — con tre fili in parallelo e cento brani sono trecento finestre che
/// rubano il primo piano.
#[cfg(windows)]
fn senza_finestra(comando: &mut Command) {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW` di `winbase.h`.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    comando.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn senza_finestra(_comando: &mut Command) {}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un programma che esiste ovunque e fa quel che gli si dice.
    ///
    /// Le prove qui devono eseguire *qualcosa*: la macchinery da provare è
    /// proprio quella dei processi figli, e sostituirla con una finta proverebbe
    /// la finta. Si usa l'interprete di comandi del sistema, che c'è sempre.
    fn guscio(comando: &str) -> (std::path::PathBuf, Vec<String>) {
        if cfg!(windows) {
            (
                std::path::PathBuf::from("cmd"),
                vec!["/C".to_owned(), comando.to_owned()],
            )
        } else {
            (
                std::path::PathBuf::from("/bin/sh"),
                vec!["-c".to_owned(), comando.to_owned()],
            )
        }
    }

    fn mai_annullato() -> impl Fn() -> bool {
        || false
    }

    #[test]
    fn raccoglie_stdout_e_il_codice_duscita() {
        let (programma, argomenti) = guscio("echo ciao");
        let esito = esegui(
            &programma,
            &argomenti,
            Duration::from_secs(30),
            &mai_annullato(),
        )
        .expect("il guscio di sistema si esegue");
        assert!(esito.riuscito());
        assert!(esito.stdout.contains("ciao"));
    }

    #[test]
    fn un_codice_diverso_da_zero_non_e_un_errore_di_rust() {
        // La distinzione è il punto: `Err` vuol dire «non sono riuscito a
        // eseguirlo», un codice storto vuol dire «l'ho eseguito e ha fallito», e
        // solo il secondo ha uno stderr da classificare.
        let (programma, argomenti) = guscio("exit 3");
        let esito = esegui(
            &programma,
            &argomenti,
            Duration::from_secs(30),
            &mai_annullato(),
        )
        .expect("il guscio di sistema si esegue");
        assert!(!esito.riuscito());
        assert_eq!(esito.codice, Some(3));
    }

    #[test]
    fn lo_spawn_di_un_programma_inesistente_e_un_errore() {
        let esito = esegui(
            Path::new("questo-programma-non-esiste-davvero"),
            &[],
            Duration::from_secs(5),
            &mai_annullato(),
        );
        assert!(esito.is_err());
    }

    #[test]
    fn lannullamento_non_aspetta_la_fine() {
        // Un processo che dormirebbe a lungo: se l'annullamento funziona, la
        // chiamata torna in un battito, non fra dieci secondi.
        let (programma, argomenti) = if cfg!(windows) {
            guscio("ping -n 11 127.0.0.1 >NUL")
        } else {
            guscio("sleep 10")
        };
        let inizio = Instant::now();
        let esito = esegui_a_righe(&programma, &argomenti, &|| true, &mut |_| {})
            .expect("il guscio di sistema si esegue");
        assert!(esito.annullato);
        assert!(!esito.riuscito());
        assert!(
            inizio.elapsed() < Duration::from_secs(5),
            "l'annullamento ha aspettato la fine del processo"
        );
    }

    #[test]
    fn la_scadenza_uccide_chi_non_risponde() {
        let (programma, argomenti) = if cfg!(windows) {
            guscio("ping -n 11 127.0.0.1 >NUL")
        } else {
            guscio("sleep 10")
        };
        let inizio = Instant::now();
        let esito = esegui(
            &programma,
            &argomenti,
            Duration::from_millis(500),
            &mai_annullato(),
        )
        .expect("il guscio di sistema si esegue");
        assert!(esito.scaduto);
        assert!(inizio.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn le_righe_arrivano_una_per_una() {
        let (programma, argomenti) = guscio("echo uno&& echo due&& echo tre");
        let mut viste = Vec::new();
        let esito = esegui_a_righe(&programma, &argomenti, &mai_annullato(), &mut |riga| {
            if !riga.is_empty() {
                viste.push(riga.to_owned());
            }
        })
        .expect("il guscio di sistema si esegue");
        assert!(esito.riuscito());
        assert_eq!(viste, vec!["uno", "due", "tre"]);
    }

    #[test]
    fn stdout_e_stderr_si_drenano_tutti_e_due() {
        // La prova dello stallo: se solo una delle due pipe venisse letta,
        // l'altra si riempirebbe e il processo non uscirebbe mai. Con poche
        // righe non si riempie niente, ma la separazione dei due flussi sì che
        // si vede, ed è quella che il resto del crate usa per classificare.
        let (programma, argomenti) = guscio("echo buono&& echo cattivo 1>&2");
        let mut uscita = String::new();
        let esito = esegui_a_righe(&programma, &argomenti, &mai_annullato(), &mut |riga| {
            uscita.push_str(riga);
        })
        .expect("il guscio di sistema si esegue");
        assert!(uscita.contains("buono"));
        assert!(esito.stderr.contains("cattivo"));
        assert!(!uscita.contains("cattivo"), "lo stderr non è avanzamento");
    }
}
