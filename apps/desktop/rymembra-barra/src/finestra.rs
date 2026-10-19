//! La parte che parla con Windows: l'unico modulo di Rymembra con `unsafe`.
//!
//! # Come funziona
//!
//! I pulsanti nell'anteprima si chiedono alla barra delle applicazioni con
//! `ITaskbarList3::ThumbBarAddButtons`, e i clic tornano come `WM_COMMAND`
//! alla finestra che li ha chiesti. Per vedere quei messaggi bisogna mettersi
//! in mezzo alla procedura della finestra, che è di `tao`: lo si fa con
//! `SetWindowSubclass`, il modo che Windows offre apposta per aggiungere un
//! ascoltatore senza togliere quello che c'era. Ogni messaggio che non è nostro
//! prosegue com'era arrivato.
//!
//! Tre messaggi contano:
//!
//! - **`TaskbarButtonCreated`**, che la barra manda ogni volta che crea l'icona
//!   della finestra: alla prima comparsa, quando la finestra torna dal vassoio,
//!   quando Esplora risorse si riavvia. Ogni volta l'icona è nuova e i pulsanti
//!   vanno rimessi — con l'ultimo aspetto ricordato, non con quello di
//!   partenza.
//! - **`WM_COMMAND` con `THBN_CLICKED`**: un pulsante premuto.
//! - **il nostro messaggio di aggiornamento**, che [`Barra::mostra`] manda da
//!   qualunque filo perché sia il filo della finestra a parlare con la barra.
//!
//! Tutto il lavoro con la barra avviene sul filo della finestra: l'oggetto COM
//! vive lì, le icone nascono e muoiono lì, e i clic arrivano lì.
//!
//! # Cosa vive quanto
//!
//! Il [`Contesto`] nasce in [`aggancia`], diventa un puntatore consegnato a
//! Windows insieme alla sottoclasse, e muore in `WM_NCDESTROY`, che è l'ultimo
//! messaggio che una finestra riceve. Le icone vivono il tempo di una
//! consegna: la barra se ne fa una copia, e le nostre si distruggono subito
//! dopo.

#![allow(
    unsafe_code,
    reason = "COM e la sottoclasse della finestra non hanno un'interfaccia sicura: \
              ogni blocco dice perché è corretto"
)]

use std::cell::Cell;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, ITaskbarList3, RemoveWindowSubclass, SetWindowSubclass,
    THB_FLAGS, THB_ICON, THB_TOOLTIP, THBF_DISABLED, THBF_ENABLED, THBN_CLICKED, THUMBBUTTON,
    TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, CreateIcon, DestroyIcon, GetWindowThreadProcessId, HICON,
    IsWindowVisible, MSGFLT_ALLOW, PostMessageW, RegisterWindowMessageW, SM_CXSMICON, WM_COMMAND,
    WM_DPICHANGED, WM_NCDESTROY, WM_SETTINGCHANGE, WM_SHOWWINDOW,
};
use windows::core::w;

use crate::Tasto;
use crate::aspetto::{self, Aspetto};
use crate::glifi;

/// L'identificativo della nostra sottoclasse sulla finestra.
///
/// Windows distingue le sottoclassi per procedura **e** identificativo: con la
/// stessa procedura, lo stesso numero vuol dire «la stessa». È quel che
/// [`aggancia`] guarda per non agganciarsi due volte.
const ID_SOTTOCLASSE: usize = 0xAE7E;

/// Il colore dei simboli sulla barra scura.
const SU_SCURO: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// Il colore dei simboli sulla barra chiara: quasi nero, come le icone di
/// sistema di Windows 11 in quel tema.
const SU_CHIARO: [u8; 3] = [0x1C, 0x1C, 0x1C];

/// Il lato più piccolo che si disegna, in pixel.
///
/// Sedici è il lato delle icone piccole a 100%; sotto non si scende nemmeno se
/// il sistema rispondesse qualcosa di strano.
const LATO_MINIMO: u32 = 16;

