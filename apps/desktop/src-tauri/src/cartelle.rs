//! I comandi del pannello «Cartelle», e l'albero che tengono in vita.
//!
//! L'albero vero lo costruisce [`aether_app::cartelle`], che spiega perché
//! viene dal database e non dal disco. Qui c'è quel che il nucleo non può
//! sapere: **quando** costruirlo, quanto tenerlo, e da quale connessione
//! leggerlo.
//!
//! # Una seconda connessione, di sola lettura
//!
//! Sul modello di `enrich::DepositoSqlite`, e per una ragione parente: questo
//! modulo legge **mentre** la libreria è occupata. Un pannello di navigazione
//! che si apre durante una scansione, e che dovesse mettersi in coda dietro il
//! lucchetto globale, sarebbe una finestra ferma — cioè esattamente il difetto
//! che questa release sta chiudendo altrove. Il database è in WAL: un lettore
//! in più non disturba nessuno e non viene disturbato.
//!
//! `SQLITE_OPEN_READ_ONLY` non è cautela decorativa. È il modo di dire nel tipo
//! che di qui non passa **nessuna** migrazione e nessuna scrittura: due
//! connessioni che migrassero lo stesso file insieme sono il guasto che la
//! documentazione di `DepositoSqlite` dichiara di evitare per convenzione, e una
//! convenzione la si dimentica.
//!
//! La connessione **non resta aperta**. Si apre per costruire l'albero e si
//! chiude subito: a vista chiusa questo modulo deve costare zero, e una
//! connessione SQLite viva costa la sua cache di pagine anche quando nessuno la
//! interroga. L'apertura è rara — la prima volta che si apre la vista, e dopo
//! ogni scansione — e accanto ai trentacinque millisecondi della costruzione
//! non si nota.
//!
//! # Quando l'albero nasce e quando muore
//!
//! Nasce **alla prima domanda**, non all'avvio: chi non apre mai il pannello non
//! paga niente. Muore dopo [`VITA`] senza domande, e a portarselo via è un filo
//! che esiste solo finché c'è qualcosa da sfrattare — appena l'albero se n'è
//! andato, il filo finisce. A riposo quindi non c'è né albero, né connessione,
//! né filo.
//!
//! «Vista chiusa» qui si legge «nessuna domanda»: la finestra non annuncia le
//! sue chiusure, e un comando in più solo per dirlo sarebbe una promessa che il
//! giorno in cui la finestra cade male non viene mantenuta. Il costo di questa
//! lettura è che chi resta cinque minuti a guardare l'albero senza toccarlo se
//! lo vede ricostruire al clic dopo, e sono i trentacinque millisecondi della
//! costruzione.
//!
//! # Il lucchetto globale non si prende mai
//!
//! Nessuna funzione di questo modulo nomina `con_libreria`. Tutti e tre i
//! comandi passano da [`crate::disparte::in_disparte`], perché la sonda delle
//! radici tocca il disco e perché una costruzione da trentacinque millisecondi
//! sul filo principale sono due fotogrammi persi.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use aether_app::cartelle::{Albero, NodoCartella, brani_sotto_dal_database};
use aether_app::files::MusicFiles as _;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::paths::PathRules;
use rusqlite::{Connection, OpenFlags};

use crate::disparte::in_disparte;
use crate::errore::{Esito, errore};
use crate::stato::NOME_DATABASE;

/// Quanto sopravvive l'albero all'ultima domanda.
///
/// Cinque minuti: molto più di una pausa fra due clic, molto meno del tempo in
/// cui ci si dimentica di aver aperto un pannello. Ricostruirlo costa una
/// trentina di millisecondi su una libreria da diciottomila brani, quindi
/// sbagliare per difetto costa un'attesa che non si vede; sbagliare per eccesso
/// costa megabyte tenuti per niente.
const VITA: Duration = Duration::from_secs(5 * 60);

/// Ogni quanto il filo di sfratto si sveglia a guardare l'orologio.
const RITMO_SFRATTO: Duration = Duration::from_secs(30);

