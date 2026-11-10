//! Cosa si dicono due dispositivi collegati.
//!
//! # Tre protocolli, tre porte
//!
//! QUIC sceglie il protocollo con l'ALPN, già nella stretta di mano:
//!
//! - [`ALPN_ASSOCIA`]: l'associazione col QR. Aperto a chiunque, ma solo
//!   mentre un QR è mostrato, e con un tetto di tentativi;
//! - [`ALPN_LONTANO`]: la libreria dell'altro. Solo per le chiavi associate;
//! - [`ALPN_INSIEME`]: «Ascolta insieme» via internet, col suo canale Noise
//!   sopra. Solo per le chiavi associate.
//!
//! # Una domanda per flusso
//!
//! Ogni domanda apre un flusso bidirezionale di QUIC, che costa quasi niente:
//! chi chiede scrive una [`Domanda`] e chiude il suo lato, chi risponde scrive
//! una testa e, per una lettura, i byte grezzi che la seguono. Una domanda
//! lenta (un pezzo da due megabyte) non ferma le altre.
//!
//! # La cornice
//!
//! Quattro byte di lunghezza (big endian) e un JSON. Il bit più alto della
//! lunghezza dice che il JSON è compresso (deflate grezzo, sopra
//! [`SOGLIA_COMPRESSIONE`]); il resto non supera mai [`MASSIMA_CORNICE`].
//!
//! # In avanti
//!
//! Una domanda o una risposta di una versione più nuova, che questa non
//! conosce, diventa `Sconosciuta` (`serde(other)`), e a una domanda
//! sconosciuta si risponde col rifiuto `lontano.protocollo`: la connessione
//! resta in piedi per tutto il resto.

use std::io::{Read as _, Write as _};

use rymembra_domain::errors::AppError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::errori::protocollo;

/// L'associazione col QR.
pub const ALPN_ASSOCIA: &[u8] = b"rymembra/associa/1";

/// La libreria dell'altro dispositivo.
pub const ALPN_LONTANO: &[u8] = b"rymembra/lontano/1";

/// «Ascolta insieme» via internet.
pub const ALPN_INSIEME: &[u8] = b"rymembra/insieme/1";

/// Quanto può pesare il JSON di una cornice: una pagina di cinquecento brani
/// sta in qualche centinaio di kilobyte.
pub const MASSIMA_CORNICE: usize = 1 << 20;

/// Oltre questo il JSON si comprime.
pub const SOGLIA_COMPRESSIONE: usize = 8 * 1024;

/// Il bit della lunghezza che dice «compresso».
const COMPRESSA: u32 = 1 << 31;

/// Il pezzo più grande che si chiede in una lettura.
pub const MASSIMO_PEZZO: u64 = 2 * 1024 * 1024;

/// Quante voci per pagina, al più.
pub const MASSIMA_PAGINA: u32 = 500;

/// Una domanda a un dispositivo associato.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "tipo",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Domanda {
    /// Chi sei, e cosa fai: risponde [`Risposta::Presenza`].
    Presenza,
    /// Ecco chi sono, e cosa faccio: la presenza spinta da chi è cambiato,
    /// senza aspettare che l'altro chieda. Risponde [`Risposta::Fatto`].
    Annuncio(Presenza),
    /// Gli album, a pagine.
    Album {
        /// Da dove.
        da: u32,
        /// Quanti, al più [`MASSIMA_PAGINA`].
        quanti: u32,
    },
    /// Gli album di un artista.
    AlbumArtista {
        /// L'artista, com'è scritto.
        artista: String,
    },
    /// Gli artisti, a pagine.
    Artisti {
        /// Da dove.
        da: u32,
        /// Quanti.
        quanti: u32,
    },
    /// I brani, a pagine e in un ordine.
    Brani {
        /// L'ordine, come lo intende la libreria (`titolo`, `artista`…).
        ordine: String,
        /// Da dove.
        da: u32,
        /// Quanti.
        quanti: u32,
    },
    /// I brani di un album.
    BraniAlbum {
        /// L'album: la sua chiave nella libreria dell'altro.
        album: String,
    },
    /// La ricerca.
    Cerca {
        /// Il testo.
        testo: String,
        /// Da dove.
        da: u32,
        /// Quanti.
        quanti: u32,
    },
    /// Quanti risultati ha una ricerca.
    CercaConteggio {
        /// Il testo.
        testo: String,
    },
    /// Una copertina, dalla sua impronta.
    Copertina {
        /// L'impronta, come la dà la libreria.
        impronta: String,
    },
    /// Un pezzo di un brano: risponde [`Risposta::Byte`].
    Leggi {
        /// Il brano.
        brano: i64,
        /// Da dove.
        da: u64,
        /// Quanti byte, al più [`MASSIMO_PEZZO`].
        quanti: u64,
    },
    /// Un ascolto fatto qui di un brano dell'altro: lo conta lui.
    Ascoltato {
        /// Il brano.
        brano: i64,
        /// Quando è cominciato.
        quando_ms: i64,
        /// Quanto si è ascoltato.
        ascoltato_ms: i64,
        /// Rende innocuo un invio ripetuto.
        chiave: String,
    },
    /// Dimentica questo dispositivo.
    Dimenticami,
    /// Una domanda di una versione più nuova.
    #[serde(other)]
    Sconosciuta,
}