/// La metà dei pulsanti che si vede da fuori, e che si può chiamare da
/// qualunque filo.
pub(crate) struct Barra {
    condivisa: Arc<Condivisa>,
}

/// Quel che i due fili — chi chiama [`Barra::mostra`] e la finestra — hanno in
/// comune.
struct Condivisa {
    /// L'`HWND` della finestra, come intero: un puntatore grezzo non può
    /// passare da un filo all'altro, un numero sì. Si rifà `HWND` solo per
    /// consegnarlo a Windows.
    finestra: isize,
    /// Il nostro messaggio di aggiornamento, registrato in [`aggancia`].
    aggiorna: u32,
    /// L'ultimo aspetto chiesto.
    aspetto: Mutex<Aspetto>,
    /// La finestra esiste ancora. Si abbassa in `WM_NCDESTROY`: dopo, un
    /// `PostMessageW` potrebbe arrivare a un'altra finestra che ha ereditato
    /// lo stesso numero.
    viva: AtomicBool,
}

impl Barra {
    /// Vedi [`crate::Barra::mostra`].
    pub(crate) fn mostra(&self, aspetto: Aspetto) {
        {
            let mut dentro = self
                .condivisa
                .aspetto
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if *dentro == aspetto {
                return;
            }
            *dentro = aspetto;
        }
        if !self.condivisa.viva.load(Ordering::Acquire) {
            return;
        }
        // SAFETY: `PostMessageW` mette un messaggio in coda e torna; non legge
        // né scrive memoria nostra. Su una finestra che non c'è più fallisce e
        // basta, e `viva` evita il caso in cui il numero sia già passato a
        // un'altra.
        let _ = unsafe {
            PostMessageW(
                Some(hwnd_di(self.condivisa.finestra)),
                self.condivisa.aggiorna,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }
}

/// Quel che la procedura della finestra si porta dietro.
///
/// Solo riferimenti condivisi, mai `&mut`: la procedura può essere richiamata
/// mentre è già al lavoro — un messaggio mandato durante una chiamata al
/// sistema — e due `&mut` vivi alla stessa memoria non devono esistere nemmeno
/// per un istante. Quel che cambia sta in `Cell`, che si legge e si scrive
/// senza prestiti.
struct Contesto {
    condivisa: Arc<Condivisa>,
    su_tasto: Box<dyn Fn(Tasto) + Send + Sync>,
    diario: Box<dyn Fn(&str) + Send + Sync>,
    /// Il numero di `TaskbarButtonCreated`.
    creata: u32,
    /// La barra, presa al primo bisogno e tenuta fino all'icona successiva.
    barra: Cell<Option<ITaskbarList3>>,
    /// I pulsanti sono già stati aggiunti all'icona di adesso.
    aggiunti: Cell<bool>,
    /// La barra di Windows è chiara.
    chiara: Cell<bool>,
    /// L'ultima riga scritta nel diario, per non ripeterla.
    ultima_riga: Cell<Option<String>>,
}

impl Contesto {
    /// Un pulsante premuto: lo passa a chi ha agganciato.
    ///
    /// Dentro `catch_unwind` perché questa è una funzione chiamata da Windows:
    /// un panico che ne uscisse non avrebbe nessuno a raccoglierlo, e Rust
    /// termina il processo. Un clic che non fa niente è meglio di un lettore
    /// che si chiude.
    fn premi(&self, tasto: Tasto) {
        let _ = catch_unwind(AssertUnwindSafe(|| (self.su_tasto)(tasto)));
    }

    /// Scrive un guasto nel diario, se non è lo stesso di prima.
    ///
    /// Un rifiuto tende a ripetersi a ogni aggiornamento finché la causa non
    /// passa: una riga basta.
    fn annota(&self, riga: String) {
        let prima = self.ultima_riga.take();
        if prima.as_deref() != Some(riga.as_str()) {
            self.scrivi(&riga);
        }
        self.ultima_riga.set(Some(riga));
    }

    /// Scrive una riga nel diario, sempre, e dimentica l'ultimo guasto: se
    /// tornasse, merita di nuovo la sua riga.
    fn scrivi(&self, riga: &str) {
        self.ultima_riga.set(None);
        let _ = catch_unwind(AssertUnwindSafe(|| (self.diario)(riga)));
    }

    /// La barra nuova di zecca, o `None` se il sistema non la dà.
    fn nuova_barra(&self) -> Option<ITaskbarList3> {
        // SAFETY: una chiamata COM con argomenti costanti, su un filo dove COM
        // è inizializzato — vedi `aggancia`.
        let barra: ITaskbarList3 =
            match unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) } {
                Ok(barra) => barra,
                Err(err) => {
                    self.annota(format!(
                        "[barra] la barra delle applicazioni non risponde: {err}"
                    ));
                    return None;
                }
            };
        // SAFETY: `HrInit` è la prima chiamata che la documentazione chiede su
        // un oggetto appena creato, e non prende argomenti.
        if let Err(err) = unsafe { barra.HrInit() } {
            self.annota(format!(
                "[barra] la barra delle applicazioni non si apre: {err}"
            ));
            return None;
        }
        Some(barra)
    }

