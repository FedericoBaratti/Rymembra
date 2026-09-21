//! Togliere un brano dalla libreria, e — se lo si chiede — anche dal disco.
//!
//! Due comandi e non uno, perché sono due domande diverse.
//!
//! [`brani_togli`] dice «questo non lo voglio in elenco»: la riga se ne va, il
//! file resta dov'è. È la risposta giusta per quel che è entrato in libreria per
//! sbaglio — una cartella sorvegliata di troppo, un disco esterno scansionato
//! una volta — e ha un limite che non si nasconde: se il file sta ancora sotto
//! una cartella sorvegliata, la prossima scansione lo rimette. La libreria è un
//! indice del disco, non il disco, e nessun indice può dire a un file di non
//! esistere. Per questo [`aether_app::library::Cancellazione`] porta indietro
//! `torneranno`: è la sola cosa onesta da mostrare a chi ha appena premuto.
//!
//! [`brani_elimina`] dice «questo non lo voglio più»: il file va nel Cestino, e
//! la riga con lui. Nel Cestino e non cancellato, perché una voce di menù che
//! distrugge senza ritorno è una voce di menù sbagliata; e per la stessa ragione
//! la riga si toglie **dopo**, e solo per i file che ci sono davvero arrivati —
//! una libreria che dichiara vuoto quel che sul disco c'è ancora è peggio di una
//! che dichiara troppo.
//!
//! # L'ordine, in [`brani_elimina`]
//!
//! Prima si sgombera la coda **fermando il motore**, poi si cestina, poi si
//! scrive nel database. Non è prudenza generica: su Windows un file aperto non
//! si può mandare nel Cestino, e il motore audio il suo file ce l'ha aperto per
//! tutta la durata del brano — compreso quello «in canna», che il gapless apre
//! in anticipo. Cestinare prima di aver lasciato andare quei due handle vuol
//! dire fallire proprio sul brano che si sta ascoltando, cioè su quello che si
//! sta guardando mentre si preme.
//!
//! # Perché un file suo
//!
//! Per la stessa ragione di [`crate::playlist`]: `comandi.rs` è già il più
//! lungo, e questi due comandi hanno un solo argomento in comune.

use aether_app::library::{self, Cancellazione};
use aether_app::settings::{self, CHIAVE_CARTELLE};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::paths::PathRules;
use tauri::Manager as _;

use crate::disparte::in_disparte;
use crate::errore::{Esito, errore};
use crate::nota;
use crate::riproduzione::{StatoLettore, sgombra_dalla_coda};
use crate::stato::{Stato, con_libreria};

/// La libreria, o il motivo per cui questo filo non ce l'ha più.
///
/// `try_state` e non `state`: `state` panica se lo stato non c'è, e questi corpi
/// girano sul pool bloccante, dove possono ritrovarsi vivi mentre la finestra si
/// chiude e gli stati gestiti vengono lasciati cadere. È la stessa guardia di
/// `comandi::scansiona_ora`.
fn stato_di<'a>(
    app: &'a tauri::AppHandle,
    cosa: &'static str,
) -> Result<tauri::State<'a, Stato>, AppError> {
    app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some(cosa.to_owned()),
        })
        .with_cause("la libreria non è più fra gli stati gestiti")
    })
}

/// Le cartelle sorvegliate, per sapere chi tornerà alla prossima scansione.
fn sorvegliate(stato: &Stato) -> Result<Vec<String>, AppError> {
    con_libreria(stato, |libreria| {
        settings::read_json(&libreria.connection, CHIAVE_CARTELLE).map(Option::unwrap_or_default)
    })
}

