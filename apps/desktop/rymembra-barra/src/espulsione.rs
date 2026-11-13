//! Il volume che Windows sta per espellere: avvisare chi ha cartelle aperte
//! lì sopra, perché le lasci.
//!
//! # Perché sta qui
//!
//! La sorveglianza delle cartelle (`notify`, `ReadDirectoryChangesW`) tiene
//! aperto un handle su ogni cartella della libreria. Su una chiavetta o su un
//! disco USB questo fa rispondere «Il dispositivo è in uso» a «Espelli» di
//! Esplora risorse, per tutto il tempo in cui Rymembra è aperto, anche chiuso
//! nel vassoio (R24-1). Windows ha un modo per chiedere a chi tiene aperto un
//! dispositivo di lasciarlo — è quello che Microsoft descrive in «Processing a
//! Request to Remove a Device» — e passa da una chiamata `unsafe`: la radice del
//! workspace vieta `unsafe` con `forbid`, e questo è l'unico crate con
//! l'eccezione dichiarata. Quel che si fa quando Windows chiede lo decide chi
//! chiama: questo modulo sa dire «quale volume», e «è successo questo».
//!
//! # La strada: `CM_Register_Notification`
//!
//! Due strade erano possibili. `RegisterDeviceNotificationW` vuole una
//! finestra, e la finestra è quella della barra delle applicazioni: se
//! `aggancia` fallisce, l'espulsione tornerebbe a essere rifiutata senza che
//! nessuno lo sappia; e il messaggio `WM_DEVICECHANGE` arriva sul filo
//! dell'interfaccia, che resterebbe fermo per tutto il tempo in cui chi ha le
//! cartelle risponde. `CM_Register_Notification` non vuole una finestra: la
//! callback gira su un filo del sistema, e l'handle con cui ci si registra
//! deve restare aperto **solo durante la chiamata** — dopo, dice la
//! documentazione di `CM_NOTIFY_FILTER`, «the handle can be closed without
//! affecting the ability for the registration to receive notifications». Qui lo
//! si apre, ci si registra e lo si chiude subito: nessun handle nostro resta sul
//! volume, e non c'è niente da chiudere quando Windows chiede di lasciarlo.
//!
//! # Il volume, non la lettera
//!
//! La chiave di un volume è il suo nome unico, `\\?\Volume{…}\`, che
//! `GetVolumeNameForVolumeMountPointW` dà per il punto di montaggio che
//! `GetVolumePathNameW` trova: `E:\`, oppure `C:\Musica\SD\` se la scheda è
//! montata in una cartella di `C:`. Due cartelle sullo stesso volume hanno la
//! stessa chiave — e una cartella che è la radice del volume, o una cartella
//! montata altrove, anche. Una condivisione di rete non ha un nome di volume, e
//! un percorso senza lettera non arriva nemmeno a Windows: [`volume_di`]
//! risponde `None`, e la sorveglianza non registra niente (vedi
//! [`crate::unita`] per lo stesso cancello).
//!
//! # Cosa succede quando
//!
//! Il filo del sistema chiama la callback con una delle azioni di
//! `CM_NOTIFY_ACTION`; qui diventano un [`Avviso`]:
//!
//! - **`StaPerAndarsene`** (query remove, o remove pending se nessuno l'ha
//!   preceduto: l'estrazione a sorpresa): chi ha cartelle del volume aperte le
//!   chiude **prima che la funzione che si passa a [`osserva_volume`] ritorni**.
//!   Windows aspetta quel ritorno, quindi la funzione deve ritornare presto: chi
//!   la scrive aspetta le sue risposte con una scadenza, e qui si risponde
//!   sempre «va bene» (`ERROR_SUCCESS`, mai `ERROR_CANCELLED`): se chi doveva
//!   lasciare non ha fatto in tempo, l'espulsione fallirà per l'handle che
//!   resta, e Windows lo dirà all'utente. Dire di no al posto suo non
//!   servirebbe a niente.
//! - **`Rifiutato`** (query remove failed): un altro programma ha detto di no.
//!   Il volume resta, e chi l'aveva lasciato torna a guardarlo. La
//!   registrazione resta valida.
//! - **`Andato`** (remove complete): il volume non c'è più, e la registrazione
//!   con lui. Si lascia cadere la [`Registrazione`]; quando il volume torna se
//!   ne fa una nuova.
//!
//! # Chi toglie la registrazione
//!
//! `CM_Unregister_Notification` aspetta che le callback in corso finiscano:
//! chiamata dentro una callback, o da un filo da cui dipende una callback in
//! attesa, si blocca. Lasciar cadere una [`Registrazione`] la toglie su un
//! filo apposta e subito, così chi la lascia cadere non aspetta mai; il
//! contesto della callback si libera solo dopo che la funzione di sistema è
//! tornata.

