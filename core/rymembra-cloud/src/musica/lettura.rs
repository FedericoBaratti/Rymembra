//! Il lettore dei tag di un file del cloud: pochi byte, contati, con un tetto.
//!
//! # Perché non il flusso della riproduzione
//!
//! Perché le domande sono diverse. Chi suona legge il brano intero, e il flusso
//! gli chiede finestre da 256 KiB che crescono fino a due megabyte. Chi legge i
//! tag vuole la testa (ID3v2, i blocchi FLAC, l'atomo `moov`), a volte la coda
//! (ID3v1, APE) e qualche salto in mezzo; con le finestre del flusso, una
//! scansione da ventimila brani porterebbe giù gigabyte per leggere titoli.
//!
//! Quindi finestre piccole e mirate:
//!
//! - **64 KiB in testa**, alla prima lettura: bastano ai tag di quasi tutti i
//!   file, copertina piccola compresa;
//! - **16 KiB in coda**, quando si legge negli ultimi 16 KiB: ID3v1 e APE;
//! - **altrove**, quanto serve alla lettura arrotondato a 32 KiB: è il caso di
//!   un ID3v2 con una copertina grande, letto di fila, o di un salto all'atomo
//!   `moov` in fondo a un M4A.
//!
//! # Il tetto, e il segnale
//!
//! Ogni byte arrivato si conta ([`LettoreTag::letti`]): la misura di
//! accettazione è la mediana dei byte per brano. Oltre [`TETTO_LETTURA`] la
//! lettura si ferma con un `std::io::Error` di tipo
//! [`std::io::ErrorKind::FileTooLarge`]: è il segnale che chi legge i tag
//! riconosce, e a cui risponde rileggendo senza le proprietà audio invece di
//! contarlo come un file rotto. Un file che chiede più di otto megabyte per
//! dire il suo titolo (una copertina enorme, un MP3 senza Xing letto per
//! calcolarne la durata) lo dice così, e la durata arriva al primo ascolto.
//!
//! # Niente disco
//!
//! I pezzi stanno in memoria finché il lettore vive — al più il tetto — e se
//! ne vanno con lui. La cache su disco è della riproduzione: ventimila teste
//! di file la riempirebbero di cose che nessuno ascolterà.
//!
//! # Gli errori della rete
//!
//! Un guasto della sorgente diventa un `std::io::Error` che porta **dentro**
//! l'[`AppError`] intero: [`errore_remoto`] lo ritrova. Non un numero di
//! sistema come fa il flusso: chi legge i tag deve poter distinguere un `429`
//! (aspetta, non è un guasto) da una rete caduta da un file illeggibile, e un
//! numero quella differenza la perde.

use std::io::{Read, Seek, SeekFrom};

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_net::flusso::Sorgente;

/// Quanti byte, al più, legge un lettore dei tag.
pub const TETTO_LETTURA: u64 = 8 * 1024 * 1024;

/// La finestra della testa.
const TESTA: u64 = 64 * 1024;

/// La finestra della coda.
const CODA: u64 = 16 * 1024;

/// Il passo a cui si arrotonda una lettura altrove.
const PASSO: u64 = 32 * 1024;

/// Un file del cloud letto a finestre piccole, per i tag. Vedi il `//!`.
#[derive(Debug)]
pub struct LettoreTag {
    sorgente: Box<dyn Sorgente>,
    /// Quanto è lungo il file, se si sa: dall'elenco, o dalla prima risposta.
    totale: Option<u64>,
    /// Dove è il cursore.
    posizione: u64,
    /// I pezzi arrivati: da dove cominciano, e i byte.
    pezzi: Vec<(u64, Vec<u8>)>,
    /// I byte arrivati finora.
    letti: u64,
}

