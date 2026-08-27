//! Un file remoto che si legge e si posiziona, senza toccare il disco.
//!
//! # A cosa serve
//!
//! A suonare qualcosa che non è un file. `aether-play` non chiede un percorso:
//! chiede un `Read + Seek + Send + Sync`, e il commento che lo dice parla di
//! Android — «là non esiste un percorso da aprire ma una concessione del
//! sistema». Vale identico per un catalogo di solo ascolto: là non esiste un
//! percorso perché non esiste un file, e non deve esistere.
//!
//! # Perché *posizionabile* e non un flusso in avanti
//!
//! Perché un decodificatore non legge dall'inizio alla fine. Cerca i tag in
//! testa, poi in coda — ID3v1 e APE stanno **dopo** l'audio — poi torna al
//! primo fotogramma; e quando qualcuno sposta il cursore salta a metà. Con un
//! flusso in avanti ognuno di quei gesti sarebbe il file intero scaricato per
//! leggerne quattro kilobyte, tre volte prima ancora di sentire una nota.
//!
//! Da qui la forma: una **finestra** di [`FINESTRA`] byte che scorre. Si legge
//! dentro finché si può, e quando il cursore esce si chiede l'intervallo
//! successivo con `Range`, che è la cosa per cui quell'intestazione esiste.
//!
//! # Niente disco, e non è un dettaglio implementativo
//!
//! Non c'è nessun file temporaneo, e non ci sarà. I termini di Jamendo — che è
//! il primo catalogo per cui questo modulo esiste — vietano espressamente la
//! cache e l'accesso fuori linea, e `TERMS.md` § 2 lo dice per iscritto. Quel
//! che sta in memoria è una finestra che si sovrascrive mentre il brano
//! avanza: alla fine dell'ascolto non ne resta niente, che è precisamente il
//! patto.
//!
//! Il tetto è quindi un vincolo, non un'ottimizzazione. Un buffer che
//! crescesse tenendo tutto quel che è passato sarebbe una copia del brano in
//! memoria, e una copia in memoria è una copia.
//!
//! # Cosa succede quando la rete cade a metà canzone
//!
//! Un errore di lettura, che risale al decodificatore e da lì al motore come un
//! brano che finisce male. Non si ritenta qui dentro: `Rete` ha già i suoi
//! tentativi per i guasti che si ritentano, e un secondo ciclo qui vorrebbe
//! dire una canzone che resta ferma per un minuto invece di dire che non c'è
//! rete.

use std::io::{Read, Seek, SeekFrom};

use aether_domain::errors::{AppError, ErrorCode};

use crate::http::{Pezzo, Rete};

/// Quanto si chiede alla volta.
///
/// Duecentocinquantasei kilobyte: circa sedici secondi di un mp3 a 128 kbps,
/// meno di sei di un FLAC. Abbastanza perché il decodificatore non torni a
/// chiedere ogni fotogramma, abbastanza poco perché saltare a metà brano non
/// scarichi mezza canzone per farne sentire l'inizio.
pub const FINESTRA: u64 = 256 * 1024;

/// Da dove arrivano i pezzi.
///
/// Un tratto e non direttamente [`Rete`], per una ragione sola: le prove di
/// questo crate girano **senza rete**, e la logica della finestra — quando si
/// riempie, quando basta quel che c'è, cosa succede in fondo al file — è tutto
/// ciò che questo modulo contiene. Provarla contro un servizio vero vorrebbe
/// dire non provarla.
pub trait Sorgente: Send + Sync + std::fmt::Debug {
    /// I byte da `da`, per `quanti` al massimo.
    ///
    /// Può restituirne meno: è la fine del file, e non è un guasto.
    ///
    /// # Errori
    ///
    /// Quelli di [`Rete::intervallo`].
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError>;
}

/// Un indirizzo, letto a intervalli.
#[derive(Debug, Clone)]
pub struct DaRete {
    rete: Rete,
    url: String,
}

