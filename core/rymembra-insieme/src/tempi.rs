//! I tempi del trasporto, tutti in un posto.

use std::time::Duration;

/// I tempi del trasporto.
///
/// Quelli di serie ([`Tempi::default`]) sono quelli del protocollo, e l'altro
/// capo li dà per scontati: un battito più rado di [`Tempi::silenzio`] farebbe
/// chiudere la connessione dall'altra parte. Si cambiano solo nelle prove, con
/// [`crate::Rete::avvia_con`], per non aspettare quindici secondi di silenzio
/// veri — e in una prova i due capi sono tutti e due nostri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tempi {
    /// Ogni quanto si manda un battito, se non si è mandato altro: 5 s.
    pub battito: Duration,
    /// Quanto tempo senza una cornice intera chiude la connessione: 15 s.
    ///
    /// Vale anche come scadenza di ogni scrittura: chi non legge per quindici
    /// secondi è un capo che non c'è più, anche se il cavo è ancora attaccato.
    pub silenzio: Duration,
    /// La stretta Noise più il saluto, dal primo byte: 5 s.
    pub stretta: Duration,
    /// Il tentativo di connessione TCP: 3 s. Su una LAN un host che c'è
    /// risponde in millisecondi; tre secondi coprono un Wi-Fi che si sveglia.
    pub collegamento: Duration,
    /// Un'associazione, dall'impegno all'ultima conferma: 2 min. Abbastanza
    /// per alzarsi e andare a guardare l'altro schermo; poi vale come fallita,
    /// e la connessione si chiude con `insieme.associazioneScaduta`.
    pub associazione: Duration,
    /// I ping d'orologio nei primi [`Tempi::orologio_avvio`]: ogni 250 ms.
    pub ping_svelto: Duration,
    /// Poi: ogni secondo.
    pub ping_lento: Duration,
    /// Quanto dura la cadenza svelta dopo `Rete::sincronizza`: 10 s.
    ///
    /// All'inizio lo stimatore non sa niente e ogni campione vale molto; a
    /// regime la deriva fra due quarzi è di qualche ppm, e un campione al
    /// secondo basta a seguirla.
    pub orologio_avvio: Duration,
    /// Quanto aspetta l'host i byte che chiede al proprietario di un gettone
    /// inoltrato (fase 2): 10 s.
    pub inoltro: Duration,
    /// Quanto vive sull'host una connessione di una chiave non fidata che non
    /// si sta associando: 2 min. Il tempo di vedere chi ospita e decidere; poi
    /// si chiude con `insieme.estraneoScaduto`, e chi vuole si ricollega.
    /// Un'associazione in corso la tiene aperta; il battito da solo no: senza
    /// questo tetto, otto estranei che battono e basta terrebbero occupati per
    /// sempre i posti di chi vuole associarsi.
    pub vita_estraneo: Duration,
    /// Quanto aspetta chi chiude che l'altro capo chiuda la sua metà: 250 ms.
    ///
    /// È il tempo che serve perché il `Chiudo` arrivi davvero: chiudere il
    /// socket con dati non letti dall'altra parte fa partire un reset, e il
    /// reset può cancellare proprio il messaggio che diceva perché.
    pub congedo: Duration,
}

impl Default for Tempi {
    fn default() -> Self {
        Self {
            battito: Duration::from_secs(5),
            silenzio: Duration::from_secs(15),
            stretta: Duration::from_secs(5),
            collegamento: Duration::from_secs(3),
            associazione: Duration::from_secs(120),
            ping_svelto: Duration::from_millis(250),
            ping_lento: Duration::from_secs(1),
            orologio_avvio: Duration::from_secs(10),
            inoltro: Duration::from_secs(10),
            vita_estraneo: Duration::from_secs(120),
            congedo: Duration::from_millis(250),
        }
    }
}