impl LettoreTag {
    /// Legge `sorgente`, lunga `dimensione` se la si sa già (dall'elenco).
    ///
    /// Non chiede niente alla rete: la prima richiesta parte alla prima
    /// lettura.
    #[must_use]
    pub fn nuovo(sorgente: Box<dyn Sorgente>, dimensione: Option<u64>) -> Self {
        Self {
            sorgente,
            totale: dimensione,
            posizione: 0,
            pezzi: Vec::new(),
            letti: 0,
        }
    }

    /// I byte arrivati dalla rete finora.
    #[must_use]
    pub const fn letti(&self) -> u64 {
        self.letti
    }

    /// Copia in `buf` quel che un pezzo già arrivato ha alla posizione del
    /// cursore. `None` se nessun pezzo la copre.
    fn dai_pezzi(&self, buf: &mut [u8]) -> Option<usize> {
        let (inizio, byte) = self.pezzi.iter().find(|(inizio, byte)| {
            let fine = inizio.saturating_add(u64::try_from(byte.len()).unwrap_or(0));
            *inizio <= self.posizione && self.posizione < fine
        })?;
        let scarto = usize::try_from(self.posizione - inizio).ok()?;
        let resto = byte.get(scarto..)?;
        let quanti = resto.len().min(buf.len());
        buf.get_mut(..quanti)?.copy_from_slice(resto.get(..quanti)?);
        Some(quanti)
    }

    /// Il primo byte da `da` in poi che nessun pezzo copre, senza superare
    /// `fino`: una finestra non richiede quel che si ha già.
    fn primo_scoperto(&self, mut da: u64, fino: u64) -> u64 {
        while da < fino {
            let coperto = self.pezzi.iter().find_map(|(inizio, byte)| {
                let fine = inizio.saturating_add(u64::try_from(byte.len()).unwrap_or(0));
                (*inizio <= da && da < fine).then_some(fine)
            });
            match coperto {
                Some(fine) => da = fine,
                None => break,
            }
        }
        da.min(fino)
    }

    /// Quale finestra chiedere per leggere `voglio` byte dal cursore.
    fn finestra(&self, voglio: usize) -> (u64, u64) {
        let voglio = u64::try_from(voglio).unwrap_or(u64::MAX).max(1);
        let posizione = self.posizione;
        let (da, quanti) = if posizione < TESTA {
            let da = self.primo_scoperto(0, posizione);
            (da, TESTA.saturating_sub(da).max(voglio))
        } else if let Some(totale) = self.totale
            && posizione >= totale.saturating_sub(CODA)
        {
            let da = self.primo_scoperto(totale.saturating_sub(CODA), posizione);
            (da, totale.saturating_sub(da))
        } else {
            // Arrotondato in su al passo: `PASSO` è una potenza di due, e la
            // maschera è il resto della divisione senza dividere.
            let arrotondato = voglio.saturating_add(PASSO - 1) & !(PASSO - 1);
            (posizione, arrotondato)
        };
        let quanti = match self.totale {
            Some(totale) => quanti.min(totale.saturating_sub(da)),
            None => quanti,
        };
        (da, quanti)
    }

    /// Chiede una finestra alla sorgente, dentro il tetto.
    fn chiedi(&mut self, da: u64, quanti: u64) -> std::io::Result<()> {
        if self.letti.saturating_add(quanti) > TETTO_LETTURA {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                format!(
                    "i tag chiedono più di {} MiB: letti {} byte, ne servirebbero altri {quanti}",
                    TETTO_LETTURA >> 20,
                    self.letti
                ),
            ));
        }
        let pezzo = self.sorgente.pezzo(da, quanti).map_err(in_io)?;
        self.letti = self
            .letti
            .saturating_add(u64::try_from(pezzo.byte.len()).unwrap_or(u64::MAX));
        if self.totale.is_none() {
            self.totale = pezzo.totale;
        }
        if !pezzo.byte.is_empty() {
            self.pezzi.push((da, pezzo.byte));
        }
        Ok(())
    }
}

