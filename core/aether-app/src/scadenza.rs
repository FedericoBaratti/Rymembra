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
//! Il filo abbandonato continua a girare mentre chi l'ha lanciato è già andato
//! avanti: tutto quel che si manda di qui deve quindi essere `Send + 'static`,
//! cioè posseduto. È il motivo per cui la scansione tiene i suoi file dietro un
//! `Arc` e la sua cartella delle copertine per valore invece che in prestito.
//!
//! # Le tre forme
//!
//! Tre modi di comprare la stessa cosa, che si scelgono in base a **quanti**
//! lavori si aspettano e a **quando** se ne vuole l'esito.
//!
//! - [`con_scadenza`]: un lavoro solo, un filo solo, un risultato solo. È la
//!   forma più semplice e va bene finché i lavori sono radi — la sonda di una
//!   radice, l'apertura di un brano che sta per suonare.
//! - [`Operaio`]: molti lavori uno dopo l'altro, **un filo solo** che li serve
//!   tutti. Centomila `con_scadenza` di fila vorrebbero dire centomila fili
//!   creati e distrutti, che su Windows non è gratis; un operaio ne crea uno e
//!   lo riusa finché non gli si appende un compito addosso.
//! - [`a_rate`]: un lavoro lungo che produce **molti** risultati, consegnati man
//!   mano. Serve quando la cosa da limitare non è la durata totale ma il
//!   silenzio fra due consegne: una camminata su una libreria da centomila file
//!   dura legittimamente dei minuti, e l'unica domanda sensata è «da quanto non
//!   arriva più niente?».
//!
//! Niente tokio e nessun pool condiviso: un `std::thread::Builder` nominato e un
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
/// Un lavoro solo per filo: se i lavori sono tanti e brevi, [`Operaio`] fa la
/// stessa promessa riusando lo stesso filo.
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

/// Un lavoro da svolgere, già confezionato: la risposta se la manda da sé.
///
/// Scatolato perché i lavori di un [`Operaio`] hanno risultati di tipi diversi e
/// viaggiano tutti sullo stesso canale: il tipo del risultato sparisce qui
/// dentro, dove la chiusura si porta appresso il proprio mittente.
type Compito = Box<dyn FnOnce() + Send + 'static>;

/// Molti lavori a scadenza, un filo solo.
///
/// # Perché non basta [`con_scadenza`]
///
/// Perché una scansione legge centomila file, e centomila fili creati e
/// distrutti sono centomila chiamate al sistema operativo — su Windows la
/// creazione di un filo costa qualche decina di microsecondi e uno stack
/// riservato di un megabyte. Sul caso normale, quello in cui il disco risponde,
/// si finirebbe per pagare l'intera assicurazione contro la share morta anche a
/// chi la musica ce l'ha sull'SSD.
///
/// L'operaio tiene un filo suo e gli passa i compiti uno dopo l'altro. Finché i
/// lavori finiscono in tempo, è lo stesso filo per tutta la scansione.
///
/// # Cosa succede quando un compito si appende
///
/// Alla scadenza si smette di aspettare e si lascia cadere il mittente: il filo
/// resta dentro la `open` che non torna, e quando ne uscirà troverà il canale
/// chiuso e finirà da solo. L'operaio se ne dimentica — `in_servizio` torna a
/// `None` — e il compito successivo ne assume uno nuovo. Un file appeso costa
/// quindi un filo abbandonato, non uno per ogni file rimasto.
///
/// Nessun `Drop`: lasciando cadere l'operaio cade il suo mittente, il `for` sul
/// ricevitore finisce e il filo esce da sé. Scriverne uno vorrebbe dire
/// aspettare la fine di un filo che potrebbe essere quello appeso, cioè
/// rimettere il blocco esattamente dove lo si è tolto.
pub struct Operaio {
    /// Il nome che i suoi fili portano nel diario dei panici.
    nome: String,
    /// Il mittente verso il filo assunto, se ce n'è uno che risponde ancora.
    in_servizio: Option<mpsc::Sender<Compito>>,
}

impl Operaio {
    /// Un operaio che non ha ancora assunto nessuno.
    ///
    /// Il filo si assume al primo compito: un operaio costruito e mai usato —
    /// una scansione senza niente da leggere — non costa un filo.
    ///
    /// `nome` finisce nel nome del filo, quindi va breve e senza spazi.
    pub fn nuovo(nome: &str) -> Self {
        Self {
            nome: nome.to_owned(),
            in_servizio: None,
        }
    }

