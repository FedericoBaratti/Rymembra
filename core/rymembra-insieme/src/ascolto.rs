//! L'ascolto di chi ospita: la porta, i tetti, l'accettazione.
//!
//! # La porta
//!
//! [`crate::PORTA`], poi le nove dopo, poi una qualunque. TCP e UDP **sulla
//! stessa** porta: la scoperta dice a chi cerca un numero solo, e un host con
//! la TCP su una porta e la UDP su un'altra sarebbe trovato e irraggiungibile.
//! Se la UDP non si lega dove si è legata la TCP, si passa alla porta dopo per
//! tutte e due.
//!
//! # I tetti
//!
//! Al più [`CONNESSIONI`] connessioni in entrata e [`PER_INDIRIZZO`] dallo
//! stesso indirizzo. Un membro ne usa quattro (controllo, due di lettura,
//! servizio): sei per indirizzo lasciano il posto a una riconnessione mentre
//! la vecchia non è ancora caduta. Il tetto per indirizzo non vale per
//! `127.0.0.1`: da lì arrivano solo programmi della stessa macchina — le
//! prove, le due istanze di debug — e un programma sulla stessa macchina che
//! volesse far danno ha modi molto più semplici di questo.
//!
//! Dopo la stretta, i tetti per chiave: vedi `Condiviso::registra_entrante`.
//!
//! # Le strette, e chi non parla
//!
//! Le strette sono la parte che costa — due moltiplicazioni sulla curva a
//! testa — e i posti sono pochi: [`STRETTE`] in tutto, [`STRETTE_PER_INDIRIZZO`]
//! dallo stesso indirizzo. Fino alla revisione del 26 ottobre bastavano
//! quattro connessioni mute per tenerli tutti per i cinque secondi della
//! stretta, e riaprirle appena chiuse: in venti secondi d'attacco, zero
//! membri onesti su quaranta riuscivano a entrare.
//!
//! Adesso:
//!
//! - chi bussa deve mandare il primo messaggio della stretta entro
//!   [`PRIMO_MESSAGGIO`], **prima** di avere un posto: leggerlo non costa
//!   niente, e chi tace non arriva mai alla parte che costa;
//! - con i posti pieni si aspetta un posto per [`ATTESA_POSTO`] invece di
//!   buttare subito la connessione: le strette oneste durano millisecondi;
//! - due posti al più dallo stesso indirizzo: chi manda il primo messaggio e
//!   poi tace occupa i suoi due, non quelli degli altri.

use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCode};

use crate::canale;
use crate::collegamento::{Verso, prepara, vivi};
use crate::condiviso::Condiviso;
#[cfg(doc)]
use crate::condiviso::RESPIRO;
use crate::errori::chiave;

/// Quante connessioni in entrata, al più.
pub(crate) const CONNESSIONI: usize = 48;

/// Quante dallo stesso indirizzo, al più.
pub(crate) const PER_INDIRIZZO: usize = 6;

/// Quante strette in corso, al più.
pub(crate) const STRETTE: usize = 4;

/// Quante strette in corso dallo stesso indirizzo, al più. Vale anche per
/// `127.0.0.1`: le strette oneste durano millisecondi, e chi aspetta un
/// posto lo aspetta (vedi [`ATTESA_POSTO`]).
pub(crate) const STRETTE_PER_INDIRIZZO: usize = 2;

/// Entro quanto deve arrivare il primo messaggio della stretta: un secondo.
/// Chi si collega a un host lo manda subito dopo la connessione TCP; un
/// secondo copre un Wi-Fi lento e un PC che compila.
const PRIMO_MESSAGGIO: Duration = Duration::from_secs(1);

/// Quanto aspetta un posto fra le strette chi ha già parlato: un secondo.
const ATTESA_POSTO: Duration = Duration::from_secs(1);

/// Ogni quanto l'ascolto guarda se qualcuno bussa.
///
/// Più corto del [`RESPIRO`] degli altri fili perché questo ritardo lo paga
/// ogni connessione nuova, e la prima lettura di un brano apre la sua:
/// misurato, con cinquanta millisecondi la prima lettura su `127.0.0.1`
/// costava cinquanta millisecondi, quasi tutti di attesa, su un anticipo di
/// partenza di duecentocinquanta. Dieci (in pratica il quanto del timer di
/// Windows, una quindicina) sono un centinaio di risvegli al secondo di una
/// chiamata che torna subito: non si vedono.
const RESPIRO_ASCOLTO: std::time::Duration = std::time::Duration::from_millis(10);

