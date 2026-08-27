//! La copertina, e la catena di ripiego con cui la si trova.
//!
//! # Tre tentativi, in quest'ordine
//!
//! 1. **L'indirizzo diretto della fonte che ha vinto** (iTunes, Deezer). È
//!    legato alla pubblicazione *esatta* che è stata abbinata, quindi è quello
//!    che ha meno modi di sbagliare.
//! 2. **Il Cover Art Archive per gruppo di pubblicazione.** È la scelta che i
//!    collaboratori di MusicBrainz hanno fatto come immagine rappresentativa del
//!    disco, a prescindere da quale ristampa si abbia in mano. Su un disco
//!    ristampato dieci volte è la risposta giusta nove volte su dieci.
//! 3. **Il Cover Art Archive per singola pubblicazione**, provando quelle
//!    candidate una per una. È l'ultima spiaggia, e la più rumorosa: la
//!    copertina di una ristampa giapponese del 2004 è una copertina di quel
//!    disco, ma non quella che l'utente riconosce.
//!
//! L'ordine non è casuale: va dal più specifico al più generico, e ogni gradino
//! costa una richiesta in più. Ci si ferma al primo che restituisce dei byte che
//! **somigliano davvero a un'immagine**.
//!
//! # Il cancello sui byte
//!
//! Ogni immagine passa da [`aether_net::immagine::plausibile`], e non è
//! prudenza generica. Il Cover Art Archive risponde con un reindirizzamento
//! verso un archivio, e un archivio che ha un intoppo risponde `200` con una
//! pagina HTML. Senza il cancello quella pagina finisce nello store delle
//! copertine con la sua impronta — e da lì dentro un file musicale, dove resta
//! per sempre e in griglia si vede come un rettangolo rotto.
//!
//! # Perché una copertina non è mai un errore
//!
//! Tutte le funzioni qui restituiscono `Option` e non `Result`. Un brano senza
//! immagine è un brano; un arricchimento che fallisse perché la copertina non si
//! scarica butterebbe via anche il titolo e l'anno che ha appena trovato, che è
//! il contrario di quel che serve.

use aether_net::http::{Corpo, Metodo, Richiesta};
use aether_net::immagine;
use aether_net::percento;

use crate::Fornitori;

/// Quanto può essere grande un'immagine che si accetta di scaricare.
///
/// Otto megabyte, lo stesso tetto di `tag_scrittura::COPERTINA_MASSIMA` e per la
/// stessa ragione: sopra questa soglia non è una copertina, è un'immagine finita
/// lì per sbaglio.
pub const BYTE_MASSIMI: usize = 8 * 1024 * 1024;

/// Il punto del Cover Art Archive.
const BASE: &str = "https://coverartarchive.org";

/// Il lato che si chiede.
///
/// Cinquecento: lo store ricodifica a 640, quindi chiedere l'originale a
/// millecinquecento pixel vorrebbe dire scaricare un megabyte per buttarne i
/// due terzi. Fra i tagli disponibili (250, 500, 1200) è quello che si avvicina
/// di più senza sprecare.
const LATO: &str = "front-500";

/// Da dove viene una copertina.
///
/// Finisce in `cover_art.source`, dove `aether_domain::album::pick_album_cover`
/// la usa per decidere se una copertina trovata online possa sostituirne una
/// presa dai tag del file. Non può, ed è voluto: chi ha incorporato un'immagine
/// nel proprio file ha già espresso una preferenza.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenienza {
    /// Dall'indirizzo diretto della fonte che ha vinto l'abbinamento.
    Fornitore,
    /// Dal Cover Art Archive.
    CoverArtArchive,
}

impl Provenienza {
    /// Il nome che finisce nel database.
    ///
    /// Sono le stesse due stringhe che `album::cover_rank` conosce: cambiarle
    /// qui senza cambiarle là farebbe cadere il rango a zero, e una copertina di
    /// provenienza ignota perde contro qualunque altra.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fornitore => "provider",
            Self::CoverArtArchive => "caa",
        }
    }
}

/// Una copertina scaricata e riconosciuta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copertina {
    /// I byte, così come sono arrivati.
    pub byte: Vec<u8>,
    /// Da dove viene.
    pub provenienza: Provenienza,
    /// Il tipo riconosciuto dai byte.
    pub tipo: &'static str,
}

/// Da dove si può provare a prendere la copertina di un brano.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sorgenti<'a> {
    /// L'indirizzo diretto della fonte che ha vinto, se ne ha dato uno.
    pub url_diretto: Option<&'a str>,
    /// Il gruppo di pubblicazione MusicBrainz.
    pub mb_release_group_id: Option<&'a str>,
    /// Le pubblicazioni candidate, in ordine di preferenza.
    pub mb_release_ids: &'a [String],
}

