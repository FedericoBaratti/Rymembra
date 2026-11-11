//! Quel che esiste solo sul telefono: i permessi, le cartelle, i documenti, i
//! segreti nel Keystore.
//!
//! I comandi esistono su tutte le piattaforme, perché il registro dei comandi è
//! uno solo (vedi `lib.rs`); fuori da Android rispondono `internal.aborted` e
//! la finestra non li chiama — `piattaforma.ts` sa dove gira.
//!
//! # Perché i documenti passano da qui
//!
//! Su Android i selettori di sistema non danno percorsi: danno URI
//! `content://`, che solo Android sa leggere. Tutto quel che Rymembra apre o
//! salva — una skin, un profilo, una playlist — lo apre e lo salva il nucleo
//! con un percorso. Così sul telefono la finestra fa scegliere il documento da
//! qui, e il ponte lo **copia** nella cache dell'app (aprire) o copia nell'URI
//! scelto il file che il nucleo ha scritto nella cache (salvare). Il nucleo non
//! se ne accorge. Le cartelle invece si traducono nel percorso vero: la
//! scansione le cammina, e una copia non avrebbe senso.

use rymembra_domain::errors::{AppError, ErrorCode};
use serde::Serialize;

use crate::errore::{Esito, errore};

/// Quel che l'app può leggere sul telefono, per la finestra.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermessiIpc {
    /// «Accesso a tutti i file»: i brani, i testi e i `.cue` accanto, le
    /// eliminazioni.
    pub tutti_i_file: bool,
    /// I file audio.
    pub audio: bool,
    /// Le immagini, per le copertine accanto ai brani.
    pub immagini: bool,
    /// Le notifiche: senza, la notifica del lettore non si vede.
    pub notifiche: bool,
}

/// Il guasto del ponte, come errore del catalogo.
#[cfg(target_os = "android")]
fn guasto(cosa: &str, err: &rymembra_android::Errore) -> Box<crate::errore::ErroreIpc> {
    errore(
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some(cosa.to_owned()),
        })
        .with_cause(err.to_string()),
    )
}

/// Fuori da Android: il comando non ha niente da fare.
#[cfg(not(target_os = "android"))]
fn solo_su_android<T>() -> Esito<T> {
    Err(errore(AppError::new(ErrorCode::InternalAborted {
        what: Some("solo su Android".to_owned()),
    })))
}

#[cfg(target_os = "android")]
impl From<rymembra_android::sistema::Permessi> for PermessiIpc {
    fn from(p: rymembra_android::sistema::Permessi) -> Self {
        Self {
            tutti_i_file: p.tutti_i_file,
            audio: p.audio,
            immagini: p.immagini,
            notifiche: p.notifiche,
        }
    }
}

/// Un lavoro che aspetta una persona, lontano dal filo della finestra.
///
/// Il ponte apre il selettore sul filo principale e lo aspetta: chiamato da
/// lì, aspetterebbe se stesso. Vedi `rymembra_android::sistema`.
#[cfg(target_os = "android")]
async fn in_attesa<T: Send + 'static>(
    nome: &'static str,
    lavoro: impl FnOnce() -> Result<T, rymembra_android::Errore> + Send + 'static,
) -> Esito<T> {
    crate::disparte::in_disparte(nome, lavoro)
        .await
        .map_err(errore)?
        .map_err(|err| guasto(nome, &err))
}

/// Lo stato dei permessi adesso.
///
/// # Errori
///
/// `internal.unexpected` se il ponte non risponde; `internal.aborted` fuori da
/// Android.
#[tauri::command]
pub fn telefono_permessi() -> Esito<PermessiIpc> {
    #[cfg(target_os = "android")]
    return rymembra_android::sistema::permessi()
        .map(PermessiIpc::from)
        .map_err(|err| guasto("permessi", &err));
    #[cfg(not(target_os = "android"))]
    solo_su_android()
}

/// Chiede di leggere la musica con il dialogo di sistema, e torna con lo stato
/// dopo la risposta. Le notifiche no: si chiedono quando servono (vedi
/// `Permessi.notificheAlBisogno`).
///
/// # Errori
///
/// Come [`telefono_permessi`].
#[tauri::command]
pub async fn telefono_chiedi_permessi() -> Esito<PermessiIpc> {
    #[cfg(target_os = "android")]
    return in_attesa("permessi", rymembra_android::sistema::chiedi_permessi)
        .await
        .map(PermessiIpc::from);
    #[cfg(not(target_os = "android"))]
    solo_su_android()
}

