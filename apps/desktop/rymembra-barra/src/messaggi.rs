//! La finestra dei messaggi del nucleo: dove si appendono le SMTC.
//!
//! # Perché esiste
//!
//! I tasti multimediali e la scheda del volume (le `SystemMediaTransportControls`)
//! su Windows si chiedono **a una finestra**: `GetForWindow(hwnd)`. Fino a qui
//! la finestra era quella dell'applicazione, e con lei morivano: con la X sul
//! vassoio la finestra si chiude davvero — è il motore che resta, e la musica
//! con lui — e i tasti si sarebbero spenti proprio mentre l'utente si aspetta
//! di poter ancora mettere in pausa dalla tastiera. Così le SMTC stanno su una
//! finestra **nostra**, nativa e mai mostrata, che vive quanto il motore e non
//! sa niente della WebView.
//!
//! # Com'è fatta, e perché così
//!
//! La forma è quella che l'autore di `souvlaki` mostra nel suo esempio per i
//! programmi senza finestra (`examples/print_events.rs`, `DummyWindow`): una
//! classe con la procedura di serie (`DefWindowProcW`), una finestra di misura
//! zero, **senza genitore** e **mai mostrata**. Non è una finestra «solo
//! messaggi» (`HWND_MESSAGE`): che il sistema accetti quella per le SMTC non
//! l'ha scritto nessuna fonte che si possa citare, e l'unica cosa che non si
//! può provare qui è proprio questa. La forma dell'esempio è quella che
//! qualcuno ha visto funzionare. Una finestra mai mostrata non compare nella
//! barra delle applicazioni né in Alt+Tab, e non ha nessun disegno: la sua vita
//! è l'`HWND`.
//!
//! # Chi la pompa
//!
//! Le finestre vivono sul filo che le ha create, e un filo che non pompa i
//! messaggi le lascia «non risponde». [`FinestraDeiMessaggi::crea`] va chiamata
//! sul filo della finestra dell'applicazione — quello di `tao`, che pompa
//! sempre, anche quando l'ultima finestra si è chiusa —, e lì la finestra
//! dei messaggi non fa niente di suo: tutto è `DefWindowProcW`.
//!
//! # Lo smontaggio
//!
//! Chi la tiene (`media::smtc`) la dichiara **dopo** i controlli: i campi di una
//! struttura cadono nell'ordine in cui sono scritti, e il `Drop` dei controlli
//! chiama `detach` su questa finestra, che dev'essere ancora lì. Il `Drop` della
//! finestra poi la distrugge — se è sul suo filo, perché `DestroyWindow` non
//! funziona da un altro; altrimenti le chiede di chiudersi (`WM_CLOSE`, e la
//! procedura di serie la distrugge sul suo filo), e se quel filo sta morendo
//! la finestra se ne va con lui.

#![allow(
    unsafe_code,
    reason = "la classe e la finestra non hanno un'interfaccia sicura: ogni blocco \
              dice perché è corretto"
)]

use std::ffi::c_void;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PostMessageW, RegisterClassExW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WNDCLASSEXW,
};
use windows::core::w;

/// Una finestra nativa mai mostrata, per appenderci le SMTC. Vedi il preambolo.
///
/// `Send` e `Sync`: è un numero e il filo che l'ha creata. Chi la lascia cadere
/// da un altro filo non la distrugge — la chiude, vedi il `Drop`.
#[derive(Debug)]
pub struct FinestraDeiMessaggi {
    /// L'`HWND`, come intero: un puntatore grezzo non passa da un filo all'altro.
    finestra: isize,
    /// Il filo che l'ha creata, e che sola può distruggerla.
    filo: u32,
}

impl FinestraDeiMessaggi {
    /// Crea la finestra, sul filo che chiama. Questo filo deve pompare i
    /// messaggi finché la finestra vive: vedi il preambolo.
    ///
    /// # Errors
    ///
    /// Una frase che dice cosa ha risposto Windows: la classe non si registra,
    /// o la finestra non nasce.
    pub fn crea() -> Result<Self, String> {
        // SAFETY: nessuna precondizione. Con `None` risponde il modulo del
        // processo, cioè l'eseguibile.
        let istanza = unsafe { GetModuleHandleW(None) }
            .map(|modulo| HINSTANCE(modulo.0))
            .map_err(|err| format!("il modulo del processo non si trova: {err}"))?;
        let Ok(dimensione) = u32::try_from(std::mem::size_of::<WNDCLASSEXW>()) else {
            return Err("la classe della finestra ha una misura che non si dice".to_owned());
        };
        let classe = WNDCLASSEXW {
            cbSize: dimensione,
            lpfnWndProc: Some(procedura),
            hInstance: istanza,
            lpszClassName: w!("Rymembra.Messaggi"),
            ..WNDCLASSEXW::default()
        };
        // SAFETY: `classe` è una struttura nostra, viva per tutta la chiamata,
        // con la misura giusta; il nome è una costante terminata da zero e la
        // procedura una funzione che vive quanto il programma. Windows copia
        // la struttura e non la conserva. Registrarla due volte risponde
        // «esiste già», che sotto non è un guasto.
        let registrata = unsafe { RegisterClassExW(&raw const classe) };
        // SAFETY: legge l'ultimo errore di questo filo, subito dopo la chiamata
        // che l'ha scritto.
        if registrata == 0 && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS {
            return Err("Windows non registra la classe della finestra dei messaggi".to_owned());
        }
        // SAFETY: il nome della classe è la costante registrata qui sopra, e il
        // titolo una costante vuota, tutte e due terminate da zero. Nessun
        // genitore, nessun menù, nessun parametro: misura zero, nessun
        // `WS_VISIBLE` e nessuna chiamata a `ShowWindow`, quindi non si
        // mostra mai. L'istanza è quella del processo.
        let finestra = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("Rymembra.Messaggi"),
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(istanza),
                None,
            )
        }
        .map_err(|err| format!("la finestra dei messaggi non nasce: {err}"))?;
        // SAFETY: nessuna precondizione.
        let filo = unsafe { GetCurrentThreadId() };
        Ok(Self {
            finestra: finestra.0 as isize,
            filo,
        })
    }

    /// L'`HWND`, come lo vuole `souvlaki` (`PlatformConfig::hwnd`).
    #[must_use]
    pub const fn hwnd(&self) -> *mut c_void {
        self.finestra as *mut c_void
    }
}