/// Quante porte si provano prima della porta zero.
const PORTE: u16 = 10;

/// Quante volte si prova la porta zero: la TCP la sceglie il sistema, e la
/// UDP con lo stesso numero può essere occupata per caso.
const TENTATIVI_ZERO: usize = 8;

/// Lega TCP e UDP sulla stessa porta. Vedi «La porta».
///
/// # Errori
///
/// `insieme.portaOccupata`, con la prima porta provata.
pub(crate) fn lega(
    indirizzo: IpAddr,
    prima: u16,
) -> Result<(TcpListener, UdpSocket, u16), AppError> {
    // Con la porta zero fin dall'inizio (le prove) si va diretti all'ultimo
    // passo: dieci volte «una qualunque» sono la stessa cosa.
    let fisse = if prima == 0 { 0 } else { PORTE };
    for scarto in 0..fisse {
        let Some(porta) = prima.checked_add(scarto) else {
            break;
        };
        let Ok(tcp) = TcpListener::bind((indirizzo, porta)) else {
            continue;
        };
        if let Ok(udp) = UdpSocket::bind((indirizzo, porta)) {
            return Ok((tcp, udp, porta));
        }
    }
    for _ in 0..TENTATIVI_ZERO {
        let Ok(tcp) = TcpListener::bind((indirizzo, 0)) else {
            continue;
        };
        let Ok(porta) = tcp.local_addr().map(|a| a.port()) else {
            continue;
        };
        if let Ok(udp) = UdpSocket::bind((indirizzo, porta)) {
            return Ok((tcp, udp, porta));
        }
    }
    Err(AppError::new(ErrorCode::InsiemePortaOccupata {
        porta: prima,
    }))
}

/// Il ciclo d'accettazione, finché l'ascolto è quello di questa generazione.
///
/// Non bloccante, come quello di `rymembra_oauth::loopback`: `accept` da solo
/// non ha una scadenza, e senza un giro che guarda «si deve uscire?» un
/// `smetti_di_ospitare` lascerebbe il filo appeso sulla porta fino alla
/// prossima connessione — cioè per sempre, e con la porta ancora aperta.
pub(crate) fn accogli(rete: &Arc<Condiviso>, ascolto: &TcpListener, generazione: u64) {
    if ascolto.set_nonblocking(true).is_err() {
        return;
    }
    while !rete.fermo() && rete.generazione.load(Ordering::SeqCst) == generazione {
        match ascolto.accept() {
            Ok((presa, da)) => entra(rete, presa, da, generazione),
            // `WouldBlock` è il caso normale: nessuno bussa. Gli altri errori
            // (una connessione resettata prima di essere accettata) non
            // fermano l'ascolto.
            Err(_) => std::thread::sleep(RESPIRO_ASCOLTO),
        }
    }
}

/// Una connessione appena accettata: il tetto delle connessioni, poi un filo
/// che aspetta il primo messaggio, prende un posto fra le strette e fa il
/// resto. Vedi «Le strette, e chi non parla».
fn entra(rete: &Arc<Condiviso>, mut presa: TcpStream, da: SocketAddr, generazione: u64) {
    // Su Windows il socket accettato eredita il non bloccante dall'ascolto.
    if presa.set_nonblocking(false).is_err() {
        return;
    }
    let Some(posto) = Posto::prendi(rete, da.ip()) else {
        return;
    };
    let _ = rete.filo("entra", move |rete| {
        let Ok(primo) = canale::primo_messaggio(&mut presa, Instant::now() + PRIMO_MESSAGGIO)
        else {
            return;
        };
        let Some(stretta) = Stretta::aspetta(&rete, da.ip(), ATTESA_POSTO) else {
            return;
        };
        let pronto = prepara(&rete, presa, da, Verso::Entrante { generazione, primo });
        drop(stretta);
        if let Ok(pronto) = pronto {
            vivi(&rete, pronto);
        }
        drop(posto);
    });
}