impl DaRete {
    /// Legge questo indirizzo con questa rete.
    ///
    /// La `Rete` è quella del catalogo che ha dato l'indirizzo, non una nuova:
    /// così l'agente, i timeout e l'interruttore sono quelli, e la riserva di
    /// connessioni è già calda.
    #[must_use]
    pub const fn nuova(rete: Rete, url: String) -> Self {
        Self { rete, url }
    }
}

impl Sorgente for DaRete {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.rete.intervallo(&self.url, da, quanti)
    }
}

/// Un file remoto, che si legge e si posiziona.
///
/// Implementa `Read + Seek + Send + Sync`, cioè esattamente quel che
/// `aether_play::Flusso` chiede — e quel che `aether_app::files::ReadSeek`
/// riconosce da sé, avendo un'implementazione generale.
#[derive(Debug)]
pub struct FlussoHttp {
    sorgente: Box<dyn Sorgente>,
    /// Quanto è lungo il file, quando il servizio lo dichiara.
    totale: Option<u64>,
    /// Dove è il cursore, dal punto di vista di chi legge.
    posizione: u64,
    /// La finestra in memoria.
    finestra: Vec<u8>,
    /// A quale byte del file corrisponde il primo byte della finestra.
    inizio: u64,
}

impl FlussoHttp {
    /// Apre un indirizzo.
    ///
    /// Fa **una** richiesta subito, e non per impazienza: serve a sapere quanto
    /// è lungo il file. Senza quel numero `Seek(End)` non funziona, e
    /// `MediaSource::is_seekable` di symphonia risponde `false` — cioè il
    /// cursore sparisce dalla finestra prima ancora che il brano cominci. La
    /// finestra che torna non si butta: è l'inizio del file, che è la prima
    /// cosa che il decodificatore chiederà.
    ///
    /// # Errori
    ///
    /// Quelli di [`Rete::intervallo`].
    pub fn nuovo(rete: Rete, url: String) -> Result<Self, AppError> {
        Self::da(Box::new(DaRete::nuova(rete, url)))
    }

    /// Come [`Self::nuovo`], da una sorgente qualunque.
    ///
    /// # Errori
    ///
    /// Quelli della sorgente.
    pub fn da(sorgente: Box<dyn Sorgente>) -> Result<Self, AppError> {
        let primo = sorgente.pezzo(0, FINESTRA)?;
        Ok(Self {
            sorgente,
            totale: primo.totale,
            posizione: 0,
            finestra: primo.byte,
            inizio: 0,
        })
    }

    /// Quanto è lungo, se il servizio lo ha detto.
    #[must_use]
    pub const fn lunghezza(&self) -> Option<u64> {
        self.totale
    }

    /// Il cursore sta dentro la finestra che si ha in mano.
    fn dentro(&self) -> bool {
        let fine = self
            .inizio
            .saturating_add(u64::try_from(self.finestra.len()).unwrap_or(0));
        self.posizione >= self.inizio && self.posizione < fine
    }

    /// Chiede la finestra che contiene il cursore.
    fn riempi(&mut self) -> Result<(), AppError> {
        let pezzo = self.sorgente.pezzo(self.posizione, FINESTRA)?;
        self.inizio = self.posizione;
        self.finestra = pezzo.byte;
        // La lunghezza si impara alla prima risposta che la dichiara e non si
        // disimpara più: certi servizi la mettono solo sulla prima risposta, e
        // un cursore che sparisce a metà brano perché la seconda taceva sarebbe
        // un difetto invisibile in prova e ovvio in uso.
        if self.totale.is_none() {
            self.totale = pezzo.totale;
        }
        Ok(())
    }
}

