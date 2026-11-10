//! «Ascolta insieme» via internet: il collante fra il collegamento
//! (`rymembra-lontano`) e la rete di «Ascolta insieme» (`rymembra-insieme`).
//!
//! # Tre pezzi
//!
//! - [`PresaLontana`]: un [`Flusso`] del nodo vestito da
//!   [`rymembra_insieme::Presa`]. Sopra ci gira tutto «Ascolta insieme» com'è —
//!   la stretta Noise, il saluto, le cornici — con l'origine per chiave e i
//!   tempi larghi.
//! - [`strada`]: chi apre quei flussi per la rete di «Ascolta insieme»
//!   ([`rymembra_insieme::Rete::strada`]), verso un dispositivo associato.
//! - [`ospitano`]: i dispositivi collegati che in questo momento ospitano una
//!   sessione, con la loro chiave di «Ascolta insieme». Il filo di «Ascolta
//!   insieme» li tratta come vicini raggiungibili da lì (`Vicinato`), accanto a
//!   quelli che la scoperta trova in casa.
//!
//! I flussi in entrata li porta il nodo all'[`super::ospite::Ospite`], che li
//! passa alla rete di «Ascolta insieme» (`crate::insieme::accogli_lontano`).
//!
//! # Chi si fida di chi
//!
//! Due cancelli, uno sopra l'altro. Il nodo apre e accoglie flussi solo fra
//! dispositivi associati col QR; sopra, la stretta Noise prova la chiave di
//! «Ascolta insieme», e la rete la confronta con i suoi PC fidati — che il QR
//! ha già scritto (`Ospite::fidato_anche_insieme`).

use std::io::{Error, ErrorKind, Read, Write};
use std::net::Shutdown;
use std::sync::Arc;
use std::time::Duration;

use rymembra_insieme::{Origine, Presa, Strada};
use rymembra_lontano::Flusso;
use tauri::AppHandle;

use super::comune;
use super::filo::prendi;

/// Un [`Flusso`] del collegamento come presa di «Ascolta insieme».
pub struct PresaLontana(Flusso);

impl PresaLontana {
    #[must_use]
    pub const fn nuova(flusso: Flusso) -> Self {
        Self(flusso)
    }
}

impl std::fmt::Debug for PresaLontana {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresaLontana").finish_non_exhaustive()
    }
}