/// Un posto fra le connessioni in entrata, finché la connessione vive.
struct Posto {
    rete: Arc<Condiviso>,
    ip: IpAddr,
}

impl Posto {
    /// Un posto, se i tetti lo lasciano.
    fn prendi(rete: &Arc<Condiviso>, ip: IpAddr) -> Option<Self> {
        let mut posti = chiave(&rete.posti);
        let totale: usize = posti.values().sum();
        let suoi = posti.get(&ip).copied().unwrap_or(0);
        if totale >= CONNESSIONI || (!ip.is_loopback() && suoi >= PER_INDIRIZZO) {
            return None;
        }
        posti.insert(ip, suoi.saturating_add(1));
        Some(Self {
            rete: Arc::clone(rete),
            ip,
        })
    }
}

impl Drop for Posto {
    fn drop(&mut self) {
        let mut posti = chiave(&self.rete.posti);
        if let Some(suoi) = posti.get_mut(&self.ip) {
            *suoi = suoi.saturating_sub(1);
            if *suoi == 0 {
                posti.remove(&self.ip);
            }
        }
    }
}

/// Una stretta in corso: un posto, per indirizzo.
struct Stretta {
    rete: Arc<Condiviso>,
    ip: IpAddr,
}

impl Stretta {
    /// Un posto fra le strette, aspettandolo al più `entro`.
    fn aspetta(rete: &Arc<Condiviso>, ip: IpAddr, entro: Duration) -> Option<Self> {
        let fine = Instant::now() + entro;
        let mut strette = chiave(&rete.strette);
        loop {
            let totale: usize = strette.values().sum();
            let sue = strette.get(&ip).copied().unwrap_or(0);
            if totale < STRETTE && sue < STRETTE_PER_INDIRIZZO {
                strette.insert(ip, sue.saturating_add(1));
                return Some(Self {
                    rete: Arc::clone(rete),
                    ip,
                });
            }
            let resta = fine.saturating_duration_since(Instant::now());
            if resta.is_zero() || rete.fermo() {
                return None;
            }
            strette = rete
                .strette_libere
                .wait_timeout(strette, resta)
                .map_or_else(|e| e.into_inner().0, |(guardia, _)| guardia);
        }
    }
}

impl Drop for Stretta {
    fn drop(&mut self) {
        {
            let mut strette = chiave(&self.rete.strette);
            if let Some(sue) = strette.get_mut(&self.ip) {
                *sue = sue.saturating_sub(1);
                if *sue == 0 {
                    strette.remove(&self.ip);
                }
            }
        }
        self.rete.strette_libere.notify_all();
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn una_porta_occupata_fa_passare_alla_dopo_tcp_e_udp_insieme() {
        let locale = IpAddr::V4(Ipv4Addr::LOCALHOST);
        // Una porta libera adesso, scelta dal sistema, poi occupata in TCP.
        let occupata = TcpListener::bind((locale, 0)).expect("ascolto");
        let porta = occupata.local_addr().expect("indirizzo").port();
        let (tcp, udp, presa) = lega(locale, porta).expect("si lega");
        assert_ne!(presa, porta, "quella occupata si salta");
        assert_eq!(tcp.local_addr().expect("tcp").port(), presa);
        assert_eq!(udp.local_addr().expect("udp").port(), presa, "stessa porta");
        drop((tcp, udp));

        // Occupata solo in UDP: si salta lo stesso, per tutte e due.
        let occupata_udp = UdpSocket::bind((locale, 0)).expect("udp");
        let porta = occupata_udp.local_addr().expect("indirizzo").port();
        let (tcp, udp, presa) = lega(locale, porta).expect("si lega");
        assert_ne!(presa, porta);
        assert_eq!(tcp.local_addr().expect("tcp").port(), presa);
        assert_eq!(udp.local_addr().expect("udp").port(), presa);
    }

    #[test]
    fn la_porta_zero_va_diretta_a_una_qualunque() {
        let (tcp, udp, porta) =
            lega(IpAddr::V4(Ipv4Addr::LOCALHOST), 0).expect("una porta c'è sempre");
        assert_ne!(porta, 0);
        assert_eq!(tcp.local_addr().expect("tcp").port(), porta);
        assert_eq!(udp.local_addr().expect("udp").port(), porta);
    }
}