impl Domanda {
    /// La stessa domanda coi tetti di questo protocollo: pagine e pezzi non
    /// più grandi di quanto si serve.
    #[must_use]
    pub fn nei_tetti(self) -> Self {
        let pagina = |quanti: u32| quanti.clamp(1, MASSIMA_PAGINA);
        match self {
            Self::Album { da, quanti } => Self::Album {
                da,
                quanti: pagina(quanti),
            },
            Self::Artisti { da, quanti } => Self::Artisti {
                da,
                quanti: pagina(quanti),
            },
            Self::Brani { ordine, da, quanti } => Self::Brani {
                ordine,
                da,
                quanti: pagina(quanti),
            },
            Self::Cerca { testo, da, quanti } => Self::Cerca {
                testo,
                da,
                quanti: pagina(quanti),
            },
            Self::Leggi { brano, da, quanti } => Self::Leggi {
                brano,
                da,
                quanti: quanti.min(MASSIMO_PEZZO),
            },
            Self::Annuncio(presenza) => Self::Annuncio(presenza.nei_tetti()),
            altra => altra,
        }
    }
}

/// Quanti caratteri, al più, ha ogni testo di una [`Presenza`] che arriva da
/// un altro dispositivo: quanti ne ha il nome che l'associazione scrive.
pub const MASSIMO_TESTO_PRESENZA: usize = 64;

impl Presenza {
    /// La stessa presenza coi tetti di questo protocollo: ogni testo come
    /// testo puro — senza controlli né comandi di direzione, gli spazi
    /// ridotti a uno — entro [`MASSIMO_TESTO_PRESENZA`] caratteri. Quel che l'altro dice di sé va nel pannello e in ogni
    /// `lontano:stato` della finestra: senza tetto, una versione da qualche
    /// megabyte ci arrivava intera (reperto 02-13). Il nodo la applica a ogni
    /// presenza che riceve, annunciata o risposta.
    #[must_use]
    pub fn nei_tetti(self) -> Self {
        Self {
            nome: testo_puro(&self.nome),
            piattaforma: testo_puro(&self.piattaforma),
            versione: testo_puro(&self.versione),
            ..self
        }
    }
}