impl Drop for FinestraDeiMessaggi {
    fn drop(&mut self) {
        let finestra = HWND(self.finestra as *mut c_void);
        // SAFETY: nessuna precondizione.
        let qui = unsafe { GetCurrentThreadId() };
        if qui == self.filo {
            // SAFETY: la finestra l'ha creata questo filo (appena controllato) e
            // nessun altro la distrugge: è l'unica copia dell'`HWND` che
            // questa struttura lascia in giro.
            let _ = unsafe { DestroyWindow(finestra) };
        } else {
            // SAFETY: `PostMessageW` mette un messaggio in coda e torna; non
            // legge né scrive memoria nostra. Su una finestra che non c'è più
            // fallisce e basta.
            let _ = unsafe { PostMessageW(Some(finestra), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
    }
}

/// La procedura della finestra: quella di serie, per ogni messaggio.
///
/// `DefWindowProcW` risponde a `WM_CLOSE` con `DestroyWindow`, ed è ciò che fa
/// morire la finestra chiusa da un altro filo.
///
/// # Safety
///
/// La chiama soltanto Windows, con gli argomenti di un messaggio vero della
/// finestra di questa classe.
unsafe extern "system" fn procedura(
    finestra: HWND,
    messaggio: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: gli stessi argomenti che Windows ha dato a questa procedura,
    // passati alla procedura di serie com'è documentato.
    unsafe { DefWindowProcW(finestra, messaggio, wparam, lparam) }
}

#[cfg(test)]
mod prove {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindowVisible};

    use super::*;

    /// Il filo a cui appartiene la finestra, o zero se non c'è più.
    fn filo_di(finestra: *mut c_void) -> u32 {
        // SAFETY: due letture; su un `HWND` che non è una finestra risponde zero.
        unsafe { GetWindowThreadProcessId(HWND(finestra), None) }
    }

    #[test]
    fn la_finestra_nasce_nascosta_e_muore_con_chi_la_tiene() {
        let finestra = FinestraDeiMessaggi::crea().expect("una finestra");
        let hwnd = finestra.hwnd();
        assert!(!hwnd.is_null());
        // Un filo suo e uno che la possiede: questo.
        // SAFETY: nessuna precondizione.
        assert_eq!(filo_di(hwnd), unsafe { GetCurrentThreadId() });
        // Mai mostrata: non c'è per chi guarda.
        // SAFETY: una lettura su una finestra viva.
        assert!(!unsafe { IsWindowVisible(HWND(hwnd)) }.as_bool());
        drop(finestra);
        assert_eq!(filo_di(hwnd), 0, "la finestra è rimasta dopo il Drop");
    }

    #[test]
    fn due_finestre_dei_messaggi_non_si_pestano_i_piedi() {
        // La classe si registra una volta: la seconda `crea` la trova già.
        let prima = FinestraDeiMessaggi::crea().expect("la prima");
        let seconda = FinestraDeiMessaggi::crea().expect("la seconda");
        assert_ne!(prima.hwnd(), seconda.hwnd());
    }

    /// Lasciarla cadere da un altro filo non può distruggerla (`DestroyWindow`
    /// non funziona su una finestra altrui) e non deve cadere: le chiede di
    /// chiudersi, e se il filo non pompa se ne va quando il filo finisce.
    #[test]
    fn da_un_altro_filo_non_cade_e_non_la_distrugge_subito() {
        let (invia, ricevi) = std::sync::mpsc::channel();
        let (fine, aspetta) = std::sync::mpsc::channel::<()>();
        let proprietario = std::thread::spawn(move || {
            let finestra = FinestraDeiMessaggi::crea().expect("una finestra");
            let hwnd = finestra.hwnd() as isize;
            invia.send(finestra).expect("la consegna");
            // Il filo resta vivo finché l'altro non ha finito.
            let _ = aspetta.recv();
            hwnd
        });
        let finestra = ricevi.recv().expect("la finestra");
        let hwnd = finestra.hwnd();
        // Da questo filo, che non è il suo.
        drop(finestra);
        assert_ne!(
            filo_di(hwnd),
            0,
            "una finestra altrui non si distrugge da qui: si chiude sul suo filo"
        );
        fine.send(()).expect("la fine");
        proprietario.join().expect("il filo");
    }
}
