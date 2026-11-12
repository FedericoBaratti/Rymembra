//! La pagina della finestra: che cosa può caricare, che cosa si rifà quando se
//! ne carica una nuova, e quando disegna.
//!
//! Un plugin senza comandi, registrato in `lib.rs` accanto agli altri. Plugin e
//! non gestori sul costruttore della finestra perché la finestra nasce da
//! `tauri.conf.json`, senza un costruttore a cui appendere niente, e perché su
//! Android l'attività che rinasce ricostruisce la webview con la stessa
//! etichetta (`telefono::riapri_la_finestra`): Tauri chiede ai plugin per ogni
//! webview, quindi una regola scritta qui vale per tutte e due, senza doverla
//! ricordare in due posti.
//!
//! # La navigazione (decisione D-44)
//!
//! La finestra non mostra mai pagine web: tutto ciò che non è l'app si rifiuta.
//! Senza un gestore Tauri chiede ai plugin, e i quattro che ci sono rispondono
//! di sì a tutto: un `location.href`, un `<a href>` cliccato, un
//! `<meta http-equiv=refresh>` o — sul telefono — un `window.open` portavano la
//! cornice principale su qualunque origine. La finestra non ha decorazioni,
//! quindi una pagina remota la riempiva tutta, barra del titolo compresa: un'esca
//! a schermo intero dentro la cornice di un programma fidato, da chiudere con
//! Alt+F4. Sul telefono in più quella pagina riceveva gli script iniziali di
//! Tauri, iniettati per ogni origine. Oggi nessun elemento della pagina naviga;
//! questa è la regola che lo rende vero anche domani.
//!
//! Rifiutate anche le origini dei protocolli nostri (`rymembra-cover`,
//! `rymembra-skin`, `ipc`): servono risorse alla pagina, non pagine alla cornice,
//! e Tauri le conta come «locali», cioè con l'IPC. E `about:blank`: una cornice
//! vuota è una finestra senza app e senza modo di tornarci.
//!
//! Il rifiuto non apre niente nel browser di sistema: i link che Rymembra apre li
//! apre `comandi::apri_documento`, che sa quali sono, e trasformare ogni
//! navigazione rifiutata in un'apertura nel browser darebbe a uno script nella
//! pagina il modo di aprire qualunque indirizzo.
//!
//! # Una pagina nuova (R13-4)
//!
//! Tre cose del nucleo le accende la pagina e le spegne la pagina, con la
//! pulizia di un effetto di React: il contatore dei pannelli del testo, la presa
//! dello spettro, il QR del collegamento. Una pagina che si ricarica — «Ricarica»
//! della schermata di guasto — non esegue le pulizie, e quelle cose restavano
//! accese fino al riavvio: il testo del brano dopo chiesto a LRCLIB col pannello
//! chiuso, contro la promessa di `PRIVACY.md`; trenta trasformate al secondo per
//! uno spettro che nessuno guarda; un QR valido fuori dallo schermo. Qui si
//! spengono all'inizio di ogni pagina, **prima** dei suoi script: quella nuova
//! riaccende da sé quel che le serve, montando i suoi pannelli. Alla prima pagina
//! è tutto già spento, e spegnerlo di nuovo non fa niente.
//!
//! # A finestra ridotta (R21-3)
//!
//! Una finestra ridotta a icona resta «visible» per la pagina: WebView2 è una
//! finestra figlia, e i messaggi della riduzione arrivano alla madre. Il browser
//! quindi non rallentava niente, e con la musica che suona la pagina continuava
//! a disegnare — venti aggiornamenti al secondo, e con «In riproduzione» aperta
//! lo spettro, al 40 % di un processore. Microsoft chiede proprio questo: alla
//! riduzione `IsVisible = false` sul controller, e `true` al ripristino. Tauri lo
//! espone come visibilità della webview. La pagina diventa `hidden`, e timer,
//! `requestAnimationFrame` e animazioni si fermano da sé, qualunque sia la loro
//! origine e senza ricordarlo in ogni componente. Solo sul computer: sul
//! telefono l'attività in secondo piano lo fa già il sistema.

#[cfg(desktop)]
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::plugin::{Builder, TauriPlugin};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager as _, Url, Wry};

/// L'etichetta della finestra dell'app, quella di `tauri.conf.json`.
const FINESTRA: &str = "main";

