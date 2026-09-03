//! Il diario: quel che resta quando la console non c'è.
//!
//! # Il problema che questo modulo esiste per risolvere
//!
//! `main.rs` comincia con `#![cfg_attr(not(debug_assertions), windows_subsystem
//! = "windows")]`, e quella riga ha una conseguenza che non si vede scrivendo
//! codice: **in rilascio non c'è nessuna console**. Ogni `eprintln!` sparso per
//! l'applicazione — il filo che non parte, il dispositivo audio che se ne va,
//! il catalogo che rifiuta — in sviluppo finisce sotto gli occhi di chi
//! compila, e sul computer di chi ascolta finisce nel nulla.
//!
//! Finché l'applicazione si apre non è grave: quasi ogni guasto ha una faccia
//! nella finestra. Il caso che conta è l'altro. Quando Aether **non parte**,
//! chi lo ha scaricato ha esattamente due informazioni da darci — «non si
//! apre» e «Windows 11» — e nessuna delle due si può usare. Il diario è la
//! terza.
//!
//! # Perché non `log` o `tracing`
//!
//! Perché quel che serve qui sono trenta righe di testo su un file, non un
//! sistema di sottoscrittori con dei livelli e dei filtri. `tracing`
//! porterebbe dentro una decina di crate per fare `writeln!`, e soprattutto
//! porterebbe una seconda idea di quando qualcosa vada scritto: oggi
//! quell'idea sta nei siti di chiamata, uno per uno, ed è dove va guardata.
//!
//! La forma dei messaggi non cambia. Restano i `[modulo] cosa è successo` che
//! c'erano già, perché sono la ragione per cui questo diario si legge in
//! diagonale: la prima parentesi quadra dice subito se il guasto è del lettore,
//! della rete o del database.
//!
//! # Cosa non ci finisce
//!
//! Nessun percorso di file della libreria, nessun titolo, nessun nome di
//! artista. Un diario si spedisce, e spedire la lista di cosa qualcuno ascolta
//! non è una diagnosi: è la cosa che `PRIVACY.md` promette che non succede.
//! Quel che si scrive sono guasti — codici, cause, e i percorsi delle
//! **cartelle dell'applicazione**, che sono le stesse per tutti.
//!
//! # La rotazione, e perché tre file
//!
//! Un diario che cresce all'infinito è un difetto suo: due anni di avvii sono
//! decine di megabyte in una cartella che nessuno guarda. Tre file da due
//! megabyte sono abbastanza per contenere il giorno in cui qualcosa è andato
//! storto anche se ce se ne accorge il giorno dopo, e abbastanza pochi da non
//! doverci pensare mai più.
//!
//! # Il diario non fa mai cadere niente
//!
//! Ogni errore di scrittura qui dentro si ignora, e non è pigrizia: un disco
//! pieno, una cartella tolta da sotto i piedi o un antivirus che tiene il file
//! aperto sono tutte cose che possono succedere, e nessuna di loro deve
//! fermare la musica. Un diario che fa cadere l'applicazione che stava
//! sorvegliando è peggio di nessun diario.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use aether_domain::tempo::istante_iso;

// Il macro sta alla radice del crate — è quel che fa `#[macro_export]` — e va
// riportato qui dentro perché in questo modulo il nome `nota` è già preso dalla
// funzione che il macro chiama. I due vivono in spazi dei nomi diversi, quindi
// convivono: questa riga sceglie quale dei due significa `nota!`.
use crate::nota;
use aether_domain::errors::{AppError, ErrorCode};

use crate::errore::{Esito, errore};

/// Come si chiama il file su cui si sta scrivendo adesso.
const NOME: &str = "aether.log";

/// Quanto può crescere un file prima che si passi al successivo.
///
/// Due megabyte sono circa ventimila righe: molto più di quante ne produca una
/// sessione, anche lunga e anche storta.
const TETTO: u64 = 2 * 1024 * 1024;

/// Quanti file si tengono in tutto, contando quello corrente.
///
/// `aether.log`, `aether.1.log`, `aether.2.log`. Il terzo che invecchia se ne
/// va.
const QUANTI: u32 = 3;

