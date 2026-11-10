//! Il link del collegamento, e il suo QR.
//!
//! `rymembra://collega/<base64url>`: dentro, in binario, la chiave pubblica di
//! chi mostra il QR, un gettone casuale e i modi per raggiungerlo — fino a
//! [`MASSIMO_INDIRIZZI`] indirizzi diretti e il relay di casa. Il QR è lo
//! stesso link, che la fotocamera di sistema sa anche aprire in Rymembra.
//!
//! # Cosa vale il gettone
//!
//! Niente da solo: chi lo presenta deve anche essere accettato da chi mostra
//! il QR («"Nothing Phone 2a" vuole collegarsi — Consenti/Rifiuta»). Serve a
//! che nessuno possa nemmeno chiedere senza averlo visto: vale dieci minuti e
//! una volta sola (vedi `nodo`).
//!
//! # La forma
//!
//! ```text
//! versione (1) · chiave (32) · gettone (16) · n indirizzi (1)
//!   · per ognuno: famiglia (4 o 6) · IP (4 o 16) · porta (2, big endian)
//! · lunghezza del relay (1, zero se non c'è) · relay (UTF-8)
//! ```
//!
//! Binario e non JSON perché il QR deve restare piccolo: con quattro indirizzi
//! IPv6 e un relay il link sta sotto i trecento caratteri.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use rymembra_domain::errors::{AppError, ErrorCode};

/// Come comincia un link di collegamento.
pub const PREFISSO: &str = "rymembra://collega/";

/// La versione della forma. Una più nuova è `lontano.linkNonValido`, che dice
/// di aggiornare.
const VERSIONE: u8 = 1;

/// Quanti indirizzi diretti porta un link, al più.
pub const MASSIMO_INDIRIZZI: usize = 4;

/// Quanto può essere lungo l'indirizzo del relay.
const MASSIMO_RELAY: usize = 200;

/// Quel che un link porta.
#[derive(Clone, PartialEq, Eq)]
pub struct Invito {
    /// La chiave pubblica (ed25519) di chi mostra il QR.
    pub chiave: [u8; 32],
    /// Il gettone, da presentare una volta sola.
    pub gettone: [u8; 16],
    /// Indirizzi diretti, i primi da provare.
    pub indirizzi: Vec<SocketAddr>,
    /// Il relay di casa di chi mostra il QR.
    pub relay: Option<String>,
}

/// Scritto a mano: il gettone è un segreto, e non va in un diario.
impl std::fmt::Debug for Invito {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invito")
            .field("indirizzi", &self.indirizzi)
            .field("relay", &self.relay)
            .finish_non_exhaustive()
    }
}

impl Invito {
    /// Il link, `rymembra://collega/…`.
    #[must_use]
    pub fn link(&self) -> String {
        let mut byte = Vec::with_capacity(128);
        byte.push(VERSIONE);
        byte.extend_from_slice(&self.chiave);
        byte.extend_from_slice(&self.gettone);
        let indirizzi: Vec<&SocketAddr> = self.indirizzi.iter().take(MASSIMO_INDIRIZZI).collect();
        byte.push(u8::try_from(indirizzi.len()).unwrap_or(0));
        for indirizzo in indirizzi {
            match indirizzo.ip() {
                IpAddr::V4(ip) => {
                    byte.push(4);
                    byte.extend_from_slice(&ip.octets());
                }
                IpAddr::V6(ip) => {
                    byte.push(6);
                    byte.extend_from_slice(&ip.octets());
                }
            }
            byte.extend_from_slice(&indirizzo.port().to_be_bytes());
        }
        let relay = self
            .relay
            .as_deref()
            .filter(|r| r.len() <= MASSIMO_RELAY)
            .unwrap_or_default();
        byte.push(u8::try_from(relay.len()).unwrap_or(0));
        byte.extend_from_slice(relay.as_bytes());
        format!("{PREFISSO}{}", base64url(&byte))
    }

