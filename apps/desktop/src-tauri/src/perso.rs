//! Gli eventi una tantum che a finestra distrutta non hanno nessuno a cui
//! arrivare (S4-5).
//!
//! # Il difetto
//!
//! Con la X sul vassoio la finestra è distrutta e il motore resta (`comparsa`).
//! La pagina nuova «chiede al nucleo come stanno le cose»: vale per gli stati, non
//! per gli eventi. `riproduzione:errore` è un evento senza stato dietro: un tasto
//! multimediale o la voce «Successivo» del vassoio incontrano un file sparito,
//! il lettore si ferma, l'errore parte verso nessuna pagina e si perde. «Mostra»
//! ricrea la finestra, che trova il lettore fermo e niente che dica perché — il
//! perché resta solo nel diario.
//!
//! Gli altri eventi una tantum hanno un comando che li ricostruisce
//! (`libreria:stato`, `lontano:richiesta`, `aggiornamenti:stato`, `scansione:*`,
//! `scarico:*`, `nuvola:*`), o un posto in cui aspettano (`lontano:link`).
//!
//! # La correzione
//!
//! Il nucleo tiene l'**ultimo** errore del lettore emesso mentre nessuna pagina
//! ascolta, e la pagina nuova lo riceve **una volta** quando ha aperto l'ascolto
//! ([`riproduzione_errore_perso`]: gira dopo `listen`, nell'ordine che `useAscolto`
//! garantisce con il suo `poi`). «Nessuna pagina ascolta» vale dalla fine della
//! pagina (`pagina::pagina_finita`: la finestra distrutta, o una pagina che si
//! ricarica) fino alla richiesta della pagina nuova. Un errore di più di un'ora fa
//! non si ripresenta come se fosse di adesso.
//!
//! Sta qui e non in `riproduzione`: è la cucitura fra il nucleo e la pagina, e
//! passa da `spegnimento::Emette::emetti`, da cui passa ogni evento.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// L'evento con cui il lettore dice che non riesce a suonare. Solo sul computer:
/// il telefono non ha una finestra che muore e rinasce da sola.
#[cfg(desktop)]
pub const EVENTO_ERRORE_DEL_LETTORE: &str = "riproduzione:errore";

/// Quanto un errore perso resta buono da mostrare.
const ETA_MASSIMA_DELL_ERRORE: Duration = Duration::from_secs(60 * 60);

/// L'ultimo errore del lettore che nessuna pagina ha potuto ricevere.
#[derive(Debug, Default)]
struct Persi {
    /// C'è una pagina che ascolta `riproduzione:errore`.
    ascolta: bool,
    /// L'ultimo errore emesso a pagina assente, e quando.
    errore: Option<(serde_json::Value, Instant)>,
}

impl Persi {
    const fn nuovo() -> Self {
        Self {
            ascolta: false,
            errore: None,
        }
    }

    /// Il lettore ha emesso un errore. Con una pagina che ascolta arriva da sé, e
    /// non si copia niente: una seconda copia a ogni ricarica sarebbe un errore
    /// che torna da solo.
    #[cfg(desktop)]
    fn emesso(&mut self, carico: serde_json::Value, adesso: Instant) {
        if self.ascolta {
            return;
        }
        self.errore = Some((carico, adesso));
    }

    /// Una pagina ha aperto l'ascolto: se ne è perso uno, lo riceve ora.
    fn la_pagina_ascolta(&mut self, adesso: Instant) -> Option<serde_json::Value> {
        self.ascolta = true;
        self.errore
            .take()
            .filter(|(_, quando)| {
                adesso.saturating_duration_since(*quando) <= ETA_MASSIMA_DELL_ERRORE
            })
            .map(|(carico, _)| carico)
    }

    /// La pagina è finita: da qui un errore non ha nessuno che lo riceva.
    fn la_pagina_e_finita(&mut self) {
        self.ascolta = false;
    }
}

static PERSI: Mutex<Persi> = Mutex::new(Persi::nuovo());

fn persi() -> MutexGuard<'static, Persi> {
    PERSI
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Il lettore ha emesso un errore: chiamata da [`crate::spegnimento::Emette`]
/// per questo evento soltanto, e solo sul computer.
#[cfg(desktop)]
pub fn errore_del_lettore_emesso<C: serde::Serialize>(carico: &C) {
    let mut persi = persi();
    // La copia in JSON costa solo se nessuna pagina ascolta.
    if persi.ascolta {
        return;
    }
    if let Ok(valore) = serde_json::to_value(carico) {
        persi.emesso(valore, Instant::now());
    }
}

