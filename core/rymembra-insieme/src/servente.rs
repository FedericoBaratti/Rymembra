//! Il servente: gettone → byte.
//!
//! # Da dove vengono i percorsi
//!
//! **Solo** dal desktop di questo PC, che li prende dalla propria libreria
//! ([`crate::Rete::servi`]). Un messaggio dalla rete porta un gettone — sedici
//! byte a caso — e nient'altro: non c'è modo di chiedere un percorso, e quindi
//! non c'è modo di chiederne uno che non sia in coda.
//!
//! # Il file di adesso, non quello di prima
//!
//! Ogni lettura riapre il file e ricontrolla dimensione e data, prima e dopo
//! aver letto. Un file riscritto fra due letture (un tag corretto, un brano
//! riconvertito) non si legge a pezzi di due versioni: il decodificatore
//! dell'altro PC non saprebbe distinguerle, e suonerebbe rumore. Si rifiuta con
//! `insieme.fileCambiato`.
//!
//! # La revoca
//!
//! Ogni voce ha una generazione. Una lettura la annota quando trova il
//! gettone, e prima di rispondere controlla che il gettone ci sia ancora con
//! la **stessa** generazione: revocato nel frattempo — o revocato e ridato a un
//! altro file — la lettura in corso finisce con `insieme.gettoneScaduto`
//! invece di consegnare byte di un permesso che non c'è più.

use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::io::{Read as _, Seek as _, SeekFrom};
use std::sync::Arc;

use rymembra_domain::errors::ErrorCodeKind;

use crate::identita::ChiavePubblica;
use crate::messaggi::Gettone;
use crate::rete::FileServito;

/// Il tetto di una lettura: due mebibyte.
///
/// È la finestra più grande di `rymembra_net::flusso` (`FINESTRA_MASSIMA`):
/// un `LettoreRemoto` sotto un `FlussoHttp` non chiede mai di più, e chi
/// chiede di più per conto suo viene servito a finestre (vedi
/// `cliente::a_finestre`).
pub(crate) const LETTURA_MASSIMA: u64 = 2 * 1024 * 1024;

/// Il tetto di una copertina servita: un mebibyte, la JPEG «piena»
/// dell'archivio delle copertine con un margine largo.
pub(crate) const COPERTINA_MASSIMA: usize = 1024 * 1024;

/// Che cosa c'è dietro un gettone.
#[derive(Debug, Clone)]
pub(crate) enum Servito {
    /// Un file di questo PC.
    File(FileServito),
    /// Una copertina, già in memoria.
    Copertina(Arc<[u8]>),
    /// Fase 2, sull'host: i byte li ha il membro con questa chiave.
    Inoltrato(ChiavePubblica),
}

/// Che cosa si chiede a un gettone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chiesto {
    /// Un tratto di byte.
    Byte {
        /// Da dove.
        da: u64,
        /// Quanti, al più [`LETTURA_MASSIMA`].
        quanti: u64,
    },
    /// La copertina intera.
    Copertina,
}

/// La risposta a una lettura.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Risposta {
    /// I byte, e la lunghezza di tutto il file (di tutta la copertina).
    Byte {
        /// La lunghezza di tutto.
        totale: u64,
        /// I byte chiesti; meno di quelli chiesti alla fine del file.
        byte: Vec<u8>,
    },
    /// Un rifiuto, col suo codice.
    Rifiutata(ErrorCodeKind),
}

/// I gettoni che questo PC serve.
#[derive(Debug, Default)]
pub(crate) struct Servente {
    voci: HashMap<Gettone, (u64, Servito)>,
    /// La prossima generazione.
    prossima: u64,
}

impl Servente {
    /// Mette (o rimette) un gettone, con una generazione nuova.
    pub(crate) fn metti(&mut self, gettone: Gettone, servito: Servito) {
        self.prossima = self.prossima.wrapping_add(1);
        self.voci.insert(gettone, (self.prossima, servito));
    }

