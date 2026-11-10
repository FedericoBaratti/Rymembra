//! Un flusso bloccante sopra un bi-stream QUIC: «Ascolta insieme» via
//! internet.
//!
//! # A cosa serve
//!
//! «Ascolta insieme» parla il suo protocollo — la stretta Noise, il saluto,
//! le cornici — sopra una presa che legge e scrive byte in ordine, con una
//! scadenza (`rymembra_insieme::Presa`). In casa è un `TcpStream`; fuori è
//! questo: una connessione QUIC verso un dispositivo associato, sull'ALPN
//! [`crate::protocollo::ALPN_INSIEME`], e il suo unico bi-stream. Il canale Noise
//! resta sopra, con la chiave di «Ascolta insieme» che l'associazione col QR
//! ha scambiato: due cifrature, e la seconda è quella che «Ascolta insieme»
//! sa provare.
//!
//! # Bloccante, da fuori del runtime
//!
//! Ogni lettura e scrittura è un `block_on` sul runtime del nodo, con la sua
//! scadenza: si chiamano dai fili di «Ascolta insieme», mai da uno del
//! runtime. Le due metà hanno un lucchetto ciascuna — chi legge e chi scrive
//! stanno su due fili — e chi taglia non ne prende nessuno: chiude la
//! connessione intera, e chi è fermo a leggere si sveglia con un errore.
//!
//! Una connessione per flusso: chiuderla è chiudere quel flusso e basta, e
//! le domande della libreria (`rymembra/lontano/1`) non ne risentono.

use std::io::{Error, ErrorKind, Read, Write};
use std::net::Shutdown;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};

use crate::errori::chiave;

/// Il motivo con cui si chiude una connessione di un flusso tagliato.
const CHIUSO: &[u8] = b"insieme.fine";

/// Un flusso: vedi il `//!`.
pub struct Flusso {
    invio: Arc<tokio::sync::Mutex<SendStream>>,
    ricevi: Arc<tokio::sync::Mutex<RecvStream>>,
    conn: Connection,
    runtime: tokio::runtime::Handle,
    /// Le scadenze, comuni alle copie come quelle di un socket.
    leggi_entro: Arc<Mutex<Option<Duration>>>,
    scrivi_entro: Arc<Mutex<Option<Duration>>>,
    /// Il runtime del nodo gira ancora.
    vivo: Arc<AtomicBool>,
}

/// Senza la chiave dell'altro: una stampa di prova finisce in un diario.
impl std::fmt::Debug for Flusso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Flusso").finish_non_exhaustive()
    }
}

impl Flusso {
    pub(crate) fn nuovo(
        conn: Connection,
        invio: SendStream,
        ricevi: RecvStream,
        runtime: tokio::runtime::Handle,
        vivo: Arc<AtomicBool>,
    ) -> Self {
        Self {
            invio: Arc::new(tokio::sync::Mutex::new(invio)),
            ricevi: Arc::new(tokio::sync::Mutex::new(ricevi)),
            conn,
            runtime,
            leggi_entro: Arc::new(Mutex::new(None)),
            scrivi_entro: Arc::new(Mutex::new(None)),
            vivo,
        }
    }

    /// La chiave pubblica dell'altro capo, che QUIC ha già provato: per i
    /// tetti di chi accoglie.
    #[must_use]
    pub fn chiave_remota(&self) -> [u8; 32] {
        *self.conn.remote_id().as_bytes()
    }

    /// La scadenza delle letture; `None` è nessuna.
    pub fn set_read_timeout(&self, scadenza: Option<Duration>) {
        *chiave(&self.leggi_entro) = scadenza;
    }

    /// La scadenza delle scritture; `None` è nessuna.
    pub fn set_write_timeout(&self, scadenza: Option<Duration>) {
        *chiave(&self.scrivi_entro) = scadenza;
    }

    /// Un'altra maniglia dello stesso flusso.
    #[must_use]
    pub fn try_clone(&self) -> Self {
        Self {
            invio: Arc::clone(&self.invio),
            ricevi: Arc::clone(&self.ricevi),
            conn: self.conn.clone(),
            runtime: self.runtime.clone(),
            leggi_entro: Arc::clone(&self.leggi_entro),
            scrivi_entro: Arc::clone(&self.scrivi_entro),
            vivo: Arc::clone(&self.vivo),
        }
    }

    /// Chiude la metà in scrittura (l'altro legge la fine), o tutto: la
    /// connessione intera, che sveglia chi è fermo a leggere.
    ///
    /// # Errori
    ///
    /// Mai: un flusso già chiuso resta chiuso.
    pub fn shutdown(&self, come: Shutdown) -> std::io::Result<()> {
        match come {
            Shutdown::Write => {
                // Senza aspettare il lucchetto: lo tiene una scrittura in
                // corso, che senza scadenza potrebbe non tornare mai. Allora
                // la fine non si può scrivere in ordine dopo di lei, e si
                // chiude tutto — che per chi legge dall'altra parte è comunque
                // la fine.
                match self.invio.try_lock() {
                    Ok(mut invio) => {
                        let _ = invio.finish();
                    }
                    Err(_) => self.conn.close(VarInt::from_u32(0), CHIUSO),
                }
            }
            Shutdown::Read | Shutdown::Both => {
                self.conn.close(VarInt::from_u32(0), CHIUSO);
            }
        }
        Ok(())
    }

    /// Un `block_on` con la scadenza, o senza.
    fn aspetta<T, F: std::future::Future<Output = T>>(
        &self,
        entro: Option<Duration>,
        lavoro: F,
    ) -> std::io::Result<T> {
        if !self.vivo.load(Ordering::Acquire) {
            return Err(Error::new(ErrorKind::ConnectionAborted, "il nodo è spento"));
        }
        self.runtime.block_on(async move {
            match entro {
                Some(entro) => tokio::time::timeout(entro, lavoro)
                    .await
                    .map_err(|_| Error::new(ErrorKind::TimedOut, "scaduto")),
                None => Ok(lavoro.await),
            }
        })
    }
}

impl Read for Flusso {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let entro = *chiave(&self.leggi_entro);
        let ricevi = Arc::clone(&self.ricevi);
        let letti = self.aspetta(entro, async move {
            // Il lucchetto resta preso mentre si aspetta: l'altra metà ha il
            // suo, e chi taglia non ne prende.
            let mut ricevi = ricevi.lock().await;
            ricevi.read(buffer).await
        })?;
        match letti {
            Ok(Some(letti)) => Ok(letti),
            // La fine del flusso: come un socket chiuso dall'altra parte.
            Ok(None) => Ok(0),
            Err(err) => Err(Error::new(ErrorKind::ConnectionReset, err)),
        }
    }
}

impl Write for Flusso {
    fn write(&mut self, byte: &[u8]) -> std::io::Result<usize> {
        let entro = *chiave(&self.scrivi_entro);
        let invio = Arc::clone(&self.invio);
        let scritti = self.aspetta(entro, async move {
            let mut invio = invio.lock().await;
            invio.write(byte).await
        })?;
        scritti.map_err(|err| Error::new(ErrorKind::ConnectionReset, err))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
