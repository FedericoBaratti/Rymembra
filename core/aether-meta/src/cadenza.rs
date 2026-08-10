//! Il ritmo con cui si bussa, e quando si smette di bussare.
//!
//! # Perché serve, e perché non basta il ritentativo
//!
//! [`aether_net::Rete::ritenta`] sa aspettare **dopo** un rifiuto. Questa
//! struttura serve a non farsi rifiutare: MusicBrainz concede *una richiesta al
//! secondo per indirizzo IP*, e a chi va più veloce risponde `503`. Un
//! ritentativo su un `503` è una seconda richiesta che arriva ancora troppo
//! presto — cioè il modo di trasformare un limite in una tempesta.
//!
//! La cadenza è quindi un cancello **prima** della richiesta, non una reazione
//! dopo. Se ne tiene una per servizio, con l'intervallo che quel servizio
//! dichiara.
//!
//! # L'interruttore
//!
//! Dopo [`GUASTI_PER_APRIRE`] guasti di trasporto consecutivi il servizio si
//! considera giù, e per [`RAFFREDDAMENTO`] non gli si parla più. Non è
//! prudenza astratta: senza, una passata di arricchimento su una macchina senza
//! rete accumula centoventi timeout da venti secondi l'uno — quaranta minuti di
//! un filo occupato a scoprire ogni volta la stessa cosa.
//!
//! È lo stesso `CircuitBreaker` del vecchio albero, ridotto a quel che serviva
//! davvero: là aveva tre stati e una richiesta di prova, qui ha una scadenza.
//! Il terzo stato («semiaperto») serve a un servizio interrogato di continuo,
//! mentre questo lo si interroga a raffiche ogni mezz'ora: la raffica dopo il
//! raffreddamento **è** la richiesta di prova.
//!
//! # Perché il tempo si guarda qui e non nel dominio
//!
//! Perché è tempo. `aether-domain` non ha un orologio — è la sua regola — e una
//! cadenza è fatta di `Instant`. Questo crate ha il diritto di averne uno: parla
//! con la rete, che è fatta di attese.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Quanti guasti di fila aprono l'interruttore.
///
/// Tre e non uno: un guasto isolato capita — un pacchetto perso, un nodo che si
/// riavvia — e chiudere tutto al primo vorrebbe dire rinunciare a una passata
/// per un intoppo che il tentativo dopo non vede nemmeno.
pub const GUASTI_PER_APRIRE: u32 = 3;

/// Per quanto si smette di parlare a un servizio caduto.
pub const RAFFREDDAMENTO: Duration = Duration::from_secs(5 * 60);

/// Il ritmo di MusicBrainz.
///
/// Milleconto millisecondi e non mille: la politica dice «una al secondo in
/// media», e misurare da quando *parte* la richiesta invece che da quando
/// arriva lascia scoperto il tempo di volo. I cento millisecondi di margine
/// sono lo stesso che il vecchio albero si prendeva, e per la stessa ragione.
pub const RITMO_MUSICBRAINZ: Duration = Duration::from_millis(1100);

/// Il ritmo del Cover Art Archive.
pub const RITMO_COVERART: Duration = Duration::from_millis(250);

/// Il ritmo di iTunes Search, che tollera una ventina di richieste al minuto.
pub const RITMO_ITUNES: Duration = Duration::from_millis(350);

/// Il ritmo di Deezer.
pub const RITMO_DEEZER: Duration = Duration::from_millis(220);

/// Quel che la cadenza tiene fra una richiesta e l'altra.
#[derive(Debug)]
struct Stato {
    /// Il primo istante in cui è lecito partire.
    prossima: Instant,
    /// Guasti di trasporto consecutivi.
    guasti: u32,
    /// Quando l'interruttore si richiude; `None` se non è mai stato aperto.
    riapre: Option<Instant>,
}

/// Il cancello di ritmo di un servizio, con il suo interruttore.
#[derive(Debug)]
pub struct Cadenza {
    /// Come si chiama, per i registri e i messaggi d'errore.
    nome: &'static str,
    /// Quanto passa fra l'inizio di una richiesta e quello della successiva.
    intervallo: Duration,
    stato: Mutex<Stato>,
}

impl Cadenza {
    /// Una cadenza per il servizio dato.
    #[must_use]
    pub fn nuova(nome: &'static str, intervallo: Duration) -> Self {
        Self {
            nome,
            intervallo,
            stato: Mutex::new(Stato {
                prossima: Instant::now(),
                guasti: 0,
                riapre: None,
            }),
        }
    }