/// Toglie dalla coda dei brani che stanno per sparire, senza farne un guasto.
///
/// La coda è una comodità, non la verità: se il lettore non c'è — nessun
/// dispositivo audio, la finestra che si sta chiudendo — la cancellazione deve
/// riuscire lo stesso. Quel che non si è potuto fare finisce nel diario, che è
/// il posto dove si va a cercarlo, invece di diventare un errore rosso sopra un
/// gesto che è andato a buon fine.
fn sgombera(app: &tauri::AppHandle, brani: &[i64], ferma: bool) {
    let Some(lettore) = app.try_state::<StatoLettore>() else {
        return;
    };
    if let Err(err) = sgombra_dalla_coda(app, &lettore, brani, ferma) {
        nota!(
            "[cancellazione] la coda non si è potuta sgomberare: {} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        );
    }
}

/// Toglie i brani dalla libreria. **Non tocca i file.**
///
/// # Dove gira
///
/// In disparte: la transazione rifà gli aggregati, che su una libreria grande
/// sono una lettura intera di `tracks`, e il filo della finestra ha quattro
/// secondi di pazienza.
///
/// # Errori
///
/// `internal.aborted` se lo stato non c'è più, `db.*` se la scrittura fallisce.
#[tauri::command]
pub async fn brani_togli(app: tauri::AppHandle, brani: Vec<i64>) -> Esito<Cancellazione> {
    let mano = app.clone();
    let esito = in_disparte("rimozione di brani dalla libreria", move || {
        let stato = stato_di(&mano, "rimozione di brani")?;
        let radici = sorvegliate(&stato)?;
        let fatto = con_libreria(&stato, |libreria| {
            library::cancella_brani(
                &mut libreria.connection,
                &brani,
                &radici,
                PathRules::for_current_platform(),
            )
        })?;
        // Dopo il database e non prima: se la scrittura fallisce, la coda non
        // deve aver perso brani che in libreria ci sono ancora. Senza `ferma`,
        // perché i file restano leggibili — togliere dalla coda vuol dire «non
        // risuonarlo», non «zitto adesso».
        sgombera(&mano, &brani, false);
        Ok(fatto)
    })
    .await
    .map_err(errore)?
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Manda i file nel Cestino e toglie dalla libreria quelli che ci sono arrivati.
///
/// # Dove gira
///
/// In disparte, e il Cestino si chiama **fuori** dal lucchetto della libreria:
/// `IFileOperation` parla con la shell di Windows, che su una cartella di rete
/// può metterci quanto le pare.
///
/// # Errori
///
/// `fs.trashFailed` — o uno dei `fs.*` più precisi, quando Windows dice un
/// numero che si sa leggere — se il Cestino rifiuta un file. Lì ci si ferma,
/// dopo aver tolto dalla libreria quelli già andati: una riga che resta per un
/// file che non c'è più è un brano che non suona, una riga tolta per un file che
/// c'è ancora è un brano che nessuno ritrova. `internal.aborted` se lo stato non
/// c'è più, `db.*` se la scrittura fallisce.
#[tauri::command]
pub async fn brani_elimina(app: tauri::AppHandle, brani: Vec<i64>) -> Esito<usize> {
    let mano = app.clone();
    let esito = in_disparte("eliminazione di brani dal disco", move || {
        let stato = stato_di(&mano, "eliminazione di brani")?;
        let condannati = con_libreria(&stato, |libreria| {
            library::summaries_by_id(&libreria.connection, &brani)
        })?;
        if condannati.is_empty() {
            return Ok(0);
        }
        // Il motore lascia andare i file che ha in mano — il corrente e quello
        // in canna — prima che il Cestino provi a prenderli.
        let ids: Vec<i64> = condannati.iter().map(|brano| brano.id).collect();
        sgombera(&mano, &ids, true);

        let mut andati: Vec<i64> = Vec::with_capacity(condannati.len());
        let mut guasto = None;
        for brano in &condannati {
            // Un brano di catalogo non ha un file da mandare nel Cestino, e la
            // sua riga se ne va lo stesso: «elimina dal disco» su qualcosa che
            // sul disco non c'è vuol dire «toglilo dalla libreria», che è quel
            // che succede qui sotto insieme a tutti gli altri. Fallire sarebbe
            // rispondere a una domanda che nessuno ha fatto.
            let Some(percorso) = brano.path.as_deref() else {
                andati.push(brano.id);
                continue;
            };
            match trash::delete(percorso) {
                Ok(()) => andati.push(brano.id),
                Err(err) => {
                    guasto = Some(nel_cestino(percorso, &err));
                    break;
                }
            }
        }
        // Anche quando ci si è fermati a metà: le righe di quel che è nel
        // Cestino devono sparire comunque, o la libreria racconta file che non
        // ci sono più.
        let radici = sorvegliate(&stato)?;
        let fatto = con_libreria(&stato, |libreria| {
            library::cancella_brani(
                &mut libreria.connection,
                &andati,
                &radici,
                PathRules::for_current_platform(),
            )
        })?;
        match guasto {
            Some(err) => Err(err),
            None => Ok(fatto.tolti),
        }
    })
    .await
    .map_err(errore)?
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Il codice di sistema dentro un `trash::Error::Os`, sbucciato.
///
/// Su Windows quel numero non è un codice Win32: è l'`HRESULT` che COM ha dato
/// al crate, e `HRESULT_FROM_WIN32` il codice vero se lo tiene nei sedici bit
/// bassi con `FACILITY_WIN32` sopra. «File non trovato» arriva come
/// `0x80070002`, non come `2`.
///
/// Serve dirlo perché il confronto sbagliato non si vede: nessun ramo scatta
/// mai, cade tutto sul generico, e il generico una frase ce l'ha — sbagliata ma
/// plausibile. Qui è costato una prova dal vivo per accorgersene.
///
/// Sulle altre piattaforme il crate ci mette `errno`, che involucro non ne ha:
/// se i bit alti non sono quelli, il numero torna com'era.
fn codice_di_sistema(code: i32) -> i32 {
    /// `FACILITY_WIN32`, dove `HRESULT_FROM_WIN32` la scrive.
    const INVOLUCRO: u32 = 0x8007_0000;
    /// I sedici bit in cui sta il codice vero.
    const BASSI: u32 = 0x0000_FFFF;

    // `to_ne_bytes`/`from_ne_bytes` e non un `as`: è la stessa sequenza di bit
    // riletta senza segno, e non un troncamento che il workspace segnala.
    let grezzo = u32::from_ne_bytes(code.to_ne_bytes());
    if grezzo & !BASSI != INVOLUCRO {
        return code;
    }
    i32::try_from(grezzo & BASSI).unwrap_or(code)
}

/// Traduce il guasto del Cestino in un codice del catalogo.
///
/// Pura apposta: è l'unica parte di questo modulo che si può provare senza un
/// disco, una shell e un file da buttare.
///
/// I numeri sono quelli di Windows, e sono quattro perché quattro sono i casi in
/// cui chi legge può fare qualcosa: il file aperto altrove, il permesso negato,
/// il file già sparito, il disco pieno. Tutto il resto — compresa
/// l'«operazione interrotta» che `IFileOperation` restituisce senza dire da
/// cosa — cade su `fs.trashFailed`, che esiste apposta per non dover indovinare.
///
/// Dal vivo, però, il generico è la regola e non l'eccezione: un file tenuto
/// aperto in esclusiva da un altro programma non fa arrivare `0x80070020`, fa
/// arrivare `Unknown { "Some operations were aborted" }` — la shell prova,
/// rinuncia e non dice perché. È il motivo per cui la frase di `fs.trashFailed`
/// nomina tutte e due le cause invece di sceglierne una: quella distinzione da
/// qui non si può fare. I cinque numeri restano lo stesso, perché costano cinque
/// righe e la shell non promette di passare sempre di lì.
fn nel_cestino(percorso: &str, err: &trash::Error) -> AppError {
    /// `ERROR_FILE_NOT_FOUND`
    const NON_TROVATO: i32 = 2;
    /// `ERROR_PATH_NOT_FOUND`
    const PERCORSO_NON_TROVATO: i32 = 3;
    /// `ERROR_ACCESS_DENIED`
    const NEGATO: i32 = 5;
    /// `ERROR_SHARING_VIOLATION`
    const IN_USO: i32 = 32;
    /// `ERROR_DISK_FULL`
    const DISCO_PIENO: i32 = 112;

    let generico = || ErrorCode::FsTrashFailed {
        path: percorso.to_owned(),
        detail: None,
    };
    let codice = match err {
        trash::Error::Os { code, .. } => match codice_di_sistema(*code) {
            IN_USO => ErrorCode::FsInUse {
                path: percorso.to_owned(),
            },
            NEGATO => ErrorCode::FsPermissionDenied {
                path: percorso.to_owned(),
            },
            NON_TROVATO | PERCORSO_NON_TROVATO => ErrorCode::FsNotFound {
                path: percorso.to_owned(),
            },
            DISCO_PIENO => ErrorCode::FsDiskFull {
                path: Some(percorso.to_owned()),
            },
            _ => generico(),
        },
        // «Non ci si arriva» vuol dire tutte e due le cose insieme, lo dice la
        // documentazione del crate: il file non c'è, oppure non si ha il
        // permesso di guardarlo. Fra le due, «non c'è» è quella che chi legge
        // può verificare da sé in due secondi.
        trash::Error::CouldNotAccess { .. } | trash::Error::CanonicalizePath { .. } => {
            ErrorCode::FsNotFound {
                path: percorso.to_owned(),
            }
        }
        _ => generico(),
    };
    AppError::new(codice).with_cause(err.to_string())
}

#[cfg(test)]
mod prove {
    use super::nel_cestino;

    fn codice(err: &trash::Error) -> String {
        nel_cestino(r"C:\musica\a.mp3", err)
            .code()
            .kind()
            .code()
            .to_owned()
    }

    fn os(code: i32) -> trash::Error {
        trash::Error::Os {
            code,
            description: String::new(),
        }
    }

    /// Lo stesso numero, nell'involucro in cui Windows lo consegna davvero.
    fn hresult(win32: u32) -> trash::Error {
        os(i32::from_ne_bytes((0x8007_0000_u32 | win32).to_ne_bytes()))
    }

    #[test]
    fn i_numeri_di_windows_su_cui_si_puo_fare_qualcosa_hanno_un_codice_loro() {
        assert_eq!(codice(&hresult(32)), "fs.inUse");
        assert_eq!(codice(&hresult(5)), "fs.permissionDenied");
        assert_eq!(codice(&hresult(2)), "fs.notFound");
        assert_eq!(codice(&hresult(3)), "fs.notFound");
        assert_eq!(codice(&hresult(112)), "fs.diskFull");
    }

    #[test]
    fn il_numero_che_il_diario_ha_visto_davvero() {
        // Non un valore costruito qui: è quel che il crate ha restituito
        // cestinando la riga di un file spostato fuori da Aether, copiato dal
        // diario. La prima versione confrontava `2` con questo, non trovava mai
        // niente, e mandava a schermo il generico — che una frase ce l'ha, ed
        // è per questo che non si vedeva.
        assert_eq!(codice(&os(-2_147_024_894)), "fs.notFound");
    }

    #[test]
    fn un_errno_arriva_nudo_e_si_legge_lo_stesso() {
        // Le altre piattaforme non impacchettano niente: il crate ci mette
        // `errno`. Sono gli stessi cinque casi, e i numeri bassi non hanno
        // l'involucro da togliere.
        assert_eq!(codice(&os(2)), "fs.notFound");
        assert_eq!(codice(&os(112)), "fs.diskFull");
    }

    #[test]
    fn un_hresult_che_non_viene_da_win32_non_si_sbuccia() {
        // Facility `ITF` e non `WIN32`, con `0x20` nei bit bassi: sbucciarlo
        // senza guardare la facility direbbe «aperto da un altro programma» a
        // chi ha un guasto che con i file aperti non c'entra niente.
        assert_eq!(codice(&os(-2_147_221_472)), "fs.trashFailed");
    }

    #[test]
    fn un_guasto_che_non_si_sa_leggere_non_si_traveste_da_un_altro() {
        // Il caso vero, e il motivo per cui `fs.trashFailed` esiste:
        // `IFileOperation` dice «interrotta» e non dice da cosa. Indovinare «è
        // aperto da un altro programma» manderebbe metà delle persone a
        // chiudere un programma che non c'entra, e l'altra metà a non scoprire
        // mai che quel disco un Cestino non ce l'ha.
        assert_eq!(
            codice(&trash::Error::Unknown {
                description: "Some operations were aborted".to_owned(),
            }),
            "fs.trashFailed"
        );
        assert_eq!(codice(&os(1_234)), "fs.trashFailed");
    }

    #[test]
    fn non_ci_si_arriva_si_racconta_come_un_file_che_non_c_e() {
        assert_eq!(
            codice(&trash::Error::CouldNotAccess {
                target: r"C:\musica\a.mp3".to_owned(),
            }),
            "fs.notFound"
        );
    }

    #[test]
    fn la_causa_tecnica_non_si_perde() {
        // Il codice è per chi legge a schermo, la causa per chi legge il
        // diario: senza, un guasto generico sarebbe indistinguibile da tutti
        // gli altri guasti generici.
        let err = nel_cestino(r"C:\musica\a.mp3", &os(1_234));
        assert!(err.cause().is_some_and(|c| c.contains("1234")));
    }
}
