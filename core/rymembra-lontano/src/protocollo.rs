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
//! Aperto, il JSON ha il tetto che sceglie chi legge, secondo cosa si aspetta
//! e da chi: [`MASSIMA_ASSOCIA`] per chi bussa col QR, [`MASSIMA_DOMANDA`] per
//! le domande, [`MASSIMA_TESTA`] per le risposte.
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

/// Quanto può pesare, aperta, una cornice dell'associazione: un gettone e una
/// [`Presentazione`], qualche centinaio di byte.
///
/// Chi bussa mentre il QR è a schermo non è ancora nessuno, e la sua domanda
/// si legge **prima** di guardare il gettone. Con il tetto di tutte le cornici
/// — 8 MiB aperti — otto kibibyte di deflate diventavano un `Content` di serde
/// da ~200 MiB, dieci volte al minuto (reperto R15-1). Lo stesso tetto vale
/// per la risposta di chi mostra il QR, che si legge prima di conoscerlo.
pub const MASSIMA_ASSOCIA: usize = 16 * 1024;

/// Quanto può pesare, aperta, una [`Domanda`] di un dispositivo associato.
///
/// La più lunga è una ricerca, o un artista o un album della libreria
/// dell'altro: qualche centinaio di byte. Il margine è per i tag lunghi; oltre,
/// chi chiede si sente dire `lontano.protocollo` prima di mandarla, e chi
/// risponde non apre niente.
pub const MASSIMA_DOMANDA: usize = 64 * 1024;

/// Quanto può pesare, aperta, la testa di una risposta: una pagina di
/// cinquecento brani, che compressa sta nel tetto del filo. Solo da un
/// dispositivo associato, e solo a chi gliel'ha chiesta.
pub const MASSIMA_TESTA: usize = MASSIMA_CORNICE * 8;

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
    /// Il catalogo condiviso, in riassunto: la generazione della libreria e,
    /// per ogni secchio di righe, l'impronta di quel che contiene. Risponde
    /// [`Risposta::Dati`]. Dietro [`CAPACITA_CATALOGO`].
    CatalogoRiassunto,
    /// Le righe di un secchio del catalogo, per id: da `da_id` a `a_id`
    /// compresi, al più [`MASSIMA_PAGINA`]. Risponde [`Risposta::Dati`].
    /// Dietro [`CAPACITA_CATALOGO`].
    CatalogoSecchio {
        /// Il primo id del secchio, come l'ha detto il riassunto.
        da_id: i64,
        /// L'ultimo.
        a_id: i64,
    },
    /// Un mi piace o un voto dato qui a un brano dell'altro: lo scrive lui, e
    /// vince l'orologio più recente, per campo. Innocuo se arriva due volte,
    /// come [`Domanda::Ascoltato`]. Dietro [`CAPACITA_CATALOGO`].
    Valuta {
        /// Il brano, con l'id della libreria dell'altro.
        brano: i64,
        /// È fra i preferiti; `None` se chi manda non l'ha cambiato: un voto
        /// dato qui non deve riscrivere un preferito che là è più nuovo.
        liked: Option<bool>,
        /// Le stelle, da 0 a 5; `None` se non le ha cambiate.
        rating: Option<i64>,
        /// Quando si è deciso, ms: l'orologio con cui l'altro sceglie. Per una voce
        /// che porta due decisioni di momenti diversi — un cuore a un istante, le
        /// stelle a uno dopo — è il più recente dei due, e per chi non conosce i
        /// campi qui sotto vale per tutti e due.
        quando_ms: i64,
        /// Quando si è deciso il mi piace, ms, se è un momento diverso da
        /// `quando_ms`. Una build di prima non lo manda, e non lo legge: usa
        /// `quando_ms` (S2-9).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        liked_ms: Option<i64>,
        /// Lo stesso per le stelle.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rating_ms: Option<i64>,
        /// Distingue questo invio dall'altro per lo stesso brano.
        chiave: String,
    },
    /// Il testo della canzone di un brano dell'altro, già composto da lui con
    /// tutta la sua catena (accanto al file, nei tag, in tabella): risponde
    /// [`Risposta::Dati`] se ne ha uno, [`Risposta::Fatto`] se non ha niente.
    /// Dietro [`CAPACITA_TESTI`].
    Testo {
        /// Il brano, con l'id della libreria dell'altro.
        brano: i64,
    },
    /// Una correzione al testo di un brano dell'altro, dato qui: la scrive lui,
    /// con la logica dei suoi comandi (`testo_scarto`, `testo_salva`,
    /// `testo_scegli`, `testo_rifiuta`). Innocua se arriva due volte: dice
    /// come deve restare il testo, e non cosa aggiungere. Dietro
    /// [`CAPACITA_TESTI`], e conta fra le scritture
    /// ([`Domanda::e_una_scrittura`]).
    ///
    /// Porta l'**orologio** di chi l'ha decisa, come [`Domanda::Valuta`]: fra due
    /// correzioni vince la più recente, non l'ultima arrivata — una che aspetta in
    /// posta per giorni non deve cancellare un testo sincronizzato a mano nel
    /// frattempo da chi lo possiede.
    TestoScrivi {
        /// Il brano, con l'id della libreria dell'altro.
        brano: i64,
        /// Cosa è cambiato.
        cosa: ScritturaTesto,
        /// Quando si è deciso, ms, sull'orologio di chi manda.
        ///
        /// **Facoltativo**: una build di prima non lo manda, e vale zero,
        /// «senza orologio» — chi risponde la tratta come decisa **adesso**, che
        /// è com'è sempre stato (vince l'ultima arrivata). Non cambia il numero
        /// del protocollo: chi non lo conosce lo ignora, e chi lo conosce legge
        /// zero dove manca.
        #[serde(default)]
        quando_ms: i64,
    },
    /// Dimentica questo dispositivo.
    Dimenticami,
    /// Una domanda di una versione più nuova.
    #[serde(other)]
    Sconosciuta,
}