    /// Manda un compito al filo in servizio, assumendone uno se serve.
    ///
    /// `false` soltanto se il sistema non dà più fili, che per chi chiama è la
    /// stessa cosa di una scadenza: il risultato non arriverà.
    fn manda(&mut self, compito: Compito) -> bool {
        // Il compito torna indietro dentro il `SendError`, ed è l'unico modo di
        // riaverlo: una `FnOnce` scatolata non si duplica, e senza questo
        // recupero un filo finito nel frattempo costerebbe un lavoro perso
        // invece di un filo nuovo.
        let compito = match self.in_servizio.as_ref() {
            None => compito,
            Some(canale) => match canale.send(compito) {
                Ok(()) => return true,
                Err(mpsc::SendError(respinto)) => {
                    self.in_servizio = None;
                    respinto
                }
            },
        };

        let (manda, ricevi) = mpsc::channel::<Compito>();
        let avviato = std::thread::Builder::new()
            .name(format!("aether-operaio-{}", self.nome))
            .spawn(move || {
                // Finché qualcuno tiene il mittente. Quando l'operaio lo lascia
                // cadere — perché è stato distrutto, o perché questo filo è
                // quello che si è appeso — il ciclo finisce e il filo esce.
                for compito in ricevi {
                    compito();
                }
            });
        if avviato.is_err() {
            return false;
        }
        let consegnato = manda.send(compito).is_ok();
        self.in_servizio = Some(manda);
        consegnato
    }

    /// Esegue `lavoro` sul filo dell'operaio e aspetta al più `scadenza`.
    ///
    /// `None` se è scaduto, se il filo è caduto durante il lavoro, o se non se
    /// n'è potuto assumere uno: le tre si confondono per la stessa ragione per
    /// cui le confonde [`con_scadenza`], cioè che chi chiama può farci una cosa
    /// sola.
    ///
    /// Dopo un `None` l'operaio è di nuovo senza filo, e la chiamata successiva
    /// ne assume uno nuovo: un file appeso non contagia quelli dopo.
    pub fn esegui<T: Send + 'static>(
        &mut self,
        scadenza: Duration,
        lavoro: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let (rispondi, risposta) = mpsc::channel();
        let compito: Compito = Box::new(move || {
            // Come in `con_scadenza`: se chi aspettava se n'è andato, il canale
            // è chiuso e non c'è niente da fare né da dire.
            let _ = rispondi.send(lavoro());
        });
        if !self.manda(compito) {
            return None;
        }
        match risposta.recv_timeout(scadenza) {
            Ok(esito) => Some(esito),
            Err(_) => {
                // Scaduto, oppure il filo è caduto portandosi via il mittente.
                // In entrambi i casi quel filo non serve più a niente: lo si
                // dimentica, e lasciando cadere il mittente gli si dice di
                // uscire appena avrà finito quel che sta facendo.
                self.in_servizio = None;
                None
            }
        }
    }
}

