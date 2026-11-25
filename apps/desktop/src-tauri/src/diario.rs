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
//! nella finestra. Il caso che conta è l'altro. Quando Rymembra **non parte**,
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
//! Le stesse per tutti **tranne il nome dell'utente**, che fino alla 2.4.0 ci
//! stava dentro per esteso: la riga d'avvio scriveva
//! `dati=C:\Users\Mario Rossi\AppData\Roaming\…`, e chi mandava il diario mandava
//! anche nome e cognome. Adesso ogni riga passa da [`senza_utente`] prima di
//! toccare il file, e le cartelle dell'utente diventano `%APPDATA%`,
//! `%LOCALAPPDATA%` e `%USERPROFILE%` — le stesse parole che si scriverebbero
//! in una segnalazione, e che dicono ancora **dove** senza dire **chi**.
//!
//! # La rotazione, e perché tre file
//!
//! Un diario che cresce all'infinito è un difetto suo: due anni di avvii sono
//! decine di megabyte in una cartella che nessuno guarda. Tre file da due
//! megabyte sono abbastanza per contenere il giorno in cui qualcosa è andato
//! storto anche se ce se ne accorge il giorno dopo, e abbastanza pochi da non
//! doverci pensare mai più.
//!
//! # Una riga che si ripete non riempie il diario
//!
//! Tre file da due megabyte tengono la storia di una giornata solo se nessuno
//! li gira da solo. La stessa riga scritta a ogni richiesta di un dispositivo
//! collegato — la libreria occupata da una scansione, una domanda che cade —
//! li girava in ore. Qui la **stessa** riga si scrive [`RIPETUTE_AMMESSE`]
//! volte per minuto; le altre si contano, e il diario dice che lo fa e quante
//! sono state ([`Ripetute`]). Il tetto sta qui, dove le righe passano tutte, e
//! non in ogni posto che ne scrive troppe. Il gancio dei panici e le righe
//! d'emergenza non lo conoscono.
//!
//! # Il diario non fa mai cadere niente
//!
//! Ogni errore di scrittura qui dentro si ignora, e non è pigrizia: un disco
//! pieno, una cartella tolta da sotto i piedi o un antivirus che tiene il file
//! aperto sono tutte cose che possono succedere, e nessuna di loro deve
//! fermare la musica. Un diario che fa cadere l'applicazione che stava
//! sorvegliando è peggio di nessun diario.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};

use rymembra_domain::tempo::istante_iso;

// Il macro sta alla radice del crate — è quel che fa `#[macro_export]` — e va
// riportato qui dentro perché in questo modulo il nome `nota` è già preso dalla
// funzione che il macro chiama. I due vivono in spazi dei nomi diversi, quindi
// convivono: questa riga sceglie quale dei due significa `nota!`.
use crate::nota;
use rymembra_domain::errors::{AppError, ErrorCode};

use crate::disparte::{comanda, in_disparte};
use crate::errore::{Esito, errore};

/// Come si chiama il file su cui si sta scrivendo adesso.
const NOME: &str = "rymembra.log";

/// Quanto può crescere un file prima che si passi al successivo.
///
/// Due megabyte sono circa ventimila righe: molto più di quante ne produca una
/// sessione, anche lunga e anche storta.
const TETTO: u64 = 2 * 1024 * 1024;

/// Quanti file si tengono in tutto, contando quello corrente.
///
/// `rymembra.log`, `rymembra.1.log`, `rymembra.2.log`. Il terzo che invecchia se ne
/// va.
const QUANTI: u32 = 3;

/// La cartella del diario, dentro quella dei dati.
const CARTELLA: &str = "diario";

/// Il file di riserva: dove va una riga dell'avvio quando il diario non c'è.
///
/// # Perché un secondo file
///
/// Il diario si apre dentro il `setup`, e in Tauri 2 il `setup` gira dopo che
/// le finestre di `tauri.conf.json` sono state create. Un guasto prima — la
/// WebView2 assente o rotta, la cartella dei dati che non si risolve — diventa
/// un panico a diario chiuso: il gancio non aveva dove scrivere, `stderr` in
/// rilascio non esiste, e il processo spariva senza una riga. È proprio il
/// caso per cui il diario esiste («quando Rymembra non parte»).
///
/// Il file sta in `%LOCALAPPDATA%\<identificatore>\`, accanto alla cartella
/// della WebView2, e **non** nella cartella dei dati: quella non si deve
/// creare prima del trasloco dalla versione di prima, che vale solo se la
/// cartella nuova non c'è ancora. Ci arrivano soltanto le righe del gancio dei
/// panici a diario chiuso e quella di un avvio fallito ([`nota_di_emergenza`]),
/// mai le righe normali: una sessione che il diario non l'ha potuto aprire
/// lo farebbe crescere senza rotazione.
const RISERVA: &str = "avvio.log";

/// Oltre questa misura il file di riserva ricomincia da capo: deve tenere
/// l'ultimo avvio fallito, non la storia di tutti.
const TETTO_RISERVA: u64 = 256 * 1024;

/// L'identificatore dell'applicazione, quello di `tauri.conf.json`: il nome
/// della cartella in `%LOCALAPPDATA%`. Scritto qui perché il file di riserva
/// serve proprio quando l'applicazione non è nata; la prova
/// `l_identificatore_della_riserva_e_quello_di_tauri_conf` lo tiene allineato.
const IDENTIFICATORE: &str = "io.github.federicobaratti.rymembra";

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
    /// Le righe che si stanno ripetendo: vedi [`Ripetute`]. Passano da un file
    /// al successivo con la rotazione, o chi ripete riavrebbe il suo tetto
    /// intero a ogni file.
    ripetute: Ripetute,
}

/// Quante volte la **stessa** riga si scrive, in una finestra di
/// [`FINESTRA_DELLE_RIPETUTE_MS`], prima che ci si limiti a contarla.
const RIPETUTE_AMMESSE: u32 = 3;

/// La finestra in cui una riga uguale si conta: un minuto.
const FINESTRA_DELLE_RIPETUTE_MS: i64 = 60_000;

/// Quante righe diverse si tengono d'occhio insieme. Oltre, una riga nuova si
/// scrive e basta, senza essere contata: la memoria del diario ha un limite
/// anche quando chi lo riempie scrive righe tutte diverse.
const RIGHE_TENUTE_D_OCCHIO: usize = 256;

/// Quanto di una riga ripetuta si riporta nelle righe che la raccontano.
const TESTO_DELLA_RIPETUTA: usize = 160;

/// Una riga che si sta ripetendo, e da quanto.
#[derive(Debug)]
struct Osservata {
    /// Quando è passata la prima volta, in questa finestra.
    dal: i64,
    /// Quante volte è stata scritta, in questa finestra.
    scritte: u32,
    /// Quante volte è stata omessa, in questa finestra.
    omesse: u32,
    /// Il suo testo, per dire quale riga si ripeteva: solo quando si comincia
    /// a ometterla, perché una riga che non si ripete non lo deve pagare.
    testo: Option<Box<str>>,
}

/// Le righe che il diario ha scritto da poco, e quante volte.
///
/// # Perché un tetto per riga uguale e non per tutto il diario
///
/// Perché quel che riempie un diario non è mai la varietà ma la ripetizione:
/// la stessa riga a ogni richiesta di un dispositivo collegato (la libreria
/// occupata da una scansione, una domanda che cade), a ogni giro di un filo
/// che riprova. Un tetto sul totale toglierebbe righe diverse e vere insieme
/// alle ripetute; qui le prime volte passano tutte, e solo la quarta in poi
/// della stessa riga, dentro lo stesso minuto, si conta senza scriverla.
///
/// Il diario lo dice, con due righe: una alla prima omessa («si ripete: le
/// uguali si contano e non si scrivono») e una alla fine del minuto («altre N
/// volte»). Un'omissione non è mai silenziosa.
///
/// # Dove sta
///
/// Dentro il lucchetto del diario e non fuori: la riga si decide mentre si
/// scrive, dalla stessa mano, e un secondo lucchetto sarebbe quello che il
/// gancio dei panici — che lo chiede con `try_lock` — non conosce. Il gancio
/// e la riga d'emergenza non passano di qui: quel che dicono si scrive sempre.
///
/// Pura rispetto all'orologio, che le passa chi chiama: la prova non aspetta un
/// minuto.
#[derive(Debug)]
struct Ripetute {
    /// Per l'impronta della riga, senza l'istante.
    per_riga: HashMap<u64, Osservata>,
    /// Quando scade la prima finestra: finché l'ora è prima, niente da chiudere.
    scade_alle: i64,
}

