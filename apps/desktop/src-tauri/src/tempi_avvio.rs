//! I tempi dell'avvio, nel diario: una riga per fase, una volta per processo.
//!
//! # Perché esiste
//!
//! Perché il collaudo dal vivo di 3.3.0 ha misurato un avvio a freddo più lento
//! di R21 (finestra visibile a 3,2–4,4 s contro 2,74 s) e il diario non poteva
//! dire **quale fase** fosse cresciuta: la libreria aperta, il resto dell'avvio
//! sul filo della finestra, il cancello, la pagina che chiede, o la rete di
//! sicurezza dei due secondi che mostra la finestra al posto di `pronto`. Con
//! queste righe il prossimo collaudo lo legge dal diario e basta, senza un
//! programma di campionamento fuori dal processo.
//!
//! # Come si legge
//!
//! Ogni riga dice la fase e quanti millisecondi sono passati dall'inizio di
//! `run()` ([`comincia`]): `[avvio] fase libreria aperta: +310 ms`. L'istante
//! assoluto lo porta già la riga del diario, quindi il tempo fra il lancio del
//! processo e `run()` — il caricamento dell'eseguibile, che a freddo cresce con
//! la sua misura — è la differenza fra l'ora del lancio e la prima riga
//! (`sessione aperta`) meno il suo `+N ms`.
//!
//! # Quanto costa
//!
//! Una riga per fase, una volta per processo: una dozzina di righe per avvio, e
//! dopo la prima volta una ricerca in un elenco di una dozzina di nomi. La
//! finestra ricreata dal vassoio non riscrive niente.

use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use crate::nota;

/// L'inizio di `run()`.
static INIZIO: OnceLock<Instant> = OnceLock::new();

/// Le fasi già viste in questo processo, e quelle che aspettano il diario: la
/// prima fase passa **prima** che il diario sia aperto (si apre nel `setup`,
/// dopo la creazione della finestra), e a diario chiuso `nota!` non ha dove
/// scrivere.
static VISTE: Mutex<Viste> = Mutex::new(Viste {
    nomi: Vec::new(),
    arretrate: Vec::new(),
});

struct Viste {
    nomi: Vec<&'static str>,
    arretrate: Vec<(&'static str, u128)>,
}

/// Segna l'inizio dell'avvio. La prima volta soltanto: su Android `run()` può
/// seguire un servizio che ha già avviato il nucleo, e il tempo conta da quando
/// il processo ha cominciato.
pub fn comincia() {
    let _ = INIZIO.set(Instant::now());
}

/// Scrive la fase nel diario, con i millisecondi dall'inizio di `run()`, **la
/// prima volta** che passa di qui in questo processo. A diario ancora chiuso la
/// tiene da parte e la scrive con la prima fase che lo trova aperto. Prima di
/// [`comincia`] non scrive niente.
pub fn fase(nome: &'static str) {
    let Some(inizio) = INIZIO.get() else {
        return;
    };
    let trascorsi = inizio.elapsed().as_millis();
    let da_scrivere = {
        let mut viste = VISTE.lock().unwrap_or_else(PoisonError::into_inner);
        if viste.nomi.contains(&nome) {
            return;
        }
        viste.nomi.push(nome);
        viste.arretrate.push((nome, trascorsi));
        if crate::diario::cartella().is_none() {
            return;
        }
        std::mem::take(&mut viste.arretrate)
    };
    for (nome, trascorsi) in da_scrivere {
        nota!("[avvio] fase {nome}: +{trascorsi} ms");
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn le_fasi_hanno_il_formato_che_il_collaudo_cerca() {
        comincia();
        let diario = crate::diario::a_diario_aperto(|| {
            fase("di prova per il formato");
            fase("di prova per il formato");
        });
        let righe: Vec<&str> = diario
            .lines()
            .filter(|riga| riga.contains("[avvio] fase di prova per il formato: +"))
            .collect();
        assert_eq!(righe.len(), 1, "una riga per fase: {diario}");
        assert!(
            righe[0].trim_end().ends_with(" ms"),
            "i millisecondi in coda: {}",
            righe[0]
        );
    }
}