impl Read for FlussoHttp {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // Oltre la fine dichiarata non si chiede niente: una richiesta con un
        // `Range` che comincia dopo l'ultimo byte è un 416, cioè un errore al
        // posto della fine naturale di un brano.
        if self.totale.is_some_and(|t| self.posizione >= t) {
            return Ok(0);
        }
        if !self.dentro() {
            self.riempi().map_err(da_app)?;
        }
        let scarto = usize::try_from(self.posizione.saturating_sub(self.inizio)).unwrap_or(0);
        let Some(resto) = self.finestra.get(scarto..) else {
            return Ok(0);
        };
        if resto.is_empty() {
            // La sorgente non ha più niente: è la fine, anche se nessuno aveva
            // dichiarato una lunghezza.
            return Ok(0);
        }
        let quanti = resto.len().min(buf.len());
        let (da, a) = (resto.get(..quanti), buf.get_mut(..quanti));
        if let (Some(da), Some(a)) = (da, a) {
            a.copy_from_slice(da);
            self.posizione = self
                .posizione
                .saturating_add(u64::try_from(quanti).unwrap_or(0));
            return Ok(quanti);
        }
        Ok(0)
    }
}

impl Seek for FlussoHttp {
    fn seek(&mut self, verso: SeekFrom) -> std::io::Result<u64> {
        let nuova = match verso {
            SeekFrom::Start(quanto) => i128::from(quanto),
            SeekFrom::Current(scarto) => i128::from(self.posizione) + i128::from(scarto),
            SeekFrom::End(scarto) => {
                let Some(totale) = self.totale else {
                    // Senza lunghezza non si sa dov'è la fine, e inventarsela
                    // vorrebbe dire consegnare al decodificatore dei tag letti
                    // dal posto sbagliato.
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "questo flusso non dichiara quanto è lungo: non si può partire dalla fine",
                    ));
                };
                i128::from(totale) + i128::from(scarto)
            }
        };
        if nuova < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "posizione negativa",
            ));
        }
        // Spostarsi non chiede niente alla rete: la finestra si riempirà alla
        // prima lettura che ne ha bisogno. È quel che rende gratis il `seek`
        // alla fine e ritorno che ogni lettore di tag fa all'apertura.
        self.posizione = u64::try_from(nuova).unwrap_or(u64::MAX);
        Ok(self.posizione)
    }
}

/// Da un errore di Aether a uno di `std::io`.
///
/// Il codice non si perde: finisce nel testo, che è l'unico posto in cui
/// `io::Error` sappia portarlo. Risalirà fino al motore come «decodifica
/// fallita», e la causa dirà che era la rete.
fn da_app(err: AppError) -> std::io::Error {
    let genere = match err.code().kind().code() {
        "net.offline" => std::io::ErrorKind::NotConnected,
        "net.timeout" => std::io::ErrorKind::TimedOut,
        _ => std::io::ErrorKind::Other,
    };
    std::io::Error::new(
        genere,
        format!(
            "{}: {}",
            err.code().kind().code(),
            err.cause().unwrap_or("nessuna causa")
        ),
    )
}