impl Default for Ripetute {
    fn default() -> Self {
        Self {
            per_riga: HashMap::new(),
            scade_alle: i64::MAX,
        }
    }
}

/// Cosa fare di una riga che sta per essere scritta.
#[derive(Debug, PartialEq, Eq)]
struct Vaglio {
    /// La riga va scritta.
    scrivi: bool,
    /// Righe da scrivere **prima**, che raccontano quel che si è omesso.
    racconti: Vec<String>,
}

impl Ripetute {
    /// Decide se la riga `testo`, che passa all'ora `ora_ms`, si scrive.
    fn vaglia(&mut self, testo: &str, ora_ms: i64) -> Vaglio {
        let mut racconti = Vec::new();
        if ora_ms >= self.scade_alle {
            self.chiudi_le_scadute(ora_ms, &mut racconti);
        }
        let mut hasher = DefaultHasher::new();
        testo.hash(&mut hasher);
        let impronta = hasher.finish();
        let scrivi = if let Some(osservata) = self.per_riga.get_mut(&impronta) {
            if osservata.scritte < RIPETUTE_AMMESSE {
                osservata.scritte += 1;
                true
            } else {
                if osservata.omesse == 0 {
                    let riportata = riporta(testo);
                    racconti.push(format!(
                        "[diario] questa riga si ripete: per un minuto le uguali si contano e non si scrivono: {riportata}"
                    ));
                    osservata.testo = Some(riportata.into());
                }
                osservata.omesse = osservata.omesse.saturating_add(1);
                false
            }
        } else {
            if self.per_riga.len() < RIGHE_TENUTE_D_OCCHIO {
                self.per_riga.insert(
                    impronta,
                    Osservata {
                        dal: ora_ms,
                        scritte: 1,
                        omesse: 0,
                        testo: None,
                    },
                );
                self.scade_alle = self
                    .scade_alle
                    .min(ora_ms.saturating_add(FINESTRA_DELLE_RIPETUTE_MS));
            }
            true
        };
        Vaglio { scrivi, racconti }
    }

    /// Chiude le finestre scadute: dice quante righe si sono omesse, e smette di
    /// tenerle d'occhio.
    fn chiudi_le_scadute(&mut self, ora_ms: i64, racconti: &mut Vec<String>) {
        self.per_riga.retain(|_, osservata| {
            if ora_ms.saturating_sub(osservata.dal) < FINESTRA_DELLE_RIPETUTE_MS {
                return true;
            }
            if osservata.omesse > 0 {
                racconti.push(format!(
                    "[diario] altre {} volte in un minuto, non scritte: {}",
                    osservata.omesse,
                    osservata.testo.as_deref().unwrap_or_default()
                ));
            }
            false
        });
        self.scade_alle = self
            .per_riga
            .values()
            .map(|osservata| osservata.dal.saturating_add(FINESTRA_DELLE_RIPETUTE_MS))
            .min()
            .unwrap_or(i64::MAX);
    }
}

/// Il principio di una riga, per raccontarla: al più [`TESTO_DELLA_RIPETUTA`]
/// caratteri, senza rompere un accento.
fn riporta(testo: &str) -> String {
    let mut riportata: String = testo.chars().take(TESTO_DELLA_RIPETUTA).collect();
    if riportata.len() < testo.len() {
        riportata.push('…');
    }
    riportata
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
/// comportamento giusto: chi ha aperto Rymembra voleva ascoltare, non registrare.
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
        "[diario] Rymembra {} — sessione aperta{}",
        env!("CARGO_PKG_VERSION"),
        sistema_operativo()
    );
}

/// Su che telefono, per la prima riga: «Android 17 (API 37), A059P». Sul
/// telefono è la seconda domanda davanti a un guasto, dopo la versione — da
/// Android 17 la rete di casa vuole un permesso che prima non esisteva — e il
/// diario arriva dal telefono di qualcun altro. Sul computer niente.
fn sistema_operativo() -> String {
    #[cfg(target_os = "android")]
    {
        rymembra_android::sistema::versione()
            .map(|v| format!(" — Android {} (API {}), {}", v.android, v.api, v.modello))
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "android"))]
    {
        String::new()
    }
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
        ripetute: Ripetute::default(),
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
    let pieno = scrivi_la_riga_con_tetto(&mut preso(), argomenti, adesso_ms());

    if pieno {
        ruota();
    }
}

/// Scrive una riga nel diario aperto sotto il tetto delle righe uguali
/// ([`Ripetute`]), con l'istante `ora_ms` davanti. `true` se il file ha passato
/// il tetto di grandezza e va ruotato: vedi [`scrivi_la_riga`].
///
/// È la via di [`nota()`], cioè di tutte le righe normali. Il gancio dei panici
/// e la riga d'emergenza non passano di qui: quel che dicono non si omette.
fn scrivi_la_riga_con_tetto(
    guardia: &mut Option<Aperto>,
    argomenti: std::fmt::Arguments<'_>,
    ora_ms: i64,
) -> bool {
    let Some(aperto) = guardia.as_mut() else {
        return false;
    };
    let testo = testo_della_riga(argomenti);
    let vaglio = aperto.ripetute.vaglia(&testo, ora_ms);
    for racconto in &vaglio.racconti {
        aperto.scrivi(&riga_con_istante(racconto, ora_ms));
    }
    if vaglio.scrivi {
        aperto.scrivi(&riga_con_istante(&testo, ora_ms));
    }
    aperto.scritti >= TETTO
}

/// Scrive una riga nel diario aperto, con l'istante davanti, sotto una guardia
/// che chi chiama ha già preso. `true` se il file ha passato il tetto e va
/// ruotato — cosa che tocca a chi chiama, **dopo** aver lasciato la guardia.
///
/// Una funzione a parte perché le strade per arrivarci sono due: [`nota()`], che
/// il lucchetto lo aspetta e passa dal tetto delle righe uguali
/// ([`scrivi_la_riga_con_tetto`]), e [`nota_dal_gancio`], che non può aspettare
/// e non omette niente.
fn scrivi_la_riga(guardia: &mut Option<Aperto>, argomenti: std::fmt::Arguments<'_>) -> bool {
    let Some(aperto) = guardia.as_mut() else {
        return false;
    };
    aperto.scrivi(&componi_la_riga(argomenti));
    aperto.scritti >= TETTO
}

impl Aperto {
    /// Scrive una riga già composta, a capo compreso, e conta i byte.
    fn scrivi(&mut self, riga: &str) {
        // Un errore di scrittura non si racconta a nessuno: raccontarlo vorrebbe
        // dire scriverlo sul diario che non si riesce a scrivere.
        if self.file.write_all(riga.as_bytes()).is_ok() {
            self.scritti = self
                .scritti
                .saturating_add(u64::try_from(riga.len()).unwrap_or(0));
        }
        // `flush` a ogni riga, e sì, costa. È il prezzo del solo caso che conta: se
        // il processo muore un istante dopo — ed è precisamente quando si vuole
        // leggere l'ultima riga — un buffer non ancora svuotato è la riga che
        // spiegava tutto, persa.
        let _ = self.file.flush();
    }
}