/// Apre la pagina di Android in cui si concede «Accesso a tutti i file».
///
/// # Errori
///
/// Come [`telefono_permessi`].
#[tauri::command]
pub fn telefono_chiedi_tutti_i_file() -> Esito<()> {
    #[cfg(target_os = "android")]
    return rymembra_android::sistema::chiedi_tutti_i_file()
        .map_err(|err| guasto("accesso a tutti i file", &err));
    #[cfg(not(target_os = "android"))]
    solo_su_android()
}

/// La cartella «Music» del telefono, da proporre a chi apre Rymembra la prima
/// volta.
///
/// # Errori
///
/// Come [`telefono_permessi`].
#[tauri::command]
pub fn telefono_cartella_musica() -> Esito<String> {
    #[cfg(target_os = "android")]
    return rymembra_android::sistema::cartella_musica()
        .map(|cartella| cartella.to_string_lossy().into_owned())
        .map_err(|err| guasto("cartella della musica", &err));
    #[cfg(not(target_os = "android"))]
    solo_su_android()
}

/// Fa scegliere una cartella, e la restituisce come percorso. `None` se chi
/// guarda ha annullato.
///
/// # Errori
///
/// Come [`telefono_permessi`]; `fs.pathInvalid` se la cartella scelta non sta
/// nella memoria del telefono né su una scheda SD ([`cartella_fuori`]).
#[tauri::command]
pub async fn telefono_scegli_cartella() -> Esito<Option<String>> {
    #[cfg(target_os = "android")]
    {
        use rymembra_android::sistema::Cartella;
        match in_attesa(
            "scelta della cartella",
            rymembra_android::sistema::scegli_cartella,
        )
        .await?
        {
            None => Ok(None),
            Some(Cartella::Percorso(cartella)) => Ok(Some(cartella.to_string_lossy().into_owned())),
            Some(Cartella::Fuori(fornitore)) => Err(errore(cartella_fuori(&fornitore))),
        }
    }
    #[cfg(not(target_os = "android"))]
    solo_su_android()
}

/// Una cartella scelta nel selettore di Android che non ha un percorso: sta in
/// «Download», o in un'app cloud che offre cartelle, e la scansione non la può
/// camminare. Non un guasto — il selettore la offre, e sceglierla è un gesto
/// normale — ma un percorso che non si può usare: la frase del telefono
/// (`errors.fs.pathInvalid.telefono`) dice di sceglierne una nella memoria o
/// su una scheda SD. Prima era `internal.unexpected`, «Riavvia Rymembra».
#[cfg(any(target_os = "android", test))]
fn cartella_fuori(fornitore: &str) -> AppError {
    AppError::new(ErrorCode::FsPathInvalid {
        path: fornitore.to_owned(),
    })
    .with_cause("la cartella scelta non sta nella memoria del telefono né su una scheda SD")
}

/// Fa scegliere un documento, e ne restituisce una copia nella cache. `None`
/// se chi guarda ha annullato.
///
/// # Errori
///
/// Come [`telefono_permessi`], e se il documento non si copia.
#[tauri::command]
pub async fn telefono_apri_documento(tipi: Vec<String>) -> Esito<Option<String>> {
    #[cfg(target_os = "android")]
    return in_attesa("apertura di un documento", move || {
        rymembra_android::sistema::apri_documento(&tipi)
    })
    .await
    .map(|scelta| scelta.map(|percorso| percorso.to_string_lossy().into_owned()));
    #[cfg(not(target_os = "android"))]
    {
        let _ = tipi;
        solo_su_android()
    }
}

