//! La presa sotto una connessione: un `TcpStream` in casa, un flusso del
//! collegamento via internet fuori.
//!
//! # Perché un tratto
//!
//! Il canale Noise, il saluto, chi legge e chi scrive non hanno bisogno di
//! sapere che sotto c'è TCP: leggono e scrivono byte in ordine, con una
//! scadenza, e a volte tagliano. È quel che dà anche un flusso QUIC del
//! collegamento via internet (`rymembra-lontano`), che questo crate non
//! conosce: glielo passa il desktop, per una [`crate::Strada`] che apre e per
//! [`crate::Rete::accogli`] che riceve.
//!
//! Il tratto è lo specchio dell'uso di oggi di `TcpStream`, niente di più:
//! letture e scritture bloccanti, le due scadenze, una copia della maniglia per
//! chi scrive e una per chi taglia, la chiusura di una metà o di tutte e due.
//!
//! # L'origine
//!
//! I tetti per chi bussa (posti, strette, estranei) erano per indirizzo IP.
//! Dietro un relay o un CGNAT due dispositivi diversi possono avere lo stesso
//! indirizzo, e uno solo può averne cento: fuori casa il tetto è per
//! **chiave** del collegamento, che il trasporto sotto ha già provato.

use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

/// Dove sta l'altro capo: per aprirci una connessione, e riaprirla.
///
/// In casa un indirizzo; fuori la chiave pubblica del dispositivo nel
/// collegamento via internet, che la [`Strada`] sa raggiungere.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Dove {
    /// Un `ip:porta` della rete.
    Lan(SocketAddr),
    /// La chiave del dispositivo nel collegamento via internet, in testo.
    Lontano(String),
}

/// Senza l'indirizzo né la chiave: una stampa di prova finisce prima o poi in
/// un diario.
impl std::fmt::Debug for Dove {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Lan(_) => "Lan(…)",
            Self::Lontano(_) => "Lontano(…)",
        })
    }
}

impl Dove {
    /// Passa dal collegamento via internet.
    #[must_use]
    pub const fn e_lontano(&self) -> bool {
        matches!(self, Self::Lontano(_))
    }
}

/// Chi sa aprire un flusso verso un dispositivo del collegamento via internet:
/// il desktop, con il nodo di `rymembra-lontano`. Vedi [`crate::Rete::strada`].
pub trait Strada: Send + Sync + 'static {
    /// Un flusso nuovo verso il dispositivo `chiave`, entro `entro`.
    ///
    /// # Errori
    ///
    /// Se il dispositivo non si raggiunge, o il collegamento è spento.
    fn apri(&self, chiave: &str, entro: Duration) -> std::io::Result<Box<dyn Presa>>;
}

/// Da dove viene una connessione: chi si conta nei tetti.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origine {
    /// Un indirizzo della rete: una connessione TCP.
    Ip(IpAddr),
    /// La chiave del dispositivo all'altro capo del collegamento via internet,
    /// già provata dal trasporto sotto.
    Chiave([u8; 32]),
}

impl Origine {
    /// Questo stesso computer: i tetti per indirizzo non valgono, perché le
    /// prove aprono dieci connessioni da `127.0.0.1`.
    #[must_use]
    pub fn e_locale(&self) -> bool {
        matches!(self, Self::Ip(ip) if ip.is_loopback())
    }

    /// Viene dal collegamento via internet: per i tempi più larghi.
    #[must_use]
    pub const fn e_lontana(&self) -> bool {
        matches!(self, Self::Chiave(_))
    }
}

/// Una presa bloccante, a byte in ordine. Vedi il `//!`.
pub trait Presa: Read + Write + Send + Sync + 'static {
    /// La scadenza delle letture; `None` è nessuna.
    ///
    /// # Errori
    ///
    /// Quelli della presa.
    fn set_read_timeout(&self, scadenza: Option<Duration>) -> std::io::Result<()>;

    /// La scadenza delle scritture; `None` è nessuna.
    ///
    /// # Errori
    ///
    /// Quelli della presa.
    fn set_write_timeout(&self, scadenza: Option<Duration>) -> std::io::Result<()>;

    /// Un'altra maniglia della stessa presa: una per chi scrive, una per chi
    /// taglia mentre chi legge è fermo ad aspettare.
    ///
    /// # Errori
    ///
    /// Quelli della presa.
    fn try_clone(&self) -> std::io::Result<Box<dyn Presa>>;

    /// Chiude una metà, o tutte e due: chi è fermo a leggere si sveglia.
    ///
    /// # Errori
    ///
    /// Quelli della presa.
    fn shutdown(&self, come: Shutdown) -> std::io::Result<()>;

    /// I messaggi piccoli partono subito. Di serie non fa niente: un flusso
    /// QUIC non aspetta di riempire un pacchetto.
    ///
    /// # Errori
    ///
    /// Quelli della presa.
    fn set_nodelay(&self, _subito: bool) -> std::io::Result<()> {
        Ok(())
    }

    /// Da dove viene: vedi [`Origine`].
    fn origine(&self) -> Origine;
}

impl Presa for TcpStream {
    fn set_read_timeout(&self, scadenza: Option<Duration>) -> std::io::Result<()> {
        Self::set_read_timeout(self, scadenza)
    }

    fn set_write_timeout(&self, scadenza: Option<Duration>) -> std::io::Result<()> {
        Self::set_write_timeout(self, scadenza)
    }

    fn try_clone(&self) -> std::io::Result<Box<dyn Presa>> {
        Self::try_clone(self).map(|copia| Box::new(copia) as Box<dyn Presa>)
    }

    fn shutdown(&self, come: Shutdown) -> std::io::Result<()> {
        Self::shutdown(self, come)
    }

    fn set_nodelay(&self, subito: bool) -> std::io::Result<()> {
        Self::set_nodelay(self, subito)
    }

    fn origine(&self) -> Origine {
        // Senza l'altro capo — una presa già chiusa — conta come questo
        // computer: non si conta nei tetti, e la connessione cade da sé.
        self.peer_addr().map_or(
            Origine::Ip(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
            |da| Origine::Ip(da.ip()),
        )
    }
}

/// Una presa già in scatola è una presa: chi la tiene passa `&mut presa` dove
/// serve `&mut dyn Presa`, come faceva con un `TcpStream`.
impl Presa for Box<dyn Presa> {
    fn set_read_timeout(&self, scadenza: Option<Duration>) -> std::io::Result<()> {
        (**self).set_read_timeout(scadenza)
    }

    fn set_write_timeout(&self, scadenza: Option<Duration>) -> std::io::Result<()> {
        (**self).set_write_timeout(scadenza)
    }

    fn try_clone(&self) -> std::io::Result<Box<dyn Presa>> {
        (**self).try_clone()
    }

    fn shutdown(&self, come: Shutdown) -> std::io::Result<()> {
        (**self).shutdown(come)
    }

    fn set_nodelay(&self, subito: bool) -> std::io::Result<()> {
        (**self).set_nodelay(subito)
    }

    fn origine(&self) -> Origine {
        (**self).origine()
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn solo_l_indirizzo_di_questo_computer_e_locale() {
        assert!(Origine::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST)).e_locale());
        assert!(Origine::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST)).e_locale());
        assert!(!Origine::Ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))).e_locale());
        assert!(!Origine::Chiave([0; 32]).e_locale());
        assert!(Origine::Chiave([0; 32]).e_lontana());
        assert!(!Origine::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST)).e_lontana());
    }
}