    /// Consegna i pulsanti, nell'ultimo aspetto chiesto.
    ///
    /// Se non sono ancora stati aggiunti a questa icona si aggiungono, sennò si
    /// aggiornano. Se l'aggiunta fallisce si prova l'aggiornamento: succede
    /// quando lo stesso `TaskbarButtonCreated` arriva due volte — quello vero e
    /// quello che [`aggancia`] si manda da sé a finestra già visibile.
    fn consegna(&self, finestra: HWND) {
        let aspetto = self
            .condivisa
            .aspetto
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(barra) = self.barra.take().or_else(|| self.nuova_barra()) else {
            return;
        };
        let lato = lato_delle_icone(finestra);
        let colore = if self.chiara.get() {
            SU_CHIARO
        } else {
            SU_SCURO
        };
        let pulsanti = aspetto::pulsanti(&aspetto);
        // Le icone vivono fino alla fine di questa funzione: la barra se ne fa
        // una copia, e le nostre si distruggono uscendo.
        let icone = pulsanti
            .each_ref()
            .map(|p| Icona::nuova(&glifi::disegna(p.glifo, lato, colore), lato));
        let mut per_windows = [THUMBBUTTON::default(); 3];
        for ((posto, pulsante), icona) in per_windows.iter_mut().zip(&pulsanti).zip(&icone) {
            let mut maschera = THB_TOOLTIP | THB_FLAGS;
            if icona.is_some() {
                maschera |= THB_ICON;
            }
            *posto = THUMBBUTTON {
                dwMask: maschera,
                iId: pulsante.id,
                iBitmap: 0,
                hIcon: icona.as_ref().map_or_else(HICON::default, |i| i.0),
                szTip: pulsante.suggerimento,
                dwFlags: if pulsante.attivo {
                    THBF_ENABLED
                } else {
                    THBF_DISABLED
                },
            };
        }

        let esito = if self.aggiunti.get() {
            // SAFETY: `finestra` è la finestra di questa sottoclasse, e la
            // fetta vive per tutta la chiamata; la barra copia icone e testi.
            unsafe { barra.ThumbBarUpdateButtons(finestra, &per_windows) }
        } else {
            // SAFETY: come sopra.
            match unsafe { barra.ThumbBarAddButtons(finestra, &per_windows) } {
                Ok(()) => {
                    // A ogni aggiunta e non una volta sola: succede alla prima
                    // comparsa, a ogni ritorno dal vassoio e a ogni riavvio di
                    // Esplora risorse, e il diario deve poterle contare.
                    self.scrivi(&format!(
                        "[barra] pulsanti nell'anteprima aggiunti lato={lato}"
                    ));
                    Ok(())
                }
                Err(err) => {
                    // SAFETY: come sopra.
                    let aggiornati = unsafe { barra.ThumbBarUpdateButtons(finestra, &per_windows) };
                    aggiornati.map_err(|_| err)
                }
            }
        };
        match esito {
            Ok(()) => self.aggiunti.set(true),
            Err(err) => self.annota(format!("[barra] pulsanti nell'anteprima rifiutati: {err}")),
        }
        self.barra.set(Some(barra));
    }
}