use std::path::Path;

#[cfg(windows)]
use std::ffi::c_void;

#[cfg(windows)]
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_NOTIFY_ACTION, CM_NOTIFY_EVENT_DATA, HCMNOTIFICATION,
};

/// Quel che Windows dice di un volume che qualcuno vuole togliere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Avviso {
    /// Il volume sta per essere espulso: chi ha cartelle aperte lì sopra le
    /// chiuda adesso, e risponda presto.
    StaPerAndarsene,
    /// Qualcuno ha rifiutato l'espulsione: il volume resta.
    Rifiutato,
    /// Il volume non c'è più.
    Andato,
}

impl Avviso {
    /// L'avviso dal numero di `CM_NOTIFY_ACTION`; `None` per le azioni che qui
    /// non servono (un evento personalizzato, un'istanza che parte).
    ///
    /// I numeri sono quelli di `cfgmgr32.h`, scritti qui invece che importati:
    /// la parte dell'API che li definisce c'è solo su Windows, e questa
    /// traduzione si prova dappertutto. Una prova su Windows li confronta con
    /// le costanti vere.
    #[must_use]
    pub const fn dal_codice(azione: i32) -> Option<Self> {
        match azione {
            // `DEVICEQUERYREMOVE`, e `DEVICEREMOVEPENDING`: la seconda arriva
            // da sola se il dispositivo è tolto a sorpresa, senza che nessuno
            // sia stato interpellato.
            2 | 4 => Some(Self::StaPerAndarsene),
            // `DEVICEQUERYREMOVEFAILED`.
            3 => Some(Self::Rifiutato),
            // `DEVICEREMOVECOMPLETE`.
            5 => Some(Self::Andato),
            _ => None,
        }
    }
}

/// Il nome unico di un volume, `\\?\Volume{GUID}\`: la forma che
/// `GetVolumeNameForVolumeMountPointW` restituisce, e l'unica che
/// [`osserva_volume`] accetta.
///
/// Pura: la chiave non si inventa a mano né arriva da altrove, e qui si
/// controlla la forma, non l'esistenza.
#[cfg_attr(
    not(any(windows, test)),
    allow(dead_code, reason = "la legge solo la chiamata di Windows")
)]
fn e_un_nome_di_volume(chiave: &str) -> bool {
    const TESTA: &str = r"\\?\Volume{";
    const CODA: &str = "}\\";
    let Some(guid) = chiave
        .strip_prefix(TESTA)
        .and_then(|resto| resto.strip_suffix(CODA))
    else {
        return false;
    };
    // `8-4-4-4-12` cifre esadecimali: 36 caratteri.
    guid.len() == 36
        && guid.char_indices().all(|(posto, lettera)| match posto {
            8 | 13 | 18 | 23 => lettera == '-',
            _ => lettera.is_ascii_hexdigit(),
        })
}

/// La chiave del volume su cui sta `percorso`, se Windows la sa dare.
///
/// `None` fuori da Windows; per un percorso senza lettera (un UNC: per trovarne
/// il volume Windows andrebbe sulla rete); per una lettera mappata su una
/// condivisione, che un nome di volume non ce l'ha; e per un volume che Windows
/// non sa trovare. Chi chiama non lo distingue, e non serve: in tutti questi
/// casi non c'è niente da registrare.
///
/// Come per [`crate::unita_di`], trovare il volume vuol dire guardare le
/// cartelle del percorso, quindi il disco: va chiesto solo per un percorso che
/// si sa locale, e su un filo con la sua scadenza.
#[must_use]
pub fn volume_di(percorso: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        crate::unita::radice(percorso)?;
        nome_del_volume(percorso)
    }
    #[cfg(not(windows))]
    {
        let _ = percorso;
        None
    }
}