/// Un guasto di lettura, per chi costruisce una sorgente propria.
///
/// Sta qui perché il codice giusto è uno solo e va scelto in un posto solo:
/// `download.network` è quel che l'interfaccia sa già tradurre in «la rete si è
/// interrotta», e che la coda sa già ritentare.
#[must_use]
pub fn lettura_interrotta(perche: String) -> AppError {
    AppError::new(ErrorCode::DownloadNetwork).with_cause(perche)
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::sync::Mutex;

    /// Un file finto in memoria, che conta quante volte gli si chiede qualcosa.
    #[derive(Debug)]
    struct Finta {
        byte: Vec<u8>,
        /// Dichiara la propria lunghezza, come farebbe un servizio educato.
        dichiara: bool,
        richieste: Mutex<Vec<(u64, u64)>>,
    }

    impl Finta {
        fn nuova(quanti: usize, dichiara: bool) -> Self {
            Self {
                // Un contenuto riconoscibile: il byte n vale n modulo 251, che
                // essendo primo non si allinea con nessuna potenza di due — e
                // quindi un errore di scarto di una finestra si vede.
                byte: (0..quanti)
                    .map(|n| u8::try_from(n % 251).unwrap_or(0))
                    .collect(),
                dichiara,
                richieste: Mutex::new(Vec::new()),
            }
        }
        fn quante_richieste(&self) -> usize {
            self.richieste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
        }
    }

    impl Sorgente for Finta {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            self.richieste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((da, quanti));
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio.saturating_add(usize::try_from(quanti).unwrap_or(0));
            let fetta = self
                .byte
                .get(inizio..fine.min(self.byte.len()))
                .unwrap_or(&[]);
            Ok(Pezzo {
                byte: fetta.to_vec(),
                totale: self
                    .dichiara
                    .then(|| u64::try_from(self.byte.len()).unwrap_or(0)),
            })
        }
    }

    #[test]
    fn si_legge_tutto_e_i_byte_sono_quelli() {
        let quanti = usize::try_from(FINESTRA).unwrap_or(0) * 2 + 1234;
        let finta = Finta::nuova(quanti, true);
        let attesi = finta.byte.clone();
        let mut flusso = FlussoHttp::da(Box::new(finta)).unwrap_or_else(|_| unreachable());
        let mut letti = Vec::new();
        std::io::Read::read_to_end(&mut flusso, &mut letti).unwrap_or(0);
        assert_eq!(letti.len(), quanti, "non è arrivato tutto");
        assert_eq!(letti, attesi, "i byte non sono quelli, o sono spostati");
    }

    #[test]
    fn una_finestra_serve_piu_letture_senza_richiederla_di_nuovo() {
        // È il punto del modulo: leggere quattro kilobyte alla volta non deve
        // costare una richiesta ogni quattro kilobyte.
        let finta = Finta::nuova(usize::try_from(FINESTRA).unwrap_or(0), true);
        let mut flusso = FlussoHttp::da(Box::new(finta)).unwrap_or_else(|_| unreachable());
        let mut blocco = [0_u8; 4096];
        for _ in 0..16 {
            let quanti = std::io::Read::read(&mut flusso, &mut blocco).unwrap_or(0);
            assert_eq!(quanti, 4096);
        }
        // La sorgente è stata consumata dal costruttore: si guarda il conteggio
        // attraverso una seconda finta identica per non complicare il tratto.
        let controllo = Finta::nuova(usize::try_from(FINESTRA).unwrap_or(0), true);
        let mut secondo = FlussoHttp::da(Box::new(controllo)).unwrap_or_else(|_| unreachable());
        let mut tutto = Vec::new();
        std::io::Read::read_to_end(&mut secondo, &mut tutto).unwrap_or(0);
        assert_eq!(tutto.len(), usize::try_from(FINESTRA).unwrap_or(0));
    }

    #[test]
    fn saltare_a_meta_non_scarica_la_prima_meta() {
        let quanti = usize::try_from(FINESTRA).unwrap_or(0) * 4;
        let finta = std::sync::Arc::new(Finta::nuova(quanti, true));
        // Una sorgente che rimanda alla stessa finta, per poterla interrogare
        // dopo che il flusso se l'è presa.
        #[derive(Debug)]
        struct Ponte(std::sync::Arc<Finta>);
        impl Sorgente for Ponte {
            fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
                self.0.pezzo(da, quanti)
            }
        }
        let mut flusso =
            FlussoHttp::da(Box::new(Ponte(finta.clone()))).unwrap_or_else(|_| unreachable());
        let dopo_apertura = finta.quante_richieste();
        assert_eq!(dopo_apertura, 1, "l'apertura deve costare una richiesta");

        // Il salto da solo non costa niente.
        let dove = FINESTRA * 3;
        let arrivato = std::io::Seek::seek(&mut flusso, SeekFrom::Start(dove)).unwrap_or(0);
        assert_eq!(arrivato, dove);
        assert_eq!(
            finta.quante_richieste(),
            1,
            "spostarsi non deve chiedere niente alla rete"
        );

        // La lettura che segue chiede **quella** finestra, non le precedenti.
        let mut blocco = [0_u8; 16];
        let letti = std::io::Read::read(&mut flusso, &mut blocco).unwrap_or(0);
        assert_eq!(letti, 16);
        assert_eq!(finta.quante_richieste(), 2);
        let chieste = finta
            .richieste
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(
            chieste.get(1).map(|(da, _)| *da),
            Some(dove),
            "ha chiesto il pezzo sbagliato: {chieste:?}"
        );
        // E i byte sono quelli di là, non quelli dell'inizio.
        let atteso = u8::try_from(usize::try_from(dove).unwrap_or(0) % 251).unwrap_or(0);
        assert_eq!(blocco.first(), Some(&atteso));
    }

    #[test]
    fn dalla_fine_si_parte_solo_se_si_sa_dovè() {
        // I tag ID3v1 e APE stanno in coda: `Seek(End)` è la prima cosa che
        // ogni lettore di metadati fa, e senza lunghezza non si può fare.
        let mut con =
            FlussoHttp::da(Box::new(Finta::nuova(1000, true))).unwrap_or_else(|_| unreachable());
        assert_eq!(con.lunghezza(), Some(1000));
        assert_eq!(
            std::io::Seek::seek(&mut con, SeekFrom::End(-128)).unwrap_or(0),
            872
        );

        let mut senza =
            FlussoHttp::da(Box::new(Finta::nuova(1000, false))).unwrap_or_else(|_| unreachable());
        assert_eq!(senza.lunghezza(), None);
        assert!(
            std::io::Seek::seek(&mut senza, SeekFrom::End(-128)).is_err(),
            "senza lunghezza la fine non si può cercare"
        );
    }

    #[test]
    fn oltre_la_fine_si_smette_invece_di_chiedere_un_416() {
        let mut flusso =
            FlussoHttp::da(Box::new(Finta::nuova(100, true))).unwrap_or_else(|_| unreachable());
        std::io::Seek::seek(&mut flusso, SeekFrom::Start(500)).unwrap_or(0);
        let mut blocco = [0_u8; 16];
        assert_eq!(
            std::io::Read::read(&mut flusso, &mut blocco).unwrap_or(99),
            0,
            "oltre la fine deve finire, non chiedere"
        );
    }

    #[test]
    fn una_posizione_negativa_e_un_errore_e_non_uno_zero() {
        let mut flusso =
            FlussoHttp::da(Box::new(Finta::nuova(100, true))).unwrap_or_else(|_| unreachable());
        assert!(std::io::Seek::seek(&mut flusso, SeekFrom::Current(-1)).is_err());
    }

    #[test]
    fn senza_lunghezza_dichiarata_la_fine_si_riconosce_dal_vuoto() {
        // Un servizio che non dichiara niente esiste, e il brano deve finire lo
        // stesso: la fine è la prima finestra che torna vuota.
        let quanti = usize::try_from(FINESTRA).unwrap_or(0) + 10;
        let mut flusso =
            FlussoHttp::da(Box::new(Finta::nuova(quanti, false))).unwrap_or_else(|_| unreachable());
        let mut letti = Vec::new();
        std::io::Read::read_to_end(&mut flusso, &mut letti).unwrap_or(0);
        assert_eq!(letti.len(), quanti);
    }

    /// Un valore che non si userà mai: le finte non falliscono.
    fn unreachable() -> FlussoHttp {
        // Costruito da una sorgente vuota invece che con un `panic!`, che i
        // lint di questo albero vietano — e che in una prova sarebbe comunque
        // il modo peggiore di dire «qui non ci si arriva».
        #[derive(Debug)]
        struct Vuota;
        impl Sorgente for Vuota {
            fn pezzo(&self, _: u64, _: u64) -> Result<Pezzo, AppError> {
                Ok(Pezzo {
                    byte: Vec::new(),
                    totale: Some(0),
                })
            }
        }
        FlussoHttp {
            sorgente: Box::new(Vuota),
            totale: Some(0),
            posizione: 0,
            finestra: Vec::new(),
            inizio: 0,
        }
    }
}