/// Quanti byte, al più, ha ogni testo dentro una correzione
/// ([`ScritturaTesto::Mano`], e i due testi di [`ScritturaTesto::Scelta`]).
///
/// È il tetto di chi **scrive** (`MASSIMO_TESTO` in
/// `rymembra_app::testi::condiviso`, che l'applicazione prova uguale a questo) e
/// sta qui perché chi risponde lo guardi **prima** di passare il testo a
/// qualunque altra cosa — il parser compreso. Nei 64 KiB aperti di una
/// [`Domanda`] ci sta con margine anche con tutti gli escape.
pub const MASSIMO_TESTO_SCRITTURA: usize = 48_000;

/// Una correzione al testo di un brano, come arriva a chi lo possiede.
///
/// Una per domanda: chi ha messo in posta più gesti per lo stesso brano li
/// manda uno dopo l'altro, nell'ordine in cui vanno applicati.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "tipo",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScritturaTesto {
    /// Quanto spostare il testo, nel verso dello standard LRC (positivo
    /// anticipa): la correzione di chi ascolta.
    Scarto {
        /// Millisecondi.
        scarto_ms: i32,
    },
    /// Un testo sincronizzato a mano: l'LRC intero, con i tempi delle parole
    /// quando ci sono. Azzera lo scarto, come quando lo si fa dove sta il brano.
    Mano {
        /// L'LRC.
        lrc: String,
    },
    /// Una voce del catalogo dei testi scelta dall'elenco. Il testo viaggia
    /// con la scelta: chi la riceve non deve avere la rete per scriverla.
    Scelta {
        /// L'identificativo della voce nel catalogo.
        id_catalogo: i64,
        /// Il testo senza tempi, se la voce non ha l'LRC.
        piatto: Option<String>,
        /// L'LRC, quando la voce ce l'ha.
        sincronizzato: Option<String>,
        /// La voce dice che il brano non ha parole.
        strumentale: bool,
    },
    /// «Non è questo»: butta il testo che era arrivato dal catalogo.
    Rifiuta,
    /// Una correzione di una versione più nuova.
    #[serde(other)]
    Sconosciuta,
}

impl Domanda {
    /// La capacità che il dispositivo a cui la si fa deve dichiarare, se la
    /// domanda è di quelle nuove. Le domande di prima non ne chiedono: a
    /// ogni versione si continua a fare quelle di oggi.
    #[must_use]
    pub const fn capacita_richiesta(&self) -> Option<&'static str> {
        match self {
            Self::CatalogoRiassunto | Self::CatalogoSecchio { .. } | Self::Valuta { .. } => {
                Some(CAPACITA_CATALOGO)
            }
            Self::Testo { .. } | Self::TestoScrivi { .. } => Some(CAPACITA_TESTI),
            _ => None,
        }
    }

    /// Scrive qualcosa nella libreria di chi risponde (esclusi gli ascolti,
    /// che hanno il loro tetto all'ora): chi risponde ne ammette al più
    /// [`SCRITTURE_AL_MINUTO`] da ogni dispositivo.
    #[must_use]
    pub const fn e_una_scrittura(&self) -> bool {
        matches!(self, Self::Valuta { .. } | Self::TestoScrivi { .. })
    }

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
            Self::TestoScrivi {
                brano,
                cosa,
                quando_ms,
            } => Self::TestoScrivi {
                brano,
                cosa: cosa.nei_tetti(),
                // Un orologio negativo non è un istante: vale «senza orologio».
                quando_ms: quando_ms.max(0),
            },
            altra => altra,
        }
    }
}

impl ScritturaTesto {
    /// La stessa correzione coi tetti di questo protocollo: un testo più lungo di
    /// [`MASSIMO_TESTO_SCRITTURA`] diventa [`ScritturaTesto::Sconosciuta`].
    ///
    /// **Non si taglia.** Un LRC accorciato di colpo è un altro testo, e chi lo
    /// riceve — con un tetto suo uguale a questo — lo troverebbe nei tetti e lo
    /// scriverebbe. Come `Sconosciuta` chi risponde lo rifiuta con
    /// `lontano.protocollo`, che è quel che si dice a una correzione che non si
    /// può applicare, e non lo passa al parser né al database.
    #[must_use]
    pub fn nei_tetti(self) -> Self {
        let sta = |testo: &str| testo.len() <= MASSIMO_TESTO_SCRITTURA;
        match self {
            Self::Mano { lrc } if !sta(&lrc) => Self::Sconosciuta,
            Self::Scelta {
                piatto,
                sincronizzato,
                ..
            } if !piatto.as_deref().is_none_or(sta)
                || !sincronizzato.as_deref().is_none_or(sta) =>
            {
                Self::Sconosciuta
            }
            altra => altra,
        }
    }
}