/// La cartella del diario, dentro quella dei dati.
const CARTELLA: &str = "diario";

/// Il diario aperto: dove sta, su cosa si scrive, e quanto è lungo.
///
/// `scritti` si tiene qui invece di chiedere la lunghezza al sistema a ogni
/// riga: una `metadata()` per riga sarebbe una syscall per riga, su un percorso
/// che viene attraversato anche mentre si sta suonando.
struct Aperto {
    /// La cartella che contiene i file, per poterla mostrare a chi la chiede e
    /// per ritrovarla dopo una rotazione.
    cartella: PathBuf,
    /// Il file corrente, aperto in coda.
    file: File,
    /// Quanti byte ci sono dentro adesso.
    scritti: u64,
}

/// Il diario del processo, uno solo.
///
/// `OnceLock` per la cella e `Mutex` per il contenuto: la prima si riempie una
/// volta all'avvio, il secondo serve perché a scrivere sono sette fili di
/// sottofondo più quello della finestra, e due `writeln!` sovrapposti darebbero
/// due mezze righe intrecciate invece di due righe.
static DIARIO: OnceLock<Mutex<Option<Aperto>>> = OnceLock::new();

/// La cella, creata alla prima richiesta.
fn cella() -> &'static Mutex<Option<Aperto>> {
    DIARIO.get_or_init(|| Mutex::new(None))
}

/// Prende il lucchetto, anche se un panico lo ha avvelenato.
///
/// Un mutex avvelenato qui non è una ragione per smettere di scrivere: è anzi
/// il momento in cui scrivere serve di più, perché vuol dire che qualcuno è
/// appena caduto tenendolo. La stessa scelta di `stato::con_libreria`.
fn preso() -> MutexGuard<'static, Option<Aperto>> {
    cella().lock().unwrap_or_else(PoisonError::into_inner)
}

/// Apre il diario dentro la cartella dei dati dell'applicazione.
///
/// Va chiamata **per prima** in `setup`, prima di aprire la libreria: quel che
/// si vuole leggere quando qualcosa va storto all'avvio è proprio ciò che
/// succede subito dopo questa riga.
///
/// Se non si riesce ad aprire — cartella non scrivibile, disco pieno — non
/// succede niente di visibile e l'applicazione continua senza diario. È il
/// comportamento giusto: chi ha aperto Aether voleva ascoltare, non registrare.
pub fn apri(cartella_dati: &Path) {
    let cartella = cartella_dati.join(CARTELLA);
    if fs::create_dir_all(&cartella).is_err() {
        return;
    }
    let Some(aperto) = apri_il_file(cartella) else {
        return;
    };
    *preso() = Some(aperto);

    // La prima riga di ogni sessione dice quale versione l'ha scritta. Senza,
    // un diario che arriva da qualcun altro non si sa a quale codice appartenga
    // — e la prima domanda davanti a un guasto è sempre «quale versione».
    nota!(
        "[diario] Aether {} — sessione aperta",
        env!("CARGO_PKG_VERSION")
    );
}

/// Apre `NOME` dentro `cartella`, in coda, e conta quel che c'era già.
///
/// `None` se non si riesce: chi chiama resta senza diario e tira dritto.
fn apri_il_file(cartella: PathBuf) -> Option<Aperto> {
    let percorso = cartella.join(NOME);
    let scritti = fs::metadata(&percorso).map_or(0, |m| m.len());
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&percorso)
        .ok()?;
    Some(Aperto {
        cartella,
        file,
        scritti,
    })
}

/// Dove stanno i file del diario, se è stato aperto.
///
/// Serve al comando che apre quella cartella nel gestore file: chiedere a
/// qualcuno di navigare fino a `%APPDATA%` a mano è il modo di non ricevere mai
/// il file.
#[must_use]
pub fn cartella() -> Option<PathBuf> {
    preso().as_ref().map(|aperto| aperto.cartella.clone())
}

