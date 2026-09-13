//! Lo zoom della finestra: `Ctrl++`, `Ctrl+-`, `Ctrl+0`.
//!
//! # Perché lo zoom della WebView e non una scala nel foglio
//!
//! Perché le due cose non sono la stessa vista da due parti: cambiano lo
//! spazio in cui il codice della pagina misura.
//!
//! `WebviewWindow::set_zoom` è lo zoom del browser. Il pixel CSS resta l'unità
//! in cui la pagina ragiona, e a moltiplicare ci pensa il compositore: quel che
//! cambia è **quanti** pixel CSS ci stanno nella finestra, esattamente come se
//! qualcuno l'avesse stretta. Nessuna riga di TypeScript se ne accorge, ed è
//! questo il punto.
//!
//! La proprietà CSS `zoom` sulla radice, o una scala in un token del foglio,
//! metterebbero invece la pagina a cavallo di due spazi di coordinate. In
//! questa release ci sono due moduli che **misurano il DOM**, e sarebbero i
//! primi a caderci:
//!
//! - `apps/desktop/src/virtuale.ts` calcola la cima dell'elenco con
//!   `rettPrima.top - rettScorrevole.top + scorrevole.scrollTop`, e l'altezza
//!   di riga con `getBoundingClientRect().height`. In Chromium — cioè in
//!   WebView2 — un rettangolo di `getBoundingClientRect` porta lo zoom dentro,
//!   mentre `scrollTop` e `clientHeight` restano nello spazio locale
//!   dell'elemento. Sono le due metà di quella somma: sotto `zoom` una è
//!   moltiplicata e l'altra no, la `cima` esce sbagliata di un fattore, e
//!   l'elenco disegna righe che non sono quelle sotto il dito. Quale delle due
//!   sia la moltiplicata non cambia la conclusione: basta che stiano in due
//!   spazi diversi perché la somma non voglia dire più niente. E non è un
//!   difetto che si vede subito: si vede scorrendo, e a zoom 1 non si vede mai.
//! - `apps/desktop/src/Giro.tsx` posiziona il buco dei riflettori su un
//!   rettangolo misurato e lo disegna con `position: fixed`. Dentro un
//!   sottoalbero con `zoom`, anche un elemento `fixed` è zoomato: il rettangolo
//!   già moltiplicato verrebbe moltiplicato una seconda volta, e il buco
//!   finirebbe accanto all'elemento invece che sopra.
//!
//! Con lo zoom della WebView nessuno dei due se ne accorge, perché non c'è
//! niente di cui accorgersi: continuano a lavorare in pixel CSS, e i pixel CSS
//! sono diventati più grandi senza cambiare nome.
//!
//! # Perché **non** serve un permesso in `capabilities/`
//!
//! Perché il comando è nostro. `core:webview:allow-set-webview-zoom` esiste, e
//! sarebbe il permesso da aggiungere **se** fosse la pagina a chiamare
//! `getCurrentWebview().setZoom()`, cioè il comando del plugin. Qui la pagina
//! chiama `zoom_passo`, che è un `#[tauri::command]` di questa applicazione, e
//! i comandi dell'applicazione non passano dalle capacità: le capacità
//! governano i comandi dei plugin, `core:` compreso.
//!
//! È la stessa disciplina — e la stessa forma — di due cose che in `main.rs`
//! stanno già scritte per esteso: l'updater, che ha il plugin ma **non** ha
//! `updater:*` in `capabilities/default.json` perché tutto passa da
//! `crate::aggiornamenti`; e i quattro gesti della barra del titolo —
//! trascinare, ridurre, ingrandire, chiudere — che sono comandi nostri e non
//! `core:window:*` perché quell'elenco aperto darebbe alla pagina anche
//! `set_position`, `set_size` e `set_fullscreen`. Aprire
//! `core:webview` per lo zoom darebbe alla pagina anche
//! `clear_all_browsing_data` e `print`. L'elenco resta chiuso, e questo modulo
//! è la ragione per cui può restarci.
//!
//! # Perché la preferenza non si annuncia alla nuvola
//!
//! Gli altri comandi di preferenza passano da `nuvola::se_riuscito`, che sveglia
//! il backup e la sincronia. Questi tre no, per due ragioni che vanno insieme.
//! La prima: il documento della sincronia porta ascolti, voti e preferiti, non
//! le impostazioni — non ci sarebbe niente da spingere. La seconda: `Ctrl++` si
//! preme a raffica, tre o quattro volte di fila per arrivare al gradino che si
//! vuole, e ognuna di quelle pressioni sveglierebbe il backup per un numero che
//! su un altro schermo non vuol dire niente. La riga viaggerà col prossimo
//! backup che sveglia qualcos'altro; se non lo sveglia nessuno, quel che si
//! perde ripristinando è un gradino di zoom, che si rimette con due tasti.
//!
//! # Il fotogramma alla misura sbagliata, e perché non c'è
//!
//! Lo zoom della WebView non sopravvive alla chiusura: va riapplicato a ogni
//! avvio, e a riapplicarlo è [`zoom_avvio`] su chiamata della finestra. Il
//! fotogramma alla misura di prima che ci si aspetterebbe non si vede, perché
//! la finestra nasce nascosta (`visible: false` in `tauri.conf.json`) e a
//! mostrarla è il frontend con `pronto`, dopo aver applicato skin, tema e —
//! adesso — zoom. È lo stesso rimedio del fotogramma scuro e del fotogramma in
//! italiano, e non ne serve un terzo.