/// Il messaggio di una riga come va su disco: i caratteri di controllo resi
/// visibili ([`visibile`]) e senza il nome dell'utente ([`senza_utente`]). Senza
/// l'istante: è da questo che si riconosce una riga uguale a un'altra.
fn testo_della_riga(argomenti: std::fmt::Arguments<'_>) -> String {
    let messaggio = argomenti.to_string();
    senza_utente(&visibile(&messaggio), cartelle_dell_utente()).into_owned()
}

/// L'istante, il messaggio già composto e l'a capo.
fn riga_con_istante(testo: &str, ora_ms: i64) -> String {
    format!("{} {testo}\n", istante_iso(ora_ms))
}

/// La riga come va su disco, nel diario o nel file di riserva: l'istante, il
/// messaggio con i caratteri di controllo resi visibili ([`visibile`]), l'a
/// capo, e senza il nome dell'utente ([`senza_utente`]).
///
/// Una sola per le due strade di scrittura, perché sono le due regole che
/// fanno di un file una sequenza di righe di cui fidarsi: una che ne
/// dimenticasse una delle due sarebbe il buco da cui passa tutto.
fn componi_la_riga(argomenti: std::fmt::Arguments<'_>) -> String {
    riga_con_istante(&testo_della_riga(argomenti), adesso_ms())
}

/// Il messaggio con gli a capo e gli altri caratteri di controllo scritti in
/// chiaro: `\n`, `\r`, `\t`, e `\u{…}` per il resto.
///
/// # Perché il diario non si fida di quel che scrive
///
/// Perché una riga per evento, con l'istante davanti, è la struttura su cui
/// chi legge il diario costruisce la sua storia, e dentro le righe finisce
/// testo che arriva da fuori: il corpo d'errore di un servizio (fino a
/// cinquecento caratteri, a capo compresi), la piattaforma che dichiara un
/// dispositivo che si collega, un ordinamento mandato dalla pagina. Un a capo
/// lì dentro seguito da `2026-01-01T00:00:00Z [PANICO] …` era una riga finta
/// indistinguibile da una vera; un `\r` o un controllo bidirezionale nascondono
/// una parte della riga nell'editor di chi la legge. Resi visibili, restano
/// tutti dentro la riga a cui appartengono.
///
/// Oltre ai controlli veri e propri (C0, `DEL`, C1) anche i due separatori di
/// riga e di paragrafo di Unicode, che certi editor trattano come a capo, e i
/// segni che cambiano la direzione del testo. La barra rovesciata no: è il
/// separatore dei percorsi di Windows, e [`senza_utente`] li cerca così.
fn visibile(testo: &str) -> Cow<'_, str> {
    if !testo.chars().any(da_rendere_visibile) {
        return Cow::Borrowed(testo);
    }
    let mut fuori = String::with_capacity(testo.len().saturating_add(16));
    for carattere in testo.chars() {
        match carattere {
            '\n' => fuori.push_str("\\n"),
            '\r' => fuori.push_str("\\r"),
            '\t' => fuori.push_str("\\t"),
            altro if da_rendere_visibile(altro) => {
                let _ = std::fmt::Write::write_fmt(
                    &mut fuori,
                    format_args!("\\u{{{:x}}}", u32::from(altro)),
                );
            }
            altro => fuori.push(altro),
        }
    }
    Cow::Owned(fuori)
}

/// Un carattere che nel diario non si scrive così com'è: vedi [`visibile`].
fn da_rendere_visibile(carattere: char) -> bool {
    carattere.is_control()
        || matches!(
            carattere,
            '\u{2028}'
                | '\u{2029}'
                | '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

/// Le cartelle dell'utente che non si scrivono per esteso, con il loro
/// segnaposto, dalla più lunga alla più corta.
///
/// Lette una volta sola dall'ambiente del processo: `%LOCALAPPDATA%` e
/// `%APPDATA%` prima di `%USERPROFILE%`, perché stanno dentro di lui e la
/// sostituzione più lunga deve vincere — altrimenti la riga d'avvio direbbe
/// `%USERPROFILE%\AppData\Roaming\…`, che è vero ma meno utile. Una variabile
/// assente o troppo corta per essere una cartella (`C:\`) non si usa: togliere
/// `C:` da ogni riga non nasconderebbe nessuno e renderebbe il diario
/// illeggibile.
fn cartelle_dell_utente() -> &'static [(String, &'static str)] {
    static CARTELLE: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    CARTELLE.get_or_init(|| {
        let mut cartelle: Vec<(String, &'static str)> = [
            ("LOCALAPPDATA", "%LOCALAPPDATA%"),
            ("APPDATA", "%APPDATA%"),
            ("USERPROFILE", "%USERPROFILE%"),
        ]
        .into_iter()
        .filter_map(|(variabile, segnaposto)| {
            let valore = std::env::var(variabile).ok()?;
            let pulito = valore.trim().trim_end_matches(['\\', '/']).to_owned();
            (pulito.len() > 3).then_some((pulito, segnaposto))
        })
        .collect();
        cartelle.sort_by_key(|c| std::cmp::Reverse(c.0.len()));
        cartelle
    })
}

/// Una riga del diario senza il nome dell'utente dentro.
///
/// Ogni cartella di `cartelle` diventa il suo segnaposto, **senza badare alle
/// maiuscole**: Windows non ci bada, e lo stesso percorso arriva scritto
/// `C:\Users\mario` da un posto e `c:\users\Mario` da un altro. Il confronto
/// abbassa solo le lettere ASCII, che non cambiano la lunghezza in byte, e così
/// le posizioni trovate nella copia valgono anche nell'originale.
///
/// Pura, con le cartelle passate da fuori, perché è la riga che decide se un
/// diario spedito porta un nome e un cognome: una riga così si prova.
fn senza_utente<'a>(riga: &'a str, cartelle: &[(String, &str)]) -> Cow<'a, str> {
    let mut fuori = Cow::Borrowed(riga);
    for (cartella, segnaposto) in cartelle {
        let basso = fuori.to_ascii_lowercase();
        let cercato = cartella.to_ascii_lowercase();
        if cercato.is_empty() || !basso.contains(&cercato) {
            continue;
        }
        let mut nuova = String::with_capacity(fuori.len());
        let mut da = 0_usize;
        for (inizio, _) in basso.match_indices(&cercato) {
            nuova.push_str(fuori.get(da..inizio).unwrap_or_default());
            nuova.push_str(segnaposto);
            da = inizio.saturating_add(cercato.len());
        }
        nuova.push_str(fuori.get(da..).unwrap_or_default());
        fuori = Cow::Owned(nuova);
    }
    fuori
}