/// Scrive una riga, con l'istante davanti.
///
/// Non si chiama direttamente: la si raggiunge con [`nota!`], che rimanda la
/// formattazione a dopo — così una riga che non verrà scritta perché il diario
/// è chiuso non costa nemmeno la `String`.
///
/// In sviluppo la riga va **anche** su `stderr`, che è dove chi compila la
/// cerca. In rilascio quello `stderr` non esiste, ed è tutto il motivo per cui
/// questo modulo è stato scritto.
pub fn nota(argomenti: std::fmt::Arguments<'_>) {
    #[cfg(debug_assertions)]
    eprintln!("{argomenti}");

    // Il lucchetto si prende e si lascia **dentro** questo blocco: la rotazione
    // che può seguire lo riprende, e tenerlo qui vorrebbe dire aspettare se
    // stessi per sempre.
    let pieno = {
        let mut guardia = preso();
        let Some(aperto) = guardia.as_mut() else {
            return;
        };
        let riga = format!("{} {argomenti}\n", istante_iso(adesso_ms()));
        // Un errore di scrittura non si racconta a nessuno: raccontarlo
        // vorrebbe dire scriverlo sul diario che non si riesce a scrivere.
        if aperto.file.write_all(riga.as_bytes()).is_ok() {
            aperto.scritti = aperto
                .scritti
                .saturating_add(u64::try_from(riga.len()).unwrap_or(0));
        }
        // `flush` a ogni riga, e sì, costa. È il prezzo del solo caso che
        // conta: se il processo muore un istante dopo — ed è precisamente
        // quando si vuole leggere l'ultima riga — un buffer non ancora
        // svuotato è la riga che spiegava tutto, persa.
        let _ = aperto.file.flush();
        aperto.scritti >= TETTO
    };

    if pieno {
        ruota();
    }
}

/// Sposta i file di uno e riapre un diario vuoto.
///
/// Il file corrente si chiude **prima** di rinominarlo: su Windows un file
/// aperto non si rinomina, e una rotazione che fallisce a metà lascerebbe un
/// diario che non cresce più.
fn ruota() {
    let mut guardia = preso();
    let Some(aperto) = guardia.take() else {
        return;
    };
    let cartella = aperto.cartella;
    drop(aperto.file);

    // Dal più vecchio al più giovane: al contrario, ogni rinomina
    // sovrascriverebbe il file che sta per essere spostato a sua volta.
    for indice in (1..QUANTI).rev() {
        let da = if indice == 1 {
            cartella.join(NOME)
        } else {
            cartella.join(format!("aether.{}.log", indice - 1))
        };
        let _ = fs::rename(da, cartella.join(format!("aether.{indice}.log")));
    }
    // Quel che è invecchiato oltre l'ultimo file se ne va. `remove_file` su
    // qualcosa che non c'è non è un guasto: alla prima rotazione della vita di
    // un'installazione quel file non esiste ancora.
    let _ = fs::remove_file(cartella.join(format!("aether.{QUANTI}.log")));

    *guardia = apri_il_file(cartella);
}

/// L'orologio, in millisecondi dall'epoca.
///
/// La stessa forma di `stato::adesso_ms`, e riscritta di proposito invece che
/// importata: questo modulo si apre **prima** di `stato`, e deve poter scrivere
/// la propria prima riga anche se l'apertura della libreria è la cosa che sta
/// per fallire.
fn adesso_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

/// Scrive una riga nel diario, come `eprintln!` scriveva sulla console.
///
/// Sostituisce ogni `eprintln!`/`println!` dell'applicazione. La forma dei
/// messaggi non cambia — restano i `[modulo] cosa è successo` di prima —
/// perché è quella che rende il diario leggibile in diagonale.
#[macro_export]
macro_rules! nota {
    ($($arg:tt)*) => {
        $crate::diario::nota(format_args!($($arg)*))
    };
}

