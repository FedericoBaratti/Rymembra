//! Maiusc tenuto mentre Rymembra si apre: l'uscita d'emergenza dalle skin
//! (D-48, F10-2), dal lato del nucleo.
//!
//! # Il contratto
//!
//! Chi apre Rymembra tenendo Maiusc, e lo tiene finché vede la finestra, vuole
//! la skin di serie: una skin può rendere la finestra inservibile in modi che
//! l'esame non vede, e da dentro non si riesce più a toglierla. La regola —
//! quanto tenuto, entro quanto dalla comparsa — sta in `rymembra-barra`
//! ([`rymembra_barra::Ascolto`]), pura e provata; la lettura del tasto è
//! `GetAsyncKeyState`, l'unica chiamata `unsafe` che serve, nello stesso crate.
//!
//! # Perché il nucleo e non la pagina
//!
//! La prima versione ascoltava i `keydown` della pagina per cinque secondi da
//! quando la skin arrivava. La finestra però compare **prima** — dopo due secondi
//! comunque (`rete_di_sicurezza`), e subito se la libreria si apre in ritardo —
//! e un `keydown` arriva alla pagina solo se è caricata e ha il fuoco: chi teneva
//! Maiusc fino alla comparsa e poi lo lasciava, non scattava. Il nucleo sa
//! quando la finestra si vede e può chiedere al sistema lo stato del tasto
//! qualunque finestra abbia il fuoco. Vedi `rymembra-barra/src/maiusc.rs`.
//!
//! # La domanda della pagina
//!
//! L'ascolto parte con l'applicazione, su un filo suo che finisce da solo, e
//! lascia un esito. La pagina lo chiede dopo aver indossato la skin: `inCorso`
//! (richiedi fra poco), `tenuto` o `no`. **Un `tenuto` si dà una volta sola**:
//! una pagina che si ricarica non deve rimettere Plain a chi ha già rimesso la
//! sua skin con «Rimettila».
//!
//! Sul telefono non c'è un Maiusc da tenere: l'ascolto non parte e la risposta
//! è sempre `no`.

use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(any(windows, test))]
use std::time::Duration;

use serde::Serialize;

/// Non ancora deciso.
const IN_CORSO: u8 = 0;
/// Tenuto, e la pagina non l'ha ancora saputo.
const TENUTO: u8 = 1;
/// No, o già detto alla pagina.
const NO: u8 = 2;

/// L'esito dell'ascolto: uno solo per processo.
static ESITO: AtomicU8 = AtomicU8::new(IN_CORSO);

/// Quanto si aspetta una finestra che non compare. Una migrazione lunga la
/// mostra comunque (`finestra_mostra`), e oltre dieci minuti non c'è più un
/// «avvio» da proteggere.
#[cfg(any(windows, test))]
const ATTESA_MASSIMA: Duration = Duration::from_secs(600);

/// Quel che la pagina sa dell'ascolto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatoMaiusc {
    /// Ancora in corso: la finestra non si vede da cinque secondi.
    InCorso,
    /// Maiusc era tenuto. Si dice una volta sola.
    Tenuto,
    /// No.
    No,
}

/// Fa partire l'ascolto. Si chiama una volta, all'inizio del `setup`: la
/// pressione che conta può cominciare prima della finestra.
pub fn avvia(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        use tauri::Manager as _;

        let app = app.clone();
        let partito = std::thread::Builder::new()
            .name("rymembra-maiusc".to_owned())
            .spawn(move || {
                let inizio = std::time::Instant::now();
                ascolta(
                    &ESITO,
                    || inizio.elapsed(),
                    rymembra_barra::maiusc_giu,
                    || {
                        app.get_webview_window("main")
                            .and_then(|finestra| finestra.is_visible().ok())
                            .unwrap_or(false)
                    },
                    std::thread::sleep,
                    crate::spegnimento::in_uscita,
                );
            });
        if let Err(err) = partito {
            // Un guasto qui non deve fermare l'avvio: senza ascolto la
            // risposta è «no», e lo si dice.
            ESITO.store(NO, Ordering::Release);
            crate::nota!("[maiusc] l'ascolto non parte: {err}");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        ESITO.store(NO, Ordering::Release);
    }
}

/// Il ciclo dell'ascolto, con tutto quel che tocca il mondo passato da fuori:
/// il tempo, il tasto, la finestra, l'attesa, l'uscita. Così si prova senza
/// aspettare e senza una tastiera.
///
/// Gira finché non c'è un esito, finché l'applicazione non esce o finché la
/// finestra non compare nemmeno dopo [`ATTESA_MASSIMA`].
#[cfg(any(windows, test))]
fn ascolta(
    esito: &AtomicU8,
    ora: impl Fn() -> Duration,
    maiusc_giu: impl Fn() -> bool,
    visibile: impl Fn() -> bool,
    attendi: impl Fn(Duration),
    in_uscita: impl Fn() -> bool,
) {
    use rymembra_barra::{Ascolto, Esito, PASSO};

    let mut ascolto = Ascolto::nuovo();
    loop {
        let adesso = ora();
        match ascolto.campiona(adesso, maiusc_giu(), visibile()) {
            Esito::Tenuto => {
                crate::nota!("[maiusc] tenuto all'avvio: la finestra riparte con la skin di serie");
                esito.store(TENUTO, Ordering::Release);
                return;
            }
            Esito::No => {
                esito.store(NO, Ordering::Release);
                return;
            }
            Esito::InCorso => {}
        }
        if in_uscita() {
            return;
        }
        if adesso > ATTESA_MASSIMA {
            crate::nota!(
                "[maiusc] la finestra non si è vista in dieci minuti: niente da ascoltare"
            );
            esito.store(NO, Ordering::Release);
            return;
        }
        attendi(PASSO);
    }
}