impl Read for LettoreTag {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() || self.totale.is_some_and(|t| self.posizione >= t) {
            return Ok(0);
        }
        if let Some(quanti) = self.dai_pezzi(buf) {
            self.posizione = self
                .posizione
                .saturating_add(u64::try_from(quanti).unwrap_or(0));
            return Ok(quanti);
        }
        let (da, quanti) = self.finestra(buf.len());
        if quanti == 0 {
            return Ok(0);
        }
        self.chiedi(da, quanti)?;
        // Una finestra arrivata vuota, o più corta del cursore, è la fine del
        // file anche se nessuno l'aveva dichiarata.
        let quanti = self.dai_pezzi(buf).unwrap_or(0);
        self.posizione = self
            .posizione
            .saturating_add(u64::try_from(quanti).unwrap_or(0));
        Ok(quanti)
    }
}

impl Seek for LettoreTag {
    fn seek(&mut self, verso: SeekFrom) -> std::io::Result<u64> {
        let nuova = match verso {
            SeekFrom::Start(quanto) => i128::from(quanto),
            SeekFrom::Current(scarto) => i128::from(self.posizione) + i128::from(scarto),
            SeekFrom::End(scarto) => {
                // Senza lunghezza la si impara dalla testa, che serve comunque.
                if self.totale.is_none() {
                    let (da, quanti) = (self.primo_scoperto(0, TESTA), TESTA);
                    if da < TESTA {
                        self.chiedi(da, quanti.saturating_sub(da))?;
                    }
                }
                let Some(totale) = self.totale else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "il servizio non dichiara quanto è lungo il file: non si parte dalla fine",
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
        // Spostarsi non chiede niente: la finestra arriva alla lettura.
        self.posizione = u64::try_from(nuova).unwrap_or(u64::MAX);
        Ok(self.posizione)
    }
}

/// Un guasto della sorgente, come `std::io::Error` che porta dentro l'errore
/// intero. Il genere dice quel che si può dire senza guardarci dentro.
fn in_io(err: AppError) -> std::io::Error {
    let genere = match err.code() {
        ErrorCode::NetTimeout { .. } => std::io::ErrorKind::TimedOut,
        ErrorCode::NetOffline { .. } => std::io::ErrorKind::NotConnected,
        _ => std::io::ErrorKind::Other,
    };
    std::io::Error::new(genere, err)
}

/// L'errore della sorgente dentro un `std::io::Error` di un [`LettoreTag`].
///
/// `None` per un errore che non viene dalla sorgente: il tetto
/// ([`std::io::ErrorKind::FileTooLarge`]), un salto impossibile, o un errore di
/// qualcun altro. Serve a chi legge i tag per distinguere quel che passa da sé
/// (`net.*`, `cloud.quotaExceeded`) da un file che davvero non si legge.
#[must_use]
pub fn errore_remoto(err: &std::io::Error) -> Option<&AppError> {
    err.get_ref()?.downcast_ref::<AppError>()
}

#[cfg(test)]
mod prove {
    use std::sync::{Arc, Mutex};

    use rymembra_net::http::Pezzo;

    use super::*;

    /// Le richieste che una sorgente finta ha ricevuto: da dove, quanti.
    type Richieste = Arc<Mutex<Vec<(u64, u64)>>>;

    /// Un file in memoria che ricorda ogni richiesta.
    #[derive(Debug)]
    struct Finta {
        byte: Vec<u8>,
        dichiara: bool,
        richieste: Richieste,
        guasto: Option<ErrorCode>,
    }

    impl Sorgente for Finta {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            self.richieste.lock().unwrap().push((da, quanti));
            if let Some(codice) = &self.guasto {
                return Err(AppError::new(codice.clone()));
            }
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap())
                .min(self.byte.len());
            Ok(Pezzo {
                byte: self.byte.get(inizio..fine).unwrap_or(&[]).to_vec(),
                totale: self
                    .dichiara
                    .then(|| u64::try_from(self.byte.len()).unwrap()),
                url_finale: None,
            })
        }
    }

