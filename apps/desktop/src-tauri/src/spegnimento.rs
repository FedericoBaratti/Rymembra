//! Una bandiera sola: l'applicazione sta uscendo.
//!
//! # Il problema
//!
//! Chiudere una finestra di Tauri su Windows non è un istante, è una procedura:
//! arriva `CloseRequested`, poi `ExitRequested`, poi il ciclo degli eventi di
//! `tao` si dichiara `Destroyed`, poi `DestroyWindow` smonta la finestra
//! nascosta su cui viaggiano i messaggi fra i fili. In mezzo a tutto questo i
//! fili di sottofondo di Rymembra — l'orologio a quattro colpi al secondo, lo
//! spettro a trenta — continuano a chiamare `app.emit`, che non è una chiamata
//! innocua: è una `PostMessageW` verso quella finestra nascosta.
//!
//! Un messaggio impostato prima e consegnato dopo trova il ciclo già
//! `Destroyed`, e `tao` a quel punto non ha una via d'uscita gentile: panica.
//! Il panico è sul filo principale, dentro la libreria, e nel diario si legge
//! sempre uguale — `cannot move state from Destroyed`. Chi guarda vede
//! l'applicazione sparire mentre la chiude.
//!
//! # Perché una bandiera, e non il canale delle sveglie
//!
//! Un canale ce l'abbiamo già: i fili che ricevono una `Sveglia` si accorgono
//! dell'uscita quando il mittente cade e la `recv` risponde `Disconnected`. Non
//! serve, e non per un difetto suo: il mittente vive dentro `StatoLettore`, che
//! è stato **gestito**, e uno stato gestito viene lasciato cadere alla fine di
//! tutto — cioè quando il ciclo degli eventi è già morto e il panico è già
//! successo. La disconnessione arriva sempre troppo tardi per servire a questo.
//!
//! Serve qualcosa che si possa alzare al primo `CloseRequested`, dal filo
//! principale, e leggere da qualunque altro filo senza lucchetti e senza
//! aspettare: un `AtomicBool` statico. Non ha stato da costruire, non ha ordine
//! d'inizializzazione, non può fallire, e chi lo legge paga una lettura
//! rilassata — meno di quanto costi la `sleep` che ha appena finito di fare.
//!
//! # Cosa questa bandiera non fa
//!
//! Non azzera la corsa: fra l'istante in cui un filo la legge `false` e
//! l'istante in cui chiama `emit` c'è comunque una fessura. La restringe da
//! «tutta la durata della chiusura» a «una manciata di istruzioni», che con
//! cadenze da 33 e 250 millisecondi vuol dire che l'ultima emissione cade
//! largamente prima che la finestra venga smontata. Il resto lo copre il
//! profilo di rilascio, che non abortisce più il processo al primo panico.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::nota;

/// Alzata quando la finestra ha cominciato a chiudersi, e mai più abbassata.
///
/// Non torna `false` di proposito: dopo `CloseRequested` non esiste un modo
/// sostenuto in cui l'applicazione torni viva, e una bandiera che si può
/// riabbassare inviterebbe qualcuno a provarci.
static USCITA: AtomicBool = AtomicBool::new(false);

/// Dichiara che si sta uscendo. Si chiama dal filo principale, presto.
pub fn chiedi() {
    USCITA.store(true, Ordering::Relaxed);
}

/// Se l'uscita è cominciata.
///
/// Da leggere in cima a ogni giro dei cicli di sottofondo, **prima** di toccare
/// l'`AppHandle`: `emit`, `state`, `get_webview_window`. Chi la trova `true`
/// esce dal suo ciclo e basta — non ha niente da riordinare, perché la roba da
/// riordinare è tutta dentro stati gestiti che verranno lasciati cadere da soli.
pub fn in_uscita() -> bool {
    USCITA.load(Ordering::Relaxed)
}