    /// Come si chiama il servizio.
    #[must_use]
    pub const fn nome(&self) -> &'static str {
        self.nome
    }

    /// Il lucchetto, recuperato se un panico altrove lo ha avvelenato.
    ///
    /// Dietro c'è un istante e due contatori: un panico in un altro filo non
    /// può averli lasciati a metà di niente, e rifiutare ogni richiesta
    /// successiva trasformerebbe un guasto isolato in un arricchimento morto
    /// fino al riavvio. È la stessa scelta, con la stessa ragione, che
    /// `stato::con_libreria` fa sulla connessione al database.
    fn stato(&self) -> std::sync::MutexGuard<'_, Stato> {
        self.stato.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// L'interruttore è aperto: a questo servizio non si parla.
    ///
    /// Richiude da sé quando il raffreddamento è passato, così chi chiama non
    /// deve ricordarsi di riarmarlo.
    pub fn aperta(&self) -> bool {
        let mut stato = self.stato();
        match stato.riapre {
            None => false,
            Some(quando) if Instant::now() >= quando => {
                stato.riapre = None;
                stato.guasti = 0;
                false
            }
            Some(_) => true,
        }
    }

    /// Aspetta il proprio turno.
    ///
    /// Il turno si prenota **sotto** il lucchetto e si dorme **fuori**: due fili
    /// che chiamano insieme si mettono in fila invece di svegliarsi nello stesso
    /// istante, e nessuno dei due tiene il lucchetto mentre dorme. Oggi il filo
    /// è uno solo, ma una struttura di sincronizzazione che funziona *perché*
    /// nessuno la usa in due è una struttura che si romperà senza rumore il
    /// giorno in cui qualcuno aggiunge il secondo filo.
    pub fn attendi(&self) {
        let turno = {
            let mut stato = self.stato();
            let adesso = Instant::now();
            let turno = stato.prossima.max(adesso);
            stato.prossima = turno.checked_add(self.intervallo).unwrap_or(turno);
            turno
        };
        if let Some(quanto) = turno.checked_duration_since(Instant::now()) {
            std::thread::sleep(quanto);
        }
    }

    /// La richiesta è andata: si azzera il conto dei guasti.
    pub fn riuscita(&self) {
        let mut stato = self.stato();
        stato.guasti = 0;
        stato.riapre = None;
    }

    /// La richiesta non è arrivata a destinazione.
    ///
    /// Si chiama per i guasti di **trasporto** e per i `5xx`, non per un `404`:
    /// «non ce l'ho» è una risposta, e contarla come un guasto spegnerebbe
    /// l'interruttore sul servizio che sta funzionando meglio di tutti — quello
    /// che risponde subito di no.
    pub fn guasto(&self) {
        let mut stato = self.stato();
        stato.guasti = stato.guasti.saturating_add(1);
        if stato.guasti >= GUASTI_PER_APRIRE {
            stato.riapre = Instant::now().checked_add(RAFFREDDAMENTO);
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_primo_passaggio_non_aspetta() {
        // La prima richiesta di una passata non deve pagare un intervallo: la
        // cadenza serve a non andare troppo veloce, non a partire in ritardo.
        let cadenza = Cadenza::nuova("prova", Duration::from_millis(50));
        let prima = Instant::now();
        cadenza.attendi();
        assert!(prima.elapsed() < Duration::from_millis(20));
    }

    #[test]
    fn il_secondo_passaggio_aspetta_l_intervallo() {
        let cadenza = Cadenza::nuova("prova", Duration::from_millis(40));
        cadenza.attendi();
        let prima = Instant::now();
        cadenza.attendi();
        assert!(
            prima.elapsed() >= Duration::from_millis(30),
            "ha aspettato {:?}",
            prima.elapsed()
        );
    }

    #[test]
    fn tre_guasti_aprono_l_interruttore() {
        let cadenza = Cadenza::nuova("prova", Duration::from_millis(1));
        assert!(!cadenza.aperta());
        cadenza.guasto();
        cadenza.guasto();
        assert!(!cadenza.aperta(), "due non bastano: un intoppo capita");
        cadenza.guasto();
        assert!(cadenza.aperta());
    }

    #[test]
    fn una_riuscita_in_mezzo_azzera_il_conto() {
        // I guasti che contano sono quelli **consecutivi**: un servizio che
        // risponde una volta su due è lento, non è giù.
        let cadenza = Cadenza::nuova("prova", Duration::from_millis(1));
        cadenza.guasto();
        cadenza.guasto();
        cadenza.riuscita();
        cadenza.guasto();
        cadenza.guasto();
        assert!(!cadenza.aperta());
    }

    #[test]
    fn l_interruttore_si_richiude_da_solo() {
        // Non si prova aspettando cinque minuti: si prova che a scadenza
        // passata l'interruttore si riarma senza che nessuno lo tocchi.
        let cadenza = Cadenza::nuova("prova", Duration::from_millis(1));
        for _ in 0..GUASTI_PER_APRIRE {
            cadenza.guasto();
        }
        assert!(cadenza.aperta());
        // Si sposta la scadenza nel passato, che è l'unica cosa che il tempo
        // avrebbe fatto da sé.
        cadenza.stato().riapre = Instant::now().checked_sub(Duration::from_secs(1));
        assert!(!cadenza.aperta());
        // …e il conto riparte da zero, altrimenti il primo guasto dopo la
        // riapertura richiuderebbe subito.
        cadenza.guasto();
        assert!(!cadenza.aperta());
    }
}