/// Fa scegliere dove salvare, e ci copia `sorgente` — un file che il nucleo ha
/// appena scritto dove dice [`telefono_file_temporaneo`]. Torna `false` se chi
/// guarda ha annullato. La copia nella cache si toglie comunque.
///
/// `sorgente` arriva dalla finestra, e dopo la copia si **cancella**: si
/// accetta quindi solo un file di quella cartella ([`da_salvare`]). Fuori di lì
/// niente dialogo, niente copia e niente cancellazione — prima, uno script
/// nella finestra poteva far sparire il database con un «Annulla».
///
/// # Errori
///
/// `fs.pathInvalid` se `sorgente` non è un documento di quella cartella; come
/// [`telefono_permessi`], e se la copia non riesce.
#[tauri::command]
pub async fn telefono_salva_documento(
    app: tauri::AppHandle,
    nome: String,
    tipo: String,
    sorgente: String,
) -> Esito<bool> {
    #[cfg(target_os = "android")]
    {
        let sorgente = da_salvare(&cartella_da_salvare(&app)?, std::path::Path::new(&sorgente))
            .map_err(errore)?;
        in_attesa("salvataggio di un documento", move || {
            let esito = rymembra_android::sistema::salva_documento(&nome, &tipo, &sorgente);
            let _ = std::fs::remove_file(&sorgente);
            esito
        })
        .await
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, nome, tipo, sorgente);
        solo_su_android()
    }
}

/// `sorgente` è un documento da salvare: un file figlio diretto di
/// `cartella`, con `..` e collegamenti risolti. Restituisce la forma canonica,
/// la sola che poi si copia e si cancella.
///
/// # Errori
///
/// `fs.pathInvalid` per tutto il resto: un percorso fuori, la cartella stessa,
/// una sottocartella, un file che non c'è.
#[cfg(any(target_os = "android", test))]
fn da_salvare(
    cartella: &std::path::Path,
    sorgente: &std::path::Path,
) -> Result<std::path::PathBuf, AppError> {
    let rifiuto = |causa: String| {
        AppError::new(ErrorCode::FsPathInvalid {
            path: sorgente.display().to_string(),
        })
        .with_cause(causa)
    };
    let cartella = cartella
        .canonicalize()
        .map_err(|err| rifiuto(err.to_string()))?;
    let file = sorgente
        .canonicalize()
        .map_err(|err| rifiuto(err.to_string()))?;
    if file.parent() != Some(cartella.as_path()) || !file.is_file() {
        return Err(rifiuto(
            "non è un documento della cartella da salvare".to_owned(),
        ));
    }
    Ok(file)
}

/// La cartella dei documenti da salvare: `da-salvare`, nella cache dell'app.
#[cfg(target_os = "android")]
fn cartella_da_salvare(app: &tauri::AppHandle) -> Esito<std::path::PathBuf> {
    use tauri::Manager as _;
    app.path()
        .app_cache_dir()
        .map(|cache| cache.join("da-salvare"))
        .map_err(|err| {
            errore(
                AppError::new(ErrorCode::InternalUnexpected {
                    detail: Some("cache dell'app".to_owned()),
                })
                .with_cause(err.to_string()),
            )
        })
}

/// Dove il nucleo scrive quel che poi [`telefono_salva_documento`] copia
/// fuori: un nome nuovo dentro la cache dell'app.
///
/// # Errori
///
/// `internal.unexpected` se la cache non si trova o non si crea;
/// `internal.aborted` fuori da Android.
#[tauri::command]
pub fn telefono_file_temporaneo(app: tauri::AppHandle, nome: String) -> Esito<String> {
    #[cfg(target_os = "android")]
    {
        let cartella = cartella_da_salvare(&app)?;
        std::fs::create_dir_all(&cartella).map_err(|err| {
            errore(
                AppError::new(ErrorCode::InternalUnexpected {
                    detail: Some("cache dell'app".to_owned()),
                })
                .with_cause(err.to_string()),
            )
        })?;
        // Solo il nome, senza cartelle: arriva dalla finestra, e una barra
        // dentro porterebbe il file fuori dalla cache.
        let nome = std::path::Path::new(&nome).file_name().map_or_else(
            || "documento".into(),
            |nome| nome.to_string_lossy().into_owned(),
        );
        Ok(cartella.join(nome).to_string_lossy().into_owned())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, nome);
        solo_su_android()
    }
}