/// L'albero delle cartelle, tenuto fra una domanda e l'altra.
///
/// Si registra con `app.manage` dentro un [`Arc`], perché il filo che lo sfratta
/// deve poterlo tenere per conto suo senza passare da Tauri.
pub struct IndiceCartelle {
    /// Il file del database. Si tiene il percorso e non la connessione: vedi il
    /// `//!`.
    percorso: PathBuf,
    /// L'albero, quando c'è.
    tenuto: Mutex<Option<Tenuto>>,
    /// Cambia a ogni [`IndiceCartelle::invalida`], e un albero di una
    /// generazione vecchia si butta invece di rispondere con dati di ieri.
    ///
    /// Un contatore e non un `bool`: chi sta costruendo legge la generazione
    /// prima di cominciare, e se nel frattempo una scansione è finita il suo
    /// risultato viene scartato invece di sovrascrivere quello nuovo.
    generazione: AtomicU64,
    /// C'è già un filo che aspetta di sfrattare l'albero.
    sfrattatore: AtomicBool,
}

impl std::fmt::Debug for IndiceCartelle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndiceCartelle")
            .field("percorso", &self.percorso)
            .finish_non_exhaustive()
    }
}

/// L'albero in vita, con quel che serve a sapere se vale ancora.
struct Tenuto {
    albero: Arc<Albero>,
    /// Le radici con cui è stato costruito. Se l'utente ne aggiunge una, quel
    /// che c'è non risponde più alla domanda che gli si sta facendo.
    radici: Vec<String>,
    generazione: u64,
    ultima_domanda: Instant,
}

impl IndiceCartelle {
    /// Un indice ancora vuoto, sul database della cartella dati.
    ///
    /// Non apre niente e non legge niente: costruirlo all'avvio deve costare un
    /// `PathBuf` e tre parole di stato.
    #[must_use]
    pub fn nuovo(data_dir: &Path) -> Self {
        Self {
            percorso: data_dir.join(NOME_DATABASE),
            tenuto: Mutex::new(None),
            generazione: AtomicU64::new(0),
            sfrattatore: AtomicBool::new(false),
        }
    }

    /// La libreria è cambiata: quel che c'è non vale più.
    ///
    /// Non ricostruisce niente — sarebbe lavoro fatto per un pannello che
    /// magari non è nemmeno aperto — e non prende nessun lucchetto: alza un
    /// contatore, e la prossima domanda trova l'albero vecchio e ne fa uno
    /// nuovo. Si può chiamare da qualunque filo, anche mentre una costruzione è
    /// in corso.
    pub fn invalida(&self) {
        self.generazione.fetch_add(1, Ordering::Relaxed);
    }

    /// L'albero per queste radici, costruendolo se serve.
    ///
    /// # Errori
    ///
    /// `db.openFailed` se il file non si apre in lettura, e quel che risponde
    /// SQLite se la lettura fallisce.
    pub fn albero(self: &Arc<Self>, radici: &[String]) -> Result<Arc<Albero>, AppError> {
        let generazione = self.generazione.load(Ordering::Relaxed);
        // Il lucchetto si tiene per tutta la costruzione, ed è voluto: è un
        // lucchetto che nessun altro pezzo del programma vuole — non è quello
        // della libreria — e l'alternativa è che due espansioni simultanee
        // leggano `tracks` due volte per costruire lo stesso albero.
        let mut tenuto = self.tenuto.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(vivo) = tenuto.as_mut()
            && vivo.generazione == generazione
            && vivo.radici == radici
        {
            vivo.ultima_domanda = Instant::now();
            return Ok(Arc::clone(&vivo.albero));
        }
        let connessione = self.apri()?;
        let albero = Arc::new(Albero::costruisci(
            &connessione,
            radici,
            PathRules::for_current_platform(),
        )?);
        *tenuto = Some(Tenuto {
            albero: Arc::clone(&albero),
            radici: radici.to_vec(),
            generazione,
            ultima_domanda: Instant::now(),
        });
        drop(tenuto);
        self.avvia_sfrattatore();
        Ok(albero)
    }