/// La sequenza di spegnimento, una sola, per tutte le strade che portano fuori.
///
/// # Perché sta qui e non in `main`
///
/// Perché le strade sono due. La prima è la chiusura normale: il `RunEvent`
/// in fondo a `main`, per ognuno dei due o tre eventi che dicono «si chiude».
/// La seconda è l'aggiornamento: su Windows il plugin lancia l'installer e
/// chiama `exit(0)` dal filo dello scaricamento, e da lì il `RunEvent` non
/// passa mai. Fino alla 2.4.0 quella strada usciva senza scrivere la posizione
/// dell'ascolto, senza l'ultimo `PRAGMA optimize`, e lasciando nel vassoio
/// un'icona fantasma fino al passaggio del mouse. Due copie della sequenza
/// sarebbero due sequenze che divergono alla prima riga aggiunta a una sola:
/// questa la chiamano tutte e due (`aggiornamenti::costruisci`, con
/// `on_before_exit`).
///
/// # Ripetibile
///
/// Ogni passo regge la seconda chiamata — la bandiera resta alzata, la
/// scansione resta fermata, l'ottimizzazione si fa una volta sola, la scheda e
/// l'icona già staccate non ci sono più — perché il `RunEvent` la chiama più
/// volte di fila.
pub fn spegni(mano: &tauri::AppHandle) {
    use tauri::Manager as _;
    // Una riga nel diario, solo la prima volta: senza, una chiusura riuscita e
    // una mai cominciata si leggevano uguali — nessuna traccia — e la domanda
    // «alla fine di quella sessione la sequenza è girata?» non aveva risposta.
    // È la domanda che i panici alla fine della sessione di Windows ponevano.
    //
    // Una bandiera sua e non [`USCITA`]: «Esci» dal vassoio alza quella prima
    // di arrivare qui, e la sequenza vera risulterebbe sempre la seconda.
    static SEQUENZA: AtomicBool = AtomicBool::new(false);
    let inizio = std::time::Instant::now();
    let prima_volta = !SEQUENZA.swap(true, Ordering::Relaxed);
    chiedi();
    // E poi si dice alla scansione di smettere, se ce n'è una. Non è un di più
    // rispetto alla riga qui sopra: questa bandiera ferma i fili che la
    // guardano, e la scansione guarda la propria — quella che alza anche
    // «Annulla». Senza questa riga una scansione che sta leggendo una
    // condivisione di rete continuerebbe a leggerla mentre il resto del
    // programma se ne va, e le righe che seguono aspetterebbero il suo
    // lucchetto.
    //
    // `try_state` e non `state`: `state` panica se lo stato non c'è, e qui si
    // sta smontando tutto. Nessuno stato vuol dire nessuna scansione da
    // fermare.
    if let Some(stato) = mano.try_state::<crate::stato::Stato>() {
        stato.ferma_scansione();
    }
    // La sorveglianza delle cartelle: il suo filo esce, e gli osservatori del
    // sistema con lui, prima che una copia in corso gli faccia cominciare
    // un'altra scansione.
    crate::sorveglia::ferma(mano);
    // La riga del diario dopo i due arresti e non prima: scriverla prende il
    // lucchetto del diario e tocca il disco, e la scansione da fermare viene
    // prima di tutto.
    if prima_volta {
        nota!("[uscita] si chiude: sequenza di spegnimento");
    }
    // «Ascolta insieme» prima di tutto quel che salva: chi ospita avvisa gli
    // altri che la sessione finisce, chi segue esce, e il lettore torna alla
    // coda di questo PC — che è quella che `salva_uscendo` deve trovare.
    crate::insieme::ferma(mano);
    // L'ascolto in corso, che si chiude qui e conta, e il segno di «riprendi
    // dov'eri» — anche di un cursore mosso in pausa un attimo prima di
    // chiudere: il filo dell'orologio, che di solito lo scrive, lo fa solo
    // mentre suona, ed è appena uscito per via della bandiera.
    crate::riproduzione::salva_uscendo(mano);
    // L'ultimo `PRAGMA optimize`, che SQLite chiede prima di chiudere: una
    // volta sola, e al più un quarto di secondo d'attesa.
    crate::manutenzione::uscendo(mano);
    // Poi la scheda del sistema. Il distacco parla all'`HWND` della finestra,
    // e questo è l'ultimo momento in cui quell'`HWND` esiste ancora.
    crate::media::stacca(mano);
    // E l'icona dell'area di notifica, per la stessa ragione: anche lei se ne
    // va parlando col sistema operativo, e lasciarla cadere da sola vorrebbe
    // dire farlo a ciclo degli eventi già smontato.
    crate::vassoio::stacca(mano);
    if prima_volta {
        nota!(
            "[uscita] sequenza finita in {} ms",
            inizio.elapsed().as_millis()
        );
    }
}