/// Scarica un'immagine, se è un'immagine.
///
/// `None` per qualunque intoppo — indirizzo non cifrato, servizio irraggiungibile,
/// risposta non riuscita, byte troppi o troppo pochi, formato non riconosciuto.
/// Non c'è nessun caso in cui valga la pena distinguerli: chi chiama passa
/// comunque al gradino dopo.
fn scarica(fornitori: &Fornitori, url: &str) -> Option<Copertina> {
    // Solo `https`. L'indirizzo arriva da una risposta di rete, non dall'utente,
    // ma è pur sempre una stringa presa dalla rete che finisce in una richiesta:
    // `file:` o `http:` non hanno motivo di essere seguiti. Lo stesso cancello,
    // con la stessa ragione, di `aether_catalogo`.
    if !url.starts_with("https://") {
        return None;
    }
    if fornitori.copertine.aperta() {
        return None;
    }
    fornitori.copertine.attendi();

    let risposta = match fornitori.rete.esegui(Richiesta {
        metodo: Metodo::Get,
        url,
        intestazioni: &[("Accept", "image/*")],
        corpo: Corpo::Niente,
    }) {
        Ok(risposta) => risposta,
        Err(_) => {
            fornitori.copertine.guasto();
            return None;
        }
    };

    // Un `404` è la risposta normale a «questa pubblicazione non ha
    // un'immagine», e va contata come una riuscita del servizio: contarla come
    // guasto aprirebbe l'interruttore su una libreria fatta di dischi oscuri,
    // cioè proprio su quella che ne ha più bisogno.
    if risposta.stato == 404 {
        fornitori.copertine.riuscita();
        return None;
    }
    if !risposta.e_andata() {
        if fornitori.rete.stato_a_errore(&risposta, url).is_retryable() {
            fornitori.copertine.guasto();
        } else {
            fornitori.copertine.riuscita();
        }
        return None;
    }
    fornitori.copertine.riuscita();

    if !immagine::plausibile(&risposta.corpo, BYTE_MASSIMI) {
        return None;
    }
    let tipo = immagine::tipo(&risposta.corpo)?;
    Some(Copertina {
        byte: risposta.corpo,
        provenienza: Provenienza::CoverArtArchive,
        tipo,
    })
}

/// La copertina scelta per un gruppo di pubblicazione, se ne ha una.
#[must_use]
pub fn per_gruppo(fornitori: &Fornitori, id: &str) -> Option<Copertina> {
    scarica(
        fornitori,
        &format!("{BASE}/release-group/{}/{LATO}", percento(id)),
    )
}

/// La copertina di una singola pubblicazione, se ne ha una.
#[must_use]
pub fn per_pubblicazione(fornitori: &Fornitori, id: &str) -> Option<Copertina> {
    scarica(
        fornitori,
        &format!("{BASE}/release/{}/{LATO}", percento(id)),
    )
}

/// Percorre la catena di ripiego e restituisce la prima immagine valida.
///
/// Quante pubblicazioni si provano al massimo, nell'ultimo gradino: oltre la
/// terza si stanno chiedendo ristampe che nessuno riconoscerebbe, pagando una
/// richiesta ciascuna.
const PUBBLICAZIONI_MASSIME: usize = 3;

/// La copertina, dal gradino più specifico al più generico.
#[must_use]
pub fn risolvi(fornitori: &Fornitori, sorgenti: &Sorgenti<'_>) -> Option<Copertina> {
    if let Some(url) = sorgenti.url_diretto
        && let Some(copertina) = scarica(fornitori, url)
    {
        return Some(Copertina {
            provenienza: Provenienza::Fornitore,
            ..copertina
        });
    }
    if let Some(id) = sorgenti.mb_release_group_id
        && let Some(copertina) = per_gruppo(fornitori, id)
    {
        return Some(copertina);
    }
    for id in sorgenti.mb_release_ids.iter().take(PUBBLICAZIONI_MASSIME) {
        if let Some(copertina) = per_pubblicazione(fornitori, id) {
            return Some(copertina);
        }
    }
    None
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::deposito::Senza;

    #[test]
    fn i_nomi_della_provenienza_sono_quelli_che_il_dominio_conosce() {
        // `album::cover_rank` riconosce esattamente queste due stringhe.
        // Cambiarle qui senza cambiarle là farebbe cadere il rango a zero, e una
        // copertina di provenienza ignota perde contro qualunque altra.
        assert_eq!(Provenienza::Fornitore.as_str(), "provider");
        assert_eq!(Provenienza::CoverArtArchive.as_str(), "caa");
    }

    #[test]
    fn un_indirizzo_non_cifrato_non_si_segue() {
        // Non tocca la rete: i due indirizzi sono respinti prima della
        // richiesta, quindi la prova non dipende dall'essere collegati.
        let fornitori = Fornitori::nuovo(Box::new(Senza));
        assert!(scarica(&fornitori, "http://esempio.invalido/a.jpg").is_none());
        assert!(scarica(&fornitori, "file:///C:/segreto.jpg").is_none());
    }

    #[test]
    fn senza_nessuna_sorgente_non_si_fa_nessuna_richiesta() {
        let fornitori = Fornitori::nuovo(Box::new(Senza));
        assert!(risolvi(&fornitori, &Sorgenti::default()).is_none());
    }

    #[test]
    fn con_l_interruttore_aperto_non_si_scarica_niente() {
        let fornitori = Fornitori::nuovo(Box::new(Senza));
        for _ in 0..crate::cadenza::GUASTI_PER_APRIRE {
            fornitori.copertine.guasto();
        }
        assert!(fornitori.copertine.aperta());
        assert!(scarica(&fornitori, "https://esempio.invalido/a.jpg").is_none());
    }
}