/// Un'icona nostra, distrutta quando cade.
struct Icona(HICON);

impl Icona {
    /// L'icona dai pixel di [`glifi::disegna`], o `None` se il sistema non la
    /// crea.
    fn nuova(bgra: &[u8], lato: u32) -> Option<Self> {
        let lato_i = i32::try_from(lato).ok()?;
        let righe = usize::try_from(lato).ok()?;
        if bgra.len() != righe.checked_mul(righe)?.checked_mul(4)? {
            return None;
        }
        // La maschera: un bit per pixel, righe allineate a due byte, tutta a
        // zero. Con un colore a 32 bit Windows guarda l'alfa e non la
        // maschera, ma la maschera deve esserci.
        let maschera = vec![0u8; righe.div_ceil(16) * 2 * righe];
        // SAFETY: i due buffer vivono per tutta la chiamata e hanno le misure
        // che `CreateIcon` ricava da lato, piani e bit per pixel: `lato²·4`
        // byte il colore (controllato qui sopra), una riga allineata a 16 bit
        // per riga la maschera. Windows li copia.
        unsafe {
            CreateIcon(
                None,
                lato_i,
                lato_i,
                1,
                32,
                maschera.as_ptr(),
                bgra.as_ptr(),
            )
        }
        .ok()
        .map(Self)
    }
}

impl Drop for Icona {
    fn drop(&mut self) {
        // SAFETY: l'icona l'ha creata `Icona::nuova` e nessun altro la
        // distrugge; la barra ne tiene una copia sua.
        let _ = unsafe { DestroyIcon(self.0) };
    }
}