/// La pagina è finita (distrutta, o ricaricata): vedi [`Persi::la_pagina_e_finita`].
pub fn pagina_finita_per_gli_errori() {
    persi().la_pagina_e_finita();
}

/// La pagina ascolta `riproduzione:errore`, e chiede se se ne è perso uno. Una
/// volta sola.
#[tauri::command]
pub async fn riproduzione_errore_perso() -> crate::errore::Esito<Option<serde_json::Value>> {
    crate::disparte::comanda("errore perso del lettore", || {
        Ok(persi().la_pagina_ascolta(Instant::now()))
    })
    .await
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Rymembra nel vassoio, finestra distrutta: un tasto multimediale incontra un
    /// file sparito, il lettore si ferma e l'errore parte verso nessuna pagina.
    /// «Mostra» ricrea la finestra, che trova il lettore fermo e niente che dica
    /// perché. Il nucleo tiene l'ultimo errore e la pagina nuova lo riceve.
    #[test]
    fn un_errore_emesso_a_pagina_assente_arriva_alla_pagina_nuova() {
        let mut persi = Persi::nuovo();
        let t = Instant::now();
        let errore = serde_json::json!({ "code": "media.fileMissing" });
        persi.emesso(errore.clone(), t);
        assert_eq!(
            persi.la_pagina_ascolta(t + Duration::from_secs(30)),
            Some(errore)
        );
        // Una volta sola: la pagina l'ha ricevuto.
        assert_eq!(persi.la_pagina_ascolta(t + Duration::from_secs(31)), None);
    }

    #[test]
    fn con_una_pagina_che_ascolta_l_errore_non_si_tiene() {
        let mut persi = Persi::nuovo();
        let t = Instant::now();
        assert_eq!(persi.la_pagina_ascolta(t), None);
        persi.emesso(serde_json::json!({ "code": "a" }), t);
        // La pagina l'ha già avuto dall'evento: una seconda copia a ogni
        // ricarica sarebbe un errore che torna da solo.
        assert_eq!(persi.la_pagina_ascolta(t), None);
    }

    #[test]
    fn dopo_la_fine_della_pagina_gli_errori_si_tengono_di_nuovo() {
        let mut persi = Persi::nuovo();
        let t = Instant::now();
        assert_eq!(persi.la_pagina_ascolta(t), None);
        persi.la_pagina_e_finita();
        let errore = serde_json::json!({ "code": "b" });
        persi.emesso(errore.clone(), t);
        assert_eq!(persi.la_pagina_ascolta(t), Some(errore));
    }

    #[test]
    fn si_tiene_solo_l_ultimo_errore_e_solo_se_e_recente() {
        let mut persi = Persi::nuovo();
        let t = Instant::now();
        persi.emesso(serde_json::json!({ "code": "primo" }), t);
        let ultimo = serde_json::json!({ "code": "ultimo" });
        persi.emesso(ultimo.clone(), t + Duration::from_secs(1));
        assert_eq!(
            persi.la_pagina_ascolta(t + Duration::from_secs(2)),
            Some(ultimo)
        );
        // Un errore di ore fa non si ripresenta come se fosse di adesso.
        persi.la_pagina_e_finita();
        persi.emesso(serde_json::json!({ "code": "vecchio" }), t);
        assert_eq!(
            persi.la_pagina_ascolta(t + ETA_MASSIMA_DELL_ERRORE + Duration::from_secs(1)),
            None
        );
    }

    /// Il cablaggio: ogni evento passa da `emetti` (e solo quello degli errori si
    /// copia), la fine della pagina lo dice, e la pagina chiede dopo aver aperto
    /// l'ascolto.
    #[test]
    fn l_errore_perso_e_cablato_dall_emissione_alla_pagina() {
        let emetti = include_str!("spegnimento.rs");
        let emetti = emetti.split("pub trait Emette").nth(1).unwrap_or_default();
        let emetti = emetti.split("#[cfg(test)]").next().unwrap_or(emetti);
        assert!(emetti.contains("perso::EVENTO_ERRORE_DEL_LETTORE"));
        assert!(emetti.contains("perso::errore_del_lettore_emesso(&carico)"));
        assert!(
            emetti.contains("#[cfg(desktop)]"),
            "la copia dell'errore è del computer"
        );
        let pagina = include_str!("pagina.rs");
        let finita = pagina
            .split("pub(crate) fn pagina_finita(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert!(finita.contains("perso::pagina_finita_per_gli_errori()"));
        let lib = include_str!("lib.rs");
        assert!(lib.contains("perso::riproduzione_errore_perso,"));
        let ipc = include_str!("../../src/ipc.ts");
        assert!(ipc.contains("riproduzione_errore_perso"));
    }
}