/// Il guardiano della pagina, da registrare con `.plugin(…)`.
pub fn plugin() -> TauriPlugin<Wry> {
    let costruttore = Builder::new("pagina")
        .on_navigation(|webview, url| {
            let sviluppo = tauri::is_dev()
                .then(|| webview.config().build.dev_url.clone())
                .flatten();
            let ammessa = navigazione_ammessa(url, sviluppo.as_ref());
            if !ammessa {
                // L'origine e basta: un indirizzo intero può portare nel
                // percorso o nella domanda quel che nel diario non va.
                crate::nota!(
                    "[finestra] navigazione rifiutata verso {}://{}",
                    url.scheme(),
                    url.host_str().unwrap_or("—")
                );
            }
            ammessa
        })
        .on_page_load(|webview, carico| {
            if webview.label() == FINESTRA && carico.event() == PageLoadEvent::Started {
                pagina_nuova(webview.app_handle());
            }
        });
    #[cfg(desktop)]
    let costruttore = costruttore.on_event(|app, evento| {
        if let tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Resized(misura),
            ..
        } = evento
            && label == FINESTRA
        {
            regola_il_disegno(app, si_vede(misura.width, misura.height));
        }
    });
    costruttore.build()
}

/// La cornice principale può andare qui? Solo se è l'app.
///
/// L'app è `http(s)://tauri.localhost`, su Windows e su Android, senza porta e
/// senza credenziali nell'indirizzo; `tauri://localhost` sulle altre
/// piattaforme di Tauri. In sviluppo anche l'origine del server di Vite
/// (`sviluppo`, il `devUrl` della configurazione). Tutto il resto no: vedi la
/// documentazione del modulo.
fn navigazione_ammessa(url: &Url, sviluppo: Option<&Url>) -> bool {
    let nuda = url.username().is_empty() && url.password().is_none();
    let dell_app = match url.scheme() {
        "http" | "https" => url.host_str() == Some("tauri.localhost") && url.port().is_none(),
        "tauri" => url.host_str() == Some("localhost") && url.port().is_none(),
        _ => false,
    };
    let di_sviluppo = sviluppo.is_some_and(|base| base.origin() == url.origin());
    nuda && (dell_app || di_sviluppo)
}

/// Spegne quel che la pagina di prima aveva acceso e non ha potuto spegnere.
///
/// Gira all'inizio della pagina, sul filo della finestra e prima che la pagina
/// esegua i suoi script: lo spettro in particolare va spento **prima** che la
/// pagina nuova lo riaccenda, e lo stesso filo dà l'ordine. `try_state` perché
/// la prima pagina può arrivare prima del `setup`, che registra gli stati.
fn pagina_nuova(app: &AppHandle) {
    if let Some(testi) = app.try_state::<crate::testi::StatoTesti>() {
        testi.pagina_nuova();
    }
    if let Some(lettore) = app.try_state::<crate::riproduzione::StatoLettore>() {
        // Un motore che non c'è non ha prese da chiudere: l'errore non dice
        // niente di più.
        let _ = crate::riproduzione::spettro(lettore, false);
    }
    crate::lontano::finestra_nascosta(app);
}

/// La pagina è nascosta perché la finestra è ridotta: l'ultima cosa detta a
/// WebView2, perché `Resized` arriva a ogni ridimensionamento e la visibilità si
/// cambia solo quando cambia.
#[cfg(desktop)]
static NASCOSTA: AtomicBool = AtomicBool::new(false);

/// La finestra si vede, con questa misura? Ridotta a icona, `tao` manda
/// `Resized` con l'area client a zero.
#[cfg(desktop)]
const fn si_vede(larghezza: u32, altezza: u32) -> bool {
    larghezza > 0 && altezza > 0
}

/// Quel che va detto alla webview, dato com'era: `Some(true)` mostrala,
/// `Some(false)` nascondila, `None` niente, è già così.
#[cfg(desktop)]
const fn cambio(nascosta: bool, si_vede: bool) -> Option<bool> {
    if nascosta == si_vede {
        Some(si_vede)
    } else {
        None
    }
}