/// Un testo di un altro dispositivo come testo puro, con le regole dei nomi
/// che arrivano da fuori (`rymembra_app::insieme::ripulisci_nome`, che questo
/// crate non vede): i caratteri di controllo e gli spazi di ogni sorta
/// diventano uno spazio solo, e mai in testa né in coda; spariscono gli spazi
/// di larghezza zero (U+200B, U+FEFF) e i comandi di direzione (U+202A–202E,
/// U+2066–2069), che farebbero leggere a schermo un testo diverso. Il taglio è
/// a [`MASSIMO_TESTO_PRESENZA`] caratteri, prima dello spazio che non ci
/// starebbe.
fn testo_puro(grezzo: &str) -> String {
    let mut pulito = String::new();
    let mut quanti = 0_usize;
    let mut spazio_in_sospeso = false;
    for carattere in grezzo.chars() {
        if carattere.is_control() || carattere.is_whitespace() {
            spazio_in_sospeso = !pulito.is_empty();
            continue;
        }
        if matches!(
            carattere,
            '\u{200B}' | '\u{FEFF}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        ) {
            continue;
        }
        let servono = if spazio_in_sospeso { 2 } else { 1 };
        if quanti.saturating_add(servono) > MASSIMO_TESTO_PRESENZA {
            break;
        }
        if spazio_in_sospeso {
            pulito.push(' ');
            spazio_in_sospeso = false;
        }
        pulito.push(carattere);
        quanti = quanti.saturating_add(servono);
    }
    pulito
}

/// Chi è un dispositivo e cosa sta facendo: la prima cosa che si chiede.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Presenza {
    /// Il nome che mostra («PC di Mario», «Nothing Phone 2a»).
    pub nome: String,
    /// `windows`, `android`.
    pub piattaforma: String,
    /// La versione di Rymembra.
    pub versione: String,
    /// «Ascolta insieme» è acceso.
    pub insieme_acceso: bool,
    /// E sta ospitando una sessione.
    pub insieme_ospita: bool,
}

/// La risposta a una domanda.
#[derive(Debug, Clone, PartialEq)]
pub enum Risposta {
    /// A [`Domanda::Presenza`].
    Presenza(Presenza),
    /// Dei dati della libreria, nella forma che l'app sceglie.
    Dati(serde_json::Value),
    /// Dei byte: un pezzo di un brano, una copertina.
    Byte {
        /// I byte.
        byte: Vec<u8>,
        /// Quanto è lungo il file intero.
        totale: u64,
        /// Cambia se il file cambia (dimensione e data): chi legge un brano a
        /// pezzi se ne accorge fra un pezzo e l'altro.
        versione: String,
    },
    /// Fatto, e non c'è altro da dire.
    Fatto,
}

/// La testa della risposta sul filo. [`Risposta::Byte`] porta solo la
/// lunghezza: i byte seguono grezzi, senza JSON di mezzo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "tipo",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Testa {
    Presenza(Presenza),
    Dati {
        dati: serde_json::Value,
    },
    Byte {
        lunghezza: u64,
        totale: u64,
        versione: String,
    },
    Fatto,
    /// Un rifiuto: solo il codice (vedi `errori`).
    Rifiuto {
        codice: String,
    },
    #[serde(other)]
    Sconosciuta,
}

/// Come si presenta chi vuole collegarsi, e chi lo accoglie.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Presentazione {
    /// Il nome da mostrare.
    pub nome: String,
    /// `windows`, `android`.
    pub piattaforma: String,
    /// L'identificativo del dispositivo nella sincronia, per unire le voci
    /// della pagina «Dispositivi».
    pub dispositivo: String,
    /// La chiave pubblica di «Ascolta insieme» (X25519, esadecimale): chi la
    /// riceve si fida di lei per le sessioni, senza il codice a quattro cifre.
    pub chiave_insieme: Option<String>,
}

/// I messaggi dell'associazione.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "tipo",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Associa {
    /// Chi ha letto il QR: il gettone (esadecimale) e chi è.
    Chiedo { gettone: String, chi: Presentazione },
    /// Chi mostrava il QR ha detto di sì: ecco chi è lui.
    Accolto { chi: Presentazione },
    /// No, e perché: solo il codice.
    Rifiutato { codice: String },
    #[serde(other)]
    Sconosciuta,
}