/// Fa in modo che un panico lasci scritto cos'era.
///
/// # Perché serve, dato che i panici sono vietati
///
/// Perché il divieto vale per il codice di questo albero, non per le 429 crate
/// che ci stanno sotto. `Cargo.toml` della radice mette `unwrap_used`,
/// `expect_used` e `panic` su `deny`, e quella regola tiene: un panico che
/// arriva qui viene da un decodificatore audio davanti a un file storto, da un
/// parser davanti a una risposta che non si aspettava, da un backend audio.
///
/// # Perché è l'unica occasione
///
/// Perché il profilo di rilascio dichiara `panic = "abort"`. Non c'è
/// svolgimento dello stack, non c'è un `catch_unwind` che possa raccogliere i
/// pezzi, non c'è un `Drop` che venga eseguito: il processo termina qui. Questo
/// gancio è letteralmente l'ultimo codice nostro che gira, e se non scrive lui,
/// non scrive nessuno.
///
/// Va installato per primo in `main`, prima ancora di costruire la finestra.
/// Finché il diario non è aperto le righe finiscono su `stderr` e basta — cioè
/// da nessuna parte, in rilascio — ma dal momento in cui `setup` chiama
/// [`apri`] in poi, cioè per tutta la vita utile del processo, restano.
pub fn installa_gancio_dei_panici() {
    // Il gancio di serie non si perde: continua a stampare quel che stampava,
    // che in sviluppo è la cosa che si sta guardando.
    let precedente = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let dove = info.location().map_or_else(
            || "posizione ignota".to_owned(),
            |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
        );
        // Il nome del filo è la metà che dice **quale** delle sette passate di
        // sottofondo è caduta: senza, un panico nel filo dell'arricchimento e
        // uno nel filo del backup si leggono uguali.
        let filo = std::thread::current()
            .name()
            .unwrap_or("senza nome")
            .to_owned();
        nota!("[PANICO] filo={filo} a {dove}: {info}");
        precedente(info);
    }));
}

/// Apre la cartella del diario nel gestore file del sistema.
///
/// # Perché un bottone e non un percorso scritto in una schermata
///
/// Perché `%APPDATA%\io.github.federicobaratti.aether\diario` non è un posto in
/// cui qualcuno arriva leggendolo: `%APPDATA%` è nascosta di serie in Esplora
/// risorse, e il primo passo di ogni segnalazione diventa spiegare come
/// mostrare le cartelle nascoste. Un bottone toglie quel passo, ed è l'unica
/// ragione per cui questo comando esiste.
///
/// # Errori
///
/// `internal.aborted` se il diario non è mai stato aperto — cartella dei dati
/// non scrivibile — o se il gestore file rifiuta di aprirsi. Sono due guasti
/// diversi e portano due cause diverse, perché nel primo caso non c'è niente da
/// aprire e nel secondo il file c'è ma non lo si sta vedendo.
#[tauri::command]
pub fn diario_apri() -> Esito<()> {
    let Some(dove) = cartella() else {
        return Err(errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura della cartella del diario".to_owned()),
            })
            .with_cause(
                "il diario non è stato aperto: la cartella dei dati non è scrivibile".to_owned(),
            ),
        ));
    };
    tauri_plugin_opener::open_path(&dove, None::<&str>).map_err(|err| {
        errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura della cartella del diario".to_owned()),
            })
            .with_cause(err.to_string()),
        )
    })
}

/// Quanto si tiene di un messaggio arrivato dalla finestra.
///
/// Abbastanza per riconoscere il guasto, poco abbastanza da non trascinarsi
/// dietro mezzo stack trace. Un `TypeError` utile sta in una riga.
const QUANTO_DAL_DAVANTI: usize = 300;

/// Prende una riga sola, e non troppo lunga.
///
/// # Perché il taglio non è un vezzo
///
/// Perché quel che arriva da JavaScript è uno stack trace, e uno stack trace
/// del davanti porta dentro gli URL dei moduli — e in sviluppo quegli URL sono
/// percorsi del disco di chi sta lavorando. Il diario si spedisce; il vincolo
/// scritto in cima a questo file dice che dentro non ci finiscono percorsi. La
/// prima riga di un errore JavaScript è il messaggio, che è la parte che serve
/// a capire, e le righe dopo sono la parte che serve a nessuno che non abbia
/// già i sorgenti davanti.
///
/// Si taglia sui **caratteri**, non sui byte: `«…»` e gli accenti dei messaggi
/// italiani sono più di un byte l'uno, e un taglio a metà di un carattere
/// scriverebbe nel diario un rombo con il punto interrogativo.
fn una_riga_sola(testo: &str) -> String {
    let prima = testo.lines().next().unwrap_or("").trim();
    if prima.chars().count() <= QUANTO_DAL_DAVANTI {
        return prima.to_owned();
    }
    let corto: String = prima.chars().take(QUANTO_DAL_DAVANTI).collect();
    format!("{corto}…")
}