    /// Apre la connessione di sola lettura. Vedi il `//!`.
    ///
    /// # Errori
    ///
    /// `db.openFailed` col percorso, che è l'unica cosa utile da dire quando un
    /// file non si apre.
    fn apri(&self) -> Result<Connection, AppError> {
        let connessione = Connection::open_with_flags(
            &self.percorso,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|err| {
            AppError::new(ErrorCode::DbOpenFailed {
                path: Some(self.percorso.display().to_string()),
            })
            .with_cause(err.to_string())
        })?;
        // Solo l'attesa. Niente `journal_mode`, niente migrazioni: la
        // connessione principale ha già fatto entrambe, e questa non potrebbe
        // nemmeno se volesse.
        let _ = connessione.busy_timeout(Duration::from_secs(5));
        Ok(connessione)
    }

    /// Butta l'albero se nessuno lo chiede da [`VITA`].
    ///
    /// Restituisce `true` finché resta qualcosa da sfrattare: è quel che dice al
    /// filo di sfratto se ha ancora un motivo di esistere.
    fn sfratta_se_scaduto(&self) -> bool {
        let mut tenuto = self.tenuto.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(vivo) = tenuto.as_ref() else {
            return false;
        };
        if vivo.ultima_domanda.elapsed() < VITA {
            return true;
        }
        *tenuto = None;
        false
    }

    /// Mette in piedi il filo che sfratterà l'albero, se non c'è già.
    ///
    /// Un filo che nasce con l'albero e muore con lui, invece di uno che dorme
    /// per tutta la vita del processo: a pannello mai aperto — cioè per chi non
    /// usa questa funzione — non deve esistere niente.
    fn avvia_sfrattatore(self: &Arc<Self>) {
        if self.sfrattatore.swap(true, Ordering::Relaxed) {
            return;
        }
        let indice = Arc::clone(self);
        let avviato = std::thread::Builder::new()
            .name("cartelle-sfratto".to_owned())
            // Questo filo dorme e guarda un orologio: due megabyte di stack
            // riservato sarebbero un'abitudine, non una necessità.
            .stack_size(64 * 1024)
            .spawn(move || {
                loop {
                    std::thread::sleep(RITMO_SFRATTO);
                    // In uscita non c'è niente da liberare che il processo non
                    // stia già liberando da sé.
                    if crate::spegnimento::in_uscita() || !indice.sfratta_se_scaduto() {
                        break;
                    }
                }
                indice.sfrattatore.store(false, Ordering::Relaxed);
            });
        if avviato.is_err() {
            // Senza filo l'albero resta finché la libreria non cambia. È un
            // degrado, non un guasto: si dice, e si va avanti.
            self.sfrattatore.store(false, Ordering::Relaxed);
            crate::nota!("[cartelle] il filo di sfratto non è partito");
        }
    }
}

/// Lo stato gestito, preso dal registro di Tauri.
///
/// `try_state` e non `state`: questi corpi girano sul pool bloccante, che può
/// ritrovarsi vivo mentre la finestra si chiude e gli stati gestiti vengono
/// lasciati cadere. `state` lì dentro sarebbe un panico.
fn indice(app: &tauri::AppHandle) -> Result<Arc<IndiceCartelle>, AppError> {
    use tauri::Manager as _;
    app.try_state::<Arc<IndiceCartelle>>()
        .map(|stato| Arc::clone(&stato))
        .ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some("albero delle cartelle".to_owned()),
            })
            .with_cause("l'indice non è più fra gli stati gestiti")
        })
}