/// Quanto la pagina deve stare lontana dai bordi: vedi `riquadri.ts`.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct RiquadriIpc {
    /// La barra di stato.
    pub alto: f64,
    /// A destra.
    pub destra: f64,
    /// La barra dei gesti, o la tastiera aperta.
    pub basso: f64,
    /// A sinistra.
    pub sinistra: f64,
    /// La tastiera aperta, zero se è chiusa (già contata in `basso`).
    pub tastiera: f64,
}

/// I riquadri dello schermo, in pixel CSS.
///
/// # Errori
///
/// Come [`telefono_permessi`].
#[tauri::command]
pub fn telefono_riquadri() -> Esito<RiquadriIpc> {
    #[cfg(target_os = "android")]
    return rymembra_android::sistema::riquadri()
        .map(|r| RiquadriIpc {
            alto: r.alto,
            destra: r.destra,
            basso: r.basso,
            sinistra: r.sinistra,
            tastiera: r.tastiera,
        })
        .map_err(|err| guasto("riquadri", &err));
    #[cfg(not(target_os = "android"))]
    solo_su_android()
}

/// L'esito dello scanner del QR, per la finestra.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScansioneIpc {
    /// Il link letto; `None` se si è annullato.
    pub link: Option<String>,
    /// La fotocamera non è stata concessa: la finestra lo dice, e propone il
    /// link da incollare.
    pub negato: bool,
}

/// Apre lo scanner del QR di «Collega un dispositivo» e ne aspetta l'esito.
/// Il link letto non si usa da qui: la finestra lo passa a `lontano_collega`,
/// che lo controlla.
///
/// # Errori
///
/// Come [`telefono_permessi`].
#[tauri::command]
pub async fn telefono_scansiona_qr() -> Esito<ScansioneIpc> {
    #[cfg(target_os = "android")]
    return in_attesa("scanner del QR", rymembra_android::sistema::scansiona_qr)
        .await
        .map(|s| ScansioneIpc {
            link: s.link,
            negato: s.negato,
        });
    #[cfg(not(target_os = "android"))]
    solo_su_android()
}

/// Una vibrazione breve: vedi `rymembra_android::sistema::vibra`.
///
/// Un tipo che il telefono non conosce vibra come uno scatto: è la più lieve,
/// e un errore qui non avrebbe nessuno a cui dirlo.
///
/// # Errori
///
/// Come [`telefono_permessi`].
#[tauri::command]
pub fn telefono_vibra(tipo: String) -> Esito<()> {
    #[cfg(target_os = "android")]
    return rymembra_android::sistema::vibra(&tipo).map_err(|err| guasto("vibrazione", &err));
    #[cfg(not(target_os = "android"))]
    {
        let _ = tipo;
        solo_su_android()
    }
}

/// Il colore delle icone delle barre di sistema, sul fondo dell'app: vedi
/// `rymembra_android::sistema::barre`.
///
/// # Errori
///
/// Come [`telefono_permessi`].
#[tauri::command]
pub fn telefono_barre(chiaro: bool) -> Esito<()> {
    #[cfg(target_os = "android")]
    return rymembra_android::sistema::barre(chiaro)
        .map_err(|err| guasto("barre di sistema", &err));
    #[cfg(not(target_os = "android"))]
    {
        let _ = chiaro;
        solo_su_android()
    }
}

/// Manda l'app in secondo piano, come il tasto Home: la musica continua.
///
/// # Errori
///
/// Come [`telefono_permessi`].
#[tauri::command]
pub fn telefono_secondo_piano() -> Esito<()> {
    #[cfg(target_os = "android")]
    return rymembra_android::sistema::in_secondo_piano()
        .map_err(|err| guasto("secondo piano", &err));
    #[cfg(not(target_os = "android"))]
    solo_su_android()
}