/// Una registrazione per gli avvisi su un volume. Lasciarla cadere la toglie.
///
/// Vedi «Chi toglie la registrazione» nell'introduzione del modulo: la caduta
/// non aspetta nessuno.
#[derive(Debug)]
#[must_use = "la registrazione dura quanto il valore: lasciato cadere subito, non resta niente"]
pub struct Registrazione {
    #[cfg(windows)]
    dentro: Option<Dentro>,
}

/// I due puntatori che una registrazione si porta dietro, come interi: un
/// puntatore grezzo non passa da un filo all'altro, un numero sì.
#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct Dentro {
    /// L'`HCMNOTIFICATION` che `CM_Register_Notification` ha dato.
    notifica: usize,
    /// Il `Box<Contesto>` consegnato alla callback, vivo fino a quando
    /// `CM_Unregister_Notification` non è tornata.
    contesto: usize,
}

impl Drop for Registrazione {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(dentro) = self.dentro.take() {
            scarica(dentro);
        }
    }
}

/// Chiede a Windows di avvisare quando il volume `volume` — la chiave di
/// [`volume_di`] — sta per andarsene, resta, o è andato.
///
/// `su_avviso` gira su un filo del sistema, e per [`Avviso::StaPerAndarsene`]
/// Windows aspetta il suo ritorno: deve tornare presto, con le cartelle già
/// chiuse. Non deve toccare la [`Registrazione`] che la possiede: toglierla da
/// lì sarebbe il blocco che la documentazione di Microsoft avverte di non
/// provocare. Per lo stesso avviso, `su_avviso` è chiamata **una volta sola**
/// per tentativo, anche se Windows ne manda due di fila (query remove e remove
/// pending): la seconda non aspetterebbe niente di nuovo.
///
/// # Errors
///
/// Una frase che dice perché non si è registrato: la chiave non è un nome di
/// volume, il volume non si apre, Windows rifiuta, o non si è su Windows. In
/// ognuno di questi casi la sorveglianza resta com'era: niente va perso, solo
/// l'espulsione tornerà a essere rifiutata.
pub fn osserva_volume(
    volume: &str,
    su_avviso: impl Fn(Avviso) + Send + Sync + 'static,
) -> Result<Registrazione, String> {
    #[cfg(windows)]
    {
        registra(volume, Box::new(su_avviso))
    }
    #[cfg(not(windows))]
    {
        let _ = (volume, su_avviso);
        Err("gli avvisi di espulsione esistono solo su Windows".to_owned())
    }
}

// ── la parte che parla con Windows ──────────────────────────────────────────

/// Quel che la callback si porta dietro.
#[cfg(windows)]
struct Contesto {
    su_avviso: Box<dyn Fn(Avviso) + Send + Sync>,
    /// Un tentativo di espulsione è già stato annunciato, e non è né rifiutato
    /// né finito.
    in_partenza: std::sync::atomic::AtomicBool,
}

/// Se l'avviso di `azione` va davvero consegnato, e aggiorna lo stato di
/// `in_partenza`: pura sul dato, perché è la parte che si prova senza
/// Windows.
///
/// Il primo `StaPerAndarsene` passa e il secondo no; un `Rifiutato` riapre il
/// tentativo; `Andato` passa sempre.
#[cfg(windows)]
fn da_consegnare(avviso: Avviso, in_partenza: &std::sync::atomic::AtomicBool) -> bool {
    use std::sync::atomic::Ordering::SeqCst;
    match avviso {
        Avviso::StaPerAndarsene => !in_partenza.swap(true, SeqCst),
        Avviso::Rifiutato => {
            in_partenza.store(false, SeqCst);
            true
        }
        Avviso::Andato => true,
    }
}