use tauri::State;

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Applica un gradino alla finestra, e lo restituisce.
///
/// Applicare qui invece di lasciarlo fare alla pagina non è una scelta di
/// stile: `set_zoom` è una chiamata sulla finestra, e farla dal TypeScript
/// vorrebbe dire il permesso `core:webview` di cui il preambolo dice perché non
/// si apre.
///
/// Un `set_zoom` che fallisce non è un errore da mostrare — vorrebbe dire un
/// avviso rosso per annunciare che l'interfaccia è rimasta della misura di
/// prima — ma il numero restituito resta quello **in tabella**, non quello
/// sullo schermo: al riavvio successivo si riproverà, e nel frattempo le
/// impostazioni non devono raccontare una misura diversa da quella che si
/// ricorda.
fn applica(finestra: &tauri::WebviewWindow, fattore: f64) -> f64 {
    let _ = finestra.set_zoom(fattore);
    fattore
}

/// Il gradino ricordato, già applicato alla finestra.
///
/// La finestra lo chiede una volta all'apertura. Legge **e** applica in un giro
/// solo: sono due facce dello stesso fatto — «di quanto è ingrandita questa
/// finestra» — e separarle vorrebbe dire un secondo giro di IPC per un numero
/// che di qua è già in mano.
///
/// # Errori
///
/// `db.queryFailed`.
#[tauri::command]
pub fn zoom_avvio(finestra: tauri::WebviewWindow, stato: State<'_, Stato>) -> Esito<f64> {
    let fattore = con_libreria(&stato, |libreria| {
        aether_app::preferenze::zoom(&libreria.connection)
    })
    .map_err(errore)?;
    Ok(applica(&finestra, fattore))
}

/// Un gradino in su o in giù, applicato e ricordato.
///
/// La scala sta nel nucleo (`aether_app::preferenze::SCALA_ZOOM`), non qui e
/// nemmeno nella pagina: è la stessa regola per cui una preferenza si convalida
/// dove si scrive. Da qui passa un **verso**, non un numero, quindi non esiste
/// nessun modo di chiedere un fattore fuori scala: la finestra non ne conosce
/// nemmeno uno.
///
/// Ai due estremi non succede niente, e non è un errore: vedi
/// `aether_app::preferenze::zoom_al_gradino` sul perché la scala non gira.
///
/// # Errori
///
/// `db.queryFailed`.
#[tauri::command]
pub fn zoom_passo(finestra: tauri::WebviewWindow, stato: State<'_, Stato>, su: bool) -> Esito<f64> {
    let fattore = con_libreria(&stato, |libreria| {
        let adesso = aether_app::preferenze::zoom(&libreria.connection)?;
        aether_app::preferenze::imposta_zoom(
            &libreria.connection,
            aether_app::preferenze::zoom_al_gradino(adesso, su),
        )
    })
    .map_err(errore)?;
    Ok(applica(&finestra, fattore))
}

/// La misura di serie, applicata e ricordata: `Ctrl+0`.
///
/// Nessun argomento, e non è una comodità: **nessun numero attraversa questa
/// interfaccia**. Da di là arrivano tre gesti — uno in su, uno in giù, torna
/// al vero — e la scala resta l'unica cosa che sa quali numeri esistono. Un
/// comando che prendesse un `f64` obbligherebbe la pagina a conoscere almeno
/// un gradino per poterlo mandare, e da lì a conoscerli tutti è un passo.
///
/// # Perché questo comando esiste anche se una scorciatoia lo fa già
///
/// Perché le tre scorciatoie si possono **togliere** dalla scheda che le
/// elenca — un elenco vuoto vuol dire «nessuna», ed è una scelta che
/// `tastiera.ts` rispetta apposta. Senza un comando visibile, chi togliesse
/// quella dello zoom normale dopo aver ingrandito resterebbe a 2,0 senza
/// nessun modo di tornare indietro. È la sola preferenza dell'applicazione
/// che, sbagliata, rende più difficile correggere sé stessa.
///
/// # Errori
///
/// `db.queryFailed`.
#[tauri::command]
pub fn zoom_normale(finestra: tauri::WebviewWindow, stato: State<'_, Stato>) -> Esito<f64> {
    let scritto = con_libreria(&stato, |libreria| {
        aether_app::preferenze::imposta_zoom(
            &libreria.connection,
            aether_app::preferenze::ZOOM_NEUTRO,
        )
    })
    .map_err(errore)?;
    Ok(applica(&finestra, scritto))
}