/// Sposta i file di uno e riapre un diario vuoto.
///
/// Il file corrente si chiude **prima** di rinominarlo: su Windows un file
/// aperto non si rinomina, e una rotazione che fallisce a metà lascerebbe un
/// diario che non cresce più.
///
/// # Il tetto si ricontrolla col lucchetto in mano
///
/// [`nota()`] scrive, lascia il lucchetto, e solo dopo chiama questa: in mezzo
/// un altro filo può scrivere la sua riga, trovare anche lui il contatore
/// oltre il tetto, e chiamarla a sua volta. Senza ricontrollare, la seconda
/// chiamata trovava il diario appena riaperto e ruotava di nuovo: il file
/// pieno passava da `.1` a `.2` al posto di quello di prima, e in `.1` restava
/// un file di poche righe — due megabyte di storia persi.
fn ruota() {
    let mut guardia = preso();
    if guardia.as_ref().is_none_or(|aperto| aperto.scritti < TETTO) {
        return;
    }
    let Some(aperto) = guardia.take() else {
        return;
    };
    let cartella = aperto.cartella;
    let ripetute = aperto.ripetute;
    drop(aperto.file);

    // Dal più vecchio al più giovane: al contrario, ogni rinomina
    // sovrascriverebbe il file che sta per essere spostato a sua volta. Quel
    // che è invecchiato oltre l'ultimo file se ne va qui, senza un passo a
    // parte: `fs::rename` su Windows sostituisce la destinazione, e il
    // `.2` di prima è proprio la destinazione dell'ultima rinomina. Un `.3`
    // non nasce mai.
    for indice in (1..QUANTI).rev() {
        let da = if indice == 1 {
            cartella.join(NOME)
        } else {
            cartella.join(format!("rymembra.{}.log", indice - 1))
        };
        let _ = fs::rename(da, cartella.join(format!("rymembra.{indice}.log")));
    }

    *guardia = apri_il_file(cartella).map(|mut nuovo| {
        nuovo.ripetute = ripetute;
        nuovo
    });
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
/// # Perché è l'unica occasione in cui resta la traccia
///
/// Il profilo di rilascio dichiara `panic = "unwind"` — la ragione sta in
/// `Cargo.toml`, ed è che un panico su un filo qualunque non deve portarsi via
/// il database di chi ascolta musica. Lo svolgimento dello stack quindi c'è, e
/// il processo non termina qui.
///
/// Resta però che questo è l'unico posto in cui si sappia **da dove** il panico
/// veniva. Il gancio gira prima dello svolgimento, cioè mentre lo stack esiste
/// ancora: la posizione nel sorgente e il messaggio sono qui, e da qui in poi
/// non li ha più nessuno. Un `catch_unwind` riceve il carico — una `String` o
/// uno `&str` — e nient'altro: né file, né riga, né la catena delle chiamate.
///
/// Nell'albero ce n'è **uno**, `rymembra_play::motore::filo_sorvegliato`, che
/// raccoglie i pezzi del filo di decodifica per ricostruire il motore invece di
/// lasciarlo suonare zeri; quel che ne ricava è il messaggio, e lo mette in un
/// evento per la finestra. Il perché il filo era caduto lì lo scrive questo
/// gancio, e se non scrive lui non scrive nessuno.
///
/// Va installato per primo in `main`, prima ancora di costruire la finestra.
/// Finché il diario non è aperto le righe finiscono nel file di riserva
/// ([`RISERVA`]) — è il caso della WebView2 che non si crea, prima del
/// `setup` — e dal momento in cui `setup` chiama [`apri`] in poi, cioè per
/// tutta la vita utile del processo, nel diario.
///
/// Il panico di un avvio fallito ([`e_l_avvio_fallito`]) mostra anche un
/// dialogo ([`mostra_l_avvio_fallito`]): la finestra non è comparsa e non
/// comparirà, e senza il processo spariva senza dire niente a chi l'aveva
/// aperto. Il dialogo viene **dopo** la riga su disco, che resta anche se lui
/// non si mostrasse.
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
        let scritta = nota_dal_gancio(format_args!("[PANICO] filo={filo} a {dove}: {info}"));
        precedente(info);
        // Il testo del panico: una `String` per un `panic!` con argomenti,
        // come quello di Tauri, uno `&str` per un testo fisso. Non
        // `payload_as_str`, che chiede Rust 1.91 (il crate dice 1.90).
        let carico = info.payload();
        let messaggio = carico
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| carico.downcast_ref::<&str>().copied());
        if e_l_avvio_fallito(messaggio, &filo) {
            mostra_l_avvio_fallito(&testo_dell_avvio_fallito(
                messaggio.unwrap_or_default(),
                scritta.as_deref(),
            ));
        }
    }));
}

/// È il panico di un avvio fallito?
///
/// In Tauri 2 il `setup` gira dentro `App::run`, all'evento `Ready`, dopo aver
/// creato le finestre di `tauri.conf.json`; se fallisce — la WebView2 assente o
/// rotta, la cartella dei dati che non si risolve, un `?` nel nostro `setup` —
/// Tauri lo trasforma in `panic!("Failed to setup app: …")`, sul filo
/// principale. Solo lui merita il dialogo: un panico in un filo di sottofondo
/// lo racconta il diario, e l'applicazione va avanti.
fn e_l_avvio_fallito(messaggio: Option<&str>, filo: &str) -> bool {
    filo == "main" && messaggio.is_some_and(|testo| testo.starts_with("Failed to setup app"))
}

/// Il testo del dialogo di un avvio fallito: che cosa è successo, il rimedio
/// più probabile, dove sta la riga per chi deve capire, e il motivo.
///
/// Bilingue, italiano poi inglese separati da una riga vuota, come il dialogo
/// del trasloco (`rymembra_app::trasloco::messaggio_di_rifiuto`): la finestra e
/// le sue traduzioni non ci sono. Il motivo — il testo dell'errore di Tauri, in
/// inglese — sta una volta sola, in fondo, ridotto a una riga
/// ([`una_riga_sola`]). Il posto della riga passa da [`senza_utente`]:
/// `%LOCALAPPDATA%\…` si incolla così com'è nella barra di Esplora risorse.
#[must_use]
pub fn testo_dell_avvio_fallito(motivo: &str, scritta: Option<&Path>) -> String {
    // Il posto si **dice** a chi guarda: dove lo trova lui. Nella copia del
    // Store il file sta nella cartella privata del pacchetto (R20-4).
    let dove = scritta.map(|percorso| {
        let visibile = crate::pacchetto::per_esplora(percorso);
        senza_utente(&visibile.display().to_string(), cartelle_dell_utente()).into_owned()
    });
    let (dove_it, dove_en) = dove.map_or_else(
        || (String::new(), String::new()),
        |dove| {
            (
                format!(" Il motivo è scritto anche in {dove}."),
                format!(" The reason is also written in {dove}."),
            )
        },
    );
    format!(
        "Rymembra non è riuscito ad avviarsi. Spesso la causa è Microsoft Edge WebView2 Runtime, \
         mancante o danneggiato: reinstallalo (o reinstalla Rymembra) e riprova.{dove_it}\n\n\
         Rymembra could not start. Often the cause is a missing or damaged Microsoft Edge \
         WebView2 Runtime: reinstall it (or reinstall Rymembra) and try again.{dove_en}\n\n{}",
        una_riga_sola(motivo)
    )
}

