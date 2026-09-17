//! Il giro guidato: se va fatto, e che è stato fatto.
//!
//! Due comandi e una costante. Tutto il resto del giro — il velo, il buco, il
//! fumetto, i dieci passi — sta nella finestra (`apps/desktop/src/Giro.tsx`),
//! perché è materia di rettangoli sullo schermo e non di libreria. Qui c'è
//! soltanto la memoria: quel che va ricordato fra due avvii, e che quindi deve
//! stare nel database.
//!
//! # Perché una versione del **copione**, e non quella dell'applicazione
//!
//! In tabella finisce [`VERSIONE_COPIONE`], che è un numero di questo file e
//! non `env!("CARGO_PKG_VERSION")`. La differenza si vede alla prima release
//! correttiva: con la versione dell'applicazione, una 2.3.2 che sistema un
//! errore di battitura riaprirebbe il giro a tutti — dieci fumetti da leggere
//! per una virgola. La domanda giusta non è «l'applicazione è cambiata» ma «il
//! giro ha qualcosa di nuovo da dire», e a quella sa rispondere solo chi il
//! copione lo scrive.
//!
//! Il confronto è quindi `!=` e non «maggiore di», ed è voluto: un copione
//! riscritto più corto non è una versione più bassa, è un copione diverso, e
//! chi lo ha visto nella forma di prima non l'ha visto in questa. Per la stessa
//! ragione il nucleo conserva il testo com'è, senza interpretarlo — vedi il
//! preambolo di `aether_app::preferenze`.
//!
//! # Perché la decisione sta qui e non nella finestra
//!
//! Perché è una riga sola e ha un solo modo di essere giusta. Mandando la
//! versione vista in TypeScript, il confronto lo scriverebbe la finestra, e il
//! numero del copione starebbe in due posti: qui per essere scritto, là per
//! essere confrontato. Due posti per un numero solo è il modo in cui i due
//! divergono, e il sintomo — un giro che non riparte, o che riparte sempre —
//! non nomina la sua causa.
//!
//! # Il modello, e cosa non si copia
//!
//! La forma è quella di `vassoio::secondo_piano`/`secondo_piano_attiva`:
//! lettura sincrona, scrittura `async`, tutte e due sotto `con_libreria`.
//! **Non** si copia il ritorno riletto dal database di `secondo_piano_attiva`:
//! là serve perché un interruttore disegna la preferenza e deve smettere di
//! mentire se la scrittura fallisce, qui non c'è niente da disegnare — il giro
//! è già finito quando questo comando parte, e riaprirlo perché la scrittura
//! non è passata sarebbe il rimedio peggiore del male.

use tauri::State;

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// La versione del copione del giro guidato.
///
/// Si alza **soltanto** quando il giro ha qualcosa di nuovo da mostrare — un
/// passo in più, una schermata che prima non c'era — e allora chi l'ha già
/// fatto se lo rivede una volta. Non si alza per una correzione di testo, per
/// una traduzione, né perché è uscita una release: vedi il preambolo.
pub const VERSIONE_COPIONE: &str = "2.3.2";

/// Se il giro di [`VERSIONE_COPIONE`] è ancora da fare.
///
/// Separata dal comando perché è l'unica cosa qui dentro che si può sbagliare,
/// ed è quindi l'unica che vale la pena provare senza un database di mezzo.
fn da_fare(visto: Option<&str>) -> bool {
    visto != Some(VERSIONE_COPIONE)
}

/// Se il giro guidato va proposto.
///
/// La finestra la chiede una volta all'avvio e poi decide da sé **quando**:
/// non subito, non con la libreria vuota, e mai sopra il primo avvio. Qui si
/// risponde soltanto alla domanda che il database sa: «questo copione è già
/// stato visto?».
///
/// # Errori
///
/// `db.queryFailed`.
#[tauri::command]
pub fn giro_da_fare(stato: State<'_, Stato>) -> Esito<bool> {
    let visto = con_libreria(&stato, |libreria| {
        aether_app::preferenze::giro_visto(&libreria.connection)
    })
    .map_err(errore)?;
    Ok(da_fare(visto.as_deref()))
}

/// Segna il giro come fatto, alla versione del copione di oggi.
///
/// La chiama la finestra quando il giro finisce **e** quando lo si salta: sono
/// due modi di dire la stessa cosa al programma — «questo l'ho visto» — e
/// distinguerli vorrebbe dire riproporre a ogni avvio un giro che qualcuno ha
/// già rifiutato una volta, che è il modo di farlo odiare. «Rifai il giro»
/// resta la via di ritorno, ed è esplicita.
///
/// # Errori
///
/// `db.queryFailed`.
#[tauri::command(async)]
pub fn giro_fatto(stato: State<'_, Stato>) -> Esito<()> {
    con_libreria(&stato, |libreria| {
        aether_app::preferenze::imposta_giro_visto(&libreria.connection, VERSIONE_COPIONE)
    })
    .map_err(errore)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn senza_niente_in_tabella_il_giro_si_fa() {
        assert!(da_fare(None), "è il primo avvio: il giro esiste per questo");
    }

    #[test]
    fn il_copione_gia_visto_non_si_ripropone() {
        assert!(!da_fare(Some(VERSIONE_COPIONE)));
    }

    #[test]
    fn un_copione_diverso_si_ripropone() {
        // Diverso, non «più vecchio»: il confronto è `!=` per la ragione
        // scritta nel preambolo.
        assert!(da_fare(Some("2.2.0")));
        assert!(da_fare(Some("9.9.9")));
    }
}