impl Read for PresaLontana {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Write for PresaLontana {
    fn write(&mut self, byte: &[u8]) -> std::io::Result<usize> {
        self.0.write(byte)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl Presa for PresaLontana {
    fn set_read_timeout(&self, scadenza: Option<Duration>) -> std::io::Result<()> {
        self.0.set_read_timeout(scadenza);
        Ok(())
    }

    fn set_write_timeout(&self, scadenza: Option<Duration>) -> std::io::Result<()> {
        self.0.set_write_timeout(scadenza);
        Ok(())
    }

    fn try_clone(&self) -> std::io::Result<Box<dyn Presa>> {
        Ok(Box::new(Self(self.0.try_clone())))
    }

    fn shutdown(&self, come: Shutdown) -> std::io::Result<()> {
        self.0.shutdown(come)
    }

    fn origine(&self) -> Origine {
        Origine::Chiave(self.0.chiave_remota())
    }
}

/// Chi apre i flussi verso i dispositivi associati.
struct StradaLontana {
    app: AppHandle,
}

impl Strada for StradaLontana {
    fn apri(&self, chiave: &str, entro: Duration) -> std::io::Result<Box<dyn Presa>> {
        let spento = || {
            Error::new(
                ErrorKind::NotConnected,
                "il collegamento via internet è spento",
            )
        };
        let comune = comune(&self.app).map_err(|_| spento())?;
        // Il nodo per il tempo dell'apertura e basta: il flusso tiene la sua
        // connessione, non il nodo (vedi `Comune::nodo`).
        let nodo = comune.nodo().ok_or_else(spento)?;
        let flusso = nodo
            .apri_insieme(chiave, entro)
            .map_err(|err| Error::new(ErrorKind::ConnectionRefused, err.code().kind().code()))?;
        drop(nodo);
        Ok(Box::new(PresaLontana(flusso)))
    }
}

/// La strada per la rete di «Ascolta insieme».
#[must_use]
pub fn strada(app: &AppHandle) -> Arc<dyn Strada> {
    Arc::new(StradaLontana { app: app.clone() })
}

/// «Solo Wi-Fi» per una sessione di «Ascolta insieme» via internet: la stessa
/// regola dei brani di «Sfoglia» (`sfoglia::dati_permessi`), perché la musica
/// di una sessione pesa come quella. A collegamento spento non c'è niente da
/// chiedere: non c'è nemmeno la connessione.
///
/// # Errori
///
/// `lontano.reteAConsumo` sui dati mobili senza il sì; la finestra chiede
/// come per «Sfoglia» (`gestoLontano`).
pub fn dati_permessi(app: &AppHandle) -> Result<(), rymembra_domain::errors::AppError> {
    match comune(app) {
        Ok(comune) => super::sfoglia::dati_permessi(&comune),
        Err(_) => Ok(()),
    }
}

/// I dispositivi collegati che adesso ospitano una sessione, con la loro
/// chiave di «Ascolta insieme» in testo: `(chiave del collegamento, chiave di
/// «Ascolta insieme»)`. Vuoto a collegamento spento.
#[must_use]
pub fn ospitano(app: &AppHandle) -> Vec<(String, String)> {
    comune(app)
        .map(|comune| prendi(&comune.ospitano).clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod prove {
    //! «Ascolta insieme» da capo a capo sopra il collegamento: due nodi veri
    //! nello stesso processo, senza relay (solo `127.0.0.1`), e sopra due reti
    //! di «Ascolta insieme». Chi segue apre la connessione di controllo con
    //! `Dove::Lontano`; chi ospita la riceve dal nodo e la accoglie.

    use std::collections::HashSet;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Mutex;
    use std::sync::mpsc::Receiver;
    use std::time::Instant;

    use rymembra_domain::errors::{AppError, ErrorCode};
    use rymembra_insieme::{Dove, Evento as EventoInsieme};
    use rymembra_lontano::{
        Domanda, Evento, Identita, Impostazioni, Nodo, Ospite, Presentazione, Presenza, Rete,
        Risposta,
    };

    use super::*;

    /// L'app finta di un nodo: chi è associato, e la rete di «Ascolta
    /// insieme» a cui passare i flussi che arrivano.
    #[derive(Default)]
    struct Finta {
        associati: Mutex<HashSet<String>>,
        rete: Mutex<Option<rymembra_insieme::Rete>>,
    }

    impl Ospite for Finta {
        fn associato(&self, chiave: &str) -> bool {
            self.associati.lock().unwrap().contains(chiave)
        }

        fn conferma(&self, _chiave: &str, _chi: &Presentazione) -> bool {
            true
        }

        fn nuovo_pari(&self, chiave: &str, _chi: &Presentazione, _indirizzi: &[SocketAddr]) {
            self.associati.lock().unwrap().insert(chiave.to_owned());
        }

        fn rispondi(&self, _da: &str, _domanda: Domanda) -> Result<Risposta, AppError> {
            Err(AppError::new(ErrorCode::LontanoProtocollo))
        }

        fn dimenticato(&self, _da: &str) {}

        fn evento(&self, _evento: Evento) {}

        fn insieme(&self, da: &str, flusso: Flusso) {
            if let Some(rete) = self.rete.lock().unwrap().as_ref() {
                rete.accogli(
                    Box::new(PresaLontana::nuova(flusso)),
                    Dove::Lontano(da.to_owned()),
                );
            }
        }
    }

    fn nodo(nome: &str) -> (Arc<Nodo>, Arc<Finta>) {
        let finta = Arc::new(Finta::default());
        let nodo = Nodo::avvia(
            Impostazioni {
                identita: Identita::nuova(),
                rete: Rete::SoloQuesta,
                io: Presentazione {
                    nome: nome.to_owned(),
                    piattaforma: "prova".to_owned(),
                    dispositivo: format!("d-{nome}"),
                    chiave_insieme: None,
                },
                presenza: Presenza::default(),
            },
            Arc::clone(&finta) as Arc<dyn Ospite>,
        )
        .unwrap();
        (Arc::new(nodo), finta)
    }

    fn rete(nome: &str) -> (rymembra_insieme::Rete, Receiver<EventoInsieme>) {
        rymembra_insieme::Rete::avvia(rymembra_insieme::Impostazioni {
            identita: rymembra_insieme::Identita::nuova().unwrap(),
            nome: nome.to_owned(),
            lega: IpAddr::V4(Ipv4Addr::LOCALHOST),
            porta: 0,
            adesso_ns: rymembra_play::tempo::adesso_ns,
        })
        .unwrap()
    }

    /// La strada di chi segue: il suo nodo.
    struct DalNodo(Arc<Nodo>);

    impl Strada for DalNodo {
        fn apri(&self, chiave: &str, entro: Duration) -> std::io::Result<Box<dyn Presa>> {
            let flusso = self.0.apri_insieme(chiave, entro).map_err(|err| {
                Error::new(ErrorKind::ConnectionRefused, err.code().kind().code())
            })?;
            Ok(Box::new(PresaLontana::nuova(flusso)))
        }
    }

    /// Il primo evento per cui `cerca` dice qualcosa, entro venti secondi.
    fn aspetta<T>(
        eventi: &Receiver<EventoInsieme>,
        cerca: impl Fn(&EventoInsieme) -> Option<T>,
    ) -> Option<T> {
        let fine = Instant::now() + Duration::from_secs(20);
        while let Some(resta) = fine.checked_duration_since(Instant::now()) {
            match eventi.recv_timeout(resta) {
                Ok(evento) => {
                    if let Some(trovato) = cerca(&evento) {
                        return Some(trovato);
                    }
                }
                Err(_) => return None,
            }
        }
        None
    }

    #[test]
    fn una_sessione_si_apre_via_internet_fra_due_dispositivi_collegati() {
        let (pc, del_pc) = nodo("PC");
        let (telefono, _) = nodo("Telefono");
        let invito = pc.invita().unwrap();
        telefono.collega(&invito.link()).expect("associati");

        // Il PC ospita; i flussi che il suo nodo riceve vanno alla sua rete.
        let (ospite, eventi_ospite) = rete("PC");
        ospite.ospita().expect("ospita");
        let impronta = ospite.pubblica().impronta();
        *del_pc.rete.lock().unwrap() = Some(ospite.clone());

        // Il telefono segue: la sua rete arriva al PC dal suo nodo.
        let (membro, eventi_membro) = rete("Telefono");
        membro.strada(Some(Arc::new(DalNodo(Arc::clone(&telefono)))));
        let id = membro.collega_atteso(Dove::Lontano(pc.chiave()), impronta);

        let aperta = aspetta(&eventi_membro, |e| match e {
            EventoInsieme::Aperta {
                id: aperta,
                dove,
                pari,
                ospita,
                ..
            } if *aperta == id => Some((dove.clone(), *pari, *ospita)),
            EventoInsieme::Chiusa { id: chiusa, codice } if *chiusa == id => {
                panic!("la connessione si è chiusa: {codice:?}")
            }
            _ => None,
        })
        .expect("la connessione si apre dal telefono");
        assert_eq!(aperta.0, Dove::Lontano(pc.chiave()));
        assert_eq!(aperta.1, ospite.pubblica(), "la stretta Noise prova l'host");
        assert!(aperta.2, "l'host dice di ospitare");

        let dall_ospite = aspetta(&eventi_ospite, |e| match e {
            EventoInsieme::Aperta {
                dove, in_entrata, ..
            } if *in_entrata => Some(dove.clone()),
            _ => None,
        })
        .expect("e arriva al PC");
        assert_eq!(dall_ospite, Dove::Lontano(telefono.chiave()));

        membro.ferma();
        ospite.ferma();
        *del_pc.rete.lock().unwrap() = None;
    }
}
