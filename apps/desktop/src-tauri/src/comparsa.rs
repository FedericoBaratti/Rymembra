//! Come compare la finestra, e come ricompare.
//!
//! # Il motore non ha bisogno della finestra
//!
//! Con il vassoio acceso la X **chiude** la finestra — la distrugge, con la sua
//! WebView2 — e il programma resta: la musica, la libreria, i fili di
//! sottofondo, le scorciatoie multimediali e l'icona nell'area di notifica sono
//! del nucleo, e nessuno di loro sta dentro la pagina. È il risparmio che il
//! secondo piano prometteva e non dava: una finestra solo nascosta teneva la
//! pagina viva, con i suoi processi `msedgewebview2` e la loro memoria. Vedi
//! `lib.rs` (`RunEvent::ExitRequested`) per come il programma sopravvive
//! all'ultima finestra.
//!
//! Quel che serve, allora, è un modo di **ricrearla**: la ricrea questo
//! modulo, dalla stessa configurazione da cui Tauri l'ha creata la prima volta
//! (`tauri.conf.json`), e la pagina riparte da capo e chiede al nucleo come
//! stanno le cose, come all'avvio. La chiamano:
//!
//! - «Mostra» del vassoio, e un clic sull'icona ([`crate::vassoio::mostra`]);
//! - un secondo avvio di Rymembra ([`secondo_avvio`]), con o senza link;
//! - un evento che vuole una persona davanti, come un invito di «Ascolta
//!   insieme» (`insieme::filo`).
//!
//! # L'avvio automatico: ridotta a icona, una volta
//!
//! Lanciata da Windows al login (`--avvio-automatico`, o l'attività di avvio del
//! Store) la finestra non deve piombare davanti a chi sta ancora aprendo il
//! suo lavoro: compare **ridotta a icona nella barra**. Ma solo la **prima**
//! volta: una finestra ricreata dal vassoio la vuole chi ha cliccato, e la
//! vuole davanti. Il «una volta» lo tiene [`PrimaComparsa`], e lo prende il
//! primo che mostra la finestra fra i tre che lo fanno — la pagina quando è
//! pronta (`pronto`), l'attesa di una migrazione (`finestra_mostra`) e la rete
//! di sicurezza dei due secondi — senza che si coordinino.
//!
//! # Quel che una finestra nuova deve riavere
//!
//! Non è solo la pagina. Ogni finestra ha le sue cose che si agganciano
//! all'`HWND`: la guardia dell'Alt da solo, i pulsanti nell'anteprima della
//! barra, il conto di `pagina::NASCOSTA`, la rete di sicurezza che la mostra
//! comunque. Le ridà tutte [`finestra_nata`], e **sta qui** e non nel `setup`
//! perché il `setup` gira una volta, e la finestra può nascere più d'una.
//! Le SMTC non ci sono: stanno su una finestra nativa del nucleo, e sopravvivono
//! (vedi `media::smtc`).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager as _;

use crate::argomenti::{Argomenti, SecondoAvvio, secondo_avvio as decisione};
use crate::{nota, spegnimento, vassoio};

/// L'etichetta della finestra, quella di `tauri.conf.json`.
const ETICHETTA: &str = "main";

/// Se la prossima comparsa della finestra dev'essere ridotta a icona, e vale una
/// volta sola.
#[derive(Debug, Default)]
pub struct PrimaComparsa(AtomicBool);

impl PrimaComparsa {
    /// Nessuna richiesta: la finestra compare normale.
    #[must_use]
    pub const fn nuova() -> Self {
        Self(AtomicBool::new(false))
    }

    /// Da qui in poi la prossima comparsa è ridotta a icona, o torna normale.
    pub fn imposta(&self, ridotta: bool) {
        self.0.store(ridotta, Ordering::Release);
    }