/// La capacità di servire e usare il catalogo intero (libreria unica): le
/// domande [`Domanda::CatalogoRiassunto`], [`Domanda::CatalogoSecchio`] e
/// [`Domanda::Valuta`]. Un dispositivo che non la dichiara — una build di
/// prima — non le riceve: [`crate::Nodo::chiedi`] le rifiuta con
/// `lontano.protocollo` senza mandarle, e quelle di sempre continuano a
/// funzionare.
pub const CAPACITA_CATALOGO: &str = "catalogo";

/// La capacità di dare e ricevere il testo della canzone di un brano: le
/// domande [`Domanda::Testo`] e [`Domanda::TestoScrivi`].
///
/// È una sigla a sé e non un allargamento di [`CAPACITA_CATALOGO`] per due
/// ragioni. Quella del catalogo c'è già in una build che non conosce i testi,
/// e «sa il catalogo» non dice «sa i testi»: una domanda che l'altro non sa
/// leggere gli arriverebbe come `Sconosciuta`, andrebbe a vuoto sul filo, e una
/// correzione in posta si ripeterebbe per trenta giorni. E ogni famiglia di
/// domande che dichiara la sua capacità si può spegnere da sola, senza
/// toccare le altre.
pub const CAPACITA_TESTI: &str = "testi";

/// Quante capacità, al più, si ricordano di un altro dispositivo.
pub const MASSIME_CAPACITA: usize = 8;

/// Quanto può essere lunga una sigla di capacità.
pub const MASSIMA_CAPACITA: usize = 24;

/// Scritture ([`Domanda::e_una_scrittura`]) servite al minuto per ogni
/// dispositivo: un voto dato a mano ne fa una ogni tanto, e la coda di chi
/// torna dopo ore senza rete se ne sente fermare le ultime e le rimanda al
/// giro dopo.
pub const SCRITTURE_AL_MINUTO: usize = 60;

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
            capacita: sigle_buone(&self.capacita),
            ..self
        }
    }

    /// Questo dispositivo dichiara la capacità `sigla`?
    #[must_use]
    pub fn dichiara(&self, sigla: &str) -> bool {
        self.capacita.iter().any(|c| c == sigla)
    }
}