/// # La finestra che rinasce con l'attività
///
/// Sul telefono la finestra è l'attività, e l'attività muore senza che muoia
/// l'app: tolta dalle recenti, o chiusa dal sistema per memoria, mentre la
/// musica continua nel servizio. Quando la si riapre — dall'icona, dalla
/// notifica — Android crea un'attività nuova nello stesso processo, e nessuno
/// le dà una finestra: `tao` la registra e basta, e Tauri, che una finestra
/// l'ha già creata all'avvio e l'ha vista distruggere, non lo sa. Resta una
/// schermata vuota.
///
/// Così, quando la finestra se ne va ([`finestra_persa`], dal `RunEvent` di
/// `lib.rs`), ogni giro del ciclo degli eventi guarda se c'è di nuovo
/// un'attività ([`riapri_la_finestra`]), e quando c'è le ricostruisce la
/// finestra dalla configurazione, come all'avvio. `tao` la lega all'attività
/// libera, cioè a quella appena nata. La pagina riparte da capo e chiede al
/// nucleo lo stato, che è rimasto quello: la coda, il brano, la posizione.
#[cfg(mobile)]
static FINESTRA_PERSA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Se un tentativo di ricostruirla è già fallito: il diario lo dice una volta,
/// non a ogni giro del ciclo.
#[cfg(mobile)]
static RINASCITA_FALLITA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// La finestra se n'è andata con la sua attività: vedi [`FINESTRA_PERSA`].
#[cfg(mobile)]
pub fn finestra_persa() {
    FINESTRA_PERSA.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Ricostruisce la finestra se l'attività è tornata: vedi [`FINESTRA_PERSA`].
///
/// Gira sul filo del ciclo degli eventi, a ogni giro, e quando la finestra c'è
/// costa una lettura atomica.
#[cfg(mobile)]
pub fn riapri_la_finestra(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering::Relaxed;
    use tauri::Manager as _;

    if !FINESTRA_PERSA.load(Relaxed) {
        return;
    }
    if app.get_webview_window("main").is_some() {
        FINESTRA_PERSA.store(false, Relaxed);
        return;
    }
    // Senza un'attività `tao` non avrebbe dove metterla: si aspetta.
    #[cfg(target_os = "android")]
    if !rymembra_android::sistema::attivita_aperta().unwrap_or(false) {
        return;
    }
    let Some(configurazione) = app.config().app.windows.first() else {
        return;
    };
    match tauri::WebviewWindowBuilder::from_config(app, configurazione)
        .and_then(tauri::WebviewWindowBuilder::build)
    {
        Ok(_) => {
            FINESTRA_PERSA.store(false, Relaxed);
            RINASCITA_FALLITA.store(false, Relaxed);
            crate::nota!("[finestra] l'attività è tornata: la finestra rinasce");
        }
        Err(err) => {
            if !RINASCITA_FALLITA.swap(true, Relaxed) {
                crate::nota!("[finestra] l'attività è tornata, ma la finestra non rinasce: {err}");
            }
        }
    }
}

/// Il Keystore di Android, come portachiavi del sistema.
///
/// Lo installa [`installa_il_portachiavi`] all'avvio; da lì `DiSistema` — e
/// con lui Drive, lo scrobbling, i modelli di linguaggio, «Ascolta insieme» —
/// lo usa come sul computer usa il Credential Manager.
#[cfg(target_os = "android")]
struct Keystore;

#[cfg(target_os = "android")]
impl Keystore {
    /// Il guasto del Keystore, nominando la voce: lo stesso codice del
    /// Credential Manager.
    fn non_raggiungibile(chiave: &str, err: &rymembra_android::Errore) -> AppError {
        AppError::new(ErrorCode::SettingsSecretUnavailable {
            key: chiave.to_owned(),
        })
        .with_cause(err.to_string())
    }
}

#[cfg(target_os = "android")]
impl rymembra_oauth::portachiavi::Portachiavi for Keystore {
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
        rymembra_android::segreti::leggi(chiave)
            .map_err(|err| Self::non_raggiungibile(chiave, &err))
    }

    fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
        rymembra_android::segreti::scrivi(chiave, valore)
            .map_err(|err| Self::non_raggiungibile(chiave, &err))
    }

    fn cancella(&self, chiave: &str) -> Result<(), AppError> {
        rymembra_android::segreti::cancella(chiave)
            .map_err(|err| Self::non_raggiungibile(chiave, &err))
    }
}

/// Mette il Keystore dietro `DiSistema`. Su Android va chiamata una volta,
/// prima che parta chiunque usi un segreto; altrove non fa niente, perché il
/// portachiavi `DiSistema` ce l'ha già.
pub fn installa_il_portachiavi() {
    #[cfg(target_os = "android")]
    {
        static KEYSTORE: Keystore = Keystore;
        rymembra_oauth::portachiavi::installa_di_sistema(&KEYSTORE);
    }
}