/// Vedi [`crate::aggancia`].
pub(crate) fn aggancia(
    hwnd: *mut c_void,
    su_tasto: Box<dyn Fn(Tasto) + Send + Sync>,
    diario: Box<dyn Fn(&str) + Send + Sync>,
) -> Result<Barra, String> {
    let finestra = HWND(hwnd);
    // SAFETY: due letture; su un `HWND` che non è una finestra la prima
    // risponde zero.
    let (proprietario, io) = unsafe {
        (
            GetWindowThreadProcessId(finestra, None),
            GetCurrentThreadId(),
        )
    };
    if proprietario == 0 {
        return Err("la finestra non esiste".to_owned());
    }
    if proprietario != io {
        return Err("non si è sul filo della finestra".to_owned());
    }
    // SAFETY: legge soltanto; `None` come destinazione del dato.
    if unsafe { GetWindowSubclass(finestra, Some(sottoclasse), ID_SOTTOCLASSE, None) }.as_bool() {
        return Err("la finestra è già agganciata".to_owned());
    }

    // SAFETY: due nomi costanti, terminati da zero.
    let (creata, aggiorna) = unsafe {
        (
            RegisterWindowMessageW(w!("TaskbarButtonCreated")),
            RegisterWindowMessageW(w!("Rymembra.Barra.Aggiorna")),
        )
    };
    if creata == 0 || aggiorna == 0 {
        return Err("Windows non registra i messaggi della barra".to_owned());
    }
    // Se Rymembra girasse come amministratore, Windows scarterebbe i messaggi
    // che la barra — un processo normale — manda alla finestra: la creazione
    // dell'icona e i clic. Due permessi esplicitamente per quei due.
    //
    // SAFETY: `finestra` è nostra (controllato sopra), il resto sono costanti.
    unsafe {
        let _ = ChangeWindowMessageFilterEx(finestra, creata, MSGFLT_ALLOW, None);
        let _ = ChangeWindowMessageFilterEx(finestra, WM_COMMAND, MSGFLT_ALLOW, None);
    }
    // La barra è un oggetto COM, e COM su questo filo c'è già: la finestra lo
    // ha inizializzato per il trascinamento. Chiederlo di nuovo con lo stesso
    // modello costa un contatore; con un modello diverso risponde di no e non
    // cambia niente. Non si chiude mai: il filo è quello della finestra, e
    // vive quanto il programma.
    //
    // SAFETY: nessun puntatore, una costante.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    let condivisa = Arc::new(Condivisa {
        finestra: hwnd as isize,
        aggiorna,
        aspetto: Mutex::new(Aspetto::default()),
        viva: AtomicBool::new(true),
    });
    let contesto = Box::new(Contesto {
        condivisa: Arc::clone(&condivisa),
        su_tasto,
        diario,
        creata,
        barra: Cell::new(None),
        aggiunti: Cell::new(false),
        chiara: Cell::new(barra_chiara()),
        ultima_riga: Cell::new(None),
    });
    let dati = Box::into_raw(contesto);
    // SAFETY: si è sul filo della finestra (controllato sopra), come
    // `SetWindowSubclass` pretende. `dati` resta valido finché la sottoclasse
    // c'è: lo libera soltanto `sottoclasse`, in `WM_NCDESTROY`, dopo averla
    // tolta.
    let agganciata =
        unsafe { SetWindowSubclass(finestra, Some(sottoclasse), ID_SOTTOCLASSE, dati as usize) };
    if !agganciata.as_bool() {
        // SAFETY: la sottoclasse non c'è, quindi nessuno ha ricevuto il
        // puntatore: torna a essere soltanto nostro.
        drop(unsafe { Box::from_raw(dati) });
        return Err("Windows non accetta la sottoclasse della finestra".to_owned());
    }

    // A finestra già visibile l'icona nella barra c'è già, e il suo
    // `TaskbarButtonCreated` è passato prima che ascoltassimo: lo si manda da
    // sé, una volta.
    //
    // SAFETY: una lettura e un messaggio in coda alla nostra finestra.
    unsafe {
        if IsWindowVisible(finestra).as_bool() {
            let _ = PostMessageW(Some(finestra), creata, WPARAM(0), LPARAM(0));
        }
    }
    Ok(Barra { condivisa })
}