    /// Fase 2: i byte di questo gettone li ha quel membro. Solo se il gettone
    /// è libero, o se è già inoltrato allo stesso membro (e allora non cambia
    /// niente, generazione compresa). `false` se era in uso.
    ///
    /// Mai sopra un altro gettone: il gettone di un'aggiunta lo sceglie il
    /// membro, e fino alla revisione del 26 ottobre un membro poteva
    /// annunciare come suo il gettone di un brano dell'host (lo vede in ogni
    /// stato della sessione) o di un altro membro, e l'host da lì in poi
    /// avrebbe chiesto quei byte a lui: chi ascoltava quel brano avrebbe
    /// sentito quel che voleva il membro.
    pub(crate) fn inoltra(&mut self, gettone: Gettone, proprietario: ChiavePubblica) -> bool {
        match self.voci.get(&gettone) {
            Some((_, Servito::Inoltrato(suo))) => *suo == proprietario,
            Some(_) => false,
            None => {
                self.metti(gettone, Servito::Inoltrato(proprietario));
                true
            }
        }
    }

    /// Revoca un gettone.
    pub(crate) fn togli(&mut self, gettone: &Gettone) {
        self.voci.remove(gettone);
    }

    /// Revoca tutto.
    pub(crate) fn svuota(&mut self) {
        self.voci.clear();
    }

    /// Che cosa c'è dietro un gettone, con la sua generazione. Una copia:
    /// la lettura vera si fa fuori dal lucchetto.
    pub(crate) fn cerca(&self, gettone: &Gettone) -> Option<(u64, Servito)> {
        self.voci.get(gettone).cloned()
    }

    /// La generazione di un gettone, se c'è ancora.
    pub(crate) fn generazione(&self, gettone: &Gettone) -> Option<u64> {
        self.voci.get(gettone).map(|(generazione, _)| *generazione)
    }
}

/// La data di modifica in millisecondi dall'epoca Unix, troncata.
///
/// La stessa forma di `rymembra_app::files`, che è quella con cui la libreria
/// la salva: un confronto fra un valore troncato e uno coi decimali vedrebbe
/// cambiato ogni file. Un orologio che non si legge dà 0, come lì.
pub(crate) fn modificato_ms(metadati: &Metadata) -> i64 {
    metadati
        .modified()
        .ok()
        .and_then(|quando| quando.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|durata| i64::try_from(durata.as_millis()).ok())
        .unwrap_or(0)
}

/// I byte `da..da+quanti` di un file servito, e la sua lunghezza.
///
/// # Errori
///
/// `insieme.fileCambiato` se il file non c'è più o non è più quello di
/// quando il gettone è stato dato; `insieme.irraggiungibile` se il disco non
/// risponde (un NAS staccato): per chi legge dall'altro PC il file è
/// irraggiungibile, e il rimedio non è riprovare a leggerlo da qui.
pub(crate) fn leggi_file(
    file: &FileServito,
    da: u64,
    quanti: u64,
) -> Result<(Vec<u8>, u64), ErrorCodeKind> {
    let guasto = |err: std::io::Error| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ErrorCodeKind::InsiemeFileCambiato
        } else {
            ErrorCodeKind::InsiemeIrraggiungibile
        }
    };
    let quello = |metadati: &Metadata| {
        metadati.len() == file.dimensione && modificato_ms(metadati) == file.modificato_ms
    };
    let mut aperto = File::open(&file.percorso).map_err(guasto)?;
    if !quello(&aperto.metadata().map_err(guasto)?) {
        return Err(ErrorCodeKind::InsiemeFileCambiato);
    }
    let totale = file.dimensione;
    if da >= totale {
        return Ok((Vec::new(), totale));
    }
    let quanti = quanti.min(LETTURA_MASSIMA).min(totale.saturating_sub(da));
    let mut byte =
        vec![0_u8; usize::try_from(quanti).map_err(|_| ErrorCodeKind::InsiemeProtocollo)?];
    aperto.seek(SeekFrom::Start(da)).map_err(guasto)?;
    // Un file più corto di quel che dichiarava: è cambiato mentre lo si
    // leggeva, ed è lo stesso caso del controllo qui sotto.
    aperto.read_exact(&mut byte).map_err(|err| {
        if err.kind() == std::io::ErrorKind::UnexpectedEof {
            ErrorCodeKind::InsiemeFileCambiato
        } else {
            guasto(err)
        }
    })?;
    if !quello(&aperto.metadata().map_err(guasto)?) {
        return Err(ErrorCodeKind::InsiemeFileCambiato);
    }
    Ok((byte, totale))
}

