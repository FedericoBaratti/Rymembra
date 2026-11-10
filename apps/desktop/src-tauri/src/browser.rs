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
/// Solo una pagina `https://`, e nella forma normalizzata: vedi
/// [`indirizzo_web`].
///
/// # Errori
///
/// Il motivo, in testo, se l'indirizzo non è una pagina web cifrata o se il
/// sistema rifiuta: chi chiama lo mette come causa dell'errore del catalogo
/// che gli compete.
pub fn apri(indirizzo: &str) -> Result<(), String> {
    let indirizzo = indirizzo_web(indirizzo)?;
    let indirizzo = indirizzo.as_str();
    #[cfg(target_os = "android")]
    {
        rymembra_android::sistema::apri_indirizzo(indirizzo).map_err(|err| err.to_string())
    }
    #[cfg(not(target_os = "android"))]
    {
        tauri_plugin_opener::open_url(indirizzo, None::<&str>).map_err(|err| err.to_string())
    }
}

/// L'indirizzo come lo si consegna al sistema: una pagina `https://` con un
/// ospite e senza credenziali, nella forma che il parser ha normalizzato.
///
/// # Perché il controllo sta qui, e non in chi chiama
///
/// Perché sul computer quel che passa di qui va a `ShellExecuteExW` così
/// com'è, e lì non contano solo le pagine web: `file:`, un percorso UNC (che
/// apre una sessione SMB verso un host esterno e gli consegna l'hash della
/// password di Windows), i protocolli di sistema come `search-ms:` e
/// `ms-settings:`, quelli registrati da altre app. Oggi ogni chiamante compone
/// un `https://` da costanti o da identificativi, ma due di loro aprono un
/// indirizzo del catalogo, e il giorno in cui un catalogo lo consegnasse tale e
/// quale questo sarebbe l'unico punto a difenderlo. Su Android `Browser.kt`
/// tiene già la stessa regola; adesso la funzione ha una garanzia sola su tutte
/// e due le piattaforme. Tutti i chiamanti di oggi aprono pagine cifrate:
/// `http:` non serve a nessuno, e non passa.
///
/// # Perché la stringa si guarda prima del parser
///
/// Perché il parser degli URL perdona: toglie in silenzio tabulazioni e a capo,
/// e per `https` legge una barra rovesciata come una barra. Quel che si
/// controllerebbe dopo non sarebbe più quel che è arrivato. Poi si apre la
/// forma normalizzata, e non la stringa ricevuta: il controllo e l'apertura
/// guardano la stessa cosa.
///
/// Il motivo del rifiuto non ripete l'indirizzo, che può portare un gettone
/// (il consenso di Last.fm) e finisce nel diario come causa.
fn indirizzo_web(indirizzo: &str) -> Result<String, String> {
    if indirizzo.chars().any(|c| c.is_control() || c == '\\') {
        return Err(
            "l'indirizzo contiene caratteri di controllo o barre rovesciate: non si apre"
                .to_owned(),
        );
    }
    let url = tauri::Url::parse(indirizzo.trim())
        .map_err(|err| format!("non è un indirizzo web completo: {err}"))?;
    if url.scheme() != "https" {
        return Err(format!(
            "si aprono solo pagine https, non «{}:»",
            url.scheme()
        ));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("l'indirizzo non ha un ospite".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("l'indirizzo porta delle credenziali: non si apre".to_owned());
    }
    Ok(url.into())
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

#[cfg(test)]
mod prove {
    use super::*;

    /// Al sistema arriva solo una pagina web cifrata, e nella forma
    /// normalizzata. Su Windows ogni altra stringa andava a `ShellExecuteExW`
    /// così com'era: `file:`, un percorso UNC (una sessione SMB verso un host
    /// esterno, con l'hash della password), `search-ms:`, `ms-settings:` e i
    /// protocolli registrati da altre app.
    #[test]
    fn solo_indirizzi_web() {
        for buono in [
            "https://archive.org/details/gd77",
            "https://audius.co/deadmau5/strobe-123",
            "https://www.qobuz.com/it-it/search?q=a%20b",
            "https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade",
            "https://www.last.fm/api/auth/?api_key=k&token=t",
        ] {
            assert_eq!(
                indirizzo_web(buono).as_deref(),
                Ok(buono),
                "rifiutato un indirizzo buono: {buono}"
            );
        }
        for cattivo in [
            // Non web, o non cifrati.
            "http://archive.org/details/gd77",
            "file:///C:/Windows/System32/calc.exe",
            "file://evil.example/share/x",
            "search-ms:query=x&crumb=location:\\\\evil.example\\share",
            "ms-settings:privacy",
            "javascript:alert(1)",
            "mailto:a@b.c",
            // Senza schema: per `ShellExecuteExW` è un percorso o un programma.
            "archive.org/details/gd77",
            "calc.exe",
            // UNC, e barre rovesciate che il parser trasformerebbe in barre.
            "\\\\evil.example\\share\\x.archive.org",
            "https:\\\\evil.example\\share",
            "https://archive.org\\details\\x",
            // Host vuoto, credenziali nell'indirizzo.
            "https://",
            "https://:443/details",
            "https://utente:segreto@archive.org/details/x",
            "https://utente@archive.org/details/x",
            // Caratteri di controllo, che il parser toglierebbe in silenzio.
            "https://archive.org/details/x\nfile:///C:/",
            "https://arch\tive.org/details/x",
            "https://archive.org/details/x\u{0}",
            // Vuota.
            "",
        ] {
            assert!(
                indirizzo_web(cattivo).is_err(),
                "lasciato passare al sistema: {cattivo:?}"
            );
        }
    }

    /// Si apre la forma normalizzata, non la stringa ricevuta: quella che il
    /// controllo ha guardato è quella che arriva al sistema.
    #[test]
    fn si_apre_la_forma_normalizzata() {
        assert_eq!(
            indirizzo_web("  https://ARCHIVE.org/details/gd77").as_deref(),
            Ok("https://archive.org/details/gd77")
        );
    }
}