    /// Un lettore su un file finto di `quanti` byte, e le sue richieste.
    fn lettore(quanti: usize, dimensione: Option<u64>, dichiara: bool) -> (LettoreTag, Richieste) {
        let richieste = Arc::new(Mutex::new(Vec::new()));
        let finta = Finta {
            byte: (0..quanti)
                .map(|n| u8::try_from(n % 251).unwrap())
                .collect(),
            dichiara,
            richieste: Arc::clone(&richieste),
            guasto: None,
        };
        (LettoreTag::nuovo(Box::new(finta), dimensione), richieste)
    }

    fn byte_atteso(n: u64) -> u8 {
        u8::try_from(n % 251).unwrap()
    }

    #[test]
    fn la_testa_si_chiede_una_volta_e_serve_tante_letture() {
        let (mut lettore, richieste) = lettore(5_000_000, Some(5_000_000), true);
        let mut buf = [0_u8; 10];
        lettore.read_exact(&mut buf).unwrap();
        assert_eq!(buf[3], byte_atteso(3));
        lettore.seek(SeekFrom::Start(60_000)).unwrap();
        let mut altro = [0_u8; 100];
        lettore.read_exact(&mut altro).unwrap();
        assert_eq!(altro[0], byte_atteso(60_000));
        assert_eq!(*richieste.lock().unwrap(), vec![(0, 64 * 1024)]);
        assert_eq!(lettore.letti(), 64 * 1024);
    }