/// Un lavoro lungo che consegna i suoi risultati man mano.
///
/// `lavoro` riceve un mittente e ci manda dentro quel che produce; chi chiama
/// riceve il ricevitore e decide da sé quanto aspettare fra due consegne — di
/// solito con un `recv_timeout`, che è la scadenza che qui non c'è.
///
/// `None` soltanto se il filo non è partito.
///
/// # Perché il canale non ha un tetto
///
/// È un canale **senza limite**, e deve restare tale. Il produttore tipico di
/// questa forma è una camminata sul filesystem, che oggi accumulerebbe gli
/// stessi elementi in un `Vec` e li restituirebbe in blocco: la memoria che il
/// canale può arrivare a tenere è esattamente quella, non un byte di più.
///
/// Trasformarlo in un `sync_channel` con un tetto sembrerebbe una prudenza e
/// sarebbe il contrario: alla coda piena il produttore si blocca nella `send`, e
/// chi consuma sta misurando proprio **il silenzio del produttore** per decidere
/// se la rete è morta. Un produttore fermo perché il consumatore è lento
/// diventerebbe indistinguibile da un produttore fermo perché la share non
/// risponde, e la scansione dichiarerebbe irraggiungibile un disco sano.
///
/// # Chi smette di ascoltare ferma il produttore
///
/// Lasciando cadere il ricevitore, la prima `send` successiva fallisce. Il
/// produttore che tratta quell'errore come «basta così» esce da sé, ed è così
/// che un annullamento durante una camminata non lascia un filo a enumerare
/// centomila file per nessuno.
///
/// `nome` finisce nel nome del filo, quindi va breve e senza spazi.
pub fn a_rate<T: Send + 'static>(
    nome: &str,
    lavoro: impl FnOnce(&mpsc::Sender<T>) + Send + 'static,
) -> Option<mpsc::Receiver<T>> {
    let (manda, ricevi) = mpsc::channel();
    let avviato = std::thread::Builder::new()
        .name(format!("aether-a-rate-{nome}"))
        .spawn(move || lavoro(&manda));
    if avviato.is_err() {
        return None;
    }
    Some(ricevi)
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
        // quella `send` panicasse, il gancio dei panici scriverebbe nel diario e
        // la prova finirebbe comunque verde — ma il filo si porterebbe via il
        // risultato invece di lasciarlo cadere in silenzio, che è la promessa
        // scritta qui sopra. Aspettarne la fine è il modo più economico di
        // esercitare quel percorso.
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

    #[test]
    fn l_operaio_riusa_il_suo_filo() {
        // Il motivo per cui l'operaio esiste: su centomila file, un filo solo.
        let mut operaio = Operaio::nuovo("prova");
        let primo = operaio.esegui(Duration::from_secs(5), || std::thread::current().id());
        let secondo = operaio.esegui(Duration::from_secs(5), || std::thread::current().id());
        assert!(primo.is_some() && secondo.is_some(), "entrambi in tempo");
        assert_eq!(primo, secondo, "due compiti, un filo solo");
        // E non è il filo di chi chiama: il lavoro deve poter restare indietro.
        assert_ne!(primo, Some(std::thread::current().id()));
    }

    #[test]
    fn un_compito_appeso_scade_e_il_prossimo_trova_un_filo_nuovo() {
        let mut operaio = Operaio::nuovo("appeso");
        // Il compito appeso dice chi è **prima** di appendersi, su un canale
        // suo. Dopo non lo dirà a nessuno: il suo valore di ritorno è già stato
        // buttato via, ed è il punto della scadenza. Senza questo canale l'unico
        // identificativo in mano sarebbe il `None` della scadenza, e
        // confrontarlo con quello del compito dopo sarebbe vero per
        // costruzione — cioè non proverebbe niente.
        let (dice_chi_e, chi_era) = mpsc::channel();
        let prima = std::time::Instant::now();
        let appeso = operaio.esegui(Duration::from_millis(20), move || {
            let _ = dice_chi_e.send(std::thread::current().id());
            std::thread::sleep(Duration::from_millis(300));
        });
        let passato = prima.elapsed();
        assert_eq!(appeso, None, "doveva scadere");
        assert!(
            passato < Duration::from_millis(150),
            "ha aspettato la fine del lavoro appeso: {passato:?}"
        );

        // Il filo di prima è ancora dentro la sua `sleep`: il compito dopo non
        // deve mettersi in coda dietro di lui, deve trovarne uno nuovo. È la
        // differenza fra «un file illeggibile costa una scadenza» e «un file
        // illeggibile blocca tutti quelli dopo».
        let filo_appeso = chi_era
            .recv_timeout(Duration::from_secs(5))
            .expect("il compito appeso deve aver detto su quale filo girava");
        let dopo = operaio.esegui(Duration::from_secs(5), || std::thread::current().id());
        assert!(dopo.is_some(), "il compito dopo deve passare");
        assert_ne!(
            dopo,
            Some(filo_appeso),
            "il compito dopo ha trovato il filo appeso, non uno nuovo"
        );

        // Il filo abbandonato finisce da sé e prova a mandare su un canale
        // chiuso: si aspetta che lo faccia, per esercitare quel percorso.
        std::thread::sleep(Duration::from_millis(400));
    }

    #[test]
    fn i_passi_arrivano_a_rate_e_il_silenzio_si_riconosce() {
        let passi = a_rate("passi", |manda| {
            for n in 0..3 {
                let _ = manda.send(n);
            }
            std::thread::sleep(Duration::from_millis(400));
        })
        .expect("il filo deve partire");

        // I tre arrivano, e arrivano subito: nessuno aspetta la fine del lavoro.
        let prima = std::time::Instant::now();
        for atteso in 0..3 {
            assert_eq!(passi.recv_timeout(Duration::from_secs(5)), Ok(atteso));
        }
        assert!(
            prima.elapsed() < Duration::from_millis(300),
            "le rate non devono aspettare la fine"
        );

        // E poi il silenzio si riconosce per quel che è: non «finito», ma «da
        // qui in poi non arriva più niente entro il tempo che aspetto». È la
        // domanda con cui la scansione decide che una radice si è arenata.
        assert_eq!(
            passi.recv_timeout(Duration::from_millis(30)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
    }

    #[test]
    fn chi_smette_di_ascoltare_ferma_il_produttore() {
        let mandati = Arc::new(AtomicUsize::new(0));
        let finito = Arc::new(AtomicBool::new(false));
        let contatore = Arc::clone(&mandati);
        let bandiera = Arc::clone(&finito);
        let passi = a_rate("basta", move |manda| {
            // Centomila giri da un millisecondo l'uno: se la `send` non
            // fallisse mai, questo filo starebbe qui per cento secondi, e la
            // prova qui sotto se ne accorgerebbe entro due.
            for n in 0..100_000 {
                if manda.send(n).is_err() {
                    break;
                }
                contatore.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(1));
            }
            bandiera.store(true, Ordering::SeqCst);
        })
        .expect("il filo deve partire");

        assert_eq!(passi.recv_timeout(Duration::from_secs(5)), Ok(0));
        drop(passi);

        // Il produttore se ne accorge alla `send` dopo, non prima: gli si dà il
        // tempo di arrivarci. Il numero di elementi mandati non è il punto — il
        // punto è che il filo **finisca**, invece di enumerare centomila file
        // per nessuno.
        for _ in 0..100 {
            if finito.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            finito.load(Ordering::SeqCst),
            "il produttore ha continuato senza nessuno che ascoltasse: {} mandati",
            mandati.load(Ordering::SeqCst)
        );
    }
}