/// `GetVolumePathNameW` sul percorso, e `GetVolumeNameForVolumeMountPointW` sul
/// punto di montaggio che restituisce.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "GetVolumePathNameW e GetVolumeNameForVolumeMountPointW non hanno \
              un'interfaccia sicura: ogni blocco dice perché è corretto"
)]
fn nome_del_volume(percorso: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows::Win32::Storage::FileSystem::{
        GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    };
    use windows::core::PCWSTR;

    /// `MAX_PATH`, il minimo che si dà al punto di montaggio.
    const LUNGHEZZA_MINIMA: usize = 260;
    /// `\\?\Volume{GUID}\` sono 49 caratteri; con lo zero e un po' di margine.
    const LUNGHEZZA_DEL_NOME: usize = 64;

    let largo: Vec<u16> = percorso.as_os_str().encode_wide().collect();
    // Uno zero dentro il percorso lo taglierebbe, e Windows cercherebbe il
    // volume di un altro percorso: meglio non sapere.
    if largo.contains(&0) {
        return None;
    }
    let largo: Vec<u16> = largo.into_iter().chain(std::iter::once(0)).collect();
    // Come in `unita::chiedi_a_windows`: il punto di montaggio è un pezzo
    // iniziale del percorso, al più con una barra in più.
    let mut montaggio = vec![0_u16; largo.len().saturating_add(1).max(LUNGHEZZA_MINIMA)];
    // SAFETY: `largo` è una stringa UTF-16 terminata da zero che vive fino alla
    // fine di questa funzione, e Windows la legge soltanto. `montaggio` è un
    // buffer di cui si passano puntatore e lunghezza veri: Windows ci scrive al
    // più quella lunghezza, zero finale compreso, e non conserva né l'uno né
    // l'altro.
    if unsafe { GetVolumePathNameW(PCWSTR(largo.as_ptr()), &mut montaggio) }.is_err() {
        return None;
    }
    // Andato a buon fine, il punto di montaggio ha il suo zero dentro il
    // buffer; se mancasse, la stringa non andrebbe passata a nessuno.
    if !montaggio.contains(&0) {
        return None;
    }
    let mut nome = [0_u16; LUNGHEZZA_DEL_NOME];
    // SAFETY: `montaggio` è una stringa UTF-16 terminata da zero dentro il suo
    // buffer (appena controllato), che vive oltre la chiamata e che Windows
    // legge soltanto. `nome` è un array nostro di cui si passano puntatore e
    // lunghezza veri: Windows ci scrive al più quella lunghezza, e non lo
    // conserva.
    if unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(montaggio.as_ptr()), &mut nome) }.is_err()
    {
        return None;
    }
    let fine = nome.iter().position(|&lettera| lettera == 0)?;
    let nome = String::from_utf16(nome.get(..fine)?).ok()?;
    e_un_nome_di_volume(&nome).then_some(nome)
}

/// Apre il volume, come lo chiede `CM_NOTIFY_FILTER`: un handle «al
/// dispositivo». Si tenta il dispositivo del volume (`\\?\Volume{…}` senza la
/// barra), e se Windows non lo concede la radice del volume come cartella: la
/// documentazione di Microsoft ammette per la registrazione un handle a «file
/// or device».
///
/// Senza diritti di accesso (basta l'handle per identificare il dispositivo) e
/// con tutte le condivisioni, per non disturbare nessuno: chi lo apre lo
/// chiude dopo la registrazione. Per questo niente `unsafe`: lo apre la
/// libreria standard, e lo chiude `Drop`.
#[cfg(windows)]
fn apri_il_volume(volume: &str) -> Result<std::fs::File, String> {
    use std::os::windows::fs::OpenOptionsExt as _;

    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let mut opzioni = std::fs::OpenOptions::new();
    opzioni
        .access_mode(0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        // Per la radice aperta come cartella; sul dispositivo non conta.
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0);
    let dispositivo = volume.strip_suffix('\\').unwrap_or(volume);
    opzioni
        .open(dispositivo)
        .or_else(|_| opzioni.open(volume))
        .map_err(|err| format!("il volume non si apre: {err}"))
}

#[cfg(windows)]
fn registra(
    volume: &str,
    su_avviso: Box<dyn Fn(Avviso) + Send + Sync>,
) -> Result<Registrazione, String> {
    if !e_un_nome_di_volume(volume) {
        return Err("non è il nome di un volume".to_owned());
    }
    let maniglia = apri_il_volume(volume)?;
    let contesto = Box::new(Contesto {
        su_avviso,
        in_partenza: std::sync::atomic::AtomicBool::new(false),
    });
    // La maniglia vive fino alla fine di questa funzione, cioè oltre la
    // chiamata: poi si chiude, e la registrazione resta (vedi «La strada» nel
    // preambolo del modulo).
    let dentro = chiedi_la_notifica(&maniglia, contesto)?;
    Ok(Registrazione {
        dentro: Some(dentro),
    })
}