/// Se Android lascia vedere la rete di casa. Fuori da Android, e prima di
/// Android 17, sempre sì; un ponte che non risponde vale sì, perché a dire di
/// no senza saperlo si fermerebbe «Ascolta insieme» per niente. Non chiede
/// niente a nessuno: vedi [`rete_locale_o_chiedila`].
pub fn rete_locale_concessa() -> bool {
    #[cfg(target_os = "android")]
    {
        rymembra_android::sistema::rete_locale().unwrap_or(true)
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}

/// La rete di casa: c'è, o la si chiede adesso col dialogo di Android.
///
/// Da Android 17 un'app che si dichiara per Android 17 non la vede senza il
/// permesso «Dispositivi nelle vicinanze», e senza un errore che lo dica: i
/// pacchetti verso la Wi-Fi si fermano nel sistema, e il telefono non trova
/// nessuno né si fa trovare. La chiamano i gesti di «Ascolta insieme» che ne
/// hanno bisogno — accendere, ospitare, associarsi, entrare — prima di farli,
/// e [`per_il_server`] per un server di casa.
///
/// Bloccante finché la persona risponde: dal pool bloccante, mai dal filo
/// della finestra.
///
/// # Errori
///
/// `net.localNetwork` se il permesso non c'è e non è stato concesso.
pub fn rete_locale_o_chiedila() -> Result<(), AppError> {
    #[cfg(target_os = "android")]
    {
        if rete_locale_concessa() {
            return Ok(());
        }
        let concessa = rymembra_android::sistema::chiedi_rete_locale()
            .map_err(|err| AppError::new(ErrorCode::NetLocalNetwork).with_cause(err.to_string()))?;
        if !concessa {
            crate::nota!("[rete] «Dispositivi nelle vicinanze» non concesso");
            return Err(AppError::new(ErrorCode::NetLocalNetwork));
        }
        crate::nota!("[rete] «Dispositivi nelle vicinanze» concesso");
    }
    Ok(())
}

/// Un server da raggiungere a `indirizzo` — un NAS WebDAV, un modello di
/// linguaggio su un altro PC — passa se non sta nella rete di casa, o se la rete
/// di casa c'è. Se sta lì e manca, con `chiedi` la si chiede adesso (vedi
/// [`rete_locale_o_chiedila`]), senza si risponde con l'errore che dice perché:
/// è la strada di chi riapre da sé, all'avvio, senza nessuno davanti.
///
/// «Nella rete di casa» è un indirizzo privato o di collegamento locale, scritto
/// così o come nome che si risolve in uno di quelli: è quel che Android 17
/// tiene chiuso. Fuori da Android passa sempre.
///
/// # Errori
///
/// `net.localNetwork`, come [`rete_locale_o_chiedila`].
pub fn per_il_server(indirizzo: &str, chiedi: bool) -> Result<(), AppError> {
    #[cfg(target_os = "android")]
    {
        if rete_locale_concessa()
            || !rymembra_net::http::ospite(indirizzo).is_some_and(nella_rete_di_casa)
        {
            return Ok(());
        }
        if chiedi {
            return rete_locale_o_chiedila();
        }
        Err(AppError::new(ErrorCode::NetLocalNetwork))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (indirizzo, chiedi);
        Ok(())
    }
}

/// L'ospite è nella rete di casa: un indirizzo privato o di collegamento
/// locale, o un nome che si risolve in uno di quelli. Un nome che non si
/// risolve non lo è: dirà da sé che non esiste.
#[cfg(target_os = "android")]
fn nella_rete_di_casa(ospite: &str) -> bool {
    use std::net::{IpAddr, ToSocketAddrs as _};
    let di_casa = |ip: &IpAddr| match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        // `fc00::/7`, gli indirizzi locali unici, e `fe80::/10`.
        IpAddr::V6(v6) => {
            let primo = v6.segments()[0];
            primo & 0xfe00 == 0xfc00 || primo & 0xffc0 == 0xfe80
        }
    };
    if let Ok(ip) = ospite.parse::<IpAddr>() {
        return di_casa(&ip);
    }
    (ospite, 0)
        .to_socket_addrs()
        .is_ok_and(|mut indirizzi| indirizzi.any(|indirizzo| di_casa(&indirizzo.ip())))
}