/// Nasconde la pagina a finestra ridotta, e la rimostra al ripristino.
#[cfg(desktop)]
fn regola_il_disegno(app: &AppHandle, si_vede: bool) {
    let Some(mostra) = cambio(NASCOSTA.load(Ordering::Acquire), si_vede) else {
        return;
    };
    let Some(finestra) = app.get_webview_window(FINESTRA) else {
        return;
    };
    let pagina: &tauri::Webview = finestra.as_ref();
    let esito = if mostra {
        // Il fuoco torna alla pagina: una finestra figlia nascosta lo perde, e
        // al ripristino la tastiera parlerebbe alla madre — lo Spazio non
        // metterebbe in pausa finché non si clicca dentro.
        pagina.show().and_then(|()| pagina.set_focus())
    } else {
        pagina.hide()
    };
    match esito {
        Ok(()) => NASCOSTA.store(!mostra, Ordering::Release),
        Err(err) => crate::nota!(
            "[finestra] la pagina non si è {}: {err}",
            if mostra { "rimostrata" } else { "nascosta" }
        ),
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn url(testo: &str) -> Url {
        Url::parse(testo).expect("un indirizzo valido")
    }

    /// D-44: solo la pagina dell'app si naviga. La funzione non c'era, e tutto
    /// passava.
    #[test]
    fn solo_la_pagina_dell_app_si_naviga() {
        for sì in [
            "http://tauri.localhost/",
            "http://tauri.localhost/index.html?x=1#y",
            "https://tauri.localhost/",
            "tauri://localhost/",
        ] {
            assert!(navigazione_ammessa(&url(sì), None), "{sì}");
        }
        for no in [
            "https://example.com/",
            "http://example.com/tauri.localhost",
            "http://tauri.localhost.evil.com/",
            "http://tauri.localhost@evil.com/",
            "http://utente@tauri.localhost/",
            "http://tauri.localhost:8080/",
            "http://localhost:5173/",
            "file:///C:/",
            "about:blank",
            "data:text/html,ciao",
            "http://rymembra-skin.localhost/bozza/x",
            "http://rymembra-cover.localhost/x",
            "http://ipc.localhost/comando",
            "rymembra-skin://localhost/x",
            "javascript:alert(1)",
        ] {
            assert!(!navigazione_ammessa(&url(no), None), "{no}");
        }
    }

    /// In sviluppo la pagina la serve Vite: la sua origine, e non «localhost»
    /// in generale.
    #[test]
    fn in_sviluppo_si_naviga_anche_il_server_di_vite() {
        let vite = url("http://localhost:5173");
        assert!(navigazione_ammessa(
            &url("http://localhost:5173/"),
            Some(&vite)
        ));
        assert!(navigazione_ammessa(
            &url("http://localhost:5173/src/x.ts?t=1"),
            Some(&vite)
        ));
        assert!(navigazione_ammessa(
            &url("http://tauri.localhost/"),
            Some(&vite)
        ));
        assert!(!navigazione_ammessa(
            &url("http://localhost:5174/"),
            Some(&vite)
        ));
        assert!(!navigazione_ammessa(
            &url("https://localhost:5173/"),
            Some(&vite)
        ));
        assert!(!navigazione_ammessa(
            &url("https://example.com/"),
            Some(&vite)
        ));
    }

    /// R21-3: ridotta, la pagina si nasconde; al ripristino torna; e a ogni
    /// altro ridimensionamento non si dice niente.
    #[cfg(desktop)]
    #[test]
    fn a_finestra_ridotta_la_pagina_non_disegna() {
        assert!(!si_vede(0, 0), "ridotta a icona");
        assert!(si_vede(1280, 820));
        // Visibile e ridotta: si nasconde.
        assert_eq!(cambio(false, si_vede(0, 0)), Some(false));
        // Nascosta e ripristinata: si mostra.
        assert_eq!(cambio(true, si_vede(1280, 820)), Some(true));
        // Visibile e ridimensionata, nascosta e ancora ridotta: niente.
        assert_eq!(cambio(false, si_vede(1000, 700)), None);
        assert_eq!(cambio(true, si_vede(0, 0)), None);
    }

    /// Il cablaggio: il guardiano risponde con [`navigazione_ammessa`], a ogni
    /// pagina che comincia spegne le tre cose che la pagina tiene accese, e
    /// regola il disegno a ogni ridimensionamento della finestra.
    #[test]
    fn il_guardiano_e_cablato() {
        let sorgente = include_str!("pagina.rs");
        let corpo = |firma: &str| {
            sorgente
                .split(firma)
                .nth(1)
                .and_then(|resto| resto.split("\n}\n").next())
                .unwrap_or_default()
        };
        let costruttore = corpo("pub fn plugin()");
        assert!(costruttore.contains(".on_navigation("));
        assert!(costruttore.contains("navigazione_ammessa(url"));
        assert!(costruttore.contains("PageLoadEvent::Started"));
        assert!(costruttore.contains("pagina_nuova("));
        assert!(costruttore.contains("WindowEvent::Resized"));
        assert!(costruttore.contains("regola_il_disegno("));
        let nuova = corpo("fn pagina_nuova(");
        for pulizia in [
            "testi.pagina_nuova()",
            "riproduzione::spettro(lettore, false)",
            "lontano::finestra_nascosta(app)",
        ] {
            assert!(nuova.contains(pulizia), "la pagina nuova non fa {pulizia}");
        }
    }
}