/// Quel che si risponde alla pagina, e il segno che l'ha saputo.
fn leggi_e_consuma(esito: &AtomicU8) -> StatoMaiusc {
    // `Tenuto` si dà una volta sola: chi lo riceve lo fa diventare `No`.
    if esito
        .compare_exchange(TENUTO, NO, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        return StatoMaiusc::Tenuto;
    }
    match esito.load(Ordering::Acquire) {
        IN_CORSO => StatoMaiusc::InCorso,
        _ => StatoMaiusc::No,
    }
}

/// Maiusc era tenuto mentre l'applicazione si apriva? La pagina lo chiede dopo
/// aver indossato la skin, e richiede finché risponde `inCorso`.
#[tauri::command]
pub fn maiusc_all_avvio() -> StatoMaiusc {
    leggi_e_consuma(&ESITO)
}

#[cfg(test)]
mod prove {
    use std::cell::Cell;

    use super::*;

    /// Un orologio finto, che avanza solo quando l'ascolto aspetta, e una scena
    /// in cui Maiusc è giù in `giu` e la finestra si vede da `comparsa`.
    fn gira(giu: Option<(u64, u64)>, comparsa: u64, uscita_a: Option<u64>) -> u8 {
        let t = Cell::new(0_u64);
        let esito = AtomicU8::new(IN_CORSO);
        ascolta(
            &esito,
            || Duration::from_millis(t.get()),
            || giu.is_some_and(|(da, a)| t.get() >= da && t.get() < a),
            || t.get() >= comparsa,
            |passo| t.set(t.get() + u64::try_from(passo.as_millis()).unwrap_or(40)),
            || uscita_a.is_some_and(|quando| t.get() >= quando),
        );
        esito.load(Ordering::Acquire)
    }

    /// F10-2: Maiusc tenuto dall'avvio e rilasciato a +1 s, +3 s, +5 s dalla
    /// comparsa della finestra scatta; il ciclo lascia l'esito per la pagina.
    #[test]
    fn un_maiusc_rilasciato_dopo_la_comparsa_lascia_un_tenuto() {
        for rilascio in [1_000, 3_000, 5_000] {
            assert_eq!(
                gira(Some((100, 2_000 + rilascio)), 2_000, None),
                TENUTO,
                "rilasciato a +{rilascio} ms dalla comparsa"
            );
        }
    }

    #[test]
    fn senza_maiusc_l_esito_e_no_dopo_i_cinque_secondi() {
        assert_eq!(gira(None, 2_000, None), NO);
        // Una pressione breve non scatta.
        assert_eq!(gira(Some((2_500, 2_700)), 2_000, None), NO);
    }

    /// Chi esce non lascia un esito e non resta in attesa; la finestra che non
    /// compare per dieci minuti chiude l'ascolto con un no.
    #[test]
    fn l_uscita_e_la_finestra_che_non_compare_fermano_il_ciclo() {
        assert_eq!(gira(None, u64::MAX, Some(300)), IN_CORSO);
        assert_eq!(gira(None, u64::MAX, None), NO);
    }

    /// `tenuto` si dà una volta sola: una pagina che si ricarica riceve `no`.
    #[test]
    fn un_tenuto_si_dice_una_volta_sola() {
        let esito = AtomicU8::new(IN_CORSO);
        assert_eq!(leggi_e_consuma(&esito), StatoMaiusc::InCorso);
        esito.store(TENUTO, Ordering::Release);
        assert_eq!(leggi_e_consuma(&esito), StatoMaiusc::Tenuto);
        assert_eq!(leggi_e_consuma(&esito), StatoMaiusc::No);
        assert_eq!(leggi_e_consuma(&esito), StatoMaiusc::No);
        let esito = AtomicU8::new(NO);
        assert_eq!(leggi_e_consuma(&esito), StatoMaiusc::No);
    }

    /// Le parole che la pagina legge.
    #[test]
    fn le_tre_risposte_hanno_i_nomi_che_la_pagina_aspetta() {
        for (stato, nome) in [
            (StatoMaiusc::InCorso, "\"inCorso\""),
            (StatoMaiusc::Tenuto, "\"tenuto\""),
            (StatoMaiusc::No, "\"no\""),
        ] {
            assert_eq!(serde_json::to_string(&stato).unwrap_or_default(), nome);
        }
    }

    /// Il cablaggio: l'ascolto parte nel `setup`, e il comando è registrato.
    #[test]
    fn l_ascolto_e_cablato() {
        let lib = include_str!("lib.rs");
        let setup = lib
            .split(".setup(|app| {")
            .nth(1)
            .and_then(|resto| resto.split("\n        })\n").next())
            .unwrap_or_default();
        assert!(setup.contains("maiusc::avvia(app.handle())"));
        assert!(lib.contains("maiusc::maiusc_all_avvio,"));
    }
}