/// Una cornice: la lunghezza e il JSON, compresso se è grande.
///
/// # Errori
///
/// `lontano.protocollo` per un valore che non si serializza o che, compresso,
/// supera [`MASSIMA_CORNICE`].
pub(crate) fn cornice(valore: &impl Serialize) -> Result<Vec<u8>, AppError> {
    let json = serde_json::to_vec(valore).map_err(protocollo)?;
    let (corpo, compressa) = if json.len() > SOGLIA_COMPRESSIONE {
        let mut compressore =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        compressore.write_all(&json).map_err(protocollo)?;
        (compressore.finish().map_err(protocollo)?, true)
    } else {
        (json, false)
    };
    if corpo.len() > MASSIMA_CORNICE {
        return Err(protocollo(format!(
            "una cornice da {} byte supera il tetto",
            corpo.len()
        )));
    }
    let mut lunghezza = u32::try_from(corpo.len()).map_err(protocollo)?;
    if compressa {
        lunghezza |= COMPRESSA;
    }
    let mut tutto = Vec::with_capacity(corpo.len() + 4);
    tutto.extend_from_slice(&lunghezza.to_be_bytes());
    tutto.extend_from_slice(&corpo);
    Ok(tutto)
}

/// Quanti byte segue la testa di una cornice, e se sono compressi.
///
/// # Errori
///
/// `lontano.protocollo` oltre [`MASSIMA_CORNICE`].
pub(crate) fn lunghezza(testa: [u8; 4]) -> Result<(usize, bool), AppError> {
    let valore = u32::from_be_bytes(testa);
    let quanti = usize::try_from(valore & !COMPRESSA).map_err(protocollo)?;
    if quanti > MASSIMA_CORNICE {
        return Err(protocollo(format!(
            "una cornice da {quanti} byte supera il tetto"
        )));
    }
    Ok((quanti, valore & COMPRESSA != 0))
}

/// Il valore di un corpo di cornice.
///
/// # Errori
///
/// `lontano.protocollo` per un JSON che non si legge, o che decompresso
/// supera il tetto (una bomba di deflate).
pub(crate) fn valore<T: DeserializeOwned>(corpo: &[u8], compressa: bool) -> Result<T, AppError> {
    if !compressa {
        return serde_json::from_slice(corpo).map_err(protocollo);
    }
    let mut json = Vec::new();
    let tetto = u64::try_from(MASSIMA_CORNICE * 8).unwrap_or(u64::MAX);
    flate2::read::DeflateDecoder::new(corpo)
        .take(tetto + 1)
        .read_to_end(&mut json)
        .map_err(protocollo)?;
    if u64::try_from(json.len()).unwrap_or(u64::MAX) > tetto {
        return Err(protocollo("una cornice decompressa supera il tetto"));
    }
    serde_json::from_slice(&json).map_err(protocollo)
}

#[cfg(test)]
mod prove {
    use super::*;

    fn andata_e_ritorno<T: Serialize + DeserializeOwned>(valore: &T) -> T {
        let byte = cornice(valore).unwrap();
        let testa: [u8; 4] = byte[..4].try_into().unwrap();
        let (quanti, compressa) = lunghezza(testa).unwrap();
        assert_eq!(quanti, byte.len() - 4);
        self::valore(&byte[4..], compressa).unwrap()
    }

    #[test]
    fn ogni_domanda_torna_com_era() {
        for domanda in [
            Domanda::Presenza,
            Domanda::Album { da: 0, quanti: 50 },
            Domanda::AlbumArtista {
                artista: "Battiato".to_owned(),
            },
            Domanda::Brani {
                ordine: "titolo".to_owned(),
                da: 100,
                quanti: 500,
            },
            Domanda::BraniAlbum {
                album: "queen|a night at the opera".to_owned(),
            },
            Domanda::Cerca {
                testo: "la cura".to_owned(),
                da: 0,
                quanti: 20,
            },
            Domanda::Leggi {
                brano: 3,
                da: 1 << 20,
                quanti: 256 * 1024,
            },
            Domanda::Ascoltato {
                brano: 3,
                quando_ms: 1_700_000_000_000,
                ascoltato_ms: 180_000,
                chiave: "a1".to_owned(),
            },
            Domanda::Dimenticami,
            Domanda::Annuncio(Presenza {
                nome: "PC di Mario".to_owned(),
                insieme_acceso: true,
                ..Presenza::default()
            }),
        ] {
            assert_eq!(andata_e_ritorno(&domanda), domanda);
        }
    }