    #[test]
    fn la_coda_chiede_sedici_kib() {
        let (mut lettore, richieste) = lettore(5_000_000, Some(5_000_000), true);
        lettore.seek(SeekFrom::End(-128)).unwrap();
        let mut buf = [0_u8; 128];
        lettore.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], byte_atteso(5_000_000 - 128));
        assert_eq!(
            *richieste.lock().unwrap(),
            vec![(5_000_000 - 16 * 1024, 16 * 1024)],
            "la coda e basta: il salto alla fine non costa niente da sé"
        );
    }

    #[test]
    fn un_salto_lontano_chiede_il_minimo_arrotondato() {
        let (mut lettore, richieste) = lettore(5_000_000, Some(5_000_000), true);
        lettore.seek(SeekFrom::Start(1_000_000)).unwrap();
        let mut piccolo = [0_u8; 100];
        lettore.read_exact(&mut piccolo).unwrap();
        assert_eq!(piccolo[0], byte_atteso(1_000_000));
        lettore.seek(SeekFrom::Start(2_000_000)).unwrap();
        let mut grande = vec![0_u8; 40_000];
        lettore.read_exact(&mut grande).unwrap();
        assert_eq!(grande[39_999], byte_atteso(2_039_999));
        assert_eq!(
            *richieste.lock().unwrap(),
            vec![(1_000_000, 32 * 1024), (2_000_000, 64 * 1024)]
        );
    }

    #[test]
    fn una_testa_grande_si_legge_di_fila_a_passi() {
        // Un ID3v2 da 300 KiB con la copertina, letto come lo legge un lettore
        // di tag: i dieci byte dell'intestazione, poi il tag intero.
        let (mut lettore, richieste) = lettore(5_000_000, Some(5_000_000), true);
        let mut intestazione = [0_u8; 10];
        lettore.read_exact(&mut intestazione).unwrap();
        let mut testa = vec![0_u8; 300 * 1024];
        lettore.read_exact(&mut testa).unwrap();
        assert_eq!(testa[300 * 1024 - 1], byte_atteso(300 * 1024 + 9));
        let richieste = richieste.lock().unwrap().clone();
        assert_eq!(richieste.first(), Some(&(0, 64 * 1024)));
        assert!(
            richieste.iter().skip(1).all(|(_, q)| q % (32 * 1024) == 0),
            "{richieste:?}"
        );
        let in_tutto: u64 = richieste.iter().map(|(_, q)| q).sum();
        assert!(in_tutto < 600 * 1024, "letti {in_tutto} per 300 KiB");
    }

    #[test]
    fn oltre_il_tetto_il_segnale_e_file_too_large() {
        let (mut lettore, _) = lettore(20 * 1024 * 1024, Some(20 * 1024 * 1024), true);
        let mut blocco = vec![0_u8; 64 * 1024];
        let err = loop {
            match lettore.read(&mut blocco) {
                Ok(0) => panic!("il file è più lungo del tetto"),
                Ok(_) => {}
                Err(err) => break err,
            }
        };
        assert_eq!(err.kind(), std::io::ErrorKind::FileTooLarge);
        assert!(errore_remoto(&err).is_none(), "non è un guasto della rete");
        assert!(lettore.letti() <= TETTO_LETTURA);
        assert!(
            lettore.letti() > TETTO_LETTURA - 64 * 1024,
            "il tetto si raggiunge, non si sfiora: {}",
            lettore.letti()
        );
    }

    #[test]
    fn senza_dimensione_la_si_impara_dalla_prima_risposta() {
        let (mut lettore, richieste) = lettore(200_000, None, true);
        let fine = lettore.seek(SeekFrom::End(-10)).unwrap();
        assert_eq!(fine, 199_990);
        let mut buf = [0_u8; 10];
        lettore.read_exact(&mut buf).unwrap();
        assert_eq!(buf[9], byte_atteso(199_999));
        let richieste = richieste.lock().unwrap().clone();
        assert_eq!(
            richieste.first(),
            Some(&(0, 64 * 1024)),
            "la testa insegna la lunghezza"
        );
        assert_eq!(richieste.get(1), Some(&(200_000 - 16 * 1024, 16 * 1024)));
    }

    #[test]
    fn un_file_piccolo_si_legge_tutto_con_una_richiesta() {
        let (mut lettore, richieste) = lettore(1000, Some(1000), true);
        let mut tutto = Vec::new();
        lettore.read_to_end(&mut tutto).unwrap();
        assert_eq!(tutto.len(), 1000);
        assert_eq!(*richieste.lock().unwrap(), vec![(0, 1000)]);
        // Anche la coda è già in mano.
        lettore.seek(SeekFrom::End(-128)).unwrap();
        let mut coda = [0_u8; 128];
        lettore.read_exact(&mut coda).unwrap();
        assert_eq!(richieste.lock().unwrap().len(), 1);
    }

    #[test]
    fn un_guasto_della_rete_si_ritrova_intero() {
        let richieste = Arc::new(Mutex::new(Vec::new()));
        let finta = Finta {
            byte: Vec::new(),
            dichiara: true,
            richieste,
            guasto: Some(ErrorCode::NetRateLimited {
                service: Some("onedrive".to_owned()),
                retry_after_ms: Some(5000),
            }),
        };
        let mut lettore = LettoreTag::nuovo(Box::new(finta), Some(1000));
        let mut buf = [0_u8; 4];
        let err = lettore.read(&mut buf).unwrap_err();
        let dentro = errore_remoto(&err).expect("l'errore della sorgente");
        assert_eq!(dentro.code().kind().code(), "net.rateLimited");
        assert!(
            matches!(
                dentro.code(),
                ErrorCode::NetRateLimited {
                    retry_after_ms: Some(5000),
                    ..
                }
            ),
            "il tempo chiesto dal servizio arriva fino a chi legge"
        );
        assert_ne!(err.kind(), std::io::ErrorKind::FileTooLarge);
        assert_ne!(
            err.kind(),
            std::io::ErrorKind::Interrupted,
            "un `Interrupted` farebbe ripetere la lettura all'infinito a `read_exact`"
        );
    }

    #[test]
    fn un_lettore_dei_tag_va_fra_i_fili() {
        fn richiede<T: Read + Seek + Send + Sync>() {}
        richiede::<LettoreTag>();
    }
}