    /// Il contenuto di un link.
    ///
    /// Accetta anche il link con spazi intorno, com'è quando lo si incolla da
    /// un messaggio.
    ///
    /// # Errori
    ///
    /// `lontano.linkNonValido` per un testo che non è un link di
    /// collegamento, o di una versione che questa non sa leggere.
    pub fn da_link(testo: &str) -> Result<Self, AppError> {
        let non_valido = |perche: &str| {
            AppError::new(ErrorCode::LontanoLinkNonValido).with_cause(perche.to_owned())
        };
        let corpo = testo
            .trim()
            .strip_prefix(PREFISSO)
            .ok_or_else(|| non_valido("non comincia con rymembra://collega/"))?;
        let byte = da_base64url(corpo.trim_end_matches('/'))
            .ok_or_else(|| non_valido("non è base64url"))?;
        let mut lettore = Lettore(&byte);
        let versione = lettore.uno().ok_or_else(|| non_valido("vuoto"))?;
        if versione != VERSIONE {
            return Err(non_valido(&format!("versione {versione}")));
        }
        let chiave: [u8; 32] = lettore
            .fissi()
            .ok_or_else(|| non_valido("manca la chiave"))?;
        let gettone: [u8; 16] = lettore
            .fissi()
            .ok_or_else(|| non_valido("manca il gettone"))?;
        let quanti = usize::from(lettore.uno().ok_or_else(|| non_valido("corto"))?);
        if quanti > MASSIMO_INDIRIZZI {
            return Err(non_valido("troppi indirizzi"));
        }
        let mut indirizzi = Vec::with_capacity(quanti);
        for _ in 0..quanti {
            let ip = match lettore.uno() {
                Some(4) => IpAddr::V4(Ipv4Addr::from(
                    lettore
                        .fissi::<4>()
                        .ok_or_else(|| non_valido("IPv4 corto"))?,
                )),
                Some(6) => IpAddr::V6(Ipv6Addr::from(
                    lettore
                        .fissi::<16>()
                        .ok_or_else(|| non_valido("IPv6 corto"))?,
                )),
                _ => return Err(non_valido("famiglia sconosciuta")),
            };
            let porta = u16::from_be_bytes(
                lettore
                    .fissi::<2>()
                    .ok_or_else(|| non_valido("porta corta"))?,
            );
            indirizzi.push(SocketAddr::new(ip, porta));
        }
        let lunghezza = usize::from(lettore.uno().ok_or_else(|| non_valido("corto"))?);
        let relay = lettore
            .tanti(lunghezza)
            .ok_or_else(|| non_valido("relay corto"))?;
        let relay = std::str::from_utf8(relay)
            .map_err(|_| non_valido("relay non UTF-8"))?
            .to_owned();
        if !lettore.0.is_empty() {
            return Err(non_valido("byte in più"));
        }
        Ok(Self {
            chiave,
            gettone,
            indirizzi,
            relay: (!relay.is_empty()).then_some(relay),
        })
    }
}

/// I moduli di un QR, riga per riga: `true` è un modulo scuro.
///
/// La finestra lo disegna da sé come `<svg>`, un rettangolo per modulo: niente
/// immagini, e niente HTML che arrivi già fatto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrice {
    /// Moduli per lato (senza il margine bianco, che aggiunge chi disegna).
    pub lato: usize,
    /// `lato × lato` moduli.
    pub moduli: Vec<bool>,
}

/// Il QR di un testo, con la correzione d'errore media: regge un riflesso
/// sullo schermo e una fotocamera storta.
///
/// # Errori
///
/// `internal.unexpected` per un testo troppo lungo per un QR (un link non lo è
/// mai).
pub fn matrice(testo: &str) -> Result<Matrice, AppError> {
    let codice = qrcode::QrCode::with_error_correction_level(testo.as_bytes(), qrcode::EcLevel::M)
        .map_err(crate::errori::inatteso)?;
    Ok(Matrice {
        lato: codice.width(),
        moduli: codice
            .to_colors()
            .into_iter()
            .map(|colore| colore == qrcode::Color::Dark)
            .collect(),
    })
}

/// Legge un poco alla volta, senza mai andare oltre la fine.
struct Lettore<'a>(&'a [u8]);

impl<'a> Lettore<'a> {
    fn uno(&mut self) -> Option<u8> {
        let (primo, resto) = self.0.split_first()?;
        self.0 = resto;
        Some(*primo)
    }

    fn tanti(&mut self, quanti: usize) -> Option<&'a [u8]> {
        if self.0.len() < quanti {
            return None;
        }
        let (presi, resto) = self.0.split_at(quanti);
        self.0 = resto;
        Some(presi)
    }

    fn fissi<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.tanti(N)?.try_into().ok()
    }
}