/// Le sigle di capacità che stanno nei tetti: minuscole, cifre e trattini,
/// brevi, e non più di [`MASSIME_CAPACITA`]. Le altre si scartano e non si
/// tagliano: una sigla tagliata direbbe un'altra cosa.
fn sigle_buone(grezze: &[String]) -> Vec<String> {
    let mut buone: Vec<String> = Vec::new();
    for sigla in grezze {
        let valida = !sigla.is_empty()
            && sigla.len() <= MASSIMA_CAPACITA
            && sigla
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if valida && !buone.contains(sigla) {
            buone.push(sigla.clone());
        }
        if buone.len() >= MASSIME_CAPACITA {
            break;
        }
    }
    buone
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
    /// Cosa sa fare oltre a quel che sapevano tutte le versioni
    /// ([`CAPACITA_CATALOGO`], [`CAPACITA_TESTI`]). Una build di prima non manda il campo: vale
    /// «niente», e a lei le domande nuove non si fanno.
    pub capacita: Vec<String>,
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

/// Una cornice: la lunghezza e il JSON, compresso se è grande. `tetto` è
/// quanto chi la legge accetta aperta ([`valore_entro`]): oltre, non parte.
///
/// # Errori
///
/// `lontano.protocollo` per un valore che non si serializza, che aperto supera
/// `tetto` o che, compresso, supera [`MASSIMA_CORNICE`].
pub(crate) fn cornice(valore: &impl Serialize, tetto: usize) -> Result<Vec<u8>, AppError> {
    let json = serde_json::to_vec(valore).map_err(protocollo)?;
    if json.len() > tetto {
        return Err(protocollo(format!(
            "una cornice da {} byte supera il tetto di chi la legge",
            json.len()
        )));
    }
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

/// Quanto pesa sul filo una risposta di dati — la cornice intera, compressa se
/// è grande — o il perché non ci sta ([`MASSIMA_CORNICE`]). Per chi serve una
/// pagina di righe lunghe e vuole sapere, nelle prove, che parte.
///
/// # Errori
///
/// `lontano.protocollo` se la cornice non si compone o supera i tetti.
pub fn peso_della_risposta(dati: &serde_json::Value) -> Result<usize, AppError> {
    cornice(&Testa::Dati { dati: dati.clone() }, MASSIMA_TESTA).map(|byte| byte.len())
}

/// Quanti byte segue la testa di una cornice, e se sono compressi.
///
/// Il tetto lo decide chi legge, secondo cosa si aspetta: [`MASSIMA_ASSOCIA`],
/// [`MASSIMA_DOMANDA`], [`MASSIMA_TESTA`]. Sul filo non si passa comunque
/// [`MASSIMA_CORNICE`].
///
/// # Errori
///
/// `lontano.protocollo` oltre `tetto` o [`MASSIMA_CORNICE`].
pub(crate) fn lunghezza_entro(testa: [u8; 4], tetto: usize) -> Result<(usize, bool), AppError> {
    let valore = u32::from_be_bytes(testa);
    let quanti = usize::try_from(valore & !COMPRESSA).map_err(protocollo)?;
    if quanti > tetto.min(MASSIMA_CORNICE) {
        return Err(protocollo(format!(
            "una cornice da {quanti} byte supera il tetto"
        )));
    }
    Ok((quanti, valore & COMPRESSA != 0))
}

/// Il valore di un corpo di cornice, che aperto non passa `tetto`.
///
/// Il tetto vale sul JSON **aperto**, compresso o no: serde, per un enum con
/// `tag` come [`Associa`] o [`Domanda`], tiene l'oggetto intero in memoria —
/// campi sconosciuti compresi — prima di scegliere la variante, e ogni byte di
/// JSON gliene costa una ventina (reperto R15-1).
///
/// # Errori
///
/// `lontano.protocollo` per un JSON che non si legge, o che aperto supera
/// `tetto` (una bomba di deflate).
pub(crate) fn valore_entro<T: DeserializeOwned>(
    corpo: &[u8],
    compressa: bool,
    tetto: usize,
) -> Result<T, AppError> {
    if !compressa {
        if corpo.len() > tetto {
            return Err(protocollo("una cornice supera il tetto"));
        }
        return serde_json::from_slice(corpo).map_err(protocollo);
    }
    let mut json = Vec::new();
    let tetto = u64::try_from(tetto).unwrap_or(u64::MAX);
    flate2::read::DeflateDecoder::new(corpo)
        .take(tetto.saturating_add(1))
        .read_to_end(&mut json)
        .map_err(protocollo)?;
    if u64::try_from(json.len()).unwrap_or(u64::MAX) > tetto {
        return Err(protocollo("una cornice decompressa supera il tetto"));
    }
    serde_json::from_slice(&json).map_err(protocollo)
}

/// La testa di una risposta, dal corpo di una cornice: l'unica porta da cui una
/// [`Testa`] entra, e dove le si danno i suoi tetti (reperto S1-2).
///
/// # Errori
///
/// `lontano.protocollo` per un JSON che non si legge, o che aperto supera `tetto`.
pub(crate) fn testa_entro(corpo: &[u8], compressa: bool, tetto: usize) -> Result<Testa, AppError> {
    valore_entro::<Testa>(corpo, compressa, tetto)
}

#[cfg(test)]
mod misure;

#[cfg(test)]
mod prove {
    use super::*;

    fn andata_e_ritorno<T: Serialize + DeserializeOwned>(valore: &T) -> T {
        let byte = cornice(valore, MASSIMA_DOMANDA).unwrap();
        let testa: [u8; 4] = byte[..4].try_into().unwrap();
        let (quanti, compressa) = lunghezza_entro(testa, MASSIMA_DOMANDA).unwrap();
        assert_eq!(quanti, byte.len() - 4);
        valore_entro(&byte[4..], compressa, MASSIMA_DOMANDA).unwrap()
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
                capacita: vec![CAPACITA_CATALOGO.to_owned()],
                ..Presenza::default()
            }),
            Domanda::CatalogoRiassunto,
            Domanda::CatalogoSecchio {
                da_id: 1,
                a_id: 4_120,
            },
            Domanda::Valuta {
                brano: 3,
                liked: Some(true),
                rating: Some(4),
                quando_ms: 1_700_000_000_000,
                liked_ms: None,
                rating_ms: None,
                chiave: "v1".to_owned(),
            },
            Domanda::Valuta {
                brano: 3,
                liked: None,
                rating: Some(0),
                quando_ms: 1_700_000_000_001,
                liked_ms: None,
                rating_ms: None,
                chiave: "v2".to_owned(),
            },
            Domanda::Testo { brano: 3 },
            Domanda::TestoScrivi {
                brano: 3,
                quando_ms: 0,
                cosa: ScritturaTesto::Scarto { scarto_ms: -250 },
            },
            Domanda::TestoScrivi {
                brano: 3,
                quando_ms: 0,
                cosa: ScritturaTesto::Mano {
                    lrc: "[00:01.00]la\n[00:02.50]cura\n".to_owned(),
                },
            },
            Domanda::TestoScrivi {
                brano: 3,
                quando_ms: 0,
                cosa: ScritturaTesto::Scelta {
                    id_catalogo: 4_242,
                    piatto: None,
                    sincronizzato: Some("[00:01.00]la\n".to_owned()),
                    strumentale: false,
                },
            },
            Domanda::TestoScrivi {
                brano: 3,
                quando_ms: 0,
                cosa: ScritturaTesto::Rifiuta,
            },
        ] {
            assert_eq!(andata_e_ritorno(&domanda), domanda);
        }
    }

    #[test]
    fn una_scrittura_di_testo_di_domani_e_sconosciuta_e_non_un_guasto() {
        // Un tipo di correzione che questa versione non conosce non rompe la
        // domanda: diventa `Sconosciuta`, e chi risponde la rifiuta con
        // `lontano.protocollo` senza far cadere la connessione.
        let domani: Domanda = serde_json::from_str(
            r#"{"tipo":"testoScrivi","brano":3,"cosa":{"tipo":"traduzione","lingua":"fr"}}"#,
        )
        .unwrap();
        assert_eq!(
            domani,
            Domanda::TestoScrivi {
                brano: 3,
                quando_ms: 0,
                cosa: ScritturaTesto::Sconosciuta
            }
        );
    }

    #[test]
    fn l_orologio_di_una_correzione_di_testo_e_facoltativo_nei_due_versi() {
        // Una build di prima non lo manda: chi legge trova zero, «senza
        // orologio». È la forma del telefono vero dell'utente (`8b89651`).
        let di_prima: Domanda =
            serde_json::from_str(r#"{"tipo":"testoScrivi","brano":3,"cosa":{"tipo":"rifiuta"}}"#)
                .unwrap();
        assert_eq!(
            di_prima,
            Domanda::TestoScrivi {
                brano: 3,
                cosa: ScritturaTesto::Rifiuta,
                quando_ms: 0,
            }
        );

        // Una build di domani ne manda altri: i campi che non si conoscono si
        // ignorano, e questa non li nega (`deny_unknown_fields` non c'è), quindi
        // lo stesso vale per la build di prima davanti al campo di oggi.
        let con_orologio = Domanda::TestoScrivi {
            brano: 3,
            cosa: ScritturaTesto::Scarto { scarto_ms: 5 },
            quando_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_value(&con_orologio).unwrap();
        assert_eq!(json["quandoMs"], 1_700_000_000_000_i64);
        assert_eq!(andata_e_ritorno(&con_orologio), con_orologio);
        let mut aggiunto = json.clone();
        aggiunto["campoDiDomani"] = serde_json::json!(1);
        assert_eq!(
            serde_json::from_value::<Domanda>(aggiunto).unwrap(),
            con_orologio
        );
        // E tolto il campo, torna la forma di prima.
        let mut senza = json;
        senza.as_object_mut().unwrap().remove("quandoMs");
        assert_eq!(
            serde_json::from_value::<Domanda>(senza).unwrap(),
            Domanda::TestoScrivi {
                brano: 3,
                cosa: ScritturaTesto::Scarto { scarto_ms: 5 },
                quando_ms: 0,
            }
        );
        // Un orologio negativo non è un istante.
        let storto = Domanda::TestoScrivi {
            brano: 3,
            cosa: ScritturaTesto::Rifiuta,
            quando_ms: -5,
        };
        assert!(matches!(
            storto.nei_tetti(),
            Domanda::TestoScrivi { quando_ms: 0, .. }
        ));
    }

    #[test]
    fn le_domande_nuove_dicono_a_quale_capacita_appartengono() {
        // Solo le domande nuove stanno dietro `catalogo`: quelle di prima
        // non chiedono niente a nessuno, e continuano ad avere risposta da
        // ogni versione.
        for nuova in [
            Domanda::CatalogoRiassunto,
            Domanda::CatalogoSecchio { da_id: 1, a_id: 2 },
            Domanda::Valuta {
                brano: 1,
                liked: Some(false),
                rating: None,
                quando_ms: 0,
                liked_ms: None,
                rating_ms: None,
                chiave: String::new(),
            },
        ] {
            assert_eq!(
                nuova.capacita_richiesta(),
                Some(CAPACITA_CATALOGO),
                "{nuova:?}"
            );
        }
        // I testi hanno la loro capacità: un dispositivo che ha il catalogo
        // ma non i testi (una build di mezzo) non li riceve.
        for testo in [
            Domanda::Testo { brano: 1 },
            Domanda::TestoScrivi {
                brano: 1,
                quando_ms: 0,
                cosa: ScritturaTesto::Rifiuta,
            },
        ] {
            assert_eq!(
                testo.capacita_richiesta(),
                Some(CAPACITA_TESTI),
                "{testo:?}"
            );
        }
        for vecchia in [
            Domanda::Presenza,
            Domanda::Album { da: 0, quanti: 1 },
            Domanda::Brani {
                ordine: "titolo".to_owned(),
                da: 0,
                quanti: 1,
            },
            Domanda::Copertina {
                impronta: "ab".to_owned(),
            },
            Domanda::Leggi {
                brano: 1,
                da: 0,
                quanti: 1,
            },
            Domanda::Ascoltato {
                brano: 1,
                quando_ms: 0,
                ascoltato_ms: 0,
                chiave: String::new(),
            },
            Domanda::Dimenticami,
            Domanda::Sconosciuta,
        ] {
            assert_eq!(vecchia.capacita_richiesta(), None, "{vecchia:?}");
        }
    }

    #[test]
    fn la_valutazione_con_un_orologio_per_campo_si_capisce_anche_dalla_build_di_prima() {
        // S2-9. Un cuore a un istante e le stelle a uno dopo viaggiano con due
        // orologi, in campi **facoltativi**: la build di oggi sul telefono vero
        // ignora quel che non conosce (`deny_unknown_fields` non c'è) e usa
        // `quandoMs`; e la build di prima, che non li manda, si legge come prima.
        let con_i_campi = Domanda::Valuta {
            brano: 3,
            liked: Some(true),
            rating: Some(3),
            quando_ms: 200,
            liked_ms: Some(100),
            rating_ms: Some(200),
            chiave: "3-200".to_owned(),
        };
        let json = serde_json::to_value(&con_i_campi).unwrap();
        assert_eq!(json["likedMs"], 100);
        assert_eq!(json["ratingMs"], 200);
        assert_eq!(andata_e_ritorno(&con_i_campi), con_i_campi);
        // Una voce che non ha niente da dire sui campi non li scrive.
        let senza = Domanda::Valuta {
            brano: 3,
            liked: Some(true),
            rating: Some(3),
            quando_ms: 200,
            liked_ms: None,
            rating_ms: None,
            chiave: "3-200".to_owned(),
        };
        let json = serde_json::to_value(&senza).unwrap();
        assert!(json.get("likedMs").is_none() && json.get("ratingMs").is_none());
        // Il messaggio di una build di prima, senza i due campi.
        let di_prima = serde_json::json!({
            "tipo": "valuta", "brano": 3, "liked": true, "rating": 3,
            "quandoMs": 200, "chiave": "3-200"
        });
        assert_eq!(serde_json::from_value::<Domanda>(di_prima).unwrap(), senza);
        // E la build di prima, che ne ignora i campi nuovi: lo stesso messaggio con
        // un campo che non conosce si legge lo stesso.
        let mut con_uno_sconosciuto = serde_json::to_value(&con_i_campi).unwrap();
        con_uno_sconosciuto["campoDiDomani"] = serde_json::json!(1);
        assert_eq!(
            serde_json::from_value::<Domanda>(con_uno_sconosciuto).unwrap(),
            con_i_campi
        );
    }

    #[test]
    fn valuta_e_la_correzione_di_un_testo_sono_le_scritture_verso_il_proprietario() {
        // Gli ascolti hanno il loro tetto (all'ora, e dura quanto la
        // ricezione); `Valuta` e `TestoScrivi` sono le scritture che si
        // limitano per pari.
        assert!(
            Domanda::Valuta {
                brano: 1,
                liked: Some(true),
                rating: Some(5),
                quando_ms: 1,
                liked_ms: None,
                rating_ms: None,
                chiave: "k".to_owned(),
            }
            .e_una_scrittura()
        );
        // E la correzione di un testo, in ogni sua forma.
        for cosa in [
            ScritturaTesto::Scarto { scarto_ms: 100 },
            ScritturaTesto::Mano {
                lrc: "[00:01.00]a".to_owned(),
            },
            ScritturaTesto::Rifiuta,
        ] {
            assert!(
                Domanda::TestoScrivi {
                    brano: 1,
                    cosa,
                    quando_ms: 0,
                }
                .e_una_scrittura()
            );
        }
        // Leggere un testo no: è una lettura, come quelle del catalogo.
        assert!(!Domanda::Testo { brano: 1 }.e_una_scrittura());
        assert!(!Domanda::CatalogoRiassunto.e_una_scrittura());
        assert!(!Domanda::Album { da: 0, quanti: 1 }.e_una_scrittura());
    }

    #[test]
    fn la_presenza_di_una_versione_di_prima_non_dichiara_capacita() {
        // Un telefono o un PC con una build di prima non manda il campo: vale
        // «niente», e a lui le domande nuove non si fanno.
        let vecchia: Presenza =
            serde_json::from_str(r#"{"nome":"PC","piattaforma":"windows","versione":"3.2.1"}"#)
                .unwrap();
        assert!(vecchia.capacita.is_empty());
        assert!(!vecchia.dichiara(CAPACITA_CATALOGO));

        // E quella di adesso la dichiara, nella forma che una build di prima
        // ignora senza fiatare (non ha `deny_unknown_fields`).
        let nuova = Presenza {
            capacita: vec![CAPACITA_CATALOGO.to_owned()],
            ..Presenza::default()
        };
        let json = serde_json::to_string(&nuova).unwrap();
        assert!(json.contains(r#""capacita":["catalogo"]"#), "{json}");
        assert!(nuova.dichiara(CAPACITA_CATALOGO));
        assert_eq!(serde_json::from_str::<Presenza>(&json).unwrap(), nuova);
    }

    #[test]
    fn un_testo_oltre_il_tetto_non_arriva_a_chi_lo_legge() {
        // Il tetto in byte prima di ogni altro: chi risponde non deve nemmeno
        // arrivare a passare a qualcosa un testo che nessuno scrive così lungo.
        //
        // Con `assert!` e non `assert_eq!`: un'asserzione che fallisce stampa le
        // due metà, e queste sono da quarantottomila byte.
        let troppo = "a".repeat(MASSIMO_TESTO_SCRITTURA + 1);
        let giusto = "a".repeat(MASSIMO_TESTO_SCRITTURA);
        let scrittura = |cosa| {
            Domanda::TestoScrivi {
                brano: 3,
                cosa,
                quando_ms: 0,
            }
            .nei_tetti()
        };
        let e_sconosciuta = |domanda: &Domanda| {
            matches!(
                domanda,
                Domanda::TestoScrivi {
                    brano: 3,
                    quando_ms: 0,
                    cosa: ScritturaTesto::Sconosciuta
                }
            )
        };

        assert!(e_sconosciuta(&scrittura(ScritturaTesto::Mano {
            lrc: troppo.clone()
        })));
        // Una scelta con un testo troppo lungo, nell'uno o nell'altro campo.
        for (piatto, sincronizzato) in [
            (Some(troppo.clone()), None),
            (None, Some(troppo.clone())),
            (Some(giusto.clone()), Some(troppo.clone())),
        ] {
            assert!(e_sconosciuta(&scrittura(ScritturaTesto::Scelta {
                id_catalogo: 1,
                piatto,
                sincronizzato,
                strumentale: false,
            })));
        }
        // Quel che sta nel tetto passa com'è.
        let sta = Domanda::TestoScrivi {
            brano: 3,
            quando_ms: 0,
            cosa: ScritturaTesto::Mano { lrc: giusto },
        };
        assert!(sta.clone().nei_tetti() == sta);
        for senza_testo in [
            ScritturaTesto::Scarto { scarto_ms: 5 },
            ScritturaTesto::Rifiuta,
        ] {
            let domanda = Domanda::TestoScrivi {
                brano: 3,
                quando_ms: 0,
                cosa: senza_testo,
            };
            assert_eq!(domanda.clone().nei_tetti(), domanda);
        }
    }

    #[test]
    fn le_capacita_dell_altro_hanno_un_tetto() {
        // Come nome, piattaforma e versione (reperto 02-13): quel che l'altro
        // dice di sé non arriva intero, e una capacità è una sigla.
        let enorme = Presenza {
            capacita: (0..100)
                .map(|n| format!("capacita-{n}-{}", "x".repeat(1000)))
                .chain([
                    "CATALOGO".to_owned(),
                    "cata logo".to_owned(),
                    "catalogo\u{0}".to_owned(),
                    "catalogo".to_owned(),
                ])
                .collect(),
            ..Presenza::default()
        };
        let Domanda::Annuncio(tagliata) = Domanda::Annuncio(enorme).nei_tetti() else {
            panic!("resta un annuncio");
        };
        assert!(
            tagliata.capacita.len() <= MASSIME_CAPACITA,
            "{}",
            tagliata.capacita.len()
        );
        assert!(
            tagliata.capacita.iter().all(|c| c.len() <= MASSIMA_CAPACITA
                && c.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')),
            "{:?}",
            tagliata.capacita
        );
        // La sigla vera, scritta bene, passa: anche dopo cento sconosciute.
        let giusta = Presenza {
            capacita: vec!["x".repeat(3), CAPACITA_CATALOGO.to_owned()],
            ..Presenza::default()
        }
        .nei_tetti();
        assert!(giusta.dichiara(CAPACITA_CATALOGO));
    }

    #[test]
    fn cinquecento_righe_lunghe_stanno_nella_cornice() {
        // Il catalogo di un secchio: `MASSIMA_PAGINA` righe, ognuna coi campi
        // al massimo di quel che chi riceve accetta. Casuali, e quindi poco
        // comprimibili: se qui sta, sta ovunque. (Le righe vere sono
        // `library::condivisa::BranoCatalogo`; qui conta il peso sul filo.)
        let mut seme = 0x9E37_79B9_7F4A_7C15_u64;
        let mut a_caso = |lunghezza: usize| -> String {
            (0..lunghezza)
                .map(|_| {
                    seme ^= seme << 13;
                    seme ^= seme >> 7;
                    seme ^= seme << 17;
                    let cifra = u8::try_from(seme % 36).unwrap();
                    char::from(if cifra < 10 {
                        b'0' + cifra
                    } else {
                        b'a' + cifra - 10
                    })
                })
                .collect()
        };
        let righe: Vec<serde_json::Value> = (0..MASSIMA_PAGINA)
            .map(|n| {
                serde_json::json!({
                    "id": n,
                    "title": a_caso(200), "artist": a_caso(200), "album": a_caso(200),
                    "albumArtist": a_caso(200), "genre": a_caso(64), "codec": a_caso(24),
                    "trackKey": a_caso(300), "contentKey": a_caso(300),
                    "albumKey": a_caso(32), "coverArtHash": a_caso(64),
                    "mbRecordingId": a_caso(36), "mbReleaseGroupId": a_caso(36),
                    "mbReleaseId": a_caso(36), "versione": a_caso(32),
                })
            })
            .collect();
        let testa = Testa::Dati {
            dati: serde_json::json!({ "generazione": 1, "brani": righe }),
        };
        let byte = cornice(&testa, MASSIMA_TESTA).expect("la cornice di un secchio lungo parte");
        assert!(
            byte.len() - 4 <= MASSIMA_CORNICE,
            "{} byte sul filo",
            byte.len()
        );
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
        let byte = cornice(&grande, MASSIMA_TESTA).unwrap();
        let (quanti, compressa) =
            lunghezza_entro(byte[..4].try_into().unwrap(), MASSIMA_TESTA).unwrap();
        assert!(compressa);
        assert!(quanti < 10_000, "compressa: {quanti} byte");
        assert_eq!(
            valore_entro::<Testa>(&byte[4..], true, MASSIMA_TESTA).unwrap(),
            grande
        );

        // Dieci megabyte di zeri compressi: stanno in pochi kilobyte, e
        // aperti supererebbero il tetto.
        let mut bomba = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        bomba.write_all(&vec![b' '; 10 << 20]).unwrap();
        let bomba = bomba.finish().unwrap();
        assert!(valore_entro::<Testa>(&bomba, true, MASSIMA_TESTA).is_err());

        let troppo = (u32::try_from(MASSIMA_CORNICE).unwrap() + 1).to_be_bytes();
        assert!(lunghezza_entro(troppo, MASSIMA_TESTA).is_err());
    }

    #[test]
    fn una_domanda_di_associazione_non_si_apre_oltre_il_suo_tetto() {
        // Reperto R15-1. Chi bussa mentre il QR è a schermo non è ancora
        // nessuno: la sua domanda (un gettone e una Presentazione) sta in
        // qualche centinaio di byte. Otto kibibyte di deflate che si aprono in
        // 8 MiB di `[0,0,…]` diventavano ~200 MiB di `Content` di serde.
        let mut grande = String::from(
            r#"{"tipo":"chiedo","gettone":"00000000000000000000000000000000","chi":{"nome":"x"},"x":["#,
        );
        // Appena sotto gli 8 MiB che si concedevano a ogni cornice aperta.
        grande.push_str(&"0,".repeat((4 << 20) - 200));
        grande.push_str("0]}");
        assert!(grande.len() < 8 << 20);
        let mut compressore =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        compressore.write_all(grande.as_bytes()).unwrap();
        let bomba = compressore.finish().unwrap();
        assert!(bomba.len() < 16 * 1024, "{}", bomba.len());
        assert!(
            valore_entro::<Associa>(&bomba, true, MASSIMA_ASSOCIA).is_err(),
            "compressa"
        );
        // In chiaro, oltre il tetto dell'associazione, la testa basta a dire
        // di no: il corpo non si legge nemmeno.
        let testa = u32::try_from(MASSIMA_ASSOCIA + 1).unwrap().to_be_bytes();
        assert!(lunghezza_entro(testa, MASSIMA_ASSOCIA).is_err());
        // Una domanda vera passa.
        let vera = cornice(
            &Associa::Chiedo {
                gettone: "00".repeat(16),
                chi: Presentazione {
                    nome: "Telefono".into(),
                    ..Presentazione::default()
                },
            },
            MASSIMA_ASSOCIA,
        )
        .unwrap();
        let (quanti, compressa) =
            lunghezza_entro(vera[..4].try_into().unwrap(), MASSIMA_ASSOCIA).unwrap();
        assert_eq!(quanti, vera.len() - 4);
        assert!(valore_entro::<Associa>(&vera[4..], compressa, MASSIMA_ASSOCIA).is_ok());
    }

    #[test]
    fn un_associazione_di_un_tipo_sconosciuto_non_si_apre_oltre_il_tetto() {
        // Reperto R15-1, la seconda forma: con `"tipo":"futuro"` la cornice
        // diventa `Associa::Sconosciuta`, e `associa` esce **prima** di
        // `consuma`. Non brucia il QR e non conta fra i fallimenti: chi bussa
        // può rimandarla dieci volte al minuto finché il QR è a schermo, e
        // ognuna valeva gli stessi ~200 MiB di `Content` della forma `chiedo`.
        let mut grande = String::from(r#"{"tipo":"futuro","x":["#);
        grande.push_str(&"0,".repeat((4 << 20) - 200));
        grande.push_str("0]}");
        assert!(grande.len() < 8 << 20);
        let mut compressore =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        compressore.write_all(grande.as_bytes()).unwrap();
        let bomba = compressore.finish().unwrap();
        assert!(bomba.len() < 16 * 1024, "{}", bomba.len());
        assert!(
            valore_entro::<Associa>(&bomba, true, MASSIMA_ASSOCIA).is_err(),
            "{} byte sul filo, {} aperti: si leggono",
            bomba.len(),
            grande.len()
        );
        // In chiaro, appena oltre il tetto, nemmeno.
        let mut oltre = String::from(r#"{"tipo":"futuro","x":""#);
        oltre.push_str(&"x".repeat(MASSIMA_ASSOCIA));
        oltre.push_str(r#""}"#);
        assert!(valore_entro::<Associa>(oltre.as_bytes(), false, MASSIMA_ASSOCIA).is_err());
        // Una variante di domani, piccola, resta quel che era: sconosciuta,
        // e non un guasto.
        let piccola = br#"{"tipo":"futuro","x":[0,0,0]}"#;
        assert_eq!(
            valore_entro::<Associa>(piccola, false, MASSIMA_ASSOCIA).unwrap(),
            Associa::Sconosciuta
        );
    }

    #[test]
    fn una_domanda_oltre_il_tetto_non_parte_e_non_si_apre() {
        // Chi chiede lo sa prima di mandarla; chi risponde non la apre.
        let lunga = Domanda::Cerca {
            testo: "x".repeat(MASSIMA_DOMANDA),
            da: 0,
            quanti: 1,
        };
        assert!(cornice(&lunga, MASSIMA_DOMANDA).is_err());
        let mandata = cornice(&lunga, MASSIMA_TESTA).unwrap();
        let (_, compressa) =
            lunghezza_entro(mandata[..4].try_into().unwrap(), MASSIMA_DOMANDA).unwrap();
        assert!(valore_entro::<Domanda>(&mandata[4..], compressa, MASSIMA_DOMANDA).is_err());
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
            capacita: Vec::new(),
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