/// La procedura che Windows chiama per ogni messaggio della finestra, prima di
/// quella di `tao`.
///
/// # Safety
///
/// La chiama soltanto Windows, per la sottoclasse installata in [`aggancia`]:
/// `dati` è il `Box<Contesto>` consegnato lì, vivo fino a `WM_NCDESTROY`.
unsafe extern "system" fn sottoclasse(
    finestra: HWND,
    messaggio: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    dati: usize,
) -> LRESULT {
    let puntatore = dati as *const Contesto;

    if messaggio == WM_NCDESTROY {
        // SAFETY: `puntatore` è il `Box<Contesto>` di `aggancia`, ancora vivo:
        // lo si libera tre righe più giù.
        unsafe { &*puntatore }
            .condivisa
            .viva
            .store(false, Ordering::Release);
        // SAFETY: la stessa finestra, procedura e identificativo installati.
        // Dopo questa riga Windows non ci chiama più con questo puntatore,
        // che torna a essere soltanto nostro, e nessun riferimento al
        // contesto sopravvive a questo ramo.
        unsafe {
            let _ = RemoveWindowSubclass(finestra, Some(sottoclasse), ID_SOTTOCLASSE);
            drop(Box::from_raw(puntatore.cast_mut()));
        }
        // SAFETY: lo schema documentato: tolta la sottoclasse, il messaggio
        // prosegue verso la procedura della finestra.
        return unsafe { DefSubclassProc(finestra, messaggio, wparam, lparam) };
    }

    // SAFETY: come sopra, il contesto è vivo fino a `WM_NCDESTROY`, che qui
    // non è; e se ne prende un riferimento condiviso, mai esclusivo — vedi
    // [`Contesto`].
    let contesto = unsafe { &*puntatore };

    if messaggio == contesto.creata {
        // Un'icona nuova: la barra di prima non vale più per lei, e i pulsanti
        // vanno aggiunti di nuovo.
        contesto.barra.set(None);
        contesto.aggiunti.set(false);
        contesto.consegna(finestra);
    } else if messaggio == contesto.condivisa.aggiorna {
        // Prima che i pulsanti ci siano non si consegna niente: lo farà
        // `TaskbarButtonCreated`, con l'aspetto che intanto è stato ricordato.
        if contesto.aggiunti.get() {
            contesto.consegna(finestra);
        }
        return LRESULT(0);
    } else if messaggio == WM_COMMAND {
        let (alta, bassa) = parole(wparam);
        if alta == THBN_CLICKED
            && let Some(tasto) = aspetto::tasto_di(bassa)
        {
            contesto.premi(tasto);
            return LRESULT(0);
        }
    } else if messaggio == WM_SHOWWINDOW && wparam.0 == 0 {
        // La finestra si nasconde — nel vassoio — e la sua icona sparisce con
        // i pulsanti. Fino al prossimo `TaskbarButtonCreated` non c'è niente
        // da aggiornare.
        contesto.aggiunti.set(false);
    } else if messaggio == WM_SETTINGCHANGE {
        // Il tema della barra può essere cambiato: si rilegge, e si
        // ridisegna solo se è cambiato davvero.
        let chiara = barra_chiara();
        if chiara != contesto.chiara.replace(chiara) && contesto.aggiunti.get() {
            contesto.consegna(finestra);
        }
    } else if messaggio == WM_DPICHANGED && contesto.aggiunti.get() {
        // Un'altra scala: le icone vanno ridisegnate al lato nuovo.
        contesto.consegna(finestra);
    }

    // SAFETY: ogni messaggio prosegue verso la procedura di `tao`, con gli
    // argomenti ricevuti.
    unsafe { DefSubclassProc(finestra, messaggio, wparam, lparam) }
}

/// La parola alta e la parola bassa di un `WPARAM`.
fn parole(wparam: WPARAM) -> (u32, u32) {
    let alta = u32::try_from((wparam.0 >> 16) & 0xFFFF).unwrap_or(0);
    let bassa = u32::try_from(wparam.0 & 0xFFFF).unwrap_or(0);
    (alta, bassa)
}

/// Il lato delle icone piccole alla scala della finestra.
fn lato_delle_icone(finestra: HWND) -> u32 {
    // SAFETY: due letture senza puntatori; su una finestra sparita la prima
    // risponde zero, e qualunque risposta strana della seconda la raccoglie il
    // `max` qui sotto.
    let lato = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(finestra)) };
    u32::try_from(lato).unwrap_or(0).max(LATO_MINIMO)
}

/// La barra di Windows è nel tema chiaro.
///
/// È la preferenza «Scegli la modalità predefinita di Windows» di
/// Personalizzazione → Colori, che governa barra e anteprime. Se non si legge,
/// si risponde scuro: era l'unico tema della barra fino a Windows 10, e i
/// simboli bianchi sono quelli che si vedono nel caso più comune.
fn barra_chiara() -> bool {
    let mut valore: u32 = 0;
    let mut misura = u32::try_from(std::mem::size_of::<u32>()).unwrap_or(4);
    // SAFETY: due puntatori a variabili locali che vivono oltre la chiamata,
    // con la misura giusta del dato che `RRF_RT_REG_DWORD` può scrivere.
    let esito = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut valore).cast()),
            Some(&raw mut misura),
        )
    };
    esito == ERROR_SUCCESS && valore != 0
}

/// Da intero a `HWND`.
const fn hwnd_di(numero: isize) -> HWND {
    HWND(numero as *mut c_void)
}