/// `CM_Register_Notification` con `CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE` sulla
/// maniglia, e `contesto` come contesto della callback.
///
/// Il contesto diventa un puntatore **prima** della chiamata, perché la
/// callback può scattare prima che `CM_Register_Notification` ritorni; se la
/// registrazione non nasce torna a essere un `Box` e cade qui.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "CM_Register_Notification non ha un'interfaccia sicura, e il contesto è un \
              puntatore grezzo: ogni blocco dice perché è corretto"
)]
fn chiedi_la_notifica(maniglia: &std::fs::File, contesto: Box<Contesto>) -> Result<Dentro, String> {
    use std::os::windows::io::AsRawHandle as _;

    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        CM_NOTIFY_FILTER, CM_NOTIFY_FILTER_0_1, CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE,
        CM_Register_Notification, CR_SUCCESS,
    };
    use windows::Win32::Foundation::HANDLE;

    let contesto = Box::into_raw(contesto);
    let mut filtro = CM_NOTIFY_FILTER {
        cbSize: u32::try_from(std::mem::size_of::<CM_NOTIFY_FILTER>()).unwrap_or(0),
        FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE,
        ..CM_NOTIFY_FILTER::default()
    };
    filtro.u.DeviceHandle = CM_NOTIFY_FILTER_0_1 {
        hTarget: HANDLE(maniglia.as_raw_handle()),
    };
    let mut notifica = HCMNOTIFICATION::default();
    // SAFETY: `filtro` è un `CM_NOTIFY_FILTER` nostro, con la sua misura e il
    // tipo `DEVICEHANDLE` che ha l'handle dentro, e Windows lo legge soltanto
    // durante la chiamata. L'handle è aperto (lo tiene `maniglia`, che vive
    // oltre la chiamata) e a registrazione fatta può essere chiuso. `contesto`
    // è un `Box<Contesto>` consegnato per intero: la callback lo legge finché
    // la registrazione dura, e lo libera soltanto `disfai`, dopo
    // `CM_Unregister_Notification`, che aspetta le callback in corso.
    // `notifica` è una variabile nostra, viva per la chiamata.
    let esito = unsafe {
        CM_Register_Notification(
            &raw const filtro,
            Some(contesto.cast::<c_void>()),
            Some(quando_windows_avvisa),
            &raw mut notifica,
        )
    };
    if esito == CR_SUCCESS {
        return Ok(Dentro {
            notifica: notifica.0 as usize,
            contesto: contesto as usize,
        });
    }
    // SAFETY: la registrazione non è nata, quindi nessuna callback ha il
    // puntatore e nessuna lo avrà: torna a essere soltanto nostro, ed è
    // quello che `Box::into_raw` ha consegnato qui sopra.
    drop(unsafe { Box::from_raw(contesto) });
    Err(format!(
        "Windows rifiuta la registrazione degli avvisi (CR {})",
        esito.0
    ))
}

/// La callback che Windows chiama, su un filo suo, per ogni avviso.
///
/// # Safety
///
/// La chiama soltanto Windows, per la registrazione di [`registra`]: `contesto`
/// è il `Box<Contesto>` consegnato lì, vivo fino a quando
/// `CM_Unregister_Notification` non è tornata.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "è la funzione che Windows chiama: ogni blocco dice perché è corretto"
)]
unsafe extern "system" fn quando_windows_avvisa(
    _notifica: HCMNOTIFICATION,
    contesto: *const c_void,
    azione: CM_NOTIFY_ACTION,
    _dati: *const CM_NOTIFY_EVENT_DATA,
    _misura: u32,
) -> u32 {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    // SAFETY: `contesto` è il puntatore consegnato a `CM_Register_Notification`,
    // un `Box<Contesto>` vivo: `disfai` lo libera solo dopo che
    // `CM_Unregister_Notification` è tornata, e quella aspetta questa
    // chiamata. Se ne prende un riferimento condiviso, mai esclusivo: due
    // callback possono girare insieme.
    let Some(contesto) = (unsafe { contesto.cast::<Contesto>().as_ref() }) else {
        return 0;
    };
    if let Some(avviso) = Avviso::dal_codice(azione.0)
        && da_consegnare(avviso, &contesto.in_partenza)
    {
        // Dentro `catch_unwind` perché questa è una funzione chiamata da
        // Windows: un panico che ne uscisse non avrebbe nessuno a raccoglierlo,
        // e Rust termina il processo. Un avviso perso è meglio di un lettore
        // che si chiude.
        let _ = catch_unwind(AssertUnwindSafe(|| (contesto.su_avviso)(avviso)));
    }
    // `ERROR_SUCCESS`: mai `ERROR_CANCELLED`, vedi l'introduzione del modulo.
    0
}