/// Un tratto di una copertina in memoria, o tutta.
pub(crate) fn leggi_copertina(byte: &[u8], chiesto: Chiesto) -> Risposta {
    let totale = u64::try_from(byte.len()).unwrap_or(u64::MAX);
    match chiesto {
        Chiesto::Copertina => Risposta::Byte {
            totale,
            byte: byte.to_vec(),
        },
        Chiesto::Byte { da, quanti } => {
            let da = usize::try_from(da).unwrap_or(usize::MAX).min(byte.len());
            let quanti = usize::try_from(quanti.min(LETTURA_MASSIMA)).unwrap_or(0);
            let fine = da.saturating_add(quanti).min(byte.len());
            Risposta::Byte {
                totale,
                byte: byte.get(da..fine).map(<[u8]>::to_vec).unwrap_or_default(),
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod prove {
    use super::*;
    use std::path::PathBuf;

    /// Una cartella temporanea che si cancella da sola.
    ///
    /// Senza `tempfile`: aggiungerla qui cambierebbe il `Cargo.lock`, che non
    /// è di questo crate. Bastano un nome unico e un `Drop`.
    pub(crate) struct Cartella(pub(crate) PathBuf);

    impl Cartella {
        pub(crate) fn nuova(nome: &str) -> Self {
            let mut caso = [0_u8; 8];
            getrandom::fill(&mut caso).expect("generatore");
            let percorso = std::env::temp_dir().join(format!(
                "rymembra-insieme-{nome}-{}",
                crate::esadecimale::scrivi(&caso)
            ));
            std::fs::create_dir_all(&percorso).expect("cartella");
            Self(percorso)
        }

        /// Scrive un file e restituisce il suo `FileServito`.
        pub(crate) fn file(&self, nome: &str, byte: &[u8]) -> FileServito {
            let percorso = self.0.join(nome);
            std::fs::write(&percorso, byte).expect("scrittura");
            FileServito::da_percorso(&percorso).expect("metadati")
        }
    }

    impl Drop for Cartella {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Byte riconoscibili: ognuno dice dov'è.
    pub(crate) fn byte_di_prova(quanti: usize) -> Vec<u8> {
        (0..quanti)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect()
    }

    #[test]
    fn si_legge_l_intervallo_esatto_e_la_fine_del_file() {
        let cartella = Cartella::nuova("servente");
        let contenuto = byte_di_prova(10_000);
        let file = cartella.file("a.flac", &contenuto);
        assert_eq!(
            leggi_file(&file, 100, 50),
            Ok((contenuto.get(100..150).expect("dentro").to_vec(), 10_000))
        );
        assert_eq!(
            leggi_file(&file, 9_990, 50),
            Ok((contenuto.get(9_990..).expect("dentro").to_vec(), 10_000)),
            "meno di quanti alla fine"
        );
        assert_eq!(leggi_file(&file, 10_000, 50), Ok((Vec::new(), 10_000)));
        assert_eq!(leggi_file(&file, u64::MAX, 50), Ok((Vec::new(), 10_000)));
    }

    #[test]
    fn un_file_riscritto_o_sparito_e_un_file_cambiato() {
        let cartella = Cartella::nuova("servente");
        let file = cartella.file("a.flac", &byte_di_prova(1_000));
        std::fs::write(&file.percorso, byte_di_prova(999)).expect("riscrittura");
        assert_eq!(
            leggi_file(&file, 0, 10),
            Err(ErrorCodeKind::InsiemeFileCambiato)
        );

        let altro = cartella.file("b.flac", &byte_di_prova(1_000));
        let mut piu_vecchio = altro.clone();
        piu_vecchio.modificato_ms -= 1;
        assert_eq!(
            leggi_file(&piu_vecchio, 0, 10),
            Err(ErrorCodeKind::InsiemeFileCambiato),
            "stessa misura, altra data"
        );

        std::fs::remove_file(&altro.percorso).expect("cancellazione");
        assert_eq!(
            leggi_file(&altro, 0, 10),
            Err(ErrorCodeKind::InsiemeFileCambiato)
        );
    }

    #[test]
    fn una_lettura_non_va_oltre_il_tetto() {
        let cartella = Cartella::nuova("servente");
        let quanti = usize::try_from(LETTURA_MASSIMA).expect("sta") + 10;
        let file = cartella.file("grande.flac", &byte_di_prova(quanti));
        let (byte, totale) = leggi_file(&file, 0, u64::MAX).expect("si legge");
        assert_eq!(u64::try_from(byte.len()).expect("sta"), LETTURA_MASSIMA);
        assert_eq!(totale, u64::try_from(quanti).expect("sta"));
    }

    #[test]
    fn la_generazione_cambia_a_ogni_gettone_ridato() {
        let mut servente = Servente::default();
        let gettone = Gettone([1; 16]);
        servente.metti(gettone, Servito::Copertina(Arc::from(vec![1_u8])));
        let prima = servente.generazione(&gettone);
        servente.togli(&gettone);
        assert_eq!(servente.generazione(&gettone), None);
        servente.metti(gettone, Servito::Copertina(Arc::from(vec![2_u8])));
        assert_ne!(
            servente.generazione(&gettone),
            prima,
            "ridato è un altro permesso"
        );
    }

    #[test]
    fn un_inoltro_non_prende_mai_il_posto_di_un_altro_gettone() {
        let mut servente = Servente::default();
        let (mio, suo, altro) = (Gettone([1; 16]), Gettone([2; 16]), Gettone([3; 16]));
        let (anna, bruno) = (ChiavePubblica([10; 32]), ChiavePubblica([11; 32]));
        servente.metti(mio, Servito::Copertina(Arc::from(vec![1_u8])));
        assert!(!servente.inoltra(mio, anna), "un gettone dell'host");
        assert!(matches!(
            servente.cerca(&mio),
            Some((_, Servito::Copertina(_)))
        ));
        assert!(servente.inoltra(suo, anna), "libero");
        let prima = servente.generazione(&suo);
        assert!(
            servente.inoltra(suo, anna),
            "lo stesso proprietario: niente cambia"
        );
        assert_eq!(servente.generazione(&suo), prima);
        assert!(
            !servente.inoltra(suo, bruno),
            "il gettone di un altro membro"
        );
        assert!(matches!(servente.cerca(&suo), Some((_, Servito::Inoltrato(k))) if k == anna));
        assert!(servente.inoltra(altro, bruno));
    }

    #[test]
    fn una_copertina_si_legge_tutta_o_a_tratti() {
        let byte = byte_di_prova(100);
        assert_eq!(
            leggi_copertina(&byte, Chiesto::Copertina),
            Risposta::Byte {
                totale: 100,
                byte: byte.clone()
            }
        );
        assert_eq!(
            leggi_copertina(&byte, Chiesto::Byte { da: 90, quanti: 50 }),
            Risposta::Byte {
                totale: 100,
                byte: byte.get(90..).expect("dentro").to_vec()
            }
        );
        assert_eq!(
            leggi_copertina(&byte, Chiesto::Byte { da: 500, quanti: 5 }),
            Risposta::Byte {
                totale: 100,
                byte: Vec::new()
            }
        );
    }
}
