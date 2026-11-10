//! Il browser di sistema, per un indirizzo web.
//!
//! Un posto solo da cui aprirlo, perché le strade sono due. Sul computer
//! `tauri-plugin-opener`, che lancia il programma predefinito. Su Android il
//! ponte ([`rymembra_android::sistema::apri_indirizzo`]), che lo consegna al
//! sistema con un `Intent`: un'app Android un programma non lo lancia, e
//! `open_url` lì rispondeva `Permission denied` a ogni link — le pagine del
//! progetto, le donazioni e il consenso di ogni servizio da collegare.

/// Apre `indirizzo` nel browser.
///
/// # Errori
///
/// Il motivo, in testo, se il sistema rifiuta: chi chiama lo mette come causa
/// dell'errore del catalogo che gli compete.
pub fn apri(indirizzo: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        rymembra_android::sistema::apri_indirizzo(indirizzo).map_err(|err| err.to_string())
    }
    #[cfg(not(target_os = "android"))]
    {
        tauri_plugin_opener::open_url(indirizzo, None::<&str>).map_err(|err| err.to_string())
    }
}

/// Mentre si aspetta nel browser il consenso di un servizio da collegare.
///
/// Su Android accende `ConsensoInAttesa`, un servizio in primo piano breve:
/// il consenso torna a un indirizzo di questo telefono (`127.0.0.1`), e con
/// Rymembra dietro al browser il sistema può congelarne il processo, che
/// allora non sente il ritorno. Si spegne quando la guardia cade: al ritorno,
/// all'«Annulla», alla scadenza, a un errore. Sul computer non fa niente.
#[must_use = "il servizio resta acceso solo finché c'è la guardia"]
pub fn in_attesa_del_consenso() -> AttesaDelConsenso {
    #[cfg(target_os = "android")]
    if let Err(err) = rymembra_android::sistema::attesa_consenso(true) {
        crate::nota!("[consenso] il servizio in primo piano non si accende: {err}");
    }
    AttesaDelConsenso
}

/// Vedi [`in_attesa_del_consenso`].
#[derive(Debug)]
pub struct AttesaDelConsenso;

impl Drop for AttesaDelConsenso {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        if let Err(err) = rymembra_android::sistema::attesa_consenso(false) {
            crate::nota!("[consenso] il servizio in primo piano non si spegne: {err}");
        }
    }
}