/// Toglie la registrazione su un filo apposta: chi lascia cadere una
/// [`Registrazione`] non aspetta le callback in corso.
#[cfg(windows)]
fn scarica(dentro: Dentro) {
    let dello_scarico = dentro;
    let avviato = std::thread::Builder::new()
        .name("rymembra-volume-scarico".to_owned())
        .spawn(move || disfai(dello_scarico));
    if avviato.is_err() {
        // Nessun filo da avere: si toglie qui. Chi lascia cadere una
        // registrazione non è una callback, e al più aspetta quella in corso,
        // che ha la sua scadenza.
        disfai(dentro);
    }
}

/// `CM_Unregister_Notification`, e solo dopo il contesto liberato.
///
/// Se Windows non toglie la registrazione le callback potrebbero arrivare
/// ancora: il contesto allora si lascia dov'è, e costa la sua memoria invece di
/// un accesso a memoria liberata.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "CM_Unregister_Notification non ha un'interfaccia sicura, e il contesto \
              è un puntatore grezzo: ogni blocco dice perché è corretto"
)]
fn disfai(dentro: Dentro) {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Unregister_Notification, CR_SUCCESS,
    };

    // SAFETY: `notifica` è l'`HCMNOTIFICATION` che `chiedi_la_notifica` ha avuto
    // da Windows, e questa è l'unica volta che lo si consegna: `Dentro` si
    // porta via da `Registrazione` con `take`. Non si è dentro una callback
    // (la chiama un filo nostro), quindi l'attesa delle callback in corso non
    // si blocca su se stessa.
    let esito =
        unsafe { CM_Unregister_Notification(HCMNOTIFICATION(dentro.notifica as *mut c_void)) };
    if esito == CR_SUCCESS {
        // SAFETY: il `Box<Contesto>` di `registra`, e nessun altro lo libera;
        // `CM_Unregister_Notification` è tornata, quindi nessuna callback lo
        // sta più usando né lo userà.
        drop(unsafe { Box::from_raw(dentro.contesto as *mut Contesto) });
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_numeri_di_windows_si_leggono() {
        // Query remove e remove pending: il volume sta per andarsene.
        assert_eq!(Avviso::dal_codice(2), Some(Avviso::StaPerAndarsene));
        assert_eq!(Avviso::dal_codice(4), Some(Avviso::StaPerAndarsene));
        assert_eq!(Avviso::dal_codice(3), Some(Avviso::Rifiutato));
        assert_eq!(Avviso::dal_codice(5), Some(Avviso::Andato));
        // Il resto non serve: un arrivo, una rimozione di interfaccia, un
        // evento personalizzato, un'istanza che parte.
        for altro in [0, 1, 6, 7, 8, 9, -1, 99] {
            assert_eq!(Avviso::dal_codice(altro), None, "{altro}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn i_numeri_sono_quelli_di_windows() {
        use windows::Win32::Devices::DeviceAndDriverInstallation as cm;
        assert_eq!(
            Avviso::dal_codice(cm::CM_NOTIFY_ACTION_DEVICEQUERYREMOVE.0),
            Some(Avviso::StaPerAndarsene)
        );
        assert_eq!(
            Avviso::dal_codice(cm::CM_NOTIFY_ACTION_DEVICEREMOVEPENDING.0),
            Some(Avviso::StaPerAndarsene)
        );
        assert_eq!(
            Avviso::dal_codice(cm::CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED.0),
            Some(Avviso::Rifiutato)
        );
        assert_eq!(
            Avviso::dal_codice(cm::CM_NOTIFY_ACTION_DEVICEREMOVECOMPLETE.0),
            Some(Avviso::Andato)
        );
        for altro in [
            cm::CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL,
            cm::CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL,
            cm::CM_NOTIFY_ACTION_DEVICECUSTOMEVENT,
            cm::CM_NOTIFY_ACTION_DEVICEINSTANCEENUMERATED,
            cm::CM_NOTIFY_ACTION_DEVICEINSTANCESTARTED,
            cm::CM_NOTIFY_ACTION_DEVICEINSTANCEREMOVED,
        ] {
            assert_eq!(Avviso::dal_codice(altro.0), None, "{}", altro.0);
        }
    }

    #[test]
    fn il_nome_di_un_volume_ha_la_sua_forma() {
        let buono = r"\\?\Volume{0a1b2c3d-4e5f-6789-abcd-ef0123456789}\";
        assert!(e_un_nome_di_volume(buono));
        // Maiuscole comprese: Windows le dà come vuole.
        assert!(e_un_nome_di_volume(
            r"\\?\Volume{0A1B2C3D-4E5F-6789-ABCD-EF0123456789}\"
        ));
        for storto in [
            "",
            r"E:\",
            r"\\?\E:\",
            // Senza la barra finale: è il dispositivo, non il nome.
            r"\\?\Volume{0a1b2c3d-4e5f-6789-abcd-ef0123456789}",
            // Un percorso sotto il volume non è il volume.
            r"\\?\Volume{0a1b2c3d-4e5f-6789-abcd-ef0123456789}\Musica\",
            r"\\?\Volume{0a1b2c3d-4e5f-6789-abcd-ef012345678}\",
            r"\\?\Volume{0a1b2c3d-4e5f-6789-abcd-ef01234567zz}\",
            r"\\?\Volume{0a1b2c3d04e5f-6789-abcd-ef0123456789}\",
            r"\\.\Volume{0a1b2c3d-4e5f-6789-abcd-ef0123456789}\",
            r"\\nas\musica\",
        ] {
            assert!(!e_un_nome_di_volume(storto), "{storto}");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn fuori_da_windows_non_si_chiede_e_non_si_registra() {
        assert_eq!(volume_di(Path::new("/media/chiavetta/a.flac")), None);
        assert!(osserva_volume("x", |_| {}).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn il_volume_di_un_percorso_e_il_suo_e_non_la_lettera() {
        let qui = std::fs::canonicalize(".").expect("la cartella di lavoro c'è");
        let volume = volume_di(&qui).expect("il volume della cartella di lavoro");
        assert!(e_un_nome_di_volume(&volume), "{volume}");
        // Una cartella dentro lo stesso volume, esistente o no, ha la stessa
        // chiave: sono due cartelle sorvegliate sullo stesso disco.
        assert_eq!(volume_di(&qui.join("non-esiste").join("a")), Some(volume));
    }

    #[cfg(windows)]
    #[test]
    fn senza_lettera_non_c_e_un_volume() {
        // Un UNC non arriva a Windows: per trovarne il volume andrebbe sulla
        // rete. Né un percorso relativo.
        let prima = std::time::Instant::now();
        assert_eq!(
            volume_di(Path::new(
                r"\\rymembra-server-che-non-esiste.invalid\m\a.flac"
            )),
            None
        );
        assert_eq!(
            volume_di(Path::new(
                r"\\?\UNC\rymembra-server-che-non-esiste.invalid\m\a.flac"
            )),
            None
        );
        assert_eq!(volume_di(Path::new(r"Musica\a.flac")), None);
        assert!(
            prima.elapsed() < std::time::Duration::from_secs(1),
            "la forma basta: nessuna rete"
        );
    }

    #[cfg(windows)]
    #[test]
    fn una_chiave_che_non_e_un_volume_non_si_registra() {
        for storta in ["", r"C:\", r"\\?\C:\", r"\\?\Volume{x}\"] {
            assert!(osserva_volume(storta, |_| {}).is_err(), "{storta}");
        }
    }

    /// Il volume vero della cartella di lavoro si apre e Windows accetta la
    /// registrazione: è la prova che l'handle e il filtro sono quelli giusti.
    /// Non toglie niente a nessuno, e la registrazione cade subito.
    #[cfg(windows)]
    #[test]
    fn windows_accetta_la_registrazione_e_la_toglie() {
        let qui = std::fs::canonicalize(".").expect("la cartella di lavoro c'è");
        let volume = volume_di(&qui).expect("il volume della cartella di lavoro");
        let registrazione = osserva_volume(&volume, |_| {}).expect("la registrazione");
        drop(registrazione);
    }

    /// La callback, chiamata a mano come la chiamerebbe Windows: il primo
    /// «sta per andarsene» arriva a chi l'ha chiesto e il secondo no, un
    /// rifiuto riapre il tentativo, e la risposta è sempre «va bene».
    #[cfg(windows)]
    #[test]
    #[allow(unsafe_code, reason = "si chiama la callback come farebbe Windows")]
    fn la_callback_consegna_una_volta_sola_e_risponde_di_si() {
        use std::sync::{Arc, Mutex};

        use windows::Win32::Devices::DeviceAndDriverInstallation as cm;

        let visti = Arc::new(Mutex::new(Vec::new()));
        let dentro = Arc::clone(&visti);
        let contesto = Box::into_raw(Box::new(Contesto {
            su_avviso: Box::new(move |avviso| {
                dentro.lock().expect("lucchetto").push(avviso);
            }),
            in_partenza: std::sync::atomic::AtomicBool::new(false),
        }));
        let manda = |azione: cm::CM_NOTIFY_ACTION| {
            // SAFETY: `contesto` è il `Box<Contesto>` di sopra, vivo fino alla
            // fine della prova; i dati dell'evento la callback non li legge.
            unsafe {
                quando_windows_avvisa(
                    cm::HCMNOTIFICATION::default(),
                    contesto.cast::<c_void>(),
                    azione,
                    std::ptr::null(),
                    0,
                )
            }
        };
        assert_eq!(manda(cm::CM_NOTIFY_ACTION_DEVICEQUERYREMOVE), 0);
        assert_eq!(manda(cm::CM_NOTIFY_ACTION_DEVICEREMOVEPENDING), 0);
        assert_eq!(manda(cm::CM_NOTIFY_ACTION_DEVICECUSTOMEVENT), 0);
        assert_eq!(manda(cm::CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED), 0);
        // Il tentativo dopo il rifiuto è un altro: si annuncia di nuovo.
        assert_eq!(manda(cm::CM_NOTIFY_ACTION_DEVICEREMOVEPENDING), 0);
        assert_eq!(manda(cm::CM_NOTIFY_ACTION_DEVICEREMOVECOMPLETE), 0);
        assert_eq!(
            *visti.lock().expect("lucchetto"),
            vec![
                Avviso::StaPerAndarsene,
                Avviso::Rifiutato,
                Avviso::StaPerAndarsene,
                Avviso::Andato,
            ]
        );
        // SAFETY: il `Box` di sopra, e nessun altro lo libera.
        drop(unsafe { Box::from_raw(contesto) });
    }

    /// Un panico di chi ascolta non esce dalla callback: Rust terminerebbe il
    /// processo.
    #[cfg(windows)]
    #[test]
    #[allow(unsafe_code, reason = "si chiama la callback come farebbe Windows")]
    #[allow(clippy::panic, reason = "è proprio il panico che si prova")]
    fn un_panico_non_esce_dalla_callback() {
        use windows::Win32::Devices::DeviceAndDriverInstallation as cm;

        let contesto = Box::into_raw(Box::new(Contesto {
            su_avviso: Box::new(|_| panic!("chi ascolta cade")),
            in_partenza: std::sync::atomic::AtomicBool::new(false),
        }));
        // SAFETY: come nella prova qui sopra.
        let risposta = unsafe {
            quando_windows_avvisa(
                cm::HCMNOTIFICATION::default(),
                contesto.cast::<c_void>(),
                cm::CM_NOTIFY_ACTION_DEVICEQUERYREMOVE,
                std::ptr::null(),
                0,
            )
        };
        assert_eq!(risposta, 0);
        // SAFETY: il `Box` di sopra, e nessun altro lo libera.
        drop(unsafe { Box::from_raw(contesto) });
    }
}
