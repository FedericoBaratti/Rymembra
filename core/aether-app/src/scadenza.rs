//! Lavoro bloccante con una scadenza addosso.
//!
//! # Il problema
//!
//! Su una condivisione SMB che non risponde più — server spento, VPN caduta,
//! cavo staccato — una `std::fs::File::open` non fallisce: **aspetta**. Il
//! timeout di connessione di Windows su quel percorso è di una quarantina di
//! secondi, e in quei quaranta secondi il filo che ha chiamato non esiste per
//! nessuno. Se è il filo principale della finestra, quella è una finestra che il
//! sistema dichiara «non risponde»; se tiene un lucchetto, sono fermi anche
//! tutti i fili di sottofondo che quel lucchetto lo vogliono.
//!
//! Non c'è un modo portabile di dire a `std::fs` «rinuncia dopo cinque secondi».
//! Quel che c'è è un altro filo: si manda lì il lavoro e si aspetta il
//! risultato per quanto si è disposti ad aspettare.
//!
//! # Il filo che resta indietro
//!
//! Alla scadenza si restituisce `None` e **non si aspetta più**. Il filo che sta
//! ancora dentro la `open` non lo si può interrompere — nessun sistema lo
//! consente in modo sicuro — quindi finisce da solo, prova a mandare il
//! risultato su un canale che non ha più nessun ascoltatore, e la `send`
//! fallisce senza fare rumore. Il filo esce, il risultato viene lasciato cadere.
//! È deliberato: costa un filo appeso per il tempo del timeout di sistema, e
//! compra una finestra che resta viva.
//!
//! Niente tokio, niente pool: un `std::thread::Builder` nominato e un
//! `recv_timeout`, che sono gli stessi mattoni con cui è fatto tutto il resto
//! dell'applicazione. Il nome del filo serve al gancio dei panici, che senza di
//! quello direbbe soltanto «un filo è caduto».

use std::sync::mpsc;
use std::time::Duration;

/// Esegue `lavoro` su un filo suo e aspetta al più `scadenza`.
///
/// `Some(esito)` se il lavoro è finito in tempo, `None` se è scaduto — o se il
/// filo non è nemmeno partito, che è il caso in cui il sistema ha finito i fili.
/// I due si confondono di proposito: per chi chiama sono la stessa cosa, «il
/// risultato non c'è», e distinguerli vorrebbe dire un tipo d'errore in più per
/// una condizione che nessun chiamante saprebbe trattare diversamente.
///
/// Il ripiego «se non parte il filo, eseguilo qui» non c'è, e non è una
/// dimenticanza: la chiusura è già stata mossa dentro quella che il filo
/// avrebbe eseguito, riprenderla in mano vorrebbe dire duplicarla — e comunque
/// eseguirla in linea rimetterebbe esattamente il blocco che questa funzione
/// esiste per togliere.
///
/// `nome` finisce nel nome del filo, quindi va breve e senza spazi.
pub fn con_scadenza<T: Send + 'static>(
    nome: &str,
    scadenza: Duration,
    lavoro: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (manda, ricevi) = mpsc::channel();
    let avviato = std::thread::Builder::new()
        .name(format!("aether-scadenza-{nome}"))
        .spawn(move || {
            // L'esito si ignora apposta: se chi aspettava se n'è andato, il
            // canale è chiuso e non c'è niente da fare né da dire.
            let _ = manda.send(lavoro());
        });
    if avviato.is_err() {
        return None;
    }
    ricevi.recv_timeout(scadenza).ok()
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_lavoro_svelto_torna_col_suo_risultato() {
        assert_eq!(
            con_scadenza("svelto", Duration::from_secs(5), || 42),
            Some(42)
        );
    }

    #[test]
    fn un_lavoro_appeso_scade_e_non_si_aspetta_la_sua_fine() {
        let prima = std::time::Instant::now();
        let esito = con_scadenza("appeso", Duration::from_millis(20), || {
            std::thread::sleep(Duration::from_millis(200));
            42
        });
        let passato = prima.elapsed();
        assert_eq!(esito, None, "doveva scadere");
        assert!(
            passato < Duration::from_millis(150),
            "ha aspettato la fine del lavoro: {passato:?}"
        );

        // E adesso la parte che conta davvero: il filo ritardatario finisce da
        // solo e prova a mandare su un canale che non ascolta più nessuno. Se
        // quella `send` panicasse, con `panic = "abort"` in rilascio cadrebbe
        // l'intero processo — qui cadrebbe l'intera suite di prove, che è il
        // modo più economico di accorgersene.
        std::thread::sleep(Duration::from_millis(250));
    }

    #[test]
    fn il_risultato_puo_essere_qualcosa_di_grosso() {
        // Attraversa il canale, quindi deve essere `Send`: è la ragione per cui
        // `Sorgente` può passare di qui, ed è quel che rende non bloccante
        // l'apertura di un brano.
        let esito = con_scadenza("grosso", Duration::from_secs(5), || vec![1_u8; 4_096]);
        assert_eq!(esito.map(|v| v.len()), Some(4_096));
    }
}