const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64 per gli indirizzi (RFC 4648 § 5), senza `=`.
fn base64url(byte: &[u8]) -> String {
    let mut testo = String::with_capacity(byte.len().div_ceil(3) * 4);
    for gruppo in byte.chunks(3) {
        let a = u32::from(gruppo.first().copied().unwrap_or(0));
        let b = u32::from(gruppo.get(1).copied().unwrap_or(0));
        let c = u32::from(gruppo.get(2).copied().unwrap_or(0));
        let tre = (a << 16) | (b << 8) | c;
        let quanti = gruppo.len() + 1;
        for i in 0..quanti {
            let indice = usize::try_from((tre >> (18 - 6 * i)) & 0x3f).unwrap_or(0);
            if let Some(&carattere) = ALFABETO.get(indice) {
                testo.push(char::from(carattere));
            }
        }
    }
    testo
}

/// Il contrario di [`base64url`]; `None` per un carattere fuori alfabeto o
/// una lunghezza impossibile.
fn da_base64url(testo: &str) -> Option<Vec<u8>> {
    let valori: Vec<u32> = testo
        .bytes()
        .map(|c| {
            ALFABETO
                .iter()
                .position(|a| *a == c)
                .and_then(|p| u32::try_from(p).ok())
        })
        .collect::<Option<_>>()?;
    if valori.len() % 4 == 1 {
        return None;
    }
    let mut byte = Vec::with_capacity(valori.len().saturating_mul(3) >> 2);
    for gruppo in valori.chunks(4) {
        let mut tutti = 0_u32;
        for (i, valore) in gruppo.iter().enumerate() {
            tutti |= valore << (18 - 6 * i);
        }
        let quanti = gruppo.len() - 1;
        for i in 0..quanti {
            byte.push(u8::try_from((tutti >> (16 - 8 * i)) & 0xff).ok()?);
        }
    }
    Some(byte)
}

#[cfg(test)]
mod prove {
    use rymembra_domain::errors::ErrorCodeKind;

    use super::*;

    fn invito() -> Invito {
        Invito {
            chiave: [7; 32],
            gettone: [9; 16],
            indirizzi: vec![
                "192.168.1.20:52000".parse().unwrap(),
                "[2001:db8::1]:443".parse().unwrap(),
            ],
            relay: Some("https://euw1-1.relay.n0.iroh-canary.iroh.link./".to_owned()),
        }
    }

    #[test]
    fn un_link_torna_com_era() {
        let originale = invito();
        let link = originale.link();
        assert!(link.starts_with(PREFISSO));
        assert!(link.len() < 300, "{} caratteri", link.len());
        assert_eq!(Invito::da_link(&link).unwrap(), originale);
        assert_eq!(Invito::da_link(&format!("  {link}\n")).unwrap(), originale);
        let senza = Invito {
            indirizzi: Vec::new(),
            relay: None,
            ..originale
        };
        assert_eq!(Invito::da_link(&senza.link()).unwrap(), senza);
    }

    #[test]
    fn un_link_storto_si_dice() {
        let link = invito().link();
        for storto in [
            "https://esempio.it",
            "rymembra://collega/",
            "rymembra://collega/!!!",
            &link[..link.len() - 10],
            &format!("{link}AAAA"),
        ] {
            assert_eq!(
                Invito::da_link(storto).unwrap_err().code().kind(),
                ErrorCodeKind::LontanoLinkNonValido,
                "{storto}"
            );
        }
        let mut byte = da_base64url(link.strip_prefix(PREFISSO).unwrap()).unwrap();
        byte[0] = 2;
        let futuro = format!("{PREFISSO}{}", base64url(&byte));
        assert!(Invito::da_link(&futuro).is_err(), "una versione nuova");
    }

    #[test]
    fn il_gettone_non_finisce_in_un_diario() {
        let scritto = format!("{:?}", invito());
        assert!(!scritto.contains("9, 9"), "{scritto}");
    }

    #[test]
    fn base64url_come_la_rfc() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        for n in 0..40_u8 {
            let byte: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(da_base64url(&base64url(&byte)).unwrap(), byte);
        }
        assert_eq!(da_base64url("A"), None);
        assert_eq!(da_base64url("a=b"), None);
    }

    #[test]
    fn il_qr_e_quadrato_e_ha_i_tre_occhi() {
        let qr = matrice(&invito().link()).unwrap();
        assert_eq!(qr.moduli.len(), qr.lato * qr.lato);
        // L'angolo in alto a sinistra di un QR è sempre scuro: il primo occhio.
        assert!(qr.moduli[0]);
        assert!(qr.moduli[qr.lato - 1], "in alto a destra");
        assert!(qr.moduli[qr.lato * (qr.lato - 1)], "in basso a sinistra");
    }
}