/// Il dialogo di un avvio fallito, nativo e bloccante: torna quando lo si
/// chiude.
///
/// `rfd` e non `tauri-plugin-dialog`, perché quello passa dal ciclo degli
/// eventi, che qui non è mai nato (un `build` fallito) o sta cadendo (il
/// panico del `setup`). Una volta sola per processo.
pub fn mostra_l_avvio_fallito(testo: &str) {
    static MOSTRATO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if MOSTRATO.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    // Nelle prove mai: un dialogo bloccante fermerebbe la prova per sempre.
    //
    // E su Android nemmeno: `rfd` lì non esiste, e un dialogo nativo fuori dal
    // ciclo degli eventi vorrebbe un'attività che a questo punto non c'è. La
    // traccia resta il file di riserva, come per chi non legge mai i dialoghi.
    #[cfg(all(not(test), desktop))]
    {
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Rymembra")
            .set_description(testo)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
    #[cfg(any(test, mobile))]
    let _ = testo;
}

/// Quante volte il gancio dei panici chiede il lucchetto del diario prima di
/// rinunciare. Vedi [`nota_dal_gancio`].
const TENTATIVI_DAL_GANCIO: u32 = 20;

/// La pausa fra due richieste del gancio: venti per un millisecondo sono al
/// più qualche decina di millisecondi di ritardo su un filo che sta cadendo.
const PAUSA_DAL_GANCIO: std::time::Duration = std::time::Duration::from_millis(1);

/// Scrive una riga dal gancio dei panici, senza mai aspettare il lucchetto del
/// diario più di un attimo.
///
/// # Perché non [`nota()`]
///
/// Perché il panico può arrivare **mentre questo stesso filo tiene il
/// lucchetto**: dentro [`nota()`], con la guardia presa — un formattatore di terzi
/// fra gli argomenti, una scrittura che cade. Il gancio gira prima dello
/// svolgimento, quando la guardia non è ancora stata lasciata, e un `Mutex` di
/// `std` non è rientrante: `nota` lì dentro aspettava se stesso, per sempre. E
/// con lui ogni altro filo al suo primo `nota!`, cioè l'applicazione intera.
///
/// Qui il lucchetto si chiede con `try_lock`, qualche volta e con una pausa
/// breve: un altro filo che sta scrivendo una riga la finisce in microsecondi,
/// e la riga del panico entra lo stesso nel file. Se resta occupato, è quasi
/// certamente questo filo a tenerlo, e la riga va su `stderr` — in rilascio da
/// nessuna parte, ma una riga persa è meglio di un'applicazione ferma.
///
/// Un lucchetto avvelenato si prende lo stesso, come in [`preso`]: è proprio il
/// momento in cui scrivere serve di più.
///
/// Nessuna rotazione da qui, nemmeno a diario pieno: [`ruota`] riprende il
/// lucchetto aspettandolo. La fa la prossima riga normale.
///
/// Restituisce dove è andata la riga — la cartella del diario o il file di
/// riserva — per il dialogo di un avvio fallito, che lo dice: lo si legge qui,
/// con la guardia già in mano, perché il gancio non deve chiedere il lucchetto
/// una seconda volta. `None` se la riga non è andata su disco.
fn nota_dal_gancio(argomenti: std::fmt::Arguments<'_>) -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    eprintln!("{argomenti}");

    for _ in 0..TENTATIVI_DAL_GANCIO {
        let mut guardia = match cella().try_lock() {
            Ok(guardia) => guardia,
            Err(TryLockError::Poisoned(avvelenato)) => avvelenato.into_inner(),
            Err(TryLockError::WouldBlock) => {
                std::thread::sleep(PAUSA_DAL_GANCIO);
                continue;
            }
        };
        // A diario chiuso — il panico dell'avvio, prima che il `setup` lo
        // apra — la riga va nel file di riserva: vedi [`RISERVA`].
        if guardia.is_none() {
            drop(guardia);
            return scrivi_di_riserva(argomenti);
        }
        let _ = scrivi_la_riga(&mut guardia, argomenti);
        return guardia.as_ref().map(|aperto| aperto.cartella.clone());
    }
    // In sviluppo la riga è già su `stderr`, qui sopra. `writeln!` e non
    // `eprintln!`: questo è un gancio dei panici, e un `eprintln!` che non
    // riesce a scrivere va in panico a sua volta — dentro il gancio, cioè
    // l'interruzione del processo.
    #[cfg(not(debug_assertions))]
    {
        let _ = writeln!(std::io::stderr(), "{argomenti}");
    }
    None
}

/// Una riga che deve restare anche se il diario non si è aperto: quella di un
/// avvio fallito.
///
/// A diario aperto va nel diario come ogni altra riga; a diario chiuso nel
/// file di riserva ([`RISERVA`]). Non è la strada delle righe normali: vedi
/// là perché. Restituisce dove è andata, come [`nota_dal_gancio`], per il
/// dialogo che la nomina.
pub fn nota_di_emergenza(argomenti: std::fmt::Arguments<'_>) -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    eprintln!("{argomenti}");

    let mut guardia = preso();
    if guardia.is_none() {
        drop(guardia);
        return scrivi_di_riserva(argomenti);
    }
    let pieno = scrivi_la_riga(&mut guardia, argomenti);
    let cartella = guardia.as_ref().map(|aperto| aperto.cartella.clone());
    drop(guardia);
    if pieno {
        ruota();
    }
    cartella
}

/// Scrive una riga nel file di riserva, con l'istante davanti e senza il nome
/// dell'utente, come nel diario. Ogni errore si ignora, come nel diario.
/// Restituisce il file, se la riga c'è entrata.
fn scrivi_di_riserva(argomenti: std::fmt::Arguments<'_>) -> Option<PathBuf> {
    let cartella = cartella_di_riserva()?;
    fs::create_dir_all(&cartella).ok()?;
    let percorso = cartella.join(RISERVA);
    let pieno = fs::metadata(&percorso).is_ok_and(|m| m.len() >= TETTO_RISERVA);
    let mut opzioni = OpenOptions::new();
    if pieno {
        opzioni.create(true).write(true).truncate(true);
    } else {
        opzioni.create(true).append(true);
    }
    let mut file = opzioni.open(&percorso).ok()?;
    let riga = componi_la_riga(argomenti);
    file.write_all(riga.as_bytes()).ok()?;
    let _ = file.flush();
    Some(percorso)
}

/// Dove sta il file di riserva: `%LOCALAPPDATA%\<identificatore>`, o la
/// cartella temporanea se quella variabile manca.
#[cfg(not(test))]
fn cartella_di_riserva() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|cartella| cartella.is_absolute())
        .unwrap_or_else(std::env::temp_dir);
    Some(base.join(IDENTIFICATORE))
}