    #[test]
    fn una_domanda_di_domani_e_sconosciuta_e_non_un_guasto() {
        let domani: Domanda = serde_json::from_str(r#"{"tipo":"playlist","quale":3}"#).unwrap();
        assert_eq!(domani, Domanda::Sconosciuta);
        let testa: Testa = serde_json::from_str(r#"{"tipo":"video"}"#).unwrap();
        assert_eq!(testa, Testa::Sconosciuta);
        // E i campi in più di una presenza nuova si ignorano.
        let presenza: Presenza = serde_json::from_str(r#"{"nome":"PC","colore":"blu"}"#).unwrap();
        assert_eq!(presenza.nome, "PC");
    }

    #[test]
    fn una_cornice_grande_si_comprime_e_una_bomba_no() {
        let grande = Testa::Dati {
            dati: serde_json::json!({ "brani": vec!["la stessa riga ripetuta"; 5000] }),
        };
        let byte = cornice(&grande).unwrap();
        let (quanti, compressa) = lunghezza(byte[..4].try_into().unwrap()).unwrap();
        assert!(compressa);
        assert!(quanti < 10_000, "compressa: {quanti} byte");
        assert_eq!(valore::<Testa>(&byte[4..], true).unwrap(), grande);

        // Dieci megabyte di zeri compressi: stanno in pochi kilobyte, e
        // aperti supererebbero il tetto.
        let mut bomba = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        bomba.write_all(&vec![b' '; 10 << 20]).unwrap();
        let bomba = bomba.finish().unwrap();
        assert!(valore::<Testa>(&bomba, true).is_err());

        let troppo = (u32::try_from(MASSIMA_CORNICE).unwrap() + 1).to_be_bytes();
        assert!(lunghezza(troppo).is_err());
    }

    #[test]
    fn la_presenza_dell_altro_ha_un_tetto() {
        // Reperto 02-13: un dispositivo collegato annunciava una versione (o
        // un nome, una piattaforma) da qualche megabyte, e finiva nel pannello
        // e in ogni `lontano:stato` mandato alla finestra.
        let enorme = Presenza {
            nome: format!(" PC\u{202E} di\tcasa {}", "x".repeat(10_000)),
            piattaforma: "windows\u{0}".repeat(1_000),
            versione: format!("3.2.0\n{}", "9".repeat(1 << 20)),
            insieme_acceso: true,
            insieme_ospita: true,
        };
        let Domanda::Annuncio(tagliata) = Domanda::Annuncio(enorme).nei_tetti() else {
            panic!("resta un annuncio");
        };
        for testo in [&tagliata.nome, &tagliata.piattaforma, &tagliata.versione] {
            assert!(
                testo.chars().count() <= MASSIMO_TESTO_PRESENZA,
                "{} caratteri",
                testo.len()
            );
            assert!(!testo.chars().any(char::is_control), "{testo:?}");
            assert!(!testo.contains('\u{202E}'), "{testo:?}");
        }
        assert!(
            tagliata.nome.starts_with("PC di casa x"),
            "{}",
            tagliata.nome
        );
        assert!(tagliata.versione.starts_with("3.2.0 9"));
        assert!(tagliata.insieme_acceso && tagliata.insieme_ospita);
        // Una presenza normale resta com'è.
        let normale = Presenza {
            nome: "PC di Mario".to_owned(),
            piattaforma: "windows".to_owned(),
            versione: "3.2.0".to_owned(),
            ..Presenza::default()
        };
        assert_eq!(
            Domanda::Annuncio(normale.clone()).nei_tetti(),
            Domanda::Annuncio(normale)
        );
    }

    #[test]
    fn i_tetti_delle_domande() {
        assert_eq!(
            Domanda::Album {
                da: 0,
                quanti: 10_000
            }
            .nei_tetti(),
            Domanda::Album {
                da: 0,
                quanti: MASSIMA_PAGINA
            }
        );
        assert_eq!(
            Domanda::Leggi {
                brano: 1,
                da: 0,
                quanti: u64::MAX
            }
            .nei_tetti(),
            Domanda::Leggi {
                brano: 1,
                da: 0,
                quanti: MASSIMO_PEZZO
            }
        );
    }
}