/// Mandare eventi alla finestra solo finché la finestra c'è.
///
/// # Perché passa tutto di qui
///
/// Perché `emit` è la chiamata pericolosa — è una `PostMessageW` verso la
/// finestra nascosta di `tao` — e i posti da cui si chiama sono trentuno,
/// sparsi su undici file. Un controllo in cima a ogni ciclo di sottofondo non
/// basterebbe: fra il controllo e l'emissione ci può stare un lotto di lavoro
/// lungo secondi, e in quei secondi la finestra si chiude. Qui invece il
/// controllo e l'emissione sono adiacenti, e non c'è modo di dimenticarsene
/// aggiungendo un evento nuovo, perché l'unico modo di aggiungerne uno è
/// chiamare questo.
///
/// # Perché un tratto e non una funzione
///
/// Perché così la chiamata resta un metodo, e i metodi si prendono l'`AppHandle`
/// da soli tanto se chi chiama ne ha uno quanto se ne ha un prestito. Una
/// funzione libera avrebbe costretto ogni sito a sapere quale dei due ha in
/// mano, che è esattamente il tipo di dettaglio che si sbaglia in trentun posti.
pub trait Emette<R: tauri::Runtime>: tauri::Emitter<R> {
    /// Come `emit`, ma tace se l'applicazione sta chiudendo.
    ///
    /// L'esito si scarta come lo scartavano tutti i chiamanti di prima: un
    /// evento che non arriva perché la finestra non c'è più non è un guasto di
    /// cui qualcuno debba fare qualcosa.
    fn emetti<C: serde::Serialize + Clone>(&self, evento: &str, carico: C) {
        if in_uscita() {
            return;
        }
        let _ = self.emit(evento, carico);
    }
}

impl<R: tauri::Runtime, T: tauri::Emitter<R>> Emette<R> for T {}

#[cfg(test)]
mod prove {
    use super::*;

    /// La bandiera è statica e le prove girano tutte nello stesso processo:
    /// una sola prova la alza, e verifica entrambi i versi in ordine. Due prove
    /// separate si guarderebbero a vicenda a seconda di chi parte prima.
    ///
    /// # Attenzione a chi verrà dopo
    ///
    /// Questa prova alza la bandiera **per tutto il binario di prova**, e da
    /// quel momento [`Emette::emetti`] tace. Oggi non disturba nessuno perché
    /// nessun'altra prova di questo modulo chiama `emetti` o `in_uscita` — è
    /// stato controllato. Una prova futura che volesse verificare un'emissione
    /// non può stare qui dentro: l'ordine fra le due non è deciso da nessuno, e
    /// passerebbe o fallirebbe a caso.
    #[test]
    fn si_alza_e_non_torna_giu() {
        assert!(!in_uscita(), "nasce abbassata");
        chiedi();
        assert!(in_uscita(), "alzata resta alzata");
        chiedi();
        assert!(in_uscita(), "alzarla due volte non la abbassa");
    }
}