/// Nelle prove la cartella la dà la prova, e di serie non c'è: nessuna prova
/// deve scrivere nel `%LOCALAPPDATA%` vero.
#[cfg(test)]
static RISERVA_DI_PROVA: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(test)]
fn cartella_di_riserva() -> Option<PathBuf> {
    RISERVA_DI_PROVA
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Le prove che toccano il diario del processo si danno il turno.
///
/// Il diario è **uno per processo** — è il suo punto — e le prove girano
/// tutte nello stesso. Senza questo lucchetto, quella che lo chiude per
/// verificare che un diario chiuso non faccia danni lo chiuderebbe sotto i
/// piedi di quella che sta misurando la rotazione, e a fallire sarebbe la
/// seconda: il difetto peggiore che una prova possa avere. Sta fuori da
/// `prove` perché lo prendono anche le prove di altri moduli, attraverso
/// [`a_diario_chiuso`].
#[cfg(test)]
static IL_TURNO: Mutex<()> = Mutex::new(());

/// Per le prove di altri moduli: esegue `prova` con il diario **aperto** in una
/// cartella temporanea, e restituisce tutto quel che ci è stato scritto.
///
/// È la condizione dopo [`apri`]: quel che una prova vuole sapere è se una riga
/// finisce nel diario di chi legge — a diario chiuso le righe di `nota!` non
/// vanno da nessuna parte, e solo [`a_diario_chiuso`] vede le righe di riserva.
#[cfg(test)]
pub(crate) fn a_diario_aperto(prova: impl FnOnce()) -> String {
    let _turno = IL_TURNO.lock().unwrap_or_else(PoisonError::into_inner);
    let Ok(radice) = tempfile::tempdir() else {
        return String::new();
    };
    apri(radice.path());
    prova();
    let testo = fs::read_to_string(radice.path().join(CARTELLA).join(NOME)).unwrap_or_default();
    // Si chiude, o su Windows la cartella temporanea non si cancella.
    *preso() = None;
    testo
}

/// Per le prove di altri moduli: esegue `prova` a diario chiuso, col file di
/// riserva in una cartella temporanea, e restituisce quel che ci è finito.
///
/// È la condizione dell'avvio prima di [`apri`]: quel che una prova vuole
/// sapere è se una riga di quel momento resta su disco.
#[cfg(test)]
pub(crate) fn a_diario_chiuso(prova: impl FnOnce()) -> String {
    let _turno = IL_TURNO.lock().unwrap_or_else(PoisonError::into_inner);
    let Ok(riserva) = tempfile::tempdir() else {
        return String::new();
    };
    *RISERVA_DI_PROVA
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(riserva.path().to_path_buf());
    *preso() = None;
    prova();
    *RISERVA_DI_PROVA
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = None;
    fs::read_to_string(riserva.path().join(RISERVA)).unwrap_or_default()
}

/// Apre la cartella del diario nel gestore file del sistema.
///
/// # Perché un bottone e non un percorso scritto in una schermata
///
/// Perché `%APPDATA%\io.github.federicobaratti.rymembra\diario` non è un posto in
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
pub async fn diario_apri() -> Esito<()> {
    // Sul computer chiede al sistema di aprire una cartella (un processo, e su
    // una cartella dentro il pacchetto del Store un percorso da tradurre); sul
    // telefono copia il diario e apre la condivisione. Aspetta tutte e due.
    comanda("apertura del diario", || {
        let Some(dove) = cartella() else {
            return Err(errore(
                AppError::new(ErrorCode::InternalAborted {
                    what: Some("apertura della cartella del diario".to_owned()),
                })
                .with_cause(
                    "il diario non è stato aperto: la cartella dei dati non è scrivibile"
                        .to_owned(),
                ),
            ));
        };
        #[cfg(target_os = "android")]
        return condividi_sul_telefono(&dove);
        #[cfg(not(target_os = "android"))]
        {
            // Nella copia del Store la cartella del diario è nuova, quindi sta
            // nella cartella privata del pacchetto, e Esplora risorse — che gira
            // fuori — non vede il percorso di dentro (R20-4). Vedi `pacchetto`.
            let dove = crate::pacchetto::per_esplora(&dove);
            tauri_plugin_opener::open_path(&dove, None::<&str>).map_err(|err| {
                errore(
                    AppError::new(ErrorCode::InternalAborted {
                        what: Some("apertura della cartella del diario".to_owned()),
                    })
                    .with_cause(err.to_string()),
                )
            })
        }
    })
    .await
}

/// [`diario_apri`] sul telefono: una cartella non c'è nessuno che la apra —
/// i dati dell'app sono privati, e il gestore file di Android lì non entra —
/// quindi il diario si **condivide**: posta, messaggi, «Salva in File». Una
/// copia in `cache/diario`, perché è una delle due cartelle che il
/// `FileProvider` sa mostrare alle altre app (`res/xml/file_paths.xml`, con
/// quella degli aggiornamenti); il diario vero resta dov'è.
#[cfg(target_os = "android")]
fn condividi_sul_telefono(dove: &Path) -> Esito<()> {
    let guasto = |causa: String| {
        errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("condivisione del diario".to_owned()),
            })
            .with_cause(causa),
        )
    };
    let cartella = rymembra_android::sistema::cartella_cache()
        .map_err(|err| guasto(err.to_string()))?
        .join("diario");
    fs::create_dir_all(&cartella).map_err(|err| guasto(err.to_string()))?;
    let copia = cartella.join("rymembra-diario.txt");
    fs::copy(dove.join(NOME), &copia).map_err(|err| guasto(err.to_string()))?;
    rymembra_android::sistema::condividi(&copia, "text/plain", "Rymembra")
        .map_err(|err| guasto(err.to_string()))
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
pub async fn diario_annota(dove: String, cosa: String) {
    // Sul pool bloccante: `nota` prende il lucchetto del diario e scrive su
    // disco, e può ruotare il file. Un disco lento non deve fermare la pagina
    // che sta raccontando il proprio guasto. L'esito non si guarda: se la
    // scrittura cade, il diario non ha dove dirlo.
    let _ = in_disparte("annotazione della finestra", move || {
        nota(format_args!(
            "[{}] {}",
            una_riga_sola(&dove),
            una_riga_sola(&cosa)
        ));
    })
    .await;
}

#[cfg(test)]
mod prove {
    use super::*;

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
    fn un_avvio_fallito_lascia_una_riga_anche_senza_diario() {
        // Il caso vero: la WebView2 non si crea, il `setup` non arriva ad aprire
        // il diario, e il processo se ne va. La riga del ramo d'errore e quella
        // del gancio dei panici devono restare su disco.
        let _turno = IL_TURNO.lock().unwrap_or_else(PoisonError::into_inner);
        let Ok(riserva) = tempfile::tempdir() else {
            return;
        };
        *RISERVA_DI_PROVA
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(riserva.path().to_path_buf());
        *preso() = None;
        nota_di_emergenza(format_args!(
            "[avvio] Rymembra non è riuscito ad avviarsi: x"
        ));
        nota_dal_gancio(format_args!(
            "[PANICO] filo=main a qui: Failed to setup app: y"
        ));
        *RISERVA_DI_PROVA
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        let testo = fs::read_to_string(riserva.path().join(RISERVA)).unwrap_or_default();
        assert!(
            testo.contains("[avvio] Rymembra non è riuscito ad avviarsi: x"),
            "la riga dell'avvio fallito non è da nessuna parte: {testo:?}"
        );
        assert!(
            testo.contains("Failed to setup app: y"),
            "la riga del panico a diario chiuso non è da nessuna parte: {testo:?}"
        );
        // Le righe normali, a diario chiuso, non ci vanno.
        nota!("[prova] una riga normale");
        let testo = fs::read_to_string(riserva.path().join(RISERVA)).unwrap_or_default();
        assert!(!testo.contains("una riga normale"));
    }

    /// Il dialogo è solo per il panico che Tauri solleva quando il `setup`
    /// fallisce, sul filo principale: un panico di un filo di sottofondo lo
    /// racconta il diario, e l'applicazione va avanti.
    #[test]
    fn l_avvio_fallito_si_riconosce_solo_dal_suo_panico() {
        let setup = "Failed to setup app: error encountered during setup hook: WebView2";
        assert!(e_l_avvio_fallito(Some(setup), "main"));
        assert!(!e_l_avvio_fallito(Some(setup), "rymembra-analisi"));
        assert!(!e_l_avvio_fallito(Some("index out of bounds"), "main"));
        assert!(!e_l_avvio_fallito(None, "main"));
    }

    /// Il testo del dialogo: in italiano e poi in inglese, ognuno col rimedio e
    /// col posto della riga, e il motivo una volta, in fondo e su una riga.
    #[test]
    fn il_dialogo_dell_avvio_fallito_dice_rimedio_e_posto_in_due_lingue() {
        let riserva = Path::new(r"C:\Prova\io.github.federicobaratti.rymembra\avvio.log");
        let testo = testo_dell_avvio_fallito(
            "Failed to setup app: WebView2 non trovata\n   at qualcosa di lungo",
            Some(riserva),
        );
        let parti: Vec<&str> = testo.split("\n\n").collect();
        assert_eq!(parti.len(), 3, "italiano, inglese, motivo: {testo}");
        assert!(parti[0].starts_with("Rymembra non è riuscito ad avviarsi"));
        assert!(parti[1].starts_with("Rymembra could not start"));
        for parte in &parti[..2] {
            assert!(parte.contains("WebView2"), "{parte}");
            assert!(parte.contains(&riserva.display().to_string()), "{parte}");
        }
        assert_eq!(parti[2], "Failed to setup app: WebView2 non trovata");
        // Senza riga su disco non si promette un posto che non c'è.
        let senza = testo_dell_avvio_fallito("Failed to setup app: x", None);
        assert!(!senza.contains("scritto anche in"), "{senza}");
        assert!(!senza.contains("also written in"), "{senza}");
    }

    /// Il gancio dei panici e il ramo d'errore di `build` mostrano il dialogo,
    /// **dopo** aver scritto la riga.
    #[test]
    fn il_gancio_e_il_ramo_d_errore_mostrano_il_dialogo() {
        let diario = include_str!("diario.rs");
        let gancio = diario
            .split("pub fn installa_gancio_dei_panici() {")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let riga = gancio.find("nota_dal_gancio(");
        let dialogo = gancio.find("mostra_l_avvio_fallito(");
        assert!(
            riga.is_some() && dialogo > riga,
            "il gancio non mostra il dialogo"
        );
        assert!(gancio.contains("e_l_avvio_fallito("));

        // In `lib.rs` e non più in `main.rs`: il ramo sta in `run`, che è da
        // dove entrano sia l'eseguibile di Windows sia Android.
        let main = include_str!("lib.rs");
        let ramo = main
            .split("Err(err) => {")
            .nth(1)
            .and_then(|resto| resto.split("std::process::exit(1);").next())
            .unwrap_or_default();
        let riga = ramo.find("diario::nota_di_emergenza(");
        let dialogo = ramo.find("diario::mostra_l_avvio_fallito(");
        assert!(
            riga.is_some() && dialogo > riga,
            "il ramo d'errore non mostra il dialogo"
        );
    }