    /// La prossima comparsa dev'essere ridotta? Si dice **sì una volta sola**:
    /// chi lo prende lo consuma, e gli altri mostrano la finestra come sempre.
    pub fn prendi(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

/// La comparsa di questo avvio.
static COMPARSA: PrimaComparsa = PrimaComparsa::nuova();

/// Gli argomenti con cui questo processo è partito.
static AVVIO: OnceLock<Argomenti> = OnceLock::new();

/// Registra come è partito il programma. Una volta, all'inizio di `run`.
///
/// `dal_login_store` è la risposta dell'attivazione del pacchetto: il Store non
/// passa argomenti, e l'avvio al login si riconosce da lì
/// (`rymembra_barra::avviata_dal_login_store`).
pub fn registra_l_avvio(argomenti: Argomenti, dal_login_store: bool) {
    COMPARSA.imposta(argomenti.automatico || dal_login_store);
    if argomenti.automatico || dal_login_store {
        nota!("[avvio] partito con Windows: la finestra compare ridotta a icona");
    }
    let _ = AVVIO.set(argomenti);
}

/// Qualcuno ha chiesto la finestra davanti: la comparsa ridotta dell'avvio
/// automatico non vale più.
pub fn annulla_la_riduzione() {
    COMPARSA.imposta(false);
}

/// Gli argomenti dell'avvio, o nessuno se non sono stati registrati.
pub fn avvio() -> &'static Argomenti {
    static NESSUNO: Argomenti = Argomenti {
        automatico: false,
        link: None,
    };
    AVVIO.get().unwrap_or(&NESSUNO)
}

/// Mostra la finestra: ridotta a icona se è la comparsa dell'avvio automatico,
/// normale e davanti in ogni altro caso.
///
/// # L'ordine, e perché non basta fidarsi
///
/// Ridotta **prima** e mostrata dopo: nell'altro ordine la finestra appare a
/// misura piena e si riduce con l'animazione di Windows, che a chi sta
/// arrivando al desktop è un lampo di troppo. Ma l'effetto di `minimize` su una
/// finestra che non si vede ancora è quello di `tao` — che la mostra, la riduce
/// e la nasconde di nuovo — e non si può provare qui. Per questo si controlla
/// com'è finita: se la finestra non si vede la si mostra, e se si vede e non è
/// ridotta la si riduce adesso. **Il guasto che non si può permettere** è una
/// finestra nascosta senza un'icona a cui tornare; un lampo, sì.
pub fn mostra(finestra: &tauri::WebviewWindow) {
    if !COMPARSA.prendi() {
        let _ = finestra.show();
        return;
    }
    let _ = finestra.minimize();
    let _ = finestra.show();
    if !finestra.is_visible().unwrap_or(true) {
        let _ = finestra.show();
    }
    if !finestra.is_minimized().unwrap_or(true) {
        let _ = finestra.minimize();
    }
}

/// Se una ricreazione è già in coda: due richieste vicine — un clic
/// sull'icona e un secondo avvio — non devono fare due finestre.
static RICREANDO: AtomicBool = AtomicBool::new(false);

/// Se la finestra non c'è, la ricrea dalla configurazione.
///
/// Non fa niente se si sta uscendo (toccare l'`AppHandle` durante la
/// demolizione è il modo in cui questa applicazione moriva: vedi
/// `spegnimento`), né se la finestra c'è già. La costruzione si fa **sul filo
/// della finestra**: da chi chiama può non essere quello, e creare una WebView2
/// altrove vuol dire aspettare lui a bloccare chi chiama.
pub fn ricrea_se_manca(app: &tauri::AppHandle) {
    if spegnimento::in_uscita() || app.get_webview_window(ETICHETTA).is_some() {
        return;
    }
    if RICREANDO.swap(true, Ordering::AcqRel) {
        return;
    }
    let mano = app.clone();
    let spedita = app.run_on_main_thread(move || {
        crea(&mano);
        RICREANDO.store(false, Ordering::Release);
    });
    if let Err(err) = spedita {
        RICREANDO.store(false, Ordering::Release);
        nota!("[finestra] il filo della finestra non risponde, la finestra non rinasce: {err}");
    }
}

/// Costruisce la finestra dalla configurazione. Sul filo della finestra.
fn crea(app: &tauri::AppHandle) {
    if spegnimento::in_uscita() || app.get_webview_window(ETICHETTA).is_some() {
        return;
    }
    let Some(configurazione) = app.config().app.windows.first() else {
        nota!("[finestra] la configurazione non ha una finestra: non rinasce");
        return;
    };
    let costruita = tauri::WebviewWindowBuilder::from_config(app, configurazione)
        .and_then(tauri::WebviewWindowBuilder::build);
    match costruita {
        Ok(_) => {
            nota!("[finestra] il vassoio la riapre: la finestra rinasce dalla configurazione");
            finestra_nata(app);
        }
        Err(err) => nota!("[finestra] la finestra non rinasce: {err}"),
    }
}

/// Quel che una finestra **nata dopo l'avvio** deve riavere. Vedi il preambolo.
fn finestra_nata(app: &tauri::AppHandle) {
    crate::pagina::finestra_nuova();
    // La guardia dell'Alt e i pulsanti nell'anteprima si appendono all'`HWND`, e
    // quello vecchio è morto con la finestra.
    crate::barra::ferma_alt(app);
    crate::barra::avvia(app);
    // E la rete di sicurezza: una pagina che non arriva a dire `pronto` non deve
    // lasciare una finestra nuova nascosta per sempre.
    crate::rete_di_sicurezza(app);
}

/// Un secondo processo è partito mentre questo c'è già: cosa fare.
///
/// `argomenti` è la riga di comando del secondo, col nome dell'eseguibile in
/// testa. La decisione è di [`crate::argomenti::secondo_avvio`] e si prova lì;
/// qui si esegue.
pub fn secondo_avvio(app: &tauri::AppHandle, argomenti: &[String]) {
    let letti = Argomenti::da(argomenti.iter().skip(1));
    match decisione(&letti) {
        SecondoAvvio::Ignora => {
            nota!("[avvio] un secondo avvio automatico: la finestra resta com'è");
        }
        SecondoAvvio::Mostra => vassoio::mostra(app),
        SecondoAvvio::ConLink(link) => {
            // Il link passa dalla stessa conferma di sempre — vedi
            // `argomenti` e `lontano::link_arrivato` — e la finestra, se manca,
            // nasce per mostrargliela: la pagina prende il link rimasto in
            // attesa appena si monta.
            crate::lontano::link_arrivato(app, &link);
            vassoio::mostra(app);
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_comparsa_ridotta_si_consuma_alla_prima_finestra_mostrata() {
        let comparsa = PrimaComparsa::nuova();
        // Senza richiesta, ogni comparsa è normale.
        assert!(!comparsa.prendi());
        // Con la richiesta, il primo che mostra la finestra la riduce...
        comparsa.imposta(true);
        assert!(comparsa.prendi());
        // ...e gli altri due — `pronto`, `finestra_mostra`, la rete di
        // sicurezza — la mostrano com'è: una finestra ricreata dal vassoio la
        // vuole chi ha cliccato, e la vuole davanti.
        assert!(!comparsa.prendi());
        assert!(!comparsa.prendi());
    }

    /// Le tre strade che mostrano la finestra passano tutte da [`mostra`]: se
    /// una facesse `show()` da sé, l'avvio automatico la alzerebbe a metà.
    #[test]
    fn le_tre_comparse_passano_dalla_stessa_funzione() {
        let lib = include_str!("lib.rs");
        let corpo = |firma: &str| {
            lib.split(firma)
                .nth(1)
                .and_then(|resto| resto.split("\n}\n").next())
                .unwrap_or_default()
                .to_owned()
        };
        for (firma, nome) in [
            ("fn pronto(", "pronto"),
            ("fn finestra_mostra(", "finestra_mostra"),
            ("fn rete_di_sicurezza(", "rete_di_sicurezza"),
        ] {
            let testo = corpo(firma);
            assert!(!testo.is_empty(), "{nome} non si trova");
            assert!(
                testo.contains("comparsa::mostra("),
                "{nome} non passa da comparsa::mostra"
            );
        }
    }

    /// Una finestra nata dopo l'avvio deve riavere tutto quel che l'avvio le dava.
    #[test]
    fn una_finestra_nuova_riprende_guardia_pulsanti_e_rete_di_sicurezza() {
        let sorgente = include_str!("comparsa.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let corpo = sorgente
            .split("fn finestra_nata(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        for chiamata in [
            "pagina::finestra_nuova()",
            "barra::ferma_alt(app)",
            "barra::avvia(app)",
            "rete_di_sicurezza(app)",
        ] {
            assert!(corpo.contains(chiamata), "manca {chiamata}");
        }
    }

    /// Il secondo avvio con link e quello senza finestra passano per la stessa
    /// strada che alza o ricrea la finestra.
    #[test]
    fn il_secondo_avvio_alza_o_ricrea_la_finestra_dal_vassoio() {
        let sorgente = include_str!("comparsa.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let corpo = sorgente
            .split("pub fn secondo_avvio(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert_eq!(corpo.matches("vassoio::mostra(app)").count(), 2);
        assert!(corpo.contains("lontano::link_arrivato(app, &link)"));
    }
}