/// Scrive nel diario un guasto arrivato dalla finestra.
///
/// # Perché esiste
///
/// Perché un errore JavaScript non catturato, in rilascio, non lascia niente:
/// la console non c'è, la finestra resta bianca, e di quel guasto non resta
/// traccia da nessuna parte. Il diario del nucleo raccoglie tutto quel che
/// succede sotto e niente di quel che succede sopra, che è metà
/// dell'applicazione.
///
/// # Cosa ci arriva
///
/// `dove` dice chi ha chiamato — `finestra`, `promessa`, `recinto` — e finisce
/// nella parentesi quadra, così il diario resta leggibile in diagonale come il
/// resto. `cosa` è il messaggio, che viene ridotto a una riga sola da
/// [`una_riga_sola`] prima di toccare il file, per la ragione di privacy
/// scritta là.
///
/// # Perché non restituisce errori
///
/// Perché è il diario, e «il diario non fa mai cadere niente» (vedi la testa di
/// questo file). Un `Esito` qui vorrebbe dire una finestra già in avaria che
/// riceve un secondo errore dal codice chiamato a raccontare il primo.
#[tauri::command]
pub fn diario_annota(dove: String, cosa: String) {
    nota(format_args!(
        "[{}] {}",
        una_riga_sola(&dove),
        una_riga_sola(&cosa)
    ));
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Le prove di questo modulo si danno il turno.
    ///
    /// Il diario è **uno per processo** — è il suo punto — e le prove girano
    /// tutte nello stesso. Senza questo lucchetto, quella che lo chiude per
    /// verificare che un diario chiuso non faccia danni lo chiuderebbe sotto i
    /// piedi di quella che sta misurando la rotazione, e a fallire sarebbe la
    /// seconda: il difetto peggiore che una prova possa avere.
    static IL_TURNO: Mutex<()> = Mutex::new(());

    /// Apre un diario in una cartella temporanea che sparisce da sé.
    fn in_una_cartella(prova: impl FnOnce(&Path)) {
        let _turno = IL_TURNO.lock().unwrap_or_else(PoisonError::into_inner);
        let radice = tempfile::tempdir();
        let Ok(radice) = radice else {
            return;
        };
        apri(radice.path());
        prova(radice.path());
        // Si chiude, o su Windows la cartella temporanea non si cancella: un
        // file aperto non si elimina, e `TempDir` se ne andrebbe in silenzio
        // lasciandola lì.
        *preso() = None;
    }

    #[test]
    fn la_prima_riga_dice_la_versione() {
        in_una_cartella(|radice| {
            let percorso = radice.join(CARTELLA).join(NOME);
            let Ok(testo) = fs::read_to_string(&percorso) else {
                return;
            };
            assert!(
                testo.contains(env!("CARGO_PKG_VERSION")),
                "la sessione si apre senza dire quale versione la scrive: {testo}"
            );
            assert!(
                testo.contains("[diario]"),
                "manca l'etichetta del modulo: {testo}"
            );
        });
    }

    #[test]
    fn ogni_riga_porta_il_suo_istante() {
        in_una_cartella(|radice| {
            nota!("[prova] una riga qualunque");
            let percorso = radice.join(CARTELLA).join(NOME);
            let Ok(testo) = fs::read_to_string(&percorso) else {
                return;
            };
            let riga = testo.lines().find(|r| r.contains("[prova]"));
            assert!(riga.is_some(), "la riga scritta non è nel file: {testo}");
            let riga = riga.unwrap_or_default();
            // `2026-08-27 14:03:11.482Z [prova] …`: l'istante sta davanti, ed è
            // quel che permette di ordinare due diari messi insieme.
            assert!(
                riga.ends_with("[prova] una riga qualunque"),
                "il messaggio non è in coda alla riga: {riga}"
            );
            let istante = riga.trim_end_matches("[prova] una riga qualunque").trim();
            assert!(
                istante.len() == 24 && istante.ends_with('Z'),
                "l'istante non ha la forma attesa: {istante:?}"
            );
        });
    }

    #[test]
    fn il_diario_chiuso_non_fa_cadere_niente() {
        // Il caso vero: `apri` è fallita — cartella non scrivibile — e i sette
        // fili di sottofondo continuano a scrivere per tutta la sessione.
        // Nessuno di loro deve accorgersene.
        let _turno = IL_TURNO.lock().unwrap_or_else(PoisonError::into_inner);
        *preso() = None;
        nota!("[prova] nessuno mi legge");
        assert!(cartella().is_none(), "il diario risulta aperto e non lo è");
    }

    #[test]
    fn quando_e_pieno_si_passa_al_file_dopo() {
        in_una_cartella(|radice| {
            let dove = radice.join(CARTELLA);
            // Si porta il contatore appena sotto il tetto invece di scrivere
            // due megabyte davvero: la prova deve misurare la rotazione, non la
            // velocità del disco.
            riempi_fino_al_tetto();
            nota!("[prova] la riga che fa traboccare");
            assert!(
                dove.join("aether.1.log").exists(),
                "il file pieno non è stato messo da parte"
            );
            let Ok(nuovo) = fs::read_to_string(dove.join(NOME)) else {
                return;
            };
            assert!(
                !nuovo.contains("traboccare"),
                "il file nuovo non è vuoto: la rotazione ha riaperto quello di prima"
            );
            assert!(
                cartella().is_some(),
                "dopo la rotazione il diario è rimasto chiuso"
            );
        });
    }

    #[test]
    fn oltre_l_ultimo_file_si_dimentica() {
        in_una_cartella(|radice| {
            let dove = radice.join(CARTELLA);
            // Tre rotazioni di fila: alla fine devono esserci `aether.log`,
            // `.1` e `.2`, e non un `.3`.
            for giro in 0..3 {
                riempi_fino_al_tetto();
                nota!("[prova] giro {giro}");
            }
            assert!(dove.join(NOME).exists(), "manca il file corrente");
            assert!(dove.join("aether.1.log").exists(), "manca il primo");
            assert!(dove.join("aether.2.log").exists(), "manca il secondo");
            assert!(
                !dove.join("aether.3.log").exists(),
                "il quarto file non doveva sopravvivere: la rotazione non dimentica"
            );
        });
    }

    #[test]
    fn dallo_stack_trace_resta_solo_il_messaggio() {
        // Il danno che questa prova tiene chiuso: un diario che si spedisce e
        // che porta dentro i percorsi del disco di chi lo spedisce. Le righe
        // dopo la prima di un errore JavaScript sono tutte URL di moduli.
        let dentro = una_riga_sola(
            "TypeError: x is not a function
    at suona (http://localhost:1420/src/App.tsx:12:3)",
        );
        assert_eq!(
            dentro, "TypeError: x is not a function",
            "lo stack trace è finito nel diario insieme ai percorsi che si porta dietro"
        );
    }

    #[test]
    fn un_messaggio_lunghissimo_si_taglia_senza_rompere_gli_accenti() {
        // Tagliare sui byte spezzerebbe una «à» a metà e scriverebbe un rombo.
        let lungo = "à".repeat(QUANTO_DAL_DAVANTI + 50);
        let corto = una_riga_sola(&lungo);
        assert_eq!(
            corto.chars().count(),
            QUANTO_DAL_DAVANTI + 1,
            "il taglio non ha lasciato la lunghezza che dice di lasciare"
        );
        assert!(
            corto.chars().take(QUANTO_DAL_DAVANTI).all(|c| c == 'à'),
            "il taglio ha rotto un carattere a metà"
        );
    }

    #[test]
    fn una_riga_corta_resta_com_e() {
        assert_eq!(una_riga_sola("  recinto  "), "recinto");
        assert_eq!(una_riga_sola(""), "");
    }

    /// Mette il contatore a un byte dal tetto, così la riga dopo trabocca.
    fn riempi_fino_al_tetto() {
        let mut guardia = preso();
        if let Some(aperto) = guardia.as_mut() {
            aperto.scritti = TETTO.saturating_sub(1);
        }
    }
}