    #[test]
    fn l_identificatore_della_riserva_e_quello_di_tauri_conf() {
        let configurazione = include_str!("../tauri.conf.json");
        assert!(
            configurazione.contains(&format!("\"identifier\": \"{IDENTIFICATORE}\"")),
            "l'identificatore di `tauri.conf.json` non è più «{IDENTIFICATORE}»"
        );
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
                dove.join("rymembra.1.log").exists(),
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
    fn due_rotazioni_di_fila_ne_fanno_una() {
        // Il caso vero: due fili passano il tetto insieme, e tutti e due
        // chiamano `ruota`. La seconda trovava il diario appena riaperto e
        // ruotava di nuovo: il file pieno finiva in `.2` al posto di quello
        // di prima, e in `.1` restava un file quasi vuoto.
        in_una_cartella(|radice| {
            let dove = radice.join(CARTELLA);
            riempi_fino_al_tetto();
            nota!("[prova] la riga che fa traboccare");
            ruota();
            let primo = fs::read_to_string(dove.join("rymembra.1.log")).unwrap_or_default();
            assert!(
                primo.contains("traboccare"),
                "il file pieno non è più il primo: la seconda rotazione l'ha spinto oltre"
            );
        });
    }

    #[test]
    fn oltre_l_ultimo_file_si_dimentica() {
        in_una_cartella(|radice| {
            let dove = radice.join(CARTELLA);
            // Tre rotazioni di fila: alla fine devono esserci `rymembra.log`,
            // `.1` e `.2`, e non un `.3`.
            for giro in 0..3 {
                riempi_fino_al_tetto();
                nota!("[prova] giro {giro}");
            }
            assert!(dove.join(NOME).exists(), "manca il file corrente");
            assert!(dove.join("rymembra.1.log").exists(), "manca il primo");
            assert!(dove.join("rymembra.2.log").exists(), "manca il secondo");
            assert!(
                !dove.join("rymembra.3.log").exists(),
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
    fn il_nome_dell_utente_non_finisce_nel_diario() {
        let cartelle = [
            (
                r"C:\Users\Mario Rossi\AppData\Roaming".to_owned(),
                "%APPDATA%",
            ),
            (r"C:\Users\Mario Rossi".to_owned(), "%USERPROFILE%"),
        ];
        let riga = r"[avvio] libreria aperta dati=C:\Users\Mario Rossi\AppData\Roaming\io.github.federicobaratti.rymembra migrazioni=0";
        let pulita = senza_utente(riga, &cartelle);
        assert_eq!(
            pulita,
            r"[avvio] libreria aperta dati=%APPDATA%\io.github.federicobaratti.rymembra migrazioni=0"
        );
        // Le maiuscole non contano, come per Windows; e più volte nella riga.
        let due = senza_utente(
            r"da c:\users\mario rossi\Musica a C:\USERS\MARIO ROSSI\Desktop",
            &cartelle,
        );
        assert_eq!(due, r"da %USERPROFILE%\Musica a %USERPROFILE%\Desktop");
        assert!(!due.to_lowercase().contains("mario"));
        // Una riga senza cartelle dell'utente resta quella, senza copie.
        assert!(matches!(
            senza_utente("[rete] net.offline", &cartelle),
            Cow::Borrowed(_)
        ));
        // Gli accenti di un nome non spostano niente.
        let accentata = [(r"C:\Users\Niccolò".to_owned(), "%USERPROFILE%")];
        assert_eq!(
            senza_utente(r"«C:\Users\Niccolò\x» è qui", &accentata),
            r"«%USERPROFILE%\x» è qui"
        );
    }

    /// Un testo che arriva da fuori — il corpo d'errore di un servizio, la
    /// piattaforma di un dispositivo che si collega — non scrive righe sue.
    ///
    /// Il caso vero: una causa con un a capo dentro, seguito da quel che ha
    /// l'aria di una riga del diario. Chi legge vedeva un `[PANICO]` mai
    /// successo, con il suo istante davanti.
    #[test]
    fn una_causa_su_piu_righe_resta_una_riga() {
        in_una_cartella(|radice| {
            let percorso = radice.join(CARTELLA).join(NOME);
            let prima = fs::read_to_string(&percorso).unwrap_or_default();
            nota!(
                "[x] causa={}",
                "a\n2026-01-01T00:00:00Z [PANICO] finto\r\u{0}\u{1b}[31m\u{85}\u{2028}\u{202e}fine"
            );
            let dopo = fs::read_to_string(&percorso).unwrap_or_default();
            let nuovo = dopo.get(prima.len()..).unwrap_or_default();
            // Il diario è uno solo per il processo: mentre questa prova gira,
            // le prove di altri moduli (insieme, fili) ci scrivono le loro
            // righe. Contano solo quelle che vengono dalla causa di qui: la
            // riga vera e, se il testo da fuori ne avesse scritta una sua, il
            // `[PANICO]` finto a inizio riga.
            let nostre: Vec<&str> = nuovo
                .lines()
                .filter(|r| r.contains("[x] causa=") || r.contains("[PANICO] finto"))
                .collect();
            assert_eq!(
                nostre.len(),
                1,
                "il testo da fuori ha scritto righe sue: {nuovo:?}"
            );
            let riga = nostre[0];
            assert!(
                riga.ends_with(
                    r"[x] causa=a\n2026-01-01T00:00:00Z [PANICO] finto\r\u{0}\u{1b}[31m\u{85}\u{2028}\u{202e}fine"
                ),
                "i caratteri di controllo non sono diventati visibili: {riga:?}"
            );
            // Le barre rovesciate dei percorsi restano quelle: sono il modo in
            // cui Windows scrive una cartella, e `senza_utente` le cerca.
            nota!(r"[x] dati=C:\Prova\dati");
            let ancora = fs::read_to_string(&percorso).unwrap_or_default();
            assert!(ancora.contains(r"[x] dati=C:\Prova\dati"), "{ancora:?}");
        });
    }

    /// La stessa regola vale per il file di riserva, che si scrive con la stessa
    /// riga.
    #[test]
    fn anche_la_riserva_non_si_lascia_scrivere_righe_finte() {
        let _turno = IL_TURNO.lock().unwrap_or_else(PoisonError::into_inner);
        let Ok(riserva) = tempfile::tempdir() else {
            return;
        };
        *RISERVA_DI_PROVA
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(riserva.path().to_path_buf());
        *preso() = None;
        nota_di_emergenza(format_args!(
            "[avvio] x\n2026-01-01T00:00:00Z [PANICO] finto"
        ));
        *RISERVA_DI_PROVA
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        let testo = fs::read_to_string(riserva.path().join(RISERVA)).unwrap_or_default();
        assert_eq!(testo.lines().count(), 1, "{testo:?}");
        assert!(testo.contains(r"[avvio] x\n2026"), "{testo:?}");
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

    /// Un panico che arriva mentre lo stesso filo tiene il lucchetto del
    /// diario non deve aspettare se stesso.
    ///
    /// Il caso vero: qualcosa va in panico **dentro** [`nota`], mentre la
    /// guardia è presa — un formattatore di terzi dentro gli argomenti, una
    /// scrittura che cade. Il gancio chiedeva lo stesso lucchetto sullo stesso
    /// filo, e un `Mutex` di `std` non è rientrante: il filo restava fermo per
    /// sempre, e con lui ogni altro filo che volesse scrivere una riga.
    #[test]
    fn il_gancio_non_aspetta_un_lucchetto_che_tiene_lui() {
        let _turno = IL_TURNO.lock().unwrap_or_else(PoisonError::into_inner);
        let (fatto, finito) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let guardia = preso();
            nota_dal_gancio(format_args!("[PANICO] filo=prova a qui: dentro il diario"));
            drop(guardia);
            let _ = fatto.send(());
        });
        assert!(
            finito
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_ok(),
            "il gancio si è fermato ad aspettare il lucchetto del suo stesso filo"
        );
    }

    /// Col lucchetto libero, il gancio scrive nel diario come ogni altra riga.
    #[test]
    fn il_gancio_scrive_nel_diario_quando_e_libero() {
        in_una_cartella(|radice| {
            nota_dal_gancio(format_args!("[PANICO] filo=prova a qui: fuori"));
            let Ok(testo) = fs::read_to_string(radice.join(CARTELLA).join(NOME)) else {
                return;
            };
            assert!(
                testo.contains("[PANICO] filo=prova a qui: fuori"),
                "la riga del panico non è nel diario: {testo}"
            );
        });
    }

    /// La riga che `lontano::servizio::occupata` scrive a ogni domanda caduta
    /// per la libreria occupata (S1-5, resto): sempre la stessa, a ogni
    /// richiesta di un pari mentre una scansione tiene il lucchetto.
    const RIGA_RIPETUTA: &str = "[lontano] una domanda non servita: la libreria è occupata codice=db.locked causa=la libreria è occupata: il collegamento non l'aspetta";

    #[test]
    fn cento_righe_uguali_in_un_minuto_non_sono_cento_righe() {
        // Il caso vero: con una scansione in corso un pari fa scrivere fino a
        // cento righe al minuto (otto posti, cinque secondi l'uno), tutte
        // uguali, e tre file da due megabyte sono la storia di una giornata.
        // La prova gira dentro un minuto: nessun orologio da muovere.
        in_una_cartella(|radice| {
            for _ in 0..100 {
                nota!("{RIGA_RIPETUTA}");
            }
            let testo = fs::read_to_string(radice.join(CARTELLA).join(NOME)).unwrap_or_default();
            let scritte = testo
                .lines()
                .filter(|riga| riga.contains("una domanda non servita"))
                .count();
            assert!(
                scritte <= 5,
                "100 righe uguali in un minuto: {scritte} righe nel file"
            );
            // Le prime passano intere, e una riga dice che le altre si contano.
            // Quelle intere: l'istante (24 caratteri) e uno spazio, poi la riga. La
            // riga che racconta l'omissione la porta in coda, ma non è lei.
            let intere = testo
                .lines()
                .filter(|riga| riga.get(25..) == Some(RIGA_RIPETUTA))
                .count();
            assert_eq!(intere, RIPETUTE_AMMESSE as usize, "{testo}");
            assert!(
                testo.contains("[diario] questa riga si ripete"),
                "l'omissione non è detta: {testo}"
            );
        });
    }

    #[test]
    fn passato_il_minuto_il_diario_dice_quante_ne_ha_omesse() {
        let mut ripetute = Ripetute::default();
        let inizio = 1_800_000_000_000;
        for passo in 0..100 {
            let _ = ripetute.vaglia("[prova] sempre uguale", inizio + passo);
        }
        let dopo = ripetute.vaglia("[prova] sempre uguale", inizio + FINESTRA_DELLE_RIPETUTE_MS);
        assert!(dopo.scrivi, "una finestra nuova ricomincia da capo");
        assert_eq!(dopo.racconti.len(), 1);
        assert!(
            dopo.racconti[0].contains("altre 97 volte")
                && dopo.racconti[0].contains("sempre uguale"),
            "il riepilogo non dice quante né quale: {:?}",
            dopo.racconti
        );
    }

    #[test]
    fn il_riepilogo_arriva_anche_con_la_riga_di_un_altro() {
        // Chi si ripeteva ha smesso: nessuna sua riga verrà a chiudere la
        // finestra, e il conto lo chiude la prima riga qualunque dopo il minuto
        // (alla chiusura, quella dello spegnimento).
        let mut ripetute = Ripetute::default();
        let inizio = 1_800_000_000_000;
        for passo in 0..10 {
            let _ = ripetute.vaglia("[prova] sempre uguale", inizio + passo);
        }
        let altra = ripetute.vaglia(
            "[uscita] si chiude",
            inizio + FINESTRA_DELLE_RIPETUTE_MS + 5,
        );
        assert!(altra.scrivi);
        assert_eq!(altra.racconti.len(), 1, "{:?}", altra.racconti);
        assert!(
            altra.racconti[0].contains("altre 7 volte"),
            "{:?}",
            altra.racconti
        );
    }

    #[test]
    fn righe_diverse_non_si_tolgono_la_parola() {
        let mut ripetute = Ripetute::default();
        for numero in 0..50 {
            let vaglio = ripetute.vaglia(&format!("[prova] riga {numero}"), 1_800_000_000_000);
            assert!(vaglio.scrivi && vaglio.racconti.is_empty());
        }
    }

    #[test]
    fn una_riga_che_torna_ogni_minuto_passa_sempre() {
        // L'aggiornamento che non trova la rete ripete la stessa riga ogni
        // minuto: è un fatto, non una raffica.
        let mut ripetute = Ripetute::default();
        let inizio = 1_800_000_000_000;
        for giro in 0..20 {
            let vaglio = ripetute.vaglia(
                "[aggiornamenti] controllo non riuscito",
                inizio + giro * FINESTRA_DELLE_RIPETUTE_MS,
            );
            assert!(
                vaglio.scrivi && vaglio.racconti.is_empty(),
                "al giro {giro}"
            );
        }
    }

    #[test]
    fn le_righe_tenute_d_occhio_hanno_un_limite_e_le_nuove_passano_lo_stesso() {
        let mut ripetute = Ripetute::default();
        for numero in 0..(RIGHE_TENUTE_D_OCCHIO + 100) {
            let vaglio = ripetute.vaglia(&format!("[prova] riga {numero}"), 1_800_000_000_000);
            assert!(vaglio.scrivi, "la riga {numero} non si scrive");
        }
        assert_eq!(ripetute.per_riga.len(), RIGHE_TENUTE_D_OCCHIO);
    }

    #[test]
    fn il_gancio_dei_panici_e_l_emergenza_non_omettono_niente() {
        in_una_cartella(|radice| {
            for _ in 0..20 {
                nota_dal_gancio(format_args!("[PANICO] filo=prova a qui: sempre uguale"));
                let _ = nota_di_emergenza(format_args!("[avvio] sempre uguale"));
            }
            let testo = fs::read_to_string(radice.join(CARTELLA).join(NOME)).unwrap_or_default();
            assert_eq!(testo.matches("[PANICO] filo=prova").count(), 20, "{testo}");
            assert_eq!(
                testo.matches("[avvio] sempre uguale").count(),
                20,
                "{testo}"
            );
        });
    }

    #[test]
    fn la_rotazione_non_rida_il_tetto_a_chi_si_ripete() {
        in_una_cartella(|radice| {
            for _ in 0..10 {
                nota!("{RIGA_RIPETUTA}");
            }
            riempi_fino_al_tetto();
            nota!("[prova] la riga che fa traboccare");
            for _ in 0..10 {
                nota!("{RIGA_RIPETUTA}");
            }
            let nuovo = fs::read_to_string(radice.join(CARTELLA).join(NOME)).unwrap_or_default();
            assert!(
                !nuovo.contains(RIGA_RIPETUTA),
                "il file nuovo ha ricominciato a scrivere la riga che si ripete: {nuovo}"
            );
        });
    }
}