/// Le cartelle dentro `percorso`; senza `percorso`, le radici.
///
/// # Dove gira
///
/// In disparte, e **mai** sotto il lucchetto della libreria: legge da una
/// connessione sua. La prima chiamata costruisce l'albero e costa la lettura di
/// `tracks`; le successive costano un accesso a una mappa.
///
/// # Errori
///
/// `db.openFailed` se il database non si apre in lettura, `db.*` se la lettura
/// fallisce, `internal.aborted` se lo stato non c'è più.
#[tauri::command]
pub async fn cartelle_figlie(
    app: tauri::AppHandle,
    radici: Vec<String>,
    percorso: Option<String>,
) -> Esito<Vec<NodoCartella>> {
    in_disparte("cartelle figlie", move || {
        let albero = indice(&app)?.albero(&radici)?;
        Ok(albero.figlie(percorso.as_deref()))
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Gli identificativi dei brani sotto `percorso`, nell'ordine di riproduzione.
///
/// L'ordine lo decide [`aether_app::cartelle`], e dentro una cartella è quello
/// dell'album — che è la cosa giusta perché una cartella, nel caso normale, *è*
/// un album.
///
/// # Dove gira
///
/// In disparte, e mai sotto il lucchetto della libreria.
///
/// # Errori
///
/// Gli stessi di [`cartelle_figlie`].
#[tauri::command]
pub async fn cartelle_brani(
    app: tauri::AppHandle,
    radici: Vec<String>,
    percorso: String,
) -> Esito<Vec<i64>> {
    in_disparte("cartelle brani", move || {
        let indice = indice(&app)?;
        let albero = indice.albero(&radici)?;
        if let Some(brani) = albero.brani_sotto(&percorso) {
            return Ok(brani);
        }
        // `None` vuol dire «non lo so», non «non ce n'è»: oltre la soglia il
        // trie i brani non se li ricorda, ed è la rinuncia che tiene la memoria
        // sotto controllo su una libreria enorme. Qui si rilegge, e si può — è
        // un gesto solo, non un'espansione.
        let connessione = indice.apri()?;
        brani_sotto_dal_database(&connessione, &percorso, PathRules::for_current_platform())
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Quali radici rispondono, adesso.
///
/// Un filo per radice, così una share morta costa gli otto secondi della sonda
/// una volta sola invece di otto per ognuna. La scadenza sta già dentro
/// `files::radice_raggiungibile`, e «scaduta» vale «non risponde»: una radice
/// che non si fa sentire in otto secondi non è una radice su cui si possa
/// concludere niente.
///
/// # Perché è un comando a parte
///
/// Perché l'albero non lo aspetta. Le radici si disegnano subito dai percorsi
/// che il database conosce già, e questa risposta arriva dopo a spegnere quelle
/// che non ci sono: è il motivo per cui il pannello si apre anche col Wi-Fi
/// staccato.
///
/// # Errori
///
/// `internal.aborted` se il lavoro cade. Una radice che non risponde **non** è
/// un errore: è un `false` nel suo posto dell'elenco, che è lungo e ordinato
/// come quello ricevuto.
#[tauri::command]
pub async fn cartelle_radici_vive(radici: Vec<String>) -> Esito<Vec<bool>> {
    in_disparte("radici vive", move || {
        std::thread::scope(|ambito| {
            let fili: Vec<_> = radici
                .iter()
                .map(|radice| {
                    ambito.spawn(move || aether_app::files::LocalFiles.radice_raggiungibile(radice))
                })
                .collect();
            fili.into_iter()
                // Un filo caduto vale «non risponde»: è la stessa risposta
                // prudente della scadenza, e per lo stesso motivo.
                .map(|filo| filo.join().unwrap_or(false))
                .collect()
        })
    })
    .await
    .map_err(errore)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::mpsc;

    /// Una cartella dati vera, con la libreria aperta e una riga dentro.
    fn banco(dir: &Path) -> crate::stato::Stato {
        let stato = crate::stato::Stato::apri(dir.to_path_buf());
        {
            let mut guardia = stato.libreria.lock().expect("lucchetto");
            let libreria = guardia.as_mut().expect("libreria aperta");
            libreria
                .connection
                .execute(
                    "INSERT INTO tracks
                         (id, path, track_key, title, artist, album, duration_ms,
                          file_size, date_added, date_modified)
                     VALUES (1, ?1, 'k', 'T', 'A', 'Al', 1000, 99999, 0, 0)",
                    [r"C:\M\Rock\a.mp3"],
                )
                .expect("riga di brano");
        }
        stato
    }

    #[test]
    fn l_albero_si_costruisce_senza_il_lucchetto() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let stato = banco(dir.path());
        let indice = Arc::new(IndiceCartelle::nuovo(dir.path()));

        let (preso, aspetta_preso) = mpsc::channel();
        let (finito, aspetta_finito) = mpsc::channel();
        let banco = &stato;
        std::thread::scope(|ambito| {
            // Un filo tiene il lucchetto della libreria, come farebbe una
            // scansione in corso.
            ambito.spawn(move || {
                let _guardia = banco.libreria.lock().expect("lucchetto");
                preso.send(()).expect("annuncio");
                // Lo si lascia solo quando la prova ha finito, o dopo un tetto
                // che serve a non appendere la suite se qualcosa va storto.
                let _ = aspetta_finito.recv_timeout(Duration::from_secs(30));
            });
            aspetta_preso
                .recv_timeout(Duration::from_secs(10))
                .expect("il lucchetto è stato preso");

            // E il comando risponde lo stesso: legge da una connessione sua.
            let albero = indice.albero(&[r"C:\M".to_owned()]).expect("albero");
            assert_eq!(albero.quanti_sotto(r"C:\M"), 1);
            assert_eq!(albero.brani_sotto(r"C:\M"), Some(vec![1]));

            finito.send(()).expect("liberazione");
        });
    }

    #[test]
    fn invalidare_fa_ricostruire() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let stato = banco(dir.path());
        let indice = Arc::new(IndiceCartelle::nuovo(dir.path()));
        let radici = vec![r"C:\M".to_owned()];

        let primo = indice.albero(&radici).expect("albero");
        assert_eq!(primo.quanti_sotto(r"C:\M"), 1);
        // Senza invalidare, la stessa domanda dà lo stesso oggetto.
        let ancora = indice.albero(&radici).expect("albero");
        assert!(
            Arc::ptr_eq(&primo, &ancora),
            "non si ricostruisce per niente"
        );

        {
            let mut guardia = stato.libreria.lock().expect("lucchetto");
            let libreria = guardia.as_mut().expect("libreria aperta");
            libreria
                .connection
                .execute(
                    "INSERT INTO tracks
                         (id, path, track_key, title, artist, album, duration_ms,
                          file_size, date_added, date_modified)
                     VALUES (2, ?1, 'k2', 'T', 'A', 'Al', 1000, 99999, 0, 0)",
                    [r"C:\M\Jazz\b.mp3"],
                )
                .expect("riga di brano");
        }
        indice.invalida();
        let dopo = indice.albero(&radici).expect("albero");
        assert_eq!(dopo.quanti_sotto(r"C:\M"), 2, "la scansione si vede");
    }

    #[test]
    fn radici_diverse_non_riusano_l_albero() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let _stato = banco(dir.path());
        let indice = Arc::new(IndiceCartelle::nuovo(dir.path()));

        let stretto = indice.albero(&[r"C:\M\Rock".to_owned()]).expect("albero");
        assert_eq!(stretto.figlie(None).len(), 1);
        let largo = indice.albero(&[r"C:\M".to_owned()]).expect("albero");
        assert_eq!(
            largo.figlie(Some(r"C:\M")).len(),
            1,
            "l'albero è quello delle radici nuove"
        );
    }

    #[test]
    fn le_radici_vive_rispondono_una_per_posto() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        // Nessuna rete: una cartella che esiste e una che non esiste, sul disco
        // locale. La sonda è la stessa che userebbe una share.
        let esiste = dir.path().display().to_string();
        let vive = std::thread::scope(|ambito| {
            let fili: Vec<_> = [esiste.as_str(), "Z:/non/esiste/di/sicuro"]
                .into_iter()
                .map(|radice| {
                    ambito.spawn(move || aether_app::files::LocalFiles.radice_raggiungibile(radice))
                })
                .collect();
            fili.into_iter()
                .map(|filo| filo.join().unwrap_or(false))
                .collect::<Vec<_>>()
        });
        assert_eq!(vive, vec![true, false]);
    }
}