/// Insegna al motore audio a chiedere ad Android dove sta suonando. Su
/// Android va chiamata una volta, prima che il motore si apra; altrove non fa
/// niente, perché lì le uscite le enumera `cpal`. Vedi
/// `rymembra_play::dispositivi`, «Su Android si segue Android».
pub fn installa_l_uscita_audio() {
    #[cfg(target_os = "android")]
    rymembra_play::dispositivi::installa_l_uscita_di_android(|| {
        // Un ponte che non risponde vale «non lo so»: l'uscita si chiama
        // «Android», e si suona lo stesso dove dice Android.
        rymembra_android::sistema::uscita_audio()
            .ok()
            .flatten()
            .map(|uscita| rymembra_play::dispositivi::UscitaDiAndroid {
                nome: uscita.nome,
                bluetooth: uscita.bluetooth,
            })
    });
}

#[cfg(test)]
mod prove {
    use super::*;

    /// «Download» o un'app cloud scelti nel selettore: un percorso che non si
    /// può usare, con la frase che dice dove sceglierne uno, e non un guasto da
    /// «Riavvia Rymembra».
    #[test]
    fn una_cartella_fuori_memoria_non_e_un_guasto() {
        let err = cartella_fuori("com.android.providers.downloads.documents");
        assert!(
            matches!(err.code(), ErrorCode::FsPathInvalid { path } if path.contains("downloads")),
            "{:?}",
            err.code()
        );
        assert!(!err.is_retryable());
    }

    #[test]
    fn un_percorso_fuori_dalla_cache_non_si_salva_ne_si_cancella() {
        let radice = tempfile::tempdir().expect("cartella");
        let cache = radice.path().join("cache");
        let cartella = cache.join("da-salvare");
        std::fs::create_dir_all(cartella.join("dentro")).expect("cartelle");
        std::fs::create_dir_all(radice.path().join("databases")).expect("cartelle");
        std::fs::write(cartella.join("lista.m3u"), b"#EXTM3U").expect("file");
        std::fs::write(cartella.join("dentro").join("x"), b"x").expect("file");
        std::fs::write(cache.join("lista.m3u"), b"#EXTM3U").expect("file");
        let libreria = radice.path().join("databases").join("libreria.db");
        std::fs::write(&libreria, b"SQLite").expect("file");

        // Risalire con `..`, un percorso assoluto altrove, la cartella stessa,
        // una sottocartella, un file che non c'è, la cache sopra: niente.
        for fuori in [
            cartella
                .join("..")
                .join("..")
                .join("databases")
                .join("libreria.db"),
            libreria.clone(),
            cartella.clone(),
            cartella.join("dentro").join("x"),
            cartella.join("manca.m3u"),
            cache.join("lista.m3u"),
        ] {
            let err = da_salvare(&cartella, &fuori).expect_err("fuori dalla cartella");
            assert!(
                matches!(err.code(), ErrorCode::FsPathInvalid { .. }),
                "{}: {:?}",
                fuori.display(),
                err.code()
            );
        }
        // E il file fuori non è stato toccato.
        assert!(libreria.is_file());

        // Quel che `telefono_file_temporaneo` dà: un figlio diretto, nella
        // forma canonica, che è quella che poi si copia e si cancella.
        let buono = da_salvare(&cartella, &cartella.join("lista.m3u")).expect("dentro");
        assert_eq!(
            buono,
            cartella.join("lista.m3u").canonicalize().expect("canonico")
        );
    }

    #[cfg(unix)]
    #[test]
    fn un_collegamento_dentro_la_cache_non_porta_fuori() {
        let radice = tempfile::tempdir().expect("cartella");
        let cartella = radice.path().join("cache").join("da-salvare");
        std::fs::create_dir_all(&cartella).expect("cartelle");
        let libreria = radice.path().join("libreria.db");
        std::fs::write(&libreria, b"SQLite").expect("file");
        let collegamento = cartella.join("lista.m3u");
        std::os::unix::fs::symlink(&libreria, &collegamento).expect("collegamento");
        assert!(da_salvare(&cartella, &collegamento).is_err());
    }
}
